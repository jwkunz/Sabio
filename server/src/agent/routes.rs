use std::{
    convert::Infallible,
    path::PathBuf,
    process::{Command, Stdio},
};

use async_stream::stream;
use axum::{
    extract::{Path as AxumPath, Query},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use futures_util::Stream;
use serde_json::json;

use super::storage;
use super::tools::{
    execute_command, execute_read_only_tool, tool_specs, validate_tool_call,
    CommandExecutionRequest, ToolCallValidationRequest, ToolExecutionRequest,
};
use super::types::{
    AgentApiError, AgentCapability, AgentEventsResponse, AgentHealthResponse, AgentRouteStatus,
    AgentSessionRecord, AgentSessionSummary, AgentWorkspaceStatus, CreateSessionRequest,
    ListSessionsQuery, RenameSessionRequest, ValidateWorkspaceRequest, ValidateWorkspaceResponse,
};

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/health", get(health))
        .route("/capabilities", get(capabilities))
        .route("/tools", get(tools))
        .route("/tools/validate", post(validate_tool))
        .route(
            "/tools/execute-read-only",
            post(execute_read_only_tool_route),
        )
        .route("/commands/execute", post(execute_command_route))
        .route("/sessions", get(sessions).post(create_session))
        .route("/sessions/:session_id", get(session))
        .route("/sessions/:session_id/rename", post(rename_session))
        .route("/sessions/:session_id/events", get(session_events))
        .route(
            "/sessions/:session_id/events/stream",
            get(session_event_stream),
        )
        .route("/workspace", get(workspace_status))
        .route("/workspace/validate", post(validate_workspace))
}

async fn health() -> Json<AgentHealthResponse> {
    Json(AgentHealthResponse {
        ok: true,
        status: AgentRouteStatus::Stub,
        message: "Agent Mode backend routes are installed. Execution is not enabled yet."
            .to_string(),
    })
}

async fn capabilities() -> Json<Vec<AgentCapability>> {
    Json(vec![
        AgentCapability::WorkspaceSelection,
        AgentCapability::WorkspaceTrust,
        AgentCapability::SessionPersistence,
        AgentCapability::EventStreaming,
        AgentCapability::ToolValidation,
        AgentCapability::CommandExecution,
        AgentCapability::Approvals,
        AgentCapability::AgentLoop,
    ])
}

async fn tools() -> Json<Vec<super::tools::AgentToolSpec>> {
    Json(tool_specs())
}

async fn validate_tool(
    Json(request): Json<ToolCallValidationRequest>,
) -> Json<super::tools::ToolCallValidationResponse> {
    Json(validate_tool_call(request))
}

async fn execute_read_only_tool_route(
    Json(request): Json<ToolExecutionRequest>,
) -> Json<super::tools::ToolExecutionResponse> {
    Json(execute_read_only_tool(request))
}

async fn execute_command_route(
    Json(request): Json<CommandExecutionRequest>,
) -> Json<super::tools::CommandExecutionResponse> {
    Json(execute_command(request).await)
}

async fn sessions(
    Query(query): Query<ListSessionsQuery>,
) -> Result<Json<Vec<AgentSessionSummary>>, (StatusCode, Json<AgentApiError>)> {
    let workspace_filter = query
        .workspace_path
        .as_deref()
        .map(canonical_workspace_path)
        .transpose()?;
    let sessions = storage::list_sessions()
        .map_err(|error| agent_error(StatusCode::INTERNAL_SERVER_ERROR, error))?
        .into_iter()
        .filter(|session| {
            workspace_filter
                .as_ref()
                .map(|workspace| session.workspace_path == *workspace)
                .unwrap_or(true)
        })
        .map(|session| session.summary())
        .collect();

    Ok(Json(sessions))
}

async fn create_session(
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<AgentSessionRecord>, (StatusCode, Json<AgentApiError>)> {
    let canonical_path = canonical_workspace_path(&request.workspace_path)?;
    let session = storage::create_session(canonical_path, request.title, request.git_branch)
        .map_err(|error| agent_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;

    Ok(Json(session))
}

async fn session(
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<AgentSessionRecord>, (StatusCode, Json<AgentApiError>)> {
    storage::get_session(&session_id)
        .map(Json)
        .map_err(|error| agent_error(StatusCode::NOT_FOUND, error))
}

async fn rename_session(
    AxumPath(session_id): AxumPath<String>,
    Json(request): Json<RenameSessionRequest>,
) -> Result<Json<AgentSessionRecord>, (StatusCode, Json<AgentApiError>)> {
    storage::rename_session(&session_id, request.title)
        .map(Json)
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))
}

async fn session_events(
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<AgentEventsResponse>, (StatusCode, Json<AgentApiError>)> {
    let session = storage::get_session(&session_id)
        .map_err(|error| agent_error(StatusCode::NOT_FOUND, error))?;

    Ok(Json(AgentEventsResponse {
        events: session.event_log,
    }))
}

async fn session_event_stream(
    AxumPath(session_id): AxumPath<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<AgentApiError>)> {
    let session = storage::get_session(&session_id)
        .map_err(|error| agent_error(StatusCode::NOT_FOUND, error))?;

    let event_stream = stream! {
        for event in session.event_log {
            yield Ok(Event::default().data(json!(event).to_string()));
        }

        yield Ok(Event::default().data(json!({
            "id": format!("{}-replay-finished", session_id),
            "sessionId": session_id,
            "timestamp": chrono::Utc::now().timestamp_millis(),
            "type": "session_finished",
            "payload": {
                "message": "Stored event replay finished."
            },
            "parentEventId": null
        }).to_string()));
    };

    Ok(Sse::new(event_stream).keep_alive(KeepAlive::default()))
}

async fn workspace_status() -> Json<AgentWorkspaceStatus> {
    Json(AgentWorkspaceStatus {
        selected: false,
        trusted: false,
        workspace_path: None,
        git_branch: None,
        clean_worktree: None,
        message: "No trusted workspace has been selected.".to_string(),
    })
}

async fn validate_workspace(
    Json(request): Json<ValidateWorkspaceRequest>,
) -> Result<Json<ValidateWorkspaceResponse>, (StatusCode, Json<AgentApiError>)> {
    let raw_path = request.path.trim();

    if raw_path.is_empty() {
        return Err(agent_error(
            StatusCode::BAD_REQUEST,
            "Workspace path is required.",
        ));
    }

    let path = PathBuf::from(raw_path);
    let canonical_path = path.canonicalize().map_err(|error| {
        agent_error(
            StatusCode::BAD_REQUEST,
            format!("Unable to resolve workspace path: {error}"),
        )
    })?;

    if !canonical_path.is_dir() {
        return Err(agent_error(
            StatusCode::BAD_REQUEST,
            "Workspace path must be a directory.",
        ));
    }

    let git_root = run_git(&canonical_path, &["rev-parse", "--show-toplevel"]);
    let is_git_repo = git_root
        .as_ref()
        .map(|output| {
            PathBuf::from(output.trim()).canonicalize().ok() == Some(canonical_path.clone())
        })
        .unwrap_or(false);

    let git_branch = if is_git_repo {
        run_git(&canonical_path, &["branch", "--show-current"]).ok()
    } else {
        None
    }
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty());

    let clean_worktree = if is_git_repo {
        run_git(&canonical_path, &["status", "--porcelain"])
            .ok()
            .map(|output| output.trim().is_empty())
    } else {
        None
    };

    let message = if !is_git_repo {
        "Workspace is not a git repository.".to_string()
    } else if clean_worktree == Some(false) {
        "Workspace has uncommitted changes.".to_string()
    } else {
        "Workspace is ready to trust.".to_string()
    };

    Ok(Json(ValidateWorkspaceResponse {
        canonical_path: canonical_path.to_string_lossy().to_string(),
        is_git_repo,
        git_branch,
        clean_worktree,
        trusted: false,
        message,
    }))
}

fn run_git(workspace: &PathBuf, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn canonical_workspace_path(path: &str) -> Result<String, (StatusCode, Json<AgentApiError>)> {
    let raw_path = path.trim();

    if raw_path.is_empty() {
        return Err(agent_error(
            StatusCode::BAD_REQUEST,
            "Workspace path is required.",
        ));
    }

    let canonical_path = PathBuf::from(raw_path).canonicalize().map_err(|error| {
        agent_error(
            StatusCode::BAD_REQUEST,
            format!("Unable to resolve workspace path: {error}"),
        )
    })?;

    if !canonical_path.is_dir() {
        return Err(agent_error(
            StatusCode::BAD_REQUEST,
            "Workspace path must be a directory.",
        ));
    }

    Ok(canonical_path.to_string_lossy().to_string())
}

fn agent_error(
    status: StatusCode,
    message: impl Into<String>,
) -> (StatusCode, Json<AgentApiError>) {
    let message = message.into();

    (
        status,
        Json(AgentApiError {
            error: message.clone(),
            detail: message,
        }),
    )
}
