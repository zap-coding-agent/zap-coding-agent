# Design for Secure Rust AI Coding Agent

## Overview
This design defines a modular Rust-based architecture for a secure AI coding agent harness, inspired by the Claude Code leak but implemented cleanly and safely.

## System Architecture

### Modules
- `cli` – terminal interface and command parsing.
- `agent_core` – orchestrates the agent loop, planning, and tool invocation.
- `llm_client` – handles API calls to Claude/Anthropic or compatible LLM providers.
- `tool_registry` – registers tools and manages tool metadata and execution.
- `skill_manager` – manages reusable skill definitions and dynamic tool compositions.
- `mcp_adapter` – handles Model Context Protocol-style tool orchestration and API payloads.
- `context_manager` – builds prompts, manages session history, and compacts context.
- `permission_manager` – evaluates tool permissions and approval workflows.
- `shell_runner` – sandboxed command execution and shell validation.
- `audit` – logs actions, approvals, and results.
- `persistence` – stores session memory, summaries, and feature flag state.
- `config` – loads runtime feature flags, corporate settings, and security policies.

## Data Flow
1. User enters a goal through CLI.
2. `agent_core` constructs the prompt using `context_manager`.
3. `llm_client` sends the prompt and receives streaming tool directives.
4. `agent_core` selects the appropriate tool from `tool_registry`.
5. `permission_manager` checks whether the tool is allowed.
6. If needed, the user is prompted for approval.
7. `tool` executes, possibly through `shell_runner`.
8. Results are logged by `audit` and fed back into the agent loop.
9. `context_manager` updates session state and summary memory.

## Key Components

### Agent Core
- Implements the main agent cycle.
- Supports a planning stage before execution.
- Handles tool call loops, retries, and error handling.
- Maintains session state.

### LLM Client
- Wraps Claude/Anthropic API requests.
- Supports streaming responses and tool-call style JSON.
- Validates responses and detects prompt injection risk.

### Tool Registry
- Defines a `Tool` trait with:
  - metadata
  - input validation
  - permission category
  - execution function
- Starts with core tools:
  - `read_file`
  - `write_file`
  - `search_code`
  - `run_shell`
  - `git_status`
- Provides natural-language descriptions for prompt assembly.

### Skill Manager
- Defines reusable skill wrappers around collections of tools.
- Supports skills as higher-level capabilities such as refactoring, testing, and code generation.
- Allows dynamic skill composition and registration for advanced workflows.

### MCP Support
- Implements a `mcp_adapter` for Model Context Protocol-style tool calls.
- Translates LLM JSON tool directives into internal tool executions.
- Enables compatibility with external MCP-based agents or future prompt schemas.

### Context Manager
- Loads `CLAUDE.md`-style repo hints and user context.
- Reads git status and current workspace state.
- Builds the final system prompt from template components.
- Performs context compaction and summary generation.

### Permission Manager
- Supports modes: `Ask`, `Auto`, `Deny`.
- Enforces per-tool policies.
- Evaluates command risk for shell tools.
- Records approvals and denials.

### Shell Runner
- Executes commands in a restricted environment.
- Validates shell inputs against safe patterns.
- Captures stdout/stderr and applies time/resource limits.
- Can be extended to container or wasm sandbox later.

### Audit Logging
- Records every tool call, prompt, user approval, and result.
- Persists logs to disk in append-only format.
- Supports exporting logs for review.

### Persistence and Memory
- Stores session summaries and agent memory in a lightweight DB.
- Compresses long histories into short recallable summaries.
- Uses a durable persistence layer for repeat sessions.

### Feature Flags
- Runtime toggles for experimental behavior.
- Gated `KAIROS`-style autonomous skeleton only enabled explicitly.
- Designed to hide unfinished features until corporate review.

## Security Design
- Default deny for destructive actions.
- No automatic write/execute without approval.
- Audit trail for every change.
- No prompt injection content in system prompt.
- No hidden or undocumented modes for corporate builds.
- Use Rust safety, explicit types, and boundary checks.

## Technology Stack
- Rust 1.75+ or latest stable.
- Async runtime: `tokio`.
- HTTP client: `reqwest`.
- CLI: `clap`.
- JSON: `serde`, `serde_json`.
- Validation: `schemars` / custom typed inputs.
- Logging: `tracing`.
- Persistence: `sled` or `sqlx` + SQLite.
- Optional sandboxing: `wasmtime` / container integration.

## Project Structure

```text
agent-harness/
  Cargo.toml
  src/
    main.rs
    cli.rs
    agent_core.rs
    llm_client.rs
    tool_registry.rs
    context_manager.rs
    permission_manager.rs
    shell_runner.rs
    audit.rs
    persistence.rs
    config.rs
```

## Implementation Strategy
- Begin with a minimal MVP that proves the tool-call loop and secure shell gating.
- Add context management and credit the prompt builder.
- Add audit and persistence in the second iteration.
- Add skill registry, MCP-compatible tool orchestration, and subagent flag-based advanced behavior later.
- Keep all development aligned with `tasks.md`.
