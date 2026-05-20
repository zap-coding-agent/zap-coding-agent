/// Ratatui rendering for the TUI.
use ratatui::{
    prelude::*,
    widgets::*,
};

use super::app::{App, AppState, MsgRole, StreamingBlock, UiBlock, UiMessage, UiToolCall};
use std::collections::HashSet;
use ratatui::style::Modifier;
use super::commands::filter_commands;

pub const SPINNER_FRAMES: &[&str] = &[
    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏",
];

/// Words that rotate while the LLM is generating — changes roughly every 1.3s at 16ms tick.
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

pub fn draw(frame: &mut Frame, app: &App) {
    let size = frame.area();

    // Outer: header(7) + body(fill) + status(1)
    let outer = Layout::vertical([
        Constraint::Length(7),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(size);

    draw_header(frame, app, outer[0]);
    draw_status(frame, app, outer[2]);

    // Body: left-chat(fill) + right-sidebar(SIDEBAR_W) — only when wide enough
    let use_sidebar = size.width > SIDEBAR_W + 24;
    if use_sidebar {
        let body = Layout::horizontal([
            Constraint::Min(24),
            Constraint::Length(SIDEBAR_W),
        ])
        .split(outer[1]);

        // messages | input | dir-panel (keeps input off the bottom edge)
        let left = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(2),
            Constraint::Length(6),
        ])
        .split(body[0]);

        draw_messages(frame, app, left[0]);
        draw_picker_overlay(frame, app, left[0]);
        draw_input(frame, app, left[1]);
        draw_dir_panel(frame, app, left[2]);
        draw_sidebar(frame, app, body[1]);
    } else {
        let left = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(2),
            Constraint::Length(6),
        ])
        .split(outer[1]);

        draw_messages(frame, app, left[0]);
        draw_picker_overlay(frame, app, left[0]);
        draw_input(frame, app, left[1]);
        draw_dir_panel(frame, app, left[2]);
    }
    
    // Draw file browser overlay if open
    if app.file_browser.is_some() {
        draw_file_browser(frame, app, size);
    }

    // Draw domain/language picker overlay if open (shown once at session start).
    if app.domain_picker.is_some() {
        draw_domain_picker(frame, app, size);
    }

    // Draw session picker overlay if open
    if app.session_picker.is_some() {
        draw_session_picker(frame, app, size);
    }
}

// ── Header — 7-line rich brand (border + 5 content rows + border) ────────────
//
// ZAP in 5-row pixel letters, 8 chars wide, 2-char gap:
//
//   Z: ████████    A:   ████    P: ███████
//         ██          ██   ██      ██   ██
//       ████          ████████     ███████
//     ██              ██   ██      ██
//   ████████          ██   ██      ██
//
const ZAP_ROW: [&str; 5] = [
    " ████████   ████     ███████ ",  // Z: full top   A: wide peak   P: loop top
    "       ██  ██   ██   ██   ██ ",  // Z: diag step  A: sides open  P: loop side
    "     ████  ████████  ███████  ",  // Z: mid step   A: crossbar    P: loop close
    "   ██      ██   ██   ██       ",  // Z: diag step  A: open legs   P: stem
    " ████████  ██   ██   ██       ",  // Z: full bot   A: open legs   P: stem
];

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(60, 55, 80)));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split header inner area: ZAP art (left fill) | info panel (right, sidebar-aligned)
    let use_split = inner.width > SIDEBAR_W + 30;
    let info_w = SIDEBAR_W.saturating_sub(1); // 1 char for the │ separator

    if use_split {
        let cols = Layout::horizontal([
            Constraint::Min(20),
            Constraint::Length(1),
            Constraint::Length(info_w),
        ])
        .split(inner);

        // Vertical separator aligned with body sidebar
        for row in 0..inner.height {
            frame.render_widget(
                Paragraph::new(Span::styled("│", Style::default().fg(Color::Rgb(60, 55, 80)))),
                Rect { x: cols[1].x, y: inner.y + row, width: 1, height: 1 },
            );
        }

        draw_zap_art(frame, cols[0]);
        draw_header_info(frame, app, cols[2]);
    } else {
        draw_zap_art(frame, inner);
    }
}

fn draw_zap_art(frame: &mut Frame, area: Rect) {
    // Amber gradient: bright gold top → deep orange bottom
    let colors = [
        Color::Rgb(255, 215, 40),
        Color::Rgb(255, 195, 20),
        Color::Rgb(255, 170,  0),
        Color::Rgb(240, 145,  0),
        Color::Rgb(220, 120,  0),
    ];
    let rows: Vec<Line<'static>> = ZAP_ROW
        .iter()
        .zip(colors.iter())
        .map(|(row, &c)| Line::from(Span::styled(row.to_string(), Style::default().fg(c).bold())))
        .collect();
    frame.render_widget(Paragraph::new(rows), area);
}

fn draw_header_info(frame: &mut Frame, app: &App, area: Rect) {
    let ver = env!("CARGO_PKG_VERSION");
    let model_short: String = if app.model.chars().count() > 16 {
        let mut s: String = app.model.chars().take(15).collect();
        s.push('…');
        s
    } else {
        app.model.clone()
    };

    // Build git status string
    let mut git_status = format!(" ◉ {}", app.branch);
    if app.git_dirty {
        git_status.push_str(" *");
    }
    if app.git_ahead > 0 {
        git_status.push_str(&format!(" ↑{}", app.git_ahead));
    }
    if app.git_behind > 0 {
        git_status.push_str(&format!(" ↓{}", app.git_behind));
    }
    
    let git_color = if app.git_dirty { Color::Yellow } else { Color::Green };

    let rows: Vec<Line<'static>> = vec![
        Line::from(vec![
            Span::styled(" ⚡ ", Style::default().fg(Color::Rgb(255, 210, 40)).bold()),
            Span::styled("zap", Style::default().fg(Color::Rgb(255, 210, 40)).bold()),
            Span::styled(format!("  v{}", ver), Style::default().fg(Color::Rgb(80, 75, 100))),
        ]),
        Line::from(Span::styled(format!(" {}", model_short), Style::default().fg(Color::Rgb(140, 135, 165)))),
        Line::from(Span::styled(git_status, Style::default().fg(git_color))),
        Line::from(Span::styled(format!(" {} turns", app.turn), Style::default().fg(Color::Rgb(80, 75, 100)))),
        Line::from(vec![
            Span::styled(format!(" {}%", app.context_pct), Style::default().fg(
                if app.context_pct > 80 { Color::Red }
                else if app.context_pct > 60 { Color::Yellow }
                else { Color::Rgb(80, 75, 100) }
            )),
            Span::styled(" ctx", Style::default().fg(Color::Rgb(80, 75, 100))),
        ]),
    ];
    frame.render_widget(Paragraph::new(rows), area);
}

// ── Messages ──────────────────────────────────────────────────────────────────

fn draw_messages(frame: &mut Frame, app: &App, area: Rect) {
    let width = area.width;
    let viewport_h = area.height as usize;

    let all_lines = render_all_lines(app, width);
    let total = all_lines.len();

    let scroll = if app.auto_scroll {
        total.saturating_sub(viewport_h)
    } else {
        app.scroll.min(total.saturating_sub(viewport_h))
    };

    let para = Paragraph::new(all_lines)
        .scroll((scroll.min(u16::MAX as usize) as u16, 0));
    frame.render_widget(para, area);

    // Scrollbar overlay — only when content overflows the viewport.
    if total > viewport_h {
        let mut sb_state = ScrollbarState::new(total.saturating_sub(viewport_h))
            .position(scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut sb_state,
        );
    }
}

// ── Command picker overlay ─────────────────────────────────────────────────────

fn draw_picker_overlay(frame: &mut Frame, app: &App, area: Rect) {
    if !matches!(app.state, AppState::Idle) || !app.input.starts_with('/') {
        return;
    }

    let items = filter_commands(&app.input, &app.skill_names);
    if items.is_empty() {
        return;
    }

    let visible = items.len().min(PICKER_MAX_ROWS);
    let sel = app.picker_sel.min(items.len().saturating_sub(1));

    // The picker sits at the bottom of the messages area.
    let picker_h = (visible + 1) as u16; // rows + top border
    if area.height < picker_h + 1 {
        return; // not enough room
    }

    let picker_area = Rect {
        x: area.x,
        y: area.y + area.height - picker_h,
        width: area.width,
        height: picker_h,
    };

    // Clear background
    frame.render_widget(Clear, picker_area);

    // Border block
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(" commands ", Style::default().fg(Color::DarkGray)));
    let inner = block.inner(picker_area);
    frame.render_widget(block, picker_area);

    // Items (scroll window around selection)
    let start = if sel >= visible { sel - visible + 1 } else { 0 };
    let rows: Vec<Line<'static>> = items[start..start + visible]
        .iter()
        .enumerate()
        .map(|(i, (cmd, desc))| {
            let idx = start + i;
            let is_sel = idx == sel;

            // Truncate desc to fit
            let cmd_w = 18usize;
            let desc_max = (inner.width as usize).saturating_sub(cmd_w + 2);
            let desc_s: String = desc.chars().take(desc_max).collect();

            let sel_bg = Color::Rgb(60, 55, 80);
            if is_sel {
                Line::from(vec![
                    Span::styled(
                        format!(" {:<w$}", cmd, w = cmd_w),
                        Style::default().fg(Color::Rgb(255, 200, 50)).bg(sel_bg).bold(),
                    ),
                    Span::styled(
                        format!("{}", desc_s),
                        Style::default().fg(Color::Rgb(170, 165, 195)).bg(sel_bg),
                    ),
                    Span::styled(" ".repeat(inner.width as usize), Style::default().bg(sel_bg)),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        format!(" {:<w$}", cmd, w = cmd_w),
                        Style::default().fg(Color::Rgb(100, 180, 255)),
                    ),
                    Span::styled(desc_s.to_string(), Style::default().fg(Color::Rgb(80, 75, 100))),
                ])
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(rows), inner);
}

// ── Sidebar ───────────────────────────────────────────────────────────────────

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let filled = (app.context_pct as usize).min(100) * 10 / 100;
    let ctx_bar: String = (0..10).map(|i| if i < filled { '█' } else { '░' }).collect();

    let cost_str = if app.total_cost_usd == 0.0 {
        "—".to_string()
    } else {
        format!("${:.4}", app.total_cost_usd)
    };

    let model_short: String = if app.model.chars().count() > 17 {
        let mut s: String = app.model.chars().take(14).collect();
        s.push_str("…");
        s
    } else {
        app.model.clone()
    };

    let spin = SPINNER_FRAMES[app.spinner_frame % SPINNER_FRAMES.len()];
    let word_idx = (app.turn.wrapping_mul(31).wrapping_add(app.spinner_frame / 40)) % THINKING_WORDS.len();
    let (state_icon, state_color, state_text): (&str, Color, String) = match &app.state {
        AppState::Idle => ("●", Color::Green, "idle".to_string()),
        AppState::Thinking => (
            spin, Color::Yellow,
            format!("{}…", THINKING_WORDS[word_idx]),
        ),
        AppState::ToolRunning { name, label } => {
            let verb = tool_verb(name);
            let short: String = if label.chars().count() > 12 {
                format!("{}…", label.chars().take(11).collect::<String>())
            } else { label.clone() };
            (spin, Color::Cyan, format!("{}  {}", verb, short))
        }
    };

    let label_c = Color::Rgb(80, 75, 100);
    let value_c = Color::Rgb(170, 165, 195);
    let head_c  = Color::Rgb(100, 95, 125);

    let kv = |k: &str, v: String, vc: Color| {
        Line::from(vec![
            Span::styled(format!(" {:<9}", k), Style::default().fg(label_c)),
            Span::styled(v, Style::default().fg(vc)),
        ])
    };

    let mut rows: Vec<Line<'static>> = Vec::new();
    rows.push(Line::from(Span::styled(" session", Style::default().fg(head_c).bold())));
    rows.push(Line::from(""));
    rows.push(kv("model", model_short, value_c));
    rows.push(kv("branch", app.branch.clone(), Color::Rgb(100, 200, 100)));
    rows.push(kv("turn", app.turn.to_string(), value_c));
    rows.push(kv("cost", cost_str, Color::Rgb(200, 180, 80)));
    rows.push(Line::from(""));
    rows.push(Line::from(Span::styled(" context", Style::default().fg(head_c).bold())));
    let bar_color = if app.context_pct > 80 { Color::Rgb(220, 80, 80) }
        else if app.context_pct > 60 { Color::Rgb(220, 180, 50) }
        else { Color::Rgb(80, 160, 255) };
    rows.push(Line::from(vec![
        Span::styled(format!(" {}", ctx_bar), Style::default().fg(bar_color)),
        Span::styled(format!(" {}%", app.context_pct), Style::default().fg(label_c)),
    ]));
    rows.push(Line::from(""));
    rows.push(Line::from(Span::styled(" status", Style::default().fg(head_c).bold())));
    rows.push(Line::from(vec![
        Span::styled(format!(" {} ", state_icon), Style::default().fg(state_color)),
        Span::styled(state_text, Style::default().fg(value_c)),
    ]));

    let block = Block::default().borders(Borders::LEFT).border_style(Style::default().fg(Color::Rgb(45, 42, 60)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(rows).wrap(Wrap { trim: false }), inner);
}

// ── Directory panel (lives in the dead zone below input) ─────────────────────

fn draw_dir_panel(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 { return; }

    let mut rows: Vec<Line<'static>> = Vec::new();

    // Show full absolute path, wrapping if needed
    let max_width = (area.width as usize).saturating_sub(6);
    let path_lines = wrap_path(&app.cwd, max_width);
    
    // Row 0: current dir (first line)
    rows.push(Line::from(vec![
        Span::styled("  ⌂ ", Style::default().fg(Color::Rgb(100, 95, 125))),
        Span::styled(path_lines[0].clone(), Style::default().fg(Color::Rgb(140, 200, 255)).bold()),
    ]));

    // Additional path lines if wrapped
    for line in path_lines.iter().skip(1) {
        rows.push(Line::from(vec![
            Span::styled("    ", Style::default()),
            Span::styled(line.clone(), Style::default().fg(Color::Rgb(140, 200, 255)).bold()),
        ]));
    }
    
    // Hints row
    rows.push(Line::from(vec![
        Span::styled("     ", Style::default()),
        Span::styled("Ctrl+P pick dir  /cd <path>", Style::default().fg(Color::Rgb(80, 75, 100))),
    ]));

    frame.render_widget(Paragraph::new(rows), area);
}

/// Wrap a path into multiple lines if it exceeds max_width.
/// Tries to break at directory separators for readability.
fn wrap_path(path: &str, max_width: usize) -> Vec<String> {
    if path.chars().count() <= max_width {
        return vec![path.to_string()];
    }
    
    let mut lines = Vec::new();
    let mut current = String::new();
    
    for part in path.split('/') {
        let addition = if current.is_empty() {
            part.to_string()
        } else {
            format!("/{}", part)
        };
        
        if current.chars().count() + addition.chars().count() > max_width && !current.is_empty() {
            lines.push(current);
            current = part.to_string();
        } else {
            current.push_str(&addition);
        }
    }
    
    if !current.is_empty() {
        lines.push(current);
    }
    
    if lines.is_empty() {
        vec![path.to_string()]
    } else {
        lines
    }
}

// ── Input ─────────────────────────────────────────────────────────────────────

fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    frame.render_widget(Block::default().borders(Borders::TOP), chunks[0]);

    let prefix = "  ❯ ";
    let char_count = app.input.chars().count();
    let cursor_pos = app.cursor;

    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(prefix.to_string(), Style::default().fg(Color::Rgb(255, 200, 50)).bold()));

    if cursor_pos >= char_count {
        spans.push(Span::raw(app.input.clone()));
        spans.push(Span::styled(" ".to_string(), Style::default().bg(Color::Rgb(255, 200, 50)).fg(Color::Black)));
    } else {
        let before: String = app.input.chars().take(cursor_pos).collect();
        let at: String     = app.input.chars().nth(cursor_pos).map(|c| c.to_string()).unwrap_or_default();
        let after: String  = app.input.chars().skip(cursor_pos + 1).collect();
        spans.push(Span::raw(before));
        spans.push(Span::styled(at, Style::default().bg(Color::Rgb(255, 200, 50)).fg(Color::Black)));
        spans.push(Span::raw(after));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[1]);
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let spin = SPINNER_FRAMES[app.spinner_frame % SPINNER_FRAMES.len()];
    let word_idx = (app.turn.wrapping_mul(31).wrapping_add(app.spinner_frame / 40)) % THINKING_WORDS.len();
    let (hint, hint_color) = match &app.state {
        AppState::Idle => (String::new(), Color::DarkGray),
        AppState::Thinking => (
            format!("  {} {}…  │", spin, THINKING_WORDS[word_idx]),
            Color::Yellow,
        ),
        AppState::ToolRunning { name, .. } => (
            format!("  {} {}…  │", spin, tool_verb(name)),
            Color::Cyan,
        ),
    };
    let keybinds = "  ↑↓ scroll  Tab complete  Enter submit  Ctrl+O expand  Ctrl+P dir  Ctrl+Q quit";
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(hint, Style::default().fg(hint_color)),
            Span::styled(keybinds, Style::default().fg(Color::DarkGray)),
        ])),
        area,
    );
}

// ── Message rendering helpers ─────────────────────────────────────────────────

/// Build all rendered lines for the messages area.
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
                    // In-flight tool calls are never expanded.
                    lines.extend(tool_call_lines(tc, false));
                }
            }
        }

        match &app.state {
            AppState::Thinking | AppState::ToolRunning { .. } => {
                let spin = SPINNER_FRAMES[app.spinner_frame % SPINNER_FRAMES.len()];
                let word_idx = (app.turn.wrapping_mul(31).wrapping_add(app.spinner_frame / 40)) % THINKING_WORDS.len();
                let (label, color) = match &app.state {
                    AppState::ToolRunning { name, label } => {
                        let verb = tool_verb(name);
                        let short: String = if label.chars().count() > 38 {
                            format!("{}…", label.chars().take(37).collect::<String>())
                        } else { label.clone() };
                        (format!("  {} {}  {}", spin, verb, short), Color::Cyan)
                    }
                    _ => (
                        format!("  {} {}…", spin, THINKING_WORDS[word_idx]),
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
            Span::styled("  ✗ Error: ".to_string(), Style::default().fg(Color::Red).bold()),
            Span::styled(err.clone(), Style::default().fg(Color::Red)),
        ]));
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
            UiBlock::Tool(tc)                          => lines.extend(tool_call_lines(tc, expanded.contains(&tc.id))),
            UiBlock::Diff { path, content }            => lines.extend(diff_block_lines(path, content)),
            UiBlock::Thinking { char_count }           => lines.extend(thinking_collapsed_line(*char_count)),
        }
    }
    lines
}

pub fn diff_block_lines(path: &str, content: &str) -> Vec<Line<'static>> {
    let border = Color::Rgb(55, 50, 75);
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("  ✦ ", Style::default().fg(Color::Rgb(100, 180, 255)).bold()),
        Span::styled(path.to_string(), Style::default().fg(Color::Rgb(180, 175, 210)).bold()),
    ]));

    for raw in content.lines() {
        let (style, marker) = if raw.starts_with("+++") || raw.starts_with("---") {
            (Style::default().fg(Color::Rgb(100, 100, 100)), "  │ ")
        } else if raw.starts_with('+') {
            (Style::default().fg(Color::Rgb(100, 210, 120)), "  │ ")
        } else if raw.starts_with('-') {
            (Style::default().fg(Color::Rgb(220,  80,  80)), "  │ ")
        } else if raw.starts_with("@@") {
            (Style::default().fg(Color::Rgb(100, 180, 255)), "  │ ")
        } else {
            (Style::default().fg(Color::Rgb(110, 105, 135)), "  │ ")
        };
        lines.push(Line::from(vec![
            Span::styled(marker.to_string(), Style::default().fg(border)),
            Span::styled(raw.to_string(), style),
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
    // Try to parse as markdown first
    let md_lines = super::syntax::parse_markdown(text);
    
    // If markdown parsing produced good results, use it
    if !md_lines.is_empty() && md_lines.len() > 1 {
        let mut result = Vec::new();
        for line in md_lines {
            // Add indentation
            let mut spans = vec![Span::raw("  ")];
            spans.extend(line.spans);
            result.push(Line::from(spans));
        }
        return result;
    }
    
    // Fallback to plain text wrapping
    let wrap_width = (width as usize).saturating_sub(4).max(20);
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
                    Some(p) => slice[..p].chars().count() + 1,
                    None    => wrap_width,
                }
            };
            let chunk: String = remaining.chars().take(take_chars).collect();
            let byte_len: usize = chunk.len();
            lines.push(Line::from(Span::styled(
                format!("  {}", chunk),
                Style::default().fg(Color::Rgb(210, 205, 230))
            )));
            remaining = remaining[byte_len..].trim_start();
        }
    }
    lines
}

pub fn code_block_lines(lang: &str, code_lines: &[String]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let lang_label = if lang.is_empty() { String::new() } else { format!(" {} ", lang) };
    lines.push(Line::from(vec![
        Span::styled(format!("  ┌─{}", lang_label), Style::default().fg(Color::DarkGray)),
    ]));
    
    // Use syntax highlighting if language is specified
    if !lang.is_empty() && !code_lines.is_empty() {
        let code = code_lines.join("\n");
        let highlighted = super::syntax::highlight_code(lang, &code);
        
        let num_width = code_lines.len().to_string().len().max(2);
        for (i, hl_line) in highlighted.iter().enumerate() {
            let line_num = format!("{:>w$}", i + 1, w = num_width);
            let mut spans = vec![
                Span::styled("  │  ".to_string(), Style::default().fg(Color::DarkGray)),
                Span::styled(line_num, Style::default().fg(Color::DarkGray)),
                Span::styled("  ".to_string(), Style::default()),
            ];
            spans.extend(hl_line.spans.clone());
            lines.push(Line::from(spans));
        }
    } else {
        // Fallback to plain rendering
        let num_width = code_lines.len().to_string().len().max(2);
        for (i, code_line) in code_lines.iter().enumerate() {
            let line_num = format!("{:>w$}", i + 1, w = num_width);
            lines.push(Line::from(vec![
                Span::styled("  │  ".to_string(), Style::default().fg(Color::DarkGray)),
                Span::styled(line_num, Style::default().fg(Color::DarkGray)),
                Span::styled("  ".to_string(), Style::default()),
                Span::styled(code_line.clone(), Style::default().fg(Color::White)),
            ]));
        }
    }
    
    lines.push(Line::from(vec![
        Span::styled("  └".to_string(), Style::default().fg(Color::DarkGray)),
    ]));
    lines
}

pub fn tool_call_lines(tc: &UiToolCall, expanded: bool) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let border = Color::Rgb(55, 50, 75);

    // ── Header line ───────────────────────────────────────────────────────────
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
        if d.success { Color::Rgb(80, 110, 80) } else { Color::Rgb(150, 60, 60) }
    ).unwrap_or(Color::DarkGray);

    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(icon, Style::default().fg(icon_color).bold()),
        Span::styled(format!(" {}", tc.name), Style::default().fg(Color::Rgb(140, 135, 165)).bold()),
        Span::styled(format!("  {}", label_short), Style::default().fg(Color::Rgb(110, 105, 135))),
        Span::styled(elapsed, Style::default().fg(elapsed_color)),
    ]));

    // ── Preview lines ─────────────────────────────────────────────────────────
    if let Some(ref done) = tc.result {
        let all_lines: Vec<&str> = done.preview.lines().collect();
        let truncate_at = 6usize;
        let shown = if expanded { all_lines.len() } else { all_lines.len().min(truncate_at) };

        for raw in &all_lines[..shown] {
            let (line_style, marker) = if raw.starts_with("+++") || raw.starts_with("---") {
                (Style::default().fg(Color::Rgb(100, 100, 100)), "  │ ")
            } else if raw.starts_with('+') {
                (Style::default().fg(Color::Rgb(100, 210, 120)), "  │ ")
            } else if raw.starts_with('-') {
                (Style::default().fg(Color::Rgb(220,  80,  80)), "  │ ")
            } else if raw.starts_with("@@") {
                (Style::default().fg(Color::Rgb(100, 180, 255)), "  │ ")
            } else {
                (Style::default().fg(Color::Rgb(130, 125, 155)), "  │ ")
            };
            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), Style::default().fg(border)),
                Span::styled(raw.to_string(), line_style),
            ]));
        }

        // Truncation hint / collapse hint
        if !expanded && all_lines.len() > truncate_at {
            let remaining = all_lines.len() - truncate_at;
            lines.push(Line::from(vec![
                Span::styled("  │ ".to_string(), Style::default().fg(border)),
                Span::styled(
                    format!("  +{} lines  Ctrl+O to expand", remaining),
                    Style::default().fg(Color::Rgb(80, 75, 100)).add_modifier(Modifier::ITALIC),
                ),
            ]));
        } else if expanded && all_lines.len() > truncate_at {
            lines.push(Line::from(vec![
                Span::styled("  │ ".to_string(), Style::default().fg(border)),
                Span::styled(
                    "  Ctrl+O to collapse".to_string(),
                    Style::default().fg(Color::Rgb(80, 75, 100)).add_modifier(Modifier::ITALIC),
                ),
            ]));
        }
    }

    lines
}


/// Thinking shown while streaming: up to 4 dimmed lines of reasoning text.
pub fn thinking_streaming_lines(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let thinking_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC);
    lines.push(Line::from(vec![
        Span::styled("  🧠 ".to_string(), Style::default().fg(Color::DarkGray)),
        Span::styled("Thinking…", thinking_style),
    ]));
    for line in text.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev() {
        let display: String = if line.chars().count() > 80 {
            format!("{}…", line.chars().take(79).collect::<String>())
        } else {
            line.to_string()
        };
        lines.push(Line::from(vec![
            Span::styled("  │  ".to_string(), Style::default().fg(Color::DarkGray)),
            Span::styled(display, thinking_style),
        ]));
    }
    lines
}

/// Collapsed thinking header shown in completed message history.
pub fn thinking_collapsed_line(char_count: usize) -> Vec<Line<'static>> {
    vec![Line::from(vec![
        Span::styled(
            format!("  🧠 Thinking ({} chars)  ", char_count),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        ),
    ])]
}

// ── File Browser Overlay ──────────────────────────────────────────────────────

fn draw_file_browser(frame: &mut Frame, app: &App, area: Rect) {
    let browser = match &app.file_browser {
        Some(b) => b,
        None => return,
    };
    
    // Create centered overlay (80% width, 80% height)
    let overlay_w = (area.width as f32 * 0.8) as u16;
    let overlay_h = (area.height as f32 * 0.8) as u16;
    let overlay_x = (area.width - overlay_w) / 2;
    let overlay_y = (area.height - overlay_h) / 2;
    
    let overlay_area = Rect {
        x: overlay_x,
        y: overlay_y,
        width: overlay_w,
        height: overlay_h,
    };
    
    // Clear background
    frame.render_widget(Clear, overlay_area);
    
    // Split into file list (left) and preview (right)
    let chunks = Layout::horizontal([
        Constraint::Percentage(40),
        Constraint::Percentage(60),
    ])
    .split(overlay_area);
    
    draw_file_list(frame, browser, chunks[0]);
    draw_file_preview(frame, browser, chunks[1]);
}

fn draw_file_list(frame: &mut Frame, browser: &super::file_browser::FileBrowser, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(" Files (Ctrl+F to close) ", Style::default().fg(Color::Cyan).bold()));
    
    let inner = block.inner(area);
    frame.render_widget(block, area);
    
    // Render file entries
    let mut lines = Vec::new();
    let filtered = browser.filtered_entries();
    
    for (idx, entry) in filtered.iter() {
        let is_selected = *idx == browser.selected;
        
        // Indentation
        let indent = "  ".repeat(entry.depth);
        
        // Icon
        let icon = if entry.is_dir {
            if entry.is_expanded { "▼ " } else { "▶ " }
        } else {
            "  "
        };
        
        // Git status indicator
        let git_icon = match entry.git_status {
            super::file_browser::GitStatus::Modified => "M",
            super::file_browser::GitStatus::Untracked => "?",
            super::file_browser::GitStatus::Staged => "A",
            super::file_browser::GitStatus::Ignored => "!",
            super::file_browser::GitStatus::Clean => " ",
        };
        
        let git_color = match entry.git_status {
            super::file_browser::GitStatus::Modified => Color::Yellow,
            super::file_browser::GitStatus::Untracked => Color::Red,
            super::file_browser::GitStatus::Staged => Color::Green,
            super::file_browser::GitStatus::Ignored => Color::DarkGray,
            super::file_browser::GitStatus::Clean => Color::Gray,
        };
        
        // Name color
        let name_color = if entry.is_dir {
            Color::Cyan
        } else {
            Color::White
        };
        
        let line = if is_selected {
            Line::from(vec![
                Span::styled(format!("{}{}{} ", indent, icon, git_icon), Style::default().fg(git_color).bg(Color::DarkGray)),
                Span::styled(entry.name.clone(), Style::default().fg(name_color).bg(Color::DarkGray).add_modifier(Modifier::BOLD)),
            ])
        } else {
            Line::from(vec![
                Span::styled(format!("{}{}{} ", indent, icon, git_icon), Style::default().fg(git_color)),
                Span::styled(entry.name.clone(), Style::default().fg(name_color)),
            ])
        };
        
        lines.push(line);
    }
    
    // Add help text at bottom
    if lines.len() < inner.height as usize - 2 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("↑↓ navigate  Enter open  Esc close", Style::default().fg(Color::DarkGray))));
    }
    
    let para = Paragraph::new(lines)
        .scroll((browser.scroll as u16, 0));
    frame.render_widget(para, inner);
}

fn draw_file_preview(frame: &mut Frame, browser: &super::file_browser::FileBrowser, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(" Preview ", Style::default().fg(Color::Cyan).bold()));
    
    let inner = block.inner(area);
    frame.render_widget(block, area);
    
    if let Some(ref content) = browser.preview_content {
        let lines = if let Some(ref lang) = browser.preview_lang {
            // Use syntax highlighting
            super::syntax::highlight_code(lang, content)
        } else {
            // Plain text
            content.lines()
                .map(|line| Line::from(Span::styled(line.to_string(), Style::default().fg(Color::White))))
                .collect()
        };
        
        let para = Paragraph::new(lines)
            .wrap(Wrap { trim: false });
        frame.render_widget(para, inner);
    } else {
        let text = if let Some(entry) = browser.entries.get(browser.selected) {
            if entry.is_dir {
                "[Directory]"
            } else {
                "[No preview available]"
            }
        } else {
            "[No file selected]"
        };
        
        let para = Paragraph::new(text)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(para, inner);
    }
}

// ── Session picker overlay ─────────────────────────────────────────────────────

fn draw_session_picker(frame: &mut Frame, app: &App, area: Rect) {
    let picker = match app.session_picker.as_ref() {
        Some(p) => p,
        None    => return,
    };

    // Centred overlay: 82% wide, up to 22 rows tall.
    let w       = (area.width * 82 / 100).max(40).min(area.width);
    let visible = picker.entries.len().min(18);
    let h       = (visible as u16 + 4).min(area.height); // borders + header + footer hint
    let x       = area.x + (area.width.saturating_sub(w)) / 2;
    let y       = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " sessions  ↑↓ navigate   Enter load   Esc cancel ",
            Style::default().fg(Color::Yellow).bold(),
        ));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    if picker.entries.is_empty() {
        frame.render_widget(
            Paragraph::new("No sessions found.")
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    let sel   = picker.selected.min(picker.entries.len().saturating_sub(1));
    let start = if sel >= visible { sel - visible + 1 } else { 0 };
    let end   = (start + visible).min(picker.entries.len());

    let col_id_w    = 5usize;
    let col_date_w  = 10usize;
    let col_model_w = 18usize;
    let sep_w       = 3usize; // " │ "
    let goal_w      = (inner.width as usize)
        .saturating_sub(col_id_w + sep_w + col_date_w + sep_w + col_model_w + sep_w + 1);

    let rows: Vec<Line<'static>> = picker.entries[start..end]
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let is_sel = (start + i) == sel;

            let goal_disp: String = if entry.goal.chars().count() > goal_w {
                format!("{}…", entry.goal.chars().take(goal_w.saturating_sub(1)).collect::<String>())
            } else {
                format!("{:<width$}", entry.goal, width = goal_w)
            };
            let model_disp: String = if entry.model.chars().count() > col_model_w {
                format!("{}…", entry.model.chars().take(col_model_w - 1).collect::<String>())
            } else {
                format!("{:<width$}", entry.model, width = col_model_w)
            };

            let text = format!(
                " #{:<id$} │ {:<date$} │ {} │ {}",
                entry.id,
                entry.date,
                goal_disp,
                model_disp,
                id   = col_id_w - 1,
                date = col_date_w,
            );

            if is_sel {
                Line::from(Span::styled(text, Style::default().fg(Color::Black).bg(Color::Cyan)))
            } else {
                Line::from(Span::styled(text, Style::default().fg(Color::White)))
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(rows), inner);
}

// ── Domain / language scope picker overlay ─────────────────────────────────────

fn draw_domain_picker(frame: &mut Frame, app: &App, area: Rect) {
    let picker = match app.domain_picker.as_ref() {
        Some(p) => p,
        None    => return,
    };

    let visible = picker.options.len().min(16);
    let w = (area.width * 60 / 100).max(44).min(area.width);
    let h = (visible as u16 + 6).min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let proj = &picker.project_name;
    let title = format!(" {} — existing project: scope to languages / frameworks? ", proj);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(255, 200, 50)))
        .title(Span::styled(
            title,
            Style::default().fg(Color::Rgb(255, 200, 50)).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    let hint_area = Rect { y: inner.y + inner.height.saturating_sub(1), height: 1, ..inner };
    let content_area = Rect { height: inner.height.saturating_sub(1), ..inner };

    frame.render_widget(
        Paragraph::new(Span::styled(
            "  Space toggle   Enter confirm   Esc skip (no restriction)",
            Style::default().fg(Color::Rgb(80, 75, 100)),
        )),
        hint_area,
    );

    if picker.options.is_empty() {
        return;
    }

    let sel   = picker.cursor.min(picker.options.len().saturating_sub(1));
    let start = if sel >= visible { sel - visible + 1 } else { 0 };
    let end   = (start + visible).min(picker.options.len());

    let rows: Vec<Line<'static>> = picker.options[start..end]
        .iter()
        .zip(&picker.checked[start..end])
        .enumerate()
        .map(|(i, (name, &checked))| {
            let idx = start + i;
            let is_sel = idx == sel;
            let checkbox = if checked { "[x] " } else { "[ ] " };
            let text = format!("  {}{}", checkbox, name);
            if is_sel {
                Line::from(Span::styled(text, Style::default().fg(Color::Black).bg(Color::Yellow)))
            } else if checked {
                Line::from(Span::styled(text, Style::default().fg(Color::Cyan)))
            } else {
                Line::from(Span::styled(text, Style::default().fg(Color::White)))
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(rows), content_area);
}
