#!/usr/bin/env bash
# Shared helpers for zap e2e tests.
set -euo pipefail

ZAP="${ZAP_BIN:-$HOME/.local/bin/zap}"
TIMEOUT="${ZAP_TEST_TIMEOUT:-60}"

# Colours
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'

PASS=0; FAIL=0

pass() { echo -e "${GREEN}PASS${NC} $1"; PASS=$((PASS+1)); }
fail() { echo -e "${RED}FAIL${NC} $1: $2"; FAIL=$((FAIL+1)); }
info() { echo -e "${YELLOW}    ${NC} $1"; }

# Run zap in single-shot CLI mode and capture output.
# Usage: zap_run <tmp_dir> <goal> [extra_args...]
zap_run() {
    local dir="$1" goal="$2"; shift 2
    timeout "$TIMEOUT" "$ZAP" --goal "$goal" --auto --cli "$@" 2>&1
}

# Send slash commands to zap's REPL via stdin.
# Usage: zap_repl <tmp_dir> <commands_string>
# commands_string: newline-separated, e.g. $'/index\n/exit'
zap_repl() {
    local dir="$1" cmds="$2"
    (cd "$dir" && printf '%s\n' "$cmds" | timeout "$TIMEOUT" "$ZAP" --cli 2>&1) || true
}

# Make a fresh isolated project dir.
make_project() {
    local tmp
    tmp=$(mktemp -d /tmp/zap_e2e_XXXXXX)
    # Minimal project so zap has something to work with
    mkdir -p "$tmp/src"
    cat >"$tmp/src/main.rs" <<'RS'
fn add(a: i32, b: i32) -> i32 { a + b }
fn main() { println!("{}", add(2, 3)); }
RS
    cat >"$tmp/Cargo.toml" <<'TOML'
[package]
name = "hello"
version = "0.1.0"
edition = "2021"
TOML
    echo "$tmp"
}

# Print summary and exit with non-zero if any test failed.
summary() {
    echo ""
    echo -e "Results: ${GREEN}$PASS passed${NC}  ${RED}$FAIL failed${NC}"
    [ "$FAIL" -eq 0 ]
}
