# Test 3 — Handoff Prompt (run the SLM through zap itself)

This is a **self-contained handoff**. Paste the prompt block below into a fresh model/agent
(any model — Claude Code or otherwise) and it has everything needed to finish Test 3: run the
SLM through **zap itself** (the real product) instead of the standalone research harness.

## Current state (as of handoff)
- Branch `research/slm-coding-eval` — committed & pushed (Test 1 + Test 2 + docs).
- Local release binary built & verified: `./target/release/zap` → `zap 0.15.14` (no codesigning issue).
- PR pending only: `gh auth login` needed, or open via
  https://github.com/zap-coding-agent/zap-coding-agent/pull/new/research/slm-coding-eval
- Open question this test answers: **does zap need code changes to be SLM-friendly, or only config?**

---

## The prompt

````text
You are continuing work in the zap repo: /Users/sanjeevgulati/personal-repos/ideas
(zap = a Rust TUI AI coding agent). Read .zap/context.md and .zap/session_log.md first.

## Background — what's already done

We're validating a capability: a FRONTIER model writes the plan, a LOCAL SMALL model (SLM)
executes the coding. Evidence lives in research/slm-coding-eval/ (already committed & pushed
on branch `research/slm-coding-eval`):
- Test 1 (harness.mjs): 5 local models × 3 tasks. Scoped single-file fixes pass 5/5; the
  agentic-tuned tier (qwen3-coder-30b, devstral-small-2) passes 3/3; a generic 14B "coder"
  scores 1/3. Model CLASS beats SIZE.
- Test 2 (test2.mjs): qwen3-coder-30b built a real multi-file feature (a Node tasks REST API)
  from a frontier-authored, step-decomposed plan. Feature code passed first try; the
  test-writing step failed when UNDER-SPECIFIED, then passed once the plan named the state
  setup explicitly. Lesson: an SLM's success = how tightly the frontier scopes the step.
- docs/slm-support.md = positioning + a propose-and-confirm model-routing design.
- A local release binary is already built: ./target/release/zap  (runs as `zap 0.15.14`).

## Key facts you need

- zap can ALREADY talk to a local SLM — NO code change required to RUN it. ~/.agent.toml has an
  `lm_studio` provider at http://localhost:1234/v1/chat/completions. Override the model per-run
  with env vars (config.rs reads these): AGENT_PROVIDER, AGENT_MODEL, AGENT_BASE_URL.
- zap headless one-shot mode:  zap --goal "<prompt>" -y --output-format text
  (-y = auto-approve tools, non-interactive). This is how you run the test non-interactively.
- The SLM to use: "qwen/qwen3-coder-30b" (already downloaded in LM Studio).
- LM Studio model management:  `lms load "<id>"`, `lms unload --all`, `lms ps`, `lms ls`.
  ALWAYS unload models when done to free RAM (machine is Apple M5 / 32 GB — keep models ≤17 GB).
- Node 25 + global fetch is available. pytest is NOT installed; use Node for any test fixtures.

## YOUR TASK — "Test 3": run the SLM through ZAP ITSELF (the real product), not a toy harness

Build research/slm-coding-eval/test3-zap-run/ containing:

1. project-template/ — a pristine zero-dep Node "tasks" REST API to be modified. Use these 4
   files (copy the exact seed from test2.mjs's SEED object — store.js, router.js, server.js,
   test.js). It's a tasks API with GET/POST /tasks, GET /tasks/:id, and a passing test.js suite
   (run `node test.js` → "all tests passed").

2. TASK.md — a FRONTIER-AUTHORED, SLM-FRIENDLY plan for a NEW small feature: "Add DELETE
   /tasks/:id". Decompose into explicit, state-named steps (this is the whole point — apply the
   Test 2 lesson):
     - store.js: add+export `remove(id)` → returns true if a task was removed, false if no task
       had that id. Don't change other functions.
     - router.js: handle `DELETE /tasks/:id` (parse numeric id with the existing
       /^\/tasks\/(\d+)$/ shape). If removed → { status: 204, json: null }. If not found →
       { status: 404, json: { error: 'not found' } }. Keep all existing routes working.
     - test.js: add NEW assertions at the END (don't modify/reorder existing ones; keep
       console.log('all tests passed') last). Be STATE-EXPLICIT: call store._reset(), create one
       task via handle('POST','/tasks',{},{title:'t'}) (id 1), then assert
       handle('DELETE','/tasks/1',{},null).status === 204 AND
       handle('GET','/tasks/1',{},null).status === 404 (gone), AND
       handle('DELETE','/tasks/999',{},null).status === 404. Then `node test.js` must still pass.
   Write TASK.md as a single self-contained prompt string (this becomes the --goal).

3. run.sh — end-to-end script that: copies project-template/ to a scratch dir (so the template
   stays pristine), `lms load "qwen/qwen3-coder-30b"`, runs
   `AGENT_PROVIDER=lm_studio AGENT_MODEL="qwen/qwen3-coder-30b" /path/to/target/release/zap
   --goal "$(cat TASK.md)" -y --output-format text` IN the scratch dir, then objectively
   verifies (run `node test.js`; check DELETE returns 204 and the task is gone via `node -e`),
   then `lms unload --all`. Print PASS/FAIL.

4. RUNBOOK.md — human instructions + what success looks like.

Then RUN run.sh, capture the result, and write a "Test 3 — executed by zap" section into
research/slm-coding-eval/README.md covering: did zap's REAL harness (which has a code index +
grep + better edit errors than the toy harness) succeed where/how the toy harness did? Record
turns/time if observable, the final suite result, and any SLM failure modes seen THROUGH zap.

## The open question to ANSWER with evidence

Does zap need code changes to be SLM-friendly? Hypothesis: NO code change is needed to RUN
(config only), and zap's richer scaffolding may do BETTER than the toy harness. Only AFTER
observing real failures through zap, propose targeted changes. Likely candidates (don't
implement preemptively — justify from observed behavior):
  - src/tools/file/edit.rs: the "old_string not found" error (around line 101) could append a
    "re-read the file to see its exact current text" hint.
  - The agent loop (src/session/tools.rs / turn.rs): a repeated-identical-failing-tool-call
    breaker, and/or "N consecutive failed verifies → stop & escalate" (the toy harness's
    identical-call breaker did NOT catch a model trying DIFFERENT broken fixes).

## Housekeeping
- Work on branch `research/slm-coding-eval` (already checked out & pushed). Commit Test 3 there.
- Commit message footer:  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
- A PR could not be opened (gh not authenticated — `gh auth login` needed, or open via:
  https://github.com/zap-coding-agent/zap-coding-agent/pull/new/research/slm-coding-eval ).
- Unload all LM Studio models at the end.
````
