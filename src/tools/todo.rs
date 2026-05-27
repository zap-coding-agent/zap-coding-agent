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
    pub(crate) fn from_str(s: &str) -> Self {
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
    pub(crate) fn from_str(s: &str) -> Self {
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Serialise tests that mutate the global TODO_LIST.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn locked_clean<F: FnOnce()>(f: F) {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_todos();
        f();
        clear_todos();
    }

    // ── Parsing ───────────────────────────────────────────────────────────────

    #[test]
    fn status_known_values() {
        assert_eq!(TodoStatus::from_str("pending"),     TodoStatus::Pending);
        assert_eq!(TodoStatus::from_str("in_progress"), TodoStatus::InProgress);
        assert_eq!(TodoStatus::from_str("inprogress"),  TodoStatus::InProgress);
        assert_eq!(TodoStatus::from_str("active"),      TodoStatus::InProgress);
        assert_eq!(TodoStatus::from_str("done"),        TodoStatus::Done);
        assert_eq!(TodoStatus::from_str("completed"),   TodoStatus::Done);
        assert_eq!(TodoStatus::from_str("complete"),    TodoStatus::Done);
    }

    #[test]
    fn status_unknown_defaults_to_pending() {
        assert_eq!(TodoStatus::from_str(""),        TodoStatus::Pending);
        assert_eq!(TodoStatus::from_str("garbage"), TodoStatus::Pending);
        assert_eq!(TodoStatus::from_str("DONE"),    TodoStatus::Done); // case-insensitive
    }

    #[test]
    fn priority_known_values() {
        assert_eq!(Priority::from_str("high"),   Priority::High);
        assert_eq!(Priority::from_str("medium"), Priority::Medium);
        assert_eq!(Priority::from_str("low"),    Priority::Low);
        assert_eq!(Priority::from_str("HIGH"),   Priority::High); // case-insensitive
    }

    #[test]
    fn priority_unknown_defaults_to_medium() {
        assert_eq!(Priority::from_str(""),       Priority::Medium);
        assert_eq!(Priority::from_str("urgent"), Priority::Medium);
    }

    // ── Global state ──────────────────────────────────────────────────────────

    #[test]
    fn clear_and_read_empty() {
        locked_clean(|| {
            assert!(global_todos().is_empty());
        });
    }

    #[test]
    fn set_and_read_round_trip() {
        locked_clean(|| {
            set_todos(vec![
                TodoItem { id: 1, content: "Write tests".into(), status: TodoStatus::InProgress, priority: Priority::High },
                TodoItem { id: 2, content: "Review PR".into(),   status: TodoStatus::Pending,    priority: Priority::Low  },
            ]);
            let list = global_todos();
            assert_eq!(list.len(), 2);
            assert_eq!(list[0].content, "Write tests");
            assert_eq!(list[0].status,  TodoStatus::InProgress);
            assert_eq!(list[1].priority, Priority::Low);
        });
    }

    // ── TodoWrite::execute ────────────────────────────────────────────────────

    #[tokio::test]
    async fn write_stores_items_and_reports_done_count() {
        locked_clean(|| {});
        let tool = TodoWriteTool;
        let input = serde_json::json!({ "todos": [
            { "id": 1, "content": "Step A", "status": "done",        "priority": "high"   },
            { "id": 2, "content": "Step B", "status": "in_progress", "priority": "medium" },
            { "id": 3, "content": "Step C", "status": "pending",     "priority": "low"    },
        ]});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("1/3"), "expected '1/3 done', got: {result}");

        let list = global_todos();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].status, TodoStatus::Done);
        assert_eq!(list[1].status, TodoStatus::InProgress);
        assert_eq!(list[2].priority, Priority::Low);
        clear_todos();
    }

    #[tokio::test]
    async fn write_empty_array_clears_list() {
        locked_clean(|| {});
        set_todos(vec![TodoItem { id: 1, content: "old".into(), status: TodoStatus::Pending, priority: Priority::Medium }]);
        let tool = TodoWriteTool;
        let result = tool.execute(serde_json::json!({ "todos": [] })).await.unwrap();
        assert!(result.contains("0/0"));
        assert!(global_todos().is_empty());
        clear_todos();
    }

    #[tokio::test]
    async fn write_missing_todos_key_errors() {
        let tool = TodoWriteTool;
        let err = tool.execute(serde_json::json!({})).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn write_missing_fields_use_defaults() {
        locked_clean(|| {});
        let tool = TodoWriteTool;
        // content missing → empty string; status missing → pending
        let input = serde_json::json!({ "todos": [{ "id": 1 }] });
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("0/1"));
        let list = global_todos();
        assert_eq!(list[0].content, "");
        assert_eq!(list[0].status,  TodoStatus::Pending);
        assert_eq!(list[0].priority, Priority::Medium);
        clear_todos();
    }

    // ── TodoRead::execute ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn read_empty_list() {
        locked_clean(|| {});
        let result = TodoReadTool.execute(serde_json::json!({})).await.unwrap();
        assert_eq!(result, "No tasks in the current session.");
    }

    #[tokio::test]
    async fn read_shows_items_with_icons() {
        locked_clean(|| {});
        set_todos(vec![
            TodoItem { id: 1, content: "First".into(),  status: TodoStatus::Done,       priority: Priority::High   },
            TodoItem { id: 2, content: "Second".into(), status: TodoStatus::InProgress, priority: Priority::Medium },
        ]);
        let result = TodoReadTool.execute(serde_json::json!({})).await.unwrap();
        assert!(result.contains("1/2 done"), "got: {result}");
        assert!(result.contains("●"), "done icon missing");
        assert!(result.contains("◑"), "in-progress icon missing");
        assert!(result.contains("First"));
        assert!(result.contains("Second"));
        clear_todos();
    }
}
