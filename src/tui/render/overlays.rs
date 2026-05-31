use ratatui::{prelude::*, widgets::*};
use ratatui::style::Modifier;
use super::super::app::{App, InitWizardStep};
use super::super::file_browser::{FileBrowser, GitStatus};
use super::super::syntax;

pub(super) fn draw_file_browser(frame: &mut Frame, app: &App, area: Rect) {
    let browser = match &app.file_browser {
        Some(b) => b,
        None => return,
    };

    let overlay_w = (area.width as f32 * 0.8) as u16;
    let overlay_h = (area.height as f32 * 0.8) as u16;
    let overlay_x = (area.width - overlay_w) / 2;
    let overlay_y = (area.height - overlay_h) / 2;

    let overlay_area = Rect {
        x: overlay_x,
        y: overlay_y,
        width: overlay_w,
        height: overlay_h,
    };

    frame.render_widget(Clear, overlay_area);

    let chunks = Layout::horizontal([
        Constraint::Percentage(40),
        Constraint::Percentage(60),
    ])
    .split(overlay_area);

    draw_file_list(frame, browser, chunks[0]);
    draw_file_preview(frame, browser, chunks[1]);
}

fn draw_file_list(frame: &mut Frame, browser: &FileBrowser, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(" Files (Ctrl+F to close) ", Style::default().fg(Color::Cyan).bold()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    let filtered = browser.filtered_entries();

    for (idx, entry) in filtered.iter() {
        let is_selected = *idx == browser.selected;

        let indent = "  ".repeat(entry.depth);

        let icon = if entry.is_dir {
            if entry.is_expanded { "▼ " } else { "▶ " }
        } else {
            "  "
        };

        let git_icon = match entry.git_status {
            GitStatus::Modified  => "M",
            GitStatus::Untracked => "?",
            GitStatus::Staged    => "A",
            GitStatus::Ignored   => "!",
            GitStatus::Clean     => " ",
        };

        let git_color = match entry.git_status {
            GitStatus::Modified  => Color::Yellow,
            GitStatus::Untracked => Color::Red,
            GitStatus::Staged    => Color::Green,
            GitStatus::Ignored   => Color::DarkGray,
            GitStatus::Clean     => Color::Gray,
        };

        let name_color = if entry.is_dir { Color::Cyan } else { Color::White };

        let line = if is_selected {
            Line::from(vec![
                Span::styled(format!("{}{}{} ", indent, icon, git_icon), Style::default().fg(git_color).bg(Color::DarkGray)),
                Span::styled(entry.name.clone(), Style::default().fg(name_color).bg(Color::DarkGray).add_modifier(Modifier::BOLD)),
            ])
        } else {
            Line::from(vec![
                Span::styled(format!("{}{}{} ", indent, icon, git_icon), Style::default().fg(git_color)),
                Span::styled(entry.name.clone(), Style::default().fg(name_color)),
            ])
        };

        lines.push(line);
    }

    if lines.len() < inner.height as usize - 2 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("↑↓ navigate  Enter open  Esc close", Style::default().fg(Color::DarkGray))));
    }

    let para = Paragraph::new(lines).scroll((browser.scroll as u16, 0));
    frame.render_widget(para, inner);
}

fn draw_file_preview(frame: &mut Frame, browser: &FileBrowser, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(" Preview ", Style::default().fg(Color::Cyan).bold()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(ref content) = browser.preview_content {
        let lines = if let Some(ref lang) = browser.preview_lang {
            syntax::highlight_code(lang, content)
        } else {
            content.lines()
                .map(|line| Line::from(Span::styled(line.to_string(), Style::default().fg(Color::White))))
                .collect()
        };

        let para = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(para, inner);
    } else {
        let text = if let Some(entry) = browser.entries.get(browser.selected) {
            if entry.is_dir { "[Directory]" } else { "[No preview available]" }
        } else {
            "[No file selected]"
        };

        let para = Paragraph::new(text).style(Style::default().fg(Color::DarkGray));
        frame.render_widget(para, inner);
    }
}

pub(super) fn draw_session_picker(frame: &mut Frame, app: &App, area: Rect) {
    let picker = match app.session_picker.as_ref() {
        Some(p) => p,
        None    => return,
    };

    let w       = (area.width * 82 / 100).max(40).min(area.width);
    let visible = picker.entries.len().min(18);
    let h       = (visible as u16 + 4).min(area.height);
    let x       = area.x + (area.width.saturating_sub(w)) / 2;
    let y       = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " sessions  ↑↓ navigate   Enter load   N new   Esc cancel ",
            Style::default().fg(Color::Yellow).bold(),
        ));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    if picker.entries.is_empty() {
        frame.render_widget(
            Paragraph::new("No sessions found.").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    let sel   = picker.selected.min(picker.entries.len().saturating_sub(1));
    let start = if sel >= visible { sel - visible + 1 } else { 0 };
    let end   = (start + visible).min(picker.entries.len());

    let col_id_w    = 5usize;
    let col_date_w  = 10usize;
    let col_model_w = 18usize;
    let sep_w       = 3usize;
    let goal_w      = (inner.width as usize)
        .saturating_sub(col_id_w + sep_w + col_date_w + sep_w + col_model_w + sep_w + 1);

    let rows: Vec<Line<'static>> = picker.entries[start..end]
        .iter()
        .enumerate()
        .flat_map(|(i, entry)| {
            let is_sel = (start + i) == sel;

            if entry.id == 0 {
                // Synthetic "New session" entry — render as a highlighted row.
                let label_text = if inner.width >= 40 {
                    "  ✚  Start a fresh session"
                } else {
                    " ✚ New session"
                };
                if is_sel {
                    return vec![Line::from(Span::styled(
                        label_text.to_string(),
                        Style::default().fg(Color::Black).bg(Color::Cyan).bold(),
                    ))];
                } else {
                    return vec![Line::from(Span::styled(
                        label_text.to_string(),
                        Style::default().fg(Color::Rgb(100, 200, 255)).bold(),
                    ))];
                }
            }

            let goal_disp: String = if entry.goal.chars().count() > goal_w {
                format!("{}…", entry.goal.chars().take(goal_w.saturating_sub(1)).collect::<String>())
            } else {
                format!("{:<width$}", entry.goal, width = goal_w)
            };
            let model_disp: String = if entry.model.chars().count() > col_model_w {
                format!("{}…", entry.model.chars().take(col_model_w - 1).collect::<String>())
            } else {
                format!("{:<width$}", entry.model, width = col_model_w)
            };

            let text = format!(
                " #{:<id$} │ {:<date$} │ {} │ {}",
                entry.id,
                entry.date,
                goal_disp,
                model_disp,
                id   = col_id_w - 1,
                date = col_date_w,
            );

            if is_sel {
                vec![Line::from(Span::styled(text, Style::default().fg(Color::Black).bg(Color::Cyan)))]
            } else {
                vec![Line::from(Span::styled(text, Style::default().fg(Color::White)))]
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(rows), inner);
}

pub(super) fn draw_domain_picker(frame: &mut Frame, app: &App, area: Rect) {
    let picker = match app.domain_picker.as_ref() {
        Some(p) => p,
        None    => return,
    };

    let visible = picker.options.len().min(16);
    let w = (area.width * 60 / 100).max(44).min(area.width);
    let h = (visible as u16 + 6).min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let proj = &picker.project_name;
    let title = format!(" {} — existing project: scope to languages / frameworks? ", proj);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(255, 200, 50)))
        .title(Span::styled(
            title,
            Style::default().fg(Color::Rgb(255, 200, 50)).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    let hint_area    = Rect { y: inner.y + inner.height.saturating_sub(1), height: 1, ..inner };
    let content_area = Rect { height: inner.height.saturating_sub(1), ..inner };

    frame.render_widget(
        Paragraph::new(Span::styled(
            "  Space toggle   Enter confirm   Esc skip (no restriction)",
            Style::default().fg(Color::Rgb(80, 75, 100)),
        )),
        hint_area,
    );

    if picker.options.is_empty() { return; }

    let sel   = picker.cursor.min(picker.options.len().saturating_sub(1));
    let start = if sel >= visible { sel - visible + 1 } else { 0 };
    let end   = (start + visible).min(picker.options.len());

    let rows: Vec<Line<'static>> = picker.options[start..end]
        .iter()
        .zip(&picker.checked[start..end])
        .enumerate()
        .map(|(i, (name, &checked))| {
            let idx = start + i;
            let is_sel = idx == sel;
            let checkbox = if checked { "[x] " } else { "[ ] " };
            let text = format!("  {}{}", checkbox, name);
            if is_sel {
                Line::from(Span::styled(text, Style::default().fg(Color::Black).bg(Color::Yellow)))
            } else if checked {
                Line::from(Span::styled(text, Style::default().fg(Color::Cyan)))
            } else {
                Line::from(Span::styled(text, Style::default().fg(Color::White)))
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(rows), content_area);
}

pub(super) fn draw_provider_picker(frame: &mut Frame, app: &App, area: Rect) {
    let picker = match app.provider_picker.as_ref() {
        Some(p) => p,
        None    => return,
    };

    let w = (area.width * 82 / 100).max(50).min(area.width);
    let visible = picker.entries.len().min(18);
    let h = (visible as u16 + 4).min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let border_c = Color::Rgb(100, 210, 255);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_c))
        .title(Span::styled(
            " switch provider   ↑↓ navigate   Enter select   Esc cancel ",
            Style::default().fg(Color::Yellow).bold(),
        ));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    let sel   = picker.selected.min(picker.entries.len().saturating_sub(1));
    let start = if sel >= visible { sel - visible + 1 } else { 0 };
    let end   = (start + visible).min(picker.entries.len());

    let sel_bg = Color::Rgb(60, 55, 80);
    let rows: Vec<Line<'static>> = picker.entries[start..end]
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let is_sel = (start + i) == sel;
            let marker = if is_sel { " ❯ " } else { "   " };
            let hint_icon = if entry.coming_soon { " ◷ " } else { "   " };

            let hint_line = format!("{}{}  {}", hint_icon, entry.name, entry.hint);
            let padding = w.saturating_sub(hint_line.chars().count() as u16 + 4) as usize;
            if is_sel {
                Line::from(vec![
                    Span::styled(marker.to_string(), Style::default().fg(border_c).bg(sel_bg).bold()),
                    Span::styled(hint_line, Style::default().fg(Color::White).bg(sel_bg).bold()),
                    Span::styled(
                        " ".repeat(padding),
                        Style::default().bg(sel_bg),
                    ),
                ])
            } else if entry.coming_soon {
                Line::from(vec![
                    Span::styled(marker.to_string(), Style::default().fg(Color::Rgb(80, 75, 100))),
                    Span::styled(hint_line, Style::default().fg(Color::Rgb(80, 75, 100))),
                ])
            } else {
                Line::from(vec![
                    Span::styled(marker.to_string(), Style::default().fg(Color::Rgb(80, 75, 100))),
                    Span::styled(hint_icon.to_string(), Style::default().fg(Color::Rgb(80, 75, 100))),
                    Span::styled(entry.name.clone(), Style::default().fg(Color::Rgb(100, 210, 255)).bold()),
                    Span::styled(format!("  {}", entry.hint), Style::default().fg(Color::Rgb(120, 115, 150))),
                ])
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(rows), inner);
}

pub(super) fn draw_mode_picker(frame: &mut Frame, app: &App, area: Rect) {
    let picker = match app.mode_picker.as_ref() {
        Some(p) => p,
        None    => return,
    };

    let w = 52u16.min(area.width);
    let h = 10u16.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let border_c = Color::Rgb(255, 200, 50);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_c))
        .title(Span::styled(
            " ⚡ How do you want to work? ",
            Style::default().fg(border_c).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    let opts: &[(&str, &str)] = &[
        ("Vibe", "start talking — no structure"),
        ("Task", "plan first, then execute"),
    ];

    let sel_bg = Color::Rgb(60, 55, 80);
    let mut rows: Vec<Line<'static>> = vec![Line::from("")];
    for (i, (name, desc)) in opts.iter().enumerate() {
        let is_sel = i == picker.cursor;
        let marker = if is_sel { " ❯ " } else { "   " };
        let line = if is_sel {
            Line::from(vec![
                Span::styled(marker.to_string(), Style::default().fg(border_c).bg(sel_bg).bold()),
                Span::styled(format!("{:<6}", name), Style::default().fg(border_c).bg(sel_bg).bold()),
                Span::styled(format!("  {}", desc), Style::default().fg(Color::Rgb(170, 165, 195)).bg(sel_bg)),
                Span::styled(" ".repeat(w as usize), Style::default().bg(sel_bg)),
            ])
        } else {
            Line::from(vec![
                Span::styled(marker.to_string(), Style::default().fg(Color::Rgb(80, 75, 100))),
                Span::styled(format!("{:<6}", name), Style::default().fg(Color::Rgb(140, 135, 165))),
                Span::styled(format!("  {}", desc), Style::default().fg(Color::Rgb(80, 75, 100))),
            ])
        };
        rows.push(line);
    }
    rows.push(Line::from(""));
    rows.push(Line::from(Span::styled(
        "  ↑↓ navigate   Enter confirm   Esc = Vibe",
        Style::default().fg(Color::Rgb(60, 55, 80)),
    )));

    frame.render_widget(Paragraph::new(rows), inner);
}

pub(super) fn draw_init_wizard(frame: &mut Frame, app: &App, area: Rect) {
    let wizard = match app.init_wizard.as_ref() {
        Some(w) => w,
        None    => return,
    };

    let (title, hint, content_h) = match &wizard.step {
        InitWizardStep::Language          => (" ⚡ Set up this project ", "  Edit   Enter next   Esc skip", 14u16),
        InitWizardStep::IndexConfirm      => (" ⚡ Set up this project ", "  y/Enter = yes   n = no   Esc back", 10u16),
        InitWizardStep::UnderstandConfirm => (" ⚡ Set up this project ", "  y/Enter = yes   n = no   Esc back", 10u16),
    };

    let w = 62u16.min(area.width);
    let h = (content_h + 2).min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let border_c = Color::Rgb(255, 200, 50);
    let accent   = Color::Rgb(100, 210, 255);
    let muted    = Color::Rgb(100, 95, 125);
    let dim      = Color::Rgb(70, 65, 90);
    let body_c   = Color::Rgb(210, 205, 235);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_c))
        .title(Span::styled(title, Style::default().fg(border_c).add_modifier(Modifier::BOLD)));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    let hint_area    = Rect { y: inner.y + inner.height.saturating_sub(1), height: 1, ..inner };
    let content_area = Rect { height: inner.height.saturating_sub(1), ..inner };

    frame.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(dim))),
        hint_area,
    );

    let mut rows: Vec<Line<'static>> = Vec::new();

    match &wizard.step {
        InitWizardStep::Language => {
            rows.push(Line::from(vec![
                Span::styled("  Step 1 of 3  ", Style::default().fg(dim)),
                Span::styled("language", Style::default().fg(border_c).bold()),
                Span::styled("  ·  index  ·  understand", Style::default().fg(dim)),
            ]));
            rows.push(Line::from(""));
            rows.push(Line::from(Span::styled(
                "  Init prepares zap to work well in this project:",
                Style::default().fg(body_c),
            )));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("Index code symbols", Style::default().fg(body_c).bold()),
                Span::styled(" — jump to any function/type instantly", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("Write ZAP.md", Style::default().fg(body_c).bold()),
                Span::styled(" — your project's instructions, loaded every session", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("Build understanding", Style::default().fg(body_c).bold()),
                Span::styled(" — zap reads your code and learns its structure", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(""));

            let label  = Span::styled("  Language(s): ", Style::default().fg(Color::Rgb(140, 135, 165)));
            let text   = wizard.language_input.clone();
            let cursor = Span::styled("▋", Style::default().fg(border_c));
            rows.push(Line::from(vec![
                label,
                Span::styled(text, Style::default().fg(Color::White).bold()),
                cursor,
            ]));
            rows.push(Line::from(""));
            let note = if wizard.detected_language.is_empty() {
                "  no build file detected — type language (e.g. rust, python, typescript)".to_string()
            } else {
                format!("  auto-detected from project files: {}", wizard.detected_language)
            };
            rows.push(Line::from(Span::styled(note, Style::default().fg(dim))));
        }

        InitWizardStep::IndexConfirm => {
            rows.push(Line::from(vec![
                Span::styled("  Step 2 of 3  ", Style::default().fg(dim)),
                Span::styled("language  ·  ", Style::default().fg(dim)),
                Span::styled("index", Style::default().fg(border_c).bold()),
                Span::styled("  ·  understand", Style::default().fg(dim)),
            ]));
            rows.push(Line::from(""));
            rows.push(Line::from(Span::styled(
                "  Index code symbols now?  (recommended, ~10–30s)",
                Style::default().fg(body_c).bold(),
            )));
            rows.push(Line::from(""));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("zap parses every file with tree-sitter", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("extracts functions, types, structs, imports", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("stays current: auto-refreshes every 2 min + at session end", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("run /index any time to force a refresh", Style::default().fg(muted)),
            ]));
        }

        InitWizardStep::UnderstandConfirm => {
            rows.push(Line::from(vec![
                Span::styled("  Step 3 of 3  ", Style::default().fg(dim)),
                Span::styled("language  ·  index  ·  ", Style::default().fg(dim)),
                Span::styled("understand", Style::default().fg(border_c).bold()),
            ]));
            rows.push(Line::from(""));
            rows.push(Line::from(Span::styled(
                "  Let zap read your codebase and write ZAP.md?  (~30–60s)",
                Style::default().fg(body_c).bold(),
            )));
            rows.push(Line::from(""));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("zap reads your source files and fills ZAP.md", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("records architecture, build commands, conventions", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("ZAP.md is loaded into every future session automatically", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("you can edit ZAP.md any time to update zap's context", Style::default().fg(muted)),
            ]));
        }
    }

    frame.render_widget(Paragraph::new(rows), content_area);
}
