use anyhow::{Context, Result};
use async_trait::async_trait;

use super::Tool;

mod symbols;
mod search_impl;
use search_impl::{build_code_map, find_symbol_definition, search_with_rg_or_grep};

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
        "Find all references (call sites) to a symbol across the codebase. \
         Uses the AST-indexed call graph when available (Rust/Python/JS/TS), \
         falling back to text search otherwise. Returns file paths, line numbers, \
         and the enclosing function for each call site. Useful for impact analysis \
         before renaming or deleting a symbol."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "symbol":        { "type": "string",  "description": "Symbol name to search for." },
                "path":          { "type": "string",  "description": "Directory to search (text-fallback only). Default: '.'" },
                "file_type":     { "type": "string",  "description": "Limit to a file type, e.g. 'rust', 'py' (text-fallback only)." },
                "context_lines": { "type": "integer", "description": "Lines of context around each reference (text-fallback only, default 1)." },
                "max_results":   { "type": "integer", "description": "Cap on results (default 100)." }
            },
            "required": ["symbol"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("find references to '{}'", input["symbol"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let symbol    = input["symbol"].as_str().context("find_references: 'symbol' required")?;
        let max_n     = input["max_results"].as_u64().unwrap_or(100) as usize;

        // Prefer the indexed call graph when available.
        let hits = crate::code_index::global_find_references(symbol, max_n);
        if !hits.is_empty() {
            return Ok(format_call_sites(symbol, &hits));
        }

        // Fall back to text search.
        let path      = input["path"].as_str().unwrap_or(".");
        let file_type = input["file_type"].as_str();
        let ctx       = input["context_lines"].as_u64().unwrap_or(1) as usize;
        search_with_rg_or_grep(symbol, path, file_type, false, true, ctx, max_n as u64).await
    }
}

fn format_call_sites(symbol: &str, hits: &[crate::code_index::CallSite]) -> String {
    let mut out = format!("Found {} call site(s) for `{}` (from code index):\n\n", hits.len(), symbol);
    for h in hits {
        out.push_str(&h.display());
        out.push('\n');
    }
    out
}

// ── who_calls ─────────────────────────────────────────────────────────────────

pub(super) struct WhoCallsTool;

#[async_trait]
impl Tool for WhoCallsTool {
    fn name(&self) -> &str { "who_calls" }
    fn description(&self) -> &str {
        "Find callers of a function or method, optionally narrowed by qualifier. \
         Example: who_calls(name='foo', qualifier='Bar') matches only `Bar::foo(...)` calls, \
         not every `foo(...)` in the codebase. Use qualifier='' to match bare/unqualified calls only. \
         Uses the indexed call graph (Rust/Python/JS/TS) — returns nothing for unsupported languages."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name":        { "type": "string",  "description": "Function or method name." },
                "qualifier":   { "type": "string",  "description": "Optional path/type qualifier. Omit to match any qualifier. Empty string matches bare calls only." },
                "max_results": { "type": "integer", "description": "Result cap (default 100)." }
            },
            "required": ["name"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        let name = input["name"].as_str().unwrap_or("?");
        match input.get("qualifier").and_then(|v| v.as_str()) {
            Some(q) if !q.is_empty() => format!("who calls {}::{}", q, name),
            Some(_)                  => format!("who calls bare `{}`", name),
            None                     => format!("who calls {}", name),
        }
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let name = input["name"].as_str().context("who_calls: 'name' required")?;
        let qualifier = input.get("qualifier").and_then(|v| v.as_str());
        let max_n = input["max_results"].as_u64().unwrap_or(100) as usize;

        let hits = crate::code_index::global_callers_of(name, qualifier, max_n);
        if hits.is_empty() {
            let header = match qualifier {
                Some(q) if !q.is_empty() => format!("No call sites found for `{}::{}` in the index.", q, name),
                Some(_)                  => format!("No bare/unqualified calls to `{}` found in the index.", name),
                None                     => format!("No call sites found for `{}` in the index.", name),
            };
            return Ok(format!("{}\n(Tip: the index covers Rust, Python, JS/TS. Run `--index-only` to rebuild.)", header));
        }
        Ok(format_call_sites(name, &hits))
    }
}

// ── file_imports ──────────────────────────────────────────────────────────────

pub(super) struct FileImportsTool;

#[async_trait]
impl Tool for FileImportsTool {
    fn name(&self) -> &str { "file_imports" }
    fn description(&self) -> &str {
        "List the imports/use declarations in a source file. \
         Returns module, imported name, and alias per row. \
         Uses the indexed import graph (Rust/Python/JS/TS)."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute or repo-relative path to the source file." }
            },
            "required": ["path"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("file_imports for '{}'", input["path"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path = input["path"].as_str().context("file_imports: 'path' required")?;

        // Try both the literal path and the absolutised form (matches what's stored).
        let mut hits = crate::code_index::global_imports_for(path);
        if hits.is_empty() {
            if let Ok(abs) = std::fs::canonicalize(path) {
                hits = crate::code_index::global_imports_for(&abs.to_string_lossy());
            }
        }

        if hits.is_empty() {
            return Ok(format!("No imports indexed for `{}`. (Index may not cover this file's language, or it hasn't been indexed yet.)", path));
        }
        let mut out = format!("{} import(s) in `{}`:\n\n", hits.len(), path);
        for im in &hits {
            out.push_str(&im.display());
            out.push('\n');
        }
        Ok(out)
    }
}

// ── where_imported ────────────────────────────────────────────────────────────

pub(super) struct WhereImportedTool;

#[async_trait]
impl Tool for WhereImportedTool {
    fn name(&self) -> &str { "where_imported" }
    fn description(&self) -> &str {
        "Find every file that imports a given name (or module). \
         Use this for blast-radius analysis before renaming or moving a symbol. \
         Uses the indexed import graph (Rust/Python/JS/TS)."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name":   { "type": "string", "description": "Name (or alias) to look up — e.g. 'HashMap', 'useState'." },
                "module": { "type": "string", "description": "Optional: instead of a name, find files importing from this module — e.g. 'crate::util' or 'react'." }
            }
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        if let Some(m) = input.get("module").and_then(|v| v.as_str()) {
            format!("where module `{}` is imported", m)
        } else {
            format!("where `{}` is imported", input["name"].as_str().unwrap_or("?"))
        }
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let module = input.get("module").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
        let name   = input.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty());

        let (label, hits) = match (module, name) {
            (Some(m), _) => (format!("module `{}`", m), crate::code_index::global_users_of_module(m)),
            (None, Some(n)) => (format!("name `{}`", n), crate::code_index::global_importers_of(n)),
            _ => return Err(anyhow::anyhow!("where_imported: provide either 'name' or 'module'")),
        };

        if hits.is_empty() {
            return Ok(format!("No importers found for {}.", label));
        }
        let mut out = format!("{} importer(s) of {}:\n\n", hits.len(), label);
        for im in &hits {
            out.push_str(&im.display());
            out.push('\n');
        }
        Ok(out)
    }
}

// ── pack_context ──────────────────────────────────────────────────────────────

pub(super) struct PackContextTool;

#[async_trait]
impl Tool for PackContextTool {
    fn name(&self) -> &str { "pack_context" }
    fn description(&self) -> &str {
        "Curate a context bundle for a task within a token budget. Returns the most relevant \
         symbols (signatures only, no bodies) ranked by keyword match × PageRank × one-hop expansion \
         to callers and importers. Use this when you want to load the *right* code for a task without \
         reading files blindly. Coverage: Rust, Python, JS/TS, Go, Java, C# (symbols); call/import graph \
         depends on extractor support per language."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task":         { "type": "string",  "description": "Short description of the task — what you want context for. E.g. 'refactor the PageRank computation', 'where are tools registered'." },
                "token_budget": { "type": "integer", "description": "Approximate token budget for the returned bundle (default 4000). The packer counts ~4 chars/token." }
            },
            "required": ["task"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("pack_context for '{}'", input["task"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let task = input["task"].as_str().context("pack_context: 'task' required")?;
        let budget = input["token_budget"].as_u64().unwrap_or(4000) as usize;
        match crate::code_index::global_pack_context(task, budget) {
            Some(ctx) => Ok(ctx.to_display()),
            None => Ok("Code index not initialised — run `--index-only` first.".to_string()),
        }
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
