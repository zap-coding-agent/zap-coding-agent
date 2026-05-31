use ratatui::{prelude::*, widgets::*};
use super::super::app::App;

pub(super) fn draw_command_popup(frame: &mut Frame, app: &App, area: Rect) {
    let popup = match app.command_popup.as_ref() {
        Some(p) => p,
        None    => return,
    };

    let w = (area.width as f32 * 0.82) as u16;
    let h = ((area.height as f32 * 0.7) as u16).max(6).min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let border_c = Color::Rgb(100, 180, 255);
    let dim = Color::Rgb(80, 75, 100);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_c))
        .title(Span::styled(
            format!(" {}  Esc dismiss ", popup.title),
            Style::default().fg(Color::Yellow).bold(),
        ));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    let text_lines: Vec<Line<'static>> = popup.text.lines().map(|l| {
        Line::from(Span::styled(l.to_string(), Style::default().fg(Color::Rgb(205, 200, 230))))
    }).collect();

    let total = text_lines.len();
    let viewport_h = inner.height as usize;
    let scroll = popup.scroll.min(total.saturating_sub(viewport_h));

    frame.render_widget(
        Paragraph::new(text_lines).scroll((scroll as u16, 0)),
        inner,
    );

    if total > viewport_h {
        let mut sb_state = ScrollbarState::new(total.saturating_sub(viewport_h)).position(scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            inner,
            &mut sb_state,
        );
    }

    let hint = Rect { y: inner.y + inner.height.saturating_sub(1), height: 1, ..inner };
    frame.render_widget(
        Paragraph::new(Span::styled("  ↑↓ scroll   Esc dismiss ", Style::default().fg(dim))),
        hint,
    );
}

pub(super) fn draw_permission_popup(frame: &mut Frame, app: &App, area: Rect) {
    let popup = match app.permission_popup.as_ref() {
        Some(p) => p,
        None => return,
    };

    let pending = &popup.pending;
    let mut lines: Vec<Line<'static>> = Vec::new();

    if pending.len() == 1 {
        let (_, name, ctx) = &pending[0];
        lines.push(Line::from(vec![
            Span::styled("  Tool: ", Style::default().fg(Color::Rgb(100, 95, 130))),
            Span::styled(name.clone(), Style::default().fg(Color::Rgb(100, 210, 255)).bold()),
        ]));
        if !ctx.is_empty() {
            let ctx_flat = ctx.replace('\n', " ").replace('\r', "");
            lines.push(Line::from(vec![
                Span::styled("  What: ", Style::default().fg(Color::Rgb(100, 95, 130))),
                Span::styled(ctx_flat, Style::default().fg(Color::Rgb(130, 125, 150))),
            ]));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  Agent wants to run {} operation(s):", pending.len()),
                Style::default().fg(Color::Rgb(100, 95, 130)),
            ),
        ]));
        for (_, name, ctx) in pending.iter().take(8) {
            let name_col = if name.chars().count() > 14 {
                format!("{}…", name.chars().take(13).collect::<String>())
            } else {
                name.clone()
            };
            let ctx_flat = ctx.replace('\n', " ").replace('\r', "");
            let ctx_disp = if ctx_flat.chars().count() > 40 {
                format!("{}…", ctx_flat.chars().take(39).collect::<String>())
            } else {
                ctx_flat
            };
            lines.push(Line::from(vec![
                Span::styled("    · ", Style::default().fg(Color::Rgb(70, 65, 90))),
                Span::styled(format!("{:<14}", name_col), Style::default().fg(Color::Rgb(100, 210, 255)).bold()),
                Span::styled("  ", Style::default().fg(Color::Reset)),
                Span::styled(ctx_disp, Style::default().fg(Color::Rgb(130, 125, 150))),
            ]));
        }
        if pending.len() > 8 {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("    … and {} more", pending.len() - 8),
                    Style::default().fg(Color::Rgb(80, 75, 100)),
                ),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  [", Style::default().fg(Color::Rgb(80, 75, 100))),
        Span::styled("Y", Style::default().fg(Color::Rgb(100, 230, 100)).bold()),
        Span::styled("] Allow   ", Style::default().fg(Color::Rgb(80, 75, 100))),
        Span::styled("[", Style::default().fg(Color::Rgb(80, 75, 100))),
        Span::styled("N", Style::default().fg(Color::Rgb(230, 100, 100)).bold()),
        Span::styled("] Deny   ", Style::default().fg(Color::Rgb(80, 75, 100))),
        Span::styled("[", Style::default().fg(Color::Rgb(80, 75, 100))),
        Span::styled("A", Style::default().fg(Color::Rgb(230, 230, 100)).bold()),
        Span::styled("] Always allow", Style::default().fg(Color::Rgb(80, 75, 100))),
    ]));

    let dialog_h = (lines.len() + 3).min(area.height as usize) as u16;
    let dialog_w = (area.width as f32 * 0.78) as u16;
    let x = area.x + (area.width.saturating_sub(dialog_w)) / 2;
    let y = area.y + area.height.saturating_sub(dialog_h);
    let overlay = Rect { x, y, width: dialog_w, height: dialog_h };

    frame.render_widget(Clear, overlay);

    let border_c = Color::Rgb(180, 130, 70);
    let bg = Color::Rgb(25, 22, 35);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_c))
        .style(Style::default().bg(bg))
        .title(Span::styled(
            " Permission Required ",
            Style::default().fg(Color::Rgb(255, 200, 100)).bold(),
        ));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);
    frame.render_widget(Paragraph::new(lines).style(Style::default().bg(bg)), inner);
}

pub(super) fn draw_btw_input(frame: &mut Frame, app: &App, area: Rect) {
    let bg = Color::Rgb(15, 22, 30);
    let border_c = Color::Rgb(80, 160, 220);

    let dialog_h: u16 = 4;
    let dialog_w = (area.width as f32 * 0.72) as u16;
    let x = area.x + (area.width.saturating_sub(dialog_w)) / 2;
    let y = area.y + area.height.saturating_sub(dialog_h);
    let overlay = Rect { x, y, width: dialog_w, height: dialog_h };

    frame.render_widget(Clear, overlay);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_c))
        .style(Style::default().bg(bg))
        .title(Span::styled(
            " ↳ btw — add context to the running turn ",
            Style::default().fg(Color::Rgb(120, 200, 255)).bold(),
        ));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    let input_line = Line::from(vec![
        Span::styled("  › ", Style::default().fg(Color::Rgb(80, 160, 220))),
        Span::styled(app.btw_draft.clone(), Style::default().fg(Color::White)),
        Span::styled("▌", Style::default().fg(Color::Rgb(120, 200, 255))),
    ]);
    let hint_line = Line::from(Span::styled(
        "  Enter to send · Esc to cancel",
        Style::default().fg(Color::Rgb(60, 80, 100)),
    ));

    frame.render_widget(
        Paragraph::new(vec![input_line, hint_line]).style(Style::default().bg(bg)),
        inner,
    );
}
