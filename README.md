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

### 1. Skill-First Approach — Context That Earns Its Place

Most AI coding agents front-load a massive system prompt every request — language conventions, architecture notes, team rules, API patterns, all of it, whether it's relevant or not. zap replaces that wall with a **skill system**: markdown files that are injected surgically, only when your message triggers them.

**Two kinds of skills:**

| Kind | When injected | Example |
|---|---|---|
| **Always-on** | Every turn, baked into the base system prompt | `karpathy-guidelines` — Andrej Karpathy's 4 coding principles |
| **Triggered** | Only when your message matches keywords | `rust` fires on "cargo", "fn ", "trait "; `git` fires on "commit", "push" |

**Built-in skills** (compiled into the binary, zero config):

| Skill | Type | Triggers on |
|---|---|---|
| `karpathy-guidelines` | always-on | every turn |
| `rust` | triggered | rust, cargo, crate, async fn, clippy… |
| `python` | triggered | python, pip, pytest, dataclass… |
| `typescript` | triggered | typescript, tsx, interface, npm… |
| `react` | triggered | react, component, jsx, hook, useState… |
| `go` | triggered | go, goroutine, chan, go.mod… |
| `git` | triggered | commit, branch, merge, pull request… |
| `code-review` | triggered | review, pr review, lgtm, critique… |
| `debugging` | triggered | debug, error, crash, panic, stacktrace… |
| `security` | triggered | auth, password, token, jwt, xss, sql injection… |

Stack auto-detection fires the right language skill on startup — Rust project with `Cargo.toml` gets the `rust` skill loaded automatically.

**Example** — a Rust project, custom `api-conventions` skill also loaded:

| You type | Skills injected | Base + skills |
|---|---|---|
| `"refactor this async fn to use channels"` | karpathy + rust | ~2.4k tokens |
| `"commit these changes"` | karpathy + git | ~2.0k tokens |
| `"add a new REST endpoint"` | karpathy + api-conventions | ~2.2k tokens |
| `"explain what this function does"` | karpathy only | ~1.8k tokens |

> **Honest baseline:** the always-on karpathy-guidelines skill and the base system prompt together run ~1.8k tokens — much leaner than Claude Code (~10k) or Gemini CLI (~8k), but not the "200 token" figure you might see in older docs.

**Custom skills override built-in ones** of the same name. Write a skill once and zap injects it exactly when needed:

```markdown
---
name: api-conventions
description: REST endpoint conventions for this project.
trigger: ["endpoint", "route", "handler", "REST"]
tokens: ~400
---
All endpoints must validate input with ValidateRequest(), return structured
errors as {"error": "...", "code": N}, and use snake_case JSON keys.
```

**Always-on skill** (no `trigger:` field — injected every turn):

```markdown
---
name: our-principles
description: Team engineering principles.
---
We ship small, reversible changes. Every PR needs a test. No console.log in prod.
```

**Where to put them:**

| Path | Scope | Priority |
|---|---|---|
| `.zap/skills/` | project — check into git, team-shared | highest |
| `~/.zap/skills/` | personal — all projects | middle |
| binary | built-in defaults | lowest |

On first launch zap writes all built-in skills to `~/.zap/skills/` automatically — open any file there to read or edit it. Same-name files you create override the built-in version on the next run.

```
/skill list              # see all skills with source and always-on/triggered label
/skill show <name>       # preview content + description + license
/skill export <name>     # re-export a built-in to ~/.zap/skills/ (if you deleted it)
/skill export --all      # re-export every built-in skill
/skill create <name>     # scaffold a new skill in .zap/skills/
/skill capture <name>    # extract instructions from this session into a reusable skill
```

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

### 4. Lazy MCP Loading — Zero Token Cost Until You Need It

Most agents with MCP support connect to **every configured server at startup** and dump all their tool schemas into the LLM's context on every turn. Ten MCP servers with five tools each? That's potentially 10,000+ wasted tokens per request — even if you never touch those tools.

zap solves this with **lazy loading**:

| Stage | Other agents | zap |
|---|---|---|
| Startup | Spawns all server processes | Reads `.mcp.json`, stores configs only |
| System prompt | (silent) | Server names + descriptions injected |
| LLM tool list | All tool schemas, always | Just `mcp_connect(server)` until needed |
| First use | Already connected | Spawns process, handshakes, loads tools |
| Next LLM call | — | Real schemas in context, ready to call |

**How it works in practice:**

The LLM is told what MCP servers exist (by name and description) in the system prompt — enough to decide which to use. When it needs one, it calls `mcp_connect("filesystem")`. zap spawns the process, runs the MCP handshake, fetches `tools/list`, registers the tools, and updates the LLM's tool list in the same agentic turn. The very next call in that turn sees the full tool list and can invoke them directly.

Once connected, `mcp_connect` disappears from the tool list for that server — no overhead for already-loaded servers.

**`.mcp.json` with optional descriptions** (backwards-compatible):

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user"],
      "description": "Read and write local files"
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "ghp_..." },
      "description": "GitHub repos, issues, and pull requests"
    }
  }
}
```

The `description` field is shown to the LLM before the server is connected — so it can decide which server is relevant without loading any of them.

---

### 5. Security is a First-Class Concern

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
| **Skills** | 10 built-in; always-on + keyword-triggered; user skills in `~/.zap/skills/` or `.zap/skills/`; SKILL.md standard |
| **Skill capture** | `/skill capture <name>` — extract session rules into a reusable skill file |
| **Context mgmt** | Skill injection, `/compact` in-place summarisation, Anthropic prompt caching |
| **Sessions** | Every conversation persisted; `/sessions` fuzzy picker to resume any |
| **Branching** | `/branch` forks a conversation like a git branch; `/switch` to move between them |
| **Sub-agents** | `spawn_agent` runs parallel sub-agents, each with its own tool loop |
| **MCP (lazy-loaded)** | `.mcp.json` servers connected on demand — zero token cost until first use; `mcp_connect` tool auto-expands tool list mid-turn |
| **Workflows** | Declarative YAML multi-step pipelines in `.zap/workflows/` — versioned with your repo |
| **Images** | `/attach <path>` or `/paste` clipboard — multimodal on supported models |
| **Audit log** | Every tool call written to `agent_audit.jsonl` |
| **Secret scanner** | Blocks API keys, tokens, and passwords from being sent to cloud LLMs |
| **Cost display** | Token breakdown per turn — skills, message, context, estimated $ cost |

---

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

2. Rename and move it somewhere on your PATH, e.g.:

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
zap --auto --goal "..."                    # skip all permission prompts (CI)
zap --sdk                                  # JSON-lines remote control (stdin/stdout)
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

## CI / Headless Mode

zap runs fully non-interactively. Add `--auto` (or `AGENT_PERMISSION_MODE=auto`) to skip all permission prompts:

```bash
# single-shot — clean for scripts
zap --auto --goal "review staged changes and write a summary to REVIEW.md"

# environment variable alternative
AGENT_PERMISSION_MODE=auto zap --goal "run cargo test and fix any failures"
```

### GitLab CI example

```yaml
# .gitlab-ci.yml
ai-review:
  image: ubuntu:24.04
  variables:
    ANTHROPIC_API_KEY: $ANTHROPIC_API_KEY   # set in CI/CD → Variables
  before_script:
    - curl -L https://github.com/sanjeev23oct/zap/releases/download/latest/zap-linux-x86_64
        -o /usr/local/bin/zap && chmod +x /usr/local/bin/zap
  script:
    - zap --auto --goal "review the diff since origin/main, identify bugs or missing tests,
        and write a report to ai-review.md"
  artifacts:
    paths: [ai-review.md]
    expire_in: 1 week
```

### GitHub Actions example

```yaml
- name: AI code review
  env:
    ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
  run: |
    zap --auto --goal "read the changed files, add docstrings where missing, and commit"
```

---

## SDK / Remote Control Mode

`--sdk` turns zap into a **JSON-lines server** — stdin carries prompts, stdout carries responses. It keeps session state across turns, so context accumulates.

```bash
zap --sdk          # stdin → stdout, --auto implied, no banner
```

### Protocol

**stdin** (one JSON object per line):
```json
{"type":"user","text":"refactor the auth module to use JWT"}
{"type":"user","text":"now write tests for the new auth module"}
{"type":"quit"}
```

**stdout** (one JSON object per line):
```json
{"type":"assistant","text":"I've refactored the auth module...","turn":1,"ctx_pct":12,"usage":{"input_tokens":1842,"output_tokens":487}}
{"type":"assistant","text":"I've written tests for...","turn":2,"ctx_pct":24,"usage":{"input_tokens":3210,"output_tokens":612}}
```

All terminal noise (tool call boxes, spinners) goes to **stderr** — stdout is clean JSON for machine consumption.

### Remote control over SSH

```bash
ssh user@dev-server 'ANTHROPIC_API_KEY=sk-ant-... zap --sdk' << 'PROMPTS'
{"type":"user","text":"run cargo test and fix any failures"}
{"type":"quit"}
PROMPTS
```

### Python script example

```python
import subprocess, json

proc = subprocess.Popen(
    ["zap", "--sdk"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    env={**os.environ, "ANTHROPIC_API_KEY": "sk-ant-..."},
)

def ask(prompt: str) -> dict:
    proc.stdin.write(json.dumps({"type": "user", "text": prompt}).encode() + b"\n")
    proc.stdin.flush()
    return json.loads(proc.stdout.readline())

reply = ask("add input validation to src/api.rs")
print(reply["text"])

proc.stdin.write(b'{"type":"quit"}\n')
proc.stdin.flush()
proc.wait()
```

---

## CLAUDE.md support

Place a `CLAUDE.md` in your project root — or any parent directory up to `$HOME` — for persistent project context. A global `~/.claude/CLAUDE.md` is also loaded. All matching files are stacked; innermost directory wins.

Run `/init` to create a template the agent fills in automatically by reading your repo.

---

---

## Skills

Skills are markdown files (`.md`) with YAML frontmatter. They follow the [SKILL.md standard](https://github.com/multica-ai/andrej-karpathy-skills) — compatible with Claude Code, Cursor, and other agents.

**Triggered skill** — injected only when keywords match:

```markdown
---
name: conventional-commits
description: Enforce Conventional Commits format on all git operations.
trigger: ["commit", "git log", "stage", "push"]
tokens: ~400
---
Always use Conventional Commits format: <type>(<scope>): <description>
Types: feat, fix, docs, style, refactor, perf, test, chore
```

**Always-on skill** — no `trigger:` field, injected every session:

```markdown
---
name: team-principles
description: Engineering principles applied to every task.
---
Ship small. Write tests first. No magic numbers. Document the why, not the what.
```

**Where to place skills:**

```
~/.zap/skills/          personal, applies to all projects  ← written here on first launch
.zap/skills/            project-local, check into git for team sharing
```

On first launch zap writes all built-in skills to `~/.zap/skills/`. Open any file there, edit it, and your version takes effect on the next run. Files are never overwritten — only new ones are added when you update zap.

If you delete a file or want to reset a skill to its default, re-export it:

```
/skill export rust              # restore rust.md from the built-in
/skill export --all             # restore every built-in skill
/skill export rust --overwrite  # force-overwrite even if the file exists
```

**All commands:**

```
/skill list                      list all skills (grouped: always-on / triggered)
/skill show <name>               preview content, description, license
/skill export <name>             re-export a built-in to ~/.zap/skills/
/skill export --all              re-export every built-in skill
/skill create <name>             scaffold a new skill in .zap/skills/
/skill create <name> --global    scaffold in ~/.zap/skills/
/skill capture <name>            extract rules from this session into a skill file
/skill capture <name> --global   save captured skill globally
```

Same-name skills override lower-priority ones: `.zap/skills/` > `~/.zap/skills/` > built-in.

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

## Roadmap — Skill Ecosystem

zap's bet is on **skills as a platform**, not on being a better terminal agent. The goal: turn team knowledge into code, make it shareable, composable, and cross-compatible with other agents.

| # | Feature | Status | What it enables |
|---|---|---|---|
| `/skill install github:user/repo/path` | Pull a skill from any GitHub path | planned | One-command community skill install |
| Skill extends / composition | `extends: [rust, code-review]` in frontmatter | planned | Composable skill layers |
| Semantic skill routing | Embedding similarity instead of keyword match | planned | Intent-based matching, no keyword guessing |
| Public skill directory | `zap.sh/skills` — browse, search, install | planned | Discoverable ecosystem |
| Stack auto-detection expansion | Detect more stacks: Ruby, Swift, Kotlin, C++ | planned | Zero-config for more users |
| Cross-agent compatibility | Test skill files against Claude Code, Cursor | planned | Write once, use anywhere |

The skill format is already compatible with Claude Code (`CLAUDE.md`-style) and the [multica-ai SKILL.md standard](https://github.com/multica-ai/andrej-karpathy-skills). Skills you write for zap work in other agents today.

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
