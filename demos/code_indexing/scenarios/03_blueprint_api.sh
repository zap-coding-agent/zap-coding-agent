#!/usr/bin/env bash
# Scenario 3 — API surface: Blueprint public methods.
#
# Demonstrates: code_map returning structured symbol list without reading the
# full file. A ~600-line class reduced to a method outline in one tool call.
# Without index: read entire blueprints.py (~600 lines), burn tokens on internals.
# With index:    code_map returns only public method names + line numbers.
set -euo pipefail

DEMO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$DEMO_DIR/flask"

echo "Prompt: List all public methods of the Blueprint class with a one-line"
echo "        description of each. Use code_map for a structured view."
echo ""

printf '{"type":"user","text":"List all public methods of the Blueprint class with a one-line description of each. Use code_map to get a structured view without reading the whole file."}\n{"type":"quit"}\n' \
    | zap --sdk --auto 2>/dev/null
