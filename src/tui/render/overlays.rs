use ratatui::{prelude::*, widgets::*};
use ratatui::style::Modifier;
use super::super::app::App;
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

pub(super) fn draw_gemini_auth_prompt(frame: &mut Frame, app: &App, area: Rect) {
    if !app.gemini_auth_prompt { return; }

    let w = 60u16.min(area.width);
    let h = 11u16.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let border_c = Color::Rgb(100, 210, 255);
    let accent   = Color::Rgb(100, 210, 255);
    let muted    = Color::Rgb(140, 135, 165);
    let dim      = Color::Rgb(80, 75, 100);

    let title = if app.gemini_reauth {
        " Google Gemini — re-authenticate "
    } else {
        " Google Gemini — sign in "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_c))
        .title(Span::styled(
            title,
            Style::default().fg(border_c).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    let hint_area    = Rect { y: inner.y + inner.height.saturating_sub(1), height: 1, ..inner };
    let content_area = Rect { height: inner.height.saturating_sub(1), ..inner };

    frame.render_widget(
        Paragraph::new(Span::styled(
            "  Enter/G = open browser   K = API key instructions   Esc = cancel",
            Style::default().fg(dim),
        )),
        hint_area,
    );

    let action_label = if app.gemini_reauth {
        "Re-authenticate via gcloud"
    } else {
        "Keyless sign-in via gcloud"
    };
    let action_hint = if app.gemini_reauth {
        "Refreshes scopes — fixes 401 errors."
    } else {
        "Opens browser — sign in, then return here."
    };

    let rows = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  ◎ ", Style::default().fg(accent)),
            Span::styled(action_label, Style::default().fg(Color::White).bold()),
            Span::styled("  (no API key needed)", Style::default().fg(muted)),
        ]),
        Line::from(vec![
            Span::styled("    ", Style::default()),
            Span::styled(action_hint, Style::default().fg(muted)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Press ", Style::default().fg(muted)),
            Span::styled("Enter", Style::default().fg(accent).bold()),
            Span::styled(" or ", Style::default().fg(muted)),
            Span::styled("G", Style::default().fg(accent).bold()),
            Span::styled(" to start the sign-in flow.", Style::default().fg(muted)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Press ", Style::default().fg(muted)),
            Span::styled("K", Style::default().fg(accent).bold()),
            Span::styled(" instead to use an API key.", Style::default().fg(muted)),
        ]),
    ];

    frame.render_widget(Paragraph::new(rows), content_area);
}

pub(super) fn draw_api_key_input(frame: &mut Frame, app: &App, area: Rect) {
    let pending = match app.api_key_input.as_ref() {
        Some(p) => p,
        None    => return,
    };

    if pending.picking_model {
        draw_model_picker(frame, pending, area);
    } else {
        draw_key_entry(frame, pending, area);
    }
}

fn draw_key_entry(frame: &mut Frame, pending: &crate::tui::app::PendingProviderSwitch, area: Rect) {
    let w = 64u16.min(area.width);
    let h = 10u16.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let border_c = Color::Rgb(255, 200, 80);
    let accent   = Color::Rgb(255, 200, 80);
    let muted    = Color::Rgb(140, 135, 165);
    let dim      = Color::Rgb(80, 75, 100);

    let title = format!(" {} — enter API key ", pending.name);
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
        Paragraph::new(Span::styled(
            "  Enter = confirm   Backspace = delete   Esc = cancel",
            Style::default().fg(dim),
        )),
        hint_area,
    );

    let masked: String = "*".repeat(pending.input.len());
    let field_text = if pending.input.is_empty() { "_".to_string() } else { masked };

    let mut rows = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  API key: ", Style::default().fg(muted)),
            Span::styled(field_text, Style::default().fg(Color::White).bold()),
            Span::styled("▎", Style::default().fg(accent)),
        ]),
        Line::from(""),
    ];

    if pending.has_existing_key {
        rows.push(Line::from(vec![
            Span::styled("  ✓ Key already saved — ", Style::default().fg(muted)),
            Span::styled("press Enter to keep", Style::default().fg(accent)),
            Span::styled(", or type a new one.", Style::default().fg(muted)),
        ]));
    } else if pending.slug == "gemini" {
        rows.push(Line::from(vec![
            Span::styled("  Don't have one? Get it at ", Style::default().fg(muted)),
            Span::styled("aistudio.google.com/apikey", Style::default().fg(accent)),
            Span::styled(" — includes credits.", Style::default().fg(muted)),
        ]));
    } else {
        rows.push(Line::from(vec![
            Span::styled("  Type your key, then press ", Style::default().fg(muted)),
            Span::styled("Enter", Style::default().fg(accent)),
            Span::styled(".", Style::default().fg(muted)),
        ]));
    }

    frame.render_widget(Paragraph::new(rows), content_area);
}

fn draw_model_picker(frame: &mut Frame, pending: &crate::tui::app::PendingProviderSwitch, area: Rect) {
    let n = pending.models.len().min(8);
    let h = (n as u16 + 5).min(area.height);
    let w = 52u16.min(area.width);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let border_c = Color::Rgb(255, 200, 80);
    let accent   = Color::Rgb(255, 200, 80);
    let muted    = Color::Rgb(140, 135, 165);
    let dim      = Color::Rgb(80, 75, 100);

    let title = format!(" {} — pick model ", pending.name);
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
        Paragraph::new(Span::styled(
            "  ↑↓/jk navigate   Enter = select   Esc = cancel",
            Style::default().fg(dim),
        )),
        hint_area,
    );

    let rows: Vec<Line> = pending.models.iter().enumerate().map(|(i, m)| {
        if i == pending.model_sel {
            Line::from(vec![
                Span::styled("  ▸ ", Style::default().fg(accent).bold()),
                Span::styled(m.as_str(), Style::default().fg(Color::White).bold()),
            ])
        } else {
            Line::from(vec![
                Span::styled("    ", Style::default()),
                Span::styled(m.as_str(), Style::default().fg(muted)),
            ])
        }
    }).collect();

    frame.render_widget(Paragraph::new(rows), content_area);
}

