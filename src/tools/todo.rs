use anyhow::Result;
use async_trait::async_trait;
use std::sync::Mutex;

use super::Tool;

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
}

impl TodoStatus {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "in_progress" | "inprogress" | "active" => Self::InProgress,
            "done" | "completed" | "complete" => Self::Done,
            _ => Self::Pending,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending    => "pending",
            Self::InProgress => "in_progress",
            Self::Done       => "done",
        }
    }
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Pending    => "○",
            Self::InProgress => "◑",
            Self::Done       => "●",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Priority {
    High,
    Medium,
    Low,
}

impl Priority {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "high"   => Self::High,
            "low"    => Self::Low,
            _        => Self::Medium,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::High   => "high",
            Self::Medium => "medium",
            Self::Low    => "low",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TodoItem {
    pub id:       u32,
    pub content:  String,
    pub status:   TodoStatus,
    pub priority: Priority,
}

// ── Global session-scoped state ───────────────────────────────────────────────

static TODO_LIST: Mutex<Vec<TodoItem>> = Mutex::new(Vec::new());

pub fn global_todos() -> Vec<TodoItem> {
    TODO_LIST.lock().map(|g| g.clone()).unwrap_or_default()
}

pub fn clear_todos() {
    if let Ok(mut g) = TODO_LIST.lock() { g.clear(); }
}

fn set_todos(items: Vec<TodoItem>) {
    if let Ok(mut g) = TODO_LIST.lock() { *g = items; }
}

// ── TodoWrite ─────────────────────────────────────────────────────────────────

pub struct TodoWriteTool;

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str { "todo_write" }

    fn description(&self) -> &str {
        "Create or update the session task list. Call this when you receive a \
         multi-step task (3+ steps) to track progress. Replace the entire list \
         on each call — include all items, not just new ones. Mark items \
         in_progress while working on them, done when complete. Use todo_read \
         at the start of a complex task to check for existing items."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "Complete replacement list of todo items.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id":       { "type": "integer", "description": "Stable numeric id (1, 2, 3…)" },
                            "content":  { "type": "string",  "description": "One-line task description" },
                            "status":   { "type": "string",  "enum": ["pending", "in_progress", "done"] },
                            "priority": { "type": "string",  "enum": ["high", "medium", "low"] }
                        },
                        "required": ["id", "content", "status", "priority"]
                    }
                }
            },
            "required": ["todos"]
        })
    }

    fn permission_context(&self, _input: &serde_json::Value) -> String {
        "update task list".to_string()
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let arr = input["todos"].as_array()
            .ok_or_else(|| anyhow::anyhow!("todos must be an array"))?;

        let items: Vec<TodoItem> = arr.iter().enumerate().map(|(i, v)| {
            TodoItem {
                id:       v["id"].as_u64().unwrap_or(i as u64 + 1) as u32,
                content:  v["content"].as_str().unwrap_or("").to_string(),
                status:   TodoStatus::from_str(v["status"].as_str().unwrap_or("pending")),
                priority: Priority::from_str(v["priority"].as_str().unwrap_or("medium")),
            }
        }).collect();

        let total = items.len();
        let done  = items.iter().filter(|t| t.status == TodoStatus::Done).count();
        set_todos(items);

        Ok(format!("Task list updated: {done}/{total} done."))
    }
}

// ── TodoRead ──────────────────────────────────────────────────────────────────

pub struct TodoReadTool;

#[async_trait]
impl Tool for TodoReadTool {
    fn name(&self) -> &str { "todo_read" }

    fn description(&self) -> &str {
        "Read the current session task list. Call this at the start of a complex \
         task to check whether a list already exists before calling todo_write."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }

    fn permission_context(&self, _input: &serde_json::Value) -> String {
        "read task list".to_string()
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String> {
        let todos = global_todos();
        if todos.is_empty() {
            return Ok("No tasks in the current session.".to_string());
        }

        let lines: Vec<String> = todos.iter().map(|t| {
            format!("{} [{}] [{}] {}",
                t.status.icon(), t.priority.as_str(), t.status.as_str(), t.content)
        }).collect();

        let done  = todos.iter().filter(|t| t.status == TodoStatus::Done).count();
        let total = todos.len();
        Ok(format!("Tasks ({done}/{total} done):\n{}", lines.join("\n")))
    }
}
