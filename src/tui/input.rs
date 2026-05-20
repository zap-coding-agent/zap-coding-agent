/// Keyboard input handling for the TUI.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, AppState};
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
    LoadSession(i64),
    CloseSessionPicker,
    /// Ctrl+O: toggle expansion of the last tool call with output.
    ToggleLastToolExpand,
    /// Domain picker confirmed — carries the selected skill names (may be empty = no restriction).
    ConfirmDomainScope(Vec<String>),
}

/// Returns true when the command picker is active (idle + input starts with '/').
fn picker_active(app: &App) -> bool {
    matches!(app.state, AppState::Idle) && app.input.starts_with('/')
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> InputAction {
    // Domain picker takes priority when open (shown at session start).
    if app.domain_picker.is_some() {
        return handle_domain_picker_key(app, key);
    }

    // Session picker takes priority when open.
    if app.session_picker.is_some() {
        return handle_session_picker_key(app, key);
    }

    // If file browser is open, handle its keys first
    if app.file_browser.is_some() {
        return handle_file_browser_key(app, key);
    }
    
    // Ctrl+C: cancel during a turn
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        match &app.state {
            AppState::Thinking | AppState::ToolRunning { .. } => return InputAction::Cancel,
            AppState::Idle => return InputAction::None,
        }
    }

    // Ctrl+D: quit when idle and input is empty
    if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(app.state, AppState::Idle) && app.input.is_empty() {
            return InputAction::Quit;
        }
        return InputAction::None;
    }

    // Ctrl+O: expand/collapse the last tool call output (idle only)
    if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(app.state, AppState::Idle) {
            return InputAction::ToggleLastToolExpand;
        }
        return InputAction::None;
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
                let id = entry.id;
                app.session_picker = None;
                InputAction::LoadSession(id)
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
