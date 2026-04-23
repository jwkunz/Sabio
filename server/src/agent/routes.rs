use axum::{routing::get, Json, Router};

use super::types::{
    AgentCapability, AgentHealthResponse, AgentRouteStatus, AgentSessionSummary, AgentWorkspaceStatus,
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
