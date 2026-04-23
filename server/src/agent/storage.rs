use std::{
    env, fs,
    path::{Path, PathBuf},
};

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use super::types::{AgentEvent, AgentEventType, AgentSessionRecord};

const MAX_EVENTS_PER_SESSION: usize = 500;
const MAX_EVENT_PAYLOAD_CHARS: usize = 12_000;

pub fn list_sessions() -> Result<Vec<AgentSessionRecord>, String> {
    let sessions_dir = sessions_dir()?;

    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();

    for entry in fs::read_dir(&sessions_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();

        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }

        match read_session_file(&path) {
            Ok(session) => sessions.push(session),
            Err(error) => {
                tracing::warn!(path = %path.display(), error = %error, "Unable to read agent session file");
            }
        }
    }

    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(sessions)
}

pub fn create_session(
    workspace_path: String,
    title: Option<String>,
    git_branch: Option<String>,
) -> Result<AgentSessionRecord, String> {
    let canonical_workspace = PathBuf::from(workspace_path)
        .canonicalize()
        .map_err(|error| format!("Unable to resolve workspace path: {error}"))?;
    let now = Utc::now().timestamp_millis();
    let id = Uuid::new_v4().to_string();
    let title = title
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            canonical_workspace
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("Agent Session")
                .to_string()
        });

    let mut session = AgentSessionRecord {
        id: id.clone(),
        title,
        workspace_path: canonical_workspace.to_string_lossy().to_string(),
        git_branch,
        created_at: now,
        updated_at: now,
        memory_summary: String::new(),
        preferred_commands: Vec::new(),
        event_log: Vec::new(),
    };

    let event_workspace_path = session.workspace_path.clone();
    let event_git_branch = session.git_branch.clone();

    push_event(
        &mut session,
        AgentEventType::SessionStarted,
        json!({
            "message": "Agent session created.",
            "workspacePath": event_workspace_path,
            "gitBranch": event_git_branch,
        }),
        None,
    );
    write_session(&session)?;
    Ok(session)
}

pub fn get_session(id: &str) -> Result<AgentSessionRecord, String> {
    read_session_file(&session_path(id)?)
}

pub fn rename_session(id: &str, title: String) -> Result<AgentSessionRecord, String> {
    let mut session = get_session(id)?;
    let title = title.trim();

    if title.is_empty() {
        return Err("Session title cannot be empty.".to_string());
    }

    session.title = title.to_string();
    session.updated_at = Utc::now().timestamp_millis();
    write_session(&session)?;
    Ok(session)
}

pub fn write_session(session: &AgentSessionRecord) -> Result<(), String> {
    let sessions_dir = sessions_dir()?;
    fs::create_dir_all(&sessions_dir).map_err(|error| error.to_string())?;
    let path = session_path(&session.id)?;
    let content = serde_json::to_string_pretty(session).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

fn push_event(
    session: &mut AgentSessionRecord,
    event_type: AgentEventType,
    payload: serde_json::Value,
    parent_event_id: Option<String>,
) {
    let payload = capped_payload(payload);
    let now = Utc::now().timestamp_millis();

    session.event_log.push(AgentEvent {
        id: Uuid::new_v4().to_string(),
        session_id: session.id.clone(),
        timestamp: now,
        event_type,
        payload,
        parent_event_id,
    });

    if session.event_log.len() > MAX_EVENTS_PER_SESSION {
        let overflow = session.event_log.len() - MAX_EVENTS_PER_SESSION;
        session.event_log.drain(0..overflow);
    }

    session.updated_at = now;
}

fn capped_payload(payload: serde_json::Value) -> serde_json::Value {
    let serialized = payload.to_string();

    if serialized.chars().count() <= MAX_EVENT_PAYLOAD_CHARS {
        return payload;
    }

    let truncated = serialized
        .chars()
        .take(MAX_EVENT_PAYLOAD_CHARS)
        .collect::<String>();

    json!({
        "truncated": true,
        "content": truncated,
    })
}

fn read_session_file(path: &Path) -> Result<AgentSessionRecord, String> {
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&content).map_err(|error| error.to_string())
}

fn session_path(id: &str) -> Result<PathBuf, String> {
    if !id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err("Invalid session id.".to_string());
    }

    Ok(sessions_dir()?.join(format!("{id}.json")))
}

fn sessions_dir() -> Result<PathBuf, String> {
    Ok(agent_data_dir()?.join("sessions"))
}

fn agent_data_dir() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("SABIO_AGENT_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }

    if cfg!(target_os = "windows") {
        return env::var("APPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("Sabio").join("agent"))
            .map_err(|_| "APPDATA is not set.".to_string());
    }

    if cfg!(target_os = "macos") {
        return env::var("HOME")
            .map(PathBuf::from)
            .map(|path| path.join("Library/Application Support/Sabio/agent"))
            .map_err(|_| "HOME is not set.".to_string());
    }

    if let Ok(path) = env::var("XDG_DATA_HOME") {
        return Ok(PathBuf::from(path).join("sabio").join("agent"));
    }

    env::var("HOME")
        .map(PathBuf::from)
        .map(|path| path.join(".local/share/sabio/agent"))
        .map_err(|_| "HOME is not set.".to_string())
}
