# Test 3 Runbook — SLM through the real zap TUI

## What this tests
Tests 1–2 used a toy harness (raw chat-completions loop + 5 hand-rolled tools).
Test 3 runs the same class of task through **zap itself** — the real TUI, real tool
implementations (code index, grep, edit-with-better-errors), real agent loop — with
the local SLM `qwen/qwen3-coder-30b` served by LM Studio. The open question:
**does zap need code changes to be SLM-friendly, or is config enough?**

## Prereqs
- `target/release/zap` built (`cargo build --release`) — config-only, no code changes
- LM Studio running with `qwen/qwen3-coder-30b` downloaded (`lms ls`)
- `tmux`, `node` (v25+), `lms` CLI on PATH
- ~/.agent.toml has the `lm_studio` provider (http://localhost:1234/v1)

## Run it
```bash
cd research/slm-coding-eval/test3-zap-run
./run.sh
```

What it does:
1. Copies `project-template/` (pristine tasks REST API, green suite) to a scratch dir
2. `lms load qwen/qwen3-coder-30b`
3. Starts the **real zap TUI** inside tmux with `AGENT_PROVIDER=lm_studio`,
   `AGENT_MODEL=qwen/qwen3-coder-30b`, `AGENT_PERMISSION_MODE=auto` (auto-approve
   tools, but the genuine interactive TUI — not headless mode),
   `AGENT_TOOL_PROFILE=core` (slim 6-tool surface — cuts prompt-processing time
   ~10x for local models), and `AGENT_STREAM_IDLE_SECS=600` (idle watchdog)
4. Pastes `TASK.md` (the frontier-authored, state-explicit DELETE /tasks/:id plan)
   as the user prompt
5. Polls the tmux pane; declares the turn finished when the pane is idle 90s
   (or 15 min timeout)
6. Saves the full TUI scrollback to `results/tui-pane-<stamp>.log`
7. **Objectively verifies** (never trusts the model's claims):
   - `node test.js` prints `all tests passed`
   - `DELETE /tasks/1` → 204, then `GET /tasks/1` → 404, `DELETE /tasks/999` → 404
   - existing assertions untouched (`buy milk` + final console.log still present)
8. `lms unload --all`, prints `RESULT: PASS` or `RESULT: FAIL`

## Success looks like
```
✓ node test.js green
✓ DELETE behavior correct (204 / gone / 404)
✓ existing test content preserved
RESULT: PASS
```
Plus a `results/tui-pane-*.log` showing zap's tool calls (read_file/edit_file/shell)
driven by the SLM.

## Watch it live (optional)
While run.sh is polling: `tmux attach -t zap-test3` (detach: Ctrl-b d).

## Failure modes to look for in the pane log
- repeated identical failing edit_file calls (loop-breaker candidate, src/session/tools.rs)
- "old_string not found" thrash (error-hint candidate, src/tools/file/edit.rs ~line 101)
- the model claiming success without running `node test.js`
