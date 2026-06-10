# zap → World-Class: Execution Plan

**Author:** Claude (Opus 4.8)
**Date:** 2026-06-10
**Companion to:** `docs/opus-4.8-review.md`
**Audience:** This plan is written to be **executed by another AI model with no prior context**. Every task is self-contained: it states where the code lives, what to change, and how to prove it works. Do the tasks in order within a tier; tiers are ordered by impact-to-effort.

---

## How to use this document (read first)

- **Codebase:** `zap` — a Rust TUI/CLI AI coding agent. Binary name `zap`, package `zap-coding-agent`. Entry: `src/main.rs`. ~29k LOC across `src/**`.
- **Before you start any task:** read the files named in that task. Do **not** trust other `.md` files in this repo (README, FEATURES.md, etc. are marketing and may be stale) — trust the source.
- **After every task:** run `cargo build --release` and `cargo test --lib`. Both must stay green. The baseline is **159 lib tests passing, zero warnings**.
- **Commit discipline:** one task = one commit. Branch off `main`. Do not push or open PRs unless explicitly asked. End commit messages with the project's co-author trailer if one is configured.
- **Definition of done per task:** code compiles, all existing tests pass, new tests added pass, and the task's stated Acceptance criteria are met.
- **Scope discipline:** implement exactly what the task says. Do not refactor unrelated code. If a task is blocked, stop and report rather than guessing.

A progress checklist is at the bottom — update it as you complete tasks.

---

# TIER 1 — Foundations (do these first; cheap, highest impact)

These four turn zap from "untestable and unmeasured" into "tunable." Without an eval harness and deterministic agent-loop tests, no later quality work can be verified.

---

## Task 1.1 — Full Anthropic prompt caching (history breakpoint)

**Goal:** Reuse the conversation prefix across turns to cut input-token cost on multi-turn sessions.

**Why:** Currently only the system prompt and the last tool definition carry `cache_control: ephemeral`. The conversation history is re-billed in full every turn. Anthropic supports up to **4** cache breakpoints. Adding one on the last block of the last message lets the whole prior conversation be served from cache.

**Files:**
- `src/llm_client/anthropic.rs` — `encode_messages_anthropic()` (the function that maps internal `Message`s to Anthropic JSON) and the `send()` body where `system_blocks` and `cached_tools` already get `cache_control`.

**Steps:**
1. In `send()`, after `encode_messages_anthropic(messages)` produces the JSON array, add `"cache_control": {"type": "ephemeral"}` to the **last content block of the last message** — but only when that message is not empty. Do this on the encoded JSON, not the internal struct.
2. Anthropic counts a cache breakpoint per marked block. You will now have markers on: system (1), last tool (1), last message (1) = 3, within the 4-breakpoint limit. Do **not** add more.
3. Edge cases: if `messages` is empty, skip. If the last message's last block is an image, prefer marking the message's last text block if present; otherwise mark the last block regardless (Anthropic allows it on any block type).
4. Leave the `disable_stream` path and OpenAI client untouched (OpenAI-compatible caching is provider-specific and out of scope here).

**Acceptance:**
- `cargo build --release` clean.
- Add a unit test in `anthropic.rs` (`#[cfg(test)]`) that calls `encode_messages_anthropic` + your new marking logic on a 3-message history and asserts exactly one message carries `cache_control` and it is the last one. (Extract the marking into a small testable helper, e.g. `mark_last_message_cacheable(&mut Vec<Value>)`.)
- Manually verify against the existing request logging: the request written to `~/.zap/llm.log` (via `crate::log::write_llm`) shows `cache_control` on the final message.

**Risk:** Low. If a gateway rejects multiple breakpoints, the existing error path surfaces it; gate the history marker behind `!self.bearer_auth` is **not** required — real Anthropic supports it. Leave it on for all Anthropic requests.

---

## Task 1.2 — Mock `LlmProvider` for deterministic agent-loop tests

**Goal:** Make the core agent loop testable without a live API key.

**Why:** Today the heart of the system — `Session::handle_user_turn` (the tool loop, compaction, summarization) — has **zero** automated coverage. All 7 e2e tests are `#[ignore]` behind a live key. This task is a prerequisite for safely changing any agent behavior (Tasks 2.1, 3.2).

**Files:**
- `src/llm_client/mod.rs` — defines `pub trait LlmProvider` with `async fn send(...) -> Result<ApiResponse>` and the shared types (`Message`, `ContentBlock`, `ApiResponse`, `Usage`).
- `src/session/mod.rs` — `Session` struct; find where `self.client: Box<dyn LlmProvider>` is constructed (via `create_client`).

**Steps:**
1. Create `src/llm_client/mock.rs`. Implement `pub struct MockClient { responses: Mutex<VecDeque<ApiResponse>>, pub calls: Mutex<Vec<RecordedCall>> }` where `RecordedCall` captures `{ system, messages, tools }` (clone what you need). Implement `LlmProvider for MockClient`: each `send()` pops the next scripted `ApiResponse`; if the queue is empty, return a default end-turn text response. Record the call.
2. Add a constructor `MockClient::with_script(Vec<ApiResponse>)` and helpers to build common `ApiResponse`s: `text(&str)`, `tool_call(id, name, input_json)`.
3. Expose a test-only seam on `Session` to inject a client. Cleanest option: add `pub(crate) fn new_with_client(config: &Config, client: Box<dyn LlmProvider>) -> Result<Session>` that mirrors `Session::new` but takes the client instead of calling `create_client`. Keep `Session::new` delegating to it.
4. Add `mod mock;` under `#[cfg(test)]` or `pub(crate) mod mock;` in `llm_client/mod.rs` so tests can reach it.

**Acceptance:**
- New test module (e.g. `src/session/turn.rs` `#[cfg(test)]` or a new `tests/agent_loop.rs`) proving:
  - **Single text turn:** script one text response → `handle_user_turn` appends one assistant message, makes exactly one `send()` call, no tool execution.
  - **One tool round:** script `[tool_call(read_file...), text("done")]` against a temp file → loop executes the tool, sends the result back, then terminates. Assert 2 `send()` calls and the file was read.
  - **Loop bound:** script responses that always request a tool → loop stops at `MAX_TURNS` without hanging.
- `cargo test --lib` green, no live key needed.

**Risk:** Medium — `Session::new` may do real I/O (DB open, index load). Use a temp dir as cwd in tests and `PermissionMode::Auto`. If `Session::new` is hard to construct in tests, that itself is a finding — minimize the seam to just the client and stub/disable the code-index + persistence side effects via the existing config flags or env vars (`DISABLE_COMPACT`, etc.).

---

## Task 1.3 — Eval harness (the most important task in this document)

**Goal:** A repeatable way to measure "is the agent getting better or worse" across changes, with pass/fail + token/cost per task.

**Why:** You cannot tune toward "world class" without a number. This is the foundation that makes every other quality claim verifiable.

**Files (new):**
- `evals/` (new top-level dir)
- `evals/tasks/*.json` — task definitions
- `evals/runner.rs` or a small `evals/` binary, **or** a `tests/evals.rs` integration test gated by env var. Prefer a standalone binary target so it can run against real models without polluting `cargo test`.

**Steps:**
1. Define a task schema (JSON): `{ id, prompt, setup (shell to scaffold a temp repo), check (shell that exits 0 on success), timeout_secs, max_turns }`. Example task: "Add a function `add(a,b)` to `math.py` and a passing test" → `check` runs `pytest`.
2. Write 20–30 tasks spanning: file edit, multi-file refactor, bug fix from a failing test, search/navigation ("where is X defined"), shell/build, and a few "should refuse / no-op" negative tasks. Keep them small and deterministic.
3. Build a runner that, per task: creates a temp dir, runs `setup`, invokes `zap --sdk --auto` (reuse the pattern in `tests/sdk_e2e.rs`, which already spawns the binary and pipes NDJSON), pipes the prompt, runs `check`, and records `{id, pass, turns, input_tokens, output_tokens, est_cost, wall_secs}`. The SDK output JSON already includes `turn` and `usage` — parse those.
4. Emit a summary: pass rate, total cost, total tokens, per-task table. Write results to `evals/results/<timestamp>.json` and print a markdown table.
5. Add a `README.md` in `evals/` documenting: how to run (`cargo run --bin evals -- --model <m>`), the env/keys required, and how to add a task. Make the model + provider configurable via flags so it can run against Anthropic, an OpenAI-compatible endpoint, or local LM Studio.

**Acceptance:**
- `cargo run --bin evals -- --help` works.
- A dry-run mode (`--list`) prints all tasks without calling an LLM.
- One full run against any configured model produces a results JSON + a printed pass-rate table. (The executor may not have keys — in that case, deliver the harness + tasks + a `--list` that works, and document the run command.)

**Risk:** Medium. Keep tasks hermetic (temp dirs, no network in `check`). Pin a turn/timeout budget so a misbehaving run can't hang CI.

---

## Task 1.4 — Model-aware output token cap + default model bump

**Goal:** Stop hardcoding a 16k output ceiling; correct a stale default.

**Files:**
- `src/llm_client/anthropic.rs` — `const MAX_TOKENS: u32 = 16_000;` and its uses in the request body + `thinking` budget clamp.
- `src/config.rs` — `default_model` match: `Provider::Anthropic => "claude-opus-4-7"` (stale; current latest is `claude-opus-4-8`).

**Steps:**
1. Add a helper `fn max_output_tokens(model: &str) -> u32` (in `anthropic.rs` or `session/history.rs` next to `model_context_limit`). Map known families to sensible caps (e.g. Claude Opus/Sonnet 4.x → 32_000 or the documented max; smaller/unknown → 16_000 or 8_192). Keep the `thinking_budget` clamp consistent (`effective_budget = thinking_budget.min(max_tokens - 1)`).
2. Replace the hardcoded `MAX_TOKENS` usages with the computed value from the request's model.
3. In `config.rs`, change the Anthropic default model from `claude-opus-4-7` to `claude-opus-4-8`.
4. **Verify the model id before committing** — read `docs/` is not allowed for facts, but the Claude API skill / `claude-api` reference is authoritative. If unsure of the exact max output tokens for a model, choose a conservative documented value and leave a `// TODO: confirm cap` comment rather than guessing high.

**Acceptance:**
- Unit test: `max_output_tokens("claude-opus-4-8") > 16_000` and `max_output_tokens("some-unknown-model")` returns the safe default.
- `cargo build --release` clean; tests green.

**Risk:** Low. Over-high caps can cause API 400s — stay within documented limits.

---

# TIER 2 — De-risk the agent (reduce brittleness, remove footguns)

Do these only after Task 1.2 (mock client) lands, so behavior changes are test-guarded.

---

## Task 2.1 — Make the "casual / no-history" path additive-safe

**Goal:** Stop the agent from ever answering blind due to a keyword misclassification.

**Why:** `session/casual.rs::is_casual_message()` is a hardcoded keyword list. When it returns true, the turn sends **zero tools** and **only the last message** (see `session/turn.rs`: `effective_tools = &[]` and `effective_msgs_owned = last message only`). A false positive means the model loses all conversation context and all tools. The fix is not to make the regex smarter — it's to make the failure mode harmless.

**Files:**
- `src/session/casual.rs` — `is_casual_message`, `needs_prior_context`, `is_action_confirmation`.
- `src/session/turn.rs` — the `is_casual` branch that zeroes tools and truncates history.

**Steps (pick the lighter option that satisfies Acceptance):**
- **Option A (preferred, minimal):** Keep casual detection for the *cosmetic* win (skip skill injection / show a fast reply), but change the casual branch in `turn.rs` to still include a **short tail of history** (e.g. last 2 exchanges via the existing `windowed_history` with a small window) instead of only the last message. Tools may stay empty for true greetings. This bounds the blast radius.
- **Option B:** Add a guard so casual classification is **suppressed whenever the message is a reply to a pending action** (`is_action_confirmation` already exists) or references prior context (`needs_prior_context`). Ensure both are wired into the `is_casual` computation in `turn.rs` (verify they actually gate it — currently `needs_prior_context` is consulted; confirm `is_action_confirmation` is too).

**Acceptance:**
- Using the mock client (Task 1.2): a session where turn 1 establishes context, then turn 2 is `"yes"` (confirmation) → the second `send()` call's `messages` argument **includes** the prior turns (assert via `MockClient.calls`).
- Unit tests in `casual.rs` for the gating logic (confirmation + context-reference suppress casual).
- No regression: greetings on turn 1 still take the fast path.

**Risk:** Medium. Don't over-correct into "never casual" — that defeats the token-saving purpose. Keep true cold greetings fast.

---

## Task 2.2 — Audit and remove panics from the hot path

**Goal:** A coding agent should degrade gracefully, never crash mid-session.

**Why:** ~77 `unwrap()/expect()` exist outside tests. Many are `.lock().unwrap()` on mutexes (global code index, TUI channel) — a poisoned lock panics the whole process.

**Files:** Sweep `src/**`. Prioritize, in order: `src/code_index/mod.rs` (global index locks), `src/tui/channel.rs`, `src/session/*.rs`, `src/tools/**`.

**Steps:**
1. Find candidates: `grep -rn "\.unwrap()\|\.expect(" src --include="*.rs" | grep -v "#\[cfg(test)\]"` then filter to non-test code.
2. For mutex locks: replace `.lock().unwrap()` with `.lock().ok()` + graceful fallback (the `global_*` accessors in `code_index/mod.rs` already use this `and_then(|g| g.lock().ok())` pattern — make the rest match it).
3. For I/O / parsing in the agent loop and tools: convert to `?` with `.context(...)` (anyhow) or a logged fallback. Never introduce a new panic.
4. Leave genuinely-infallible `unwrap`s (e.g. on a regex compiled from a string literal, or `count > 0` invariants already proven) but add a one-line comment explaining why it cannot fail.

**Acceptance:**
- The non-test panic count drops materially (target: cut mutex `unwrap`s to ~0). Record before/after counts in the commit message.
- `cargo build --release` clean; `cargo test --lib` green.

**Risk:** Low, but mechanical — change behavior only to "degrade," never to silently swallow errors the user needs to see.

---

## Task 2.3 — Fix `batch_edit` count/ordering hazard

**Goal:** Correct replacement counting and prevent cross-edit interference.

**Why:** In `src/tools/file/edit.rs`, `BatchEditTool::execute` validates each edit's occurrence count against the **original** content, but the apply loop re-`matches()` against the **progressively mutated** `content` and sums those into `total_replacements`. If edit N's `new_string` introduces text matching edit N+1's `old_string`, the applied result and the reported count drift from intent.

**Files:** `src/tools/file/edit.rs` — `BatchEditTool::execute` (apply loop).

**Steps:**
1. Compute `total_replacements` during the **validation** pass (against the original), not the apply pass — or track per-edit the count you intend to apply and sum those, so the count reflects intent.
2. Keep edits applied **in order** (current behavior), but make the report accurate. Optionally add a note to the tool result if a later edit's `old_string` was no longer found after earlier edits (currently `replacen`/`replace` would silently no-op).
3. Add a regression test: a 2-edit batch where edit 1's `new_string` contains edit 2's `old_string` — assert the final content and reported count match documented intent (edit 2 applies to original occurrences, not ones created by edit 1).

**Acceptance:** New unit test passes; existing edit tests stay green.

**Risk:** Low. This is a correctness tightening, not a behavior overhaul — be careful not to change the common (non-overlapping) case.

---

# TIER 3 — Close the maturity gap

Larger efforts. Sequence after Tiers 1–2.

---

## Task 3.1 — Real `shell` isolation (and honest labeling)

**Goal:** Move shell safety from "substring denylist" to an actual boundary, and stop the denylist from reading like security.

**Why:** `src/tools/shell.rs::guard_shell` is a case-insensitive substring denylist — trivially bypassable (`rm  -rf  /`, `rm -rf "/"`, `X=/; rm -rf $X`, base64/eval indirection). It is valuable as *footgun prevention* but is not a sandbox, and there is no path-jail on `shell` execution (only `list_directory` enforces a cwd boundary).

**Steps (scope to what the platform allows; this is a design+impl task):**
1. Keep the denylist but rename/comment it honestly as "confused-model guardrail, NOT a security sandbox."
2. Add real isolation behind a config flag (`sandbox = "off" | "workdir" | "container"`):
   - **workdir mode:** run commands in a restricted working directory; reject absolute paths outside cwd in arguments where detectable; set a minimal env.
   - **container mode (best):** when available, run `shell` inside a disposable container (Docker/Podman) mounting only the project dir. Detect availability at startup; fall back with a clear warning.
3. Document the threat model in a new `docs/SECURITY.md` (this is allowed — you're creating it, not trusting it): what is and isn't protected, and how to enable strong isolation.

**Acceptance:** Config flag parsed; `workdir` mode enforced and unit-tested for the path-rejection logic; container mode gated on availability with graceful fallback; `SECURITY.md` written.

**Risk:** High surface area / platform-dependent. Land `workdir` mode first; container mode can be a follow-up commit.

---

## Task 3.2 — Structured session memory beyond a single summary blob

**Goal:** Long sessions shouldn't lose track of what was edited earlier.

**Why:** Context management (`session/history.rs` + `session/summarizer.rs`) drops old turns and prepends one LLM-generated summary string. Edits made before the window can be forgotten. A structured "what changed this session" ledger would survive windowing.

**Steps:**
1. Maintain a session-level `edited_files: Map<path, {first_turn, last_turn, ops_count}>` updated from the `affected_path()` trait hook (already used in `agent_core.rs::run_subagent` and incremental indexing — reuse it).
2. Inject a compact "Files modified this session" block into the system/context on each non-casual turn (token-budgeted, e.g. ≤300 tokens), independent of the sliding window.
3. Ensure it's deduplicated and ordered (most-recently-touched first).

**Acceptance:** Mock-client test: edit file A on turn 1, run enough turns to slide turn 1 out of the window, then assert turn N's `send()` system/context still mentions file A as modified.

**Risk:** Medium. Keep the block small — it competes with the real context budget.

---

## Task 3.3 — Documentation diet

**Goal:** One accurate README + one ARCHITECTURE doc; retire drift-prone marketing files.

**Why:** The repo carries large overlapping markdown (FEATURES.md ~118 KB, README ~76 KB, plus COMPARISON/GAPS/VISION/PHASE1/IMPLEMENTED_FEATURES/TUI_ENHANCEMENTS/etc.). Much is aspirational and will diverge from code. (The review of this very codebase had to *ignore* all of it to stay accurate — that's the signal.)

**Steps:**
1. Write a fresh `ARCHITECTURE.md` derived **from the source** (module map: agent loop, llm_client, tools, code_index, skill_manager, session, tui). Keep it factual and ≤ ~600 lines.
2. Trim `README.md` to: what it is, install, quickstart, supported providers, config, and a link to ARCHITECTURE. Move everything else out.
3. Archive (move to `docs/archive/` or delete) the redundant/aspirational files. Do **not** delete anything that is referenced by code, build scripts, or the website without checking (`grep -rn "FEATURES.md" .` etc.).

**Acceptance:** README ≤ ~15 KB, ARCHITECTURE.md exists and matches the actual module layout, no build/script/website reference is broken. Get explicit human sign-off before deleting (vs. archiving) any file.

**Risk:** Low technically, but **destructive** — prefer `git mv` to `rm`, and confirm before permanent deletion.

---

## Suggested sequencing & effort

| Order | Task | Effort | Unlocks |
|---|---|---|---|
| 1 | 1.2 Mock LlmProvider | M | All later behavior changes |
| 2 | 1.3 Eval harness | M–L | Measurement of everything |
| 3 | 1.1 Full prompt caching | S | Immediate cost win |
| 4 | 1.4 Model-aware tokens + default bump | S | Correctness |
| 5 | 2.1 Casual path safety | M | Reliability |
| 6 | 2.2 Panic audit | M | Reliability |
| 7 | 2.3 batch_edit fix | S | Correctness |
| 8 | 3.1 Shell isolation | L | Security |
| 9 | 3.2 Structured memory | M | Long-session quality |
| 10 | 3.3 Docs diet | M | Maintainability |

*(S ≈ <½ day, M ≈ ~1 day, L ≈ multi-day for an executing agent.)*

> Note: 1.2 is listed before 1.1/1.3 in execution order even though it's "Task 1.2" — the mock client should land first so the eval harness and caching changes can be regression-tested. Tier numbers denote priority of *outcome*; the table above is the build order.

---

## Global acceptance — "world class" exit criteria

zap can credibly claim world-class harness quality when **all** hold:
- [ ] Eval harness runs in CI (or one command) and reports a pass rate + cost; a baseline number is recorded.
- [ ] The agent loop, compaction, and summarization have deterministic tests via the mock client.
- [ ] Multi-turn sessions reuse the conversation cache (verified in `~/.zap/llm.log`).
- [ ] No code path can answer with silently-dropped context due to a keyword misclassification.
- [ ] No mutex/IO panic in the hot path; the process degrades instead of crashing.
- [ ] `shell` has a real isolation mode and an honest `SECURITY.md`.
- [ ] One accurate README + ARCHITECTURE; no aspirational doc sprawl.
- [ ] Default model and token caps are current and model-aware.

---

## Progress checklist (update as you go)

```
TIER 1
[ ] 1.1 Full Anthropic prompt caching (history breakpoint)
[x] 1.2 Mock LlmProvider — partial (mock.rs + test_factory.rs exist; Session::new_with_client missing)
[x] 1.3 Eval harness (evals/) — done (15 tasks, runner, README)
[ ] 1.4 Model-aware max_tokens + default model bump

TIER 2
[x] 2.1 Casual/no-history path made additive-safe — done (needs_prior_context gate wired, unit tests)
[ ] 2.2 Panic audit — partial (code_index + tui/channel cleaned; 93 unwraps remain across other files)
[ ] 2.3 batch_edit count/ordering fix

TIER 3
[ ] 3.1 Real shell isolation + SECURITY.md
[ ] 3.2 Structured session memory (edited-files ledger)
[ ] 3.3 Documentation diet (README + ARCHITECTURE)
```

---

### Reminders for the executing model
- Build + `cargo test --lib` after every task; keep the 159-test baseline green.
- One task per commit; branch off `main`; don't push/PR unless asked.
- Don't trust repo `.md` files as fact — read the source.
- If a task's assumptions don't match the code you find, **stop and report** the discrepancy instead of improvising.
