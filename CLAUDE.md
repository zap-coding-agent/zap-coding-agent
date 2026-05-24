# Project Context

This is **zap** — a Rust TUI AI coding agent.

## At session start — always do this first

Read these files before answering any question about the project:

1. `.zap/context.md` — last session's work, files touched, and what's next
2. `.zap/session_log.md` — brief history of past sessions (goal + files per session)

## Code index — use sqlite3 for symbol/file lookups

`.zap/code.db` is a SQLite database with full symbol and file indexes. Use it instead of grepping when looking up symbols, file contents, or definitions.

**Find a symbol by name:**
```
sqlite3 .zap/code.db "SELECT path, line, kind, signature FROM symbols WHERE name LIKE '%SymbolName%' COLLATE NOCASE LIMIT 20;"
```

**Find all symbols in a file:**
```
sqlite3 .zap/code.db "SELECT name, kind, line, signature FROM symbols WHERE path LIKE '%filename%' ORDER BY line;"
```

**Find symbols by kind (function, struct, enum, impl, trait, etc.):**
```
sqlite3 .zap/code.db "SELECT name, path, line FROM symbols WHERE kind = 'function' AND name LIKE '%search%';"
```

**List all indexed files:**
```
sqlite3 .zap/code.db "SELECT path, symbol_count FROM indexed_files ORDER BY symbol_count DESC;"
```
