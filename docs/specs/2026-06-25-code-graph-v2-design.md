# Code Graph v2 — Design Spec

**Goal:** Close four structural gaps in the code graph index while respecting the existing name-based, no-LSP architecture.

**Status:** Draft
**Date:** 2026-06-25

---

## Context

The current code graph (shipped in phases 1–5, documented in `docs/code-graph-decisions.md`) is A-tier: tree-sitter symbols + call sites + imports + PageRank + `pack_context`. It deliberately avoids type resolution (S-tier is deferred indefinitely) because "type resolution earns ~5-10% accuracy on ambiguous cases — not transformative. The complexity cost is 10x."

Four improvements can be made within the name-based architecture:

---

## Improvement 1: Import-Aware Call Resolution

### Problem

`compute_reference_counts` and PageRank edge-building match calls to definitions by **name only** (case-insensitive). If `src/auth.rs` and `src/payment.rs` both define `validate()`, a call `crate::auth::validate(...)` distributes ref_count and PageRank weight to both files equally — even though the qualifier clearly points to one of them.

The `imports` table already maps every file to the modules it pulls from. We can cross-reference call qualifiers against imports to narrow which file's definition a call actually targets.

### Design

**New query: `resolve_call(defining_path, callee_name, qualifier)`**

Given a call site's qualifier (e.g. `crate::auth`), look up which files import the name `callee_name` from a module matching that qualifier. Return the defining file(s) that actually provide the symbol.

```
Resolution algorithm:
  1. If qualifier is empty → fall back to name-only match (existing behavior)
  2. Normalise qualifier to module-path form (e.g. "crate::auth" → impl matching on imports.module)
  3. For each importing file, check: does it define a symbol with name = callee_name?
  4. Return the intersection: (files that import X from module matching qualifier) ∩ (files that define X)
```

**Changes to `compute_reference_counts`:**

```sql
-- Current (name-only):
UPDATE symbols SET ref_count = (
    SELECT COUNT(*) FROM call_sites cs
    WHERE cs.name = symbols.name COLLATE NOCASE
)

-- New (import-aware):
UPDATE symbols SET ref_count = (
    SELECT COUNT(*) FROM call_sites cs
    WHERE cs.name = symbols.name COLLATE NOCASE
      AND (
        cs.qualifier = ''  -- unqualified calls → name-only match
        OR EXISTS (
          SELECT 1 FROM imports im
          WHERE im.path = cs.path
            AND im.imported_name = symbols.name COLLATE NOCASE
            AND (instr(im.module, cs.qualifier) > 0 OR instr(cs.qualifier, im.module) > 0)
        )
      )
)
```

**Changes to PageRank edge-building (`compute_file_ranks`):**

Same import-cross-referencing logic when distributing edge weight. Only split weight to files that actually import the callee from a module matching the qualifier.

**New public API:**

- `resolve_call(callee_name: &str, qualifier: &str) -> Vec<String>` — returns defining file paths

**Impact:**
- `ref_count` becomes per-actual-definition instead of per-name-match for qualified calls
- PageRank edges become more precise (fewer false targets)
- `find_references` ordering improves because PageRank is more accurate

---

## Improvement 2: Type Hierarchy Extraction

### Problem

No class/interface/trait hierarchy is tracked. You can't ask "what extends `BaseHandler`" or "what implements `Serializer`". For OOP-heavy codebases this is a significant structural blind spot.

### Design

**New table: `type_edges`**

```sql
CREATE TABLE IF NOT EXISTS type_edges (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    child_path  TEXT NOT NULL,
    child_name  TEXT NOT NULL,
    parent_name TEXT NOT NULL,      -- base class / trait / interface name
    edge_kind   TEXT NOT NULL,      -- "extends", "implements", "mixin"
    line        INTEGER NOT NULL,
    language    TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_te_child  ON type_edges(child_path, child_name);
CREATE INDEX IF NOT EXISTS idx_te_parent ON type_edges(parent_name COLLATE NOCASE);
```

**Per-language extraction:**

| Language | AST node | Edge extracted | edge_kind |
|----------|----------|---------------|-----------|
| Python | `class_definition` → `superclasses` argument list | `class Foo(Bar, Baz)` → edges Foo→Bar, Foo→Baz | `extends` |
| Rust | `impl_item` with `trait` keyword | `impl Serialize for Foo` → edge Foo→Serialize | `implements` |
| Java | `class_declaration` → `superclass`, `super_interfaces` | `class Foo extends Bar implements Baz` → edges Foo→Bar, Foo→Baz | `extends`, `implements` |
| C# | `class_declaration` → `base_list` | `class Foo : Bar, IBaz` → edges Foo→Bar, Foo→IBaz | `extends`, `implements` |
| JS/TS | `class_declaration` → `class_heritage` | `class Foo extends Bar` → edge Foo→Bar | `extends` |
| Go | No class inheritance. Embedded structs are implicit. | Skip for v2. | — |

**New public APIs:**

- `find_subtypes_of(parent_name: &str) -> Vec<TypeEdge>` — all classes that extend/implement a given type
- `find_supertypes_of(child_name: &str) -> Vec<TypeEdge>` — all types a class extends/implements
- Type edges included in `pack_context` one-hop expansion

**New column: `context` on type_edges**

Mirrors symbols.context: `"class module.Foo"` or `"impl Foo"`. Useful for disambiguation.

**Impact:**
- Agent can ask "what are all subclasses of BaseHandler" and get a structural answer
- `pack_context` can pull in subtypes when a base class matches a keyword
- PageRank can optionally use type edges as weak structural signals (deferred to v2.1)

---

## Improvement 3: Structured Signatures

### Problem

Signatures are stored as opaque truncated text. You can LIKE-search them, but you can't query structurally — "all functions returning `Result<T, E>`" or "all methods taking `&mut self`".

### Design

**New columns on `symbols` table:**

```sql
ALTER TABLE symbols ADD COLUMN return_type TEXT NOT NULL DEFAULT '';
ALTER TABLE symbols ADD COLUMN params TEXT NOT NULL DEFAULT '';  -- JSON array of strings
```

**Per-language parsing:**

**Rust:** `fn foo(x: u32, y: &str) -> Result<Bar, E>`
- `return_type` = `"Result<Bar, E>"`
- `params` = `["x: u32", "y: &str"]`

**Python:** `def foo(x: int, y: str = "default") -> Optional[Bar]:`
- `return_type` = `"Optional[Bar]"`
- `params` = `["x: int", "y: str"]`

**JS/TS:** `function foo(x: number, y: string): Promise<Bar>`
- `return_type` = `"Promise<Bar>"`
- `params` = `["x: number", "y: string"]`

**Java:** `public Bar foo(int x, String y)`
- `return_type` = `"Bar"`
- `params` = `["int x", "String y"]`

**C#:** `public Bar Foo(int x, string y)`
- `return_type` = `"Bar"`
- `params` = `["int x", "string y"]`

**Go:** `func Foo(x int, y string) (*Bar, error)`
- `return_type` = `"(*Bar, error)"`
- `params` = `["x int", "y string"]`

**Parsing strategy:** Tree-sitter already gives parameter and return-type child nodes for most languages. Use `child_by_field_name("parameters")` and `child_by_field_name("return_type")` or equivalent. Fall back to the existing opaque truncation if parsing fails for any reason.

**Backward compatibility:** The `signature` column is preserved unchanged. `return_type` and `params` default to empty string for existing rows and for unparseable signatures.

**New public API:**

- `find_by_return_type(type_name: &str) -> Vec<Symbol>` — all functions returning a given type

**Impact:**
- Structural queries become possible without LSP
- `pack_context` can score symbols by return type keyword match
- `return_type` and `params` are exposed in `PackedItem` for richer context

---

## Improvement 4: Budget-Proportional pack_context Expansion

### Problem

`pack_context` truncates one-hop expansion to exactly 10 symbols regardless of budget. A 12K-token budget request gets the same expansion as a 2K-token request. Larger budgets should get more caller/importer breadcrumbs.

### Design

Replace the hardcoded `truncate(10)`:

```rust
// Current:
top_symbols_for_expand.truncate(10);

// New:
let expansion_limit = (budget_chars / 200).max(10).min(50);
top_symbols_for_expand.truncate(expansion_limit);
```

**Formula:** 1 expandable symbol per ~200 chars of budget. Minimum 10, maximum 50 to prevent runaway expansion.

At typical budgets:
- 2,000 chars → 10 symbols (minimum)
- 4,000 chars → 20 symbols
- 8,000 chars → 40 symbols
- 12,000+ chars → 50 symbols (maximum)

**Impact:**
- Larger context requests get proportionally richer caller/importer breadcrumbs
- No change to the greedy-packing algorithm itself
- Max cap (50) prevents degenerate cases on huge budgets

---

## Schema Migration

All changes are additive — no data loss on re-index:

1. New table: `type_edges` (created in migration, populated on next index)
2. New columns on `symbols`: `return_type TEXT NOT NULL DEFAULT ''`, `params TEXT NOT NULL DEFAULT ''`
3. Existing tables unchanged

Migration runs on `CodeIndex::open()` — detected by checking if `type_edges` table exists.

---

## File Changes

| File | Change |
|------|--------|
| `src/code_index/mod.rs` | Add `TypeEdge` struct, `RawTypeEdge`, wire into `ExtractResult` and `index_file` |
| `src/code_index/extract.rs` | Add `RawTypeEdge` struct, wire into `extract_all` return |
| `src/code_index/extract_python.rs` | Extract parent classes from `class_definition` superclasses |
| `src/code_index/extract_rust.rs` | Extract `impl Trait for Type` as type edge; parse return_type + params from `function_item` |
| `src/code_index/extract_js.rs` | Extract class heritage; parse return_type + params for function_declaration |
| `src/code_index/extract_java.rs` | Extract extends/implements; parse return_type + params |
| `src/code_index/extract_csharp.rs` | Extract base_list; parse return_type + params |
| `src/code_index/extract_go.rs` | Parse return_type + params from function_declaration |
| `src/code_index/index_impl.rs` | Schema migration; import-aware `compute_reference_counts`; store type_edges + structured sigs |
| `src/code_index/index_query.rs` | New queries: `resolve_call`, `find_subtypes_of`, `find_supertypes_of`, `find_by_return_type` |
| `src/code_index/index_rank.rs` | Import-aware PageRank edge building |
| `src/code_index/index_pack.rs` | Budget-proportional expansion; include type edges in one-hop |
| `src/tools/code_index_tools.rs` | Wire new queries as agent tools |
| Tests | Integration tests for new extractors + queries |

---

## What Is NOT Included

- **Full type resolution** (LSP-grade "which exact definition does this call target"). Remains deferred. Type resolution is 10x complexity for 5-10% accuracy gain on ambiguous cases.
- **Module dependency graph** (Gap 4). The import-based narrowing in Improvement 1 provides most of the practical value. Full module-level transitive dependency analysis is lower ROI.
- **Incremental tree-sitter parsing** (Gap 6). The DELETE+INSERT per-file pattern makes incremental CST reuse complex. Only worth pursuing if profiling shows indexing time is a bottleneck.

---

## Success Criteria

1. `compute_reference_counts` produces per-definition counts for qualified calls (not inflated by name collisions)
2. `find_subtypes_of("BaseHandler")` returns all Python classes that extend `BaseHandler`
3. `find_by_return_type("Result")` returns all Rust functions returning `Result`
4. `pack_context` with 8000-char budget pulls caller/importer breadcrumbs for ~40 symbols
5. All 159+ existing tests pass; new tests for each extractor and query
6. Re-indexing the zap codebase produces correct type edges + structured signatures
