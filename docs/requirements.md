Below is a **clean, structured `requirements.md` document** suitable for consumption by tools like Codex. It follows standard SRS-style organization (purpose, scope, functional + non-functional requirements, etc.) as recommended in common requirements documentation practices ([Asana][1]).

---

# Sabio — Requirements Specification (v1)

> Historical note:
> This document describes the original chat-first Sabio scope.
> Current Agent Mode behavior is documented in [agent_requirements.md](agent_requirements.md).

## 1. Overview

### 1.1 Purpose

Sabio is a **local-first web application** that provides an interactive interface for communicating with a locally running Ollama model. The application enables users to upload files, incorporate their contents into prompts, and generate structured Markdown outputs.

### 1.2 Scope

Sabio is designed for:

* Local execution only (no cloud dependency)
* Integration with a locally running Ollama instance
* File-driven workflows with LLM-assisted generation
* Markdown-first output for easy reuse

---

## 2. System Architecture

### 2.1 High-Level Architecture

* **Frontend:** React (bundled)
* **Backend:** Node.js (local proxy server)
* **Model Provider:** Ollama
* **Storage:** IndexedDB (browser)

### 2.2 Runtime Model

* Single command startup (e.g., `npm start`)
* Backend serves frontend
* Cross-platform (Windows, macOS, Linux)

### 2.3 Constraints

* No authentication
* Localhost-only usage assumed
* Single active generation at a time

---

## 3. Functional Requirements

### 3.1 Chat Interface

#### FR-1: Multi-line Input

* Input supports multi-line typing
* `Enter` does NOT submit
* Submit via:

  * `Ctrl + Shift + Enter`
  * Send button

#### FR-2: Streaming Responses

* Display streaming response in a temporary chat bubble
* Replace temporary bubble with final response upon completion

#### FR-3: Cancel Generation

* User can cancel in-progress generation
* Streaming stops immediately
* No final response is committed after cancellation

#### FR-4: Conversation History

* Maintain full conversation history
* Include history in each prompt
* Allow manual reset

#### FR-5: Edit and Re-run

* User can edit any prior user message
* System truncates subsequent messages
* Regenerates response from edited point

---

### 3.2 File Handling

#### FR-6: File Upload

Supported formats:

* `.txt`, `.md`, `.pdf`, `.docx`, `.csv`, `.json`
* Source code files (e.g., `.js`, `.ts`, `.py`)

#### FR-7: File Parsing

* Convert all files to **raw text only**
* No formatting preservation
* Fallback to plain-text parsing if needed
* Reject if parsing fails

#### FR-8: File Storage

* Store full parsed text locally using IndexedDB
* Persist across session reloads

#### FR-9: File Selection

* Checkbox-based selection
* Multiple files allowed
* Order determined by upload timestamp

#### FR-10: File Metadata Display

Each file displays:

* Name
* Type
* Size
* Upload timestamp

#### FR-11: Large File Handling

* Warn user when file is large
* Allow inclusion without restriction

---

### 3.3 Prompt Construction

#### FR-12: Structured Prompt Template

Each request includes:

1. System instructions
2. Conversation history
3. Selected file contents
4. Current user input

#### FR-13: File Context Formatting

Each file must be delimited:

```
--- FILE: filename ---
<content>
--- END FILE ---
```

#### FR-14: System Prompt

* Editable by user
* Persisted locally
* Reset-to-default supported

---

### 3.4 Model Integration

#### FR-15: Model Discovery

* Query available models from Ollama

#### FR-16: Model Selection

* Dropdown UI
* Persist selected model

#### FR-17: Request Handling

* Backend proxies all requests to Ollama
* Streaming responses supported

---

### 3.5 Output Handling

#### FR-18: Markdown Rendering

* Render responses as Markdown

#### FR-19: Copy Functionality

* Single copy button per response
* Copies raw Markdown

#### FR-20: Code Blocks

* Syntax highlighting
* Copy button per code block

#### FR-21: Download

* Allow download of response as `.md`

---

### 3.6 Session Management

#### FR-22: Persistent Session

* Single session
* Stored locally
* Restored on reload

#### FR-23: Input Persistence

* Preserve unsent input across reload

---

### 3.7 Error Handling

#### FR-24: User-Friendly Errors

* Display clear error messages

#### FR-25: Retry Mechanism

* Allow retry of failed requests

#### FR-26: Diagnostics

* Provide basic hints (e.g., Ollama not running)

---

## 4. User Interface Requirements

### 4.1 Layout

* Three-pane layout:

  * Left: File list
  * Center: Chat (widest)
  * Right: Settings
* Panes are **resizable**
* Pane widths persist

### 4.2 Theme

* Dark theme inspired by VS Code

### 4.3 File Selection UI

* Checkbox list

### 4.4 Settings Panel

Contains:

* Model selector
* System prompt editor

---

## 5. Non-Functional Requirements

### 5.1 Performance

* Must handle large text inputs with warnings
* Streaming should provide responsive feedback

### 5.2 Reliability

* Prevent concurrent request conflicts
* Ensure consistent state after cancel/edit

### 5.3 Usability

* Minimal friction UI
* Markdown-first workflow
* Clear feedback during operations

### 5.4 Portability

* Must run on:

  * Windows
  * macOS
  * Linux

### 5.5 Maintainability

* Clear separation of frontend/backend
* Modular API endpoints

---

## 6. API (Backend Endpoints)

### 6.1 `/api/models`

* GET available models

### 6.2 `/api/chat`

* POST prompt
* Stream response

### 6.3 `/api/upload`

* Handle file parsing

---

## 7. Constraints & Assumptions

* Ollama must be installed and running locally
* No authentication required
* Single-user environment assumed
* No cloud dependencies

---

## 8. Future Considerations (Out of Scope for v1)

* Multi-session management
* File preview/editing
* Chunking or embeddings
* Advanced prompt inspection/debugging
* Multi-user support
* Preset system prompts

---

## 9. Acceptance Criteria (High-Level)

* User can upload files and include them in prompts
* User receives streamed responses from Ollama
* Responses are rendered and downloadable as Markdown
* Session persists across reload
* System behaves correctly under edit/re-run scenarios
* App runs locally with a single startup command

---
