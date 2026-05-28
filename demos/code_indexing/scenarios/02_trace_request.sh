#!/usr/bin/env bash
# Scenario 2 — Cross-file trace: HTTP request to route handler.
#
# Demonstrates: code index enabling accurate multi-file navigation.
# Without index: LLM guesses file names, reads full files, loses context.
# With index:    wsgi_app → full_dispatch_request → dispatch_request, each
#               resolved to exact lines across src/flask/app.py and wrappers.
set -euo pipefail

DEMO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$DEMO_DIR/flask"

echo "Prompt: Trace how an incoming HTTP request flows from the WSGI entry"
echo "        point to the route handler. Show key method calls and file names."
echo ""

printf '{"type":"user","text":"Trace how an incoming HTTP request flows from the WSGI entry point to the route handler function. Show the key method calls and which files they are in."}\n{"type":"quit"}\n' \
    | zap --sdk --auto 2>/dev/null
