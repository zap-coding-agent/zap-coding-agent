#!/usr/bin/env bash
# T02 — Tool use: list_directory and read_file work correctly.
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

TMP=$(make_project)
trap "rm -rf $TMP" EXIT

info "T02a: list_directory — agent can list src/"
OUT=$(cd "$TMP" && zap_run "$TMP" "Use the list_directory tool to list the src/ directory and tell me the filenames.")
if echo "$OUT" | grep -qi "main.rs"; then
    pass "T02a list_directory sees main.rs"
else
    fail "T02a list_directory sees main.rs" "got: $(echo "$OUT" | tail -5)"
fi

info "T02b: read_file — agent can read src/main.rs"
OUT=$(cd "$TMP" && zap_run "$TMP" "Read src/main.rs and tell me what the add function does.")
if echo "$OUT" | grep -qiE "add|sum|plus|addition"; then
    pass "T02b read_file understands add fn"
else
    fail "T02b read_file understands add fn" "got: $(echo "$OUT" | tail -5)"
fi

info "T02c: shell tool — agent can run a shell command"
OUT=$(cd "$TMP" && zap_run "$TMP" "Run the shell command 'echo hello_zap_test' and tell me the output.")
if echo "$OUT" | grep -q "hello_zap_test"; then
    pass "T02c shell tool returns output"
else
    fail "T02c shell tool returns output" "got: $(echo "$OUT" | tail -5)"
fi

summary
