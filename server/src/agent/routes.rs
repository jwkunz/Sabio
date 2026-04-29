use std::{
    convert::Infallible,
    path::PathBuf,
    process::{Command, Stdio},
};

use async_stream::stream;
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use futures_util::Stream;
use serde::Deserialize;
use serde_json::json;

use crate::AppState;

use super::agent_loop::run_approved_plan;
use super::storage;
use super::tools::{
    checkout_git_branch, create_git_branch, execute_command, execute_git_commit,
    execute_read_only_tool, execute_write_tool, preview_command, read_git_branches,
    read_git_history, tool_specs, validate_tool_call, AgentToolName, CommandClassification,
    CommandExecutionRequest, GitCommitRequest, ToolCallValidationRequest, ToolExecutionRequest,
    command_approval_payload,
};
use super::types::{
    AgentApiError, AgentApprovalKind, AgentApprovalsResponse, AgentCapability, AgentEventsResponse,
    AgentHealthResponse, AgentPlansResponse, AgentRouteStatus, AgentSessionRecord,
    AgentSessionSummary, AgentWorkspaceStatus, CreatePlanRequest, CreateSessionRequest,
    DeleteSessionResponse, ExecuteWriteToolRequest, GeneratePlanRequest, GitBranchMutationResponse,
    GitCheckoutBranchRequest, GitCreateBranchRequest, GitHistoryResponse, InitializeGitRequest,
    ListSessionsQuery, RenameSessionRequest, ResolveApprovalRequest, RunPlanRequest,
    ValidateWorkspaceRequest, ValidateWorkspaceResponse,
};

pub fn router() -> Router<AppState> {
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
        .route("/sessions/:session_id", get(session).delete(delete_session))
        .route("/sessions/:session_id/rename", post(rename_session))
        .route("/sessions/:session_id/events", get(session_events))
        .route(
            "/sessions/:session_id/events/stream",
            get(session_event_stream),
        )
        .route("/sessions/:session_id/approvals", get(session_approvals))
        .route(
            "/sessions/:session_id/plans",
            get(session_plans).post(create_plan),
        )
        .route("/sessions/:session_id/plans/generate", post(generate_plan))
        .route("/sessions/:session_id/plans/:plan_id/run", post(run_plan))
        .route("/sessions/:session_id/run/status", get(run_status))
        .route("/sessions/:session_id/run/cancel", post(cancel_run))
        .route(
            "/sessions/:session_id/tools/execute-write",
            post(execute_write_tool_route),
        )
        .route("/sessions/:session_id/git/commit", post(git_commit))
        .route("/sessions/:session_id/git/history", get(git_history))
        .route("/sessions/:session_id/git/checkout-branch", post(checkout_branch))
        .route("/sessions/:session_id/git/create-branch", post(create_branch))
        .route(
            "/sessions/:session_id/approvals/command",
            post(request_command_approval),
        )
        .route(
            "/sessions/:session_id/approvals/:approval_id/resolve",
            post(resolve_approval),
        )
        .route("/workspace", get(workspace_status))
        .route("/workspace/validate", post(validate_workspace))
        .route("/workspace/init-git", post(initialize_git_workspace))
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

async fn execute_write_tool_route(
    AxumPath(session_id): AxumPath<String>,
    Json(request): Json<ExecuteWriteToolRequest>,
) -> Result<Json<super::tools::ToolExecutionResponse>, (StatusCode, Json<AgentApiError>)> {
    let session = storage::get_session(&session_id)
        .map_err(|error| agent_error(StatusCode::NOT_FOUND, error))?;
    let tool = serde_json::from_value::<AgentToolName>(json!(request.tool))
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    let response = execute_write_tool(ToolExecutionRequest {
        workspace_path: session.workspace_path.clone(),
        tool,
        args: request.args,
    });

    if response.ok {
        let _ = storage::append_event(
            &session_id,
            super::types::AgentEventType::PatchCreated,
            json!({
                "tool": response.tool,
                "payload": response.payload,
            }),
        )
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;
    }

    Ok(Json(response))
}

async fn execute_command_route(
    Json(request): Json<CommandExecutionRequest>,
) -> Json<super::tools::CommandExecutionResponse> {
    Json(execute_command(request).await)
}

async fn git_commit(
    AxumPath(session_id): AxumPath<String>,
    Json(request): Json<GitCommitRequest>,
) -> Result<Json<super::tools::GitCommitResponse>, (StatusCode, Json<AgentApiError>)> {
    let session = storage::get_session(&session_id)
        .map_err(|error| agent_error(StatusCode::NOT_FOUND, error))?;
    let response = execute_git_commit(&session.workspace_path, request);

    if response.ok {
        let _ = storage::append_event(
            &session_id,
            super::types::AgentEventType::GitCommitCreated,
            json!({
                "commitHash": response.commit_hash,
                "stdout": response.stdout,
                "stderr": response.stderr,
            }),
        )
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;
    }

    Ok(Json(response))
}

async fn git_history(
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<GitHistoryResponse>, (StatusCode, Json<AgentApiError>)> {
    let session = storage::get_session(&session_id)
        .map_err(|error| agent_error(StatusCode::NOT_FOUND, error))?;
    let (current_branch, branches) = read_git_branches(&session.workspace_path)
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;
    let entries = read_git_history(&session.workspace_path, 10)
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

    Ok(Json(GitHistoryResponse {
        current_branch,
        branches: branches
            .into_iter()
            .map(|entry| super::types::GitBranchEntry {
                name: entry.name,
                current: entry.current,
            })
            .collect(),
        entries: entries
            .into_iter()
            .map(|entry| super::types::GitHistoryEntry {
                hash: entry.hash,
                short_hash: entry.short_hash,
                author: entry.author,
                authored_at: entry.authored_at,
                summary: entry.summary,
            })
            .collect(),
    }))
}

async fn checkout_branch(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    Json(request): Json<GitCheckoutBranchRequest>,
) -> Result<Json<GitBranchMutationResponse>, (StatusCode, Json<AgentApiError>)> {
    if state.agent_runs.status(&session_id).running {
        return Err(agent_error(
            StatusCode::CONFLICT,
            "Cancel the active agent run before switching branches.",
        ));
    }

    let session = storage::get_session(&session_id)
        .map_err(|error| agent_error(StatusCode::NOT_FOUND, error))?;
    let workspace = inspect_workspace(&session.workspace_path)?;

    if workspace.clean_worktree != Some(true) {
        return Err(agent_error(
            StatusCode::CONFLICT,
            "Workspace must be clean before switching branches.",
        ));
    }

    let current_branch = checkout_git_branch(&session.workspace_path, &request.branch_name)
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;
    storage::update_session_git_branch(&session_id, Some(current_branch.clone()))
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

    Ok(Json(GitBranchMutationResponse {
        ok: true,
        current_branch,
        message: "Branch switched.".to_string(),
    }))
}

async fn create_branch(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    Json(request): Json<GitCreateBranchRequest>,
) -> Result<Json<GitBranchMutationResponse>, (StatusCode, Json<AgentApiError>)> {
    if state.agent_runs.status(&session_id).running {
        return Err(agent_error(
            StatusCode::CONFLICT,
            "Cancel the active agent run before creating a branch.",
        ));
    }

    let session = storage::get_session(&session_id)
        .map_err(|error| agent_error(StatusCode::NOT_FOUND, error))?;
    let workspace = inspect_workspace(&session.workspace_path)?;

    if workspace.clean_worktree != Some(true) {
        return Err(agent_error(
            StatusCode::CONFLICT,
            "Workspace must be clean before creating a branch.",
        ));
    }

    let current_branch = create_git_branch(&session.workspace_path, &request.branch_name)
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;
    storage::update_session_git_branch(&session_id, Some(current_branch.clone()))
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

    Ok(Json(GitBranchMutationResponse {
        ok: true,
        current_branch,
        message: "Branch created and checked out.".to_string(),
    }))
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

async fn delete_session(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<DeleteSessionResponse>, (StatusCode, Json<AgentApiError>)> {
    let run_status = state.agent_runs.status(&session_id);

    if run_status.running {
        return Err(agent_error(
            StatusCode::CONFLICT,
            "Cancel the active agent run before deleting this session.",
        ));
    }

    storage::delete_session(&session_id)
        .map_err(|error| agent_error(StatusCode::NOT_FOUND, error))?;

    Ok(Json(DeleteSessionResponse {
        deleted: true,
        message: "Agent session deleted.".to_string(),
    }))
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

async fn session_approvals(
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<AgentApprovalsResponse>, (StatusCode, Json<AgentApiError>)> {
    storage::list_approvals(&session_id)
        .map(|approvals| Json(AgentApprovalsResponse { approvals }))
        .map_err(|error| agent_error(StatusCode::NOT_FOUND, error))
}

async fn session_plans(
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<AgentPlansResponse>, (StatusCode, Json<AgentApiError>)> {
    storage::list_plans(&session_id)
        .map(|plans| Json(AgentPlansResponse { plans }))
        .map_err(|error| agent_error(StatusCode::NOT_FOUND, error))
}

async fn create_plan(
    AxumPath(session_id): AxumPath<String>,
    Json(request): Json<CreatePlanRequest>,
) -> Result<Json<super::types::AgentPlan>, (StatusCode, Json<AgentApiError>)> {
    let steps = request
        .steps
        .into_iter()
        .map(|step| (step.title, step.detail.unwrap_or_default()))
        .collect();

    storage::create_plan(&session_id, request.title, request.summary, steps)
        .map(Json)
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))
}

async fn generate_plan(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    Json(request): Json<GeneratePlanRequest>,
) -> Result<Json<super::types::AgentPlan>, (StatusCode, Json<AgentApiError>)> {
    let model = request.model.trim();
    let task = request.task.trim();

    if model.is_empty() {
        return Err(agent_error(StatusCode::BAD_REQUEST, "Model is required."));
    }

    if task.is_empty() {
        return Err(agent_error(StatusCode::BAD_REQUEST, "Task is required."));
    }

    let session = storage::get_session(&session_id)
        .map_err(|error| agent_error(StatusCode::NOT_FOUND, error))?;
    let prompt = plan_prompt(&session, task);
    let response = state
        .client
        .post(format!("{}/api/generate", state.ollama_base_url))
        .json(&json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
        }))
        .send()
        .await
        .map_err(|error| agent_error(StatusCode::BAD_GATEWAY, error.to_string()))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(agent_error(
            StatusCode::BAD_GATEWAY,
            format!("Ollama plan generation failed with {status}: {body}"),
        ));
    }

    let body = response
        .json::<OllamaGenerateResponse>()
        .await
        .map_err(|error| agent_error(StatusCode::BAD_GATEWAY, error.to_string()))?;

    if let Some(error) = body.error.filter(|value| !value.trim().is_empty()) {
        return Err(agent_error(StatusCode::BAD_GATEWAY, error));
    }

    let response_text = body.response.unwrap_or_default();
    let plan_json = extract_json_object(&response_text).ok_or_else(|| {
        agent_error(
            StatusCode::BAD_GATEWAY,
            "Model did not return a JSON plan object.",
        )
    })?;
    let draft = serde_json::from_str::<ModelPlanDraft>(&plan_json)
        .map_err(|error| agent_error(StatusCode::BAD_GATEWAY, error.to_string()))?;
    let title = fallback_trim(draft.title, "Generated agent plan", 120);
    let summary = fallback_trim(draft.summary, "Model-generated implementation plan.", 600);
    let steps: Vec<(String, String)> = draft
        .steps
        .into_iter()
        .take(8)
        .filter_map(|step| {
            let title = step.title.trim();
            if title.is_empty() {
                return None;
            }

            Some((
                truncate(title, 160),
                truncate(step.detail.unwrap_or_default().trim(), 800),
            ))
        })
        .collect();

    if steps.is_empty() {
        return Err(agent_error(
            StatusCode::BAD_GATEWAY,
            "Model plan did not include any usable steps.",
        ));
    }

    storage::create_plan(&session_id, title, summary, steps)
        .map(Json)
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))
}

async fn run_plan(
    State(state): State<AppState>,
    AxumPath((session_id, plan_id)): AxumPath<(String, String)>,
    Json(request): Json<RunPlanRequest>,
) -> Result<Json<super::types::RunPlanResponse>, (StatusCode, Json<AgentApiError>)> {
    run_approved_plan(
        &state.agent_runs,
        &state.client,
        &state.ollama_base_url,
        &session_id,
        &plan_id,
        &request.model,
    )
    .await
    .map(Json)
}

async fn run_status(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<super::types::AgentRunStatusResponse> {
    Json(state.agent_runs.status(&session_id))
}

async fn cancel_run(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<super::types::CancelRunResponse> {
    Json(state.agent_runs.cancel_run(&session_id))
}

async fn request_command_approval(
    AxumPath(session_id): AxumPath<String>,
    Json(request): Json<CommandExecutionRequest>,
) -> Result<Json<super::types::AgentApproval>, (StatusCode, Json<AgentApiError>)> {
    let preview = preview_command(&request);

    if preview.blocked {
        return Err(agent_error(
            StatusCode::BAD_REQUEST,
            preview
                .errors
                .first()
                .cloned()
                .unwrap_or_else(|| "Command is blocked by Sabio policy.".to_string()),
        ));
    }

    let kind = match preview.classification {
        CommandClassification::NetworkApprovalRequired => AgentApprovalKind::NetworkCommand,
        CommandClassification::DestructiveApprovalRequired => AgentApprovalKind::DestructiveCommand,
        CommandClassification::Autonomous => {
            return Err(agent_error(
                StatusCode::BAD_REQUEST,
                "Command does not require approval.",
            ));
        }
        CommandClassification::Blocked => {
            return Err(agent_error(
                StatusCode::BAD_REQUEST,
                "Command is blocked by Sabio policy.",
            ));
        }
    };
    let title = format!("{} {}", request.command, request.args.join(" "))
        .trim()
        .to_string();
    let detail = match kind {
        AgentApprovalKind::NetworkCommand => "Network command requires approval.",
        AgentApprovalKind::DestructiveCommand => "Destructive command requires approval.",
        AgentApprovalKind::FileDeletion => "File deletion requires approval.",
        AgentApprovalKind::Plan => "Plan requires approval.",
    }
    .to_string();
    let payload = command_approval_payload(&request, None, None, None, None);

    storage::create_approval(&session_id, kind, title, detail, payload)
        .map(Json)
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))
}

async fn resolve_approval(
    AxumPath((session_id, approval_id)): AxumPath<(String, String)>,
    Json(request): Json<ResolveApprovalRequest>,
) -> Result<Json<super::types::AgentApproval>, (StatusCode, Json<AgentApiError>)> {
    storage::resolve_approval(&session_id, &approval_id, request.approved)
        .map(Json)
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))
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
    inspect_workspace(&request.path).map(Json)
}

async fn initialize_git_workspace(
    Json(request): Json<InitializeGitRequest>,
) -> Result<Json<ValidateWorkspaceResponse>, (StatusCode, Json<AgentApiError>)> {
    let canonical_path = canonical_workspace_path(&request.path)?;
    let workspace = PathBuf::from(&canonical_path);
    let current_status = inspect_workspace(&canonical_path)?;

    if current_status.is_git_repo {
        return Ok(Json(ValidateWorkspaceResponse {
            message: "Workspace is already a git repository.".to_string(),
            ..current_status
        }));
    }

    run_git(&workspace, &["init"]).map_err(|error| {
        agent_error(
            StatusCode::BAD_REQUEST,
            format!("Unable to initialize git repository: {error}"),
        )
    })?;

    let mut initialized = inspect_workspace(&canonical_path)?;
    initialized.message = if initialized.clean_worktree == Some(true) {
        "Git repository initialized and ready to trust.".to_string()
    } else {
        "Git repository initialized. Commit existing files before trust.".to_string()
    };

    Ok(Json(initialized))
}

fn inspect_workspace(
    path: &str,
) -> Result<ValidateWorkspaceResponse, (StatusCode, Json<AgentApiError>)> {
    let raw_path = path.trim();

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

    Ok(ValidateWorkspaceResponse {
        canonical_path: canonical_path.to_string_lossy().to_string(),
        is_git_repo,
        git_branch,
        clean_worktree,
        trusted: false,
        message,
    })
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

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelPlanDraft {
    title: String,
    summary: String,
    #[serde(default)]
    steps: Vec<ModelPlanStepDraft>,
}

#[derive(Debug, Deserialize)]
struct ModelPlanStepDraft {
    title: String,
    detail: Option<String>,
}

fn plan_prompt(session: &AgentSessionRecord, task: &str) -> String {
    let memory_summary = session.memory_summary.trim();
    let preferred_commands = if session.preferred_commands.is_empty() {
        "None recorded.".to_string()
    } else {
        session.preferred_commands.join(", ")
    };

    format!(
        r#"You are Sabio Agent Mode, a local coding agent planning work before execution.
Return ONLY valid JSON with this shape:
{{"title":"short plan title","summary":"one or two sentence summary","steps":[{{"title":"concrete step","detail":"what to inspect or change and how to verify"}}]}}

Planning rules:
- Make 3 to 7 concrete implementation steps.
- Prefer small, reviewable changes.
- Include verification in the final step.
- Do not include Markdown fences, prose before JSON, comments, or trailing commas.
- Commands with network access, destructive file operations, or risky git operations require approval.

Workspace: {}
Git branch: {}
Session memory summary: {}
Preferred autonomous commands: {}
User task: {}
"#,
        session.workspace_path,
        session.git_branch.as_deref().unwrap_or("unknown"),
        if memory_summary.is_empty() {
            "No prior memory."
        } else {
            memory_summary
        },
        preferred_commands,
        task
    )
}

fn extract_json_object(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let without_fence = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let start = without_fence.find('{')?;
    let end = without_fence.rfind('}')?;

    if start > end {
        return None;
    }

    Some(without_fence[start..=end].to_string())
}

fn fallback_trim(value: String, fallback: &str, max_chars: usize) -> String {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        truncate(trimmed, max_chars)
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
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
