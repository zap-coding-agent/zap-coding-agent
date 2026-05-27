/// One-line TUI preview for a tool result, shown in the thinking/tool trace.
/// Index-backed tools show a count summary so the developer can see that zap
/// navigated via the code index (not a blind grep or file scan).
pub(super) fn smart_tool_preview(tool_name: &str, output: &str) -> String {
    match tool_name {
        "find_definition" | "find_symbol" => {
            if output.starts_with("no definition") || output.starts_with("no match") {
                return "⚠ not in index".to_string();
            }
            let hits = output.lines()
                .filter(|l| l.starts_with("  ") && l.contains(':'))
                .count();
            if hits > 0 {
                format!("🎯 index: {} result{}", hits, if hits == 1 { "" } else { "s" })
            } else {
                output.lines().next().unwrap_or("").to_string()
            }
        }
        "search_code" => {
            if output.starts_with("no matches") {
                return "🔍 no matches found".to_string();
            }
            let header = output.lines().next().unwrap_or(output);
            if let Some(paren) = header.rfind('(') {
                let inner = header[paren + 1..].trim_end_matches(')');
                let counts = inner
                    .trim_start_matches("native, ")
                    .trim_start_matches("ripgrep, ")
                    .trim_start_matches("grep, ");
                return format!("🔍 {}", counts);
            }
            let hits = output.lines().skip(2)
                .filter(|l| !l.is_empty() && *l != "--")
                .count();
            format!("🔍 ~{} matches", hits)
        }
        "code_map" => {
            if let Some(first) = output.lines().next() {
                if let Some(bracket) = first.find("[AST index, ") {
                    let after = &first[bracket + "[AST index, ".len()..];
                    let count_str = after.split_whitespace().next().unwrap_or("?");
                    return format!("📚 index: {} symbols loaded", count_str);
                }
            }
            let total_syms: usize = output.lines()
                .filter_map(|l| {
                    let end = l.find(" symbol")?;
                    let start = l[..end].rfind('(')?;
                    l[start + 1..end].trim().parse::<usize>().ok()
                })
                .sum();
            let files = output.lines()
                .filter(|l| l.ends_with(':') && !l.starts_with(' ') && !l.starts_with('#'))
                .count();
            if total_syms > 0 {
                format!("📚 index: {} files, {} symbols", files.max(1), total_syms)
            } else {
                output.lines().next().unwrap_or("code map").to_string()
            }
        }
        _ => output.lines()
            .find(|l| !l.trim().is_empty() && !l.starts_with("##"))
            .unwrap_or(output)
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::smart_tool_preview;

    #[test]
    fn find_definition_index_hit() {
        let out = "Definition(s) of 'foo' [AST index]:\n  src/foo.rs:10 fn foo\n  src/bar.rs:5 fn foo";
        assert_eq!(smart_tool_preview("find_definition", out), "🎯 index: 2 results");
    }

    #[test]
    fn find_definition_single_hit() {
        let out = "Definition(s) of 'bar' [AST index]:\n  src/bar.rs:5 fn bar";
        assert_eq!(smart_tool_preview("find_definition", out), "🎯 index: 1 result");
    }

    #[test]
    fn find_definition_miss() {
        assert_eq!(smart_tool_preview("find_definition", "no matches for 'x'"), "⚠ not in index");
        assert_eq!(smart_tool_preview("find_definition", "no definition found for 'x'"), "⚠ not in index");
    }

    #[test]
    fn search_code_no_matches() {
        assert_eq!(smart_tool_preview("search_code", "no matches for 'x' in '.'"), "🔍 no matches found");
    }

    #[test]
    fn search_code_with_count_header() {
        let out = "## Search: 'foo' (ripgrep, 5 matches in 3 files)\n...";
        assert_eq!(smart_tool_preview("search_code", out), "🔍 5 matches in 3 files");
    }

    #[test]
    fn code_map_ast_index() {
        let out = "## Code map: src/ [AST index, 42 symbol(s)]\n  fn foo\n  fn bar";
        assert_eq!(smart_tool_preview("code_map", out), "📚 index: 42 symbols loaded");
    }

    #[test]
    fn unknown_tool_first_line() {
        let out = "## Some header\nactual content here";
        assert_eq!(smart_tool_preview("read_file", out), "actual content here");
    }
}
