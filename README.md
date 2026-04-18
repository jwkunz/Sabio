# Sabio

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

## Build

```bash
npm run build
```

## Requirements

- Node.js 18+
- A local Ollama instance running on `http://127.0.0.1:11434`
