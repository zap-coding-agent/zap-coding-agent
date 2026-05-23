# Medium — Series Overview

**Title:** Introducing ZAP — The Open-Source AI Coding Agent That Doesn't Bloat Your LLM Context

**Subtitle:** A skill-first, Rust-built coding agent with AST-powered code indexing, lazy MCP loading, and zero system prompt waste.

**Suggested Medium tags:** `Rust`, `Artificial Intelligence`, `Software Development`, `Developer Tools`, `Open Source`

**SEO focus keywords:** AI coding agent, open source coding agent, Rust AI agent, LLM context optimization, Tree-sitter code indexing, system prompt bloat, terminal coding agent

---

## Every AI Coding Agent Has a Dirty Secret

Open any popular AI coding agent — Cursor, Copilot, Cline, Aider — and inspect the raw request it sends to the LLM on every turn.

You'll find hundreds, sometimes thousands, of lines of system prompt. Instructions for every possible situation. Tool schemas the agent won't use this session. Filler context added "just in case."

This isn't a small problem. Bloated system prompts are the silent tax on every AI coding agent response:

- **Token waste** — you pay for context the LLM ignores
- **Context dilution** — useful signal drowns in noise
- **Degraded reasoning** — the LLM works harder to find what matters

Most agents accept this as the cost of doing business. I built ZAP because I didn't have to.

---

## Introducing ZAP — A Skill-First AI Coding Agent Built in Rust

**[ZAP on GitHub →](https://github.com/sanjeev23oct/zap)**

ZAP is an open-source, terminal-first AI coding agent written in Rust. It's built around one principle: **send the LLM exactly what it needs for this task, and nothing else.**

That constraint drives every architectural decision in ZAP.

---

## What Makes ZAP Different

### 1. Skill-First Architecture — Progressive Context Injection

Traditional coding agents load a monolithic system prompt at startup and send it on every request regardless of what the task actually requires.

ZAP replaces this with **skills** — discrete, composable units of instruction and tooling that are injected progressively as the agent determines they're needed.

Starting a refactor? ZAP loads refactoring skills. Moving into test generation? Those skills get added. At no point does the LLM receive instructions for capabilities it isn't using.

The result is measurable: smaller context windows, cleaner reasoning, and higher quality output per token. For developers frustrated by the inconsistency of other agents, this matters more than it might sound.

### 2. Code Indexing with AST — Powered by Tree-sitter

Most AI coding agents retrieve context by scanning raw file contents and hoping the right code lands in the window. This is fundamentally a guessing game.

ZAP uses **[Tree-sitter](https://tree-sitter.github.io/tree-sitter/)** to parse your codebase into a structured Abstract Syntax Tree at index time. The agent works with a real understanding of your code's structure — functions, types, call relationships, imports, symbol definitions — not pattern-matched text.

When ZAP needs to find where a function is defined, it queries the index. When it needs to understand what a change might affect, it traverses the AST. Less hallucination. Fewer incorrect edits. Diffs that are actually right.

This is the same technique language servers use. It's overdue in AI coding agents.

### 3. Lazy MCP Loading — Tools Enter Context On Demand

ZAP supports the **Model Context Protocol (MCP)** for extensible tooling, but unlike agents that dump all MCP tool schemas into every request, ZAP loads tools lazily.

MCPs are registered but not injected until the agent determines they're relevant to the current task. If you have a database tool, a browser tool, and a search tool configured, none of them inflate your context while you're editing a React component.

Clean context isn't just about tokens — it's about the agent staying focused.

### 4. Project Initialization — Make Your Codebase AI-Ready in One Command

The first-run experience of most AI coding agents is: read the docs, write a config, hope the agent understands your project.

ZAP ships with `zap init` — a project initialization command that analyzes your repository structure, detects your stack, builds the initial code index, and produces a minimal config that gives the agent an accurate picture of your project from session one.

AI-readiness shouldn't be a manual process. It should be a feature.

### 5. CLI and TUI — Your Terminal, Your Choice

ZAP runs as a **CLI** for scripting, automation, and pipeline integration, and as a full **TUI** for interactive sessions.

The TUI is built with [ratatui](https://ratatui.rs/) — real-time streaming, tool call visibility, token transparency, and context state — without the overhead of a browser or Electron app.

No managed cloud required. Run it against any LLM endpoint: OpenAI, Anthropic, local models via LM Studio or Ollama, or anything with an OpenAI-compatible API.

---

## Why Rust?

Safety, performance, and reliability — not because it's trendy.

A coding agent runs shell commands, reads and writes your files, and manages complex async state across LLM calls and tool executions. Memory safety bugs in that environment aren't just crashes — they're corrupted files and lost work. Rust eliminates the class of bugs that would be most damaging in an agent runtime.

Beyond safety: ZAP starts fast, stays lean on memory, and compiles to a single binary with no runtime dependency. That's the kind of tool you actually reach for.

---

## What This Series Covers

This is the first post in a series breaking down ZAP's architecture in depth — the design decisions, the tradeoffs, and the things that turned out to be harder than expected.

Here's the full roadmap:

1. **Overview** *(you are here)* — the problem, the approach, and the project
2. **Skill injection** — how progressive context loading replaces system prompt bloat
3. **Code indexing with Tree-sitter** — building and querying an AST for real code intelligence
4. **The agent loop** — how ZAP plans, executes, and recovers from errors
5. **Lazy MCP loading** — keeping context clean with on-demand tooling
6. **Project initialization** — making any codebase AI-ready from day one
7. **CLI vs TUI in Rust** — building both interfaces and the tradeoffs involved
8. **Retrospective** — what I'd do differently, and where ZAP goes next

Each post is self-contained. Read in order or jump to whatever's relevant to you.

---

## Who This Is For

- Developers curious how AI coding agents work under the hood — not at the marketing level, but at the architecture level
- Rust practitioners interested in real-world async, TUI, and systems design patterns
- Anyone building AI tooling who wants to see an alternative to the dominant bloat-heavy approach

---

## Start Here

**[github.com/sanjeev23oct/zap](https://github.com/sanjeev23oct/zap)** — star it, open an issue, or just read the code.

---

*Next up: skill injection — why progressive context loading beats system prompt bloat, and exactly how ZAP implements it.*

*Follow to get notified when the next post drops.*
