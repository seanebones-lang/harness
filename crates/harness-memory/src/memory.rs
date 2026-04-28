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
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO memories (id, session_id, text, embedding, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, session_id, text, emb_json, Utc::now().to_rfc3339()],
        )?;
        Ok(id)
    }

    /// Return top-k memories by cosine similarity to `query_embedding`.
    /// Excludes memories from the current session to avoid redundancy.
    pub fn search(
        &self,
        query_embedding: &[f32],
        exclude_session: &str,
        top_k: usize,
    ) -> Result<Vec<(Memory, f32)>> {
        let conn = self.conn.lock().unwrap();
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
