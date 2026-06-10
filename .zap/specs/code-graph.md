# Code Graph — Design Spec

Status: Draft
Owner: zap
Replaces: text-scan `ref_count` (kept as derived view)

## Goal

Upgrade the code index from **symbols-only (B-tier)** to **symbols + call graph + import graph + agent-shaped context APIs (A-tier+)**, without taking on type-resolution complexity.

Concretely, after this lands:
- `find_references("foo")` returns real call sites with file + line + enclosing function — not a popularity integer.
- `who_imports("crate::util::parse")` returns every importing file.
- `pack_context(task, token_budget)` returns a curated bundle of the most relevant symbols/files for a task, ranked.

## Non-goals

- Type resolution. `foo.bar()` where `foo: Box<dyn Trait>` stays unresolved. Agents handle name-based ambiguity fine.
- Macro expansion.
- Cross-language edges (e.g. TS → Rust FFI).
- LSP integration. Reserved as opt-in escalation if name-based queries prove insufficient (revisit after phase 3).

## Schema

Additions to `.zap/code.db`:

```sql
CREATE TABLE call_sites (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    path          TEXT    NOT NULL,
    line          INTEGER NOT NULL,
    col           INTEGER NOT NULL DEFAULT 0,
    name          TEXT    NOT NULL,       -- "foo" in foo(...) or x.foo()
    qualifier     TEXT    NOT NULL DEFAULT '',  -- "Bar" in Bar::foo, "self" in self.foo, "" if bare
    receiver_expr TEXT    NOT NULL DEFAULT '',  -- raw text of receiver, truncated to 64 chars
    caller_scope  TEXT    NOT NULL DEFAULT '',  -- enclosing fn/method context ("impl Foo · bar")
    language      TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX idx_cs_name      ON call_sites(name COLLATE NOCASE);
CREATE INDEX idx_cs_path      ON call_sites(path);
CREATE INDEX idx_cs_qualifier ON call_sites(qualifier);

CREATE TABLE imports (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    path           TEXT    NOT NULL,
    line           INTEGER NOT NULL,
    module         TEXT    NOT NULL,        -- "std::collections" / "react" / "github.com/x/y"
    imported_name  TEXT    NOT NULL DEFAULT '',  -- "HashMap" — empty if whole-module ("import os")
    alias          TEXT    NOT NULL DEFAULT '',
    language       TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX idx_imp_path ON imports(path);
CREATE INDEX idx_imp_name ON imports(imported_name COLLATE NOCASE);
CREATE INDEX idx_imp_mod  ON imports(module);
```

`symbols.ref_count` stays as a denormalised column for fast queries, but is now **derived from call_sites** during indexing rather than from the text scan:

```sql
UPDATE symbols
SET ref_count = (SELECT COUNT(*) FROM call_sites cs WHERE cs.name = symbols.name);
```

This preserves the existing `quality_report` queries unchanged.

## Extraction approach

Per-language tree-sitter additions, mirroring the existing `extract_<lang>_node` pattern in `src/code_index/extract.rs`. Walk the same AST that already produces symbols; emit call_sites and imports as side outputs.

### Tree-sitter node kinds (cheat sheet)

| Language | Call sites | Imports |
|---|---|---|
| Rust | `call_expression`, `macro_invocation` | `use_declaration` |
| Python | `call` | `import_statement`, `import_from_statement` |
| JS/TS | `call_expression`, `new_expression` | `import_statement`, `import_clause` |
| Go | `call_expression` | `import_declaration`, `import_spec` |
| Java | `method_invocation`, `object_creation_expression` | `import_declaration` |
| C# | `invocation_expression`, `object_creation_expression` | `using_directive` |

### Call-site shape

For `foo::bar::baz(x, y)`:
- `name = "baz"`, `qualifier = "foo::bar"`, `receiver_expr = ""`

For `x.foo(...)`:
- `name = "foo"`, `qualifier = "", `receiver_expr = "x"` (truncated to 64 chars)

For `Bar::new()`:
- `name = "new"`, `qualifier = "Bar"`

Rule of thumb: split on the **last** path/dot separator. Everything left is `qualifier`, the trailing identifier is `name`. Receiver expressions (method calls on values) go in `receiver_expr` for downstream heuristics.

### Caller scope

Reuse the existing `context` propagation pattern from `extract_rust_node`. Track the enclosing fn/method while walking; when a call node is hit, record the current scope. Format: same as existing `Symbol.context` so joins work cleanly.

### Imports shape

For Rust `use std::collections::HashMap as Map;`:
- `module = "std::collections"`, `imported_name = "HashMap"`, `alias = "Map"`

For Rust glob `use std::collections::*;`:
- `module = "std::collections"`, `imported_name = "*"`

For Python `from os import path, sep as separator`:
- Two rows, both with `module = "os"`. First: `imported_name="path"`. Second: `imported_name="sep"`, `alias="separator"`.

For Python `import os`:
- `module = "os"`, `imported_name = ""`.

Flatten group imports into individual rows. Optimise for query simplicity over table compactness.

## Update logic

The existing mtime-based incremental loop already handles this cleanly. Per file:

```rust
// In CodeIndex::index_file, inside the existing transaction:
tx.execute("DELETE FROM symbols     WHERE path = ?1", params![path_str])?;
tx.execute("DELETE FROM call_sites  WHERE path = ?1", params![path_str])?;
tx.execute("DELETE FROM imports     WHERE path = ?1", params![path_str])?;
// re-insert all three from the single tree-sitter pass
```

**Key invariant**: nothing stores cross-file IDs. Call sites store *names*, not symbol_ids. So a rename in file A doesn't invalidate call sites in file B — they correctly point to a name that no longer has a definition (the right answer, not staleness). No cross-file invalidation logic needed.

`compute_reference_counts` becomes a single SQL UPDATE (see schema). Runs at end of `index_dir` and `index_file`, same hook points as today.

## Query API surface

New `CodeIndex` methods (and matching `global_*` wrappers):

```rust
// Reverse index — the headline feature.
fn find_references(&self, name: &str, limit: usize)
    -> Result<Vec<CallSite>>;

// "Who calls this specific function?" — narrows by qualifier when known.
fn callers_of(&self, name: &str, qualifier: Option<&str>, limit: usize)
    -> Result<Vec<CallSite>>;

// "What does this file pull in?"
fn imports_for(&self, path: &str) -> Result<Vec<Import>>;

// "Which files import this name?" — useful for blast-radius checks.
fn importers_of(&self, name: &str) -> Result<Vec<Import>>;

// PageRank-style file ranking. See next section.
fn rank_files(&self) -> Result<Vec<(String, f32)>>;

// Agent-shaped: returns a packed bundle of relevant symbols within budget.
fn pack_context(&self, task: &str, token_budget: usize)
    -> Result<PackedContext>;
```

Expose via tools in `src/tools/search/` (alongside existing `symbols.rs`). Suggested tool names for the agent: `find_references`, `who_calls`, `file_imports`, `pack_context`.

## PageRank for file ranking

Aider's killer move. Trivial to add once call_sites + imports exist.

Build a sparse graph:
- Nodes = files
- Edges = (file containing call site) → (file containing definition of that name)
- Edge weight = number of references
- Imports contribute fractional edges too (so `mod.rs` style hubs surface correctly)

Iterate PageRank ~25 times with damping 0.85. Pure SQL + a Rust loop, no extra dependencies. Store result in a `file_rank` table (or compute on demand if the codebase is small — recompute is cheap).

Use ranks to:
1. Order results in `find_references` when many sites exist
2. Pick the top-N files in `pack_context`
3. Surface "load-bearing" files in `/health` or a new `/important` view

## `pack_context` — the differentiator

Most tools expose IDE primitives. This one is agent-shaped.

```rust
pub struct PackedContext {
    pub files: Vec<PackedFile>,
    pub total_tokens: usize,
    pub strategy: String,  // "name_match", "rank_neighbourhood", "import_closure"
}
```

v1 algorithm (keep simple):
1. Extract candidate symbols by name match against task text
2. Expand to one hop: callers of those symbols + files importing those symbols
3. Score candidates by `task_text_match * pagerank * recency`
4. Greedy-pack symbol signatures (cheap) until 30% of budget, then full bodies of top-ranked symbols until budget hit
5. Return with provenance per chunk so the agent knows *why* each piece is included

This is the spot where iteration matters most — start basic, instrument, improve.

## Migration from current `ref_count`

Backwards-compatible. Sequence:

1. Add tables. Existing index files keep working.
2. New extractors emit call_sites/imports alongside existing symbol extraction.
3. `compute_reference_counts` switches from `count_call_sites` text scan to SQL aggregate over `call_sites`.
4. Delete `count_call_sites` from [walk.rs](../src/code_index/walk.rs) once the SQL path is live and verified against a few known-good files.
5. `quality_report` queries unchanged — they read `ref_count` which is now derived from real call sites instead of text occurrences.

Expected effect: `ref_count` numbers will drop modestly (text scan over-counts — matches inside generics, type names, etc.). `dead_candidates` and `high_coupling` should get *more* accurate, not less.

## Phased rollout

| Phase | Scope | Cost |
|---|---|---|
| **1. Extraction** | Add call_sites + imports tables. Add Rust + Python + TS/JS extractors. Wire into `index_file`. Migrate `ref_count` to SQL aggregate. | ~3 days |
| **2. Query APIs** | `find_references`, `callers_of`, `imports_for`, `importers_of`. Expose as agent tools. | ~1 day |
| **3. Ranking** | PageRank over the graph. `rank_files`. Use in `find_references` ordering. | ~1 day |
| **4. Pack context** | `pack_context` v1 with provenance. Iterate. | ~2 days, then ongoing |
| **5. Remaining langs** | Go, Java, C# extractors. | ~1 day |
| **6. (Optional) LSP** | Only if we hit a ceiling on name-based queries. | Defer indefinitely |

Phase 1–4 = ~1 week, gets zap to A-tier+ with the agent-shaped layer that no MCP server currently ships.

## Open questions

- **Storage**: at large repos (~100k symbols), call_sites table could reach ~1M rows. SQLite handles this fine, but worth measuring on a real repo before committing to indexed columns we don't need.
- **Macro calls in Rust**: `println!`, `vec!` — treat as call sites or filter? Probably emit with a separate `kind = "macro"` field if we extend the schema, otherwise filter the noisy stdlib ones.
- **Method receiver resolution**: should we attempt a quick heuristic (look for `let receiver: Type = ...` in same scope) or stay strictly name-based? Stay name-based for v1.
- **Re-export tracking** (`pub use foo::Bar`): adds complexity; skip in v1, revisit if it hurts `importers_of` accuracy.

## Out of scope (deliberately)

- Vector/embedding-based search. Orthogonal; can be added as a separate layer later that *reads* call_sites for filtering. Don't entangle.
- A web UI / browser. zap is TUI.
- Cross-repo / workspace-of-workspaces. Single project root, like today.
