use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

// ── Tool trait ────────────────────────────────────────────────────────────────

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// JSON Schema object describing the `input` the tool accepts.
    fn input_schema(&self) -> serde_json::Value;
    /// One-line human-readable summary of what this invocation will do,
    /// shown to the user in the permission prompt.
    fn permission_context(&self, input: &serde_json::Value) -> String;
    async fn execute(&self, input: serde_json::Value) -> Result<String>;
}

// ── Registry ──────────────────────────────────────────────────────────────────

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut r = Self { tools: HashMap::new() };
        r.register(Arc::new(ReadFileTool));
        r.register(Arc::new(EditFileTool));
        r.register(Arc::new(WriteFileTool));
        r.register(Arc::new(ShellTool));
        r.register(Arc::new(GitStatusTool));
        r.register(Arc::new(SearchCodeTool));
        r.register(Arc::new(ListDirectoryTool));
        r
    }

    fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Returns a cloned Arc so callers can move the tool into async tasks.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Returns tool definitions in the shape the Anthropic API expects.
    pub fn tool_definitions(&self) -> Vec<serde_json::Value> {
        let mut defs: Vec<serde_json::Value> = self
            .tools
            .values()
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "input_schema": t.input_schema(),
                })
            })
            .collect();
        // Stable order so the model sees a consistent tool list.
        defs.sort_by_key(|d| d["name"].as_str().unwrap_or("").to_string());
        defs
    }
}

// ── read_file ─────────────────────────────────────────────────────────────────

struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str { "read_file" }
    fn description(&self) -> &str {
        "Read a file's contents, with optional line range. \
         Output is prefixed with 1-based line numbers (same as cat -n) so you \
         can reference exact lines in subsequent edit_file calls. \
         For large files, use offset + limit to read only the relevant section."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":   { "type": "string", "description": "Path to the file to read." },
                "offset": { "type": "integer", "description": "First line to read (0-based, default 0)." },
                "limit":  { "type": "integer", "description": "Maximum number of lines to return." }
            },
            "required": ["path"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("read '{}'", input["path"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path = input["path"]
            .as_str()
            .context("read_file: 'path' must be a string")?;

        let raw = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("read_file: cannot read '{}'", path))?;

        let offset = input["offset"].as_u64().unwrap_or(0) as usize;
        let limit  = input["limit"].as_u64().map(|l| l as usize);

        let lines: Vec<&str> = raw.lines().collect();
        let total = lines.len();
        let start = offset.min(total);
        let end   = limit.map(|l| (start + l).min(total)).unwrap_or(total);

        if start == end {
            return Ok(format!("(file '{}' has {} lines; offset {} is past the end)", path, total, offset));
        }

        // Prefix each line with its 1-based line number so the model can
        // reference exact lines in edit_file calls.
        let out = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{}\t{}", start + i + 1, line))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(out)
    }
}

// ── edit_file ─────────────────────────────────────────────────────────────────

struct EditFileTool;

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str { "edit_file" }
    fn description(&self) -> &str {
        "Make a surgical edit to a file by replacing an exact string with a new one. \
         The old_string must match exactly (including whitespace and indentation). \
         If old_string appears more than once and replace_all is false, the call is \
         rejected — add more surrounding context to make it unique."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":        { "type": "string",  "description": "File to edit." },
                "old_string":  { "type": "string",  "description": "Exact text to find and replace." },
                "new_string":  { "type": "string",  "description": "Text to replace it with." },
                "replace_all": { "type": "boolean", "description": "Replace every occurrence (default false)." }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("edit '{}': replace old_string with new_string",
            input["path"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path = input["path"]
            .as_str()
            .context("edit_file: 'path' must be a string")?;
        let old_string = input["old_string"]
            .as_str()
            .context("edit_file: 'old_string' must be a string")?;
        let new_string = input["new_string"]
            .as_str()
            .context("edit_file: 'new_string' must be a string")?;
        let replace_all = input["replace_all"].as_bool().unwrap_or(false);

        let content = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("edit_file: cannot read '{}'", path))?;

        let count = content.matches(old_string).count();
        if count == 0 {
            anyhow::bail!(
                "edit_file: old_string not found in '{}'. \
                 Make sure the text matches exactly (including whitespace and indentation).",
                path
            );
        }
        if count > 1 && !replace_all {
            anyhow::bail!(
                "edit_file: old_string appears {} times in '{}'. \
                 Add more surrounding context to make it unique, or set replace_all=true.",
                count, path
            );
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        tokio::fs::write(path, &new_content)
            .await
            .with_context(|| format!("edit_file: cannot write '{}'", path))?;

        Ok(format!(
            "edited '{}': replaced {} occurrence(s) ({} → {} bytes)",
            path, count, content.len(), new_content.len()
        ))
    }
}

// ── write_file ────────────────────────────────────────────────────────────────

struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str { "write_file" }
    fn description(&self) -> &str {
        "Write content to a file, creating it or overwriting it. \
         Requires user approval before executing."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":    { "type": "string", "description": "Destination file path." },
                "content": { "type": "string", "description": "Content to write." }
            },
            "required": ["path", "content"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        let path = input["path"].as_str().unwrap_or("?");
        let bytes = input["content"].as_str().map(|s| s.len()).unwrap_or(0);
        format!("write {} bytes to '{}'", bytes, path)
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path = input["path"]
            .as_str()
            .context("write_file: 'path' must be a string")?;
        let content = input["content"]
            .as_str()
            .context("write_file: 'content' must be a string")?;

        // Create parent directories if they don't exist.
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("write_file: cannot create dirs for '{}'", path))?;
            }
        }

        tokio::fs::write(path, content)
            .await
            .with_context(|| format!("write_file: cannot write '{}'", path))?;

        Ok(format!("wrote {} bytes to '{}'", content.len(), path))
    }
}

// ── shell ─────────────────────────────────────────────────────────────────────

struct ShellTool;

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str { "shell" }
    fn description(&self) -> &str {
        "Execute a shell command and return its stdout + stderr. \
         Requires user approval before executing. Timeout: 30 s."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command":     { "type": "string",  "description": "Shell command to run." },
                "description": { "type": "string",  "description": "One-line human-readable description of what this command does." },
                "timeout":     { "type": "integer", "description": "Timeout in seconds (default 30)." }
            },
            "required": ["command"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        let cmd = input["command"].as_str().unwrap_or("?");
        if let Some(desc) = input["description"].as_str() {
            format!("{}\n         $ {}", desc, cmd)
        } else {
            format!("$ {}", cmd)
        }
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let command = input["command"]
            .as_str()
            .context("shell: 'command' must be a string")?;

        let out = crate::shell_runner::run_command(command).await?;
        let mut result = String::new();
        if !out.stdout.is_empty() {
            result.push_str(&out.stdout);
        }
        if !out.stderr.is_empty() {
            result.push_str(&format!("\n[stderr]\n{}", out.stderr));
        }
        if out.exit_code != 0 {
            result.push_str(&format!("\n[exit code: {}]", out.exit_code));
        }
        Ok(result)
    }
}

// ── git_status ────────────────────────────────────────────────────────────────

struct GitStatusTool;

#[async_trait]
impl Tool for GitStatusTool {
    fn name(&self) -> &str { "git_status" }
    fn description(&self) -> &str {
        "Return the current git status and recent log of the working directory."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to run git status in (default: current dir)."
                }
            }
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("git status in '{}'", input["path"].as_str().unwrap_or("."))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let dir = input["path"].as_str().unwrap_or(".");
        let status = crate::shell_runner::run_args_in(
            "git", &["status", "--short"], dir,
        )
        .await?;
        let log = crate::shell_runner::run_args_in(
            "git", &["log", "--oneline", "-10"], dir,
        )
        .await?;

        let mut out = format!("## git status\n{}", status.stdout.trim());
        if !log.stdout.is_empty() {
            out.push_str(&format!("\n\n## recent commits\n{}", log.stdout.trim()));
        }
        Ok(out)
    }
}

// ── search_code ───────────────────────────────────────────────────────────────

struct SearchCodeTool;

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
                "pattern":      { "type": "string", "description": "Regex pattern to search for." },
                "path":         { "type": "string", "description": "Root directory to search (default: .)." },
                "file_pattern": { "type": "string", "description": "Glob for file names, e.g. '*.rs' (optional)." }
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
        let pattern = input["pattern"]
            .as_str()
            .context("search_code: 'pattern' must be a string")?;
        let path = input["path"].as_str().unwrap_or(".");
        let file_pattern = input["file_pattern"].as_str();

        // Build args list without any shell interpolation.
        let mut args: Vec<&str> = vec!["-rn", "--color=never", "-m", "50"];
        if let Some(fp) = file_pattern {
            args.push("--include");
            args.push(fp);
        }
        args.push(pattern);
        args.push(path);

        let out = crate::shell_runner::run_args("grep", &args).await?;
        if out.stdout.is_empty() {
            Ok(format!("no matches for '{}' in '{}'", pattern, path))
        } else {
            Ok(out.stdout)
        }
    }
}

// ── list_directory ────────────────────────────────────────────────────────────

struct ListDirectoryTool;

#[async_trait]
impl Tool for ListDirectoryTool {
    fn name(&self) -> &str { "list_directory" }
    fn description(&self) -> &str {
        "List files and directories at the given path."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory to list (default: .)." }
            }
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("ls '{}'", input["path"].as_str().unwrap_or("."))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path = input["path"].as_str().unwrap_or(".");
        let out = crate::shell_runner::run_args("ls", &["-la", path]).await?;
        Ok(out.stdout)
    }
}
