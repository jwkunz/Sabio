# Sabio — Detailed Implementation Plan

> Historical note:
> This document is the original chat-first implementation plan.
> The delivered Agent Mode MVP is summarized in [agent_implementation_plan.md](agent_implementation_plan.md).

## Purpose
This document is a build-oriented implementation plan for Codex to construct **Sabio**, a local-first web application that provides a React-based chat interface for a locally running Ollama instance via a Node.js backend proxy.

This plan assumes the requirements defined in `requirements.md` are the source of truth. Where implementation details are not explicitly defined in the requirements, this document chooses pragmatic defaults that preserve the required behavior.

---

# 1. Delivery Objectives

Codex should build a production-ready v1 that satisfies the following top-level goals:

1. Run locally with a **single-command startup experience**.
2. Use a **React frontend** and **Node.js backend**.
3. Communicate with a **locally running Ollama** instance through the backend.
4. Persist a **single long-lived session** locally.
5. Support **file upload → local parsing → raw text extraction → prompt inclusion**.
6. Render assistant responses as **Markdown**, with code highlighting and copy support.
7. Support **streaming responses**, **cancel generation**, **edit and re-run**, and **hybrid context management**.
8. Present a **three-pane resizable layout** with a dark VS Code–like theme.
9. Include **`Sabio_logo.png`** as a visible hero graphic in the UI.
10. Maintain a clean Git history with **milestone commits**.

---

# 2. Recommended Technical Stack

## Frontend
- React
- TypeScript is optional, but **preferred** for maintainability. If TypeScript is used, keep strictness practical.
- Vite for development/build pipeline
- React Markdown renderer
- Syntax highlighting library for fenced code blocks
- Resizable pane library, or a custom splitter if necessary
- IndexedDB wrapper library for simpler persistence

## Backend
- Node.js
- Express or a similarly lightweight HTTP framework
- Multipart upload handling library
- File parsers for:
  - PDF
  - DOCX
  - CSV
  - JSON
  - plain text / markdown / source files

## Storage
- IndexedDB in the browser for:
  - session messages
  - parsed file contents
  - pane widths
  - selected model
  - system prompt
  - unsent input

## Packaging / Run Model
- Backend serves the built frontend
- One command to start the app in local use
- Frontend build should be aggressively bundled and compact
- The final frontend may be a single HTML entry with bundled assets if feasible, but the backend is still required and must serve it

---

# 3. Repository Structure

Codex should create a repository with a clear separation between frontend, backend, shared types, and assets.

```text
sabio/
  README.md
  requirements.md
  implementation_plan.md
  package.json
  .gitignore
  assets/
    Sabio_logo.png
  client/
    index.html
    src/
      main.tsx
      App.tsx
      components/
      hooks/
      lib/
      store/
      styles/
      types/
    public/
  server/
    index.ts
    routes/
    services/
    parsers/
    utils/
    types/
  dist/
```

If TypeScript is not used, the same structure should be preserved with `.js/.jsx` files.

---

# 4. Build Sequence Overview

Codex should implement the project in the following milestone order:

1. **Project scaffolding and local server bootstrap**
2. **Core layout and theme**
3. **Ollama connectivity and model listing**
4. **Chat request/response path with streaming**
5. **Markdown rendering and copy/download behavior**
6. **File upload and parsing pipeline**
7. **IndexedDB persistence layer**
8. **Conversation history, context assembly, and reset logic**
9. **Edit and re-run behavior**
10. **Cancellation and error handling**
11. **Hero graphic integration and UI polish**
12. **Production build, bundling, and cross-platform startup verification**

Each milestone should end with a **Git commit**.

---

# 5. Milestone-by-Milestone Instructions

## Milestone 1 — Project Scaffolding and Bootstrap

### Goals
- Initialize repository
- Configure package management
- Set up frontend and backend directories
- Create basic start scripts
- Ensure backend can serve a placeholder frontend page

### Tasks
1. Initialize Git repository.
2. Create root `package.json` with scripts such as:
   - `dev`
   - `build`
   - `start`
3. Add a backend server that:
   - starts on localhost
   - serves a placeholder HTML page or the frontend build output
4. Add `.gitignore` entries for:
   - `node_modules`
   - build output
   - local caches
   - OS/editor noise
5. Add `README.md` with setup notes.

### Definition of Done
- `npm install` works
- `npm start` starts the backend and serves a page locally
- Repository structure exists

### Required Git Commit
Use a milestone commit such as:

```bash
git add .
git commit -m "milestone: scaffold project and local server bootstrap"
```

---

## Milestone 2 — Core Layout and Theme

### Goals
- Implement the three-pane layout
- Make panes resizable
- Apply dark VS Code–like styling
- Make the chat pane widest by default

### Tasks
1. Build the main application shell with three columns:
   - left: file pane
   - center: chat pane
   - right: settings pane
2. Add draggable splitters between panes.
3. Persist pane widths locally.
4. Apply a dark theme inspired by VS Code:
   - dark background
   - muted borders
   - accessible text contrast
   - subtle hover/focus states
5. Ensure responsive behavior is reasonable on smaller screens, but desktop layout is primary.

### Definition of Done
- Three panes render correctly
- Panes resize interactively
- Reload preserves widths
- Chat pane is widest by default

### Required Git Commit
```bash
git add .
git commit -m "milestone: implement resizable three-pane shell and dark theme"
```

---

## Milestone 3 — Hero Graphic Integration (`Sabio_logo.png`)

### Goals
- Place `Sabio_logo.png` prominently as a hero graphic
- Make the branding visually integrated without reducing usability

### Required Asset Placement
Codex must assume the file is located at:

```text
assets/Sabio_logo.png
```

If the frontend build system requires public/static asset placement, Codex may copy or reference it appropriately during build, but the source asset should remain in `assets/`.

### UI Instructions
1. Render `Sabio_logo.png` near the top of the application.
2. Treat it as a **hero graphic / branded header element**, not merely a favicon.
3. Recommended placement:
   - top of the **center chat pane**, above the message list and below the app frame/header, or
   - top center spanning the upper content region, as long as it does not interfere with the pane layout
4. The logo should:
   - be clearly visible on the dark theme
   - preserve aspect ratio
   - scale responsively
   - not dominate vertical space excessively
5. Add accompanying title text:
   - `Sabio`
   - optional subtitle such as `Local Ollama Workspace`
6. Keep the presentation refined and restrained:
   - no gaudy animation
   - subtle spacing
   - crisp rendering on dark backgrounds

### Styling Guidance
- Use generous top padding
- Constrain max width/height so the logo does not consume too much space
- Align branding consistently with the app’s design language
- Ensure the logo looks intentional in both dev and production builds

### Definition of Done
- `Sabio_logo.png` appears in the UI as a hero graphic
- The image renders correctly in local dev and production build
- The layout still prioritizes chat usability

### Required Git Commit
```bash
git add .
git commit -m "milestone: add Sabio hero branding with logo graphic"
```

---

## Milestone 4 — Ollama Connectivity and Model Listing

### Goals
- Connect backend to Ollama
- Add frontend model dropdown

### Tasks
1. Implement backend endpoint:
   - `GET /api/models`
2. Proxy Ollama model listing endpoint.
3. Normalize response shape for frontend consumption.
4. In frontend settings pane:
   - fetch model list on load
   - render dropdown
   - persist selection locally
5. Handle failure states gracefully:
   - Ollama unavailable
   - empty model list

### Definition of Done
- Model dropdown populates from local Ollama
- Selected model persists on reload
- Clear user-facing errors appear if Ollama is unavailable

### Required Git Commit
```bash
git add .
git commit -m "milestone: add Ollama model discovery and selection"
```

---

## Milestone 5 — Chat Pipeline and Streaming Response

### Goals
- Build the chat flow end-to-end
- Support streaming temporary bubble and committed final response

### Tasks
1. Implement frontend chat message list and input area.
2. Input behavior:
   - multiline input
   - Enter inserts newline
   - `Ctrl+Shift+Enter` sends
   - visible Send button
3. Implement backend `POST /api/chat` endpoint.
4. Build streaming support from backend to frontend.
5. While stream is active:
   - show temporary assistant bubble
   - append chunks incrementally
6. When stream completes:
   - remove or replace temporary bubble
   - commit final assistant response as permanent history entry
7. Enforce single active generation.

### Definition of Done
- User can send a prompt
- Response streams visibly
- Final message replaces temporary bubble
- Only one request can be active at a time

### Required Git Commit
```bash
git add .
git commit -m "milestone: implement chat flow and streaming assistant responses"
```

---

## Milestone 6 — Markdown Rendering, Copy, and Download

### Goals
- Render assistant messages as Markdown
- Support copy and `.md` download
- Add code block copy support and syntax highlighting

### Tasks
1. Render committed assistant responses as Markdown.
2. Add syntax highlighting for fenced code blocks.
3. Add one copy button per assistant response:
   - copy raw Markdown
4. Add one copy button per code block.
5. Add `.md` download action for final responses.
6. Ensure copied/downloaded content is the canonical final Markdown, not rendered HTML.

### Definition of Done
- Markdown renders correctly
- Code blocks are highlighted
- Copy buttons work
- `.md` download works

### Required Git Commit
```bash
git add .
git commit -m "milestone: add markdown rendering, copy actions, and md export"
```

---

## Milestone 7 — File Upload and Parsing Pipeline

### Goals
- Allow file upload
- Parse supported file types into raw text
- Return metadata and extracted text to frontend

### Tasks
1. Implement upload UI in left pane.
2. Support file selection from local disk.
3. Create backend upload endpoint:
   - `POST /api/upload`
4. For each file:
   - detect type
   - parse to raw text
   - if type-specific parsing fails, try plain-text fallback
   - if fallback fails, reject with clear error
5. Capture metadata:
   - name
   - type
   - size
   - upload timestamp
6. Warn for large files, but allow them.

### Definition of Done
- User can upload supported files
- Parsed raw text is returned
- Metadata is visible in file list
- Failures produce clear errors

### Required Git Commit
```bash
git add .
git commit -m "milestone: implement file upload, parsing, and metadata capture"
```

---

## Milestone 8 — File List, Selection, and IndexedDB Persistence

### Goals
- Persist uploaded files and session state locally
- Support checkbox-based file selection

### Tasks
1. Create IndexedDB schema/store(s) for:
   - messages
   - uploaded files
   - selected model
   - system prompt
   - pane widths
   - unsent input
2. Store full parsed file text locally.
3. Render file list in left pane.
4. Each file item must show:
   - name
   - type
   - size
   - timestamp
5. Add checkbox selection for inclusion in the next prompt.
6. Permit multiple selection.
7. Keep deterministic file ordering by upload timestamp.

### Definition of Done
- Reload restores session state
- Uploaded files persist
- Checkbox selection works
- Multiple selected files are supported

### Required Git Commit
```bash
git add .
git commit -m "milestone: persist session and files with IndexedDB"
```

---

## Milestone 9 — Prompt Construction and Context Management

### Goals
- Assemble structured prompts correctly
- Implement hybrid context limit behavior
- Add reset context control

### Tasks
1. Create a shared prompt builder module.
2. Prompt structure must include:
   - editable system prompt
   - conversation history
   - selected file content
   - current user message
3. Delimit each selected file clearly:

```text
--- FILE: filename.ext ---
<raw text>
--- END FILE ---
```

4. Implement hybrid context management:
   - maintain session history
   - provide reset/clear context action
   - warn when context is growing large
   - truncate oldest history when necessary
5. Selected files for the current turn must take priority over older history when trimming context.
6. Use a practical approximation strategy if true token counting is not implemented initially.

### Definition of Done
- Prompts are consistently structured
- Selected files are injected correctly
- Reset works
- Context warnings/truncation function without breaking chat

### Required Git Commit
```bash
git add .
git commit -m "milestone: add structured prompt builder and context management"
```

---

## Milestone 10 — Editable System Prompt and Settings Pane

### Goals
- Build settings pane features
- Persist editable system prompt

### Tasks
1. In right pane, add:
   - model dropdown
   - system prompt editor
   - reset-to-default system prompt control
2. Persist system prompt locally.
3. Initialize a sensible default system prompt aligned with Markdown-first behavior.
4. Ensure edits are reflected in subsequent requests.

### Definition of Done
- User can edit system prompt
- Reset restores default
- Prompt persists across reload

### Required Git Commit
```bash
git add .
git commit -m "milestone: implement settings pane and editable system prompt"
```

---

## Milestone 11 — Conversation Persistence, Edit-and-Re-Run, and Input Restore

### Goals
- Persist messages and input
- Support editing prior user messages and regenerating from that point

### Tasks
1. Persist all message history locally.
2. Restore full history on reload.
3. Restore unsent input on reload.
4. Add edit action to user messages.
5. When user edits an earlier message and reruns:
   - truncate subsequent messages
   - rebuild prompt from retained history
   - submit regenerated request
6. For v1, re-run should use the **current file selections**, not historical per-message selections.

### Definition of Done
- History survives reload
- Editing prior message works
- Later conversation is discarded and replaced correctly
- No duplicate or orphaned assistant messages remain

### Required Git Commit
```bash
git add .
git commit -m "milestone: add persistent history and edit-and-rerun workflow"
```

---

## Milestone 12 — Cancellation, Retry, and Error Handling

### Goals
- Provide robust user-facing failure handling
- Support cancel and retry semantics cleanly

### Tasks
1. Add cancel button for active generation.
2. Use abort/cancellation semantics in frontend and backend.
3. If canceled:
   - stop stream
   - do not commit a final assistant message
4. Normalize backend error payloads.
5. Provide user-friendly messages for:
   - Ollama unavailable
   - timeout
   - model missing
   - malformed response
   - interrupted stream
6. Add retry action for failed requests.
7. Ensure retry does not corrupt state or duplicate messages.

### Definition of Done
- Cancel works reliably
- Failed requests show meaningful errors
- Retry works cleanly

### Required Git Commit
```bash
git add .
git commit -m "milestone: add cancellation, retry, and friendly error handling"
```

---

## Milestone 13 — UI Polish and Final Integration Pass

### Goals
- Make the app coherent, polished, and stable
- Ensure dark theme consistency and layout refinement

### Tasks
1. Refine spacing, typography, borders, and states.
2. Verify logo/header area remains balanced.
3. Verify file pane, chat pane, and settings pane are visually coherent.
4. Ensure auto-scroll behavior functions correctly.
5. Ensure copy/download controls are discoverable but not noisy.
6. Ensure the streaming bubble and final bubble transition feels polished.

### Definition of Done
- UI looks finished
- No obvious layout regressions
- Hero graphic integrates well with the rest of the page

### Required Git Commit
```bash
git add .
git commit -m "milestone: polish UI and finalize Sabio visual integration"
```

---

## Milestone 14 — Production Build and Cross-Platform Verification

### Goals
- Validate the production build
- Ensure single-command local startup works reliably

### Tasks
1. Configure production frontend build.
2. Ensure backend serves the built frontend assets.
3. Keep frontend output compact and optimized.
4. Verify startup flow is straightforward.
5. Test on major platforms where possible:
   - Windows
   - macOS
   - Linux
6. Confirm asset resolution for `Sabio_logo.png` in production.
7. Confirm build output and runtime do not depend on cloud services.

### Definition of Done
- `npm run build` succeeds
- `npm start` serves the production app locally
- App works end-to-end against local Ollama

### Required Git Commit
```bash
git add .
git commit -m "milestone: finalize production build and cross-platform local startup"
```

---

# 6. Detailed Implementation Notes

## 6.1 Prompt Builder Rules
Codex must implement prompt assembly as a centralized function. Do not scatter prompt formatting logic across components.

### Required Order
1. System prompt
2. Conversation history
3. Selected file contents
4. Current user input

### Required Behavior
- Include only currently selected files
- File inclusion must be deterministic by upload timestamp
- History truncation must prefer retaining:
  - recent messages
  - selected file text for the current turn
- Oldest history should be dropped first when over soft limit

---

## 6.2 Context Limit Strategy
True token counting may be deferred if necessary, but Codex must still implement a practical context management layer.

### Acceptable v1 Strategy
- Estimate size using characters or rough token approximation
- Define soft and hard thresholds
- Show warning when nearing soft threshold
- Drop oldest history when threshold is exceeded

### Do Not
- Silently drop selected file context before old history unless unavoidable
- Let unbounded context growth degrade the app indefinitely

---

## 6.3 Streaming Semantics
Codex should separate:
- **ephemeral streaming state**
- **committed conversation history**

### Rules
- Temporary assistant bubble exists only while a request is active
- Final assistant message is committed only on successful completion
- Cancellation or error must not produce a fake completed answer

---

## 6.4 Edit-and-Re-Run Semantics
When editing an earlier user message:
1. Replace the edited user message content
2. Remove all later messages
3. Reconstruct prompt from remaining history
4. Re-run generation

This behavior must be deterministic and not fork history.

---

## 6.5 IndexedDB Persistence Model
At minimum, store:
- session metadata
- messages
- files
- pane widths
- selected model
- system prompt
- unsent input

Suggested logical shapes:

```text
session:
  id
  selectedModel
  systemPrompt
  draftInput
  paneWidths

messages:
  id
  role
  content
  createdAt

files:
  id
  name
  type
  size
  uploadedAt
  rawText
  isSelected
```

Exact schema may vary.

---

# 7. UI/UX Implementation Guidance

## 7.1 File Pane (Left)
Include:
- upload control
- file list
- file metadata
- inclusion checkboxes
- large file warnings where applicable

Do not add extra file preview/edit features in v1.

## 7.2 Chat Pane (Center)
Include:
- hero graphic with Sabio branding
- chat transcript
- temporary streaming bubble
- final assistant messages
- user messages with edit action
- multiline composer
- send button
- cancel button when generating

This pane must be the primary visual focus.

## 7.3 Settings Pane (Right)
Include:
- model selector
- system prompt editor
- reset system prompt action
- reset/clear context control

Avoid overloading this pane with debugging controls in v1.

---

# 8. Git Discipline Requirements

Codex must make Git commits at the end of each milestone. The repository history should clearly reflect progress.

## Commit Rules
1. Commit after each milestone reaches its definition of done.
2. Use descriptive commit messages prefixed with `milestone:`.
3. Avoid one giant final commit.
4. Avoid noisy micro-commits unless needed for recovery.
5. If a milestone spans several hours of implementation, intermediate checkpoint commits are acceptable, but milestone commit messages must still exist.

## Required Pattern
Examples:

```bash
git commit -m "milestone: scaffold project and local server bootstrap"
git commit -m "milestone: implement resizable three-pane shell and dark theme"
git commit -m "milestone: add Ollama model discovery and selection"
git commit -m "milestone: implement chat flow and streaming assistant responses"
```

## Final Integration Commit
After all milestone work and final QA:

```bash
git add .
git commit -m "milestone: complete Sabio v1 implementation"
```

---

# 9. Testing and Validation Checklist

Codex should validate the following before considering the implementation complete.

## Startup / Runtime
- App starts locally with one command
- Backend serves frontend successfully
- Ollama connection works when local service is running

## Layout
- Three panes render
- Panes resize correctly
- Widths persist
- Chat pane is widest
- Logo renders correctly

## File Handling
- Upload supported file types
- Parse to raw text
- Persist in IndexedDB
- Checkbox selection works
- Multiple files can be selected

## Chat
- Multiline input works
- `Ctrl+Shift+Enter` submits
- Send button works
- Streaming bubble appears
- Final response replaces temporary bubble
- Cancel works
- Retry works

## Output
- Markdown renders correctly
- Code highlighting works
- Copy response works
- Copy code block works
- `.md` download works

## Persistence
- History restores
- Files restore
- Selected model restores
- System prompt restores
- Draft input restores

## Context / Editing
- Reset clears context
- Soft-limit warnings appear
- Old history truncates when necessary
- Edit-and-rerun truncates forward history and regenerates correctly

## Production
- Production build succeeds
- Served production app works locally
- Logo path works in production

---

# 10. Final Instruction to Codex

Build Sabio incrementally using the milestone order in this document. Prioritize correctness of state management, streaming behavior, file persistence, and prompt assembly over cosmetic embellishment.

Use the requirements as the contract, and use this implementation plan as the execution guide.

Do not skip milestone commits.

Ensure `Sabio_logo.png` is integrated as a hero graphic in the UI, not merely stored as an unused asset.
