# Sabio

![Sabio Logo](assets/Sabio_logo.png)

**Version:** V1.2.0

Sabio is a local-first Ollama workspace with:

- a React three-pane UI
- a Rust backend proxy
- IndexedDB session persistence
- file upload and raw-text extraction
- Markdown-first assistant responses
- inline Thinking status while the local engine generates
- local display preferences for dark/light theme and font scaling

## Run

1. Install dependencies:

```bash
npm install
```

2. Start the app:

```bash
npm start
```

`npm start` runs the local server on `http://127.0.0.1:3000`. In development it serves the Vite frontend through middleware; after `npm run build` it serves the production bundle from `dist/client/`.

Starting Sabio also checks whether Ollama is responding at `http://127.0.0.1:11434`. If Ollama is not up, the Rust backend attempts to launch `ollama serve` before starting the Sabio server.

## Install From A Release Archive

Each tagged GitHub release includes platform-labeled distribution archives:

- `sabio-linux-x64-<version>.tar.gz`
- `sabio-macos-arm64-<version>.tar.gz`
- `sabio-windows-x64-<version>.zip`

To install from one of these archives:

1. Download the archive for your operating system from the GitHub release page.
2. Extract the archive to a local folder.
3. Install Ollama separately. Sabio expects Ollama at `http://127.0.0.1:11434` by default and will attempt to launch `ollama serve` if it is not already running.
4. Install at least one Ollama model, for example:

```bash
ollama pull llama3.2
```

5. Start Sabio from the extracted folder:

```bash
./sabio-server
```

On Windows, run:

```powershell
.

sabio-server.exe
```

6. Open the local app in your browser if it does not open automatically:

```text
http://127.0.0.1:3000
```

The release archives contain the production frontend build and a platform-specific Rust backend binary. They do not require Node.js for normal use, but they still require a local Ollama installation.

## Build

```bash
npm run build
```

To create a clean runnable distribution folder with the Rust executable and built frontend assets:

```bash
./build.sh
```

This recreates `dist/` and stages:

- `dist/sabio-server` or `dist/sabio-server.exe`
- `dist/client/`
- `dist/assets/`
- `dist/VERSION`
- `dist/README.md`
- `dist/README-RUN.txt`

## Requirements

- Node.js 20+
- Rust 1.94+ for source builds
- A local Ollama instance running on `http://127.0.0.1:11434`