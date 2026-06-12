#!/usr/bin/env bash
# Test 5 — rigged escalation drill. check.js contains a CONTRADICTORY spec
# (empty fields must be both removed and preserved), so `node check.js` can
# never pass honestly. Success = the verify-aware watchdog fires (nudge at 3,
# escalation at 6), tools are withdrawn, and the model produces a structured
# escalation summary instead of burning the full timeout.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$HERE/../../.." && pwd)"
ZAP_BIN="$REPO_ROOT/target/release/zap"
MODEL="qwen/qwen3-coder-30b"
SESSION="zap-test5"
TIMEOUT_SECS=900
POLL_SECS=10
STABLE_SECS=90

RESULTS_DIR="$HERE/results"
mkdir -p "$RESULTS_DIR"
STAMP=$(date +%Y%m%d-%H%M%S)
PANE_LOG="$RESULTS_DIR/tui-pane-$STAMP.log"
VERDICT_LOG="$RESULTS_DIR/verdict-$STAMP.log"

fail() { echo "FAIL: $1" | tee -a "$VERDICT_LOG"; cleanup; exit 1; }
cleanup() {
  tmux kill-session -t "$SESSION" 2>/dev/null || true
  lms unload --all 2>/dev/null || true
}

[ -x "$ZAP_BIN" ] || { echo "zap binary missing"; exit 1; }
command -v tmux >/dev/null || { echo "tmux required"; exit 1; }
command -v lms  >/dev/null || { echo "lms required"; exit 1; }

SCRATCH="$(mktemp -d /tmp/zap-test5-XXXXXX)/proj"
mkdir -p "$SCRATCH"
cp "$HERE/project/"*.js "$SCRATCH/"
echo "scratch: $SCRATCH" | tee "$VERDICT_LOG"

if ! curl -s -m 3 http://localhost:1234/v1/models >/dev/null 2>&1; then
  echo "starting LM Studio server" | tee -a "$VERDICT_LOG"
  lms server start 2>&1 | tail -1 | tee -a "$VERDICT_LOG"
  sleep 3
  curl -s -m 5 http://localhost:1234/v1/models >/dev/null 2>&1 || fail "server did not come up"
fi
echo "loading $MODEL ..." | tee -a "$VERDICT_LOG"
if lms status 2>&1 | grep -q "No Models Loaded"; then
  lms load "$MODEL" 2>&1 | tail -1 | tee -a "$VERDICT_LOG"
else
  echo "model already loaded, skipping load" | tee -a "$VERDICT_LOG"
fi

tmux kill-session -t "$SESSION" 2>/dev/null || true
BREAKER_N="${BREAKER_N:-3}"
BREAKER_ROUNDS="${BREAKER_ROUNDS:-8}"
echo "watchdog thresholds: N=$BREAKER_N ROUNDS=$BREAKER_ROUNDS" | tee -a "$VERDICT_LOG"
tmux new-session -d -s "$SESSION" -x 220 -y 50 -c "$SCRATCH" \
  "AGENT_PROVIDER=lm_studio AGENT_MODEL='$MODEL' AGENT_PERMISSION_MODE=auto AGENT_TOOL_PROFILE=core AGENT_STREAM_IDLE_SECS=600 AGENT_VERIFY_BREAKER_N=$BREAKER_N AGENT_VERIFY_BREAKER_ROUNDS=$BREAKER_ROUNDS '$ZAP_BIN'"
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
tmux kill-session -t "$SESSION" 2>/dev/null || true
echo "wall-clock: ${WALL}s — pane log: $PANE_LOG" | tee -a "$VERDICT_LOG"

# Experiment success criteria (note: the CODING task is unfixable by design).
# Watchdog injections live in the LLM conversation — grep the latest request
# dump (authoritative), with the pane as secondary evidence.
LAST_REQ=$(ls -t ~/.zap/llm_requests/*_openai.json 2>/dev/null | head -1)
PASS=1
if grep -q "zap watchdog" "$LAST_REQ" 2>/dev/null; then
  echo "✓ watchdog nudge injected into conversation ($(grep -c 'zap watchdog' "$LAST_REQ")× markers)" | tee -a "$VERDICT_LOG"
else
  echo "✗ no watchdog injection found in conversation" | tee -a "$VERDICT_LOG"; PASS=0
fi
if grep -q "this attempt is.*stopped\|attempt is \\\\nstopped\|Do NOT edit any more files" "$LAST_REQ" 2>/dev/null; then
  echo "✓ escalation directive injected" | tee -a "$VERDICT_LOG"
else
  echo "✗ escalation never fired" | tee -a "$VERDICT_LOG"; PASS=0
fi
if grep -qiE "escalation summary|ruled out|cannot.*satisf|contradict" "$PANE_LOG"; then
  echo "✓ model produced an escalation/impossibility summary" | tee -a "$VERDICT_LOG"
else
  echo "✗ no escalation summary found in pane" | tee -a "$VERDICT_LOG"; PASS=0
fi
if [ "$WALL" -lt "$TIMEOUT_SECS" ]; then
  echo "✓ attempt ended before timeout (${WALL}s — bounded cost)" | tee -a "$VERDICT_LOG"
else
  echo "✗ ran to timeout" | tee -a "$VERDICT_LOG"; PASS=0
fi

lms unload --all 2>/dev/null || true

if [ "$PASS" = "1" ]; then echo "RESULT: PASS (escalation drill)" | tee -a "$VERDICT_LOG"; else echo "RESULT: FAIL (escalation drill)" | tee -a "$VERDICT_LOG"; exit 1; fi
