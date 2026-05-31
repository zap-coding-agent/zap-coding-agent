use ratatui::{prelude::*, widgets::*};
use super::super::app::App;

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
            let ready_badge = if entry.ready { " ✓ ready" } else { "" };

            let hint_line = format!("{}{}  {}", hint_icon, entry.name, entry.hint);
            let padding = w.saturating_sub(hint_line.chars().count() as u16 + 4) as usize;
            if is_sel {
                let mut spans = vec![
                    Span::styled(marker.to_string(), Style::default().fg(border_c).bg(sel_bg).bold()),
                    Span::styled(hint_line, Style::default().fg(Color::White).bg(sel_bg).bold()),
                ];
                if entry.ready {
                    spans.push(Span::styled(
                        ready_badge,
                        Style::default().fg(Color::Green).bg(sel_bg).bold(),
                    ));
                }
                spans.push(Span::styled(
                    " ".repeat(padding.saturating_sub(ready_badge.len())),
                    Style::default().bg(sel_bg),
                ));
                Line::from(spans)
            } else if entry.coming_soon {
                Line::from(vec![
                    Span::styled(marker.to_string(), Style::default().fg(Color::Rgb(80, 75, 100))),
                    Span::styled(hint_line, Style::default().fg(Color::Rgb(80, 75, 100))),
                ])
            } else {
                let mut spans = vec![
                    Span::styled(marker.to_string(), Style::default().fg(Color::Rgb(80, 75, 100))),
                    Span::styled(hint_icon.to_string(), Style::default().fg(Color::Rgb(80, 75, 100))),
                    Span::styled(entry.name.clone(), Style::default().fg(Color::Rgb(100, 210, 255)).bold()),
                    Span::styled(format!("  {}", entry.hint), Style::default().fg(Color::Rgb(120, 115, 150))),
                ];
                if entry.ready {
                    spans.push(Span::styled(
                        ready_badge,
                        Style::default().fg(Color::Green),
                    ));
                }
                Line::from(spans)
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(rows), inner);
}
