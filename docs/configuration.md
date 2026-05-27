# Configuration

## Install

| Platform | Status |
|---|---|
| macOS ARM (Apple Silicon) | Available |
| Windows x86_64 | Available |
| macOS Intel | Coming soon |
| Linux x86_64 | Coming soon |

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

### Windows x86_64

1. Download `zap-windows-x86_64.exe` from the [latest release](https://github.com/sanjeev23oct/zap/releases/latest)

2. Rename and move it somewhere on your PATH:

```powershell
Move-Item zap-windows-x86_64.exe C:\Users\You\bin\zap.exe
```

3. Run:

```powershell
zap
```

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

## ~/.agent.toml

All settings live in `~/.agent.toml`. Environment variables always take precedence.

Use `/provider` inside zap to switch interactively — settings are saved automatically per provider, so switching back restores your previous key and model.

```toml
# ~/.agent.toml — managed by zap /provider

provider        = "anthropic"   # active provider slug
permission_mode = "ask"         # ask | auto | deny

# Optional: import skills from other tools or shared libraries.
# Loaded after ~/.zap/skills/ but before .zap/skills/ — higher entry wins on name collision.
skill_paths = [
    ".kiro/skills",       # Amazon Kiro skills
    ".claude/skills",     # Claude Code skills
]

# Optional: always-on context from steering docs, project wikis, etc.
# All .md files in these dirs are appended to the system prompt every turn.
context_paths = [
    ".kiro/steering",
]

[providers.anthropic]
kind     = "anthropic"
model    = "claude-sonnet-4-6"
api_key  = "sk-ant-..."

[providers.lm_studio]
kind     = "openai"
model    = "gemma-4-e4b-it"
base_url = "http://localhost:1234/v1/chat/completions"

[providers.groq]
kind     = "openai"
model    = "llama-3.3-70b-versatile"
base_url = "https://api.groq.com/openai/v1/chat/completions"
api_key  = "gsk_..."
```

Each `[providers.<slug>]` block stores settings independently — switching providers never overwrites another provider's key.

## Environment variable overrides

```bash
AGENT_PROVIDER=anthropic \
AGENT_API_KEY=sk-ant-... \
AGENT_MODEL=claude-sonnet-4-6 \
zap
```

`ANTHROPIC_API_KEY` and `OPENAI_API_KEY` are also read automatically.

---

## /init — Zero to Context-Aware in 30 Seconds

Most agents start every session blind. They don't know your project structure, your build commands, your architecture, or what you worked on last time — unless you tell them. And you tell them again. And again.

`/init` fixes this once, permanently.

```
/init
```

Here's what happens:

1. **Auto-detects your stack** — identifies the language/framework from your repo
2. **Indexes the codebase** — builds the AST symbol index so the agent can navigate structurally from turn one
3. **Creates `ZAP.md`** — asks the LLM to read your source files and fill in: project overview, build/test commands, architecture layout, key files, and a do-not-touch list
4. **Creates `.zap/understanding.md`** — a deeper technical summary: module map, data flows, non-obvious patterns, constraints
5. **Writes `.zap/project.json`** — persisted project config (language, index state)

Total time: ~30 seconds. From that point forward, every session starts informed.

**What `ZAP.md` looks like after `/init`:**

```markdown
## Overview
Order service — handles order lifecycle (create, fulfil, cancel).

## Build & Test
mvn clean install
mvn test
mvn spring-boot:run

## Architecture
- OrderController  → REST handlers (controller/)
- OrderService     → business logic, calls OrderRepository
- OrderRepository  → JPA, Postgres via spring-data

## Important Files
- OrderService.java     — core domain logic, start here
- application.yml       — all config including Kafka brokers

## Do Not Touch
- LegacyOrderMapper.java — deprecated, backwards compat only
```

**Context files update schedule:**

| File | Updated | By |
|---|---|---|
| `ZAP.md` | Once, during `/init` | LLM (reads project, fills template) |
| `.zap/understanding.md` | Once, during `/init` | LLM (deep structural analysis) |
| `.zap/context.md` | Every session end | Auto (goal, files touched, what's next) |
| `.zap/session_log.md` | Every session | Auto (date-indexed history) |
| `.zap/project.json` | `/init` + on index changes | Auto |

## CLAUDE.md support

Place a `CLAUDE.md` in your project root — or any parent directory up to `$HOME` — for persistent project context. A global `~/.claude/CLAUDE.md` is also loaded. All matching files are stacked; innermost directory wins.

Run `/init` to create a template the agent fills in automatically by reading your repo.
