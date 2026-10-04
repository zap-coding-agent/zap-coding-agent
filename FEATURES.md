# zap — Feature Registry

Living reference of what's implemented, where it lives, and what's planned.
Update this file whenever a feature ships or a plan changes — no code scanning needed.

---

## Implemented ✅

### feat(acp): `zap acp` — run zap inside Zed, JetBrains and VS Code (v0.16.0 – v0.16.1)

zap now speaks the [Agent Client Protocol](https://agentclientprotocol.com):
`zap acp` runs it as a native agent in any ACP client — Zed, JetBrains IDEs,
VS Code (via ACP extensions), Neovim, Emacs. Replies stream into the editor's
agent panel, tool calls show kind / file locations / diffs, permission prompts
use the editor's Allow / Always allow / Reject buttons, and threads can be
cancelled and resumed. Plan and design notes: `docs/roadmap/acp.md`.

**Protocol:** ACP v1 via the official `agent-client-protocol` crate (2.2, the
one Zed uses); clients offering the draft v2 are answered with v1. Methods:
`initialize`, `authenticate`, `session/new`, `session/load` (replays the stored
conversation), `session/prompt`, `session/cancel`, `session/set_mode`
(`ask` / `auto` / `read-only` ↔ `PermissionMode`). Prompts accept text, images
and `@`-mentions (embedded resources or links). Editor-configured stdio MCP
servers are added to the session's lazy MCP pool. A `terminal` auth method
("Set up zap" → the TUI's `/provider`) is offered only to clients that advertise
`auth.terminal`; with no provider configured, `session/new` returns
`auth_required`.

**Design:**
- `acp::stdio` — the protocol gets private close-on-exec dups of fd 0/1; fd 1 is
  pointed at stderr and fd 0 at the null device, so zap's ~500 `println!` sites
  and child processes (shell, LSP, MCP) can never corrupt the JSON-RPC stream.
- `acp::worker` — a dedicated thread (own multi-threaded runtime — the
  background indexer's `block_in_place` panics on a current-thread one) owns all
  `Session`s. It installs itself as the consumer of `tui::channel`, exactly as
  the TUI does, so no agent-loop changes were needed: `TuiEvent`s become
  `session/update`s, `PERM_REQUEST` becomes `session/request_permission`, and
  cancel drops the turn future (same as TUI Ctrl+C). The channel is
  process-global, so prompts are serialized per process.
- `acp::translate` — pure zap ⇄ ACP mapping (tool kinds, diffs from edit
  arguments, todo list → `plan`, prompt flattening, history replay, MCP config).
- `TuiEvent::ToolStart` gained an `input` field (raw tool arguments) for
  locations and diffs; `Config::no_provider_configured()` is now shared by TUI
  onboarding and ACP; `spawn_background_indexer` starts at most one indexer per
  directory per process (ACP opens a `Session` per editor thread).

**Slash commands (v0.16.1):** in the TUI these are handled by the front-end, not the agent
loop, so ACP needed its own routing (`acp::commands`). The command list is sent
as `available_commands_update` (built-ins + every skill as `/<skill>`), then:
- inline commands that already return text reuse `tui::commands::handle_inline`;
- CLI-style commands that `println!` run through `Session::handle_slash` with
  stdout captured (`acp::stdio::Capture` — fd 1 redirected to a temp file,
  polled every 25 ms, ANSI stripped, streamed into a fenced block) inside the
  same event/permission/cancel loop as a prompt turn;
- picker/wizard commands get argument-driven versions: `/init [languages]`,
  `/provider [name [model]]` (via new `Config::load_with_provider`), `/model`,
  `/sessions`, `/context`, `/diff`, `/tasks [session n]`, `/goal` (turn loop
  until `✓ DONE` or `--max`);
- `/schedule`, `/unschedule`, `/bg`, `/agents`, `/remote` answer "terminal only":
  they need agent-initiated turns, which ACP v1 has no way to express.
`/permissions` pushes a `current_mode_update` so the editor's mode selector stays
in sync. `session/list` exposes zap's saved sessions to the editor's history.

**Editor-specific behaviour:** a new thread starts with an empty conversation
(the TUI auto-resumes the previous one — hidden history must not leak across
editor threads); `.zap/context.md` is saved after every turn and `SessionEnd`
hooks fire on disconnect, since an editor never "exits" a session; a panic inside
a turn or command is caught and returned as an error instead of killing the
worker thread.

**Verification:** 21 unit tests (every emitted update shape round-trips through
the crate's typed structs), 24 e2e tests that drive the real binary over stdio
against a scripted fake OpenAI-compatible server in a temp HOME (handshake,
stdout isolation incl. a mutation check, streaming, tool call + permission +
diff + file written, reject, cancel within 5 s while the LLM hangs,
`auth_required`, set_mode, load across a process restart, command announcement,
inline / captured / terminal-only commands, `/init`, `/goal` to completion and
to its turn limit, `/provider`, `/model`, session list, fresh-thread history),
and the official **ACP TCK v1 suite: CONFORMANT** (21/21 mandatory, 44 passed,
0 failures).

**Files:** `src/acp/{mod,stdio,worker,translate,commands}.rs`, `src/cli.rs`, `src/lib.rs`, `src/tui/channel.rs`, `src/tui/app.rs`, `src/session/tools.rs`, `src/config/mod.rs`, `src/tui/startup.rs`, `src/tui/mod.rs`, `src/tui/provider_picker.rs`, `src/code_index/mod.rs`, `tests/acp_e2e.rs`, `tests/acp_commands_e2e.rs`, `tests/acp_support/mod.rs`, `docs/roadmap/acp.md`, `README.md`, `Cargo.toml`

---

### fix(deps): bump h2 to 0.4.16 for RUSTSEC-2026-0258 (v0.15.142 patch)

CI security audit (`cargo audit`) started failing after RustSec published
[RUSTSEC-2026-0258](https://rustsec.org/advisories/RUSTSEC-2026-0258) on
2026-08-17 — *"h2 unbounded empty DATA frames"* — against `h2 0.4.14`, a
transitive dependency pulled in via `hyper` (through `axum` and `reqwest`).
Fixed with a lockfile-only bump `h2 0.4.14 → 0.4.16` (`cargo update -p h2`);
semver-compatible, no code changes. The other audit lines (bincode/paste/lru)
are allow-listed unmaintained/unsound warnings and don't fail the build.

**Files:** `Cargo.lock`

---

### feat(tui): queue a follow-up request while a turn is running (v0.15.141 patch)

Requested in #10. While the agent is busy (waiting on the LLM or running tools),
you can now **type into the input box and press Enter to queue** a follow-up —
it fires automatically when the current turn ends, ahead of scheduler jobs.

The queue infrastructure already existed end-to-end — `App.queued_input`, the
Enter-while-busy branch in `handle_key`, the post-turn drain in the main loop,
the "⏎ queued — Esc to cancel" render, and Esc-to-cancel. The only missing wire
was that `run_normal_turn`'s in-turn event loop swallowed every keystroke except
Ctrl+C, permission-popup keys, and btw/Ctrl+B, so plain typing never reached
`handle_key`. Added an `else` branch that forwards **only** editing/submit keys
(Char without Ctrl, Backspace, Left/Right, Home/End, Enter, Esc) to `handle_key`;
Ctrl shortcuts, navigation, and heavier actions (pickers, paste, diff) keep their
existing mid-turn behavior and are intentionally not run inside the turn loop.
Added a faint "type to queue a follow-up" hint in the input box while busy so the
capability is discoverable. Single-slot for now (a second queued message
overwrites the first); this is distinct from **btw (Ctrl+B)**, which injects into
the *current* turn rather than queuing for the next.

**Files:** `src/tui/turn_handler.rs`, `src/tui/render/layout.rs`, `src/tui/input.rs` (tests)

---

### fix(provider): Custom (OpenAI-compatible) flow + Linux arm64 release (v0.15.140 patch)

Two reported issues.

**#9 — Custom provider `Other…` did nothing.** Selecting *Custom
(OpenAI-compatible)* jumped straight to a model list whose only entry was
`Other…`; pressing Enter on it filtered `Other…` out and fell back to the
first item — which was `Other…` again — so it "switched" to a provider with
model `"Other…"` and **no endpoint URL ever collected**. There was no way to
type a URL or a model name.

Fixed by giving `PendingProviderSwitch` two new steps — `picking_base_url`
and `typing_model` — so the Custom flow is: **endpoint URL** (normalized via
`normalize_openai_url`) → **API key** (Enter to skip for local servers like
Ollama/LM Studio) → live model list fetched from the endpoint's `/models`, or
a **free-text model name** when the endpoint lists none. `Other…` now opens
free-text model entry everywhere (including `/model`) instead of resolving to
the literal string. Existing-entry base_url is now updated on re-config too.

**#8 — no Linux arm64 build; install.sh looked silent.** Added a native
`aarch64-unknown-linux-gnu` matrix leg on GitHub's free `ubuntu-24.04-arm`
runner, publishing `zap-linux-arm64.tar.gz`. `install.sh` already had the
`aarch64|arm64` case and a "no asset → clear error" fallback, so once the
asset ships the one-liner install just works.

**Files:** `src/tui/actions.rs`, `src/tui/app.rs`, `src/tui/lifecycle.rs`, `src/tui/render/overlays.rs`, `src/tui/turn_handler.rs`, `.github/workflows/build.yml`, `README.md`

---

### fix(tui): pasting into the API key input box did nothing (v0.15.139 patch)

`InputAction::PasteText` only ever inserted pasted text into the main chat
input box, gated on `AppState::Idle` — it had no idea `app.api_key_input`
(the provider-switch key-entry overlay) exists, since that overlay isn't
tracked via `AppState` at all. Result: pasting a key while the overlay was
open (`app.state` is still `Idle` underneath it) silently landed in the
hidden chat input instead of the visible key field, so Cmd+V appeared to
do nothing.

Fixed by checking `app.api_key_input` first and appending the pasted text
(trimmed) to `pending.input` when the overlay is open, falling back to the
existing main-input behavior otherwise.

**Files:** `src/tui/actions.rs`

---

### feat(provider): add OpenCode Zen (Go plan) as a built-in OpenAI-compatible provider (v0.15.138 patch)

New preset entry `opencode_zen` alongside DeepSeek/Groq/Fireworks/etc. — same
`chat/completions`-shaped OpenAI-compatible transport, no new code path
needed. Endpoint `https://opencode.ai/zen/go/v1/chat/completions`, ten
models (`grok-4.5`, `glm-5.2`, `glm-5.1`, `kimi-k3`, `kimi-k2.7-code`,
`kimi-k2.6`, `deepseek-v4-pro`, `deepseek-v4-flash`, `mimo-v2.5`,
`mimo-v2.5-pro`), `needs_key: true`. Added to all three provider-list
definition sites (`tui/provider_picker.rs`, `tui/startup.rs`,
`session/commands/provider.rs`) to keep the picker, onboarding flow, and
`/provider` REPL command consistent — same three-site sync pattern as
v0.15.136.

The Zen "Go" plan's Anthropic-compatible models (MiniMax, Qwen) use a
different `/v1/messages` request/response shape that zap's `OpenAiClient`
doesn't parse — not added; would need a second client code path.

**Files:** `src/tui/provider_picker.rs`, `src/tui/startup.rs`, `src/session/commands/provider.rs`, `tests/provider_e2e.rs`

---

### fix(llm_client/claude_code): stop corrupting output when two text blocks arrive with no tool call between them (v0.15.137 patch)

Real bug, not a depth/style issue: `send()`'s "assistant" event handler treated
each event's text as a cumulative delta of one growing message, diffing
against a running `prev_len` and resetting only when the new text was
*shorter* than the last. Verified directly against the CLI (no
`--include-partial-messages` passed) that this assumption is wrong — every
block arrives already complete, never as a partial delta, even across
several events sharing the same `message.id` in one turn. When two
independent, complete text blocks happened to *not* satisfy the
`len() < prev_len` shortening check (e.g. a short "Let me check X" line
followed by a longer, unrelated one, no tool call between them), the old
code silently sliced off however many characters matched the first block's
length from the *second* block's own text and glued the remainder onto the
first with zero separator — producing exactly the glued-together,
missing-space output reported ("…defined.Let me check…").

Fixed by extracting a pure `text_blocks()` helper and treating every block
as a complete, standalone unit, separated by a blank line — no length
diffing at all. Added regression tests reproducing the exact corruption
(confirmed the old logic would have failed them).

**Files:** `src/llm_client/claude_code.rs`

---

### feat(provider): expand Claude/Claude Code model lists; add `auto` option (v0.15.136)

Provider picker and REPL `/provider` command now show a complete, consistent
model list for both Anthropic and Claude Code providers:
- `claude_code` gains `auto` as the first option (lets the `claude` CLI pick
  the best model for your plan) plus `claude-haiku-4-5-20251001`
- Session/REPL `provider.rs` was stale (opus-4-7, no fable-5) — synced to
  match the TUI lists (fable-5, sonnet-4-6, opus-4-8, haiku)
- All three definition sites (startup, picker, REPL) are now consistent

### fix(context_manager): inject domain map content, counter Claude Code's built-in terseness (v0.15.135 patch)

Two fixes prompted by "claude_code output feels basic/terse compared to
direct claude" — investigated and found the root cause is *not* a zap bug:
Claude Code's own built-in system prompt has an explicit "Output and tone"
section biasing toward terse replies ("a concise response is generally less
than 4 lines") — this is why Claude Code is terser than plain Claude in any
context, zap or bare terminal. Since zap can only append to that baked-in
prompt, not remove it, `build_claude_code_system_prompt` now explicitly asks
for fuller explanations to counteract it (new "Response Depth" section).

Also fixed a real, separate gap found while investigating: `.zap/understanding.md`'s
Domain Map section (business domains, dependency direction, cross-cutting
concerns from `/understand`) was never actually injected into *any* system
prompt — only a "you're missing one, run /understand" nudge fired, and only
when the map didn't exist. When one does exist, its content is now injected
as "## Codebase Domain Map" in both the claude_code prompt and the regular
API-provider prompt.

**Files:** `src/context_manager.rs`, `src/domain_map.rs`, `src/project.rs`

---

### feat(tui/quota): show all providers simultaneously, pre-populate Claude at startup (v0.15.134)

Quota sidebar now displays every provider that has reported usage independently
(HashMap keyed by provider name) instead of overwriting a single slot. Both
`quota (claude)` and `quota (codex)` appear at the same time when both have
data. Claude quota is pre-fetched at TUI startup via a background task so the
sidebar is populated immediately without waiting for the first turn.

**Files:** `src/tui/app.rs`, `src/tui/mod.rs`, `src/tui/render/layout.rs`

---

### fix(llm_client/claude_code): Ask mode falls back to Auto instead of stalling (v0.15.133 patch)

Verified directly against the installed `claude` CLI (2.1.177) that the
"live per-edit approval" idea scoped in v0.15.132's entry is a dead end for
a subprocess-driven integration: running with `--permission-mode default`
just silently denies the tool (visible in the `result` event's
`permission_denials`) and has the model say "please approve the permission
prompt" in plain text — there is no prompt to approve. The `control_request`/
`can_use_tool` protocol described in some Agent-SDK docs/blog posts requires
a `--permission-prompt-tool` flag that doesn't exist on this CLI at all
(`claude --help` confirms it) — that protocol belongs to the embedded
Python/TypeScript Agent SDK, not the standalone binary zap shells out to.

So `Ask` mode for `claude_code` now falls back to `Auto` (`bypassPermissions`)
with a one-time notice explaining why, instead of guaranteed-stalling on
`default`. `Deny` is unaffected — it maps to `plan` (read-only), which
doesn't need any interactive channel and works as intended.

**Files:** `src/llm_client/claude_code.rs`

---

### fix(llm_client/claude_code): lean system prompt, honest permission-mode mapping, proactive 5h/weekly usage warnings for Codex + Claude (v0.15.132 patch)

Root cause of `claude_code` provider output feeling worse than the bare
`claude` CLI: `ClaudeCodeClient::send` never forwarded zap's tool schemas to
the subprocess (the `_tools` param was unused — Claude Code always uses its
own built-in tools), yet it reused the exact same system prompt built for
API-driven providers, full of zap's own tool vocabulary (`code_map`,
`edit_file`, `batch_edit`, `find_references`, `spawn_agent`, `todo_write`)
that doesn't exist in that process. Fixed with a new
`context_manager::build_claude_code_system_prompt` — identity, ZAP.md/
understanding.md project context, agent memory, non-negotiable safety
rules, git status — with none of the tool-policy/navigation/sub-agent
sections written for tools Claude Code doesn't have.

Also fixed a real safety bug found alongside it: zap's `Ask` and `Deny`
permission modes both silently mapped to Claude Code's `acceptEdits`
(auto-accept every edit), so choosing either expecting gated/blocked edits
got silent auto-apply instead. Now `Auto`→`bypassPermissions`,
`Ask`→`default`, `Deny`→`plan` (read-only). `Ask` is not yet a live
per-edit approval popup in zap's TUI — that needs the subprocess's stdin
kept open for the whole turn (currently closed after one write) plus
bridging Claude Code's own permission-request stream-json events into the
existing `PermissionPromptRequest`/`take_perm_request` channel. Not started.

New `quota_watch` module warns proactively at 80% usage for both
subscription-based providers and feeds a live sidebar `quota` section:
- **Codex**: real response headers (`x-codex-primary-used-percent`,
  `x-codex-secondary-used-percent`), checked on every response.
- **Claude**: no official CLI flag/endpoint exists (anthropics/claude-code
  issues #20399, #38380, #44328 are open feature requests) — uses
  Anthropic's undocumented `https://api.anthropic.com/api/oauth/usage`
  endpoint instead (same data Claude Code's own official `statusLine`
  feature exposes as `rate_limits.five_hour`/`.seven_day`; confirmed
  working live with a real Claude Code OAuth token read from macOS
  Keychain / `~/.claude/.credentials.json`). Checked at the top of every
  `claude_code` turn, throttled to once per 5 minutes; every failure mode
  is swallowed silently since it's a best-effort side channel that must
  never block or break a real turn. This endpoint is unofficial and could
  change without notice.

**Files:** `src/context_manager.rs`, `src/session/mod.rs`,
`src/llm_client/claude_code.rs`, `src/llm_client/codex.rs`,
`src/llm_client/mod.rs`, `src/lib.rs`, `src/quota_watch.rs` (new),
`src/tui/app.rs`, `src/tui/channel.rs`, `src/tui/render/layout.rs`

---

### fix(config,session): project-local `.agent.toml`, broader coding-task keywords, `/new` routes through `StartNewSession` (v0.15.131 patch)

`config_path()` now checks `./.agent.toml` (current directory) before the
XDG (`~/.config/zap/agent.toml`) and legacy (`~/.agent.toml`) paths, so a
project can carry its own config without touching global settings.
`task_classifier::is_coding` recognizes more everyday phrasing ("fix ",
"update ", "change ", "edit ", "modify ", "patch ") so more requests route
to a coding-capable model instead of falling through to a general one.
`/new` now goes through the same `StartNewSession` action as other
session-reset paths instead of a separate inline history-clear handler,
keeping the "does /new clear the window" behavior consistent with fork/switch.

**Files:** `src/config/mod.rs`, `src/config/tests.rs`,
`src/session/task_classifier.rs`, `src/tui/commands/mod.rs`,
`src/tui/turn_handler.rs`

---

### feat(session/tui): background agents — `/bg`, `/agents` (list/view/kill) (v0.15.130 patch)

> ⚠ **Has known bugs — enabled with a caveat, not fully fixed.** The picker,
> `/help`, and `/bg`'s own ack notice all say "has known bugs": while a
> background agent is actively streaming, it can briefly affect the main
> session's display (see "Known issue" below). Functionally it works
> end-to-end (verified live against a real provider) — this is a UI-polish
> gap, not data loss or a hang.

Lets a user fire off independent tasks that run in the background inside the
current TUI session, each optionally on its own model, and monitor them
without blocking the main conversation. `/bg <goal> [--model <slug>]` spawns
a detached tokio task running its own `Session` (fresh history, not shared
with the main conversation); model selection falls back to the existing
`task_classifier` + `model_routes` lookup when `--model` is omitted.
`/agents` lists active agents (id, model, status, elapsed, goal); `/agents
view <id>` shows a running agent's elapsed time or a finished agent's
summary/files-changed/turn-count; `/agents kill <id>` aborts one in flight.
Capped by `max_background_agents` (default 5). Transcripts persist to the
normal `sessions`/`session_messages` tables (findable later via
`/sessions`), even though the live `/agents` registry is scoped to the TUI
process that spawned them.

Also fixes a latent bug found while building this: destructive shell
commands (`rm -rf`, `git push --force`, `DROP TABLE`, ...) under
`is_subagent = true` previously queued an interactive approval prompt that
could never be answered (no controlling terminal), hanging the turn
forever. They now auto-deny with a clear reason instead — this also fixes
the existing model-invoked `spawn_agent` tool, not just `/bg`.

**Files:** `src/session/background_agent.rs`, `src/agent_core.rs`,
`src/session/tools.rs`, `src/session/mod.rs`, `src/config/mod.rs`,
`src/tui/app.rs`, `src/tui/background_handler.rs`, `src/tui/turn_handler.rs`,
`src/tui/channel.rs`, `src/tui/commands/mod.rs`,
`src/session/commands/info.rs`, `tests/e2e/test_background_agents.sh`

Design spec: `docs/specs/2026-07-05-background-agents-design.md`
Implementation plan: `docs/superpowers/plans/2026-07-05-background-agents.md`

**Known issue found during manual live-provider verification (2026-07-06):**
a running background agent's own `Session::handle_user_turn()` streams its
LLM response chunks, cost/context updates, and turn-start events through the
same process-global `tui_send` channel the main foreground session uses.
Those intermediate per-turn `TuiEvent`s (`LlmChunk`, `CostUpdate`,
`ContextUpdate`) aren't tagged with the background agent's id and aren't
filtered — they get applied directly to the main `App`, so a `/bg` task's
reply text appears as a phantom chat bubble in the main transcript, the
main sidebar's turn/cost/context stats get overwritten with the background
agent's numbers, and — most importantly — the main `AppState` flips to
`Thinking` while a background agent streams, which queues any command the
user types at the main prompt until the background agent's stream ends.
This defeats "monitor without blocking the main conversation" for the
duration of the background agent's own turn (only the final
`BackgroundAgentDone` notice is correctly attributed/isolated). Needs a
follow-up fix — likely tagging or routing per-session `TuiEvent`s so only
the spawning/main session's events reach the live `App`.

---

### feat(tui): /bg and /agents reachable from the TUI (v0.15.127 patch)

Part of the in-progress `/bg` background-agents feature (see
`docs/superpowers/plans/2026-07-05-background-agents.md`). `/bg <goal>
[--model <slug>]` and `/agents` (list/`view <id>`/`kill <id>`) are now
wired into slash-command dispatch, the command picker, and `/help` — the
feature built across the last several commits is now actually usable from
a running `zap` TUI session.

**Files:** `src/tui/turn_handler.rs`, `src/tui/commands/mod.rs`,
`src/session/commands/info.rs`, `src/tui/background_handler.rs`

---

### feat(tui): /bg and /agents command handlers (v0.15.126 patch)

Part of the in-progress `/bg` background-agents feature (see
`docs/superpowers/plans/2026-07-05-background-agents.md`). New
`tui::background_handler` module: `/bg <goal> [--model <slug>]` spawns a
background agent (rejecting at `max_background_agents` concurrent
running), `/agents` lists them, `/agents view <id>` shows progress/result,
`/agents kill <id>` aborts a running one. Not yet wired to slash-command
dispatch — that lands in the next task.

**Files:** `src/tui/background_handler.rs`, `src/tui/mod.rs`

---

### feat(tui): App.background_agents registry + completion notice (v0.15.125 patch)

Part of the in-progress `/bg` background-agents feature (see
`docs/superpowers/plans/2026-07-05-background-agents.md`). `App` now
tracks spawned `/bg` agents (mirrors the existing `scheduled_jobs`
pattern); when one finishes, `apply_event` updates its status and appends
a one-line `✓`/`✗` completion notice to the transcript. Not yet reachable
from any command — `/bg`/`/agents` themselves land in later tasks.

**Files:** `src/tui/app.rs`

---

### feat(session): background_agent::spawn (v0.15.124 patch)

Part of the in-progress `/bg` background-agents feature (see
`docs/superpowers/plans/2026-07-05-background-agents.md`). Adds the
detached-tokio-task spawn function: builds an independent `Config`/
`Session` for a `/bg` goal (own model, `is_subagent`+`is_background_agent`
set so its transcript persists, `Auto` permission mode, propagated
recursion-depth caps), runs it to completion, and reports the outcome
over the existing `TuiEvent` channel. Not yet reachable from any command
— the `/bg` slash command itself lands in a later task.

**Files:** `src/session/background_agent.rs`

---

### feat(session): background_agent module foundation (v0.15.123 patch)

Part of the in-progress `/bg` background-agents feature (see
`docs/superpowers/plans/2026-07-05-background-agents.md`). New
`session::background_agent` module: `BackgroundAgent`/`BgStatus` registry
types, `resolve_bg_model` (explicit `--model` > `model_routes` >
default model — same lookup already used for in-session turn routing),
and `parse_bg_args` for `/bg <goal> [--model <slug>]`. Not wired to any
command yet.

**Files:** `src/session/background_agent.rs`, `src/session/mod.rs`

---

### feat(tui): TuiEvent::BackgroundAgentDone plumbing (v0.15.122 patch)

Part of the in-progress `/bg` background-agents feature (see
`docs/superpowers/plans/2026-07-05-background-agents.md`). Adds the event
type the detached background-agent task will use to report completion
back to the TUI; full handling lands in a follow-up task.

**Files:** `src/tui/channel.rs`, `src/tui/app.rs`

---

### refactor(agent_core): extract SubagentResult/extract_result (v0.15.121 patch)

Part of the in-progress `/bg` background-agents feature (see
`docs/superpowers/plans/2026-07-05-background-agents.md`). Pulls
`run_subagent`'s inline summary/turns/tool_calls/files_changed extraction
into a standalone `agent_core::extract_result()` function, unchanged in
behavior, so the upcoming `/bg` background-agent path can reuse it instead
of duplicating the logic.

**Files:** `src/agent_core.rs`, `src/session/agent_loop_tests.rs`

---

### fix(session): auto-deny destructive commands for unattended sub-agents (v0.15.120 patch)

Part of the in-progress `/bg` background-agents feature (see
`docs/superpowers/plans/2026-07-05-background-agents.md`). `run_subagent`
forces `PermissionMode::Auto` because sub-agents have no controlling
terminal to prompt for approval — but the destructive-command check
(`rm -rf`, `git push --force`, `DROP TABLE`, ...) queued an interactive
prompt regardless of permission mode, so a sub-agent hitting one would
hang forever waiting for input that could never arrive. Now returns an
immediate `blocked: ...` tool error instead, which the model can react to.

**Files:** `src/session/tools.rs`, `src/session/agent_loop_tests.rs`

---

### fix(session): persist background-agent sessions correctly (v0.15.119 patch)

Part of the in-progress `/bg` background-agents feature (see
`docs/superpowers/plans/2026-07-05-background-agents.md`). `Session::new`
previously zeroed `session_id` (skip SQLite persistence) for any
`is_subagent = true` config — correct for model-invoked `spawn_agent`
sub-agents, but would have silently discarded `/bg` background-agent
transcripts too, since they also need `is_subagent = true` for banner
suppression. `should_persist_session()` carves out the `is_background_agent`
exception. The call-site comment above the old check was also trimmed —
it stated the now-incomplete "subagents never persist" rationale.

**Files:** `src/session/mod.rs`, `src/session/agent_loop_tests.rs`

---

### feat(config): background-agent config fields (v0.15.117 patch)

Foundation for `/bg` background agents (in progress — see
`docs/superpowers/plans/2026-07-05-background-agents.md`): `is_background_agent`
distinguishes user-invoked `/bg` sub-sessions from the model-invoked
`spawn_agent` tool (so background-agent transcripts persist to SQLite while
plain sub-agents still don't), and `max_background_agents` caps concurrent
background agents (default 5). The test-only `Default for Config` impl moved
from `mod.rs` to `tests.rs` to keep `mod.rs` under the project's 600-line
pre-commit gate.

**Files:** `src/config/mod.rs`, `src/config/tests.rs`, `tests/provider_e2e.rs`

---

### feat(scheduler): persist jobs and make `daily HH:MM` explicit (v0.15.115 patch)

`/schedule 15:30 ...` now means a one-shot run at the next 15:30, matching normal user
expectation instead of silently acting like a daily recurring schedule. Explicit wall-clock
recurrence now requires `/schedule daily 15:30 ...`. Scheduled jobs are also persisted to
`.zap/scheduled_jobs.json`, reloaded when the TUI starts, and `/schedule list` now shows
whether each job is one-shot vs recurring, plus next run, last run, and fire count.

**Files:** `src/session/scheduler.rs`, `src/tui/schedule_handler.rs`, `src/tui/mod.rs`, `src/tui/app.rs`

---

### fix(tui): send topic-shift prompt immediately after branch fork (v0.15.113 patch)

In the TUI topic-shift confirmation, pressing `b` correctly created a forked branch but left
the drafted prompt sitting in the input box instead of actually sending it. That made the
flow feel broken and could trigger the same "new topic" prompt again on the next submit.
Now `b` behaves like "fork and send": zap switches to the new branch, echoes the user
message in the UI, queues it as pending input, and clears the draft box.

**Files:** `src/tui/actions.rs`

---

### fix(tui): update Claude provider fallbacks to include Fable and latest Opus (v0.15.113 patch)

The Claude model lists were only partially updated: onboarding/startup showed the new
Anthropic models, but the runtime provider picker still had stale fallback entries for
`anthropic` and `claude_code`. That meant users in Claude provider mode could still see old
options like `claude-opus-4-7` and miss `claude-fable-5`. This patch aligns the runtime
fallbacks with the startup picker, adds `anthropic/claude-fable-5` to the OpenRouter sample
list there as well, and preserves the updated Anthropic output-token cap handling for Fable.

**Files:** `src/tui/provider_picker.rs`, `src/tui/startup.rs`, `src/llm_client/anthropic.rs`

---

### feat(codex): support image input via Responses API multipart content (v0.15.111 patch)

Codex previously dropped every image block with a warning ("not yet supported"). Rewrote
`encode_input` to emit multipart `content` (`input_text` + `input_image` parts, images as
`data:<mime>;base64,...` URIs) when a user message has images, keeping the flat-string
format for text-only messages. Also fixed a second, independent bug uncovered while
testing this live: `provider_supports_vision`'s local-model heuristic reads
`config.base_url`, but Codex never sets a real one (it hardcodes its own endpoint) — so a
codex config commonly carries the LM Studio default
(`http://localhost:1234/...`), which the heuristic misread as "local model, check vision
support by name" and blocked images even after the encoding fix. Verified end-to-end
against the live Codex API (ChatGPT ⁠subscription) with `/attach` + a real screenshot —
model correctly identified the image content.

**Files:** `src/llm_client/codex.rs`, `src/llm_client/mod.rs`, `src/tui/commands/mod.rs`

---

### fix(config): hardcode Provider kind for built-in CLI-passthrough slugs (v0.15.110 patch)

`config.provider_slug == "claude_code"` resolved `config.provider` to `Provider::OpenAi`
whenever the TOML had no explicit `kind` for that slug (the fallback interpreted the slug
name itself, and "claude_code" doesn't match "anthropic"). This only "worked" because
`create_client()` special-cases the slug before ever consulting `config.provider` — but
every OTHER piece of code that reads `config.provider` (e.g. `provider_supports_vision`)
saw the wrong value, working only by coincidence. Extracted a `resolve_provider_kind`
function that hardcodes `claude_code` → Anthropic and `codex` → OpenAi regardless of TOML
state, with unit tests covering the regression.

**Files:** `src/config/mod.rs`, `src/config/tests.rs`

---

### fix(session): dedupe clipboard auto-attach by content hash (v0.15.109 patch)

Every turn, if the OS clipboard held an image, zap silently attached it — even if it was
the same screenshot from 10 turns ago that the user had long forgotten about. That meant
token waste and a privacy leak (an old screenshot riding along on unrelated messages).
Now hashes the clipboard image bytes and only auto-attaches when the hash differs from
the last auto-attached image, so a stale clipboard stops resending itself. Explicit
`/paste`, `/attach`, and Ctrl+V are untouched and always work regardless of the hash.

**Files:** `src/session/mod.rs`, `src/session/turn.rs`, `src/session/test_factory.rs`, `src/session/clipboard_paste.rs`

---

### fix(tui): mouse-wheel scroll off by default, native text selection always works (v0.15.108 patch)

Mouse reporting (needed for scroll-wheel support) was ON by default, which blocked the
terminal's native click-drag selection unless you knew to press Ctrl+T first — bad
discoverability, and it meant copy was broken out of the box. Flipped the default: mouse
reporting starts OFF, so click-drag selection just works immediately, matching Claude
Code / opencode / Codex. Ctrl+T is now the opt-in for mouse-wheel scrolling (at the cost
of needing Option/Alt+drag to select while it's on). Footer and in-app notices reworded
to match the new framing.

**Files:** `src/tui/mod.rs`, `src/tui/app.rs`, `src/tui/actions.rs`, `src/tui/input.rs`, `src/tui/render/layout.rs`

---

### fix(remote): reuse running ngrok agent, match tunnels by port, reap leaked agents (v0.15.107 patch)

`/remote` was returning 502s in practice because a leaked `ngrok` process from a prior
session (upstream port long dead) still owned the `:4040` API — the old code accepted
*any* https tunnel from that API without checking which port it pointed at. Rewrote
`launch_tunnel` to: (1) only accept a tunnel whose upstream `addr` matches our port,
(2) reuse an already-running agent via its HTTP API instead of spawning a second one
(ngrok's free plan allows one agent session), and (3) kill any agent this call spawned
if the tunnel never became reachable, so a failed attempt can't leak a process that
poisons the next one. Also fixed the web remote UI getting stuck on "busy" forever
after a slash command or errored turn (no `done` event was ever sent for those paths).

**Files:** `src/remote.rs`, `src/tui/mod.rs`

---

### fix(tui): `/attach` path normalization, pngpaste hint (v0.15.106 patch)

`/attach` failed on paths as terminals actually deliver them from drag-and-drop:
surrounding quotes, backslash-escaped spaces, and `~`. Now normalizes all three before
reading the file. The clipboard-paste failure message also suggests `brew install
pngpaste` when it's missing (macOS fast path; AppleScript fallback still works without it).

**Files:** `src/tui/commands/mod.rs`, `src/tui/lifecycle.rs`

---

### fix(tui): show more transcript and richer collapsed tool previews (v0.15.113 patch)

The TUI now reserves less vertical space for header/footer chrome so more transcript lines stay
visible during normal use. Collapsed tool cards no longer reduce to a single tiny summary line:
they now show up to three wrapped preview lines and keep the `Ctrl+O` expansion hint when more
content is available. Added PTY/tmux-backed E2E harnesses for both a local render smoke check and
a real-provider session capture.

**Files:** `src/tui/render/mod.rs`, `src/tui/render/messages.rs`, `tests/e2e/test_tui_render_evidence.sh`, `tests/e2e/test_tui_real_provider.sh`

---

### feat(tui): copy last reply (Ctrl+Y) and native text-selection mode (Ctrl+T) (v0.15.105 patch)

The TUI's mouse-reporting mode (needed for scroll-wheel support) silently blocked normal
click-drag text selection, and there was no way to copy a reply without it. Ctrl+Y copies
the last assistant reply to the system clipboard (OSC 52 fallback for SSH sessions).
Ctrl+T toggles mouse reporting off so the terminal's native selection takes over, with a
footer indicator (`▣ SELECT MODE`) so it's discoverable.

**Files:** `src/tui/actions.rs`, `src/tui/app.rs`, `src/tui/input.rs`, `src/tui/render/layout.rs`

---

### fix(claude_code): session continuity, permission mode, image passthrough (v0.15.104 patch)

The `claude_code` provider (routes through the local `claude` CLI) replayed the entire
conversation as separate stream-json user events on every turn — the CLI answers each
user event independently, so turn N re-answered every prior message before touching the
new one. It also never passed `--permission-mode`, so every file write was silently
denied with no visible error (stderr went to `/dev/null`). Rewrote to use `--resume
<session_id>` for continuity (first turn inlines prior history into one event),
`--permission-mode acceptEdits`/`bypassPermissions` (mapped from zap's own permission
mode), stderr capture surfaced on failure, and image content blocks now pass through as
base64 instead of being dropped.

**Files:** `src/llm_client/claude_code.rs`, `src/llm_client/mod.rs`

---

### fix(remote): stop returning flaky localhost.run `/remote` URLs that 502 (v0.15.103 patch)

`/remote` no longer silently falls back to `localhost.run` when ngrok is unavailable or not
ready. Instead, zap now requires a working ngrok tunnel and returns a clear setup error if
ngrok is missing, unauthenticated, or never becomes reachable end-to-end. This avoids
handing out public remote-control URLs that later fail with browser-side 502 errors.

**Files:** `src/remote.rs`

---

### fix(tui): restore `/schedule` and `/unschedule` in slash-command picker (v0.15.102 patch)

The TUI already implemented `/schedule` and `/unschedule`, but both commands were missing
from `SLASH_COMMANDS`, so they did not appear in slash completion. This patch restores them
in the picker and adds a built-in `slash-commands` skill to remind future command work to
update handler wiring, picker registration, docs, and verification together.

**Files:** `src/tui/commands/mod.rs`, `src/default_skills/slash-commands.md`, `src/skill_manager.rs`

---

### fix(remote): validate the public tunneled URL before showing `/remote` link (v0.15.101 patch)

Zap now probes the full public `/remote` URL, including the access token, before it
prints or copies the link. This catches cases where the local server is healthy but the
public tunnel still cannot reach the upstream localhost service, avoiding bad links that
fail immediately in the browser.

**Files:** `src/remote.rs`, `src/session/mod.rs`, `src/tui/commands/mod.rs`

---

### fix(remote): wait for local `/remote` server before publishing tunnel URL (v0.15.100 patch)

After creating the public tunnel, zap now probes the local remote-control web server
on `127.0.0.1` and only announces/copies the public URL once the upstream is actually
reachable. This avoids "failed to establish connection to upstream web service" errors
caused by exposing the tunnel before the local server was ready.

**Files:** `src/remote.rs`

---

### feat(remote): auto-copy generated `/remote` URL to clipboard (v0.15.99 patch)

When `/remote` successfully starts a public tunnel, zap now builds the final
remote-control URL including the token, copies it to the system clipboard, and
prints `Copied to clipboard.` alongside the existing secrecy warning. This avoids
manual selection/copy issues when the terminal makes the generated URL hard to grab.

Implemented in both the TUI slash-command path and the session command path.

**Files:** `src/remote.rs`, `src/session/mod.rs`, `src/tui/commands/mod.rs`

---

### fix(tui): save `.zap/context.md` before session switches (v0.15.97 patch)

Before TUI session-boundary transitions, zap now calls `session.save_context()` before
`LoadSession` and `StartNewSession`. This preserves the outgoing session's latest context in
`.zap/context.md` instead of leaving it stale until a later exit.

**Files:** `src/tui/actions.rs`

---

### feat(session/tui): per-turn model routing with TUI approval prompt

At the start of each turn, classifies the user's input via `task_classifier::classify()`.
If `config.model_routes` has an entry for the classified type and the routed model differs
from `session.model`, zap temporarily swaps the client/model for that turn only, restoring
the original at every return path.

In **CLI mode**: prints `  ◎ Routing coding task to codex/gpt-5.5 (model_routes)` before
sending to the LLM.

In **TUI mode**: the Submit handler intercepts before the turn starts and sets
`app.model_switch_confirm`. The status bar shows:

```
🔀 Route to codex/gpt-5.5 for coding task — [Enter] confirm  [n] use default  [Esc] cancel
```

`[n]` sets `session.skip_routing_once = true`, which turn.rs checks and clears, causing the
turn to run on the default model. `[Esc]` restores the text to the input box without sending.

E2E test: `tests/e2e/test_model_routing.sh` (T20a/T20b).
Website docs: `website/docs.html` — `[model_routes]` table row + config block example.

**Files:** `src/session/turn.rs`, `src/session/mod.rs`, `src/tui/app.rs`, `src/tui/actions.rs`,
`src/tui/input.rs`, `src/tui/render/layout.rs`, `tests/e2e/test_model_routing.sh`,
`website/docs.html`

---

### feat(session): keyword-heuristic task type classifier

Adds `src/session/task_classifier.rs` with a `TaskType` enum (Coding, Review, Explain,
Search, Default) and a `classify(input: &str) -> TaskType` function using keyword heuristics.
Priority order: Review > Explain > Search > Coding > Default. Zero allocations beyond the
lowercase copy — no LLM call. 6 unit tests cover each type and priority ordering.

**Files:** `src/session/task_classifier.rs`, `src/session/mod.rs`

---

### feat(config): add model_routes config for per-task model assignment

Adds `model_routes: HashMap<String, String>` to `FileConfig`, `Config`, and the
`Config::load` builder. Users can map task types to specific model slugs via a
`[model_routes]` table in `~/.agent.toml`:

```toml
[model_routes]
coding = "codex/gpt-5.5"
review = "claude-opus-4-7"
explain = "claude-sonnet-4-6"
search  = "gemma-4-e4b-it"
```

The field defaults to an empty map (`#[serde(default)]`) so existing configs
parse without changes. This is the foundation for Task 1 of the model-routing plan;
the routing lookup will be wired in subsequent tasks.

**Files:** `src/config/mod.rs`, `src/config/tests.rs`, `tests/provider_e2e.rs`

---

### feat(scheduler): /schedule and /unschedule entries in /help; e2e smoke tests (v0.15.96)

- v0.15.96: scheduler: /schedule and /unschedule entries in /help; e2e smoke tests

---

### feat(scheduler): show active job count in status bar (v0.15.95)

- v0.15.95: scheduler: show active scheduled job count in status bar

---

### feat(scheduler): drain scheduled_queue after each turn (v0.15.94)

- v0.15.94: scheduler: drain scheduled_queue after each turn (busy-queue support)

---

### feat(scheduler): /schedule and /unschedule TUI slash commands (v0.15.93)

- v0.15.93: scheduler: /schedule and /unschedule TUI slash commands

---

### feat(scheduler): ScheduledFire TuiEvent, App fields, apply_event handler (v0.15.92)

- v0.15.92: scheduler: TuiEvent::ScheduledFire, App::scheduled_jobs, App::scheduled_queue, apply_event handler

---

### feat(scheduler): in-session scheduler data model and parsers (v0.15.91)

`src/session/scheduler.rs` adds the `ScheduledJob` struct (name, goal, interval_str,
Tokio JoinHandle, fire_count), `parse_interval` (accepts "30s", "5m", "2h", "1h30m"),
`parse_wallclock` (strict "HH:MM" format), and `schedule_label` for display.

**Files:** `src/session/scheduler.rs`, `src/session/mod.rs`

---

### feat(tui): show active tool count in startup notice (v0.15.87)

At TUI startup, a notice line "Tools: N loaded — /tools to list all" is added to
`startup_notices` so users immediately see how many tools are active without typing `/tools`.
When `config.disabled_tools` is non-empty the notice reads "Tools: N active (M disabled) —
/tools to list". The count is derived from `tools.active_tool_names().len()`.

**Files:** `src/session/mod.rs`

---

### feat(tools): filter disabled_tools and disabled_skills from session (v0.15.86)

`ToolRegistry` gains two new methods: `tool_definitions_filtered(disabled: &[String])` which
returns tool definitions excluding any whose name appears in the disabled list, and
`active_tool_names()` which returns a sorted list of all registered tool names (built-in +
connected MCP). Both the session initialisation path (`Session::new`) and the `mcp_connect`
tool handler (which refreshes `self.tool_defs` after connecting a server) now call
`tool_definitions_filtered(&config.disabled_tools)` instead of `tool_definitions()`.

Skill injection in `turn.rs` calls `retain` after the match-skills + pinned-skills merge —
in both the projected-token calculation block and the actual `matched_skills` block — to drop
any skill whose name is in `config.disabled_skills`.

A new `/tools` slash command lists all built-in tools, connected MCP tools, pending MCP
servers, and disabled tools.

**Files:** `src/tools/mod.rs`, `src/session/mod.rs`, `src/session/tools.rs`,
`src/session/turn.rs`, `src/session/commands/info.rs`

### feat(tui): show current branch name in status bar when not on main (v0.15.84)

When the active git branch is not "main" (and not empty), the status bar now shows
`⎇ <branch-name>` in green bold text, making it immediately visible that the session
is running on a fork/feature branch. The branch is cached at startup and refreshed
after slash commands (existing behaviour from `App::branch`).

**Files:** `src/tui/render/layout.rs`

### feat(tui): replace status-bar topic-shift hint with full-width highlighted banner (v0.15.83)

When a topic shift is detected, a 3-line highlighted banner now appears above the input
box instead of a small hint in the status bar. The banner flashes on first appearance
(alternating bg via `topic_shift_flash` counter) and shows `[Enter]` / `[b]` / `[Esc]`
key hints with colour coding. The layout inserts a zero-height slot when the banner is
hidden so the rest of the UI is unaffected.

Update: pressing `[b]` now auto-creates a conversation fork like `fork-1`, saves it,
switches to that branch in-place, and keeps the drafted text in the TUI instead of
queueing a bare `/branch` command.

**Files:** `src/tui/app.rs`, `src/tui/actions.rs`, `src/tui/mod.rs`,
`src/tui/render/layout.rs`, `src/tui/render/mod.rs`

### fix(session): test production function + trim module to 600 lines (v0.15.81)

Unit test for `load_recent_whats_next` now calls the real function via a tempdir+chdir
fixture instead of re-implementing parsing inline. Ordering confirmed correct (log is
prepended newest-first, so `take(limit)` already returns most recent). Removed redundant
blank lines and collapsed two-line comments to stay within the 600-line hook limit.

Update: the understanding/stats helpers now live in `src/project/project_understanding.rs`,
which keeps `src/project.rs` under the line-limit hook while preserving behavior. The
source-module listing also now prefers directories before truncation so entries like
`session` are not dropped when many top-level modules exist.

**Files:** `src/project.rs`, `src/project/project_understanding.rs`

### feat: inject last 3 sessions what's-next into startup system prompt (v0.15.80)

At startup, `load_recent_whats_next(3)` scans `.zap/session_log.md` for `Next:` lines
from the most recent sessions and injects them as a `## Recent What's Next` block into
the system prompt. This means the LLM always knows what was planned even when starting
a fresh session or resuming after multiple sessions away.

**Files:** `src/project.rs` (new `load_recent_whats_next`; test now calls production fn via tempdir+chdir fixture), `src/session/mod.rs` (TUI + CLI injection)

### fix: wire context_window to Ollama num_ctx + deny.toml wit-bindgen skip (v0.15.77)

`num_ctx` for Ollama was hardcoded to 8192 regardless of `context_window` in the
provider config. Added `ollama_num_ctx` field to `OpenAiClient` and plumbed it from
`create_client` so a `context_window = 32768` entry in `agent.toml` actually takes
effect. Also suppressed the `cargo-deny` duplicate-version warning for `wit-bindgen`
(two versions pulled in by incompatible getrandom 0.3 / 0.4 major versions via wasip2
and wasip3 — unresolvable without upstream changes).

**Files:** `src/llm_client/openai.rs`, `src/llm_client/mod.rs`, `deny.toml`

### feat: SLM write_file literal-content rule (v0.15.76)

Adds a harness rule to the SLM system prompt: `write_file` content must be the
**complete, literal file text** with correct indentation — never a variable name
or placeholder. Addresses a recurring failure mode where Devstral passed a variable
reference (e.g. `content=corrected_code`) in the tool call instead of the actual text.

**Files:** `src/context_manager.rs` (tool_rules in `build_slm_system_prompt`),
`evals/tasks/slm-taskqueue/` (refreshed eval: queue.Queue directly, sentinel shutdown,
queue.Empty handled in worker — 5/5 pass).

---

### feat: SLM agentic loop hardening — chat nudges + loop detector (v0.15.75)

Two new harness guards that fire only in SLM tier:

**Chat-without-tools nudge** — detects when the model returns a ``` code block but makes no
`tool_calls`. Injects an escalating nudge (up to 2 per tool-call round) telling the model to
call `write_file` immediately. Counter resets to 0 after every successful tool-call round, so
the budget is always fresh for the next phase of a task.

**Identical-call loop detector** — fingerprints `(tool_name, args_json)` for every tool called
in a round. If the current round is identical to the previous, appends a nudge to the last tool
result: *"the results will not change — draw a conclusion and act on it."*

Also strengthens the SLM system prompt identity line to mandate immediate tool use.

Frontier model paths: zero changes. Both guards gated on `is_slm_tier()`.

Tier detection logic extracted to `src/config/tier.rs` (module split to stay under 600-line limit).

**Validated results (Devstral 24B, Ollama, Apple Silicon, one user turn each):**

| Task | Result |
|---|---|
| Recursive-descent expression evaluator + pytest | 6/6 pass |
| Sentinel-node LRU cache + tests | 6/6 pass |
| Thread-safe bounded task queue (harder) | 5/5 pass — Future+Condition correct; fixed outer-lock deadlock in ThreadSafeQueue wrapper (removed, use queue.Queue directly) |

**Files:** `src/session/turn.rs` (nudge logic), `src/config/tier.rs` (extracted tier detection),
`src/context_manager.rs` (SLM identity line), `evals/tasks/slm-standalone/`,
`evals/tasks/slm-taskqueue/`.

### feat: SLM tier — Gemma 9B / Qwen / local model optimisations (v0.15.74)

Adds a first-class SLM (small local model) mode that auto-activates for Ollama ≤13B models
and can be set explicitly via `tier = "slm"` in `~/.agent.toml`.

**What changes in SLM mode:**

| Aspect | Before | After |
|---|---|---|
| System prompt | ~3 000 tokens (full) | ~400 tokens (identity + nav rules + tool rules + git + ZAP.md) |
| Tool set | Full (~20 tools) | Core 6: `read_file`, `edit_file`, `write_file`, `shell`, `search_code`, `list_directory` |
| Ollama `num_ctx` | not sent (defaults to 2 048) | injected as `8 192` — fixes truncated prompts on all Gemma/Qwen Ollama models |
| Message alternation | not done | consecutive same-role turns collapsed — prevents Gemma/Mistral chat-template crash |

**Auto-detection:** localhost URL + model name containing a ≤13B size suffix (`7b`, `8b`, `9b`,
`11b`, `13b`, etc.) triggers SLM mode with zero config. A 70B local model does not trigger it.

**Explicit opt-in:**
```toml
[providers.ollama_gemma]
kind     = "openai"
base_url = "http://localhost:11434/v1/chat/completions"
model    = "gemma3:9b"
tier     = "slm"
```

**Files:** `src/config/mod.rs` (`ProviderEntry.tier`, `is_slm_tier()`),
`src/context_manager.rs` (`build_slm_system_prompt()`),
`src/session/mod.rs` (SLM branch in `Session::new`),
`src/llm_client/openai.rs` (`is_ollama`, `num_ctx` injection, `collapse_consecutive_roles`).

### fix: Ollama non-streaming JSON fallback in SSE parser (v0.15.73)

Ollama sometimes returns a plain JSON body (`{"choices":[{"message":...}]}`) instead of SSE
even when `stream:true` is requested (observed with Qwen3-based models like Ornith). The SSE
parser found no `data:` prefix lines, left `streaming_blocks` empty, and `finalize_turn()`
silently produced no output.

Fix in `src/llm_client/openai.rs`: after the SSE loop exits, if all accumulators are still
empty, check whether `buf` holds a complete non-streaming JSON object and parse it as a
non-streaming response — firing a `LlmChunk` TUI event so the reply actually renders.
Existing SSE providers (Anthropic, OpenAI, Codex) are unaffected: the fallback only triggers
when zero SSE `data:` lines were found.

### fix: Ollama reasoning field + image paste E2E tests (v0.15.72)

- `src/llm_client/openai.rs`: SSE parser now recognises both `reasoning_content` (DeepSeek) and `reasoning` (Ollama) delta fields — fixes empty responses from Qwen3-based models (e.g. Ornith) served via Ollama.
- `src/session/agent_loop_tests.rs`: two E2E tests verify the full image-paste pipeline: staged images arrive as `ContentBlock::Image` in the first turn and are cleared afterwards.
- `src/tui/commands/mod.rs`: three unit tests cover `/attach` format rejection, `/attach` vision-gate, and `/paste` vision-gate for non-vision providers.
- `src/tui/input.rs`, `src/tui/actions.rs`, `src/tui/render/overlays.rs`, `src/tui/turn_handler.rs`: file-picker `@path` flow, context-viewer improvements, and general TUI polish landed alongside.

### fix: harden web_search no-key error to stop LLM CLI fallbacks (v0.15.70 patch)

When `BRAVE_SEARCH_API_KEY` is missing, the tool now returns an explicit `STOP` instruction so the LLM doesn't fall back to shell commands (`zap web-search`) instead of telling the user to configure the key.

### fix: Brave Search API + TUI scroll-jump-to-top (v0.15.69)

**web_search**: replaced DuckDuckGo HTML scraping (broken — challenge/CAPTCHA pages on every request) with Brave Search API. Reads `BRAVE_SEARCH_API_KEY` env var or `brave_search_api_key` in `~/.config/zap/agent.toml`. If key is missing, returns a clear setup message with the link to get a free key (2000/month). No DDG fallback — it was never reliable.

**web_fetch**: auto-rewrites `github.com/.../blob/branch/path` → `raw.githubusercontent.com` for direct file content. Bare repo URLs (`github.com/owner/repo`) hit the GitHub API to return the README as markdown. Makes "read this repo and follow the README" work seamlessly without HTML noise.

**TUI scroll jump to top**: fixed the bug where the first scroll-up after a turn would jump to the very top (oldest messages) instead of scrolling smoothly from the bottom. Root cause: `app.scroll` was stale (often 0) when `auto_scroll=true`. Added `rendered_scroll: Cell<usize>` to `App`, written by `draw_messages` every frame. `scroll_up` now syncs from `rendered_scroll` when `auto_scroll=true`, so the first scroll-up moves exactly `n` lines from the actual rendered bottom — no layout approximation.

### feat: full Codex model list in /provider picker (v0.15.67)

Codex provider now shows all 7 models available via ChatGPT subscription, not just gpt-5.5: `gpt-5.5`, `gpt-5.4`, `gpt-5`, `gpt-4.1`, `o4-mini`, `o3`, `gpt-4o`. Both the static model list and provider picker entry updated in `src/tui/provider_picker.rs`.

### fix: command picker header navigation bugs (v0.15.66 patch)

- **Up navigation**: while loop terminated at index 0 without checking if item 0 is a header — pressing Up from the first real command left `picker_sel` pointing at the "Actions" header with nothing visually selected. Fixed by adding the same guard present in Down navigation (stay at old position if final candidate is a header).
- **Tab completion**: didn't check for section headers before doing `app.input = command_text(cmd)` — Tab when `picker_sel` pointed to a header would insert `\x00 Actions` into the input box. Fixed by skipping Tab if selected item starts with `SECTION_PREFIX`.

### UX: /model picker, XDG config, grouped commands (v0.15.66)

Three UX improvements inspired by ndaidong's issue #2 feedback:

**1. `/model` → interactive picker**
`/model` with no args now opens the model picker overlay (same UI as `/provider`'s model-selection step) instead of requiring you to type the model ID. Reuses `PendingProviderSwitch { picking_model: true }` — no new overlay needed.

**2. XDG config path (`~/.config/zap/agent.toml`)**
`config_path()` in `config/mod.rs` now prefers `~/.config/zap/agent.toml` (XDG standard) over `~/.agent.toml`. Read priority: XDG if it exists → legacy `~/.agent.toml` → default to XDG for new installs. `Config::save()` creates parent dirs with `create_dir_all`.

**3. Command picker: grouped with section headers**
`SLASH_COMMANDS` in `commands/mod.rs` is now organized into 5 sections (Actions, Browse, Tools, View, Session) using a `\x00` sentinel for non-selectable header entries. Headers render as italic dim labels in the picker. Navigation (up/down) skips headers. Headers only shown on full `/` list — hidden during prefix filtering.

### Image paste: clear errors for non-vision models/providers (v0.15.64)

Previously, pasting an image gave "✓ Image attached" but the model responded "I don't have access to the image" because the image was silently dropped.

**Root causes fixed:**
1. `handle_paste_image` (lifecycle.rs) now checks `provider_supports_vision` first — shows a clear "✗ model does not support image input" TUI error instead of staging.
2. `provider_supports_vision` (llm_client/mod.rs) is now smarter:
   - **Codex** (`gpt-5.5` via `codex` backend): returns `false` — codex `encode_input` drops image blocks silently; now blocked upfront
   - **Local models** (LM Studio/Ollama on localhost): uses model-name heuristic via `local_model_supports_vision` — known coding models (devstral, codestral, qwen-coder) → false; known vision models (llava, moondream, vision, -vl) → true
   - deepseek.com: still false
3. `OpenAiClient::new` (openai.rs): `image_support` now also applies model-name heuristic for localhost URLs — consistent with `provider_supports_vision`
4. Drop warning in openai.rs now fires a `TuiEvent::Warning` (visible in TUI) instead of only writing to the log file
5. Codex `encode_input` (codex.rs): logs a visible TUI warning when images are present but dropped

**New public helper:** `local_model_supports_vision(model: &str) -> bool`

Also added `!cfg!(test)` guard to the clipboard auto-paste in `handle_user_turn` to prevent test flakiness when the OS clipboard contains a large PNG.

### GoModel built-in in `/provider` picker; live model fetch with auth (v0.15.63)

GoModel is now a first-class provider in the `/provider` picker — no manual `~/.agent.toml` editing required to see it. Appears alongside OpenAI, Anthropic, Groq, etc.

**Model list behavior (priority order):**
1. Live fetch from the configured gateway endpoint (with `api_key` + `extra_headers` auth) — same auth as chat completions
2. Fall back to models defined in `[providers.gomodel.models.*]` in TOML
3. Fall back to `Other…` (free-text entry)

Also adds `fetch_openai_compatible_models_with_auth()` — a variant of the LM Studio model-fetch helper that accepts an API key and extra headers for authenticated `/v1/models` endpoints.

Other fixes bundled in this release:
- `apply_provider_switch` now preserves `extra_headers` and `models` when saving a provider that was already configured (previously wiped them on every TUI provider selection)
- Trust test race condition fixed via `OnceLock<Mutex<()>>`
- `Config::save_to(path)` extracted for testability; config tests split to `tests.rs` with real round-trip coverage

- [src/tui/provider_picker.rs](src/tui/provider_picker.rs): provider picker list extracted from `turn_handler.rs`; GoModel entry with dynamic model fetch
- [src/llm_client/mod.rs](src/llm_client/mod.rs): `fetch_openai_compatible_models_with_auth()`
- [src/tui/lifecycle.rs](src/tui/lifecycle.rs): `apply_provider_switch` preserves existing entry on save
- [src/config/mod.rs](src/config/mod.rs), [src/config/tests.rs](src/config/tests.rs): `save_to(path)` + round-trip tests
- [src/trust.rs](src/trust.rs): env-var test mutex

### Per-model config in `~/.agent.toml` (v0.15.62)

Adds `[providers.<slug>.models."<model-id>"]` support — users can set per-model `name`, `reasoning`, `context`, and `output` directly in config, similar to opencode.

**TOML example:**
```toml
[providers.my-gateway]
api_key = "sk-..."
base_url = "https://gateway.example.com/v1"

[providers.my-gateway.models."llama-3.3-70b-instruct"]
name    = "Llama 70B"
context = 131072
output  = 8192

[providers.my-gateway.models."qwen-2.5-coder-32b"]
name      = "Qwen Coder 32B"
reasoning = true
context   = 131072
```

**Context priority** (highest first): env var → budget → per-model `context` field → provider-level `context_window` → model name heuristic.

**Model picker** — when `/provider` selects a user-configured provider, if the config has a non-empty `models` map the picker shows a sorted Select list (+ "Other…") instead of a free-text field.

**`Config::save()` fix** — `extra_headers` was silently dropped on every save. Both `extra_headers` and `models` sub-tables are now written correctly.

- [src/config.rs](src/config.rs): `ModelEntry` struct, `models` field on `ProviderEntry`, `Config::save()` writes sub-tables, 10 unit tests
- [src/session/mod.rs](src/session/mod.rs): `configured_context_limit()` checks per-model context before provider-level
- [src/session/commands/provider.rs](src/session/commands/provider.rs): model picker uses `models` map when present

### `/models` auth + `/provider` shows user-configured providers (v0.15.61)

Two gaps reported by a gateway user (issue #2):

**`/models` 401 fix** — `cmd_models` was sending a bare GET with no auth. Now it reads the current provider's `api_key` and `extra_headers` from config and adds them to the request, exactly like chat completion calls. If the user has set `Authorization` in `extra_headers`, the default Bearer header is skipped to avoid conflicts.

**`/provider` shows user-configured providers** — The picker was a hardcoded list; any provider configured in `~/.agent.toml` under a non-standard slug (e.g. `[providers.gomodel]`) was invisible. Now, user-configured providers not in the built-in list are appended at the bottom of the picker with a `✓` badge if currently active. Selecting one prompts for model (pre-filled from config) and saves.

- [src/session/commands/provider.rs](src/session/commands/provider.rs): `cmd_models` adds auth + extra_headers to the `/models` request; `cmd_provider` appends user-configured entries from `config.all_providers`

### `/understand` trailing blank lines trimmed after LLM write (v0.15.60)

The LLM sometimes wrote dozens of trailing blank lines to `.zap/understanding.md` via `edit_file`. The session handler only called `mark_domain_map_current()` after the turn — no normalization. Fix: after the turn succeeds, read the file and `trim_end_matches('\n')` before writing it back.

- [src/session/mod.rs](src/session/mod.rs): `/understand` handler trims trailing newlines from `understanding.md` after the LLM turn

### web_search HTML fallback + clearer DuckDuckGo challenge handling (v0.15.68)

Zap's built-in `web_search` now parses DuckDuckGo's HTML results page instead of relying only on the instant-answer JSON endpoint, which often returned "No results found" for normal web queries. It also detects DuckDuckGo challenge pages more explicitly and includes parser tests for link/snippet alignment.

- [src/tools/web.rs](src/tools/web.rs): switch search requests to `https://html.duckduckgo.com/html/`, parse result links/snippets, detect challenge pages, and add parser/live tests.

### `/understand` prompt v3: ban recycling old output, require 3+ code_map calls (v0.15.59)

Second prompt fix pass. Root cause of still-poor output: model was reading `.zap/understanding.md` first and re-merging the old domain map rather than doing fresh analysis, even after the v0.15.58 doc-file prohibition.

Changes:
- **"NEVER read .zap/understanding.md"** added as rule #1 — closes the recycling loop
- **Minimum 3 `code_map` calls required** — must explore top-level layout, main source dir, and 1-2 subdomain directories; repeat for multi-area codebases
- **"Do not invent names"** rule — all symbols in output must be visible in code_map results
- Simplified the write instruction to `edit_file` only (no merge language that tempted the model to read first)

- [src/domain_map.rs](src/domain_map.rs): `build_domain_extraction_prompt()` updated; all 6 unit tests pass

### `/understand` prompt: block doc files, add Hotspots section (v0.15.58)

Fixed the root cause of low-quality `/understand` output: the model was reading `CLAUDE.md`/`README.md` and paraphrasing them instead of analysing code structure. Two fixes:

1. **Explicit doc-file prohibition** — prompt now opens with a STRICT RULES block that names `README.md`, `CLAUDE.md`, `AGENTS.md`, and all markdown/doc files as off-limits. All insights must come from `code_map` output.
2. **Code-first framing** — reframed the task as "surface things the documentation does NOT say" — hotspots, real entry points, actual coupling — so the model produces additive value rather than a paraphrase.
3. **New `### Hotspots` section** — up to 5 most-connected files/modules from `code_map` output, the highest-leverage files for understanding the system.
4. **New `### Tech Stack` section** — one-line summary extracted from manifests, kept separate from domain concerns.
5. **Multi-directory `code_map` guidance** — prompt now explicitly lists common source dir names (`src/`, `Services/`, `Controllers/`, `pages/`, `api/`, etc.) so the model explores polyglot/framework repos instead of stopping after `code_map '.'`.

- [src/domain_map.rs](src/domain_map.rs): `build_domain_extraction_prompt()` rewritten; all 6 existing unit tests still pass; E2E test `b5_understand_writes_domain_map` passes

### `zap skill install/uninstall/list` — skill package management (v0.15.57)

Install community skills (slash-command packs) from GitHub or any URL, without leaving the terminal. Mirrors how Claude Code and Gemini CLI handle extension installation.

```bash
# Install from GitHub shorthand (fetches SKILL.md from repo root)
zap skill install alice/code-review-skills

# Install a specific file from a repo
zap skill install alice/skills-pack/debug.md

# Install from a raw URL
zap skill install https://raw.githubusercontent.com/.../SKILL.md

# Install into the current project only (default is global ~/.zap/skills/)
zap skill install alice/my-skill --local

# List all installed skills (local + global)
zap skill list

# Remove a skill
zap skill uninstall code-review
zap skill uninstall debug --local
```

Installed skills are auto-loaded on the next zap session and invokable via `/<slug>`.

- [src/skill_installer.rs](src/skill_installer.rs): `install()`, `uninstall()`, `list()`; `resolve_url()` handles GitHub shorthand (`user/repo`, `user/repo/path/skill.md`) and raw HTTPS URLs; 4 unit tests
- [src/cli.rs](src/cli.rs): `Commands::Skill` subcommand with `SkillAction::{Install, Uninstall, List}`; dispatched before config load so it runs fast with no agent session overhead
- [src/lib.rs](src/lib.rs): `pub mod skill_installer` declaration

### `extra_headers` in provider config (v0.15.55)

Providers can now declare arbitrary extra HTTP headers sent on every request. Useful for AI gateway routing (GoModel `X-GoModel-User-Path`) or any other per-request header a proxy requires. Configured as a TOML table under each provider's section.

```toml
[providers.gomodel]
kind = "openai"
base_url = "https://gateway.gomodel.ai/v1"
api_key = "sk-..."

[providers.gomodel.extra_headers]
X-GoModel-User-Path = "my/user/path"
```

- [src/config.rs](src/config.rs): `extra_headers: HashMap<String, String>` field on `ProviderEntry` (`#[serde(default)]`)
- [src/llm_client/openai.rs](src/llm_client/openai.rs): `extra_headers: Vec<(String, String)>` applied in request builder
- [src/llm_client/anthropic.rs](src/llm_client/anthropic.rs): same, applied after auth header
- [src/llm_client/mod.rs](src/llm_client/mod.rs): threads extra_headers from entry into both clients; 3 HTTP-layer tests using axum mock server (OpenAI path, Anthropic path, control/no-headers)

### `/understand` — real examples in README + website (v0.15.54)

Added real output from running `/understand` on the zap repo itself (the domain map the agent actually wrote during E2E testing) to both README.md and website/docs.html. The example shows the exact tool calls (2× code_map, 1× edit_file), timing, and the full domain map table.

- [README.md](README.md): "Real output: /understand run on the zap repo itself" section with terminal trace + rendered domain map
- [website/docs.html](website/docs.html): "Real output — run on the zap repo itself" code block replacing the placeholder example

### `/understand` — domain extraction + website docs + E2E test (v0.15.53)

Adds a `/understand` command that makes one LLM call using only `code_map` output to extract the project's business-domain map and write it to `.zap/understanding.md`. The domain section is wrapped in sentinel comments so it survives future `/understand` refreshes without destroying other content. Auto-staleness detection: if the top-level source module count drifts >10% from when the map was last generated, the system prompt includes a nudge to re-run `/understand`. 6 unit tests (domain_map.rs) + 1 E2E test (sdk_e2e b5) that spawns a real zap binary, sends the domain prompt, and asserts `code_map` was called and sentinels appear in the written file.

- [src/domain_map.rs](src/domain_map.rs): `build_domain_extraction_prompt`, `save_domain_section_to`, `save_domain_section`, `has_domain_map`, `domain_map_is_stale`, `source_module_count`, `mark_domain_map_current`; 6 unit tests
- [src/project.rs](src/project.rs): `domain_module_count` field added to `ProjectMeta`; re-exports from `domain_map`
- [src/session/commands/code.rs](src/session/commands/code.rs): `cmd_understand` method
- [src/session/mod.rs](src/session/mod.rs): `/understand` command dispatch with `mark_domain_map_current` on success
- [src/context_manager.rs](src/context_manager.rs): staleness nudge section in system prompt
- [src/tui/mod.rs](src/tui/mod.rs): `domain_module_count: None` in `ProjectMeta` initializer
- [tests/sdk_e2e.rs](tests/sdk_e2e.rs): `b5_understand_writes_domain_map` E2E test (ignored, requires API key)
- [website/docs.html](website/docs.html): `/understand` section, sidebar link, commands table entry

### ripple_analysis — BFS tests + closure injection (v0.15.50)

Refactored `ripple_bfs` to use closure injection (`ripple_bfs_with`) so the BFS core is testable without needing the global index. Added 9 new end-to-end BFS tests covering: direct callers, no callers, two-depth chains, max-depth cutoff, cycle termination, visited-set re-expansion prevention, multi-caller files, and `impl Trait for Struct · method` scope parsing.

- [src/tools/ripple.rs](src/tools/ripple.rs): `ripple_bfs_with` (testable core), 23 total tests

### ripple_analysis tool — blast radius for symbol changes (v0.15.49)

BFS walk of the call graph to show who calls a symbol, who calls those callers, and so on. Pure SQLite graph traversal — no language server or network needed. Groups output by depth and file, sorts by line number. Accepts optional `depth` parameter (1–5, default 3).

- [src/tools/ripple.rs](src/tools/ripple.rs): `RippleAnalysisTool`, `ripple_bfs`, `format_ripple`, `extract_fn_name`, `shorten_path` — 14 unit tests
- [src/tools/mod.rs](src/tools/mod.rs): registered `RippleAnalysisTool`

### LSP Java support via jdtls (v0.15.51)

Added Java to the LSP language map. `.java` files now map to `jdtls` (Eclipse JDT Language Server), installable via `brew install jdtls`. Tests cover both the extension-to-language mapping and the spec lookup.

- [src/lsp/servers.rs](src/lsp/servers.rs): added `"java" → jdtls` in `spec_for_language`, `"java"` extension in `language_for_path`, and two test assertions

### LSP client module skeleton (Task 1 of lsp-integration) + code-review fixes

Adds `src/lsp/` — a thin async-lsp wrapper that will back future LSP tools (go-to-definition, hover, diagnostics). No tools are wired yet; this task establishes the module structure and verifies it compiles. A follow-up code-review pass fixed child-process leaks, URL encoding, dead code, and mutex-poison panics.

- [src/lsp/servers.rs](src/lsp/servers.rs): maps languages to LSP binaries (rust-analyzer, pylsp, gopls, tsserver), PATH detection, and file-extension → language mapping; dead `spawn_server()` removed
- [src/lsp/client.rs](src/lsp/client.rs): `ZapLspClient` wraps an `async_lsp::ServerSocket`; handles initialize handshake, did-open/did-save notifications, hover and goto-definition requests, and caches incoming diagnostics; `kill_on_drop(true)` + stored `JoinHandle` prevent orphan child processes; `Url::from_file_path` replaces unsafe string-concat URLs; mutex poison recovered with `unwrap_or_else`; `Drop` impl aborts the mainloop task; `is_alive()` exposed
- [src/lsp/mod.rs](src/lsp/mod.rs): `LspManager` pools per-language clients with dead-client eviction and `has_client_for()` liveness check; binary resolved once to avoid TOCTOU; `GLOBAL_LSP` singleton mirrors the `GLOBAL_INDEX` pattern

### LspManager initialization at session startup (Task 2 of lsp-integration)

Initializes the `LspManager` singleton when a new session starts, right after the `CodeIndex` singleton setup. Language servers are spawned lazily on first tool use, so session startup remains fast. Tools can now call `crate::lsp::global_lsp()` to access the singleton instance.

- [src/session/mod.rs](src/session/mod.rs): `LspManager::new()` initialized with current working directory at session creation time, mirroring the `CodeIndex` setup pattern

### get_diagnostics tool — instant compiler errors via LSP (Task 3 of lsp-integration)

Adds the `get_diagnostics` tool to `ToolRegistry`. The tool opens a file with the language server, waits 500 ms for a `publishDiagnostics` notification, and returns the cached diagnostics formatted as `line:col severity[code]: message`. Returns a friendly message if the LSP is not initialized or the file type is unsupported. Includes 3 unit tests for `format_diagnostics` covering the empty case, 1-indexed line/col conversion, and severity labels.

- [src/tools/lsp_tools.rs](src/tools/lsp_tools.rs): `format_diagnostics()` helper + `GetDiagnosticsTool` struct implementing the `Tool` trait
- [src/tools/mod.rs](src/tools/mod.rs): `pub mod lsp_tools` declaration + `GetDiagnosticsTool` registered in `ToolRegistry::new`

### get_diagnostics bug fixes (Task 3 follow-up)

Three protocol-correctness bugs fixed in the LSP diagnostic path:

- [src/lsp/client.rs](src/lsp/client.rs): `open_file` changed to `&mut self`; added `opened_files: HashSet<String>` to `ZapLspClient` to deduplicate `textDocument/didOpen` — sending it twice for the same URI is a protocol error that causes rust-analyzer to stop delivering diagnostics for that file; `publish_diagnostics` now keys the diagnostics map by the full `file://` URL string (`params.uri.to_string()`) instead of the bare percent-decoded path; `cached_diags` converts the lookup path through `Url::from_file_path` to match — fixes key mismatch for paths containing spaces or other characters that percent-encode differently
- [src/lsp/mod.rs](src/lsp/mod.rs): `client_for` return type changed from `&ZapLspClient` to `&mut ZapLspClient` so callers can invoke the now-`&mut self` `open_file`
- [src/tools/lsp_tools.rs](src/tools/lsp_tools.rs): removed spurious `block_in_place`/`block_on` wrapper from `execute` (already `async fn`); replaced with direct `.await` calls — the old pattern blocked a thread for the full 500 ms sleep

### lsp_definition tool — type-resolved go-to-definition via LSP (Task 4 of lsp-integration)

Adds the `lsp_definition` tool to `ToolRegistry`. The tool invokes `goto_definition` on the language server at a specific line and column, returning locations as `path:line:col` (1-indexed). Unlike `find_definition` (symbol-table and import-graph search), `lsp_definition` resolves full type information — accurate for cross-crate symbols, generics, and trait implementations. Returns a friendly message if the LSP is not initialized or the file type is unsupported. Includes 2 unit tests for `format_locations` covering the empty case and 1-indexed line/col conversion.

- [src/tools/lsp_tools.rs](src/tools/lsp_tools.rs): `format_locations()` helper + `LspDefinitionTool` struct implementing the `Tool` trait
- [src/tools/mod.rs](src/tools/mod.rs): `LspDefinitionTool` added to `use lsp_tools::...` import and registered in `ToolRegistry::new`

### lsp_definition tool fixes (Task 4 follow-up)

Four correctness and usability fixes to the LSP definition path:

- [src/tools/lsp_tools.rs](src/tools/lsp_tools.rs): (1) added 100ms sleep after `open_file` before `goto_definition` so the LSP has time to index cold files before resolving definitions; (2) fixed `format_locations()` to use `to_file_path()` instead of `path()` to properly decode percent-encoded URLs, so paths with spaces display correctly; (3) updated tool description to clarify that output locations use 1-indexed line and column numbers (input is 0-indexed); (4) added `test_format_locations_multiple()` unit test to cover the multi-location join path

### lsp_type_at tool — expression types and signatures via LSP hover (Task 5 of lsp-integration)

Adds the `lsp_type_at` tool to `ToolRegistry`. The tool queries the language server's hover capability at a specific line and column, returning the type or signature of the expression at that position — the same information shown in editor tooltips. Use this when you need the exact type of a variable, function signature, or doc comment without manual type inference. Input uses 0-indexed line/column; output includes friendly 1-indexed position information. Returns a friendly message if the LSP is not initialized or the file type is unsupported.

- [src/tools/lsp_tools.rs](src/tools/lsp_tools.rs): `LspTypeAtTool` struct implementing the `Tool` trait, wraps `client.hover_text()` 
- [src/tools/mod.rs](src/tools/mod.rs): `LspTypeAtTool` added to `use lsp_tools::...` import and registered in `ToolRegistry::new`

### LSP did_save notification on file edit (Task 6 of lsp-integration)

Zap now notifies the language server of file saves via textDocument/didSave whenever tools write to files. This keeps the LSP server in sync with the current file state, enabling accurate diagnostics, definitions, and hovers after edits. The notification is sent after existing code-index reindex, using non-blocking `try_lock()` to avoid contention with async operations.

- [src/lsp/mod.rs](src/lsp/mod.rs): added `notify_save()` method to `LspManager` that gets a live client and sends the save notification if the LSP is alive
- [src/session/tools.rs](src/session/tools.rs): after reindexing files, send LSP did_save notification with canonicalized absolute path, using `try_lock()` for non-blocking lock acquisition

### LSP fallback in find_definition for cross-crate symbols (Task 7 of lsp-integration)

When the AST index and grep both return no results, `find_definition` now attempts a `goto_definition` request via the language server if the caller supplies `path`, `line`, and `col` fields and a live LSP client is already running for the file's language. No new LSP servers are spawned; `try_lock()` is used for non-blocking lock acquisition. The result is annotated with "(AST index had no result; found via LSP)". The `find_definition` input schema now accepts optional `line` and `col` integer fields (0-indexed) for this fallback.

- [src/tools/search/mod.rs](src/tools/search/mod.rs): LSP fallback block in `FindDefinitionTool::execute`; added `line` and `col` to `input_schema`

### Fix TOCTOU race in LSP fallback path (Task 7 follow-up)

The LSP fallback in `find_definition` had a race condition: `has_client_for()` checked if a live client existed, but between that check and the subsequent `client_for()` call, the client could die. When it died, `client_for()` would spawn a new server — violating the spec that says "no new server spawns" in the fallback path. The fix replaces the two-call pattern with a single atomic method `get_client_if_alive()` that checks liveness and returns the reference in a single operation, with no TOCTOU window.

- [src/lsp/mod.rs](src/lsp/mod.rs): new `get_client_if_alive()` method atomically checks client liveness and returns a mutable reference, or None if dead (and silently evicts it); never spawns a new server
- [src/tools/search/mod.rs](src/tools/search/mod.rs): LSP fallback block now uses `get_client_if_alive()` instead of the `has_client_for()` + `client_for()` pattern; simplified nested-if chain as a result

### LSP vs AST tool guidance in system prompt (Task 8 of lsp-integration)

Added a new "LSP tools (semantic, live)" section to the system prompt in `src/context_manager.rs` explaining when to use each of the three new LSP tools (`get_diagnostics`, `lsp_definition`, `lsp_type_at`) vs the existing AST tools. Provides decision guidance: structural questions → AST tools; post-edit correctness → `get_diagnostics`; cross-crate resolution → `lsp_definition`; expression types → `lsp_type_at`.

- [src/context_manager.rs](src/context_manager.rs): added ~27-line LSP tool guidance section after Code Navigation Strategy

### Background index errors no longer block the TUI (v0.15.37)

Background index WARN/ERROR logs now display as warning bubbles instead of hijacking the streaming state. Previously, a background indexer error sent `LlmChunk` to the TUI channel, which unconditionally set `state = AppState::Thinking` — trapping the user because Ctrl+C in Thinking state mapped to `Cancel` (a no-op in the idle path). The indexer also now reads files via `read()` + `from_utf8_lossy` so files with invalid UTF-8 index with replacement chars instead of failing; and the file path is included in error messages so the culprit is identifiable in logs.

- [src/log.rs](src/log.rs): WARN/ERROR sends `TuiEvent::Warning` (no state change) instead of `TuiEvent::LlmChunk`
- [src/tui/actions.rs](src/tui/actions.rs): `InputAction::Cancel` in the idle path resets phantom Thinking state as defense-in-depth
- [src/code_index/index_impl.rs](src/code_index/index_impl.rs): lossy UTF-8 read + file path in error messages

### TUI mouse-wheel scrolling without full mouse capture (v0.15.36)

Zap now enables narrow terminal mouse button/wheel reporting instead of crossterm's full mouse capture in TUI mode. This keeps mouse-wheel scrolling in the chat history while avoiding the drag-motion capture that commonly prevents normal terminal text selection.

- [src/tui/mod.rs](src/tui/mod.rs): enters TUI with SGR/button mouse reporting (`?1000h`/`?1006h`) plus bracketed paste, and restores those modes on exit. Full `EnableMouseCapture` remains disabled so click-drag terminal selection has a better chance to work.

Update: on macOS terminals, wheel events could still leak to terminal scrollback and appear to jump the UI to the top. Zap now also enables DECSET `?1002h` button-event tracking while in TUI mode, which keeps wheel scrolling inside the app; plain click events still remain no-ops in the TUI.

### Codex provider context and resumed tool history hardening (v0.15.35)

Zap now treats the Codex provider as a 400,000-token context window instead of falling back to the generic 32k default, so `gpt-5.5` and future Codex model names are handled correctly without fragile model-name matching.

- [src/config.rs](src/config.rs) and [src/session/mod.rs](src/session/mod.rs): `configured_context_limit()` honors `ZAP_MAX_CONTEXT_TOKENS`, explicit provider `context_window`, and provider/kind-level Codex defaults.
- [src/session/history.rs](src/session/history.rs): windowed/resumed history now removes orphan tool results before OpenAI-compatible requests, preventing HTTP 400 errors after history trimming.
- [tests/provider_e2e.rs](tests/provider_e2e.rs): e2e-style regression coverage verifies Codex provider context stays 400k for plain `gpt-5.5`, future Codex model names, custom `kind = "codex"` providers, and explicit overrides.

### TUI image attach, text paste, and mouse-wheel scroll (v0.15.33)

Fixes three TUI usability gaps: image attachments are staged for the next multimodal turn, normal clipboard text paste inserts into the input box, and mouse-wheel scrolling moves the chat history.

- [src/tui/commands/mod.rs](src/tui/commands/mod.rs): adds `/attach <image-path>` and `/paste` to the command picker and stages supported image formats (`png`, `jpg/jpeg`, `gif`, `webp`) in `session.staged_images` after vision-provider checks.
- [src/tui/input.rs](src/tui/input.rs) and [src/tui/actions.rs](src/tui/actions.rs): route bracketed paste text through a cursor-safe insertion helper so multi-line clipboard text pastes like normal typing.
- [src/tui/mod.rs](src/tui/mod.rs): enables bracketed paste and mouse capture, handles `Event::Paste`, maps wheel events to existing scroll actions, and restores terminal modes on exit.

### Multi-folder workspaces via --add-dir (v0.15.32)

Open zap across multiple project folders simultaneously, equivalent to Claude Code's `--add-dir` flag.

```bash
zap --add-dir ../api --add-dir ~/shared-lib
```

The model gets full read/write access to all listed directories. They appear in the system prompt under `## Environment` and in the TUI welcome message so it's always clear which folders are in scope.

- [src/cli.rs](src/cli.rs): `--add-dir <PATH>` repeatable flag. Paths are resolved to canonical absolute paths (including `~` expansion) at startup.
- [src/config.rs](src/config.rs): `additional_dirs: Vec<String>` on `Config` and `FileConfig` (also settable in `~/.agent.toml` as `additional_dirs = [...]`).
- [src/context_manager.rs](src/context_manager.rs): additional dirs listed under `## Environment` in the system prompt.
- [src/session/mod.rs](src/session/mod.rs): dirs merged into write-roots so file edits are allowed; startup notice shown in TUI.

### Fix git credential prompts corrupting TUI terminal state (v0.15.30)

When zap ran `git clone` or similar commands requiring credentials, git would open `/dev/tty` directly to prompt for a username/password — bypassing crossterm's raw mode. The user saw a garbled prompt with no way to type into it, and after pressing Ctrl+C the prompt area became erratic (slow, dropped characters) because the SIGKILL'd git process left the TTY in a corrupted state.

- [src/shell_runner.rs](src/shell_runner.rs): `run_with_timeout` and `run_with_timeout_secs` now set `GIT_TERMINAL_PROMPT=0` and `SSH_ASKPASS_REQUIRE=never` on every subprocess. Git fails immediately with a clear error instead of opening `/dev/tty` and hanging.
- [src/tui/turn_handler.rs](src/tui/turn_handler.rs): `run_normal_turn` calls `enable_raw_mode()` after every turn completes (including cancelled ones) to force-restore crossterm's terminal state if a killed subprocess left the TTY in a bad state.

### search_code and code_map now find hidden project dirs like .kiro (v0.15.30)

ripgrep skips dot-prefixed directories by default, and `code_map`'s filesystem walker had its own blanket "skip anything starting with `.`" check — so `.kiro/specs/`, `.claude/`, `.cursor/` and similar real project directories were invisible to both tools until the user spelled out the exact path. `glob_read` already handled this correctly by maintaining an explicit noise list rather than a blanket dot-skip.

- Added `SKIP_DIR_NAMES` constant in [src/tools/mod.rs](src/tools/mod.rs): an explicit list of build artifacts, VCS internals, and caches (`target`, `node_modules`, `.git`, `.venv`, etc.) that all three exploration tools skip. Real hidden project dirs (`.kiro`, `.claude`, `.github`, `.cursor`) are **not** in the list and are therefore walked.
- [src/tools/search/search_impl.rs](src/tools/search/search_impl.rs): added `--hidden` flag to the ripgrep invocation so it descends into dot-prefixed dirs, then passes `--exclude-dir=<name>` for each entry in `SKIP_DIR_NAMES` to keep the actual noise out. The fallback `grep -r` path gets matching `--exclude-dir` flags. The native Rust walker's hardcoded skip list is replaced with `SKIP_DIR_NAMES.contains()`.
- [src/tools/file/glob.rs](src/tools/file/glob.rs): `glob_walk_safe`'s inline skip list replaced with `SKIP_DIR_NAMES`.
- [src/tools/search/search_impl.rs](src/tools/search/search_impl.rs) `walk_dir_for_map`: blanket `name_str.starts_with('.')` guard removed, replaced with `SKIP_DIR_NAMES.contains()`.
- 2 regression tests (`hidden_dir_tests` in `search_impl.rs`): a `.kiro`-shaped fixture verifies `search_code` finds matches in hidden dirs while `.git` stays excluded; a second test verifies `code_map` surfaces markdown headings from `.kiro/specs/`.
- Also: serial execution for mutating tools (`shell`, `write_file`, `edit_file`, etc.) within a turn — read-only tools stay parallel; `context_fill_pct` now uses windowed history instead of the full log (prevents auto-compact triggering at percentages the status bar never shows); 2 new unit tests for the `context_fill_pct` fix.

### Show the literal sqlite query in code-index tool-call headers, Claude-Code-style (v0.15.29)
v0.15.25 stopped `find_definition`/`find_references`/`who_calls`/`code_map` from appending their raw SQL to the tool's *returned text*, because that text also goes to the LLM and small/local models parroted the most command-shaped line back into chat — the query was moved to a log-only `crate::log::write("INDEX", ...)` line. That fixed the parroting bug but, as a side effect, also removed the query from the UI entirely: the tool-call header just showed a generic phrase like "find definition of 'foo'", with no equivalent of how Claude Code's own Bash tool shows the literal command it's running (`Bash(npm test)`) in the header — never narrated in prose, but always visible.

- Added `definition_query_sql()` / `call_sites_query_sql()` / `code_map_query_sql()` in [src/tools/search/search_impl.rs](src/tools/search/search_impl.rs) as the single source of truth for each tool's literal (parameterized-by-value) SQL text.
- `permission_context()` for `FindDefinitionTool`/`FindReferencesTool`/`WhoCallsTool`/`CodeMapTool` in [src/tools/search/mod.rs](src/tools/search/mod.rs) now returns `sql› SELECT ...` instead of a friendly phrase. `permission_context` only feeds the tool-call header (`ApprovedCall::ctx` in `session/tools.rs`) and the audit log — never the LLM-bound result text — so this is UI-only, the same way `ShellTool` already shows `$ <command>`; these 4 tools are read-only and never gated behind a permission prompt either, so there's no risk of this confusing an approval dialog.
- Also fixed two pre-existing inaccuracies in the `.zap/zap.log` diagnostic lines while consolidating onto the shared helpers: `find_definition`'s logged query claimed `LIKE '%name%'` (infix) when the real query is `name = ? COLLATE NOCASE` (exact, tried first); `who_calls`'s logged query never reflected the `qualifier` filter even when one was passed. `format_call_sites()` now takes `qualifier: Option<&str>` to fix the latter.
- 4 new unit tests assert the exact SQL text per tool (`sql_preview_tests` in `search/mod.rs`) — regression coverage given this exact feature silently disappeared once already.

### Fix mouse-scroll junk characters in input; live row counts during indexing (v0.15.28)
Two complaints: scrolling the mouse wheel over the TUI was typing junk characters (`[A`, `[B`) into the input box, and `/init`/`/index` gave no sense of progress beyond a spinner during a long scan.

- **Mouse-scroll junk regression.** This is the same bug v0.15.1 fixed by turning *on* `EnableMouseCapture` (see the old Windows-fixes entry below) — and the same bug v0.15.25 reintroduced by turning capture back *off* to restore click-drag copy/paste. With capture off, terminals fall back to "alternate scroll mode" (xterm DECSET 1007) and synthesize Up/Down key sequences for wheel scroll instead; on the terminal where this was reported, those sequences don't reassemble cleanly and leak as literal `[A`/`[B` characters into the input. Fix: explicitly disable mode 1007 (`\x1b[?1007l`) when entering the alternate screen in [src/tui/mod.rs](src/tui/mod.rs) and `resume_tui` in [src/tui/lifecycle.rs](src/tui/lifecycle.rs), and restore it (`\x1b[?1007h`) on exit/`suspend_tui` so other alt-screen apps (less, vim, git pager) keep their normal scroll-as-arrows behavior. This avoids re-enabling full mouse capture, so click-drag copy/select stays intact — the wheel is just a no-op now; PageUp/PageDown still scroll.
- **Live progress during indexing.** `run_indexing_with_spinner()` in [src/tui/actions.rs](src/tui/actions.rs) already ticks a spinner every 16ms while the scan runs on a `spawn_blocking` task holding the index's mutex for the whole scan — too long to query through that same lock. Added `peek_scan_progress()` in [src/code_index/mod.rs](src/code_index/mod.rs), which opens its own short-lived **read-only** connection to `.zap/code.db` (WAL mode allows concurrent reads while the indexer's connection commits per-file) and counts rows in `indexed_files`/`symbols`/`call_sites`. Every 10s of wall-clock the spinner loop calls it and updates the status label to `codebase — N files · N syms · N calls`, so a multi-minute scan on a large repo shows real movement instead of a static label.

### Collapse dead-end tool exploration by default (v0.15.27)
During `/init`'s navigation-map turn (and any agentic exploration), the model often tries a path that turns out empty — `code_map` on a directory with no source files, `code_map` on a file with no recognised symbols, `search_code`/`find_definition`/`who_calls` with nothing to show. None of these are errors, but every tool result was auto-expanded by default (`src/tui/app.rs`, "Auto-expand every tool result"), so a normal multi-step exploration showed as a wall of "No source files found in 'Lib'…", "(no recognised symbols)", "no matches found" — confirmed as the literal complaint from a user screenshot.

- [src/session/preview.rs](src/session/preview.rs): `smart_tool_preview` now recognises `code_map`'s two "nothing found" shapes (empty dir, file with no symbols) and condenses both to `📚 no symbols found`; added a `find_references`/`who_calls` branch (`🔗 N call site(s)` / `🔗 no callers found`) that didn't have one before. New `preview_found_nothing()` classifies the four canned "nothing found" previews (`⚠ not in index`, `🔍 no matches found`, `📚 no symbols found`, `🔗 no callers found`).
- [src/tui/app.rs](src/tui/app.rs): `ToolDone` only auto-expands when the result is an error or actually found something — dead-end results stay collapsed to a single dim line, still reachable via `Ctrl+O`. 7 new unit tests for the preview changes.

Verified live: the same `code_map 'Lib'` / `code_map 'server.mjs'` calls that previously showed full raw sentences now collapse to one line each ("📚 no symbols found"), no expand hint, while a `search_code` call that found something stays expanded as before.

### Fix slash-command picker submitting bracket placeholder text; /index now inline (v0.15.27)
Selecting `/index` from the command picker (Enter or Tab) submitted the literal label text `/index [quality]` — the `[quality]` was meant as a display-only hint ("you can optionally pass 'quality' here"), not real input. That sent `arg = "[quality]"` (brackets included) to `cmd_index`, which didn't match the `"quality"`/`"health"` special cases, fell through to the generic path, and treated the literal string `"[quality]"` as a target *directory* — hence "tree-sitter scanning [quality]…" and "0 file(s) indexed" reported by the user. Same bug affected `/remote [port]`, the only other bracketed entry in `SLASH_COMMANDS`.

- [src/tui/input.rs](src/tui/input.rs): added `command_text()`, which strips a trailing `[placeholder]` hint from a picker label before it's used as real input/submitted text. Applied at both accept points (Enter-to-submit, Tab-to-complete). Literal subcommands with no brackets (`/remote stop`, `/help`, skill names) are left untouched. 3 new unit tests.
- Separately, plain `/index` (no args) was *also* still dropping out of the TUI into the old "suspend terminal, print, press any key to return" fallback — jarring even with the bracket bug fixed. Extracted the `/init`-progress-spinner logic (added in v0.15.26) into a shared `run_indexing_with_spinner()` in [src/tui/actions.rs](src/tui/actions.rs) and wired plain `/index` to use it directly in [src/tui/turn_handler.rs](src/tui/turn_handler.rs) — it now renders inline with the same animated "Indexing…" status as `/init`, no terminal suspend at all.

Verified live via tmux: before the fix, `/index` printed "tree-sitter scanning [quality]…" / "0 file(s) indexed"; after, it shows the correct cwd path inline with no suspend screen.

### /init: show real progress, stop leaking index warnings into chat (v0.15.26)
Two complaints, one root cause area (`/init`'s TUI wizard): the wizard froze with zero visual feedback while indexing, and unrelated per-file skip warnings showed up as confusing inline chat messages.

- **Indexing now shows live progress instead of freezing.** The wizard's "Index now?" step ran `index_dir()` via `tokio::task::block_in_place`, which blocks the calling task — so the whole TUI (redraws included) froze for the entire scan with no feedback. Split the slow scan out into a standalone `run_init_indexing()` ([src/session/commands/code.rs](src/session/commands/code.rs)), run via `tokio::task::spawn_blocking` from [src/tui/actions.rs](src/tui/actions.rs)'s `ConfirmInit` handler inside a `tokio::select!` tick loop (the same pattern `run_normal_turn` already uses for LLM calls) so the event loop keeps redrawing. Status bar now shows an animated "⠋ Indexing codebase" the whole time. Verified live via a tmux-driven TUI session indexing 3600 files: spinner animated continuously for the full ~90s scan, no freeze, correct final result.
- **Stopped routine index-skip warnings from leaking into chat.** `crate::log::write("WARN ", ...)` auto-injects into the chat stream in TUI mode (by design, for genuinely actionable warnings). `index_dir()` was logging "N file(s) skipped — ... (fix .zap/code.db permissions or run /init again)" at WARN level for *any* per-file read/parse failure — including totally routine ones (one binary file, one odd permission) — so users saw a confusing, often-irrelevant warning mid-`/init`. Confirmed via the user's own historical `.zap/zap.log` that this was firing in past sessions. Downgraded to `INDEX`-level (logged to `.zap/zap.log`, never chat-injected) in [src/code_index/index_impl.rs](src/code_index/index_impl.rs) — the genuinely systemic case (every file failed, e.g. a corrupt index DB) still surfaces because `index_dir` returns an `Err` in that case, handled separately.

### TUI input box: fix cursor drift on line wrap (v0.15.25)
The text input box computed where to draw the blinking cursor using hard-wrap math (wrap at exactly column N), but handed the actual text to ratatui's `Paragraph::wrap()`, which word-wraps (breaks at the nearest space). The two disagreed whenever a wrap landed mid-word, so the cursor visibly drifted away from the character you'd just typed — the "not smooth" jump the user hit while typing past the line edge.

Fixed by replacing both the old `cursor_visual_pos` (layout.rs) and `visual_line_count` (render/mod.rs) — two separate hard-wrap estimators that also disagreed with *each other* on how much width row 0 got — with one `wrap_input()` function that hard-wraps the text into rows and locates the cursor in the same pass. `draw_input` now renders those exact pre-wrapped rows directly (no `.wrap()` call), so the screen and the cursor math can never diverge. 6 new unit tests cover mid-word wraps, newlines, and zero-width edges.

### TUI: stop capturing the mouse, restore native copy/select (v0.15.25)
`EnableMouseCapture` was on for scroll-wheel support, which (as in any terminal app with mouse reporting on) blocks the terminal emulator's native click-drag text selection — copying anything out of the zap TUI required holding a modifier key most users don't know about. Removed `EnableMouseCapture`/`DisableMouseCapture` from `tui/mod.rs` and `tui/lifecycle.rs` (`suspend_tui`/`resume_tui`) and the now-dead `Event::Mouse` handler. PageUp/PageDown still scroll via the keyboard; copy/select now works exactly like a normal terminal.

### Stop leaking raw sqlite queries into chat (v0.15.25)
`find_definition`, `find_references`, `who_calls`, and `code_map` (v0.15.21) appended the literal `# sqlite3 .zap/code.db "SELECT ..."` query into the tool's returned text — which is also what's sent back to the LLM. Small/local models tend to parrot the most "command-shaped" line in a tool result back into their reply, so users saw raw SQL dumped into the conversation — not how Claude Code surfaces tool internals (params live in the UI's tool-call display, never narrated by the model). Moved the query string out of the LLM-bound text and into the existing `crate::log::write("INDEX", ...)` diagnostic line for that lookup (`.zap/zap.log`), where it's still available to inspect but can no longer end up in the chat.

### Project-scoped sessions + subagent session-bloat fix (v0.15.25)
Root-caused two long-standing complaints: resume sometimes showed only the `context.md` files-changed banner with no conversation, and `~/.zap/agent.db` had accumulated 381 session rows (137 with zero messages).

Root cause: `sessions` was a single global table shared by every project zap runs in, with no project column. "Resume last session" grabbed whichever row was most recently inserted *anywhere* — which could be a different project's session, or an empty one — while the project-local `.zap/context.md` banner (correctly scoped per-project) kept showing the right goal/files. Separately, every `spawn_agent` subagent call also inserted a top-level session row with no way to tell it apart from a real interactive session.

| Change | Where | Detail |
|---|---|---|
| `sessions.cwd` column | `src/persistence.rs` | Added via idempotent migration (`PRAGMA table_info` check + `ALTER TABLE`). Historical rows have `cwd = NULL` and are simply invisible to the new scoped queries — not deleted. |
| `recent_sessions_for_cwd` | `src/persistence.rs` | Replaces `recent_sessions`. Filters by `cwd = ?` AND `EXISTS` a `session_messages` row — hides both other-project sessions and empty/trivial ones from `/sessions` and resume. |
| `load_previous_messages(id, cwd)` | `src/persistence.rs` | Same scoping applied to the "previous session" lookup used by auto-resume. |
| Subagents don't persist a session row | `src/session/mod.rs`, `src/session/turn.rs` | `session_id = 0` sentinel when `config.is_subagent`; `save_messages`/`update_session_goal` calls are guarded on `session_id != 0`. Stops `spawn_agent` from writing `(repl)` rows. |
| `/sessions`, auto-resume-on-startup | `src/session/commands/session_mgmt.rs`, `src/tui/turn_handler.rs`, `src/tui/startup.rs` | All three call sites switched to the cwd-scoped query. |

Verified live: ran a real single-shot turn against a local LM Studio model, confirmed the new session row got the correct `cwd`, and confirmed the cwd-scoped query excludes the pre-migration rows while surfacing only this project's history.

Also verified (no code change needed): the durable cross-session memory system (`memory_set`/`memory_delete` tools, `/memory` command, `## Agent Memory` system-prompt injection in `src/tools/memory.rs` / `src/persistence.rs` / `src/session/memory_refresh.rs`) was already fully wired but had zero saved facts — added round-trip unit tests and confirmed a live `memory_set` call persists and round-trips correctly.

### Fix Cerebras model names (v0.15.24)
Cerebras retired the Llama model IDs (`llama3.3-70b`, `llama3.1-70b`, `llama3.1-8b`, `qwen-3-32b`). Updated all three pickers to the current live models returned by `/v1/models`: `gpt-oss-120b` and `zai-glm-4.7`. Both verified working via live API call.

### Zhipu AI and Qwen (DashScope) providers (v0.15.23)
Adds two Chinese AI providers. Provider count grows from 19 to 21.

| Provider | Slug | Base URL | Notable models |
|---|---|---|---|
| Zhipu AI (GLM) | `zhipu` | `https://open.bigmodel.cn/api/paas/v4/chat/completions` | `glm-4-flash` (free!), `glm-4-air`, `glm-4-plus`, `glm-z1-flash` (thinking) |
| Qwen (DashScope) | `qwen` | `https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions` | `qwen-turbo`, `qwen-plus`, `qwen-max`, `qwen-long`, `qwen2.5-72b-instruct` |

Note: Kimi (Moonshot AI) was already added in v0.15.21. Get keys at open.bigmodel.cn and dashscope.aliyun.com.

### OpenRouter expanded model list + e2e test suite (v0.15.22)
Expanded the OpenRouter model picker from 4 models to 13 covering all major provider families, plus a comprehensive e2e test suite with 34 live tests (all `#[ignore]`; run with `cargo test --test provider_e2e -- --ignored`).

**New model list in picker:** `anthropic/claude-opus-4.8`, `anthropic/claude-sonnet-4.6`, `openai/gpt-4.1`, `openai/gpt-4.1-mini`, `meta-llama/llama-4-maverick`, `meta-llama/llama-3.3-70b-instruct:free`, `google/gemini-2.5-pro`, `google/gemini-2.5-flash`, `deepseek/deepseek-r1`, `deepseek/deepseek-chat`, `qwen/qwen3-235b-a22b`, `mistralai/mistral-large-2512`, `x-ai/grok-4.20`, `Other…`

**e2e test groups** (in `tests/provider_e2e.rs`):

| Group | Models tested |
|---|---|
| Free ($0) | llama-3.3-70b:free, gemma-4-26b:free, qwen3-coder:free, nemotron-super-120b:free, gpt-oss-120b:free |
| Anthropic | claude-haiku-4.5, claude-sonnet-4.6, claude-opus-4.8 |
| OpenAI | gpt-4.1-nano, gpt-4.1-mini, gpt-4o-mini, gpt-4.1 |
| Meta Llama | llama-3.3-70b, llama-4-maverick, llama-4-scout |
| Google | gemini-2.5-flash-lite, gemini-2.5-flash, gemini-2.5-pro |
| DeepSeek | deepseek-chat, deepseek-r1 (reasoning model) |
| Qwen | qwen3-8b, qwen3-32b, qwen3-235b-a22b |
| Mistral | mistral-nemo, mistral-small-3.2, mistral-large-2512 |
| xAI | grok-4.20 |
| Amazon | nova-lite-v1, nova-pro-v1 |
| Cohere | command-r7b-12-2024 |
| Perplexity | sonar |

Reasoning models (DeepSeek R1, Qwen3 thinking variants) handled: `assert_openrouter_ok` accepts either `content` or `reasoning_content` being populated.

### 4 new cloud providers: OpenRouter, Kimi, Fireworks, Cerebras (v0.15.21)
Adds four OpenAI-compatible providers to all three picker surfaces (CLI `/provider`, TUI overlay, onboarding). Provider count grows from 15 to 19.

| Provider | Base URL | Notable models |
|---|---|---|
| OpenRouter | `https://openrouter.ai/api/v1/chat/completions` | 200+ models (Claude, GPT, Gemini, Llama) via single key |
| Kimi (Moonshot AI) | `https://api.moonshot.cn/v1/chat/completions` | `moonshot-v1-8k/32k/128k` — long-context |
| Fireworks AI | `https://api.fireworks.ai/inference/v1/chat/completions` | Fast open-model inference (Llama, DeepSeek, Qwen) |
| Cerebras | `https://api.cerebras.ai/v1/chat/completions` | Wafer-scale fastest inference (llama3.3-70b, qwen-3-32b) |

| Feature | File | Notes |
|---|---|---|
| CLI provider picker | `src/session/commands/provider.rs` | 4 entries added before `custom`; all use `OpenAiCompatible` wire format |
| TUI `/provider` overlay | `src/tui/turn_handler.rs` | Same 4 entries |
| TUI onboarding | `src/tui/startup.rs` | Same 4 entries |
| e2e provider count | `tests/provider_e2e.rs` | `all_expected_providers_present` asserts 19 slugs |

### Cohere model update (v0.15.21)
`command-r-plus` and `command-r` were retired by Cohere on September 15 2025 and now return 404. Updated all three pickers to current models.

| Old | New |
|---|---|
| `command-r-plus`, `command-r` | `command-a-03-2025`, `command-r7b-12-2024`, `command-r-08-2024` |

### Code index query visibility in tool output (v0.15.21)
`find_definition`, `find_references`, `who_calls`, and `code_map` now append the exact `sqlite3 .zap/code.db "..."` query that was fired to their output. Both the user and the LLM can see what was queried and re-run it manually without guessing the SQL.

| Feature | File | Notes |
|---|---|---|
| `find_definition` hint | `src/tools/search/search_impl.rs` | Appended on index hit: `# sqlite3 .zap/code.db "SELECT path, line, kind, signature FROM symbols WHERE name LIKE '%…%' COLLATE NOCASE LIMIT 20;"` |
| `code_map` hint | `src/tools/search/search_impl.rs` | Appended on index hit: `# sqlite3 .zap/code.db "SELECT name, kind, line, signature FROM symbols WHERE path LIKE '…%' ORDER BY path, line LIMIT 2000;"` |
| `find_references` / `who_calls` hint | `src/tools/search/mod.rs` | `format_call_sites` header includes `# sqlite3 .zap/code.db "SELECT … FROM call_sites …"` with actual limit |

### OpenAI Codex provider via ChatGPT subscription (v0.15.20)
Adds a Codex provider backed by the ChatGPT internal Responses API (`https://chatgpt.com/backend-api/codex/responses`). Only `gpt-5.5` is currently supported by the endpoint. Free-plan accounts are automatically detected via JWT `chatgpt_plan_type` claim and shown as unavailable in the picker.

| Feature | File | Notes |
|---|---|---|
| `CodexClient` | `src/llm_client/codex.rs` | SSE streaming, JWT expiry check + token refresh, `encode_input`/`encode_tools` for Responses API wire format |
| `check_codex()` | `src/llm_client/auth.rs` | Reads `~/.codex/auth.json`; decodes JWT to block free-plan accounts; respects `CODEX_HOME` env var |
| Provider routing | `src/llm_client/mod.rs` | `create_client` routes `provider_slug == "codex"` to `CodexClient` before standard OpenAI path |
| CLI/TUI provider pickers | `src/session/commands/provider.rs`, `src/tui/turn_handler.rs`, `src/tui/startup.rs` | Codex entry with `gpt-5.5` model list; shows ready/not-ready badge based on auth |
| Mistral parser extracted | `src/llm_client/tool_parsing.rs` | Moved `parse_mistral_tool_calls` out of `mod.rs` to stay under 600-line limit |
| Live e2e tests | `tests/provider_e2e.rs` | `cargo test -- --ignored` hits real Codex API; asserts `gpt-5.5` → 200, `gpt-5.4`/`gpt-5.3-codex` → 400 "not supported" |

### Dynamic LM Studio model list (v0.15.19)
LM Studio's model list was hardcoded — new models downloaded in LM Studio never appeared in zap's provider/model pickers. Now zap queries LM Studio's `/v1/models` endpoint at provider-selection time to show the actual available models. Falls back to a sensible default list if LM Studio isn't running.

| Feature | File | Notes |
|---|---|---|
| `fetch_openai_compatible_models` | `src/llm_client/mod.rs` | Sync HTTP request to `/v1/models`, parses `data[].id`, 5s timeout. Reusable for Ollama and other local providers. |
| CLI provider picker | `src/session/commands/provider.rs` | `models` field changed from `&'static [&'static str]` to `Vec<String>`; LM Studio entry uses dynamic fetch with hardcoded fallback |
| TUI onboarding | `src/tui/startup.rs` | Same dynamic fetch logic on first-launch provider picker |
| TUI `/provider` command | `src/tui/turn_handler.rs` | Same dynamic fetch logic on manual provider switch |

### Verify-aware progress watchdog (v0.15.17)
Bounds the cost of a stuck agent. Counts consecutive failing verification runs (shell exit ≠ 0) within a turn — catches a model trying *different* broken fixes, which identical-action loop detectors (OpenHands StuckDetector) miss. Born from SLM research Test 4: 13 minutes of methodical-but-fruitless debugging on one conditional.

| Feature | File | Notes |
|---|---|---|
| Streak tracking | `src/session/watchdog.rs`, `src/session/tools.rs` | Failing `shell` results increment; any successful shell resets (no false positives on long legitimate work). `AGENT_VERIFY_BREAKER_N` (default 3, 0 disables) |
| Rethink nudge (streak = N) | `src/session/watchdog.rs` | Injected into the failing tool result: stop editing, list 2-3 distinct root-cause hypotheses (incl. conditional/validation classes), test one directly |
| Escalation (streak = 2N) | `src/session/turn.rs` | Tools withdrawn for the rest of the turn; model must write a structured handoff summary (works/fails/files/hypotheses ruled out). User sees ⚠ watchdog warnings in TUI + REPL |


### Structured plan execution for SLMs (v0.15.18)
A frontier model pre-writes a step-by-step plan; an SLM executes it mechanically — one step, one verification, one result. Validated in Test 6.

| Feature | File | Notes |
|---|---|---|
| Plan execution system prompt | `src/context_manager.rs` | "Plan Execution (for Pre-Written Task Plans)" section: read all steps first, execute one at a time, verify after every edit, hard stop after 2 failures |
| Plan execution test | `src/context_manager.rs` (tests) | `system_prompt_contains_plan_execution` test — verifies the section and key rules are in the prompt |
| Structured task test (Test 6) | `research/slm-coding-eval/test6-structured/` | SLM follows a pre-written 4-step plan to add a REST endpoint, passes verification in 2 turns, 294s wall-clock. 100% first-attempt success vs. 50% for open-ended goals |

### Pre-index for SLM tasks (v0.15.18)
`zap --index-only` pre-builds the AST index (tree-sitter + SQLite) so the SLM uses `code_map`/`find_definition`/`find_references` instead of manual file reads. Documented in `docs/slm-support.md`.

### Escalation drill (Test 5)
Deliberately impossible task validates the watchdog detects verification loops and escalates. Watchdog nudge + tool-withdrawal works; escalation summary production is inconsistent (known limitation). Mitigation: use structured plans, not contradictory specs.

| File | Description |
|---|---|
| `research/slm-coding-eval/test5-escalation/` | Contradictory CSV parser spec — watchdog validates, escalation handoff inconsistent |
| `research/slm-coding-eval/test6-structured/` | Structured plan execution — SLM follows pre-written plan, 100% success |

### Skill prompt-bloat guardrails (v0.15.16)
External skill collections (Claude/Kiro-style SKILL.md) used to be classified always-on — mounting `~/.claude/skills` injected ~28k tokens into EVERY prompt, silently taxing cloud cost and making local models unusable. Verified: with 63 foreign skills mounted, the system prompt stays at ~3.2k tokens.

| Feature | File | Notes |
|---|---|---|
| Foreign skills never always-on | `src/skill_manager.rs` | Frontmatter with `description:` but no `triggers:` → Practice (triggered), with triggers derived from the skill name. Zap-native trigger-less skills stay Core |
| Always-on token budget | `src/skill_manager.rs` | `build_always_on_prompt_budgeted` caps the Core block at `skill_token_budget` (default 4000); ranked by priority → source → size; overflow dropped |
| User warning | `src/session/mod.rs` | Warns (REPL + TUI startup notice) when skills are dropped by budget or the always-on block exceeds ~2k tokens |

### SLM-friendly streaming + core tool profile (v0.15.15)
Local small models (LM Studio/Ollama) prefill big prompts for minutes — zap's plumbing must tolerate that and show progress. Born from the SLM research Test 3 (`research/slm-coding-eval/`).

| Feature | File | Notes |
|---|---|---|
| Streaming idle watchdog | `src/llm_client/openai.rs` | Streaming requests get a 1h cap instead of the 120s total timeout; a stuck server is detected by idle time (`AGENT_STREAM_IDLE_SECS`, default 600s) |
| First-token progress notices | `src/llm_client/openai.rs` | "⏳ waiting for first token (~Nk tokens, Ns elapsed)…" every 30s in TUI/REPL while the model processes the prompt |
| Stream-drop backoff | `src/session/turn.rs` | Retries back off 3/6/12/24s (max 4) — flat 3s retries stacked prefills against local servers and ground the machine down |
| Core tool profile | `src/session/history.rs`, `src/config.rs` | `AGENT_TOOL_PROFILE=core` (or `tool_profile` in `~/.agent.toml`) sends only 6 tool schemas: file ops + shell + search |

### Install UX polish (v0.15.13)
Friction-less first-run experience for new users coming from the website.

| Feature | File | Notes |
|---|---|---|
| `zap --version` flag | `src/cli.rs` | `#[command(version)]` wires the Cargo.toml version into clap's `--version` / `-V`. `install.sh` already calls it (line 62) and now reports the real version instead of "local" |
| README install table | `README.md` | Added macOS Intel (x86_64) row; the binary has shipped since v0.15.10 |
| Website install step 3 | `website/index.html` | "Run" → "Verify & run" with `zap --version` as the first command in each OS tab; gives new users a confirm step before they enter the REPL |

### Security hardening pass 2 (v0.15.12)
Takes the Mythos security posture from 8.0 → 9.0 (`docs/security-review-v3.md`). The four high-leverage levers from the v2 report.

| Feature | File | Notes |
|---|---|---|
| Egress scan at every source | `src/secret_scanner.rs`, `src/context_manager.rs`, `src/session/turn.rs` | Entropy detector for unprefixed random tokens (tuned to skip git SHAs/hex); injected project context (ZAP.md/context_paths) redacted before entering the system prompt; user's own message scanned and warned before a cloud send (warned, not auto-redacted) |
| `/remote` with per-session token | `src/remote.rs`, `src/tui/commands/mod.rs`, `src/session/mod.rs` | `generate_token()` (OS CSPRNG, URL-safe base64) appended to the printed URL; page + `/ws` upgrade require it via constant-time-ish `token_matches`; refuses to start in Auto permission mode; re-enables the feature removed in v0.15.11 |
| File-write jail | `src/tools/file/mod.rs`, `src/config.rs` | `guard_write_path` confines `write_file`/`edit_file`/`batch_edit` to the project root, system temp, or configured `allowed_paths` (new config field). Reads stay broad but symlink-safe + denylisted |
| Supply-chain CI gate | `.github/workflows/security-audit.yml`, `deny.toml` | `cargo audit` + `cargo deny check advisories bans sources` on every push/PR + weekly; denies yanked crates and unknown registries/git sources |

### Security hardening (v0.15.11)
Addresses the six findings from the independent Mythos security review (`docs/security-review.md`); results in `docs/security-review-v2.md` (posture 6.5 → 8.0).

| Feature | File | Notes |
|---|---|---|
| `/remote` disabled | `src/tui/commands/mod.rs` | The remote server tunneled the session to a public URL with no WebSocket auth (read stream + inject prompts; shell exec in Auto mode). Start path is now a no-op with an explanatory message; `/remote stop` still tears down an old server. Returns once a per-session token gate lands |
| Project-trust gate | `src/trust.rs`, `src/hooks.rs`, `src/mcp.rs` | `project_trusted()` gates project-local `.zap/hooks.json` and `.mcp.json` behind explicit trust (`ZAP_TRUST_PROJECT=1`, `.zap/trusted` marker, or `~/.zap/trusted_dirs`). Global `~/.zap/` config always loads. Cloning + opening an untrusted repo no longer runs its `SessionStart` hook or spawns its MCP servers |
| Symlink-safe path guard | `src/tools/file/mod.rs` | `resolve_symlinks()` canonicalizes the nearest existing ancestor before the denylist check, closing the bypass where a project symlink pointed at `~/.ssh/id_rsa`. Works for create-new-file writes too |
| Hardened credential denylist | `src/tools/file/mod.rs` | `guard_path` denylist expanded to cover SSH/GPG key files by name, all major cloud credential stores, VCS/registry tokens, DB credentials, and shell history |
| Broader secret pre-flight scan | `src/secret_scanner.rs` | Pattern set ~doubled: Google `AIza`, Hugging Face, more GitHub prefixes, Slack, Azure connection strings/keys, npm `_authToken`, OAuth `client_secret`, bearer headers, credentialed DB URLs. Over-broad needles deliberately rejected |
| Session DB `0600` | `src/persistence.rs` | `~/.zap/agent.db` (plaintext conversation history) restricted to owner read/write at open, mirroring `~/.agent.toml` |

### Test infrastructure (v0.15.4)
| Feature | File | Notes |
|---|---|---|
| Mock `LlmProvider` | `src/llm_client/mock.rs` | `MockClient` — cheap-to-clone (`Arc<MockState>`), scripted `ApiResponse` queue + recorded calls. `text(...)` and `tool_call(...)` builders for end-turn vs tool-use responses. Compiled only under `#[cfg(test)]` |
| `Session::new_for_test` | `src/session/test_factory.rs` | Minimal session constructor for tests — in-memory `persistence::Store`, in-memory `CodeIndex`, empty `HookRunner`/skills, `permission_mode=Auto`, `is_subagent=true`. Skips skill bootstrap, MCP load, project banners |
| `Store::open_in_memory` | `src/persistence.rs` | Test-only in-memory sqlite store so tests never touch `~/.zap/agent.db` |
| Agent-loop tests | `src/session/agent_loop_tests.rs` | Deterministic coverage of `handle_user_turn` — single text turn, one tool round (real `read_file` against tempfile), runaway-tool-call cap at `MAX_TURNS` |

### Documentation (README.md)
| Feature | File | Notes |
|---|---|---|
| Context quality principle | `README.md` | New "Context Quality is Supreme" section — states the guiding principle, shows what gets updated and when |
| /init command section | `README.md` | New "6. /init" section in "What makes zap different" — step-by-step flow, example ZAP.md output, update timing table |
| Features table /init row | `README.md` | New "Project init" row in Features at a Glance table |

### Windows fixes (v0.15.1)
| Feature | File | Notes |
|---|---|---|
| Mouse scroll on Windows | `src/tui/mod.rs`, `src/tui/lifecycle.rs` | Enable `EnableMouseCapture`; handle `Event::Mouse ScrollUp/Down` in event loop — fixes mouse wheel injecting `[A[B` escape sequences into prompt on Windows cmd/ConPTY. **Superseded in v0.15.25** (capture removed, broke copy/paste) **then v0.15.28** (root-caused via DECSET 1007 instead of capture — see top of file) |
| Permission popup frozen on Windows | `src/tui/turn_handler.rs` | Replace blocking `poll(Duration::ZERO)+read()` with async `EventStream` arm in `tokio::select!` — fixes FOCUS_EVENT freezing the Tokio executor so Y/N/A key presses never dismissed the popup |
| `dir /s` unblocked | `src/tools/shell.rs` | Removed from hard-blocked patterns (Windows handles junction cycles; 60s timeout is the safety net); added missing Windows destructive patterns: `rmdir /s`, `rd /s`, `del /f /s`, `format c/d/e:`, `reg delete` |

### Session resume (v0.15.2)
| Feature | File | Notes |
|---|---|---|
| | Restore conversation history on resume | `src/persistence.rs`, `src/session/mod.rs` | `load_previous_messages()` finds the prior session and deserializes its saved `session_messages` JSON back into `Vec<Message>` — on resume, the LLM now sees the full previous conversation, not just the `context.md` handoff summary |

### Anthropic efficiency (v0.15.4)
| Feature | File | Notes |
|---|---|---|
| | Full prompt caching (history breakpoint) | `src/llm_client/anthropic.rs` | Adds `cache_control: ephemeral` on the last conversation message so Anthropic reuses the conversation prefix across turns instead of re-billing it |
| | Model-aware output token cap | `src/llm_client/anthropic.rs` | Replaces hardcoded 16k max_tokens with `max_output_tokens()` — Opus/Sonnet → 32k, others → 16k |
| | Default model bumped | `src/config.rs` | Anthropic default from `claude-opus-4-7` → `claude-opus-4-8` |
| Resume history token-budget guard | `src/session/mod.rs` | `load_and_guard_previous_messages` — apply `windowed_history` (last 8 user turns, tool results outside last 2 turns pruned) plus a token cap at 30% of the model's context window, dropping oldest user+assistant pairs until under budget |

### Tools (v0.15.4)

### Correctness (v0.15.6)
| Feature | File | Notes |
|---|---|---|
| batch_edit count fix | `src/tools/file/edit.rs` | Replacement counts now tracked during validation pass (original content) instead of apply pass (mutated content), preventing drift when later edits introduce text matching earlier edits' old_strings |
| Panic audit — bare `unwrap()` → `expect()` | `src/llm_client/mod.rs`, `src/remote.rs`, `src/context_manager.rs`, `src/ui.rs`, `src/session/commands/index.rs`, `src/session/commands/memory.rs` | 9 risky bare `unwrap()` calls replaced with `expect()` across 6 files. All remaining unwraps confined to `#[cfg(test)]` blocks only || | Shell isolation — sandbox modes | `src/config.rs`, `src/tools/shell.rs`, `src/shell_runner.rs`, `docs/SECURITY.md` | Added `sandbox` config (off/workdir/container). `workdir` jails `current_dir` to project root. `container` wraps commands in disposable Docker/Podman container (`--network none`, read-only mount). Wrote full threat model doc |

### Code graph v2 (v0.15.0)
| Feature | File | Notes |
|---|---|---|
| Type edges | `src/code_index/index_impl.rs`, `src/code_index/extract_rust.rs`, `src/code_index/extract_python.rs`, `src/code_index/extract_js.rs`, `src/code_index/extract_java.rs`, `src/code_index/extract_csharp.rs` | `type_edges` table: extends/implements for Rust, Python, JS/TS, Java, C#; structured `return_type` + `params` columns on symbols |
| Import-aware resolution | `src/code_index/index_query.rs` | `resolve_call` — jump from call site to definition via import graph; `find_subtypes_of` / `find_supertypes_of` / `find_by_return_type` queries |
| Context packer | `src/code_index/index_pack.rs` | `pack_context(task, budget)` — budget-proportional one-hop expansion (budget/200, clamp 10–50 symbols) for targeted context injection |
| Quality report | `src/code_index/index_quality.rs` | `quality_report()` — god objects, large files, high coupling, dead code candidates, health score |


| Feature | File | Notes |
|---|---|---|
| Mode picker | `src/task_planner.rs:pick_session_mode` | inquire Select at REPL startup: Vibe / Task |
| Goal prompt | `src/task_planner.rs:run_task_planning` | freeform "what do you want to build?" |
| Clarifying questions | `src/task_planner.rs:fetch_clarifying_questions` | LLM call → JSON array of 2-3 Qs |
| Structured plan | `src/task_planner.rs:fetch_task_plan` | LLM call → JSON: tasks + suggested_skill + verify |
| Skill matching | `src/task_planner.rs:parse_task_plan` | LLM suggests skill from available list |
| Missing skill resolution | `src/task_planner.rs:resolve_missing_skills` | prompts user → creates stub skill if needed |
| Stub skill creation | `src/task_planner.rs:create_skill_stub` | calls `skill_manager::create_skill` + appends context |
| tasks.md writer | `src/task_planner.rs:write_tasks_md` | `.zap/tasks/<slug>/tasks.md` with skill annotations |
| Plan summary | `src/task_planner.rs:print_plan_summary` | numbered task list with skill tags in terminal |
| Session pre-load | `src/agent_core.rs:run_repl` | plan goal sent as first user turn after task mode |

### Module structure
| Module | File | Responsibility |
|---|---|---|
| Session core | `src/session/mod.rs` | struct, `new()`, slash dispatcher, context helpers |
| Session turn | `src/session/turn.rs` | `handle_user_turn` — LLM loop, compaction, streaming |
| Session tools | `src/session/tools.rs` | `execute_tool_round` — permissions, parallel execution, secrets scan |
| Session history | `src/session/history.rs` | windowed history, token limits, tool-result pruning |
| Session preview | `src/session/preview.rs` | `smart_tool_preview` — tool-specific one-liner summaries |
| Session casual | `src/session/casual.rs` | casual-message detection, context injection rules |
| Slash commands | `src/session/commands/` | 10 focused submodules (code, index, tasks, git, skills, memory, media, provider, session_mgmt, info) |
| Code index | `src/code_index/mod.rs` | global singleton, `spawn_background_indexer`, public types |
| Code index impl | `src/code_index/index_impl.rs` | `open`, `index_dir`, `stats` (query/pack/rank/quality delegated to submodules) |
| Code index walk | `src/code_index/walk.rs` | filesystem walk, language detection, mtime helpers, row helpers |
| Code index extract (dispatcher) | `src/code_index/extract.rs` | `extract_all` dispatcher + shared helpers; per-language extraction in submodules below |
| Extract: Rust | `src/code_index/extract_rust.rs` | tree-sitter Rust extraction — fn, struct, enum, trait, impl, macro, use tree flattening |
| Extract: Python | `src/code_index/extract_python.rs` | tree-sitter Python extraction — class, def, call, import flattening |
| Extract: JS/TS/TSX | `src/code_index/extract_js.rs` | tree-sitter JS/TS extraction — function, class, var decls, call, import/require |
| Extract: Go | `src/code_index/extract_go.rs` | tree-sitter Go extraction — func, type, method, call, import flattening |
| Extract: Java | `src/code_index/extract_java.rs` | tree-sitter Java extraction — class, method, invocation, object creation, import |
| Extract: C# | `src/code_index/extract_csharp.rs` | tree-sitter C# extraction — class, method, property, invocation, using |
| Index query | `src/code_index/index_query.rs` | `find_definition`, `symbols_in_path`, `search`, `find_references`, `callers_of`, `imports_for`, `importers_of`, `users_of_module`, `find_subtypes_of`, `find_supertypes_of`, `find_by_return_type`, `resolve_call` |
| Index rank | `src/code_index/index_rank.rs` | `compute_file_ranks`, `rank_files(n)`, `file_rank(path)` — import-aware PageRank edge building |
| Index pack | `src/code_index/index_pack.rs` | `pack_context(task, budget)` — budget-proportional one-hop expansion (budget/200, clamp 10–50 symbols) |
| Type edges | `src/code_index/index_impl.rs` | `type_edges` table: extends/implements for Rust, Python, JS/TS, Java, C#; structured `return_type` + `params` columns on symbols |
| Index quality | `src/code_index/index_quality.rs` | `quality_report()` — god objects, large files, high coupling, dead code candidates, health score |
| TUI render | `src/tui/render/` | 7 focused submodules (messages, layout, header, overlays, diff, dialogs) |
| TUI commands | `src/tui/commands/` | command picker, filter/resolve, handle_inline; text builders in text.rs |
| TUI turn handler | `src/tui/turn_handler.rs` | `run_normal_turn`, `handle_tui_slash` — main turn/slash routing |
| TUI startup | `src/tui/startup.rs` | session replay, welcome messages, session load |
| TUI goal | `src/tui/goal.rs` | `/goal` command handler, completion detection, tool-expand cycling |
| TUI lifecycle | `src/tui/lifecycle.rs` | suspend/resume terminal, dir picker, task-planning flow |
| TUI git info | `src/tui/git_info.rs` | branch name, dirty/ahead/behind status, diff shortstat |
| Theme constants | `src/ui.rs:theme` | named colour palette (PRIMARY, MUTED, BORDER, …) |
| inquire picker style | `src/ui.rs:inquire_render_config` | shared RenderConfig for all pickers |

### Core agent loop
| Feature | File | Notes |
|---|---|---|
| REPL (interactive) | `src/agent_core.rs:run_repl` | rustyline, tab completion, slash picker |
| Windows ANSI colors | `src/main.rs` | `set_virtual_terminal(true)` at startup; renders correctly in CMD and PowerShell |
| Single-shot mode | `src/agent_core.rs:run` | `--goal "..."` flag |
| Sub-agent spawning | `src/agent_core.rs:run_subagent` | `--agent-depth N` enables; returns JSON: summary, files_changed, turns, token usage |
| Sub-agent orchestration prompt | `src/context_manager.rs` | LLM taught trigger patterns, anti-patterns, and how to announce parallel plans |
| Sub-agent startup suppression | `src/session/mod.rs` + `src/config.rs:is_subagent` | sub-agents don't reprint banners; clean parallel output |
| Sub-agent Auto permission mode | `src/agent_core.rs:run_subagent` | sub-agents forced to Auto to prevent stdin deadlock with parent session |
| Sub-agent depth tracking | `src/config.rs:spawn_depth` | nesting level tracked in config; L1/L2/L3 labels always correct |
| `spawn_agent` permission gate | `src/permission_manager.rs` | spawn_agent now requires user approval (was auto-approved) |
| `files_in_scope` schema field | `src/tools/agent.rs` | advisory list of files each sub-agent will touch; visible in permission prompt |
| `files_changed` via trait | `src/agent_core.rs:run_subagent` | uses `Tool::affected_path()` instead of hardcoded tool name list |
| Parallel tool execution | `src/session/mod.rs:handle_user_turn` | `join_all` after permission phase |
| Ctrl+C cancellation | `src/session/mod.rs` | `tokio::select!` around turn loop |
| Fix: "Press any key" no longer hangs on Ctrl+C | `src/tui/mod.rs` | After a complex command (e.g. `/tasks`) runs in suspended terminal, the "Press any key to return" wait now uses raw-mode `crossterm::event::read()` instead of `stdin.read_line()`. With `read_line`, tokio installs SIGINT handler with SA_RESTART so Ctrl+C was silently retried forever — TUI appeared completely hung. Raw-mode read receives Ctrl+C as a key event (not SIGINT), so any keypress returns to the TUI immediately. |
| Ctrl+C cancels through popups | `src/tui/mod.rs` | Ctrl+C now checked before popup routing — dismisses permission/secret popup (sends Deny/false) then cancels turn; previously fell through to `_ => {}` when any popup was active |
| Non-blocking permission + secret prompts | `src/permission_manager.rs`, `src/session/mod.rs`, `src/tui/channel.rs`, `src/tui/app.rs` | Replaced `std::sync::mpsc::SyncSender` + blocking `rx.recv()` with `tokio::sync::oneshot` + async `.await` — TUI tick loop stays unblocked during permission/secret popups so Ctrl+C always works |
| Mid-turn btw injection (Ctrl+B) | `src/tui/channel.rs`, `src/tui/app.rs`, `src/tui/input.rs`, `src/tui/mod.rs`, `src/tui/render.rs`, `src/session/mod.rs` | Ctrl+B during an active turn opens a blue input box; user types a note, Enter queues it via `BTW_QUEUE`; session drains the queue between tool-call rounds and appends as "↳ User note (added mid-turn): …" to the tool-results message so the model sees it next iteration; displayed in chat with `↳ btw:` prefix. If the turn ends before the next tool call (no-tool final response), leftover btw messages are surfaced as `BtwCarryover` TUI event and auto-submitted as the next user turn so they always get a response. |
| Turn counter + ctx% in prompt | `src/agent_core.rs` | `[N:branch\|42%] ❯`; % colour-coded at 70/85% |
| Session branching | `src/session/commands.rs:cmd_branch/switch/merge` | SQLite-backed; `/branch`, `/switch`, `/merge` |
| Context bar in turn footer | `src/session/mod.rs:ctx_bar` | `[████████░░] 42%` after every LLM response |
| Model-aware context limits | `src/session/mod.rs:model_context_limit` | Claude 200k, GPT-4o 128k, local 32k |
| Context pressure thresholds | `src/session/mod.rs:handle_user_turn` | Silent auto-compact at 90%; reactive overflow (compact+retry on API "too long" errors and empty 0-token responses); circuit breaker after 3 failures; `DISABLE_COMPACT` and `ZAP_MAX_CONTEXT_TOKENS` env vars |
| ZAP.md caching | `src/session/mod.rs:handle_user_turn` | Skill-triggered turns append to cached `self.system` instead of re-reading CLAUDE.md from disk; also stabilises Anthropic prompt-cache prefix |
| Tool result truncation | `src/session/mod.rs:handle_user_turn` | tool outputs capped at 20 000 chars before being sent to LLM; prevents context overflow on large file/dir reads |
| Empty response detection | `src/session/mod.rs:handle_user_turn` | two-case detection: zero input_tokens → reactive compact+retry, then warn with ctx size; non-zero input_tokens → proxy/gateway dropped body (warns with stop_reason + log path) |
| Multi-turn history fix | `src/session/mod.rs:handle_user_turn` | assistant response is now always pushed to `self.messages` before the tool-calls check; previously text-only turns were not saved, breaking context on subsequent turns |
| Proxy tool_use parse warning | `src/session/mod.rs:handle_user_turn` | when `stop_reason=tool_use` but no tool blocks were parsed, warns about unified/normalized proxy schema instead of silently breaking |
| Secret scanner native TUI popup | `src/tui/channel.rs`, `src/tui/app.rs`, `src/tui/input.rs`, `src/tui/mod.rs`, `src/tui/render.rs`, `src/session/mod.rs:handle_user_turn` | Replaces suspend/resume terminal hack with a channel-based TUI-native overlay (amber-toned popup at screen bottom listing hits; Y=send anyway, any other key=cancel turn). In TUI mode no terminal manipulation happens — ratatui stays in control throughout. CLI mode retains suspend/resume with the post-resume println! bug fixed. |
| TUI stdout audit — hook + LLM retry output | `src/hooks.rs:run_hook`, `run_pre_hook`, `src/llm_client.rs` | Comprehensive scan of all println!/print! calls that could fire while ratatui owns the terminal. Hook errors/warnings/stdout and LLM 429/503 retry warnings now route through `tui_send(LlmChunk)` in TUI mode instead of writing raw text to the alternate screen buffer. All other risky sites were already gated with `is_tui_mode()` checks. |
| SSE stream-drop auto-retry | `src/session/mod.rs:handle_user_turn` | When the server drops the SSE connection mid-stream ("SSE stream error", "connection reset", "broken pipe", "incomplete message"), the turn retries automatically after a 3s pause with a ⚠ notice in chat/log. Sits alongside the existing overflow-compact retry in the same match arm so both recoveries share the same loop. |
| Reasoning and Investigation system prompt (v0.13.60) | `src/context_manager.rs` | New "Reasoning and Investigation" section: (1) Decompose before acting — question "which X does codebase use?" ≠ find X's definition; means all instantiation/call/indirect sites; (2) Completeness mindset — empty result = "not found this way", partial = "found some", always check other forms; direct/wrappers/aliases/injected instances/dynamic dispatch each need different search pass; (3) Synthesise don't list — integrate findings into conclusions, state found-directly vs inferred vs what static search cannot see. Merged prior "Semantic Search Strategy" + "Finding All References" sections into one tighter "Search and Discovery" section to keep file under 600 lines. |
| Windows TUI hang fix + call-site system prompt (v0.13.59) | `src/tui/mod.rs`, `src/context_manager.rs` | (1) **Windows hang**: idle loop `poll()+read()` replaced with async `EventStream` — on Windows, `read()` blocks indefinitely when only MENU_EVENT/FOCUS_EVENT records are in the console buffer; EventStream runs the blocking call on a background OS thread and signals tokio. (2) **System prompt gap**: new "Finding All References / Call Sites" section — LLM was answering "what does this codebase use for X?" using only `find_definition` (definitions), never tracing call sites; rule now mandates `search_code` after `find_definition` for enumeration queries, and forbids inferring usage from comments. |
| Code indexing demo + `--index-only` flag (v0.13.58) | `src/cli.rs`, `src/code_index/mod.rs`, `demos/code_indexing/` | New `zap --index-only` flag: runs tree-sitter scan + SQLite write for the current directory and exits — no session, no LLM. Enables headless CI indexing and setup scripts. New `demos/` folder with first capability showcase: `demos/code_indexing/` targets pallets/flask (Python, 83 files, 1577 symbols). `setup.sh` clones Flask at tag 3.1.0 and runs `zap --index-only`. `run.sh` fires 3 live scenarios via `--sdk --auto`: (1) find Flask class → `find_definition` INDEX hit → exact file:line in 1 call; (2) trace HTTP request path across 5 files with line-precise navigation; (3) Blueprint API surface via `code_map` without reading full file. All 3 verified live against Deepseek. |
| Tests: system prompt contracts + shell timeout + SDK E2E (v0.13.57) | `src/context_manager.rs`, `src/shell_runner.rs`, `src/config.rs`, `tests/sdk_e2e.rs` | Unit tests verifying B2-B5 contracts: (1) 4 tests in `context_manager` assert system prompt contains `batch_edit`, `find_references`, `Cargo.toml`/`Exception` carve-out, and that casual prompt omits tool policy; (2) 3 async tests in `shell_runner` assert `run_command_timeout` times out in ~1s, completes short commands, and includes the timeout value in the error message; (3) 3 ignored E2E tests in `tests/sdk_e2e.rs` spawn `zap --sdk --auto` as a subprocess and test: smoke greeting, shell tool output (echo zaptest123), and shell timeout abort. E2E tests run against Deepseek in 9.5s. `Config::default()` added (cfg(test) only) for compact test setup. |
| System prompt + shell hardening (v0.13.56) | `src/context_manager.rs`, `src/tools/shell.rs`, `src/shell_runner.rs` | Four backlog items: (B2) Tool Usage Policy now guides `batch_edit` for multi-edit same-file tasks; (B3) `find_references` called out as required before rename/delete with text-search caveat; (B4) shell `timeout` param now wired through — `{ "timeout": 120 }` is respected up to 300s cap, description updated from "30s" to "60s default, max 300s"; (B5) `code_map` "always first" rule now has explicit carve-out for config/manifest files (Cargo.toml, package.json, go.mod, etc.) that can be read directly. |
| Tests: parse_markdown Windows path escaping (v0.13.55) | `src/tui/syntax.rs` | 3 unit tests: `windows_backslash_dot_preserved` (asserts `\.kiro` survives markdown rendering), `unix_path_unchanged` (forward-slash paths pass through), `markdown_formatting_still_works` (bold and code spans unaffected by the fix). |
| Fix Windows path corruption in TUI markdown renderer (v0.13.54) | `src/tui/syntax.rs`, `src/tools/file.rs` | pulldown-cmark treats `\.` (backslash + dot) as a CommonMark escape, consuming the backslash — so `cicrm-react\.kiro` rendered as `cicrm-react.kiro` in the TUI. Fix: pre-escape all `\.` → `\\.` in the text before parsing (only when `\.` is present, zero cost otherwise). Also changed write_file success/error messages to output absolute paths with forward slashes so the LLM echoes cross-platform-safe paths and avoids the issue at the source. |
| write_file success response includes resolved absolute path (v0.13.53) | `src/tools/file.rs` | Success message now returns "wrote N bytes to 'rel/path' (C:\abs\path)" — the LLM sees where the file actually landed and can detect CWD mismatches immediately (e.g. `cicrm-react.kiro\specs\...` instead of `cicrm-react\.kiro\specs\...`). Previously only errors reported the resolved path; silent wrong-location writes went undetected. |
| write_file error messages include resolved absolute path (v0.13.52) | `src/tools/file.rs` | `create_dir_all` and `tokio::fs::write` errors now report the resolved absolute path (via `normalize_path`) alongside the original path string. Previously the OS error appeared without context, making it impossible to tell whether the failure was a permissions issue or a working-directory mismatch (especially on Windows where the CWD may differ from the repo root). |
| Hybrid execution model — skill + index + todo (v0.13.51) | `src/context_manager.rs` | Replaced fragile "forbidden phrases" rule with Claude Code's todo-commitment pattern. Task Tracking now says: when a skill triggers, write ALL steps as todos first, then execute every item in the same turn without stopping between them. The todo list is the execution contract. Code index answers structural questions inline so the model never stops to ask the user what already exists. Skills provide domain knowledge + workflow steps; index provides structural navigation; todos drive continuous execution — best of all three. |
| Ban turn-ending "next I will" announcements (v0.13.50) | `src/context_manager.rs` | LLM was ending turns with "Next I will use systematic debugging to find RCA..." forcing the user to reply to continue. Different from asking a question — it narrated a future step and stopped. Added explicit rule: "Next I will / I'll now proceed to / The next step is" are forbidden as turn-ending statements; if there is a next step, execute it immediately in the same turn. |
| Remove pre-search narration line (v0.13.49) | `src/context_manager.rs` | Semantic Search Strategy instructed the LLM to write "Searching index for: ..." before making tool calls. This contradicted "do not narrate what you are about to do" and caused the TUI to show only the narration text when the subsequent tool calls froze the UI. Removed the pre-call line; only the trailing "Found via index / Not found" summary remains. |
| Skill workflow: no clarifying questions (v0.13.48) | `src/context_manager.rs` | LLM was stopping mid-skill to ask the user for reproduction steps / confirmation instead of gathering information via tools (code search, MCP, index). Added explicit rule to Response Style: when a skill workflow is triggered, execute it fully using tools — only pause for destructive actions or when the skill explicitly requires user input. |
| Background indexer block_in_place (v0.13.47) | `src/code_index/mod.rs` | Background indexer called `index_dir` synchronously on a tokio worker thread, consuming one of the N async worker threads for the full parse+SQLite duration. On a 2-core machine this starved the async pool — TUI stopped responding to keypresses (Enter, Ctrl+Q/C). Wrapping with `tokio::task::block_in_place` signals the runtime to spin up a replacement worker thread during the blocking call, keeping the async pool at full capacity. |
| Exit hang fix — force-exit after save (v0.13.46) | `src/tui/mod.rs`, `src/agent_core.rs` | After `save_context_with_summary` + `fire_session_end` complete, call `std::process::exit(0)` instead of returning through the tokio runtime. Tokio runtime shutdown blocks waiting for background tasks (indexer holding a worker thread) to complete their current synchronous section — the process hung indefinitely. Force-exit bypasses this; the OS cleans up all resources. Combined with the 5s LLM timeout (v0.13.45), exit is now reliable and fast. |
| Readonly DB bail-early + indexer auto-stop (v0.13.43) | `src/code_index/index_impl.rs` | `index_dir` breaks out of the file loop on first readonly error (prevents holding the code_index mutex for O(N) files → fixes TUI hang); returns `Err` when `files==0 && skipped>0` so the background indexer's `consecutive_errors` counter increments properly and the indexer stops after 3 failures instead of retrying every 120s forever |
| Kiro skills subdirectory support (v0.13.42) | `src/skill_manager.rs` | `load_all_skills` now checks each directory entry: if it's a dir containing `SKILL.md`, loads it as a Kiro-format skill using the **directory name** (not "SKILL") as the skill name — `parse_skill_file` returns file_stem which would be "SKILL", so name is overridden with `path.file_name()` after loading |
| Index flood + TUI hang fix (v0.13.37) | `src/code_index/index_impl.rs`, `src/code_index/mod.rs`, `src/tui/turn_handler.rs` | WAL pragma non-fatal (fallback to DELETE on Windows locked .db-shm); index_dir batches per-file errors into one summary WARN instead of flooding TUI chat; background indexer stops after 3 consecutive failures; TUI event drain capped at 64/tick so warning floods cannot starve the spinner |
| Fix async test isolation for todo global state (v0.13.40) | `src/tools/todo.rs` | Replaced locked_clean pattern (released lock before async body) with OnceLock<tokio::sync::Mutex<()>> held for entire async test; sync tests hold std::sync::Mutex for full test body; async tests that only verify execute() return strings no longer access global_todos() at all, eliminating the race |
| Test coverage — todo tools + extract_whats_next (v0.13.40) | `src/tools/todo.rs`, `src/project.rs` | 19 new unit tests: TodoStatus/Priority parsing, global state round-trip, TodoWriteTool::execute (normal, empty, missing key, missing fields), TodoReadTool::execute (empty, with items), extract_whats_next (content, placeholder, absent, blank, section boundary, whitespace trim, multiline); from_str methods made pub(crate); extract_whats_next made pub(crate); total 126 tests passing |
| Session task tracking — todo_write/todo_read tools (v0.13.39) | `src/tools/todo.rs`, `src/tools/mod.rs`, `src/tui/render/layout.rs`, `src/context_manager.rs`, `src/session/mod.rs` | Two new tools: `todo_write` replaces the full task list (id, content, status, priority); `todo_read` returns the current list. Global `Mutex<Vec<TodoItem>>` cleared at session start (no persistence). System prompt instructs LLM to create a list when given ≥3-step tasks, mark items in_progress/done as it works. TUI sidebar shows a "tasks N/M" section with ○/◑/● icons and priority-coloured text when any tasks exist. |
| session_log.md what's-next persistence | `src/project.rs`, `src/session/commands/code.rs`, `src/agent_core.rs` | `append_session_log` now accepts `whats_next: Option<&str>` and writes a `Next:` line (first bullet from the LLM summary) into each session_log.md entry; call site in `save_context_inner` threads through the existing `whats_next` parameter; single-shot (`--goal`) mode now also calls `save_context_with_summary` on exit so context.md and session_log.md are written in CLI/scripting flows; T05d e2e test added; fixed: all bullet lines now joined with ` \| ` separator (was silently dropping lines 2-3); T05d anchored to `^Next:` and guards offline CI with LLM-availability check |
| Automated session continuity (v0.13.38) | `src/project.rs`, `src/session/commands/code.rs`, `src/tui/mod.rs`, `src/agent_core.rs`, `src/context_manager.rs` | On-exit LLM call (`summarize_whats_next`, 20s timeout) generates 1-3 bullet "What's next" summary from last 10 messages; `save_session_context` now accepts `whats_next: Option<&str>` and preserves existing content when `None`; fixed overwrite bug (was always writing blank placeholder); `save_context_with_summary` async replaces `save_context` at both exit points (TUI + REPL); removed duplicate context.md hint from context_manager (already injected at startup); added `/memory set` proactive note to agent memory system prompt section |
| TUI streaming auto-scroll fix | `src/tui/app.rs:apply_event` | `auto_scroll` is re-enabled on every `LlmChunk` so the viewport follows active streaming output even if the user scrolled up earlier in the turn; previously scrolling up mid-response caused the rest of the output to appear off-screen |
| Rotating thinking words | `src/tui/render.rs:THINKING_WORDS` | 200-word rotation; per-turn prime offset (`turn * 31`) ensures each response starts at a different word; ~640ms change interval (down from 1.3s) so variety is visible in short turns; status bar, sidebar, and chat all use the same index |
| TUI visual overhaul (v0.7.1) | `src/tui/render.rs` | Amber gradient ZAP art, muted purple `#3c3750` borders, `◆ You`/`◆ zap` role markers, diff-aware tool preview (+green/-red/@@blue), `✓`/`✗`/`⏺` tool icons with elapsed time, Ctrl+O expands last tool output (shows +N lines hint), dir picker moved to Ctrl+P |
| Domain scope picker in TUI | `src/tui/`, `src/skill_manager.rs`, `src/config.rs` | At TUI session start, if no scope auto-detected, shows ratatui overlay "existing project: scope to languages?" with project dir name; extension-based pre-checking (`.rs`→rust, `.py`→python, etc.); `skip_domain_prompt` flag prevents duplicate CLI inquire prompt; Esc = no restriction |
| Clean TUI startup (v0.7.2) | `src/tui/mod.rs`, `src/config.rs:tui_mode`, `src/session/mod.rs` | `tui_mode` flag suppresses all startup println!s (skills/hooks/MCP); Vibe/Task mode picker is now a TUI overlay (amber ❯ selection, descriptions); Task mode suspends TUI, runs task planning in CLI, resumes; skill/domain info shown in welcome message instead |
| TUI color + rendering fixes (v0.7.3) | `src/tui/render.rs` | Replace `│` (U+2502) markers with plain spaces in tool_call_lines and diff_block_lines — U+2502 rendered as 'd' on many terminals causing '181d' artifacts. Brighten all muted colors: preview text Rgb(205,200,225), context diff lines Rgb(175,170,200), tool names/labels, elapsed times, code block line numbers. Replace all `Color::DarkGray` with explicit Rgb values for consistent cross-terminal contrast. |
| TUI preview width clamp + scatter fix (v0.7.4) | `src/tui/render.rs` | Pass `width` through `tool_call_lines` and `diff_block_lines`; truncate every preview/diff line to `width-6` chars with `…` suffix — prevents long lines overflowing into sidebar and soft-wrapping as scattered characters. Tab expansion (`\t` → 4 spaces) stops tab-indented code rendering as giant gaps. Sidebar and header-info label/value colors brightened (Rgb(130,125,155) labels, Rgb(205,200,230) values). |
| Fix green/red diff lines leaking into TUI (v0.7.9) | `src/tools/file.rs` | `print_diff()` called `println!` with ANSI colored `+`/`-` lines directly to stdout inside `edit_file` and `batch_edit`, bypassing TUI mode — the output wrote into the alternate screen buffer mid-render. Gated both `print_diff()` calls with `if !is_tui_mode()` to match the existing session-level `print_tool_output` gate. |
| Fix `/sessions` resume — session_id + model not updated (v0.10.2) | `src/session/commands.rs:cmd_sessions` | After loading an old session, `self.session_id` was never updated so all subsequent saves wrote to the new (empty) session instead of the resumed one. Also updates `self.model` and rebuilds `self.client` to match the loaded session's model. |
| Session content display (goal + files) on select and startup (v0.12.4) | `src/session/commands.rs:cmd_sessions`, `src/session/mod.rs`, `src/project.rs` | Selecting a session via `/sessions` now shows the full goal + files (parsed from `session_log.md`), not just the goal — even works for sessions with no saved messages. Startup banner shows full last-session goal + files on separate lines instead of a truncated one-liner. Added `session_log_files()` helper to parse files from `session_log.md` by session ID. |
| TUI diff viewer + view-changes hint (v0.11.3) | `src/tui/app.rs`, `src/tui/input.rs`, `src/tui/mod.rs`, `src/tui/render.rs` | After any turn where the agent writes/edits files, shows "✎ N files modified — Ctrl+G or /diff to view changes" in the chat. `Ctrl+G` opens the diff viewer from idle mode (previously `OpenDiffViewer` action existed but had no keybinding). Status bar keybinds updated to include `Ctrl+G diff`. `files_changed_this_turn` counter tracks `write_file`/`edit_file`/`batch_edit` ToolDone events and resets at turn start. |
| Tree-sitter logging (v0.11.1) | `src/code_index.rs:index_file`, `global_reindex_file`, `index_dir` | Every parse, reindex, and scan writes `INDEX tree-sitter · <lang> · <path> · N symbols` to `~/.zap/zap.log`; `/index` output names tree-sitter explicitly; reindex-after-edit was previously silent |
| Periodic background indexer (v0.11.1) | `src/code_index.rs:spawn_background_indexer`, `src/session/mod.rs:Session::new` | Spawns a tokio task at session start that runs `index_dir` every 120s (`ZAP_INDEX_INTERVAL` env var); uses `try_lock` to avoid blocking foreground reindex; logs to `zap.log` only — never pollutes stdout/TUI |
| Index hit/miss logging per turn | `src/tools/search.rs:find_symbol_definition`, `build_code_map` | Every `find_definition` and `code_map` call logs whether it hit the AST index or fell back to grep: `INDEX hit · find_definition · 'SymbolName' · N result(s)` / `INDEX miss · find_definition · 'X' · grep fallback`; also appended to `~/.zap/audit.jsonl` with `op`, `symbol`/`path`, and result count; makes index vs. grep ratio visible per session |
| TUI startup notices (v0.11.1) | `src/session/mod.rs`, `src/tui/mod.rs` | `startup_notices: Vec<String>` on Session populated during `new()` for TUI mode; C1 context banner ("↩ Last session: X") and C3 init/index nudge now shown in TUI chat via `app.messages`; previously TUI users saw neither |
| Greeting stays casual after model question (v0.12.3) | `src/session/mod.rs:needs_prior_context`, `is_pure_greeting` | Pure greetings (hi/hello/hey/thanks…) are never treated as answers to a question — second "hi" in a session no longer gets full context just because the model's previous reply ended with "?". Only action-confirmations (yes/no/proceed) trigger history injection when last message was a question. |
| Smart context pruning (v0.10.1) | `src/session/mod.rs:windowed_history`, `is_casual_message`, `is_action_confirmation`, `last_message_was_question`, `needs_prior_context` | Three-layer token optimisation: (1) **Casual turns** (greetings, acks) send only the current message — zero history, minimal system prompt, no tools; saves full session history cost per greeting turn. (2) **Sliding history window** — non-casual turns send only the last `ZAP_HISTORY_WINDOW` real user turns (default 8); bounds token cost regardless of session length. (3) **Tool-result pruning** — `ToolResult` blocks outside the last 2 complete exchanges are replaced with a one-line stub `[pruned — N chars]`; large file reads from earlier turns no longer inflate every subsequent prompt. Casual detection hardened: git/ops keywords added (`push`, `pull`, `commit`, `merge`, `deploy`, `revert`, etc.); action-confirmations (`yes`, `no`, `go ahead`, `proceed`, `do it`, `continue`) and question-answers (last assistant message ended with `?`) bypass casual path and always receive windowed history. |
| Token transparency + casual-message skip (v0.8.2) | `src/tui/channel.rs`, `src/tui/app.rs`, `src/tui/render.rs`, `src/session/mod.rs` | Sidebar now shows actual API token counts: `in/out` (blue/green) = cumulative session input & output tokens from API response (includes system prompt, history, tool defs); `cached` shown when cache hits > 0. Greeting/casual messages (hi, hello, thanks, ok, etc.) skip skill injection AND tool definitions entirely — saves 3-12k tokens per casual turn. `is_casual_message()` checks message length, absence of technical keywords, and presence of greeting patterns. |
| Ctrl+O cycling + deeper preview + deepseek context (v0.8.1) | `src/tui/mod.rs`, `src/tui/render.rs`, `src/tui/input.rs`, `src/session/mod.rs` | Ctrl+O now cycles through tools newest-first (each press expands the next unexpanded tool; when all are expanded, one more press collapses all). Works in all states (not just Idle). Streaming tool calls also respect `expanded_tools`. Preview depth increased from 3 to 10 lines. `deepseek` models get 64k context limit instead of the 32k local default. |
| `/goal` autonomous loop + TUI polish (v0.8.3) | `src/tui/mod.rs`, `src/tui/app.rs`, `src/tui/render.rs` | `/goal <condition>` runs turns automatically until LLM ends response with `✓ DONE` or max-turns limit (default 20, `--max N`). `/goal stop` cancels mid-flight. Goal section in sidebar (condition, turn X/max, elapsed). Goal badge in status bar. Goal indicator in dir panel replaces hints when active. Ctrl+C cancels goal. Dir panel condensed 6→3 rows. Debug logging stripped. |
| TUI summary rendering + elapsed time (v0.8.0) | `src/tui/render.rs`, `src/tui/app.rs`, `src/tui/syntax.rs` | Span-aware markdown word wrap: long prose paragraphs now reflow across lines while preserving bold/italic/inline-code styling. Inline code rendered with cyan-on-dark-blue background box. Elapsed seconds shown next to thinking spinner ("Analyzing… 4s") in status bar, sidebar, and messages area. Word-rotation interval slowed from 240ms to ~3s (`word_tick / 188`). `turn_tick` counter resets to 0 on each new turn start. |
| Tool preview collapsed by default (v0.7.8) | `src/tui/render.rs:tool_call_lines` | File content no longer shown inline by default — collapsed view shows only `N lines  Ctrl+O to expand` hint. Expanded (Ctrl+O) shows full diff-coloured content. Eliminates the root cause of tab/overflow scatter: no inline content = no overflow possible. |
| Fix UTF-8 panic on tool output truncation (v0.7.7) | `src/session/mod.rs:862`, `src/tools/web.rs:49` | Panic: "byte index 20000 is not a char boundary" — `—` (em-dash, 3 bytes) straddled the 20000-byte cut point. Both truncation sites now walk back to the nearest valid char boundary with `is_char_boundary` before slicing. |
| TUI text overflow final fix (v0.7.6) | `src/tui/render.rs` | Three missed overflow sources: (1) `text_to_lines` markdown path returned unsplit prose paragraphs — now checks if all markdown lines fit in `wrap_width`; if any line is too wide, extracts plain text and word-wraps it. (2) Added `truncate_spans` helper + global safety-net pass at end of `render_all_lines` that hard-clips every line to `width-2` chars — catches code blocks and any future overflow source. (3) `word_wrap_plain` extracted as shared helper. |
| Thinking word rotation fix (v0.7.5) | `src/tui/app.rs`, `src/tui/render.rs` | Root bug: `spinner_frame` was clamped to `% 10` (spinner glyphs), so `spinner_frame / 40` was always 0 — thinking words never changed within a turn. Added `word_tick: usize` (monotonically increasing, never clamped) incremented in `tick_spinner`; render uses `word_tick / 15` = new word every 240ms. Per-turn prime offset (`turn * 31`) ensures each response starts at a different word. |
| TUI-visible warnings | `src/log.rs:write` | WARN/ERROR from `zap_warn!`/`zap_error!` are forwarded via `TuiEvent::LlmChunk` so they appear in the TUI chat; previously invisible behind the alternate screen |
| Ctrl+G snapshot fallback for non-git dirs (v0.11.7) | `src/snapshot.rs`, `src/tui/render.rs` | `open_diff_viewer` now falls back to in-session snapshots when git is unavailable (non-git directory or clean tree with no prior commit). `snapshot_diffs()` returns (path, before, after) for every file edited this session; `similar::TextDiff` computes the unified diff in-memory. Panel title shows "session edits". |
| TUI notices + sidebar timer/token fixes (v0.13.64) | `src/tui/channel.rs`, `src/tui/app.rs`, `src/session/turn.rs`, `src/session/commands/session_mgmt.rs`, `src/tui/render/layout.rs` | `TuiEvent::Notice` routes hints and compact status as proper assistant bubbles instead of raw `println!` (fixes overlap with input area). `cmd_compact` uses `ThinkingSpinner::noop()` in TUI mode (indicatif spinner was writing directly to terminal, disrupting layout). Sidebar timer drops thinking word above 99s to prevent overflow in 22-char column. In/out token display split into separate "in"/"out" rows; M suffix added for 1M+ tokens. Ctrl+N new session shortcut; session picker gets "✚ New session" entry at top. |
| Skill label in sidebar + line counts in files hint (v0.11.6) | `src/session/mod.rs`, `src/tui/channel.rs`, `src/tui/app.rs`, `src/tui/render.rs`, `src/tui/mod.rs` | Active skill no longer printed mid-chat via `println!`; in TUI mode sends `TuiEvent::ActiveSkill` which shows as a "skill / active" row in the sidebar (amber, cleared at turn end). "N files modified" hint now includes `(+A/−R)` line counts from `git diff HEAD --shortstat`. |
| Fix Ctrl+G diff viewer fallback + feedback (v0.11.5) | `src/tui/render.rs:open_diff_viewer`, `src/tui/app.rs`, `src/tui/mod.rs` | `open_diff_viewer` now falls back to `git diff HEAD~1` when the working tree is clean (previously returned `None` silently after a commit). Added `title` field to `DiffViewerState` shown in the panel header ("working changes" vs "last commit"). When both diffs are empty, sets `app.error` with a clear message instead of doing nothing. |
| Fix INDEX println! overlap in TUI (v0.11.4) | `src/log.rs:write` | Background tree-sitter threads called `log::write("INDEX",…)` which did a raw `println!` while ratatui owned the terminal; writes landed at whatever cursor position the last render left, scattering text across panel boundaries. Guarded the `println!` with `is_tui_mode()` — INDEX messages now go only to `zap.log` during TUI sessions. |
| Removed redundant git tools | `src/tools/shell.rs`, `src/tools/mod.rs` | `git_status`, `git_pull`, `git_diff` removed — model uses `shell` directly; saves ~250 tokens per request |
| Per-turn tool filtering | `src/session/mod.rs:select_tools_for_turn` | For OpenAI-compatible (local) providers, `web_fetch`/`web_search` are omitted unless the user mentions web/url/docs or web tools were already used this session; Anthropic sends all tools (cached) |
| Windows shell compatibility | `src/shell_runner.rs:run_command` | Uses PowerShell (`-NoProfile -NonInteractive`) on Windows, `sh -c` on Unix/macOS — PowerShell has `ls`/`sleep` aliases, fixing "command not recognised" failures from LLM-generated Unix commands |
| list_directory noise filtering (v0.13.0) | `src/tools/shell.rs:list_directory_native` | Filters `node_modules`, `target`, `dist`, `build`, `bin`, `obj`, `.git`, `__pycache__`, `.venv`, etc. from directory listings — model no longer recurses into vendor/build dirs |
| list_directory filter transparency (v0.13.9) | `src/tools/shell.rs:list_directory_native` | Prepends `# Note: N hidden (dot) entries omitted; build/vendor dirs skipped: X` whenever entries are filtered, so the LLM knows to use `read_file` or `shell` to access them — prevents "directory looks empty" confusion and retry loops with absolute paths. |
| search_code Windows fix + smart tool previews (v0.13.27–31) | `src/tools/search/`, `src/session/mod.rs:smart_tool_preview` | (1) `find_tool()` helper probes `%PROGRAMFILES%\Git\usr\bin\` for rg/grep when not on process PATH — fixes the case where Git for Windows is installed but only VS Code (not PowerShell/CMD) adds it to PATH. (2) When neither tool is found, a `zap_warn!` appears inline in the TUI with platform-specific install instructions (winget/brew/apt). (3) Pure-Rust fallback (`search_rust_native`): restricts to known text extensions, `:` for match lines, `-` for context, deduplicates adjacent matches. (4) `smart_tool_preview()` replaces generic 10-line truncation with tool-specific one-liners. (5) `search.rs` refactored to `search/mod.rs` + `search/search_impl.rs`. Fix: code_map preview panicked on AST index header `## Code map: . [AST index, 2000 symbol(s)]` because `find('(')` returned pos 38 while `find(" symbol")` returned pos 31 — now parses header directly; fallback uses `rfind` so `(` is always before `" symbol"`. |
| Tool results expanded by default (v0.13.34) | `src/tui/app.rs:apply_event`, `src/tui/render.rs` | Tool results auto-expand when they complete — `expanded_tools.insert(id)` on every `ToolDone` event. Ctrl+O collapses all; another Ctrl+O re-expands newest-first. Hint text updated from "Ctrl+O expand" to "Ctrl+O collapse". |
| Tool result smart preview in TUI + Git PATH warning (v0.13.33) | `src/tui/render.rs:tool_call_lines`, `src/tools/search/search_impl.rs:find_tool` | (1) Collapsed tool call now shows the `smart_tool_preview` summary (e.g. `🗄 3 hits [index]`, `🔍 native, 2 matches in 1 files`) instead of a raw `N lines` line count — makes code index usage and search method visible at a glance without expanding. (2) When grep/rg is found via Git-for-Windows path (not on system PATH), a one-time `zap_warn!` appears with exact PATH entry to add and ripgrep install command. Fires at most once per session via `static AtomicBool`. |
| TUI cursor + Ctrl+C fix (v0.13.32) | `src/tui/render.rs:draw_input`, `src/tui/input.rs`, `src/tui/mod.rs` | (1) Native terminal cursor hidden at startup; input cursor now uses `frame.set_cursor_position()` instead of yellow-background span — eliminates the double-cursor artifact (native + fake) that showed as a "yellow bar" on Windows Terminal. Cursor hidden automatically when popups/pickers overlay the input. (2) Ctrl+C when idle now triggers quit-confirmation ("press again to quit") same as Ctrl+Q, instead of silently doing nothing. |
| Language auto-detection — no startup prompt (v0.13.31) | `src/session/mod.rs:Session::new` | Removed `prompt_domain_scope` interactive picker that appeared at startup when no project.json existed. Domain scope now falls through: project.json → manifest detection (`package.json`, `Cargo.toml`, etc.) → extension scan (`detect_from_extensions`) → empty scope. Extension scan catches React/TS repos whose `package.json` is in a subdirectory. Empty scope = all domain skills are trigger-matchable, which is the correct default. |
| Multi-tool project interop — context_paths (v0.13.26) | `src/config.rs`, `src/context_manager.rs:load_zap_md` | New `context_paths` config key in `~/.agent.toml`: directories whose `.md` files are loaded as always-on context alongside ZAP.md/CLAUDE.md (frontmatter stripped). Use `context_paths = [".kiro/steering"]` for Kiro steering or `context_paths = [".claude/context"]` for Claude Code extras. Kiro/Claude skills use existing `skill_paths`. Skill precedence (lowest→highest): bundled → `~/.zap/skills/` → `skill_paths` entries left-to-right → `.zap/skills/`. Shown in `/config` output. |
| Practice skill trigger quality pass (v0.13.24) | `src/default_skills/{security,git,deploy,debugging}.md` | Removed high-noise single-word triggers from all 4 practice skills: `"session"`, `"token"`, `"hash"` from security (→ phrases like `"session token"`, `"api token"`); `"stage"`, `"push"`, `"release"` from git (→ `"git push"`, `"git stage"`, `"git release"`); `"release"`, `"ship"`, `"publish"` from deploy (→ `"release version"`, `"ship it"`, `"publish release"`); `"undefined"`, `"trace"` from debugging (→ `"undefined behavior"`, `"stack trace"`). |
| Skill trigger word-boundary matching (v0.13.23) | `src/skill_manager.rs:trigger_word_match`, `src/default_skills/code-review.md` | Single-word triggers now require word-start boundaries (so "review" no longer fires on "preview"/"irrelevant"); short triggers ≤ 3 chars (e.g. "pr") also require a word-END boundary so "pr" doesn't match "process". Phrase triggers (containing spaces) still use simple substring match. Also removed overly broad single-word triggers from code-review: `"review"`, `"feedback"`, `"approve"` replaced with explicit phrases like `"review my"`, `"review this"`, `"please review"`, `"can you review"`. |
| Corporate gateway tool result DLP detection (v0.13.22) | `src/llm_client.rs:AnthropicClient::send` | When using a custom gateway (`base_url` set): (1) tool_result blocks now include `is_error: false` for strict Anthropic-compat gateways; (2) pre-send warning if any tool_result content is empty before it even leaves zap; (3) post-response heuristic detects when the model's reply contains "empty result"/"no content" phrases after sending tool results — emits a targeted warning pointing to `~/.zap/llm.log` and advising to check corporate proxy DLP policy. |
| Startup conversation replay (v0.13.21) | `src/tui/mod.rs:run_tui` | When zap auto-continues from the last session (shows "↩ Last: ..."), the previous session's conversation messages are now replayed into `app.messages` so the user sees the full history. A "─── end of session #N ───" separator marks where the new session begins. Only activates when the startup banner is present (i.e., a previous session exists with stored messages). |
| Fix first-run crash: CodeIndex /tmp fallback panics on Windows (v0.13.20) | `src/session/mod.rs`, `src/code_index.rs` | The old fallback `CodeIndex::open("/tmp").unwrap()` panicked on Windows (no `/tmp`). Replaced with a 3-step chain: cwd → `std::env::temp_dir()` → `open_in_memory()`. Added `CodeIndex::open_in_memory()` using `Connection::open_in_memory()` — never fails, no persistence. Prevents silent crash-before-TUI on first run in restricted directories. |
| Sidebar skill log with turn numbers (v0.13.19) | `src/tui/app.rs`, `src/tui/render.rs` | Skill history expanded to 8 entries; each entry now stores `(turn_number, label)` so the sidebar shows `T3 rust`, `T5 git`, etc. Active turn shows in yellow; history in dim with turn index. `skill_history` type changed from `Vec<String>` to `Vec<(usize, String)>`. |
| Fix code_map double-slash on Windows (v0.13.18) | `src/tools/search.rs:build_code_map` | `canonicalize()` on Windows returns `\\?\C:\...` (UNC extended prefix) which broke LIKE queries against stored index paths (no `\\?\ ` in stored paths) and surfaced as double-slash in output. Added `strip_unc_prefix()` helper that strips the `\\?\` prefix after every `canonicalize()` call in `build_code_map`. |
| Sidebar skill history (v0.13.17) | `src/tui/render.rs:draw_sidebar`, `src/tui/app.rs` | Sidebar now shows active skill with `▶` indicator while it fires, plus the last 3 skills used (dimmed, newest first) that persist across turns; `skill_history: Vec<String>` added to App state, populated on `ActiveSkill` event with deduplication |
| Session load clears view (v0.13.16) | `src/tui/mod.rs:InputAction::LoadSession`, `src/tui/input.rs` | `app.messages.clear()` before replaying loaded session — startup notices / prior messages no longer bleed through; `LoadSession` now carries `goal` string; no-message sessions show goal + files note instead of error popup |
| Code nav fallback in system prompt (v0.13.0) | `src/context_manager.rs` | Explicit instruction: if `code_map`/`find_definition` return 0, fall back to `list_directory` + `search_code`; never explore `node_modules`/`target`/etc.; suggest `/index` to user |
| Expanded index exclusions (v0.12.9) | `src/code_index.rs:walkdir_filtered` | Added `bin`, `obj`, `out`, `coverage`, `.venv`, `venv`, `site-packages`, `.nuxt`, `tmp`, `temp`, `logs` to skip list — prevents indexing .NET build output, Java IDE output, Python virtualenvs, etc. |
| `/index clear` (v0.12.9) | `src/code_index.rs:clear`, `src/session/commands.rs:cmd_index` | Wipes all symbols + indexed_files in-place (WAL checkpoint first); use when .zap/code.db is stale or locked and you can't delete the file |
| `global_index()` accessor (v0.12.9) | `src/code_index.rs:global_index` | Returns the `Arc<Mutex<CodeIndex>>` for commands that need write access to the global index |
| TSX indexing fix (v0.12.8) | `src/code_index.rs:extract_js`, `detect_language` | `.tsx` files now use `language_tsx()` grammar instead of `language_typescript()` — the TS grammar rejects JSX syntax causing 0 symbols; `detect_language` returns `"tsx"` for `.tsx` so the right parser is selected |
| Windows hook shell compatibility (v0.12.7) | `src/hooks.rs:hook_cmd` | `run_hook` and `run_pre_hook` now use `powershell -NoProfile -NonInteractive -Command` on Windows instead of hardcoded `sh -c`; added `hook_cmd()` helper with `#[cfg(windows)]` / `#[cfg(not(windows))]` |
| Git-root CLAUDE.md boundary (v0.12.7) | `src/context_manager.rs:load_zap_md` | Walk stops at git root (not $HOME), preventing a parent repo's CLAUDE.md from bleeding into child/sibling projects; `home_dir()` helper checks `$HOME`, `%USERPROFILE%`, `%HOMEDRIVE%+%HOMEPATH%` for Windows compatibility |
| TUI permission prompt | `src/permission_manager.rs:prompt_batch_tui`, `src/session/mod.rs` | Permission dialog renders inside the TUI at the bottom of the screen (cursor-positioned, raw mode stays active); suspend/resume only called in CLI mode — previously broke out to CLI |
| PowerShell system prompt guidance | `src/context_manager.rs` | Shell shown as "PowerShell" on Windows (not "sh"); system prompt includes Windows-specific command guidance (PowerShell syntax, background process pattern) |
| Full request logging in llm.log | `src/llm_client.rs` | Every REQUEST log entry now includes `POST <url>` and `Authorization: <redacted>` so you can see exactly what endpoint and credentials are used |
| Corporate gateway tool-support detection | `src/llm_client.rs` | HTTP 400/422 errors mentioning "tool"/"function" emit a `zap_warn!` explaining the gateway likely doesn't support function calling; text responses containing JSON-style tool-call blobs (gateway stripped tools array) also trigger a warning |
| Curl-ready request replay | `src/log.rs:save_request_body`, `src/llm_client.rs:build_curl_block` | Every REQUEST entry in `llm.log` ends with a ready-to-run `curl` command; the full request body (stream:false) is saved to `~/.zap/llm_requests/<ts>_<provider>.json` and referenced via `-d @path`; the curl block uses the real API key (treat these files as sensitive) |
| Corporate proxy streaming fix | `src/config.rs`, `src/llm_client.rs` | `disable_stream = true` in `~/.agent.toml` (or `AGENT_DISABLE_STREAM=true`) sends `stream:false` and parses a plain JSON response instead of SSE; fixes empty `tool_use` blocks on proxies that mangle SSE |
| Three-tier skill system | `src/skill_manager.rs`, `src/session/` | Skills categorised as Core (always injected), Practice (always trigger-matchable: git, debugging, security, code-review), Domain (session-scoped language skills). At startup, manifests are detected (Cargo.toml → rust, pom.xml → java, etc.); if nothing found and session is interactive, a multi-select prompt asks which languages are in scope. Per-turn trigger matching only searches Practice + scoped Domain. `/skill scope [add\|remove\|reset]` changes scope mid-session. All 23 language skills now bundled. |
| System prompt git refs cleaned | `src/context_manager.rs` | Shell commands section no longer references deleted `git_status`/`git_pull`/`git_diff` tools |
| 12 new language/platform skills | `src/default_skills/` | Added: Java, C#, C++, Kotlin, Swift, Ruby, SQL, Bash, PHP, Scala, Vue.js, CSS/SCSS, Dart/Flutter — each triggered by language keywords and grounded in canonical style guides |
| `/index files` | `src/session/commands.rs:cmd_index`, `src/code_index.rs:list_indexed_files` | Lists all files in the code index with their symbol count; sorted by symbol count desc |
| `/index db` | `src/session/commands.rs:cmd_index` | Shows agent.db summary: session count, memory entries, branches, last 10 sessions and all memory key-value pairs |
| Ctrl+Q confirmation | `src/tui/input.rs`, `src/tui/app.rs` | First Ctrl+Q shows "Press Ctrl+Q again to quit" notice; any other key cancels; second Ctrl+Q quits — prevents accidental exits |
| Extended thinking (`/think`) | `src/session/commands.rs:cmd_think`, `src/llm_client.rs`, `src/tui/` | `/think on` (8k budget), `/think off`, `/think <N>` tokens; thinking streams in TUI as dimmed italic text with last 3 lines visible; collapses to "🧠 Thinking (N chars)" after turn completes; thinking blocks preserved in multi-turn history (with Anthropic signature); OpenAI providers ignore the budget; budget clamped to MAX_TOKENS-1 to satisfy Anthropic constraint; /think handled inline in TUI (no suspend/Press-Enter) |
| Topic-shift confirmation | `src/session/casual.rs:is_topic_shift`, `src/tui/actions.rs`, `src/tui/render/layout.rs` | Detected before the turn starts (TUI only); status bar shows [Enter] send / [b] branch / [any] cancel — message never lost; CLI keeps the post-hoc print |
| Prompt history (Up/Down) | `src/tui/input.rs`, `src/tui/app.rs` | Up arrow cycles through previously sent prompts (newest-first); Down goes forward; typing breaks out of history mode |
| Ctrl+Z drop last turn | `src/tui/input.rs`, `src/tui/actions.rs` | Idle + empty input: removes last user+assistant pair from session.messages, restores prompt to input box |
| Multi-line input badge | `src/tui/render/mod.rs:visual_line_count`, `src/tui/render/layout.rs` | Input box capped at 3 rows; when pasted content exceeds that, border shows ↕ N lines; cursor tracks newlines + word-wrap correctly |
| TUI action handler module | `src/tui/actions.rs` | InputAction match extracted from mod.rs → keeps mod.rs under 600 lines |
| `/compact` | `src/session/commands.rs:cmd_compact` | summarises history in-place |
| Command output popup | `src/tui/render.rs:draw_command_popup` | Inline slash commands show output in centered overlay instead of dumping into chat; Esc dismisses, ↑↓/PgUp/PgDn scrolls |

### Remote control
| Feature | File | Notes |
|---|---|---|
| `/remote [port]` command | `src/remote.rs`, `src/remote_channel.rs` | Starts a local HTTP server + public tunnel; prints a URL you can open on any device (phone, tablet) to drive the current session |
| Web chat UI | `src/remote.rs:UI_HTML` | Dark-theme mobile-friendly chat page embedded in the binary; WebSocket for real-time streaming; auto-reconnect on disconnect; uses wss:// over HTTPS tunnels to avoid mixed-content block |
| Streaming to browser | `src/llm_client.rs`, `src/remote_channel.rs` | `send_chunk()` tapped into both Anthropic SSE and OpenAI streaming paths — no-op when remote is inactive |
| Turn-done signal | `src/session/mod.rs`, `src/remote_channel.rs` | `send_done()` called after every `handle_user_turn` so the browser re-enables input exactly when the agent finishes |
| TUI integration | `src/tui/mod.rs` | `try_recv()` at top of each TUI loop iteration — remote messages injected as user turns with a chat bubble; zero overhead when inactive |
| CLI integration | `src/session/mod.rs` | `/remote` in CLI slash dispatcher; local server URL printed; tunnel URL printed when ready |
| `/remote stop` | `src/tui/commands.rs`, `src/remote_channel.rs` | Aborts the HTTP server task and kills the tunnel process (ngrok or SSH); `deactivate()` sets ACTIVE=false, aborts AbortHandle, kills PID |
| DeepSeek V4 models | `src/session/commands.rs` | Added `deepseek-v4-pro` and `deepseek-v4-flash` to the /provider picker; V4 models listed first |
| TUI-native provider picker | `src/tui/app.rs`, `src/tui/input.rs`, `src/tui/mod.rs`, `src/tui/render/overlays.rs`, `src/tui/turn_handler.rs` | `/provider` slash command now opens a TUI-native overlay instead of dropping to CLI mode; ↑↓ or j/k navigate, Enter selects, Esc cancels; shows Anthropic, OpenAI, Ollama, LM Studio, Claude Code (coming soon), Gemini, and Groq providers with model lists; includes "Coming soon" entry with explanatory message; provider switch happens instantly — config saved and LLM client recreated without terminal suspend |
| Decouple auto-index from /init (v0.13.69) | `src/project.rs`, `src/session/mod.rs` | `mark_indexed()` now creates `project.json` if absent via `unwrap_or_default()` — running `/index` alone is sufficient to set the indexed flag and enable auto-indexing for future sessions, no `/init` required. Startup nudge simplified: new projects get "Run /index for fast code symbol lookup" instead of pushing `/init`; existing projects with no index get "Run /index… Run /init for full project context (LLM analysis)".
| Auto-redact secrets — no blocking prompt (v0.13.70) | `src/secret_scanner.rs`, `src/session/tools.rs`, `src/tui/` | When tool output contains secrets (API keys, tokens, passwords) detected by the secret scanner, the lines are now automatically redacted with `[REDACTED: <type>]` markers and a notice is shown — the turn continues without user intervention. Removes the blocking "send anyway? [y/N]" prompt (both TUI popup and CLI stdin prompt) and its ~130 lines of dead code (SecretPopup, SecretScannerRequest, channel plumbing).
| DeepSeek V4 reasoning replay | `src/llm_client.rs` | V4 is a thinking model; `reasoning_content` is now captured in `ContentBlock::Reasoning` and echoed back in subsequent turns to avoid 400 errors |
| Tunnel — ngrok | `src/remote.rs:launch_tunnel` | If ngrok is installed, starts it and queries `localhost:4040/api/tunnels` for the HTTPS URL |
| Tunnel — localhost.run | `src/remote.rs:localhost_run_tunnel` | SSH fallback (`ssh -R 80:localhost:PORT nokey@localhost.run`) — needs no binary, just SSH |

### Skill system
| Feature | File | Notes |
|---|---|---|
| Skill bootstrap | `src/skill_manager.rs:bootstrap_bundled_skills` | on first launch writes all built-in skills to `~/.zap/skills/`; never overwrites existing files |
| Skill loader | `src/skill_manager.rs:load_all_skills` | bundled → global → extra paths → project, same-name override |
| Extra skill paths | `src/config.rs`, `src/skill_manager.rs` | `skill_paths = [".kiro/skills"]` in `~/.agent.toml`; shown as `◉ external` in `/skill list`; `~` expansion supported |
| Always-on skills | `src/skill_manager.rs:always_on_skills` | no `trigger:` field = always injected |
| Triggered skills | `src/skill_manager.rs:match_skills` | keyword match per turn |
| Stack auto-detection | `src/skill_manager.rs:detect_stack_skills` | Cargo.toml/go.mod/package.json/pyproject |
| Skill prompt builder | `src/skill_manager.rs:build_skill_prompt` | for triggered skills per turn |
| Always-on prompt builder | `src/skill_manager.rs:build_always_on_prompt` | baked into base system at session start |
| `source_label()` | `src/skill_manager.rs:source_label` | built-in / global / project display |
| `/skill list` | `src/session.rs:cmd_skill`, `src/tui/commands.rs` | grouped: Core/Practice/Domain; source glyph; inline in TUI (no CLI break-out) |
| `/skill use <name>` | `src/session/commands.rs`, `src/tui/commands.rs` | pin a skill — injected every turn regardless of triggers; 📌 shown in list; inline in TUI |
| `/skill unuse <name>` | `src/session/commands.rs`, `src/tui/commands.rs` | unpin a skill; inline in TUI |
| `/skill show <name>` | `src/session.rs:cmd_skill`, `src/tui/commands.rs` | description, license, content preview; inline in TUI |
| `/skill scope` | `src/session/commands.rs`, `src/tui/commands.rs` | show/change domain scope; inline in TUI |
| `/skill export <name>` | `src/session/commands.rs:cmd_skill` | write built-in skill to `~/.zap/skills/` for editing; `--overwrite` flag |
| `/skill export --all` | `src/session/commands.rs:cmd_skill` | export every built-in skill at once |
| `skill_to_markdown()` | `src/skill_manager.rs` | serialize Skill struct → `.md` frontmatter + body |
| `/skill create` | `src/session/commands.rs:cmd_skill` | scaffolds frontmatter template |
| `/skill capture` | `src/session/commands.rs:cmd_skill` | LLM extracts session rules → skill file |
| `/skill log` | `src/session/commands.rs:cmd_skill` | per-session skill trace: shows turn#, input preview, skills that fired; "no match" in orange, "casual" in dimmed — lets you debug why a skill didn't trigger |
| `skill_trace` | `src/session/mod.rs` | `Vec<(usize, String, Vec<String>, Option<String>)>` — (turn, input preview, skill names, reason); recorded after every skill match before turn_count increment |
| Pinned skills | `src/session/mod.rs:pinned_skills` | `HashSet` of skills pinned via `/skill use`; merged into per-turn matched skills before prompt build |
| Project skills | `src/skill_manager.rs:skill_dirs` | `.zap/skills/` in CWD scanned at session start and on `/skill list`; highest priority, overrides same-name global/built-in |
| Frontmatter: name, description, license, trigger, tokens | `src/skill_manager.rs:parse_frontmatter` | SKILL.md standard + zap extensions |

### Built-in skills (src/default_skills/)
| Skill | Type | Triggers |
|---|---|---|
| `karpathy-guidelines` | always-on | every turn — Karpathy's 4 coding principles (MIT) |
| `rust` | triggered | rust, cargo, crate, fn, struct, clippy… |
| `python` | triggered | python, pip, pytest, dataclass… |
| `typescript` | triggered | typescript, tsx, interface, npm… |
| `react` | triggered | react, component, jsx, hook, useState… |
| `go` | triggered | go, goroutine, chan, go.mod… |
| `git` | triggered | commit, branch, merge, pull request… |
| `code-review` | triggered | review, pr review, lgtm, critique… |
| `debugging` | triggered | debug, error, crash, panic, stacktrace… |
| `security` | triggered | auth, password, token, jwt, xss, injection… |
| `understand` | triggered | what is this, summarize, architecture, generate docs, onboard me, tour, orient me… |

> **Note:** `understand` is registered in `bundled_skills()` via `include_str!` — available in all installs without any setup.

### Corporate / network settings
| Feature | File | Notes |
|---|---|---|
| Global HTTP client | `src/http.rs:init` | `OnceLock<reqwest::Client>` singleton; call once at startup |
| Proxy support | `src/http.rs` | `AGENT_PROXY` env / `~/.agent.toml`; auto-detects `HTTP_PROXY`/`HTTPS_PROXY` |
| No-proxy bypass | `src/http.rs` | `AGENT_NO_PROXY` env / config; passed to `reqwest::NoProxy` |
| Custom CA bundle | `src/http.rs:load_ca` | `AGENT_CA_BUNDLE` / `SSL_CERT_FILE` / `CURL_CA_BUNDLE`; PEM or DER |
| TLS skip verify | `src/http.rs` | `AGENT_TLS_SKIP_VERIFY=1`; dangerous, prints warning |
| Timeout | `src/http.rs` | `AGENT_TIMEOUT_SECS` env / config; default 120s |
| Proxy credential redaction | `src/http.rs:redact_proxy_url` | strips `user:pass@` before display |
| Network startup banner | `src/session/mod.rs:Session::new` | shown when proxy/CA/TLS-verify-off is active |
| `/config` network rows | `src/session/commands.rs:cmd_config` | shows proxy, ca_bundle, tls_verify, timeout when non-default |
| Config persistence | `src/config.rs:Config::save` | network fields written to `~/.agent.toml` |

### Providers & LLM client
| Feature | File | Notes |
|---|---|---|
| Anthropic (native) | `src/llm_client.rs` | SSE streaming, tool use, prompt caching; `Authorization: Bearer` when custom base_url set (corporate gateways) |
| Anthropic base_url | `src/llm_client.rs` | accepts full endpoint or base URL; appends `/v1/messages` if needed; handles corporate gateways that use non-standard paths |
| OpenAI-compatible | `src/llm_client.rs` | accepts full endpoint or base URL; appends `/v1/chat/completions` if needed; LM Studio, Ollama, Gemini, DeepSeek, Groq, Mistral, xAI, Together, Perplexity, Cohere |
| Multi-provider TOML | `src/config.rs:ProviderEntry`, `src/session/commands.rs:cmd_provider` | `[providers.<slug>]` sections in `~/.agent.toml`; switching providers preserves all other providers' keys/models/URLs; active provider set by `provider = "slug"` top-level key |
| Retry on 429/503/502 | `src/llm_client.rs:send_with_retry` | Retries on 429 (rate limit) AND 503/502 (transient server unavailable, e.g. DeepSeek "service busy"); Retry-After header honoured; 5s/10s/20s/40s/80s backoff; labelled message per status code |
| URL normalisation tests | `src/llm_client.rs:url_tests` | 10 unit tests covering full-endpoint, base-URL, /v1-suffix, trailing-slash, and None cases for both Anthropic and OpenAI-compatible providers |
| Provider switching | `src/session/commands.rs:cmd_provider` | interactive picker, saved to `~/.agent.toml`; shows existing key suffix when re-configuring |
| Model switching | `src/session.rs:cmd_model` | `/model <id>` mid-session |
| `/models` list | `src/session.rs:cmd_models` | lists OpenAI-compatible server models; strips `/chat/completions` suffix to get `/models` URL |
| Config from file | `src/config.rs` | `~/.agent.toml` |
| Config from env | `src/config.rs` | `AGENT_*`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` |
| Gemini gcloud ADC (keyless) (v0.13.73) | `src/llm_client/auth.rs`, `src/llm_client/credentials.rs`, `src/config.rs`, `src/llm_client/mod.rs`, `src/session/commands/provider.rs`, `src/tui/` | Auto-detects `gcloud auth application-default` credentials; `credential_method = "gcloud_adc"` in `ProviderEntry`; `CredentialProvider::GcloudAdc` shells out to gcloud per-request (50-min cache); `check_gcloud_adc()` runs at CLI/TUI provider picker render (60-sec cache) to show "✓ ready" badge; auth header forced to `Authorization: Bearer` (not `x-goog-api-key`) when using ADC tokens; fallback to `GOOGLE_API_KEY` env var for API key users; TUI provider wizard shows green "✓ ready" for auto-detected Gemini; provider switching persists `credential_method` + `auth_header` to `~/.agent.toml`; 11 unit tests covering env var detection, gcloud cache, ADC credential refresh, and header selection |
| Drop-summarizer robustness: input pruning + UI notice (v0.13.82) | `src/session/summarizer.rs` | Two hardening changes: (1) **Input pruning** — messages sent to the summarizer LLM are passed through `prune_for_summarizer()` which caps each tool result at 500 chars (vs. raw, which could be 100k chars from file reads); if total pruned input still exceeds 40 000 chars the LLM call is skipped and text-fallback runs instead — prevents oversized summarizer calls; (2) **UI notice** — emits "⟳ Summarizing dropped context…" via TUI Notice or CLI println before the LLM call so the user sees something instead of a silent pause; re-compression failure now also logs a WARN instead of silently ignoring. |
| LLM-based drop-summary for windowed history (v0.13.81) | `src/session/mod.rs`, `src/session/history.rs`, `src/session/turn.rs`, `src/session/commands/session_mgmt.rs` | **Replaces** the v0.13.80 text-based approach with a proper LLM summarization pipeline: (1) `Session::maybe_summarize_dropped_turns()` — called once per non-casual user turn; detects when new turns slid off the 8-turn window since the last call, sends the newly-dropped batch to the LLM for concise bullet-point summarization (≤ 8 bullets: goals, decisions, files, errors, constraints), appends to a running `self.dropped_summary`; falls back to text-only truncation if the LLM call fails; (2) auto-recompression — if `dropped_summary` exceeds 6 000 chars the LLM is asked to compress it to ≤ 400 words, bounding cost in arbitrarily long sessions; (3) `dropped_summary` prepended to every non-casual LLM call as a synthetic message pair so the model always has the full session context even when early turns are off-window; (4) `cmd_compact` includes `dropped_summary` in the compaction input so nothing is lost on manual compaction, then resets both fields; (5) `cmd_clear` resets both fields; (6) `windowed_history` reverted to a pure function — drop-summary is session state, not computed inline. Robustness: `call_drop_summarizer` uses `&mut self` (not `&self`) so the future is `Send`-compatible with Tokio's thread pool (rusqlite's RefCell makes `&Session` !Send). |
| Tool result pruning with content preview (v0.13.80) | `src/session/history.rs` | Old `[pruned — N chars]` stub now includes the first 150 chars of the original content so the LLM has a context hint about what it saw in pruned tool results. |
| Context viewer UX fixes (v0.13.86) | `src/tui/input.rs`, `src/tui/turn_handler.rs`, `src/tui/render/context_viewer.rs` | Three fixes: (1) `detail_scroll` now resets to 0 when exiting detail focus (Esc/Left) so the panel doesn't stay stuck at the bottom showing only the last AssistantText block; (2) default `selected` on open now points to the last (most recent) turn rather than the first; (3) a "↓ more (→/Enter to scroll)" indicator appears at the bottom of the detail panel when content overflows, and the hint text clarifies "→ or Enter = scroll detail panel". |
| Context fill % accuracy fix (v0.13.76) | `src/session/mod.rs` | `context_fill_pct()` previously calculated from `self.messages` (full history) rather than `windowed_history(&self.messages)` (what's actually sent to the LLM). Fixed by extracting `tokens_for_messages(&[Message])` helper and calling it with windowed history in `context_fill_pct()`; `estimated_context_tokens()` still uses full history for the context viewer's proportional turn display. |
| Context viewer detail title shows cumulative context (v0.13.79) | `src/tui/render/context_viewer.rs` | Detail panel title now reads "~8.4k this  (prev + this = ~38k  51% of limit)" so it's clear the panel shows the selected turn's content while the title communicates what was actually sent to the LLM at that point. |
| Context viewer dual % columns (v0.13.78) | `src/tui/render/context_viewer.rs` | Turn list now shows two percentage columns: own% (this turn's share of total stored history, yellow) and cum% (running cumulative total as % of the model's context limit, heat-mapped green→yellow→red). cum% always grows turn-by-turn showing actual context pressure build-up. |
| Context viewer detail scroll fix (v0.13.76) | `src/tui/render/context_viewer.rs` | Fixed scroll cap in detail panel: was capped at `all_lines.len()-1` allowing scroll past end until only the last line was visible at the top; now capped at `all_lines.len().saturating_sub(area.height)` so scrolling stops when the last line reaches the bottom of the visible area. |
| /context viewer overlay (v0.13.75) | `src/tui/context_viewer.rs`, `src/tui/render/context_viewer.rs`, `src/tui/app.rs`, `src/tui/mod.rs`, `src/tui/input.rs`, `src/tui/turn_handler.rs` | TUI-native `/context` command opens a full-screen overlay with a two-panel layout: left panel lists all conversation turns with per-turn token estimates and context window % share; right panel shows a detailed breakdown of each turn (user text, tool calls, tool results, assistant text). Features: drop individual turns from history with `d`+Enter confirmation, clear all history with `x`+y, compact with `c`, scroll detail panel with j/k. Turn list shows ▓ (in window) vs ░ (pruned) indicators. Token fill bar shown in title. |

### Tools (src/tools/ or tool_registry.rs)
| Tool | Notes |
|---|---|
| `read_file` | offset/limit, line-numbered output |
| `edit_file` | find-and-replace, rejects ambiguous matches |
| `batch_edit` | multiple edits to one file, single diff |
| `write_file` | create or overwrite |
| `undo_edit` | restore from pre-edit snapshot |
| `shell` | with permission check; description required; output printed inline |
| `search_code` | ripgrep (grep fallback), file-type filter |
| `list_directory` | pure Rust `read_dir` — works on Windows without Git Bash; trailing `/` on dirs |
| `glob_read` | list/preview files matching a pattern |
| `code_map` | AST structural outline (tree-sitter) |
| `find_definition` | AST index → ripgrep fallback |
| `find_references` | all call sites in codebase |
| `web_fetch` | fetch URL, strip HTML |
| `web_search` | DuckDuckGo, no API key |
| `spawn_agent` | parallel sub-agent with own tool loop |

### Code index
| Feature | File | Notes |
|---|---|---|
| Tree-sitter AST index | `src/code_index.rs` | Rust, Python, TS, JS, Go, Java |
| SQLite persistence | `src/code_index.rs` | `.zap/code.db`, incremental re-parse |
| Global index singleton | `src/code_index.rs:set_global` | shared across tool calls |
| Auto-reindex on write/edit | `src/session.rs:handle_user_turn` | fires after `write_file`/`edit_file`/`batch_edit` |
| `/index [path\|stats]` | `src/session.rs:cmd_index` | manual reindex or stats; appears in / picker and tab-completion |
| `/index quality` | `src/tui/commands.rs:index_quality_text`, `src/code_index.rs:quality_report`, `src/code_index.rs:global_file_line_counts` | TUI popup showing: file sizes by actual line count (⚠ >1000, ⚡ 500-1000, · healthy) with bar chart, god objects, high coupling, complex fns, dead code candidates, and 0-100 health score. Also aliased as `/index health`. Line counts read from disk; symbol counts from DB. |
| `QualityReport` struct | `src/code_index.rs` | god_objects, large_files, high_coupling, dead_candidates, complex_fns, async_files fields; `score()` method returns 0–100 |
| `compute_reference_counts()` | `src/code_index.rs` | Scans source for `identifier(` call-site patterns (skipping string literals and comments); updates `ref_count` per symbol. Previous word-frequency approach counted field access (`self.name`), strings, comments — `name` showed 962 false refs. Now only actual call sites counted. Called after every `index_dir` and after `global_reindex_file` (single-file tool writes). Empty index at session start shows a startup notice. |
| `ref_count` column | `src/code_index.rs` | added via `ALTER TABLE` migration in `open()`; how many times a symbol name appears in source |
| Signature cap | `src/code_index.rs:signature()` | increased 120 → 200 chars for more useful symbol previews |
| `stats_by_kind()` | `src/code_index.rs` | `SELECT kind, COUNT(*) FROM symbols GROUP BY kind` — used in `/index stats` kind breakdown |
| `top_files(n)` | `src/code_index.rs` | top N files by symbol count — used in `/index stats` bar chart |
| Global helpers | `src/code_index.rs` | `global_quality_report()`, `global_compute_reference_counts()`, `global_stats_by_kind()`, `global_top_files(n)` |
| Auto-index gated on /init | `src/session/mod.rs:288` | Background indexer only starts if `project.json` has `indexed: true` — prevents accidentally indexing C:\, /, or other system roots on first launch |
| read_file pipe delimiter | `src/tools/file.rs:116` | Changed line-number prefix from tab to `" | "` so whitespace in file content is never ambiguous when copying into `edit_file` old_string |
| Better edit_file error | `src/tools/file.rs:176,329` | When `old_string` not found, error shows head/tail preview with whitespace as visible symbols (→ for tab, ↵ for newline) plus hint to use `cat -A` or `python3 repr()` |
| Positional guard (expected_line) | `src/tools/file/edit.rs` | `edit_file` and `batch_edit` accept optional `expected_line` (1-based). Only enforced when the match is ambiguous (replace_all with count > 1). For unique matches (count == 1), line-number drift from prior edits in the same turn is silently ignored — the match is unambiguous regardless of position. |
| Leading-whitespace fallback (edit_file + batch_edit) | `src/tools/file/edit.rs` | When `old_string` exact match fails, automatically tries stripping leading whitespace from each line (trim_start per line, join). Applies only if the stripped form gives exactly 1 match. Recovers from the frequent LLM mistake of carrying over indentation from read_file's line-number prefix into old_string. |
| read_file directory guard | `src/tools/file/read.rs` | When path points at a directory, returns a sorted listing of files inside rather than an opaque IO error. Recovers from the LLM confusing Rust module paths (`crate::session::commands`) with file paths. |
| Code graph — `CallSite` + `Import` structs | `src/code_index/mod.rs` | CallSite captures call-site resolution (path, line, col, name, qualifier, receiver_expr, caller_scope, language); Import captures import resolution (path, line, module, imported_name, alias, language). Both have `display()` for human-readable output. |
| Graph index mode (A tier) | `src/code_index/mod.rs:graph_enabled` | Default mode emits call_sites + imports into SQLite tables alongside symbols. Falls back to symbol-only (B tier) via `ZAP_INDEX_MODE=symbols`. Enables cross-file reference tracking. |
| `find_references` + `callers_of` | `src/code_index/index_impl.rs` | SQLite-backed call-site resolution — finds all calls TO a symbol (find_references) or all callers FROM a scope (callers_of). Global helpers: `global_find_references`, `global_callers_of`. |
| `imports_for` + `importers_of` + `users_of_module` | `src/code_index/index_impl.rs` | Import graph queries — what does file X import? Who imports symbol Y? Who uses module Z? Global helpers: `global_imports_for`, `global_importers_of`, `global_users_of_module`. |
| File ranking (`rank_files`, `file_rank`) | `src/code_index/index_impl.rs` | Ranks files by importance score (symbol count × reference density). Global helpers: `global_rank_files(n)`, `global_file_rank(path)`. |
| Context packing (`pack_context`) | `src/code_index/mod.rs:PackedContext` | Assembles a relevance-ranked symbol bundle for an LLM task within a token budget. Returns strategy + items with provenance. Global helper: `global_pack_context(task, token_budget)`. |
| Code-graph spec + design doc | `.zap/specs/code-graph.md`, `docs/code-graph-decisions.md` | Full spec and architecture decisions for the code graph feature. |

### Hooks (src/hooks.rs)
| Feature | File | Notes |
|---|---|---|
| Hook loader | `src/hooks.rs:HookRunner::load` | merges `~/.zap/hooks.json` (global) + `.zap/hooks.json` (project) |
| `PreToolUse` | `src/hooks.rs` | fires before tool runs; exit code 2 blocks the call |
| `PostToolUse` | `src/hooks.rs` | fires after tool completes; informational, cannot block |
| `SessionStart` | `src/hooks.rs` | fires once after session initialises |
| `SessionEnd` | `src/hooks.rs` | fires before goodbye message |
| `UserPromptSubmit` | `src/hooks.rs` | fires on every user message; stdout replaces the prompt |
| Tool matcher | `src/hooks.rs:HookEntry::matches` | `"shell"`, `"*"`, or absent = all tools |
| File-size pre-commit guard (v0.12.7) | `scripts/pre-commit` | blocks commit if any staged `.rs` file exceeds 600 lines; shows offending files + line counts |
| File-size PostToolUse warning (v0.12.7) | `scripts/check-file-size.sh`, `.claude/settings.json` | Claude Code hook warns in-editor when an edited `.rs` file exceeds 500 lines |
| Hook count banner | `src/session.rs:Session::new` | shown at startup if hooks are configured |
| `/hooks` | `src/session.rs:handle_slash` | lists all configured hooks grouped by event |

### Security & permissions
| Feature | File | Notes |
|---|---|---|
| Permission modes (ask/auto/deny) | `src/permission_manager.rs` | per-tool, per-session; WRITE_TOOLS includes batch_edit |
| Batch permission prompt | `src/permission_manager.rs:prompt_batch` | ONE grouped UI for all pending tool calls per turn instead of per-call prompts |
| TUI-native permission popup | `src/permission_manager.rs:prompt_batch_tui`, `src/tui/render.rs:draw_permission_popup` | amber-bordered ratatui overlay at screen bottom — Y/N/A keys, no raw stdout writes |
| Tool grant classes | `src/permission_manager.rs:tool_grant_class` | 'a' for edit_file grants write_file + batch_edit for the session |
| `Tool::affected_path()` | `src/tools/mod.rs` | trait method — tools declare what file they write; drives reindex |
| Secret scanner | `src/secret_scanner.rs` | 29 patterns: API keys, VCS tokens, AWS, GCP, JWT, cert blocks, credential fields |
| Path traversal guard | `src/tools/file.rs:guard_path` | normalizes `..`, blocks `~/.ssh`, `~/.aws`, `~/.kube`, certs, `/etc/shadow`, `~/.agent.toml` |
| list_directory project boundary | `src/tools/shell.rs:list_directory_native` | Both sides canonicalized before `starts_with` — fixes Windows `\\?\` long-path prefix mismatch that caused every call to fail with "outside project" |
| glob_read symlink-safe walker | `src/tools/file.rs:glob_walk_safe` | Replaces `glob::glob()` (no cycle detection) with a custom walker that skips symlinks; prevents `.kiro/skills/.kiro/…` infinite loops while still walking real hidden dirs (`.kiro`, `.claude`) |
| Recursive shell listing blocked | `src/tools/shell.rs:BLOCKED_PATTERNS` | `dir /s`, `ls -R`, `ls --recursive` added to blocked patterns — they hang forever on symlink cycles; model directed to use `list_directory`/`glob_read` instead |
| Shell: no file listing rule | `src/context_manager.rs` | System prompt explicitly forbids using `shell` for directory listing/file discovery |
| Project Overview answers summary in-place | `src/context_manager.rs` | `understanding.md` inlined section now instructs model to answer summary/overview/architecture questions directly from it without calling tools |
| walk_dir_for_map symlink guard | `src/tools/search.rs:walk_dir_for_map` | `!path.is_symlink()` guard before `is_dir()` / `is_file()` — defense-in-depth against symlink cycles |
| Orphaned tool_use repair | `src/session/mod.rs:handle_user_turn` | At start of every turn, inject synthetic `ToolResult` blocks for any orphaned `ToolUse` from an interrupted turn (Ctrl+C, secrets abort) — prevents HTTP 400 "tool_use ids without tool_result blocks" loop |
| ~/.agent.toml permissions | `src/config.rs:Config::save` | chmod 0600 on save (Unix) |
| Mutex poison recovery | `src/snapshot.rs` | `unwrap_or_else(|e| e.into_inner())` — no panic on poisoned lock |
| Pre-edit snapshots | `src/snapshot.rs` | `/undo <file>` or `undo_edit` tool |
| Audit log | `src/audit.rs` | every event → `~/.zap/audit.jsonl` (JSONL, global) |
| `/audit [N]` | `src/session/commands.rs:cmd_audit` | last N entries |

### CI / headless / remote control
| Feature | File | Notes |
|---|---|---|
| `--auto` / `-y` flag | `src/cli.rs` | shorthand for AGENT_PERMISSION_MODE=auto; no env var needed |
| `--sdk` mode | `src/agent_core.rs:run_sdk` | JSON-lines stdin→stdout; multi-turn session; NO_COLOR auto-set; stderr for terminal noise |
| SDK protocol | `src/agent_core.rs:run_sdk` | stdin: `{"type":"user","text":"..."}` / `{"type":"quit"}`; stdout: `{"type":"assistant","text":"...","turn":N,"ctx_pct":N}` |

### Session & persistence
| Feature | File | Notes |
|---|---|---|
| SQLite session store | `src/persistence.rs` | `~/.zap/agent.db` (global — shared across all projects) |
| Message persistence | `src/persistence.rs:save_messages` | serialised after every turn |
| Fix: session conversation not replayed on restart (v0.13.65) | `src/session/commands/code.rs:save_context_inner` | `save_messages` was gated to compaction + new-session-in-TUI only — normal session exit never persisted messages to the DB. On restart, `replay_last_session_into_app` found no messages row for the previous session and fell back to an older compacted session (or showed nothing). Fix: added `save_messages` call to `save_context_inner` so every clean exit persists the conversation. |
| Session resume | `src/session.rs:cmd_sessions` | fuzzy picker via inquire |
| Key-value memory | `src/persistence.rs` | `/memory list/get/set/del` |
| memory_set / memory_delete LLM tools (v0.13.84) | `src/tools/memory.rs`, `src/session/memory_refresh.rs`, `src/session/turn.rs` | Two new tools let the LLM persist and delete memory facts autonomously without user involvement. `MEMORY_DIRTY` AtomicBool flag is set on success; `patch_memory_in_system()` replaces the `## Agent Memory` block in `self.system` after each tool round so the next LLM call in the same session sees updated facts. Fix (same version): block_len calc consumed the `\n\n` section separator — corrected to `rel + 1`; memory_delete on absent key no longer sets dirty flag or returns a false "Deleted" message. |
| Branch storage | `src/persistence.rs:save_branch` | per session, in SQLite |

### Context
| Feature | File | Notes |
|---|---|---|
| System prompt builder | `src/context_manager.rs` | identity, env, code nav, tool policy, security rules |
| Language-agnostic identity | `src/context_manager.rs` | reads language from `.zap/project.json`; identity line is e.g. "AI coding agent (rust)" or generic "AI coding agent" when unknown |
| Casual system prompt | `src/context_manager.rs:build_casual_system_prompt` | ~50-token minimal prompt for greeting/casual turns; skips code-nav, tool-policy, security, CLAUDE.md, git status |
| ZAP.md loading | `src/context_manager.rs:load_claude_md` | walks cwd → $HOME, global `~/.claude/CLAUDE.md` |
| understanding.md inlined in system prompt | `src/context_manager.rs` | Always included (capped 4k chars): auto-stats block always present; full LLM Analysis section shown when `/init` has run. Used as technical background for code tasks — model instructed NOT to recite it verbatim for user-facing queries; "summarize"/"overview" answers should be plain end-user language. `context.md` and `session_log.md` remain lazy hints. |
| Code navigation policy: code_map before read_file | `src/context_manager.rs` | Explicit rule: never call `read_file` on a file before calling `code_map` on it. `find_definition` for known symbol names, `search_code` only when symbol is unknown. Prevents the LLM defaulting to 20+ read_file calls instead of the AST index. |
| list_directory hard limit: once per turn, root only | `src/context_manager.rs`, `src/tools/shell.rs` | list_directory capped to 1 call per turn on root only; never chain across subdirectories. Self-orientation routine replaced list_directory step with `code_map '.'`. Tool output appends a reminder: "non-recursive — use code_map on subdirs". Fixes recursive file enumeration on large projects. |
| Self-orientation when /init not run | `src/context_manager.rs` | When `understanding.md` has no LLM analysis, system prompt injects an active exploration routine: list_directory root → code_map key dirs → read manifests → code_map entry point. LLM answers from what it discovers, not guesses. Same quality floor regardless of whether /init was run. |
| /init produces navigation map not code review | `src/session/commands.rs:cmd_init_direct` | Init LLM prompt rewritten around goal: "where do I go for X?" not "how does X work?". Uses code_map (index) for symbol/file map, reads only manifest + entry point. understanding.md structured as: Entry Points, Module Map (table), Where To Find X (lookup), Non-Obvious Constraints. No recursive listing, no glob **\/*, no reading every file. |
| Silent language detection on first launch | `src/tui/mod.rs` | New projects: auto-detect language, silently save `project.json`, then proceed — no wizard, no indexing, no LLM call. Indexing and analysis only happen when user explicitly runs `/init`. Unindexed projects see a tip clarifying: indexing is 100% local (tree-sitter → .zap/code.db SQLite), nothing sent to cloud — only typed messages go to the LLM. Same message shown in /init output after indexing completes. |
| detect_project_type extension fallback | `src/session/commands.rs:detect_project_type` | When no build manifest found, scans file extensions (`.rs`, `.py`, `.ts`, etc.) and returns the dominant language. Projects with only docs/markdown/config return "general" instead of "". Empty directories still return "" to trigger the wizard. |
| understanding.md auto-refresh | `src/project.rs:refresh_understanding_md`, `src/session/mod.rs:Session::new` | At every session start, rewrites the `<!-- zap:auto-stats:begin/end -->` block with deterministic facts: version (Cargo.toml/package.json), file+symbol counts, language stats, source module list (including sub-directories like `tui/`, `session/`, `tools/`), built-in skill count. LLM-written `/init` content below the block is preserved. No LLM call — zero latency. |
| Git status in prompt | `src/context_manager.rs:git_status_summary` | 2s timeout |
| Agent memory in prompt | `src/context_manager.rs` | from SQLite store |

### Workflows
| Feature | File | Notes |
|---|---|---|
| Workflow parser | `src/workflow.rs:load_workflow` | YAML, `.zap/workflows/*.yaml` |
| Workflow execution | `src/session.rs:cmd_run_workflow` | sequential steps, approval gate |
| Workflow discovery | `src/workflow.rs:discover_workflows` | listed by `/run` with no args |
| Workflow scaffold | `src/workflow.rs:scaffold_workflow` | `/workflow new <name>` |

### UI & UX
| Feature | File | Notes |
|---|---|---|
| TUI (ratatui) | `src/tui/` | Scrollable conversation, header bar, input box, streaming; default mode; `--cli` for REPL |
| TUI textarea input | `src/tui/render.rs:draw_input` | Input area renders as bordered box (always visible); replaces bare top-border line |
| Windows TTY check | `src/session/mod.rs`, `src/task_planner.rs` | `libc::STDIN_FILENO` → `0 as libc::c_int`; `STDIN_FILENO` not exported on Windows MSVC |
| TUI syntax highlighting | `src/tui/syntax.rs` | syntect integration, 50+ languages, base16-ocean.dark theme |
| TUI markdown rendering | `src/tui/syntax.rs` | pulldown-cmark, bold/italic/headers/lists/links |
| TUI git status | `src/tui/mod.rs` | branch with dirty/ahead/behind indicators in header |
| TUI diff rendering | `src/tui/syntax.rs` | color-coded diffs (green/red/cyan) |
| TUI file browser | `src/tui/file_browser.rs` | Ctrl+F, tree view, syntax-highlighted preview, git status |
| TUI directory picker | `src/tui/mod.rs` | Ctrl+O, native macOS/Windows folder picker |
| TUI session picker | `src/tui/app.rs` + `src/tui/render.rs` | `/sessions` opens centred overlay (↑↓/Enter/Esc) — no CLI breakout |
| `/new` command (TUI) | `src/tui/commands.rs` | clears history, creates new DB session, stays in TUI |
| Windows key doubling fix | `src/tui/mod.rs` | skip `KeyEventKind::Release` events so each key fires once |
| Panic hook + log | `src/main.rs` | restores terminal on panic, writes `[PANIC]` to `~/.zap/zap.log` |
| Tracing to stderr | `src/main.rs` | tracing no longer corrupts TUI alternate screen |
| TUI permissions | `src/permission_manager.rs` | native TUI dialogs, no CLI breakout |
| Permission popup key routing fix | `src/tui/mod.rs`, `src/tui/input.rs` | Permission popup moved to top of key-handler priority chain — previously btw_mode or secret_popup could consume Y/A before it reached the permission handler. Enter added as allow key alongside Y. |
| grant_session helper | `src/permission_manager.rs:grant_session` | Public helper to pre-grant tools by name (respects grant classes). Reserved for explicit user consent flows. |
| TUI permission event-race fix | `src/tui/channel.rs`, `src/permission_manager.rs`, `src/tui/mod.rs` | `PERM_PROMPT_ACTIVE` AtomicBool: set while `prompt_batch_tui` owns the crossterm queue; TUI tick loop skips its own `event::poll` so Y/N/A keypresses aren't stolen — fixes MCP/shell dialogs hanging silently |
| DeepSeek vision block | `src/llm_client.rs:provider_supports_vision`, `OpenAiClient::new` | all deepseek.com models reject `image_url` content blocks (text-only API); /paste and /attach warn immediately; history drops are silent |
| Deploy skill | `src/default_skills/deploy.md` | triggers on: deploy, release, ship, publish; nohup background pattern (`nohup bash scripts/deploy.sh > /tmp/zap-deploy.log 2>&1 &`) avoids the 60s shell timeout |
| `/deploy [--check]` | `src/session/commands.rs:cmd_deploy`, `src/session/mod.rs:handle_slash` | streams live output from `bash scripts/deploy.sh` with no timeout; `--check` just shows installed versions; bypasses the LLM entirely — build output arrives line-by-line via tokio async read |
| TUI scrollbar | `src/tui/render.rs:draw_messages` | `Scrollbar`/`ScrollbarState` overlay on the messages area; only shown when content overflows the viewport height |
| Dynamic skill picker | `src/tui/commands.rs:filter_commands` | `/skill ` shows sub-commands (list/use/unuse/show/scope); `/skill use <name>` auto-completes loaded skill names; `filter_commands` now accepts `skill_names: &[String]`; `App::skill_names` populated at session start and refreshed after `/skill` commands |
| Thinking spinner | `src/ui.rs:ThinkingSpinner` | manual tick (no enable_steady_tick) + `stopped` flag; before_output waits for thread exit before clearing bar — prevents Windows terminal race |
| Colored diff on edit | `src/ui.rs` | similar crate, red/green |
| Token + cost display | `src/session.rs:handle_user_turn` | per-turn: skills t, msg t, ctx k, est. $ |
| Tab completion | `src/ui.rs:ZapHelper` | slash commands |
| Slash command picker | `src/ui.rs:show_command_picker` | `/` on empty line opens inquire picker |
| Startup noise reduction | `src/tui/startup.rs:49` | Removed "Ready. X tools loaded" and skill count from TUI startup — reduces visual noise on every launch |
| Image attach | `src/session.rs:cmd_attach` | staged until next message |
| Clipboard image paste | `src/session/commands.rs:cmd_paste` | macOS: pngpaste/AppleScript · Windows: PowerShell Clipboard::GetImage · Linux: xclip/wl-paste |
| Ctrl+V text paste (v0.13.87) | `src/tui/lifecycle.rs:handle_paste_image`, `src/session/commands/media.rs` | Ctrl+V now detects clipboard content: images → attaches (with 128-byte guard against corrupt tiny files); text → inserts into input at cursor. Added `paste_clipboard_text()` with pbpaste (macOS), Get-Clipboard -Raw with UTF-8 encoding (Windows), wl-paste/xclip (Linux). |
| Skill production-grade hardening (v0.13.88) | `src/skill_manager.rs`, `src/session/turn.rs`, `src/config.rs`, `src/session/mod.rs` | Multi-word trigger word-boundary: `"literature review"` no longer falsely triggers code-review. Skill priority field (`priority: 0-9` in frontmatter). Token budget enforcement: `skill_token_budget` config (default 4000) caps per-turn skill tokens; `rank_and_truncate_skills()` sorts by priority→source tier→tokens. Compaction skill-awareness: compaction check includes projected skill tokens. |
| Fix: todo test race condition — serialize write tests through async_lock (v0.13.94) | `src/tools/todo.rs` | `write_empty_array_reports_zero` was calling `set_todos([])` concurrently with `read_shows_items_and_icons`, wiping global state between write and read. Added `async_lock()` to all async tests that mutate `TODO_LIST`. |
| Fix: parse YAML-list triggers from Claude Code / cross-agent skills (v0.13.100) | `src/skill_manager.rs:parse_frontmatter` | Accept `triggers:` as alias for `trigger:` and parse YAML block-list items (`- item`) so skills from `~/.claude/skills` (Claude Code) load with proper trigger matching instead of becoming always-on. |
| Tree-sitter C# indexing (v0.13.101) | `src/code_index/extract.rs`, `src/code_index/walk.rs`, `src/code_index/index_impl.rs`, `Cargo.toml` | New `tree-sitter-c-sharp` dependency (v0.20.0). Full tree-sitter symbol extraction for `.cs` files: classes, structs, interfaces, enums, records, namespaces (context tracking), methods, constructors, properties, delegates, and `const`/`static readonly` fields. Integrated into language detection, file walk, and the `extract_symbols` dispatch. |
| Fix: LM Studio & Ollama provider detection — "✓ ready" only when installed (v0.13.102) | `src/llm_client/auth.rs`, `src/tui/startup.rs`, `src/session/commands/provider.rs` | Added `check_ollama()` (checks `ollama` binary on PATH) and `check_lm_studio()` (checks `/Applications/LM Studio.app`, known config dirs, and `lms` CLI) with 60s caching. Previously both showed "✓ ready" unconditionally in the provider picker even when not installed. Updated both the CLI `/provider` command and the TUI onboarding picker. Also fixed `maybe_open_onboarding_picker` to check `config.api_key` (legacy config field) in addition to `all_providers` and env vars — users with an OpenAI-compatible provider configured via legacy `~/.agent.toml` format no longer see the "no provider configured" popup on first launch. |
| Fix: remove "found via search/index" trailing instruction from system prompt (v0.13.99) | `src/context_manager.rs` | Removed the "End every answer with one line on how you found it" instruction. The LLM was garbling it into nonsense like "found via search index" — a mashup of the two options. No value to the user; pure noise. |
| Fix: index-aware system prompt — grep-first when no index, no hardcoded Cargo.toml (v0.13.98) | `src/context_manager.rs` | (1) System prompt now checks if `.zap/code.db` exists at startup. When not built: navigation strategy switches to `list_directory → search_code → read_file`; `code_map` and `find_definition` are explicitly forbidden. When built: keeps the index-first strategy. (2) Manifest exception (formerly hardcoded `Cargo.toml, package.json, go.mod`) now checks which manifest files actually exist in the project root and only mentions those — prevents the LLM from trying `Cargo.toml` in a plain HTML project. (3) Project Orientation explore steps also branch on index state. |
| Fix: Up/Down arrow scrolls content when input empty; history nav only when typing (v0.13.97) | `src/tui/input.rs` | History navigation (Up/Down) now only activates when the user has already typed something in the input box. From an empty prompt, Up/Down scrolls the content area as before. This restores content scrollability which was broken when any prompt history existed. |
| Fix: code_map supports HTML/CSS/MD + shows all files + index-not-built hint (v0.13.97) | `src/tools/search/search_impl.rs` | (1) `walk_dir_for_map` now includes HTML, CSS, SCSS, MD, MDX, JSX, TSX, Vue, Svelte, PHP, Kotlin, C#, TOML, YAML, JSON. (2) Files are always listed even when no symbols are extracted (shows file type label). (3) Symbol extraction added for HTML (headings/landmarks), CSS (selectors/@rules), Markdown (headings). (4) When the code index is in-memory (not yet built), code_map appends a note suggesting to run `/index`. |
| Queued input — type next prompt while turn is in progress (v0.13.96) | `src/tui/app.rs`, `src/tui/input.rs`, `src/tui/mod.rs`, `src/tui/render/layout.rs` | Pressing Enter during a busy turn now queues the typed text instead of discarding it. The input box switches to a `⏎ queued — Esc to cancel` view showing the queued message dimmed. When the turn finishes, the queued message fires automatically as the next turn. Esc cancels the queue (first Esc) without clearing the current draft. |
| install.ps1 for Windows — PATH setup + offline install (v0.13.95) | `install.ps1`, `.github/workflows/build.yml` | PowerShell installer bundled in the Windows `.zip` package. Same dual-mode logic as `install.sh`: uses local `zap.exe` when run from an extracted package, falls back to GitHub API download. Adds `~\\.local\\bin` to the user PATH via `[Environment]::SetEnvironmentVariable` (user scope, no admin required). Restart terminal for PATH to take effect. |
| install.sh bundled in release package (v0.13.93) | `install.sh`, `.github/workflows/build.yml` | `install.sh` is now included alongside the binary in every `.tar.gz` release package. Supports dual-mode operation: detects `BASH_SOURCE[0]` — if the script is run from an extracted package directory (not piped via `curl`), it uses the local binary and skips the GitHub API download entirely. Corporate users who download and extract the package can run `./install.sh` with no internet access required. |
| Fix: domain scope picker auto-popup + lazy code.db creation (v0.13.92) | `src/tui/mod.rs`, `src/session/mod.rs`, `src/session/commands/index.rs`, `src/session/commands/code.rs`, `src/code_index/index_impl.rs` | (1) **Domain picker removed**: the automatic language scope overlay that fired on every revisit to a project with no detected language is gone. Domain scope is still auto-detected from manifest files / file extensions; users can change it via `/skill scope`. (2) **Lazy code.db**: `Session::new()` no longer creates `.zap/code.db` at startup — it starts with an in-memory SQLite index when the DB file doesn't exist yet. `cmd_index` (and `/init` with indexing) upgrades the global index to file-backed on first use. Eliminates 200–500 ms SQLite WAL-mode initialization cost on first visit to any project. |
| Log rotation — trim llm.log + llm_requests to 24 h (v0.13.91) | `src/log.rs`, `src/cli.rs` | `rotate_logs()` spawns a background thread at startup: (1) `trim_llm_log` scans `~/.zap/llm.log` for the first `=== TIMESTAMP` entry within the last 24 h, stream-copies from that byte offset to a temp file, then atomically renames — O(1) memory regardless of file size; (2) `trim_llm_requests` deletes `~/.zap/llm_requests/` files whose mtime is older than 24 h. Runs fully in background — no startup blocking. |
| Summarization frequency fix + paste cursor fix (v0.13.89) | `src/session/summarizer.rs`, `src/skill_manager.rs`, `src/tui/lifecycle.rs` | (1) **Summarization gate**: `maybe_summarize_dropped_turns` now only fires when context fill ≥ 75% (`ZAP_SUMMARIZE_THRESHOLD` env var); previously fired every turn once session exceeded the window size (8 turns), triggering a costly LLM call per single dropped turn. Min-batch lowered to 3. Notice text changed to "Summarizing older history…". (2) **Paste cursor fix**: Ctrl+V text paste now advances cursor by pasted length instead of jumping to end of all input. (3) **Skill panic guard**: `windows(0)` panic when a multi-word trigger is all-whitespace is now guarded with `if trig_words.is_empty() { return false }`. Dead `_remaining` variable removed from `rank_and_truncate_skills`. |
| `/help` | `src/tui/commands.rs:help_text` | grouped command reference; includes goal, deploy, index quality, diff, attach, paste, skill log, remote |
| `/config` | `src/session.rs:cmd_config` | provider, model, URL, mode |
| `/cost` | `src/session.rs:cmd_cost` | session token totals + est. $ |
| MCP (lazy-loaded) | `src/mcp.rs` + `src/tools/mod.rs` | stdio JSON-RPC 2.0; servers stay in `pending_mcp` at startup — zero process overhead; a synthetic `mcp_connect` tool is injected into every turn's tool list with per-server `description` + `toolsHint` lines so the LLM knows when to connect without paying for actual tool defs; on `mcp_connect(server)` call the process spawns, handshake runs, real tools are registered, and `tool_defs` is rebuilt for the next turn; servers from `~/.zap/mcp.json` (global) + `.mcp.json` (project); respects `disabled: true`; SSE/HTTP entries skipped with warning; `autoApprove`/`disabledTools`/`toolsHint` fields supported |
| MCP permission gate | `src/session/mod.rs`, `src/tools/mod.rs:is_mcp_tool` | in Ask mode, MCP tool calls are always shown for user approval (they aren't in WRITE_TOOLS so quick_check previously auto-approved them); `ToolRegistry::is_mcp_tool()` backed by a `HashSet` populated at connect time |
| MCP stderr visibility | `src/mcp.rs:McpServer::connect` | server stderr piped to a background task; each line forwarded via `zap_warn!` — auth errors, startup failures, and 401s now appear in the TUI chat and `~/.zap/zap.log` instead of being silently discarded |
| MCP permission context | `src/mcp.rs:McpTool::permission_context` | permission prompt shows `MCP · tool_name  (key=val  key=val)` — flat key=value pairs (strings truncated at 40 chars, nested objects skipped, max 4 args) |
| `/mcp` command | `src/session/commands.rs:cmd_mcp` | `list` — shows all servers (global/project, connected/pending); `edit` — opens `~/.zap/mcp.json` in $EDITOR; `edit project` — opens `.mcp.json`; `path` — prints file paths |
| API error URL in message | `src/llm_client.rs` | 404/40x errors include the exact constructed URL for instant diagnosis |
| base_url used as-is | `src/llm_client.rs` | when set, `base_url` is posted to directly — no path appended; gateway handles routing |
| Error log (screen + file) | `src/log.rs` | `zap_warn!`/`zap_error!` print to stdout AND append to `~/.zap/zap.log`; log path shown in `/config` |
| LLM I/O log | `src/log.rs` + `src/llm_client.rs` | every request and response (pretty JSON) appended to `~/.zap/llm.log`; tool schemas replaced with count summary to keep log readable; HTTP errors logged as ERROR blocks; image data redacted; path shown in `/config` |
| MCP command validation | `src/mcp.rs:validate_mcp_command` | blocks non-absolute paths (allowlist: node/python/npx/deno/…), shell metacharacters, `..` traversal |
| Shell dangerous-command guard | `src/tools/shell.rs:guard_shell` | blocks `rm -rf /~`, fork bomb, `mkfs`, `dd`, `curl\|sh`, `wget\|sh` — applies even in Auto mode |
| `--budget N` token cap | `src/cli.rs`, `src/config.rs`, `src/session/mod.rs` | overrides model context limit for fill-% tracking; warns at 80%, hard-stops at 100% |
| MCP startup banner | `src/session/mod.rs:Session::new` | shows `⬡ N MCP server(s) connected: name (M tools)` for successes and `✗ MCP 'name' failed: reason` for failures |
| Server description field | `src/mcp.rs:McpServerConfig` | optional `"description"` in `.mcp.json` stored and shown in `/mcp list` |
| `/init` | `src/session/commands.rs:cmd_init` | guided setup: language confirm, indexing, writes ZAP.md + .zap/project.json; agent fills in ZAP.md + creates .zap/understanding.md |

### Tests
| Area | File | Count |
|---|---|---|
| Permission modes, session grants, grant-class cross-grants, MCP "always" fallback, ctx newline contract | `src/permission_manager.rs` | 14 |
| MCP command validation: known interpreters, Windows .exe variants, absolute paths, metacharacter/traversal rejection | `src/mcp.rs` | 9 |
| Destructive pattern detection, safe commands, ShellTool permission_context newline contract | `src/tools/shell.rs` | 6 |
| `list_directory_native`: real dir, trailing slash, missing path, file path, empty dir | `src/tools/shell.rs` | 5 |
| `is_casual_message`: bare greetings, trailing text, acks, capability Q, mixed case, technical blocking, long msg, non-greeting prefix | `src/session/mod.rs` | 9 |
| `spawn_agent` char-based truncation regression (byte-slice panic fix) | `src/tools/agent.rs` | 3 |
| `filter_commands` skill completions | `src/tui/commands.rs` | 8 |
| Pre-push hook | `.git/hooks/pre-push` | runs `cargo test` before every push |

### E2E tests (`tests/e2e/`)
Black-box tests that run the installed `zap` binary and assert on observable output and file system state.  Run with `./tests/e2e/run_all.sh` or a single suite e.g. `./tests/e2e/run_all.sh test_basic`.

| Suite | File | What it covers |
|---|---|---|
| T01 Basic | `test_basic.sh` | Single-shot goal answer, no panic |
| T02 Tools | `test_tools.sh` | `list_directory`, `read_file`, `shell` tool use |
| T03 Index | `test_index.sh` | `/index` slash command; tree-sitter log lines; `.zap/code.db`; `/index stats` |
| T04 Init | `test_init.sh` | `/init` CLI + TUI modes; `project.json`, `ZAP.md` written; no nudge on 2nd run |
| T05 Session | `test_session.sh` | Session end writes `context.md` and `session_log.md` |
| T06 TUI | `test_tui.sh` | TUI starts with PTY, renders banner, exits cleanly — uses `script(1)` |
| T07 Regression | `test_regression.sh` | R01: UTF-8 char-boundary panic on large grep output with em-dashes; R02: `/sessions` no crash |


### Opus 4.8 worldclass improvements (v0.15.5–0.15.8)
| Feature | File | Notes |
|---|---|---|
| `batch_edit` count validation fix (v0.15.5) | `src/tools/file/edit.rs` | `batch_edit` count test now uses `replace_all: true` for multi-occurrence old_strings — validates against mutations |
| Panic audit — 0 bare `.unwrap()` calls (v0.15.6) | `src/llm_client/mod.rs`, `src/remote.rs`, `src/context_manager.rs`, `src/ui.rs`, `src/session/commands/` | 9 risky `.unwrap()` calls replaced with `.expect(...)` or `if let`; remaining bare unwraps are only in `#[cfg(test)]` blocks |
| Shell isolation — sandbox modes (v0.15.7) | `src/config.rs`, `src/tools/shell.rs`, `src/shell_runner.rs` | `SandboxMode` enum: `Off` (default), `Workdir` (sets `current_dir` to project root), `Container` (Docker/Podman with `--network none`, read-only mount, `--tmpfs /tmp`); configurable via `AGENT_SANDBOX` env var or `sandbox = "workdir"` in TOML; `ShellTool` accepts `SandboxMode` at construction |
| Security threat model doc (v0.15.7) | `docs/SECURITY.md` | Full threat model: sandbox modes, permission modes, secret scanner, audit trail, known limitations, honest labeling |
| Edit ledger — session memory beyond summary blob (v0.15.8) | `src/session/mod.rs`, `src/session/tools.rs`, `src/session/turn.rs` | `EditedFile` struct records `first_turn`, `last_turn`, `ops_count` per file; `Session::edited_files` `HashMap` populated on every tool execution via `affected_path()`; injected as "Edit Ledger" block into `effective_system` before each LLM call (sorted by recency, capped at 20 entries, skipped on casual turns); survives sliding-window eviction |
| Edit ledger mock-client tests (v0.15.8) | `src/session/agent_loop_tests.rs` | `edit_ledger_appears_on_next_turn`: verifies ledger injected on turn after file edit; `edit_ledger_persists_after_turns_slide_out_of_window`: 13-turn test verifying ledger still mentions turn-1 file after sliding window (window=8) |
| Documentation diet (v0.15.8) | `ARCHITECTURE.md`, `README.md` | New `ARCHITECTURE.md` (~300 lines) derived from source: module map, data flow, design decisions, testing strategy; README trimmed from 76KB to 5.2KB — what it is, install, quickstart, providers, config, slash commands, security, link to ARCHITECTURE; 17 aspirational `.md` files archived to `docs/archive/` |
| Graceful mutex degradation in slash-trigger path (v0.15.8) | `src/ui.rs`, `src/agent_core.rs` | Replaced the two remaining production `.lock().unwrap()` sites with `.lock().ok()` + safe fallback so a poisoned mutex degrades instead of panicking the REPL/readline thread; on poison the slash trigger is treated as "not fired". Completes the panic-audit goal of task 2.2 |
| Tool/skill disable list in config | `src/config/mod.rs` | `disabled_tools = ["shell", "web_fetch"]` and `disabled_skills = ["deploy"]` in `~/.agent.toml`; fields added to both `FileConfig` (TOML-deserializable) and `Config` struct, wired in `Config::load()`, defaulting to empty `Vec<String>`. Task 1 of tool-transparency plan. |

---

## Planned 🗓

### Bet C — Smart `.zap/` Project Intelligence

> Status key: ⬜ planned · 🔨 in progress · ✅ done · ⚠ redesigned (see notes)
>
> **Market context:** Claude Code has `/init` → manual CLAUDE.md (fills once, never auto-updates). Cursor auto-indexes silently. Aider has a repo map (condensed tree-sitter, always in context). Windsurf/Cascade has per-project Memories (auto-maintained, but it's a full IDE). **No CLI agent** does structured session handoff or auto-updates a project knowledge file at session end — that's the gap.
>
> **What already exists in zap** that this builds on (don't re-implement):
> - Stack detection: `detect_stack_skills` reads Cargo.toml/go.mod/package.json/pyproject.toml (`src/skill_manager.rs:469`)
> - `SessionEnd` hook: `fire_session_end()` already fires in all exit paths — CLI, TUI, SDK (`src/agent_core.rs:209`)
> - CLAUDE.md load + inject: `load_claude_md` already runs every session (`src/context_manager.rs`)
> - Code index: tree-sitter, SQLite, auto-reindex on write — fully built (`src/code_index.rs`)
> - `/init`: already creates and fills CLAUDE.md via LLM (`src/session/commands.rs:820`)

| # | Feature | Status | Files | Effort | Notes |
|---|---|---|---|---|---|
| C1 | `context.md` — session handoff file | ✅ done | `.zap/context.md`, `src/session/commands.rs:cmd_exit`, `src/hooks.rs` | 1 day | Written at session end via `SessionEnd` hook (already exists). Content: goal, what was done, what's next, files touched. On next startup: banner "Last session: X — Done: Y — Next: Z · Resume? [Y/n]". No competitor does this. |
| C2 | `project.json` — persist init state | ✅ done | `.zap/project.json`, `src/persistence.rs` or new `src/project.rs`, `src/session/mod.rs:Session::new` | 0.5 day | Thin file: `{language, indexed_at, initialized_at}`. On startup: if present, skip domain-scope prompt entirely (already detected). Builds on `detect_stack_skills` — no re-detection. |
| C3 | Indexing nudge on first open | ✅ done | `src/session/mod.rs:Session::new`, `src/session/commands.rs:cmd_index` | 0.5 day | If `project.json` missing or `indexed_at` is null, show one-time prompt: "This project hasn't been indexed yet. Indexing lets zap find symbols without reading every file. Index now? [Y/n]". Cursor does this silently; zap should explain the benefit. |
| C4 | `.zap/understanding.md` — auto-updated project knowledge | ✅ done | `.zap/understanding.md`, `src/session/commands.rs`, `src/context_manager.rs` | 2 days | Separate from user-controlled CLAUDE.md. Written/appended at session end via LLM summarization call. Sections: Architecture, Key Files, Patterns, Known Constraints. Listed as on-demand hint in system prompt (not pre-loaded every turn) — model reads via `read_file` when asked about architecture/overview. ⚠ Don't free-form rewrite — append with timestamps to avoid LLM drift. |
| C5 | `.zap/session_log.md` — session intent log | ✅ done | `.zap/session_log.md`, `src/session/commands.rs`, `src/hooks.rs` | 1 day | One entry per session: `{session_id, goal, files_touched, outcome}`. Written at session end. **Not** per-edit logging (redundant with git). The value is intent ("why") that git log doesn't have. Referenced by C1 context.md to show recent history. |
| C6 | `/init` upgrade — guided onboarding flow | ✅ done | `src/session/commands.rs:cmd_init` | 1 day | Extend existing `/init`: (1) detect + confirm language, (2) offer indexing with explanation, (3) write `project.json`, (4) fill CLAUDE.md as today, (5) print "Project initialized — zap will remember this project." Make `/init` the recommended first step, shown in startup hint for new projects. |

**Implementation order:** C2 → C3 → C1 → C6 → C4 → C5 (C2/C3 are small+safe, C1 is highest value, C4/C5 need C1 infrastructure)

**Risks to watch:**
- `understanding.md` injection token cost: cap at 2000 tokens, summarize if over limit
- `SessionEnd` hook doesn't fire on SIGKILL — context saves are best-effort (clean exit = guaranteed)
- `context.md` resume banner must be skippable (Esc/n) — can't block users who don't want it

---

### Bet A — Skill ecosystem (priority order)

| Feature | What it does | Effort |
|---|---|---|
| `/skill install github:user/repo/path` | fetch skill from GitHub raw URL → `~/.zap/skills/` | 1 day |
| Skill `extends:` composition | inherit another skill's content, then add rules | 1 day |
| Semantic skill routing | fastembed local embeddings instead of keyword match | 2 days |
| Public skill directory | `zap.sh/skills` — browse, search, install community skills | 1 week |
| Cross-agent compat test | verify skill files work in Claude Code and Cursor | 0.5 day |
| Stack detection expansion | Ruby (Gemfile), Swift (Package.swift), Kotlin (build.gradle.kts), C++ (CMakeLists.txt) | 1 day |

### Quality & foundations

| Feature | What it does | Effort |
|---|---|---|
| Integration test suite | skill loader, permission flow, session round-trip | 2 days |
| Break up session.rs | split slash handlers into separate modules | 1 day |
| Multi-model routing | cheap model for tool calls, capable model for generation | 2 days |
| Token budget flag | `--budget N` warns at 80%, stops at 100% | 0.5 day |
| Prompt caching breakpoints | `cache_control: ephemeral` on Anthropic for ~90% cost reduction on repeated turns | 0.5 day |
| Per-session permission memory | re-prompt only once per tool class per session | 0.5 day |

### Bet B — CC-inspired capabilities (priority order)

Features from Claude Code worth bringing into zap. IDE integration, voice, enterprise MDM, and OpenTelemetry explicitly excluded — wrong scope for a single-binary local tool.

| Priority | Feature | What it does | Effort |
|---|---|---|---|
| P1 | Effort levels (`/effort low\|medium\|high`) | Upgrade binary `/think on/off` to a 3-step thinking budget — low (~1k tokens), medium (8k, today's default), high (32k+); low effort saves real money on simple tasks | 0.5 day |
| P1 | Prompt caching breakpoints *(already in foundations)* | `cache_control: ephemeral` on Anthropic system prompt + skill injections; ~90% cost reduction on repeated long sessions — promote to P1, implement before other Bet B items | 0.5 day |
| P1 | MCP `tools/list` pagination | Handle servers that page tool listings with a cursor; today only the first page loads — breaks any large MCP server | 0.5 day |
| ~~P2~~ ✅ | `/goal` autonomous loop | **Shipped v0.8.3.** `/goal <cond>` runs until `✓ DONE` or max turns. Goal section in sidebar/status/dir panel. `/goal stop` cancels. | done |
| P2 | HTTP/SSE MCP servers + OAuth | Support remote MCP servers over HTTP/SSE transport (currently skipped with warning); OAuth bearer token with refresh; opens the full remote MCP ecosystem | 2–3 days |
| P2 | MCP incremental reconnect | On transient MCP failure, retry with exponential backoff instead of marking server permanently failed for the session | 1 day |
| P3 | Background / daemon sessions | `zap --bg "refactor auth"` spawns a detached session written to DB; `zap agents` shows live status; `zap attach <id>` to resume — biggest capability gap vs CC today | 1 week |
| P3 | AWS Bedrock provider | Native Bedrock API with SigV4 auth + Claude model ARNs; required for teams locked to AWS | 2 days |
| P3 | Google Vertex provider | Native Vertex AI endpoint with service-account auth; required for teams locked to GCP | 2 days |
| P3 | Custom TUI themes (`/theme`) | Named palettes (dark/light/high-contrast/custom); saved to `~/.agent.toml`; fixes the one visible polish gap vs CC | 1 day |

---

| Image paste fix for DeepSeek (v0.10.1) | `src/llm_client.rs` | When using `base_url = "https://api.deepseek.com"`, `/paste` or `/attach <image>` no longer sends `image_url` content blocks (which DeepSeek rejects with 400). `OpenAiClient` added `image_support: bool` — auto-detected `false` for DeepSeek, `true` for others. Image blocks are silently dropped with a log warning instead of crashing the request.
|
## Cut / deferred ✗

| Feature | Why cut |
|---|---|
| Syntax highlighting (syntect) | 4MB+ dep, polish not substance |
| Session replay / export | nice-to-have, not a reason to choose zap |
| `find_definition` as standalone module | `code_map` + ripgrep covers 80% for free |
| "200 token baseline" marketing claim | real baseline with karpathy-guidelines is ~1.8k; update messaging to be accurate |

---

## Baseline token budget (honest numbers)

### Normal turn (system prompt + tools + karpathy skill)
| Component | Tokens |
|---|---|
| Identity + environment section | ~120 |
| Code nav strategy | ~280 |
| Tool usage policy | ~380 |
| Security rules | ~160 |
| Response style | ~120 |
| CLAUDE.md (if present) | varies |
| Agent memory (if any entries) | varies |
| `karpathy-guidelines` (always-on) | ~600 |
| Tool definitions (~20 tools) | ~1,800 |
| **Total baseline (no CLAUDE.md, no memory)** | **~3,460 tokens** |
| Per triggered skill (avg) | +400–800 |

### Casual turn (greeting / ack — `is_casual_message()` = true)
| Component | Tokens |
|---|---|
| Minimal system prompt (2 lines) | ~20 |
| Tool definitions | 0 (skipped) |
| Skills | 0 (skipped) |
| **Total baseline** | **~20–30 tokens** |
