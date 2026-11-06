#!/usr/bin/env bash
# Build the frontend (if Node is installed) and start the server.
set -e
cd "$(dirname "$0")"
if command -v npm >/dev/null 2>&1; then
  (cd web && { [ -d node_modules ] || npm install --no-audit --no-fund; } && npm run build)
fi
exec cargo run --release
