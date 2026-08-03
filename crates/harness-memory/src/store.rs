use anyhow::Context;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::debug;

use crate::session::{Session, SessionId};
use harness_provider_core::Role;

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
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".harness")
            .join("sessions.db")
    }

    pub fn save(&self, session: &Session) -> anyhow::Result<()> {
        let data = serde_json::to_string(session)?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("mutex lock failed in save: {}", e))?;
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
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("mutex lock failed in load: {}", e))?;
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
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("mutex lock failed in find: {}", e))?;
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
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("mutex lock failed in list: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, name, updated_at FROM sessions ORDER BY updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Resolve a human-readable session title for list views.
    pub fn display_name_for(&self, id: &str, name_col: Option<String>) -> String {
        if let Some(n) = name_col.filter(|s| !s.is_empty()) {
            return n;
        }
        if let Ok(Some(session)) = self.load(&id.to_string()) {
            if let Some(n) = session.name.filter(|s| !s.is_empty()) {
                return n;
            }
            if let Some(first) = session
                .messages
                .iter()
                .find(|m| matches!(m.role, Role::User))
            {
                let s = first.content.as_str().trim();
                if !s.is_empty() {
                    let truncated: String = s.chars().take(48).collect();
                    if s.chars().count() > 48 {
                        return format!("{truncated}…");
                    }
                    return truncated.to_string();
                }
            }
        }
        let short: String = id.chars().take(8).collect();
        format!("Session {short}")
    }

    pub fn delete(&self, id_or_prefix: &str) -> anyhow::Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("mutex lock failed in delete: {}", e))?;
        let id = if id_or_prefix.len() < 36 {
            let pattern = format!("{id_or_prefix}%");
            let mut stmt = conn.prepare(
                "SELECT id FROM sessions WHERE id LIKE ?1 ORDER BY updated_at DESC LIMIT 1",
            )?;
            let mut rows = stmt.query(params![pattern])?;
            if let Some(row) = rows.next()? {
                row.get::<_, String>(0)?
            } else {
                return Ok(false);
            }
        } else {
            id_or_prefix.to_string()
        };

        let changed = conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    pub fn set_name_if_missing(&self, id: &str, name: &str) -> anyhow::Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("mutex lock failed in set_name_if_missing: {}", e))?;
        let mut stmt = conn.prepare("SELECT data FROM sessions WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        let Some(row) = rows.next()? else {
            return Ok(false);
        };

        let data: String = row.get(0)?;
        let mut session: Session = serde_json::from_str(&data)?;
        if session.name.as_deref().is_some_and(|n| !n.is_empty()) {
            return Ok(false);
        }

        session.name = Some(name.to_string());
        session.updated_at = chrono::Utc::now();
        let updated_data = serde_json::to_string(&session)?;
        let changed = conn.execute(
            "UPDATE sessions
             SET name = ?2, updated_at = ?3, data = ?4
             WHERE id = ?1",
            params![
                id,
                session.name,
                session.updated_at.to_rfc3339(),
                updated_data
            ],
        )?;
        Ok(changed == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_provider_core::Message;
    use tempfile::tempdir;

    #[test]
    fn save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let store = SessionStore::open(dir.path().join("sessions.db")).unwrap();
        let mut session = Session::new("claude-sonnet-4-6").with_name("test session");
        session.push(Message::user("hello"));
        store.save(&session).unwrap();

        let loaded = store.load(&session.id).unwrap().expect("session");
        assert_eq!(loaded.id, session.id);
        assert_eq!(loaded.name.as_deref(), Some("test session"));
        assert_eq!(loaded.messages.len(), 1);
    }

    #[test]
    fn find_by_id_prefix_and_name() {
        let dir = tempdir().unwrap();
        let store = SessionStore::open(dir.path().join("sessions.db")).unwrap();
        let session = Session::new("gpt-5.5").with_name("My Chat");
        store.save(&session).unwrap();

        let by_prefix = store.find(&session.id[..8]).unwrap().expect("prefix");
        assert_eq!(by_prefix.id, session.id);

        let by_name = store.find("my chat").unwrap().expect("name");
        assert_eq!(by_name.id, session.id);
    }

    #[test]
    fn set_name_if_missing_only_when_empty() {
        let dir = tempdir().unwrap();
        let store = SessionStore::open(dir.path().join("sessions.db")).unwrap();
        let session = Session::new("claude-sonnet-4-6");
        store.save(&session).unwrap();

        assert!(store.set_name_if_missing(&session.id, "Renamed").unwrap());
        let loaded = store.load(&session.id).unwrap().unwrap();
        assert_eq!(loaded.name.as_deref(), Some("Renamed"));

        assert!(!store.set_name_if_missing(&session.id, "Other").unwrap());
    }

    #[test]
    fn delete_by_prefix() {
        let dir = tempdir().unwrap();
        let store = SessionStore::open(dir.path().join("sessions.db")).unwrap();
        let session = Session::new("claude-sonnet-4-6");
        store.save(&session).unwrap();

        assert!(store.delete(&session.id[..8]).unwrap());
        assert!(store.load(&session.id).unwrap().is_none());
    }

    #[test]
    fn display_name_falls_back_to_first_user_message() {
        let dir = tempdir().unwrap();
        let store = SessionStore::open(dir.path().join("sessions.db")).unwrap();
        let mut session = Session::new("claude-sonnet-4-6");
        session.push(Message::user("refactor the agent loop"));
        store.save(&session).unwrap();

        let name = store.display_name_for(&session.id, None);
        assert_eq!(name, "refactor the agent loop");
    }

    #[test]
    fn list_and_save_upsert_and_delete_missing() {
        let dir = tempdir().unwrap();
        let store = SessionStore::open(dir.path().join("sessions.db")).unwrap();
        let mut a = Session::new("m1").with_name("A");
        a.push(Message::user("first"));
        store.save(&a).unwrap();
        let b = Session::new("m2").with_name("B");
        store.save(&b).unwrap();

        let listed = store.list(10).unwrap();
        assert_eq!(listed.len(), 2);
        // ids present
        let ids: Vec<&str> = listed.iter().map(|(id, _, _)| id.as_str()).collect();
        assert!(ids.contains(&a.id.as_str()));
        assert!(ids.contains(&b.id.as_str()));

        // Upsert updates name/data
        a.name = Some("A2".into());
        a.push(Message::user("second"));
        store.save(&a).unwrap();
        let loaded = store.load(&a.id).unwrap().unwrap();
        assert_eq!(loaded.name.as_deref(), Some("A2"));
        assert_eq!(loaded.messages.len(), 2);

        assert!(!store.delete("does-not-exist-zzzz").unwrap());
        assert!(store.delete(&a.id).unwrap()); // full id path (>= 36)
        assert!(store.load(&a.id).unwrap().is_none());
    }

    #[test]
    fn display_name_prefers_col_truncates_and_falls_back() {
        let dir = tempdir().unwrap();
        let store = SessionStore::open(dir.path().join("sessions.db")).unwrap();

        // Prefer non-empty name column without loading
        assert_eq!(
            store.display_name_for("any-id", Some("FromCol".into())),
            "FromCol"
        );
        // Empty name col → load path
        let mut long = Session::new("m");
        let long_msg = "x".repeat(60);
        long.push(Message::user(&long_msg));
        store.save(&long).unwrap();
        let name = store.display_name_for(&long.id, Some(String::new()));
        assert!(name.ends_with('…'), "got: {name}");
        assert_eq!(name.chars().count(), 49); // 48 + ellipsis

        // No messages → Session {short}
        let empty = Session::new("m");
        store.save(&empty).unwrap();
        let fallback = store.display_name_for(&empty.id, None);
        assert!(fallback.starts_with("Session "), "got: {fallback}");
    }

    #[test]
    fn default_path_ends_with_sessions_db() {
        let p = SessionStore::default_path();
        assert!(p.ends_with("sessions.db") || p.file_name().unwrap() == "sessions.db");
    }

    #[test]
    fn find_returns_none_when_missing() {
        let dir = tempdir().unwrap();
        let store = SessionStore::open(dir.path().join("sessions.db")).unwrap();
        assert!(store.find("nope").unwrap().is_none());
        assert!(store.load(&"missing-id".to_string()).unwrap().is_none());
        assert!(!store.set_name_if_missing("missing-id", "x").unwrap());
    }
}
