/// App state for the TUI.
use super::channel::TuiEvent;

// ── Types ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum AppState {
    Idle,
    Thinking,
    ToolRunning { name: String, label: String },
}

#[derive(Debug, Clone)]
pub enum MsgRole {
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub struct ToolDone {
    pub elapsed_ms: u64,
    pub success: bool,
    pub preview: String,
}

#[derive(Debug, Clone)]
pub struct UiToolCall {
    pub id: String,
    pub name: String,
    pub label: String,
    pub result: Option<ToolDone>,
}

/// A single rendered block inside a completed UiMessage.
#[derive(Debug, Clone)]
pub enum UiBlock {
    Text(String),
    Code { lang: String, lines: Vec<String> },
    Tool(UiToolCall),
    Diff { path: String, content: String },
    /// Extended-thinking block: stores full text, shown collapsed in history.
    Thinking { char_count: usize },
}

/// An in-flight block during the current assistant turn.
/// Preserves the interleaved ordering of text runs and tool calls exactly
/// as they arrive from the LLM — text, then tool, then more text, etc.
#[derive(Debug, Clone)]
pub enum StreamingBlock {
    /// Accumulated text chunk (may span multiple LlmChunk events).
    Text(String),
    Tool(UiToolCall),
    /// Accumulated extended-thinking text (Anthropic only).
    Thinking(String),
}

#[derive(Debug, Clone)]
pub struct UiMessage {
    pub role: MsgRole,
    pub blocks: Vec<UiBlock>,
}

// ── Goal mode ─────────────────────────────────────────────────────────────────

pub struct GoalState {
    pub condition: String,
    pub max_turns: usize,
    pub turns_done: usize,
    pub started_at: std::time::Instant,
}

// ── Session mode picker ────────────────────────────────────────────────────────

pub struct ModePickerState {
    /// 0 = Vibe, 1 = Task
    pub cursor: usize,
}

// ── Domain / language scope picker ────────────────────────────────────────────

pub struct DomainPickerState {
    /// All available domain skill names.
    pub options: Vec<String>,
    /// Whether each option is currently checked.
    pub checked: Vec<bool>,
    /// Currently highlighted row.
    pub cursor: usize,
    /// Project directory name shown in the picker title.
    pub project_name: String,
}

impl DomainPickerState {
    pub fn new(options: Vec<String>, project_name: String) -> Self {
        let len = options.len();
        Self { options, checked: vec![false; len], cursor: 0, project_name }
    }

    /// Returns the names of all checked options.
    pub fn selected(&self) -> Vec<String> {
        self.options.iter().zip(&self.checked)
            .filter_map(|(name, &on)| if on { Some(name.clone()) } else { None })
            .collect()
    }
}

// ── Session picker ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub id:    i64,
    pub goal:  String,
    pub model: String,
    pub date:  String,
}

pub struct SessionPickerState {
    pub entries:  Vec<SessionEntry>,
    pub selected: usize,
}

// ── App ────────────────────────────────────────────────────────────────────────

pub struct App {
    /// Completed conversation messages.
    pub messages: Vec<UiMessage>,

    /// In-flight blocks for the current assistant turn.
    /// Text and tool calls are interleaved in arrival order.
    pub streaming_blocks: Vec<StreamingBlock>,

    /// Current git branch, cached once on startup and refreshed after slash cmds.
    pub branch: String,

    // Input
    pub input: String,
    pub cursor: usize,
    /// Set when Enter is pressed; consumed by the event loop to start a turn.
    pub pending_input: Option<String>,

    // Scrolling
    pub scroll: usize,
    pub auto_scroll: bool,

    // Session state
    pub state: AppState,
    pub spinner_frame: usize,
    /// Monotonically increasing tick counter — never clamped, used for word rotation.
    pub word_tick: usize,
    /// Tick counter reset to 0 each time a new turn starts — used for elapsed-time display.
    pub turn_tick: usize,

    // Header info
    pub model: String,
    pub context_pct: u8,
    pub turn: usize,
    pub total_cost_usd: f64,
    pub tokens_input: u32,
    pub tokens_output: u32,
    pub tokens_cache_read: u32,

    pub error: Option<String>,

    /// Currently highlighted row in the slash-command picker.
    pub picker_sel: usize,

    /// Current working directory, refreshed after /cd.
    pub cwd: String,
    /// Recent directories visited via /cd (newest first, max 4).
    pub recent_dirs: Vec<String>,
    
    /// Expanded tool IDs (for collapsible tool output).
    pub expanded_tools: std::collections::HashSet<String>,
    
    /// Git repository status.
    pub git_dirty: bool,
    pub git_ahead: usize,
    pub git_behind: usize,
    
    /// File browser state (None when closed).
    pub file_browser: Option<super::file_browser::FileBrowser>,

    /// Vibe/Task mode picker shown at session start before domain picker.
    pub mode_picker: Option<ModePickerState>,

    /// Language/framework domain picker shown once at session start (None after confirmed).
    pub domain_picker: Option<DomainPickerState>,

    /// Session picker overlay (None when closed).
    pub session_picker: Option<SessionPickerState>,

    /// True after first Ctrl+Q press; second press confirms quit.
    pub quit_confirm: bool,

    /// Skill names available in this session, used for dynamic picker completions.
    pub skill_names: Vec<String>,

    /// Active autonomous goal (set by `/goal <condition>`).
    pub goal_state: Option<GoalState>,
}

impl App {
    pub fn new(model: &str, branch: &str) -> Self {
        Self {
            messages: Vec::new(),
            streaming_blocks: Vec::new(),
            branch: branch.to_string(),
            input: String::new(),
            cursor: 0,
            pending_input: None,
            scroll: 0,
            auto_scroll: true,
            state: AppState::Idle,
            spinner_frame: 0,
            word_tick: 0,
            turn_tick: 0,
            model: model.to_string(),
            context_pct: 0,
            turn: 0,
            total_cost_usd: 0.0,
            tokens_input: 0,
            tokens_output: 0,
            tokens_cache_read: 0,
            error: None,
            picker_sel: 0,
            cwd: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "?".to_string()),
            recent_dirs: Vec::new(),
            expanded_tools: std::collections::HashSet::new(),
            git_dirty: false,
            git_ahead: 0,
            git_behind: 0,
            file_browser: None,
            mode_picker: None,
            domain_picker: None,
            session_picker: None,
            quit_confirm: false,
            skill_names: Vec::new(),
            goal_state: None,
        }
    }

    pub fn tick_spinner(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % 10;
        self.word_tick = self.word_tick.wrapping_add(1);
        self.turn_tick = self.turn_tick.wrapping_add(1);
    }

    /// Apply an incoming TUI event to App state.
    pub fn apply_event(&mut self, ev: TuiEvent) {
        match ev {
            TuiEvent::LlmChunk(text) => {
                if matches!(self.state, AppState::Idle) {
                    self.turn_tick = 0;
                }
                self.state = AppState::Thinking;
                // Re-enable auto-scroll so the viewport follows active streaming
                // even if the user scrolled up earlier in the turn.
                self.auto_scroll = true;
                // Append to the last Text block, or create one if needed.
                match self.streaming_blocks.last_mut() {
                    Some(StreamingBlock::Text(ref mut s)) => s.push_str(&text),
                    _ => self.streaming_blocks.push(StreamingBlock::Text(text)),
                }
            }
            TuiEvent::ThinkingChunk(text) => {
                if matches!(self.state, AppState::Idle) {
                    self.turn_tick = 0;
                }
                self.state = AppState::Thinking;
                match self.streaming_blocks.last_mut() {
                    Some(StreamingBlock::Thinking(ref mut s)) => s.push_str(&text),
                    _ => self.streaming_blocks.push(StreamingBlock::Thinking(text)),
                }
            }
            TuiEvent::ToolStart { id, name, label } => {
                self.state = AppState::ToolRunning { name: name.clone(), label: label.clone() };
                self.streaming_blocks.push(StreamingBlock::Tool(UiToolCall {
                    id,
                    name,
                    label,
                    result: None,
                }));
            }
            TuiEvent::ToolDone { id, elapsed_ms, success, preview } => {
                // Find the matching pending tool call and fill in its result.
                for sb in self.streaming_blocks.iter_mut().rev() {
                    if let StreamingBlock::Tool(ref mut tc) = sb {
                        if tc.id == id {
                            tc.result = Some(ToolDone { elapsed_ms, success, preview });
                            break;
                        }
                    }
                }
                self.state = AppState::Thinking;
            }
            TuiEvent::CostUpdate { total_usd, input, output, cache_read } => {
                self.total_cost_usd = total_usd;
                self.tokens_input = input;
                self.tokens_output = output;
                self.tokens_cache_read = cache_read;
            }
            TuiEvent::ContextUpdate { pct, turn } => {
                self.context_pct = pct;
                self.turn = turn;
            }
        }
    }

    /// Finalise the current streaming turn: parse accumulated text for code fences,
    /// preserve interleaving with tool calls, push a completed UiMessage.
    pub fn finalize_turn(&mut self) {
        let blocks_in: Vec<StreamingBlock> = self.streaming_blocks.drain(..).collect();
        if blocks_in.is_empty() { return; }

        let mut blocks_out: Vec<UiBlock> = Vec::new();
        for sb in blocks_in {
            match sb {
                StreamingBlock::Text(text) => {
                    parse_text_into_blocks(&text, &mut blocks_out);
                }
                StreamingBlock::Tool(tc) => {
                    blocks_out.push(UiBlock::Tool(tc));
                }
                StreamingBlock::Thinking(text) => {
                    blocks_out.push(UiBlock::Thinking { char_count: text.chars().count() });
                }
            }
        }

        if !blocks_out.is_empty() {
            self.messages.push(UiMessage {
                role: MsgRole::Assistant,
                blocks: blocks_out,
            });
        }
    }

    pub fn total_lines(&self, width: u16) -> usize {
        super::render::render_all_lines(self, width).len()
    }

    pub fn scroll_down(&mut self, n: usize, viewport_h: usize, total: usize) {
        let max_scroll = total.saturating_sub(viewport_h);
        self.scroll = (self.scroll + n).min(max_scroll);
        if self.scroll >= max_scroll {
            self.auto_scroll = true;
        }
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll = self.scroll.saturating_sub(n);
        self.auto_scroll = false;
    }
}

// ── Code-fence parser ──────────────────────────────────────────────────────────

/// Split a raw text string into alternating Text and Code UiBlocks.
/// Used by finalize_turn() to post-process accumulated streaming text.
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

    // Flush unclosed content.
    if in_fence && !fence_lines.is_empty() {
        blocks.push(UiBlock::Code { lang: fence_lang, lines: fence_lines });
    } else if !current_text.is_empty() {
        blocks.push(UiBlock::Text(current_text));
    }
}
