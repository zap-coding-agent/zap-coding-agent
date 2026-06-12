#!/usr/bin/env bash
# Test 3 — run qwen3-coder-30b through the REAL zap TUI (driven via tmux).
# Usage: ./run.sh   (from research/slm-coding-eval/test3-zap-run/)
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$HERE/../../.." && pwd)"
ZAP_BIN="$REPO_ROOT/target/release/zap"
MODEL="qwen/qwen3-coder-30b"
SESSION="zap-test3"
TIMEOUT_SECS=900   # 15 min hard cap
POLL_SECS=10
STABLE_SECS=90     # pane unchanged this long after last activity => model idle

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

[ -x "$ZAP_BIN" ] || { echo "zap binary missing — run: cargo build --release"; exit 1; }
command -v tmux >/dev/null || { echo "tmux required"; exit 1; }
command -v lms  >/dev/null || { echo "lms (LM Studio CLI) required"; exit 1; }

# 1. Pristine scratch copy
SCRATCH="$(mktemp -d /tmp/zap-test3-XXXXXX)/proj"
mkdir -p "$SCRATCH"
cp "$HERE/project-template/"*.js "$SCRATCH/"
echo "scratch: $SCRATCH" | tee "$VERDICT_LOG"
( cd "$SCRATCH" && node test.js | grep -q "all tests passed" ) || fail "template suite not green before run"

# 2. Ensure the LM Studio API server is up, then load the SLM
if ! curl -s -m 3 http://localhost:1234/v1/models >/dev/null 2>&1; then
  echo "LM Studio server not running — starting it" | tee -a "$VERDICT_LOG"
  lms server start 2>&1 | tail -1 | tee -a "$VERDICT_LOG"
  sleep 3
  curl -s -m 5 http://localhost:1234/v1/models >/dev/null 2>&1 || fail "LM Studio server did not come up on :1234"
fi
echo "loading $MODEL ..." | tee -a "$VERDICT_LOG"
lms load "$MODEL" 2>&1 | tail -1 | tee -a "$VERDICT_LOG"

# 3. Start the REAL zap TUI in tmux (auto-approve tools via env, interactive UI preserved)
tmux kill-session -t "$SESSION" 2>/dev/null || true
tmux new-session -d -s "$SESSION" -x 220 -y 50 -c "$SCRATCH" \
  "AGENT_PROVIDER=lm_studio AGENT_MODEL='$MODEL' AGENT_PERMISSION_MODE=auto AGENT_TOOL_PROFILE=core AGENT_STREAM_IDLE_SECS=600 '$ZAP_BIN'"
sleep 6   # TUI startup

# 4. Send the task prompt (single line) + Enter
TASK="$(cat "$HERE/TASK.md" | tr '\n' ' ')"
tmux send-keys -t "$SESSION" -l "$TASK"
sleep 1
tmux send-keys -t "$SESSION" Enter
START=$(date +%s)
echo "task sent $(date '+%H:%M:%S')" | tee -a "$VERDICT_LOG"

# 5. Poll zap's own status field: wait until it leaves "idle" (turn started),
#    then until it returns to "idle" (turn finished). Pane-stability is the fallback.
SEEN_BUSY=0
LAST_PANE=""
LAST_CHANGE=$(date +%s)
while :; do
  sleep "$POLL_SECS"
  NOW=$(date +%s)
  PANE="$(tmux capture-pane -t "$SESSION" -p -S -2000 2>/dev/null || true)"
  [ -z "$PANE" ] && break   # session died
  if [ "$PANE" != "$LAST_PANE" ]; then
    LAST_PANE="$PANE"; LAST_CHANGE=$NOW
  fi
  ELAPSED=$((NOW - START))
  IDLE_FOR=$((NOW - LAST_CHANGE))
  STATUS_IDLE=0
  printf '%s' "$PANE" | grep -q "● idle" && STATUS_IDLE=1
  if [ "$STATUS_IDLE" = "0" ]; then SEEN_BUSY=1; fi
  if [ "$SEEN_BUSY" = "1" ] && [ "$STATUS_IDLE" = "1" ]; then
    echo "status back to idle after ${ELAPSED}s — turn finished" | tee -a "$VERDICT_LOG"; break
  fi
  if [ "$SEEN_BUSY" = "0" ] && [ "$ELAPSED" -ge 60 ]; then
    echo "WARN: never saw busy status in 60s — prompt may not have submitted" | tee -a "$VERDICT_LOG"
    SEEN_BUSY=1   # fall through to idle/stability detection
  fi
  if [ "$IDLE_FOR" -ge "$STABLE_SECS" ]; then echo "pane stable ${IDLE_FOR}s after ${ELAPSED}s — assuming done" | tee -a "$VERDICT_LOG"; break; fi
  if [ "$ELAPSED" -ge "$TIMEOUT_SECS" ]; then echo "TIMEOUT after ${ELAPSED}s" | tee -a "$VERDICT_LOG"; break; fi
done
WALL=$(( $(date +%s) - START ))

# 6. Save full pane scrollback, then quit the TUI
tmux capture-pane -t "$SESSION" -p -S -5000 > "$PANE_LOG" 2>/dev/null || true
tmux send-keys -t "$SESSION" -l "/exit"; tmux send-keys -t "$SESSION" Enter
sleep 2
tmux kill-session -t "$SESSION" 2>/dev/null || true
echo "wall-clock: ${WALL}s — pane log: $PANE_LOG" | tee -a "$VERDICT_LOG"

# 7. Objective verification (independent of anything the model claimed)
PASS=1
cd "$SCRATCH"
if node test.js 2>&1 | grep -q "all tests passed"; then
  echo "✓ node test.js green" | tee -a "$VERDICT_LOG"
else
  echo "✗ node test.js failed: $(node test.js 2>&1 | head -3)" | tee -a "$VERDICT_LOG"; PASS=0
fi
DELETE_CHECK=$(node -e "
const s=require('./store'); const {handle}=require('./router');
s._reset(); handle('POST','/tasks',{},{title:'t'});
const del=handle('DELETE','/tasks/1',{},null);
const gone=handle('GET','/tasks/1',{},null);
const miss=handle('DELETE','/tasks/999',{},null);
console.log(del.status, gone.status, miss.status);" 2>&1)
if [ "$DELETE_CHECK" = "204 404 404" ]; then
  echo "✓ DELETE behavior correct (204 / gone / 404)" | tee -a "$VERDICT_LOG"
else
  echo "✗ DELETE behavior wrong: got '$DELETE_CHECK' want '204 404 404'" | tee -a "$VERDICT_LOG"; PASS=0
fi
# existing assertions untouched?
if grep -q "console.log('all tests passed')" test.js && grep -q "buy milk" test.js; then
  echo "✓ existing test content preserved" | tee -a "$VERDICT_LOG"
else
  echo "✗ existing test content damaged" | tee -a "$VERDICT_LOG"; PASS=0
fi

# 8. Unload models (free RAM on the 32 GB M5)
lms unload --all 2>/dev/null || true

if [ "$PASS" = "1" ]; then echo "RESULT: PASS" | tee -a "$VERDICT_LOG"; else echo "RESULT: FAIL" | tee -a "$VERDICT_LOG"; exit 1; fi
