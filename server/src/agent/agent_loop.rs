use axum::{http::StatusCode, Json};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{
    storage,
    tools::{execute_read_only_tool, execute_write_tool, AgentToolName, ToolExecutionRequest},
    types::{
        AgentApiError, AgentApprovalStatus, AgentEventType, AgentPlan, AgentPlanStepStatus,
        AgentSessionRecord, RunPlanResponse,
    },
};

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

pub async fn run_approved_plan(
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
    let mut summary_parts = Vec::new();
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
        let _ = storage::update_plan_step_status(
            session_id,
            &plan.id,
            &step.id,
            AgentPlanStepStatus::InProgress,
        )
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

        let action = match request_step_action(
            client,
            ollama_base_url,
            model,
            &session,
            &plan,
            &step.title,
            &step.detail,
        )
        .await
        {
            Ok(action) => action,
            Err(error) => {
                let _ = storage::update_plan_step_status(
                    session_id,
                    &plan.id,
                    &step.id,
                    AgentPlanStepStatus::Failed,
                );
                return Err(error);
            }
        };

        if !is_agent_loop_tool(&action.tool) {
            let _ = storage::update_plan_step_status(
                session_id,
                &plan.id,
                &step.id,
                AgentPlanStepStatus::Failed,
            );
            return Err(agent_error(
                StatusCode::BAD_GATEWAY,
                "Model requested a tool that is not available in the approved plan loop.",
            ));
        }

        let _ = storage::append_event(
            session_id,
            AgentEventType::ToolStarted,
            json!({
                "tool": action.tool,
                "args": action.args,
                "stepId": step.id,
                "note": action.note,
            }),
        )
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

        let is_write_action = is_write_tool(&action.tool);
        let output = execute_step_tool(ToolExecutionRequest {
            workspace_path: session.workspace_path.clone(),
            tool: action.tool.clone(),
            args: action.args.clone(),
        });
        let output_ok = output.ok;

        let _ = storage::append_event(
            session_id,
            AgentEventType::ToolFinished,
            json!({
                "tool": output.tool,
                "ok": output.ok,
                "payload": output.payload,
                "errors": output.errors,
                "stepId": step.id,
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
                    "stepId": step.id,
                }),
            )
            .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;
        }

        if !output_ok {
            let _ = storage::update_plan_step_status(
                session_id,
                &plan.id,
                &step.id,
                AgentPlanStepStatus::Failed,
            );
            return Err(agent_error(
                StatusCode::BAD_REQUEST,
                "Tool execution failed during plan run.",
            ));
        }

        let note = action
            .note
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("Completed plan step '{}'.", step.title));
        summary_parts.push(note.clone());
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
        }),
    )
    .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?;

    let plan = storage::list_plans(session_id)
        .map_err(|error| agent_error(StatusCode::BAD_REQUEST, error))?
        .into_iter()
        .find(|candidate| candidate.id == plan_id)
        .ok_or_else(|| agent_error(StatusCode::NOT_FOUND, "Plan not found."))?;

    Ok(RunPlanResponse { plan, summary })
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
- apply_patch with args {{"patch":"unified diff that applies cleanly with git apply"}}
- write_file with args {{"path":"relative/file","content":"complete UTF-8 content"}}

Choose exactly one tool that best advances this plan step.
Prefer apply_patch for modifying existing files.
Use write_file only for clearly bounded new files or full-file replacement.
Do not request run_command or git_commit.
Do not delete files.

Workspace: {}
Plan: {}
Plan summary: {}
Step: {}
Step detail: {}
"#,
        session.workspace_path, plan.title, plan.summary, step_title, step_detail
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

fn execute_step_tool(request: ToolExecutionRequest) -> super::tools::ToolExecutionResponse {
    if is_write_tool(&request.tool) {
        execute_write_tool(request)
    } else {
        execute_read_only_tool(request)
    }
}

fn is_agent_loop_tool(tool: &AgentToolName) -> bool {
    matches!(
        tool,
        AgentToolName::ListFiles
            | AgentToolName::ReadFile
            | AgentToolName::SearchText
            | AgentToolName::GitStatus
            | AgentToolName::GitDiff
            | AgentToolName::WriteFile
            | AgentToolName::ApplyPatch
    )
}

fn is_write_tool(tool: &AgentToolName) -> bool {
    matches!(tool, AgentToolName::WriteFile | AgentToolName::ApplyPatch)
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
