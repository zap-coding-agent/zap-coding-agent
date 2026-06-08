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
