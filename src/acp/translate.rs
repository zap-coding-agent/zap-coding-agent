//! Pure mapping between zap's internals and ACP v1 wire shapes.
//!
//! Updates are built as `serde_json::Value` in the exact shape the spec
//! documents (`sessionUpdate`, `toolCallId`, …) and only converted to the
//! crate's typed structs at the protocol edge, so these functions read like the
//! spec and unit-test without a connection.

use crate::llm_client::{ContentBlock, Message};
use crate::tools::todo::{Priority, TodoItem, TodoStatus};
use crate::tui::channel::TuiEvent;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// ACP `ToolKind` for a zap tool name — drives the icon/grouping in clients.
pub fn tool_kind(name: &str) -> &'static str {
    match name {
        "read_file" | "glob_read" | "list_directory" | "code_map" | "file_imports"
        | "pack_context" | "todo_read" | "get_diagnostics" | "lsp_type_at" => "read",
        "edit_file" | "batch_edit" | "write_file" | "undo_edit" => "edit",
        "shell" => "execute",
        "search_code" | "find_definition" | "find_references" | "find_subtypes"
        | "find_supertypes" | "find_by_return_type" | "where_imported" | "who_calls"
        | "lsp_definition" | "ripple_analysis" => "search",
        "web_fetch" | "web_search" => "fetch",
        "todo_write" | "spawn_agent" => "think",
        "memory_delete" => "delete",
        _ => "other",
    }
}

fn text(s: &str) -> Value {
    json!({ "type": "text", "text": s })
}

fn chunk(kind: &str, s: &str) -> Value {
    json!({ "sessionUpdate": kind, "content": text(s) })
}

/// Tool-call title shown in the client: `name` plus zap's one-line context.
fn title(name: &str, label: &str) -> String {
    if label.is_empty() { name.to_string() } else { format!("{name} {label}") }
}

/// Resolve a tool's `path` argument against the session cwd — ACP requires
/// absolute paths for locations and diffs.
fn abs_path(cwd: &Path, p: &str) -> PathBuf {
    let p = Path::new(p);
    if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) }
}

/// File locations a tool touches, for "follow along" in the editor.
fn locations(cwd: &Path, input: &Value) -> Vec<Value> {
    input["path"]
        .as_str()
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut loc = json!({ "path": abs_path(cwd, p) });
            if let Some(line) = input["expected_line"].as_u64() {
                loc["line"] = json!(line);
            }
            vec![loc]
        })
        .unwrap_or_default()
}

/// Diff content for edit tools, built from the tool's own arguments. Edits are
/// shown as old/new snippets (what the model replaced), writes as full content.
pub fn diffs(cwd: &Path, name: &str, input: &Value) -> Vec<Value> {
    let Some(path) = input["path"].as_str() else { return vec![] };
    let path = abs_path(cwd, path);
    let diff = |old: Option<&str>, new: &str| {
        json!({ "type": "diff", "path": path, "oldText": old, "newText": new })
    };
    match name {
        "edit_file" => match (input["old_string"].as_str(), input["new_string"].as_str()) {
            (Some(old), Some(new)) => vec![diff(Some(old), new)],
            _ => vec![],
        },
        "batch_edit" => input["edits"]
            .as_array()
            .map(|edits| {
                edits
                    .iter()
                    .filter_map(|e| Some(diff(Some(e["old_string"].as_str()?), e["new_string"].as_str()?)))
                    .collect()
            })
            .unwrap_or_default(),
        "write_file" => input["content"].as_str().map(|c| vec![diff(None, c)]).unwrap_or_default(),
        _ => vec![],
    }
}

/// ACP `plan` update from zap's todo list.
pub fn plan(todos: &[TodoItem]) -> Value {
    let entries: Vec<Value> = todos
        .iter()
        .map(|t| {
            json!({
                "content": t.content,
                "priority": match t.priority { Priority::High => "high", Priority::Medium => "medium", Priority::Low => "low" },
                "status": match t.status { TodoStatus::Pending => "pending", TodoStatus::InProgress => "in_progress", TodoStatus::Done => "completed" },
            })
        })
        .collect();
    json!({ "sessionUpdate": "plan", "entries": entries })
}

#[derive(Default)]
struct ToolState {
    name: String,
    /// Diffs are resent with the final update — v1 `content` replaces, not appends.
    diffs: Vec<Value>,
}

/// Turns zap's `TuiEvent` stream for one prompt turn into `session/update`
/// payloads. Stateful because ACP tool calls are created once, then updated.
pub struct Translator {
    cwd: PathBuf,
    tools: HashMap<String, ToolState>,
}

impl Translator {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd, tools: HashMap::new() }
    }

    /// Announce tool calls that are about to be shown in a permission prompt,
    /// so the client can render them before the user decides.
    pub fn announce(&mut self, id: &str, name: &str, label: &str) -> Value {
        self.tools.insert(id.to_string(), ToolState { name: name.to_string(), ..Default::default() });
        json!({
            "sessionUpdate": "tool_call",
            "toolCallId": id,
            "title": title(name, label),
            "kind": tool_kind(name),
            "status": "pending",
        })
    }

    pub fn event(&mut self, ev: &TuiEvent) -> Vec<Value> {
        match ev {
            TuiEvent::LlmChunk(s) if !s.is_empty() => vec![chunk("agent_message_chunk", s)],
            TuiEvent::ThinkingChunk(s) if !s.is_empty() => vec![chunk("agent_thought_chunk", s)],
            TuiEvent::Notice(s) => vec![chunk("agent_thought_chunk", &format!("{s}\n"))],
            TuiEvent::ActiveSkill(s) => vec![chunk("agent_thought_chunk", &format!("Using skill: {s}\n"))],
            TuiEvent::Warning(s) => vec![chunk("agent_message_chunk", &format!("\n\n> ⚠ {s}\n\n"))],
            TuiEvent::ToolStart { id, name, label, input } => {
                let diffs = diffs(&self.cwd, name, input);
                let announced = self.tools.contains_key(id);
                self.tools.insert(id.clone(), ToolState { name: name.clone(), diffs: diffs.clone() });
                let mut u = json!({
                    "sessionUpdate": if announced { "tool_call_update" } else { "tool_call" },
                    "toolCallId": id,
                    "title": title(name, label),
                    "kind": tool_kind(name),
                    "status": "in_progress",
                    "rawInput": input,
                    "locations": locations(&self.cwd, input),
                });
                if !diffs.is_empty() {
                    u["content"] = json!(diffs);
                }
                vec![u]
            }
            TuiEvent::ToolDone { id, success, preview, .. } => {
                let state = self.tools.get(id);
                let mut content: Vec<Value> = state.map(|s| s.diffs.clone()).unwrap_or_default();
                // Diffs already say what an edit did; the preview is noise next to them.
                if content.is_empty() && !preview.is_empty() {
                    content.push(json!({ "type": "content", "content": text(preview) }));
                }
                let mut out = vec![json!({
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": id,
                    "status": if *success { "completed" } else { "failed" },
                    "content": content,
                })];
                if state.is_some_and(|s| s.name == "todo_write") {
                    out.push(plan(&crate::tools::todo::global_todos()));
                }
                out
            }
            _ => vec![],
        }
    }
}

/// Flatten an ACP prompt (content blocks as JSON) into zap's turn input:
/// the text to send plus images to stage.
pub fn prompt_input(blocks: &[Value]) -> (String, Vec<(String, String)>) {
    let mut parts: Vec<String> = Vec::new();
    let mut images = Vec::new();
    for b in blocks {
        match b["type"].as_str() {
            Some("text") => parts.push(b["text"].as_str().unwrap_or_default().to_string()),
            Some("image") => {
                if let (Some(mime), Some(data)) = (b["mimeType"].as_str(), b["data"].as_str()) {
                    images.push((mime.to_string(), data.to_string()));
                }
            }
            Some("resource") => {
                let r = &b["resource"];
                if let Some(t) = r["text"].as_str() {
                    let uri = r["uri"].as_str().unwrap_or_default();
                    parts.push(format!("\n<file uri=\"{uri}\">\n{t}\n</file>"));
                }
            }
            // Mentions the client didn't embed: hand the model the path so it
            // can read the file itself with its own tools.
            Some("resource_link") => {
                let uri = b["uri"].as_str().unwrap_or_default();
                parts.push(format!("@{}", uri.strip_prefix("file://").unwrap_or(uri)));
            }
            _ => {}
        }
    }
    (parts.join(" ").trim().to_string(), images)
}

/// Client-provided MCP servers (`session/new` `mcpServers`) as zap configs.
/// Only stdio servers — zap advertises no http/sse MCP support.
pub fn mcp_servers(servers: &[Value]) -> Vec<(String, crate::mcp::McpServerConfig)> {
    servers
        .iter()
        .filter(|s| s.get("type").is_none() || s["type"] == "stdio")
        .filter_map(|s| {
            let name = s["name"].as_str()?.to_string();
            let cfg = crate::mcp::McpServerConfig {
                command: s["command"].as_str()?.to_string(),
                args: s["args"].as_array().into_iter().flatten()
                    .filter_map(|a| a.as_str().map(String::from)).collect(),
                env: s["env"].as_array().into_iter().flatten()
                    .filter_map(|e| Some((e["name"].as_str()?.to_string(), e["value"].as_str()?.to_string())))
                    .collect(),
                description: Some("Provided by the editor".to_string()),
                tools_hint: None,
            };
            Some((name, cfg))
        })
        .collect()
}

/// Remove ANSI escape sequences (colours, cursor moves) from terminal output.
pub fn strip_ansi(s: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\r").expect("ansi regex"));
    re.replace_all(s, "").into_owned()
}

/// Replay a stored conversation as `session/update`s for `session/load`.
pub fn history(messages: &[Message]) -> Vec<Value> {
    let mut out = Vec::new();
    for m in messages {
        for b in &m.content {
            match (m.role.as_str(), b) {
                ("user", ContentBlock::Text { text: t }) => out.push(chunk("user_message_chunk", t)),
                ("assistant", ContentBlock::Text { text: t }) => out.push(chunk("agent_message_chunk", t)),
                ("assistant", ContentBlock::ToolUse { id, name, .. }) => out.push(json!({
                    "sessionUpdate": "tool_call",
                    "toolCallId": id,
                    "title": name,
                    "kind": tool_kind(name),
                    "status": "completed",
                })),
                _ => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cwd() -> PathBuf {
        PathBuf::from("/proj")
    }

    #[test]
    fn text_chunks_map_to_message_and_thought() {
        let mut t = Translator::new(cwd());
        let u = t.event(&TuiEvent::LlmChunk("hi".into()));
        assert_eq!(u[0]["sessionUpdate"], "agent_message_chunk");
        assert_eq!(u[0]["content"]["text"], "hi");
        let u = t.event(&TuiEvent::ThinkingChunk("hmm".into()));
        assert_eq!(u[0]["sessionUpdate"], "agent_thought_chunk");
        assert!(t.event(&TuiEvent::LlmChunk(String::new())).is_empty());
    }

    #[test]
    fn edit_tool_emits_tool_call_with_diff_and_absolute_location() {
        let mut t = Translator::new(cwd());
        let input = json!({ "path": "src/a.rs", "old_string": "x", "new_string": "y", "expected_line": 3 });
        let u = t.event(&TuiEvent::ToolStart { id: "t1".into(), name: "edit_file".into(), label: "src/a.rs".into(), input });
        let u = &u[0];
        assert_eq!(u["sessionUpdate"], "tool_call");
        assert_eq!(u["kind"], "edit");
        assert_eq!(u["status"], "in_progress");
        assert_eq!(u["locations"][0]["path"], "/proj/src/a.rs");
        assert_eq!(u["locations"][0]["line"], 3);
        assert_eq!(u["content"][0]["type"], "diff");
        assert_eq!(u["content"][0]["oldText"], "x");
        assert_eq!(u["content"][0]["newText"], "y");
    }

    #[test]
    fn tool_done_keeps_diff_and_marks_status() {
        let mut t = Translator::new(cwd());
        let input = json!({ "path": "/abs/f.rs", "content": "new file" });
        t.event(&TuiEvent::ToolStart { id: "w".into(), name: "write_file".into(), label: String::new(), input });
        let u = t.event(&TuiEvent::ToolDone { id: "w".into(), elapsed_ms: 1, success: true, preview: "wrote".into() });
        assert_eq!(u[0]["sessionUpdate"], "tool_call_update");
        assert_eq!(u[0]["status"], "completed");
        assert_eq!(u[0]["content"][0]["type"], "diff");
        assert_eq!(u[0]["content"][0]["oldText"], Value::Null);
        assert_eq!(u[0]["content"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn non_edit_tool_done_carries_preview_text() {
        let mut t = Translator::new(cwd());
        t.event(&TuiEvent::ToolStart { id: "s".into(), name: "shell".into(), label: "ls".into(), input: json!({ "command": "ls" }) });
        let u = t.event(&TuiEvent::ToolDone { id: "s".into(), elapsed_ms: 1, success: false, preview: "boom".into() });
        assert_eq!(u[0]["status"], "failed");
        assert_eq!(u[0]["content"][0]["content"]["text"], "boom");
    }

    #[test]
    fn announced_tool_is_updated_not_recreated() {
        let mut t = Translator::new(cwd());
        let a = t.announce("t9", "shell", "rm -rf build");
        assert_eq!(a["sessionUpdate"], "tool_call");
        assert_eq!(a["status"], "pending");
        assert_eq!(a["kind"], "execute");
        let u = t.event(&TuiEvent::ToolStart { id: "t9".into(), name: "shell".into(), label: "rm -rf build".into(), input: json!({}) });
        assert_eq!(u[0]["sessionUpdate"], "tool_call_update");
    }

    #[test]
    fn batch_edit_yields_one_diff_per_edit() {
        let input = json!({ "path": "a.rs", "edits": [
            { "old_string": "a", "new_string": "b" },
            { "old_string": "c", "new_string": "d" },
        ]});
        assert_eq!(diffs(&cwd(), "batch_edit", &input).len(), 2);
        assert!(diffs(&cwd(), "shell", &input).is_empty());
    }

    #[test]
    fn plan_maps_todo_status_and_priority() {
        let todos = vec![
            TodoItem { id: 1, content: "A".into(), status: TodoStatus::Done, priority: Priority::High },
            TodoItem { id: 2, content: "B".into(), status: TodoStatus::InProgress, priority: Priority::Low },
        ];
        let p = plan(&todos);
        assert_eq!(p["sessionUpdate"], "plan");
        assert_eq!(p["entries"][0]["status"], "completed");
        assert_eq!(p["entries"][0]["priority"], "high");
        assert_eq!(p["entries"][1]["status"], "in_progress");
    }

    #[test]
    fn prompt_input_flattens_text_resources_links_and_images() {
        let blocks = vec![
            json!({ "type": "text", "text": "explain" }),
            json!({ "type": "resource_link", "name": "a.rs", "uri": "file:///proj/a.rs" }),
            json!({ "type": "resource", "resource": { "uri": "file:///proj/b.rs", "text": "fn b() {}" } }),
            json!({ "type": "image", "mimeType": "image/png", "data": "AAAA" }),
        ];
        let (text, images) = prompt_input(&blocks);
        assert!(text.starts_with("explain @/proj/a.rs"));
        assert!(text.contains("<file uri=\"file:///proj/b.rs\">\nfn b() {}\n</file>"));
        assert_eq!(images, vec![("image/png".to_string(), "AAAA".to_string())]);
    }

    #[test]
    fn strip_ansi_removes_colours_and_carriage_returns() {
        assert_eq!(strip_ansi("\x1b[2m── ok ──\x1b[0m\r\n"), "── ok ──\n");
        assert_eq!(strip_ansi("\x1b[38;2;100;210;255mblue\x1b[0m"), "blue");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn mcp_servers_keeps_stdio_only() {
        let servers = vec![
            json!({ "name": "fs", "command": "/usr/bin/fs-mcp", "args": ["--root", "/p"], "env": [{ "name": "K", "value": "V" }] }),
            json!({ "type": "http", "name": "web", "url": "https://x", "headers": [] }),
        ];
        let out = mcp_servers(&servers);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "fs");
        assert_eq!(out[0].1.args, vec!["--root", "/p"]);
        assert_eq!(out[0].1.env.get("K").map(String::as_str), Some("V"));
    }

    #[test]
    fn history_replays_user_agent_and_tool_calls() {
        let msgs = vec![
            Message::user_text("hi"),
            Message { role: "assistant".into(), content: vec![
                ContentBlock::Text { text: "hello".into() },
                ContentBlock::ToolUse { id: "u1".into(), name: "read_file".into(), input: json!({}) },
            ]},
            Message::tool_results(vec![ContentBlock::ToolResult { tool_use_id: "u1".into(), content: "..".into() }]),
        ];
        let h = history(&msgs);
        assert_eq!(h.len(), 3);
        assert_eq!(h[0]["sessionUpdate"], "user_message_chunk");
        assert_eq!(h[1]["sessionUpdate"], "agent_message_chunk");
        assert_eq!(h[2]["sessionUpdate"], "tool_call");
        assert_eq!(h[2]["status"], "completed");
    }
}
