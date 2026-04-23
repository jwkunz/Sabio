# Sabio Agent Mode - Requirements Specification (v2)

## 1. Overview

### 1.1 Purpose
Sabio Agent Mode upgrades Sabio from a local Ollama chat interface into a local autonomous coding agent capable of:

- inspecting a selected workspace
- planning coding tasks
- modifying code through validated tools
- executing commands in a workspace-scoped sandbox
- iterating on failures
- committing progress
- preserving resumable project context

### 1.2 Dual Mode System
Sabio supports two top-level modes.

#### Chat Mode
- Preserves existing v1 chatbot behavior.
- Does not expose workspace tools or command execution.
- Uses existing file upload, prompt construction, and streaming chat behavior.

#### Agent Mode
- Provides a workspace-aware agent runtime.
- Uses a Rust-controlled tool loop.
- Executes validated tools and commands.
- Streams transparent agent events to the UI.
- Requires explicit workspace trust before agent execution.

## 2. Workspace Model

### 2.1 Workspace Selection
- User selects a local folder as the active workspace.
- Sabio should use a native folder picker when possible.
- Sabio must provide a manual path fallback.
- The selected workspace must be explicitly trusted by the user before Agent Mode can read, write, or execute inside it.

### 2.2 Workspace Scope
- Agent actions are constrained to the trusted workspace root.
- Backend must canonicalize paths before use.
- Backend must reject path traversal and path escape attempts.
- Symlinks must not allow access outside the workspace root.
- Commands must run with an enforced working directory inside the workspace.

### 2.3 Git Requirements
- Agent Mode requires a clean git worktree before autonomous execution begins.
- If the selected workspace is not a git repo, Sabio may offer to initialize one.
- If the workspace is a git repo with uncommitted changes, Sabio must block autonomous execution and explain how to proceed.
- Agent Mode operates on the current branch by default.
- Sabio must not auto-create a branch in v2.

## 3. Session Model

### 3.1 Session Types
- One default resumable session per workspace.
- Multiple named sessions per workspace.

### 3.2 Session Identity
- Internal workspace identity is based on canonical `workspacePath`.
- Default display name is the workspace folder name.
- Users may rename a session with:

```text
\rename <title>
```

### 3.3 Session Persistence
Sabio uses hybrid persistence:

- Backend stores authoritative agent session data.
- Frontend stores UI preferences and lightweight view state.

Backend session data includes:

- agent history
- user task history
- plan history
- tool/event log
- approvals and approval outcomes
- memory summary
- active workspace path
- current branch
- preferred commands

Frontend UI state includes:

- selected mode
- selected session
- pane sizes
- expanded/collapsed panels
- transient input draft

### 3.4 Event Log Retention
- Sabio persists full logs with size caps.
- Each event should have a per-event size cap.
- Each session should have a total retained log cap.
- Oversized command output should be truncated with a clear marker.
- Summaries may be retained alongside truncated logs.

## 4. Agent Workflow

Agent Mode follows a plan-first execution model:

1. User submits a task.
2. Agent analyzes the workspace.
3. Agent produces a structured plan.
4. User approves the plan.
5. Agent executes autonomously within policy.
6. Agent commits after each approved plan step.
7. Agent summarizes changes, tests, commits, and remaining risks.

The first MVP demo target is an end-to-end task:

```text
task -> plan -> approval -> patch -> command/test -> commit -> summary
```

## 5. Agent Loop

### 5.1 Rust-Controlled Loop
The Rust backend owns the agent loop:

1. Build prompt and tool schema.
2. Send request to Ollama.
3. Receive assistant output.
4. Parse structured JSON tool calls.
5. Validate requested tool call.
6. Execute approved tool call.
7. Return tool result to model.
8. Repeat until final answer, cancellation, or abort condition.

### 5.2 Model Compatibility
- Agent Mode should provide recommended model presets for models that follow structured JSON reliably.
- Users may override the model.
- Backend must validate all model-produced tool calls.
- Invalid tool calls should be returned to the model as structured validation errors when recoverable.
- Repeated invalid tool calls should abort the run with a useful error.

### 5.3 Structured Tool Calls
Tool calls must use structured JSON.
Commands must be represented as direct executable plus argument array.
Sabio must not accept shell strings for command execution.

Example:

```json
{
  "tool": "run_command",
  "args": {
    "command": "cargo",
    "args": ["check", "--manifest-path", "server/Cargo.toml"],
    "cwd": "."
  }
}
```

## 6. Tool System

### 6.1 File Tools
- `list_files`
- `read_file`
- `write_file`
- `apply_patch`

### 6.2 Search Tools
- `search_text`

### 6.3 Command Tools
- `run_command`

### 6.4 Git Tools
- `git_status`
- `git_diff`
- `git_commit`

### 6.5 Write Policy
- File modification should be patch-first.
- The agent may apply patches automatically after plan approval.
- Direct file overwrite should be reserved for clearly bounded file creation or replacement cases.
- File deletion is only autonomous inside generated temporary directories.
- Deleting user/workspace files requires explicit approval.

## 7. Command Execution Model

### 7.1 Sandbox
Sabio v2 uses a workspace-scoped process sandbox:

- Direct process spawning only.
- No shell execution.
- No shell chaining.
- Enforced working directory.
- Path validation for all workspace-relative inputs.
- Process timeouts.
- Captured stdout and stderr.
- Cancellation support for active child processes.

### 7.2 Autonomous Commands
After plan approval, the agent may autonomously run any command except commands classified as:

- network commands requiring approval
- destructive commands requiring approval or blocking
- privileged/system commands
- commands outside the workspace
- shell invocations

### 7.3 Approval Required
Approval is required for:

- destructive commands
- package-manager network commands
- explicit network commands
- file deletion outside generated temporary directories

### 7.4 Network Policy
- Package-manager network commands may be allowed, but require user approval.
- Examples include `npm install`, `npm update`, `cargo fetch`, `cargo install`, `pip install`, and equivalent dependency operations.
- Non-package-manager network commands require user approval.
- Network approvals are per command in v2.

### 7.5 Blocked Commands
Sabio must block:

- commands outside the workspace
- unsafe shell execution
- shell chaining
- privileged/system commands
- attempts to escalate privileges
- commands that intentionally modify system-wide state

## 8. Git Strategy

### 8.1 Branch Behavior
- Agent Mode uses the current branch.
- Sabio does not automatically create a new branch in v2.
- Sabio must display the current branch in Agent Mode.

### 8.2 Clean Worktree
- Agent Mode requires a clean worktree before autonomous execution begins.
- If the worktree is dirty, Sabio must show `git status` information and block execution.

### 8.3 Commits
- Agent commits after each approved plan step.
- Commit format:

```text
sabio(agent): <step description>
```

- Commit failures must be surfaced in the transcript and event log.
- The final summary must list created commits.

## 9. Failure Handling

- Agent should retry recoverable failures.
- Agent must detect repeated failures.
- Agent must abort when no progress is being made.
- Agent must not loop indefinitely.
- Retry attempts and decisions must be visible in the event log.

## 10. Cancellation

When the user cancels an agent run:

- Backend kills the active command process if one is running.
- Backend stops the agent loop.
- UI shows the current state and any partial diff.
- Sabio does not automatically revert changes.
- User decides what to keep, edit, commit, or discard.

## 11. Memory Model

- Memory is persistent per workspace.
- Memory supports session resume.
- Memory should summarize durable project context, preferred commands, and user preferences.
- Memory must not replace authoritative event logs.

## 12. UI Requirements

### 12.1 Mode Switch
Top-level toggle:

- Chat Mode
- Agent Mode

### 12.2 Agent Mode Layout
Agent Mode includes:

- Top: mode switch, workspace selector, trust/current branch status
- Left: file explorer
- Center: agent transcript
- Right: command log and approvals panel
- Bottom or overlay: diff viewer

### 12.3 Approval UX
Approvals appear in both:

- inline transcript context
- right-side approvals queue

The user must be able to approve or reject each pending approval.

## 13. Transparency Model

UI must display:

- workspace trust status
- current branch and clean/dirty status
- plan
- approvals
- tool calls
- command outputs
- patch/diff previews
- commits
- retry attempts
- final summaries

## 14. Agent Event Protocol

Agent Mode must stream typed events from backend to frontend. Minimum event types:

- `session_started`
- `assistant_message_delta`
- `plan_created`
- `approval_requested`
- `approval_resolved`
- `tool_started`
- `tool_output`
- `tool_finished`
- `patch_created`
- `git_commit_created`
- `error`
- `cancelled`
- `session_finished`

Events should include:

- stable event id
- session id
- timestamp
- type
- payload
- optional parent event id

## 15. Slash Commands

Minimum:

```text
\rename <title>
```

Future slash commands may include session, approval, and memory controls.

## 16. Safety Constraints

- Workspace trust required.
- Clean git worktree required.
- No path escape.
- No shell execution.
- No shell chaining.
- Structured command plus args only.
- Command validation required.
- Network approval required.
- Destructive operation approval required unless limited to generated temporary directories.
- Full logs persisted with caps.

## 17. Non-Goals

- Container sandboxing.
- Multi-user support.
- Full IDE editor.
- Remote workspace execution.
- Automatic branch creation.
- Git push/publish automation.
