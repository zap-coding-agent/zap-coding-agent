use colored::Colorize;
use super::super::Session;

impl Session {
    pub fn cmd_think(&mut self, arg: &str) {
        match arg.trim() {
            "" | "status" => {
                if self.thinking_budget == 0 {
                    println!("  {} Extended thinking: {}", "◎".truecolor(100, 200, 255), "off".dimmed());
                } else {
                    println!("  {} Extended thinking: {} token budget", "◎".truecolor(100, 200, 255), self.thinking_budget.to_string().cyan());
                }
                println!("  {} Usage: /think on|off|<tokens>  (e.g. /think 8000)", "·".dimmed());
                println!("  {} Note: extended thinking requires claude-3-7-sonnet or newer.", "·".dimmed());
            }
            "off" | "0" => {
                self.thinking_budget = 0;
                println!("  {} Extended thinking {}", "◎".truecolor(100, 200, 255), "disabled".dimmed());
            }
            "on" => {
                self.thinking_budget = 8000;
                println!("  {} Extended thinking {} ({} token budget)", "◎".truecolor(100, 200, 255), "enabled".green(), "8000".cyan());
            }
            n => match n.parse::<u32>() {
                Ok(0) => {
                    self.thinking_budget = 0;
                    println!("  {} Extended thinking {}", "◎".truecolor(100, 200, 255), "disabled".dimmed());
                }
                Ok(v) => {
                    self.thinking_budget = v;
                    println!("  {} Extended thinking {} ({} token budget)", "◎".truecolor(100, 200, 255), "enabled".green(), v.to_string().cyan());
                }
                Err(_) => println!("  {} Usage: /think on|off|<budget_tokens>  e.g. /think 8000", "✗".red()),
            }
        }
    }

    pub fn cmd_index(&mut self, arg: &str) {
        let cwd    = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let target = if arg.is_empty() { cwd.clone() } else { std::path::PathBuf::from(arg) };

        if arg == "clear" || arg == "reset" {
            match crate::code_index::global_index() {
                None => println!("  {} index not initialised", "✗".red()),
                Some(arc) => match arc.lock() {
                    Ok(mut idx) => match idx.clear() {
                        Ok(_) => println!("  {} Index cleared — run {} to rebuild.", "✓".green(), "/index".cyan()),
                        Err(e) => println!("  {} clear failed: {}", "✗".red(), e),
                    },
                    Err(_) => println!("  {} index lock busy", "✗".red()),
                },
            }
            return;
        }

        if arg == "stats" || arg == "status" {
            let (files, syms) = crate::code_index::global_stats();
            let db = cwd.join(".zap").join("code.db");
            let db_kb = db.exists()
                .then(|| std::fs::metadata(&db).ok().map(|m| m.len() / 1024))
                .flatten()
                .unwrap_or(0);

            println!();
            println!("  {} code index", "◎".truecolor(100, 200, 255).bold());
            println!("  {}", "─".repeat(50).truecolor(60, 55, 80));
            println!("  {:<10} {}    {:<10} {}    {:<6} {} KB",
                "files".truecolor(100, 95, 130),  files.to_string().cyan().bold(),
                "symbols".truecolor(100, 95, 130), syms.to_string().cyan().bold(),
                "db".truecolor(100, 95, 130),      db_kb.to_string().dimmed());

            let by_kind = crate::code_index::global_stats_by_kind();
            if !by_kind.is_empty() {
                println!();
                println!("  {} by kind", "▸".truecolor(255, 210, 50));
                let max = by_kind.iter().map(|(_, n)| *n).max().unwrap_or(1);
                for (kind, count) in &by_kind {
                    let bar_len = (count * 20 / max).max(1);
                    let bar: String = "█".repeat(bar_len);
                    let pct = count * 100 / syms.max(1);
                    println!("    {:<8} {:>5}  {}  {}%",
                        kind.truecolor(150, 210, 255),
                        count.to_string().dimmed(),
                        bar.truecolor(80, 160, 220),
                        pct.to_string().truecolor(100, 95, 130));
                }
            }

            let top = crate::code_index::global_top_files(8);
            if !top.is_empty() {
                println!();
                println!("  {} top files by symbol count", "▸".truecolor(255, 210, 50));
                for (path, count) in &top {
                    let short = path
                        .strip_prefix(cwd.to_str().unwrap_or(""))
                        .unwrap_or(path)
                        .trim_start_matches('/');
                    println!("    {:>4}  {}", count.to_string().cyan(), short.truecolor(140, 135, 160));
                }
            }
            println!();
            return;
        }

        if arg == "files" || arg == "list" {
            let entries = crate::code_index::global_list_indexed_files(200);
            if entries.is_empty() {
                println!("  {} No files indexed yet. Run {} to index the project.", "·".dimmed(), "/index".cyan());
            } else {
                println!("  {} {} file(s) in code index:", "◎".truecolor(100, 200, 255), entries.len());
                for (path, syms) in &entries {
                    println!("  {} {:>4} sym  {}", "·".dimmed(), syms, path.dimmed());
                }
            }
            return;
        }

        if arg == "db" {
            let db_path = dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".zap").join("agent.db");
            if !db_path.exists() {
                println!("  {} agent.db not found at {}", "✗".red(), db_path.display());
                return;
            }
            match rusqlite::Connection::open(&db_path) {
                Err(e) => println!("  {} failed to open agent.db: {}", "✗".red(), e),
                Ok(conn) => {
                    let sessions: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap_or(0);
                    let memory: i64  = conn.query_row("SELECT COUNT(*) FROM memory", [], |r| r.get(0)).unwrap_or(0);
                    let branches: i64 = conn.query_row("SELECT COUNT(*) FROM branches", [], |r| r.get(0)).unwrap_or(0);
                    println!("  {} agent.db  ({})", "◎".truecolor(100, 200, 255), db_path.display().to_string().dimmed());
                    println!("  {} sessions: {}  memory entries: {}  branches: {}", "·".dimmed(), sessions, memory, branches);

                    let mut stmt = conn.prepare(
                        "SELECT id, goal, model, created_at FROM sessions ORDER BY id DESC LIMIT 10"
                    ).unwrap();
                    let rows: Vec<(i64, String, String, String)> = stmt.query_map([], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                    }).unwrap().flatten().collect();
                    if !rows.is_empty() {
                        println!("  {} Recent sessions:", "·".dimmed());
                        for (id, goal, model, created) in &rows {
                            let short_goal = if goal.chars().count() > 60 {
                                format!("{}…", goal.chars().take(60).collect::<String>())
                            } else {
                                goal.clone()
                            };
                            println!("    {} #{} [{}] {} — {}", "·".dimmed(), id, model.dimmed(), short_goal.cyan(), created.dimmed());
                        }
                    }

                    let mut mstmt = conn.prepare("SELECT key, value FROM memory ORDER BY key LIMIT 20").unwrap();
                    let mrows: Vec<(String, String)> = mstmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().flatten().collect();
                    if !mrows.is_empty() {
                        println!("  {} Memory ({} entries):", "·".dimmed(), mrows.len());
                        for (key, val) in &mrows {
                            let short_val = if val.chars().count() > 80 {
                                format!("{}…", val.chars().take(80).collect::<String>())
                            } else {
                                val.clone()
                            };
                            println!("    {} {}: {}", "·".dimmed(), key.cyan(), short_val.dimmed());
                        }
                    }
                }
            }
            return;
        }

        if arg == "quality" {
            let cwd_str = cwd.to_string_lossy();
            let shorten = |p: &str| -> String {
                p.strip_prefix(cwd_str.as_ref())
                    .unwrap_or(p)
                    .trim_start_matches('/')
                    .to_string()
            };

            if let Ok(mut guard) = self.code_index.lock() {
                let _ = guard.compute_reference_counts();
            }

            let report = match crate::code_index::global_quality_report() {
                Some(r) => r,
                None => {
                    println!("  {} index not ready — run {} first", "✗".red(), "/index".cyan());
                    return;
                }
            };

            let score = report.score();
            let score_color = if score >= 80 { score.to_string().green().to_string() }
                              else if score >= 60 { score.to_string().truecolor(255,200,60).to_string() }
                              else { score.to_string().red().to_string() };

            println!();
            println!("  {} code quality — {} files · {} symbols",
                "◎".truecolor(100, 200, 255).bold(),
                report.total_files.to_string().cyan(),
                report.total_syms.to_string().cyan());
            println!("  {}", "─".repeat(60).truecolor(60, 55, 80));

            if !report.god_objects.is_empty() {
                println!();
                println!("  {} god objects  (>15 methods — split recommended)", "⚠".truecolor(255, 140, 60).bold());
                for (label, count, path) in &report.god_objects {
                    let bar: String = "█".repeat((*count / 5).min(20));
                    println!("    {:<28} {} methods  {}  {}",
                        label.truecolor(255, 180, 80).bold(),
                        count.to_string().truecolor(255,140,60),
                        bar.truecolor(255,140,60),
                        shorten(path).truecolor(100, 95, 130));
                }
            }

            if !report.large_files.is_empty() {
                println!();
                println!("  {} large files  (>50 symbols)", "⚠".truecolor(255, 200, 60).bold());
                let max_syms = report.large_files.iter().map(|(_, n)| *n).max().unwrap_or(1);
                for (path, syms) in &report.large_files {
                    let bar_len = (syms * 20 / max_syms).max(1);
                    let bar: String = "█".repeat(bar_len);
                    println!("    {:>5} sym  {}  {}",
                        syms.to_string().cyan(),
                        bar.truecolor(100, 200, 255),
                        shorten(path).truecolor(140, 135, 160));
                }
            }

            if !report.high_coupling.is_empty() {
                println!();
                println!("  {} high coupling  (many references — risky to change)", "✦".truecolor(200, 150, 255).bold());
                for (name, path, line, refs) in &report.high_coupling {
                    let call_sites = refs.saturating_sub(1);
                    println!("    {:<32} {}×  {}:{}",
                        name.truecolor(200, 150, 255).bold(),
                        call_sites.to_string().truecolor(200, 150, 255),
                        shorten(path).truecolor(100, 95, 130),
                        line.to_string().dimmed());
                }
            }

            if !report.dead_candidates.is_empty() {
                println!();
                println!("  {} dead code candidates  (pub fn, 0 external refs)", "◌".truecolor(130, 125, 150).bold());
                for (name, path, line) in &report.dead_candidates {
                    println!("    {:<32} {}:{}",
                        name.truecolor(130, 125, 150),
                        shorten(path).truecolor(100, 95, 130),
                        line.to_string().dimmed());
                }
            }

            if !report.complex_fns.is_empty() {
                println!();
                println!("  {} complex signatures  (truncated — many params or generics)", "◈".truecolor(255, 200, 60).bold());
                for (name, path, line) in &report.complex_fns {
                    println!("    {:<32} {}:{}",
                        name.truecolor(255, 200, 60),
                        shorten(path).truecolor(100, 95, 130),
                        line.to_string().dimmed());
                }
            }

            if !report.async_files.is_empty() {
                println!();
                println!("  {} async density", "⚡".bright_yellow().bold());
                for (path, total, async_n) in &report.async_files {
                    let pct = async_n * 100 / total.max(&1);
                    let bar: String = "█".repeat(pct / 5);
                    println!("    {:>3}%  {}  {}",
                        pct.to_string().truecolor(255, 220, 80),
                        bar.truecolor(255, 200, 60),
                        shorten(path).truecolor(140, 135, 160));
                }
            }

            println!();
            println!("  {}", "─".repeat(60).truecolor(60, 55, 80));
            println!("  quality score  {}/100", score_color);
            if score < 80 {
                println!();
                if report.god_objects.iter().any(|(_, n, _)| *n > 30) {
                    println!("  {} largest god object has >30 methods — extract sub-handlers", "→".truecolor(255,140,60));
                }
                if report.large_files.iter().any(|(_, n)| *n > 80) {
                    println!("  {} files with >80 symbols should be split by responsibility", "→".truecolor(255,200,60));
                }
                if !report.dead_candidates.is_empty() {
                    println!("  {} {} pub fn never referenced — check if they can be removed",
                        "→".truecolor(130, 125, 150), report.dead_candidates.len());
                }
            }
            println!();
            return;
        }

        println!("  {} tree-sitter scanning {}…", "◎".truecolor(100, 200, 255), target.display().to_string().cyan());
        if let Ok(mut guard) = self.code_index.lock() {
            // Upgrade in-memory index to file-backed on first /index run.
            if guard.is_in_memory() {
                match crate::code_index::CodeIndex::open(&cwd) {
                    Ok(file_idx) => { *guard = file_idx; }
                    Err(e) => println!("  {} could not create code.db: {e}", "⚠".truecolor(255, 200, 60)),
                }
            }
            match guard.index_dir(&target) {
                Ok((files, syms)) => {
                    println!("  {} tree-sitter: {} file(s) indexed · {} symbol(s) extracted",
                        "✓".green(), files.to_string().cyan(), syms.to_string().cyan());
                    let (total_f, total_s) = guard.total_stats().unwrap_or((0, 0));
                    println!("  {} total in index: {} file(s) · {} symbol(s)", "·".dimmed(), total_f, total_s);
                    crate::project::mark_indexed();
                }
                Err(e) => println!("  {} index error: {}", "✗".red(), e),
            }
        } else {
            println!("  {} index is locked (reindexing in progress?)", "✗".red());
        }
    }
}
