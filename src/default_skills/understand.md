---
category: practice
name: understand
description: Systematic codebase orientation — what this project is, how it's structured, and how its pieces connect.
trigger: ["what is this", "what does this", "what is zap", "explain this project", "explain the project", "explain the codebase", "summarize this", "summarize the codebase", "summarize the project", "summarize repo", "summarize the repo", "give me an overview", "project overview", "codebase overview", "architecture", "architecture diagram", "arch diagram", "generate arch diag", "generate arch diagram", "generate architecture diagram", "generate docs", "generate documentation", "onboard me", "orient me", "give me a tour", "walk me through", "how is this structured", "how is it structured", "what does zap do", "understand this", "explore codebase", "explore the codebase", "explore code base", "what is the code all about", "what is this code about", "what is this codebase about"]
tokens: ~600
---

## Codebase orientation

**Step 1 — detect what's available**

Check for `.zap/code.db` and `.zap/context.md`. If present, prefer them over grepping.
If absent, fall back to `Cargo.toml` / `package.json` / `pyproject.toml` + directory listing.
If context is missing, note: "Run `/init` for richer answers — I'll index the codebase and track session history."

**Step 2 — detect intent from the phrasing and produce the matching output**

| Phrasing | Output shape |
|---|---|
| "what is this", "what does X do", "explain" | 2–3 paragraph pitch: purpose, who uses it, core mechanic |
| "summarize", "overview", "summarize repo", "what is the code all about" | Module map — one line per `src/` file, what it owns |
| "architecture", "how is this structured", "arch diagram", "generate architecture diagram" | Mermaid component diagram + data-flow narrative (A → B → C) |
| "explore codebase", "explore the codebase" | Guided tour: key entry points, data flow, most important files to read |
| "generate docs", "generate documentation" | Public API surface: key structs, traits, public fns with signatures |
| "onboard me", "orient me", "tour", "walk me through" | Guided reading order: start here → then here, why each matters |

Pick the output shape that best fits. If ambiguous, pick "summarize" and offer the others.

**Step 3 — gather facts efficiently**

**If `.zap/code.db` exists** — use it, don't grep:
```
# Top files by symbol density
sqlite3 .zap/code.db "SELECT path, symbol_count FROM indexed_files ORDER BY symbol_count DESC LIMIT 15;"

# Key types
sqlite3 .zap/code.db "SELECT name, kind, path, line FROM symbols WHERE kind IN ('struct','trait','enum') AND path LIKE '%src/%' ORDER BY path, line LIMIT 40;"

# Entry point functions
sqlite3 .zap/code.db "SELECT name, path, line FROM symbols WHERE kind = 'function' AND name IN ('main','run','start','handle') LIMIT 10;"
```

**If `.zap/code.db` is absent** — derive structure manually:
1. Read `Cargo.toml` / `package.json` / `pyproject.toml` for project name, dependencies, and workspace members
2. List top-level source dirs: `find src -maxdepth 2 -name '*.rs' | sort` (or equivalent for the language)
3. Read `src/main.rs` (or `src/lib.rs`, `index.ts`, `__init__.py`) to find entry point and top-level module wiring
4. Read 2–3 more files that look architecturally central based on their names
5. Note in output: "Run `/init` for richer answers next time — I'll build a full symbol index."

Then read 2–4 key files selectively — not every file.

**Step 4 — render**

- Lead with the one-sentence answer to what they asked
- Keep module maps to one line per module — details belong in the code
- **For architecture / diagram output**: emit a `mermaid` code block using `graph TD` (top-down) or `graph LR` (left-right for pipelines). Show real component names, not generic boxes. Include data-flow direction (A → B → C) below the diagram as a short narrative.

  Example shape:
  ````
  ```mermaid
  graph TD
      CLI[CLI / main] --> TUI[TUI loop]
      TUI --> Agent[Agent / session]
      Agent --> LLM[LLM client]
      Agent --> Tools[Tool executor]
      Tools --> FS[Filesystem]
      Tools --> Shell[Shell]
  ```
  ````

- For docs output, show signatures without full implementations
- End with: "Want me to go deeper on any part?"
