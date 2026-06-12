#!/usr/bin/env bash
# Test 4 — goal-level spec (no step decomposition) through the real zap TUI.
# Level-up from Test 3: the SLM must plan its own edits. Validation edge cases included.
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$HERE/../../.." && pwd)"
ZAP_BIN="$REPO_ROOT/target/release/zap"
TEMPLATE="$HERE/../test3-zap-run/project-template"
MODEL="qwen/qwen3-coder-30b"
SESSION="zap-test4"
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

[ -x "$ZAP_BIN" ] || { echo "zap binary missing — run: cargo build --release"; exit 1; }
command -v tmux >/dev/null || { echo "tmux required"; exit 1; }
command -v lms  >/dev/null || { echo "lms (LM Studio CLI) required"; exit 1; }

SCRATCH="$(mktemp -d /tmp/zap-test4-XXXXXX)/proj"
mkdir -p "$SCRATCH"
cp "$TEMPLATE/"*.js "$SCRATCH/"
echo "scratch: $SCRATCH" | tee "$VERDICT_LOG"
( cd "$SCRATCH" && node test.js | grep -q "all tests passed" ) || fail "template suite not green before run"

if ! curl -s -m 3 http://localhost:1234/v1/models >/dev/null 2>&1; then
  echo "LM Studio server not running — starting it" | tee -a "$VERDICT_LOG"
  lms server start 2>&1 | tail -1 | tee -a "$VERDICT_LOG"
  sleep 3
  curl -s -m 5 http://localhost:1234/v1/models >/dev/null 2>&1 || fail "LM Studio server did not come up on :1234"
fi
echo "loading $MODEL ..." | tee -a "$VERDICT_LOG"
lms load "$MODEL" 2>&1 | tail -1 | tee -a "$VERDICT_LOG"

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
    echo "WARN: never saw busy status in 60s" | tee -a "$VERDICT_LOG"
    SEEN_BUSY=1
  fi
  if [ "$IDLE_FOR" -ge "$STABLE_SECS" ]; then echo "pane stable ${IDLE_FOR}s after ${ELAPSED}s — assuming done" | tee -a "$VERDICT_LOG"; break; fi
  if [ "$ELAPSED" -ge "$TIMEOUT_SECS" ]; then echo "TIMEOUT after ${ELAPSED}s" | tee -a "$VERDICT_LOG"; break; fi
done
WALL=$(( $(date +%s) - START ))

tmux capture-pane -t "$SESSION" -p -S -5000 > "$PANE_LOG" 2>/dev/null || true
tmux send-keys -t "$SESSION" -l "/exit"; tmux send-keys -t "$SESSION" Enter
sleep 2
tmux kill-session -t "$SESSION" 2>/dev/null || true
echo "wall-clock: ${WALL}s — pane log: $PANE_LOG" | tee -a "$VERDICT_LOG"

PASS=1
cd "$SCRATCH"
if node test.js 2>&1 | grep -q "all tests passed"; then
  echo "✓ node test.js green" | tee -a "$VERDICT_LOG"
else
  echo "✗ node test.js failed: $(node test.js 2>&1 | head -3)" | tee -a "$VERDICT_LOG"; PASS=0
fi
PATCH_CHECK=$(node -e "
const s=require('./store'); const {handle}=require('./router');
s._reset(); handle('POST','/tasks',{},{title:'t'});
const ok   = handle('PATCH','/tasks/1',{},{completed:true});
const ttl  = handle('PATCH','/tasks/1',{},{title:'renamed'});
const bad1 = handle('PATCH','/tasks/1',{},{});
const bad2 = handle('PATCH','/tasks/1',{},{title:''});
const bad3 = handle('PATCH','/tasks/1',{},{completed:'yes'});
const miss = handle('PATCH','/tasks/999',{},{completed:true});
console.log(ok.status, ok.json && ok.json.completed, ttl.status, ttl.json && ttl.json.title,
            bad1.status, bad2.status, bad3.status, miss.status);" 2>&1)
if [ "$PATCH_CHECK" = "200 true 200 renamed 400 400 400 404" ]; then
  echo "✓ PATCH behavior correct (200/200/400/400/400/404)" | tee -a "$VERDICT_LOG"
else
  echo "✗ PATCH behavior wrong: got '$PATCH_CHECK' want '200 true 200 renamed 400 400 400 404'" | tee -a "$VERDICT_LOG"; PASS=0
fi
GET_CHECK=$(node -e "
const s=require('./store'); const {handle}=require('./router');
s._reset(); handle('POST','/tasks',{},{title:'a'});
console.log(handle('GET','/tasks',{},null).status, handle('GET','/tasks/1',{},null).status,
            handle('POST','/tasks',{},{}).status);" 2>&1)
if [ "$GET_CHECK" = "200 200 400" ]; then
  echo "✓ existing routes still work (GET list/id, POST validation)" | tee -a "$VERDICT_LOG"
else
  echo "✗ existing routes broken: got '$GET_CHECK' want '200 200 400'" | tee -a "$VERDICT_LOG"; PASS=0
fi
if grep -q "console.log('all tests passed')" test.js && grep -q "buy milk" test.js && grep -qi "patch" test.js; then
  echo "✓ tests extended, existing content preserved" | tee -a "$VERDICT_LOG"
else
  echo "✗ test.js missing PATCH assertions or damaged" | tee -a "$VERDICT_LOG"; PASS=0
fi

lms unload --all 2>/dev/null || true

if [ "$PASS" = "1" ]; then echo "RESULT: PASS" | tee -a "$VERDICT_LOG"; else echo "RESULT: FAIL" | tee -a "$VERDICT_LOG"; exit 1; fi
