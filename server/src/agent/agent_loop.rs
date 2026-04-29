use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{http::StatusCode, Json};
use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

use super::{
    storage,
    tools::{
        execute_approved_command, execute_command, execute_git_commit, execute_read_only_tool,
        execute_write_tool, preview_command, AgentToolName, CommandClassification,
        CommandExecutionRequest, CommandExecutionResponse, GitCommitRequest, ToolExecutionRequest,
        ToolExecutionResponse, command_approval_payload,
    },
    types::{
        AgentApiError, AgentApproval, AgentApprovalKind, AgentApprovalStatus, AgentEventType,
        AgentPlan, AgentPlanStepStatus, AgentRunOutcome, AgentRunStatusResponse,
        AgentSessionRecord, CancelRunResponse, RunPlanResponse,
    },
};

const MAX_STEP_ATTEMPTS: usize = 3;
const CANCEL_POLL_INTERVAL_MS: u64 = 200;
const APPROVAL_PAUSE_PREFIX: &str = "Agent paused pending approval:";

#[derive(Clone, Default)]
pub struct AgentRunRegistry {
    inner: Arc<Mutex<HashMap<String, AgentRunState>>>,
}

#[derive(Clone)]
struct AgentRunState {
    run_id: String,
    plan_id: String,
    started_at: i64,
    cancelled: bool,
}

pub struct AgentRunLease {
    registry: AgentRunRegistry,
    session_id: String,
    run_id: String,
}

impl AgentRunRegistry {
    fn start_run(&self, session_id: &str, plan_id: &str) -> Result<AgentRunLease, String> {
        let mut runs = self
            .inner
            .lock()
            .map_err(|_| "Unable to lock agent run registry.".to_string())?;

        if runs
            .get(session_id)
            .map(|state| !state.cancelled)
            .unwrap_or(false)
        {
            return Err("An agent run is already active for this session.".to_string());
        }

        let run_id = Uuid::new_v4().to_string();
        runs.insert(
            session_id.to_string(),
            AgentRunState {
                run_id: run_id.clone(),
                plan_id: plan_id.to_string(),
                started_at: Utc::now().timestamp_millis(),
                cancelled: false,
            },
        );

        Ok(AgentRunLease {
            registry: self.clone(),
            session_id: session_id.to_string(),
            run_id,
        })
    }

    pub fn cancel_run(&self, session_id: &str) -> CancelRunResponse {
        let Ok(mut runs) = self.inner.lock() else {
            return CancelRunResponse {
                cancelled: false,
                message: "Unable to lock agent run registry.".to_string(),
            };
        };

        if let Some(state) = runs.get_mut(session_id) {
            state.cancelled = true;
            return CancelRunResponse {
                cancelled: true,
                message: "Cancellation requested.".to_string(),
            };
        }

        CancelRunResponse {
            cancelled: false,
            message: "No active run for this session.".to_string(),
        }
    }

    pub fn status(&self, session_id: &str) -> AgentRunStatusResponse {
        let Ok(runs) = self.inner.lock() else {
            return AgentRunStatusResponse {
                running: false,
                cancelled: false,
                run_id: None,
                plan_id: None,
                started_at: None,
            };
        };

        if let Some(state) = runs.get(session_id) {
            return AgentRunStatusResponse {
                running: !state.cancelled,
                cancelled: state.cancelled,
                run_id: Some(state.run_id.clone()),
                plan_id: Some(state.plan_id.clone()),
                started_at: Some(state.started_at),
            };
        }

        AgentRunStatusResponse {
            running: false,
            cancelled: false,
            run_id: None,
            plan_id: None,
            started_at: None,
        }
    }
}

impl AgentRunLease {
    fn is_cancelled(&self) -> bool {
        self.registry.status(&self.session_id).cancelled
    }
}

impl Drop for AgentRunLease {
    fn drop(&mut self) {
        if let Ok(mut runs) = self.registry.inner.lock() {
            let should_remove = runs
                .get(&self.session_id)
                .map(|state| state.run_id == self.run_id)
                .unwrap_or(false);

            if should_remove {
                runs.remove(&self.session_id);
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelStepAction {
    tool: AgentToolName,
    #[serde(default)]
    args: Value,
    note: Option<String>,
}

enum StepToolOutcome {
    Completed(ToolExecutionResponse),
    ApprovalRequested {
        response: ToolExecutionResponse,
        message: String,
    },
}

pub async fn run_approved_plan(
    registry: &AgentRunRegistry,
    client: &Client,
    ollama_base_url: &str,
    session_id: &str,
    plan_id: &str,
    model: &str,
) -> Result<RunPlanResponse, (StatusCode, Json<AgentApiError>)> {
    let model = model.trim();

    if model.is_empty() {
        return Err(agent_error(StatusCode::BAD_REQUEST, "Model is required."));
    }

    let session = storage::get_session(session_id)
        .map_err(|error| agent_error(StatusCode::NOT_FOUND, error))?;
    let plan = session
        .plans
        .iter()
        .find(|candidate| candidate.id == plan_id)
        .cloned()
        .ok_or_else(|| agent_error(StatusCode::NOT_FOUND, "Plan not found."))?;

    ensure_plan_approved(&session, &plan)?;
    ensure_clean_worktree(&session.workspace_path)?;
    let run_lease = registry
        .start_run(session_id, &plan.id)
        .map_err(|error| agent_error(StatusCode::CONFLICT, error))?;
    let mut summary_parts = Vec::new();
    let mut commit_hashes = Vec::new();
    let _ = storage::append_event(
        session_id,
        AgentEventType::ToolStarted,
        json!({
            "tool": "agent_loop",
            "planId": plan.id,
            "message": "Started approved plan run."
        }),
    )
    .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

    for step in plan.steps.clone() {
        ensure_run_not_cancelled(session_id, &plan.id, &step.id, &run_lease)?;
        let _ = storage::update_plan_step_status(
            session_id,
            &plan.id,
            &step.id,
            AgentPlanStepStatus::InProgress,
        )
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

        let action = match run_step_with_retries(
            client,
            ollama_base_url,
            model,
            session_id,
            &session,
            &plan,
            &step.id,
            &step.title,
            &step.detail,
            &run_lease,
        )
        .await
        {
            Ok(action) => action,
            Err(error) => {
                if is_approval_pause_error(&error.1.detail) {
                    let _ = storage::update_plan_step_status(
                        session_id,
                        &plan.id,
                        &step.id,
                        AgentPlanStepStatus::Pending,
                    );
                    return Ok(RunPlanResponse {
                        plan: storage::list_plans(session_id)
                            .map_err(|storage_error| agent_error(StatusCode::BAD_REQUEST, storage_error))?
                            .into_iter()
                            .find(|candidate| candidate.id == plan_id)
                            .ok_or_else(|| agent_error(StatusCode::NOT_FOUND, "Plan not found."))?,
                        summary: error.1.detail.clone(),
                        outcome: AgentRunOutcome::Paused,
                    });
                }
                if is_cancelled_error(&error.1.detail) {
                    return Ok(RunPlanResponse {
                        plan: storage::list_plans(session_id)
                            .map_err(|storage_error| agent_error(StatusCode::BAD_REQUEST, storage_error))?
                            .into_iter()
                            .find(|candidate| candidate.id == plan_id)
                            .ok_or_else(|| agent_error(StatusCode::NOT_FOUND, "Plan not found."))?,
                        summary: error.1.detail.clone(),
                        outcome: AgentRunOutcome::Cancelled,
                    });
                }
                let _ = append_run_memory(
                    session_id,
                    &build_failure_memory_entry(
                        &plan.title,
                        Some(&step.title),
                        &error.1.detail,
                        if is_cancelled_error(&error.1.detail) {
                            "cancelled"
                        } else {
                            "failed"
                        },
                    ),
                );
                if !run_lease.is_cancelled() {
                    let _ = storage::update_plan_step_status(
                        session_id,
                        &plan.id,
                        &step.id,
                        AgentPlanStepStatus::Failed,
                    );
                }
                return Err(error);
            }
        };

        let note = action
            .note
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("Completed plan step '{}'.", step.title));
        summary_parts.push(note.clone());

        ensure_run_not_cancelled(session_id, &plan.id, &step.id, &run_lease)?;
        if workspace_has_changes(&session.workspace_path)? {
            let commit = execute_git_commit(
                &session.workspace_path,
                GitCommitRequest {
                    message: step_commit_message(&step.title),
                },
            );

            if !commit.ok {
                let _ = storage::update_plan_step_status(
                    session_id,
                    &plan.id,
                    &step.id,
                    AgentPlanStepStatus::Failed,
                );
                let _ = storage::append_event(
                    session_id,
                    AgentEventType::Error,
                    json!({
                        "message": "Unable to commit completed plan step.",
                        "stepId": step.id,
                        "stdout": commit.stdout,
                        "stderr": commit.stderr,
                        "errors": commit.errors,
                    }),
                );
                let _ = append_run_memory(
                    session_id,
                    &build_failure_memory_entry(
                        &plan.title,
                        Some(&step.title),
                        "Unable to commit completed plan step.",
                        "failed",
                    ),
                );
                return Err(agent_error(
                    StatusCode::BAD_REQUEST,
                    "Unable to commit completed plan step.",
                ));
            }

            let commit_hash = commit.commit_hash.clone();
            let _ = storage::append_event(
                session_id,
                AgentEventType::GitCommitCreated,
                json!({
                    "stepId": step.id,
                    "commitHash": commit.commit_hash,
                    "stdout": commit.stdout,
                    "stderr": commit.stderr,
                }),
            )
            .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

            if let Some(hash) = commit_hash {
                commit_hashes.push(hash.clone());
                summary_parts.push(format!("Committed step as {hash}."));
            }
        }

        let _ = storage::append_event(
            session_id,
            AgentEventType::AssistantMessageDelta,
            json!({
                "stepId": step.id,
                "message": note,
            }),
        )
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

        let _ = storage::update_plan_step_status(
            session_id,
            &plan.id,
            &step.id,
            AgentPlanStepStatus::Completed,
        )
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;
    }

    let summary = if summary_parts.is_empty() {
        "Approved plan run completed.".to_string()
    } else {
        summary_parts.join("\n")
    };
    let _ = storage::append_event(
        session_id,
        AgentEventType::SessionFinished,
        json!({
            "planId": plan.id,
            "summary": summary,
            "commitHashes": commit_hashes,
            "outcome": "completed",
        }),
    )
    .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;
    let _ = storage::update_memory_summary(
        session_id,
        &build_memory_entry(&plan.title, &summary, &commit_hashes),
    )
    .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

    let plan = storage::list_plans(session_id)
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?
        .into_iter()
        .find(|candidate| candidate.id == plan_id)
        .ok_or_else(|| agent_error(StatusCode::NOT_FOUND, "Plan not found."))?;

    Ok(RunPlanResponse {
        plan,
        summary,
        outcome: AgentRunOutcome::Completed,
    })
}

async fn request_step_action(
    client: &Client,
    ollama_base_url: &str,
    model: &str,
    session: &AgentSessionRecord,
    plan: &AgentPlan,
    step_title: &str,
    step_detail: &str,
) -> Result<ModelStepAction, (StatusCode, Json<AgentApiError>)> {
    let preferred_commands = if session.preferred_commands.is_empty() {
        "None recorded.".to_string()
    } else {
        session.preferred_commands.join(", ")
    };
    let prompt = format!(
        r#"You are Sabio Agent Mode running one step of an approved plan.
Return ONLY valid JSON with this shape:
{{"tool":"git_status","args":{{}},"note":"short observation or change summary"}}

Allowed tool names:
- list_files with args {{"path":"."}}
- read_file with args {{"path":"relative/file"}}
- search_text with args {{"pattern":"literal or regex","path":"optional/relative/path"}}
- git_status with args {{}}
- git_diff with args {{}}
- run_command with args {{"command":"cargo","args":["check"],"cwd":".","timeoutSeconds":30}}
- apply_patch with args {{"patch":"unified diff that applies cleanly with git apply"}}
- write_file with args {{"path":"relative/file","content":"complete UTF-8 content"}}

Choose exactly one tool that best advances this plan step.
Prefer apply_patch for modifying existing files.
Use write_file only for clearly bounded new files or full-file replacement.
Use run_command for workspace-scoped commands. If a necessary command requires network or destructive approval, Sabio will pause and request approval before continuing.
Do not request git_commit.
Do not delete files.

Workspace: {}
Plan: {}
Plan summary: {}
Session memory summary: {}
Preferred autonomous commands: {}
Step: {}
Step detail: {}
"#,
        session.workspace_path,
        plan.title,
        plan.summary,
        if session.memory_summary.trim().is_empty() {
            "No prior memory."
        } else {
            session.memory_summary.trim()
        },
        preferred_commands,
        step_title,
        step_detail
    );
    let response = client
        .post(format!("{ollama_base_url}/api/generate"))
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
            format!("Ollama agent loop request failed with {status}: {body}"),
        ));
    }

    let body = response
        .json::<OllamaGenerateResponse>()
        .await
        .map_err(|error| agent_error(StatusCode::BAD_GATEWAY, error.to_string()))?;

    if let Some(error) = body.error.filter(|value| !value.trim().is_empty()) {
        return Err(agent_error(StatusCode::BAD_GATEWAY, error));
    }

    let raw = body.response.unwrap_or_default();
    let json_text = extract_json_object(&raw).ok_or_else(|| {
        agent_error(
            StatusCode::BAD_GATEWAY,
            "Model did not return a JSON tool action.",
        )
    })?;

    serde_json::from_str::<ModelStepAction>(&json_text)
        .map_err(|error| agent_error(StatusCode::BAD_GATEWAY, error.to_string()))
}

async fn request_step_action_cancellable(
    client: &Client,
    ollama_base_url: &str,
    model: &str,
    session: &AgentSessionRecord,
    plan: &AgentPlan,
    step_id: &str,
    step_title: &str,
    step_detail: &str,
    run_lease: &AgentRunLease,
) -> Result<ModelStepAction, (StatusCode, Json<AgentApiError>)> {
    let request_future = request_step_action(
        client,
        ollama_base_url,
        model,
        session,
        plan,
        step_title,
        step_detail,
    );
    tokio::pin!(request_future);

    loop {
        tokio::select! {
            result = &mut request_future => return result,
            _ = sleep(Duration::from_millis(CANCEL_POLL_INTERVAL_MS)) => {
                ensure_run_not_cancelled(&session.id, &plan.id, step_id, run_lease)?;
            }
        }
    }
}

async fn run_step_with_retries(
    client: &Client,
    ollama_base_url: &str,
    model: &str,
    session_id: &str,
    session: &AgentSessionRecord,
    plan: &AgentPlan,
    step_id: &str,
    step_title: &str,
    step_detail: &str,
    run_lease: &AgentRunLease,
) -> Result<ModelStepAction, (StatusCode, Json<AgentApiError>)> {
    let mut last_diagnostic = String::new();

    for attempt in 1..=MAX_STEP_ATTEMPTS {
        ensure_run_not_cancelled(session_id, &plan.id, step_id, run_lease)?;
        let latest_session = storage::get_session(session_id).unwrap_or_else(|_| session.clone());
        let action = match request_step_action_cancellable(
            client,
            ollama_base_url,
            model,
            &latest_session,
            plan,
            step_id,
            step_title,
            step_detail,
            run_lease,
        )
        .await
        {
            Ok(action) => action,
            Err(error) => {
                if run_lease.is_cancelled() || is_cancelled_error(&error.1.detail) {
                    return Err(error);
                }
                last_diagnostic = error.1.detail.clone();
                if attempt < MAX_STEP_ATTEMPTS {
                    emit_retry_event(
                        session_id,
                        step_id,
                        attempt,
                        "model_action",
                        &last_diagnostic,
                    )?;
                    continue;
                }

                emit_abort_event(session_id, step_id, attempt, &last_diagnostic)?;
                return Err(error);
            }
        };

        ensure_run_not_cancelled(session_id, &plan.id, step_id, run_lease)?;
        if !is_agent_loop_tool(&action.tool) {
            last_diagnostic = format!(
                "Model requested unavailable tool '{}'.",
                serde_json::to_string(&action.tool).unwrap_or_else(|_| "unknown".to_string())
            );
            if attempt < MAX_STEP_ATTEMPTS {
                emit_retry_event(
                    session_id,
                    step_id,
                    attempt,
                    "unavailable_tool",
                    &last_diagnostic,
                )?;
                continue;
            }

            emit_abort_event(session_id, step_id, attempt, &last_diagnostic)?;
            return Err(agent_error(StatusCode::BAD_GATEWAY, last_diagnostic));
        }

        let _ = storage::append_event(
            session_id,
            AgentEventType::ToolStarted,
            json!({
                "tool": action.tool,
                "args": action.args,
                "stepId": step_id,
                "attempt": attempt,
                "note": action.note,
            }),
        )
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

        let is_write_action = is_write_tool(&action.tool);
        let outcome = execute_step_tool(&latest_session, plan, step_id, &action).await;
        let (output, approval_pause_message) = match outcome {
            StepToolOutcome::Completed(output) => (output, None),
            StepToolOutcome::ApprovalRequested { response, message } => (response, Some(message)),
        };
        let output_ok = output.ok;

        let _ = storage::append_event(
            session_id,
            AgentEventType::ToolFinished,
            json!({
                "tool": output.tool,
                "ok": output.ok,
                "payload": output.payload,
                "errors": output.errors,
                "stepId": step_id,
                "attempt": attempt,
            }),
        )
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

        if output_ok && is_write_action {
            let _ = storage::append_event(
                session_id,
                AgentEventType::PatchCreated,
                json!({
                    "tool": output.tool,
                    "payload": output.payload,
                    "stepId": step_id,
                    "attempt": attempt,
                }),
            )
            .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;
        }

        if output_ok {
            remember_successful_command(session.id.as_str(), &action);
            return Ok(action);
        }

        if let Some(message) = approval_pause_message {
            return Err(agent_error(StatusCode::CONFLICT, message));
        }

        last_diagnostic = output.errors.join("; ");
        if attempt < MAX_STEP_ATTEMPTS {
            emit_retry_event(
                session_id,
                step_id,
                attempt,
                "tool_failure",
                &last_diagnostic,
            )?;
            continue;
        }
    }

    emit_abort_event(session_id, step_id, MAX_STEP_ATTEMPTS, &last_diagnostic)?;
    Err(agent_error(
        StatusCode::BAD_REQUEST,
        if last_diagnostic.is_empty() {
            "No progress after repeated step attempts.".to_string()
        } else {
            format!("No progress after repeated step attempts: {last_diagnostic}")
        },
    ))
}

async fn execute_step_tool(
    session: &AgentSessionRecord,
    plan: &AgentPlan,
    step_id: &str,
    action: &ModelStepAction,
) -> StepToolOutcome {
    match action.tool {
        AgentToolName::RunCommand => {
            execute_step_command(session, plan, step_id, &action.args).await
        }
        _ => {
            let request = ToolExecutionRequest {
                workspace_path: session.workspace_path.clone(),
                tool: action.tool.clone(),
                args: action.args.clone(),
            };

            if is_write_tool(&action.tool) {
                StepToolOutcome::Completed(execute_write_tool(request))
            } else {
                StepToolOutcome::Completed(execute_read_only_tool(request))
            }
        }
    }
}

async fn execute_step_command(
    session: &AgentSessionRecord,
    plan: &AgentPlan,
    step_id: &str,
    args: &Value,
) -> StepToolOutcome {
    let request = match serde_json::from_value::<CommandExecutionRequest>(json!({
        "workspacePath": session.workspace_path,
        "command": args.get("command").cloned().unwrap_or(Value::Null),
        "args": args.get("args").cloned().unwrap_or_else(|| json!([])),
        "cwd": args.get("cwd").cloned().unwrap_or_else(|| json!(".")),
        "timeoutSeconds": args.get("timeoutSeconds").cloned().unwrap_or(Value::Null),
    })) {
        Ok(request) => request,
        Err(error) => {
            return StepToolOutcome::Completed(ToolExecutionResponse {
                ok: false,
                tool: AgentToolName::RunCommand,
                payload: json!({}),
                errors: vec![format!("Invalid run_command args: {error}")],
            });
        }
    };
    let preview = preview_command(&request);

    if preview.blocked || !preview.approval_required {
        let response = execute_command(request).await;
        return StepToolOutcome::Completed(command_execution_to_tool_response(response));
    }

    if let Some(approval) = find_matching_command_approval(session, &request) {
        return match approval.status {
            AgentApprovalStatus::Approved => {
                let response = execute_approved_command(request).await;
                StepToolOutcome::Completed(command_execution_to_tool_response(response))
            }
            AgentApprovalStatus::Pending => {
                let message = format!(
                    "{APPROVAL_PAUSE_PREFIX} Resolve pending approval '{}' and rerun the plan.",
                    approval.title
                );
                StepToolOutcome::ApprovalRequested {
                    response: command_approval_response(preview.classification, &approval, false),
                    message,
                }
            }
            AgentApprovalStatus::Rejected => StepToolOutcome::Completed(ToolExecutionResponse {
                ok: false,
                tool: AgentToolName::RunCommand,
                payload: json!({
                    "classification": preview.classification,
                    "approvalId": approval.id,
                    "approvalStatus": approval.status,
                }),
                errors: vec![format!(
                    "Command approval '{}' was rejected.",
                    approval.title
                )],
            }),
        };
    }

    let kind = match preview.classification {
        CommandClassification::NetworkApprovalRequired => AgentApprovalKind::NetworkCommand,
        CommandClassification::DestructiveApprovalRequired => AgentApprovalKind::DestructiveCommand,
        CommandClassification::Autonomous | CommandClassification::Blocked => {
            let response = execute_command(request).await;
            return StepToolOutcome::Completed(command_execution_to_tool_response(response));
        }
    };
    let title = format!("{} {}", request.command, request.args.join(" "))
        .trim()
        .to_string();
    let detail = format!(
        "{} approval required before the agent can continue plan '{}' step '{}'.",
        match kind {
            AgentApprovalKind::NetworkCommand => "Network command",
            AgentApprovalKind::DestructiveCommand => "Destructive command",
            AgentApprovalKind::FileDeletion => "File deletion",
            AgentApprovalKind::Plan => "Plan",
        },
        plan.title,
        step_id
    );
    let payload = command_approval_payload(
        &request,
        Some(&plan.id),
        Some(&plan.title),
        Some(step_id),
        Some(&step_title_from_plan(plan, step_id)),
    );
    let approval = match storage::create_approval(&session.id, kind, title, detail, payload) {
        Ok(approval) => approval,
        Err(error) => {
            return StepToolOutcome::Completed(ToolExecutionResponse {
                ok: false,
                tool: AgentToolName::RunCommand,
                payload: json!({}),
                errors: vec![error],
            });
        }
    };
    let message = format!(
        "{APPROVAL_PAUSE_PREFIX} Approve '{}' and rerun the plan.",
        approval.title
    );

    StepToolOutcome::ApprovalRequested {
        response: command_approval_response(preview.classification, &approval, true),
        message,
    }
}

fn command_execution_to_tool_response(response: CommandExecutionResponse) -> ToolExecutionResponse {
    let ok = response.ok && !response.approval_required && !response.blocked;
    let mut errors = response.errors.clone();

    if response.approval_required && errors.is_empty() {
        errors.push(
            "Command requires approval and is not allowed inside the autonomous agent loop."
                .to_string(),
        );
    }

    ToolExecutionResponse {
        ok,
        tool: AgentToolName::RunCommand,
        payload: json!({
            "classification": response.classification,
            "approvalRequired": response.approval_required,
            "blocked": response.blocked,
            "exitCode": response.exit_code,
            "stdout": response.stdout,
            "stderr": response.stderr,
            "timedOut": response.timed_out,
        }),
        errors,
    }
}

fn command_approval_response(
    classification: CommandClassification,
    approval: &AgentApproval,
    created_now: bool,
) -> ToolExecutionResponse {
    ToolExecutionResponse {
        ok: false,
        tool: AgentToolName::RunCommand,
        payload: json!({
            "classification": classification,
            "approvalRequired": true,
            "approvalId": approval.id,
            "approvalStatus": approval.status,
            "approvalKind": approval.kind,
            "createdNow": created_now,
        }),
        errors: vec![format!(
            "Command requires approval before execution. Resolve approval '{}'.",
            approval.title
        )],
    }
}

fn find_matching_command_approval(
    session: &AgentSessionRecord,
    request: &CommandExecutionRequest,
) -> Option<AgentApproval> {
    let payload = command_approval_payload(request, None, None, None, None);
    let mut matched = session
        .approvals
        .iter()
        .rev()
        .filter(|approval| {
            matches!(
                approval.kind,
                AgentApprovalKind::NetworkCommand | AgentApprovalKind::DestructiveCommand
            ) && approval.payload == payload
        })
        .cloned()
        .collect::<Vec<_>>();

    matched.sort_by_key(|approval| match approval.status {
        AgentApprovalStatus::Approved => 0,
        AgentApprovalStatus::Pending => 1,
        AgentApprovalStatus::Rejected => 2,
    });

    matched.into_iter().next()
}

fn step_title_from_plan(plan: &AgentPlan, step_id: &str) -> String {
    plan.steps
        .iter()
        .find(|step| step.id == step_id)
        .map(|step| step.title.clone())
        .unwrap_or_else(|| step_id.to_string())
}

fn ensure_plan_approved(
    session: &AgentSessionRecord,
    plan: &AgentPlan,
) -> Result<(), (StatusCode, Json<AgentApiError>)> {
    let approval_id = plan.approval_id.as_deref().ok_or_else(|| {
        agent_error(
            StatusCode::CONFLICT,
            "Plan has no approval record and cannot be run.",
        )
    })?;
    let approval = session
        .approvals
        .iter()
        .find(|approval| approval.id == approval_id)
        .ok_or_else(|| agent_error(StatusCode::CONFLICT, "Plan approval record not found."))?;

    if approval.status != AgentApprovalStatus::Approved {
        return Err(agent_error(
            StatusCode::CONFLICT,
            "Plan must be approved before it can run.",
        ));
    }

    Ok(())
}

fn ensure_clean_worktree(workspace_path: &str) -> Result<(), (StatusCode, Json<AgentApiError>)> {
    if workspace_has_changes(workspace_path)? {
        return Err(agent_error(
            StatusCode::CONFLICT,
            "Workspace must be clean before running an approved plan.",
        ));
    }

    Ok(())
}

fn ensure_run_not_cancelled(
    session_id: &str,
    plan_id: &str,
    step_id: &str,
    run_lease: &AgentRunLease,
) -> Result<(), (StatusCode, Json<AgentApiError>)> {
    if !run_lease.is_cancelled() {
        return Ok(());
    }

    let _ = storage::update_plan_step_status(
        session_id,
        plan_id,
        step_id,
        AgentPlanStepStatus::Cancelled,
    );
    let _ = storage::append_event(
        session_id,
        AgentEventType::Cancelled,
        json!({
            "message": "Agent run cancelled by user.",
            "planId": plan_id,
            "stepId": step_id,
            "outcome": "cancelled",
        }),
    );

    Err(agent_error(StatusCode::CONFLICT, "Agent run cancelled."))
}

fn workspace_has_changes(workspace_path: &str) -> Result<bool, (StatusCode, Json<AgentApiError>)> {
    let output = execute_read_only_tool(ToolExecutionRequest {
        workspace_path: workspace_path.to_string(),
        tool: AgentToolName::GitStatus,
        args: json!({}),
    });

    if !output.ok {
        return Err(agent_error(
            StatusCode::BAD_REQUEST,
            output
                .errors
                .first()
                .cloned()
                .unwrap_or_else(|| "Unable to read git status.".to_string()),
        ));
    }

    let stdout = output
        .payload
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok(stdout
        .lines()
        .any(|line| !line.trim().is_empty() && !line.starts_with("##")))
}

fn is_agent_loop_tool(tool: &AgentToolName) -> bool {
    matches!(
        tool,
        AgentToolName::ListFiles
            | AgentToolName::ReadFile
            | AgentToolName::SearchText
            | AgentToolName::GitStatus
            | AgentToolName::GitDiff
            | AgentToolName::RunCommand
            | AgentToolName::WriteFile
            | AgentToolName::ApplyPatch
    )
}

fn is_write_tool(tool: &AgentToolName) -> bool {
    matches!(tool, AgentToolName::WriteFile | AgentToolName::ApplyPatch)
}

fn step_commit_message(step_title: &str) -> String {
    let title = step_title
        .lines()
        .next()
        .unwrap_or("completed plan step")
        .trim();
    let title = if title.is_empty() {
        "completed plan step"
    } else {
        title
    };
    let title: String = title.chars().take(80).collect();

    format!("sabio(agent): {title}")
}

fn build_memory_entry(plan_title: &str, summary: &str, commit_hashes: &[String]) -> String {
    let short_summary = summary
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    let commit_summary = if commit_hashes.is_empty() {
        "no commits".to_string()
    } else {
        format!("commits {}", commit_hashes.join(", "))
    };

    format!(
        "- {}: {} ({})",
        plan_title.trim(),
        truncate_memory_text(&short_summary, 280),
        commit_summary
    )
}

fn build_failure_memory_entry(
    plan_title: &str,
    step_title: Option<&str>,
    diagnostic: &str,
    outcome: &str,
) -> String {
    let mut detail = String::new();

    if let Some(step_title) = step_title.map(str::trim).filter(|value| !value.is_empty()) {
        detail.push_str("during ");
        detail.push_str(step_title);
        detail.push_str(": ");
    }

    detail.push_str(&truncate_memory_text(diagnostic.trim(), 220));

    format!(
        "- {}: {} ({})",
        plan_title.trim(),
        detail,
        outcome.trim()
    )
}

fn append_run_memory(
    session_id: &str,
    entry: &str,
) -> Result<(), (StatusCode, Json<AgentApiError>)> {
    storage::update_memory_summary(session_id, entry)
        .map(|_| ())
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))
}

fn remember_successful_command(session_id: &str, action: &ModelStepAction) {
    if !matches!(action.tool, AgentToolName::RunCommand) {
        return;
    }

    if let Some(command_entry) = build_command_preference(&action.args) {
        let _ = storage::update_preferred_commands(session_id, &command_entry);
    }
}

fn build_command_preference(args: &Value) -> Option<String> {
    let command = args.get("command")?.as_str()?.trim();

    if command.is_empty() {
        return None;
    }

    let command_args = args
        .get("args")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let cwd = args
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != ".");

    let mut parts = vec![command.to_string()];
    parts.extend(command_args.into_iter().map(ToString::to_string));
    let mut entry = parts.join(" ");

    if let Some(cwd) = cwd {
        entry.push_str(" @ ");
        entry.push_str(cwd);
    }

    Some(truncate_memory_text(&entry, 140))
}

fn truncate_memory_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn emit_retry_event(
    session_id: &str,
    step_id: &str,
    attempt: usize,
    reason: &str,
    diagnostic: &str,
) -> Result<(), (StatusCode, Json<AgentApiError>)> {
    let _ = storage::append_event(
        session_id,
        AgentEventType::Error,
        json!({
            "message": "Retrying plan step after recoverable failure.",
            "stepId": step_id,
            "attempt": attempt,
            "nextAttempt": attempt + 1,
            "maxAttempts": MAX_STEP_ATTEMPTS,
            "reason": reason,
            "diagnostic": diagnostic,
        }),
    )
    .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

    Ok(())
}

fn emit_abort_event(
    session_id: &str,
    step_id: &str,
    attempt: usize,
    diagnostic: &str,
) -> Result<(), (StatusCode, Json<AgentApiError>)> {
    let _ = storage::append_event(
        session_id,
        AgentEventType::Error,
        json!({
            "message": "Aborting plan step after repeated failures.",
            "stepId": step_id,
            "attempt": attempt,
            "maxAttempts": MAX_STEP_ATTEMPTS,
            "diagnostic": diagnostic,
        }),
    )
    .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

    Ok(())
}

fn is_cancelled_error(detail: &str) -> bool {
    detail.trim() == "Agent run cancelled."
}

fn is_approval_pause_error(detail: &str) -> bool {
    detail.trim().starts_with(APPROVAL_PAUSE_PREFIX)
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
