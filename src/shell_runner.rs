use anyhow::{Context, Result};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Default timeout for foreground commands. Long-running tasks should use
/// background processes (`nohup cmd > /dev/null 2>&1 &`) and return immediately.
const COMMAND_TIMEOUT_SECS: u64 = 60;

pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Run an arbitrary shell string.
/// Uses PowerShell on Windows, `sh -c` everywhere else.
/// Only use this for the user-facing `shell` tool.  Internal tools must use
/// `run_args` / `run_args_in` to avoid shell-injection.
pub async fn run_command(command: &str) -> Result<ShellOutput> {
    tracing::info!(command = %command, "executing shell command");
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", command]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    };
    run_with_timeout(&mut cmd).await
}

/// Run a program with explicit arguments — no shell, no injection risk.
pub async fn run_args(program: &str, args: &[&str]) -> Result<ShellOutput> {
    tracing::debug!(program = %program, ?args, "executing command");
    let mut cmd = Command::new(program);
    cmd.args(args);
    run_with_timeout(&mut cmd).await
}

/// Run a program with explicit arguments inside a specific working directory.
pub async fn run_args_in(program: &str, args: &[&str], dir: &str) -> Result<ShellOutput> {
    tracing::debug!(program = %program, ?args, dir = %dir, "executing command in dir");
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.current_dir(dir);
    run_with_timeout(&mut cmd).await
}

async fn run_with_timeout(cmd: &mut Command) -> Result<ShellOutput> {
    // kill_on_drop(true): if this future is cancelled (e.g. Ctrl+C in the REPL),
    // tokio sends SIGKILL to the child so it doesn't linger.
    let child = cmd
        .kill_on_drop(true)
        .output(); // returns a future; child is owned inside it

    match timeout(Duration::from_secs(COMMAND_TIMEOUT_SECS), child).await {
        Ok(Ok(output)) => Ok(ShellOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        }),
        Ok(Err(e)) => Err(e).context("command execution failed"),
        Err(_) => Err(anyhow::anyhow!(
            "command timed out after {}s\n\
             Tip: for long-running processes use: nohup <cmd> > /tmp/out.log 2>&1 &",
            COMMAND_TIMEOUT_SECS
        )),
    }
}
