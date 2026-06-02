/// One content block inside a turn's detail panel.
#[derive(Debug, Clone)]
pub enum DetailBlock {
    UserText    { text: String, tokens: usize },
    ToolCall    { name: String, input_json: String, tokens: usize },
    ToolResult  { tool_name: String, content: String, tokens: usize },
    AssistantText { text: String, tokens: usize },
}

pub struct TurnDetail {
    pub blocks: Vec<DetailBlock>,
}

pub struct ContextTurnEntry {
    /// Index into session.messages where this turn starts.
    pub msg_index: usize,
    /// Number of session.messages entries this turn spans (user + assistant + tools).
    pub msg_count: usize,
    /// First ~60 chars of the user message.
    pub preview: String,
    /// Estimated token cost for this turn.
    pub tokens_est: usize,
    /// True if within the active sliding window (live context, not stubbed).
    pub in_window: bool,
    /// Snapshot of the real turn content for the detail panel.
    pub detail: TurnDetail,
}

pub struct ContextViewerState {
    pub turns: Vec<ContextTurnEntry>,
    pub selected: usize,
    pub total_tokens: usize,
    pub limit_tokens: usize,
    pub context_pct: u8,
    /// Awaiting y/N confirmation before clearing all history.
    pub confirm_clear: bool,
    /// Awaiting Enter/Esc confirmation before dropping the selected turn.
    pub confirm_drop: bool,
    /// True when keyboard focus is in the right (detail) panel.
    pub detail_focus: bool,
    /// Scroll offset inside the detail panel.
    pub detail_scroll: usize,
}
