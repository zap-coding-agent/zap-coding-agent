# ⚡ zap

> An AI coding agent built in Rust — skill-first context injection, a hard security boundary, and a single binary with no runtime.

```
  ╭────────────────────────────────────────────────────────────────────╮
  │                                                                    │
  │   _____     _     ___                                              │
  │    ___/    /_\   | _ \   fast AI coding agent  v0.3.0             │
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

## What makes zap different

### 1. Skill-First Approach — No Heavy System Prompts

Most AI coding agents front-load a massive system prompt: language conventions, architecture notes, team rules, API patterns, deployment steps — all of it, every request, whether it's relevant or not. You pay for those tokens every turn, and the model's attention gets diluted by context it doesn't need right now.

**zap has replaced most of its own system prompt with skills.** Instead of a wall of static instructions, zap ships built-in skills — `rust`, `python`, `typescript`, `react`, `go`, `git`, `code-review`, etc. — compiled directly into the binary. On startup, zap detects your project stack (from `Cargo.toml`, `go.mod`, `package.json`, etc.) and automatically activates the matching skill. No configuration needed.

Then, on every message, zap checks your input against each skill's trigger keywords. Only the skills that match get injected into that request. Everything else stays out of the prompt.

**Example** — a project with three skills loaded: `rust`, `git`, and a custom `api-conventions`:

| You type | Skills injected | Est. prompt tokens |
|---|---|---|
| `"refactor this async fn to use channels"` | `rust` | ~820 tokens |
| `"commit these changes"` | `git` | ~340 tokens |
| `"add a new REST endpoint"` | `api-conventions` | ~600 tokens |
| `"explain what this function does"` | *(none)* | ~120 tokens |

Without this approach, all three skill documents would pad every prompt — including when you're just asking the model to explain a function.

**User-created skills are fully supported and override built-in ones.** Write a skill once and zap injects it automatically whenever it's relevant:

```markdown
---
name: api-conventions
trigger: ["endpoint", "route", "handler", "REST"]
tokens: 600
---
All endpoints must validate input with ValidateRequest(), return structured
errors as {"error": "...", "code": N}, and use snake_case JSON keys.
```

Place custom skills in `~/.zap/skills/` (global) or `.zap/skills/` (project-local). Project skills take the highest priority, overriding global and built-in skills of the same name. Use `/skill list` to see what's active and `/skill show <name>` to inspect any skill.

---

### 2. AST Code Index — Understands Your Code, Not Just Text

Most agents navigate code the same way a shell script does — grep for a string, hope the result is what you meant. zap builds a real **AST symbol index** at startup using tree-sitter + SQLite, giving the model genuine structural understanding of your codebase.

When you ask zap to "refactor the `UserStore` struct", it doesn't search for the string `"UserStore"` — it looks up the symbol in the index, finds the exact file and line number, reads only that section, and makes a precise edit. No false matches, no reading entire files to find one function.

The index is **incremental** — on subsequent runs, only files that changed since the last session are re-parsed. Cold-indexing a 50k-line repo takes a few seconds; warm starts are near-instant.

**Supported languages:** Rust, Python, TypeScript, JavaScript, Go, Java

**Powered tools:**

| Tool | What it does |
|---|---|
| `code_map` | Structural outline of any file or directory — functions, structs, classes, enums, with line numbers |
| `find_definition` | Jump directly to where a symbol is defined — AST index first, ripgrep fallback |
| `find_references` | Every call site of a symbol across the entire codebase |

The model is instructed to always use `code_map` or `find_definition` before reaching for `read_file` — so it reads only the lines it actually needs, not whole files.

---

### 3. Built in Rust — One Binary, No Runtime

zap is written entirely in Rust and ships as a single statically-linked binary.

- **No Python venv.** No Node.js. No Docker. No dependency hell.
- **Instant startup** — cold start in milliseconds, not seconds.
- **Low memory footprint** — the process sits at ~20 MB idle.
- **Memory-safe by construction** — no buffer overflows, no use-after-free, no data races.
- **Compile once, run anywhere** — drop the binary on your PATH and it works.

```bash
cargo build --release
cp target/release/zap ~/.local/bin/zap
# that's it
```

---

### 4. Security is a First-Class Concern

zap handles your source code, credentials, and shell — so it treats security as a core feature, not an afterthought.

**Secret scanner**
Before any content is sent to a cloud LLM, zap scans it for secrets — API keys, tokens, private keys, passwords, connection strings. Matching content is blocked and you're warned, not silently forwarded.

**Explicit permission model**
Three modes, your choice:

| Mode | What happens |
|---|---|
| `ask` *(default)* | Every write and shell command requires your approval. Type "always" once to trust a specific tool for the session. |
| `auto` | Approves everything — for sandboxed CI or scripts where you control the environment. |
| `deny` | Completely read-only. The agent can read and reason but cannot write files or run commands. |

**Full audit trail**
Every tool call — file reads, edits, shell commands, web fetches — is appended to `agent_audit.jsonl` with a timestamp. You have a complete record of everything the agent did.

**Undo for every edit**
Before modifying any file, zap snapshots the previous content in memory. Mistakes are reversible:

```
/undo src/main.rs      # restore last snapshot
```

The model can also undo its own edits using the `undo_edit` tool.

---

## Features at a glance

| | |
|---|---|
| **Providers** | LM Studio, Ollama, Anthropic, OpenAI, Gemini, DeepSeek, Groq, Mistral, xAI, Together AI, Perplexity, Cohere + any OpenAI-compatible URL |
| **Tools** | 15 built-in — read, edit, write, batch-edit, undo, shell, git, search, glob, code-map, find-def, find-refs, web-fetch, web-search, spawn-agent |
| **Languages** | AST index: Rust, Python, TypeScript, JavaScript, Go, Java |
| **Permission modes** | `ask` (prompt per op), `auto` (approve all), `deny` (read-only) |
| **Context mgmt** | Skill injection, `/compact` in-place summarisation, Anthropic prompt caching |
| **Sessions** | Every conversation persisted; `/sessions` fuzzy picker to resume any |
| **Branching** | `/branch` forks a conversation like a git branch; `/switch` to move between them |
| **Sub-agents** | `spawn_agent` runs parallel sub-agents, each with its own tool loop |
| **MCP** | Standard Model Context Protocol support via `.mcp.json` (stdio JSON-RPC 2.0) |
| **Workflows** | Declarative YAML multi-step pipelines in `.zap/workflows/` |
| **Images** | `/attach <path>` or `/paste` clipboard — multimodal on supported models |
| **Audit log** | Every tool call written to `agent_audit.jsonl` |
| **Secret scanner** | Blocks secrets from being sent to cloud LLMs |

---

## Install

| Platform | Status |
|---|---|
| macOS ARM (Apple Silicon) | Available |
| macOS Intel | Coming soon |
| Linux x86_64 | Coming soon |
| Windows | Coming soon |

### macOS ARM — Apple Silicon

1. Download `zap` from the [latest release](https://github.com/sanjeev23oct/zap/releases/latest)

2. Make it executable and move it onto your PATH:

```bash
chmod +x zap
mv zap ~/.local/bin/zap
```

3. If `~/.local/bin` is not already on your PATH, add it:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc
```

4. Copy the example config:

```bash
curl -o ~/.agent.toml \
  https://raw.githubusercontent.com/sanjeev23oct/zap/main/agent.toml.example
```

5. Run:

```bash
zap
```

> **macOS Gatekeeper note:** On macOS 15+ you may see `zsh: killed zap` on first run.
> Fix: `codesign --sign - ~/.local/bin/zap`

### Build from source

Requires [Rust](https://rustup.rs) 1.75+.

```bash
git clone https://github.com/sanjeev23oct/zap
cd zap
cargo build --release
cp target/release/zap ~/.local/bin/zap
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc
```

---

## Configuration

All settings live in `~/.agent.toml`. Environment variables always take precedence.

```toml
# ~/.agent.toml

provider         = "openai"                 # "openai" or "anthropic"
model            = "gemma-4-e4b-it"
base_url         = "http://localhost:1234"   # omit for cloud providers
api_key          = ""                       # empty for local
permission_mode  = "ask"                    # ask | auto | deny
```

### Provider quick reference

| Setup | Config |
|---|---|
| **LM Studio** (local) | `provider="openai"` · `base_url="http://localhost:1234"` · no key |
| **Ollama** (local) | `provider="openai"` · `base_url="http://localhost:11434"` · no key |
| **Anthropic** | `provider="anthropic"` · `api_key="sk-ant-..."` · `model="claude-sonnet-4-6"` |
| **OpenAI** | `provider="openai"` · `api_key="sk-..."` · `model="gpt-4o"` |
| **Gemini** | `provider="openai"` · `base_url="https://generativelanguage.googleapis.com/v1beta/openai"` · `api_key="..."` |
| **DeepSeek** | `provider="openai"` · `base_url="https://api.deepseek.com"` · `api_key="..."` |
| **Groq** | `provider="openai"` · `base_url="https://api.groq.com/openai"` · `api_key="..."` |

Use `/provider` inside zap to switch interactively with a guided picker — no restart needed.

### Environment variable overrides

```bash
AGENT_PROVIDER=anthropic \
AGENT_API_KEY=sk-ant-... \
AGENT_MODEL=claude-sonnet-4-6 \
zap
```

`ANTHROPIC_API_KEY` and `OPENAI_API_KEY` are also read automatically.

---

## Usage

```bash
zap                                        # interactive REPL
zap --goal "add tests for src/lib.rs"      # single-shot
zap --goal "..." --output-format json      # JSON output (for piping)
```

### Slash commands

| Command | Description |
|---|---|
| `/help` | Show all commands |
| `/config` | Show active provider, model, URL, sub-agent depth |
| `/cost` | Token usage and estimated cost for this session |
| `/history` | Show message count |
| `/clear` | Clear conversation history |
| `/compact` | Model summarises history in-place to free context |
| `/sessions [N]` | Browse and resume old sessions (fuzzy picker) |
| `/model <id>` | Switch model mid-session |
| `/models` | List models on your LM Studio / Ollama server |
| `/provider` | Switch provider interactively |
| `/permissions ask\|auto\|deny` | Change permission mode for this session |
| `/index [path\|stats]` | Reindex AST code symbols |
| `/undo [file]` | Undo the last file edit |
| `/init` | Create a `CLAUDE.md` for this project (auto-filled by the agent) |
| `/run <workflow>` | Run a `.zap/workflows/<name>.yaml` pipeline |
| `/workflow new <name>` | Scaffold a new workflow file |
| `/attach <path>` | Stage an image for the next message |
| `/paste` | Paste an image from the clipboard |
| `/memory list\|get\|set\|del` | Manage persistent key-value memory |
| `/skill list\|show\|create` | Manage skills |
| `/branch <name>` | Fork the current conversation |
| `/branches` | List all conversation branches |
| `/switch <name>` | Switch to a different branch |
| `/audit [N]` | Show last N audit log lines |
| `/exit` | Quit |

---

## Tools

| Tool | What it does |
|---|---|
| `read_file` | Read with optional offset/limit, output prefixed with line numbers |
| `edit_file` | Surgical find-and-replace (rejects ambiguous matches) |
| `batch_edit` | Multiple edits to one file in a single validated call |
| `write_file` | Write or overwrite a file |
| `undo_edit` | Restore a file to its pre-edit snapshot |
| `shell` | Run a shell command (approval required in `ask` mode) |
| `git_status` | Git status + recent log |
| `search_code` | Ripgrep (falls back to grep) with file-type filter and context lines |
| `list_directory` | `ls -la` |
| `glob_read` | List/preview files matching a glob pattern |
| `code_map` | AST-backed structural outline — functions, structs, classes, line numbers |
| `find_definition` | Jump to where a symbol is defined (AST index → ripgrep fallback) |
| `find_references` | All call sites of a symbol across the codebase |
| `web_fetch` | Fetch a URL, strip HTML, return readable text |
| `web_search` | DuckDuckGo search — no API key required |
| `spawn_agent` | Spawn a parallel sub-agent with its own tool loop |

---

## CLAUDE.md support

Place a `CLAUDE.md` in your project root — or any parent directory up to `$HOME` — for persistent project context. A global `~/.claude/CLAUDE.md` is also loaded. All matching files are stacked; innermost directory wins.

Run `/init` to create a template the agent fills in automatically by reading your repo.

---

---

## Skills

Skills inject project-specific instructions into the system prompt only when triggered by keywords in your message. The system prompt stays lean — context is added surgically, not by default.

```markdown
---
name: conventional-commits
trigger: ["commit", "git log", "stage", "push"]
tokens: 800
---
Always use Conventional Commits format: <type>(<scope>): <description>
Types: feat, fix, docs, style, refactor, perf, test, chore
```

Place skill files in:
- `~/.zap/skills/` — global, shared across all projects
- `.zap/skills/` — project-local, highest priority

---

## Workflows

Declarative multi-step pipelines in `.zap/workflows/<name>.yaml`. Run with `/run <name>`.

```yaml
name: ship-feature
description: Review → test → commit → changelog
steps:
  - prompt: "Review all staged changes and flag anything blocking"
    requires_approval: true
  - skill: test-runner
    prompt: "Run the test suite, fix any failures"
  - prompt: "Commit with a conventional commit message"
  - prompt: "Append a one-line entry to CHANGELOG.md"
```

---

## Code Index

An incremental AST symbol index is built at startup using tree-sitter + SQLite. Only files changed since the last run are re-parsed. Supports **Rust, Python, TypeScript, JavaScript, Go, Java**.

Powers `code_map`, `find_definition`, and `find_references`.

Run `/index` to reindex manually or `/index stats` for statistics.

---

## Session Management

Every conversation is persisted locally. Use `/sessions` to browse and resume any previous session with an interactive fuzzy picker.

---

## Sub-agents

When `agent_depth > 0` (default: 3), the model can call `spawn_agent` to delegate independent tasks. Multiple spawns within a single LLM turn run in parallel, each with its own message history and tool access.

---

## Contributing

Contributions are welcome — bug fixes, new providers, language support, skill improvements, or anything that makes zap more useful.

**Reporting bugs**
Open an issue at [github.com/sanjeev23oct/zap/issues](https://github.com/sanjeev23oct/zap/issues). Include your OS, model/provider, the command you ran, and what you expected vs what happened. Attach the relevant lines from `agent_audit.jsonl` if the problem is tool-related.

**Feature requests**
Open an issue with the `enhancement` label. Describe the use case, not just the feature — it helps prioritise.

**Pull requests**
1. Fork the repo and create a branch from `main`
2. Keep changes focused — one PR per fix or feature
3. Run `cargo check` and `cargo clippy` before submitting — zero warnings expected
4. Update the README if you're adding a visible feature

**Adding a built-in skill**
Built-in skills live in `src/default_skills/`. Each is a markdown file with YAML frontmatter (name, trigger keywords, token estimate). If you have good conventions for a language or framework not yet covered, a skill PR is one of the easiest contributions to make.

**Adding a provider**
Provider switching lives in `src/session.rs` (`cmd_provider`). All providers speak the OpenAI wire format — adding one is usually just a new entry in the picker with a `base_url` and default model.

---

## License

MIT
