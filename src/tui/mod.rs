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

use app::{App, AppState, GoalState, InitWizardState, InitWizardStep, ModePickerState, MsgRole, SessionEntry, SessionPickerState, StreamingBlock, UiBlock, UiMessage};
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
    tui_config.tui_mode = true;
    let config = &tui_config;
    let mut session = Session::new(config).await?;
    session.hooks.fire_session_start();

    // Mode picker is now handled inside the TUI (see mode_picker overlay).

    // Fetch branch while still in the normal terminal.
    let branch = git_branch();

    // 2. Set up the TUI event channel (is_tui_mode() becomes true from here).
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TuiEvent>();
    channel::set_tui_sender(tx.clone());
    channel::init_perm_channel();
    channel::init_secret_channel();

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

    let is_new_project = crate::project::load_project_meta().is_none();

    if is_new_project {
        // New project: init wizard first, mode picker queued for after wizard.
        let detected = crate::session::commands::detect_project_type().to_string();
        let cursor = detected.chars().count();
        app.init_wizard = Some(InitWizardState {
            step: InitWizardStep::Language,
            detected_language: detected.clone(),
            language_input: detected,
            language_cursor: cursor,
            do_index: false,
        });
        app.show_mode_picker_after_init = true;
    } else {
        // Returning project: mode picker right away, no init wizard.
        app.mode_picker = Some(ModePickerState { cursor: 0 });
    }

    // Queue domain picker only when language is unknown (domain_scope empty = no project.json language).
    if session.domain_scope.is_empty() && !is_new_project {
        let domain_options = crate::skill_manager::all_domain_skill_names(&session.skills);
        if !domain_options.is_empty() {
            let project_name = std::env::current_dir()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| ".".to_string());
            let mut picker = app::DomainPickerState::new(domain_options, project_name);
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

    // Build rich welcome line (replaces the startup println!s we suppressed).
    let skill_note = {
        let always_on_count = crate::skill_manager::always_on_skills(&session.skills).len();
        let practice_count = session.skills.iter()
            .filter(|s| s.category == crate::skill_manager::SkillCategory::Practice).count();
        let domain_count = session.skills.iter()
            .filter(|s| s.category == crate::skill_manager::SkillCategory::Domain).count();
        if session.skills.is_empty() {
            String::new()
        } else {
            format!("  ·  {} skills ({} core · {} practice · {} domain)",
                session.skills.len(), always_on_count, practice_count, domain_count)
        }
    };
    app.messages.push(UiMessage {
        role: MsgRole::Assistant,
        blocks: vec![UiBlock::Text(format!(
            "Ready. {} tools loaded{}.",
            session.tool_count, skill_note
        ))],
    });

    // Drain startup notices (context banner, init nudge) accumulated during Session::new().
    for notice in session.startup_notices.drain(..) {
        app.messages.push(UiMessage {
            role: MsgRole::Assistant,
            blocks: vec![UiBlock::Text(notice)],
        });
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

    session.save_context();
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

        // Inject any message arriving from remote control web UI.
        if app.pending_input.is_none() {
            if let Some(remote_msg) = crate::remote_channel::try_recv() {
                // Show it in the chat as a user bubble so it's visible.
                app.messages.push(crate::tui::app::UiMessage {
                    role:   crate::tui::app::MsgRole::User,
                    blocks: vec![crate::tui::app::UiBlock::Text(remote_msg.clone())],
                });
                app.pending_input = Some(remote_msg);
                app.auto_scroll = true;
            }
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

                // /goal — autonomous loop handler
                if cmd == "/goal" || cmd.starts_with("/goal ") {
                    let arg = cmd.strip_prefix("/goal").unwrap_or("").trim().to_string();
                    handle_goal_command(app, &arg);
                    terminal.draw(|frame| render::draw(frame, app))?;
                    continue;
                }

                // /init → open TUI wizard (no suspend)
                if cmd == "/init" {
                    let detected = crate::session::commands::detect_project_type().to_string();
                    let cursor = detected.chars().count();
                    app.init_wizard = Some(InitWizardState {
                        step: InitWizardStep::Language,
                        detected_language: detected.clone(),
                        language_input: detected,
                        language_cursor: cursor,
                        do_index: false,
                    });
                    continue;
                }

                // /diff → open TUI-native diff viewer (runs `git diff`)
                if cmd == "/diff" {
                    app.diff_viewer = crate::tui::render::open_diff_viewer();
                    if app.diff_viewer.is_none() {
                        app.messages.push(UiMessage {
                            role: MsgRole::Assistant,
                            blocks: vec![UiBlock::Text("No diff available or not in a git repository.".to_string())],
                        });
                        terminal.draw(|frame| render::draw(frame, app))?;
                    }
                    continue;
                }

                // 1. Try native inline handler (output rendered in a popup).
                if let Some(text) = commands::handle_inline(session, &input, config) {
                    if !text.is_empty() {
                        // Derive a title from the command.
                        let title = input.trim().split(' ').next().unwrap_or("/cmd").to_string();
                        app.command_popup = Some(app::CommandPopup {
                            title,
                            text,
                            scroll: 0,
                        });
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
                    // Sync model/branch in case /provider or /model changed them.
                    app.model = session.model.clone();
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
                app.files_changed_this_turn = 0;
                let mut cancelled = false;

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

                                // Pick up any incoming permission prompt request.
                                if let Some(req) = channel::take_perm_request() {
                                    app.permission_popup = Some(app::PermissionPopup {
                                        pending: req.pending,
                                        response_tx: Some(req.response_tx),
                                    });
                                }

                                // Pick up any incoming secret-scanner request.
                                if let Some(req) = channel::take_secret_request() {
                                    app.secret_popup = Some(app::SecretPopup {
                                        hits: req.hits,
                                        response_tx: Some(req.response_tx),
                                    });
                                }

                                // Always draw — popups render as TUI overlays.
                                terminal.draw(|frame| render::draw(frame, app))?;

                                // Drain all pending key events per tick so we never miss
                                // a Ctrl+C when other events precede it in the queue.
                                while crossterm::event::poll(Duration::ZERO)? {
                                    if let Ok(Event::Key(k)) = crossterm::event::read() {
                                        use crossterm::event::KeyEventKind;
                                        if k.kind == KeyEventKind::Release { continue; }

                                        // Ctrl+C always cancels — even when a popup is open.
                                        if k.code == KeyCode::Char('c')
                                            && k.modifiers.contains(KeyModifiers::CONTROL)
                                        {
                                            // Dismiss any blocking popup so its channel unblocks.
                                            if let Some(ref mut popup) = app.secret_popup {
                                                if let Some(tx) = popup.response_tx.take() {
                                                    let _ = tx.send(false);
                                                }
                                            }
                                            app.secret_popup = None;
                                            if let Some(ref mut popup) = app.permission_popup {
                                                if let Some(tx) = popup.response_tx.take() {
                                                    let _ = tx.send(channel::PermissionDecision::Deny);
                                                }
                                            }
                                            app.permission_popup = None;
                                            done = true;
                                            cancelled = true;
                                            app.goal_state = None;
                                            break; // stop draining — turn is cancelled
                                        }

                                        if app.secret_popup.is_some() {
                                            // Route Y/N/Esc to the secret scanner popup.
                                            match handle_key(app, k) {
                                                InputAction::SecretAllow => {
                                                    if let Some(ref mut popup) = app.secret_popup {
                                                        if let Some(tx) = popup.response_tx.take() {
                                                            let _ = tx.send(true);
                                                        }
                                                    }
                                                    app.secret_popup = None;
                                                }
                                                InputAction::SecretDeny => {
                                                    if let Some(ref mut popup) = app.secret_popup {
                                                        if let Some(tx) = popup.response_tx.take() {
                                                            let _ = tx.send(false);
                                                        }
                                                    }
                                                    app.secret_popup = None;
                                                }
                                                _ => {}
                                            }
                                        } else if app.permission_popup.is_some() {
                                            // Route Y/N/A to the permission popup.
                                            match handle_key(app, k) {
                                                InputAction::PermitAllow => {
                                                    if let Some(ref mut popup) = app.permission_popup {
                                                        if let Some(tx) = popup.response_tx.take() {
                                                            let _ = tx.send(channel::PermissionDecision::Allow);
                                                        }
                                                    }
                                                    app.permission_popup = None;
                                                }
                                                InputAction::PermitDeny => {
                                                    if let Some(ref mut popup) = app.permission_popup {
                                                        if let Some(tx) = popup.response_tx.take() {
                                                            let _ = tx.send(channel::PermissionDecision::Deny);
                                                        }
                                                    }
                                                    app.permission_popup = None;
                                                }
                                                InputAction::PermitAlways => {
                                                    if let Some(ref mut popup) = app.permission_popup {
                                                        if let Some(tx) = popup.response_tx.take() {
                                                            let _ = tx.send(channel::PermissionDecision::Always);
                                                        }
                                                    }
                                                    app.permission_popup = None;
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                } // turn_fut dropped here, cancelling the LLM request

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

                if cancelled {
                    app.messages.push(UiMessage {
                        role: MsgRole::Assistant,
                        blocks: vec![UiBlock::Text("  ⏹ Turn cancelled.".to_string())],
                    });
                    app.auto_scroll = true;
                }

                // Show a "files modified" hint if the agent wrote/edited files.
                if app.files_changed_this_turn > 0 {
                    let n = app.files_changed_this_turn;
                    app.files_changed_this_turn = 0;
                    let s = if n == 1 { "" } else { "s" };
                    // Pull +/- line counts from git diff HEAD --shortstat.
                    let stat_suffix = git_diff_shortstat();
                    app.messages.push(UiMessage {
                        role: MsgRole::Assistant,
                        blocks: vec![UiBlock::Text(format!(
                            "  ✎ {} file{} modified{} — Ctrl+G or /diff to view changes",
                            n, s, stat_suffix
                        ))],
                    });
                    app.auto_scroll = true;
                }
                // Clear active skill label at turn end.
                app.active_skill = None;
                // Update context % from session (safe now that turn_fut is dropped)
                app.context_pct = session.context_fill_pct();
                app.turn = session.turn_count;

                // Goal mode: check completion, auto-continue or declare done
                if app.goal_state.is_some() {
                    let done = goal_response_is_done(app);
                    let (condition, turns_done, max_turns) = {
                        let gs = app.goal_state.as_mut().unwrap();
                        gs.turns_done += 1;
                        (gs.condition.clone(), gs.turns_done, gs.max_turns)
                    };
                    if done || turns_done >= max_turns {
                        app.goal_state = None;
                        let msg = if done {
                            format!("✓ Goal complete in {} turn{}.", turns_done, if turns_done == 1 { "" } else { "s" })
                        } else {
                            format!("⏹ Goal stopped: {} turn limit reached.", max_turns)
                        };
                        app.messages.push(UiMessage {
                            role: MsgRole::Assistant,
                            blocks: vec![UiBlock::Text(msg)],
                        });
                        app.auto_scroll = true;
                    } else {
                        let next = format!(
                            "[Goal {}/{}] Continue toward: {}. When fully done, end your response with: ✓ DONE",
                            turns_done + 1, max_turns, condition
                        );
                        app.messages.push(UiMessage {
                            role: MsgRole::User,
                            blocks: vec![UiBlock::Text(next.clone())],
                        });
                        app.pending_input = Some(next);
                        app.auto_scroll = true;
                    }
                }
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
                            resume_tui(terminal)?;

                            if let Some(dir) = chosen {
                                match std::env::set_current_dir(&dir) {
                                    Ok(()) => {
                                        let new_cwd = std::env::current_dir()
                                            .map(|p| p.display().to_string())
                                            .unwrap_or_else(|_| dir.clone());
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
                        InputAction::SelectMode(is_task) => {
                            if is_task {
                                suspend_tui(terminal)?;
                                let task_intro = run_task_planning_tui(session).await;
                                resume_tui(terminal)?;
                                if let Some(intro) = task_intro {
                                    app.messages.push(UiMessage {
                                        role: MsgRole::User,
                                        blocks: vec![UiBlock::Text("Starting task session…".to_string())],
                                    });
                                    app.pending_input = Some(intro);
                                }
                            }
                            // Vibe: nothing extra needed, just proceed
                        }
                        InputAction::ToggleLastToolExpand => {
                            match next_tool_id_to_expand(app) {
                                Some(id) => { app.expanded_tools.insert(id); }
                                None     => { app.expanded_tools.clear(); }
                            }
                        }
                        InputAction::ConfirmDomainScope(names) => {
                            session.domain_scope = names.into_iter().collect();
                        }
                        InputAction::ConfirmInit { language, do_index, do_understand } => {
                            let languages: Vec<String> = language
                                .split([',', ' '])
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_lowercase)
                                .collect();
                            app.state = AppState::Thinking;
                            terminal.draw(|frame| render::draw(frame, app))?;
                            let (output, llm_prompt) = tokio::task::block_in_place(|| {
                                session.cmd_init_direct(languages, do_index, do_understand)
                            });
                            app.state = AppState::Idle;
                            app.messages.push(UiMessage {
                                role: MsgRole::Assistant,
                                blocks: vec![UiBlock::Text(output)],
                            });
                            app.auto_scroll = true;
                            if let Some(prompt) = llm_prompt {
                                app.pending_input = Some(prompt);
                            }
                        }
                        InputAction::CancelInit => {}
                        InputAction::PasteImage => {
                            let tmp = "/tmp/zap_clipboard_paste.png";
                            let ok = crate::session::commands::paste_clipboard_image(tmp);
                            if ok && std::path::Path::new(tmp).exists() {
                                // Stage image directly without calling cmd_attach (which println!s and corrupts TUI)
                                match std::fs::read(tmp) {
                                    Ok(bytes) => {
                                        use base64::Engine;
                                        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                                        let kb = bytes.len() / 1024;
                                        session.staged_images.push(("image/png".to_string(), data));
                                        app.messages.push(UiMessage {
                                            role: MsgRole::Assistant,
                                            blocks: vec![UiBlock::Text(format!(
                                                "✓ Image attached ({} KB) — it will be sent with your next message.", kb
                                            ))],
                                        });
                                        app.auto_scroll = true;
                                    }
                                    Err(e) => {
                                        app.messages.push(UiMessage {
                                            role: MsgRole::Assistant,
                                            blocks: vec![UiBlock::Text(format!("✗ Failed to read clipboard image: {}", e))],
                                        });
                                        app.auto_scroll = true;
                                    }
                                }
                            } else {
                                app.messages.push(UiMessage {
                                    role: MsgRole::Assistant,
                                    blocks: vec![UiBlock::Text(
                                        "✗ No image in clipboard. Copy a screenshot first, then press Ctrl+V again.".to_string(),
                                    )],
                                });
                                app.auto_scroll = true;
                            }
                        }
                        InputAction::OpenDiffViewer => {
                            app.diff_viewer = crate::tui::render::open_diff_viewer();
                            if app.diff_viewer.is_none() {
                                app.error = Some("No diff found (clean working tree and no previous commit)".to_string());
                            }
                        }
                        InputAction::CloseDiffViewer => {
                            app.diff_viewer = None;
                        }
                        InputAction::CloseCommandPopup => {
                            app.command_popup = None;
                        }
                        InputAction::SecretAllow => {
                            if let Some(ref mut popup) = app.secret_popup {
                                if let Some(tx) = popup.response_tx.take() {
                                    let _ = tx.send(true);
                                }
                            }
                            app.secret_popup = None;
                        }
                        InputAction::SecretDeny => {
                            if let Some(ref mut popup) = app.secret_popup {
                                if let Some(tx) = popup.response_tx.take() {
                                    let _ = tx.send(false);
                                }
                            }
                            app.secret_popup = None;
                        }
                        InputAction::PermitAllow => {
                            if let Some(ref mut popup) = app.permission_popup {
                                if let Some(tx) = popup.response_tx.take() {
                                    let _ = tx.send(channel::PermissionDecision::Allow);
                                }
                            }
                            app.permission_popup = None;
                        }
                        InputAction::PermitDeny => {
                            if let Some(ref mut popup) = app.permission_popup {
                                if let Some(tx) = popup.response_tx.take() {
                                    let _ = tx.send(channel::PermissionDecision::Deny);
                                }
                            }
                            app.permission_popup = None;
                        }
                        InputAction::PermitAlways => {
                            if let Some(ref mut popup) = app.permission_popup {
                                if let Some(tx) = popup.response_tx.take() {
                                    let _ = tx.send(channel::PermissionDecision::Always);
                                }
                            }
                            app.permission_popup = None;
                        }
                        InputAction::CommandPopupScrollUp(n) => {
                            if let Some(ref mut p) = app.command_popup {
                                p.scroll = p.scroll.saturating_sub(n);
                            }
                        }
                        InputAction::CommandPopupScrollDown(n) => {
                            if let Some(ref mut p) = app.command_popup {
                                p.scroll = p.scroll.saturating_add(n);
                            }
                        }
                        InputAction::DiffNavUp => {
                            if let Some(ref mut dv) = app.diff_viewer {
                                if dv.panel == crate::tui::app::DiffPanel::Files && !dv.files.is_empty() {
                                    dv.selected = dv.selected.saturating_sub(1);
                                }
                            }
                        }
                        InputAction::DiffNavDown => {
                            if let Some(ref mut dv) = app.diff_viewer {
                                if dv.panel == crate::tui::app::DiffPanel::Files {
                                    dv.selected = dv.selected.saturating_add(1).min(dv.files.len().saturating_sub(1));
                                }
                            }
                        }
                        InputAction::DiffSwitchPanel => {
                            if let Some(ref mut dv) = app.diff_viewer {
                                dv.panel = match dv.panel {
                                    crate::tui::app::DiffPanel::Files => crate::tui::app::DiffPanel::Diff,
                                    crate::tui::app::DiffPanel::Diff => crate::tui::app::DiffPanel::Files,
                                };
                            }
                        }
                        InputAction::DiffScrollUp(n) => {
                            if let Some(ref mut dv) = app.diff_viewer {
                                if dv.panel == crate::tui::app::DiffPanel::Diff {
                                    dv.diff_scroll = dv.diff_scroll.saturating_sub(n);
                                }
                            }
                        }
                        InputAction::DiffScrollDown(n) => {
                            if let Some(ref mut dv) = app.diff_viewer {
                                if dv.panel == crate::tui::app::DiffPanel::Diff {
                                    dv.diff_scroll = dv.diff_scroll.saturating_add(n);
                                }
                            }
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

/// Returns " (+N/-M)" from `git diff HEAD --shortstat`, or "" if no changes / git unavailable.
fn git_diff_shortstat() -> String {
    let out = std::process::Command::new("git")
        .args(["diff", "HEAD", "--shortstat"])
        .output()
        .ok();
    let Some(out) = out else { return String::new() };
    if !out.status.success() { return String::new(); }
    let text = String::from_utf8(out.stdout).unwrap_or_default();
    // text looks like " 3 files changed, 9 insertions(+), 6 deletions(-)\n"
    let mut added = 0usize;
    let mut removed = 0usize;
    for part in text.split(',') {
        let p = part.trim();
        if p.contains("insertion") {
            added = p.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
        } else if p.contains("deletion") {
            removed = p.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
        }
    }
    if added == 0 && removed == 0 {
        String::new()
    } else {
        format!(" (+{}/−{})", added, removed)
    }
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
            let path = path.trim().trim_end_matches('/');
            if !path.is_empty() && std::path::Path::new(path).exists() {
                return Some(path.to_string());
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

/// Run task planning in the normal terminal (TUI suspended) and return the
/// intro message to prime the first TUI turn, or None if aborted.
async fn run_task_planning_tui(session: &crate::session::Session) -> Option<String> {
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

/// Handle `/goal [condition | stop | status]` in the TUI.
fn handle_goal_command(app: &mut App, arg: &str) {
    let arg = arg.trim();
    if arg.is_empty() || arg == "status" {
        let text = if let Some(ref gs) = app.goal_state {
            format!(
                "**Goal active** — {}/{} turns  {}s elapsed\n\nCondition: {}\n\n`/goal stop` to cancel",
                gs.turns_done, gs.max_turns,
                gs.started_at.elapsed().as_secs(),
                gs.condition,
            )
        } else {
            "No active goal.\n\nUsage: `/goal <condition>` — zap keeps working turn-by-turn until the goal is met or `--max N` turns (default 20) are exhausted.\n\nExample: `/goal add unit tests for the auth module`".to_string()
        };
        app.messages.push(UiMessage { role: MsgRole::Assistant, blocks: vec![UiBlock::Text(text)] });
        app.auto_scroll = true;
        return;
    }
    if arg == "stop" || arg == "cancel" {
        app.goal_state = None;
        app.messages.push(UiMessage {
            role: MsgRole::Assistant,
            blocks: vec![UiBlock::Text("Goal stopped.".to_string())],
        });
        app.auto_scroll = true;
        return;
    }
    // Parse optional --max N
    let (condition, max_turns) = if let Some(idx) = arg.find("--max") {
        let cond = arg[..idx].trim().to_string();
        let rest = arg[idx + 5..].trim();
        let n: usize = rest.split_whitespace().next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);
        (cond, n)
    } else {
        (arg.to_string(), 20)
    };

    app.goal_state = Some(GoalState {
        condition: condition.clone(),
        max_turns,
        turns_done: 0,
        started_at: std::time::Instant::now(),
    });
    // Send the first goal turn — shown in chat and dispatched to the LLM
    let first = format!(
        "[Goal 1/{max}] {cond}\n\nWhen the goal is fully complete, end your response with exactly: ✓ DONE",
        max = max_turns, cond = condition
    );
    app.messages.push(UiMessage { role: MsgRole::User, blocks: vec![UiBlock::Text(first.clone())] });
    app.pending_input = Some(first);
    app.auto_scroll = true;
}

/// Check the last assistant message for the ✓ DONE completion marker.
fn goal_response_is_done(app: &App) -> bool {
    for msg in app.messages.iter().rev() {
        if matches!(msg.role, MsgRole::Assistant) {
            for block in &msg.blocks {
                if let UiBlock::Text(text) = block {
                    if text.contains("✓ DONE") || text.contains("✓DONE")
                        || text.to_lowercase().contains("✓ done")
                    {
                        return true;
                    }
                }
            }
            break; // only inspect the last assistant message
        }
    }
    false
}

/// Walk all completed messages (newest first) to find the next tool to expand.
/// Cycles: finds the most-recent tool that is NOT yet expanded.
/// If all tools are expanded, collapses all of them (full reset).
fn next_tool_id_to_expand(app: &App) -> Option<String> {
    // Collect all tool IDs with results, newest first.
    let mut all_ids: Vec<String> = Vec::new();
    for msg in app.messages.iter().rev() {
        for block in msg.blocks.iter().rev() {
            if let UiBlock::Tool(tc) = block {
                if tc.result.is_some() {
                    all_ids.push(tc.id.clone());
                }
            }
        }
    }
    // Also include completed tools from the current streaming turn.
    for sb in app.streaming_blocks.iter().rev() {
        if let StreamingBlock::Tool(tc) = sb {
            if tc.result.is_some() {
                all_ids.push(tc.id.clone());
            }
        }
    }

    if all_ids.is_empty() {
        return None;
    }

    // Find the first (most-recent) tool that isn't already expanded.
    if let Some(id) = all_ids.iter().find(|id| !app.expanded_tools.contains(*id)) {
        return Some(id.clone());
    }

    // All expanded — collapse everything (return None signals the caller to clear).
    None
}
