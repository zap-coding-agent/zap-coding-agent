use super::app::UiBlock;

/// Split a raw text string into alternating Text and Code UiBlocks.
pub fn parse_text_into_blocks(text: &str, blocks: &mut Vec<UiBlock>) {
    let mut current_text = String::new();
    let mut in_fence = false;
    let mut fence_lang = String::new();
    let mut fence_lines: Vec<String> = Vec::new();

    for line in text.lines() {
        if !in_fence {
            if line.trim_start().starts_with("```") {
                if !current_text.is_empty() {
                    blocks.push(UiBlock::Text(std::mem::take(&mut current_text)));
                }
                in_fence = true;
                fence_lang = line.trim().trim_start_matches('`').to_string();
                fence_lines.clear();
            } else {
                if !current_text.is_empty() {
                    current_text.push('\n');
                }
                current_text.push_str(line);
            }
        } else if line.trim() == "```" || line.trim() == "~~~" {
            blocks.push(UiBlock::Code {
                lang: fence_lang.clone(),
                lines: fence_lines.clone(),
            });
            in_fence = false;
            fence_lang.clear();
            fence_lines.clear();
        } else {
            fence_lines.push(line.to_string());
        }
    }

    if in_fence && !fence_lines.is_empty() {
        blocks.push(UiBlock::Code { lang: fence_lang, lines: fence_lines });
    } else if !current_text.is_empty() {
        blocks.push(UiBlock::Text(current_text));
    }
}
