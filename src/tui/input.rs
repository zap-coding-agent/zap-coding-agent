/// Keyboard input handling for the TUI.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, AppState, DiffPanel, InitWizardStep};
use super::commands::filter_commands;

pub enum InputAction {
    None,
    Submit(String),
    Slash(String),
    Quit,
    Cancel,
    ScrollUp(usize),
    ScrollDown(usize),
    ClearInput,
    OpenDirPicker,
    ToggleFileBrowser,
    LoadSession { id: i64, goal: String },
    CloseSessionPicker,
    /// Start a new session — clear messages and create a fresh session ID.
    StartNewSession,
    /// Ctrl+O: toggle expansion of the last tool call with output.
    ToggleLastToolExpand,
    /// Ctrl+V: paste image from clipboard.
    PasteImage,
    /// Vibe/Task mode selected: true = Task, false = Vibe.
    SelectMode(bool),
    /// Domain picker confirmed — carries the selected skill names (may be empty = no restriction).
    ConfirmDomainScope(Vec<String>),
    /// /init wizard confirmed with all collected choices.
    ConfirmInit { language: String, do_index: bool, do_understand: bool },
    /// /init wizard cancelled.
    CancelInit,
    /// Open diff viewer (triggered by /diff command).
    OpenDiffViewer,
    /// Close diff viewer.
    CloseDiffViewer,
    /// Command popup actions.
    CloseCommandPopup,
    CommandPopupScrollUp(usize),
    CommandPopupScrollDown(usize),
    /// Permission popup responses.
    PermitAllow,
    PermitDeny,
    PermitAlways,
    /// Secret scanner popup responses.
    SecretAllow,
    SecretDeny,
    /// Diff viewer navigation.
    DiffNavUp,
    DiffNavDown,
    DiffScrollUp(usize),
    DiffScrollDown(usize),
    DiffSwitchPanel,
    /// Mid-turn btw input box.
    BtwOpen,
    BtwSubmit(String),
    BtwClose,
}

/// Returns true when the command picker is active (idle + input starts with '/').
fn picker_active(app: &App) -> bool {
    matches!(app.state, AppState::Idle) && app.input.starts_with('/')
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> InputAction {
    // Mode picker is shown first, before everything else.
    if app.mode_picker.is_some() {
        return handle_mode_picker_key(app, key);
    }

    // Domain picker follows mode picker.
    if app.domain_picker.is_some() {
        return handle_domain_picker_key(app, key);
    }

    // Session picker takes priority when open.
    if app.session_picker.is_some() {
        return handle_session_picker_key(app, key);
    }

    // /init wizard takes priority when open.
    if app.init_wizard.is_some() {
        return handle_init_wizard_key(app, key);
    }

    // Diff viewer takes priority when open.
    if app.diff_viewer.is_some() {
        return handle_diff_viewer_key(app, key);
    }

    // Btw input box — active during a turn, Ctrl+B to open, Enter to submit, Esc to close.
    if app.btw_mode {
        match key.code {
            KeyCode::Enter => {
                let text = app.btw_draft.trim().to_string();
                app.btw_draft.clear();
                app.btw_cursor = 0;
                app.btw_mode = false;
                if !text.is_empty() {
                    return InputAction::BtwSubmit(text);
                }
                return InputAction::BtwClose;
            }
            KeyCode::Esc => {
                app.btw_draft.clear();
                app.btw_cursor = 0;
                app.btw_mode = false;
                return InputAction::BtwClose;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.btw_draft.insert(app.btw_cursor, c);
                app.btw_cursor += c.len_utf8();
                return InputAction::None;
            }
            KeyCode::Backspace => {
                if app.btw_cursor > 0 {
                    let prev = app.btw_draft[..app.btw_cursor]
                        .char_indices().next_back().map(|(i, _)| i).unwrap_or(0);
                    app.btw_draft.drain(prev..app.btw_cursor);
                    app.btw_cursor = prev;
                }
                return InputAction::None;
            }
            KeyCode::Left => {
                if app.btw_cursor > 0 {
                    app.btw_cursor = app.btw_draft[..app.btw_cursor]
                        .char_indices().next_back().map(|(i, _)| i).unwrap_or(0);
                }
                return InputAction::None;
            }
            KeyCode::Right => {
                if app.btw_cursor < app.btw_draft.len() {
                    let next = app.btw_draft[app.btw_cursor..]
                        .char_indices().nth(1).map(|(i, _)| app.btw_cursor + i)
                        .unwrap_or(app.btw_draft.len());
                    app.btw_cursor = next;
                }
                return InputAction::None;
            }
            _ => return InputAction::None,
        }
    }

    // Ctrl+B: open btw input during an active turn.
    if key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(app.state, AppState::Thinking | AppState::ToolRunning { .. }) {
            app.btw_mode = true;
            return InputAction::BtwOpen;
        }
        return InputAction::None;
    }

    // Secret scanner popup — Y/Enter to send anyway, everything else = deny.
    if app.secret_popup.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => return InputAction::SecretAllow,
            _ => return InputAction::SecretDeny,
        }
    }

    // Permission popup — highest priority. Y/Enter=allow, A=always, N/Esc=deny.
    if app.permission_popup.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => return InputAction::PermitAllow,
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => return InputAction::PermitDeny,
            KeyCode::Char('a') | KeyCode::Char('A') => return InputAction::PermitAlways,
            _ => return InputAction::None,
        }
    }

    // Command popup — Esc to dismiss, ↑↓/PgUp/PgDn to scroll.
    if app.command_popup.is_some() {
        match key.code {
            KeyCode::Esc     => return InputAction::CloseCommandPopup,
            KeyCode::Up      => return InputAction::CommandPopupScrollUp(1),
            KeyCode::Down    => return InputAction::CommandPopupScrollDown(1),
            KeyCode::PageUp  => return InputAction::CommandPopupScrollUp(10),
            KeyCode::PageDown => return InputAction::CommandPopupScrollDown(10),
            _ => return InputAction::None,
        }
    }

    // If file browser is open, handle its keys first
    if app.file_browser.is_some() {
        return handle_file_browser_key(app, key);
    }
    
    // Ctrl+C: cancel during a turn; quit-confirm when idle.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        match &app.state {
            AppState::Thinking | AppState::ToolRunning { .. } => return InputAction::Cancel,
            AppState::Idle => {
                if app.quit_confirm {
                    return InputAction::Quit;
                }
                app.quit_confirm = true;
                app.error = Some("Press Ctrl+C again to quit — any other key cancels".to_string());
                return InputAction::None;
            }
        }
    }

    // Ctrl+D: quit when idle and input is empty
    if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(app.state, AppState::Idle) && app.input.is_empty() {
            return InputAction::Quit;
        }
        return InputAction::None;
    }

    // Ctrl+O: cycle through tool output expansions (works in all states)
    if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return InputAction::ToggleLastToolExpand;
    }

    // Ctrl+P: open directory picker (idle only)
    if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(app.state, AppState::Idle) {
            return InputAction::OpenDirPicker;
        }
        return InputAction::None;
    }

    // Ctrl+F: toggle file browser (idle only)
    if key.code == KeyCode::Char('f') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(app.state, AppState::Idle) {
            return InputAction::ToggleFileBrowser;
        }
        return InputAction::None;
    }

    // Ctrl+G: open diff viewer (idle only)
    if key.code == KeyCode::Char('g') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(app.state, AppState::Idle) {
            return InputAction::OpenDiffViewer;
        }
        return InputAction::None;
    }

    // Ctrl+N: start a new session (idle only)
    if key.code == KeyCode::Char('n') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(app.state, AppState::Idle) {
            return InputAction::StartNewSession;
        }
        return InputAction::None;
    }

    // Ctrl+Q: quit with confirmation (idle only; two presses required)
    if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(app.state, AppState::Idle) {
            if app.quit_confirm {
                return InputAction::Quit;
            }
            app.quit_confirm = true;
            app.error = Some("Press Ctrl+Q again to quit, any other key to cancel".to_string());
        }
        return InputAction::None;
    }
    // Any other key resets quit confirmation.
    if app.quit_confirm {
        app.quit_confirm = false;
        app.error = None;
    }

    match key.code {
        KeyCode::Enter => {
            if matches!(app.state, AppState::Idle) {
                if picker_active(app) {
                    // Submit the currently highlighted picker item.
                    let items = filter_commands(&app.input, &app.skill_names);
                    let sel = app.picker_sel.min(items.len().saturating_sub(1));
                    if let Some((cmd, _)) = items.get(sel) {
                        let text = cmd.to_string();
                        app.input.clear();
                        app.cursor = 0;
                        app.picker_sel = 0;
                        return InputAction::Slash(text);
                    }
                }
                // No picker match: submit raw typed text.
                let text = app.input.trim().to_string();
                if text.is_empty() {
                    return InputAction::None;
                }
                app.input.clear();
                app.cursor = 0;
                app.picker_sel = 0;
                if text.starts_with('/') {
                    return InputAction::Slash(text);
                }
                return InputAction::Submit(text);
            }
            InputAction::None
        }

        KeyCode::Esc => {
            app.input.clear();
            app.cursor = 0;
            app.picker_sel = 0;
            InputAction::ClearInput
        }

        KeyCode::Up => {
            if picker_active(app) {
                app.picker_sel = app.picker_sel.saturating_sub(1);
                InputAction::None
            } else {
                InputAction::ScrollUp(3)
            }
        }

        KeyCode::Down => {
            if picker_active(app) {
                let count = filter_commands(&app.input, &app.skill_names).len();
                if count > 0 {
                    app.picker_sel = (app.picker_sel + 1).min(count - 1);
                }
                InputAction::None
            } else {
                InputAction::ScrollDown(3)
            }
        }

        KeyCode::Tab => {
            if picker_active(app) {
                let items = filter_commands(&app.input, &app.skill_names);
                let sel = app.picker_sel.min(items.len().saturating_sub(1));
                if let Some((cmd, _)) = items.get(sel) {
                    app.input = cmd.to_string();
                    app.cursor = app.input.chars().count();
                    app.picker_sel = 0;
                }
            }
            InputAction::None
        }

        KeyCode::Backspace => {
            if app.cursor > 0 {
                let byte_idx = char_to_byte_idx(&app.input, app.cursor - 1);
                let char_len = app.input[byte_idx..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                app.input.drain(byte_idx..byte_idx + char_len);
                app.cursor -= 1;
                app.picker_sel = 0;
            }
            InputAction::None
        }

        KeyCode::Delete => {
            if app.cursor < app.input.chars().count() {
                let byte_idx = char_to_byte_idx(&app.input, app.cursor);
                let char_len = app.input[byte_idx..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                app.input.drain(byte_idx..byte_idx + char_len);
                app.picker_sel = 0;
            }
            InputAction::None
        }

        KeyCode::Left => {
            if app.cursor > 0 {
                app.cursor -= 1;
            }
            InputAction::None
        }

        KeyCode::Right => {
            if app.cursor < app.input.chars().count() {
                app.cursor += 1;
            }
            InputAction::None
        }

        KeyCode::Home => {
            app.cursor = 0;
            InputAction::None
        }

        KeyCode::End => {
            app.cursor = app.input.chars().count();
            InputAction::None
        }

        // Ctrl+V: paste image from clipboard (idle only)
        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if matches!(app.state, AppState::Idle) {
                InputAction::PasteImage
            } else {
                InputAction::None
            }
        }

        KeyCode::PageUp => InputAction::ScrollUp(10),

        KeyCode::PageDown => InputAction::ScrollDown(10),

        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL)
                         && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            let byte_idx = char_to_byte_idx(&app.input, app.cursor);
            app.input.insert(byte_idx, c);
            app.cursor += 1;
            app.picker_sel = 0; // reset selection when typing
            InputAction::None
        }

        _ => InputAction::None,
    }
}

/// Convert a char-index into a byte index into `s`.
fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// Handle keys when the /init wizard overlay is open.
fn handle_init_wizard_key(app: &mut App, key: KeyEvent) -> InputAction {
    let wizard = app.init_wizard.as_mut().unwrap();
    match &wizard.step {
        InitWizardStep::Language => match key.code {
            KeyCode::Esc => {
                app.init_wizard = None;
                InputAction::CancelInit
            }
            KeyCode::Enter => {
                wizard.step = InitWizardStep::IndexConfirm;
                InputAction::None
            }
            KeyCode::Backspace => {
                let cursor = wizard.language_cursor;
                if cursor > 0 {
                    let byte = char_to_byte_idx(&wizard.language_input, cursor - 1);
                    let end  = char_to_byte_idx(&wizard.language_input, cursor);
                    wizard.language_input.drain(byte..end);
                    wizard.language_cursor -= 1;
                }
                InputAction::None
            }
            KeyCode::Left => {
                wizard.language_cursor = wizard.language_cursor.saturating_sub(1);
                InputAction::None
            }
            KeyCode::Right => {
                let max = wizard.language_input.chars().count();
                if wizard.language_cursor < max { wizard.language_cursor += 1; }
                InputAction::None
            }
            KeyCode::Home => { wizard.language_cursor = 0; InputAction::None }
            KeyCode::End => {
                wizard.language_cursor = wizard.language_input.chars().count();
                InputAction::None
            }
            KeyCode::Char(c) => {
                let byte = char_to_byte_idx(&wizard.language_input, wizard.language_cursor);
                wizard.language_input.insert(byte, c);
                wizard.language_cursor += 1;
                InputAction::None
            }
            _ => InputAction::None,
        },
        InitWizardStep::IndexConfirm => match key.code {
            KeyCode::Esc => {
                wizard.step = InitWizardStep::Language;
                InputAction::None
            }
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                wizard.do_index = true;
                wizard.step = InitWizardStep::UnderstandConfirm;
                InputAction::None
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                wizard.do_index = false;
                wizard.step = InitWizardStep::UnderstandConfirm;
                InputAction::None
            }
            _ => InputAction::None,
        },
        InitWizardStep::UnderstandConfirm => match key.code {
            KeyCode::Esc => {
                wizard.step = InitWizardStep::IndexConfirm;
                InputAction::None
            }
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                let lang    = app.init_wizard.as_ref().unwrap().language_input.clone();
                let do_idx  = app.init_wizard.as_ref().unwrap().do_index;
                app.init_wizard = None;
                InputAction::ConfirmInit { language: lang, do_index: do_idx, do_understand: true }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                let lang    = app.init_wizard.as_ref().unwrap().language_input.clone();
                let do_idx  = app.init_wizard.as_ref().unwrap().do_index;
                app.init_wizard = None;
                InputAction::ConfirmInit { language: lang, do_index: do_idx, do_understand: false }
            }
            _ => InputAction::None,
        },
    }
}

/// Handle keys when the Vibe/Task mode picker overlay is open.
fn handle_mode_picker_key(app: &mut App, key: KeyEvent) -> InputAction {
    let picker = app.mode_picker.as_mut().unwrap();
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            picker.cursor = picker.cursor.saturating_sub(1);
            InputAction::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            picker.cursor = picker.cursor.saturating_add(1).min(1);
            InputAction::None
        }
        KeyCode::Tab => {
            picker.cursor = 1 - picker.cursor;
            InputAction::None
        }
        KeyCode::Enter => {
            let is_task = picker.cursor == 1;
            app.mode_picker = None;
            InputAction::SelectMode(is_task)
        }
        KeyCode::Esc => {
            // Esc defaults to Vibe
            app.mode_picker = None;
            InputAction::SelectMode(false)
        }
        _ => InputAction::None,
    }
}

/// Handle keys when the domain/language scope picker is open.
fn handle_domain_picker_key(app: &mut App, key: KeyEvent) -> InputAction {
    let picker = app.domain_picker.as_mut().unwrap();
    match key.code {
        KeyCode::Esc => {
            // Esc = no restriction (all domains active).
            app.domain_picker = None;
            InputAction::ConfirmDomainScope(vec![])
        }
        KeyCode::Up | KeyCode::Char('k') => {
            picker.cursor = picker.cursor.saturating_sub(1);
            InputAction::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let max = picker.options.len().saturating_sub(1);
            picker.cursor = (picker.cursor + 1).min(max);
            InputAction::None
        }
        KeyCode::Char(' ') => {
            let i = picker.cursor;
            if i < picker.checked.len() {
                picker.checked[i] = !picker.checked[i];
            }
            InputAction::None
        }
        KeyCode::Enter => {
            let selected = picker.selected();
            app.domain_picker = None;
            InputAction::ConfirmDomainScope(selected)
        }
        _ => InputAction::None,
    }
}

/// Handle keys when the session picker overlay is open.
fn handle_session_picker_key(app: &mut App, key: KeyEvent) -> InputAction {
    let picker = app.session_picker.as_mut().unwrap();
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.session_picker = None;
            InputAction::CloseSessionPicker
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            app.session_picker = None;
            InputAction::StartNewSession
        }
        KeyCode::Up | KeyCode::Char('k') => {
            picker.selected = picker.selected.saturating_sub(1);
            InputAction::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let max = picker.entries.len().saturating_sub(1);
            picker.selected = (picker.selected + 1).min(max);
            InputAction::None
        }
        KeyCode::Enter => {
            if let Some(entry) = picker.entries.get(picker.selected) {
                if entry.id == 0 {
                    // Synthetic "New session" entry.
                    app.session_picker = None;
                    InputAction::StartNewSession
                } else {
                    let id = entry.id;
                    let goal = entry.goal.clone();
                    app.session_picker = None;
                    InputAction::LoadSession { id, goal }
                }
            } else {
                app.session_picker = None;
                InputAction::CloseSessionPicker
            }
        }
        _ => InputAction::None,
    }
}

/// Handle keys when file browser is open.
fn handle_file_browser_key(app: &mut App, key: KeyEvent) -> InputAction {
    let browser = app.file_browser.as_mut().unwrap();
    
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.file_browser = None;
            InputAction::None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            browser.move_up();
            let _ = browser.load_preview();
            InputAction::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            browser.move_down();
            let _ = browser.load_preview();
            InputAction::None
        }
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
            // Toggle expand for directories, or insert file path for files
            if let Some(entry) = browser.entries.get(browser.selected) {
                if entry.is_dir {
                    let _ = browser.toggle_expand();
                } else {
                    // Insert file path into input
                    let path = entry.path.display().to_string();
                    app.input = format!("show me {}", path);
                    app.cursor = app.input.chars().count();
                    app.file_browser = None;
                }
            }
            InputAction::None
        }
        KeyCode::Left | KeyCode::Char('h') => {
            // Collapse directory
            if let Some(entry) = browser.entries.get(browser.selected) {
                if entry.is_dir && entry.is_expanded {
                    let _ = browser.toggle_expand();
                }
            }
            InputAction::None
        }
        KeyCode::Char('/') => {
            // Start search mode (for now, just a placeholder)
            InputAction::None
        }
        _ => InputAction::None,
    }
}

// ── Diff viewer key handler ───────────────────────────────────────────────────

fn handle_diff_viewer_key(app: &mut App, key: KeyEvent) -> InputAction {
    let dv = match app.diff_viewer.as_mut() {
        Some(d) => d,
        None    => return InputAction::None,
    };

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => InputAction::CloseDiffViewer,

        KeyCode::Tab | KeyCode::Left | KeyCode::Right => InputAction::DiffSwitchPanel,

        KeyCode::Up | KeyCode::Char('k') => {
            if dv.panel == DiffPanel::Files {
                InputAction::DiffNavUp
            } else {
                InputAction::DiffScrollUp(1)
            }
        }

        KeyCode::Down | KeyCode::Char('j') => {
            if dv.panel == DiffPanel::Files {
                InputAction::DiffNavDown
            } else {
                InputAction::DiffScrollDown(1)
            }
        }

        KeyCode::PageUp => {
            if dv.panel == DiffPanel::Files {
                InputAction::DiffNavUp
            } else {
                InputAction::DiffScrollUp(10)
            }
        }

        KeyCode::PageDown => {
            if dv.panel == DiffPanel::Files {
                InputAction::DiffNavDown
            } else {
                InputAction::DiffScrollDown(10)
            }
        }

        KeyCode::Enter => {
            // Enter on file list: switch focus to diff panel
            dv.panel = DiffPanel::Diff;
            dv.diff_scroll = 0;
            InputAction::None
        }

        _ => InputAction::None,
    }
}
