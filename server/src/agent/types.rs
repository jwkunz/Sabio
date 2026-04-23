use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentApiError {
    pub error: String,
    pub detail: String,
}

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

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSummary {
    pub id: String,
    pub title: String,
    pub workspace_path: String,
    pub git_branch: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRecord {
    pub id: String,
    pub title: String,
    pub workspace_path: String,
    pub git_branch: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub memory_summary: String,
    pub preferred_commands: Vec<String>,
    pub event_log: Vec<AgentEvent>,
    #[serde(default)]
    pub approvals: Vec<AgentApproval>,
    #[serde(default)]
    pub plans: Vec<AgentPlan>,
}

impl AgentSessionRecord {
    pub fn summary(&self) -> AgentSessionSummary {
        AgentSessionSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            workspace_path: self.workspace_path.clone(),
            git_branch: self.git_branch.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub id: String,
    pub session_id: String,
    pub timestamp: i64,
    #[serde(rename = "type")]
    pub event_type: AgentEventType,
    pub payload: Value,
    pub parent_event_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventType {
    SessionStarted,
    AssistantMessageDelta,
    PlanCreated,
    PlanUpdated,
    ApprovalRequested,
    ApprovalResolved,
    ToolStarted,
    ToolOutput,
    ToolFinished,
    PatchCreated,
    GitCommitCreated,
    Error,
    Cancelled,
    SessionFinished,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentApproval {
    pub id: String,
    pub session_id: String,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
    pub kind: AgentApprovalKind,
    pub status: AgentApprovalStatus,
    pub title: String,
    pub detail: String,
    pub payload: Value,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AgentApprovalKind {
    NetworkCommand,
    DestructiveCommand,
    FileDeletion,
    Plan,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentApprovalStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentPlan {
    pub id: String,
    pub session_id: String,
    pub created_at: i64,
    pub title: String,
    pub summary: String,
    pub steps: Vec<AgentPlanStep>,
    pub approval_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentPlanStep {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub status: AgentPlanStepStatus,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AgentPlanStepStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateWorkspaceRequest {
    pub path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeGitRequest {
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateWorkspaceResponse {
    pub canonical_path: String,
    pub is_git_repo: bool,
    pub git_branch: Option<String>,
    pub clean_worktree: Option<bool>,
    pub trusted: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSessionsQuery {
    pub workspace_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    pub workspace_path: String,
    pub title: Option<String>,
    pub git_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameSessionRequest {
    pub title: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEventsResponse {
    pub events: Vec<AgentEvent>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentApprovalsResponse {
    pub approvals: Vec<AgentApproval>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPlansResponse {
    pub plans: Vec<AgentPlan>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePlanRequest {
    pub title: String,
    pub summary: String,
    pub steps: Vec<CreatePlanStepRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePlanStepRequest {
    pub title: String,
    pub detail: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratePlanRequest {
    pub model: String,
    pub task: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPlanRequest {
    pub model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPlanResponse {
    pub plan: AgentPlan,
    pub summary: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunStatusResponse {
    pub running: bool,
    pub cancelled: bool,
    pub run_id: Option<String>,
    pub plan_id: Option<String>,
    pub started_at: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelRunResponse {
    pub cancelled: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteWriteToolRequest {
    pub tool: String,
    pub args: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveApprovalRequest {
    pub approved: bool,
}
