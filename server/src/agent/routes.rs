use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use axum::{
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};

use super::types::{
    AgentApiError, AgentCapability, AgentHealthResponse, AgentRouteStatus, AgentSessionSummary,
    AgentWorkspaceStatus, ValidateWorkspaceRequest, ValidateWorkspaceResponse,
};

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/health", get(health))
        .route("/capabilities", get(capabilities))
        .route("/sessions", get(sessions))
        .route("/workspace", get(workspace_status))
        .route("/workspace/validate", post(validate_workspace))
}

async fn health() -> Json<AgentHealthResponse> {
    Json(AgentHealthResponse {
        ok: true,
        status: AgentRouteStatus::Stub,
        message: "Agent Mode backend routes are installed. Execution is not enabled yet.".to_string(),
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

async fn sessions() -> Json<Vec<AgentSessionSummary>> {
    Json(Vec::new())
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
        return Err(agent_error(StatusCode::BAD_REQUEST, "Workspace path is required."));
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
        .map(|output| PathBuf::from(output.trim()).canonicalize().ok() == Some(canonical_path.clone()))
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
