use anyhow::Result;
use rusqlite::params;
use std::path::Path;

use super::{CallSite, CodeIndex, Import, Symbol};
use super::walk::{row_to_call_site, row_to_import, row_to_symbol};

impl CodeIndex {
    pub fn find_definition(&self, name: &str) -> Result<Vec<Symbol>> {
        let exact: Vec<Symbol> = self.conn
            .prepare("SELECT path, name, kind, line, signature, language, context
                       FROM symbols WHERE name = ?1 COLLATE NOCASE LIMIT 20")?
            .query_map(params![name], row_to_symbol)?
            .flatten()
            .collect();
        if !exact.is_empty() { return Ok(exact); }

        let pattern = format!("{}%", name);
        let results: Vec<Symbol> = self.conn
            .prepare("SELECT path, name, kind, line, signature, language, context
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
                .prepare("SELECT path, name, kind, line, signature, language, context
                           FROM symbols WHERE path LIKE ?1 ORDER BY path, line LIMIT 2000")?
                .query_map(params![pattern], row_to_symbol)?
                .flatten()
                .collect()
        } else {
            self.conn
                .prepare("SELECT path, name, kind, line, signature, language, context
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
            .prepare("SELECT path, name, kind, line, signature, language, context
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
}
