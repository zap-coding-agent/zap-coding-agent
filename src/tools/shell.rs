use anyhow::Result;
use async_trait::async_trait;

use super::Tool;

// ── Dangerous command patterns (blocked even in auto mode) ────────────────────

/// Substrings that indicate a destructive or exfiltration-prone command.
/// These are rejected before the command runs regardless of permission mode.
const BLOCKED_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf ~",
    "rm -rf $HOME",
    ":(){ :|:&};:",   // fork bomb
    "mkfs",
    "dd if=",
    "> /dev/sd",
    "| sh\n", "| sh ",  "| sh\"", "|sh",
    "| bash\n","| bash ","| bash\"","|bash",
    "| zsh\n", "| zsh ", "| zsh\"", "|zsh",
    "curl | ", "wget | ",
    "curl|",   "wget|",
];

fn guard_shell(command: &str) -> Result<()> {
    let lower = command.to_lowercase();
    for pat in BLOCKED_PATTERNS {
        if lower.contains(pat) {
            anyhow::bail!(
                "shell: command contains a blocked pattern '{}'. \
                 Destructive or pipe-to-shell commands cannot run automatically.",
                pat.trim()
            );
        }
    }
    Ok(())
}

// ── shell ─────────────────────────────────────────────────────────────────────

pub(super) struct ShellTool;

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
            .ok_or_else(|| anyhow::anyhow!("shell: 'command' must be a string"))?;
        guard_shell(command)?;

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

pub(super) struct GitStatusTool;

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
        ).await?;
        let log = crate::shell_runner::run_args_in(
            "git", &["log", "--oneline", "-10"], dir,
        ).await?;

        let mut out = format!("## git status\n{}", status.stdout.trim());
        if !log.stdout.is_empty() {
            out.push_str(&format!("\n\n## recent commits\n{}", log.stdout.trim()));
        }
        Ok(out)
    }
}

// ── list_directory ────────────────────────────────────────────────────────────

pub(super) struct ListDirectoryTool;

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
