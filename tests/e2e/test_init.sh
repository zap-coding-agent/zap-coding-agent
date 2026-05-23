#!/usr/bin/env bash
# T04 — /init: creates project.json, ZAP.md; tested in both CLI and TUI modes.
# /init uses inquire interactive prompts.  CLI mode can be driven via stdin pipe.
# TUI mode suspends the raw terminal for /init prompts, so we need a PTY (script).
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

# ── CLI mode ─────────────────────────────────────────────────────────────────
TMP=$(make_project)
trap "rm -rf $TMP" EXIT

info "T04-cli: /init via --cli mode with stdin-piped answers"
# Prompt sequence: project-name → language(s) → index? → generate understanding?
OUT=$(cd "$TMP" && printf 'hello\nrust\ny\ny\n/exit\n' | timeout "$TIMEOUT" "$ZAP" --cli 2>&1) || true

info "T04a: project.json written (CLI)"
if [ -f "$TMP/.zap/project.json" ]; then
    pass "T04a .zap/project.json created (CLI)"
    info "  $(cat "$TMP/.zap/project.json")"
else
    fail "T04a .zap/project.json created (CLI)" "not found — output: $(echo "$OUT" | tail -5)"
fi

info "T04b: ZAP.md written (CLI)"
if [ -f "$TMP/ZAP.md" ]; then
    pass "T04b ZAP.md created (CLI)"
else
    fail "T04b ZAP.md created (CLI)" "not found"
fi

info "T04c: project.json has required fields"
if [ -f "$TMP/.zap/project.json" ]; then
    JSON=$(cat "$TMP/.zap/project.json")
    if echo "$JSON" | grep -q '"language"' && echo "$JSON" | grep -q '"initialized_at"'; then
        pass "T04c project.json has language + initialized_at"
    else
        fail "T04c project.json has language + initialized_at" "got: $JSON"
    fi
fi

info "T04d: second run skips /init nudge once project is known"
OUT2=$(cd "$TMP" && zap_run "$TMP" "what is 1+1" 2>&1) || true
if echo "$OUT2" | grep -q "Run /init"; then
    fail "T04d no /init nudge after init" "nudge still shown"
else
    pass "T04d no /init nudge after init"
fi

# ── TUI mode ─────────────────────────────────────────────────────────────────
TMP2=$(make_project)
trap "rm -rf $TMP $TMP2" EXIT

if ! command -v expect &>/dev/null; then
    info "T04-tui: 'expect' not found — skipping TUI /init test"
    pass "T04-tui skipped (install expect: brew install expect)"
    summary; exit 0
fi

info "T04-tui: /init via TUI mode (PTY via expect)"
CAPTURE="$TMP2/tui_init.txt"
(cd "$TMP2" && expect -f - >"$CAPTURE" 2>&1 <<EXPECT
set timeout 40
spawn $ZAP
# Confirm mode selector (Enter = Vibe default)
sleep 2
send "\r"
# TUI is running; send /init
sleep 2
send "/init\r"
# Answer prompts: project name, language, index?, understanding?
sleep 1; send "myproj\r"
sleep 1; send "rust\r"
sleep 1; send "y\r"
sleep 1; send "y\r"
sleep 8
send "/exit\r"
expect eof
EXPECT
) || true

info "T04e: project.json written (TUI)"
if [ -f "$TMP2/.zap/project.json" ]; then
    pass "T04e .zap/project.json created (TUI)"
    info "  $(cat "$TMP2/.zap/project.json")"
else
    fail "T04e .zap/project.json created (TUI)" "not found; capture: $(tail -10 "$CAPTURE" 2>/dev/null | cat -v)"
fi

info "T04f: ZAP.md written (TUI)"
if [ -f "$TMP2/ZAP.md" ]; then
    pass "T04f ZAP.md created (TUI)"
else
    fail "T04f ZAP.md created (TUI)" "not found"
fi

summary
