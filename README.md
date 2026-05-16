# ⚡ zap

> Fast AI coding agent for your terminal — works with LM Studio, Ollama, OpenAI, and Anthropic Claude.

```
  ⚡ zap  v0.1.0
  Fast AI coding agent
  ─────────────────────────────
  Type /help for commands · Ctrl+D to quit

› _
```

## Features

- **Local-first** — points to your LM Studio or Ollama by default, no cloud account needed
- **Cloud-ready** — switch to OpenAI or Anthropic with one config line
- **Streaming output** — text appears token-by-token on both local and cloud providers
- **Tool use** — reads files, edits code, runs shell commands, searches, git status
- **Parallel tool execution** — multiple tools run concurrently when the model requests them
- **Permission model** — prompts before any write or shell operation (`ask` mode)
- **Slash commands** — `/models`, `/model`, `/config`, `/clear`, `/history`
- **Audit log** — every tool call is appended to `agent_audit.jsonl`
- **Self-contained binary** — single file, no runtime or dependencies required

## Install

### Download (macOS ARM — Apple Silicon)

1. Download `zap` from the [latest release](https://github.com/sanjeev23oct/zap/releases/latest)
2. Make it executable and put it on your PATH:

```bash
chmod +x zap
mv zap /usr/local/bin/zap   # or ~/.local/bin/zap
```

3. Copy the example config to your home directory:

```bash
curl -o ~/.agent.toml \
  https://raw.githubusercontent.com/sanjeev23oct/zap/main/agent.toml.example
```

4. Edit `~/.agent.toml` with your URL / model / key and run:

```bash
zap
```

### Build from source

Requires [Rust](https://rustup.rs) 1.75+.

```bash
git clone https://github.com/sanjeev23oct/zap
cd zap
cargo build --release
cp target/release/zap ~/.local/bin/zap
```

## Configuration

All settings live in `~/.agent.toml`. Environment variables always take precedence.

```toml
# ~/.agent.toml

provider         = "openai"                    # "openai" or "anthropic"
model            = "gemma-4-e4b-it"
base_url         = "http://192.168.1.17:1234"  # omit for cloud
api_key          = ""                          # empty for local
permission_mode  = "ask"                       # ask | auto | deny
```

### Provider examples

| Setup | Config |
|---|---|
| **LM Studio** (local) | `provider="openai"` · `base_url="http://localhost:1234"` · no key |
| **Ollama** (local) | `provider="openai"` · `base_url="http://localhost:11434"` · no key |
| **OpenAI** (cloud) | `provider="openai"` · `api_key="sk-..."` · `model="gpt-4o"` |
| **Anthropic** (cloud) | `provider="anthropic"` · `api_key="sk-ant-..."` · `model="claude-opus-4-7"` |

### Environment variable overrides

```bash
AGENT_PROVIDER=anthropic \
AGENT_API_KEY=sk-ant-... \
AGENT_MODEL=claude-opus-4-7 \
zap
```

## Usage

```bash
zap                            # interactive REPL
zap --goal "add tests for src/lib.rs"   # single-shot
```

### Slash commands

| Command | Description |
|---|---|
| `/help` | Show all commands |
| `/config` | Show active provider, model, and URL |
| `/models` | List all models on your LM Studio / Ollama server |
| `/model <id>` | Switch model mid-session |
| `/clear` | Clear conversation history |
| `/history` | Show turn count |
| `/exit` | Quit |

### Permission modes

| Mode | Behaviour |
|---|---|
| `ask` *(default)* | Prompts before any write or shell operation |
| `auto` | Approves everything — use in trusted environments |
| `deny` | Read-only — never executes writes or shell commands |

## Tools

| Tool | What it does |
|---|---|
| `read_file` | Read a file with optional offset/limit and line numbers |
| `edit_file` | Surgical find-and-replace (rejects ambiguous matches) |
| `write_file` | Write or overwrite a file |
| `shell` | Run a shell command (requires approval in `ask` mode) |
| `git_status` | Show git status and recent commits |
| `search_code` | Grep for a pattern across source files |
| `list_directory` | List files in a directory |

## CLAUDE.md support

Place a `CLAUDE.md` file in your project root (or any parent directory up to `$HOME`) to give zap project-specific context. A global `~/.claude/CLAUDE.md` is also loaded.

## License

MIT
