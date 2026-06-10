use anyhow::Result;
use rusqlite::params;
use std::path::Path;

use super::{CallSite, CodeIndex, Import, Symbol, TypeEdge};
use super::walk::{row_to_call_site, row_to_import, row_to_symbol};

fn row_to_type_edge(row: &rusqlite::Row) -> rusqlite::Result<TypeEdge> {
    Ok(TypeEdge {
        id:          row.get(0)?,
        child_path:  row.get(1)?,
        child_name:  row.get(2)?,
        parent_name: row.get(3)?,
        edge_kind:   row.get(4)?,
        line:        row.get::<_, i64>(5)? as usize,
        language:    row.get(6)?,
    })
}

impl CodeIndex {
    pub fn find_definition(&self, name: &str) -> Result<Vec<Symbol>> {
        let exact: Vec<Symbol> = self.conn
            .prepare("SELECT path, name, kind, line, signature, language, context, return_type, params
                       FROM symbols WHERE name = ?1 COLLATE NOCASE LIMIT 20")?
            .query_map(params![name], row_to_symbol)?
            .flatten()
            .collect();
        if !exact.is_empty() { return Ok(exact); }

        let pattern = format!("{}%", name);
        let results: Vec<Symbol> = self.conn
            .prepare("SELECT path, name, kind, line, signature, language, context, return_type, params
                       FROM symbols WHERE name LIKE ?1 LIMIT 20")?
            .query_map(params![pattern], row_to_symbol)?
            .flatten()
            .collect();
        Ok(results)
    }

    pub fn symbols_in_path(&self, path: &str) -> Result<Vec<Symbol>> {
        let pattern = if path.ends_with('/') || Path::new(path).is_dir() {
            format!("{}%", path.trim_end_matches('/'))
        } else {
            path.to_string()
        };

        let results: Vec<Symbol> = if pattern.contains('%') {
            self.conn
                .prepare("SELECT path, name, kind, line, signature, language, context, return_type, params
                           FROM symbols WHERE path LIKE ?1 ORDER BY path, line LIMIT 2000")?
                .query_map(params![pattern], row_to_symbol)?
                .flatten()
                .collect()
        } else {
            self.conn
                .prepare("SELECT path, name, kind, line, signature, language, context, return_type, params
                           FROM symbols WHERE path = ?1 ORDER BY line LIMIT 2000")?
                .query_map(params![pattern], row_to_symbol)?
                .flatten()
                .collect()
        };
        Ok(results)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Symbol>> {
        let pattern = format!("%{}%", query);
        let results: Vec<Symbol> = self.conn
            .prepare("SELECT path, name, kind, line, signature, language, context, return_type, params
                       FROM symbols
                       WHERE name LIKE ?1 OR signature LIKE ?1
                       ORDER BY CASE WHEN name LIKE ?2 THEN 0 ELSE 1 END, name
                       LIMIT ?3")?
            .query_map(params![pattern, format!("{}%", query), limit as i64], row_to_symbol)?
            .flatten()
            .collect();
        Ok(results)
    }

    /// All call sites that reference `name` (case-insensitive). The headline graph query.
    /// Results are ordered by the caller file's PageRank when available — load-bearing
    /// files first — falling back to path/line order when ranks aren't computed yet.
    pub fn find_references(&self, name: &str, limit: usize) -> Result<Vec<CallSite>> {
        let raw: Vec<CallSite> = self.conn
            .prepare("SELECT path, line, col, name, qualifier, receiver_expr, caller_scope, language
                       FROM call_sites
                      WHERE name = ?1 COLLATE NOCASE
                      ORDER BY path, line
                      LIMIT ?2")?
            .query_map(params![name, (limit * 2).max(limit) as i64], row_to_call_site)?
            .flatten()
            .collect();

        let mut with_rank: Vec<(f32, CallSite)> = raw.into_iter()
            .map(|cs| (self.file_rank(&cs.path), cs))
            .collect();
        with_rank.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.path.cmp(&b.1.path))
            .then_with(|| a.1.line.cmp(&b.1.line)));
        with_rank.truncate(limit);
        Ok(with_rank.into_iter().map(|(_, cs)| cs).collect())
    }

    /// References narrowed by qualifier — e.g. only `Bar::foo` instead of every `foo` call.
    /// Pass `qualifier=None` to match any qualifier; pass `Some("")` to match bare/unqualified calls only.
    pub fn callers_of(&self, name: &str, qualifier: Option<&str>, limit: usize) -> Result<Vec<CallSite>> {
        let results: Vec<CallSite> = match qualifier {
            None => self.find_references(name, limit)?,
            Some(q) => self.conn
                .prepare("SELECT path, line, col, name, qualifier, receiver_expr, caller_scope, language
                           FROM call_sites
                          WHERE name = ?1 COLLATE NOCASE AND qualifier = ?2
                          ORDER BY path, line
                          LIMIT ?3")?
                .query_map(params![name, q, limit as i64], row_to_call_site)?
                .flatten()
                .collect(),
        };
        Ok(results)
    }

    /// What does this file pull in.
    pub fn imports_for(&self, path: &str) -> Result<Vec<Import>> {
        let results: Vec<Import> = self.conn
            .prepare("SELECT path, line, module, imported_name, alias, language
                       FROM imports
                      WHERE path = ?1
                      ORDER BY line")?
            .query_map(params![path], row_to_import)?
            .flatten()
            .collect();
        Ok(results)
    }

    /// Which files import this name — blast radius for a rename.
    pub fn importers_of(&self, name: &str) -> Result<Vec<Import>> {
        let results: Vec<Import> = self.conn
            .prepare("SELECT path, line, module, imported_name, alias, language
                       FROM imports
                      WHERE imported_name = ?1 COLLATE NOCASE OR alias = ?1 COLLATE NOCASE
                      ORDER BY path, line")?
            .query_map(params![name], row_to_import)?
            .flatten()
            .collect();
        Ok(results)
    }

    /// Files that import from a specific module (e.g. `crate::util`, `react`).
    pub fn users_of_module(&self, module: &str) -> Result<Vec<Import>> {
        let pattern = format!("{}%", module);
        let results: Vec<Import> = self.conn
            .prepare("SELECT path, line, module, imported_name, alias, language
                       FROM imports
                      WHERE module = ?1 OR module LIKE ?2
                      ORDER BY path, line")?
            .query_map(params![module, pattern], row_to_import)?
            .flatten()
            .collect();
        Ok(results)
    }

    /// All types that extend or implement `parent_name`.
    pub fn find_subtypes_of(&self, parent_name: &str) -> Result<Vec<TypeEdge>> {
        let results: Vec<TypeEdge> = self.conn
            .prepare("SELECT id, child_path, child_name, parent_name, edge_kind, line, language
                       FROM type_edges
                      WHERE parent_name = ?1 COLLATE NOCASE
                      ORDER BY child_path, child_name")?
            .query_map(params![parent_name], row_to_type_edge)?
            .flatten()
            .collect();
        Ok(results)
    }

    /// All types that `child_name` extends or implements.
    pub fn find_supertypes_of(&self, child_name: &str) -> Result<Vec<TypeEdge>> {
        let results: Vec<TypeEdge> = self.conn
            .prepare("SELECT id, child_path, child_name, parent_name, edge_kind, line, language
                       FROM type_edges
                      WHERE child_name = ?1 COLLATE NOCASE
                      ORDER BY child_path, line")?
            .query_map(params![child_name], row_to_type_edge)?
            .flatten()
            .collect();
        Ok(results)
    }

    /// All functions/methods returning a type whose name contains `type_name`.
    pub fn find_by_return_type(&self, type_name: &str) -> Result<Vec<Symbol>> {
        let pattern = format!("%{}%", type_name);
        let results: Vec<Symbol> = self.conn
            .prepare("SELECT path, name, kind, line, signature, language, context, return_type, params
                       FROM symbols
                      WHERE return_type LIKE ?1
                        AND kind IN ('fn', 'method', 'function', 'func', 'def', 'async fn')
                      ORDER BY path, line
                      LIMIT 200")?
            .query_map(params![pattern], row_to_symbol)?
            .flatten()
            .collect();
        Ok(results)
    }

    /// Resolve which defining files a qualified call most likely targets.
    /// Returns the defining file path(s) that match both name and module qualifier.
    pub fn resolve_call(&self, callee_name: &str, qualifier: &str) -> Result<Vec<String>> {
        if qualifier.is_empty() {
            // Unqualified — return all files defining this name.
            let paths: Vec<String> = self.conn
                .prepare("SELECT DISTINCT path FROM symbols WHERE name = ?1 COLLATE NOCASE LIMIT 20")?
                .query_map(params![callee_name], |r| r.get(0))?
                .flatten()
                .collect();
            return Ok(paths);
        }

        let qual_lower = qualifier.to_lowercase();
        // Find files that both define `callee_name` and are reachable via an import whose
        // module overlaps with `qualifier`.
        let paths: Vec<String> = self.conn
            .prepare("SELECT DISTINCT s.path
                       FROM symbols s
                       JOIN imports im ON im.imported_name = s.name COLLATE NOCASE
                      WHERE s.name = ?1 COLLATE NOCASE
                        AND (instr(lower(im.module), ?2) > 0 OR instr(?2, lower(im.module)) > 0)
                      LIMIT 20")?
            .query_map(params![callee_name, qual_lower], |r| r.get(0))?
            .flatten()
            .collect();

        if paths.is_empty() {
            // No import match — fall back to path-contains-qualifier heuristic.
            let all_paths: Vec<String> = self.conn
                .prepare("SELECT DISTINCT path FROM symbols WHERE name = ?1 COLLATE NOCASE LIMIT 20")?
                .query_map(params![callee_name], |r| r.get(0))?
                .flatten()
                .collect();
            let filtered: Vec<String> = all_paths.into_iter()
                .filter(|p| p.to_lowercase().contains(&qual_lower))
                .collect();
            return Ok(filtered);
        }
        Ok(paths)
    }
}
