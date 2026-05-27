use ratatui::{prelude::*, widgets::*};
use super::super::app::App;
use super::SIDEBAR_W;

// ZAP in 5-row pixel letters, 8 chars wide, 2-char gap:
//
//   Z: ████████    A:   ████    P: ███████
//         ██          ██   ██      ██   ██
//       ████          ████████     ███████
//     ██              ██   ██      ██
//   ████████          ██   ██      ██
const ZAP_ROW: [&str; 5] = [
    " ████████   ████     ███████ ",
    "       ██  ██   ██   ██   ██ ",
    "     ████  ████████  ███████  ",
    "   ██      ██   ██   ██       ",
    " ████████  ██   ██   ██       ",
];

pub(super) fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(60, 55, 80)));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let use_split = inner.width > SIDEBAR_W + 30;
    let info_w = SIDEBAR_W.saturating_sub(1);

    if use_split {
        let cols = Layout::horizontal([
            Constraint::Min(20),
            Constraint::Length(1),
            Constraint::Length(info_w),
        ])
        .split(inner);

        for row in 0..inner.height {
            frame.render_widget(
                Paragraph::new(Span::styled("│", Style::default().fg(Color::Rgb(60, 55, 80)))),
                Rect { x: cols[1].x, y: inner.y + row, width: 1, height: 1 },
            );
        }

        draw_zap_art(frame, cols[0]);
        draw_header_info(frame, app, cols[2]);
    } else {
        draw_zap_art(frame, inner);
    }
}

fn draw_zap_art(frame: &mut Frame, area: Rect) {
    let colors = [
        Color::Rgb(255, 215, 40),
        Color::Rgb(255, 195, 20),
        Color::Rgb(255, 170,  0),
        Color::Rgb(240, 145,  0),
        Color::Rgb(220, 120,  0),
    ];
    let rows: Vec<Line<'static>> = ZAP_ROW
        .iter()
        .zip(colors.iter())
        .map(|(row, &c)| Line::from(Span::styled(row.to_string(), Style::default().fg(c).bold())))
        .collect();
    frame.render_widget(Paragraph::new(rows), area);
}

fn draw_header_info(frame: &mut Frame, app: &App, area: Rect) {
    let ver = env!("CARGO_PKG_VERSION");
    let model_short: String = if app.model.chars().count() > 16 {
        let mut s: String = app.model.chars().take(15).collect();
        s.push('…');
        s
    } else {
        app.model.clone()
    };

    let mut git_status = format!(" ◉ {}", app.branch);
    if app.git_dirty {
        git_status.push_str(" *");
    }
    if app.git_ahead > 0 {
        git_status.push_str(&format!(" ↑{}", app.git_ahead));
    }
    if app.git_behind > 0 {
        git_status.push_str(&format!(" ↓{}", app.git_behind));
    }

    let git_color = if app.git_dirty { Color::Yellow } else { Color::Green };

    let rows: Vec<Line<'static>> = vec![
        Line::from(vec![
            Span::styled(" ⚡ ", Style::default().fg(Color::Rgb(255, 210, 40)).bold()),
            Span::styled("zap", Style::default().fg(Color::Rgb(255, 210, 40)).bold()),
            Span::styled(format!("  v{}", ver), Style::default().fg(Color::Rgb(130, 125, 155))),
        ]),
        Line::from(Span::styled(format!(" {}", model_short), Style::default().fg(Color::Rgb(170, 165, 200)))),
        Line::from(Span::styled(git_status, Style::default().fg(git_color))),
        Line::from(Span::styled(format!(" {} turns", app.turn), Style::default().fg(Color::Rgb(155, 150, 185)))),
        Line::from(vec![
            Span::styled(format!(" {}%", app.context_pct), Style::default().fg(
                if app.context_pct > 80 { Color::Red }
                else if app.context_pct > 60 { Color::Yellow }
                else { Color::Rgb(155, 150, 185) }
            )),
            Span::styled(" ctx", Style::default().fg(Color::Rgb(130, 125, 155))),
        ]),
    ];
    frame.render_widget(Paragraph::new(rows), area);
}
