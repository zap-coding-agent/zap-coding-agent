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

mod git_info;
mod goal;
mod lifecycle;
mod startup;
mod turn_handler;

use std::io::Stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::Event;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc::UnboundedReceiver;

use app::{App, AppState, DiffPanel, MsgRole, UiBlock, UiMessage};
use channel::{PermissionDecision, TuiEvent};
use input::{handle_key, InputAction};

use crate::config::Config;
use crate::session::Session;

pub async fn run_tui(config: &Config) -> Result<()> {
    let mut tui_config = config.clone();
    tui_config.skip_domain_prompt = true;
    tui_config.tui_mode = true;
    let config = &tui_config;
    let mut session = Session::new(config).await?;
    session.hooks.fire_session_start();

    let branch = git_info::git_branch();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<TuiEvent>();
    channel::set_tui_sender(tx.clone());
    channel::init_perm_channel();
    channel::init_secret_channel();
    channel::init_btw_queue();

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::cursor::Hide,
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new(&session.model, &branch);
    app.skill_names = session.skills.iter().map(|s| s.name.clone()).collect();

    let is_new_project = crate::project::load_project_meta().is_none();
    if is_new_project {
        let detected = crate::session::commands::detect_project_type().to_string();
        let language = if detected.is_empty() { vec![] } else { vec![detected] };
        let name = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let meta = crate::project::ProjectMeta { name, language, indexed: false, indexed_at: None, initialized_at: None };
        let _ = crate::project::save_project_meta(&meta);
    }

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
                if ext_detected.contains(opt) { picker.checked[i] = true; }
            }
            app.domain_picker = Some(picker);
        }
    }

    let (dirty, ahead, behind) = git_info::git_status();
    app.git_dirty = dirty;
    app.git_ahead = ahead;
    app.git_behind = behind;

    startup::replay_last_session_into_app(&mut app, &session);
    startup::push_startup_messages(&mut app, &mut session);

    let result = tui_loop(&mut terminal, &mut app, &mut session, config, &mut rx).await;

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
        while let Ok(ev) = rx.try_recv() {
            app.apply_event(ev);
        }

        if app.pending_input.is_none() {
            if let Some(remote_msg) = crate::remote_channel::try_recv() {
                app.messages.push(UiMessage {
                    role:   MsgRole::User,
                    blocks: vec![UiBlock::Text(remote_msg.clone())],
                });
                app.pending_input = Some(remote_msg);
                app.auto_scroll = true;
            }
        }

        terminal.draw(|frame| render::draw(frame, app))?;

        if let Some(mut input) = app.pending_input.take() {
            let skill_names_snap: Vec<String> = if commands::could_be_skill_command(&input) {
                session.skills.iter().map(|s| s.name.clone()).collect()
            } else {
                Vec::new()
            };
            let one_shot_unpin: Option<String> =
                match commands::resolve_skill_command(&input, &skill_names_snap) {
                    Some(skill_name) => {
                        input = input[1..].to_string();
                        if session.pinned_skills.contains(&skill_name) {
                            None
                        } else {
                            session.pinned_skills.insert(skill_name.clone());
                            Some(skill_name)
                        }
                    }
                    None => None,
                };

            if input.starts_with('/') {
                let should_exit = turn_handler::handle_tui_slash(app, session, config, &input, terminal).await?;
                if should_exit { break; }
            } else {
                turn_handler::run_normal_turn(app, session, &input, terminal, rx).await?;
            }

            if let Some(ref name) = one_shot_unpin {
                session.pinned_skills.remove(name.as_str());
            }
            continue;
        }

        if crossterm::event::poll(Duration::from_millis(50))? {
            match crossterm::event::read()? {
                Event::Key(key)
                    if key.kind != crossterm::event::KeyEventKind::Release =>
                {
                    match handle_key(app, key) {
                        InputAction::Quit => break,
                        InputAction::Submit(text) => {
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
                        InputAction::Cancel => {}
                        InputAction::ScrollUp(n) => { app.scroll_up(n); }
                        InputAction::ScrollDown(n) => {
                            let sz = terminal.size()?;
                            let sidebar_w: u16 = if sz.width > render::SIDEBAR_W + 20 { render::SIDEBAR_W + 1 } else { 0 };
                            let chat_w = sz.width.saturating_sub(sidebar_w);
                            let viewport_h = sz.height as usize;
                            let total = app.total_lines(chat_w);
                            app.scroll_down(n, viewport_h.saturating_sub(3), total);
                        }
                        InputAction::OpenDirPicker => {
                            lifecycle::suspend_tui(terminal)?;
                            let chosen = lifecycle::open_dir_picker();
                            lifecycle::resume_tui(terminal)?;
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
                                        app.branch = git_info::git_branch();
                                        let (dirty, ahead, behind) = git_info::git_status();
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
                                let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                                match file_browser::FileBrowser::new(cwd) {
                                    Ok(browser) => {
                                        app.file_browser = Some(browser);
                                        if let Some(ref mut browser) = app.file_browser {
                                            let _ = browser.load_preview();
                                        }
                                    }
                                    Err(e) => { app.error = Some(format!("Failed to open file browser: {}", e)); }
                                }
                            }
                        }
                        InputAction::LoadSession { id: sid, goal } => {
                            startup::load_session_into_app(app, session, sid, goal);
                        }
                        InputAction::CloseSessionPicker => {}
                        InputAction::ClearInput => {}
                        InputAction::SelectMode(is_task) => {
                            if is_task {
                                lifecycle::suspend_tui(terminal)?;
                                let task_intro = lifecycle::run_task_planning_tui(session).await;
                                lifecycle::resume_tui(terminal)?;
                                if let Some(intro) = task_intro {
                                    app.messages.push(UiMessage {
                                        role: MsgRole::User,
                                        blocks: vec![UiBlock::Text("Starting task session…".to_string())],
                                    });
                                    app.pending_input = Some(intro);
                                }
                            }
                        }
                        InputAction::ToggleLastToolExpand => {
                            match goal::next_tool_id_to_expand(app) {
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
                                app.messages.push(UiMessage {
                                    role: MsgRole::Assistant,
                                    blocks: vec![UiBlock::Text(
                                        "Analysing codebase to fill ZAP.md — this may take a minute…".to_string()
                                    )],
                                });
                                app.pending_input = Some(prompt);
                            }
                        }
                        InputAction::CancelInit => {}
                        InputAction::PasteImage => {
                            let tmp = "/tmp/zap_clipboard_paste.png";
                            let ok = crate::session::commands::paste_clipboard_image(tmp);
                            if ok && std::path::Path::new(tmp).exists() {
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
                        InputAction::CloseDiffViewer => { app.diff_viewer = None; }
                        InputAction::CloseCommandPopup => { app.command_popup = None; }
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
                        InputAction::CommandPopupScrollUp(n) => {
                            if let Some(ref mut p) = app.command_popup { p.scroll = p.scroll.saturating_sub(n); }
                        }
                        InputAction::CommandPopupScrollDown(n) => {
                            if let Some(ref mut p) = app.command_popup { p.scroll = p.scroll.saturating_add(n); }
                        }
                        InputAction::DiffNavUp => {
                            if let Some(ref mut dv) = app.diff_viewer {
                                if dv.panel == DiffPanel::Files && !dv.files.is_empty() {
                                    dv.selected = dv.selected.saturating_sub(1);
                                }
                            }
                        }
                        InputAction::DiffNavDown => {
                            if let Some(ref mut dv) = app.diff_viewer {
                                if dv.panel == DiffPanel::Files {
                                    dv.selected = dv.selected.saturating_add(1).min(dv.files.len().saturating_sub(1));
                                }
                            }
                        }
                        InputAction::DiffSwitchPanel => {
                            if let Some(ref mut dv) = app.diff_viewer {
                                dv.panel = match dv.panel {
                                    DiffPanel::Files => DiffPanel::Diff,
                                    DiffPanel::Diff  => DiffPanel::Files,
                                };
                            }
                        }
                        InputAction::DiffScrollUp(n) => {
                            if let Some(ref mut dv) = app.diff_viewer {
                                if dv.panel == DiffPanel::Diff { dv.diff_scroll = dv.diff_scroll.saturating_sub(n); }
                            }
                        }
                        InputAction::DiffScrollDown(n) => {
                            if let Some(ref mut dv) = app.diff_viewer {
                                if dv.panel == DiffPanel::Diff { dv.diff_scroll = dv.diff_scroll.saturating_add(n); }
                            }
                        }
                        InputAction::BtwOpen | InputAction::BtwClose => {}
                        InputAction::BtwSubmit(_) => {}
                        InputAction::None => {}
                    }
                }
                Event::Resize(_, _) => { terminal.autoresize()?; }
                _ => {}
            }
        }
    }
    Ok(())
}
