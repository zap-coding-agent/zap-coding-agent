# Code Graph: Decisions and Research

Captures the reasoning behind zap's upgrade from a symbols-only AST index (B-tier) to a symbol + call_sites + imports graph (A-tier+) — and the deliberate choices not to chase LSP-grade type resolution or 159-language coverage.

Companion to [`.zap/specs/code-graph.md`](../.zap/specs/code-graph.md) (the implementation spec). Read this for *why*; read the spec for *what*.

Date: 2026-06-10

---

## Where we started

Before this work, the index was solid B-tier:

- Tree-sitter symbol extraction for Rust, Python, JS/TS, Go, Java, C#
- SQLite store at `.zap/code.db` with two tables: `symbols`, `indexed_files`
- mtime-based incremental reindex (foreground + background tokio task)
- `ref_count` column populated by a text-level lexer (`count_call_sites`) that counted occurrences of `identifier(` across all files, ignoring strings and comments

The `ref_count` was a popularity score, not a call graph. We could answer "is this symbol used a lot?" but not "*who* calls it" or "*where* does this file pull from?" That's a meaningful gap for an AI coding agent.

## The market we benchmarked against

Four points on the spectrum, ordered by structural depth:

| Tier | Examples | What they have |
|---|---|---|
| **S (semantic)** | Serena, Sourcegraph SCIP, rust-analyzer/gopls under LSP-MCP, **codebase-memory-mcp** | Real type resolution, cross-file inference, "go to definition" actually correct |
| **A (graph)** | Aider RepoMap, Codanna | Tree-sitter symbols + call graph + PageRank-style ranking |
| **B (symbols)** | Most "code memory" MCP servers, Cline, Continue's chunker | Tree-sitter symbols only, no edges |
| **C (text)** | grep wrappers, glob+regex | No structure |
| **Orthogonal** | Cursor codebase, Continue embeddings | Vector RAG — fuzzy, not structural |

The clearest reference point for "world-class" right now is **codebase-memory-mcp**. Things it has that we don't:

- 159 languages via vendored tree-sitter grammars (we have 7)
- Hybrid LSP semantic analysis for Python, TS, JS, PHP, C#, Go, C, C++ (we have none)
- Bundled `nomic-embed-code` embeddings (768d int8) for semantic search
- Louvain community detection for module discovery
- Cross-service HTTP/gRPC/GraphQL edges
- Architecture extraction, ADR management
- Git diff impact mapping
- Indexes Linux kernel (28M LOC) in 3 minutes
- Single-binary, auto-installs into 11 agents

That's the bar. We chose not to chase all of it.

## The fork: build vs. plug in vs. both

Three honest options when zap needs better code intelligence:

**Plug in only** — speak MCP to codebase-memory-mcp (or LSP-bridge to Serena) and become a thin wrapper.
- Pro: instant 159-language coverage and type info
- Con: 10-50ms per query (process boundary + JSON serialization). Agents hit the index hundreds of times per turn. That latency is felt. Also: zap is no longer self-contained, and we ship someone else's UX as our agent's experience.

**Build only** — own everything.
- Pro: microsecond in-process queries, agent-shaped APIs, no external dependency
- Con: we will never catch SCIP on language breadth or LSP on type accuracy

**Both** — own the hot path, plug in the cold escalations.
- Pro: best of both worlds for the common case
- Con: more surface area to maintain

**Decision: build the graph in-house. Defer plug-in escalation until we have evidence we need it.**

Rationale:

1. **Latency is product**. zap is a TUI agent — fast feedback is the experience. In-process SQLite queries are microseconds; MCP roundtrips are 10-50ms. For tools the agent invokes per-turn (find_references, who_calls, imports_for), in-process wins.
2. **The hard layer is small for our top languages**. Rust/Python/JS/TS extractors for call_sites + imports are ~500 lines of tree-sitter code. One week of work, not one year.
3. **Type resolution is the trap**. It earns ~5-10% accuracy on ambiguous cases — not transformative. The complexity cost is 10x. We accept name-based ambiguity; the agent handles it fine because it reads files anyway.
4. **Agent-shaped APIs are the differentiator nobody ships**. Every existing tool exposes IDE primitives (`find_references`, `get_architecture`). None expose `pack_context(task, token_budget)` — "give me the curated 4K-token bundle to load for this task." This is what zap can uniquely own.
5. **Plug-in is additive, not foundational**. We can wire in codebase-memory-mcp later as an opt-in escalation. We can't unwire a foundation we built on top of it.

## Architectural choices

### Call sites as nodes, not edges

The intuitive design is an `edges (caller_id, callee_id)` table. We chose against it.

**Why:** edges go stale on every rename. Renaming `foo` to `bar` in file A means every `(*, foo_id)` edge from file B points to a definition that no longer exists. Cross-file invalidation logic is real complexity.

**What we did instead:** the `call_sites` table stores *names*, not symbol IDs. A row says "at this file:line, something named `foo` is called with qualifier `Bar` and receiver `x`." When the agent asks "who calls foo," we query call_sites by name. When `foo` is renamed, the call sites in file B correctly point to a name with no definition — which is the *right answer*, not staleness.

This means our update logic is trivial:

```sql
DELETE FROM symbols    WHERE path = ?
DELETE FROM call_sites WHERE path = ?
DELETE FROM imports    WHERE path = ?
-- then re-extract and re-insert from one tree-sitter pass
```

No cross-file invalidation. The mtime check already in place is sufficient. Same transaction, same hook point.

### Name-based, not type-resolved

We do not attempt to resolve `x.foo()` to a specific `Bar::foo` definition. This means:

- "Who calls `foo`" returns all call sites for any name `foo` (with qualifier filter as a refinement)
- Ambiguous cases (two different `foo`s in different modules) return both

This is the conscious tradeoff for not building rust-analyzer/gopls. The qualifier and receiver_expr columns let downstream code filter heuristically — e.g. "callers of `foo` where qualifier='Bar'" narrows correctly when the call site was written `Bar::foo(...)`.

### Schema additions

```sql
CREATE TABLE call_sites (
    id, path, line, col,
    name, qualifier, receiver_expr,  -- 'name' is the leaf; qualifier is everything left of the last separator
    caller_scope,                    -- enclosing fn/method context, mirrors the existing symbols.context format
    language
);
CREATE INDEX idx_cs_name      ON call_sites(name COLLATE NOCASE);
CREATE INDEX idx_cs_path      ON call_sites(path);
CREATE INDEX idx_cs_qualifier ON call_sites(qualifier);

CREATE TABLE imports (
    id, path, line,
    module,                          -- the path: 'std::collections', 'react', 'os'
    imported_name,                   -- the leaf name: 'HashMap', 'useState', '' for whole-module imports
    alias,
    language
);
CREATE INDEX idx_imp_path ON imports(path);
CREATE INDEX idx_imp_name ON imports(imported_name COLLATE NOCASE);
CREATE INDEX idx_imp_mod  ON imports(module);
```

Group imports (`use std::sync::{Arc, Mutex, OnceLock}` or `from os import path, sep`) are *flattened* — one row per leaf. Query simplicity over storage compactness.

### `ref_count` becomes derived

The text-scan `count_call_sites` was deleted. `compute_reference_counts` is now a single SQL aggregate:

```sql
UPDATE symbols
SET ref_count = (SELECT COUNT(*) FROM call_sites cs
                  WHERE cs.name = symbols.name COLLATE NOCASE)
```

Backwards compatible: the existing `quality_report` queries (high_coupling, dead_candidates) read `ref_count` unchanged. Numbers got slightly more accurate (text scan over-counted matches inside generics and type names).

### Noise filter

v1 drops:
- Single-char identifiers (`r`, `n`, etc.)
- Rust stdlib macros: `println`, `vec`, `format`, `assert*`, `dbg`, `panic`, `todo`, `unimplemented`, `unreachable`, `eprintln`, `print`, `write`, `writeln`, `include_*`, `env`, `cfg`, `matches`, `derive`, `concat`, `stringify`, `line`, `file`, `module_path`, `column`

Not filtered (deliberately): method names like `get`, `to_string`, `new`. Yes, they're noisy in raw counts — but a qualifier filter narrows them, and an agent asking "who calls `new`" probably means a specific type's `new`. We don't want to drop signal at index time.

### AST-only switch

`ZAP_INDEX_MODE=symbols` disables call_sites/imports emission. Default `graph`. Switching modes triggers a full reindex on next file change tick (because per-file DELETE+INSERT still runs; data accumulates in the appropriate tables only).

Use case: regression escape hatch. If a graph extractor breaks badly on someone's codebase, they can set the env var and fall back to B-tier behavior without rebuilding zap.

### What is *not* in the graph

Deliberate exclusions:

- **Type resolution**. `foo.bar()` where `foo: Box<dyn Trait>` stays unresolved.
- **Macro expansion**. Rust `println!` calls are visible as `macro_invocation`, but we don't expand them. `proc_macro` is fully opaque.
- **Cross-language edges**. TS calling into Rust via WASM/FFI is two separate graphs.
- **Re-exports** (`pub use foo::Bar`). Adds chase-the-pointer complexity. Skip in v1, revisit if `importers_of` accuracy hurts.
- **Generics monomorphization**. `Vec::<u32>::new()` and `Vec::<String>::new()` both record as `Vec::new`.
- **Embeddings/semantic search**. Orthogonal layer; planned for after phase 4.

## Phased rollout

| Phase | Scope | Status |
|---|---|---|
| 1. Extraction | call_sites + imports tables. Rust + Python + TS/JS extractors. Wire into `index_file`. Migrate `ref_count` to SQL aggregate. | **Shipped** |
| 2. Query APIs | `find_references`, `callers_of`, `imports_for`, `importers_of`, `users_of_module`. Agent tools: `find_references` (upgraded), `who_calls`, `file_imports`, `where_imported`. | **Shipped** |
| 3. Ranking | PageRank over the graph. `rank_files`, `file_rank`. Used in `find_references` ordering and `pack_context` scoring. | **Shipped** |
| 4. Pack context | `pack_context(task, budget)` with provenance. The agent-shaped differentiator — keyword match × PageRank × one-hop expansion to callers + importers, greedy-packed within budget. | **Shipped** |
| 5. Remaining langs | Go, Java, C# graph extractors. | **Shipped** |
| 4.5. Embeddings | Optional semantic layer (e.g. `nomic-embed-code` via `candle`/`ort` or local Ollama). Hybrid retrieval alongside structural graph. | Pending |
| 6. (Optional) LSP escalation | Only if name-based queries hit a wall. | Deferred indefinitely |

Phases 1–5 shipped this session. zap moved from B-tier (symbols only, text-scan reference popularity) to A-tier+ across all 7 supported languages — call graph, import graph, PageRank-ranked file importance, plus the `pack_context` agent-shaped layer no public MCP server currently ships. The only deliberate remaining gap to S-tier is the embeddings layer (4.5) and full type resolution (deferred via LSP escalation).

## Verification on the zap codebase itself

After full phase 1-5 reindex of the zap repo:

- 264 files indexed
- 4,294 symbols
- 20,272 call sites
- 2,064 imports
- 264 files ranked (PageRank)

Spot-checks (phase 1-2 graph queries):

- `find_references('extract_all')` → 1 hit at [src/code_index/index_impl.rs:160](../src/code_index/index_impl.rs#L160), caller_scope = `impl CodeIndex · index_file`. Correct.
- `callers_of('write', qualifier='crate::log')` → 20 hits. Bare `find_references('write')` returns 51 — the qualifier narrows precisely.
- `imports_for(src/code_index/index_impl.rs)` → 13 rows. `use super::{graph_enabled, CodeIndex, QualityReport, Symbol}` correctly flattened to 4 rows with module=`super`.
- `importers_of('HashMap')` → 6 files, all importing from `std::collections`.

Spot-checks (phase 3 PageRank):

- Top-ranked files reflect actual centrality: flask's `scaffold.py`, `ctx.py`, `app.py` (heavily-imported across the flask demos), then zap's own `tools/todo.rs` and `llm_client/credentials.rs` (called from many sites).
- Call sites in high-rank files surface before low-rank files in `find_references` ordering.

Spot-checks (phase 4 pack_context):

- For task "pagerank ranking and find_references", candidate symbols include `find_references`, `compute_file_ranks`, `rank_files`, `file_rank` — all in `src/code_index/index_impl.rs`, the highest-ranked file containing them. Symbols in `src/code_index/mod.rs` (the `global_*` wrappers) come next at lower rank. Off-topic `rank_and_truncate_skills` is correctly demoted by file_rank.

Spot-checks (phase 5 Go/Java/C# extraction, run on synthetic fixtures):

- **Go** `import log "github.com/sirupsen/logrus"` → captured with `alias=log`. Call `log.Info(...)` resolved to `qualifier=log, name=Info`. Method calls (`s.Start()`) get receiver in `receiver_expr`.
- **Java** `new AtomicInteger(0)` → recorded as call with `qualifier=new, name=AtomicInteger` (useful for "who constructs X" queries). Wildcard imports `import com.example.util.*` correctly marked `imported_name=*`.
- **C#** `Console.WriteLine(...)` → `qualifier=Console, name=WriteLine`. `new Program()` → constructor call. `using System.Collections.Generic` captured as module-level import.

Full test suite (159 tests) passes against the new index.

## When to revisit this decision

We chose not to chase LSP/type resolution. Triggers that would force a re-evaluation:

1. **Agent feedback says "wrong target"** repeatedly on graph queries. If `who_calls('parse')` returns 30 unrelated parses and the agent can't disambiguate even with qualifier hints, name-based is failing.
2. **The 7-language ceiling becomes a constraint.** If users routinely work in Kotlin, Swift, Ruby, PHP, etc., and our index returns "no graph data" for half their files.
3. **`pack_context` quality is bad** because we can't tell which symbol the agent actually meant.

In those cases, the plug-in escalation path becomes the right move: keep our in-process graph for hot queries, route ambiguous cases to an LSP-backed second tier (codebase-memory-mcp or a direct LSP integration).

Until then, we ship our way to the agent-shaped layer no one else has.
