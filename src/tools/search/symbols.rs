//! Regex-free symbol extraction for code_map filesystem walk.

pub(super) fn ext_label(ext: &str) -> &'static str {
    match ext {
        "html" | "htm" => "HTML file",
        "css" | "scss" | "sass" => "CSS/stylesheet",
        "md" | "mdx" => "Markdown",
        "json" => "JSON",
        "toml" => "TOML config",
        "yaml" | "yml" => "YAML config",
        "vue" => "Vue component",
        "svelte" => "Svelte component",
        "php" => "PHP",
        "kt" => "Kotlin",
        "cs" => "C#",
        _ => "source file",
    }
}

pub(super) fn extract_symbols(content: &str, ext: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let line_no = i + 1;

        let sym = match ext {
            "rs"                        => extract_rust_symbol(trimmed),
            "py"                        => extract_python_symbol(trimmed),
            "ts" | "tsx" | "js" | "jsx" => extract_ts_symbol(trimmed),
            "go"                        => extract_go_symbol(trimmed),
            "java" | "kt" | "cs"        => extract_java_symbol(trimmed),
            "html" | "htm"              => extract_html_symbol(trimmed),
            "css" | "scss" | "sass"     => extract_css_symbol(trimmed),
            "md" | "mdx"               => extract_md_symbol(trimmed),
            _                           => None,
        };

        if let Some(label) = sym {
            symbols.push(format!("  {} (line {})", label, line_no));
        }
    }
    symbols
}

fn extract_html_symbol(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    for tag in &["<h1", "<h2", "<h3", "<section", "<article", "<form",
                 "<nav", "<main", "<header", "<footer", "<script", "<style"] {
        if lower.starts_with(tag) {
            let id = if let Some(s) = line.find("id=\"") {
                let rest = &line[s + 4..];
                rest.split('"').next().map(|v| format!(" #{v}"))
            } else {
                None
            };
            let tag_name = &tag[1..];
            return Some(format!("<{}{}>", tag_name, id.unwrap_or_default()));
        }
    }
    None
}

fn extract_css_symbol(line: &str) -> Option<String> {
    let t = line.trim_end_matches('{').trim();
    if t.is_empty() || !line.trim_end().ends_with('{') { return None; }
    if t.contains(':') && !t.starts_with('@') { return None; }
    Some(t.to_string())
}

fn extract_md_symbol(line: &str) -> Option<String> {
    if line.starts_with('#') {
        let text = line.trim_start_matches('#').trim();
        if !text.is_empty() {
            let level = line.chars().take_while(|&c| c == '#').count();
            return Some(format!("h{level}: {text}"));
        }
    }
    None
}

fn extract_rust_symbol(line: &str) -> Option<String> {
    for prefix in &["pub fn ", "fn ", "pub async fn ", "async fn ",
                    "pub struct ", "struct ", "pub enum ", "enum ",
                    "pub trait ", "trait ", "pub type ", "type ",
                    "pub const ", "const ", "pub static ", "static ",
                    "impl "] {
        if let Some(after) = line.strip_prefix(prefix) {
            let rest = after.split(|c: char| !c.is_alphanumeric() && c != '_').next()?;
            if !rest.is_empty() {
                return Some(format!("{} {}", prefix.trim(), rest));
            }
        }
    }
    None
}

fn extract_python_symbol(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("async def ").or_else(|| line.strip_prefix("def ")) {
        let name = rest.split('(').next()?.trim();
        return Some(format!("def {}", name));
    }
    if let Some(after) = line.strip_prefix("class ") {
        let name = after.split(['(', ':']).next()?.trim();
        return Some(format!("class {}", name));
    }
    None
}

fn extract_ts_symbol(line: &str) -> Option<String> {
    for prefix in &["export function ", "function ", "export class ", "class ",
                    "export interface ", "interface ", "export type ", "type ",
                    "export const ", "const ", "export let ", "let ", "export enum ", "enum "] {
        if let Some(after) = line.strip_prefix(prefix) {
            let rest: &str = after.split(['(', '<', ' ', '=']).next()?;
            if !rest.is_empty() {
                return Some(format!("{}{}", prefix.trim_start_matches("export "), rest));
            }
        }
    }
    None
}

fn extract_go_symbol(line: &str) -> Option<String> {
    if let Some(after) = line.strip_prefix("func ") {
        let rest: &str = after.split(['(', ' ']).next()?;
        return Some(format!("func {}", rest));
    }
    if let Some(after) = line.strip_prefix("type ") {
        let name: &str = after.split_whitespace().next()?;
        return Some(format!("type {}", name));
    }
    None
}

fn extract_java_symbol(line: &str) -> Option<String> {
    for kw in &["class ", "interface ", "enum ", "record "] {
        if line.contains(kw) {
            let after = line.split(kw).nth(1)?;
            let name: &str = after.split([' ', '{', '(', '<']).next()?;
            if !name.is_empty() {
                return Some(format!("{} {}", kw.trim(), name));
            }
        }
    }
    if (line.contains("public ") || line.contains("private ") || line.contains("protected "))
        && line.contains('(')
    {
        let before_paren = line.split('(').next()?;
        let name: &str = before_paren.split_whitespace().last()?;
        if !name.is_empty() && name != "class" && name != "interface" {
            return Some(format!("method {}", name));
        }
    }
    None
}
