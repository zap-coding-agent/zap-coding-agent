use ratatui::{prelude::*, widgets::*};
use super::super::app::{App, AppState};
use super::super::commands::filter_commands;
use super::messages::render_all_lines;

pub(super) fn draw_messages(frame: &mut Frame, app: &App, area: Rect) {
    let width = area.width;
    let viewport_h = area.height as usize;

    let all_lines = render_all_lines(app, width);
    let total = all_lines.len();

    let scroll = if app.auto_scroll {
        total.saturating_sub(viewport_h)
    } else {
        app.scroll.min(total.saturating_sub(viewport_h))
    };

    let para = Paragraph::new(all_lines)
        .scroll((scroll.min(u16::MAX as usize) as u16, 0));
    frame.render_widget(para, area);

    if total > viewport_h {
        let mut sb_state = ScrollbarState::new(total.saturating_sub(viewport_h))
            .position(scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut sb_state,
        );
    }
}

pub(super) fn draw_picker_overlay(frame: &mut Frame, app: &App, area: Rect) {
    if !matches!(app.state, AppState::Idle) || !app.input.starts_with('/') {
        return;
    }

    let items = filter_commands(&app.input, &app.skill_names);
    if items.is_empty() {
        return;
    }

    let visible = items.len().min(super::PICKER_MAX_ROWS);
    let sel = app.picker_sel.min(items.len().saturating_sub(1));

    let picker_h = (visible + 1) as u16;
    if area.height < picker_h + 1 {
        return;
    }

    let picker_area = Rect {
        x: area.x,
        y: area.y + area.height - picker_h,
        width: area.width,
        height: picker_h,
    };

    frame.render_widget(Clear, picker_area);

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(" commands ", Style::default().fg(Color::DarkGray)));
    let inner = block.inner(picker_area);
    frame.render_widget(block, picker_area);

    let start = if sel >= visible { sel - visible + 1 } else { 0 };
    let rows: Vec<Line<'static>> = items[start..start + visible]
        .iter()
        .enumerate()
        .map(|(i, (cmd, desc))| {
            let idx = start + i;
            let is_sel = idx == sel;

            let cmd_w = 18usize;
            let desc_max = (inner.width as usize).saturating_sub(cmd_w + 2);
            let desc_s: String = desc.chars().take(desc_max).collect();

            let sel_bg = Color::Rgb(60, 55, 80);
            if is_sel {
                Line::from(vec![
                    Span::styled(
                        format!(" {:<w$}", cmd, w = cmd_w),
                        Style::default().fg(Color::Rgb(255, 200, 50)).bg(sel_bg).bold(),
                    ),
                    Span::styled(
                        desc_s,
                        Style::default().fg(Color::Rgb(170, 165, 195)).bg(sel_bg),
                    ),
                    Span::styled(" ".repeat(inner.width as usize), Style::default().bg(sel_bg)),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        format!(" {:<w$}", cmd, w = cmd_w),
                        Style::default().fg(Color::Rgb(100, 180, 255)),
                    ),
                    Span::styled(desc_s.to_string(), Style::default().fg(Color::Rgb(80, 75, 100))),
                ])
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(rows), inner);
}

pub(super) fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let filled = (app.context_pct as usize).min(100) * 10 / 100;
    let ctx_bar: String = (0..10).map(|i| if i < filled { '█' } else { '░' }).collect();

    let cost_str = if app.total_cost_usd == 0.0 {
        "—".to_string()
    } else {
        format!("${:.4}", app.total_cost_usd)
    };

    let model_short: String = if app.model.chars().count() > 17 {
        let mut s: String = app.model.chars().take(14).collect();
        s.push('…');
        s
    } else {
        app.model.clone()
    };

    let spin = super::SPINNER_FRAMES[app.spinner_frame % super::SPINNER_FRAMES.len()];
    let word_idx = (app.turn.wrapping_mul(31).wrapping_add(app.word_tick / 188)) % super::THINKING_WORDS.len();
    let elapsed_secs = app.turn_tick / 62;
    let (state_icon, state_color, state_text): (&str, Color, String) = match &app.state {
        AppState::Idle => ("●", Color::Green, "idle".to_string()),
        AppState::Thinking => {
            // Sidebar is 22 chars; prefix " ⠙ " = 4 chars → 18 chars for text.
            // Drop the thinking word above 99s to keep the seconds counter visible.
            let text = if elapsed_secs >= 100 {
                format!("… {}s", elapsed_secs)
            } else {
                format!("{}… {}s", super::THINKING_WORDS[word_idx], elapsed_secs)
            };
            (spin, Color::Yellow, text)
        }
        AppState::ToolRunning { name, label } => {
            let verb = super::tool_verb(name);
            let short: String = if label.chars().count() > 12 {
                format!("{}…", label.chars().take(11).collect::<String>())
            } else { label.clone() };
            (spin, Color::Cyan, format!("{}  {}", verb, short))
        }
    };

    let label_c = Color::Rgb(130, 125, 155);
    let value_c = Color::Rgb(205, 200, 230);
    let head_c  = Color::Rgb(160, 155, 190);

    let kv = |k: &str, v: String, vc: Color| {
        Line::from(vec![
            Span::styled(format!(" {:<9}", k), Style::default().fg(label_c)),
            Span::styled(v, Style::default().fg(vc)),
        ])
    };

    let mut rows: Vec<Line<'static>> = vec![
        Line::from(Span::styled(" session", Style::default().fg(head_c).bold())),
        Line::from(""),
        kv("model", model_short, value_c),
        kv("branch", app.branch.clone(), Color::Rgb(100, 200, 100)),
        kv("turn", app.turn.to_string(), value_c),
        kv("cost", cost_str, Color::Rgb(200, 180, 80)),
    ];

    if app.tokens_input > 0 || app.tokens_output > 0 {
        let fmt_k = |n: u32| -> String {
            if n >= 1_000_000 { format!("{:.1}M", n as f64 / 1_000_000.0) }
            else if n >= 1000  { format!("{:.1}k", n as f64 / 1_000.0) }
            else               { n.to_string() }
        };
        // Two rows avoids overflow: " in       27.9k" = 15 chars ≤ 22.
        rows.push(kv("in",  fmt_k(app.tokens_input),  Color::Rgb(140, 200, 255)));
        rows.push(kv("out", fmt_k(app.tokens_output), Color::Rgb(160, 220, 160)));
        if app.tokens_cache_read > 0 {
            rows.push(kv("cached", fmt_k(app.tokens_cache_read), Color::Rgb(160, 140, 220)));
        }
    }

    rows.push(Line::from(""));
    rows.push(Line::from(Span::styled(" context", Style::default().fg(head_c).bold())));
    let bar_color = if app.context_pct > 80 { Color::Rgb(220, 80, 80) }
        else if app.context_pct > 60 { Color::Rgb(220, 180, 50) }
        else { Color::Rgb(80, 160, 255) };
    rows.push(Line::from(vec![
        Span::styled(format!(" {}", ctx_bar), Style::default().fg(bar_color)),
        Span::styled(format!(" {}%", app.context_pct), Style::default().fg(Color::Rgb(155, 150, 185))),
    ]));
    rows.push(Line::from(""));
    rows.push(Line::from(Span::styled(" status", Style::default().fg(head_c).bold())));
    rows.push(Line::from(vec![
        Span::styled(format!(" {} ", state_icon), Style::default().fg(state_color)),
        Span::styled(state_text, Style::default().fg(value_c)),
    ]));

    if app.active_skill.is_some() || !app.skill_history.is_empty() {
        let skill_c  = Color::Rgb(255, 200, 60);
        let dim_c    = Color::Rgb(110, 105, 140);
        let turn_c   = Color::Rgb(80, 75, 105);
        let trunc = |s: &str, n: usize| -> String {
            if s.chars().count() > n { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
            else { s.to_string() }
        };
        rows.push(Line::from(""));
        rows.push(Line::from(Span::styled(" skills", Style::default().fg(skill_c).bold())));
        if let Some(ref label) = app.active_skill {
            let turn_no = app.turn + 1;
            rows.push(Line::from(vec![
                Span::styled(format!(" T{:<3}", turn_no), Style::default().fg(skill_c)),
                Span::styled(trunc(label, 14), Style::default().fg(Color::Rgb(255, 230, 140)).bold()),
            ]));
        }
        let history_start = if app.active_skill.is_some() { 1 } else { 0 };
        for (turn_no, label) in app.skill_history.iter().skip(history_start).take(8) {
            rows.push(Line::from(vec![
                Span::styled(format!(" T{:<3}", turn_no), Style::default().fg(turn_c)),
                Span::styled(trunc(label, 14), Style::default().fg(dim_c)),
            ]));
        }
    }

    if let Some(ref gs) = app.goal_state {
        let goal_c = Color::Rgb(120, 220, 180);
        let short_cond: String = if gs.condition.chars().count() > 15 {
            format!("{}…", gs.condition.chars().take(14).collect::<String>())
        } else {
            gs.condition.clone()
        };
        let elapsed = gs.started_at.elapsed().as_secs();
        rows.push(Line::from(""));
        rows.push(Line::from(Span::styled(" goal", Style::default().fg(goal_c).bold())));
        rows.push(kv("cond", short_cond, Color::Rgb(190, 240, 210)));
        rows.push(kv("turn", format!("{}/{}", gs.turns_done, gs.max_turns), Color::Rgb(220, 210, 100)));
        rows.push(kv("time", format!("{}s", elapsed), Color::Rgb(170, 165, 195)));
    }

    // ── Active task list (todo_write/todo_read tools) ─────────────────────────
    let todos = crate::tools::global_todos();
    if !todos.is_empty() {
        let done  = todos.iter().filter(|t| t.status == crate::tools::todo::TodoStatus::Done).count();
        let total = todos.len();
        let todo_head_c = Color::Rgb(100, 200, 255);
        let done_c   = Color::Rgb(100, 180, 120);
        let active_c = Color::Rgb(255, 210, 80);
        let pend_c   = Color::Rgb(130, 125, 160);
        rows.push(Line::from(""));
        rows.push(Line::from(vec![
            Span::styled(" tasks", Style::default().fg(todo_head_c).bold()),
            Span::styled(format!(" {done}/{total}"), Style::default().fg(pend_c)),
        ]));
        let max_w = (area.width as usize).saturating_sub(5).max(10);
        for t in &todos {
            let (icon_c, icon) = match t.status {
                crate::tools::todo::TodoStatus::Done       => (done_c,   "●"),
                crate::tools::todo::TodoStatus::InProgress => (active_c, "◑"),
                crate::tools::todo::TodoStatus::Pending    => (pend_c,   "○"),
            };
            let text: String = if t.content.chars().count() > max_w {
                format!("{}…", t.content.chars().take(max_w.saturating_sub(1)).collect::<String>())
            } else {
                t.content.clone()
            };
            rows.push(Line::from(vec![
                Span::styled(format!(" {} ", icon), Style::default().fg(icon_c)),
                Span::styled(text, Style::default().fg(pend_c)),
            ]));
        }
    }

    let block = Block::default().borders(Borders::LEFT).border_style(Style::default().fg(Color::Rgb(45, 42, 60)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(rows).wrap(Wrap { trim: false }), inner);
}

pub(super) fn draw_dir_panel(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 { return; }
    let max_w = (area.width as usize).saturating_sub(6).max(20);

    let path_display: String = {
        let chars: Vec<char> = app.cwd.chars().collect();
        if chars.len() <= max_w {
            app.cwd.clone()
        } else {
            let keep = max_w.saturating_sub(1);
            format!("…{}", chars[chars.len() - keep..].iter().collect::<String>())
        }
    };

    let hint_row: Line<'static> = if let Some(ref gs) = app.goal_state {
        let cond: String = if gs.condition.chars().count() > 22 {
            format!("{}…", gs.condition.chars().take(21).collect::<String>())
        } else {
            gs.condition.clone()
        };
        Line::from(vec![
            Span::styled("     ⊛ ".to_string(), Style::default().fg(Color::Rgb(120, 220, 180)).bold()),
            Span::styled(
                format!("{}/{}: {}", gs.turns_done, gs.max_turns, cond),
                Style::default().fg(Color::Rgb(150, 230, 200)),
            ),
            Span::styled("  /goal stop".to_string(), Style::default().fg(Color::Rgb(80, 75, 100))),
        ])
    } else {
        Line::from(Span::styled(
            "     Ctrl+F files  Ctrl+P dir  /cd <path>".to_string(),
            Style::default().fg(Color::Rgb(60, 58, 80)),
        ))
    };

    let rows: Vec<Line<'static>> = vec![
        Line::from(vec![
            Span::styled("  ⌂ ".to_string(), Style::default().fg(Color::Rgb(100, 95, 125))),
            Span::styled(path_display, Style::default().fg(Color::Rgb(140, 200, 255))),
        ]),
        hint_row,
        Line::from(""),
    ];
    frame.render_widget(Paragraph::new(rows), area);
}

/// Render the input box; returns the screen position for the native cursor.
pub(super) fn draw_input(frame: &mut Frame, app: &App, area: Rect) -> Option<(u16, u16)> {
    let prefix = "❯ ";
    let prefix_chars = prefix.chars().count();
    let cursor_pos = app.cursor;
    let content_w = area.width.saturating_sub(2) as usize;

    let (scroll, cursor_screen) = if content_w > 0 {
        let cursor_char_in_text = prefix_chars + cursor_pos;
        let cursor_row = cursor_char_in_text / content_w;
        let cursor_col = cursor_char_in_text % content_w;
        let visible_rows = area.height.saturating_sub(2) as usize;
        let scroll = if cursor_row >= visible_rows {
            (cursor_row - visible_rows + 1) as u16
        } else {
            0u16
        };
        let screen_x = area.x + 1 + cursor_col as u16;
        let screen_y = area.y + 1 + cursor_row as u16 - scroll;
        (scroll, Some((screen_x, screen_y)))
    } else {
        (0u16, None)
    };

    let spans: Vec<Span<'static>> = vec![
        Span::styled(prefix.to_string(), Style::default().fg(Color::Rgb(255, 200, 50)).bold()),
        Span::raw(app.input.clone()),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(80, 80, 80)));

    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );

    cursor_screen
}

pub(super) fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let spin = super::SPINNER_FRAMES[app.spinner_frame % super::SPINNER_FRAMES.len()];
    let word_idx = (app.turn.wrapping_mul(31).wrapping_add(app.word_tick / 188)) % super::THINKING_WORDS.len();
    let elapsed_secs = app.turn_tick / 62;
    let (hint, hint_color) = match &app.state {
        AppState::Idle => (String::new(), Color::DarkGray),
        AppState::Thinking => (
            format!("  {} {}… {}s  │", spin, super::THINKING_WORDS[word_idx], elapsed_secs),
            Color::Yellow,
        ),
        AppState::ToolRunning { name, .. } => (
            format!("  {} {}…  │", spin, super::tool_verb(name)),
            Color::Cyan,
        ),
    };

    let goal_badge: String = if let Some(ref gs) = app.goal_state {
        format!("  ⊛ goal {}/{}  │", gs.turns_done, gs.max_turns)
    } else {
        String::new()
    };

    let keybinds = if matches!(app.state, AppState::Idle) {
        "  ↑↓ scroll  Tab  Ctrl+O collapse  Ctrl+F files  Ctrl+G diff  Ctrl+P dir  Ctrl+N new  Ctrl+Q quit"
    } else {
        "  ↑↓ scroll  Ctrl+O collapse  Ctrl+C cancel"
    };
    let mut spans = vec![
        Span::styled(hint, Style::default().fg(hint_color)),
    ];
    if !goal_badge.is_empty() {
        spans.push(Span::styled(goal_badge, Style::default().fg(Color::Rgb(120, 220, 180)).bold()));
    }
    spans.push(Span::styled(keybinds, Style::default().fg(Color::DarkGray)));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Set the native cursor when idle and no popup is covering the input area.
pub(super) fn maybe_set_cursor(frame: &mut Frame, app: &App, cursor_pos: Option<(u16, u16)>) {
    let no_popup = app.permission_popup.is_none()
        && app.session_picker.is_none()
        && app.mode_picker.is_none()
        && app.domain_picker.is_none()
        && app.file_browser.is_none();
    if matches!(app.state, AppState::Idle) && no_popup {
        if let Some((cx, cy)) = cursor_pos {
            frame.set_cursor_position((cx, cy));
        }
    }
}
