use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};

const DB_PATH: &str = "agent_sessions.db";

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open() -> Result<Self> {
        let conn = Connection::open(DB_PATH)
            .context("failed to open SQLite database")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                goal       TEXT    NOT NULL,
                model      TEXT    NOT NULL,
                created_at TEXT    NOT NULL
            );
            CREATE TABLE IF NOT EXISTS memory (
                key        TEXT PRIMARY KEY,
                value      TEXT NOT NULL,
                updated_at TEXT NOT NULL
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
}

pub fn init() -> Result<Store> {
    Store::open()
}
