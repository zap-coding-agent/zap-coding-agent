use anyhow::Result;

use super::{CodeIndex, QualityReport};

impl CodeIndex {
    pub fn quality_report(&self) -> Result<QualityReport> {
        let (total_files, total_syms) = self.total_stats().unwrap_or((0, 0));

        let mut stmt = self.conn.prepare(
            "SELECT context, COUNT(*) as n, MAX(path) as path \
             FROM symbols WHERE kind='fn' AND context != '' \
             GROUP BY context HAVING n > 15 ORDER BY n DESC LIMIT 10"
        )?;
        let god_objects: Vec<(String, usize, String)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize, r.get::<_, String>(2)?))
        })?.flatten().collect();
        drop(stmt);

        let mut stmt = self.conn.prepare(
            "SELECT path, symbol_count FROM indexed_files WHERE symbol_count > 50 \
             ORDER BY symbol_count DESC LIMIT 8"
        )?;
        let large_files: Vec<(String, usize)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?.flatten().collect();
        drop(stmt);

        let mut stmt = self.conn.prepare(
            "SELECT name, MIN(path) as path, MIN(line) as line, ref_count \
             FROM symbols WHERE kind='fn' AND ref_count > 5 AND LENGTH(name) >= 5 \
             GROUP BY name HAVING COUNT(*) = 1 \
             ORDER BY ref_count DESC LIMIT 10"
        )?;
        let high_coupling: Vec<(String, String, usize, usize)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? as usize, r.get::<_, i64>(3)? as usize))
        })?.flatten().collect();
        drop(stmt);

        let mut stmt = self.conn.prepare(
            "SELECT name, path, line FROM symbols \
             WHERE kind='fn' AND signature LIKE 'pub %' \
             AND (ref_count = 0 OR ref_count = 1) \
             AND name NOT IN ('main','new','default','from','into','clone','fmt','drop') \
             ORDER BY path LIMIT 15"
        )?;
        let dead_candidates: Vec<(String, String, usize)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)? as usize))
        })?.flatten().collect();
        drop(stmt);

        let mut stmt = self.conn.prepare(
            "SELECT name, path, line FROM symbols \
             WHERE kind='fn' AND signature LIKE '%…' \
             ORDER BY LENGTH(signature) DESC LIMIT 10"
        )?;
        let complex_fns: Vec<(String, String, usize)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)? as usize))
        })?.flatten().collect();
        drop(stmt);

        let mut stmt = self.conn.prepare(
            "SELECT path, COUNT(*) as total, \
             SUM(CASE WHEN signature LIKE '%async%' THEN 1 ELSE 0 END) as async_n \
             FROM symbols WHERE kind='fn' \
             GROUP BY path HAVING total > 5 AND async_n > 0 \
             ORDER BY CAST(async_n AS REAL)/total DESC LIMIT 8"
        )?;
        let async_files: Vec<(String, usize, usize)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize, r.get::<_, i64>(2)? as usize))
        })?.flatten().collect();
        drop(stmt);

        Ok(QualityReport {
            total_files, total_syms,
            god_objects, large_files, high_coupling,
            dead_candidates, complex_fns, async_files,
        })
    }
}
