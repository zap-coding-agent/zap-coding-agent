# ⚡ zap — Skill-first AI coding agent. No prompt bloat, no wasted tokens.

> Open-source AI coding agent in Rust — injects only the context your task needs, never a 4,000-token monolith. Single binary, no runtime.

```
  ╭────────────────────────────────────────────────────────────────────╮
  │                                                                    │
  │   _____     _     ___                                              │
  │    ___/    /_\   | _ \   fast AI coding agent  v0.12.5            │
  │   /       / _ \  |  _/                                            │
  │  /_____  /_/ \_\ |_|                                              │
  │                                                                    │
  ├──────────────────────────────────────┬─────────────────────────────┤
  │  model     claude-sonnet-4-6         │  Tips for getting started   │
  │  backend   Anthropic API             │    Tab  ↑↓  autocomplete    │
  │  mode      ask                       │    /          commands      │
  │                                      │    /provider  switch LLM    │
  │  ~/my-project                        │    /help      all commands  │
  ╰──────────────────────────────────────┴─────────────────────────────╯
```

---

## The Problem Every AI Coding Agent Has

Open any popular AI coding agent and inspect the raw request it sends to the LLM. You'll find hundreds — sometimes thousands — of lines of system prompt sent on **every single turn**, regardless of what you're actually doing.

We measured this. Here's what Gemini CLI and OpenCode send when you ask them to write a Spring Boot service vs. a React component:

| | Gemini CLI | OpenCode | zap |
|---|---|---|---|
| Spring Boot request | **4,096 tokens** | **2,003 tokens** | 1,889 tokens |
| React request | **4,096 tokens** | **2,003 tokens** | 1,661 tokens |
| Prompts identical? | ✅ Yes — same bytes | ✅ Yes — same bytes | ❌ No — different skill injected |
| Java conventions in prompt? | ❌ None | ❌ None | ✅ 650 tokens |
| React conventions in prompt? | ❌ None | ❌ None | ✅ 422 tokens |

**Gemini CLI sends the same 4,096-token prompt for both.** The word "java" does not appear anywhere in its 68,410-character prompt file. Neither does "react", "kotlin", or any other language. ([source](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/prompts/snippets.ts))

**OpenCode uses a single static string constant** — `baseAnthropicCoderPrompt` — sent verbatim on every turn. Zero mentions of Java, TypeScript, Rust, Python, React, or any specific language. ([source](https://github.com/opencode-ai/opencode/blob/main/internal/llm/prompt/coder.go))

zap sends a **different prompt for different tasks** — the Java skill fires for Spring Boot, the React skill fires for components — and a greeting costs 12 tokens, not 2,000–4,000.

> Full methodology, raw token counts, and source links: [docs/why-zap.md](docs/why-zap.md)

---

## What makes zap different

| | |
|---|---|
| **[Skill-first context](docs/skills.md)** | Markdown skills injected only when your message triggers them — Java conventions for Java tasks, React for React, nothing irrelevant ever |
| **[AST code index](docs/code-index.md)** | tree-sitter + SQLite index — the agent looks up what already exists before writing a single line, so it never duplicates files or invents patterns |
| **[Lazy MCP](docs/mcp.md)** | MCP server schemas stay out of context until the model explicitly needs them — zero cost until first use |
| **[Security-first](docs/security.md)** | 25+ secret patterns blocked before any cloud send; full audit trail; `ask`/`auto`/`deny` permission modes; per-edit undo |
| **[/init](docs/configuration.md#init)** | 30 seconds from blank slate to a fully context-aware agent — auto-detects stack, indexes codebase, generates `ZAP.md` |
| **Rust binary** | Single statically-linked binary — no Python venv, no Node.js, no Docker. Cold start in milliseconds, ~20 MB idle |
| **Token display** | Every turn shows exactly what went into context — skills, message, system, estimated $ |

---

## Features at a glance

| | |
|---|---|
| **TUI** | Ratatui terminal UI — streaming output, sidebar with token counts, diff viewer (Ctrl+G), file browser (Ctrl+F), syntax highlighting |
| **Providers** | LM Studio, Ollama, Anthropic, OpenAI, Gemini, DeepSeek, Groq, Mistral, xAI, Together AI, Perplexity, Cohere + any OpenAI-compatible endpoint |
| **Tools** | 15 built-in — read, edit, write, batch-edit, undo, shell, search, glob, code-map, find-def, find-refs, web-fetch, web-search, spawn-agent |
| **Languages** | AST index: Rust, Python, TypeScript, JavaScript, Go, Java |
| **Skills** | 23 built-in; always-on + keyword-triggered; user skills in `~/.zap/skills/` or `.zap/skills/` |
| **Sessions** | Every conversation persisted; `/sessions` fuzzy picker to resume any |
| **Sub-agents** | `spawn_agent` runs parallel sub-agents with their own tool loop |
| **Autonomous loop** | `/goal <condition>` runs turns automatically until done or a turn limit is reached |
| **Extended thinking** | `/think [on\|off\|N]` — Anthropic extended thinking with configurable token budget |
| **Workflows** | Declarative YAML multi-step pipelines in `.zap/workflows/` |
| **Hooks** | `PreToolUse` / `PostToolUse` / `SessionStart` / `SessionEnd` / `UserPromptSubmit` |
| **Remote control** | `/remote` starts a local HTTP server + public tunnel |
| **CI / headless** | `--auto` flag, `--sdk` JSON-lines mode, GitHub Actions + GitLab CI examples |

---

## Install

| Platform | Status |
|---|---|
| macOS ARM (Apple Silicon) | Available |
| Windows x86_64 | Available |
| macOS Intel | Coming soon |
| Linux x86_64 | Coming soon |

### macOS ARM

```bash
# 1. Download from https://github.com/sanjeev23oct/zap/releases/latest
chmod +x zap && mv zap ~/.local/bin/zap

# 2. Add to PATH if needed
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc

# 3. Copy example config
curl -o ~/.agent.toml https://raw.githubusercontent.com/sanjeev23oct/zap/main/agent.toml.example

# 4. Run
zap
```

> **macOS Gatekeeper note:** On macOS 15+ you may see `zsh: killed zap`. Fix: `codesign --sign - ~/.local/bin/zap`

### Build from source

```bash
git clone https://github.com/sanjeev23oct/zap
cd zap
cargo build --release
cp target/release/zap ~/.local/bin/zap
```

Requires [Rust](https://rustup.rs) 1.75+.

---

## Documentation

| Doc | What's in it |
|---|---|
| [Why zap](docs/why-zap.md) | The prompt bloat problem, context quality manifesto, full comparison methodology |
| [Skills](docs/skills.md) | Writing skills, built-in list, commands, multi-tool sources (Kiro, Claude Code) |
| [AST Code Index](docs/code-index.md) | How the index works, SQL examples, blind-write problem, vs Claude Code's approach |
| [MCP Support](docs/mcp.md) | Lazy loading, config format, sample servers, commands |
| [Security](docs/security.md) | Permission modes, secret scanner, audit trail, undo |
| [Configuration](docs/configuration.md) | Install, `~/.agent.toml`, providers, environment variables, `/init` |
| [Commands & Tools](docs/commands.md) | All slash commands and built-in tools |
| [CI / SDK](docs/ci-sdk.md) | Headless mode, GitHub Actions, GitLab CI, JSON-lines SDK |
| [Developer Journeys](docs/journeys.md) | End-to-end walkthroughs: first session, returning, adding features, debugging |
| [Roadmap](docs/roadmap.md) | Planned features and contributing guide |

---

## License

MIT
