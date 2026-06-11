# ⚡ Zap Coding Agent

[![Crates.io](https://img.shields.io/crates/v/zap-coding-agent?color=5b3ff8)](https://crates.io/crates/zap-coding-agent)
[![License: MIT](https://img.shields.io/badge/license-MIT-5b3ff8)](LICENSE)
[![GitHub release](https://img.shields.io/github/v/release/zap-coding-agent/zap-coding-agent?color=5b3ff8)](https://github.com/zap-coding-agent/zap-coding-agent/releases/latest)

An AI coding agent built in Rust — skill-first context injection, a hard security boundary, and a single binary with no runtime.

[Website](https://zap.justpush.cloud) · [Docs](https://zap.justpush.cloud/docs.html) · [Architecture](ARCHITECTURE.md)

## Install

### macOS / Linux — one-liner

```bash
curl -fsSL https://raw.githubusercontent.com/zap-coding-agent/zap-coding-agent/main/install.sh | bash
```

The script detects your OS and architecture, downloads the latest release, installs to `~/.local/bin`, and patches your shell config if needed. On macOS it also runs `codesign --sign -` automatically.

| Platform | Binary |
|---|---|
| macOS Apple Silicon (ARM64) | `zap-macos-arm64.tar.gz` |
| macOS Intel (x86_64) | `zap-macos-x86_64.tar.gz` |
| Linux x86_64 | `zap-linux-x86_64.tar.gz` |

### Windows x86_64

Download `zap-windows-x86_64.zip` from the [latest release](https://github.com/zap-coding-agent/zap-coding-agent/releases/latest), extract, and move `zap.exe` somewhere on your PATH:

```powershell
Expand-Archive zap-windows-x86_64.zip .
Move-Item pkg\zap.exe "$env:USERPROFILE\.local\bin\zap.exe"
```

### Build from source

Requires [Rust](https://rustup.rs) 1.75+.

```bash
git clone https://github.com/zap-coding-agent/zap-coding-agent
cd zap-coding-agent
cargo build --release
cp target/release/zap ~/.local/bin/zap
```

## Quickstart

```bash
zap                     # interactive TUI
zap --goal "add tests"  # single-shot (non-interactive)
zap --auto --goal "..." # skip all permission prompts (CI)
zap --sdk               # JSON-lines remote control over stdin/stdout
```

First run will prompt for an API key and model. Use `/provider` to switch later.

## Supported Providers

| Provider | Model examples | Auth |
|---|---|---|
| Anthropic | claude-sonnet-4-6, claude-opus-4-6 | API key |
| OpenAI | gpt-4o, gpt-4-turbo | API key |
| Google Gemini | gemini-2.0-flash, gemini-2.5-pro | API key or gcloud ADC (keyless) |
| LM Studio | gemma-4-e4b-it | None (local) |
| Groq | llama-3.3-70b-versatile | API key |
| OpenRouter | (various) | API key |

## Configuration

All settings live in `~/.agent.toml`. Environment variables take precedence.

Use `/provider` inside zap to switch interactively — settings are saved per provider.

```toml
# ~/.agent.toml — managed by zap /provider

provider        = "anthropic"   # active provider slug
permission_mode = "ask"         # ask | auto | deny

# Optional: import skills from other tools
skill_paths = [".kiro/skills", ".claude/skills"]

# Optional: always-on context from project docs
steering_dirs = ["docs/decisions", ".wiki"]

[providers.anthropic]
kind    = "anthropic"
model   = "claude-sonnet-4-6"
api_key = "sk-ant-..."

[providers.openai]
kind    = "openai"
model   = "gpt-4o"
api_key = "sk-..."

[providers.lm_studio]
kind     = "openai"
model    = "gemma-4-e4b-it"
base_url = "http://localhost:1234/v1/chat/completions"
```

### Environment variable overrides

```bash
AGENT_PROVIDER=anthropic AGENT_API_KEY=sk-ant-... AGENT_MODEL=claude-sonnet-4-6 zap
```

`ANTHROPIC_API_KEY` and `OPENAI_API_KEY` are also read automatically.

### Google Gemini — keyless via gcloud

```bash
gcloud auth login
gcloud auth application-default login
zap
# /provider → select "Google Gemini"
# Auto-detected credentials show a "✓ ready" badge.
```

Or configure manually:

```toml
provider = "gemini"

[providers.gemini]
kind = "openai"
model = "gemini-2.0-flash"
base_url = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
credential_method = "gcloud_adc"
```

Set `GOOGLE_API_KEY` in your environment to use an API key instead.

## Slash Commands

| Command | Description |
|---|---|
| `/help` | List all commands |
| `/provider` | Switch LLM provider and model |
| `/branch` | Create a new conversation branch |
| `/compact` | Summarize old turns to free context |
| `/skill` | List, show, use, or unuse skills |
| `/init` | Analyze the project and save context to ZAP.md |
| `/memory` | View/edit session memory |
| `/tasks` | Show the current task list |
| `/paste` | Paste clipboard content |
| `/clear` | Clear the screen |
| `/exit` | End the session |

## Security

Zap has a hard security boundary:
- **Permission modes:** `ask` (default), `auto`, or `deny` — controls all tool access
- **Secret scanner:** 25+ patterns (API keys, tokens, passwords) detected and redacted before sending to cloud
- **Shell sandbox:** `workdir` mode confines shell to the project root; `container` mode wraps commands via Docker/Podman with `--network none`
- **Audit trail:** Every tool call, permission decision, and LLM request is logged

See [docs/SECURITY.md](docs/SECURITY.md) for the full threat model.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for a module map and design decisions derived from the source code.

## License

MIT
