#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

BINARY_NAME="sabio-server"

if [[ "${OS:-}" == "Windows_NT" ]]; then
  BINARY_NAME="sabio-server.exe"
fi

echo "Cleaning dist..."
rm -rf dist

echo "Building frontend and Rust backend..."
npm run check
npm run build

echo "Staging release folder..."
cp "server/target/release/$BINARY_NAME" "dist/$BINARY_NAME"
cp VERSION dist/VERSION
cp docs/README.md dist/README.md
cp -R assets dist/assets
perl -0pi -e 's#\.\./assets/#assets/#g' dist/README.md

cat > dist/README-RUN.txt <<EOF
Sabio local distribution

Version: $(tr -d '[:space:]' < VERSION)

Run:
  ./$BINARY_NAME

The Sabio backend will:
  1. Check whether Ollama is available at http://127.0.0.1:11434.
  2. Attempt to launch 'ollama serve' if Ollama is not already running.
  3. Start Sabio at http://127.0.0.1:3000.
  4. Open the default browser to the Sabio UI.

Requirements:
  - A local Ollama installation.
  - At least one installed Ollama model, for example:
      ollama pull llama3.2

The executable expects the built frontend at ./client relative to this folder.
EOF

chmod +x "dist/$BINARY_NAME"

echo
echo "Build complete: dist/$BINARY_NAME"
echo "Run it with:"
echo "  cd dist && ./$BINARY_NAME"
