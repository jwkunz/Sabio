use std::{
    env,
    io::{Cursor, Read},
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use axum::{
    extract::{DefaultBodyLimit, Multipart, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures_util::{Stream, StreamExt};
use quick_xml::{events::Event as XmlEvent, Reader};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{net::TcpListener, time::sleep};
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::{error, info, warn};
use uuid::Uuid;
use zip::ZipArchive;

mod agent;

const APP_URL: &str = "http://127.0.0.1:3000";
const LARGE_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) client: Client,
    pub(crate) ollama_base_url: String,
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
    detail: String,
}

#[derive(Debug)]
struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        let message = message.into();
        Self { status, message }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        error!(status = %self.status, error = %self.message, "[sabio] request failed");
        (
            self.status,
            Json(ApiError {
                error: self.message.clone(),
                detail: self.message,
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
    size: Option<u64>,
    #[serde(rename = "modified_at")]
    modified_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct ModelsResponse {
    models: Vec<ModelOption>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelOption {
    name: String,
    size: Option<u64>,
    modified_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct UploadResponse {
    files: Vec<UploadedFile>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadedFile {
    id: Uuid,
    name: String,
    #[serde(rename = "type")]
    mime_type: String,
    size: u64,
    uploaded_at: i64,
    raw_text: String,
    warning: String,
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    model: String,
    prompt: String,
    #[allow(dead_code)]
    request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateChunk {
    response: Option<String>,
    done: Option<bool>,
    error: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sabio_server=info,tower_http=warn".into()),
        )
        .init();

    let ollama_base_url =
        env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;

    ensure_ollama_running(&client, &ollama_base_url).await;

    let state = AppState {
        client,
        ollama_base_url,
    };

    let app = build_router(state);
    let address: SocketAddr = "127.0.0.1:3000".parse()?;
    let listener = TcpListener::bind(address).await?;

    info!("Sabio listening on {APP_URL}");
    open_browser(APP_URL);

    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router(state: AppState) -> Router {
    let dist_dir = frontend_dist_dir();
    let static_service = ServeDir::new(&dist_dir)
        .append_index_html_on_directories(true)
        .fallback(ServeDir::new(&dist_dir).append_index_html_on_directories(true));

    Router::new()
        .route("/api/health", get(health))
        .route("/api/models", get(models))
        .route("/api/upload", post(upload))
        .route("/api/chat", post(chat))
        .nest("/api/agent", agent::router())
        .nest_service("/", static_service)
        .layer(DefaultBodyLimit::max(128 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

async fn models(State(state): State<AppState>) -> Result<Json<ModelsResponse>, AppError> {
    let response = state
        .client
        .get(format!("{}/api/tags", state.ollama_base_url))
        .send()
        .await
        .map_err(|error| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                format!("Ollama is unavailable: {error}"),
            )
        })?;

    if !response.status().is_success() {
        return Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            "Ollama is unavailable.",
        ));
    }

    let payload = response
        .json::<OllamaTagsResponse>()
        .await
        .map_err(|error| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                format!("Unable to parse Ollama model list: {error}"),
            )
        })?;

    Ok(Json(ModelsResponse {
        models: payload
            .models
            .into_iter()
            .map(|model| ModelOption {
                name: model.name,
                size: model.size,
                modified_at: model.modified_at,
            })
            .collect(),
    }))
}

async fn upload(mut multipart: Multipart) -> Result<Json<UploadResponse>, AppError> {
    let mut files = Vec::new();

    while let Some(field) = multipart.next_field().await.map_err(|error| {
        AppError::new(
            StatusCode::BAD_REQUEST,
            format!("Unable to read uploaded file: {error}"),
        )
    })? {
        let file_name = field
            .file_name()
            .map(str::to_owned)
            .unwrap_or_else(|| "uploaded-file".to_string());
        let mime_type = field
            .content_type()
            .map(str::to_owned)
            .unwrap_or_else(|| "text/plain".to_string());
        let bytes = field.bytes().await.map_err(|error| {
            AppError::new(
                StatusCode::BAD_REQUEST,
                format!("Unable to read uploaded file bytes: {error}"),
            )
        })?;
        let size = bytes.len() as u64;
        let raw_text = extract_raw_text(&file_name, &bytes)?;

        if raw_text.trim().is_empty() {
            return Err(AppError::new(
                StatusCode::BAD_REQUEST,
                format!("Parsed file is empty: {file_name}"),
            ));
        }

        files.push(UploadedFile {
            id: Uuid::new_v4(),
            name: file_name,
            mime_type,
            size,
            uploaded_at: chrono::Utc::now().timestamp_millis(),
            raw_text,
            warning: if size > LARGE_FILE_BYTES {
                "Large file: prompt size may increase.".to_string()
            } else {
                String::new()
            },
        });
    }

    Ok(Json(UploadResponse { files }))
}

async fn chat(
    State(state): State<AppState>,
    Json(request): Json<ChatRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, AppError> {
    if request.model.trim().is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "A model must be selected.",
        ));
    }

    if request.prompt.trim().is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "Prompt cannot be empty.",
        ));
    }

    let response = state
        .client
        .post(format!("{}/api/generate", state.ollama_base_url))
        .json(&json!({
            "model": request.model,
            "prompt": request.prompt,
            "stream": true
        }))
        .send()
        .await
        .map_err(|error| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                format!("Unable to stream from Ollama: {error}"),
            )
        })?;

    if !response.status().is_success() {
        return Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            "Unable to stream from Ollama.",
        ));
    }

    let mut byte_stream = response.bytes_stream();
    let stream = async_stream::stream! {
        let mut buffer = String::new();

        while let Some(chunk_result) = byte_stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    buffer.push_str(&String::from_utf8_lossy(&chunk));
                    let complete_line_count = buffer.matches('\n').count();
                    let lines: Vec<String> = buffer.split('\n').map(ToOwned::to_owned).collect();
                    buffer = lines.last().cloned().unwrap_or_default();

                    for line in lines.into_iter().take(complete_line_count) {
                        let line = line.trim();

                        if line.is_empty() {
                            continue;
                        }

                        match serde_json::from_str::<OllamaGenerateChunk>(line) {
                            Ok(parsed) => {
                                if let Some(error) = parsed.error {
                                    yield Ok(Event::default().data(json!({
                                        "type": "error",
                                        "content": error
                                    }).to_string()));
                                    continue;
                                }

                                if let Some(content) = parsed.response {
                                    if !content.is_empty() {
                                        yield Ok(Event::default().data(json!({
                                            "type": "chunk",
                                            "content": content
                                        }).to_string()));
                                    }
                                }

                                if parsed.done.unwrap_or(false) {
                                    yield Ok(Event::default().data(json!({
                                        "type": "done"
                                    }).to_string()));
                                }
                            }
                            Err(error) => {
                                yield Ok(Event::default().data(json!({
                                    "type": "error",
                                    "content": format!("Malformed Ollama stream response: {error}")
                                }).to_string()));
                            }
                        }
                    }
                }
                Err(error) => {
                    yield Ok(Event::default().data(json!({
                        "type": "error",
                        "content": format!("Interrupted Ollama stream: {error}")
                    }).to_string()));
                    break;
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn extract_raw_text(file_name: &str, bytes: &[u8]) -> Result<String, AppError> {
    let extension = Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let parsed = match extension.as_str() {
        "docx" => extract_docx_text(bytes).or_else(|_| decode_utf8(bytes)),
        "csv" => decode_utf8(bytes).map(|text| parse_csv(&text)),
        "json" => parse_json(bytes).or_else(|_| decode_utf8(bytes)),
        "pdf" => pdf_extract::extract_text_from_mem(bytes).or_else(|_| decode_utf8(bytes)),
        _ => decode_utf8(bytes),
    };

    parsed.map(|text| text.trim().to_string()).map_err(|_| {
        AppError::new(
            StatusCode::BAD_REQUEST,
            format!("Unable to parse file: {file_name}"),
        )
    })
}

fn decode_utf8(bytes: &[u8]) -> Result<String, std::str::Utf8Error> {
    std::str::from_utf8(bytes).map(ToOwned::to_owned)
}

fn parse_json(bytes: &[u8]) -> Result<String, serde_json::Error> {
    let value = serde_json::from_slice::<serde_json::Value>(bytes)?;
    serde_json::to_string_pretty(&value)
}

fn parse_csv(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.split(',')
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_docx_text(bytes: &[u8]) -> anyhow::Result<String> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)?;
    let mut document = archive.by_name("word/document.xml")?;
    let mut xml = String::new();
    document.read_to_string(&mut xml)?;

    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);
    let mut text = String::new();

    loop {
        match reader.read_event() {
            Ok(XmlEvent::Text(value)) => {
                text.push_str(&value.unescape()?);
            }
            Ok(XmlEvent::Start(value)) | Ok(XmlEvent::Empty(value)) => {
                let name = value.name();

                if name.as_ref() == b"w:p" {
                    text.push('\n');
                } else if name.as_ref() == b"w:tab" {
                    text.push('\t');
                }
            }
            Ok(XmlEvent::Eof) => break,
            Err(error) => return Err(error.into()),
            _ => {}
        }
    }

    Ok(text)
}

async fn ensure_ollama_running(client: &Client, ollama_base_url: &str) {
    if ollama_is_ready(client, ollama_base_url).await {
        info!("Ollama is already available at {ollama_base_url}");
        return;
    }

    warn!("Ollama is not responding at {ollama_base_url}; attempting to run `ollama serve`.");

    match Command::new("ollama")
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => {
            for _ in 0..30 {
                if ollama_is_ready(client, ollama_base_url).await {
                    info!("Ollama is now available at {ollama_base_url}");
                    return;
                }

                sleep(Duration::from_millis(500)).await;
            }

            warn!("Started `ollama serve`, but Ollama did not become ready within 15 seconds.");
        }
        Err(error) => {
            warn!("Unable to start `ollama serve`: {error}. Start Ollama manually if model listing fails.");
        }
    }
}

async fn ollama_is_ready(client: &Client, ollama_base_url: &str) -> bool {
    client
        .get(format!("{ollama_base_url}/api/tags"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

fn open_browser(url: &str) {
    match webbrowser::open(url) {
        Ok(_) => info!("Opened browser at {url}"),
        Err(error) => warn!("Unable to open browser automatically: {error}. Open {url} manually."),
    }
}

fn frontend_dist_dir() -> PathBuf {
    if let Ok(current_dir) = env::current_dir() {
        if current_dir.join("dist/client/index.html").exists() {
            return current_dir.join("dist/client");
        }

        if current_dir.join("client/index.html").exists() {
            return current_dir.join("client");
        }
    }

    if let Ok(current_exe) = env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            if exe_dir.join("dist/client/index.html").exists() {
                return exe_dir.join("dist/client");
            }

            if exe_dir.join("client/index.html").exists() {
                return exe_dir.join("client");
            }
        }
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("server crate should live inside repository root")
        .join("dist/client")
}
