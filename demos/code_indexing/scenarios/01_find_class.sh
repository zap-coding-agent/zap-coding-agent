#!/usr/bin/env bash
# Scenario 1 — Symbol lookup: find the Flask class.
#
# Demonstrates: find_definition + read_file on targeted line range.
# Without index: LLM would list_directory → read 3-4 files hunting for "class Flask".
# With index:    find_definition("Flask") → exact file:line → read 20 lines. Done.
set -euo pipefail

DEMO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$DEMO_DIR/flask"

echo "Prompt: Where is the Flask class defined? Give me the exact file and line"
echo "        number, and list its base classes."
echo ""

printf '{"type":"user","text":"Where is the Flask class defined? Give me the exact file and line number, and list its base classes."}\n{"type":"quit"}\n' \
    | zap --sdk --auto 2>/dev/null
