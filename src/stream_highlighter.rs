/// Streaming code-fence detector.
///
/// Feed text chunks from the LLM stream into `push(chunk)`. The highlighter
/// buffers code blocks (``` ... ```) and prints them with dim line numbers once
/// the closing fence arrives. All other text is printed immediately.
///
/// Call `flush()` after the stream ends to drain any partial content.
use colored::Colorize;

pub struct StreamHighlighter {
    /// Partial line being built (no newline yet).
    line_buf: String,
    /// True once we're inside a code fence block.
    in_fence: bool,
    /// Language tag from the opening fence, e.g. "rust".
    fence_lang: String,
    /// Complete lines accumulated inside the current fence block.
    fence_lines: Vec<String>,
    /// Whether anything has been printed yet (used for before_output callback).
    pub printed_anything: bool,
    /// When true, suppress all print! calls (used in TUI mode to avoid
    /// writing to stdout behind the TUI).
    pub suppress_print: bool,
}

impl Default for StreamHighlighter {
    fn default() -> Self { Self::new() }
}

impl StreamHighlighter {
    pub fn new() -> Self {
        Self {
            line_buf: String::new(),
            in_fence: false,
            fence_lang: String::new(),
            fence_lines: Vec::new(),
            printed_anything: false,
            suppress_print: false,
        }
    }

    /// Process one text chunk from the stream.
    pub fn push(&mut self, chunk: &str) {
        // Forward to TUI channel if active (no-op when not in TUI mode)
        crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::LlmChunk(chunk.to_string()));

        for ch in chunk.chars() {
            if ch == '\n' {
                let line = std::mem::take(&mut self.line_buf);
                self.process_complete_line(line);
            } else {
                if !self.in_fence && !self.suppress_print {
                    // In normal mode, print partial line immediately so text streams live.
                    print!("{}", ch);
                    self.printed_anything = true;
                }
                self.line_buf.push(ch);
            }
        }
    }

    /// Process a fully-received line (without the trailing newline).
    fn process_complete_line(&mut self, line: String) {
        if !self.in_fence {
            if line.trim_start().starts_with("```") {
                // Opening fence — switch to fence mode.
                self.in_fence = true;
                self.fence_lang = line.trim().trim_start_matches('`').to_string();
                self.fence_lines.clear();
                if !self.suppress_print {
                    // Erase the partial line we may have already printed (the ```) by
                    // overwriting it. Since we printed it char-by-char in push(), we need
                    // to clear the line and reprint it styled.
                    // Use ANSI carriage-return + erase-line to clean up the partial.
                    print!("\r\x1b[2K{}\n", line.dimmed());
                    self.printed_anything = true;
                }
            } else {
                // Normal line — print it now (partial was already printed char-by-char).
                if !self.suppress_print {
                    println!();
                    self.printed_anything = true;
                }
            }
        } else {
            // Inside a fence.
            if line.trim() == "```" || line.trim() == "~~~" {
                // Closing fence — render the accumulated code block.
                if !self.suppress_print {
                    self.render_fence();
                    // Print closing fence dimly.
                    println!("{}", "```".dimmed());
                    self.printed_anything = true;
                }
                self.in_fence = false;
                self.fence_lang.clear();
                self.fence_lines.clear();
            } else {
                self.fence_lines.push(line);
            }
        }
    }

    fn render_fence(&self) {
        let lang_label = if self.fence_lang.is_empty() {
            String::new()
        } else {
            format!(" {}", self.fence_lang.bright_cyan())
        };
        if !self.fence_lang.is_empty() {
            println!("{}{}", "─── code".dimmed(), lang_label);
        }
        let width = self.fence_lines.len().to_string().len().max(2);
        for (i, code_line) in self.fence_lines.iter().enumerate() {
            println!(
                "{}  {}",
                format!("{:>w$}", i + 1, w = width).dimmed(),
                code_line.bright_white()
            );
        }
    }

    /// Drain any buffered content after the stream ends.
    pub fn flush(&mut self) {
        if !self.line_buf.is_empty() {
            let remaining = std::mem::take(&mut self.line_buf);
            if self.in_fence {
                self.fence_lines.push(remaining);
            } else {
                // Partial line already printed char-by-char; just end the line.
                if !self.suppress_print {
                    println!();
                    self.printed_anything = true;
                }
            }
        }
        if self.in_fence {
            // Unclosed fence at end of stream — print what we have.
            if !self.suppress_print {
                self.render_fence();
            }
            self.in_fence = false;
        }
    }
}
