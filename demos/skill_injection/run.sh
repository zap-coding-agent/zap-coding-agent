#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR/flask"

export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"

run_scenario() {
  local n="$1" label="$2" script="$3"
  echo ""
  echo "══════════════════════════════════════════════════════════"
  echo "  Scenario $n: $label"
  echo "══════════════════════════════════════════════════════════"
  bash "$SCRIPT_DIR/scenarios/$script"
}

run_scenario 1 "Python skill auto-injection"        "01_python_skill.sh"
run_scenario 2 "Two skills on one turn (python+git)" "02_two_skills.sh"
run_scenario 3 "Casual message — no skills"         "03_casual.sh"

echo ""
echo "  ✓ all scenarios complete"
