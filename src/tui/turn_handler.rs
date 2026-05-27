use std::io::Stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc::UnboundedReceiver;

use super::app::{App, AppState, MsgRole, UiBlock, UiMessage};
use super::channel::{self, TuiEvent, PermissionDecision};
use super::input::{handle_key, InputAction};
use super::render;
use crate::config::Config;
use crate::session::Session;

/// Handle a slash command in TUI mode. Returns `true` if the session should exit.
pub(super) async fn handle_tui_slash(
    app: &mut App,
    session: &mut Session,
    config: &Config,
    input: &str,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> Result<bool> {
    let cmd = input.trim();

    if cmd == "/sessions" || cmd.starts_with("/sessions ") {
        match session.store.recent_sessions(30) {
            Ok(rows) => {
                app.session_picker = Some(super::app::SessionPickerState {
                    entries: rows.iter().map(|(id, goal, model, ts)| super::app::SessionEntry {
                        id:    *id,
                        goal:  goal.clone(),
                        model: model.clone(),
                        date:  ts.get(..10).unwrap_or(ts).to_string(),
                    }).collect(),
                    selected: 0,
                });
            }
            Err(e) => { app.error = Some(format!("sessions: {e}")); }
        }
        return Ok(false);
    }

    if cmd == "/goal" || cmd.starts_with("/goal ") {
        let arg = cmd.strip_prefix("/goal").unwrap_or("").trim().to_string();
        super::goal::handle_goal_command(app, &arg);
        terminal.draw(|frame| render::draw(frame, app))?;
        return Ok(false);
    }

    if cmd == "/init" {
        let detected = crate::session::commands::detect_project_type().to_string();
        let cursor = detected.chars().count();
        app.init_wizard = Some(super::app::InitWizardState {
            step: super::app::InitWizardStep::Language,
            detected_language: detected.clone(),
            language_input: detected,
            language_cursor: cursor,
            do_index: false,
        });
        return Ok(false);
    }

    if cmd == "/diff" {
        app.diff_viewer = crate::tui::render::open_diff_viewer();
        if app.diff_viewer.is_none() {
            app.messages.push(UiMessage {
                role: MsgRole::Assistant,
                blocks: vec![UiBlock::Text("No diff available or not in a git repository.".to_string())],
            });
            terminal.draw(|frame| render::draw(frame, app))?;
        }
        return Ok(false);
    }

    // 1. Try native inline handler (output rendered in a popup).
    if let Some(text) = super::commands::handle_inline(session, input, config) {
        if !text.is_empty() {
            let title = input.trim().split(' ').next().unwrap_or("/cmd").to_string();
            app.command_popup = Some(super::app::CommandPopup { title, text, scroll: 0 });
            terminal.draw(|frame| render::draw(frame, app))?;
        }
        app.branch = super::git_info::git_branch();
        let (dirty, ahead, behind) = super::git_info::git_status();
        app.git_dirty = dirty;
        app.git_ahead = ahead;
        app.git_behind = behind;
        if input.trim_start().starts_with("/skill") {
            app.skill_names = session.skills.iter().map(|s| s.name.clone()).collect();
        }
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
        return Ok(false);
    }

    if input.trim() == "/exit" {
        return Ok(true);
    }

    // 2. Complex command — suspend TUI, run in normal terminal, wait for Enter.
    super::lifecycle::suspend_tui(terminal)?;
    let should_exit = session.handle_slash(input, config).await;
    if !should_exit {
        use std::io::Write;
        println!();
        print!("  \x1b[2m── Press any key to return to zap ──\x1b[0m ");
        std::io::stdout().flush().ok();
        crossterm::terminal::enable_raw_mode().ok();
        loop {
            match crossterm::event::read() {
                Ok(crossterm::event::Event::Key(_)) => break,
                _ => continue,
            }
        }
        crossterm::terminal::disable_raw_mode().ok();
    }
    super::lifecycle::resume_tui(terminal)?;
    app.model = session.model.clone();
    app.branch = super::git_info::git_branch();
    let (dirty, ahead, behind) = super::git_info::git_status();
    app.git_dirty = dirty;
    app.git_ahead = ahead;
    app.git_behind = behind;
    Ok(should_exit)
}

/// Execute a normal (non-slash) user turn, animating the TUI during the LLM call.
pub(super) async fn run_normal_turn(
    app: &mut App,
    session: &mut Session,
    input: &str,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    rx: &mut UnboundedReceiver<TuiEvent>,
) -> Result<()> {
    app.state = AppState::Thinking;
    app.auto_scroll = true;
    app.files_changed_this_turn = 0;
    let mut cancelled = false;

    {
        let turn_fut = session.handle_user_turn(input);
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
                    // Cap at 64 events per tick so a warning flood (e.g. index errors)
                    // cannot starve the spinner or freeze the UI.
                    for _ in 0..64 {
                        match rx.try_recv() {
                            Ok(ev) => app.apply_event(ev),
                            Err(_) => break,
                        }
                    }
                    app.tick_spinner();

                    if let Some(req) = channel::take_perm_request() {
                        app.permission_popup = Some(super::app::PermissionPopup {
                            pending: req.pending,
                            response_tx: Some(req.response_tx),
                        });
                    }
                    if let Some(req) = channel::take_secret_request() {
                        app.secret_popup = Some(super::app::SecretPopup {
                            hits: req.hits,
                            response_tx: Some(req.response_tx),
                        });
                    }

                    terminal.draw(|frame| render::draw(frame, app))?;

                    while crossterm::event::poll(Duration::ZERO)? {
                        if let Ok(Event::Key(k)) = crossterm::event::read() {
                            use crossterm::event::KeyEventKind;
                            if k.kind == KeyEventKind::Release { continue; }

                            if k.code == KeyCode::Char('c')
                                && k.modifiers.contains(KeyModifiers::CONTROL)
                            {
                                if let Some(ref mut popup) = app.secret_popup {
                                    if let Some(tx) = popup.response_tx.take() { let _ = tx.send(false); }
                                }
                                app.secret_popup = None;
                                if let Some(ref mut popup) = app.permission_popup {
                                    if let Some(tx) = popup.response_tx.take() { let _ = tx.send(PermissionDecision::Deny); }
                                }
                                app.permission_popup = None;
                                done = true;
                                cancelled = true;
                                app.goal_state = None;
                                break;
                            }

                            if app.permission_popup.is_some() {
                                match handle_key(app, k) {
                                    InputAction::PermitAllow => {
                                        if let Some(ref mut popup) = app.permission_popup {
                                            if let Some(tx) = popup.response_tx.take() { let _ = tx.send(PermissionDecision::Allow); }
                                        }
                                        app.permission_popup = None;
                                    }
                                    InputAction::PermitDeny => {
                                        if let Some(ref mut popup) = app.permission_popup {
                                            if let Some(tx) = popup.response_tx.take() { let _ = tx.send(PermissionDecision::Deny); }
                                        }
                                        app.permission_popup = None;
                                    }
                                    InputAction::PermitAlways => {
                                        if let Some(ref mut popup) = app.permission_popup {
                                            if let Some(tx) = popup.response_tx.take() { let _ = tx.send(PermissionDecision::Always); }
                                        }
                                        app.permission_popup = None;
                                    }
                                    _ => {}
                                }
                            } else if app.secret_popup.is_some() {
                                match handle_key(app, k) {
                                    InputAction::SecretAllow => {
                                        if let Some(ref mut popup) = app.secret_popup {
                                            if let Some(tx) = popup.response_tx.take() { let _ = tx.send(true); }
                                        }
                                        app.secret_popup = None;
                                    }
                                    InputAction::SecretDeny => {
                                        if let Some(ref mut popup) = app.secret_popup {
                                            if let Some(tx) = popup.response_tx.take() { let _ = tx.send(false); }
                                        }
                                        app.secret_popup = None;
                                    }
                                    _ => {}
                                }
                            } else if app.btw_mode || (k.code == KeyCode::Char('b') && k.modifiers.contains(KeyModifiers::CONTROL)) {
                                if let InputAction::BtwSubmit(text) = handle_key(app, k) {
                                    app.messages.push(UiMessage {
                                        role: MsgRole::User,
                                        blocks: vec![UiBlock::Text(format!("↳ btw: {text}"))],
                                    });
                                    app.auto_scroll = true;
                                    channel::push_btw(text);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    while let Ok(ev) = rx.try_recv() { app.apply_event(ev); }
    app.finalize_turn();
    app.state = AppState::Idle;
    while rx.try_recv().is_ok() {}

    if cancelled {
        app.messages.push(UiMessage {
            role: MsgRole::Assistant,
            blocks: vec![UiBlock::Text("  ⏹ Turn cancelled.".to_string())],
        });
        app.auto_scroll = true;
    }

    if app.files_changed_this_turn > 0 {
        let n = app.files_changed_this_turn;
        app.files_changed_this_turn = 0;
        let s = if n == 1 { "" } else { "s" };
        let stat_suffix = super::git_info::git_diff_shortstat();
        app.messages.push(UiMessage {
            role: MsgRole::Assistant,
            blocks: vec![UiBlock::Text(format!(
                "  ✎ {} file{} modified{} — Ctrl+G or /diff to view changes",
                n, s, stat_suffix
            ))],
        });
        app.auto_scroll = true;
    }
    app.active_skill = None;
    app.context_pct = session.context_fill_pct();
    app.turn = session.turn_count;

    // Goal mode: check completion, auto-continue or declare done.
    if app.goal_state.is_some() {
        let done = super::goal::goal_response_is_done(app);
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

    Ok(())
}
