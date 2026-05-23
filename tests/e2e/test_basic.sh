#!/usr/bin/env bash
# T01 — Basic response: zap answers a simple question without crashing.
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

TMP=$(make_project)
trap "rm -rf $TMP" EXIT

info "T01a: single-shot goal, expect a numeric answer"
OUT=$(cd "$TMP" && zap_run "$TMP" "What is 7 times 6? Reply with just the number.")
if echo "$OUT" | grep -qE '\b42\b'; then
    pass "T01a basic arithmetic answer"
else
    fail "T01a basic arithmetic answer" "got: $(echo "$OUT" | tail -3)"
fi

info "T01b: no panic / no 'panicked at' in output"
if echo "$OUT" | grep -q "panicked at"; then
    fail "T01b no panic" "panic detected in output"
else
    pass "T01b no panic"
fi

summary
