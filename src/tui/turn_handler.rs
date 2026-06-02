use std::io::Stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc::UnboundedReceiver;

use super::app::{App, AppState, ContextTurnEntry, ContextViewerState, DetailBlock, MsgRole, TurnDetail, UiBlock, UiMessage};
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
                let mut entries: Vec<super::app::SessionEntry> = rows.iter().map(|(id, goal, model, ts)| super::app::SessionEntry {
                    id:    *id,
                    goal:  goal.clone(),
                    model: model.clone(),
                    date:  ts.get(..10).unwrap_or(ts).to_string(),
                }).collect();
                // Prepend a synthetic "New session" entry at the top.
                entries.insert(0, super::app::SessionEntry {
                    id:    0,
                    goal:  "New session (start fresh)".to_string(),
                    model: String::new(),
                    date:  String::new(),
                });
                app.session_picker = Some(super::app::SessionPickerState {
                    entries,
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

    if cmd == "/provider" {
        use super::app::{ProviderEntry, ProviderKind, ProviderPickerState};

        let gemini_ready = crate::llm_client::auth::check_gcloud_adc().is_some()
            || crate::llm_client::auth::check_google_api_key_env().is_some();
        let claude_code_ready = crate::llm_client::auth::check_claude_code().is_some();

        let entries: Vec<ProviderEntry> = vec![
            ProviderEntry { slug: "lm_studio".into(),  name: "LM Studio".into(),                  hint: "local · OpenAI-compatible".into(),             kind: ProviderKind::OpenAi,    models: vec!["gemma-4-e4b-it".into(), "qwen2.5-coder-7b-instruct".into(), "mistral-7b-instruct".into(), "Other…".into()],    base_url: Some("http://localhost:1234/v1/chat/completions".into()),                     needs_key: false, coming_soon: false, auth_header: None,       ready: true },
            ProviderEntry { slug: "ollama".into(),     name: "Ollama".into(),                     hint: "local · OpenAI-compatible".into(),             kind: ProviderKind::OpenAi,    models: vec!["llama3.2".into(), "llama3.1:70b".into(), "codellama".into(), "qwen2.5-coder".into(), "Other…".into()],   base_url: Some("http://localhost:11434/v1/chat/completions".into()),                      needs_key: false, coming_soon: false, auth_header: None,       ready: true },
            ProviderEntry { slug: "anthropic".into(),  name: "Anthropic".into(),                  hint: "claude-sonnet-4-6 / claude-opus-4-7".into(),   kind: ProviderKind::Anthropic, models: vec!["claude-sonnet-4-6".into(), "claude-opus-4-7".into(), "claude-haiku-4-5".into(), "Other…".into()],    base_url: None,                                                                                 needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderEntry { slug: "claude_code".into(),name: "Claude Code (Pro/Max API)".into(),  hint: if claude_code_ready { "claude-sonnet-4-6 / claude-opus-4-7 · via claude CLI".into() } else { "requires claude CLI · Pro/Max plan".into() }, kind: ProviderKind::Anthropic, models: vec!["claude-sonnet-4-6".into(), "claude-opus-4-7".into()],                                            base_url: None,                                                                                 needs_key: false, coming_soon: !claude_code_ready, auth_header: None, ready: claude_code_ready },
            ProviderEntry { slug: "openai".into(),     name: "OpenAI".into(),                     hint: "gpt-4o / gpt-4o-mini / o3".into(),             kind: ProviderKind::OpenAi,    models: vec!["gpt-4o".into(), "gpt-4o-mini".into(), "o3".into(), "o4-mini".into(), "Other…".into()],    base_url: None,                                                                                 needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderEntry { slug: "gemini".into(),     name: "Google Gemini".into(),              hint: "gemini-2.5-pro / gemini-2.0-flash".into(),     kind: ProviderKind::OpenAi,    models: vec!["gemini-2.0-flash".into(), "gemini-2.5-pro".into(), "gemini-2.5-flash".into(), "Other…".into()],     base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".into()), needs_key: true, coming_soon: false, auth_header: Some("x-goog-api-key"), ready: gemini_ready },
            ProviderEntry { slug: "deepseek".into(),   name: "DeepSeek".into(),                   hint: "deepseek-v4-pro / deepseek-v4-flash".into(),   kind: ProviderKind::OpenAi,    models: vec!["deepseek-v4-pro".into(), "deepseek-v4-flash".into(), "deepseek-chat".into(), "deepseek-reasoner".into(), "Other…".into()], base_url: Some("https://api.deepseek.com/v1/chat/completions".into()),                    needs_key: true, coming_soon: false, auth_header: None,       ready: false },
            ProviderEntry { slug: "groq".into(),       name: "Groq".into(),                       hint: "llama-3.3-70b · fastest inference".into(),     kind: ProviderKind::OpenAi,    models: vec!["llama-3.3-70b-versatile".into(), "llama-3.1-8b-instant".into(), "mixtral-8x7b-32768".into(), "Other…".into()], base_url: Some("https://api.groq.com/openai/v1/chat/completions".into()),                   needs_key: true, coming_soon: false, auth_header: None,       ready: false },
            ProviderEntry { slug: "mistral".into(),    name: "Mistral".into(),                    hint: "mistral-large / codestral".into(),             kind: ProviderKind::OpenAi,    models: vec!["mistral-large-latest".into(), "codestral-latest".into(), "mistral-small-latest".into(), "Other…".into()],    base_url: Some("https://api.mistral.ai/v1/chat/completions".into()),                       needs_key: true, coming_soon: false, auth_header: None,       ready: false },
            ProviderEntry { slug: "xai".into(),        name: "xAI (Grok)".into(),                 hint: "grok-3 / grok-3-mini".into(),                  kind: ProviderKind::OpenAi,    models: vec!["grok-3".into(), "grok-3-mini".into(), "grok-2".into(), "Other…".into()],    base_url: Some("https://api.x.ai/v1/chat/completions".into()),                                needs_key: true, coming_soon: false, auth_header: None,       ready: false },
            ProviderEntry { slug: "together".into(),   name: "Together AI".into(),                hint: "Llama / Qwen / Mistral open models".into(),    kind: ProviderKind::OpenAi,    models: vec!["meta-llama/Llama-3-70b-chat-hf".into(), "Qwen/Qwen2.5-72B-Instruct-Turbo".into(), "Other…".into()], base_url: Some("https://api.together.xyz/v1/chat/completions".into()),                      needs_key: true, coming_soon: false, auth_header: None,       ready: false },
            ProviderEntry { slug: "perplexity".into(), name: "Perplexity".into(),                 hint: "sonar-pro · web-grounded answers".into(),      kind: ProviderKind::OpenAi,    models: vec!["sonar-pro".into(), "sonar".into(), "sonar-reasoning".into(), "Other…".into()],    base_url: Some("https://api.perplexity.ai/chat/completions".into()),                         needs_key: true, coming_soon: false, auth_header: None,       ready: false },
            ProviderEntry { slug: "cohere".into(),     name: "Cohere".into(),                     hint: "command-r-plus".into(),                        kind: ProviderKind::OpenAi,    models: vec!["command-r-plus".into(), "command-r".into(), "Other…".into()],                  base_url: Some("https://api.cohere.ai/compatibility/v1/chat/completions".into()),            needs_key: true, coming_soon: false, auth_header: None,       ready: false },
            ProviderEntry { slug: "custom".into(),     name: "Custom (OpenAI-compatible)".into(), hint: "any OpenAI-compatible endpoint".into(),         kind: ProviderKind::OpenAi,    models: vec!["Other…".into()],                                                                 base_url: None,                                                                                 needs_key: false, coming_soon: false, auth_header: None,       ready: false },
        ];

        app.provider_picker = Some(ProviderPickerState { entries, selected: 0, is_onboarding: false });
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

    if cmd == "/context" {
        app.context_viewer = Some(build_context_viewer(session));
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

                    terminal.draw(|frame| render::draw(frame, app))?;

                    while crossterm::event::poll(Duration::ZERO)? {
                        if let Ok(Event::Key(k)) = crossterm::event::read() {
                            use crossterm::event::KeyEventKind;
                            if k.kind == KeyEventKind::Release { continue; }

                            if k.code == KeyCode::Char('c')
                                && k.modifiers.contains(KeyModifiers::CONTROL)
                            {
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

/// Snapshot session.messages into a ContextViewerState for the /context overlay.
pub(super) fn build_context_viewer(session: &Session) -> ContextViewerState {
    use crate::llm_client::ContentBlock;
    use crate::session::model_context_limit;

    let window: usize = std::env::var("ZAP_HISTORY_WINDOW")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);

    let msgs = &session.messages;

    // Locate every "real user turn" — user message whose first block is Text.
    let turn_indices: Vec<usize> = msgs
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.role == "user"
                && m.content
                    .first()
                    .is_some_and(|b| matches!(b, ContentBlock::Text { .. }))
        })
        .map(|(i, _)| i)
        .collect();

    let total_turns = turn_indices.len();
    let window_start = total_turns.saturating_sub(window);

    fn msg_chars(b: &ContentBlock) -> usize {
        match b {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::ToolUse { input, .. } => input.to_string().len(),
            ContentBlock::ToolResult { content, .. } => content.len(),
            _ => 0,
        }
    }

    // First pass: compute raw char counts per turn and the grand total across
    // all messages so we can scale each turn proportionally.
    let total_chars: usize = msgs
        .iter()
        .flat_map(|m| m.content.iter())
        .map(msg_chars)
        .sum();

    let total_tokens = session.estimated_context_tokens();

    let mut per_turn: Vec<(usize, usize, String, usize, bool)> = Vec::new(); // (msg_idx, msg_count, preview, chars, in_window)
    for (turn_idx, &msg_idx) in turn_indices.iter().enumerate() {
        let next_msg = turn_indices
            .get(turn_idx + 1)
            .copied()
            .unwrap_or(msgs.len());
        let msg_count = next_msg - msg_idx;

        let preview = msgs[msg_idx]
            .content
            .iter()
            .find_map(|b| {
                if let ContentBlock::Text { text } = b {
                    Some(text.chars().take(60).collect::<String>())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let chars: usize = msgs[msg_idx..next_msg]
            .iter()
            .flat_map(|m| m.content.iter())
            .map(msg_chars)
            .sum();

        per_turn.push((msg_idx, msg_count, preview, chars, turn_idx >= window_start));
    }

    // Second pass: scale each turn's char share against the known-correct total.
    let mut turns = Vec::new();
    for (msg_idx, msg_count, preview, chars, in_window) in per_turn {
        let tokens_est = if total_chars > 0 {
            (total_tokens as f64 * (chars as f64 / total_chars as f64)) as usize
        } else {
            chars / 4
        };
        let next_msg = msg_idx + msg_count;
        let detail = build_turn_detail(&session.messages, msg_idx, next_msg, total_tokens, total_chars);
        turns.push(ContextTurnEntry {
            msg_index: msg_idx,
            msg_count,
            preview,
            tokens_est,
            in_window,
            detail,
        });
    }

    ContextViewerState {
        turns,
        selected: 0,
        total_tokens,
        limit_tokens: model_context_limit(&session.model),
        context_pct: session.context_fill_pct(),
        confirm_clear: false,
        confirm_drop: false,
        detail_focus: false,
        detail_scroll: 0,
    }
}

fn build_turn_detail(
    msgs: &[crate::llm_client::Message],
    msg_idx: usize,
    next_msg: usize,
    total_tokens: usize,
    total_chars: usize,
) -> TurnDetail {
    use crate::llm_client::ContentBlock;

    fn block_chars(b: &ContentBlock) -> usize {
        match b {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::ToolUse { input, .. } => input.to_string().len(),
            ContentBlock::ToolResult { content, .. } => content.len(),
            _ => 0,
        }
    }

    fn chars_to_tokens(chars: usize, total_tokens: usize, total_chars: usize) -> usize {
        if total_chars > 0 {
            (total_tokens as f64 * (chars as f64 / total_chars as f64)) as usize
        } else {
            chars / 4
        }
    }

    // Build a name map from tool_use_id → tool name so ToolResult can show the name.
    let mut tool_name_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for msg in &msgs[msg_idx..next_msg] {
        for block in &msg.content {
            if let ContentBlock::ToolUse { id, name, .. } = block {
                tool_name_map.insert(id.clone(), name.clone());
            }
        }
    }

    let mut blocks: Vec<DetailBlock> = Vec::new();

    for msg in &msgs[msg_idx..next_msg] {
        match msg.role.as_str() {
            "user" => {
                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } => {
                            let t = chars_to_tokens(text.len(), total_tokens, total_chars);
                            blocks.push(DetailBlock::UserText { text: text.clone(), tokens: t });
                        }
                        ContentBlock::ToolResult { tool_use_id, content } => {
                            let tool_name = tool_name_map.get(tool_use_id).cloned().unwrap_or_default();
                            let t = chars_to_tokens(content.len(), total_tokens, total_chars);
                            blocks.push(DetailBlock::ToolResult { tool_name, content: content.clone(), tokens: t });
                        }
                        _ => {}
                    }
                }
            }
            "assistant" => {
                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } if !text.trim().is_empty() => {
                            let t = chars_to_tokens(text.len(), total_tokens, total_chars);
                            blocks.push(DetailBlock::AssistantText { text: text.clone(), tokens: t });
                        }
                        ContentBlock::ToolUse { name, input, .. } => {
                            let json = serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string());
                            let t = chars_to_tokens(block_chars(block), total_tokens, total_chars);
                            blocks.push(DetailBlock::ToolCall { name: name.clone(), input_json: json, tokens: t });
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    TurnDetail { blocks }
}
