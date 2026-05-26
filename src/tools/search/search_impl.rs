use anyhow::{Context, Result};

// ── tool discovery ─────────────────────────────────────────────────────────────

/// Find a CLI tool by name. Checks the process PATH first, then probes
/// common Git-for-Windows install locations (VS Code injects these into its
/// own environment but plain PowerShell/CMD processes do not).
async fn find_tool(name: &str) -> Option<String> {
    // Fast PATH check: if the OS can find it, use the bare name.
    if crate::shell_runner::run_args(name, &["--version"])
        .await
        .map(|o| o.exit_code == 0)
        .unwrap_or(false)
    {
        return Some(name.to_string());
    }

    // On Windows, Git for Windows ships grep and sometimes rg under its usr/bin
    // directory. IDEs (VS Code, JetBrains) add this to PATH automatically, but
    // processes launched from PowerShell or CMD usually don't have it.
    #[cfg(windows)]
    {
        let pf = std::env::var("PROGRAMFILES")
            .unwrap_or_else(|_| r"C:\Program Files".to_string());
        let candidates = [
            format!(r"{}\Git\usr\bin\{}.exe", pf, name),
            format!(r"C:\Program Files\Git\usr\bin\{}.exe", name),
            format!(r"C:\Program Files (x86)\Git\usr\bin\{}.exe", name),
        ];
        for path in &candidates {
            if std::path::Path::new(path).exists() {
                return Some(path.clone());
            }
        }
    }

    None
}

// ── ripgrep / grep / native search ────────────────────────────────────────────

pub(super) async fn search_with_rg_or_grep(
    pattern: &str,
    path: &str,
    file_type: Option<&str>,
    case_insensitive: bool,
    fixed_string: bool,
    context_lines: usize,
    max_results: u64,
) -> Result<String> {
    if let Some(rg) = find_tool("rg").await {
        let mut args: Vec<String> = vec![
            "--no-heading".into(),
            "--line-number".into(),
            "--color=never".into(),
            format!("-m{}", max_results),
            format!("-C{}", context_lines),
        ];
        if case_insensitive { args.push("-i".into()); }
        if fixed_string      { args.push("-F".into()); }
        if let Some(ft) = file_type {
            args.push(format!("--type={}", ft));
        }
        args.push(pattern.to_string());
        args.push(path.to_string());

        let str_args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = crate::shell_runner::run_args(&rg, &str_args).await?;
        return if out.stdout.is_empty() {
            Ok(format!("no matches for '{}' in '{}'", pattern, path))
        } else {
            Ok(format!("## Search: '{}' (ripgrep, gitignore-aware)\n\n{}", pattern, out.stdout))
        };
    }

    if let Some(grep) = find_tool("grep").await {
        let mut args: Vec<String> = vec!["-rn".into(), "--color=never".into()];
        if case_insensitive { args.push("-i".into()); }
        if fixed_string      { args.push("-F".into()); }
        args.push(format!("-m{}", max_results));
        if let Some(ft) = file_type {
            let glob = match ft {
                "rust" => "*.rs",  "py" | "python" => "*.py",
                "ts" | "typescript" => "*.ts",  "js" | "javascript" => "*.js",
                "go" => "*.go",  "java" => "*.java",
                "c" => "*.c",  "cpp" => "*.cpp",
                other => other,
            };
            args.push("--include".into());
            args.push(glob.into());
        }
        args.push(pattern.into());
        args.push(path.into());

        let str_args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = crate::shell_runner::run_args(&grep, &str_args).await?;
        return if out.stdout.is_empty() {
            Ok(format!("no matches for '{}' in '{}'", pattern, path))
        } else {
            Ok(format!("## Search: '{}' (grep)\n\n{}", pattern, out.stdout))
        };
    }

    // Neither rg nor grep found — warn the user and fall back to built-in search.
    #[cfg(windows)]
    crate::zap_warn!(
        "ripgrep (rg) and grep not found. Using slower built-in search.\n\
         To fix (pick one):\n\
         • Install ripgrep:  winget install BurntSushi.ripgrep\n\
         • Or add Git to PATH: Settings → System → Environment Variables → add\n\
           C:\\Program Files\\Git\\usr\\bin  to your PATH, then restart the terminal."
    );
    #[cfg(not(windows))]
    crate::zap_warn!(
        "ripgrep (rg) and grep not found. Using slower built-in search.\n\
         To fix: install ripgrep — brew install ripgrep  (Mac)\n\
                                    apt install ripgrep  (Debian/Ubuntu)\n\
                                    cargo install ripgrep"
    );

    search_rust_native(pattern, path, file_type, case_insensitive, fixed_string, context_lines, max_results)
}

/// Pure-Rust text search — used when rg and grep are both unavailable (e.g. Windows).
fn search_rust_native(
    pattern: &str,
    root: &str,
    file_type: Option<&str>,
    case_insensitive: bool,
    fixed_string: bool,
    context_lines: usize,
    max_results: u64,
) -> Result<String> {
    use std::io::{BufRead, BufReader};

    let matcher: Box<dyn Fn(&str) -> bool + Send + Sync> = if fixed_string {
        let pat = if case_insensitive { pattern.to_lowercase() } else { pattern.to_string() };
        Box::new(move |line: &str| {
            if case_insensitive { line.to_lowercase().contains(&pat) } else { line.contains(&pat) }
        })
    } else {
        let pat_str = if case_insensitive {
            format!("(?i){}", pattern)
        } else {
            pattern.to_string()
        };
        let re = regex::Regex::new(&pat_str)
            .with_context(|| format!("invalid regex pattern: {}", pattern))?;
        Box::new(move |line: &str| re.is_match(line))
    };

    // When no file_type is given, restrict to known text extensions to avoid
    // reading binary files (.db, .lock, images, compiled objects, etc.).
    const DEFAULT_TEXT_EXTS: &[&str] = &[
        "rs", "py", "ts", "js", "go", "java", "c", "cpp", "cc", "cxx",
        "h", "hpp", "rb", "swift", "kt", "cs", "toml", "yaml", "yml",
        "json", "md", "txt", "sh", "bash", "zsh", "fish", "env",
        "html", "css", "scss", "xml", "sql",
    ];

    let allowed_exts: Vec<&str> = match file_type {
        Some(ft) => match ft {
            "rust"              => vec!["rs"],
            "py"|"python"       => vec!["py"],
            "ts"|"typescript"   => vec!["ts"],
            "js"|"javascript"   => vec!["js"],
            "go"                => vec!["go"],
            "java"              => vec!["java"],
            "c"                 => vec!["c", "h"],
            "cpp"               => vec!["cpp", "cc", "cxx", "hpp", "hxx"],
            other               => vec![other],
        },
        None => DEFAULT_TEXT_EXTS.to_vec(),
    };

    let mut files: Vec<std::path::PathBuf> = Vec::new();
    fn walk(dir: &std::path::Path, exts: &[&str], out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if matches!(name, "target"|"node_modules"|".git"|"dist"|"build"|".zap") { continue; }
                walk(&p, exts, out);
            } else {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                if exts.contains(&ext) { out.push(p); }
            }
        }
    }
    walk(std::path::Path::new(root), &allowed_exts, &mut files);
    files.sort();

    let mut output_lines: Vec<String> = Vec::new();
    let mut total_hits: u64 = 0;
    let mut files_with_hits: u64 = 0;

    'files: for file_path in &files {
        let Ok(f) = std::fs::File::open(file_path) else { continue };
        let reader = BufReader::new(f);
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap_or_default()).collect();
        let rel = file_path.strip_prefix(root).unwrap_or(file_path);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let mut file_hits = 0u64;
        // Track the last line we already emitted to avoid duplicating context
        // when two matches are closer than context_lines apart.
        let mut last_emitted: Option<usize> = None;

        for (i, line) in lines.iter().enumerate() {
            if !matcher(line) { continue; }
            total_hits += 1;
            file_hits += 1;
            if total_hits > max_results { break 'files; }

            // Separator between non-adjacent match groups.
            let ctx_start = i.saturating_sub(context_lines);
            if let Some(last) = last_emitted {
                if ctx_start > last + 1 {
                    output_lines.push("--".to_string());
                }
            }

            // Context before (only lines not already emitted).
            let emit_from = last_emitted.map(|l| l + 1).unwrap_or(ctx_start).max(ctx_start);
            for (ci, cl) in lines[emit_from..i].iter().enumerate() {
                let ln = emit_from + ci + 1;
                output_lines.push(format!("{}-{}-{}", rel_str, ln, cl));
            }
            // Matching line (`:` separator marks it as a match, same as rg/grep).
            output_lines.push(format!("{}:{}:{}", rel_str, i + 1, line));
            last_emitted = Some(i);

            // Context after (will be re-evaluated on next iteration to avoid dup).
            let ctx_end = (i + 1 + context_lines).min(lines.len());
            for (ci, cl) in lines[i + 1..ctx_end].iter().enumerate() {
                let ln = i + 2 + ci;
                output_lines.push(format!("{}-{}-{}", rel_str, ln, cl));
                last_emitted = Some(i + 1 + ci);
            }
        }
        if file_hits > 0 { files_with_hits += 1; }
    }

    if output_lines.is_empty() {
        return Ok(format!("no matches for '{}' in '{}'", pattern, root));
    }
    Ok(format!(
        "## Search: '{}' (native, {} matches in {} files)\n\n{}",
        pattern, total_hits, files_with_hits,
        output_lines.join("\n")
    ))
}

// ── find_symbol_definition ────────────────────────────────────────────────────

pub(super) async fn find_symbol_definition(symbol: &str, path: &str, lang_hint: &str) -> Result<String> {
    let index_hits = crate::code_index::global_find_definition(symbol);
    if !index_hits.is_empty() {
        crate::log::write("INDEX", &format!("hit · find_definition · '{}' · {} result(s)", symbol, index_hits.len()));
        let _ = crate::audit::record(&format!("index_hit op=find_definition symbol={} results={}", symbol, index_hits.len()));
        let mut lines = vec![format!("Definition(s) of '{}' [AST index]:", symbol)];
        for sym in &index_hits {
            let ctx = if sym.context.is_empty() { String::new() } else { format!(" [{}]", sym.context) };
            lines.push(format!("  {}:{} {} {}{}", sym.path, sym.line, sym.kind, sym.name, ctx));
            if !sym.signature.is_empty() {
                lines.push(format!("    {}", sym.signature));
            }
        }
        return Ok(lines.join("\n"));
    }
    crate::log::write("INDEX", &format!("miss · find_definition · '{}' · grep fallback", symbol));

    let patterns: Vec<(String, Option<&str>)> = match lang_hint {
        "rust" => vec![
            (format!("fn {symbol}[ (<]"),       Some("rust")),
            (format!("struct {symbol}[ {{<]"),  Some("rust")),
            (format!("enum {symbol}[ {{<]"),    Some("rust")),
            (format!("trait {symbol}[ {{<]"),   Some("rust")),
            (format!("type {symbol} ="),        Some("rust")),
            (format!("const {symbol}:"),        Some("rust")),
        ],
        "python" | "py" => vec![
            (format!("def {symbol}("),           Some("py")),
            (format!("class {symbol}[ (:]"),     Some("py")),
            (format!("{symbol} ="),              Some("py")),
        ],
        "typescript" | "ts" | "javascript" | "js" => vec![
            (format!("function {symbol}[ (]"),   None),
            (format!("class {symbol}[ {{]"),     None),
            (format!("const {symbol} ="),        None),
            (format!("let {symbol} ="),          None),
            (format!("interface {symbol}[ {{]"), None),
            (format!("type {symbol} ="),         None),
        ],
        "go" => vec![
            (format!("func {symbol}[ (]"),        Some("go")),
            (format!("func.*) {symbol}[ (]"),     Some("go")),
            (format!("type {symbol}[ {{]"),       Some("go")),
        ],
        _ => vec![
            (format!("fn {symbol}[ (<]"),        None),
            (format!("def {symbol}("),            None),
            (format!("function {symbol}("),      None),
            (format!("class {symbol}[ ({{]"),    None),
            (format!("struct {symbol}[ {{<]"),   None),
            (format!("type {symbol}[ =]"),       None),
            (format!("const {symbol}[ :=]"),     None),
            (format!("interface {symbol}[ {{]"), None),
        ],
    };

    for (pat, ft) in &patterns {
        let result = search_with_rg_or_grep(pat, path, *ft, false, false, 0, 10).await?;
        if !result.contains("no matches") {
            return Ok(format!("Definition of '{}':\n{}", symbol, result));
        }
    }

    Ok(format!("No definition found for '{}'.", symbol))
}

// ── code_map builder ──────────────────────────────────────────────────────────

/// Strip Windows extended-length UNC prefix (`\\?\`) from canonical paths.
fn strip_unc_prefix(p: std::path::PathBuf) -> std::path::PathBuf {
    let s = p.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        std::path::PathBuf::from(stripped.to_string())
    } else {
        p
    }
}

pub(super) async fn build_code_map(path: &str, max_depth: usize, file_type: Option<&str>) -> Result<String> {
    let p = std::path::Path::new(path);

    let canonical = strip_unc_prefix(p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));
    let index_syms = crate::code_index::global_symbols_in_path(&canonical.to_string_lossy());

    if !index_syms.is_empty() {
        crate::log::write("INDEX", &format!("hit · code_map · '{}' · {} symbol(s)", path, index_syms.len()));
        let _ = crate::audit::record(&format!("index_hit op=code_map path={} symbols={}", path, index_syms.len()));
        let filtered: Vec<_> = index_syms.iter().filter(|s| {
            if let Some(ft) = file_type {
                match ft {
                    "rust"                  => s.language == "rust",
                    "python" | "py"         => s.language == "python",
                    "typescript" | "ts"     => s.language == "typescript",
                    "javascript" | "js"     => s.language == "javascript",
                    "go"                    => s.language == "go",
                    "java"                  => s.language == "java",
                    _                       => true,
                }
            } else {
                true
            }
        }).collect();

        if !filtered.is_empty() {
            let mut output = vec![format!("## Code map: {} [AST index, {} symbol(s)]", path, filtered.len())];
            let mut last_file = String::new();
            for sym in &filtered {
                if sym.path != last_file {
                    output.push(format!("\n{}:", sym.path));
                    last_file = sym.path.clone();
                }
                let ctx = if sym.context.is_empty() { String::new() } else { format!(" [{}]", sym.context) };
                output.push(format!("  {:>5}  {} {}{}", sym.line, sym.kind, sym.name, ctx));
            }
            return Ok(output.join("\n"));
        }
    }

    if p.is_file() {
        let content = tokio::fs::read_to_string(path).await
            .with_context(|| format!("code_map: cannot read '{}'", path))?;
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        let symbols = extract_symbols(&content, ext);
        if symbols.is_empty() {
            return Ok(format!("{}: (no recognised symbols)", path));
        }
        return Ok(format!("{}:\n{}", path, symbols.join("\n")));
    }

    let mut output = Vec::<String>::new();
    walk_dir_for_map(p, 0, max_depth, file_type, &mut output)?;

    if output.is_empty() {
        return Ok(format!("No source files found in '{}'", path));
    }
    Ok(output.join("\n"))
}

fn walk_dir_for_map(
    dir: &std::path::Path,
    depth: usize,
    max_depth: usize,
    file_type: Option<&str>,
    output: &mut Vec<String>,
) -> Result<()> {
    if depth > max_depth { return Ok(()); }
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') || matches!(name_str.as_ref(),
            "target" | "node_modules" | "__pycache__" | ".git" | "dist" | "build"
        ) {
            continue;
        }

        // Never follow symlinks — they can form cycles (e.g. .kiro/skills/.kiro/…).
        if !path.is_symlink() && path.is_dir() {
            walk_dir_for_map(&path, depth + 1, max_depth, file_type, output)?;
        } else if !path.is_symlink() && path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if let Some(ft) = file_type {
                let ext_matches = match ft {
                    "rust" => ext == "rs",
                    "python" | "py" => ext == "py",
                    "typescript" | "ts" => ext == "ts",
                    "javascript" | "js" => ext == "js",
                    "go" => ext == "go",
                    "java" => ext == "java",
                    _ => ext == ft,
                };
                if !ext_matches { continue; }
            } else if !matches!(ext, "rs" | "py" | "ts" | "js" | "go" | "java" | "c" | "cpp" | "h" | "rb" | "swift") {
                continue;
            }

            if let Ok(content) = std::fs::read_to_string(&path) {
                let symbols = extract_symbols(&content, ext);
                if !symbols.is_empty() {
                    output.push(format!("\n{}:", path.display()));
                    output.extend(symbols);
                }
            }
        }
    }
    Ok(())
}

fn extract_symbols(content: &str, ext: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let line_no = i + 1;

        let sym = match ext {
            "rs" => extract_rust_symbol(trimmed),
            "py" => extract_python_symbol(trimmed),
            "ts" | "js" => extract_ts_symbol(trimmed),
            "go" => extract_go_symbol(trimmed),
            "java" => extract_java_symbol(trimmed),
            _ => None,
        };

        if let Some(label) = sym {
            symbols.push(format!("  {} (line {})", label, line_no));
        }
    }
    symbols
}

fn extract_rust_symbol(line: &str) -> Option<String> {
    for prefix in &["pub fn ", "fn ", "pub async fn ", "async fn ",
                    "pub struct ", "struct ", "pub enum ", "enum ",
                    "pub trait ", "trait ", "pub type ", "type ",
                    "pub const ", "const ", "pub static ", "static ",
                    "impl "] {
        if let Some(after) = line.strip_prefix(prefix) {
            let rest = after.split(|c: char| !c.is_alphanumeric() && c != '_').next()?;
            if !rest.is_empty() {
                return Some(format!("{} {}", prefix.trim(), rest));
            }
        }
    }
    None
}

fn extract_python_symbol(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("async def ").or_else(|| line.strip_prefix("def ")) {
        let name = rest.split('(').next()?.trim();
        return Some(format!("def {}", name));
    }
    if let Some(after) = line.strip_prefix("class ") {
        let name = after.split(['(', ':']).next()?.trim();
        return Some(format!("class {}", name));
    }
    None
}

fn extract_ts_symbol(line: &str) -> Option<String> {
    for prefix in &["export function ", "function ", "export class ", "class ",
                    "export interface ", "interface ", "export type ", "type ",
                    "export const ", "const ", "export let ", "let ", "export enum ", "enum "] {
        if let Some(after) = line.strip_prefix(prefix) {
            let rest: &str = after.split(['(', '<', ' ', '=']).next()?;
            if !rest.is_empty() {
                return Some(format!("{}{}", prefix.trim_start_matches("export "), rest));
            }
        }
    }
    None
}

fn extract_go_symbol(line: &str) -> Option<String> {
    if let Some(after) = line.strip_prefix("func ") {
        let rest: &str = after.split(['(', ' ']).next()?;
        return Some(format!("func {}", rest));
    }
    if let Some(after) = line.strip_prefix("type ") {
        let name: &str = after.split_whitespace().next()?;
        return Some(format!("type {}", name));
    }
    None
}

fn extract_java_symbol(line: &str) -> Option<String> {
    for kw in &["class ", "interface ", "enum ", "record "] {
        if line.contains(kw) {
            let after = line.split(kw).nth(1)?;
            let name: &str = after.split([' ', '{', '(', '<']).next()?;
            if !name.is_empty() {
                return Some(format!("{} {}", kw.trim(), name));
            }
        }
    }
    if (line.contains("public ") || line.contains("private ") || line.contains("protected "))
        && line.contains('(')
    {
        let before_paren = line.split('(').next()?;
        let name: &str = before_paren.split_whitespace().last()?;
        if !name.is_empty() && name != "class" && name != "interface" {
            return Some(format!("method {}", name));
        }
    }
    None
}
