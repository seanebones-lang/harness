use anyhow::Context;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::debug;

use crate::session::{Session, SessionId};

/// SQLite-backed persistent session store.
/// One DB file per workspace (or `~/.harness/sessions.db` as fallback).
#[derive(Clone)]
pub struct SessionStore {
    conn: Arc<Mutex<Connection>>,
}

impl SessionStore {
    pub fn open(db_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = db_path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("opening session DB at {}", path.display()))?;

        // WAL mode for concurrent reads
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id          TEXT PRIMARY KEY,
                name        TEXT,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL,
                data        TEXT NOT NULL
            );",
        )?;

        debug!(db = %path.display(), "session store opened");
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".harness")
            .join("sessions.db")
    }

    pub fn save(&self, session: &Session) -> anyhow::Result<()> {
        let data = serde_json::to_string(session)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, name, created_at, updated_at, data)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
               name=excluded.name,
               updated_at=excluded.updated_at,
               data=excluded.data",
            params![
                session.id,
                session.name,
                session.created_at.to_rfc3339(),
                session.updated_at.to_rfc3339(),
                data,
            ],
        )?;
        Ok(())
    }

    pub fn load(&self, id: &SessionId) -> anyhow::Result<Option<Session>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT data FROM sessions WHERE id=?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            let data: String = row.get(0)?;
            let session: Session = serde_json::from_str(&data)?;
            return Ok(Some(session));
        }
        Ok(None)
    }

    /// Find a session by prefix of id or by name (case-insensitive).
    pub fn find(&self, query: &str) -> anyhow::Result<Option<Session>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("{query}%");
        let mut stmt = conn.prepare(
            "SELECT data FROM sessions
             WHERE id LIKE ?1 OR LOWER(name) = LOWER(?2)
             ORDER BY updated_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![pattern, query])?;
        if let Some(row) = rows.next()? {
            let data: String = row.get(0)?;
            return Ok(Some(serde_json::from_str(&data)?));
        }
        Ok(None)
    }

    pub fn list(&self, limit: usize) -> anyhow::Result<Vec<(String, Option<String>, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, updated_at FROM sessions ORDER BY updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, String>(2)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}
