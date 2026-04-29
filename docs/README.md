# Sabio Docs

![Sabio logo](../assets/Sabio_logo.png)

This folder contains the current product docs plus the original chat-mode planning docs.

## Current Docs

- [agent_requirements.md](agent_requirements.md)
  Current source of truth for Sabio Agent Mode behavior and constraints.

- [agent_implementation_plan.md](agent_implementation_plan.md)
  Delivery summary for the completed Agent Mode MVP on `feature/agent-mode`.

## Legacy v1 Docs

- [requirements.md](requirements.md)
  Original requirements for the chat-first Sabio build before Agent Mode.

- [implementation_plan.md](implementation_plan.md)
  Original build plan for the chat-first Sabio implementation.

## Local Development

### Run

```bash
npm ci --include=dev
npm start
```

Sabio serves the app at `http://127.0.0.1:3000`.

The Rust backend expects Ollama at `http://127.0.0.1:11434`. If Ollama is not already running, Sabio attempts to launch `ollama serve` before starting the local web app.

### Build

```bash
npm run build
```

### Package A Runnable Distribution

```bash
./build.sh
```

## Recommended Local Models

Sabio works best in Agent Mode with models that reliably follow structured JSON. A small local default that has worked well during development is:

```bash
ollama pull qwen2.5-coder:3b
```

## Agent Mode Workflow

The intended loop is:

`task -> plan -> approval -> edit -> command/test -> commit -> summary`

Before autonomous execution:

- the workspace must be validated
- the workspace must be explicitly trusted
- the workspace must be a clean git repository

If a folder is not already a git repository, Sabio can initialize one from the UI.
