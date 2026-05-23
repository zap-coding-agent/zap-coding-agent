#!/usr/bin/env bash
# Run all zap e2e tests.
# Usage:
#   ./tests/e2e/run_all.sh            # run everything
#   ./tests/e2e/run_all.sh test_basic # run one suite by name (no .sh)
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
ZAP="${ZAP_BIN:-$HOME/.local/bin/zap}"

# Check binary exists
if ! command -v "$ZAP" &>/dev/null && ! [ -x "$ZAP" ]; then
    echo "ERROR: zap not found at $ZAP (set ZAP_BIN to override)"
    exit 1
fi

echo "zap binary : $ZAP"
echo "test dir   : $DIR"
echo ""

TOTAL_PASS=0; TOTAL_FAIL=0

run_suite() {
    local script="$1"
    local name
    name=$(basename "$script" .sh)
    echo "━━━ $name ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    if bash "$script"; then
        TOTAL_PASS=$((TOTAL_PASS + 1))
    else
        TOTAL_FAIL=$((TOTAL_FAIL + 1))
    fi
    echo ""
}

if [ $# -gt 0 ]; then
    for name in "$@"; do
        script="$DIR/${name%.sh}.sh"
        if [ -f "$script" ]; then
            run_suite "$script"
        else
            echo "ERROR: test not found: $script"
            TOTAL_FAIL=$((TOTAL_FAIL + 1))
        fi
    done
else
    for script in "$DIR"/test_*.sh; do
        run_suite "$script"
    done
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Suites: $TOTAL_PASS passed  $TOTAL_FAIL failed"
[ "$TOTAL_FAIL" -eq 0 ]
