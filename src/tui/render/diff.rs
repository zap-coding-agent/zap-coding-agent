use ratatui::{prelude::*, widgets::*};
use ratatui::style::Modifier;
use super::super::app::{App, DiffFile, DiffPanel, DiffViewerState};
use super::messages::expand_tabs_and_truncate;

pub(super) fn draw_diff_viewer(frame: &mut Frame, app: &App, area: Rect) {
    let dv = match &app.diff_viewer {
        Some(dv) => dv,
        None => return,
    };

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

    frame.render_widget(Clear, overlay_area);

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
                    Style::default().fg(Color::White).bg(Color::DarkGray).add_modifier(Modifier::BOLD),
                ),
                Span::styled(stats, Style::default().fg(Color::Rgb(100, 210, 120)).bg(Color::DarkGray)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(path_disp, Style::default().fg(Color::White)),
                Span::styled(stats, Style::default().fg(Color::Rgb(100, 210, 120))),
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

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_diff_content(frame: &mut Frame, dv: &DiffViewerState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(" Diff ", Style::default().fg(Color::Cyan).bold()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line<'static>> = Vec::new();

    if let Some(file) = dv.files.get(dv.selected) {
        for raw in &file.diff_lines {
            let display = expand_tabs_and_truncate(raw, inner.width.saturating_sub(4) as usize);
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

    let scroll = if dv.panel == DiffPanel::Diff { dv.diff_scroll as u16 } else { 0 };
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

/// Run `git diff` (falling back to `git diff HEAD~1`, then session snapshots)
/// and parse the output into a `DiffViewerState`.
/// Returns `None` only if git fails AND no session snapshots exist.
pub fn open_diff_viewer() -> Option<DiffViewerState> {
    let git_result: Option<(String, String)> = (|| {
        let out = std::process::Command::new("git")
            .args(["diff", "--unified=5"])
            .output()
            .ok()?;
        let t = String::from_utf8(out.stdout).ok()?;
        if out.status.success() && !t.trim().is_empty() {
            return Some((t, "working changes".to_string()));
        }
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

    snapshot_diff_viewer()
}

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
    Some(DiffViewerState {
        files,
        selected: 0,
        diff_scroll: 0,
        panel: DiffPanel::Files,
        title: "session edits".to_string(),
    })
}

fn parse_git_diff(text: String, title: String) -> Option<DiffViewerState> {
    let mut files: Vec<DiffFile> = Vec::new();
    let mut current_path = String::new();
    let mut current_lines: Vec<String> = Vec::new();
    let mut added = 0usize;
    let mut removed = 0usize;

    for line in text.lines() {
        if let Some(path) = line.strip_prefix("diff --git a/") {
            if !current_path.is_empty() {
                files.push(DiffFile {
                    path: current_path,
                    added,
                    removed,
                    diff_lines: current_lines,
                });
            }
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
