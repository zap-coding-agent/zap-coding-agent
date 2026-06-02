use ratatui::{prelude::*, widgets::*};
use ratatui::style::Modifier;
use super::super::app::{App, InitWizardStep};

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
