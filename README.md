# Sabio

![Sabio Logo](assets/Sabio_logo.png)

Sabio is a local-first Ollama workspace with two modes:

- `Chat Mode` for file-aware local chat
- `Agent Mode` for plan-first coding workflows inside a trusted git workspace

The app uses a React frontend and a Rust backend. Ollama stays local.

## What Sabio Can Do

- list local Ollama models and chat with them
- upload files and fold their contents into prompts
- switch into Agent Mode for workspace-aware coding tasks
- validate and trust a local workspace before autonomous execution
- generate reviewed plans, pause for approval, then execute approved steps
- read files, search code, write patches/files, run safe commands, and commit progress
- remember session summaries and preferred autonomous commands per workspace

## Agent Mode Highlights

Agent Mode is built around a deliberate flow:

`task -> plan -> approval -> edit -> command/test -> commit -> summary`

Current agent behavior includes:

- clean-worktree gate before autonomous execution
- optional git initialization for non-repo folders
- per-step commits using `sabio(agent): <step description>`
- approval pauses for network/destructive commands
- cancellation
- retry and no-progress handling
- persisted sessions, plans, approvals, event logs, memory, and command preferences

## Quick Start

### Prerequisites

- Node.js 20+
- npm
- Rust 1.94+
- Git
- Ollama running locally at `http://127.0.0.1:11434`
- at least one Ollama model installed

Example:

```bash
ollama pull qwen2.5-coder:3b
```

### Run From Source

```bash
npm ci --include=dev
npm start
```

Sabio runs at `http://127.0.0.1:3000`.

### Build

```bash
npm run build
```

To assemble a runnable distribution folder:

```bash
./build.sh
```

## Using Agent Mode

1. Switch from `Chat` to `Agent`.
2. Enter a workspace path and click `Validate`.
3. If needed, click `Initialize git`.
4. Trust the workspace once it is a clean git repository.
5. Create or select an agent session.
6. Enter a task and click `Generate plan`.
7. Approve the generated plan.
8. Click `Run agent` or `Resume agent`.

When the agent needs approval for a command, Sabio pauses the run, shows the blocked plan/step, and lets you approve or reject the command before resuming.

## Safety Model

Sabio is local-first, but Agent Mode is still intentionally constrained:

- workspace trust is required
- autonomous execution requires a clean git worktree
- command execution uses executable-plus-args, not shell strings
- commands are classified as autonomous, approval-required, or blocked
- file and command activity is logged in the session event history
- Sabio does not auto-revert changes or auto-create branches

## Docs

- [docs/README.md](docs/README.md) - doc index and local usage notes
- [docs/agent_requirements.md](docs/agent_requirements.md) - current Agent Mode scope
- [docs/agent_implementation_plan.md](docs/agent_implementation_plan.md) - delivered MVP summary
- [docs/requirements.md](docs/requirements.md) - legacy chat-mode v1 requirements
- [docs/implementation_plan.md](docs/implementation_plan.md) - legacy chat-mode v1 build plan

## Releases

You can download packaged builds from the GitHub [releases page](https://github.com/jwkunz/Sabio/releases).
