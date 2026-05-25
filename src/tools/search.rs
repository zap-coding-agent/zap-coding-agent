use anyhow::{Context, Result};
use async_trait::async_trait;

use super::Tool;

// ── search_code ───────────────────────────────────────────────────────────────

pub(super) struct SearchCodeTool;

#[async_trait]
impl Tool for SearchCodeTool {
    fn name(&self) -> &str { "search_code" }
    fn description(&self) -> &str {
        "Search for a pattern (regex) in source files under a directory."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern":          { "type": "string",  "description": "Regex (or fixed string) to search for." },
                "path":             { "type": "string",  "description": "Root directory to search (default: .)." },
                "file_type":        { "type": "string",  "description": "Limit to a language/file-type, e.g. 'rust', 'py', 'ts'." },
                "case_insensitive": { "type": "boolean", "description": "Case-insensitive match (default false)." },
                "fixed_string":     { "type": "boolean", "description": "Treat pattern as literal string, not regex (default false)." },
                "context_lines":    { "type": "integer", "description": "Lines of context before/after each match (default 2)." },
                "max_results":      { "type": "integer", "description": "Max matches to return (default 50)." }
            },
            "required": ["pattern"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("grep '{}' in '{}'",
            input["pattern"].as_str().unwrap_or("?"),
            input["path"].as_str().unwrap_or("."))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let pattern     = input["pattern"].as_str().context("search_code: 'pattern' must be a string")?;
        let path        = input["path"].as_str().unwrap_or(".");
        let file_type   = input["file_type"].as_str();
        let case_insens = input["case_insensitive"].as_bool().unwrap_or(false);
        let fixed       = input["fixed_string"].as_bool().unwrap_or(false);
        let ctx_lines   = input["context_lines"].as_u64().unwrap_or(2) as usize;
        let max_results = input["max_results"].as_u64().unwrap_or(50);

        search_with_rg_or_grep(pattern, path, file_type, case_insens, fixed, ctx_lines, max_results).await
    }
}

// ── find_definition ───────────────────────────────────────────────────────────

pub(super) struct FindDefinitionTool;

#[async_trait]
impl Tool for FindDefinitionTool {
    fn name(&self) -> &str { "find_definition" }
    fn description(&self) -> &str {
        "Find where a symbol (function, struct, class, const, type, etc.) is defined. \
         Returns the file path and line number. \
         More precise than search_code because it uses language-aware definition patterns."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "symbol":   { "type": "string", "description": "Symbol name to find (e.g. 'MyStruct', 'parse_config', 'MAX_RETRIES')." },
                "path":     { "type": "string", "description": "Directory to search (default: .)." },
                "language": { "type": "string", "description": "Hint: 'rust', 'python', 'typescript', 'go', 'java', 'c'. Auto-detected if omitted." }
            },
            "required": ["symbol"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("find definition of '{}'", input["symbol"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let symbol = input["symbol"].as_str().context("find_definition: 'symbol' required")?;
        let path   = input["path"].as_str().unwrap_or(".");
        let lang   = input["language"].as_str().unwrap_or("");
        find_symbol_definition(symbol, path, lang).await
    }
}

// ── find_references ───────────────────────────────────────────────────────────

pub(super) struct FindReferencesTool;

#[async_trait]
impl Tool for FindReferencesTool {
    fn name(&self) -> &str { "find_references" }
    fn description(&self) -> &str {
        "Find all references to a symbol across the codebase. \
         Returns file paths, line numbers, and context. \
         Useful for impact analysis before renaming or deleting a symbol."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "symbol":        { "type": "string",  "description": "Symbol name to search for." },
                "path":          { "type": "string",  "description": "Directory to search (default: .)." },
                "file_type":     { "type": "string",  "description": "Limit to a file type, e.g. 'rust', 'py'." },
                "context_lines": { "type": "integer", "description": "Lines of context around each reference (default 1)." }
            },
            "required": ["symbol"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("find references to '{}'", input["symbol"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let symbol    = input["symbol"].as_str().context("find_references: 'symbol' required")?;
        let path      = input["path"].as_str().unwrap_or(".");
        let file_type = input["file_type"].as_str();
        let ctx       = input["context_lines"].as_u64().unwrap_or(1) as usize;
        search_with_rg_or_grep(symbol, path, file_type, false, true, ctx, 100).await
    }
}

// ── code_map ──────────────────────────────────────────────────────────────────

pub(super) struct CodeMapTool;

#[async_trait]
impl Tool for CodeMapTool {
    fn name(&self) -> &str { "code_map" }
    fn description(&self) -> &str {
        "Generate a structural outline of a file or directory: \
         functions, structs, classes, enums, constants, and their line numbers. \
         Use this to navigate a codebase without reading entire files. \
         Much faster than read_file + manual scanning for large files."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":      { "type": "string",  "description": "File or directory to map (default: .)." },
                "max_depth": { "type": "integer", "description": "Max directory recursion depth (default 3)." },
                "file_type": { "type": "string",  "description": "Limit to a file type: 'rust', 'py', 'ts', 'go', 'java'. Default: all supported." }
            }
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("code map of '{}'", input["path"].as_str().unwrap_or("."))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path      = input["path"].as_str().unwrap_or(".");
        let max_depth = input["max_depth"].as_u64().unwrap_or(3) as usize;
        let file_type = input["file_type"].as_str();
        build_code_map(path, max_depth, file_type).await
    }
}

// ── ripgrep / grep search helper ──────────────────────────────────────────────

async fn search_with_rg_or_grep(
    pattern: &str,
    path: &str,
    file_type: Option<&str>,
    case_insensitive: bool,
    fixed_string: bool,
    context_lines: usize,
    max_results: u64,
) -> Result<String> {
    let rg_available = crate::shell_runner::run_args("rg", &["--version"])
        .await
        .map(|o| o.exit_code == 0)
        .unwrap_or(false);

    if rg_available {
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
        let out = crate::shell_runner::run_args("rg", &str_args).await?;
        return if out.stdout.is_empty() {
            Ok(format!("no matches for '{}' in '{}'", pattern, path))
        } else {
            Ok(format!("## Search: '{}' (ripgrep, gitignore-aware)\n\n{}", pattern, out.stdout))
        };
    }

    let mut args: Vec<&str> = vec!["-rn", "--color=never"];
    if case_insensitive { args.push("-i"); }
    if fixed_string      { args.push("-F"); }
    let max_str = max_results.to_string();
    args.extend_from_slice(&["-m", max_str.as_str()]);
    if let Some(ft) = file_type {
        let glob = match ft {
            "rust" => "*.rs",  "py" | "python" => "*.py",
            "ts" | "typescript" => "*.ts",  "js" | "javascript" => "*.js",
            "go" => "*.go",  "java" => "*.java",
            "c" => "*.c",  "cpp" => "*.cpp",
            other => other,
        };
        args.push("--include"); args.push(glob);
    }
    args.push(pattern);
    args.push(path);

    let out = crate::shell_runner::run_args("grep", &args).await?;
    if out.stdout.is_empty() {
        Ok(format!("no matches for '{}' in '{}'", pattern, path))
    } else {
        Ok(format!("## Search: '{}' (grep)\n\n{}", pattern, out.stdout))
    }
}

// ── find_symbol_definition helper ─────────────────────────────────────────────

async fn find_symbol_definition(symbol: &str, path: &str, lang_hint: &str) -> Result<String> {
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

async fn build_code_map(path: &str, max_depth: usize, file_type: Option<&str>) -> Result<String> {
    let p = std::path::Path::new(path);

    let canonical = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
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
