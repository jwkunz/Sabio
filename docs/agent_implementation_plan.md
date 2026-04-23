# Sabio Agent Mode - Implementation Plan

This plan upgrades Sabio from a local Ollama chat UI into a local coding agent while preserving Chat Mode. It assumes the Rust/Axum backend is the source of truth for agent execution and that the frontend consumes streamed agent events.

## Milestone 0 - Branch Setup

### Tasks
- Create the implementation branch:

```bash
git checkout -b feature/agent-mode
```

- Confirm the worktree is clean before starting feature work.

### Commit

```text
init: agent mode branch
```

## Milestone 1 - Backend Agent Module Skeleton

### Tasks
- Split agent code into backend modules under `server/src/agent/`.
- Add placeholder types for:
  - sessions
  - events
  - workspaces
  - tools
  - approvals
- Add route stubs under `/api/agent`.
- Keep existing Chat Mode routes unchanged.

### Commit

```text
feat(agent): add backend agent module skeleton
```

## Milestone 2 - Mode Switch UI

### Tasks
- Add top-level Chat Mode / Agent Mode toggle.
- Preserve existing Chat Mode behavior.
- Create an Agent Mode shell without enabling execution yet.
- Maintain separate UI state per mode.

### Commit

```text
feat(ui): add chat and agent mode switch
```

## Milestone 3 - Workspace Selection And Trust

### Tasks
- Implement workspace selector UI:
  - native folder picker when available
  - manual path fallback
- Add backend workspace validation:
  - canonicalize workspace path
  - ensure path exists and is a directory
  - detect git repository state
  - detect current branch
  - detect clean/dirty worktree
- Add one-time workspace trust prompt.
- Block Agent Mode execution until workspace is trusted.

### Commit

```text
feat(agent): add workspace selection and trust
```

## Milestone 4 - Hybrid Session Persistence

### Tasks
- Add backend-managed authoritative session storage.
- Store:
  - session metadata
  - workspace path
  - branch
  - agent history
  - plan history
  - event log
  - approvals
  - memory summary
  - preferred commands
- Add frontend storage for UI state:
  - selected mode
  - selected session
  - pane sizes
  - expanded panels
  - transient draft
- Implement default workspace session.
- Implement named sessions.
- Implement `\rename <title>`.

### Commit

```text
feat(agent): add hybrid session persistence
```

## Milestone 5 - Agent Event Stream

### Tasks
- Define typed agent event schema.
- Add SSE endpoint for agent runs.
- Persist streamed events with size caps.
- Implement minimum event types:
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
- Add frontend event timeline rendering.

### Commit

```text
feat(agent): stream typed agent events
```

## Milestone 6 - Tool Schema And Validation

### Tasks
- Define structured JSON tool-call schema.
- Require commands as:

```json
{
  "command": "cargo",
  "args": ["check"],
  "cwd": "."
}
```

- Reject shell strings.
- Implement tool registry and validation layer.
- Return structured validation errors to the agent loop.

### Commit

```text
feat(agent): add tool schema validation
```

## Milestone 7 - Read-Only Workspace Tools

### Tasks
- Implement:
  - `list_files`
  - `read_file`
  - `search_text`
  - `git_status`
  - `git_diff`
- Enforce workspace root containment.
- Prevent symlink path escape.
- Add basic backend tests for path validation.

### Commit

```text
feat(agent): add read-only workspace tools
```

## Milestone 8 - Safe Command Runner

### Tasks
- Implement direct process spawning.
- Enforce:
  - no shell execution
  - workspace-contained cwd
  - timeout
  - stdout/stderr capture
  - live output streaming
  - child process cancellation
- Add command classification:
  - autonomous
  - approval-required network
  - approval-required destructive
  - blocked
- Allow autonomous commands after plan approval unless classified otherwise.
- Require approval for package-manager network commands.

### Commit

```text
feat(agent): add safe command runner
```

## Milestone 9 - Approval System

### Tasks
- Add backend approval state machine.
- Add approval API for approve/reject.
- Render approvals:
  - inline in transcript
  - in right-side approvals queue
- Approval-required cases:
  - network commands
  - destructive commands
  - file deletion outside generated temporary directories
- Persist approval outcomes.

### Commit

```text
feat(agent): add approval workflow
```

## Milestone 10 - Plan Generation Flow

### Tasks
- Implement plan-first agent prompt.
- Require structured plan output.
- Render plan in Agent Mode.
- Require user approval before autonomous execution.
- Block execution if workspace is untrusted or git worktree is dirty.

### Commit

```text
feat(agent): add plan approval flow
```

## Milestone 11 - Rust-Controlled Agent Loop

### Tasks
- Implement `agent_loop` module.
- Loop through:
  - model request
  - structured tool call parse
  - validation
  - tool execution or approval pause
  - observation return
  - final summary
- Add invalid-tool-call recovery.
- Abort after repeated invalid model output.
- Preserve existing chat streaming path separately.

### Commit

```text
feat(agent): implement rust-controlled agent loop
```

## Milestone 12 - Patch And Write Tools

### Tasks
- Implement `apply_patch`.
- Implement bounded `write_file` for creation/replacement cases.
- Prefer patch-first modification.
- Auto-apply patches after plan approval.
- Require explicit approval for deleting user/workspace files.
- Allow autonomous deletion only inside generated temporary directories.
- Stream and persist patch/diff events.

### Commit

```text
feat(agent): add patch and write tools
```

## Milestone 13 - Git Commit Integration

### Tasks
- Use current branch.
- Require clean worktree before the agent run starts.
- Commit after each approved plan step.
- Use commit format:

```text
sabio(agent): <step description>
```

- Surface commit failures in transcript and event log.
- Include created commits in final summary.

### Commit

```text
feat(agent): commit completed plan steps
```

## Milestone 14 - Retry And No-Progress Detection

### Tasks
- Detect recoverable command/tool failures.
- Allow bounded retries.
- Detect repeated failures.
- Detect no-progress loops.
- Stream retry decisions to event log.
- Abort with useful diagnostics when stuck.

### Commit

```text
feat(agent): add retry and no-progress handling
```

## Milestone 15 - Cancellation

### Tasks
- Add cancellation API.
- Kill active child process on cancel.
- Stop the agent loop.
- Stream `cancelled` event.
- Show partial diff after cancellation.
- Do not automatically revert changes.

### Commit

```text
feat(agent): add run cancellation
```

## Milestone 16 - Agent Console UI

### Tasks
- Build Agent Mode layout:
  - top workspace/status bar
  - left file explorer
  - center transcript
  - right command log and approvals
  - bottom or overlay diff viewer
- Display:
  - trust status
  - current branch
  - clean/dirty status
  - plan
  - tool calls
  - command output
  - patches
  - commits
  - final summary

### Commit

```text
feat(ui): build agent console
```

## Milestone 17 - Resume And Memory

### Tasks
- Load sessions by workspace.
- Add session picker.
- Restore event history and memory summary.
- Update memory summary after completed runs.
- Keep memory separate from authoritative logs.

### Commit

```text
feat(agent): add session resume and memory
```

## Milestone 18 - End-To-End MVP

### Tasks
- Verify the target flow:

```text
task -> plan -> approval -> patch -> command/test -> commit -> summary
```

- Ensure Chat Mode still works.
- Ensure Agent Mode blocks unsafe or untrusted states.
- Add focused automated checks where practical.

### Commit

```text
feat(agent): complete end-to-end agent mode
```

## Milestone 19 - Polish

### Tasks
- Improve error messages.
- Improve diagnostics for Ollama/model failures.
- Improve command classification messages.
- Review responsive behavior.
- Confirm logo/header presentation is consistent in both modes.
- Update README with Agent Mode usage and safety notes.

### Commit

```text
chore: polish agent mode
```
