---
name: sql
trigger: ["sql", "select ", "insert into", "create table", "postgres", "postgresql", "mysql", "sqlite", "migration", "query", "index", "schema", "join ", "where ", "group by", "stored procedure", "orm"]
tokens: ~590
---

## SQL / database conventions

**Source:** Use The Index, Luke (use-the-index-luke.com), PostgreSQL docs, SQL antipatterns (Karwin).

**Schema design:**
- Name tables as plural nouns (`users`, `orders`). Columns as singular snake_case.
- Every table gets a surrogate primary key (`id bigserial` / `uuid`). Avoid composite PKs as FKs — they proliferate.
- Add `created_at` / `updated_at` timestamp columns to every table.
- Foreign keys always indexed. Columns frequently used in `WHERE`/`JOIN`/`ORDER BY` need indexes.
- Use `NOT NULL` by default; nullable only when absence is meaningful.

**Queries:**
- Never `SELECT *` in production code — list columns explicitly.
- Use CTEs (`WITH expr AS (...)`) for readability over deeply nested subqueries.
- Avoid functions on indexed columns in `WHERE` (`WHERE YEAR(created_at) = 2024` disables the index; use a range instead).
- Prefer set-based operations over row-by-row cursors/loops.
- Always use parameterised queries / prepared statements — never string interpolation (SQL injection).

**Indexes:**
- B-tree indexes for equality and range. GIN for JSONB, full-text. GiST for geometric/ranges.
- Composite index column order: equality first, then range, then sort.
- Partial indexes (`WHERE active = true`) for filtered queries.
- Monitor bloat — `REINDEX CONCURRENTLY` in Postgres.

**Migrations:**
- One concern per migration. Keep them small and reversible (`up`/`down`).
- Never modify a migration already run in production — add a new one.
- Long-running migrations (adding columns to large tables): use `ADD COLUMN` with a default carefully; in PG 11+ it's instant for non-volatile defaults.

**Transactions:** Wrap related mutations. Keep transactions short — long transactions hold locks. Use `SERIALIZABLE` isolation only when you need it (has performance cost).

**Performance:** `EXPLAIN ANALYZE` before shipping slow queries. Look for Seq Scans on large tables and Hash Joins on unindexed FKs. Use connection pooling (PgBouncer for Postgres).
