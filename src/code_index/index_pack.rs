use anyhow::Result;
use rusqlite::params;

use super::{CallSite, CodeIndex, Import, PackedContext, PackedItem, Symbol};
use super::walk::row_to_symbol;

/// Stopwords stripped from task text before keyword extraction. Very small list — code-relevant
/// terms like "fix", "render", "parser" are kept on purpose; only the most generic English glue is removed.
const TASK_STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "into", "that", "this", "these", "those",
    "what", "where", "when", "which", "how", "why", "who",
    "are", "was", "were", "been", "being",
    "has", "have", "had", "does", "did",
    "can", "should", "could", "would", "may", "might", "will",
    "you", "your", "they", "them",
];

fn task_keywords(task: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let push = |cur: &mut String, out: &mut Vec<String>| {
        if cur.len() >= 3 {
            let lower = cur.to_lowercase();
            if !TASK_STOPWORDS.contains(&lower.as_str()) && !out.contains(&lower) {
                out.push(lower);
            }
        }
        cur.clear();
    };
    for c in task.chars() {
        if c.is_ascii_alphanumeric() || c == '_' { cur.push(c); }
        else { push(&mut cur, &mut out); }
    }
    push(&mut cur, &mut out);
    out
}

fn approx_item_cost(it: &PackedItem) -> usize {
    // Roughly the display row length: path + line + name + signature + provenance + framing.
    it.path.len() + it.name.len() + it.signature.len() + it.provenance.len() + 24
}

impl CodeIndex {
    /// Curate a context bundle for a task within a token budget.
    /// v1 algorithm: keyword match → file scoring (matches × PageRank) → one-hop expansion via callers/importers
    /// → greedy-pack symbol signatures until budget. No file-body fetch in v1 (signatures only).
    pub fn pack_context(&self, task: &str, token_budget: usize) -> Result<PackedContext> {
        let budget_chars = token_budget.saturating_mul(4);
        let keywords = task_keywords(task);

        let mut ctx = PackedContext {
            task: task.to_string(),
            budget_chars,
            strategy: "keyword + rank + 1-hop".into(),
            ..Default::default()
        };
        if keywords.is_empty() { return Ok(ctx); }

        // 1. Score every matching symbol: token_hits × (1 + rank * 100).
        let mut symbol_hits: Vec<(f32, Symbol, usize)> = Vec::new();
        for kw in &keywords {
            let pattern = format!("%{}%", kw);
            let rows: Vec<Symbol> = self.conn
                .prepare("SELECT path, name, kind, line, signature, language, context
                           FROM symbols
                          WHERE name LIKE ?1
                          ORDER BY CASE WHEN name = ?2 COLLATE NOCASE THEN 0 ELSE 1 END
                          LIMIT 200")?
                .query_map(params![pattern, kw], row_to_symbol)?
                .flatten()
                .collect();
            for s in rows {
                let exact = s.name.eq_ignore_ascii_case(kw);
                let bump = if exact { 5.0 } else { 1.0 };
                if let Some(prev) = symbol_hits.iter_mut().find(|(_, ss, _)| ss.path == s.path && ss.name == s.name && ss.line == s.line) {
                    prev.0 += bump;
                    prev.2 += 1;
                } else {
                    symbol_hits.push((bump, s, 1));
                }
            }
        }

        // 2. Compute per-file score (sum of symbol scores × (1 + file_rank * 100)).
        let mut file_score: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
        for (sc, s, _) in &symbol_hits {
            let rank = self.file_rank(&s.path);
            let weighted = *sc * (1.0 + rank * 100.0);
            *file_score.entry(s.path.clone()).or_insert(0.0) += weighted;
        }

        // 3. One-hop expansion: for the top-scoring exact-name symbols, pull callers + importers.
        let mut top_symbols_for_expand: Vec<&Symbol> = symbol_hits.iter()
            .filter(|(_, s, _)| keywords.iter().any(|k| s.name.eq_ignore_ascii_case(k)))
            .map(|(_, s, _)| s)
            .collect();
        top_symbols_for_expand.sort_by(|a, b| {
            file_score.get(&b.path).cloned().unwrap_or(0.0)
                .partial_cmp(&file_score.get(&a.path).cloned().unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top_symbols_for_expand.truncate(10);

        let mut callers_by_target: std::collections::HashMap<String, Vec<CallSite>> = std::collections::HashMap::new();
        let mut importers_by_target: std::collections::HashMap<String, Vec<Import>> = std::collections::HashMap::new();
        for s in &top_symbols_for_expand {
            let callers = self.find_references(&s.name, 5).unwrap_or_default();
            for c in &callers {
                let rank = self.file_rank(&c.path);
                *file_score.entry(c.path.clone()).or_insert(0.0) += 2.0 * (1.0 + rank * 100.0);
            }
            callers_by_target.insert(s.name.clone(), callers);

            let importers = self.importers_of(&s.name).unwrap_or_default();
            for im in &importers {
                let rank = self.file_rank(&im.path);
                *file_score.entry(im.path.clone()).or_insert(0.0) += 1.0 * (1.0 + rank * 100.0);
            }
            importers_by_target.insert(s.name.clone(), importers);
        }

        // 4. Rank files by aggregate score.
        let mut ranked_files: Vec<(String, f32)> = file_score.into_iter().collect();
        ranked_files.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // 5. Greedy pack — symbols from each top file, respecting budget.
        let mut used = 0usize;
        let mut seen_keys: std::collections::HashSet<(String, usize)> = std::collections::HashSet::new();

        for (path, _score) in &ranked_files {
            if used >= budget_chars { break }
            let matched_in_file: Vec<&(f32, Symbol, usize)> = symbol_hits.iter()
                .filter(|(_, s, _)| &s.path == path)
                .collect();

            for (_, s, hits) in matched_in_file {
                let key = (s.path.clone(), s.line);
                if seen_keys.contains(&key) { continue }
                let item = PackedItem {
                    path: s.path.clone(),
                    line: s.line,
                    kind: s.kind.clone(),
                    name: s.name.clone(),
                    signature: s.signature.clone(),
                    provenance: format!("name match · {} hit(s)", hits),
                };
                let cost = approx_item_cost(&item);
                if used + cost > budget_chars { break }
                used += cost;
                seen_keys.insert(key);
                ctx.items.push(item);
            }
        }

        // 6. Caller/importer breadcrumbs from the expansion.
        for (target_name, callers) in &callers_by_target {
            if used >= budget_chars { break }
            for c in callers {
                let key = (c.path.clone(), c.line);
                if seen_keys.contains(&key) { continue }
                let item = PackedItem {
                    path:       c.path.clone(),
                    line:       c.line,
                    kind:       "call".into(),
                    name:       target_name.clone(),
                    signature:  if c.qualifier.is_empty() && c.receiver_expr.is_empty() {
                                    format!("{}(...)", target_name)
                                } else if !c.qualifier.is_empty() {
                                    format!("{}::{}(...)", c.qualifier, target_name)
                                } else {
                                    format!("{}.{}(...)", c.receiver_expr, target_name)
                                },
                    provenance: format!("caller of {} [{}]", target_name,
                                        if c.caller_scope.is_empty() { "<top-level>".into() } else { c.caller_scope.clone() }),
                };
                let cost = approx_item_cost(&item);
                if used + cost > budget_chars { break }
                used += cost;
                seen_keys.insert(key);
                ctx.items.push(item);
            }
        }

        for (target_name, importers) in &importers_by_target {
            if used >= budget_chars { break }
            for im in importers {
                let key = (im.path.clone(), im.line);
                if seen_keys.contains(&key) { continue }
                let item = PackedItem {
                    path:       im.path.clone(),
                    line:       im.line,
                    kind:       "import".into(),
                    name:       im.imported_name.clone(),
                    signature:  if im.alias.is_empty() {
                                    format!("use {}::{}", im.module, im.imported_name)
                                } else {
                                    format!("use {}::{} as {}", im.module, im.imported_name, im.alias)
                                },
                    provenance: format!("importer of {}", target_name),
                };
                let cost = approx_item_cost(&item);
                if used + cost > budget_chars { break }
                used += cost;
                seen_keys.insert(key);
                ctx.items.push(item);
            }
        }

        // Stable display order: by path, then line.
        ctx.items.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));
        ctx.total_chars = used;
        Ok(ctx)
    }
}
