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
| Context pressure thresholds | `src/session/mod.rs:handle_user_turn` | 70% warn, 80% interactive choice, 95% auto-compact |
| Tool result truncation | `src/session/mod.rs:handle_user_turn` | tool outputs capped at 20 000 chars before being sent to LLM; prevents context overflow on large file/dir reads |
| Empty response detection | `src/session/mod.rs:handle_user_turn` | two-case detection: zero input_tokens → context window exceeded (warns with ctx size + /compact advice); non-zero input_tokens → proxy/gateway dropped body (warns with stop_reason + log path) |
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
| Fix UTF-8 panic on tool output truncation (v0.7.7) | `src/session/mod.rs:862`, `src/tools/web.rs:49` | Panic: "byte index 20000 is not a char boundary" — `—` (em-dash, 3 bytes) straddled the 20000-byte cut point. Both truncation sites now walk back to the nearest valid char boundary with `is_char_boundary` before slicing. |
| TUI text overflow final fix (v0.7.6) | `src/tui/render.rs` | Three missed overflow sources: (1) `text_to_lines` markdown path returned unsplit prose paragraphs — now checks if all markdown lines fit in `wrap_width`; if any line is too wide, extracts plain text and word-wraps it. (2) Added `truncate_spans` helper + global safety-net pass at end of `render_all_lines` that hard-clips every line to `width-2` chars — catches code blocks and any future overflow source. (3) `word_wrap_plain` extracted as shared helper. |
| Thinking word rotation fix (v0.7.5) | `src/tui/app.rs`, `src/tui/render.rs` | Root bug: `spinner_frame` was clamped to `% 10` (spinner glyphs), so `spinner_frame / 40` was always 0 — thinking words never changed within a turn. Added `word_tick: usize` (monotonically increasing, never clamped) incremented in `tick_spinner`; render uses `word_tick / 15` = new word every 240ms. Per-turn prime offset (`turn * 31`) ensures each response starts at a different word. |
| TUI-visible warnings | `src/log.rs:write` | WARN/ERROR from `zap_warn!`/`zap_error!` are forwarded via `TuiEvent::LlmChunk` so they appear in the TUI chat; previously invisible behind the alternate screen |
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

### Skill system
| Feature | File | Notes |
|---|---|---|
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
| `/skill create` | `src/session.rs:cmd_skill` | scaffolds frontmatter template |
| `/skill capture` | `src/session.rs:cmd_skill` | LLM extracts session rules → skill file |
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
| Anthropic base_url | `src/llm_client.rs` | appends `/messages` if base_url ends with `/v1`, else `/v1/messages`; matches Anthropic SDK / Roo Code behaviour |
| OpenAI-compatible | `src/llm_client.rs` | LM Studio, Ollama, Gemini, DeepSeek, Groq, Mistral, xAI, Together, Perplexity, Cohere |
| Rate limit retry | `src/llm_client.rs` | 5s/10s/20s/40s/80s backoff; MAX_RETRIES 5; stdout message with remaining count; clean error on exhaustion (no panic) |
| Provider switching | `src/session.rs:cmd_provider` | interactive picker, saved to `~/.agent.toml` |
| Model switching | `src/session.rs:cmd_model` | `/model <id>` mid-session |
| `/models` list | `src/session.rs:cmd_models` | lists OpenAI-compatible server models |
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
| `list_directory` | ls -la |
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
| CLAUDE.md loading | `src/context_manager.rs:load_claude_md` | walks cwd → $HOME, global `~/.claude/CLAUDE.md` |
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
| Clipboard paste | `src/session.rs:cmd_paste` | pngpaste or AppleScript |
| `/help` | `src/session.rs:cmd_help` | grouped command reference |
| `/config` | `src/session.rs:cmd_config` | provider, model, URL, mode |
| `/cost` | `src/session.rs:cmd_cost` | session token totals + est. $ |
| MCP (eager-loaded) | `src/mcp.rs` + `src/tools/mod.rs` | stdio JSON-RPC 2.0; all servers connect at session startup — tools immediately available without an explicit `mcp_connect` call; servers from `~/.zap/mcp.json` (global) + `.mcp.json` (project); respects `disabled: true`; SSE/HTTP entries (no `command`) are skipped with a warning; `autoApprove`/`disabledTools` fields tolerated — fully compatible with Claude Code / Roo Code shared configs |
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
| `/init` | `src/session.rs:cmd_init` | creates CLAUDE.md + agent fills it in |

### Tests
| Area | File | Count |
|---|---|---|
| Permission modes, session grants, grant-class cross-grants, MCP "always" fallback, ctx newline contract | `src/permission_manager.rs` | 14 |
| MCP command validation: known interpreters, Windows .exe variants, absolute paths, metacharacter/traversal rejection | `src/mcp.rs` | 9 |
| Destructive pattern detection, safe commands, ShellTool permission_context newline contract | `src/tools/shell.rs` | 6 |
| `spawn_agent` char-based truncation regression (byte-slice panic fix) | `src/tools/agent.rs` | 3 |
| `filter_commands` skill completions | `src/tui/commands.rs` | 8 |
| Pre-push hook | `.git/hooks/pre-push` | runs `cargo test` before every push |

---

## Planned 🗓

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

---

## Cut / deferred ✗

| Feature | Why cut |
|---|---|
| Syntax highlighting (syntect) | 4MB+ dep, polish not substance |
| Session replay / export | nice-to-have, not a reason to choose zap |
| `find_definition` as standalone module | `code_map` + ripgrep covers 80% for free |
| "200 token baseline" marketing claim | real baseline with karpathy-guidelines is ~1.8k; update messaging to be accurate |

---

## Baseline token budget (honest numbers)

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
| **Total baseline (no CLAUDE.md, no memory)** | **~1,660 tokens** |
| Per triggered skill (avg) | +400–800 |
