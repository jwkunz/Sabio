use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHealthResponse {
    pub ok: bool,
    pub status: AgentRouteStatus,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentRouteStatus {
    Stub,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentCapability {
    WorkspaceSelection,
    WorkspaceTrust,
    SessionPersistence,
    EventStreaming,
    ToolValidation,
    CommandExecution,
    Approvals,
    AgentLoop,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSummary {
    pub id: String,
    pub title: String,
    pub workspace_path: String,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkspaceStatus {
    pub selected: bool,
    pub trusted: bool,
    pub workspace_path: Option<String>,
    pub git_branch: Option<String>,
    pub clean_worktree: Option<bool>,
    pub message: String,
}
