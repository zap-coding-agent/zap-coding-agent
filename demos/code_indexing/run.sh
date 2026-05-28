#!/usr/bin/env bash
# Run all three code-indexing demo scenarios against the Flask repo.
set -euo pipefail

DEMO_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$DEMO_DIR/flask"

if [ ! -d "$REPO_DIR/.git" ]; then
    echo "Flask repo not found — run ./setup.sh first"
    exit 1
fi

cd "$REPO_DIR"

run_scenario() {
    local num="$1"
    local title="$2"
    local prompt="$3"

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  Scenario $num: $title"
    echo "  Prompt: $prompt"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    local start_ts
    start_ts=$(date +%s)

    # Capture full stdout (tool boxes + JSON response line).
    local raw
    raw=$(printf '{"type":"user","text":"%s"}\n{"type":"quit"}\n' "$prompt" \
        | zap --sdk --auto 2>/dev/null)

    local elapsed=$(( $(date +%s) - start_ts ))

    # ── Tool call trace ───────────────────────────────────────────────────────
    echo ""
    echo "[tool calls]"
    echo "$raw" | grep --color=never "╭─" | sed 's/^/  /' || true

    # ── Assistant response ────────────────────────────────────────────────────
    echo ""
    echo "[response]"
    echo "$raw" | python3 -c "
import sys, json
for line in sys.stdin:
    line = line.strip()
    if not line.startswith('{'): continue
    try:
        obj = json.loads(line)
    except json.JSONDecodeError:
        continue
    if obj.get('type') == 'assistant':
        print(obj['text'])
" | fold -s -w 80 | sed 's/^/  /'

    # ── Stats ─────────────────────────────────────────────────────────────────
    echo ""
    echo "[stats]"
    local tool_calls
    tool_calls=$(echo "$raw" | grep -c "╭─" 2>/dev/null || echo "?")
    echo "$raw" | python3 -c "
import sys, json
for line in sys.stdin:
    line = line.strip()
    if not line.startswith('{'): continue
    try:
        obj = json.loads(line)
    except json.JSONDecodeError:
        continue
    if obj.get('type') == 'assistant':
        u = obj.get('usage', {})
        inp = u.get('input_tokens', '?')
        out = u.get('output_tokens', '?')
        print(f'  Input: {inp:>6} tokens   Output: {out:>5} tokens')
        break
" 2>/dev/null
    echo "  Tool calls: ${tool_calls}   Wall time: ${elapsed}s"
}

# ── Scenario 1: Find a class ──────────────────────────────────────────────────
run_scenario "1" "Symbol lookup — find the Flask class" \
    "Where is the Flask class defined? Give me the exact file and line number, and list its base classes."

# ── Scenario 2: Trace a request ──────────────────────────────────────────────
run_scenario "2" "Cross-file trace — request to route handler" \
    "Trace how an incoming HTTP request flows from the WSGI entry point to the route handler function. Show the key method calls and which files they are in."

# ── Scenario 3: Blueprint API surface ────────────────────────────────────────
run_scenario "3" "API surface — Blueprint public methods" \
    "List all public methods of the Blueprint class with a one-line description of each. Use code_map to get a structured view without reading the whole file."

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  All scenarios complete."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
