#!/usr/bin/env bash
# T03 — /index: tree-sitter indexes the project and logs per-file entries.
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

TMP=$(make_project)
trap "rm -rf $TMP" EXIT

info "T03a: /index runs without crashing"
OUT=$(cd "$TMP" && zap_repl "$TMP" $'/index\n/exit')
if echo "$OUT" | grep -q "panicked at"; then
    fail "T03a /index no crash" "panic detected"
else
    pass "T03a /index no crash"
fi

info "T03b: /index shows tree-sitter scan line"
if echo "$OUT" | grep -qiE "tree-sitter.*scan|tree-sitter.*indexed|tree-sitter scanning"; then
    pass "T03b tree-sitter scan message present"
else
    fail "T03b tree-sitter scan message present" "got: $(echo "$OUT" | grep -i "tree\|index" | head -3)"
fi

info "T03c: /index creates .zap/code.db"
if [ -f "$TMP/.zap/code.db" ]; then
    pass "T03c .zap/code.db created"
else
    fail "T03c .zap/code.db created" ".zap/code.db not found"
fi

info "T03d: INDEX entries in zap.log"
LOGFILE="$HOME/.zap/zap.log"
if [ -f "$LOGFILE" ] && grep -q "INDEX tree-sitter" "$LOGFILE"; then
    pass "T03d INDEX log entries written to zap.log"
else
    fail "T03d INDEX log entries written to zap.log" "no INDEX entries in $LOGFILE"
fi

info "T03e: /index stats shows symbol count"
OUT2=$(cd "$TMP" && zap_repl "$TMP" $'/index stats\n/exit')
if echo "$OUT2" | grep -qiE "symbol|file.*indexed|tree-sitter index"; then
    pass "T03e /index stats shows symbol info"
else
    fail "T03e /index stats shows symbol info" "got: $(echo "$OUT2" | grep -i "index\|symbol" | head -3)"
fi

summary
