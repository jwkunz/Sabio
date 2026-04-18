# Sabio

![Sabio logo](assets/Sabio_logo.png)

**Version:** V1.0.0

Sabio is a local-first Ollama workspace with:

- a React three-pane UI
- a Node/Express backend proxy
- IndexedDB session persistence
- file upload and raw-text extraction
- Markdown-first assistant responses

## Run

1. Install dependencies:

```bash
npm install
```

2. Start the app:

```bash
npm start
```

`npm start` runs the local server on `http://127.0.0.1:3000`. In development it serves the Vite frontend through middleware; after `npm run build` it serves the production bundle from `dist/client`.

## Install From A Release Archive

Each tagged GitHub release includes platform-labeled distribution archives:

- `sabio-linux-x64-<version>.tar.gz`
- `sabio-macos-arm64-<version>.tar.gz`
- `sabio-windows-x64-<version>.zip`

To install from one of these archives:

1. Download the archive for your operating system from the GitHub release page.
2. Extract the archive to a local folder.
3. Install Node.js 20 or newer if it is not already installed.
4. Install dependencies from inside the extracted Sabio folder:

```bash
npm ci
```

5. Install and start Ollama separately. Sabio expects Ollama at `http://127.0.0.1:11434` by default.
6. Install at least one Ollama model, for example:

```bash
ollama pull llama3.2
```

7. Start Sabio:

```bash
npm start
```

8. Open the local app in your browser:

```text
http://127.0.0.1:3000
```

The release archives contain the production frontend build and the local Node backend. They are distributable application bundles, but they still require Node.js and a locally running Ollama installation.

## Build

```bash
npm run build
```

## Requirements

- Node.js 18+
- A local Ollama instance running on `http://127.0.0.1:11434`
