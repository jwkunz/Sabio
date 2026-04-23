use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallValidationRequest {
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
