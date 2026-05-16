use anyhow::{Context, Result};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

const COMMAND_TIMEOUT_SECS: u64 = 30;

pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Run an arbitrary shell string via `sh -c`.
/// Only use this for the user-facing `shell` tool.  Internal tools must use
/// `run_args` / `run_args_in` to avoid shell-injection.
pub async fn run_command(command: &str) -> Result<ShellOutput> {
    tracing::info!(command = %command, "executing shell command");
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
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
    let output = timeout(Duration::from_secs(COMMAND_TIMEOUT_SECS), cmd.output())
        .await
        .context("command timed out after 30s")?
        .context("failed to spawn command")?;

    Ok(ShellOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}
