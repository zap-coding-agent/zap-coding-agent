use std::io::Stdout;

use anyhow::Result;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub(super) fn suspend_tui(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

pub(super) fn resume_tui(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::EnterAlternateScreen
    )?;
    terminal.clear()?;
    Ok(())
}

/// Open a native folder picker dialog and return the chosen path.
pub(super) fn open_dir_picker() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let current_dir = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "/".to_string());

        let script = format!(
            r#"POSIX path of (choose folder with prompt "Select a directory:" default location POSIX file "{}")"#,
            current_dir
        );

        let output = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .ok()?;

        if output.status.success() {
            let path = String::from_utf8(output.stdout).ok()?;
            let path = path.trim().trim_end_matches('/');
            if !path.is_empty() && std::path::Path::new(path).exists() {
                return Some(path.to_string());
            }
        }
        None
    }

    #[cfg(target_os = "windows")]
    {
        let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'Select a directory'
$dialog.ShowNewFolderButton = $true
if ($dialog.ShowDialog() -eq 'OK') {
    Write-Output $dialog.SelectedPath
}
"#;
        let output = std::process::Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(script)
            .output()
            .ok()?;

        if output.status.success() {
            let path = String::from_utf8(output.stdout).ok()?;
            let path = path.trim();
            if !path.is_empty() && std::path::Path::new(path).exists() {
                return Some(path.to_string());
            }
        }
        None
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Run task planning in the normal terminal (TUI suspended) and return the
/// intro message to prime the first TUI turn, or None if aborted.
pub(super) async fn run_task_planning_tui(session: &crate::session::Session) -> Option<String> {
    match crate::task_planner::run_task_planning(
        session.client.as_ref(),
        &session.model,
        &session.skills,
    )
    .await
    {
        Ok(Some(plan)) => {
            println!();
            print!("  \x1b[2m── Press Enter to enter task session ──\x1b[0m ");
            use std::io::Write;
            std::io::stdout().flush().ok();
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).ok();
            Some(format!(
                "I'm starting a task session. Goal: {}\n\n\
                 The tasks.md has been created at .zap/tasks/{}/tasks.md\n\
                 Please read it and confirm you understand the plan before we start.",
                plan.goal, plan.folder_name
            ))
        }
        Ok(None) => None,
        Err(e) => {
            use colored::Colorize;
            println!("  {} Planning failed: {} — continuing in Vibe mode.", "⚠".yellow(), e);
            use std::io::Write;
            std::io::stdout().flush().ok();
            None
        }
    }
}
