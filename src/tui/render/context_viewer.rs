use ratatui::{prelude::*, widgets::*};
use ratatui::style::Modifier;
use crate::tui::app::{App, ContextViewerState, DetailBlock};

pub(super) fn draw_context_viewer(frame: &mut Frame, app: &App, area: Rect) {
    let viewer = match &app.context_viewer {
        Some(v) => v,
        None => return,
    };

    let w = ((area.width as f32 * 0.92) as u16).max(70).min(area.width);
    let h = ((area.height as f32 * 0.85) as u16).max(12).min(area.height);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let bar = ctx_fill_bar(viewer.context_pct);
    let used_k = (viewer.total_tokens / 1000).max(1);
    let limit_k = viewer.limit_tokens / 1000;
    let pct_label = if viewer.context_pct == 0 && viewer.total_tokens > 0 {
        "<1%".to_string()
    } else {
        format!("{}%", viewer.context_pct)
    };
    let title = format!(
        " ⚡ Context  {} {}  (~{}k / {}k) ",
        bar, pct_label, used_k, limit_k
    );

    let outer_border_c = Color::Yellow;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(outer_border_c))
        .title(Span::styled(title, Style::default().fg(Color::Yellow).bold()));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    if inner.height < 6 {
        return;
    }

    let footer_h: u16 = 2;
    let body_h = inner.height.saturating_sub(footer_h);

    let body_area   = Rect { x: inner.x, y: inner.y, width: inner.width, height: body_h };
    let footer_area = Rect { x: inner.x, y: inner.y + body_h, width: inner.width, height: footer_h };

    let list_w  = (inner.width as u32 * 38 / 100) as u16;
    let detail_w = inner.width.saturating_sub(list_w);

    let list_area   = Rect { x: body_area.x, y: body_area.y, width: list_w, height: body_h };
    let detail_area = Rect { x: body_area.x + list_w, y: body_area.y, width: detail_w, height: body_h };

    let (list_border_c, detail_border_c) = if viewer.detail_focus {
        (Color::DarkGray, Color::Yellow)
    } else {
        (Color::Yellow, Color::DarkGray)
    };

    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(list_border_c))
        .title(Span::styled(" Turns ", Style::default().fg(list_border_c)));
    let list_inner = list_block.inner(list_area);
    frame.render_widget(list_block, list_area);

    let detail_title = if let Some(t) = viewer.turns.get(viewer.selected) {
        let pct = if viewer.total_tokens > 0 { (t.tokens_est * 100) / viewer.total_tokens } else { 0 };
        let pct_str = if pct == 0 && t.tokens_est > 0 { "<1%".to_string() } else { format!("{}%", pct) };
        format!(" Detail  {}  {} of context ", fmt_tokens(t.tokens_est), pct_str)
    } else {
        " Detail ".to_string()
    };
    let detail_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(detail_border_c))
        .title(Span::styled(detail_title, Style::default().fg(detail_border_c)));
    let detail_inner = detail_block.inner(detail_area);
    frame.render_widget(detail_block, detail_area);

    draw_turn_list(frame, viewer, list_inner);
    draw_turn_detail(frame, viewer, detail_inner);

    let dim = Style::default().fg(Color::DarkGray);
    let sep = "─".repeat(inner.width as usize);
    frame.render_widget(Paragraph::new(sep).style(dim), footer_area);

    let hint = if viewer.confirm_clear {
        Span::styled(
            "  ⚠  Clear ALL history? [y] confirm   [any key] cancel",
            Style::default().fg(Color::Red).bold(),
        )
    } else if viewer.confirm_drop {
        let turn_num = viewer.selected + 1;
        let tok = viewer.turns.get(viewer.selected).map(|t| fmt_tokens(t.tokens_est)).unwrap_or_default();
        let pct = viewer.turns.get(viewer.selected).and_then(|t| {
            if viewer.total_tokens > 0 { Some((t.tokens_est * 100) / viewer.total_tokens) } else { None }
        }).unwrap_or(0);
        Span::styled(
            format!("  ⚠  Drop Turn {turn_num} ({tok}, {pct}% of context)? [Enter] confirm   [Esc] cancel"),
            Style::default().fg(Color::Red).bold(),
        )
    } else if viewer.detail_focus {
        Span::styled(
            "  ↑↓/jk scroll detail   ← or Esc = back to list   [d] drop   [c] compact   [x] clear",
            dim,
        )
    } else {
        Span::styled(
            "  ↑↓/jk navigate   → or Enter = detail   [d] drop   [c] compact   [x] clear   Esc close",
            dim,
        )
    };
    let hint_area = Rect { y: footer_area.y + 1, height: 1, ..footer_area };
    frame.render_widget(Paragraph::new(Line::from(hint)), hint_area);
}

fn fmt_tokens(t: usize) -> String {
    if t >= 1000 { format!("~{:.1}k", t as f64 / 1000.0) } else { format!("~{}", t) }
}

fn draw_turn_list(frame: &mut Frame, viewer: &ContextViewerState, area: Rect) {
    if viewer.turns.is_empty() {
        frame.render_widget(
            Paragraph::new("  No turns yet.")
                .style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    }

    let visible = area.height as usize;
    let scroll = if viewer.selected >= visible { viewer.selected + 1 - visible } else { 0 };
    let dim = Style::default().fg(Color::DarkGray);

    let mut lines: Vec<Line> = Vec::new();
    for (i, turn) in viewer.turns.iter().enumerate().skip(scroll).take(visible) {
        let is_sel = i == viewer.selected;
        let tok_label = fmt_tokens(turn.tokens_est);
        let pct = if viewer.total_tokens > 0 { (turn.tokens_est * 100) / viewer.total_tokens } else { 0 };

        let (win_sym, win_style) = if turn.in_window {
            ("▓", Style::default().fg(Color::Green))
        } else {
            ("░", dim)
        };

        let right_w: usize = 14;
        let prefix_w: usize = 3;
        let preview_w = (area.width as usize).saturating_sub(prefix_w + right_w).max(4);
        let preview: String = turn.preview.chars().take(preview_w).collect();

        let dropping = is_sel && viewer.confirm_drop;
        let sel_fg = if dropping { Color::Red } else { Color::Yellow };
        let cursor_style = if is_sel { Style::default().fg(sel_fg).add_modifier(Modifier::BOLD) } else { dim };
        let text_style = if dropping {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        } else if is_sel {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };

        lines.push(Line::from(vec![
            Span::styled(if dropping { "✕ " } else if is_sel { "▶ " } else { "  " }, cursor_style),
            Span::styled(format!("{:<preview_w$}", preview), text_style),
            Span::styled(format!(" {:>6}", tok_label), if dropping { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Cyan) }),
            Span::styled(format!(" {:>3}%", pct), if dropping { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Yellow) }),
            Span::styled(" ", dim),
            Span::styled(win_sym, win_style),
        ]));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_turn_detail(frame: &mut Frame, viewer: &ContextViewerState, area: Rect) {
    let turn = match viewer.turns.get(viewer.selected) {
        Some(t) => t,
        None => {
            frame.render_widget(
                Paragraph::new("  Select a turn in the list.")
                    .style(Style::default().fg(Color::DarkGray)),
                area,
            );
            return;
        }
    };

    let dim      = Style::default().fg(Color::DarkGray);
    let muted    = Style::default().fg(Color::Rgb(140, 135, 165));
    let accent   = Style::default().fg(Color::Cyan);
    let user_c   = Style::default().fg(Color::Rgb(100, 210, 255));
    let asst_c   = Style::default().fg(Color::Rgb(180, 220, 140));
    let tool_c   = Style::default().fg(Color::Rgb(255, 200, 80));
    let result_c = Style::default().fg(Color::Rgb(160, 140, 200));
    let width    = area.width as usize;

    let mut all_lines: Vec<Line<'static>> = Vec::new();

    for block in &turn.detail.blocks {
        match block {
            DetailBlock::UserText { text, tokens } => {
                all_lines.push(Line::from(vec![
                    Span::styled("━━ User ", user_c),
                    Span::styled(fmt_tokens(*tokens), accent),
                    Span::styled(" ━".repeat((width.saturating_sub(10)) / 2), dim),
                ]));
                for line in text.lines() {
                    all_lines.push(Line::from(Span::styled(
                        format!("  {}", truncate_str(line, width.saturating_sub(2))),
                        Style::default().fg(Color::White),
                    )));
                }
                all_lines.push(Line::from(""));
            }
            DetailBlock::AssistantText { text, tokens } => {
                all_lines.push(Line::from(vec![
                    Span::styled("━━ Assistant ", asst_c),
                    Span::styled(fmt_tokens(*tokens), accent),
                    Span::styled(" ━".repeat((width.saturating_sub(15)) / 2), dim),
                ]));
                for line in text.lines() {
                    all_lines.push(Line::from(Span::styled(
                        format!("  {}", truncate_str(line, width.saturating_sub(2))),
                        muted,
                    )));
                }
                all_lines.push(Line::from(""));
            }
            DetailBlock::ToolCall { name, input_json, tokens } => {
                all_lines.push(Line::from(vec![
                    Span::styled("━━ Tool call: ", tool_c),
                    Span::styled(name.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled("  ", dim),
                    Span::styled(fmt_tokens(*tokens), accent),
                    Span::styled(" ━".repeat((width.saturating_sub(name.len() + 20)) / 2), dim),
                ]));
                for line in input_json.lines() {
                    all_lines.push(Line::from(Span::styled(
                        format!("  {}", truncate_str(line, width.saturating_sub(2))),
                        Style::default().fg(Color::Rgb(200, 200, 200)),
                    )));
                }
                all_lines.push(Line::from(""));
            }
            DetailBlock::ToolResult { tool_name, content, tokens } => {
                let label = if tool_name.is_empty() {
                    "━━ Tool result ".to_string()
                } else {
                    format!("━━ Result: {} ", tool_name)
                };
                all_lines.push(Line::from(vec![
                    Span::styled(label, result_c),
                    Span::styled(fmt_tokens(*tokens), accent),
                ]));
                for line in content.lines() {
                    all_lines.push(Line::from(Span::styled(
                        format!("  {}", truncate_str(line, width.saturating_sub(2))),
                        dim,
                    )));
                }
                all_lines.push(Line::from(""));
            }
        }
    }

    if all_lines.is_empty() {
        frame.render_widget(
            Paragraph::new("  (empty turn)")
                .style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    }

    let scroll = viewer.detail_scroll.min(all_lines.len().saturating_sub(1));
    let para = Paragraph::new(all_lines)
        .scroll((scroll as u16, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max_chars.saturating_sub(1)).collect::<String>())
    }
}

fn ctx_fill_bar(pct: u8) -> String {
    let filled = (pct as usize).min(100) * 10 / 100;
    let bar: String = (0..10)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect();
    format!("[{bar}]")
}
