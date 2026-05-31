---
category: practice
name: understand
description: Systematic codebase orientation — what this project is, how it's structured, and how its pieces connect.
trigger: ["what is this", "what does this", "what is zap", "explain this project", "explain the project", "explain the codebase", "summarize this", "summarize the codebase", "summarize the project", "give me an overview", "project overview", "codebase overview", "architecture", "generate docs", "generate documentation", "onboard me", "orient me", "give me a tour", "walk me through", "how is this structured", "how is it structured", "what does zap do", "understand this"]
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
| "summarize", "overview" | Module map — one line per `src/` file, what it owns |
| "architecture", "how is this structured" | Component diagram (text/mermaid) + data-flow narrative |
| "generate docs", "generate documentation" | Public API surface: key structs, traits, public fns with signatures |
| "onboard me", "orient me", "tour", "walk me through" | Guided reading order: start here → then here, why each matters |

Pick the output shape that best fits. If ambiguous, pick "summarize" and offer the others.

**Step 3 — gather facts efficiently**

When `code.db` exists, use it — don't grep:
```
# Top files by symbol density
sqlite3 .zap/code.db "SELECT path, symbol_count FROM indexed_files ORDER BY symbol_count DESC LIMIT 15;"

# Key types
sqlite3 .zap/code.db "SELECT name, kind, path, line FROM symbols WHERE kind IN ('struct','trait','enum') AND path LIKE '%src/%' ORDER BY path, line LIMIT 40;"

# Entry point functions
sqlite3 .zap/code.db "SELECT name, path, line FROM symbols WHERE kind = 'function' AND name IN ('main','run','start','handle') LIMIT 10;"
```

Then read 2–4 key files selectively — not every file.

**Step 4 — render**

- Lead with the one-sentence answer to what they asked
- Keep module maps to one line per module — details belong in the code
- For architecture output, show data flow direction (A → B → C), not just a component list
- For docs output, show signatures without full implementations
- End with: "Want me to go deeper on any part?"
