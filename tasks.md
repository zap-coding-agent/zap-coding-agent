# Task Plan for Secure Rust AI Coding Agent

## Introduction
This document defines the implementation tasks for the project. Each task is written in a spec-driven style and grouped by phase. Follow this plan sequentially to avoid adhoc development.

## Skills and Roles
- **Product/Requirements**: clarify goals, use cases, constraints.
- **System Architect**: design modular architecture, security boundaries.
- **Rust Developer**: implement core modules, strong typing, async runtime.
- **Security Engineer**: define permissions, sandboxing, audit controls.
- **Prompt Engineer**: design prompt assembly and tool schemas.
- **DevOps/CI**: add packaging checks and release hygiene.

## Phase 1 — Foundation

### Task 1.1 — Confirm Requirements
- Review `requirements.md`.
- Ensure the project purpose, goals, and non-goals are accepted.
- Outcome: requirements document is approved.

### Task 1.2 — Establish Architecture
- Review `design.md`.
- Confirm module breakdown and data flow.
- Map features to Rust modules.
- Outcome: design document is approved.

### Task 1.3 — Create Project Skeleton
- Initialize Rust package.
- Add `src/main.rs` as CLI entrypoint.
- Add placeholder modules:
  - `cli.rs`
  - `agent_core.rs`
  - `llm_client.rs`
  - `tool_registry.rs`
  - `context_manager.rs`
  - `permission_manager.rs`
  - `shell_runner.rs`
  - `audit.rs`
  - `persistence.rs`
  - `config.rs`
- Outcome: compileable skeleton.

## Phase 2 — Minimal Agent MVP

### Task 2.1 — Implement CLI and Config
- Use `clap` to parse a `goal` command.
- Load feature flags and security config.
- Outcome: CLI starts and prints configuration.

### Task 2.2 — Build LLM Client Stub
- Implement `llm_client` abstraction with a stubbed response.
- Support simple JSON tool request parsing.
- Outcome: agent can call the LLM client and receive structured data.

### Task 2.3 — Build Tool Registry
- Implement `Tool` trait.
- Add initial tools: `read_file`, `write_file`, `search_code`, `run_shell`, `git_status`.
- Add metadata descriptions.
- Outcome: registry can resolve tools and execute a stubbed tool call.

### Task 2.4 — Implement Agent Core Loop
- Wire agent loop: prompt → LLM → tool selection → execution → feedback.
- Add a minimal plan stage.
- Outcome: end-to-end flow works for one tool call.

### Task 2.5 — Define Skill Support
- Design a `Skill` abstraction layered over tools.
- Support skill metadata, descriptions, and execution composition.
- Outcome: the architecture can expose higher-level skill capabilities to the LLM.

## Phase 3 — Security and Context

### Task 3.1 — Permission Manager
- Implement permission modes: `Ask`, `Auto`, `Deny`.
- Enforce approvals for dangerous tools.
- Outcome: unsafe actions require explicit confirmation.

### Task 3.2 — Shell Runner and Sandbox
- Add command validation and pattern checks.
- Enforce resource limits and safe environment variables.
- Outcome: shell execution is gated and logs commands.

### Task 3.3 — Context Manager and Prompt Builder
- Load workspace metadata and optional `CLAUDE.md` hints.
- Assemble a system prompt from templates.
- Implement context compaction / summary fallback.
- Outcome: agent prompt includes repository context.

### Task 3.4 — Audit Logging
- Record tool calls, prompts, approvals, and results.
- Persist logs in append-only format.
- Outcome: audit trail exists for all agent activity.

## Phase 4 — Persistence and Memory

### Task 4.1 — Session Persistence
- Implement lightweight storage for sessions and memory.
- Persist summaries and agent facts.
- Outcome: the agent can recall prior session state.

### Task 4.2 — Memory Consolidation
- Add a `dream` / summary process to compress old context.
- Keep summaries under a size threshold.
- Outcome: session history is compacted for long-running use.

## Phase 5 — Advanced Features

### Task 5.1 — Feature Flag System
- Implement runtime feature gating.
- Add flags for `background_mode`, `subagents`, `prompt_cache`, `skill_system`, `mcp_compat`.
- Outcome: experimental features remain disabled by default.

### Task 5.2 — Subagent Orchestration Skeleton
- Add abstractions for `Fork`, `Teammate`, and `Worktree` modes.
- Implement simple delegation flows.
- Outcome: architecture supports multi-step delegation.

### Task 5.3 — Planning and Approval Mode
- Add a multi-stage planning step.
- Show the intended plan to the user before execution.
- Outcome: user can approve or reject plans.

### Task 5.4 — Skill and MCP Support
- Implement a `Skill` registry and dynamic skill resolution.
- Add an `mcp_adapter` to support MCP-style tool invocation schemas.
- Outcome: the agent supports reusable skills and can interoperate with MCP-like interfaces.

## Phase 6 — Corporate Hardening

### Task 6.1 — Packaging and Release Hygiene
- Add CI-friendly checks for release artifacts.
- Prevent debug/source map artifacts from shipping.
- Outcome: safe packaging process.

### Task 6.2 — Enterprise Config and Policy
- Add role-based tool permissions.
- Add configurable corporate security policy.
- Outcome: the agent can be deployed with corporate guardrails.

### Task 6.3 — Documentation and Review
- Finalize spec docs.
- Add a developer onboarding guide.
- Outcome: project is ready for execution and review.

## Phase 7 — Claude Code Parity

Derived from a line-by-line comparison of this implementation against the observable
behaviour of Claude Code (running live). Tasks are ordered: security first, then
correctness, then UX, then performance.

### Task 7.1 — Fix Command Injection in Internal Tools
- `search_code`, `git_status`, and `list_directory` build shell strings via
  `format!("… '{}' …", user_value)` and pass them to `sh -c`.
  A single-quote in the value breaks out of the argument.
- Replace with `shell_runner::run_args(program, &[args…])` that calls
  `tokio::process::Command::new(program).args(…)` — no shell, no injection.
- Keep `run_command(cmd: &str)` for the user-facing `shell` tool only.
- Outcome: internal tools are injection-safe; the shell tool retains its behaviour.

### Task 7.2 — Replace Blocklist with Permission-Based Shell Safety
- The current `BLOCKED_PATTERNS` string-match blocklist is trivially bypassed
  (`rm  -rf /` with double space, trailing slash, etc.).
- Remove the blocklist from `shell_runner`.
- Add a `is_shell_tool(name)` check in `agent_core` so shell/write_file always
  trigger the permission prompt regardless of mode, and add clear wording to the
  system prompt: the model must never issue destructive commands without asking.
- Outcome: safety relies on the permission gate + model instruction, not fragile regex.

### Task 7.3 — Enrich Permission Prompt with Tool Context
- Current prompt: `Allow tool 'shell' to execute? [y/N]` — user cannot make an
  informed decision.
- Add a `context: &str` parameter to `PermissionManager::check()` that renders
  what is about to happen (the command, the file path + byte count, etc.).
- Each tool builds a short human-readable summary and passes it through
  `agent_core` when requesting permission.
- Outcome: user sees exactly what they are approving.

### Task 7.4 — Add Edit Tool (Surgical File Patching)
- Without Edit, modifying an existing file requires regenerating its entire content
  through `write_file`, which is token-expensive and error-prone.
- Implement `edit_file` tool with `path`, `old_string`, `new_string`, `replace_all`.
- Validate: `old_string` must exist; if it appears more than once and `replace_all`
  is false, reject with a clear error telling the model to add more context.
- Outcome: the agent can make surgical edits without rewriting whole files.

### Task 7.5 — Upgrade Read Tool: offset, limit, line Numbers
- Current `read_file` loads the entire file — unusable for large source files.
- Add `offset` (0-based line, default 0) and `limit` (line count, default all).
- Return output formatted as `{line_num}\t{content}` (same as `cat -n`) so the
  model can reference line numbers in subsequent `edit_file` calls.
- Outcome: the agent can navigate large files without exhausting the context window.

### Task 7.6 — Expand System Prompt with Full Behavioural Guidance
- Current system prompt is four sentences; Claude Code's is several thousand tokens.
- Add sections for: tool-usage policy, when to read before writing, never force-push,
  concise response style, security rules, environment context (OS, shell, cwd, model).
- Inject these at runtime so OS/cwd/model values are live, not hard-coded.
- Outcome: the agent behaves consistently and safely without relying on the model's
  training defaults.

### Task 7.7 — Add Interactive REPL Mode
- Make `--goal` optional. When absent, enter a readline loop where each line the user
  types becomes the next user message, continuing the same `messages` history.
- Keep `--goal` for scripting and CI use.
- Outcome: the agent supports multi-turn interactive coding sessions.

### Task 7.8 — SSE Streaming for Anthropic Provider
- Currently the agent blocks until the full API response arrives, then prints it.
  Long responses (30 s+) show nothing until complete.
- Implement SSE streaming: add `stream: true` to the Anthropic request, parse
  `content_block_delta` / `text_delta` events, print text tokens as they arrive.
- Tool-use blocks are still accumulated before execution (cannot partially execute).
- Outcome: text responses stream to the terminal in real time.

### Task 7.9 — Parallel Tool Execution
- When the model returns multiple tool-use blocks, the current loop runs them
  sequentially.  Independent reads/searches can run concurrently.
- Check all permissions sequentially (stdin is single-threaded), then execute
  all approved tools with `futures::future::join_all`.
- Add `futures = "0.3"` to Cargo.toml.
- Outcome: multiple read/search tool calls in one turn run in parallel.

### Task 7.10 — CLAUDE.md Directory Hierarchy Traversal
- Current context manager checks only two hard-coded paths in the current directory.
- Walk from `cwd` up to `$HOME` (or `/`) loading any `CLAUDE.md` found at each
  level; also check `~/.claude/CLAUDE.md` for a global user config.
- Deeper files take precedence (project overrides global).
- Outcome: project, workspace, and user context are all loaded automatically.

## Execution Notes
- Each task should be completed with a small, verifiable deliverable.
- Avoid implementing advanced features before the core secure agent loop is stable.
- Keep all work traceable back to these task definitions.
- After every task: re-read the written code, run `cargo check`, fix any issues
  found before marking complete.
