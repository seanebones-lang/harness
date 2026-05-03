//! Parallel sub-agent swarm: spawn non-blocking agents and track them.
//!
//! Exposes:
//! - `Swarm::spawn(prompt)` → `TaskId`
//! - `Swarm::status(id)` → `TaskStatus`
//! - `Swarm::wait(id)` → Result<String>
//! - `Swarm::cancel(id)`
//! - `Swarm::results()` → Vec<TaskResult>
//!
//! State persisted in `~/.harness/swarm.db` (SQLite).
//! Concurrency capped via a semaphore (default 4).

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{oneshot, Semaphore};

pub type TaskId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    Done,
    Cancelled,
    Failed(String),
}

impl TaskStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
            Self::Failed(_) => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub id: TaskId,
    pub prompt: String,
    pub status: TaskStatus,
    pub result: Option<String>,
    #[allow(dead_code)]
    pub created_ts: i64,
    #[allow(dead_code)]
    pub completed_ts: Option<i64>,
}

// ── Database ──────────────────────────────────────────────────────────────────

fn swarm_db_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".harness/swarm.db")
}

fn open_db() -> Result<Connection> {
    let path = swarm_db_path();
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
    let conn = Connection::open(&path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            prompt TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            result TEXT,
            created_ts INTEGER NOT NULL,
            completed_ts INTEGER
        );",
    )?;
    Ok(conn)
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn new_task_id() -> TaskId {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    now_ts().hash(&mut h);
    format!("sw{:08x}", (h.finish() & 0xFFFF_FFFF) as u32)
}

// ── Swarm ────────────────────────────────────────────────────────────────────

/// In-memory swarm state (tasks spawned in this process).
#[derive(Default)]
#[allow(dead_code)]
pub struct SwarmState {
    pub running: HashMap<TaskId, tokio::task::JoinHandle<()>>,
    pub cancel_txs: HashMap<TaskId, oneshot::Sender<()>>,
}

/// Max concurrent sub-agents.
const MAX_CONCURRENT: usize = 4;

lazy_static::lazy_static! {
    static ref SEMAPHORE: Arc<Semaphore> = Arc::new(Semaphore::new(MAX_CONCURRENT));
}

/// Register a new task in the DB and return its ID.
pub fn register_task(prompt: &str) -> Result<TaskId> {
    let conn = open_db()?;
    let id = new_task_id();
    conn.execute(
        "INSERT INTO tasks (id, prompt, status, created_ts) VALUES (?1, ?2, 'pending', ?3)",
        params![id, prompt, now_ts()],
    )?;
    Ok(id)
}

/// Update task status in the DB.
pub fn update_status(id: &str, status: &TaskStatus, result: Option<&str>) -> Result<()> {
    let conn = open_db()?;
    let status_str = status.as_str();
    let ts = now_ts();
    if result.is_some() {
        conn.execute(
            "UPDATE tasks SET status=?1, result=?2, completed_ts=?3 WHERE id=?4",
            params![status_str, result, ts, id],
        )?;
    } else {
        conn.execute(
            "UPDATE tasks SET status=?1 WHERE id=?2",
            params![status_str, id],
        )?;
    }
    Ok(())
}

/// List recent tasks from the DB.
pub fn list_tasks(limit: usize) -> Result<Vec<TaskEntry>> {
    let conn = open_db()?;
    let mut stmt = conn.prepare(
        "SELECT id, prompt, status, result, created_ts, completed_ts
         FROM tasks ORDER BY created_ts DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        let status_str: String = row.get(2)?;
        let status = match status_str.as_str() {
            "running" => TaskStatus::Running,
            "done" => TaskStatus::Done,
            "cancelled" => TaskStatus::Cancelled,
            _ if status_str.starts_with("failed:") => {
                TaskStatus::Failed(status_str[7..].to_string())
            }
            _ => TaskStatus::Pending,
        };
        Ok(TaskEntry {
            id: row.get(0)?,
            prompt: row.get(1)?,
            status,
            result: row.get(3)?,
            created_ts: row.get(4)?,
            completed_ts: row.get(5)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Get a specific task.
pub fn get_task(id: &str) -> Result<Option<TaskEntry>> {
    let tasks = list_tasks(1000)?;
    Ok(tasks.into_iter().find(|t| t.id == id))
}

/// Spawn a task: registers in DB, acquires semaphore, runs agent in background.
/// Returns immediately with the task ID.
pub async fn spawn_task<F, Fut>(id: TaskId, fut: F)
where
    F: FnOnce(TaskId) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<String>> + Send + 'static,
{
    let id2 = id.clone();
    tokio::spawn(async move {
        let _permit = SEMAPHORE.acquire().await;
        let _ = update_status(&id2, &TaskStatus::Running, None);
        match fut(id2.clone()).await {
            Ok(result) => {
                let _ = update_status(&id2, &TaskStatus::Done, Some(&result));
            }
            Err(e) => {
                let _ = update_status(
                    &id2,
                    &TaskStatus::Failed(e.to_string()),
                    Some(&e.to_string()),
                );
            }
        }
    });
}

/// Print a summary of recent swarm tasks.
pub fn print_status() -> Result<()> {
    let tasks = list_tasks(20)?;
    if tasks.is_empty() {
        println!("No swarm tasks yet.");
        return Ok(());
    }
    println!("{:<12} {:<10} Prompt", "ID", "Status");
    println!("{}", "-".repeat(70));
    for t in &tasks {
        let p = if t.prompt.len() > 50 {
            format!("{}…", &t.prompt[..50])
        } else {
            t.prompt.clone()
        };
        println!("{:<12} {:<10} {}", t.id, t.status.as_str(), p);
    }
    Ok(())
}
