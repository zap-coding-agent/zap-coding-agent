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

#[cfg(test)]
mod tests {
    use super::*;

    // ── destructive_pattern ───────────────────────────────────────────────────

    #[test]
    fn detects_rm_rf() {
        assert!(destructive_pattern("rm -rf build/").is_some());
        assert!(destructive_pattern("rm -fr /tmp/old").is_some());
    }

    #[test]
    fn detects_force_push() {
        assert!(destructive_pattern("git push --force origin main").is_some());
        assert!(destructive_pattern("git push -f origin main").is_some());
    }

    #[test]
    fn detects_sudo() {
        assert!(destructive_pattern("sudo apt install curl").is_some());
    }

    #[test]
    fn detects_sql_drops() {
        assert!(destructive_pattern("DROP TABLE users").is_some());
        assert!(destructive_pattern("drop database prod").is_some());
        assert!(destructive_pattern("TRUNCATE TABLE logs").is_some());
    }

    #[test]
    fn safe_commands_return_none() {
        assert!(destructive_pattern("cargo build").is_none());
        assert!(destructive_pattern("ls -la").is_none());
        assert!(destructive_pattern("git status").is_none());
        assert!(destructive_pattern("npm run test").is_none());
        assert!(destructive_pattern("rm file.txt").is_none()); // no -rf flag
    }

    // ── ShellTool::permission_context newline contract ────────────────────────
    // The TUI dialog sanitizes \n out of ctx. This test documents that the
    // source string does contain \n so we don't accidentally remove the sanitization.

    #[test]
    fn permission_context_with_description_contains_newline() {
        let tool = ShellTool;
        let input = serde_json::json!({
            "command": "npm install",
            "description": "install dependencies"
        });
        let ctx = tool.permission_context(&input);
        assert!(ctx.contains('\n'), "ctx with description must contain \\n for sanitization to matter");
    }

    #[test]
    fn permission_context_without_description_has_no_newline() {
        let tool = ShellTool;
        let input = serde_json::json!({ "command": "npm install" });
        let ctx = tool.permission_context(&input);
        assert!(!ctx.contains('\n'));
        assert!(ctx.starts_with("$ "));
    }
}
