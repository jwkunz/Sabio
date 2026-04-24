use std::{
    env, fs,
    path::{Path, PathBuf},
};

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use super::types::{
    AgentApproval, AgentApprovalKind, AgentApprovalStatus, AgentEvent, AgentEventType, AgentPlan,
    AgentPlanStep, AgentPlanStepStatus, AgentSessionRecord,
};

const MAX_EVENTS_PER_SESSION: usize = 500;
const MAX_EVENT_PAYLOAD_CHARS: usize = 12_000;
const MAX_MEMORY_SUMMARY_CHARS: usize = 4_000;
const MAX_PREFERRED_COMMANDS: usize = 8;

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
        approvals: Vec::new(),
        plans: Vec::new(),
    };

    let event_workspace_path = session.workspace_path.clone();
    let event_git_branch = session.git_branch.clone();

    let _ = push_event(
        &mut session,
        AgentEventType::SessionStarted,
        json!({
            "message": "Agent session created.",
            "workspacePath": event_workspace_path,
            "gitBranch": event_git_branch,
        }),
        None,
    )?;
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

pub fn create_approval(
    session_id: &str,
    kind: AgentApprovalKind,
    title: String,
    detail: String,
    payload: serde_json::Value,
) -> Result<AgentApproval, String> {
    let mut session = get_session(session_id)?;
    let now = Utc::now().timestamp_millis();
    let approval = AgentApproval {
        id: Uuid::new_v4().to_string(),
        session_id: session.id.clone(),
        created_at: now,
        resolved_at: None,
        kind,
        status: AgentApprovalStatus::Pending,
        title,
        detail,
        payload,
    };

    let event_payload = serde_json::to_value(&approval).map_err(|error| error.to_string())?;
    session.approvals.push(approval.clone());
    let _ = push_event(
        &mut session,
        AgentEventType::ApprovalRequested,
        event_payload,
        None,
    )?;
    write_session(&session)?;
    Ok(approval)
}

pub fn list_approvals(session_id: &str) -> Result<Vec<AgentApproval>, String> {
    Ok(get_session(session_id)?.approvals)
}

pub fn resolve_approval(
    session_id: &str,
    approval_id: &str,
    approved: bool,
) -> Result<AgentApproval, String> {
    let mut session = get_session(session_id)?;
    let now = Utc::now().timestamp_millis();
    let mut resolved = None;

    for approval in &mut session.approvals {
        if approval.id != approval_id {
            continue;
        }

        if approval.status != AgentApprovalStatus::Pending {
            return Err("Approval has already been resolved.".to_string());
        }

        approval.status = if approved {
            AgentApprovalStatus::Approved
        } else {
            AgentApprovalStatus::Rejected
        };
        approval.resolved_at = Some(now);
        resolved = Some(approval.clone());
        break;
    }

    let resolved = resolved.ok_or_else(|| "Approval not found.".to_string())?;
    let event_payload = serde_json::to_value(&resolved).map_err(|error| error.to_string())?;

    let _ = push_event(
        &mut session,
        AgentEventType::ApprovalResolved,
        event_payload,
        None,
    )?;
    write_session(&session)?;
    Ok(resolved)
}

pub fn create_plan(
    session_id: &str,
    title: String,
    summary: String,
    steps: Vec<(String, String)>,
) -> Result<AgentPlan, String> {
    let mut session = get_session(session_id)?;
    let title = title.trim();
    let summary = summary.trim();

    if title.is_empty() {
        return Err("Plan title cannot be empty.".to_string());
    }

    if steps.is_empty() {
        return Err("Plan must include at least one step.".to_string());
    }

    let now = Utc::now().timestamp_millis();
    let plan = AgentPlan {
        id: Uuid::new_v4().to_string(),
        session_id: session.id.clone(),
        created_at: now,
        title: title.to_string(),
        summary: summary.to_string(),
        steps: steps
            .into_iter()
            .map(|(title, detail)| AgentPlanStep {
                id: Uuid::new_v4().to_string(),
                title,
                detail,
                status: AgentPlanStepStatus::Pending,
            })
            .collect(),
        approval_id: None,
    };

    let approval = AgentApproval {
        id: Uuid::new_v4().to_string(),
        session_id: session.id.clone(),
        created_at: now,
        resolved_at: None,
        kind: AgentApprovalKind::Plan,
        status: AgentApprovalStatus::Pending,
        title: format!("Approve plan: {}", plan.title),
        detail: if plan.summary.is_empty() {
            "Plan requires approval before execution.".to_string()
        } else {
            plan.summary.clone()
        },
        payload: serde_json::to_value(&plan).map_err(|error| error.to_string())?,
    };
    let mut plan = plan;
    plan.approval_id = Some(approval.id.clone());

    let plan_payload = serde_json::to_value(&plan).map_err(|error| error.to_string())?;
    let approval_payload = serde_json::to_value(&approval).map_err(|error| error.to_string())?;
    session.plans.push(plan.clone());
    session.approvals.push(approval);
    let _ = push_event(
        &mut session,
        AgentEventType::PlanCreated,
        plan_payload,
        None,
    )?;
    let _ = push_event(
        &mut session,
        AgentEventType::ApprovalRequested,
        approval_payload,
        None,
    )?;
    write_session(&session)?;
    Ok(plan)
}

pub fn list_plans(session_id: &str) -> Result<Vec<AgentPlan>, String> {
    Ok(get_session(session_id)?.plans)
}

pub fn update_plan_step_status(
    session_id: &str,
    plan_id: &str,
    step_id: &str,
    status: AgentPlanStepStatus,
) -> Result<AgentPlan, String> {
    let mut session = get_session(session_id)?;
    let mut updated_plan = None;

    for plan in &mut session.plans {
        if plan.id != plan_id {
            continue;
        }

        let step = plan
            .steps
            .iter_mut()
            .find(|step| step.id == step_id)
            .ok_or_else(|| "Plan step not found.".to_string())?;
        step.status = status.clone();
        updated_plan = Some(plan.clone());
        break;
    }

    let updated_plan = updated_plan.ok_or_else(|| "Plan not found.".to_string())?;
    let payload = serde_json::to_value(&updated_plan).map_err(|error| error.to_string())?;

    let _ = push_event(
        &mut session,
        AgentEventType::PlanUpdated,
        json!({
            "message": "Plan step status updated.",
            "plan": payload,
            "stepId": step_id,
            "status": status,
        }),
        None,
    )?;
    write_session(&session)?;
    Ok(updated_plan)
}

pub fn append_event(
    session_id: &str,
    event_type: AgentEventType,
    payload: serde_json::Value,
) -> Result<AgentEvent, String> {
    let mut session = get_session(session_id)?;
    let event = push_event(&mut session, event_type, payload, None)?;
    write_session(&session)?;
    Ok(event)
}

pub fn update_memory_summary(session_id: &str, entry: &str) -> Result<AgentSessionRecord, String> {
    let mut session = get_session(session_id)?;
    let entry = entry.trim();

    if entry.is_empty() {
        return Ok(session);
    }

    let existing_entries = session
        .memory_summary
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string);
    let new_entries = entry
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string);
    let mut merged = existing_entries.chain(new_entries).collect::<Vec<_>>();

    if merged.len() > 8 {
        let keep_from = merged.len() - 8;
        merged.drain(0..keep_from);
    }

    let mut summary = merged.join("\n");
    if summary.chars().count() > MAX_MEMORY_SUMMARY_CHARS {
        let truncated = summary
            .chars()
            .rev()
            .take(MAX_MEMORY_SUMMARY_CHARS)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        summary = truncated;
    }

    session.memory_summary = summary;
    session.updated_at = Utc::now().timestamp_millis();
    write_session(&session)?;
    Ok(session)
}

pub fn update_preferred_commands(
    session_id: &str,
    command_entry: &str,
) -> Result<AgentSessionRecord, String> {
    let mut session = get_session(session_id)?;
    let command_entry = command_entry.trim();

    if command_entry.is_empty() {
        return Ok(session);
    }

    session
        .preferred_commands
        .retain(|entry| entry.trim() != command_entry);
    session.preferred_commands.push(command_entry.to_string());

    if session.preferred_commands.len() > MAX_PREFERRED_COMMANDS {
        let keep_from = session.preferred_commands.len() - MAX_PREFERRED_COMMANDS;
        session.preferred_commands.drain(0..keep_from);
    }

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
) -> Result<AgentEvent, String> {
    let payload = capped_payload(payload);
    let now = Utc::now().timestamp_millis();
    let event = AgentEvent {
        id: Uuid::new_v4().to_string(),
        session_id: session.id.clone(),
        timestamp: now,
        event_type,
        payload,
        parent_event_id,
    };

    session.event_log.push(event.clone());

    if session.event_log.len() > MAX_EVENTS_PER_SESSION {
        let overflow = session.event_log.len() - MAX_EVENTS_PER_SESSION;
        session.event_log.drain(0..overflow);
    }

    session.updated_at = now;
    Ok(event)
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
