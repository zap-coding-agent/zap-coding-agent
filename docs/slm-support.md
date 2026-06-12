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
| Verify + auto-escalate on failure | no | no | no | no | **planned** |

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
| Verify + escalate-on-failure | ⬜ roadmap |

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
local prefill (v0.15.15), a 3.2k-token prompt + 6-tool core profile (v0.15.15–16), and the
watchdog (v0.15.17). No other local-first agent ships a verification-aware breaker —
OpenHands' StuckDetector only catches *identical* repeated actions; a model trying different
broken fixes sails right through it.
