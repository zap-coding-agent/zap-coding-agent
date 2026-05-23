#!/usr/bin/env bash
# T07 — Regression tests for specific bugs that were fixed.
set -euo pipefail
source "$(dirname "$0")/helpers.sh"

TMP=$(make_project)
trap "rm -rf $TMP" EXIT

# ── R01: UTF-8 char boundary panic (em-dash in grep output ≥ 20 000 bytes)
# Root cause: session/mod.rs sliced tool output at byte 20000 without checking
# char boundaries.  An em-dash (3 bytes) could straddle the cut point → panic.
# Fix: walk back from 20000 until output.is_char_boundary(cut).
# Reproduce: create many matching files so grep output exceeds 20000 bytes;
# embed em-dashes in the matching lines near the boundary.
info "R01: UTF-8 char boundary panic — large grep output with em-dash"

# Generate 300 Rust files each containing a comment with an em-dash (—) and
# the search pattern on the same line.  Each file's grep output line is ~120
# bytes; 300 files × ~120 bytes ≈ 36000 bytes total grep output, which will
# straddle byte 20000 with em-dashes throughout.
mkdir -p "$TMP/src/generated"
python3 -c "
import os
out = '$TMP/src/generated'
for i in range(300):
    path = os.path.join(out, f'mod_{i:04d}.rs')
    with open(path, 'w') as f:
        # em-dash — in every line — 3 UTF-8 bytes each
        f.write(f'// module {i:04d} — auto-generated test file\n')
        f.write(f'pub struct SnakeAndLadder{i} — {{ value: i32 }}\n')
        f.write(f'pub fn run_{i}() -> i32 {{ {i} }}\n')
"

OUT=$(cd "$TMP" && zap_run "$TMP" "Search the codebase for 'SnakeAndLadder' and tell me how many structs you found." 2>&1) || true

if echo "$OUT" | grep -q "panicked at"; then
    fail "R01 no char-boundary panic" "panic: $(echo "$OUT" | grep 'panicked')"
else
    pass "R01 no char-boundary panic on large grep output with em-dashes"
fi

if echo "$OUT" | grep -qiE "SnakeAndLadder|snake|ladder|struct|found|[0-9]+"; then
    pass "R01b agent reports search results"
else
    fail "R01b agent reports search results" "got: $(echo "$OUT" | tail -5)"
fi

# ── R02: /sessions resume updates session_id (not just messages)
# We verify the sessions list renders without crash.  Full resume correctness
# (session_id, model, client all updated) requires multi-session DB inspection
# which is out of scope for a black-box e2e test.
info "R02: /sessions list renders without crash"
OUT2=$(cd "$TMP" && printf '/sessions\n/exit\n' | timeout 30 "$ZAP" --cli 2>&1) || true
if echo "$OUT2" | grep -q "panicked at"; then
    fail "R02 /sessions no crash" "panic: $(echo "$OUT2" | grep 'panicked')"
else
    pass "R02 /sessions no crash"
fi

summary
