/// App state for the TUI.
use super::channel::TuiEvent;
pub(super) use super::text_parse::parse_text_into_blocks;

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
    /// Red warning banner (secret redaction notices, etc.).
    Warning(String),
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

// ── Diff viewer ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct DiffFile {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub diff_lines: Vec<String>,
}

#[derive(Clone, PartialEq)]
pub enum DiffPanel { Files, Diff }

pub struct DiffViewerState {
    pub files: Vec<DiffFile>,
    pub selected: usize,
    pub diff_scroll: usize,
    pub panel: DiffPanel,
    pub title: String,
}

// ── Quota entry ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct QuotaEntry {
    pub five_hour_pct: Option<f32>,
    pub seven_day_pct: Option<f32>,
    pub resets_at: Option<String>,
}

// ── Goal mode ─────────────────────────────────────────────────────────────────

pub struct GoalState {
    pub condition: String,
    pub max_turns: usize,
    pub turns_done: usize,
    pub started_at: std::time::Instant,
}

// ── /init wizard ──────────────────────────────────────────────────────────────

pub enum InitWizardStep {
    Language,
    IndexConfirm,
    UnderstandConfirm,
}

pub struct InitWizardState {
    pub step: InitWizardStep,
    pub detected_language: String,
    pub language_input: String,
    pub language_cursor: usize,
    /// Set when the user answers the Index step — carried forward to UnderstandConfirm.
    pub do_index: bool,
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

// ── Context viewer ────────────────────────────────────────────────────────────

pub use crate::tui::context_viewer::{
    ContextTurnEntry, ContextViewerState, DetailBlock, TurnDetail,
};

// ── File picker ───────────────────────────────────────────────────────────────

/// Popup that lets the user fuzzy-search the project tree and insert a
/// `@path` reference into the input.
pub struct FilePickerState {
    /// Filter text typed inside the popup.
    pub query: String,
    /// Cursor position inside `query` (char index).
    pub query_cursor: usize,
    /// All file paths (relative to cwd) collected when the picker opened.
    pub all_files: Vec<String>,
    /// Selected index into the *filtered* list.
    pub selected: usize,
}

impl FilePickerState {
    /// Returns file paths that match the current query (case-insensitive substring).
    pub fn filtered(&self) -> Vec<&str> {
        let q = self.query.to_lowercase();
        self.all_files
            .iter()
            .filter(|f| q.is_empty() || f.to_lowercase().contains(&q))
            .map(|s| s.as_str())
            .collect()
    }
}

// ── Command output popup ──────────────────────────────────────────────────────

/// A centered popup that displays textual output from inline slash commands
/// (e.g. /help, /config, /cost, /skill list). Dismissed with Esc.
pub struct CommandPopup {
    /// Title shown in the border, e.g. "help" or "skill list".
    pub title: String,
    /// Full text content (may be multi-line).
    pub text: String,
    /// Scroll offset for long output.
    pub scroll: usize,
}

/// Permission prompt overlay — rendered as a TUI-native popup at the bottom.
pub struct PermissionPopup {
    /// Tool entries to display: (id, name, context).
    pub pending: Vec<(String, String, String)>,
    /// Send the decision back through this channel.
    pub response_tx: Option<tokio::sync::oneshot::Sender<super::channel::PermissionDecision>>,
}

// ── Provider picker ────────────────────────────────────────────────────────────

/// A provider entry shown in the TUI-native /provider picker.
#[derive(Debug, Clone)]
pub struct ProviderEntry {
    pub slug: String,
    pub name: String,
    pub hint: String,
    pub kind: ProviderKind,
    pub models: Vec<String>,
    pub base_url: Option<String>,
    pub needs_key: bool,
    pub coming_soon: bool,
    /// Custom auth header (e.g. "x-goog-api-key" for Gemini API keys).
    /// If None, defaults to "Authorization" (Bearer token).
    pub auth_header: Option<&'static str>,
    /// Whether credentials were auto-detected (shown as "✓ ready" badge).
    pub ready: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProviderKind { Anthropic, OpenAi }

pub struct ProviderPickerState {
    pub entries: Vec<ProviderEntry>,
    pub selected: usize,
    /// True when shown automatically on first launch (no provider configured yet).
    pub is_onboarding: bool,
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
    /// Exact scroll offset from last draw_messages call; used by scroll_up to sync from the true position.
    pub rendered_scroll: std::cell::Cell<usize>,

    // Session state
    pub state: AppState,
    pub spinner_frame: usize,
    pub word_tick: usize,
    pub turn_tick: usize,

    // Header info
    pub model: String,
    pub context_pct: u8,
    pub turn: usize,
    pub total_cost_usd: f64,
    pub tokens_input: u32,
    pub tokens_output: u32,
    pub tokens_cache_read: u32,

    /// Per-provider subscription usage windows (5h/7d). Keyed by provider name
    /// (e.g. "claude", "codex"). Entries are upserted on QuotaUpdate so both
    /// can be shown simultaneously in the sidebar.
    pub quotas: std::collections::HashMap<String, QuotaEntry>,

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

    /// Provider picker overlay (None when closed).
    pub provider_picker: Option<ProviderPickerState>,

    /// /init wizard overlay (None when closed).
    pub init_wizard: Option<InitWizardState>,

    /// When true, show the mode picker after the init wizard is dismissed.
    /// Set at startup for new projects so mode picker comes after setup.
    pub show_mode_picker_after_init: bool,

    /// True after first Ctrl+Q press; second press confirms quit.
    pub quit_confirm: bool,

    /// True while mouse reporting is toggled ON (Ctrl+T) for scroll-wheel
    /// support. Starts false — native click-drag text selection works
    /// out of the box, matching Claude Code / opencode / Codex.
    pub mouse_captured: bool,

    /// Skill names available in this session, used for dynamic picker completions.
    pub skill_names: Vec<String>,

    /// Active autonomous goal (set by `/goal <condition>`).
    pub goal_state: Option<GoalState>,

    /// Diff viewer overlay (opened by /diff or Ctrl+G).
    pub diff_viewer: Option<DiffViewerState>,

    /// Command output popup (opened by inline commands like /help, /config, etc.).
    pub command_popup: Option<CommandPopup>,

    /// Permission prompt popup — rendered as a TUI-native overlay at the bottom.
    /// When Some, Y/N/A/Esc keys are routed to it instead of normal input handling.
    pub permission_popup: Option<PermissionPopup>,

    /// Count of file-writing tool calls in the current turn (write_file/edit_file/batch_edit).
    pub files_changed_this_turn: usize,
    /// Skill(s) active this turn — displayed in sidebar, cleared at turn end.
    pub active_skill: Option<String>,
    /// Log of the last 8 skills used — (turn_number, label), newest first.
    pub skill_history: Vec<(usize, String)>,

    /// Mid-turn btw input — true while the Ctrl+B input box is open.
    pub btw_mode: bool,
    /// Draft text inside the btw input box.
    pub btw_draft: String,
    /// Cursor position in btw_draft.
    pub btw_cursor: usize,

    /// Gemini auth prompt overlay — shown when user picks Gemini without credentials,
    /// or when already authenticated (re-auth to fix scope issues).
    pub gemini_auth_prompt: bool,
    /// True when the auth prompt is shown for re-authentication (credentials exist but may be wrong).
    pub gemini_reauth: bool,

    /// Pending provider switch waiting for the user to enter an API key.
    /// Set instead of immediately switching when a provider needs a key but has none saved.
    pub api_key_input: Option<PendingProviderSwitch>,

    /// Context viewer overlay (None when closed).
    pub context_viewer: Option<ContextViewerState>,

    /// File picker popup (None when closed). Opened by typing `@` in the input.
    pub file_picker: Option<FilePickerState>,

    /// Previously sent user prompts (newest last), for Up/Down history navigation.
    pub prompt_history: Vec<String>,
    /// Index into prompt_history when navigating with Up/Down (None = not navigating).
    pub history_idx: Option<usize>,

    /// Pending message held for topic-shift confirmation.
    /// When Some, user must confirm before the message is sent.
    pub topic_shift_confirm: Option<String>,

    /// Flash counter for the topic-shift banner; decrements each tick while > 0.
    /// Used to animate the banner background on first appearance.
    pub topic_shift_flash: u8,

    /// Pending model-switch approval: (input_text, task_type_label, target_model).
    /// When Some, user must confirm (Enter), skip (n), or cancel (Esc) before the
    /// turn is submitted.
    pub model_switch_confirm: Option<(String, String, String)>,

    /// Message queued while a turn is in progress.
    /// Typed and submitted (Enter) during a busy turn; auto-fired when the turn ends.
    pub queued_input: Option<String>,
    /// Active scheduled jobs. Each holds a Tokio JoinHandle; abort to cancel.
    pub scheduled_jobs: Vec<crate::session::scheduler::ScheduledJob>,
    /// Goals queued by the scheduler while a turn was in progress.
    /// Drained one-at-a-time after each turn completes.
    pub scheduled_queue: std::collections::VecDeque<(String, String)>,
    /// Active `/bg` background agents this TUI process has spawned. Each holds
    /// a Tokio JoinHandle; abort to cancel (`/agents kill`). Scoped to this
    /// process's lifetime — the underlying transcript still persists to SQLite.
    pub background_agents: Vec<crate::session::background_agent::BackgroundAgent>,
    /// Monotonic counter for background-agent IDs ("1", "2", ...).
    pub next_bg_id: u32,
}

/// Holds all state needed to complete a provider switch once the user types their API key.
pub struct PendingProviderSwitch {
    pub slug: String,
    pub name: String,
    pub models: Vec<String>,
    pub kind_str: &'static str,
    pub provider: crate::config::Provider,
    pub base_url: Option<String>,
    pub auth_header: Option<String>,
    /// Characters typed so far (not yet confirmed).
    pub input: String,
    /// Whether a key is already saved for this provider (Enter with empty input keeps it).
    pub has_existing_key: bool,
    /// Step: entering the endpoint URL (Custom provider only, comes before the key step).
    pub picking_base_url: bool,
    /// Step: false = entering API key, true = picking model.
    pub picking_model: bool,
    /// Step: typing a free-text model name (chosen "Other…" or endpoint had no model list).
    pub typing_model: bool,
    /// Selected model index (used in model-picking step).
    pub model_sel: usize,
    /// Resolved API key (carried from key step to model step).
    pub resolved_key: Option<String>,
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
            rendered_scroll: std::cell::Cell::new(0),
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
            quotas: std::collections::HashMap::new(),
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
            provider_picker: None,
            init_wizard: None,
            show_mode_picker_after_init: false,
            quit_confirm: false,
            mouse_captured: false,
            skill_names: Vec::new(),
            goal_state: None,
            diff_viewer: None,
            command_popup: None,
            permission_popup: None,
            files_changed_this_turn: 0,
            active_skill: None,
            skill_history: Vec::new(),
            btw_mode: false,
            btw_draft: String::new(),
            btw_cursor: 0,
            gemini_auth_prompt: false,
            gemini_reauth: false,
            api_key_input: None,
            context_viewer: None,
            file_picker: None,
            prompt_history: Vec::new(),
            history_idx: None,
            topic_shift_confirm: None,
            topic_shift_flash: 0,
            model_switch_confirm: None,
            queued_input: None,
            scheduled_jobs:  Vec::new(),
            scheduled_queue: std::collections::VecDeque::new(),
            background_agents: Vec::new(),
            next_bg_id: 0,
        }
    }

    pub fn tick_spinner(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % 10;
        self.word_tick = self.word_tick.wrapping_add(1);
        self.turn_tick = self.turn_tick.wrapping_add(1);
    }

    pub fn apply_event(&mut self, ev: TuiEvent) {
        match ev {
            TuiEvent::LlmChunk(text) => {
                if matches!(self.state, AppState::Idle) {
                    self.turn_tick = 0;
                }
                self.state = AppState::Thinking;
                self.auto_scroll = true;
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
            TuiEvent::ToolStart { id, name, label, .. } => {
                self.state = AppState::ToolRunning { name: name.clone(), label: label.clone() };
                self.streaming_blocks.push(StreamingBlock::Tool(UiToolCall {
                    id,
                    name,
                    label,
                    result: None,
                }));
            }
            TuiEvent::ToolDone { id, elapsed_ms, success, preview } => {
                // Dead-end exploration (no matches found, no symbols, etc.) starts
                // collapsed instead of auto-expanding; Ctrl+O still reveals it.
                let should_expand = !(success && crate::session::preview_found_nothing(&preview));
                // Find the matching pending tool call and fill in its result.
                for sb in self.streaming_blocks.iter_mut().rev() {
                    if let StreamingBlock::Tool(ref mut tc) = sb {
                        if tc.id == id {
                            if success && matches!(tc.name.as_str(), "write_file" | "edit_file" | "batch_edit") {
                                self.files_changed_this_turn += 1;
                            }
                            tc.result = Some(ToolDone { elapsed_ms, success, preview });
                            if should_expand {
                                self.expanded_tools.insert(id.clone());
                            }
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
            TuiEvent::QuotaUpdate { provider, five_hour_pct, seven_day_pct, resets_at } => {
                self.quotas.insert(provider, QuotaEntry { five_hour_pct, seven_day_pct, resets_at });
            }
            TuiEvent::ContextUpdate { pct, turn } => {
                self.context_pct = pct;
                self.turn = turn;
            }
            TuiEvent::ActiveSkill(label) => {
                // turn + 1 because self.turn counts *completed* turns; this fires at turn start
                let turn_no = self.turn + 1;
                // Deduplicate: skip if same label was used in the immediately prior turn
                if self.skill_history.first().map(|e| e.1.as_str()) != Some(&label) {
                    self.skill_history.insert(0, (turn_no, label.clone()));
                    self.skill_history.truncate(8);
                }
                self.active_skill = Some(label);
            }
            TuiEvent::BtwCarryover(msgs) => {
                // Turn ended before these btw messages could be injected mid-turn.
                // Queue them as the next user input so they get a proper response.
                let combined = msgs.join("\n");
                self.messages.push(UiMessage {
                    role: MsgRole::User,
                    blocks: vec![UiBlock::Text(format!("↳ btw (follow-up): {combined}"))],
                });
                self.pending_input = Some(combined);
                self.auto_scroll = true;
            }
            TuiEvent::Notice(text) => {
                self.messages.push(UiMessage {
                    role: MsgRole::Assistant,
                    blocks: vec![UiBlock::Text(text)],
                });
                self.auto_scroll = true;
            }
            TuiEvent::Warning(text) => {
                self.messages.push(UiMessage {
                    role: MsgRole::Assistant,
                    blocks: vec![UiBlock::Warning(text)],
                });
                self.auto_scroll = true;
            }
            TuiEvent::ScheduledFire { name, goal } => {
                let fired_at = chrono::Local::now();
                let mut remove_after_fire = false;
                for job in &mut self.scheduled_jobs {
                    if job.name == name {
                        job.fire_count += 1;
                        job.last_run_at = Some(fired_at);
                        remove_after_fire = !job.spec.repeats();
                        break;
                    }
                }
                if matches!(self.state, AppState::Idle) && self.pending_input.is_none() {
                    // Session is free — submit immediately as a user turn.
                    self.messages.push(UiMessage {
                        role:   MsgRole::User,
                        blocks: vec![UiBlock::Text(format!("⏰ [scheduled: {name}] {goal}"))],
                    });
                    self.pending_input = Some(goal);
                    self.auto_scroll   = true;
                } else {
                    // Busy — queue for after the current turn finishes.
                    self.scheduled_queue.push_back((name.clone(), goal));
                }
                if remove_after_fire {
                    if let Some(pos) = self.scheduled_jobs.iter().position(|j| j.name == name) {
                        let job = self.scheduled_jobs.remove(pos);
                        job.handle.abort();
                    }
                }
                super::schedule_handler::persist_jobs(self);
            }
            TuiEvent::BackgroundAgentDone { id, goal, model, elapsed_secs, outcome } => {
                let failed = matches!(&outcome, crate::tui::channel::BgOutcome::Failed(_));
                if let Some(agent) = self.background_agents.iter_mut().find(|a| a.id == id) {
                    agent.status = outcome.into();
                }
                let elapsed = {
                    let s = crate::session::scheduler::format_interval_secs(elapsed_secs);
                    if s.is_empty() { "0s".to_string() } else { s }
                };
                let mark = if failed { "✗" } else { "✓" };
                let verb = if failed { "failed" } else { "finished" };
                self.messages.push(UiMessage {
                    role: MsgRole::Assistant,
                    blocks: vec![UiBlock::Text(format!(
                        "{mark} Background agent {id} {verb}: \"{goal}\" ({model}, {elapsed})\n   /agents view {id} for details"
                    ))],
                });
                self.auto_scroll = true;
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
        // app.scroll is stale when auto_scroll=true; sync from the exact rendered position first.
        if self.auto_scroll {
            self.scroll = self.rendered_scroll.get();
        }
        self.scroll = self.scroll.saturating_sub(n);
        self.auto_scroll = false;
    }
}

#[cfg(test)]
mod background_agent_tests {
    use super::*;
    use crate::session::background_agent::{BackgroundAgent, BgStatus};
    use crate::tui::channel::{BgOutcome, TuiEvent};

    fn dummy_agent(id: &str) -> BackgroundAgent {
        BackgroundAgent {
            id: id.to_string(),
            goal: "refactor auth".to_string(),
            model: "codex/gpt-5.5".to_string(),
            status: BgStatus::Running,
            started_at: chrono::Local::now(),
            handle: tokio::spawn(async {}),
        }
    }

    #[tokio::test]
    async fn background_agent_done_updates_status_and_pushes_notice() {
        let mut app = App::new("test-model", "main");
        app.background_agents.push(dummy_agent("1"));

        app.apply_event(TuiEvent::BackgroundAgentDone {
            id: "1".to_string(),
            goal: "refactor auth".to_string(),
            model: "codex/gpt-5.5".to_string(),
            elapsed_secs: 42,
            outcome: BgOutcome::Done {
                summary: "done".to_string(),
                files_changed: vec!["src/auth.rs".to_string()],
                turns: 3,
                tool_calls: 5,
            },
        });

        let agent = app.background_agents.iter().find(|a| a.id == "1").expect("agent still present");
        assert!(matches!(agent.status, BgStatus::Done { turns: 3, tool_calls: 5, .. }));

        let last = app.messages.last().expect("a notice was pushed");
        let UiBlock::Text(text) = &last.blocks[0] else { panic!("expected text block") };
        assert!(text.contains('✓'), "success notice should use a checkmark: {text}");
        assert!(text.contains("refactor auth"));
    }

    #[tokio::test]
    async fn background_agent_failed_pushes_failure_notice() {
        let mut app = App::new("test-model", "main");
        app.background_agents.push(dummy_agent("2"));

        app.apply_event(TuiEvent::BackgroundAgentDone {
            id: "2".to_string(),
            goal: "write tests".to_string(),
            model: "claude-sonnet-5".to_string(),
            elapsed_secs: 5,
            outcome: BgOutcome::Failed("provider timeout".to_string()),
        });

        let agent = app.background_agents.iter().find(|a| a.id == "2").expect("agent still present");
        assert!(matches!(&agent.status, BgStatus::Failed(e) if e == "provider timeout"));

        let last = app.messages.last().expect("a notice was pushed");
        let UiBlock::Text(text) = &last.blocks[0] else { panic!("expected text block") };
        assert!(text.contains('✗'), "failure notice should use a cross mark: {text}");
    }
}

