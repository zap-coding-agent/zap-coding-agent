/// Ratatui rendering for the TUI.
use ratatui::{
    prelude::*,
    widgets::*,
};

use super::app::{App, AppState, CommandPopup, DiffFile, DiffPanel, DiffViewerState, InitWizardState, InitWizardStep, MsgRole, StreamingBlock, UiBlock, UiMessage, UiToolCall};
use std::collections::HashSet;
use ratatui::style::Modifier;
use super::commands::filter_commands;

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

/// Compute the height (in terminal rows) needed for the input box.
/// Returns a Constraint that gives the input area enough rows for wrapped text.
fn input_height(app: &App, available_width: u16) -> Constraint {
    let prefix_len = 2u16; // "❯ "
    let border_w = 2u16;  // left + right border
    let content_w = available_width.saturating_sub(prefix_len + border_w).max(1);
    let chars = app.input.chars().count().max(1) as u16;
    let lines = (chars + content_w - 1) / content_w; // ceil division
    let lines = lines.min(6).max(1);
    Constraint::Length(lines as u16 + 2) // +2 for top/bottom border
}

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
        let input_h = input_height(app, body[0].width);
        let left = Layout::vertical([
            Constraint::Min(1),
            input_h,
            Constraint::Length(3),
        ])
        .split(body[0]);

        draw_messages(frame, app, left[0]);
        draw_picker_overlay(frame, app, left[0]);
        draw_input(frame, app, left[1]);
        draw_dir_panel(frame, app, left[2]);
        draw_sidebar(frame, app, body[1]);
    } else {
        // When sidebar is hidden, clear the area where it would have been
        // to prevent ghost characters from a previous wide-frame render.
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

        draw_messages(frame, app, left[0]);
        draw_picker_overlay(frame, app, left[0]);
        draw_input(frame, app, left[1]);
        draw_dir_panel(frame, app, left[2]);
    }
    
    // Draw file browser overlay if open
    if app.file_browser.is_some() {
        draw_file_browser(frame, app, size);
    }

    // Mode picker shown first (before domain picker).
    if app.mode_picker.is_some() {
        draw_mode_picker(frame, app, size);
        return; // nothing else rendered until mode is chosen
    }

    // Draw domain/language picker overlay if open (shown once at session start).
    if app.domain_picker.is_some() {
        draw_domain_picker(frame, app, size);
    }

    // Draw session picker overlay if open
    if app.session_picker.is_some() {
        draw_session_picker(frame, app, size);
    }

    // Draw /init wizard overlay if open (takes priority over other content)
    if app.init_wizard.is_some() {
        draw_init_wizard(frame, app, size);
    }

    // Draw diff viewer overlay if open (takes priority over everything except mode picker)
    if app.diff_viewer.is_some() {
        draw_diff_viewer(frame, app, size);
        return; // nothing else rendered until diff viewer is closed
    }

    // Draw command output popup if open (dismissed with Esc).
    if app.command_popup.is_some() {
        draw_command_popup(frame, app, size);
    }

    // Draw permission prompt overlay at bottom (Y/N/A to respond).
    if app.permission_popup.is_some() {
        draw_permission_popup(frame, app, size);
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
            Span::styled(format!("  v{}", ver), Style::default().fg(Color::Rgb(130, 125, 155))),
        ]),
        Line::from(Span::styled(format!(" {}", model_short), Style::default().fg(Color::Rgb(170, 165, 200)))),
        Line::from(Span::styled(git_status, Style::default().fg(git_color))),
        Line::from(Span::styled(format!(" {} turns", app.turn), Style::default().fg(Color::Rgb(155, 150, 185)))),
        Line::from(vec![
            Span::styled(format!(" {}%", app.context_pct), Style::default().fg(
                if app.context_pct > 80 { Color::Red }
                else if app.context_pct > 60 { Color::Yellow }
                else { Color::Rgb(155, 150, 185) }
            )),
            Span::styled(" ctx", Style::default().fg(Color::Rgb(130, 125, 155))),
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
    let word_idx = (app.turn.wrapping_mul(31).wrapping_add(app.word_tick / 188)) % THINKING_WORDS.len();
    let elapsed_secs = app.turn_tick / 62;
    let (state_icon, state_color, state_text): (&str, Color, String) = match &app.state {
        AppState::Idle => ("●", Color::Green, "idle".to_string()),
        AppState::Thinking => (
            spin, Color::Yellow,
            format!("{}… {}s", THINKING_WORDS[word_idx], elapsed_secs),
        ),
        AppState::ToolRunning { name, label } => {
            let verb = tool_verb(name);
            let short: String = if label.chars().count() > 12 {
                format!("{}…", label.chars().take(11).collect::<String>())
            } else { label.clone() };
            (spin, Color::Cyan, format!("{}  {}", verb, short))
        }
    };

    let label_c = Color::Rgb(130, 125, 155);
    let value_c = Color::Rgb(205, 200, 230);
    let head_c  = Color::Rgb(160, 155, 190);

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
    // Token breakdown — only shown once we have real data
    if app.tokens_input > 0 || app.tokens_output > 0 {
        let fmt_k = |n: u32| -> String {
            if n >= 1000 { format!("{:.1}k", n as f64 / 1000.0) } else { n.to_string() }
        };
        rows.push(Line::from(vec![
            Span::styled(format!(" {:<9}", "in/out"), Style::default().fg(label_c)),
            Span::styled(fmt_k(app.tokens_input),  Style::default().fg(Color::Rgb(140, 200, 255))),
            Span::styled(" / ".to_string(), Style::default().fg(Color::Rgb(80, 75, 100))),
            Span::styled(fmt_k(app.tokens_output), Style::default().fg(Color::Rgb(160, 220, 160))),
        ]));
        if app.tokens_cache_read > 0 {
            rows.push(kv("cached", fmt_k(app.tokens_cache_read), Color::Rgb(160, 140, 220)));
        }
    }
    rows.push(Line::from(""));
    rows.push(Line::from(Span::styled(" context", Style::default().fg(head_c).bold())));
    let bar_color = if app.context_pct > 80 { Color::Rgb(220, 80, 80) }
        else if app.context_pct > 60 { Color::Rgb(220, 180, 50) }
        else { Color::Rgb(80, 160, 255) };
    rows.push(Line::from(vec![
        Span::styled(format!(" {}", ctx_bar), Style::default().fg(bar_color)),
        Span::styled(format!(" {}%", app.context_pct), Style::default().fg(Color::Rgb(155, 150, 185))),
    ]));
    rows.push(Line::from(""));
    rows.push(Line::from(Span::styled(" status", Style::default().fg(head_c).bold())));
    rows.push(Line::from(vec![
        Span::styled(format!(" {} ", state_icon), Style::default().fg(state_color)),
        Span::styled(state_text, Style::default().fg(value_c)),
    ]));

    // Active skill — only shown while a skill is injected this turn
    if let Some(ref skill_label) = app.active_skill {
        let skill_c = Color::Rgb(255, 200, 60);
        let short: String = if skill_label.chars().count() > 15 {
            format!("{}…", skill_label.chars().take(14).collect::<String>())
        } else {
            skill_label.clone()
        };
        rows.push(Line::from(""));
        rows.push(Line::from(Span::styled(" skill", Style::default().fg(skill_c).bold())));
        rows.push(kv("active", short, Color::Rgb(255, 230, 140)));
    }

    // Goal section — only shown when a /goal is active
    if let Some(ref gs) = app.goal_state {
        let goal_c = Color::Rgb(120, 220, 180);
        let short_cond: String = if gs.condition.chars().count() > 15 {
            format!("{}…", gs.condition.chars().take(14).collect::<String>())
        } else {
            gs.condition.clone()
        };
        let elapsed = gs.started_at.elapsed().as_secs();
        rows.push(Line::from(""));
        rows.push(Line::from(Span::styled(" goal", Style::default().fg(goal_c).bold())));
        rows.push(kv("cond", short_cond, Color::Rgb(190, 240, 210)));
        rows.push(kv("turn", format!("{}/{}", gs.turns_done, gs.max_turns), Color::Rgb(220, 210, 100)));
        rows.push(kv("time", format!("{}s", elapsed), Color::Rgb(170, 165, 195)));
    }

    let block = Block::default().borders(Borders::LEFT).border_style(Style::default().fg(Color::Rgb(45, 42, 60)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(rows).wrap(Wrap { trim: false }), inner);
}

// ── Directory panel (lives in the dead zone below input) ─────────────────────

fn draw_dir_panel(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 { return; }
    let max_w = (area.width as usize).saturating_sub(6).max(20);

    // Front-truncate path if too long (show tail so the filename/project is always visible)
    let path_display: String = {
        let chars: Vec<char> = app.cwd.chars().collect();
        if chars.len() <= max_w {
            app.cwd.clone()
        } else {
            let keep = max_w.saturating_sub(1);
            format!("…{}", chars[chars.len() - keep..].iter().collect::<String>())
        }
    };

    // Second row: goal badge when active, otherwise nav hints
    let hint_row: Line<'static> = if let Some(ref gs) = app.goal_state {
        let cond: String = if gs.condition.chars().count() > 22 {
            format!("{}…", gs.condition.chars().take(21).collect::<String>())
        } else {
            gs.condition.clone()
        };
        Line::from(vec![
            Span::styled("     ⊛ ".to_string(), Style::default().fg(Color::Rgb(120, 220, 180)).bold()),
            Span::styled(
                format!("{}/{}: {}", gs.turns_done, gs.max_turns, cond),
                Style::default().fg(Color::Rgb(150, 230, 200)),
            ),
            Span::styled("  /goal stop".to_string(), Style::default().fg(Color::Rgb(80, 75, 100))),
        ])
    } else {
        Line::from(Span::styled(
            "     Ctrl+F files  Ctrl+P dir  /cd <path>".to_string(),
            Style::default().fg(Color::Rgb(60, 58, 80)),
        ))
    };

    let rows: Vec<Line<'static>> = vec![
        Line::from(vec![
            Span::styled("  ⌂ ".to_string(), Style::default().fg(Color::Rgb(100, 95, 125))),
            Span::styled(path_display, Style::default().fg(Color::Rgb(140, 200, 255))),
        ]),
        hint_row,
        Line::from(""),
    ];
    frame.render_widget(Paragraph::new(rows), area);
}

// ── Input ─────────────────────────────────────────────────────────────────────

fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let prefix = "❯ ";
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

    // Compute scroll offset so the cursor is always visible.
    let content_w = area.width.saturating_sub(2) as usize; // borders
    let prefix_chars = prefix.chars().count();
    let cursor_char_in_text = prefix_chars + cursor_pos; // cursor_pos is char-index
    let scroll = if content_w > 0 {
        let cursor_row = cursor_char_in_text / content_w;
        let visible_rows = area.height.saturating_sub(2) as usize; // borders
        if cursor_row >= visible_rows {
            (cursor_row - visible_rows + 1) as u16
        } else {
            0
        }
    } else {
        0
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(80, 80, 80)));

    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let spin = SPINNER_FRAMES[app.spinner_frame % SPINNER_FRAMES.len()];
    let word_idx = (app.turn.wrapping_mul(31).wrapping_add(app.word_tick / 188)) % THINKING_WORDS.len();
    let elapsed_secs = app.turn_tick / 62;
    let (hint, hint_color) = match &app.state {
        AppState::Idle => (String::new(), Color::DarkGray),
        AppState::Thinking => (
            format!("  {} {}… {}s  │", spin, THINKING_WORDS[word_idx], elapsed_secs),
            Color::Yellow,
        ),
        AppState::ToolRunning { name, .. } => (
            format!("  {} {}…  │", spin, tool_verb(name)),
            Color::Cyan,
        ),
    };

    let goal_badge: String = if let Some(ref gs) = app.goal_state {
        format!("  ⊛ goal {}/{}  │", gs.turns_done, gs.max_turns)
    } else {
        String::new()
    };

    let keybinds = "  ↑↓ scroll  Tab  Ctrl+O expand  Ctrl+F files  Ctrl+G diff  Ctrl+P dir  Ctrl+Q quit";
    let mut spans = vec![
        Span::styled(hint, Style::default().fg(hint_color)),
    ];
    if !goal_badge.is_empty() {
        spans.push(Span::styled(goal_badge, Style::default().fg(Color::Rgb(120, 220, 180)).bold()));
    }
    spans.push(Span::styled(keybinds, Style::default().fg(Color::DarkGray)));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
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
                    lines.extend(tool_call_lines(tc, app.expanded_tools.contains(tc.id.as_str()), width));
                }
            }
        }

        match &app.state {
            AppState::Thinking | AppState::ToolRunning { .. } => {
                let spin = SPINNER_FRAMES[app.spinner_frame % SPINNER_FRAMES.len()];
                let word_idx = (app.turn.wrapping_mul(31).wrapping_add(app.word_tick / 188)) % THINKING_WORDS.len();
                let elapsed_secs = app.turn_tick / 62;
                let (label, color) = match &app.state {
                    AppState::ToolRunning { name, label } => {
                        let verb = tool_verb(name);
                        let short: String = if label.chars().count() > 38 {
                            format!("{}…", label.chars().take(37).collect::<String>())
                        } else { label.clone() };
                        (format!("  {} {}  {}", spin, verb, short), Color::Cyan)
                    }
                    _ => (
                        format!("  {} {}… {}s", spin, THINKING_WORDS[word_idx], elapsed_secs),
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
            Span::styled("  \u{2717} Error: ".to_string(), Style::default().fg(Color::Red).bold()),
            Span::styled(err.clone(), Style::default().fg(Color::Red)),
        ]));
    }

    // ── Global safety net: hard-clip every line to width ──────────────────────
    // Prevents any line source (markdown, code blocks, tool output) from
    // overflowing into the sidebar or wrapping as scattered characters.
    let max_w = (width as usize).saturating_sub(2).max(20);
    for line in &mut lines {
        let total: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        if total > max_w {
            line.spans = truncate_spans(std::mem::take(&mut line.spans), max_w);
        }
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
            UiBlock::Tool(tc)                          => lines.extend(tool_call_lines(tc, expanded.contains(&tc.id), width)),
            UiBlock::Diff { path, content }            => lines.extend(diff_block_lines(path, content, width)),
            UiBlock::Thinking { char_count }           => lines.extend(thinking_collapsed_line(*char_count)),
        }
    }
    lines
}

pub fn diff_block_lines(path: &str, content: &str, width: u16) -> Vec<Line<'static>> {
    let border = Color::Rgb(55, 50, 75);
    let max_w = (width as usize).saturating_sub(6).max(20);
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("  \u{2726} ", Style::default().fg(Color::Rgb(100, 180, 255)).bold()),
        Span::styled(path.to_string(), Style::default().fg(Color::Rgb(180, 175, 210)).bold()),
    ]));

    for raw in content.lines() {
        let display = expand_tabs_and_truncate(raw, max_w);
        let style = if raw.starts_with("+++") || raw.starts_with("---") {
            Style::default().fg(Color::Rgb(120, 115, 145))
        } else if raw.starts_with('+') {
            Style::default().fg(Color::Rgb(100, 210, 120))
        } else if raw.starts_with('-') {
            Style::default().fg(Color::Rgb(220,  80,  80))
        } else if raw.starts_with("@@") {
            Style::default().fg(Color::Rgb(100, 180, 255))
        } else {
            Style::default().fg(Color::Rgb(175, 170, 200))
        };
        lines.push(Line::from(vec![
            Span::styled("    ", Style::default().fg(border)),
            Span::styled(display, style),
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
    let wrap_width = (width as usize).saturating_sub(4).max(20);
    let md_lines = super::syntax::parse_markdown(text);
    if !md_lines.is_empty() {
        let mut result = Vec::new();
        for line in md_lines {
            result.extend(wrap_markdown_line(line, wrap_width, "  "));
        }
        return result;
    }
    word_wrap_plain(text, wrap_width)
}

/// Span-aware word wrap: if `line` fits within `max_w - indent`, return it with indent prepended.
/// Otherwise split on word boundaries while preserving each span's style across the new lines.
fn wrap_markdown_line(line: Line<'static>, max_w: usize, indent: &str) -> Vec<Line<'static>> {
    let indent_len = indent.chars().count();
    let available = max_w.saturating_sub(indent_len).max(1);

    let total: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
    if total <= available {
        let mut spans = vec![Span::raw(indent.to_string())];
        spans.extend(line.spans);
        return vec![Line::from(spans)];
    }

    // Tokenise each span into (word, style, needs_space_before) triples.
    let mut tokens: Vec<(String, Style, bool)> = Vec::new();
    for span in line.spans {
        let started_space = span.content.starts_with(|c: char| c.is_whitespace());
        for (i, word) in span.content.split_whitespace().enumerate() {
            let space_before = i > 0 || (i == 0 && started_space && !tokens.is_empty());
            tokens.push((word.to_string(), span.style, space_before));
        }
    }

    let mut result_lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_len: usize = 0;

    for (text, style, space_before) in tokens {
        let sep = if space_before && current_len > 0 { 1usize } else { 0 };
        let tok_len = text.chars().count();

        if current_len > 0 && current_len + sep + tok_len > available {
            // Flush current line and start a new one.
            let mut ls = vec![Span::raw(indent.to_string())];
            ls.extend(current_spans.drain(..));
            result_lines.push(Line::from(ls));
            current_len = 0;
            current_spans.push(Span::styled(text, style));
            current_len = tok_len;
        } else {
            let t = if sep > 0 { format!(" {}", text) } else { text };
            current_len += t.chars().count();
            current_spans.push(Span::styled(t, style));
        }
    }

    if !current_spans.is_empty() {
        let mut ls = vec![Span::raw(indent.to_string())];
        ls.extend(current_spans);
        result_lines.push(Line::from(ls));
    }

    if result_lines.is_empty() {
        result_lines.push(Line::from(indent.to_string()));
    }
    result_lines
}

fn word_wrap_plain(text: &str, wrap_width: usize) -> Vec<Line<'static>> {
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
                    Some(p) if p > 0 => slice[..p].chars().count(),
                    _ => wrap_width,
                }
            };
            let chunk: String = remaining.chars().take(take_chars).collect();
            let byte_len: usize = chunk.len();
            lines.push(Line::from(Span::styled(
                format!("  {}", chunk),
                Style::default().fg(Color::Rgb(210, 205, 230)),
            )));
            remaining = remaining[byte_len..].trim_start_matches(' ');
        }
    }
    lines
}

pub fn code_block_lines(lang: &str, code_lines: &[String]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let lang_label = if lang.is_empty() { String::new() } else { format!(" {} ", lang) };
    let border_c = Color::Rgb(80, 75, 100);
    let linenum_c = Color::Rgb(110, 105, 135);
    lines.push(Line::from(vec![
        Span::styled(format!("  +--{}", lang_label), Style::default().fg(border_c)),
    ]));

    // Use syntax highlighting if language is specified
    if !lang.is_empty() && !code_lines.is_empty() {
        let code = code_lines.join("\n");
        let highlighted = super::syntax::highlight_code(lang, &code);

        let num_width = code_lines.len().to_string().len().max(2);
        for (i, hl_line) in highlighted.iter().enumerate() {
            let line_num = format!("{:>w$}", i + 1, w = num_width);
            let mut spans = vec![
                Span::styled("  |  ".to_string(), Style::default().fg(border_c)),
                Span::styled(line_num, Style::default().fg(linenum_c)),
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
                Span::styled("  |  ".to_string(), Style::default().fg(border_c)),
                Span::styled(line_num, Style::default().fg(linenum_c)),
                Span::styled("  ".to_string(), Style::default()),
                Span::styled(code_line.clone(), Style::default().fg(Color::Rgb(215, 210, 235))),
            ]));
        }
    }

    lines.push(Line::from(vec![
        Span::styled("  +--".to_string(), Style::default().fg(border_c)),
    ]));
    lines
}

/// Truncate a span list to at most `max_chars` visible characters, appending "…" if cut.
fn truncate_spans(spans: Vec<Span<'static>>, max_chars: usize) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut remaining = max_chars;
    for span in spans {
        if remaining == 0 { break; }
        let len = span.content.chars().count();
        if len <= remaining {
            remaining -= len;
            result.push(span);
        } else {
            let cut: String = span.content.chars().take(remaining.saturating_sub(1)).collect::<String>() + "\u{2026}";
            result.push(Span::styled(cut, span.style));
            remaining = 0;
        }
    }
    result
}

/// Expand tab characters to 4 spaces and truncate to `max_chars`.
fn expand_tabs_and_truncate(s: &str, max_chars: usize) -> String {
    let expanded: String = s.chars().flat_map(|c| {
        if c == '\t' { itertools_like_repeat(' ', 4) } else { itertools_like_repeat(c, 1) }
    }).collect();
    if expanded.chars().count() <= max_chars {
        expanded
    } else {
        let mut t: String = expanded.chars().take(max_chars.saturating_sub(1)).collect();
        t.push('\u{2026}'); // …
        t
    }
}

// Helper: repeat a char N times as an iterator-like chain without external deps.
fn itertools_like_repeat(c: char, n: usize) -> std::iter::Take<std::iter::Repeat<char>> {
    std::iter::repeat(c).take(n)
}

pub fn tool_call_lines(tc: &UiToolCall, expanded: bool, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let max_w = (width as usize).saturating_sub(6).max(20);

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
        if d.success { Color::Rgb(100, 175, 100) } else { Color::Rgb(210, 80, 80) }
    ).unwrap_or(Color::Rgb(100, 95, 125));

    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(icon, Style::default().fg(icon_color).bold()),
        Span::styled(format!(" {}", tc.name), Style::default().fg(Color::Rgb(175, 170, 205)).bold()),
        Span::styled(format!("  {}", label_short), Style::default().fg(Color::Rgb(145, 140, 170))),
        Span::styled(elapsed, Style::default().fg(elapsed_color)),
    ]));

    // ── Preview lines ─────────────────────────────────────────────────────────
    // Collapsed (default): just a line-count hint — no inline content to overflow.
    // Expanded (Ctrl+O):   show full content with diff-aware colouring.
    if let Some(ref done) = tc.result {
        let all_lines: Vec<&str> = done.preview.lines().collect();
        let hint_color = Color::Rgb(100, 95, 125);

        if expanded {
            for raw in &all_lines {
                let display = expand_tabs_and_truncate(raw, max_w);
                let line_style = if raw.starts_with("+++") || raw.starts_with("---") {
                    Style::default().fg(Color::Rgb(120, 115, 145))
                } else if raw.starts_with('+') {
                    Style::default().fg(Color::Rgb(100, 210, 120))
                } else if raw.starts_with('-') {
                    Style::default().fg(Color::Rgb(220,  80,  80))
                } else if raw.starts_with("@@") {
                    Style::default().fg(Color::Rgb(100, 180, 255))
                } else {
                    Style::default().fg(Color::Rgb(205, 200, 225))
                };
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::default().fg(border)),
                    Span::styled(display, line_style),
                ]));
            }
            if !all_lines.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::default().fg(border)),
                    Span::styled(
                        "  ↑ Ctrl+O for next tool  (all expanded → collapses all)".to_string(),
                        Style::default().fg(hint_color).add_modifier(Modifier::ITALIC),
                    ),
                ]));
            }
        } else if !all_lines.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("    ", Style::default().fg(border)),
                Span::styled(
                    format!("  {} lines  Ctrl+O to expand", all_lines.len()),
                    Style::default().fg(hint_color).add_modifier(Modifier::ITALIC),
                ),
            ]));
        }
    }

    lines
}


/// Thinking shown while streaming: up to 4 dimmed lines of reasoning text.
pub fn thinking_streaming_lines(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let dim = Color::Rgb(100, 95, 125);
    let thinking_style = Style::default().fg(dim).add_modifier(Modifier::ITALIC);
    lines.push(Line::from(vec![
        Span::styled("  \u{1f9e0} ".to_string(), Style::default().fg(dim)),
        Span::styled("Thinking\u{2026}", thinking_style),
    ]));
    for line in text.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev() {
        let display: String = if line.chars().count() > 80 {
            format!("{}\u{2026}", line.chars().take(79).collect::<String>())
        } else {
            line.to_string()
        };
        lines.push(Line::from(vec![
            Span::styled("     ".to_string(), Style::default().fg(dim)),
            Span::styled(display, thinking_style),
        ]));
    }
    lines
}

/// Collapsed thinking header shown in completed message history.
pub fn thinking_collapsed_line(char_count: usize) -> Vec<Line<'static>> {
    vec![Line::from(vec![
        Span::styled(
            format!("  \u{1f9e0} Thinking ({} chars)  ", char_count),
            Style::default().fg(Color::Rgb(100, 95, 125)).add_modifier(Modifier::ITALIC),
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

// ── Vibe / Task mode picker overlay ───────────────────────────────────────────

fn draw_mode_picker(frame: &mut Frame, app: &App, area: Rect) {
    let picker = match app.mode_picker.as_ref() {
        Some(p) => p,
        None    => return,
    };

    let w = 52u16.min(area.width);
    let h = 10u16.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let border_c = Color::Rgb(255, 200, 50);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_c))
        .title(Span::styled(
            " ⚡ How do you want to work? ",
            Style::default().fg(border_c).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    // Two options + hint
    let opts: &[(&str, &str)] = &[
        ("Vibe",  "start talking — no structure"),
        ("Task",  "plan first, then execute"),
    ];

    let sel_bg = Color::Rgb(60, 55, 80);
    let mut rows: Vec<Line<'static>> = vec![Line::from("")];
    for (i, (name, desc)) in opts.iter().enumerate() {
        let is_sel = i == picker.cursor;
        let marker = if is_sel { " ❯ " } else { "   " };
        let line = if is_sel {
            Line::from(vec![
                Span::styled(marker.to_string(), Style::default().fg(border_c).bg(sel_bg).bold()),
                Span::styled(format!("{:<6}", name), Style::default().fg(border_c).bg(sel_bg).bold()),
                Span::styled(format!("  {}", desc), Style::default().fg(Color::Rgb(170, 165, 195)).bg(sel_bg)),
                Span::styled(" ".repeat(w as usize), Style::default().bg(sel_bg)),
            ])
        } else {
            Line::from(vec![
                Span::styled(marker.to_string(), Style::default().fg(Color::Rgb(80, 75, 100))),
                Span::styled(format!("{:<6}", name), Style::default().fg(Color::Rgb(140, 135, 165))),
                Span::styled(format!("  {}", desc), Style::default().fg(Color::Rgb(80, 75, 100))),
            ])
        };
        rows.push(line);
    }
    rows.push(Line::from(""));
    rows.push(Line::from(Span::styled(
        "  ↑↓ navigate   Enter confirm   Esc = Vibe",
        Style::default().fg(Color::Rgb(60, 55, 80)),
    )));

    frame.render_widget(Paragraph::new(rows), inner);
}

// ── /init wizard overlay ──────────────────────────────────────────────────────

fn draw_init_wizard(frame: &mut Frame, app: &App, area: Rect) {
    let wizard = match app.init_wizard.as_ref() {
        Some(w) => w,
        None    => return,
    };

    let (title, hint, content_h) = match &wizard.step {
        InitWizardStep::Language       => (" ⚡ Set up this project ", "  Edit   Enter next   Esc skip", 14u16),
        InitWizardStep::IndexConfirm   => (" ⚡ Set up this project ", "  y/Enter = yes   n = no   Esc back", 10u16),
        InitWizardStep::UnderstandConfirm => (" ⚡ Set up this project ", "  y/Enter = yes   n = no   Esc back", 10u16),
    };

    let w = 62u16.min(area.width);
    let h = (content_h + 2).min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let border_c  = Color::Rgb(255, 200, 50);
    let accent    = Color::Rgb(100, 210, 255);
    let muted     = Color::Rgb(100, 95, 125);
    let dim       = Color::Rgb(70, 65, 90);
    let body_c    = Color::Rgb(210, 205, 235);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_c))
        .title(Span::styled(title, Style::default().fg(border_c).add_modifier(Modifier::BOLD)));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    let hint_area    = Rect { y: inner.y + inner.height.saturating_sub(1), height: 1, ..inner };
    let content_area = Rect { height: inner.height.saturating_sub(1), ..inner };

    frame.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(dim))),
        hint_area,
    );

    let mut rows: Vec<Line<'static>> = Vec::new();

    match &wizard.step {
        InitWizardStep::Language => {
            // Step indicator
            rows.push(Line::from(vec![
                Span::styled("  Step 1 of 3  ", Style::default().fg(dim)),
                Span::styled("language", Style::default().fg(border_c).bold()),
                Span::styled("  ·  index  ·  understand", Style::default().fg(dim)),
            ]));
            rows.push(Line::from(""));

            // What init does
            rows.push(Line::from(Span::styled(
                "  Init prepares zap to work well in this project:",
                Style::default().fg(body_c),
            )));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("Index code symbols", Style::default().fg(body_c).bold()),
                Span::styled(" — jump to any function/type instantly", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("Write ZAP.md", Style::default().fg(body_c).bold()),
                Span::styled(" — your project's instructions, loaded every session", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("Build understanding", Style::default().fg(body_c).bold()),
                Span::styled(" — zap reads your code and learns its structure", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(""));

            // Language input
            let label  = Span::styled("  Language(s): ", Style::default().fg(Color::Rgb(140, 135, 165)));
            let text   = wizard.language_input.clone();
            let cursor = Span::styled("▋", Style::default().fg(border_c));
            rows.push(Line::from(vec![
                label,
                Span::styled(text, Style::default().fg(Color::White).bold()),
                cursor,
            ]));
            rows.push(Line::from(""));
            let note = if wizard.detected_language.is_empty() {
                "  no build file detected — type language (e.g. rust, python, typescript)".to_string()
            } else {
                format!("  auto-detected from project files: {}", wizard.detected_language)
            };
            rows.push(Line::from(Span::styled(note, Style::default().fg(dim))));
        }

        InitWizardStep::IndexConfirm => {
            rows.push(Line::from(vec![
                Span::styled("  Step 2 of 3  ", Style::default().fg(dim)),
                Span::styled("language  ·  ", Style::default().fg(dim)),
                Span::styled("index", Style::default().fg(border_c).bold()),
                Span::styled("  ·  understand", Style::default().fg(dim)),
            ]));
            rows.push(Line::from(""));
            rows.push(Line::from(Span::styled(
                "  Index code symbols now?  (recommended, ~10–30s)",
                Style::default().fg(body_c).bold(),
            )));
            rows.push(Line::from(""));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("zap parses every file with tree-sitter", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("extracts functions, types, structs, imports", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("stays current: auto-refreshes every 2 min + at session end", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("run /index any time to force a refresh", Style::default().fg(muted)),
            ]));
        }

        InitWizardStep::UnderstandConfirm => {
            rows.push(Line::from(vec![
                Span::styled("  Step 3 of 3  ", Style::default().fg(dim)),
                Span::styled("language  ·  index  ·  ", Style::default().fg(dim)),
                Span::styled("understand", Style::default().fg(border_c).bold()),
            ]));
            rows.push(Line::from(""));
            rows.push(Line::from(Span::styled(
                "  Let zap read your codebase and write ZAP.md?  (~30–60s)",
                Style::default().fg(body_c).bold(),
            )));
            rows.push(Line::from(""));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("zap reads your source files and fills ZAP.md", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("records architecture, build commands, conventions", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("ZAP.md is loaded into every future session automatically", Style::default().fg(muted)),
            ]));
            rows.push(Line::from(vec![
                Span::styled("  ◎ ", Style::default().fg(accent)),
                Span::styled("you can edit ZAP.md any time to update zap's context", Style::default().fg(muted)),
            ]));
        }
    }

    frame.render_widget(Paragraph::new(rows), content_area);
}

// ── Diff Viewer Overlay ─────────────────────────────────────────────────────

fn draw_diff_viewer(frame: &mut Frame, app: &App, area: Rect) {
    let dv = match &app.diff_viewer {
        Some(dv) => dv,
        None => return,
    };

    // Create centered overlay (80% width, 80% height)
    let overlay_w = (area.width as f32 * 0.8) as u16;
    let overlay_h = (area.height as f32 * 0.8) as u16;
    let overlay_x = area.x + (area.width - overlay_w) / 2;
    let overlay_y = area.y + (area.height - overlay_h) / 2;

    let overlay_area = Rect {
        x: overlay_x,
        y: overlay_y,
        width: overlay_w,
        height: overlay_h,
    };

    // Clear background
    frame.render_widget(Clear, overlay_area);

    // Split into file list (left) and diff content (right)
    let chunks = Layout::horizontal([
        Constraint::Percentage(40),
        Constraint::Percentage(60),
    ])
    .split(overlay_area);

    draw_diff_file_list(frame, dv, chunks[0]);
    draw_diff_content(frame, dv, chunks[1]);
}

fn draw_diff_file_list(frame: &mut Frame, dv: &DiffViewerState, area: Rect) {
    let header = format!(" {} ", dv.title);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(header, Style::default().fg(Color::Cyan).bold()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line<'static>> = Vec::new();

    for (idx, file) in dv.files.iter().enumerate() {
        let is_selected = idx == dv.selected && dv.panel == DiffPanel::Files;
        let path_disp = if file.path.chars().count() > inner.width.saturating_sub(12) as usize {
            format!(
                "…{}",
                file.path
                    .chars()
                    .rev()
                    .take(inner.width.saturating_sub(13) as usize)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            )
        } else {
            file.path.clone()
        };
        let stats = format!(" +{}/-{}", file.added, file.removed);

        if is_selected {
            lines.push(Line::from(vec![
                Span::styled(
                    path_disp,
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    stats,
                    Style::default()
                        .fg(Color::Rgb(100, 210, 120))
                        .bg(Color::DarkGray),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(path_disp, Style::default().fg(Color::White)),
                Span::styled(
                    stats,
                    Style::default().fg(Color::Rgb(100, 210, 120)),
                ),
            ]));
        }
    }

    if lines.len() < inner.height as usize - 2 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "↑↓ navigate  Tab switch  Esc close",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let para = Paragraph::new(lines);
    frame.render_widget(para, inner);
}

fn draw_diff_content(frame: &mut Frame, dv: &DiffViewerState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(" Diff ", Style::default().fg(Color::Cyan).bold()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build diff lines from the selected file
    let mut lines: Vec<Line<'static>> = Vec::new();

    if let Some(file) = dv.files.get(dv.selected) {
        for raw in &file.diff_lines {
            let display =
                expand_tabs_and_truncate(raw, inner.width.saturating_sub(4) as usize);
            let style = if raw.starts_with("+++") || raw.starts_with("---") {
                Style::default().fg(Color::Rgb(120, 115, 145))
            } else if raw.starts_with('+') {
                Style::default().fg(Color::Rgb(100, 210, 120))
            } else if raw.starts_with('-') {
                Style::default().fg(Color::Rgb(220, 80, 80))
            } else if raw.starts_with("@@") {
                Style::default().fg(Color::Rgb(100, 180, 255))
            } else {
                Style::default().fg(Color::Rgb(175, 170, 200))
            };
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(display, style),
            ]));
        }
    }

    if lines.len() < inner.height as usize - 2 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "↑↓ navigate  Tab switch  Esc close",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let scroll = if dv.panel == DiffPanel::Diff {
        dv.diff_scroll as u16
    } else {
        0
    };
    let para = Paragraph::new(lines).scroll((scroll, 0));
    frame.render_widget(para, inner);
}

/// Run `git diff` (falling back to `git diff HEAD~1`, then session snapshots)
/// and parse the output into a `DiffViewerState`.
/// Returns `None` only if git fails AND no session snapshots exist.
pub fn open_diff_viewer() -> Option<DiffViewerState> {
    // Try unstaged working-tree changes first.
    let git_result: Option<(String, String)> = (|| {
        let out = std::process::Command::new("git")
            .args(["diff", "--unified=5"])
            .output()
            .ok()?;
        let t = String::from_utf8(out.stdout).ok()?;
        if out.status.success() && !t.trim().is_empty() {
            return Some((t, "working changes".to_string()));
        }
        // Nothing unstaged — try the previous commit.
        let out2 = std::process::Command::new("git")
            .args(["diff", "HEAD~1", "--unified=5"])
            .output()
            .ok()?;
        let t2 = String::from_utf8(out2.stdout).ok()?;
        if out2.status.success() && !t2.trim().is_empty() {
            return Some((t2, "last commit".to_string()));
        }
        None
    })();

    if let Some((text, title)) = git_result {
        return parse_git_diff(text, title);
    }

    // Git unavailable or no changes — fall back to in-session snapshots.
    snapshot_diff_viewer()
}

/// Build a DiffViewerState from in-memory session snapshots (works in non-git dirs).
fn snapshot_diff_viewer() -> Option<DiffViewerState> {
    use similar::TextDiff;
    let diffs = crate::snapshot::snapshot_diffs();
    if diffs.is_empty() {
        return None;
    }
    let mut files: Vec<DiffFile> = Vec::new();
    for (path, before, after) in &diffs {
        let diff = TextDiff::from_lines(before.as_str(), after.as_str());
        let mut diff_lines: Vec<String> = Vec::new();
        let mut added = 0usize;
        let mut removed = 0usize;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        diff_lines.push(format!("diff --session a/{name} b/{name}"));
        diff_lines.push(format!("--- a/{name}"));
        diff_lines.push(format!("+++ b/{name}"));
        for change in diff.iter_all_changes() {
            let tag = match change.tag() {
                similar::ChangeTag::Delete => { removed += 1; "-" },
                similar::ChangeTag::Insert => { added += 1; "+" },
                similar::ChangeTag::Equal  => " ",
            };
            diff_lines.push(format!("{}{}", tag, change.value().trim_end_matches('\n')));
        }
        let short = path.to_string_lossy().to_string();
        files.push(DiffFile { path: short, added, removed, diff_lines });
    }
    if files.is_empty() {
        return None;
    }
    Some(DiffViewerState { files, selected: 0, diff_scroll: 0, panel: DiffPanel::Files, title: "session edits".to_string() })
}

fn parse_git_diff(text: String, title: String) -> Option<DiffViewerState> {
    let mut files: Vec<DiffFile> = Vec::new();
    let mut current_path = String::new();
    let mut current_lines: Vec<String> = Vec::new();
    let mut added = 0usize;
    let mut removed = 0usize;

    for line in text.lines() {
        if let Some(path) = line.strip_prefix("diff --git a/") {
            // Save previous file
            if !current_path.is_empty() {
                files.push(DiffFile {
                    path: current_path,
                    added,
                    removed,
                    diff_lines: current_lines,
                });
            }
            // Extract the path from "diff --git a/... b/..."
            let b_part = path.split_once(" b/").map(|(_, b)| b).unwrap_or(path);
            current_path = b_part.to_string();
            current_lines = Vec::new();
            added = 0;
            removed = 0;
            current_lines.push(line.to_string());
        } else if !current_path.is_empty() {
            current_lines.push(line.to_string());
            if line.starts_with('+') && !line.starts_with("+++") {
                added += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                removed += 1;
            }
        }
    }

    // Push last file
    if !current_path.is_empty() {
        files.push(DiffFile {
            path: current_path,
            added,
            removed,
            diff_lines: current_lines,
        });
    }

    if files.is_empty() {
        return None;
    }

    Some(DiffViewerState {
        files,
        selected: 0,
        diff_scroll: 0,
        panel: DiffPanel::Files,
        title,
    })
}

// ── Command output popup ─────────────────────────────────────────────────────
//
// A centered overlay that shows output from inline slash commands (/help, /config,
// /skill list, etc.). Dismissed with Esc.

fn draw_command_popup(frame: &mut Frame, app: &App, area: Rect) {
    let popup = match app.command_popup.as_ref() {
        Some(p) => p,
        None    => return,
    };

    // Centered overlay: 80% wide, up to 70% tall.
    let w = (area.width as f32 * 0.82) as u16;
    let h = ((area.height as f32 * 0.7) as u16).max(6).min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let overlay = Rect { x, y, width: w, height: h };

    frame.render_widget(Clear, overlay);

    let border_c = Color::Rgb(100, 180, 255);
    let dim = Color::Rgb(80, 75, 100);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_c))
        .title(Span::styled(
            format!(" {}  Esc dismiss ", popup.title),
            Style::default().fg(Color::Yellow).bold(),
        ));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    // Render content with scroll support for long output.
    let text_lines: Vec<Line<'static>> = popup.text.lines().map(|l| {
        Line::from(Span::styled(l.to_string(), Style::default().fg(Color::Rgb(205, 200, 230))))
    }).collect();

    let total = text_lines.len();
    let viewport_h = inner.height as usize;
    let scroll = popup.scroll.min(total.saturating_sub(viewport_h));

    let para = Paragraph::new(text_lines)
        .scroll((scroll as u16, 0));
    frame.render_widget(para, inner);

    // Scrollbar if content overflows.
    if total > viewport_h {
        let mut sb_state = ScrollbarState::new(total.saturating_sub(viewport_h))
            .position(scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            inner,
            &mut sb_state,
        );
    }

    // Footer hint
    let hint = Rect { y: inner.y + inner.height.saturating_sub(1), height: 1, ..inner };
    frame.render_widget(
        Paragraph::new(Span::styled("  ↑↓ scroll   Esc dismiss ", Style::default().fg(dim))),
        hint,
    );
}

fn draw_permission_popup(frame: &mut Frame, app: &App, area: Rect) {
    let popup = match app.permission_popup.as_ref() {
        Some(p) => p,
        None => return,
    };

    let pending = &popup.pending;

    // Build dialog lines.
    let mut lines: Vec<Line<'static>> = Vec::new();

    if pending.len() == 1 {
        let (_, name, ctx) = &pending[0];
        lines.push(Line::from(vec![
            Span::styled("  Tool: ", Style::default().fg(Color::Rgb(100, 95, 130))),
            Span::styled(name.clone(), Style::default().fg(Color::Rgb(100, 210, 255)).bold()),
        ]));
        if !ctx.is_empty() {
            let ctx_flat = ctx.replace('\n', " ").replace('\r', "");
            lines.push(Line::from(vec![
                Span::styled("  What: ", Style::default().fg(Color::Rgb(100, 95, 130))),
                Span::styled(ctx_flat, Style::default().fg(Color::Rgb(130, 125, 150))),
            ]));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  Agent wants to run {} operation(s):", pending.len()),
                Style::default().fg(Color::Rgb(100, 95, 130)),
            ),
        ]));
        for (_, name, ctx) in pending.iter().take(8) {
            let name_col = if name.chars().count() > 14 {
                format!("{}…", name.chars().take(13).collect::<String>())
            } else {
                name.clone()
            };
            let ctx_flat = ctx.replace('\n', " ").replace('\r', "");
            let ctx_disp = if ctx_flat.chars().count() > 40 {
                format!("{}…", ctx_flat.chars().take(39).collect::<String>())
            } else {
                ctx_flat
            };
            lines.push(Line::from(vec![
                Span::styled("    · ", Style::default().fg(Color::Rgb(70, 65, 90))),
                Span::styled(format!("{:<14}", name_col), Style::default().fg(Color::Rgb(100, 210, 255)).bold()),
                Span::styled("  ", Style::default().fg(Color::Reset)),
                Span::styled(ctx_disp, Style::default().fg(Color::Rgb(130, 125, 150))),
            ]));
        }
        if pending.len() > 8 {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("    … and {} more", pending.len() - 8),
                    Style::default().fg(Color::Rgb(80, 75, 100)),
                ),
            ]));
        }
    }

    // Spacer
    lines.push(Line::from(""));

    // Key hints
    lines.push(Line::from(vec![
        Span::styled("  [", Style::default().fg(Color::Rgb(80, 75, 100))),
        Span::styled("Y", Style::default().fg(Color::Rgb(100, 230, 100)).bold()),
        Span::styled("] Allow   ", Style::default().fg(Color::Rgb(80, 75, 100))),
        Span::styled("[", Style::default().fg(Color::Rgb(80, 75, 100))),
        Span::styled("N", Style::default().fg(Color::Rgb(230, 100, 100)).bold()),
        Span::styled("] Deny   ", Style::default().fg(Color::Rgb(80, 75, 100))),
        Span::styled("[", Style::default().fg(Color::Rgb(80, 75, 100))),
        Span::styled("A", Style::default().fg(Color::Rgb(230, 230, 100)).bold()),
        Span::styled("] Always allow", Style::default().fg(Color::Rgb(80, 75, 100))),
    ]));

    // Compute height: lines + 2 for border + 1 for padding
    let dialog_h = (lines.len() + 3).min(area.height as usize) as u16;
    let dialog_w = (area.width as f32 * 0.78) as u16;

    // Position at bottom of screen
    let x = area.x + (area.width.saturating_sub(dialog_w)) / 2;
    let y = area.y + area.height.saturating_sub(dialog_h);

    let overlay = Rect { x, y, width: dialog_w, height: dialog_h };

    // Clear background
    frame.render_widget(Clear, overlay);

    let border_c = Color::Rgb(180, 130, 70); // amber/warning tone
    let bg = Color::Rgb(25, 22, 35);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_c))
        .style(Style::default().bg(bg))
        .title(Span::styled(
            " Permission Required ",
            Style::default().fg(Color::Rgb(255, 200, 100)).bold(),
        ));

    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    let para = Paragraph::new(lines)
        .style(Style::default().bg(bg));
    frame.render_widget(para, inner);
}
