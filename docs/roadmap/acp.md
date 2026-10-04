# ACP (Agent Client Protocol) support — `zap acp`

Status: **implemented in v0.16.0** (steps 1–9) · step 10 (registry submission) pending — see `docs/acp-registry/`

## Goal

Run zap as a native agent inside ACP clients — Zed, JetBrains IDEs, and VS Code
(via community extensions such as `vscode-acp-provider`), Neovim, Emacs — with
the same skills, code index, providers and permission model as the TUI.

## Protocol version decision

ACP v2 exists but is still labelled **draft** and the spec itself says to gate
it behind version negotiation. Every shipping client speaks **v1**. So:

- Implement **v1 only**. In `initialize`, answer `protocolVersion: 1` even if
  the client offers 2 (the spec's negotiation rule).
- Keep the protocol layer thin (one adapter module) so v2 is an additive
  adapter later, not a rewrite.

Use the official `agent-client-protocol` crate (Zed uses it; 2.2.0 as of
2026-09) and implement its `Agent` trait — no hand-rolled JSON-RPC.

## Architecture findings that shape the design

1. **stdout is not safe.** ~500 `println!`/`print!` call sites outside
   `src/tui/` (commands, hooks, session, permission CLI prompt). One stray
   line corrupts the JSON-RPC stream. Fix at the process level, not per call
   site: at `zap acp` startup, `dup` fd 1 into a private handle used only by
   the ACP transport, then `dup2(stderr, 1)` so every other print lands on
   stderr. (The same latent bug exists in `--sdk`; fix it there too.)
2. **stdin is not safe.** The non-TUI permission path
   (`permission_manager.rs::prompt_batch`) reads `io::stdin()` — in ACP mode
   that would steal protocol bytes. ACP mode must never reach that branch.
3. **The event surface already exists.** Streaming text, thinking, tool
   start/done, cost, notices and permission requests are all emitted through
   `tui::channel` (`TuiEvent`, `PERM_REQUEST`) when `is_tui_mode()` is true.
   ACP mode installs that sender and translates events → `session/update`,
   instead of building a second emission path through the agent loop.
4. **Those channels are process-global singletons** (`OnceLock`). Therefore:
   one *active prompt* at a time per process. Multiple ACP sessions may exist,
   but prompts are serialized; a second concurrent `session/prompt` waits.
   Zed spawns one agent process per external-agent thread, so this is fine
   for v1. Lifting it means making the sink per-session (task-local) — later.
5. **Cancellation already works** by dropping the turn future (TUI Ctrl+C),
   and `session/turn.rs` repairs orphaned `tool_use` blocks on the next turn.
   `session/cancel` = abort the turn task, reply `stopReason: "cancelled"`.

## Method mapping (v1)

| ACP | zap |
|---|---|
| `initialize` | advertise `promptCapabilities.image` (provider permitting), `embeddedContext`, `loadSession: true`, MCP `stdio`; return `agentInfo` |
| `authenticate` | one `terminal`-type auth method: client launches `zap` interactively → existing `/provider` onboarding |
| `session/new` | `Session::new` with `cwd` from params; connect client-supplied `mcpServers` through existing MCP manager |
| `session/load` | resume a zap session by id from the existing session store, replay history as `user_message_chunk` / `agent_message_chunk` |
| `session/prompt` | `session.handle_user_turn`; text + image + resource (`@file`) content blocks; returns `stopReason` (`end_turn`, `cancelled`, `max_tokens`, `refusal`) |
| `session/cancel` | abort the running turn |
| `session/set_mode` | map `ask` / `auto` / `plan`-less modes onto `PermissionMode` (`ask`, `auto`, `deny`→"read-only") |
| `session/request_permission` (agent→client) | replaces `prompt_batch_tui`; options `allow_once`, `allow_always`, `reject_once` map to `PermissionDecision::{Allow, Always, Deny}` |

## Event mapping (`TuiEvent` → `session/update`)

| TuiEvent | session/update |
|---|---|
| `LlmChunk` | `agent_message_chunk` |
| `ThinkingChunk` | `agent_thought_chunk` |
| `ToolStart` | `tool_call` (status `pending`→`in_progress`, `kind` from tool: read/edit/execute/search/fetch/think, `locations` from `affected_path()`) |
| `ToolDone` | `tool_call_update` (`completed`/`failed`, content = preview; for edit tools, a `diff` content block) |
| todo tool updates | `plan` |
| `Notice` / `Warning` | `agent_message_chunk` (prefixed), not dropped |
| `CostUpdate` / `ContextUpdate` | ignored in v1 |

Diffs need one small addition: `ToolDone` (or a sibling event) must carry
old/new text for edit tools — the undo snapshot already has the "before".

## Out of scope for v1

- Client file-system (`fs/*`) and terminal (`terminal/*`) delegation — optional
  in v1 and **removed in v2**; zap keeps its own file and shell tools.
- Slash commands over ACP (`available_commands_update`) — follow-up.
- ACP v2.

## Steps (one commit each, tests with each)

1. `zap acp` subcommand skeleton + stdout/stdin isolation (fd swap). Test:
   spawn `zap acp`, call `initialize`, assert only JSON on stdout.
2. `session/new` + `session/prompt` text-only, streaming `agent_message_chunk`,
   correct `stopReason`.
3. Tool calls → `tool_call` / `tool_call_update` with kinds + locations; diffs
   for edit tools.
4. `session/request_permission` wired into `PermissionManager`; never touches stdin.
5. `session/cancel`.
6. `session/load`, `session/set_mode`, images and `@file` resources, client MCP servers.
7. `authenticate` (terminal method) when no provider is configured.
8. Validate with the official **ACP TCK** (`agentclientprotocol/acp-tck`) —
   mandatory tier must pass; then manual check in Zed and one VS Code extension.
9. Docs: README + website section with Zed `settings.json`, JetBrains, and
   VS Code snippets; FEATURES.md entry; bump to v0.16.0.
10. Submit to the **ACP Registry** (`agentclientprotocol/registry`):
    `agent.json` with binary distribution pointing at existing GitHub release
    archives (`args: ["acp"]`) + monochrome 16×16 `icon.svg`. Requires step 7
    (registry only lists agents that support authentication).

## Risks

- Hidden stdout writers in dependencies (e.g. LSP child processes inheriting
  fd 1) — the fd swap covers children too, since they inherit the new fd 1.
- `is_tui_mode()` also gates TUI-only behaviour (spinners, notices). Audit its
  ~call sites; introduce `is_event_mode()` if ACP needs to diverge.
- Registry binary distribution: already satisfied — releases ship
  `zap-{macos,linux}-{arm64,x86_64}.tar.gz` and `zap-windows-x86_64.zip`.
  The registry `agent.json` pins a version + sha256, so each release needs a
  registry bump (automate later).
