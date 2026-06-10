# Architecture

This document describes the actual module layout of `zap` as it exists in code. It is derived from the source, not from aspirational design docs.

## High-level flow

```
User input (CLI or TUI)
    │
    ▼
┌─────────────────────────────────────────────┐
│  main.rs / cli / tui                         │
│  Entry points: parse args, load config,      │
│  bootstrap session, enter TUI or run-once.   │
└───────────────┬─────────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────────┐
│  session (Session, turn, tools, summarizer)  │
│  The agent loop. One Session per             │
│  conversation. handle_user_turn drives:      │
│    skill matching → context assembly →       │
│    windowed_history → LLM call →             │
│    tool execution → repeat (up to MAX_TURNS) │
└───────┬──────────────────┬──────────────────┘
        │                  │
        ▼                  ▼
┌───────────────┐  ┌──────────────────────────┐
│  llm_client   │  │  tools                    │
│  Anthropic /   │  │  file (read, write,      │
│  OpenAI /      │  │    edit, batch_edit)     │
│  Google /      │  │  shell (isolated)        │
│  MockClient    │  │  agent (sub-agent spawn) │
│  API auth,     │  │  todo (write/read)       │
│  credentials   │  │  code_map, find_def, etc.│
└───────────────┘  │  mcp (connect + proxy)    │
                   └──────────────────────────┘
```

## Source module map

```
src/
├── main.rs              # CLI entry point, arg parsing, session bootstrap
├── lib.rs               # Re-exports
│
├── config.rs            # Config, Provider enum, PermissionMode, SandboxMode
│
├── llm_client/          # LLM provider abstraction
│   ├── mod.rs           #   LlmProvider trait, Message, ContentBlock, ApiResponse
│   ├── anthropic.rs     #   Anthropic API + prompt caching (ephemeral breakpoints)
│   ├── openai.rs        #   OpenAI-compatible endpoint
│   ├── google.rs        #   Google Gemini
│   ├── auth.rs          #   API key resolution (env, gcloud, keychain)
│   ├── credentials.rs   #   JWT/OAuth credential providers
│   ├── url.rs           #   Endpoint URL construction
│   └── mock.rs          #   MockClient for deterministic tests
│
├── session/             # The agent loop
│   ├── mod.rs           #   Session struct, Session::new, edited_files ledger
│   ├── turn.rs          #   handle_user_turn: the main agent loop
│   ├── tools.rs         #   execute_tool_round: permission + parallel exec
│   ├── history.rs       #   windowed_history, model_context_limit, ctx_bar
│   ├── summarizer.rs    #   Drop summarizer: summarize old turns via LLM
│   ├── casual.rs        #   Casual-message detection (greetings, small talk)
│   ├── preview.rs       #   Smart tool output previews
│   ├── memory_refresh.rs#   Injects /memory facts into system prompt
│   ├── test_factory.rs  #   Session::new_for_test (mock-friendly constructor)
│   ├── commands/        #   Slash-command handlers (stateful, operate on Session)
│   │   ├── code.rs      #     /init, /save, /summarize
│   │   ├── git.rs       #     /branch, /merge, /deploy
│   │   ├── index.rs     #     /index, /think
│   │   ├── info.rs      #     /help, /config, /history, /cost, /audit
│   │   ├── media.rs     #     /paste, /attach
│   │   ├── memory.rs    #     /memory, /mcp
│   │   ├── provider.rs  #     /provider, /models
│   │   ├── session_mgmt.rs#   /clear, /compact, /model, /sessions
│   │   ├── skills.rs    #     /skill (use, unuse, list, show)
│   │   └── tasks.rs     #     /tasks
│   └── agent_loop_tests.rs  # MockClient-powered integration tests
│
├── tools/               # Tool implementations (the Tool trait)
│   ├── mod.rs           #   Tool trait, ToolRegistry, tool_definitions
│   ├── file/            #   read_file, write_file, edit_file, batch_edit, guard_path
│   ├── shell.rs         #   shell tool (sandbox, container, timeout)
│   ├── agent.rs         #   agent (spawn sub-agent, stdin/stdout IPC)
│   ├── todo.rs          #   todo_write, todo_read (per-session task list)
│   └── mcp_connect.rs   #   mcp_connect (dynamic tool registration)
│
├── code_index/          # Source-code indexer (SQLite, AST-aware)
│   ├── mod.rs           #   CodeIndex, global_reindex_file, open_in_memory
│   ├── builder.rs       #   Walk fs, extract symbols, build DB
│   └── query.rs         #   find_definition, find_references, code_map lookups
│
├── skill_manager/       # Skill matching + injection
│   ├── mod.rs           #   match_skills_scoped, rank_and_truncate_skills
│   ├── loader.rs        #   Load skills from src/default_skills/ and ~/.zap/skills/
│   └── trigger.rs       #   Pattern-based trigger engine
│
├── context_manager.rs   # System prompt assembly (Anthropic vs OpenAI variants)
│
├── permission_manager.rs# Permission checking: Auto, Ask, Deny modes
│
├── hooks.rs             # Pre/post tool-use hooks, session lifecycle hooks
│
├── tui/                 # Terminal UI (ratatui)
│   ├── mod.rs           #   TUI event loop
│   ├── channel.rs       #   TuiChannel: async event bus between TUI and session
│   ├── components/      #   Chat view, input, sidebar, context bar
│   └── commands.rs      #   Tab-completion for slash commands
│
├── shell_runner.rs      # Spawn subprocess, capture output, timeout, cancellation
│
├── mcp.rs               # MCP protocol client (connect, list tools, proxy calls)
│
├── secret_scanner.rs    # Secret detection and redaction (before sending to cloud)
│
├── persistence.rs       # SQLite session store (~/.zap/agent.db)
│
├── remote.rs            # Remote agent protocol (spawn + communicate with child zap)
│
├── audit.rs             # Audit log writer
│
├── ui.rs                # Terminal styling, spinners, cost formatting
│
├── project.rs           # Project detection (ZAP.md, Cargo.toml, etc.)
│
├── snapshot.rs          # Session snapshot/restore
│
├── log.rs               # Tracing/logging setup
│
└── default_skills/      # 26 built-in skills (markdown, loaded at startup)
```

## Key design decisions

### Tool trait
Every tool implements `Tool`, which provides `execute`, `tool_definition`, `permission_context`, `affected_path`, and `shows_inline_output`. Tools are registered in `ToolRegistry` and exposed as Anthropic/OpenAI tool definitions.

### Agent loop
`Session::handle_user_turn` drives the core loop:
1. Skill matching (trigger-based, scoped to domain)
2. Context assembly: system prompt + matched skills + edit ledger + drop summary
3. Sliding window via `windowed_history` (default: last 8 user turns)
4. LLM call with tools (up to `MAX_TURNS=20` iterations for tool loops)
5. Tool execution runs in parallel (all approved tools in one `join_all`)

### Edit ledger
`Session::edited_files` is a `HashMap<String, EditedFile>` that records every file written or edited. It is injected into the system prompt on every non-casual turn, so the LLM remembers what files were modified even after the original turns slide out of the sliding window.

### Prompt caching (Anthropic)
The system prompt + tool definitions are marked with `cache_control` breakpoints. On the final turn's last message, the user message also gets a cache breakpoint for reuse on the next request.

### Sandbox modes
The shell tool supports three `SandboxMode` levels: `Off` (no isolation), `Workdir` (sets `current_dir` to project root), `Container` (wraps commands via Docker/Podman with `--network none` and read-only mounts).

### Code index
`CodeIndex` is an SQLite-backed symbol index. It supports `find_definition`, `search_code`, `code_map`, `find_references`, and `find_subtypes`. The index is rebuilt incrementally after file writes and can be opened in-memory for tests.

### Slash commands
All slash commands (e.g., `/branch`, `/compact`, `/skill`) are async methods on `Session` in `src/session/commands/`. They have access to the full session state and can mutate tools, config, or history.

## Testing strategy

- **Unit tests:** Standard `#[cfg(test)] mod tests` in each module.
- **Agent-loop tests:** `src/session/agent_loop_tests.rs` — uses `MockClient` to script LLM responses and verify turn behaviour deterministically.
- **Code-index tests:** `code_map`, `find_definition`, `search_code`, `find_references` tested via `CodeIndex::open_in_memory()`.
- **E2E tests:** `tests/sdk_e2e.rs` — requires an API key, exercises full CLI session.
