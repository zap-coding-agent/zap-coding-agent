# SLM Agentic-Coding Evaluation

**Question:** Can a *small, local* model reliably execute clearly-defined coding tasks
inside an agent loop — enough that zap could hand it scoped work that a frontier model
has designed?

**Short answer:** Yes — *if you pick the right class of model and scope the task.*
Three local models passed every task (one only **4B**). The naïve "code-completion" 14B
was the worst performer. And the result nearly got reported wrong because of **measurement
bugs in our own harness** — documented below as a cautionary tale.

This folder is the reproducible evidence behind [`docs/slm-support.md`](../../docs/slm-support.md).

---

## TL;DR results

Local models via LM Studio, driven through a real multi-turn agent loop with real
filesystem + shell tools, verified objectively (we run the resulting code; we do **not**
trust the model's self-report).

| Model | Size (disk) | A: add fn + test | B: scoped bug-fix | C: cross-file rename | Score |
|---|---|:---:|:---:|:---:|:---:|
| **qwen3-coder-30b** (MoE, ~3.3B active) | 30B / 17 GB | ✅ | ✅ | ✅ | **3/3** |
| **devstral-small-2** (agentic-tuned) | 24B / 14 GB | ✅ | ✅ | ✅ | **3/3** |
| **gemma-4-e4b** | 4B / 6 GB | ✅ | ✅ | ✅ | **3/3** |
| glm-4.7-flash-reap | 23B MoE / 13 GB | ❌ | ✅ | ✅ | 2/3 |
| qwen2.5-coder-14b | 14B / 8 GB | ❌ | ✅ | ❌ | 1/3 |
| ministral-3-14b | 14B / 33 GB* | — | — | — | n/a |

\* Refused to load — LM Studio estimated 33 GB. Skipped (out of scope: "no heavy models").

**Per-task pass rate:** B (scoped fix) **5/5** · C (cross-file rename) **4/5** · A (add+test) **3/5**.

---

## Headline findings

1. **Model *class* beats model *size*.** A 4B (gemma) and the agentic-tuned 24–30B models
   scored 3/3. The 14B "coder" completion model scored 1/3. "Coder" in the name does **not**
   mean "good agent." The models built/RL-tuned for the agent loop —
   [Devstral](https://arxiv.org/pdf/2509.25193) (Mistral × All Hands AI, built for the
   OpenHands scaffold) and [Qwen3-Coder-30B-A3B](https://huggingface.co/Qwen/Qwen3-Coder-30B-A3B-Instruct)
   (RL-trained on SWE-bench, native tool calling) — are the right tier for an executor role.

2. **Scoped, single-concern tasks are solidly within reach today.** The single-file bug-fix
   (Task B) passed on **every** model that ran, usually in 2–4 turns. This is the strongest
   evidence for the zap thesis: if the frontier model decomposes work into tight, verifiable
   tasks, a local SLM can execute them.

3. **The weak-model failure mode is specific and harness-addressable.** qwen2.5-coder-14b on
   Task C made **56 tool calls over 14 turns**, repeating the *same three failing edits* every
   turn — it never re-read the file after a failed edit, and then **falsely claimed success**.
   This is fixable in the harness (see "Tuning levers"), not only in the model.

4. **Native tool-calling is not the bottleneck.** Every model (even the 4B) emitted proper
   native `tool_calls` with `finish_reason: tool_calls` — zap's `check_text_mode_tool_call`
   guard never fired. The bottleneck is *recovery and state-tracking across turns*, not the
   wire format.

5. **Measure twice.** Our first run concluded "no model can do the cross-file rename." That was
   **false** — caused by two harness bugs (below). Always read the transcripts; always verify
   the verifier.

---

## The measurement-rigor cautionary tale

We nearly shipped the wrong conclusion. Two bugs in the *evaluation harness* (not the models)
produced false negatives:

### Bug 1 — `python` vs `python3` (environment confound)
First harness was Python-based. The sandbox shell had `python3` but no `python`, so every
`run_cmd` the model issued with `python ...` returned `exit=127`. Models burned turns thrashing
between `python`/`python3`, **and our own verifier hit the same wall**. Switching the test
project to **Node.js** (`node` is unambiguous) removed the confound entirely.

### Bug 2 — over-strict verifiers (false negatives)
- **Task C:** the verifier flagged "`greet` still referenced" whenever `require('./greet')`
  remained — but the task renames the *function*, not the *file*. The module path legitimately
  keeps the filename. Qwen3-Coder-30B had **correctly** completed the rename; our regex failed
  it. Fix: strip the module-path string before checking for leftover identifiers.
- **Task A:** the verifier required the exact assertion spelling `multiply(3, 4)`. Devstral
  wrote a passing test in a different form and was wrongly failed. Fix: require that multiply is
  *tested and the suite passes*, not that the assertion matches one regex.

**Impact:** C went from "0/5 (universal fail)" → **4/5 pass**. A's Devstral result flipped
fail → pass. The corrected table above is what the evidence actually shows.

**Lesson for any agent eval:** an over-strict or environment-coupled verifier looks exactly
like model incompetence. Inspect the transcript and the final file state before trusting a
red verdict.

---

## Test machine

- **Apple M5, 32 GB unified memory**, macOS. Models served locally by **LM Studio** on
  `localhost:1234` (OpenAI-compatible endpoint), loaded one at a time.
- This is why models over ~17 GB were out of scope — they either don't fit comfortably in
  32 GB alongside the OS, or LM Studio's guardrail refuses them (e.g. ministral-3-14b
  estimated at 33 GB). The 6–17 GB models tested here all fit with headroom.

## Performance (throughput)

The harness records **per-task wall-time** and **effective output tokens/sec**
(`completion_tokens / generation_seconds`). We did **not** capture a clean throughput sweep
across *all* Test-1 models (it would mean re-running every model) — left for a future round.
We **did** capture real numbers for `qwen3-coder-30b` during Test 2 (see below): roughly
**14–26 tok/s** on the M5 / 32 GB for the 30B MoE (~3.3B active). On these short tasks that's
~35–45 s of model time per scoped step. The MoE design is what keeps a "30B" this responsive
on a laptop; a dense 24B (devstral) would be expected to be slower per token.

## Method

- **Driver:** [`harness.mjs`](./harness.mjs) — a real agent loop (≤14 turns) against LM Studio's
  OpenAI-compatible endpoint (`/v1/chat/completions`), `temperature: 0`.
- **Tools (real, sandbox-jailed):** `list_dir`, `read_file`, `edit_file` (exact unique
  substring replace), `write_file`, `run_cmd` (shell in sandbox). Mirrors the shape of zap's
  own toolset.
- **Sandbox:** a tiny 4-file Node project, freshly copied per run, under the system temp dir.
- **Tasks:**
  - **A** — add `multiply(a,b)` to `calc.js` (+ export), add a test, run the suite green.
    *(multi-step: edit two files + run + interpret output)*
  - **B** — fix `divide` to return `null` on divide-by-zero. *(single-file, single concern)*
  - **C** — rename function `greet` → `welcome` across `greet.js` + `main.js`, keep it running.
    *(cross-file, requires consistency + recovery from failed edits)*
- **Verification:** harness-side and objective — it executes the code (`node test_calc.js`,
  `node main.js`, `node -e ...`) and checks output. The model's claims are ignored.
- **Resource discipline:** models loaded one at a time via `lms load`, unloaded with
  `lms unload --all` between runs. Models over ~17 GB were skipped.

Raw per-model transcripts are in [`results/`](./results/).

---

## Tuning levers (what would raise pass rates)

Most of the observed failures are **scaffolding**, not raw capability — and zap's real harness
already has several of these where this eval did not:

| Lever | Why it helps | In zap? |
|---|---|---|
| Reject empty/ambiguous `old_string` with a *helpful* message | qwen2.5-coder used `old_string:""`; a clear error redirects it | partial |
| **Force a re-read after a failed edit** | the 56-call loop happened because it edited blind | add |
| **Repeated-identical-failure breaker** (detect N identical failing calls → inject a hint or stop) | converts infinite loops into graceful stops | add |
| Provide **grep / replace-all / rename** tools + the code index | cross-file renames stop being N fragile point-edits | ✅ zap has code index |
| Small temperature (0.2–0.4) or loop-breaker | `temp 0` makes a stuck model repeat the exact failing action | tune |
| System-prompt nudge: "after a failed edit, re-read before retrying; never repeat a failed call; never claim success without running it" | directly targets the observed failure mode | add |
| **Pick an agentic-tuned model for the executor role** | Devstral / Qwen3-Coder-30B are built for this loop | config |

---

## Recommendations for zap

- **Local SLM execution is viable today** for clearly-scoped, single-concern tasks. Ship the
  propose-and-confirm routing ([`docs/slm-support.md`](../../docs/slm-support.md)) with the
  executor tier defaulting to an **agentic-tuned** model, not a generic "coder."
- **Default local executor picks:** `devstral-small-2` (24B, 14 GB) or `qwen3-coder-30b`
  (30B MoE, 17 GB, ~3.3B active → fast). Both passed 3/3 here and fit a 32 GB Mac.
- **Do not** default to a code-*completion* model (qwen2.5-coder-14b scored 1/3). The name is
  misleading for agent use.
- **Add the harness scaffolding above** (re-read after failed edit, loop breaker). It will lift
  weaker/cheaper models and make the strong ones more robust — likely higher ROI than swapping
  models.

---

## Can we claim "fit for general-purpose clearly-defined tasks"?

**Partially, with honest limits.** This is `n = 3` tasks, one language, single runs at `temp 0`.
What it *does* establish:

- Scoped, single-file, single-concern tasks: **reliable** across the board (B = 5/5).
- Multi-step and cross-file tasks: **reliable on the agentic-tuned tier** (Devstral and
  Qwen3-Coder-30B = 3/3), unreliable on completion models.

What it does **not** yet establish (future work to claim "general-purpose"):
- A broader suite (20–50 tasks) across categories: new feature, multi-file refactor, debugging
  from a stack trace, test-writing, dependency wiring.
- **Variance:** multiple runs per task (sampling is non-deterministic; a single pass/fail hides
  flakiness). Report pass@k, not a single dot.
- Larger, messier repos (these fixtures are 4 tiny files; real blast radius is the hard part).
- The same tasks run through **zap's actual harness** (with its code index + grep + guards),
  which should beat this minimal scaffold.

**Bottom line:** the evidence supports "agentic-tuned local SLMs are fit to execute *clearly
scoped* tasks a frontier model has decomposed and made verifiable" — which is exactly the role
the zap design assigns them. It does **not** support "drop a 14B in as an autonomous coder."

---

## Test 2 — a realistic feature, executed from a frontier-authored plan

Test 1 used toy fixtures. **Test 2** is the actual zap workflow end-to-end: a frontier model
decomposes a real multi-file feature into scoped, verifiable steps, and a single locked-in SLM
(**`qwen3-coder-30b`**) executes each step as its own agent loop against the **same persistent
project**. Driver: [`test2.mjs`](./test2.mjs). Transcripts:
[`results/test2-qwen3-coder-30b.log`](./results/) (+ `…-step4-refined.log`).

**Project:** a zero-dependency Node "tasks" REST API — `store.js` (data layer),
`router.js` (a port-free `handle(method, path, query, body)` dispatcher), `server.js` (http
wiring), `test.js` (suite). Realistic and deterministically testable.

**Feature:** "task completion + status filtering," decomposed into 4 SLM-sized steps, each
naming the exact file, function signature, behavior, and a verify command.

### Results (qwen3-coder-30b, M5 / 32 GB)

| Step | What | Turns | Tools | Time | tok/s | Result |
|---|---|:---:|:---:|:---:|:---:|---|
| 1 | `store.update(id, patch)` | 5 | 4 | 37 s | 14 | ✅ first try |
| 2 | `PATCH /tasks/:id` (200 / 404) | 6 | 5 | 44 s | 26 | ✅ first try |
| 3 | `GET /tasks?status=open\|done` filter | 8 | 7 | 36 s | 25 | ✅ first try |
| 4 | tests — **under-specified plan** | 16 | 16 | 196 s | 22 | ❌ broke shared test state, thrashed |
| 4′ | tests — **state-explicit plan** | 5 | 4 | 43 s | 18 | ✅ suite green |

**Final:** with step 4′, the **full regression suite passes** — the whole feature
(data layer + two new routes + filtering + tests) hangs together.

### The headline result

**The SLM implemented the entire multi-file feature correctly on the first try (steps 1–3).**
The only failure was *test authoring* — and it was caused by the **plan**, not the model:
the under-specified step ("add assertions covering…") left state setup implicit, so the model
reset the store mid-suite, invalidated a task other assertions depended on, and then thrashed
for 16 turns without converging.

Re-running the **same model on the same step** with a **state-explicit** instruction
("call `store._reset()`, create exactly one task with id 1, then assert…") passed cleanly in
5 turns. That is the whole zap thesis, demonstrated:

> An SLM's success on a step is a function of how tightly the frontier model scoped it.
> Feature code with explicit signatures → first-try success. A step that needs implicit
> reasoning about shared mutable state → failure until the plan spells it out.

### Practical takeaways for zap's planner

- **Name the state.** For test-writing steps especially, the plan must specify setup/reset and
  concrete ids/values — not just the behavior to assert.
- **Per-step verify + escalate.** Each step had an objective verify; a failed step is a signal
  to *re-scope and retry* (or escalate to the frontier model), exactly what step 4 → 4′ shows.
- **Loop-breaker helps but isn't enough.** The harness's identical-failing-call breaker didn't
  fire in step 4 because the model kept trying *different* broken fixes. A "N consecutive failed
  verifies → stop and escalate" guard would be the right complement.
- **Throughput is usable.** ~35–45 s of model time per scoped step on a laptop MoE is fine for
  an interactive propose-and-confirm flow.

## Reproduce

```bash
# requires: LM Studio running on :1234, node 18+, lms CLI
cd research/slm-coding-eval

# one model, all tasks, with full transcript
node harness.mjs "qwen/qwen3-coder-30b" all --verbose

# one task
node harness.mjs "mistralai/devstral-small-2-2512" C

# resource-disciplined sweep (load → test → unload)
for m in "gemma-4-e4b-it" "mistralai/devstral-small-2-2512" "qwen/qwen3-coder-30b"; do
  lms load "$m" >/dev/null 2>&1
  node harness.mjs "$m" all
  lms unload --all >/dev/null 2>&1
done
```

Override the endpoint with `LMSTUDIO_URL=...` if not on `localhost:1234`.

```bash
# Test 2 — realistic feature from a frontier plan (full 4-step run)
lms load "qwen/qwen3-coder-30b" >/dev/null 2>&1
node test2.mjs "qwen/qwen3-coder-30b" --verbose

# re-run a single step against the project a prior pass left (e.g. refined step 4)
node test2.mjs "qwen/qwen3-coder-30b" --only=4 --verbose

lms unload --all
```

---

## Test 3 — executed by zap itself (the real product)

Test 1–2 used a toy harness. Test 3 ran the same class of task — frontier-authored,
state-explicit plan for `DELETE /tasks/:id` — through the **real zap TUI** (driven via tmux,
`AGENT_PERMISSION_MODE=auto`), with qwen3-coder-30b on LM Studio. Assets + runbook:
[`test3-zap-run/`](test3-zap-run/).

**Verdict: PASS — but only after fixing zap.** The model was never the problem.

### Run 1 (zap v0.15.14): total failure, zero tool calls
The request died mid-prompt-processing 5× and the machine ground to a halt. Root causes,
all in zap, none in the SLM:

1. **34k-token prompt.** `skill_paths = ["~/.claude/skills"]` + "no `triggers:` → always-on"
   classification injected 63 Claude-format skills (~28k tokens) into every prompt.
   LM Studio prefill was still at 93% after 2¼ minutes.
2. **120s total HTTP timeout** killed the streaming request during legitimately-silent prefill.
3. **Flat 3s retry** re-sent the full prompt while the server was still chewing the dropped
   one — a prefill stampede (LM Studio kept burning CPU on dead requests, eventually crashed).
4. **Silent spinner** — nothing told the user prefill was happening.

### Fixes shipped (v0.15.15 + v0.15.16)
- Streaming idle watchdog (`AGENT_STREAM_IDLE_SECS`, default 600s) instead of total timeout;
  first-token progress notices ("⏳ ~Nk tokens, Ns elapsed"); exponential stream-drop backoff.
- `AGENT_TOOL_PROFILE=core` — 6 tool schemas (file ops + shell + search).
- Foreign skills (description, no `triggers:`) are **never always-on**: classified Practice
  with name-derived triggers. Always-on block budgeted at `skill_token_budget`. User warned
  when skills are dropped or the block exceeds ~2k tokens.
- Net: system prompt 34k → **3.2k tokens**, even with all 63 foreign skills mounted.

### Run 2 (zap v0.15.16): clean pass, 214s wall-clock including model load
```
Step 1: ✓ edit_file store.js   (88ms)   — remove(id) added + exported
Step 2: ✓ edit_file router.js  (2ms)    — DELETE /tasks/:id, 204/404
Step 3: ✓ edit_file test.js    (41ms)   — state-explicit assertions appended
Step 4: ✓ shell node test.js   (474ms)  — "all tests passed"
```
4 tool calls, zero failed edits, zero retries, zero `old_string not found` thrash. Objective
verification: suite green, `DELETE` returns 204 → task gone (404) → missing id 404, existing
assertions untouched.

### Answer to the open question
**Better than the hypothesis.** zap needed *code* changes, not for SLM capability but for
SLM *latency tolerance* (timeouts/retries) and *prompt hygiene* (skill/tool budget). Once the
plumbing respected local-model physics, zap's real harness matched the toy harness's pass —
with richer tools and zero scaffolding tweaks. The candidate fixes the handoff flagged
(edit-error hints, repeated-failure breaker) were **not needed** for this task: the model never
produced a failing tool call through zap's real tool implementations.

Caveat: `n = 1` task through the TUI; the Test-1/2 caveats (variance, bigger repos) still apply.
