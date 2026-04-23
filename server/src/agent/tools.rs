use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MAX_READ_BYTES: u64 = 256 * 1024;
const MAX_LIST_ENTRIES: usize = 500;
const MAX_SEARCH_MATCHES: usize = 200;
const MAX_SEARCH_FILE_BYTES: u64 = 512 * 1024;

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
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(json!({
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "exitCode": output.status.code(),
    }))
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

fn require_arg<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::trim)
        .ok_or_else(|| format!("{name} is required."))
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
