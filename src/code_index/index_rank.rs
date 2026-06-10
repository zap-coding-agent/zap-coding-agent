use anyhow::Result;
use rusqlite::params;

use super::CodeIndex;

impl CodeIndex {
    /// Rebuild `file_rank` from the call_sites + imports graph using PageRank.
    ///
    /// Edges are resolved by name: every call_site (caller_path, callee_name) becomes
    /// fractional edges to all files defining a symbol with that name. Same for imports.
    /// Damping = 0.85, iterations = 25, classic PageRank. Pure in-memory; no extra deps.
    pub fn compute_file_ranks(&mut self) -> Result<usize> {
        // 1. Collect all indexed files (these are the nodes).
        let files: Vec<String> = self.conn
            .prepare("SELECT path FROM indexed_files")?
            .query_map([], |r| r.get(0))?
            .flatten()
            .collect();
        if files.is_empty() { return Ok(0); }

        let mut file_index: std::collections::HashMap<String, usize> =
            std::collections::HashMap::with_capacity(files.len());
        for (i, p) in files.iter().enumerate() {
            file_index.insert(p.clone(), i);
        }

        // 2. Build name → set<defining_file_idx> (case-insensitive resolution).
        let mut defs_by_name: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        {
            let mut stmt = self.conn.prepare("SELECT name, path FROM symbols")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows.flatten() {
                if let Some(&fi) = file_index.get(&row.1) {
                    let key = row.0.to_lowercase();
                    let entry = defs_by_name.entry(key).or_default();
                    if !entry.contains(&fi) { entry.push(fi); }
                }
            }
        }

        // 3. Build edge weights per (caller_idx, callee_idx).
        let mut edges: std::collections::HashMap<(usize, usize), f32> =
            std::collections::HashMap::new();

        // 3a. Call edges — import-aware: qualified calls are narrowed via the imports table.
        {
            let mut stmt = self.conn.prepare("SELECT path, name, qualifier FROM call_sites")?;
            let rows = stmt.query_map([], |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            )))?;
            for row in rows.flatten() {
                let (caller_path, name, qualifier) = row;
                let Some(&caller_idx) = file_index.get(&caller_path) else { continue };
                let key = name.to_lowercase();
                let Some(targets) = defs_by_name.get(&key) else { continue };
                if targets.is_empty() { continue }

                let narrowed: Vec<usize> = if qualifier.is_empty() {
                    targets.clone()
                } else {
                    // Keep only targets whose defining file has the callee imported with
                    // a module path that overlaps the qualifier.
                    let qual_lower = qualifier.to_lowercase();
                    targets.iter().copied().filter(|&t| {
                        let target_path = &files[t];
                        // check if caller_path imports name from a module matching qualifier
                        self.conn.query_row(
                            "SELECT 1 FROM imports im
                              WHERE im.path = ?1
                                AND im.imported_name = ?2 COLLATE NOCASE
                                AND (instr(lower(im.module), ?3) > 0
                                     OR instr(?3, lower(im.module)) > 0)
                              LIMIT 1",
                            rusqlite::params![caller_path, name, qual_lower],
                            |_| Ok(()),
                        ).is_ok()
                        // also accept if the target file's path itself contains the qualifier
                        || target_path.to_lowercase().contains(&qual_lower)
                    }).collect()
                };

                let effective = if narrowed.is_empty() { targets } else { &narrowed };
                if effective.is_empty() { continue }
                let share = 1.0_f32 / effective.len() as f32;
                for &t in effective {
                    if t == caller_idx { continue }
                    *edges.entry((caller_idx, t)).or_insert(0.0) += share;
                }
            }
        }

        // 3b. Import edges (weighted half — imports are weaker structural signal than calls).
        {
            let mut stmt = self.conn.prepare("SELECT path, imported_name FROM imports WHERE imported_name != ''")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows.flatten() {
                let Some(&caller_idx) = file_index.get(&row.0) else { continue };
                let key = row.1.to_lowercase();
                let Some(targets) = defs_by_name.get(&key) else { continue };
                if targets.is_empty() { continue }
                let share = 0.5_f32 / targets.len() as f32;
                for &t in targets {
                    if t == caller_idx { continue }
                    *edges.entry((caller_idx, t)).or_insert(0.0) += share;
                }
            }
        }

        // 4. Group outgoing edges by source for the PageRank loop.
        let mut out: Vec<Vec<(usize, f32)>> = vec![Vec::new(); files.len()];
        let mut out_weight: Vec<f32> = vec![0.0; files.len()];
        for ((u, v), w) in &edges {
            out[*u].push((*v, *w));
            out_weight[*u] += *w;
        }

        // 5. PageRank iterations.
        let n = files.len() as f32;
        let damping = 0.85_f32;
        let teleport = (1.0 - damping) / n;
        let mut rank = vec![1.0_f32 / n; files.len()];
        for _ in 0..25 {
            let mut new_rank = vec![teleport; files.len()];
            let mut dangling_mass = 0.0_f32;
            for (u, ow) in out_weight.iter().enumerate() {
                if *ow == 0.0 {
                    dangling_mass += rank[u];
                }
            }
            let dangling_share = damping * dangling_mass / n;
            for r in new_rank.iter_mut() { *r += dangling_share; }
            for (u, neighbors) in out.iter().enumerate() {
                let ow = out_weight[u];
                if ow == 0.0 { continue }
                let contrib = damping * rank[u] / ow;
                for &(v, w) in neighbors {
                    new_rank[v] += contrib * w;
                }
            }
            rank = new_rank;
        }

        // 6. Persist.
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM file_rank", [])?;
        for (i, p) in files.iter().enumerate() {
            tx.execute(
                "INSERT INTO file_rank (path, rank) VALUES (?1, ?2)",
                params![p, rank[i] as f64],
            )?;
        }
        tx.commit()?;
        Ok(files.len())
    }

    /// Top-N files by PageRank, descending. Returns `(path, rank)`.
    pub fn rank_files(&self, limit: usize) -> Result<Vec<(String, f32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, rank FROM file_rank ORDER BY rank DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map([limit as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)? as f32))
        })?.flatten().collect();
        Ok(rows)
    }

    /// Get rank for a single path (0.0 if unknown).
    pub fn file_rank(&self, path: &str) -> f32 {
        self.conn
            .query_row(
                "SELECT rank FROM file_rank WHERE path = ?1",
                params![path],
                |r| r.get::<_, f64>(0),
            )
            .map(|v| v as f32)
            .unwrap_or(0.0)
    }
}
