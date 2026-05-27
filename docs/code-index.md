# AST Code Index — Understands Your Code, Not Just Text

Most agents navigate code the same way a shell script does — grep for a string, hope the result is what you meant. zap builds a real **AST symbol index** at startup using tree-sitter + SQLite, giving the model genuine structural understanding of your codebase.

## The problem: agents that write without looking

Ask most coding agents to "add all the API layers for user management" in an existing project and you'll see a predictable set of mistakes:

- **Duplicate files created** — `src/user_repository.rs` already exists, but the agent creates `src/repositories/user_repo.rs` alongside it because it never checked
- **Existing patterns ignored** — the project uses a `Repository<T>` trait with a specific error type; the agent invents its own DB access style from scratch
- **Scaffolding over existing code** — `src/routes/`, `src/models/`, `src/db/` already exist with boilerplate; the agent recreates them
- **Missed abstractions** — a `BaseRepository` or shared `AppError` type already exists; the agent writes a duplicate

These aren't model failures — they're context failures. The agent is writing blind because its context window never contained the files it needed to check.

## How the index fixes it

When you ask zap the same question, before writing a single line it queries the index:

```sql
-- Does a user repository already exist?
SELECT path, line, kind FROM symbols WHERE name LIKE '%UserRepo%' OR name LIKE '%UserStore%';

-- What repository pattern does this project use?
SELECT name, path, line, signature FROM symbols WHERE kind = 'trait' AND name LIKE '%Repository%';

-- What's already in the db/ directory?
SELECT name, kind, line FROM symbols WHERE path LIKE '%/db/%' ORDER BY path, line;
```

This runs in milliseconds against the local SQLite index — no file reads, no grep, no context stuffing. The model knows what exists before it decides what to create.

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

## Index properties

**Incremental** — on subsequent runs, only files that changed since the last session are re-parsed. A background indexer runs every 120s during interactive sessions so the index stays fresh as you edit. Cold-indexing a 50k-line repo takes a few seconds; warm starts are near-instant.

**Always current during edits** — every time zap writes a file, it immediately reindexes that file before the next LLM turn. The model never queries a stale index for files it just changed.

## Index usage logging

Every time a tool call is answered by the index (rather than falling back to grep), zap logs it to `~/.zap/zap.log` and `~/.zap/audit.jsonl`:

```
[INDEX] hit  · find_definition · 'UserRepository' · 3 result(s)
[INDEX] hit  · code_map        · 'src/db/'        · 42 symbol(s)
[INDEX] miss · find_definition · 'legacy_fn'      · grep fallback
```

This makes it auditable — you can see exactly when the index was used vs. when the agent had to fall back to text search.

## Supported languages

Rust, Python, TypeScript, JavaScript, Go, Java

## Powered tools

| Tool | What it does |
|---|---|
| `code_map` | Structural outline of any file or directory — functions, structs, classes, enums, with line numbers |
| `find_definition` | Jump directly to where a symbol is defined — AST index first, ripgrep fallback |
| `find_references` | Every call site of a symbol across the entire codebase |

The model is instructed to always use `code_map` or `find_definition` before reaching for `read_file` — so it reads only the lines it actually needs, not whole files.

## Code quality report

The same SQLite index powers `/index quality`, a human-readable health report run directly in the TUI:

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

Line counts are read from disk; symbol counts and coupling metrics come from SQLite. Reference counts are computed in one O(source_size) pass at the end of every `/index` run.

## Commands

| Command | What it shows |
|---|---|
| `/index` | Reindex manually |
| `/index stats` | File count, symbol count by kind, top files by density |
| `/index quality` | God objects, large files, high coupling, dead code candidates, quality score |

## Why zap indexes when Claude Code deliberately doesn't

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

**zap makes the opposite bet.** Agentic search solves semantic questions well ("find code related to payment processing"). A persistent AST index solves structural questions better — "what already exists in this module?", "which files implement this pattern?", "is there already a `UserRepository`?" These are exactly the questions that matter when an agent is about to *write* new code.

The two approaches solve different failure modes. Agentic search avoids index drift. AST indexing avoids blind writes into a codebase the agent hasn't fully read.
