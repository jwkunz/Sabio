# Sabio Agent Mode - Current Requirements

This document describes the current Agent Mode MVP as implemented on `feature/agent-mode`.

## 1. Product Shape

Sabio supports two top-level modes:

- `Chat Mode`
  Local Ollama chat with uploaded-file context.

- `Agent Mode`
  A workspace-aware coding agent backed by the Rust server.

Agent Mode is designed around a reviewed, plan-first workflow:

`task -> plan -> approval -> edit -> command/test -> commit -> summary`

## 2. Workspace Rules

Agent Mode only operates inside a validated workspace.

Current workspace behavior:

- user enters a workspace path in the UI
- backend canonicalizes the path
- backend validates directory existence, git status, and branch
- user must explicitly trust the workspace before agent execution

Autonomous execution requires:

- a git repository
- a clean worktree

If the folder is not a git repository, Sabio may initialize one from the UI.

## 3. Session Model

Agent Mode persists backend-owned sessions per workspace.

Each session currently stores:

- workspace path
- current branch
- plans
- approvals
- structured event log
- memory summary
- preferred autonomous commands

Users can:

- create sessions by trusting a workspace
- rename sessions from the Session panel
- delete old sessions
- resume prior sessions in the same workspace

## 4. Planning Model

Agent Mode is plan-first.

Current flow:

1. User enters a task.
2. Sabio asks the selected Ollama model for a structured JSON plan.
3. Sabio stores the plan and creates a plan approval.
4. User approves the plan.
5. Sabio runs the approved plan step by step.

Manual fallback:

- the UI also supports creating a starter plan for testing the approval path

## 5. Tooling Model

The Rust backend validates all model-produced tool calls.

Current tool families:

- file tools
  - `list_files`
  - `read_file`
  - `write_file`
  - `apply_patch`

- search tools
  - `search_text`

- git tools
  - `git_status`
  - `git_diff`
  - `git_commit`

- command tools
  - `run_command`

Current write behavior:

- patch-first for existing files
- bounded direct writes for clearly scoped create/replace cases
- no autonomous deletion of normal workspace files

## 6. Command Execution Policy

Commands are represented as:

```json
{
  "command": "cargo",
  "args": ["check"],
  "cwd": "."
}
```

Sabio does not accept shell strings for autonomous command execution.

Command classes:

- autonomous
- approval-required
- blocked

Current autonomous loop behavior:

- safe commands may run after plan approval
- network/destructive commands pause the run and create a command approval
- once approved, the same command can run on resume without creating a duplicate approval

## 7. Git Strategy

Agent Mode uses the current branch.

Current git rules:

- no automatic branch creation
- clean worktree required before a run starts
- if a plan step changes files, Sabio commits after the step

Commit format:

```text
sabio(agent): <step description>
```

## 8. Run Lifecycle

Current terminal run outcomes:

- `completed`
- `paused`
- `failed`
- `cancelled`

Runs support:

- cancellation
- bounded retries
- no-progress detection
- persisted final summaries

Sabio does not automatically revert workspace changes after a cancelled or failed run.

## 9. Memory And Resume

Sabio persists lightweight session memory.

Current memory behavior:

- completed runs append a bounded summary
- failed and cancelled runs also append bounded memory entries
- successful autonomous commands are remembered as preferred commands
- later plan generation and execution prompts include both memory and preferred commands

## 10. UI Requirements

Agent Mode currently includes:

- left sidebar for workspace and session selection
- center transcript for status, plans, events, and run summaries
- right panel for session controls, approvals, and command log

The UI must show:

- workspace trust status
- git status and branch
- plans and per-step status
- approvals and approval context
- command activity
- recent commits
- session memory
- final run outcomes and summaries

## 11. Persistence And Transparency

Sabio persists structured agent state in the backend and lightweight UI state in the browser.

The backend retains:

- session metadata
- plans
- approvals
- event logs with caps
- memory summaries

The UI currently uses persisted event logs plus polling-based refresh for run state and session data.

## 12. Safety Constraints

Current safety boundaries:

- trusted workspace required
- clean git worktree required before autonomous execution
- no shell execution in autonomous commands
- no path escape outside workspace root
- approval required for network/destructive commands
- backend validation for all model-produced tool calls

## 13. Non-Goals For This MVP

Not currently part of the shipped Agent Mode MVP:

- containerized sandboxing
- automatic branch creation
- automatic revert/rollback
- remote workspace execution
- multi-user collaboration
- full IDE editor features
