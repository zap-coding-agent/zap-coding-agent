# zap — Engineering Review & Competitive Assessment

**Reviewer:** Claude (Opus 4.8)
**Date:** 2026-06-10
**Version reviewed:** 0.15.2 (`Cargo.toml`)
**Method:** Source-code read of `src/**` (105 files, ~29.4k LOC). Verified by a clean `cargo build --release` and `cargo test --lib` run. **Deliberately ignored** the marketing/spec markdown in the repo (README, FEATURES.md, COMPARISON.md, etc.) per request — every claim below is traced to code I read.

---

## TL;DR

zap is a **genuinely well-engineered Rust coding agent** — not a toy. The architecture is clean, the abstractions are right, the build is green, and the unit-test discipline on pure logic is real (159 passing). It has three things most hobby agents lack: a **persistent tree-sitter code graph**, a **skill-first context-injection model**, and **provider-agnostic LLM plumbing** (Anthropic, any OpenAI-compatible endpoint, local LM Studio, and the Claude CLI as a subprocess).

It is **not yet "world class"** in the sense Claude Code / Gemini CLI are, and the gap is mostly *not* in features — it's in **measurement, hardening, and real-world soak time**. There are no evals, the safety layer is a denylist that gives false confidence, prompt caching leaves money on the table, and several core behaviors lean on brittle keyword heuristics that can silently drop context.

**Overall: 7.0 / 10** as an engineering artifact. The bones are excellent; the proof and the polish are missing.

---

## What I actually verified

| Check | Result |
|---|---|
| `cargo build --release` | ✅ Clean, 16s, optimized binary **26 MB**, zero warnings in final link |
| `cargo test --lib` | ✅ **159 passed, 0 failed** in ~1s |
| Integration/e2e tests | ⚠️ 7 exist but all `#[ignore]` (require live API key) — not run in CI by default |
| Source size | 105 `.rs` files, **29,364 LOC** |
| Code graph | `.zap/code.db`: **4,337 symbols across 279 files** indexed |
| `unsafe` blocks | 5 (all in credential downcast test helpers + minor FFI) |
| `unwrap/expect/panic` outside tests | **77** — a real panic surface (see Weaknesses) |
| Languages parsed | Rust, Python, JS, TS, Go, Java, C# (7 tree-sitter grammars) |

---

## Architecture — the strong core

The layering is correct and the file `agent_core.rs` is honestly described as "intentionally thin." Five entry modes share one `Session`: single-shot (`run`), REPL (`run_repl`), TUI (`run_tui`), headless NDJSON SDK (`run_sdk`), and recursive sub-agent (`run_subagent`). That's a mature surface.

**Things done right:**

- **`LlmProvider` trait** (`llm_client/mod.rs`) cleanly abstracts Anthropic, OpenAI-compatible, and the `claude` CLI subprocess behind one `send()`. URL normalization is tested for DeepSeek/Groq/Mistral/LM Studio/gateway shapes. Credential resolution supports static keys *and* `gcloud_adc` Bearer tokens — and there's a real test asserting `gcloud_adc` forces the `Authorization` header over `x-goog-api-key`. That's the kind of bug that bites people in production, and it's covered.
- **`Tool` trait + registry** (`tools/mod.rs`) with a small, well-chosen tool set. The `affected_path()` hook is elegant: the sub-agent summary and incremental re-indexing both derive changed files from the trait rather than hardcoding tool names.
- **MCP integration is clever.** Servers load *lazily*; instead of paying for every server's tool schemas up front, the registry injects a single synthetic `mcp_connect` tool whose description lists available servers + hints. The model connects on demand. This is a real token-efficiency idea and it's unit-tested.
- **Sub-agents** are full agent loops with depth limiting (`agent_depth`/`spawn_depth`), forced `Auto` permission mode (correct — they have no TTY), and parallel execution within a single response.
- **Anthropic SSE streaming** is hand-parsed correctly: per-index block accumulators, thinking/signature deltas, tool-use `input_json` deltas, usage + cache token accounting. Thinking blocks and DeepSeek `reasoning` blocks are both modeled.

**The differentiators (genuinely novel vs. the competition):**

1. **Persistent code graph.** `code_index/` builds a SQLite index via tree-sitter holding *symbols, type edges (hierarchy), call sites, and imports* — not just a symbol list. It exposes `find_definition`, `find_references`, `who_calls`, `find_subtypes/supertypes`, `find_by_return_type`, `where_imported`, and a `pack_context` bundler with a token budget. A background task re-indexes every 120s using `try_lock` (won't block the agent). **Claude Code does not maintain a persistent index** — it does agentic grep/glob each time. This is architecturally ahead.
2. **Skill-first context injection** (`skill_manager.rs`). Markdown skills with frontmatter (`trigger`, `category`, `tokens`) are matched per-turn against the user message, ranked, and **truncated to a token budget** before injection. Categories (Core = always on, Practice = always candidate, Domain = session-scoped) are a thoughtful taxonomy. The "inject only what this turn needs" thesis is the project's clearest original idea.
3. **Secret pre-flight** (`secret_scanner.rs`) scans content for ~25 key/credential patterns *before* it leaves for a cloud LLM.

---

## Context management — good ideas, brittle edges

`session/turn.rs` + `session/history.rs` implement: an 8-turn sliding window, tool-result pruning (results outside the last 2 exchanges collapse to a 150-char preview), LLM summarization of dropped turns (prepended as a synthetic exchange), and auto-compaction at 90% context fill with overflow-retry on API errors. The projected-skill-token calculation *before* the compaction check is a nice touch — it prevents the "looks like 75% but skills push it to 110%" overflow.

**But:** the whole pipeline pivots on two keyword heuristics in `session/casual.rs`:

- `is_casual_message()` — if true, the turn sends **zero tools** and **only the last message** (no history). A misclassification here means the agent answers blind. The guard is a hardcoded list of ~40 technical words and ~25 greetings. "Can you walk me through what we just did?" contains none of the technical keywords and is short — it could be treated as casual and lose all context. `needs_prior_context()` mitigates this but it's heuristic stacked on heuristic.
- `is_topic_shift()` — a significant-word-overlap < 15% test that prints a "consider /branch" nudge. Low stakes (it only suggests), but it's emblematic of the pattern: lots of behavior is governed by brittle string matching rather than the model's own judgment.

This is the single biggest *design* risk. The competition increasingly lets the model decide what context it needs (agentic retrieval); zap front-runs that decision with regexes.

---

## Cost & efficiency — the clearest concrete win available

`anthropic.rs` sets `cache_control: ephemeral` on the **system prompt** and the **last tool definition**. That caches the system+tools prefix — good. But it does **not** place a cache breakpoint on the conversation history. Anthropic allows up to 4 breakpoints; adding one on the last message of the prior turn would let multi-turn sessions reuse the entire conversation prefix instead of re-billing it every turn. On a 30-turn coding session this is a large, free input-token saving. **This is the highest-ROI fix in the codebase.**

Other efficiency notes:
- For OpenAI-compatible providers, `select_tools_for_turn` gates `web_fetch`/`web_search` behind keyword detection so small models aren't bloated — sensible.
- `MAX_TOKENS = 16_000` output cap is hardcoded in `anthropic.rs`. Fine today, but it should be model-aware (Opus/Sonnet support far more) and configurable.

---

## Safety — competent UX, weak as a real boundary

`permission_manager.rs` is well-modeled: Auto/Ask/Deny modes, per-session grant classes (granting `edit_file` also grants `write_file`/`batch_edit`/`undo_edit` but **not** `shell` — tested), batched prompts, and a TUI-native async prompt that doesn't block the tokio runtime. Snapshots are saved before every edit (`snapshot::save_snapshot`). Good.

The `shell.rs` guard is the weak spot. `BLOCKED_PATTERNS` and `DESTRUCTIVE_PATTERNS` are **case-insensitive substring matches**. They catch the obvious (`rm -rf /`, fork bombs, pipe-to-shell, reverse shells) and that has real value as a guardrail against a confused model. But as a *security* control it is trivially bypassable — `rm  -rf  /` (double space), `rm -rf "/"`, `X=/; rm -rf $X`, or any base64/eval indirection slips through. There is **no sandboxing, no containerization, no path jail on `shell`** (only `list_directory` enforces a cwd boundary). The danger is that the denylist *reads* like a security layer and may breed overconfidence. It should be documented as "footgun prevention, not a sandbox," and the real isolation story (container/VM/seccomp) should be on the roadmap.

---

## Correctness nits found in the read

- **`batch_edit` replacement counting** (`tools/file/edit.rs`): validation counts occurrences against the *original*, but application re-`matches()` against the progressively-mutated `content` and sums those into `total_replacements`. If an earlier edit's `new_string` happens to introduce text matching a later edit's `old_string`, the count (and the applied result) can drift from intent. The report string would also misreport. Low-probability, but it's a real ordering hazard; validating against the running buffer would be safer.
- **77 `unwrap()/expect()` in non-test code.** Many are mutex locks (`.lock().unwrap()`) that only panic on poisoning, but a poisoned mutex in the global code index or TUI channel would take the process down. A coding agent should degrade, not panic.
- **`edit_file` matching** is exact-string + a leading-whitespace fallback. That's deliberate and safe (refuses ambiguous matches), but it's less forgiving than Claude Code's approach on large, drifting refactors — expect more "old_string not found" retries on big edits.
- **Single global `OnceLock<Mutex<CodeIndex>>`.** Correct for one session; the mutex is held across SQLite calls. The background indexer correctly uses `try_lock`, but foreground graph queries serialize. Fine at current scale.

---

## Testing & verification — the credibility gap

159 fast, focused unit tests on pure logic (permissions, history windowing, URL/credential resolution, skill matching, shell guards, list_directory) — that's good hygiene and the tests are *meaningful*, not filler. **But:**

- The **agent loop, the LLM clients, and tool execution have no deterministic tests.** There's no mock `LlmProvider`, so the most important code path (`handle_user_turn`'s tool loop, compaction, summarization) is never exercised in CI. The 7 e2e tests are all `#[ignore]` behind a live key.
- There are **no evals.** No SWE-bench, no internal task suite, no regression harness for "did this change make the agent dumber?" For a tool whose goal is "world class," this is the missing foundation. You cannot claim coding quality you don't measure — and most of the perceived quality ceiling is the underlying model anyway, so the agent's job is to *not waste* the model's capability. That requires measurement.

---

## Competitive comparison

Scope note: zap's *agentic ceiling* is set by whatever model you point it at. The comparison below is about the **harness**, not raw model IQ.

| Dimension | **zap 0.15.2** | **Claude Code** | **Gemini CLI** | **OpenCode** |
|---|---|---|---|---|
| Language / runtime | Rust, single static binary, no runtime | Node/TS (needs Node) | Node/TS (needs Node) | Go + TS, single binary |
| Binary footprint | 26 MB, zero deps | npm install + Node | npm install + Node | single binary |
| Model support | **Any** — Anthropic, any OpenAI-compatible, local LM Studio, `claude` CLI, gcloud ADC | Anthropic only (+ Bedrock/Vertex) | Gemini only | **Any** provider |
| Code understanding | **Persistent tree-sitter graph** (symbols/types/calls/imports), 7 langs | Agentic grep/glob, no persistent index; very strong search + subagents | Relies on 1M-token context (reads whole files) | LSP + grep |
| Context strategy | Skill injection + windowing + LLM summarization + auto-compact | CLAUDE.md + agentic retrieval + auto-compact + huge context | Brute-force large context | AGENTS.md + agentic |
| Sub-agents | ✅ depth-limited, parallel | ✅ mature (Task tool) | Limited | ✅ |
| MCP | ✅ lazy-connect (token-smart) | ✅ first-class | ✅ | ✅ |
| Hooks | ✅ (session/prompt lifecycle) | ✅ extensive | Limited | ✅ |
| Permissions | Auto/Ask/Deny + grant classes | Granular + allowlist + plan mode | Basic | Granular |
| Sandboxing | ❌ denylist only | ❌ (permission-based) | Some | Some |
| Prompt caching | Partial (system+tools, **not history**) | ✅ full | N/A | Provider-dependent |
| Evals / benchmarks | ❌ none | ✅ SWE-bench-grade internal | ✅ | community |
| Maturity / soak | Solo project, early | Battle-tested at scale | Backed by Google | Active OSS community |
| TUI | ✅ ratatui | TUI + IDE + web + desktop | CLI | ✅ strong TUI |

**Where zap genuinely leads:** footprint (Rust single binary), model flexibility (ties OpenCode, beats CC/Gemini), and the persistent code graph (beats all three architecturally — none maintain a queryable symbol/call/type DB). The lazy-MCP and skill-budget ideas are also ahead of the pack on token economy.

**Where zap trails:** real-world hardening, evals, the depth of Claude Code's harness (plan mode, IDE integrations, the sheer number of edge cases handled), prompt-caching completeness, and ecosystem/distribution. Claude Code's advantage isn't a feature list — it's thousands of handled edge cases and a measured quality bar. That only comes from soak time + evals, both of which zap can build.

---

## Roadmap to "world class"

Ordered by impact-to-effort.

**Tier 1 — do these first (cheap, high impact)**
1. **Full prompt caching.** Add a cache breakpoint on the last message of the prior turn. Biggest free cost win in the codebase.
2. **Build an eval harness.** Even 30–50 internal tasks with pass/fail + token/cost tracking, run on every change. You cannot tune toward "world class" without a number. This is the most important item overall.
3. **Mock `LlmProvider` + agent-loop tests.** Make `handle_user_turn`, compaction, and summarization deterministically testable. Right now the heart is untested.
4. **Model-aware `max_tokens`** instead of the hardcoded 16k.

**Tier 2 — reduce the brittleness**
5. **Replace keyword heuristics with model judgment** where the cost of being wrong is high (the casual/no-history path especially). Let a cheap classifier or the model itself decide, or at minimum make the casual path *additive-safe* (never drop history it might need).
6. **Audit the 77 panics.** Convert mutex/IO `unwrap`s in the agent loop, code index, and TUI channel to graceful degradation.
7. **Fix `batch_edit` count/ordering** to validate against the running buffer.

**Tier 3 — close the maturity gap**
8. **Real sandboxing for `shell`** (container/seccomp/path jail) and re-label the denylist honestly as footgun-prevention.
9. **Structured long-term memory** beyond a single summarization blob — track file-state/edit history across the window so long sessions don't lose what was changed earlier.
10. **Documentation diet.** The repo carries enormous overlapping markdown (FEATURES.md alone is ~118 KB, README ~76 KB, plus COMPARISON/GAPS/VISION/PHASE1/etc.). Much is aspirational and will drift from the code. Collapse to one accurate README + one ARCHITECTURE doc generated/checked against source.

---

## Bottom line

zap is the work of someone who understands systems engineering: the trait boundaries are clean, the code graph is a real differentiator, the build is green, and the test discipline on pure logic is honest. As a **harness**, the feature set already rivals the majors on paper and beats them on footprint and model flexibility.

The distance to "world class" is not more features — it's **proof and hardening**: evals to measure quality, deterministic tests on the agent loop, full prompt caching, real sandboxing, and replacing brittle keyword gates with model judgment. Ship Tier 1, and zap goes from "impressive solo project" to "credibly competitive." Ship all three tiers, and the persistent-code-graph + skill-injection thesis could be a real edge that the Node-based incumbents can't easily copy.

*Reviewed strictly from source. No repository markdown was used as evidence.*
