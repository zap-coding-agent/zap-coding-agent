# zap — Feature Registry

Living reference of what's implemented, where it lives, and what's planned.
Update this file whenever a feature ships or a plan changes — no code scanning needed.

---

## Implemented ✅

### Task planner (src/task_planner.rs)
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
| Session core | `src/session/mod.rs` | struct, `new()`, tool loop, slash dispatcher |
| Slash commands | `src/session/commands.rs` | all `cmd_*` handlers, `/init` helpers |
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
| Secret scanner TUI fix | `src/session/mod.rs:handle_user_turn` | secret-scanner prompt now calls `suspend_for_prompt`/`resume_from_prompt` so the stdin read works correctly in TUI mode (previously the TUI owned stdin and the prompt hung) |
| TUI streaming auto-scroll fix | `src/tui/app.rs:apply_event` | `auto_scroll` is re-enabled on every `LlmChunk` so the viewport follows active streaming output even if the user scrolled up earlier in the turn; previously scrolling up mid-response caused the rest of the output to appear off-screen |
| Rotating thinking words | `src/tui/render.rs:THINKING_WORDS` | 200-word rotation; per-turn prime offset (`turn * 31`) ensures each response starts at a different word; ~640ms change interval (down from 1.3s) so variety is visible in short turns; status bar, sidebar, and chat all use the same index |
| TUI visual overhaul (v0.7.1) | `src/tui/render.rs` | Amber gradient ZAP art, muted purple `#3c3750` borders, `◆ You`/`◆ zap` role markers, diff-aware tool preview (+green/-red/@@blue), `✓`/`✗`/`⏺` tool icons with elapsed time, Ctrl+O expands last tool output (shows +N lines hint), dir picker moved to Ctrl+P |
| Domain scope picker in TUI | `src/tui/`, `src/skill_manager.rs`, `src/config.rs` | At TUI session start, if no scope auto-detected, shows ratatui overlay "existing project: scope to languages?" with project dir name; extension-based pre-checking (`.rs`→rust, `.py`→python, etc.); `skip_domain_prompt` flag prevents duplicate CLI inquire prompt; Esc = no restriction |
| Clean TUI startup (v0.7.2) | `src/tui/mod.rs`, `src/config.rs:tui_mode`, `src/session/mod.rs` | `tui_mode` flag suppresses all startup println!s (skills/hooks/MCP); Vibe/Task mode picker is now a TUI overlay (amber ❯ selection, descriptions); Task mode suspends TUI, runs task planning in CLI, resumes; skill/domain info shown in welcome message instead |
| TUI color + rendering fixes (v0.7.3) | `src/tui/render.rs` | Replace `│` (U+2502) markers with plain spaces in tool_call_lines and diff_block_lines — U+2502 rendered as 'd' on many terminals causing '181d' artifacts. Brighten all muted colors: preview text Rgb(205,200,225), context diff lines Rgb(175,170,200), tool names/labels, elapsed times, code block line numbers. Replace all `Color::DarkGray` with explicit Rgb values for consistent cross-terminal contrast. |
| TUI preview width clamp + scatter fix (v0.7.4) | `src/tui/render.rs` | Pass `width` through `tool_call_lines` and `diff_block_lines`; truncate every preview/diff line to `width-6` chars with `…` suffix — prevents long lines overflowing into sidebar and soft-wrapping as scattered characters. Tab expansion (`\t` → 4 spaces) stops tab-indented code rendering as giant gaps. Sidebar and header-info label/value colors brightened (Rgb(130,125,155) labels, Rgb(205,200,230) values). |
| Fix green/red diff lines leaking into TUI (v0.7.9) | `src/tools/file.rs` | `print_diff()` called `println!` with ANSI colored `+`/`-` lines directly to stdout inside `edit_file` and `batch_edit`, bypassing TUI mode — the output wrote into the alternate screen buffer mid-render. Gated both `print_diff()` calls with `if !is_tui_mode()` to match the existing session-level `print_tool_output` gate. |
| Fix `/sessions` resume — session_id + model not updated (v0.10.2) | `src/session/commands.rs:cmd_sessions` | After loading an old session, `self.session_id` was never updated so all subsequent saves wrote to the new (empty) session instead of the resumed one. Also updates `self.model` and rebuilds `self.client` to match the loaded session's model. |
| TUI diff viewer + view-changes hint (v0.11.3) | `src/tui/app.rs`, `src/tui/input.rs`, `src/tui/mod.rs`, `src/tui/render.rs` | After any turn where the agent writes/edits files, shows "✎ N files modified — Ctrl+G or /diff to view changes" in the chat. `Ctrl+G` opens the diff viewer from idle mode (previously `OpenDiffViewer` action existed but had no keybinding). Status bar keybinds updated to include `Ctrl+G diff`. `files_changed_this_turn` counter tracks `write_file`/`edit_file`/`batch_edit` ToolDone events and resets at turn start. |
| Tree-sitter logging (v0.11.1) | `src/code_index.rs:index_file`, `global_reindex_file`, `index_dir` | Every parse, reindex, and scan writes `INDEX tree-sitter · <lang> · <path> · N symbols` to `~/.zap/zap.log`; `/index` output names tree-sitter explicitly; reindex-after-edit was previously silent |
| Periodic background indexer (v0.11.1) | `src/code_index.rs:spawn_background_indexer`, `src/session/mod.rs:Session::new` | Spawns a tokio task at session start that runs `index_dir` every 120s (`ZAP_INDEX_INTERVAL` env var); uses `try_lock` to avoid blocking foreground reindex; logs to `zap.log` only — never pollutes stdout/TUI |
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
| Skill label in sidebar + line counts in files hint (v0.11.6) | `src/session/mod.rs`, `src/tui/channel.rs`, `src/tui/app.rs`, `src/tui/render.rs`, `src/tui/mod.rs` | Active skill no longer printed mid-chat via `println!`; in TUI mode sends `TuiEvent::ActiveSkill` which shows as a "skill / active" row in the sidebar (amber, cleared at turn end). "N files modified" hint now includes `(+A/−R)` line counts from `git diff HEAD --shortstat`. |
| Fix Ctrl+G diff viewer fallback + feedback (v0.11.5) | `src/tui/render.rs:open_diff_viewer`, `src/tui/app.rs`, `src/tui/mod.rs` | `open_diff_viewer` now falls back to `git diff HEAD~1` when the working tree is clean (previously returned `None` silently after a commit). Added `title` field to `DiffViewerState` shown in the panel header ("working changes" vs "last commit"). When both diffs are empty, sets `app.error` with a clear message instead of doing nothing. |
| Fix INDEX println! overlap in TUI (v0.11.4) | `src/log.rs:write` | Background tree-sitter threads called `log::write("INDEX",…)` which did a raw `println!` while ratatui owned the terminal; writes landed at whatever cursor position the last render left, scattering text across panel boundaries. Guarded the `println!` with `is_tui_mode()` — INDEX messages now go only to `zap.log` during TUI sessions. |
| Removed redundant git tools | `src/tools/shell.rs`, `src/tools/mod.rs` | `git_status`, `git_pull`, `git_diff` removed — model uses `shell` directly; saves ~250 tokens per request |
| Per-turn tool filtering | `src/session/mod.rs:select_tools_for_turn` | For OpenAI-compatible (local) providers, `web_fetch`/`web_search` are omitted unless the user mentions web/url/docs or web tools were already used this session; Anthropic sends all tools (cached) |
| Windows shell compatibility | `src/shell_runner.rs:run_command` | Uses PowerShell (`-NoProfile -NonInteractive`) on Windows, `sh -c` on Unix/macOS — PowerShell has `ls`/`sleep` aliases, fixing "command not recognised" failures from LLM-generated Unix commands |
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
| Topic-shift detection | `src/session/mod.rs:is_topic_shift` | vocabulary overlap heuristic; suggests `/branch` or `/exit` |
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

### Tools (src/tools/ or tool_registry.rs)
| Tool | Notes |
|---|---|
| `read_file` | offset/limit, line-numbered output |
| `edit_file` | find-and-replace, rejects ambiguous matches |
| `batch_edit` | multiple edits to one file, single diff |
| `write_file` | create or overwrite |
| `undo_edit` | restore from pre-edit snapshot |
| `shell` | with permission check; description required; output printed inline |
| `git_status` | status + recent log |
| `git_pull` | fetch + merge; `rebase` flag; triggered by "pull / sync / get latest"; output printed inline |
| `git_diff` | unstaged, staged (--cached), or between refs; output printed inline |
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
| `/index [path|stats]` | `src/session.rs:cmd_index` | manual reindex or stats; appears in / picker and tab-completion |

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
| Hook count banner | `src/session.rs:Session::new` | shown at startup if hooks are configured |
| `/hooks` | `src/session.rs:handle_slash` | lists all configured hooks grouped by event |

### Security & permissions
| Feature | File | Notes |
|---|---|---|
| Permission modes (ask/auto/deny) | `src/permission_manager.rs` | per-tool, per-session; WRITE_TOOLS includes batch_edit |
| Batch permission prompt | `src/permission_manager.rs:prompt_batch` | ONE grouped UI for all pending tool calls per turn instead of per-call prompts |
| Tool grant classes | `src/permission_manager.rs:tool_grant_class` | 'a' for edit_file grants write_file + batch_edit for the session |
| `Tool::affected_path()` | `src/tools/mod.rs` | trait method — tools declare what file they write; drives reindex |
| Secret scanner | `src/secret_scanner.rs` | 29 patterns: API keys, VCS tokens, AWS, GCP, JWT, cert blocks, credential fields |
| Path traversal guard | `src/tools/file.rs:guard_path` | normalizes `..`, blocks `~/.ssh`, `~/.aws`, `~/.kube`, certs, `/etc/shadow`, `~/.agent.toml` |
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
| Session resume | `src/session.rs:cmd_sessions` | fuzzy picker via inquire |
| Key-value memory | `src/persistence.rs` | `/memory list/get/set/del` |
| Branch storage | `src/persistence.rs:save_branch` | per session, in SQLite |

### Context
| Feature | File | Notes |
|---|---|---|
| System prompt builder | `src/context_manager.rs` | identity, env, code nav, tool policy, security rules |
| Language-agnostic identity | `src/context_manager.rs` | reads language from `.zap/project.json`; identity line is e.g. "AI coding agent (rust)" or generic "AI coding agent" when unknown |
| Casual system prompt | `src/context_manager.rs:build_casual_system_prompt` | ~50-token minimal prompt for greeting/casual turns; skips code-nav, tool-policy, security, CLAUDE.md, git status |
| ZAP.md loading | `src/context_manager.rs:load_claude_md` | walks cwd → $HOME, global `~/.claude/CLAUDE.md` |
| On-demand .zap knowledge hints | `src/context_manager.rs` | `understanding.md`, `context.md`, `session_log.md` listed as read-on-demand hints (not pre-loaded); model reads them via `read_file` only when the query warrants it — project summary, resume, history questions |
| understanding.md auto-refresh | `src/project.rs:refresh_understanding_md`, `src/session/mod.rs:Session::new` | At every session start, rewrites the `<!-- zap:auto-stats:begin/end -->` block with deterministic facts: version (Cargo.toml/package.json), file+symbol counts, language stats, source module list, built-in skill count. LLM-written `/init` content below the block is preserved. No LLM call — zero latency. |
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
| TUI permission event-race fix | `src/tui/channel.rs`, `src/permission_manager.rs`, `src/tui/mod.rs` | `PERM_PROMPT_ACTIVE` AtomicBool: set while `prompt_batch_tui` owns the crossterm queue; TUI tick loop skips its own `event::poll` so Y/N/A keypresses aren't stolen — fixes MCP/shell dialogs hanging silently |
| TUI scrollbar | `src/tui/render.rs:draw_messages` | `Scrollbar`/`ScrollbarState` overlay on the messages area; only shown when content overflows the viewport height |
| Dynamic skill picker | `src/tui/commands.rs:filter_commands` | `/skill ` shows sub-commands (list/use/unuse/show/scope); `/skill use <name>` auto-completes loaded skill names; `filter_commands` now accepts `skill_names: &[String]`; `App::skill_names` populated at session start and refreshed after `/skill` commands |
| Thinking spinner | `src/ui.rs:ThinkingSpinner` | manual tick (no enable_steady_tick) + `stopped` flag; before_output waits for thread exit before clearing bar — prevents Windows terminal race |
| Colored diff on edit | `src/ui.rs` | similar crate, red/green |
| Token + cost display | `src/session.rs:handle_user_turn` | per-turn: skills t, msg t, ctx k, est. $ |
| Tab completion | `src/ui.rs:ZapHelper` | slash commands |
| Slash command picker | `src/ui.rs:show_command_picker` | `/` on empty line opens inquire picker |
| Image attach | `src/session.rs:cmd_attach` | staged until next message |
| Clipboard paste | `src/session/commands.rs:cmd_paste` | macOS: pngpaste/AppleScript · Windows: PowerShell Clipboard::GetImage · Linux: xclip/wl-paste |
| `/help` | `src/session.rs:cmd_help` | grouped command reference |
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
