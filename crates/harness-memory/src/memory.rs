use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub session_id: String,
    pub text: String,
    pub created_at: String,
}

/// Cosine similarity between two equal-length vectors.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}

/// SQLite-backed vector memory store.
/// Embeddings stored as JSON float arrays; similarity computed in Rust.
#[derive(Clone)]
pub struct MemoryStore {
    conn: Arc<Mutex<Connection>>,
}

impl MemoryStore {
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let path = db_path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening memory DB at {}", path.display()))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL,
                text        TEXT NOT NULL,
                embedding   TEXT NOT NULL,   -- JSON [f32]
                created_at  TEXT NOT NULL
            );",
        )?;

        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn insert(&self, session_id: &str, text: &str, embedding: &[f32]) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let emb_json = serde_json::to_string(embedding)?;
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("mutex lock failed in insert: {}", e))?;
        conn.execute(
            "INSERT INTO memories (id, session_id, text, embedding, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, session_id, text, emb_json, Utc::now().to_rfc3339()],
        )?;
        Ok(id)
    }

    /// Total number of memories in the store.
    pub fn count_all(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("mutex lock failed in count_all: {}", e))?;
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Return the `limit` most recently inserted memories across all sessions.
    pub fn recent_memories(&self, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("mutex lock failed in recent_memories: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, text, created_at
             FROM memories ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(Memory {
                id: row.get(0)?,
                session_id: row.get(1)?,
                text: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    /// Delete memories by id list.
    pub fn delete_memories(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("mutex lock failed in delete_memories: {}", e))?;
        let placeholders = ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("DELETE FROM memories WHERE id IN ({placeholders})");
        let params: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        conn.execute(&sql, params.as_slice())?;
        Ok(())
    }

    /// Return top-k memories by cosine similarity to `query_embedding`.
    /// Excludes memories from the current session to avoid redundancy.
    pub fn search(
        &self,
        query_embedding: &[f32],
        exclude_session: &str,
        top_k: usize,
    ) -> Result<Vec<(Memory, f32)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("mutex lock failed in search: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, text, embedding, created_at
             FROM memories WHERE session_id != ?1",
        )?;
        let rows = stmt.query_map(params![exclude_session], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;

        let mut scored: Vec<(Memory, f32)> = rows
            .filter_map(|r| r.ok())
            .filter_map(|(id, session_id, text, emb_json, created_at)| {
                let emb: Vec<f32> = serde_json::from_str(&emb_json).ok()?;
                let score = cosine(query_embedding, &emb);
                Some((Memory { id, session_id, text, created_at }, score))
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        Ok(scored)
    }
}
