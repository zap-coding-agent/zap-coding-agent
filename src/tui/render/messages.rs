use ratatui::prelude::*;
use ratatui::style::Modifier;
use std::collections::HashSet;
use super::super::app::{App, AppState, MsgRole, StreamingBlock, UiBlock, UiMessage, UiToolCall};
use super::super::syntax;

pub fn render_all_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    for msg in &app.messages {
        lines.extend(message_to_lines(msg, width, &app.expanded_tools));
        lines.push(Line::from(""));
    }

    let has_streaming = !app.streaming_blocks.is_empty()
        || matches!(app.state, AppState::Thinking | AppState::ToolRunning { .. });

    if has_streaming {
        lines.push(role_line(&MsgRole::Assistant));

        for sb in &app.streaming_blocks {
            match sb {
                StreamingBlock::Thinking(text) => {
                    lines.extend(thinking_streaming_lines(text));
                }
                StreamingBlock::Text(text) => {
                    lines.extend(text_to_lines(text, width));
                }
                StreamingBlock::Tool(tc) => {
                    lines.extend(tool_call_lines(tc, app.expanded_tools.contains(tc.id.as_str()), width));
                }
            }
        }

        match &app.state {
            AppState::Thinking | AppState::ToolRunning { .. } => {
                let spin = super::SPINNER_FRAMES[app.spinner_frame % super::SPINNER_FRAMES.len()];
                let word_idx = (app.turn.wrapping_mul(31).wrapping_add(app.word_tick / 188)) % super::THINKING_WORDS.len();
                let elapsed_secs = app.turn_tick / 62;
                let (label, color) = match &app.state {
                    AppState::ToolRunning { name, label } => {
                        let verb = super::tool_verb(name);
                        let short: String = if label.chars().count() > 38 {
                            format!("{}…", label.chars().take(37).collect::<String>())
                        } else { label.clone() };
                        (format!("  {} {}  {}", spin, verb, short), Color::Cyan)
                    }
                    _ => (
                        format!("  {} {}… {}s", spin, super::THINKING_WORDS[word_idx], elapsed_secs),
                        Color::Yellow,
                    ),
                };
                lines.push(Line::from(vec![
                    Span::styled(label, Style::default().fg(color)),
                ]));
            }
            AppState::Idle => {}
        }
    }

    if let Some(err) = &app.error {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  \u{2717} Error: ".to_string(), Style::default().fg(Color::Red).bold()),
            Span::styled(err.clone(), Style::default().fg(Color::Red)),
        ]));
    }

    // Hard-clip every line to width to prevent overflow into the sidebar.
    let max_w = (width as usize).saturating_sub(2).max(20);
    for line in &mut lines {
        let total: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        if total > max_w {
            line.spans = truncate_spans(std::mem::take(&mut line.spans), max_w);
        }
    }

    lines
}

pub fn message_to_lines(msg: &UiMessage, width: u16, expanded: &HashSet<String>) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(role_line(&msg.role));
    for block in &msg.blocks {
        match block {
            UiBlock::Text(text)                        => lines.extend(text_to_lines(text, width)),
            UiBlock::Code { lang, lines: code_lines }  => lines.extend(code_block_lines(lang, code_lines)),
            UiBlock::Tool(tc)                          => lines.extend(tool_call_lines(tc, expanded.contains(&tc.id), width)),
            UiBlock::Diff { path, content }            => lines.extend(diff_block_lines(path, content, width)),
            UiBlock::Thinking { char_count }           => lines.extend(thinking_collapsed_line(*char_count)),
            UiBlock::Warning(text) => {
                lines.push(Line::from(""));
                lines.extend(
                    text.lines().map(|l| {
                        Line::from(vec![Span::styled(
                            format!("  ⚠ {l}"),
                            Style::default().fg(Color::Red).bold(),
                        )])
                    }),
                );
                lines.push(Line::from(""));
            }
        }
    }
    lines
}

pub fn diff_block_lines(path: &str, content: &str, width: u16) -> Vec<Line<'static>> {
    let border = Color::Rgb(55, 50, 75);
    let max_w = (width as usize).saturating_sub(6).max(20);
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("  \u{2726} ", Style::default().fg(Color::Rgb(100, 180, 255)).bold()),
        Span::styled(path.to_string(), Style::default().fg(Color::Rgb(180, 175, 210)).bold()),
    ]));

    for raw in content.lines() {
        let display = expand_tabs_and_truncate(raw, max_w);
        let style = if raw.starts_with("+++") || raw.starts_with("---") {
            Style::default().fg(Color::Rgb(120, 115, 145))
        } else if raw.starts_with('+') {
            Style::default().fg(Color::Rgb(100, 210, 120))
        } else if raw.starts_with('-') {
            Style::default().fg(Color::Rgb(220,  80,  80))
        } else if raw.starts_with("@@") {
            Style::default().fg(Color::Rgb(100, 180, 255))
        } else {
            Style::default().fg(Color::Rgb(175, 170, 200))
        };
        lines.push(Line::from(vec![
            Span::styled("    ", Style::default().fg(border)),
            Span::styled(display, style),
        ]));
    }

    lines
}

pub fn role_line(role: &MsgRole) -> Line<'static> {
    match role {
        MsgRole::User => Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("◆ ", Style::default().fg(Color::Rgb(100, 210, 255)).bold()),
            Span::styled("You", Style::default().fg(Color::Rgb(100, 210, 255)).bold()),
        ]),
        MsgRole::Assistant => Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("◆ ", Style::default().fg(Color::Rgb(255, 200, 50)).bold()),
            Span::styled("zap", Style::default().fg(Color::Rgb(255, 200, 50)).bold()),
        ]),
    }
}

pub fn text_to_lines(text: &str, width: u16) -> Vec<Line<'static>> {
    let wrap_width = (width as usize).saturating_sub(4).max(20);
    let md_lines = syntax::parse_markdown(text);
    if !md_lines.is_empty() {
        let mut result = Vec::new();
        for line in md_lines {
            result.extend(wrap_markdown_line(line, wrap_width, "  "));
        }
        return result;
    }
    word_wrap_plain(text, wrap_width)
}

/// Span-aware word wrap preserving each span's style across wrapped lines.
fn wrap_markdown_line(line: Line<'static>, max_w: usize, indent: &str) -> Vec<Line<'static>> {
    let indent_len = indent.chars().count();
    let available = max_w.saturating_sub(indent_len).max(1);

    let total: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
    if total <= available {
        let mut spans = vec![Span::raw(indent.to_string())];
        spans.extend(line.spans);
        return vec![Line::from(spans)];
    }

    let mut tokens: Vec<(String, Style, bool)> = Vec::new();
    for span in line.spans {
        let started_space = span.content.starts_with(|c: char| c.is_whitespace());
        for (i, word) in span.content.split_whitespace().enumerate() {
            let space_before = i > 0 || (i == 0 && started_space && !tokens.is_empty());
            tokens.push((word.to_string(), span.style, space_before));
        }
    }

    let mut result_lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_len: usize = 0;

    for (text, style, space_before) in tokens {
        let sep = if space_before && current_len > 0 { 1usize } else { 0 };
        let tok_len = text.chars().count();

        if current_len > 0 && current_len + sep + tok_len > available {
            let mut ls = vec![Span::raw(indent.to_string())];
            ls.append(&mut current_spans);
            result_lines.push(Line::from(ls));
            current_spans.push(Span::styled(text, style));
            current_len = tok_len;
        } else {
            let t = if sep > 0 { format!(" {}", text) } else { text };
            current_len += t.chars().count();
            current_spans.push(Span::styled(t, style));
        }
    }

    if !current_spans.is_empty() {
        let mut ls = vec![Span::raw(indent.to_string())];
        ls.extend(current_spans);
        result_lines.push(Line::from(ls));
    }

    if result_lines.is_empty() {
        result_lines.push(Line::from(indent.to_string()));
    }
    result_lines
}

fn word_wrap_plain(text: &str, wrap_width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        if raw_line.is_empty() {
            lines.push(Line::from("  ".to_string()));
            continue;
        }
        let mut remaining = raw_line;
        while !remaining.is_empty() {
            let char_count = remaining.chars().count();
            let take_chars = if char_count <= wrap_width {
                char_count
            } else {
                let slice: String = remaining.chars().take(wrap_width).collect();
                match slice.rfind(' ') {
                    Some(p) if p > 0 => slice[..p].chars().count(),
                    _ => wrap_width,
                }
            };
            let chunk: String = remaining.chars().take(take_chars).collect();
            let byte_len: usize = chunk.len();
            lines.push(Line::from(Span::styled(
                format!("  {}", chunk),
                Style::default().fg(Color::Rgb(210, 205, 230)),
            )));
            remaining = remaining[byte_len..].trim_start_matches(' ');
        }
    }
    lines
}

pub fn code_block_lines(lang: &str, code_lines: &[String]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let lang_label = if lang.is_empty() { String::new() } else { format!(" {} ", lang) };
    let border_c = Color::Rgb(80, 75, 100);
    let linenum_c = Color::Rgb(110, 105, 135);
    lines.push(Line::from(vec![
        Span::styled(format!("  +--{}", lang_label), Style::default().fg(border_c)),
    ]));

    if !lang.is_empty() && !code_lines.is_empty() {
        let code = code_lines.join("\n");
        let highlighted = syntax::highlight_code(lang, &code);

        let num_width = code_lines.len().to_string().len().max(2);
        for (i, hl_line) in highlighted.iter().enumerate() {
            let line_num = format!("{:>w$}", i + 1, w = num_width);
            let mut spans = vec![
                Span::styled("  |  ".to_string(), Style::default().fg(border_c)),
                Span::styled(line_num, Style::default().fg(linenum_c)),
                Span::styled("  ".to_string(), Style::default()),
            ];
            spans.extend(hl_line.spans.clone());
            lines.push(Line::from(spans));
        }
    } else {
        let num_width = code_lines.len().to_string().len().max(2);
        for (i, code_line) in code_lines.iter().enumerate() {
            let line_num = format!("{:>w$}", i + 1, w = num_width);
            lines.push(Line::from(vec![
                Span::styled("  |  ".to_string(), Style::default().fg(border_c)),
                Span::styled(line_num, Style::default().fg(linenum_c)),
                Span::styled("  ".to_string(), Style::default()),
                Span::styled(code_line.clone(), Style::default().fg(Color::Rgb(215, 210, 235))),
            ]));
        }
    }

    lines.push(Line::from(vec![
        Span::styled("  +--".to_string(), Style::default().fg(border_c)),
    ]));
    lines
}

fn truncate_spans(spans: Vec<Span<'static>>, max_chars: usize) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut remaining = max_chars;
    for span in spans {
        if remaining == 0 { break; }
        let len = span.content.chars().count();
        if len <= remaining {
            remaining -= len;
            result.push(span);
        } else {
            let cut: String = span.content.chars().take(remaining.saturating_sub(1)).collect::<String>() + "\u{2026}";
            result.push(Span::styled(cut, span.style));
            remaining = 0;
        }
    }
    result
}

/// Expand tabs to 4 spaces and truncate to `max_chars`. Used by diff and tool output rendering.
pub(super) fn expand_tabs_and_truncate(s: &str, max_chars: usize) -> String {
    let expanded: String = s.chars().flat_map(|c| {
        if c == '\t' { itertools_like_repeat(' ', 4) } else { itertools_like_repeat(c, 1) }
    }).collect();
    if expanded.chars().count() <= max_chars {
        expanded
    } else {
        let mut t: String = expanded.chars().take(max_chars.saturating_sub(1)).collect();
        t.push('\u{2026}');
        t
    }
}

fn itertools_like_repeat(c: char, n: usize) -> impl Iterator<Item = char> {
    std::iter::repeat_n(c, n)
}

pub fn tool_call_lines(tc: &UiToolCall, expanded: bool, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let max_w = (width as usize).saturating_sub(6).max(20);
    let border = Color::Rgb(55, 50, 75);

    let (icon, icon_color) = if let Some(ref done) = tc.result {
        if done.success { ("✓", Color::Rgb(100, 210, 120)) }
        else            { ("✗", Color::Rgb(220,  80,  80)) }
    } else {
        ("⏺", Color::Rgb(100, 180, 255))
    };

    let label_short: String = if tc.label.chars().count() > 42 {
        format!("{}…", tc.label.chars().take(41).collect::<String>())
    } else {
        tc.label.clone()
    };

    let elapsed = tc.result.as_ref()
        .map(|d| format!("  {}ms", d.elapsed_ms))
        .unwrap_or_default();
    let elapsed_color = tc.result.as_ref().map(|d|
        if d.success { Color::Rgb(100, 175, 100) } else { Color::Rgb(210, 80, 80) }
    ).unwrap_or(Color::Rgb(100, 95, 125));

    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(icon, Style::default().fg(icon_color).bold()),
        Span::styled(format!(" {}", tc.name), Style::default().fg(Color::Rgb(175, 170, 205)).bold()),
        Span::styled(format!("  {}", label_short), Style::default().fg(Color::Rgb(145, 140, 170))),
        Span::styled(elapsed, Style::default().fg(elapsed_color)),
    ]));

    if let Some(ref done) = tc.result {
        let all_lines: Vec<&str> = done.preview.lines().collect();
        let hint_color = Color::Rgb(100, 95, 125);

        if expanded {
            for raw in &all_lines {
                let display = expand_tabs_and_truncate(raw, max_w);
                let line_style = if raw.starts_with("+++") || raw.starts_with("---") {
                    Style::default().fg(Color::Rgb(120, 115, 145))
                } else if raw.starts_with('+') {
                    Style::default().fg(Color::Rgb(100, 210, 120))
                } else if raw.starts_with('-') {
                    Style::default().fg(Color::Rgb(220,  80,  80))
                } else if raw.starts_with("@@") {
                    Style::default().fg(Color::Rgb(100, 180, 255))
                } else {
                    Style::default().fg(Color::Rgb(205, 200, 225))
                };
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::default().fg(border)),
                    Span::styled(display, line_style),
                ]));
            }
            if !all_lines.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::default().fg(border)),
                    Span::styled(
                        "  ↑ Ctrl+O collapses all  ·  Ctrl+O again to re-expand".to_string(),
                        Style::default().fg(hint_color).add_modifier(Modifier::ITALIC),
                    ),
                ]));
            }
        } else if !all_lines.is_empty() {
            let summary = all_lines[0].trim().to_string();
            let hint = if all_lines.len() > 1 {
                format!("  {}  ·  Ctrl+O to expand", summary)
            } else {
                format!("  {}", summary)
            };
            lines.push(Line::from(vec![
                Span::styled("    ", Style::default().fg(border)),
                Span::styled(hint, Style::default().fg(hint_color).add_modifier(Modifier::ITALIC)),
            ]));
        }
    }

    lines
}

pub fn thinking_streaming_lines(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let dim = Color::Rgb(100, 95, 125);
    let thinking_style = Style::default().fg(dim).add_modifier(Modifier::ITALIC);
    lines.push(Line::from(vec![
        Span::styled("  \u{1f9e0} ".to_string(), Style::default().fg(dim)),
        Span::styled("Thinking\u{2026}", thinking_style),
    ]));
    for line in text.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev() {
        let display: String = if line.chars().count() > 80 {
            format!("{}\u{2026}", line.chars().take(79).collect::<String>())
        } else {
            line.to_string()
        };
        lines.push(Line::from(vec![
            Span::styled("     ".to_string(), Style::default().fg(dim)),
            Span::styled(display, thinking_style),
        ]));
    }
    lines
}

pub fn thinking_collapsed_line(char_count: usize) -> Vec<Line<'static>> {
    vec![Line::from(vec![
        Span::styled(
            format!("  \u{1f9e0} Thinking ({} chars)  ", char_count),
            Style::default().fg(Color::Rgb(100, 95, 125)).add_modifier(Modifier::ITALIC),
        ),
    ])]
}
