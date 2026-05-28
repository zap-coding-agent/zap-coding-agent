# ⚡ zap

> An AI coding agent built in Rust — skill-first context injection, a hard security boundary, and a single binary with no runtime.

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

We measured this. Here's what Gemini CLI and OpenCode send when you ask them to write a Spring Boot service vs. a React component — two completely different languages, frameworks, and conventions:

| | Gemini CLI | OpenCode | zap |
|---|---|---|---|
| Spring Boot request | **4,096 tokens** | **2,003 tokens** | 1,889 tokens |
| React request | **4,096 tokens** | **2,003 tokens** | 1,661 tokens |
| Prompts identical? | ✅ Yes — same bytes | ✅ Yes — same bytes | ❌ No — different skill injected |
| Java conventions in prompt? | ❌ None | ❌ None | ✅ 650 tokens |
| React conventions in prompt? | ❌ None | ❌ None | ✅ 422 tokens |

**Gemini CLI sends the same 4,096-token prompt for both.** The word "java" does not appear anywhere in its 68,410-character prompt file. Neither does "react", "kotlin", or any other language. The LLM writing your Spring Boot service and the LLM writing your React component receive identical instructions. ([source](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/prompts/snippets.ts))

**OpenCode uses a single static string constant** — `baseAnthropicCoderPrompt` — sent verbatim on every turn, every task type. Zero mentions of Java, TypeScript, Rust, Python, React, or any specific language. ([source](https://github.com/opencode-ai/opencode/blob/main/internal/llm/prompt/coder.go))

This isn't just waste. It's why these agents give inconsistent output — the model has no language-specific guidance, so it invents its own conventions turn by turn.

zap sends a **different prompt for different tasks** — the Java skill fires for Spring Boot, the React skill fires for components — and a greeting costs 12 tokens, not 2,000–4,000.

> Full methodology, raw token counts, and source links: [`content/evidence/system-prompt-comparison.md`](content/evidence/system-prompt-comparison.md)
> Medium series: [Introducing ZAP — The Open-Source AI Coding Agent That Doesn't Bloat Your LLM Context](content/overview/medium.md)

---

## Context Quality is Supreme

Every design decision in zap follows one principle: **every token in the LLM's context window must earn its place.** Context that doesn't improve output quality is waste — it dilutes attention, burns budget, and produces inconsistent results.

This principle drives everything:

| Mechanism | What it ensures |
|---|---|
| **Skill injection** | Only language- and task-specific guidance is sent, not a one-size-fits-all monolith |
| **AST code index** | The agent knows what exists *before* it decides what to create — no blind writes |
| **`/init` command** | Auto-generates project-specific context files so every session starts informed |
| **Context files** | `ZAP.md`, `.zap/understanding.md`, `.zap/context.md`, `.zap/session_log.md` — maintained automatically, loaded on demand |
| **Casual-turn detection** | Greetings cost ~31 tokens, not 2,000–4,000 |
| **Lazy MCP** | Server tool schemas stay out of context until the model explicitly connects them |
| **Token-cost display** | Every turn shows exactly what went into context — skills, message, system, and estimated $ |

### What gets updated and when

zap maintains four project context files, each updated at a specific time to keep the agent current without polluting the context window:

| File | Updated when | What it contains | Loaded when |
|---|---|---|---|
| `ZAP.md` | `/init` (manual, once) | Project overview, build commands, architecture, do-not-touch list | Every session (on-demand) |
| `.zap/understanding.md` | `/init` (manual, once) | Deep technical map: modules, data flows, patterns, constraints | Every session (on-demand) |
| `.zap/context.md` | End of every session (auto) | Last session: goal, files touched, what's next | Session start (on-demand) |
| `.zap/session_log.md` | Every session (auto) | History of all past sessions, indexed by date | On request (on-demand) |

These files are never pre-loaded into the context window — the model reads them *only when relevant*, using `read_file`. This means a session about fixing a typo doesn't pay the cost of loading the entire project architecture.

### The result

- **First session:** `/init` analyses your repo and bootstraps all project knowledge in ~30 seconds
- **Returning sessions:** The agent already knows what you were working on, which files changed, and what was left unfinished
- **Every turn:** Skills inject only what's relevant, the index tells the agent what exists, and casual messages skip the overhead entirely

---

<!-- generate with: cd demos/code_indexing && VHS_NO_SANDBOX=1 vhs demo.tape -->
![code indexing demo](demos/code_indexing/demo.gif)

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

#### The problem: agents that write without looking

Ask most coding agents to "add all the API layers for user management" in an existing project and you'll see a predictable set of mistakes:

- **Duplicate files created** — `src/user_repository.rs` already exists, but the agent creates `src/repositories/user_repo.rs` alongside it because it never checked
- **Existing patterns ignored** — the project uses a `Repository<T>` trait with a specific error type; the agent invents its own DB access style from scratch
- **Scaffolding over existing code** — `src/routes/`, `src/models/`, `src/db/` already exist with boilerplate; the agent recreates them
- **Missed abstractions** — a `BaseRepository` or shared `AppError` type already exists; the agent writes a duplicate

These aren't model failures — they're context failures. The agent is writing blind because its context window never contained the files it needed to check.

#### How the index fixes it

When you ask zap the same question, before writing a single line it queries the index:

```sql
-- Does a user repository already exist?
SELECT path, line, kind FROM symbols WHERE name LIKE '%UserRepo%' OR name LIKE '%UserStore%';

-- What repository pattern does this project use?
SELECT name, path, line, signature FROM symbols WHERE kind = 'trait' AND name LIKE '%Repository%';

-- What's already in the db/ directory?
SELECT name, kind, line FROM symbols WHERE path LIKE '%/db/%' ORDER BY path, line;
```

This runs in milliseconds against the local SQLite index — no file reads, no grep, no context stuffing. The model knows what exists before it decides what to create. It adds to `src/user_repository.rs` instead of creating a new one. It implements the existing `Repository<T>` trait instead of inventing a new pattern.

When you ask zap to "refactor the `UserStore` struct", it doesn't search for the string `"UserStore"` — it looks up the symbol in the index, finds the exact file and line number, reads only that section, and makes a precise edit. No false matches, no reading entire files to find one function.

The index is **incremental** — on subsequent runs, only files that changed since the last session are re-parsed. A background indexer runs every 120s during interactive sessions so the index stays fresh as you edit. Cold-indexing a 50k-line repo takes a few seconds; warm starts are near-instant.

**Always current during edits** — every time zap writes a file, it immediately reindexes that file before the next LLM turn. The model never queries a stale index for files it just changed.

**Index usage is logged per turn** — every time a tool call is answered by the index (rather than falling back to grep), zap logs it to `~/.zap/zap.log` and `~/.zap/audit.jsonl`:

```
[INDEX] hit · find_definition · 'UserRepository' · 3 result(s)
[INDEX] hit · code_map · 'src/db/' · 42 symbol(s)
[INDEX] miss · find_definition · 'legacy_fn' · grep fallback
```

This makes it auditable — you can see exactly when the index was used vs. when the agent had to fall back to text search.

**Supported languages:** Rust, Python, TypeScript, JavaScript, Go, Java

**Powered tools:**

| Tool | What it does |
|---|---|
| `code_map` | Structural outline of any file or directory — functions, structs, classes, enums, with line numbers |
| `find_definition` | Jump directly to where a symbol is defined — AST index first, ripgrep fallback |
| `find_references` | Every call site of a symbol across the entire codebase |

The model is instructed to always use `code_map` or `find_definition` before reaching for `read_file` — so it reads only the lines it actually needs, not whole files.

**How the index powers every LLM turn:**

```
You: "refactor the UserStore struct"

  zap (tool call)  →  find_definition("UserStore")
  SQLite index     →  src/db/user_store.rs:42  ← instant, no file scan
  zap (tool call)  →  read_file("src/db/user_store.rs", offset=40, limit=60)
  zap (tool call)  →  edit_file(...)            ← precise edit, right lines

Without index: grep entire repo → read 3 wrong files → hallucinate location
With index:    SQLite lookup → read 20 lines → done
```

Every index hit is logged so you can see exactly when the index was used vs. when the agent fell back to search:

```
[INDEX] hit  · find_definition · 'UserStore'    · 1 result
[INDEX] hit  · code_map        · 'src/db/'      · 38 symbols
[INDEX] miss · find_definition · 'legacy_fn'    · grep fallback
```

**Code quality report** — the same SQLite index powers `/index quality`, a human-readable health report run directly in the TUI:

```
Code Health  ·  27 files  ·  1043 symbols  ·  ⚡ 74/100
────────────────────────────────────────────────────────────

File sizes  (lines)
────────────────────────────────────────────────────────────
  ⚠ 2382  src/session/commands.rs    ████████████████████  37 sym
  ⚠ 2266  src/tui/render.rs          ████████████████████  48 sym
  ⚠ 1789  src/session/mod.rs         █████████████         45 sym
  ⚡ 1177  src/tui/mod.rs             ████████
  ·   527  src/tui/app.rs             ███
  ·   312  src/code_index.rs          ██

  ⚠ >1000 lines   ⚡ 500–1000   · healthy

God objects  (>15 methods — split candidates)
────────────────────────────────────────────────────────────
  Session                        45 methods  (mod.rs)
  ToolRegistry                   18 methods  (tool_registry.rs)

Dead code candidates  (pub fn, ≤1 reference)
────────────────────────────────────────────────────────────
  export_skill                   (skill_manager.rs:599)
```

Line counts are read from disk; symbol counts and coupling metrics come from SQLite. Reference counts are computed in one O(source_size) pass at the end of every `/index` run — no call graph required.

#### Why zap indexes when Claude Code deliberately doesn't

Claude Code (Anthropic's own CLI) has **no built-in code indexing**. No tree-sitter, no SQLite, no ctags. It uses pure agentic search — grep + glob + read, chosen at runtime by the model. This was a deliberate, tested decision.

Boris Cherny (Claude Code's creator) confirmed publicly that Anthropic built and benchmarked a RAG/vector-index approach early on and dropped it because agentic search won "by a lot." The reasons:

- Grep finds exact matches; embeddings introduce false positives
- No index to build or maintain
- Index drift — code changes constantly during editing sessions
- Simpler architecture with fewer failure modes

> Sources: [Claude Code Doesn't Index Your Codebase — vadim.blog](https://vadim.blog/claude-code-no-indexing) · [Building Claude Code with Boris Cherny — Pragmatic Engineer](https://newsletter.pragmaticengineer.com/p/building-claude-code-with-boris-cherny) · [Official Claude Code docs](https://docs.anthropic.com/en/docs/claude-code/overview)

The community has noticed the gap — multiple open-source MCP servers exist to bolt indexing onto Claude Code:
- [colbymchenry/codegraph](https://github.com/colbymchenry/codegraph) — tree-sitter + SQLite FTS5
- [cocoindex-io/cocoindex-code](https://github.com/cocoindex-io/cocoindex-code) — AST-based search
- [zilliztech/claude-context](https://github.com/zilliztech/claude-context) — vector search MCP

And open feature requests asking Anthropic to add this natively: [#4556](https://github.com/anthropics/claude-code/issues/4556) · [#9277](https://github.com/anthropics/claude-code/issues/9277)

**zap makes the opposite bet.** Agentic search solves semantic questions well ("find code related to payment processing"). A persistent AST index solves structural questions better — "what already exists in this module?", "which files implement this pattern?", "is there already a `UserRepository`?" These are exactly the questions that matter when an agent is about to *write* new code. Without an index, the agent can only search for what it knows to look for. With an index, it knows what exists before it decides what to create.

The two approaches solve different failure modes. Agentic search avoids index drift. AST indexing avoids blind writes into a codebase the agent hasn't fully read.

#### Does the index trade quality for speed?

No — it trades one search strategy for a better one, with a full fallback for the cases the index doesn't own.

**The concern usually raised:** "agentic search won by a lot" — Boris Cherny said this when Anthropic dropped their RAG/vector approach early on. That's true, and the reasoning is sound: vector embeddings introduce false positives (semantically similar but wrong matches), require a build step, and drift as the codebase changes. But zap doesn't use vectors or RAG. It uses an **AST symbol index** — fundamentally different. A symbol lookup either returns the exact definition or it doesn't. No hallucinated matches, no similarity threshold to tune, no embedding model to maintain.

**The index owns structural questions — where agentic search pays full price:**

Every time Claude Code answers "where is `UserRepository` defined?", the model spends tokens deciding which files to read, issues multiple grep calls, reads partial file content, and synthesizes the answer. The index answers the same question in a single SQLite lookup. On a 50k-line repo, that's the difference between 1 tool call and 5.

**Grep handles what the index doesn't:**

The index knows symbol names, kinds, and locations. It doesn't do open-ended semantic search — "find everything related to the checkout flow" is a grep question, not a symbol question. zap's model uses `search_code` (ripgrep) for those. The index and grep are complementary layers, not competing ones. Every `find_definition` miss automatically falls back to ripgrep — so zap never does worse than pure agentic search, only better when a symbol is indexed.

**The design boundary:**

The index is scoped to the questions that matter most when writing code: "does this already exist?", "what pattern do existing implementations follow?", "where is this defined?". These are the questions that produce duplicate files, invented patterns, and blind writes when unanswered. Open-ended exploration ("show me everything related to payments") is rarer and handled by grep. Type resolution across module boundaries is an LSP problem — and a deliberate non-goal for a terminal agent that ships as a single binary.

| Question | zap | Pure agentic search |
|---|---|---|
| "Does `UserRepository` already exist?" | instant SQL lookup | grep scan + file reads |
| "What pattern do existing repos follow?" | schema query across all types | read multiple files |
| "Find all code related to the checkout flow" | ripgrep (same as agentic) | ripgrep |
| Large codebase, cold start | index pre-built, no file reads | cold grep every turn |

The index doesn't reduce quality — it reduces the number of file reads required to answer structural questions, which directly reduces token cost and the chance of the model writing over something that already exists.

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

### 4. MCP Support — Lazy-Loaded, Cross-Agent Compatible

MCP (Model Context Protocol) is an open standard. **Any MCP server you configure in zap also works in Claude Code, Cursor, Kiro, and other agents** — the config format is shared. zap adds two optional fields (`description`, `toolsHint`) that other agents silently ignore, so your config file is fully portable.

#### Config file locations

| File | Scope |
|---|---|
| `~/.zap/mcp.json` | Global — applies to every session |
| `.mcp.json` (project root) | Project-local — checked into git, takes precedence |

#### How lazy loading works

Most agents connect to every configured server at startup and dump all tool schemas into the LLM's context on every turn. Ten servers × five tools each = 10 000+ wasted tokens per request, whether you use them or not.

zap keeps every server in a **pending** state at startup. Instead of their tool schemas, the LLM gets one lightweight stub:

```
mcp_connect(server)
  - filesystem: Read/write files in /tmp and the project  [tools: read_file, write_file, list_directory…]
  - fetch: Fetch web pages as markdown                    [tools: fetch]
  - memory: Persistent knowledge graph                    [tools: create_entities, search_nodes…]
```

When the LLM decides it needs a server, it calls `mcp_connect("filesystem")`. zap spawns the process, runs the handshake, fetches the real `tools/list`, and registers those tools — all within the same agentic turn. The very next LLM call sees the full tool schema and can invoke the tools directly. Once a server is connected, `mcp_connect` no longer appears for it.

| Stage | Other agents | zap |
|---|---|---|
| Startup | Spawns all server processes | Reads config only — zero processes |
| LLM tool list per turn | All tool schemas, always | One `mcp_connect` stub until needed |
| First use of a server | Already connected | Spawns process on demand, ~200 ms |
| After first use | — | Real schemas in context, `mcp_connect` gone |

#### Sample `~/.zap/mcp.json`

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp", "/home/user/project"],
      "description": "Read and write files inside /tmp and the project directory",
      "toolsHint": "read_file, write_file, edit_file, list_directory, search_files"
    },
    "fetch": {
      "command": "uvx",
      "args": ["mcp-server-fetch"],
      "description": "Fetch any URL and return it as clean markdown",
      "toolsHint": "fetch"
    },
    "memory": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-memory"],
      "description": "Persistent knowledge graph — entities and relations survive across sessions",
      "toolsHint": "create_entities, create_relations, search_nodes, read_graph"
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "ghp_your_token_here" },
      "description": "GitHub repos, issues, pull requests, and code search",
      "toolsHint": "create_issue, create_pull_request, search_code, list_commits"
    }
  }
}
```

#### Fields

| Field | Required | Portable | Description |
|---|---|---|---|
| `command` | yes | yes | Executable to spawn (`npx`, `uvx`, `node`, absolute path) |
| `args` | yes | yes | Arguments passed to the command |
| `env` | no | yes | Extra environment variables (API keys, tokens) |
| `description` | no | yes* | What this server does — shown in `mcp_connect` stub so the LLM knows when to connect it |
| `toolsHint` | no | zap-only | Comma-separated key tool names — lets the LLM plan without connecting |
| `disabled` | no | yes | Set `true` to skip this server entirely |
| `autoApprove` | no | yes | Tool names to auto-approve (Claude Code convention, parsed but not yet enforced) |

\* `description` is a zap extension. Other agents that don't know the field simply ignore it.

#### MCP commands

```
/mcp list              list all servers — connected, pending, or failed
/mcp edit              open ~/.zap/mcp.json in $EDITOR
/mcp edit project      open .mcp.json (project-level config)
/mcp path              print both config file paths
```

#### Installing public MCP servers

The two most useful zero-config servers:

```bash
# filesystem — read/write local files (requires Node)
# add to mcp.json: "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/your/allowed/path"]

# fetch — fetch any URL as markdown (requires Python + uv)
# add to mcp.json: "command": "uvx", "args": ["mcp-server-fetch"]
```

Both install automatically on first connect via `npx -y` / `uvx` — no manual `npm install` needed.

---

### 5. Security is a First-Class Concern

zap handles your source code, credentials, and shell — so it treats security as a core feature, not an afterthought.

#### The agent cannot execute anything destructive without your explicit approval

In the default `ask` mode, every write operation and shell command is blocked until you approve it. Read-only tools — `read_file`, `search_code`, `code_map`, `find_definition`, `git_status` — are never gated and run freely. Only the tools that can cause damage require your sign-off:

| Tool class | Ask mode | Auto mode | Deny mode |
|---|---|---|---|
| `read_file`, `search_code`, `code_map`, `git_status` | ✓ always allowed | ✓ | ✗ blocked |
| `edit_file`, `write_file`, `batch_edit` | prompt | ✓ | ✗ |
| `shell` | prompt | ✓ | ✗ |
| `spawn_agent` | prompt | ✓ | ✗ |

When the model wants to run multiple tools in one turn, zap shows **one grouped prompt** covering all of them — you approve or deny the batch, not each individually.

**"Always" grants** — type `always` once at a prompt and that tool class is auto-approved for the rest of the session. Granting `edit_file` also grants `write_file` and `batch_edit` — semantically identical operations share a grant class so you're not re-prompted for the same action with a different tool name.

**Three modes, your choice:**

| Mode | When to use |
|---|---|
| `ask` *(default)* | Any interactive session — you stay in control |
| `auto` | Sandboxed CI, scripts, or headless runs where you control the environment |
| `deny` | Completely read-only — the agent can read and reason but cannot write a single byte or run any command |

Switch at any time: `/permissions ask`, `/permissions auto`, `/permissions deny`.

#### Secret scanner — 25+ patterns, blocks before sending

Before any content is sent to a cloud LLM, zap scans it for secrets. It checks for:

- **API keys**: Anthropic (`sk-ant-`), OpenAI (`sk-proj-`), Stripe live and test keys
- **VCS tokens**: GitHub personal access tokens (`ghp_`, `ghs_`, `github_pat_`), GitLab tokens (`glpat-`)
- **Cloud credentials**: AWS access keys (`AKIA`), AWS secret key fields, GCP service account JSON
- **Cryptographic material**: PEM private key blocks (`-----BEGIN`), JWT tokens (base64 header prefix)
- **Generic credential fields**: `password=`, `api_key=`, `secret=`, `access_token=` in config files

Matches are blocked and you're warned with the line number and a redacted preview — content is never silently forwarded.

#### Full audit trail

Every tool call is appended to `~/.zap/audit.jsonl` as a structured JSON record with a timestamp, tool name, and outcome. You have a complete, machine-readable record of everything the agent did — useful for debugging, compliance, or just reviewing what changed.

```bash
/audit 20       # show last 20 audit entries in the TUI
```

#### Undo for every edit

Before modifying any file, zap snapshots the previous content in memory. If the agent makes a wrong edit, you can restore it instantly:

```
/undo src/main.rs      # restore file to its pre-edit state
```

The model can also undo its own edits via the `undo_edit` tool — useful in autonomous `/goal` runs where the agent detects it made a mistake mid-task.

---

### 6. `/init` — Zero to Context-Aware in 30 Seconds

Most agents start every session blind. They don't know your project structure, your build commands, your architecture, or what you worked on last time — unless you tell them. And you tell them again. And again.

`/init` fixes this once, permanently.

```
/init
```

Here's what happens:

1. **Auto-detects your stack** — identifies the language/framework from your repo (the same mechanism that fires the right skill at startup)
2. **Indexes the codebase** — builds the AST symbol index so the agent can navigate structurally from turn one
3. **Creates `ZAP.md`** — asks the LLM to read your source files and fill in: project overview, build/test commands, architecture layout, key files, and a do-not-touch list
4. **Creates `.zap/understanding.md`** — a deeper technical summary: module map, data flows, non-obvious patterns, constraints
5. **Writes `.zap/project.json`** — persisted project config (language, index state)

Total time: ~30 seconds. From that point forward, every session starts informed — the agent knows your project.

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

**When context files get updated:**

| File | Updated | By |
|---|---|---|
| `ZAP.md` | Once, during `/init` | LLM (reads project, fills template) |
| `.zap/understanding.md` | Once, during `/init` | LLM (deep structural analysis) |
| `.zap/context.md` | Every session end | Auto (goal, files touched, what's next) |
| `.zap/session_log.md` | Every session | Auto (date-indexed history) |
| `.zap/project.json` | `/init` + on index changes | Auto |

The key insight: `/init` is not just a template generator — it's the bridge between "the agent knows nothing about your project" and "the agent starts every session with full structural knowledge." Combined with the skill system and AST index, it means context quality is built in from the very first turn.

---

### 7. Automated Session Continuity — Never Lose Your Thread

Claude Code and most other agents have no persistent memory of what you worked on last time. Every session you start over, re-explaining the goal, pasting in the error you were debugging, and reloading context you already established.

zap automates the handoff between sessions completely — you pick up exactly where you left off, without doing anything.

#### What happens at session end

When you close zap (`/exit`, Ctrl+C, or closing the terminal), it automatically:

1. **Summarizes what's next** — makes a small LLM call (~500 tokens, 20s timeout) over the last 10 messages and generates 1-3 bullet points describing the concrete next steps: file names, function names, features still in progress.
2. **Writes `.zap/context.md`** — a structured handoff file: goal, files touched, and the LLM-generated "What's next" summary.
3. **Appends `.zap/session_log.md`** — a dated history of every session: goal + files changed.

```
# Session Context

## Last updated
2026-05-27 14:32 — Session #42

## What was being worked on
Refactoring session/mod.rs into submodules

## Files touched
  - src/session/commands/code.rs
  - src/session/turn.rs
  - src/context_manager.rs

## What's next
- Add `summarize_whats_next` to session/commands/code.rs (async, 20s timeout)
- Update tui/mod.rs line 110 and agent_core.rs line 209 to call `.await`
- Bump version to 0.13.38 and update FEATURES.md before commit
```

#### What happens at session start

When you open zap again:

- The "What was being worked on" line appears in the **startup banner** — you see it before you type anything
- The full `context.md` is injected into the **system prompt** as `## Last Session Handoff` — the agent already knows the context before your first message
- In CLI mode, zap asks "Resume from last session?" — one keypress to restore context

```
  ◌ Last: Refactoring session/mod.rs into submodules
  ◌ Files: src/session/commands/code.rs, src/session/turn.rs
```

#### Better than Claude Code's approach

Claude Code has no automated session handoff. It relies on you to maintain a `CLAUDE.md` or re-paste context manually. zap does it automatically — the LLM generates the "What's next" summary, so it captures intent and in-progress state that a simple file-diff or commit log can't.

#### How it compares to Claude's memory system

| | Claude (claude.ai) | zap |
|---|---|---|
| Per-conversation | ✗ — each chat is isolated | ✓ context.md injected every session |
| Cross-project | ✓ global user memory | `/memory set` — persisted key-value store, injected globally |
| Auto-generated | ✓ Claude writes memories | ✓ LLM summarizes "What's next" at exit |
| File-visible | ✗ opaque | ✓ `.zap/context.md`, readable and editable |

**Agent memory** (`/memory set key value`) is the cross-project equivalent — facts saved here (API patterns, team conventions, preferred approaches) are injected into every session, across every project.

```
/memory set error-style  always use anyhow::Context for wrapping errors
/memory set test-db      never mock the database — always use a test container
/memory list             show all saved facts
/memory del error-style  remove a fact
```

---

## Features at a glance

| | |
|---|---|
| **TUI** | Ratatui terminal UI — streaming output, sidebar with token counts, diff viewer (Ctrl+G), file browser (Ctrl+F), syntax highlighting |
| **Providers** | LM Studio, Ollama, Anthropic, OpenAI, Gemini, DeepSeek, Groq, Mistral, xAI, Together AI, Perplexity, Cohere + any OpenAI-compatible endpoint; per-provider settings persisted in `~/.agent.toml` |
| **Tools** | 15 built-in — read, edit, write, batch-edit, undo, shell, search, glob, code-map, find-def, find-refs, web-fetch, web-search, spawn-agent |
| **Languages** | AST index: Rust, Python, TypeScript, JavaScript, Go, Java |
| **Code quality** | `/index quality` — god objects, coupling, dead code candidates, quality score (0–100); reference counts computed in one O(source-size) pass |
| **Index usage logging** | Every tool call answered by the AST index is logged to `~/.zap/zap.log` and `audit.jsonl` — hit/miss per turn, auditable |
| **Permission modes** | `ask` (grouped prompt per destructive op), `auto` (approve all), `deny` (fully read-only); "always" grant auto-approves a tool class for the session |
| **Skills** | 23 built-in; always-on + keyword-triggered; user skills in `~/.zap/skills/` or `.zap/skills/`; SKILL.md standard compatible with Claude Code and Cursor |
| **Skill trace** | `/skill log` — see which skills fired (or why they didn't) for every turn this session |
| **Skill capture** | `/skill capture <name>` — extract session rules into a reusable skill file |
| **Skill scope** | `/skill scope` — pin or restrict which domain skills are active for a session without editing files |
| **Deploy** | `/deploy` — builds and installs zap with live streamed output; no shell timeout |
| **Context mgmt** | Skill injection, casual-turn optimization (~20 tokens for greetings), sliding history window, tool-result pruning, `/compact` in-place summarisation, Anthropic prompt caching |
| **Project init** | `/init` — auto-detects stack, indexes codebase, and generates `ZAP.md` + `.zap/understanding.md` filled with project-specific architecture, build commands, and constraints; ~30 seconds to full context awareness |
| **Project intelligence** | `.zap/context.md` session handoff (last goal, files touched, what's next); `.zap/understanding.md` LLM-maintained project knowledge; `.zap/session_log.md` session history — read on demand, not pre-loaded |
| **Sessions** | Every conversation persisted; `/sessions` fuzzy picker to resume any |
| **Branching** | `/branch` forks a conversation like a git branch; `/switch` to move between them |
| **Sub-agents** | `spawn_agent` runs parallel sub-agents with their own tool loop; multiple spawns in one turn run in parallel |
| **Autonomous loop** | `/goal <condition>` runs turns automatically until the model signals done or a turn limit is reached |
| **Extended thinking** | `/think [on\|off\|N]` — Anthropic extended thinking with configurable token budget |
| **MCP (lazy-loaded)** | Standard `.mcp.json` format — works in Claude Code, Cursor, Kiro; servers connect on demand, zero cost until first use |
| **Workflows** | Declarative YAML multi-step pipelines in `.zap/workflows/` — versioned with your repo |
| **Hooks** | `PreToolUse` / `PostToolUse` / `SessionStart` / `SessionEnd` / `UserPromptSubmit` — shell commands that run on agent events |
| **Remote control** | `/remote` starts a local HTTP server + public tunnel; drive the session from any browser or phone |
| **Images** | `/attach <path>` or `/paste` clipboard — multimodal on supported models |
| **Audit log** | Every tool call written to `~/.zap/audit.jsonl` |
| **Secret scanner** | 25+ patterns — Anthropic/OpenAI/Stripe keys, GitHub/GitLab tokens, AWS/GCP credentials, private keys, JWTs, generic password/api_key/secret fields — blocked before sending to any cloud LLM |
| **Cost display** | Token breakdown per turn — skills, message, context, estimated $ |

---

### Token efficiency — smart context detection

Pure greetings and social messages use a minimal 31-token prompt with no tools, even mid-conversation. Everything else gets the full context injection:

| Message | After model asks a question? | Result |
|---|---|---|
| "hi", "hello", "hey" | yes | ~31 tokens (casual) |
| "thanks", "thank you", "ty" | yes | ~31 tokens (casual) |
| "good morning", "how are you" | yes | ~31 tokens (casual) |
| "yes" | yes | full context |
| "go ahead" | yes | full context |
| "ok", "cool", "sounds good" | yes | full context |
| any technical question | — | full context |

After the model asks a clarifying question, short replies like "yes", "ok", "go ahead" are treated as answers and receive the full context. Pure social messages always stay casual.

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
| `/index quality` | Code quality report: god objects, coupling, dead code, quality score |
| `/deploy [--check]` | Build and install zap with live streaming output, no timeout |
| `/undo [file]` | Undo the last file edit |
| `/init` | Create a `CLAUDE.md` for this project (auto-filled by the agent) |
| `/run <workflow>` | Run a `.zap/workflows/<name>.yaml` pipeline |
| `/workflow new <name>` | Scaffold a new workflow file |
| `/tasks` | Browse and execute structured task sessions from `.zap/tasks/` |
| `/attach <path>` | Stage an image for the next message |
| `/paste` | Paste an image from the clipboard |
| `/memory list\|get\|set\|del` | Manage persistent key-value memory |
| `/skill list\|show\|create\|log` | Manage skills; `log` shows which skills fired per turn |
| `/skill scope` | Show or change which domain skills are active for this session |
| `/hooks` | List all configured hooks and their trigger events |
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
/skill log                       show which skills fired (or why they didn't) per turn this session
/skill scope                     show which domain skills are active this session
/skill scope add <name>          add a skill to the active scope
/skill scope remove <name>       remove a skill from the active scope
/skill scope reset               restore default scope
/skill export <name>             re-export a built-in to ~/.zap/skills/
/skill export --all              re-export every built-in skill
/skill create <name>             scaffold a new skill in .zap/skills/
/skill create <name> --global    scaffold in ~/.zap/skills/
/skill capture <name>            extract rules from this session into a skill file
/skill capture <name> --global   save captured skill globally
```

**Skill trace** — `/skill log` lets you audit which skill fired (or didn't) for every turn this session. If a skill you expected to fire didn't, the log shows "no match" or "casual" with the turn preview so you can tune the trigger keywords:

```
  turn #3  "refactor the async fn to use channels"    → rust, karpathy-guidelines
  turn #4  "commit these changes"                     → git, karpathy-guidelines
  turn #5  "hey thanks"                               → (casual)
  turn #6  "add an endpoint for POST /users"          → (no match)  ← missing api-conventions skill?
```

Same-name skills override lower-priority ones: `.zap/skills/` > external paths > `~/.zap/skills/` > built-in.

### Multi-tool skill sources — Kiro, Claude Code, and custom dirs

If your project already has skills written for Amazon Kiro (`.kiro/skills/`) or Claude Code (`.claude/skills/`), you can pull them into zap without copying files. Add `skill_paths` to `~/.agent.toml`:

```toml
# ~/.agent.toml
skill_paths = [
    ".kiro/skills",          # Amazon Kiro skills (per-project)
    ".claude/skills",        # Claude Code skills (per-project)
    "~/shared-skills",       # your own cross-project library
]
```

zap loads every `.md` file it finds in those directories and merges them into the flat skill registry. Skills from `skill_paths` override global (`~/.zap/skills/`) but lose to project-local (`.zap/skills/`).

**Full precedence** (lowest → highest, later wins on name collision):

| Source | Location | Glyph in `/skill list` |
|---|---|---|
| Built-in | compiled into binary | `◆` |
| Global | `~/.zap/skills/` | `●` |
| External | `skill_paths` entries, left → right | `◉` |
| Project | `.zap/skills/` | `▶` |

`/skill list` shows the source tag next to every skill so you can see exactly where each one came from:

```
  ◆ rust             [built-in]   always-on
  ● karpathy-guidelins [global]   always-on
  ◉ api-design       [kiro/skills]  rest, endpoint, api
  ◉ code-review      [claude/skills]  code review, pr review
  ▶ team-principles  [project]    always-on
```

**Name collision across sources** — if Kiro and `.zap/skills/` both ship a `code-review` skill, the project-local one wins silently. There is currently no `kiro:code-review` prefix syntax to reference a specific source by name — all skills share a flat namespace. If two external sources define the same skill name, the rightmost `skill_paths` entry wins.

### Always-on context from other tools — Kiro steering, Claude context

Steering documents (`.kiro/steering/`) and Claude project context files aren't skills — they have no trigger and no frontmatter. Use `context_paths` to load them as always-on system context:

```toml
# ~/.agent.toml
context_paths = [
    ".kiro/steering",     # Kiro steering docs — loaded every session
    ".claude/context",    # Claude context docs
]
```

All `.md` files found in `context_paths` directories are appended to the system prompt every turn, after the main ZAP.md/CLAUDE.md. Frontmatter (`---` blocks) is stripped automatically. Files are sorted by filename within each directory, so `01-architecture.md` loads before `02-conventions.md`.

> **Tip:** `skill_paths` and `context_paths` are complementary. Use `skill_paths` for keyword-triggered guidance and `context_paths` for always-on context that applies regardless of what you're asking.

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

See [AST Code Index — Understands Your Code, Not Just Text](#2-ast-code-index--understands-your-code-not-just-text) above for the full explanation, including the blind-writes problem, SQL query examples, per-write reindex guarantee, index usage logging, and the comparison with Claude Code's agentic search approach.

**Quick reference:**

| Command | What it shows |
|---|---|
| `/index` | Reindex manually |
| `/index stats` | File count, symbol count by kind, top files by density |
| `/index quality` | God objects, large files, high coupling, dead code candidates, quality score |

---

## Session Management

Every conversation is persisted locally. Use `/sessions` to browse and resume any previous session with an interactive fuzzy picker.

---

## Sub-agents

When `agent_depth > 0` (default: 3), the model can call `spawn_agent` to delegate independent tasks. Multiple spawns within a single LLM turn run in parallel, each with its own message history and tool access.

---

## Developer Journeys

Real scenarios — what actually happens at each stage.

---

### Journey 1 — First time opening a project

**Scenario:** You've just cloned a Java microservice you've never seen before. Twelve services, Spring Boot, Maven, no docs.

```bash
cd order-service
zap
```

zap starts in under a second. But the agent has no knowledge of this project yet — it's a blank slate. The right move is `/init`.

```
/init
```

Here's what happens step by step:

```
◌ Detected project type: java
Language(s) for this project: java        ← confirm or correct

◌ Indexing lets zap find symbols instantly without reading every file.
Index this project now? (recommended, ~10s)  Y

  Indexing src/ ...
  ✓ tree-sitter · java · 847 symbols across 63 files

✓ .zap/project.json written.
✓ Created ZAP.md for java project.
⚡ Asking the agent to analyse the repo and fill in ZAP.md…
```

The agent now reads the source files and fills in `ZAP.md` — a persistent project knowledge file loaded into every future session:

```markdown
## Overview
Order service — handles order lifecycle (create, fulfil, cancel).
Publishes events to Kafka on state transitions.

## Build & Test
mvn clean install        # full build
mvn test                 # unit tests only
mvn spring-boot:run      # local dev server on :8080

## Architecture
- OrderController  → REST handlers (src/main/java/.../controller/)
- OrderService     → business logic, calls OrderRepository
- OrderRepository  → JPA, Postgres via spring-data
- KafkaProducer    → publishes OrderCreated / OrderFulfilled events

## Important Files
- OrderService.java     — core domain logic, start here
- application.yml       — all config including Kafka brokers
- schema.sql            — DB schema

## Do Not Touch
- LegacyOrderMapper.java — deprecated, kept for backwards compat, do not edit
```

At the same time, it writes `.zap/understanding.md` — a deeper technical summary covering entry points, data flows, non-obvious patterns, and constraints. This file is loaded silently into every future session so the agent always starts with structural knowledge of the project.

**Total time: ~30 seconds.** From zero to a fully context-aware agent.

---

### Journey 2 — Returning to a project

**Scenario:** You worked on the order service last week. You open zap today to continue.

```bash
cd order-service
zap
```

Cold start. But zap is not starting blind. Before your first message, it has already loaded:

| File | What it contains |
|---|---|
| `ZAP.md` | Project overview, build commands, architecture, do-not-touch list |
| `.zap/understanding.md` | Module map, data flows, patterns, constraints |
| `.zap/context.md` | Last session: goal, files touched, what's next |
| `.zap/session_log.md` | History of all previous sessions |

The agent's first response reflects all of this — it already knows what you were working on, which files changed, and what was left unfinished. You don't re-explain the project. You just continue:

```
you:  "what were we working on last time?"

zap:  Last session you were adding pagination to GET /orders.
      You updated OrderController.java and OrderService.java.
      The service method was done but the controller test was still failing
      — that was left as the next step.
```

No re-reading files. No re-explaining the stack. The session handoff is automatic.

---

### Journey 3 — Understanding unfamiliar code

**Scenario:** A colleague wrote the `FulfilmentService` six months ago. You need to understand it before touching it.

```
"explain how FulfilmentService works — what it does, what it calls, what could go wrong"
```

```
→ java skill fires (class keyword matched)
→ find_definition looks up FulfilmentService in the index — found at
  src/main/java/.../service/FulfilmentService.java:34
→ code_map outlines all methods: fulfil(), rollback(), notifyWarehouse()
→ reads only the relevant sections, not the whole file
→ traces the call chain: fulfil() → OrderRepository.save() → KafkaProducer.publish()
→ flags: rollback() has no test coverage, notifyWarehouse() blocks on HTTP with no timeout
```

You get a structural explanation in seconds — including the non-obvious risks — without reading 400 lines of Java yourself.

**Drill deeper:**

```
"show me every place FulfilmentService.fulfil() is called"
→ find_references scans the index
→ 3 call sites: OrderController.java:88, OrderSaga.java:142, FulfilmentTest.java:67
```

```
"what does the data flow look like from the REST call to Kafka publish?"
→ traces: POST /fulfil → FulfilmentController → FulfilmentService.fulfil()
         → OrderRepository.save() (Postgres) → KafkaProducer.publish() (Kafka)
→ notes: the Kafka publish happens inside the DB transaction — risky if Kafka is down
```

---

### Journey 4 — Adding a feature to an existing codebase

**Scenario:** You need to add a `GET /orders/{id}/history` endpoint to the existing order service.

Without zap (what most agents do): the agent creates `src/main/java/.../controller/HistoryController.java`, `src/main/java/.../service/HistoryService.java`, `src/main/java/.../repository/HistoryRepository.java` — duplicating the structure that already exists, ignoring the patterns already in use.

With zap:

```
"add a GET /orders/{id}/history endpoint that returns the state change log"
```

```
→ java skill fires
→ index query: WHERE name LIKE '%Order%' AND kind = 'class'
  finds: OrderController, OrderService, OrderRepository, OrderHistory (entity — already exists!)
→ index query: WHERE name LIKE '%Repository%' AND kind = 'interface'
  finds: OrderRepository extends JpaRepository<Order, Long> — this is the pattern to follow
→ index query: WHERE path LIKE '%controller%' ORDER BY line
  finds: all existing controllers — sees they all extend BaseController, use @RestController

zap:  OrderHistory entity already exists at OrderHistory.java:12.
      I'll add a findByOrderId() method to OrderRepository, a getHistory() method
      to OrderService, and a new endpoint to OrderController — no new files needed.
      Shall I proceed?
```

One file touched in `OrderController`, one method added to `OrderRepository`, one to `OrderService`. No duplicate classes. Follows the existing `JpaRepository` pattern. Uses `BaseController` like every other controller.

**This is the core difference.** The agent knew what existed before it decided what to create.

---

### Journey 5 — Fixing a bug

**Scenario:** Orders are occasionally being marked as fulfilled even when the warehouse notification fails.

```
"orders are being fulfilled even when notifyWarehouse() throws — trace why"
```

```
→ debugging skill fires
→ find_definition locates fulfil() at FulfilmentService.java:67
→ reads the method — sees notifyWarehouse() is called AFTER OrderRepository.save()
→ the save() commits to DB before the warehouse call — if warehouse throws, order
  is already marked fulfilled in the DB with no rollback
→ finds: @Transactional annotation is present but notifyWarehouse() is an HTTP call
  outside the transaction boundary — @Transactional won't roll it back

zap:  Root cause: the DB commit happens before the warehouse HTTP call.
      @Transactional covers the DB write but not the external HTTP call.
      Fix: move notifyWarehouse() before the save(), or wrap it in a try/catch
      that explicitly rolls back via OrderRepository.updateStatus().
      Which approach do you prefer?
```

You pick the approach, zap makes the surgical edit, shows you the diff, asks for approval before writing.

---

### Journey 6 — Checking and improving code quality

**Scenario:** The team suspects the service is getting harder to change. You want data before the next sprint planning.

```
/index quality
```

```
◎ code quality — 63 files · 847 symbols                          score: 61/100
────────────────────────────────────────────────────────────────────────────

⚠ god objects  (impl with >15 methods — split recommended)
  OrderService          34 methods  ██████████████  src/.../service/OrderService.java
  FulfilmentService     18 methods  ███████         src/.../service/FulfilmentService.java

⚠ large files  (>50 symbols)
    91 sym  ████████████████████  OrderService.java
    67 sym  ██████████████        OrderController.java

✦ high coupling  (referenced in many places — risky to change)
  OrderService.fulfil()     29×   FulfilmentService.java:67
  OrderRepository.save()    24×   (multiple callers)

◌ dead code candidates  (public method, 0 external references)
  LegacyOrderMapper.toDto()    LegacyOrderMapper.java:44
  OrderUtils.formatId()        OrderUtils.java:18

→ OrderService has 34 methods — consider splitting into OrderLifecycleService + OrderQueryService
→ 2 public methods never referenced — confirm they can be removed
```

Now you have concrete data for the sprint discussion. `OrderService` is the riskiest file to change — 29 places call `fulfil()`. The two dead methods are candidates for deletion. Score is 61 — room to improve before it becomes painful.

```
"which methods in OrderService are safe to extract to a new OrderQueryService?"
→ queries index for all methods in OrderService with kind=method
→ cross-references call sites — methods only called from read endpoints are safe to extract
→ lists: findById(), findByStatus(), findByDateRange(), getOrderSummary() — all query-only, no writes
```

---

### Journey 7 — Wrapping up and handing off

At the end of any session:

```
"we added pagination to GET /orders and fixed the fulfilment race condition —
 update context.md with what we did and what's still left"
```

zap writes `.zap/context.md`:

```markdown
## Last updated
2026-05-25 — Session #42

## What was being worked on
Added cursor-based pagination to GET /orders endpoint.
Fixed race condition in FulfilmentService where DB commit preceded warehouse HTTP call.

## Files touched
- OrderController.java
- OrderService.java
- FulfilmentService.java
- OrderControllerTest.java

## What's next
- Pagination test for edge case: empty cursor on last page
- Consider splitting OrderService (34 methods — see /index quality output)
```

Tomorrow's session picks this up automatically. No re-explaining. No lost context.

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
