/// Ratatui rendering split into focused submodules.
mod header;
mod layout;
mod messages;
mod overlays;
mod provider_picker;
mod diff;
mod dialogs;
mod context_viewer;
mod init_wizard;

// Re-export the public surface that callers outside this module depend on.
pub use diff::open_diff_viewer;
pub use messages::{
    render_all_lines, message_to_lines, diff_block_lines, role_line,
    text_to_lines, code_block_lines, tool_call_lines,
    thinking_streaming_lines, thinking_collapsed_line,
};

use ratatui::{prelude::*, widgets::*};
use super::app::App;

pub const SPINNER_FRAMES: &[&str] = &[
    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏",
];

/// Words that rotate while the LLM is generating — changes roughly every 3s at 16ms tick.
const THINKING_WORDS: &[&str] = &[
    // Cognitive core
    "Thinking",        "Analyzing",       "Reasoning",       "Reflecting",
    "Considering",     "Contemplating",   "Pondering",       "Deliberating",
    "Cogitating",      "Musing",          "Speculating",     "Theorizing",
    "Inferring",       "Deducing",        "Synthesizing",    "Integrating",
    "Processing",      "Computing",       "Calculating",     "Estimating",
    // Creative / generative
    "Planning",        "Drafting",        "Designing",       "Architecting",
    "Brainstorming",   "Ideating",        "Conceptualizing", "Envisioning",
    "Formulating",     "Composing",       "Constructing",    "Crafting",
    "Generating",      "Developing",      "Imagining",       "Prototyping",
    "Sketching",       "Outlining",       "Scaffolding",     "Blueprinting",
    // Analytical
    "Evaluating",      "Reviewing",       "Inspecting",      "Auditing",
    "Examining",       "Scrutinizing",    "Investigating",   "Researching",
    "Studying",        "Parsing",         "Decoding",        "Interpreting",
    "Comprehending",   "Absorbing",       "Assessing",       "Comparing",
    "Contrasting",     "Distinguishing",  "Benchmarking",    "Profiling",
    // Problem-solving
    "Solving",         "Debugging",       "Troubleshooting", "Diagnosing",
    "Probing",         "Testing",         "Verifying",       "Validating",
    "Correcting",      "Patching",        "Refining",        "Improving",
    "Optimizing",      "Enhancing",       "Tuning",          "Adjusting",
    "Calibrating",     "Fixing",          "Resolving",       "Untangling",
    // Code-specific
    "Refactoring",     "Rewriting",       "Abstracting",     "Mapping",
    "Tracing",         "Traversing",      "Navigating",      "Compiling",
    "Linting",         "Modularizing",    "Encapsulating",   "Decoupling",
    "Wiring",          "Bootstrapping",   "Instrumenting",   "Annotating",
    // Organizing
    "Documenting",     "Organizing",      "Structuring",     "Arranging",
    "Sequencing",      "Categorizing",    "Classifying",     "Sorting",
    "Filtering",       "Locating",        "Identifying",     "Recognizing",
    "Correlating",     "Connecting",      "Associating",     "Contextualizing",
    "Framing",         "Scoping",         "Prioritizing",    "Grouping",
    // Gathering / retrieval
    "Clustering",      "Gathering",       "Collecting",      "Aggregating",
    "Summarizing",     "Distilling",      "Extracting",      "Deriving",
    "Projecting",      "Approximating",   "Retrieving",      "Querying",
    "Fetching",        "Loading",         "Indexing",        "Searching",
    // Exploratory
    "Exploring",       "Discovering",     "Uncovering",      "Revealing",
    "Illuminating",    "Elucidating",     "Deciphering",     "Unraveling",
    "Dissecting",      "Deconstructing",  "Reconstructing",  "Reframing",
    "Rethinking",      "Reimagining",     "Revisiting",      "Deep-diving",
    // Verification / hardening
    "Cross-checking",  "Fact-checking",   "Sanity-checking", "Stress-testing",
    "Hardening",       "Streamlining",    "Consolidating",   "Normalizing",
    "Standardizing",   "Harmonizing",     "Aligning",        "Balancing",
    "Weighing",        "Confirming",      "Polishing",       "Finalizing",
    // Clarifying / finishing
    "Simplifying",     "Clarifying",      "Disambiguating",  "Reconciling",
    "Merging",         "Combining",       "Explaining",      "Modeling",
    "Simulating",      "Forecasting",     "Measuring",       "Quantifying",
    "Experimenting",   "Hypothesizing",   "Homing in",       "Focusing",
    // Meta / flow
    "Concentrating",   "Drilling down",   "Backtracking",    "Unpacking",
    "Decomposing",     "Iterating",       "Converging",      "Coalescing",
    "Scanning",        "Vetting",         "Extrapolating",   "Interpolating",
];

fn tool_verb(name: &str) -> &'static str {
    match name {
        "read_file"        => "Reading",
        "write_file"       => "Writing",
        "edit_file"        => "Editing",
        "batch_edit"       => "Editing",
        "undo_edit"        => "Undoing",
        "shell"            => "Running",
        "search_code"      => "Searching",
        "find_definition"  => "Looking up",
        "code_map"         => "Mapping",
        "list_directory"   => "Browsing",
        "web_fetch"        => "Fetching",
        "web_search"       => "Searching web",
        "spawn_agent"      => "Spawning agent",
        "read_memory"      => "Recalling",
        "write_memory"     => "Remembering",
        _                  => "Running",
    }
}

/// Width of the right sidebar (includes the left border character).
pub const SIDEBAR_W: u16 = 22;

/// Max rows the command picker occupies (excluding its own border).
const PICKER_MAX_ROWS: usize = 8;

fn input_height(app: &App, available_width: u16) -> Constraint {
    let prefix_len = 2u16;
    let border_w = 2u16;
    let content_w = available_width.saturating_sub(prefix_len + border_w).max(1);
    let chars = app.input.chars().count().max(1) as u16;
    let lines = chars.div_ceil(content_w);
    let lines = lines.clamp(1, 6);
    Constraint::Length(lines + 2)
}

pub fn draw(frame: &mut Frame, app: &App) {
    let size = frame.area();

    let outer = Layout::vertical([
        Constraint::Length(7),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(size);

    header::draw_header(frame, app, outer[0]);
    layout::draw_status(frame, app, outer[2]);

    let use_sidebar = size.width > SIDEBAR_W + 24;
    if use_sidebar {
        let body = Layout::horizontal([
            Constraint::Min(24),
            Constraint::Length(SIDEBAR_W),
        ])
        .split(outer[1]);

        let input_h = input_height(app, body[0].width);
        let left = Layout::vertical([
            Constraint::Min(1),
            input_h,
            Constraint::Length(3),
        ])
        .split(body[0]);

        layout::draw_messages(frame, app, left[0]);
        layout::draw_picker_overlay(frame, app, left[0]);
        let cursor_pos = layout::draw_input(frame, app, left[1]);
        layout::draw_dir_panel(frame, app, left[2]);
        layout::draw_sidebar(frame, app, body[1]);
        layout::maybe_set_cursor(frame, app, cursor_pos);
    } else {
        // Clear the area where the sidebar would have been to avoid ghost characters.
        let sidebar_ghost = Rect {
            x: outer[1].x + outer[1].width.saturating_sub(SIDEBAR_W),
            y: outer[1].y,
            width: SIDEBAR_W,
            height: outer[1].height,
        };
        frame.render_widget(Clear, sidebar_ghost);

        let left = Layout::vertical([
            Constraint::Min(1),
            input_height(app, outer[1].width),
            Constraint::Length(6),
        ])
        .split(outer[1]);

        layout::draw_messages(frame, app, left[0]);
        layout::draw_picker_overlay(frame, app, left[0]);
        let cursor_pos = layout::draw_input(frame, app, left[1]);
        layout::draw_dir_panel(frame, app, left[2]);
        layout::maybe_set_cursor(frame, app, cursor_pos);
    }

    if app.file_browser.is_some() {
        overlays::draw_file_browser(frame, app, size);
    }

    if app.mode_picker.is_some() {
        overlays::draw_mode_picker(frame, app, size);
        return;
    }

    if app.domain_picker.is_some() {
        overlays::draw_domain_picker(frame, app, size);
    }

    if app.session_picker.is_some() {
        overlays::draw_session_picker(frame, app, size);
    }

    if app.provider_picker.is_some() {
        provider_picker::draw_provider_picker(frame, app, size);
    }

    if app.init_wizard.is_some() {
        init_wizard::draw_init_wizard(frame, app, size);
    }

    if app.diff_viewer.is_some() {
        diff::draw_diff_viewer(frame, app, size);
        return;
    }

    if app.command_popup.is_some() {
        dialogs::draw_command_popup(frame, app, size);
    }

    if app.permission_popup.is_some() {
        dialogs::draw_permission_popup(frame, app, size);
    }

    if app.btw_mode {
        dialogs::draw_btw_input(frame, app, size);
    }

    if app.gemini_auth_prompt {
        overlays::draw_gemini_auth_prompt(frame, app, size);
    }

    if app.api_key_input.is_some() {
        overlays::draw_api_key_input(frame, app, size);
    }

    if app.context_viewer.is_some() {
        context_viewer::draw_context_viewer(frame, app, size);
    }
}
