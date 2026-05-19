/// Syntax highlighting and markdown rendering for the TUI.
use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Lazy-loaded syntax set and theme.
static SYNTAX_SET: std::sync::OnceLock<SyntaxSet> = std::sync::OnceLock::new();
static THEME_SET: std::sync::OnceLock<ThemeSet> = std::sync::OnceLock::new();

fn get_syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(|| SyntaxSet::load_defaults_newlines())
}

fn get_theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(|| ThemeSet::load_defaults())
}

/// Convert syntect color to ratatui color.
fn syntect_to_ratatui_color(color: syntect::highlighting::Color) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

/// Render code with syntax highlighting.
pub fn highlight_code(lang: &str, code: &str) -> Vec<Line<'static>> {
    let syntax_set = get_syntax_set();
    let theme_set = get_theme_set();
    
    // Use "base16-ocean.dark" theme (good for dark terminals)
    let theme = &theme_set.themes["base16-ocean.dark"];
    
    // Find syntax by language name or extension
    let syntax = syntax_set
        .find_syntax_by_extension(lang)
        .or_else(|| syntax_set.find_syntax_by_name(lang))
        .or_else(|| {
            // Try common aliases
            match lang.to_lowercase().as_str() {
                "js" => syntax_set.find_syntax_by_extension("javascript"),
                "ts" => syntax_set.find_syntax_by_extension("typescript"),
                "py" => syntax_set.find_syntax_by_extension("python"),
                "rs" => syntax_set.find_syntax_by_extension("rust"),
                "sh" | "bash" => syntax_set.find_syntax_by_extension("sh"),
                "yml" => syntax_set.find_syntax_by_extension("yaml"),
                "md" => syntax_set.find_syntax_by_extension("markdown"),
                _ => None,
            }
        })
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    
    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();
    
    for line in LinesWithEndings::from(code) {
        let ranges = highlighter
            .highlight_line(line, syntax_set)
            .unwrap_or_default();
        
        let mut spans = Vec::new();
        for (style, text) in ranges {
            let fg = syntect_to_ratatui_color(style.foreground);
            let mut ratatui_style = Style::default().fg(fg);
            
            if style.font_style.contains(syntect::highlighting::FontStyle::BOLD) {
                ratatui_style = ratatui_style.add_modifier(Modifier::BOLD);
            }
            if style.font_style.contains(syntect::highlighting::FontStyle::ITALIC) {
                ratatui_style = ratatui_style.add_modifier(Modifier::ITALIC);
            }
            if style.font_style.contains(syntect::highlighting::FontStyle::UNDERLINE) {
                ratatui_style = ratatui_style.add_modifier(Modifier::UNDERLINED);
            }
            
            spans.push(Span::styled(text.to_string(), ratatui_style));
        }
        
        lines.push(Line::from(spans));
    }
    
    lines
}

/// Parse markdown text and return styled spans.
/// Supports: **bold**, *italic*, `code`, # headers
pub fn parse_markdown(text: &str) -> Vec<Line<'static>> {
    use pulldown_cmark::{Event, Parser, Tag};
    
    let parser = Parser::new(text);
    let mut lines = Vec::new();
    let mut current_line = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    
    for event in parser {
        match event {
            Event::Start(tag) => {
                let new_style = match tag {
                    Tag::Heading(level, _, _) => {
                        use pulldown_cmark::HeadingLevel;
                        let color = match level {
                            HeadingLevel::H1 => Color::Cyan,
                            HeadingLevel::H2 => Color::Blue,
                            HeadingLevel::H3 => Color::Green,
                            _ => Color::Yellow,
                        };
                        Style::default().fg(color).add_modifier(Modifier::BOLD)
                    }
                    Tag::Strong => style_stack.last().unwrap().add_modifier(Modifier::BOLD),
                    Tag::Emphasis => style_stack.last().unwrap().add_modifier(Modifier::ITALIC),
                    Tag::CodeBlock(_) => Style::default().fg(Color::Green),
                    Tag::List(_) => *style_stack.last().unwrap(),
                    Tag::Item => *style_stack.last().unwrap(),
                    Tag::Link(_, _, _) => Style::default().fg(Color::Blue).add_modifier(Modifier::UNDERLINED),
                    _ => *style_stack.last().unwrap(),
                };
                style_stack.push(new_style);
            }
            Event::End(tag) => {
                if style_stack.len() > 1 {
                    style_stack.pop();
                }
                
                // Add newline after certain blocks
                match tag {
                    Tag::Heading(_, _, _) | Tag::Paragraph | Tag::Item => {
                        if !current_line.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_line)));
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(text) => {
                let style = *style_stack.last().unwrap();
                current_line.push(Span::styled(text.to_string(), style));
            }
            Event::Code(code) => {
                let style = Style::default().fg(Color::Green).bg(Color::DarkGray);
                current_line.push(Span::styled(format!("`{}`", code), style));
            }
            Event::SoftBreak | Event::HardBreak => {
                if !current_line.is_empty() {
                    lines.push(Line::from(std::mem::take(&mut current_line)));
                }
            }
            _ => {}
        }
    }
    
    // Flush remaining line
    if !current_line.is_empty() {
        lines.push(Line::from(current_line));
    }
    
    // If no lines were generated, return the original text
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(text.to_string(), Style::default().fg(Color::White))));
    }
    
    lines
}

/// Render a diff with color coding.
pub fn render_diff(diff_text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    
    for line in diff_text.lines() {
        let (style, text) = if line.starts_with('+') && !line.starts_with("+++") {
            (Style::default().fg(Color::Green), line)
        } else if line.starts_with('-') && !line.starts_with("---") {
            (Style::default().fg(Color::Red), line)
        } else if line.starts_with("@@") {
            (Style::default().fg(Color::Cyan), line)
        } else if line.starts_with("diff") || line.starts_with("index") {
            (Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD), line)
        } else {
            (Style::default().fg(Color::Gray), line)
        };
        
        lines.push(Line::from(Span::styled(text.to_string(), style)));
    }
    
    lines
}
