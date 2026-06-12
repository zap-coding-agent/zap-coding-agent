#!/usr/bin/env bash
# Test 6 — structured task execution. The model is given a pre-written plan
# (skill.md) and must execute it step-by-step. Success = model follows the
# plan, adds the /todos endpoint, and test.js passes within timeout.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$HERE/../../.." && pwd)"
ZAP_BIN="$REPO_ROOT/target/release/zap"
MODEL="qwen/qwen3-coder-30b"
SESSION="zap-test6"
TIMEOUT_SECS=600
POLL_SECS=10
STABLE_SECS=60

RESULTS_DIR="$HERE/results"
mkdir -p "$RESULTS_DIR"
STAMP=$(date +%Y%m%d-%H%M%S)
PANE_LOG="$RESULTS_DIR/tui-pane-$STAMP.log"
VERDICT_LOG="$RESULTS_DIR/verdict-$STAMP.log"

cleanup() {
  tmux kill-session -t "$SESSION" 2>/dev/null || true
}
trap cleanup EXIT

[ -x "$ZAP_BIN" ] || { echo "zap binary missing"; exit 1; }
command -v tmux >/dev/null || { echo "tmux required"; exit 1; }
command -v node  >/dev/null || { echo "node required"; exit 1; }

SCRATCH="$(mktemp -d /tmp/zap-test6-XXXXXX)/proj"
mkdir -p "$SCRATCH"
cp "$HERE/project/"* "$SCRATCH/"
cd "$SCRATCH" && npm install --silent 2>&1 | tail -1
echo "scratch: $SCRATCH" | tee "$VERDICT_LOG"

# test.js starts and stops its own server, so no need to pre-start one

# Pre-index the code so the model can use code_map, find_definition, etc.
echo "indexing project..." | tee -a "$VERDICT_LOG"
cd "$SCRATCH" && "$ZAP_BIN" --index-only 2>&1 | tail -1 | tee -a "$VERDICT_LOG"
cd "$SCRATCH"  # ensure we're still in the right dir

# Ensure model is loaded
if ! curl -s -m 3 http://localhost:1234/v1/models >/dev/null 2>&1; then
  lms server start 2>&1 | tail -1 | tee -a "$VERDICT_LOG"
  sleep 3
fi
echo "loading $MODEL ..." | tee -a "$VERDICT_LOG"
if lms status 2>&1 | grep -q "No Models Loaded"; then
  lms load "$MODEL" 2>&1 | tail -1 | tee -a "$VERDICT_LOG"
else
  echo "model already loaded, skipping load" | tee -a "$VERDICT_LOG"
fi

tmux kill-session -t "$SESSION" 2>/dev/null || true
tmux new-session -d -s "$SESSION" -x 220 -y 50 -c "$SCRATCH" \
  "AGENT_PROVIDER=lm_studio AGENT_MODEL='$MODEL' AGENT_PERMISSION_MODE=auto AGENT_TOOL_PROFILE=core AGENT_STREAM_IDLE_SECS=600 '$ZAP_BIN'"
sleep 6

TASK="$(cat "$HERE/TASK.md" | tr '\n' ' ')"
tmux send-keys -t "$SESSION" -l "$TASK"
sleep 1
tmux send-keys -t "$SESSION" Enter
START=$(date +%s)
echo "task sent $(date '+%H:%M:%S')" | tee -a "$VERDICT_LOG"

SEEN_BUSY=0
LAST_PANE=""
LAST_CHANGE=$(date +%s)
while :; do
  sleep "$POLL_SECS"
  NOW=$(date +%s)
  PANE="$(tmux capture-pane -t "$SESSION" -p -S -2000 2>/dev/null || true)"
  [ -z "$PANE" ] && break
  if [ "$PANE" != "$LAST_PANE" ]; then LAST_PANE="$PANE"; LAST_CHANGE=$NOW; fi
  ELAPSED=$((NOW - START))
  IDLE_FOR=$((NOW - LAST_CHANGE))
  STATUS_IDLE=0
  printf '%s' "$PANE" | grep -q "● idle" && STATUS_IDLE=1
  if [ "$STATUS_IDLE" = "0" ]; then SEEN_BUSY=1; fi
  if [ "$SEEN_BUSY" = "1" ] && [ "$STATUS_IDLE" = "1" ]; then
    echo "turn finished after ${ELAPSED}s" | tee -a "$VERDICT_LOG"; break
  fi
  if [ "$IDLE_FOR" -ge "$STABLE_SECS" ]; then echo "pane stable — done after ${ELAPSED}s" | tee -a "$VERDICT_LOG"; break; fi
  if [ "$ELAPSED" -ge "$TIMEOUT_SECS" ]; then echo "TIMEOUT after ${ELAPSED}s" | tee -a "$VERDICT_LOG"; break; fi
done
WALL=$(( $(date +%s) - START ))

tmux capture-pane -t "$SESSION" -p -S -10000 > "$PANE_LOG" 2>/dev/null || true
tmux send-keys -t "$SESSION" -l "/exit"; tmux send-keys -t "$SESSION" Enter
sleep 2

echo "wall-clock: ${WALL}s — pane log: $PANE_LOG" | tee -a "$VERDICT_LOG"

# Success criteria: did the model complete the task?
PASS=1
cd "$SCRATCH"
if node test.js 2>&1 | grep -q "^ok$"; then
  echo "✓ test.js passes — task completed" | tee -a "$VERDICT_LOG"
else
  echo "✗ test.js does NOT pass" | tee -a "$VERDICT_LOG"
  node test.js 2>&1 | tee -a "$VERDICT_LOG"
  PASS=0
fi

# Check that /todos endpoint was actually added
if grep -q "app.get('/todos'" "$SCRATCH/app.js" 2>/dev/null; then
  echo "✓ /todos route found in app.js" | tee -a "$VERDICT_LOG"
else
  echo "✗ /todos route NOT found in app.js" | tee -a "$VERDICT_LOG"; PASS=0
fi

if [ "$WALL" -lt "$TIMEOUT_SECS" ]; then
  echo "✓ finished within timeout (${WALL}s)" | tee -a "$VERDICT_LOG"
else
  echo "✗ timeout" | tee -a "$VERDICT_LOG"; PASS=0
fi

if [ "$PASS" = "1" ]; then echo "RESULT: PASS (structured task)" | tee -a "$VERDICT_LOG"; else echo "RESULT: FAIL (structured task)" | tee -a "$VERDICT_LOG"; exit 1; fi
