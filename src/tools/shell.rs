use anyhow::Result;
use async_trait::async_trait;

use super::Tool;

// ── Hard-blocked patterns (rejected unconditionally, even with explicit approval) ─

/// Commands matching any of these substrings are refused outright — no prompt,
/// no override. They cover filesystem nukes, pipe-to-shell code-injection,
/// reverse shells, and disk-level destruction.
const BLOCKED_PATTERNS: &[&str] = &[
    // Filesystem nukes
    "rm -rf /",
    "rm -rf ~/",
    "rm -rf ~",
    "rm -rf $HOME",
    "rm -rf ${HOME}",
    "rm --no-preserve-root",

    // Fork bomb
    ":(){ :|:&};:",

    // Raw disk writes / format
    "mkfs",
    "dd if=",
    "> /dev/sd",
    "> /dev/hd",
    "> /dev/nvme",
    "shred /dev/",
    "wipe /dev/",

    // Pipe-to-shell (download + execute)
    "| sh\n", "| sh ",  "| sh\"", "|sh",
    "| bash\n","| bash ","| bash\"","|bash",
    "| zsh\n", "| zsh ", "| zsh\"", "|zsh",
    "| fish\n","| fish ","| fish\"","|fish",
    "| python\n","| python ","| python3\n","| python3 ","|python","|python3",
    "| perl\n", "| perl ", "|perl",
    "| ruby\n", "| ruby ", "|ruby",
    "| node\n", "| node ", "|node",
    "curl | ", "curl|", "wget | ", "wget|",

    // Base64-decode-then-execute
    "base64 -d |", "base64 --decode |",
    "base64 -d|",  "base64 --decode|",
    "openssl base64 -d",

    // Reverse shells
    "/dev/tcp/",
    "/dev/udp/",
    "bash -i >&",
    "bash -i >",
    "nc -e /bin",
    "ncat -e /bin",
    "netcat -e /bin",
    "0>&1",

    // Kernel modules
    "insmod ",
    "modprobe ",

    // Boot partition
    "> /boot/",

    // Anti-forensics (history wiping)
    "history -c",
    "unset histfile",          // matched case-insensitively via to_lowercase()
    "export histfile=/dev/null",
    "export histsize=0",
    "export histfilesize=0",
];

// ── Destructive patterns — require explicit confirmation even in auto mode ─────

/// (pattern, human-readable reason) pairs checked case-insensitively.
/// Commands matching any of these must be confirmed by the user even when the
/// permission mode is Auto. They are still audited and logged.
pub const DESTRUCTIVE_PATTERNS: &[(&str, &str)] = &[
    ("rm -rf ",         "recursive forced deletion"),
    ("rm -fr ",         "recursive forced deletion"),
    ("git push --force","force-push overwrites remote history"),
    ("git push -f ",    "force-push overwrites remote history"),
    ("git push -f\n",   "force-push overwrites remote history"),
    ("sudo ",           "superuser privilege escalation"),
    ("drop table",      "SQL table deletion"),
    ("drop database",   "SQL database deletion"),
    ("truncate table",  "SQL table truncation"),
    ("chmod -r 000",    "recursive permission removal"),
    ("chmod -r 777",    "recursive world-writable permission"),
    ("chown -r root",   "recursive ownership change to root"),
];

/// Returns Some(reason) if `command` matches a destructive pattern that requires
/// explicit user confirmation even in Auto mode. None means the command is safe
/// to run without extra confirmation (subject to the hard-blocked list).
pub fn destructive_pattern(command: &str) -> Option<&'static str> {
    let lower = command.to_lowercase();
    for (pat, reason) in DESTRUCTIVE_PATTERNS {
        if lower.contains(pat) {
            return Some(reason);
        }
    }
    None
}

fn guard_shell(command: &str) -> Result<()> {
    let lower = command.to_lowercase();
    for pat in BLOCKED_PATTERNS {
        if lower.contains(pat) {
            anyhow::bail!(
                "shell: command blocked — matches prohibited pattern '{}'.\n\
                 Destructive or pipe-to-shell commands cannot run.",
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
    fn shows_inline_output(&self) -> bool { true }
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
    fn shows_inline_output(&self) -> bool { true }
}

// ── git_pull ──────────────────────────────────────────────────────────────────

pub(super) struct GitPullTool;

#[async_trait]
impl Tool for GitPullTool {
    fn name(&self) -> &str { "git_pull" }
    fn description(&self) -> &str {
        "Run `git pull` to fetch and merge the latest changes from the remote. \
         Use when the user asks to pull, sync, update, or get latest changes."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Repo directory (default: current dir)." },
                "rebase": { "type": "boolean", "description": "Use --rebase instead of merge." }
            }
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        let dir = input["path"].as_str().unwrap_or(".");
        let flag = if input["rebase"].as_bool().unwrap_or(false) { " --rebase" } else { "" };
        format!("git pull{} in '{}'", flag, dir)
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let dir = input["path"].as_str().unwrap_or(".");
        let rebase = input["rebase"].as_bool().unwrap_or(false);
        let args: &[&str] = if rebase { &["pull", "--rebase"] } else { &["pull"] };
        let out = crate::shell_runner::run_args_in("git", args, dir).await?;
        let combined = format!("{}{}", out.stdout, out.stderr).trim().to_string();
        if out.exit_code != 0 {
            Ok(format!("{}\n[exit code: {}]", combined, out.exit_code))
        } else {
            Ok(if combined.is_empty() { "Already up to date.".to_string() } else { combined })
        }
    }
    fn shows_inline_output(&self) -> bool { true }
}

// ── git_diff ──────────────────────────────────────────────────────────────────

pub(super) struct GitDiffTool;

#[async_trait]
impl Tool for GitDiffTool {
    fn name(&self) -> &str { "git_diff" }
    fn description(&self) -> &str {
        "Show a git diff. Supports unstaged changes, staged changes, or diff between refs/branches."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":   { "type": "string", "description": "Repo directory (default: current dir)." },
                "staged": { "type": "boolean", "description": "Show staged (--cached) diff." },
                "ref":    { "type": "string",  "description": "Ref, branch, or range e.g. 'main' or 'HEAD~3..HEAD'." }
            }
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        let r = input["ref"].as_str().unwrap_or("");
        let staged = input["staged"].as_bool().unwrap_or(false);
        if staged { "git diff --cached".to_string() }
        else if !r.is_empty() { format!("git diff {}", r) }
        else { "git diff".to_string() }
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let dir = input["path"].as_str().unwrap_or(".");
        let staged = input["staged"].as_bool().unwrap_or(false);
        let refspec = input["ref"].as_str().unwrap_or("");

        let mut args = vec!["diff"];
        if staged { args.push("--cached"); }
        if !refspec.is_empty() { args.push(refspec); }

        let out = crate::shell_runner::run_args_in("git", &args, dir).await?;
        if out.stdout.trim().is_empty() && out.stderr.trim().is_empty() {
            Ok("No differences.".to_string())
        } else {
            Ok(format!("{}{}", out.stdout, out.stderr))
        }
    }
    fn shows_inline_output(&self) -> bool { true }
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
