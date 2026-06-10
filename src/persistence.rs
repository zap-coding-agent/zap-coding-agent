use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};

pub struct Store {
    conn: Connection,
}

fn db_path() -> std::path::PathBuf {
    let base = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".zap");
    std::fs::create_dir_all(&base).ok();
    base.join("agent.db")
}

impl Store {
    pub fn open() -> Result<Self> {
        let path = db_path();
        let conn = Connection::open(&path)
            .with_context(|| format!("failed to open database at {}", path.display()))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                goal       TEXT    NOT NULL,
                model      TEXT    NOT NULL,
                created_at TEXT    NOT NULL
            );
            CREATE TABLE IF NOT EXISTS session_messages (
                session_id INTEGER PRIMARY KEY,
                content    TEXT    NOT NULL,
                updated_at TEXT    NOT NULL
            );
            CREATE TABLE IF NOT EXISTS memory (
                key        TEXT PRIMARY KEY,
                value      TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS branches (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id   INTEGER NOT NULL,
                name         TEXT    NOT NULL,
                parent_name  TEXT    NOT NULL DEFAULT 'main',
                messages_json TEXT   NOT NULL,
                turn_count   INTEGER NOT NULL DEFAULT 0,
                created_at   TEXT    NOT NULL,
                UNIQUE(session_id, name)
            );",
        )
        .context("failed to initialise schema")?;

        Ok(Self { conn })
    }

    // ── Sessions ──────────────────────────────────────────────────────────────

    pub fn save_session(&self, goal: &str, model: &str) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO sessions (goal, model, created_at) VALUES (?1, ?2, ?3)",
                params![goal, model, Utc::now().to_rfc3339()],
            )
            .context("failed to save session")?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_session_goal(&self, session_id: i64, goal: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET goal = ?1 WHERE id = ?2",
            params![goal, session_id],
        ).context("failed to update session goal")?;
        Ok(())
    }

    pub fn get_session_goal(&self, session_id: i64) -> Option<String> {
        self.conn
            .query_row("SELECT goal FROM sessions WHERE id = ?1", params![session_id], |r| r.get(0))
            .ok()
    }

    pub fn recent_sessions(&self, limit: usize) -> Result<Vec<(i64, String, String, String)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, goal, model, created_at
                 FROM sessions ORDER BY id DESC LIMIT ?1",
            )
            .context("failed to prepare sessions query")?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .context("failed to query sessions")?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to collect sessions")
    }

    // ── Message history (per session) ─────────────────────────────────────────

    pub fn save_messages(&self, session_id: i64, messages_json: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session_messages (session_id, content, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id) DO UPDATE SET content = ?2, updated_at = ?3",
            params![session_id, messages_json, Utc::now().to_rfc3339()],
        ).context("failed to save messages")?;
        Ok(())
    }

    pub fn load_messages(&self, session_id: i64) -> Result<Option<String>> {
        let mut stmt = self.conn
            .prepare("SELECT content FROM session_messages WHERE session_id = ?1")
            .context("failed to prepare load_messages")?;

        let mut rows = stmt.query(params![session_id]).context("failed to query messages")?;
        if let Some(row) = rows.next().context("failed to read row")? {
            Ok(Some(row.get(0).context("failed to get content")?))
        } else {
            Ok(None)
        }
    }

    /// Load messages from the most recent session *before* `current_session_id`.
    /// Returns None if there is no prior session or it has no saved messages.
    pub fn load_previous_messages(&self, current_session_id: i64) -> Result<Option<String>> {
        // Get the 2 most recent sessions. The one with id < current_session_id
        // and the highest id is the previous session.
        let sessions = self.recent_sessions(2)?;
        for (id, _, _, _) in &sessions {
            if *id < current_session_id {
                return self.load_messages(*id);
            }
        }
        Ok(None)
    }

    // ── Agent memory (key-value facts) ────────────────────────────────────────

    pub fn set_memory(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO memory (key, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = ?3",
                params![key, value, Utc::now().to_rfc3339()],
            )
            .context("failed to set memory")?;
        Ok(())
    }

    pub fn get_memory(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM memory WHERE key = ?1")
            .context("failed to prepare memory query")?;

        let mut rows = stmt
            .query(params![key])
            .context("failed to query memory")?;

        if let Some(row) = rows.next().context("failed to read memory row")? {
            Ok(Some(row.get(0).context("failed to get memory value")?))
        } else {
            Ok(None)
        }
    }

    pub fn delete_memory(&self, key: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM memory WHERE key = ?1", params![key])
            .context("failed to delete memory")?;
        Ok(())
    }

    pub fn all_memory(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM memory ORDER BY key")
            .context("failed to prepare all-memory query")?;

        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .context("failed to query all memory")?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to collect memory")
    }

    // ── Branches ──────────────────────────────────────────────────────────────

    pub fn save_branch(
        &self,
        session_id: i64,
        name: &str,
        parent_name: &str,
        messages_json: &str,
        turn_count: usize,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO branches (session_id, name, parent_name, messages_json, turn_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(session_id, name) DO UPDATE
               SET messages_json = ?4, turn_count = ?5, created_at = ?6",
            params![session_id, name, parent_name, messages_json, turn_count as i64, Utc::now().to_rfc3339()],
        ).context("failed to save branch")?;
        Ok(())
    }

    pub fn load_branch(&self, session_id: i64, name: &str) -> Result<Option<(String, usize)>> {
        let mut stmt = self.conn
            .prepare("SELECT messages_json, turn_count FROM branches WHERE session_id=?1 AND name=?2")
            .context("failed to prepare load_branch")?;
        let mut rows = stmt.query(params![session_id, name]).context("failed to query branch")?;
        if let Some(row) = rows.next().context("failed to read branch row")? {
            let json: String = row.get(0).context("failed to get messages_json")?;
            let turns: i64 = row.get(1).unwrap_or(0);
            Ok(Some((json, turns as usize)))
        } else {
            Ok(None)
        }
    }

    /// Returns (name, parent_name, turn_count, created_at) for all branches in session.
    pub fn list_branches(&self, session_id: i64) -> Result<Vec<(String, String, usize, String)>> {
        let mut stmt = self.conn
            .prepare("SELECT name, parent_name, turn_count, created_at FROM branches WHERE session_id=?1 ORDER BY created_at")
            .context("failed to prepare list_branches")?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? as usize, row.get(3)?))
            })
            .context("failed to query branches")?;
        rows.collect::<rusqlite::Result<Vec<_>>>().context("failed to collect branches")
    }
}

pub fn init() -> Result<Store> {
    Store::open()
}
