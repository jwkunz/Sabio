use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

const MAX_READ_BYTES: u64 = 256 * 1024;
const MAX_LIST_ENTRIES: usize = 500;
const MAX_SEARCH_MATCHES: usize = 200;
const MAX_SEARCH_FILE_BYTES: u64 = 512 * 1024;
const DEFAULT_COMMAND_TIMEOUT_SECONDS: u64 = 30;
const MAX_COMMAND_TIMEOUT_SECONDS: u64 = 300;
const MAX_COMMAND_OUTPUT_CHARS: usize = 80_000;
const MAX_WRITE_BYTES: usize = 512 * 1024;
const MAX_PATCH_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallValidationRequest {
    pub tool: AgentToolName,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionRequest {
    pub workspace_path: String,
    pub tool: AgentToolName,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandExecutionRequest {
    pub workspace_path: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: String,
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallValidationResponse {
    pub valid: bool,
    pub tool: AgentToolName,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionResponse {
    pub ok: bool,
    pub tool: AgentToolName,
    pub payload: Value,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandExecutionResponse {
    pub ok: bool,
    pub classification: CommandClassification,
    pub approval_required: bool,
    pub blocked: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitRequest {
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitResponse {
    pub ok: bool,
    pub commit_hash: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHistoryEntry {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub authored_at: String,
    pub summary: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchEntry {
    pub name: String,
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandClassification {
    Autonomous,
    NetworkApprovalRequired,
    DestructiveApprovalRequired,
    Blocked,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolName {
    ListFiles,
    ReadFile,
    WriteFile,
    ApplyPatch,
    SearchText,
    RunCommand,
    GitStatus,
    GitDiff,
    GitCommit,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolSpec {
    pub name: AgentToolName,
    pub description: &'static str,
    pub required_args: Vec<&'static str>,
}

pub fn tool_specs() -> Vec<AgentToolSpec> {
    vec![
        AgentToolSpec {
            name: AgentToolName::ListFiles,
            description: "List files under a workspace-relative directory.",
            required_args: vec!["path"],
        },
        AgentToolSpec {
            name: AgentToolName::ReadFile,
            description: "Read a workspace-contained file.",
            required_args: vec!["path"],
        },
        AgentToolSpec {
            name: AgentToolName::WriteFile,
            description: "Create or replace a bounded workspace-contained file.",
            required_args: vec!["path", "content"],
        },
        AgentToolSpec {
            name: AgentToolName::ApplyPatch,
            description: "Apply a unified patch inside the workspace.",
            required_args: vec!["patch"],
        },
        AgentToolSpec {
            name: AgentToolName::SearchText,
            description: "Search workspace text using a literal or regex pattern.",
            required_args: vec!["pattern"],
        },
        AgentToolSpec {
            name: AgentToolName::RunCommand,
            description: "Run a direct command plus argument array inside the workspace.",
            required_args: vec!["command", "args", "cwd"],
        },
        AgentToolSpec {
            name: AgentToolName::GitStatus,
            description: "Read git status for the workspace.",
            required_args: vec![],
        },
        AgentToolSpec {
            name: AgentToolName::GitDiff,
            description: "Read git diff for the workspace.",
            required_args: vec![],
        },
        AgentToolSpec {
            name: AgentToolName::GitCommit,
            description: "Commit current workspace changes.",
            required_args: vec!["message"],
        },
    ]
}

pub fn validate_tool_call(request: ToolCallValidationRequest) -> ToolCallValidationResponse {
    let mut errors = Vec::new();

    match request.tool {
        AgentToolName::ListFiles => {
            require_string_arg(&request.args, "path", &mut errors);
        }
        AgentToolName::ReadFile => {
            require_string_arg(&request.args, "path", &mut errors);
        }
        AgentToolName::WriteFile => {
            require_string_arg(&request.args, "path", &mut errors);
            require_string_arg(&request.args, "content", &mut errors);
        }
        AgentToolName::ApplyPatch => {
            require_string_arg(&request.args, "patch", &mut errors);
        }
        AgentToolName::SearchText => {
            require_string_arg(&request.args, "pattern", &mut errors);
        }
        AgentToolName::RunCommand => {
            validate_command_args(&request.args, &mut errors);
        }
        AgentToolName::GitStatus | AgentToolName::GitDiff => {}
        AgentToolName::GitCommit => {
            require_string_arg(&request.args, "message", &mut errors);
        }
    }

    ToolCallValidationResponse {
        valid: errors.is_empty(),
        tool: request.tool,
        errors,
    }
}

pub fn execute_read_only_tool(request: ToolExecutionRequest) -> ToolExecutionResponse {
    let validation = validate_tool_call(ToolCallValidationRequest {
        tool: request.tool.clone(),
        args: request.args.clone(),
    });

    if !validation.valid {
        return ToolExecutionResponse {
            ok: false,
            tool: request.tool,
            payload: json!({}),
            errors: validation.errors,
        };
    }

    let workspace_root = match canonical_workspace_root(&request.workspace_path) {
        Ok(path) => path,
        Err(error) => {
            return ToolExecutionResponse {
                ok: false,
                tool: request.tool,
                payload: json!({}),
                errors: vec![error],
            };
        }
    };

    let result = match request.tool {
        AgentToolName::ListFiles => execute_list_files(&workspace_root, &request.args),
        AgentToolName::ReadFile => execute_read_file(&workspace_root, &request.args),
        AgentToolName::SearchText => execute_search_text(&workspace_root, &request.args),
        AgentToolName::GitStatus => {
            execute_git(&workspace_root, &["status", "--short", "--branch"])
        }
        AgentToolName::GitDiff => execute_git(&workspace_root, &["diff", "--no-ext-diff"]),
        AgentToolName::WriteFile
        | AgentToolName::ApplyPatch
        | AgentToolName::RunCommand
        | AgentToolName::GitCommit => {
            Err("Tool is not available in read-only execution mode.".to_string())
        }
    };

    match result {
        Ok(payload) => ToolExecutionResponse {
            ok: true,
            tool: request.tool,
            payload,
            errors: Vec::new(),
        },
        Err(error) => ToolExecutionResponse {
            ok: false,
            tool: request.tool,
            payload: json!({}),
            errors: vec![error],
        },
    }
}

pub fn execute_write_tool(request: ToolExecutionRequest) -> ToolExecutionResponse {
    let validation = validate_tool_call(ToolCallValidationRequest {
        tool: request.tool.clone(),
        args: request.args.clone(),
    });

    if !validation.valid {
        return ToolExecutionResponse {
            ok: false,
            tool: request.tool,
            payload: json!({}),
            errors: validation.errors,
        };
    }

    let workspace_root = match canonical_workspace_root(&request.workspace_path) {
        Ok(path) => path,
        Err(error) => {
            return ToolExecutionResponse {
                ok: false,
                tool: request.tool,
                payload: json!({}),
                errors: vec![error],
            };
        }
    };

    let result = match request.tool {
        AgentToolName::WriteFile => execute_write_file(&workspace_root, &request.args),
        AgentToolName::ApplyPatch => execute_apply_patch(&workspace_root, &request.args),
        AgentToolName::ListFiles
        | AgentToolName::ReadFile
        | AgentToolName::SearchText
        | AgentToolName::GitStatus
        | AgentToolName::GitDiff => {
            Err("Use the read-only execution endpoint for this tool.".to_string())
        }
        AgentToolName::RunCommand | AgentToolName::GitCommit => {
            Err("Tool is not available in write execution mode.".to_string())
        }
    };

    match result {
        Ok(payload) => ToolExecutionResponse {
            ok: true,
            tool: request.tool,
            payload,
            errors: Vec::new(),
        },
        Err(error) => ToolExecutionResponse {
            ok: false,
            tool: request.tool,
            payload: json!({}),
            errors: vec![error],
        },
    }
}

pub fn execute_git_commit(workspace_path: &str, request: GitCommitRequest) -> GitCommitResponse {
    let message = request.message.trim();

    if message.is_empty() {
        return git_commit_response(
            None,
            String::new(),
            String::new(),
            vec!["Commit message is required.".to_string()],
        );
    }

    let workspace_root = match canonical_workspace_root(workspace_path) {
        Ok(path) => path,
        Err(error) => return git_commit_response(None, String::new(), String::new(), vec![error]),
    };

    let status = match execute_git_raw(&workspace_root, &["status", "--porcelain"]) {
        Ok(output) => output,
        Err(error) => return git_commit_response(None, error.stdout, error.stderr, error.errors),
    };

    if status.stdout.trim().is_empty() {
        return git_commit_response(
            None,
            status.stdout,
            status.stderr,
            vec!["No changes to commit.".to_string()],
        );
    }

    if let Err(error) = execute_git_raw(&workspace_root, &["add", "--all"]) {
        return git_commit_response(None, error.stdout, error.stderr, error.errors);
    }

    let commit = match execute_git_raw(&workspace_root, &["commit", "-m", message]) {
        Ok(output) => output,
        Err(error) => return git_commit_response(None, error.stdout, error.stderr, error.errors),
    };
    let hash = match execute_git_raw(&workspace_root, &["rev-parse", "HEAD"]) {
        Ok(output) => output.stdout.trim().to_string(),
        Err(error) => return git_commit_response(None, error.stdout, error.stderr, error.errors),
    };

    git_commit_response(Some(hash), commit.stdout, commit.stderr, Vec::new())
}

pub fn read_git_history(workspace_path: &str, limit: usize) -> Result<Vec<GitHistoryEntry>, String> {
    let workspace_root = canonical_workspace_root(workspace_path)?;
    let count = limit.clamp(1, 20).to_string();
    let output = execute_git_raw(
        &workspace_root,
        &[
            "log",
            "--date=iso-strict",
            "--pretty=format:%H%x1f%h%x1f%an%x1f%ad%x1f%s",
            "-n",
            &count,
        ],
    )
    .map_err(|error| error.errors.join(" ").trim().to_string())?;

    Ok(output
        .stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\u{1f}');
            let hash = parts.next()?.trim();
            let short_hash = parts.next()?.trim();
            let author = parts.next()?.trim();
            let authored_at = parts.next()?.trim();
            let summary = parts.next()?.trim();

            if hash.is_empty() || short_hash.is_empty() || summary.is_empty() {
                return None;
            }

            Some(GitHistoryEntry {
                hash: hash.to_string(),
                short_hash: short_hash.to_string(),
                author: author.to_string(),
                authored_at: authored_at.to_string(),
                summary: summary.to_string(),
            })
        })
        .collect())
}

pub fn read_git_branches(workspace_path: &str) -> Result<(String, Vec<GitBranchEntry>), String> {
    let workspace_root = canonical_workspace_root(workspace_path)?;
    let output = execute_git_raw(
        &workspace_root,
        &["branch", "--format=%(refname:short)%x1f%(HEAD)"],
    )
    .map_err(|error| error.errors.join(" ").trim().to_string())?;

    let branches = output
        .stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\u{1f}');
            let name = parts.next()?.trim();
            let head = parts.next().unwrap_or_default().trim();

            if name.is_empty() {
                return None;
            }

            Some(GitBranchEntry {
                name: name.to_string(),
                current: head == "*",
            })
        })
        .collect::<Vec<_>>();

    let current_branch = branches
        .iter()
        .find(|entry| entry.current)
        .map(|entry| entry.name.clone())
        .unwrap_or_default();

    Ok((current_branch, branches))
}

pub fn checkout_git_branch(workspace_path: &str, branch_name: &str) -> Result<String, String> {
    let workspace_root = canonical_workspace_root(workspace_path)?;
    let branch_name = branch_name.trim();

    if branch_name.is_empty() {
        return Err("Branch name is required.".to_string());
    }

    execute_git_raw(&workspace_root, &["checkout", branch_name])
        .map(|_| branch_name.to_string())
        .map_err(|error| error.errors.join(" ").trim().to_string())
}

pub fn create_git_branch(workspace_path: &str, branch_name: &str) -> Result<String, String> {
    let workspace_root = canonical_workspace_root(workspace_path)?;
    let branch_name = branch_name.trim();

    if branch_name.is_empty() {
        return Err("Branch name is required.".to_string());
    }

    execute_git_raw(&workspace_root, &["checkout", "-b", branch_name])
        .map(|_| branch_name.to_string())
        .map_err(|error| error.errors.join(" ").trim().to_string())
}

pub async fn execute_command(request: CommandExecutionRequest) -> CommandExecutionResponse {
    execute_command_with_policy(request, false).await
}

pub async fn execute_approved_command(request: CommandExecutionRequest) -> CommandExecutionResponse {
    execute_command_with_policy(request, true).await
}

async fn execute_command_with_policy(
    request: CommandExecutionRequest,
    allow_approval_required: bool,
) -> CommandExecutionResponse {
    let preview = preview_command(&request);

    if !preview.errors.is_empty() {
        return command_response_with_approval(
            preview.classification,
            false,
            true,
            None,
            String::new(),
            String::new(),
            false,
            preview.errors,
        );
    }

    let classification = preview.classification;

    if classification == CommandClassification::Blocked {
        return command_response_with_approval(
            classification,
            false,
            true,
            None,
            String::new(),
            String::new(),
            false,
            vec!["Command is blocked by Sabio policy.".to_string()],
        );
    }

    if classification != CommandClassification::Autonomous && !allow_approval_required {
        return command_response_with_approval(
            classification,
            true,
            true,
            None,
            String::new(),
            String::new(),
            false,
            vec!["Command requires approval before execution.".to_string()],
        );
    }

    let workspace_root = match canonical_workspace_root(&request.workspace_path) {
        Ok(path) => path,
        Err(error) => {
            return command_response_with_approval(
                CommandClassification::Blocked,
                false,
                true,
                None,
                String::new(),
                String::new(),
                false,
                vec![error],
            );
        }
    };
    let cwd = match contained_existing_path(&workspace_root, &request.cwd) {
        Ok(path) => path,
        Err(error) => {
            return command_response_with_approval(
                CommandClassification::Blocked,
                false,
                true,
                None,
                String::new(),
                String::new(),
                false,
                vec![error],
            );
        }
    };

    if !cwd.is_dir() {
        return command_response_with_approval(
            CommandClassification::Blocked,
            false,
            true,
            None,
            String::new(),
            String::new(),
            false,
            vec!["cwd must be a directory.".to_string()],
        );
    }

    let timeout_seconds = request
        .timeout_seconds
        .unwrap_or(DEFAULT_COMMAND_TIMEOUT_SECONDS)
        .min(MAX_COMMAND_TIMEOUT_SECONDS);

    let child = match TokioCommand::new(&request.command)
        .args(&request.args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return command_response_with_approval(
                classification,
                false,
                false,
                None,
                String::new(),
                String::new(),
                false,
                vec![format!("Unable to spawn command: {error}")],
            );
        }
    };

    match timeout(
        Duration::from_secs(timeout_seconds),
        child.wait_with_output(),
    )
    .await
    {
        Ok(Ok(output)) => command_response_with_approval(
            classification,
            false,
            false,
            output.status.code(),
            capped_output(&String::from_utf8_lossy(&output.stdout)),
            capped_output(&String::from_utf8_lossy(&output.stderr)),
            false,
            Vec::new(),
        ),
        Ok(Err(error)) => command_response_with_approval(
            classification,
            false,
            false,
            None,
            String::new(),
            String::new(),
            false,
            vec![format!("Command failed while waiting for output: {error}")],
        ),
        Err(_) => command_response_with_approval(
            classification,
            false,
            false,
            None,
            String::new(),
            String::new(),
            true,
            vec![format!(
                "Command timed out after {timeout_seconds} seconds."
            )],
        ),
    }
}

pub fn classify_command(command: &str, args: &[String]) -> CommandClassification {
    let executable = Path::new(command)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(command)
        .to_ascii_lowercase();

    if is_shell_command(&executable)
        || command.contains(['|', '&', ';', '>', '<', '\n'])
        || args.iter().any(|arg| arg.contains('\0'))
    {
        return CommandClassification::Blocked;
    }

    if is_privileged_command(&executable) {
        return CommandClassification::Blocked;
    }

    if is_destructive_command(&executable, args) {
        return CommandClassification::DestructiveApprovalRequired;
    }

    if is_network_command(&executable, args) {
        return CommandClassification::NetworkApprovalRequired;
    }

    CommandClassification::Autonomous
}

pub fn preview_command(request: &CommandExecutionRequest) -> CommandExecutionResponse {
    let errors = validate_command_request(request);
    let classification = if errors.is_empty() {
        classify_command(&request.command, &request.args)
    } else {
        CommandClassification::Blocked
    };
    let blocked = classification == CommandClassification::Blocked || !errors.is_empty();

    command_response(
        classification,
        blocked,
        None,
        String::new(),
        String::new(),
        false,
        errors,
    )
}

pub fn command_approval_payload(
    request: &CommandExecutionRequest,
    plan_id: Option<&str>,
    plan_title: Option<&str>,
    step_id: Option<&str>,
    step_title: Option<&str>,
) -> Value {
    let normalized_cwd = canonical_workspace_root(&request.workspace_path)
        .ok()
        .and_then(|workspace_root| contained_existing_path(&workspace_root, &request.cwd).ok())
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| request.cwd.trim().to_string());

    json!({
        "workspacePath": request.workspace_path,
        "command": request.command,
        "args": request.args,
        "cwd": normalized_cwd,
        "planId": plan_id,
        "planTitle": plan_title,
        "stepId": step_id,
        "stepTitle": step_title,
    })
}

fn execute_list_files(workspace_root: &Path, args: &Value) -> Result<Value, String> {
    let path = require_arg(args, "path")?;
    let target = contained_existing_path(workspace_root, path)?;

    if !target.is_dir() {
        return Err("path must point to a directory.".to_string());
    }

    let mut entries = Vec::new();

    for entry in fs::read_dir(&target).map_err(|error| error.to_string())? {
        if entries.len() >= MAX_LIST_ENTRIES {
            break;
        }

        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        let path = entry.path();
        let relative_path = path
            .strip_prefix(workspace_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        entries.push(json!({
            "path": relative_path,
            "name": entry.file_name().to_string_lossy(),
            "kind": if metadata.is_dir() { "directory" } else if metadata.is_file() { "file" } else { "other" },
            "size": metadata.len(),
        }));
    }

    Ok(json!({
        "path": target.strip_prefix(workspace_root).unwrap_or(&target).to_string_lossy(),
        "entries": entries,
        "truncated": entries.len() >= MAX_LIST_ENTRIES,
    }))
}

fn execute_read_file(workspace_root: &Path, args: &Value) -> Result<Value, String> {
    let path = require_arg(args, "path")?;
    let target = contained_existing_path(workspace_root, path)?;

    if !target.is_file() {
        return Err("path must point to a file.".to_string());
    }

    let metadata = fs::metadata(&target).map_err(|error| error.to_string())?;

    if metadata.len() > MAX_READ_BYTES {
        return Err(format!(
            "File is too large to read through this tool: {} bytes.",
            metadata.len()
        ));
    }

    let content = fs::read_to_string(&target)
        .map_err(|error| format!("Unable to read file as UTF-8 text: {error}"))?;

    Ok(json!({
        "path": target.strip_prefix(workspace_root).unwrap_or(&target).to_string_lossy(),
        "content": content,
        "size": metadata.len(),
    }))
}

fn execute_write_file(workspace_root: &Path, args: &Value) -> Result<Value, String> {
    let path = require_arg(args, "path")?;
    let content = require_raw_arg(args, "content")?;

    if content.len() > MAX_WRITE_BYTES {
        return Err(format!(
            "Content is too large to write through this tool: {} bytes.",
            content.len()
        ));
    }

    let target = contained_writable_path(workspace_root, path)?;
    let existed = target.exists();
    let previous_content = if existed {
        Some(
            fs::read_to_string(&target)
                .map_err(|error| format!("Unable to read existing file as UTF-8 text: {error}"))?,
        )
    } else {
        None
    };

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    fs::write(&target, content).map_err(|error| error.to_string())?;

    Ok(json!({
        "path": target.strip_prefix(workspace_root).unwrap_or(&target).to_string_lossy(),
        "created": !existed,
        "previousSize": previous_content.as_ref().map(|value| value.len()),
        "newSize": content.len(),
    }))
}

fn execute_apply_patch(workspace_root: &Path, args: &Value) -> Result<Value, String> {
    let patch = require_raw_arg(args, "patch")?;

    if patch.len() > MAX_PATCH_BYTES {
        return Err(format!(
            "Patch is too large to apply through this tool: {} bytes.",
            patch.len()
        ));
    }

    if patch_contains_absolute_or_parent_path(patch) {
        return Err("Patch contains an absolute path or parent directory component.".to_string());
    }

    let mut child = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .arg("apply")
        .arg("--whitespace=nowarn")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;

    let patch_input = if patch.ends_with('\n') {
        patch.to_string()
    } else {
        format!("{patch}\n")
    };

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "Unable to open git apply stdin.".to_string())?;
        stdin
            .write_all(patch_input.as_bytes())
            .map_err(|error| error.to_string())?;
    }

    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        return Err(format!(
            "Patch failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let diff = execute_git(workspace_root, &["diff", "--no-ext-diff"])?;

    Ok(json!({
        "applied": true,
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "diff": diff,
    }))
}

fn execute_search_text(workspace_root: &Path, args: &Value) -> Result<Value, String> {
    let pattern = require_arg(args, "pattern")?;
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(".");
    let target = contained_existing_path(workspace_root, path)?;
    let mut matches = Vec::new();

    search_path(workspace_root, &target, pattern, &mut matches)?;

    Ok(json!({
        "pattern": pattern,
        "matches": matches,
        "truncated": matches.len() >= MAX_SEARCH_MATCHES,
    }))
}

fn search_path(
    workspace_root: &Path,
    target: &Path,
    pattern: &str,
    matches: &mut Vec<Value>,
) -> Result<(), String> {
    if matches.len() >= MAX_SEARCH_MATCHES || should_skip_path(target) {
        return Ok(());
    }

    let metadata = fs::symlink_metadata(target).map_err(|error| error.to_string())?;

    if metadata.is_dir() {
        for entry in fs::read_dir(target).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            search_path(workspace_root, &entry.path(), pattern, matches)?;

            if matches.len() >= MAX_SEARCH_MATCHES {
                break;
            }
        }

        return Ok(());
    }

    if !metadata.is_file() || metadata.len() > MAX_SEARCH_FILE_BYTES {
        return Ok(());
    }

    let content = match fs::read_to_string(target) {
        Ok(content) => content,
        Err(_) => return Ok(()),
    };

    for (index, line) in content.lines().enumerate() {
        if line.contains(pattern) {
            matches.push(json!({
                "path": target.strip_prefix(workspace_root).unwrap_or(target).to_string_lossy(),
                "line": index + 1,
                "content": line,
            }));
        }

        if matches.len() >= MAX_SEARCH_MATCHES {
            break;
        }
    }

    Ok(())
}

fn execute_git(workspace_root: &Path, args: &[&str]) -> Result<Value, String> {
    let output = execute_git_raw(workspace_root, args).map_err(|error| error.errors.join("; "))?;

    Ok(json!({
        "stdout": output.stdout,
        "stderr": output.stderr,
        "exitCode": output.exit_code,
    }))
}

struct RawCommandOutput {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    errors: Vec<String>,
}

fn execute_git_raw(
    workspace_root: &Path,
    args: &[&str],
) -> Result<RawCommandOutput, RawCommandOutput> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| RawCommandOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            errors: vec![error.to_string()],
        })?;
    let raw = RawCommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code(),
        errors: Vec::new(),
    };

    if !output.status.success() {
        return Err(RawCommandOutput {
            errors: vec![raw.stderr.trim().to_string()],
            ..raw
        });
    }

    Ok(raw)
}

fn git_commit_response(
    commit_hash: Option<String>,
    stdout: String,
    stderr: String,
    errors: Vec<String>,
) -> GitCommitResponse {
    GitCommitResponse {
        ok: errors.is_empty() && commit_hash.is_some(),
        commit_hash,
        stdout,
        stderr,
        errors,
    }
}

fn canonical_workspace_root(path: &str) -> Result<PathBuf, String> {
    let root = PathBuf::from(path)
        .canonicalize()
        .map_err(|error| format!("Unable to resolve workspace path: {error}"))?;

    if !root.is_dir() {
        return Err("Workspace path must be a directory.".to_string());
    }

    Ok(root)
}

fn contained_existing_path(workspace_root: &Path, path: &str) -> Result<PathBuf, String> {
    let requested = requested_path(workspace_root, path)?;
    let canonical = requested
        .canonicalize()
        .map_err(|error| format!("Unable to resolve workspace-contained path: {error}"))?;

    if !canonical.starts_with(workspace_root) {
        return Err("Path escapes the trusted workspace.".to_string());
    }

    Ok(canonical)
}

fn contained_writable_path(workspace_root: &Path, path: &str) -> Result<PathBuf, String> {
    let requested = requested_path(workspace_root, path)?;
    let parent = requested
        .parent()
        .ok_or_else(|| "Writable path must have a parent directory.".to_string())?;
    let parent = if parent.exists() {
        parent
            .canonicalize()
            .map_err(|error| format!("Unable to resolve parent path: {error}"))?
    } else {
        let ancestor = nearest_existing_ancestor(parent)?;
        let canonical_ancestor = ancestor
            .canonicalize()
            .map_err(|error| format!("Unable to resolve ancestor path: {error}"))?;

        if !canonical_ancestor.starts_with(workspace_root) {
            return Err("Path escapes the trusted workspace.".to_string());
        }

        parent.to_path_buf()
    };

    if !parent.starts_with(workspace_root) {
        return Err("Path escapes the trusted workspace.".to_string());
    }

    if requested.exists() {
        let canonical = requested
            .canonicalize()
            .map_err(|error| format!("Unable to resolve writable path: {error}"))?;

        if !canonical.starts_with(workspace_root) {
            return Err("Path escapes the trusted workspace.".to_string());
        }

        return Ok(canonical);
    }

    Ok(requested)
}

fn nearest_existing_ancestor(path: &Path) -> Result<PathBuf, String> {
    let mut current = path;

    loop {
        if current.exists() {
            return Ok(current.to_path_buf());
        }

        current = current
            .parent()
            .ok_or_else(|| "Unable to find existing ancestor for writable path.".to_string())?;
    }
}

fn requested_path(workspace_root: &Path, path: &str) -> Result<PathBuf, String> {
    let requested = Path::new(path);

    if requested
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("Path must not contain parent directory components.".to_string());
    }

    let joined = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        workspace_root.join(requested)
    };

    Ok(joined)
}

fn should_skip_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|name| {
            matches!(
                name,
                ".git" | "node_modules" | "target" | "dist" | ".vite" | "coverage" | "tmp"
            )
        })
        .unwrap_or(false)
}

fn patch_contains_absolute_or_parent_path(patch: &str) -> bool {
    patch.lines().any(|line| {
        let path = line
            .strip_prefix("+++ ")
            .or_else(|| line.strip_prefix("--- "))
            .or_else(|| line.strip_prefix("diff --git "))
            .unwrap_or_default();

        path.split_whitespace().any(|part| {
            let normalized = part.trim_start_matches("a/").trim_start_matches("b/");
            normalized.starts_with('/') || normalized.split('/').any(|component| component == "..")
        })
    })
}

fn require_arg<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::trim)
        .ok_or_else(|| format!("{name} is required."))
}

fn require_raw_arg<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} is required."))
}

fn validate_command_request(request: &CommandExecutionRequest) -> Vec<String> {
    let mut errors = Vec::new();

    if request.command.trim().is_empty() {
        errors.push("command is required.".to_string());
    }

    if request.cwd.trim().is_empty() {
        errors.push("cwd is required.".to_string());
    }

    if request.command.contains('/') || request.command.contains('\\') {
        errors.push("command must be an executable name, not a path.".to_string());
    }

    if request.command.contains('\0') || request.cwd.contains('\0') {
        errors.push("command and cwd must not contain NUL bytes.".to_string());
    }

    if request.timeout_seconds == Some(0) {
        errors.push("timeoutSeconds must be greater than zero.".to_string());
    }

    errors
}

fn command_response(
    classification: CommandClassification,
    blocked: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
    errors: Vec<String>,
) -> CommandExecutionResponse {
    let approval_required = matches!(
        classification,
        CommandClassification::NetworkApprovalRequired
            | CommandClassification::DestructiveApprovalRequired
    );
    command_response_with_approval(
        classification,
        approval_required,
        blocked,
        exit_code,
        stdout,
        stderr,
        timed_out,
        errors,
    )
}

fn command_response_with_approval(
    classification: CommandClassification,
    approval_required: bool,
    blocked: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
    errors: Vec<String>,
) -> CommandExecutionResponse {
    let ok =
        errors.is_empty() && !blocked && !approval_required && !timed_out && exit_code == Some(0);

    CommandExecutionResponse {
        ok,
        classification,
        approval_required,
        blocked,
        exit_code,
        stdout,
        stderr,
        timed_out,
        errors,
    }
}

fn capped_output(output: &str) -> String {
    if output.chars().count() <= MAX_COMMAND_OUTPUT_CHARS {
        return output.to_string();
    }

    let mut capped = output
        .chars()
        .take(MAX_COMMAND_OUTPUT_CHARS)
        .collect::<String>();
    capped.push_str("\n[Sabio truncated command output]");
    capped
}

fn is_shell_command(command: &str) -> bool {
    matches!(
        command,
        "sh" | "bash" | "zsh" | "fish" | "cmd" | "powershell" | "pwsh"
    )
}

fn is_privileged_command(command: &str) -> bool {
    matches!(
        command,
        "sudo"
            | "su"
            | "doas"
            | "chmod"
            | "chown"
            | "mount"
            | "umount"
            | "systemctl"
            | "service"
            | "launchctl"
            | "reg"
            | "sc"
    )
}

fn is_destructive_command(command: &str, args: &[String]) -> bool {
    if matches!(command, "rm" | "rmdir" | "del" | "erase" | "move" | "mv") {
        return true;
    }

    if command == "git" {
        return args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "reset" | "clean" | "checkout" | "restore" | "rebase"
            )
        });
    }

    if command == "cargo" {
        return args.iter().any(|arg| arg == "clean");
    }

    false
}

fn is_network_command(command: &str, args: &[String]) -> bool {
    if matches!(
        command,
        "curl" | "wget" | "ssh" | "scp" | "sftp" | "ftp" | "gh"
    ) {
        return true;
    }

    match command {
        "npm" | "pnpm" | "yarn" | "bun" => args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "install" | "add" | "update" | "upgrade" | "dlx" | "create"
            )
        }),
        "cargo" => args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "fetch" | "install" | "update" | "publish" | "search"
            )
        }),
        "pip" | "pip3" | "python" | "python3" => args
            .iter()
            .any(|arg| matches!(arg.as_str(), "install" | "download")),
        "git" => args
            .iter()
            .any(|arg| matches!(arg.as_str(), "fetch" | "pull" | "push" | "clone")),
        _ => false,
    }
}

fn validate_command_args(args: &Value, errors: &mut Vec<String>) {
    require_string_arg(args, "command", errors);
    require_string_arg(args, "cwd", errors);

    match args.get("args") {
        Some(Value::Array(values)) => {
            for (index, value) in values.iter().enumerate() {
                if !value.is_string() {
                    errors.push(format!("args[{index}] must be a string."));
                }
            }
        }
        Some(_) => errors.push("args must be an array of strings.".to_string()),
        None => errors.push("args is required.".to_string()),
    }
}

fn require_string_arg(args: &Value, name: &str, errors: &mut Vec<String>) {
    match args.get(name) {
        Some(Value::String(value)) if !value.trim().is_empty() => {}
        Some(Value::String(_)) => errors.push(format!("{name} cannot be empty.")),
        Some(_) => errors.push(format!("{name} must be a string.")),
        None => errors.push(format!("{name} is required.")),
    }
}
