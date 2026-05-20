/// Ratatui TUI for zap — full-screen interactive mode.
///
/// Entry point: `run_tui(config)`.
/// Channel module provides global TUI event sender for session/stream_highlighter.
pub mod app;
pub mod channel;
pub mod commands;
pub mod input;
pub mod render;
pub mod syntax;
pub mod file_browser;

use std::io::Stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc::UnboundedReceiver;

use app::{App, AppState, MsgRole, SessionEntry, SessionPickerState, UiBlock, UiMessage};
use channel::TuiEvent;
use input::{handle_key, InputAction};

use crate::config::Config;
use crate::session::Session;

pub async fn run_tui(config: &Config) -> Result<()> {
    // 1. Create session BEFORE entering the alternate screen so that all startup
    //    println!s (skills loaded, hooks, MCP, code index) go to the normal
    //    terminal buffer and are visible briefly before the TUI takes over.
    //    Skip the CLI domain-scope prompt — we show a TUI picker instead.
    let mut tui_config = config.clone();
    tui_config.skip_domain_prompt = true;
    let config = &tui_config;
    let mut session = Session::new(config).await?;
    session.hooks.fire_session_start();

    // 2. Mode picker — runs on normal terminal before alternate screen.
    let mode = crate::task_planner::pick_session_mode();
    let mut task_intro: Option<String> = None;
    if mode == crate::task_planner::SessionMode::Task {
        match crate::task_planner::run_task_planning(
            session.client.as_ref(),
            &session.model,
            &session.skills,
        )
        .await
        {
            Ok(Some(plan)) => {
                task_intro = Some(format!(
                    "I'm starting a task session. Goal: {}\n\n\
                     The tasks.md has been created at .zap/tasks/{}/tasks.md\n\
                     Please read it and confirm you understand the plan before we start.",
                    plan.goal, plan.folder_name
                ));
            }
            Ok(None) => {}
            Err(e) => {
                use colored::Colorize;
                println!("  {} Planning failed: {} — continuing in Vibe mode.", "⚠".yellow(), e);
            }
        }
    }

    // Fetch branch while still in the normal terminal.
    let branch = git_branch();

    // 2. Set up the TUI event channel (is_tui_mode() becomes true from here).
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TuiEvent>();
    channel::set_tui_sender(tx.clone());

    // 3. Switch to alternate screen.
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // 4. Create App state.
    let mut app = App::new(&session.model, &branch);
    app.skill_names = session.skills.iter().map(|s| s.name.clone()).collect();

    // Show domain picker when no scope was auto-detected and domain skills exist.
    if session.domain_scope.is_empty() {
        let domain_options = crate::skill_manager::all_domain_skill_names(&session.skills);
        if !domain_options.is_empty() {
            let project_name = std::env::current_dir()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| ".".to_string());
            let mut picker = app::DomainPickerState::new(domain_options, project_name);
            // Pre-check anything we can detect from file extensions.
            let ext_detected = crate::skill_manager::detect_from_extensions(&session.skills);
            for (i, opt) in picker.options.iter().enumerate() {
                if ext_detected.contains(opt) {
                    picker.checked[i] = true;
                }
            }
            app.domain_picker = Some(picker);
        }
    }
    let (dirty, ahead, behind) = git_status();
    app.git_dirty = dirty;
    app.git_ahead = ahead;
    app.git_behind = behind;

    // Show welcome message in conversation area
    let mode_hint = if task_intro.is_some() { " · task mode" } else { " · vibe mode" };
    app.messages.push(UiMessage {
        role: MsgRole::Assistant,
        blocks: vec![UiBlock::Text(format!(
            "Ready. {} tools loaded{}. Type your message or / for commands.",
            session.tool_count, mode_hint
        ))],
    });

    // If task mode, prime the first turn so zap reads the tasks.md immediately.
    if let Some(intro) = task_intro {
        app.messages.push(UiMessage {
            role: MsgRole::User,
            blocks: vec![UiBlock::Text("Starting task session…".to_string())],
        });
        app.pending_input = Some(intro);
    }

    // 5. Main event loop
    let result = tui_loop(&mut terminal, &mut app, &mut session, config, &mut rx).await;

    // 6. Cleanup — always restore terminal even on error
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    );
    let _ = terminal.show_cursor();

    session.hooks.fire_session_end();
    result
}

async fn tui_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    session: &mut Session,
    config: &Config,
    rx: &mut UnboundedReceiver<TuiEvent>,
) -> Result<()> {
    loop {
        // Drain TUI events
        while let Ok(ev) = rx.try_recv() {
            app.apply_event(ev);
        }

        // Draw
        terminal.draw(|frame| render::draw(frame, app))?;

        // Handle pending input (user just pressed Enter)
        if let Some(input) = app.pending_input.take() {
            if input.starts_with('/') {
                // /sessions → open TUI-native session picker instead of dropping to CLI.
                let cmd = input.trim();
                if cmd == "/sessions" || cmd.starts_with("/sessions ") {
                    match session.store.recent_sessions(30) {
                        Ok(rows) => {
                            app.session_picker = Some(SessionPickerState {
                                entries: rows.iter().map(|(id, goal, model, ts)| SessionEntry {
                                    id:    *id,
                                    goal:  goal.clone(),
                                    model: model.clone(),
                                    date:  ts.get(..10).unwrap_or(ts).to_string(),
                                }).collect(),
                                selected: 0,
                            });
                        }
                        Err(e) => {
                            app.error = Some(format!("sessions: {e}"));
                        }
                    }
                    continue;
                }

                // 1. Try native inline handler (output rendered in chat area).
                if let Some(text) = commands::handle_inline(session, &input, config) {
                    if !text.is_empty() {
                        app.messages.push(UiMessage {
                            role: MsgRole::Assistant,
                            blocks: vec![UiBlock::Text(text)],
                        });
                        app.auto_scroll = true;
                        // Force immediate redraw so the response is visible now.
                        terminal.draw(|frame| render::draw(frame, app))?;
                    }
                    app.branch = git_branch();
                    let (dirty, ahead, behind) = git_status();
                    app.git_dirty = dirty;
                    app.git_ahead = ahead;
                    app.git_behind = behind;
                    // Keep skill_names in sync (e.g. after /skill list reloads from disk).
                    if input.trim_start().starts_with("/skill") {
                        app.skill_names = session.skills.iter().map(|s| s.name.clone()).collect();
                    }
                    // If /cd succeeded, update cwd and push to recent_dirs.
                    if input.trim_start().starts_with("/cd ") {
                        let new_cwd = std::env::current_dir()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|_| "?".to_string());
                        if new_cwd != app.cwd {
                            let old = app.cwd.clone();
                            app.cwd = new_cwd;
                            app.recent_dirs.insert(0, old);
                            app.recent_dirs.dedup();
                            app.recent_dirs.truncate(4);
                        }
                    }
                } else if input.trim() == "/exit" {
                    break;
                } else {
                    // 2. Complex command — suspend TUI, run, wait for Enter.
                    suspend_tui(terminal)?;
                    let should_exit = session.handle_slash(&input, config).await;
                    if !should_exit {
                        use std::io::Write;
                        println!();
                        print!("  \x1b[2m── Press Enter to return to zap ──\x1b[0m ");
                        std::io::stdout().flush().ok();
                        let mut buf = String::new();
                        std::io::stdin().read_line(&mut buf).ok();
                    }
                    resume_tui(terminal)?;
                    app.branch = git_branch();
                    let (dirty, ahead, behind) = git_status();
                    app.git_dirty = dirty;
                    app.git_ahead = ahead;
                    app.git_behind = behind;
                    if should_exit { break; }
                }
            } else {
                // Normal message — run session turn with 16ms tick for animation
                app.state = AppState::Thinking;
                app.auto_scroll = true;

                {
                    let turn_fut = session.handle_user_turn(&input);
                    tokio::pin!(turn_fut);
                    let mut done = false;

                    while !done {
                        let tick = tokio::time::sleep(Duration::from_millis(16));
                        tokio::select! {
                            result = &mut turn_fut, if !done => {
                                if let Err(e) = result {
                                    app.error = Some(e.to_string());
                                }
                                done = true;
                            }
                            _ = tick => {
                                // Animate + drain events + redraw
                                while let Ok(ev) = rx.try_recv() {
                                    app.apply_event(ev);
                                }
                                app.tick_spinner();
                                terminal.draw(|frame| render::draw(frame, app))?;

                                // Check for Ctrl+C — skip while permission dialog owns the queue.
                                if !channel::is_permission_prompt_active()
                                    && crossterm::event::poll(Duration::ZERO)?
                                {
                                    if let Ok(Event::Key(k)) = crossterm::event::read() {
                                        if k.code == KeyCode::Char('c')
                                            && k.modifiers.contains(KeyModifiers::CONTROL)
                                        {
                                            done = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                } // turn_fut dropped here, releasing mutable borrow of session

                // Drain ALL remaining events before finalizing so that
                // late-arriving LlmChunk events are folded into the message
                // and don't re-set state to Thinking after we set it to Idle.
                while let Ok(ev) = rx.try_recv() {
                    app.apply_event(ev);
                }
                app.finalize_turn();
                app.state = AppState::Idle;
                // Discard any stragglers that arrived in the gap after drain.
                while rx.try_recv().is_ok() {}
                // Update context % from session (safe now that turn_fut is dropped)
                app.context_pct = session.context_fill_pct();
                app.turn = session.turn_count;
            }
            continue;
        }

        // Idle: wait for terminal event (50ms timeout to keep spinner alive)
        if crossterm::event::poll(Duration::from_millis(50))? {
            match crossterm::event::read()? {
                // Skip Release events — on Windows crossterm fires Press+Release for every
                // key, which would insert each character twice without this guard.
                Event::Key(key)
                    if key.kind != crossterm::event::KeyEventKind::Release =>
                {
                    match handle_key(app, key) {
                        InputAction::Quit => break,
                        InputAction::Submit(text) => {
                            // Add user message to display immediately
                            app.messages.push(UiMessage {
                                role: MsgRole::User,
                                blocks: vec![UiBlock::Text(text.clone())],
                            });
                            app.pending_input = Some(text);
                        }
                        InputAction::Slash(cmd) => {
                            app.messages.push(UiMessage {
                                role: MsgRole::User,
                                blocks: vec![UiBlock::Text(cmd.clone())],
                            });
                            app.pending_input = Some(cmd);
                        }
                        InputAction::Cancel => { /* handled during turn */ }
                        InputAction::ScrollUp(n) => {
                            app.scroll_up(n);
                        }
                        InputAction::ScrollDown(n) => {
                            let sz = terminal.size()?;
                            let sidebar_w: u16 = if sz.width > render::SIDEBAR_W + 20 { render::SIDEBAR_W + 1 } else { 0 };
                            let chat_w = sz.width.saturating_sub(sidebar_w);
                            let viewport_h = sz.height as usize;
                            let total = app.total_lines(chat_w);
                            app.scroll_down(n, viewport_h.saturating_sub(3), total);
                        }
                        InputAction::OpenDirPicker => {
                            suspend_tui(terminal)?;
                            let chosen = open_dir_picker();
                            
                            // Log what we got from the picker
                            if let Some(ref dir) = chosen {
                                use std::io::Write;
                                let log_path = dirs::home_dir()
                                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                                    .join(".zap")
                                    .join("zap.log");
                                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
                                    let _ = writeln!(f, "[{}] DEBUG Picker returned: {:?}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S"), dir);
                                    let _ = writeln!(f, "[{}] DEBUG Path exists: {}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S"), std::path::Path::new(dir).exists());
                                    let _ = writeln!(f, "[{}] DEBUG Current dir before change: {:?}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S"), std::env::current_dir());
                                }
                            }
                            
                            resume_tui(terminal)?;
                            
                            if let Some(dir) = chosen {
                                // Try to change directory
                                match std::env::set_current_dir(&dir) {
                                    Ok(()) => {
                                        // Successfully changed, get the new path
                                        let new_cwd = std::env::current_dir()
                                            .map(|p| p.display().to_string())
                                            .unwrap_or_else(|_| dir.clone());
                                        
                                        // Log the result
                                        use std::io::Write;
                                        let log_path = dirs::home_dir()
                                            .unwrap_or_else(|| std::path::PathBuf::from("."))
                                            .join(".zap")
                                            .join("zap.log");
                                        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
                                            let _ = writeln!(f, "[{}] DEBUG Successfully changed to: {:?}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S"), new_cwd);
                                            let _ = writeln!(f, "[{}] DEBUG Verifying with getcwd: {:?}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S"), std::env::current_dir());
                                            // Also test with a shell command
                                            if let Ok(output) = std::process::Command::new("pwd").output() {
                                                let _ = writeln!(f, "[{}] DEBUG pwd returns: {:?}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S"), String::from_utf8_lossy(&output.stdout).trim());
                                            }
                                        }
                                        
                                        if new_cwd != app.cwd {
                                            let old = app.cwd.clone();
                                            app.cwd = new_cwd.clone();
                                            app.recent_dirs.insert(0, old);
                                            app.recent_dirs.dedup();
                                            app.recent_dirs.truncate(4);
                                        }
                                        
                                        app.branch = git_branch();
                                        let (dirty, ahead, behind) = git_status();
                                        app.git_dirty = dirty;
                                        app.git_ahead = ahead;
                                        app.git_behind = behind;
                                        
                                        // Show success message
                                        app.messages.push(UiMessage {
                                            role: MsgRole::Assistant,
                                            blocks: vec![UiBlock::Text(format!(
                                                "Changed directory to: {}\n\nYou can now ask me about this codebase, or use /index to build a code symbol index.",
                                                new_cwd
                                            ))],
                                        });
                                        app.auto_scroll = true;
                                    }
                                    Err(e) => {
                                        // Log the error
                                        use std::io::Write;
                                        let log_path = dirs::home_dir()
                                            .unwrap_or_else(|| std::path::PathBuf::from("."))
                                            .join(".zap")
                                            .join("zap.log");
                                        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
                                            let _ = writeln!(f, "[{}] ERROR Failed to change to {:?}: {}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S"), dir, e);
                                        }
                                        
                                        // Show error message
                                        app.messages.push(UiMessage {
                                            role: MsgRole::Assistant,
                                            blocks: vec![UiBlock::Text(format!(
                                                "Failed to change directory to: {}\nError: {}",
                                                dir, e
                                            ))],
                                        });
                                        app.auto_scroll = true;
                                    }
                                }
                            }
                        }
                        InputAction::ToggleFileBrowser => {
                            if app.file_browser.is_some() {
                                app.file_browser = None;
                            } else {
                                // Open file browser at current directory
                                let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                                match file_browser::FileBrowser::new(cwd) {
                                    Ok(browser) => {
                                        app.file_browser = Some(browser);
                                        // Load initial preview
                                        if let Some(ref mut browser) = app.file_browser {
                                            let _ = browser.load_preview();
                                        }
                                    }
                                    Err(e) => {
                                        app.error = Some(format!("Failed to open file browser: {}", e));
                                    }
                                }
                            }
                        }
                        InputAction::LoadSession(sid) => {
                            match session.store.load_messages(sid) {
                                Ok(Some(json)) => {
                                    match serde_json::from_str::<Vec<crate::llm_client::Message>>(&json) {
                                        Ok(msgs) => {
                                            let count = msgs.len();
                                            let turns = msgs.iter().filter(|m| m.role == "user").count();
                                            session.messages   = msgs;
                                            session.turn_count = turns;
                                            session.session_id = sid;
                                            app.messages.push(UiMessage {
                                                role: MsgRole::Assistant,
                                                blocks: vec![UiBlock::Text(format!(
                                                    "Resumed session #{sid} — {count} messages, {turns} turns."
                                                ))],
                                            });
                                            app.auto_scroll = true;
                                        }
                                        Err(e) => app.error = Some(format!("session parse error: {e}")),
                                    }
                                }
                                Ok(None) => app.error = Some(format!("No messages for session #{sid}")),
                                Err(e)   => app.error = Some(format!("load session: {e}")),
                            }
                        }
                        InputAction::CloseSessionPicker => {}
                        InputAction::ClearInput => {}
                        InputAction::ToggleLastToolExpand => {
                            if let Some(id) = last_tool_id_with_result(app) {
                                if app.expanded_tools.contains(&id) {
                                    app.expanded_tools.remove(&id);
                                } else {
                                    app.expanded_tools.insert(id);
                                }
                            }
                        }
                        InputAction::ConfirmDomainScope(names) => {
                            session.domain_scope = names.into_iter().collect();
                        }
                        InputAction::None => {}
                    }
                }
                Event::Resize(_, _) => {
                    terminal.autoresize()?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn git_branch() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Get git repository status (dirty, ahead, behind).
fn git_status() -> (bool, usize, usize) {
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    
    let (ahead, behind) = std::process::Command::new("git")
        .args(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let output = String::from_utf8(o.stdout).ok()?;
                let parts: Vec<&str> = output.trim().split_whitespace().collect();
                if parts.len() == 2 {
                    let ahead = parts[0].parse().ok()?;
                    let behind = parts[1].parse().ok()?;
                    return Some((ahead, behind));
                }
            }
            None
        })
        .unwrap_or((0, 0));
    
    (dirty, ahead, behind)
}

fn suspend_tui(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn resume_tui(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::EnterAlternateScreen
    )?;
    terminal.clear()?;
    Ok(())
}

/// Open a native folder picker dialog and return the chosen path.
/// Returns None if the user cancels or if the platform is not supported.
fn open_dir_picker() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        
        // Get current directory to use as default
        let current_dir = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "/".to_string());
        
        // Use AppleScript to open folder picker with current directory as default
        // Run with explicit stdin/stdout/stderr to avoid terminal interference
        let script = format!(
            r#"POSIX path of (choose folder with prompt "Select a directory:" default location POSIX file "{}")"#,
            current_dir
        );
        
        let output = Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .ok()?;
        
        if output.status.success() {
            let path = String::from_utf8(output.stdout).ok()?;
            let path = path.trim().trim_end_matches('/'); // Remove trailing slash and whitespace
            
            // Log the raw output for debugging
            use std::io::Write;
            let log_path = dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".zap")
                .join("zap.log");
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
                let _ = writeln!(f, "[{}] DEBUG osascript raw output: {:?}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S"), path);
                let _ = writeln!(f, "[{}] DEBUG osascript stderr: {:?}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S"), String::from_utf8_lossy(&output.stderr));
                let _ = writeln!(f, "[{}] DEBUG current_dir used: {:?}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S"), current_dir);
            }
            
            if !path.is_empty() {
                // Verify the path exists before returning
                if std::path::Path::new(path).exists() {
                    return Some(path.to_string());
                }
            }
        } else {
            // Log error
            use std::io::Write;
            let log_path = dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".zap")
                .join("zap.log");
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
                let _ = writeln!(f, "[{}] ERROR osascript failed: {:?}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S"), String::from_utf8_lossy(&output.stderr));
            }
        }
        None
    }
    
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        
        // Use PowerShell's folder browser dialog
        let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'Select a directory'
$dialog.ShowNewFolderButton = $true
if ($dialog.ShowDialog() -eq 'OK') {
    Write-Output $dialog.SelectedPath
}
"#;
        
        let output = Command::new("powershell")
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

/// Walk all completed messages to find the last tool call that has a result.
/// Returns its ID so the caller can toggle it in `app.expanded_tools`.
fn last_tool_id_with_result(app: &App) -> Option<String> {
    for msg in app.messages.iter().rev() {
        for block in msg.blocks.iter().rev() {
            if let UiBlock::Tool(tc) = block {
                if tc.result.is_some() {
                    return Some(tc.id.clone());
                }
            }
        }
    }
    None
}
