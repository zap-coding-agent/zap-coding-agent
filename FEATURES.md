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
| Topic-shift detection | `src/session/mod.rs:is_topic_shift` | vocabulary overlap heuristic; suggests `/branch` or `/exit` |
| `/compact` | `src/session/commands.rs:cmd_compact` | summarises history in-place |

### Skill system
| Feature | File | Notes |
|---|---|---|
| Skill loader | `src/skill_manager.rs:load_all_skills` | bundled → global → project, same-name override |
| Always-on skills | `src/skill_manager.rs:always_on_skills` | no `trigger:` field = always injected |
| Triggered skills | `src/skill_manager.rs:match_skills` | keyword match per turn |
| Stack auto-detection | `src/skill_manager.rs:detect_stack_skills` | Cargo.toml/go.mod/package.json/pyproject |
| Skill prompt builder | `src/skill_manager.rs:build_skill_prompt` | for triggered skills per turn |
| Always-on prompt builder | `src/skill_manager.rs:build_always_on_prompt` | baked into base system at session start |
| `source_label()` | `src/skill_manager.rs:source_label` | built-in / global / project display |
| `/skill list` | `src/session.rs:cmd_skill` | grouped: always-on / triggered; source glyph |
| `/skill show` | `src/session.rs:cmd_skill` | description, license, content preview |
| `/skill create` | `src/session.rs:cmd_skill` | scaffolds frontmatter template |
| `/skill capture` | `src/session.rs:cmd_skill` | LLM extracts session rules → skill file |
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
| `shell` | with permission check; description required |
| `git_status` | status + recent log |
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
| Thinking spinner | `src/ui.rs:ThinkingSpinner` | indicatif progress bar while LLM streams |
| Colored diff on edit | `src/ui.rs` | similar crate, red/green |
| Token + cost display | `src/session.rs:handle_user_turn` | per-turn: skills t, msg t, ctx k, est. $ |
| Tab completion | `src/ui.rs:ZapHelper` | slash commands |
| Slash command picker | `src/ui.rs:show_command_picker` | `/` on empty line opens inquire picker |
| Image attach | `src/session.rs:cmd_attach` | staged until next message |
| Clipboard paste | `src/session.rs:cmd_paste` | pngpaste or AppleScript |
| `/help` | `src/session.rs:cmd_help` | grouped command reference |
| `/config` | `src/session.rs:cmd_config` | provider, model, URL, mode |
| `/cost` | `src/session.rs:cmd_cost` | session token totals + est. $ |
| MCP (lazy-loaded) | `src/mcp.rs` + `src/tools/mod.rs` | stdio JSON-RPC 2.0; servers discovered at startup from `~/.zap/mcp.json` (global) + `.mcp.json` (project); processes spawned on first use via `mcp_connect` tool |
| `/mcp` command | `src/session/commands.rs:cmd_mcp` | `list` — shows all servers (global/project, connected/pending); `edit` — opens `~/.zap/mcp.json` in $EDITOR; `edit project` — opens `.mcp.json`; `path` — prints file paths |
| API error URL in message | `src/llm_client.rs` | 404/40x errors include the exact constructed URL for instant diagnosis |
| base_url used as-is | `src/llm_client.rs` | when set, `base_url` is posted to directly — no path appended; gateway handles routing |
| Error log (screen + file) | `src/log.rs` | `zap_warn!`/`zap_error!` print to stdout AND append to `~/.zap/zap.log`; log path shown in `/config` |
| MCP command validation | `src/mcp.rs:validate_mcp_command` | blocks non-absolute paths (allowlist: node/python/npx/deno/…), shell metacharacters, `..` traversal |
| Shell dangerous-command guard | `src/tools/shell.rs:guard_shell` | blocks `rm -rf /~`, fork bomb, `mkfs`, `dd`, `curl\|sh`, `wget\|sh` — applies even in Auto mode |
| `--budget N` token cap | `src/cli.rs`, `src/config.rs`, `src/session/mod.rs` | overrides model context limit for fill-% tracking; warns at 80%, hard-stops at 100% |
| Lazy MCP loader | `src/tools/mod.rs:load_mcp_lazy` | stores configs without spawning; `mcp_connect` synthetic tool in LLM tool list until connected |
| On-demand connect | `src/tools/mod.rs:connect_mcp` | spawn + `initialize` + `tools/list`; rebuilds `tool_defs` so next LLM turn sees real tools |
| MCP manifest in prompt | `src/session/mod.rs:Session::new` | server names+descriptions injected into system prompt; zero tool-schema tokens until connected |
| Server description field | `src/mcp.rs:McpServerConfig` | optional `"description"` in `.mcp.json` shown to LLM before connect |
| `/init` | `src/session.rs:cmd_init` | creates CLAUDE.md + agent fills it in |

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
