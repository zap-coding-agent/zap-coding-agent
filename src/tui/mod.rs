/// Ratatui TUI for zap — full-screen interactive mode.
///
/// Entry point: `run_tui(config)`.
/// Channel module provides global TUI event sender for session/stream_highlighter.
pub mod app;
pub mod channel;
pub mod context_viewer;
pub mod commands;
pub mod input;
pub mod render;
pub mod syntax;
pub mod file_browser;

mod actions;
mod git_info;
mod goal;
mod lifecycle;
mod startup;
mod text_parse;
mod turn_handler;

use std::io::Stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, EventStream, MouseEventKind};
use futures_util::StreamExt as _;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc::UnboundedReceiver;

use app::{App, MsgRole, UiBlock, UiMessage};
use channel::TuiEvent;
use input::handle_key;

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
    channel::init_btw_queue();

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::cursor::Hide,
        crossterm::event::EnableMouseCapture,
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

    let (dirty, ahead, behind) = git_info::git_status();
    app.git_dirty = dirty;
    app.git_ahead = ahead;
    app.git_behind = behind;

    startup::replay_last_session_into_app(&mut app, &session);
    startup::push_startup_messages(&mut app, &mut session);
    startup::maybe_open_onboarding_picker(&mut app, config);

    let _result = tui_loop(&mut terminal, &mut app, &mut session, config, &mut rx).await;

    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        terminal.backend_mut(),
        crossterm::event::DisableMouseCapture,
        crossterm::terminal::LeaveAlternateScreen
    );
    let _ = terminal.show_cursor();

    session.save_context_with_summary().await;
    session.hooks.fire_session_end();
    // Force-exit: bypass tokio runtime shutdown, which blocks waiting for
    // background tasks (indexer, remote server) to finish their current
    // synchronous work. Context is already saved; the OS cleans up the rest.
    std::process::exit(0);
}

async fn tui_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    session: &mut Session,
    config: &Config,
    rx: &mut UnboundedReceiver<TuiEvent>,
) -> Result<()> {
    // EventStream is async on all platforms; on Windows it runs the blocking
    // ReadConsoleInput call on a background OS thread and signals the async
    // executor when events are ready.  The old poll()+read() pattern could
    // hang indefinitely on Windows when only MENU_EVENT/FOCUS_EVENT records
    // were in the console buffer (crossterm drains them silently then blocks).
    let mut event_stream = EventStream::new();
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
                turn_handler::run_normal_turn(app, session, &input, terminal, rx, &mut event_stream).await?;
            }

            if let Some(ref name) = one_shot_unpin {
                session.pinned_skills.remove(name.as_str());
            }

            // Auto-fire any prompt queued while the turn was in progress.
            if app.pending_input.is_none() {
                if let Some(queued) = app.queued_input.take() {
                    app.pending_input = Some(queued);
                }
            }
            continue;
        }

        let maybe_event = match tokio::time::timeout(
            Duration::from_millis(50),
            event_stream.next(),
        ).await {
            Ok(Some(Ok(ev))) => Some(ev),
            Ok(Some(Err(_))) | Ok(None) => None,
            Err(_) => None,
        };
        if let Some(event) = maybe_event {
            match event {
                Event::Key(key)
                    if key.kind != crossterm::event::KeyEventKind::Release =>
                {
                    let action = handle_key(app, key);
                    if actions::handle_action(action, app, session, terminal, config).await? {
                        break;
                    }
                }
                Event::Mouse(mouse) => {
                    let action = match mouse.kind {
                        MouseEventKind::ScrollUp   => input::InputAction::ScrollUp(3),
                        MouseEventKind::ScrollDown => input::InputAction::ScrollDown(3),
                        _ => input::InputAction::None,
                    };
                    if actions::handle_action(action, app, session, terminal, config).await? {
                        break;
                    }
                }
                Event::Resize(_, _) => { terminal.autoresize()?; }
                _ => {}
            }
        }
    }
    Ok(())
}
