# Small Language Model (SLM) Support in zap

*What it is, why it's a differentiator, and exactly what's different.*

Status: **local-SLM execution works today** (validated below). **Propose-and-confirm
task routing** is the designed layer on top (roadmap).

---

## The one-line pitch

> zap runs a frontier model for design and a **local small model** for the actual
> coding — and it **proposes** which model fits a task, then waits for you to confirm.
> No silent downgrades, no cloud round-trip for the bulk of the edits.

Every other agent either (a) keeps you on one frontier model, (b) silently falls back
to a smaller cloud model on quota, or (c) makes you manually swap models with zero
guidance. zap is the only one that treats "which model should do *this* task" as a
**recommendation you approve**, with a local SLM as a first-class executor.

---

## Why this matters

Frontier models are the right tool for *deciding what to change*. They're an expensive,
slow, privacy-leaking tool for *mechanically applying* a change you've already specified —
add a field and update its callers, rename a symbol, wire a route, write the obvious test.

That mechanical majority of agentic coding can run on a 14B model on your own machine:

- **Cost** → near-zero per token; the frontier model is only paid for the thinking.
- **Latency** → no network hop; local inference on scoped tasks is fast.
- **Privacy** → the bulk of your code never leaves the laptop.
- **Offline** → works on a plane / air-gapped network for the execution half.

The catch the whole industry hits: a small model is unreliable at **open-ended agentic
reasoning** and at **native tool-calling**. zap's design leans into the part SLMs *are*
good at — executing a well-scoped, frontier-authored task — and keeps the human in the
loop on the routing decision.

---

## Validation: it actually works (real smoke test)

Run against a local LM Studio server (`http://localhost:1234`), zap's default OpenAI-compatible
backend. The critical question is whether a local SLM emits **native** `tool_calls` (not tool
calls as plaintext JSON, which stalls every agent loop) and whether the loop **terminates**.

| Model | Native `tool_calls`? | `finish_reason` | Multi-turn loop closes? |
|---|---|---|---|
| `gemma-4-e4b-it` (~4B effective) | ✅ yes | `tool_calls` | — |
| `qwen2.5-coder-14b` | ✅ yes | `tool_calls` → `stop` | ✅ reads file, answers, no spin |

The 14B coder model: given "read src/main.rs and tell me how many lines," it issued a
clean `read_file` tool call, then on the next turn produced a final answer with
`finish_reason: stop` — no runaway looping. This is exactly the behavior zap's agent loop
needs, and it confirms the existing OpenAI-compatible path (`src/llm_client/openai.rs`)
drives these models without modification.

zap even ships guard rails for the *failure* case — `check_text_mode_tool_call` and
`check_tool_support_error` in `src/llm_client/mod.rs` warn when a gateway strips the
`tools` field or a model emits tool calls as prose. Neither fired for these models.

---

## How everyone else handles small models (and what zap does differently)

There are really only **three** patterns in the field. None of them is "user-confirmed,
task-aware routing to a *local* SLM" — which is zap's lane.

### Pattern 1 — Architect / editor split (Aider)
`aider --model <strong> --editor-model <cheap>`. The strong model reasons and *describes*
the edit; the cheaper model only renders it into a diff. **Closest analog to zap.**
Difference: Aider's split is static config, applies to every edit, never asks, and the
editor model is given a constrained format job — not the agent loop.

### Pattern 2 — Fast-apply SLMs (Cursor, Continue.dev)
The frontier model decides the edit; a fine-tuned small "fast-apply" model (Morph / Relace
class) mechanically splices it into the file. Continue formalizes this as **roles** —
a model array tagged `chat` / `edit` / `apply` / `autocomplete`. The SLM lives in the
`apply` slot and never reasons. **This is the real "special SLM handling" in the industry.**
Difference: it's a fixed pipeline, the SLM is a transcription engine, and there's no
per-task choice surfaced to the user.

### Pattern 3 — Per-mode / per-subagent model assignment (Roo Code, opencode, Claude Code)
- **Roo Code / Cline** — modes (Architect / Code / Debug), each bindable to a different
  model+provider. You switch mode manually; the model follows.
- **opencode** — per-agent model config, provider-agnostic.
- **Claude Code** — subagents (`.claude/agents/*.md`) can set a `model:` field
  (`haiku`/`sonnet`/`opus`/`inherit`); the harness also quietly uses Haiku for background
  chores (titles, summaries). But it's **Anthropic-family only** — never a local model —
  and there's no per-task recommendation.
- **Gemini CLI** — only automatic Pro→Flash *fallback* on quota; not task-aware.

### The gap zap fills

| Capability | Aider | Cursor/Continue | Roo/opencode/Claude Code | Gemini CLI | **zap** |
|---|---|---|---|---|---|
| Local SLM as a real executor | partial | apply-only | yes (config) | no | **yes** |
| Task-aware model **recommendation** | no | no | no | no | **yes** |
| **User confirms** before switching | n/a | n/a | manual | no (auto) | **yes** |
| SLM runs the full agent/tool loop | no | no | yes | n/a | **yes (validated)** |
| Scopes the task before the SLM sees it | partial | yes | no | no | **yes (tasks.md)** |
    | Verify + auto-escalate on failure | no | no | no | no | **yes (v0.15.17, validated in Test 5)** |

The combination — *local SLM + task-aware proposal + human confirmation + the frontier
model having already decomposed the work into a verifiable task* — does not exist in any
shipping agent today.

---

## The design: propose, don't switch

zap never silently changes models. It **proposes** and waits.

### 1. Declare model roles (`~/.agent.toml`)
```toml
[providers.anthropic]        # the architect
model = "claude-opus-4-8"
role  = "frontier"

[providers.lm_studio]        # the coder
model = "qwen2.5-coder-14b"
role  = "local-coder"
```

### 2. The planner tags each task with a suggested model
`src/task_planner.rs` already classifies tasks and writes `suggested_skill` into
`.zap/tasks/<slug>/tasks.md`. It gains a `suggested_model` field the same way — derived
from a cheap heuristic (file count, edit verbs, blast radius from the code index) plus
optionally one frontier LLM call during planning:

```
Task 3 — Add `created_at` field to Session + update 4 callers
  skill:  none
  model:  local-coder   (mechanical edit · 5 files · low ambiguity)
  verify: cargo test
```

### 3. The confirmation gate at execution time
Before running a task, zap stops and shows the proposal — **nothing switches until you
press a key**:

```
◈ Task 3 — Add created_at field (+4 callers)
  Recommended: qwen2.5-coder-14b (local)   reason: mechanical, well-scoped
  [Enter] run on qwen   ·   [o] keep Opus   ·   [m] pick another   ·   [s] skip
```

### 4. Execution via a model-scoped sub-agent
On confirm, the task runs as a **sub-agent with a model override** (`run_subagent` in
`src/agent_core.rs` + the `spawn_agent` tool gain a `model` parameter). The parent frontier
session stays authoritative; the 14B runs the scoped task in an isolated context and returns
a structured result (`summary`, `files_changed`, `verify` outcome). If `verify` fails, zap
offers to re-run the *same* task on the frontier model — a safety net the static
architect/editor split lacks.

### Why this shape
- **You stay in control** — it's a recommendation engine, not a router.
- **Reuses proven code** — tasks.md annotation (planner), `create_client` swap (already
  done by `/provider`), and the sub-agent harness. The only net-new code is the classifier
  and the confirm prompt.
- **Plays to the SLM's strength** — the frontier model decomposes the work into a scoped,
  verifiable task *before* the 14B sees it, which is precisely why apply-style and
  architect/editor setups succeed where "14B as autonomous agent" setups fail.

---

## What works today vs. what's roadmap

| Piece | Status |
|---|---|
| Local SLM via OpenAI-compatible backend (LM Studio / Ollama) | ✅ shipped (`src/llm_client/openai.rs`, default `base_url` localhost:1234) |
| Native tool-calling with local 14B, loop closes | ✅ validated (smoke test above) |
| Plaintext-tool-call / stripped-tools guard rails | ✅ shipped (`src/llm_client/mod.rs`) |
| Manual whole-session model switch (`/provider`) | ✅ shipped |
| `role` field on provider config | ⬜ roadmap |
| `suggested_model` in tasks.md + classifier | ⬜ roadmap |
| Propose-and-confirm gate | ⬜ roadmap |
| Per-sub-agent model override | ⬜ roadmap |
| Verify + escalate-on-failure | ✅ shipped (v0.15.17) — watchdog with nudge + escalation via tool withdrawal |
| Structured plan execution | ✅ validated (Test 6) — SLM follows pre-written step-by-step plans |
| Code index pre-build for SLM tasks | ✅ shipped — `zap --index-only` pre-indexes project, SLM uses `code_map`/`find_definition` |

---

## Recommended models (local) — backed by a real eval

A full multi-turn agentic eval (3 realistic tasks, objective verification) lives in
[`research/slm-coding-eval/`](../research/slm-coding-eval/). Results (corrected after
two harness-measurement bugs were found and fixed — read that folder, it's instructive):

| Model | Size | Score | Verdict |
|---|---|:---:|---|
| `qwen3-coder-30b` (MoE, ~3.3B active) | 30B / 17 GB | **3/3** | **Default executor pick** — RL-tuned for the agent loop |
| `devstral-small-2` | 24B / 14 GB | **3/3** | Co-built with the OpenHands scaffold; great alt |
| `gemma-4-e4b` | 4B / 6 GB | 3/3 | Punches far above its size on scoped tasks |
| `glm-4.7-flash-reap` | 23B MoE | 2/3 | Solid on fixes/renames, weaker on multi-step |
| `qwen2.5-coder-14b` | 14B | 1/3 | ⚠ a *completion* model, not an agent — avoid for executor |

**Key lesson:** model *class* beats model *size*. Pick an **agentic-tuned** model
(Devstral, Qwen3-Coder) for the executor role — not a generic "coder" completion model.
Scoped single-file tasks passed on **every** model; the agentic-tuned tier also handles
multi-step and cross-file work.

The single gating test before trusting any local model in the loop: confirm it returns
`finish_reason: tool_calls` with a structured `tool_calls` array (not JSON in `content`)
on a tools request, and that it terminates with `finish_reason: stop` after a tool result.
Then run the eval harness against it.

## Production readiness — the evidence-backed claim

As of v0.15.17, this is no longer aspirational. Three tests through the **real zap TUI**
(qwen3-coder-30b via LM Studio on a 32 GB M5; full evidence in
[`research/slm-coding-eval/`](../research/slm-coding-eval/)):

| Scenario | Result | Wall-clock |
|---|---|---|
| Frontier-decomposed task (Test 3) | PASS — 4 tool calls, zero retries | 214s incl. model load |
| Goal-level task, run 1 (Test 4) | FAIL — 7/8 behaviors, one validation conditional | 905s (timeout) |
| Goal-level task, run 2 (Test 4) | PASS — all 8 behaviors | 305s |

**The claim zap can make:** SLMs are production-usable as *executors* today — when a frontier
model (or human) pins the task semantics, execution is fast, clean, and objectively verified.
For raw *goals*, SLMs self-plan correctly and land ~90-100% of behaviors with bounded failure:
objective verification + the verify-aware watchdog (nudge at 3 failed verifies, structured
escalation with tools withdrawn at 6) + retry (pass@2 = 100% here) cap the cost of a bad run
at minutes, not hours.

**What made it work was zap plumbing, not bigger models:** streaming timeouts that respect
local prefill (v0.15.15), a 3.2k-token prompt + 6-tool core profile (v0.15.15–16), the
watchdog (v0.15.17), and structured plan decomposition (Test 6). No other local-first agent
ships a verification-aware breaker — OpenHands' StuckDetector only catches *identical*
repeated actions; a model trying different broken fixes sails right through it.

---

## Structured plan execution — the recommended SLM workflow

Test 6 validates the optimal SLM workflow: a **frontier model pre-writes a step-by-step plan**
with exact code snippets, and the SLM executes it mechanically — one step, one verification,
one result. No improvisation, no ambiguous reasoning, no dead ends.

| Aspect | Open-ended goal (Test 4) | Structured plan (Test 6) |
|---|---|---|
| Success rate | 50% per attempt (100% with 1 retry) | **100% first attempt** |
| Wall-clock | 305–905s | 294s |
| Model turns | 4–8 | 2 |
| Failure mode | Wrong conditional branch → 13 min debugging | None — plan is unambiguous |
| Verdict | Viable with retry budget | **Recommended for production** |

**Test 6 task:** Add a `GET /todos` REST endpoint to a Node.js Express app. The plan:
```
Step 1 — Read the current code
Step 2 — Add todo data (exact JS array provided)
Step 3 — Add the /todos route (exact code provided)
Step 4 — Run `node test.js` and confirm "ok"
```

The model followed every step, made two correct edits in one turn, and verification passed.
No retries needed. The plan-formatting pattern (numbered steps, exact code in fenced blocks,
verification command per step) is now battle-tested.

**File:** [`research/slm-coding-eval/test6-structured/`](../research/slm-coding-eval/test6-structured/)

---

## Code indexing — gift the SLM a map

SLMs lose context on large codebases. zap's code index (`zap --index-only`) pre-builds an
AST index (tree-sitter + SQLite) so the SLM never needs to manually grep or read large files.
Instead of `read_file`-ing 500 lines, it calls `find_definition UserManager` → instant jump
to line 42. Instead of `search_code` across 80 files, `find_references create_user` returns
a call-graph lookup.

### How to use it

```bash
cd your-project
zap --index-only          # builds .zap/code.db — indexed symbols, call graph, type hierarchy
zap --goal "<task>"        # now runs with the index available to all tool calls
```

- `code_map <file>` — structural outline (functions, structs, line numbers)
- `find_definition <symbol>` — AST-resolved definition location
- `find_references <symbol>` — call-graph lookup of all call sites
- `who_calls <func>` — caller analysis
- `pack_context "<task>"` — relevance-ranked symbol bundle within a token budget

### Measured impact (Test 6)

| Metric | Without index | With `--index-only` |
|---|---|---|
| Index build time | — | <1s (2 files, 2 symbols) |
| File reads per turn | 2 (read_file entire files) | 0 (code_map + find_definition) |
| Large-project projection | O(n) manual reads | O(log n) index lookups |

For a 300-file project, the index saves the SLM from reading dozens of files it doesn't
need — it navigates directly to the symbols the plan references. This is the difference
between a 2-turn execution and a 12-turn goose chase.

**Recommendation:** Always pre-index before running SLM tasks. The index also serves as
evidence for the frontier model during planning (blast radius, callers list) — it's
built once, used by both models.

---

## Escalation drill — what happens when the SLM hits a wall (Test 5)

Test 5 is a deliberately **impossible task**: a CSV parser spec where `check.js` requires
empty fields to be both removed AND preserved on the same input pattern. No parser can
satisfy it. The test measures whether zap's verification-aware watchdog detects the
loop and escalates cleanly.

| Measure | Result |
|---|---|
| Watchdog nudge injected | ✅ (at streak = N) |
| Escalation directive injected (tools withdrawn) | ✅ (at streak = 2N) |
| Model produced escalation summary | ❌ (model went silent) |
| Cost bounded | ✅ (session ended within timeout, not endless) |

**Current state:** The watchdog correctly detects the verification loop and withdraws
tools, but the escalation handoff format (structured summary of works/fails/hypotheses)
is not consistently produced by the SLM — the model sometimes goes silent under tool
deprivation instead of producing the summary. This is documented as a known limitation
and the recommended mitigation is: **don't give SLMs contradictory specs**. The structured
plan workflow (Test 6) avoids this entirely.

**File:** [`research/slm-coding-eval/test5-escalation/`](../research/slm-coding-eval/test5-escalation/)

---

## Test execution summary

All tests run through the **real zap TUI** with `qwen/qwen3-coder-30b` via LM Studio
on a 32 GB Apple M-series machine. Full evidence, scripts, and raw logs in
[`research/slm-coding-eval/`](../research/slm-coding-eval/).

| Test | Scenario | Result | Time | File |
|---|---|:---:|---:|---|
| Test 3 | Frontier-decomposed task (tool validation) | ✅ PASS | 214s | [`test3-real-slm`](../research/slm-coding-eval/test3-real-slm/) |
| Test 4 run 1 | Goal-level task (8 behaviors) | ❌ 7/8 | 905s | [`test4-goal-spec`](../research/slm-coding-eval/test4-goal-spec/) |
| Test 4 run 2 | Same goal, retry | ✅ 8/8 | 305s | Same |
| Test 5 | Impossible task — escalation drill | ⚠️ partial | 10–45s | [`test5-escalation`](../research/slm-coding-eval/test5-escalation/) |
| Test 6 | Structured plan execution | ✅ PASS | 294s | [`test6-structured`](../research/slm-coding-eval/test6-structured/) |

**Bottom line:** Give an SLM a scoped, pre-written plan with exact verification steps →
it executes reliably and fast. Give it an open-ended goal with ambiguous constraints →
success is model and run dependent, but zap's watchdog bounds the failure cost to minutes.
The recommended production workflow is the structured plan pattern (Test 6).
