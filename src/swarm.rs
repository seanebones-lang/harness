//! Parallel sub-agent swarm: spawn non-blocking agents and track them.
//!
//! Public API (SQLite-backed at `~/.harness/swarm.db`, overridable via config / env):
//! - `configure(&SwarmConfig)` — max concurrency + DB path from `[swarm]`
//! - `register_task(prompt)` → `TaskId`
//! - `update_status(id, status, result?)`
//! - `list_tasks(limit)` / `get_task(id)`
//! - `spawn_task(id, fut)` — runs async work in background (semaphore-limited)
//! - `cancel_task(id)` / `wait_task(id, timeout)`
//! - `print_status()` — CLI summary

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{oneshot, Semaphore};

thread_local! {
    static SWARM_DB_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub type TaskId = String;

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct SwarmConfig {
    pub max_concurrency: Option<usize>,
    pub db_path: Option<PathBuf>,
    /// When set, `configure` reaps orphan pending/running older than this many seconds.
    pub auto_gc_stale_secs: Option<u64>,
}

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

    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Pending | Self::Running)
    }
}

/// Aggregate task counts across the registry (not limited to recent N).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SwarmCounts {
    pub pending: usize,
    pub running: usize,
    pub done: usize,
    pub failed: usize,
    pub cancelled: usize,
}

impl SwarmCounts {
    pub fn total(&self) -> usize {
        self.pending + self.running + self.done + self.failed + self.cancelled
    }

    pub fn active(&self) -> usize {
        self.pending + self.running
    }

    pub fn label(&self) -> String {
        format!(
            "p={} r={} d={} f={} c={} (n={})",
            self.pending,
            self.running,
            self.done,
            self.failed,
            self.cancelled,
            self.total()
        )
    }
}

/// Options for `gc` (stale reap + optional terminal purge).
#[derive(Debug, Clone)]
pub struct GcOptions {
    /// Mark non-live pending/running older than this as failed (default 3600).
    pub stale_secs: u64,
    /// Keep at most this many terminal tasks (newest first); delete the rest.
    pub keep_terminal: Option<usize>,
    /// Delete terminal tasks whose completed_ts is older than this many seconds.
    pub older_than_secs: Option<u64>,
    /// Report only; do not write.
    pub dry_run: bool,
}

impl Default for GcOptions {
    fn default() -> Self {
        Self {
            stale_secs: 3600,
            keep_terminal: None,
            older_than_secs: None,
            dry_run: false,
        }
    }
}

/// Result of a GC pass.
#[derive(Debug, Clone, Default)]
pub struct GcReport {
    pub reaped: Vec<(TaskId, String)>,
    pub deleted: usize,
}

impl GcReport {
    pub fn summary(&self) -> String {
        format!(
            "reaped {} orphan(s), deleted {} terminal task(s)",
            self.reaped.len(),
            self.deleted
        )
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

struct SwarmState {
    cancel_txs: std::collections::HashMap<TaskId, oneshot::Sender<()>>,
}

struct SwarmRuntime {
    semaphore: std::sync::Arc<Semaphore>,
    active: Mutex<SwarmState>,
    db_path: Option<PathBuf>,
}

static RUNTIME: OnceLock<SwarmRuntime> = OnceLock::new();

fn runtime() -> &'static SwarmRuntime {
    RUNTIME.get_or_init(|| SwarmRuntime {
        semaphore: std::sync::Arc::new(Semaphore::new(4)),
        active: Mutex::new(SwarmState {
            cancel_txs: std::collections::HashMap::new(),
        }),
        db_path: None,
    })
}

fn lock_active() -> std::sync::MutexGuard<'static, SwarmState> {
    runtime().active.lock().unwrap_or_else(|e| e.into_inner())
}

/// Apply `[swarm]` settings from config (safe to call multiple times; first call wins).
pub fn configure(cfg: &SwarmConfig) {
    let _ = RUNTIME.set(SwarmRuntime {
        semaphore: std::sync::Arc::new(Semaphore::new(cfg.max_concurrency.unwrap_or(4).max(1))),
        active: Mutex::new(SwarmState {
            cancel_txs: std::collections::HashMap::new(),
        }),
        db_path: cfg.db_path.clone(),
    });
    if let Some(stale) = cfg.auto_gc_stale_secs {
        if stale > 0 {
            let _ = gc(&GcOptions {
                stale_secs: stale,
                keep_terminal: None,
                older_than_secs: None,
                dry_run: false,
            });
        }
    }
}

fn swarm_db_path() -> PathBuf {
    if let Some(path) = SWARM_DB_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return path;
    }
    if let Some(path) = runtime().db_path.clone() {
        return path;
    }
    if let Ok(path) = std::env::var("HARNESS_SWARM_DB") {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    dirs::home_dir()
        .unwrap_or_default()
        .join(".harness/swarm.db")
}

#[cfg(test)]
fn set_test_swarm_db(dir: &tempfile::TempDir) {
    let path = dir.path().join("swarm.db");
    SWARM_DB_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(path));
}

#[cfg(test)]
fn clear_test_swarm_db() {
    SWARM_DB_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
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
    format!("sw{:08x}", uuid::Uuid::new_v4().as_u128() as u32)
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
    let status_str = match status {
        TaskStatus::Failed(msg) => format!("failed:{msg}"),
        other => other.as_str().to_string(),
    };
    let ts = now_ts();
    if result.is_some() || status.is_terminal() {
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
        let status = parse_status_str(&status_str, row.get::<_, Option<String>>(3)?);
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

fn parse_status_str(status_str: &str, result_col: Option<String>) -> TaskStatus {
    match status_str {
        "running" => TaskStatus::Running,
        "done" => TaskStatus::Done,
        "cancelled" => TaskStatus::Cancelled,
        "failed" => TaskStatus::Failed(result_col.unwrap_or_default()),
        s if s.starts_with("failed:") => TaskStatus::Failed(s[7..].to_string()),
        _ => TaskStatus::Pending,
    }
}

/// Get a specific task.
pub fn get_task(id: &str) -> Result<Option<TaskEntry>> {
    let conn = open_db()?;
    let mut stmt = conn.prepare(
        "SELECT id, prompt, status, result, created_ts, completed_ts
         FROM tasks WHERE id = ?1 OR id LIKE ?2 LIMIT 1",
    )?;
    let prefix = format!("{id}%");
    let mut rows = stmt.query(params![id, prefix])?;
    if let Some(row) = rows.next()? {
        let status_str: String = row.get(2)?;
        let status = parse_status_str(&status_str, row.get::<_, Option<String>>(3)?);
        return Ok(Some(TaskEntry {
            id: row.get(0)?,
            prompt: row.get(1)?,
            status,
            result: row.get(3)?,
            created_ts: row.get(4)?,
            completed_ts: row.get(5)?,
        }));
    }
    Ok(None)
}

/// Spawn a task: runs agent work in background (semaphore-limited). Returns immediately.
pub async fn spawn_task<F, Fut>(id: TaskId, fut: F)
where
    F: FnOnce(TaskId) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<String>> + Send + 'static,
{
    let (cancel_tx, cancel_rx) = oneshot::channel();
    {
        lock_active().cancel_txs.insert(id.clone(), cancel_tx);
    }

    let id2 = id.clone();
    let sem = runtime().semaphore.clone();
    tokio::spawn(async move {
        let _permit = sem.acquire().await;
        let _ = update_status(&id2, &TaskStatus::Running, None);
        let outcome = tokio::select! {
            res = fut(id2.clone()) => Some(res),
            _ = cancel_rx => None,
        };
        match outcome {
            Some(Ok(result)) => {
                let _ = update_status(&id2, &TaskStatus::Done, Some(&result));
            }
            Some(Err(e)) => {
                let msg = e.to_string();
                let _ = update_status(&id2, &TaskStatus::Failed(msg.clone()), Some(&msg));
            }
            None => {
                let _ = update_status(&id2, &TaskStatus::Cancelled, None);
            }
        }
        lock_active().cancel_txs.remove(&id2);
    });
}

/// Cancel a pending or running task. Returns true if cancellation was initiated.
pub fn cancel_task(id: &str) -> Result<bool> {
    let Some(task) = get_task(id)? else {
        return Ok(false);
    };
    if task.status.is_terminal() {
        return Ok(false);
    }

    if let Some(tx) = lock_active().cancel_txs.remove(&task.id) {
        let _ = tx.send(());
        return Ok(true);
    }

    update_status(&task.id, &TaskStatus::Cancelled, None)?;
    Ok(true)
}

/// Cancel every non-terminal task. Returns how many cancellations were initiated.
pub fn cancel_all_tasks() -> Result<usize> {
    let tasks = list_tasks(10_000)?;
    let mut n = 0usize;
    for t in tasks {
        if t.status.is_terminal() {
            continue;
        }
        if cancel_task(&t.id)? {
            n += 1;
        }
    }
    Ok(n)
}

/// Poll until a task reaches a terminal state or timeout elapses.
pub async fn wait_task(id: &str, timeout: Option<Duration>) -> Result<Option<TaskEntry>> {
    let deadline = timeout.map(|t| Instant::now() + t);
    loop {
        if let Some(task) = get_task(id)? {
            if task.status.is_terminal() {
                return Ok(Some(task));
            }
        } else {
            return Ok(None);
        }
        if let Some(d) = deadline {
            if Instant::now() >= d {
                return get_task(id);
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Format a unix timestamp as local-ish `YYYY-MM-DD HH:MM:SS` (UTC).
fn fmt_ts(ts: i64) -> String {
    // Keep dep-free: manual UTC formatting from epoch seconds.
    let secs = ts.max(0) as u64;
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let h = tod / 3600;
    let m = (tod % 3600) / 60;
    let s = tod % 60;
    // Civil date from days since 1970-01-01 (Howard Hinnant algorithm).
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
}

/// Truncate on a char boundary (never panic on multi-byte UTF-8).
fn trunc_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max_chars).collect::<String>())
    }
}

/// One-line status label, including failure detail when present.
pub fn status_label(status: &TaskStatus) -> String {
    match status {
        TaskStatus::Failed(msg) if !msg.is_empty() => {
            format!("failed({})", trunc_chars(msg, 40))
        }
        other => other.as_str().to_string(),
    }
}

/// Print a single task in a multi-line, human-readable form.
pub fn print_task_detail(task: &TaskEntry) {
    println!("id:          {}", task.id);
    println!("status:      {}", status_label(&task.status));
    println!("prompt:      {}", task.prompt);
    println!("created:     {}", fmt_ts(task.created_ts));
    match task.completed_ts {
        Some(ts) => println!("completed:   {}", fmt_ts(ts)),
        None => println!("completed:   —"),
    }
    if let Some(ref result) = task.result {
        if result.chars().count() > 500 {
            println!(
                "result:\n{} ({} bytes total)",
                trunc_chars(result, 500),
                result.len()
            );
        } else {
            println!("result:\n{result}");
        }
    } else {
        println!("result:      (none)");
    }
}

/// Serialize a task to JSON (for CLI `--json`).
pub fn task_to_json(task: &TaskEntry) -> serde_json::Value {
    let (status, error) = match &task.status {
        TaskStatus::Failed(msg) => ("failed".to_string(), Some(msg.clone())),
        other => (other.as_str().to_string(), None),
    };
    serde_json::json!({
        "id": task.id,
        "status": status,
        "error": error,
        "prompt": task.prompt,
        "result": task.result,
        "created_ts": task.created_ts,
        "completed_ts": task.completed_ts,
        "created": fmt_ts(task.created_ts),
        "completed": task.completed_ts.map(fmt_ts),
        "terminal": task.status.is_terminal(),
    })
}

/// Print task as a single JSON object.
pub fn print_task_json(task: &TaskEntry) {
    println!("{}", task_to_json(task));
}

/// Count every task in the registry.
pub fn count_tasks() -> Result<SwarmCounts> {
    let conn = open_db()?;
    let mut stmt = conn.prepare("SELECT status FROM tasks")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut counts = SwarmCounts::default();
    for status_str in rows.flatten() {
        match parse_status_str(&status_str, None) {
            TaskStatus::Pending => counts.pending += 1,
            TaskStatus::Running => counts.running += 1,
            TaskStatus::Done => counts.done += 1,
            TaskStatus::Failed(_) => counts.failed += 1,
            TaskStatus::Cancelled => counts.cancelled += 1,
        }
    }
    Ok(counts)
}

/// True if this process still holds a cancel handle for `id` (live worker).
pub fn is_live(id: &str) -> bool {
    lock_active().cancel_txs.contains_key(id)
}

/// Reap orphan pending/running tasks and optionally purge old terminal rows.
pub fn gc(opts: &GcOptions) -> Result<GcReport> {
    let mut report = GcReport::default();
    let now = now_ts();
    let tasks = list_tasks(10_000)?;

    for t in &tasks {
        if t.status.is_terminal() {
            continue;
        }
        if is_live(&t.id) {
            continue;
        }
        let age = now.saturating_sub(t.created_ts) as u64;
        if age < opts.stale_secs {
            continue;
        }
        let reason = format!("stale: orphaned after {age}s with no live worker");
        if !opts.dry_run {
            update_status(
                &t.id,
                &TaskStatus::Failed(reason.clone()),
                Some(&reason),
            )?;
        }
        report.reaped.push((t.id.clone(), reason));
    }

    // Purge terminal rows when keep / older_than requested.
    // Re-list after reap so newly failed orphans participate in keep/age rules.
    if opts.keep_terminal.is_some() || opts.older_than_secs.is_some() {
        let snapshot = if opts.dry_run {
            // Simulate reap in-memory for dry-run purge accounting.
            let reaped: std::collections::HashSet<&str> =
                report.reaped.iter().map(|(id, _)| id.as_str()).collect();
            tasks
                .iter()
                .filter(|t| t.status.is_terminal() || reaped.contains(t.id.as_str()))
                .cloned()
                .collect::<Vec<_>>()
        } else {
            list_tasks(10_000)?
                .into_iter()
                .filter(|t| t.status.is_terminal())
                .collect()
        };

        let mut delete_ids: Vec<String> = Vec::new();
        if let Some(keep) = opts.keep_terminal {
            for t in snapshot.iter().skip(keep) {
                delete_ids.push(t.id.clone());
            }
        }
        if let Some(max_age) = opts.older_than_secs {
            for t in &snapshot {
                let completed = t.completed_ts.unwrap_or(t.created_ts);
                let age = now.saturating_sub(completed) as u64;
                if age >= max_age {
                    delete_ids.push(t.id.clone());
                }
            }
        }
        delete_ids.sort();
        delete_ids.dedup();
        if !opts.dry_run {
            let conn = open_db()?;
            for id in &delete_ids {
                let n = conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
                report.deleted += n;
            }
        } else {
            report.deleted = delete_ids.len();
        }
    }

    Ok(report)
}

/// Compact lines for the TUI swarm panel (newest first).
pub fn tui_lines(limit: usize) -> Result<(SwarmCounts, Vec<String>)> {
    let counts = count_tasks()?;
    let tasks = list_tasks(limit)?;
    let mut lines = Vec::with_capacity(tasks.len() + 2);
    lines.push(format!("swarm {}", counts.label()));
    if counts.active() > 0 {
        lines.push(format!(
            "active {}  (F2/toggle · /swarm gc)",
            counts.active()
        ));
    } else {
        lines.push("no active workers  (/swarm gc cleans orphans)".into());
    }
    for t in tasks {
        let live = if !t.status.is_terminal() && is_live(&t.id) {
            "*"
        } else if !t.status.is_terminal() {
            "!"
        } else {
            " "
        };
        lines.push(format!(
            "{live}{:<10} {:<10} {}",
            t.id,
            status_label(&t.status),
            trunc_chars(&t.prompt, 36)
        ));
    }
    Ok((counts, lines))
}

/// Print a summary of recent swarm tasks (counts + table).
pub fn print_status() -> Result<()> {
    let tasks = list_tasks(20)?;
    if tasks.is_empty() {
        println!("No swarm tasks yet.");
        println!("Queue work with: harness swarm run \"…\" [--count N]");
        return Ok(());
    }

    let mut pending = 0usize;
    let mut running = 0usize;
    let mut done = 0usize;
    let mut failed = 0usize;
    let mut cancelled = 0usize;
    for t in &tasks {
        match &t.status {
            TaskStatus::Pending => pending += 1,
            TaskStatus::Running => running += 1,
            TaskStatus::Done => done += 1,
            TaskStatus::Failed(_) => failed += 1,
            TaskStatus::Cancelled => cancelled += 1,
        }
    }
    println!(
        "Swarm (last {}): pending={pending} running={running} done={done} failed={failed} cancelled={cancelled}",
        tasks.len()
    );
    println!("{:<12} {:<18} {:<20} Prompt", "ID", "Status", "Created");
    println!("{}", "-".repeat(78));
    for t in &tasks {
        println!(
            "{:<12} {:<18} {:<20} {}",
            t.id,
            status_label(&t.status),
            fmt_ts(t.created_ts),
            trunc_chars(&t.prompt, 40)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDb {
        _dir: tempfile::TempDir,
    }

    impl TestDb {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            set_test_swarm_db(&dir);
            Self { _dir: dir }
        }
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            clear_test_swarm_db();
        }
    }

    #[test]
    fn register_and_update_task_roundtrip() {
        let _db = TestDb::new();
        let id = register_task("hello swarm").expect("register");
        update_status(&id, &TaskStatus::Running, None).expect("running");
        update_status(&id, &TaskStatus::Done, Some("ok")).expect("done");

        let task = get_task(&id).expect("get").expect("found");
        assert_eq!(task.prompt, "hello swarm");
        assert_eq!(task.status, TaskStatus::Done);
        assert_eq!(task.result.as_deref(), Some("ok"));
    }

    #[test]
    fn failed_status_persists_with_message() {
        let _db = TestDb::new();
        let id = register_task("fail me").expect("register");
        update_status(&id, &TaskStatus::Failed("boom".into()), Some("boom")).expect("failed");

        let task = get_task(&id).expect("get").expect("found");
        assert_eq!(task.status, TaskStatus::Failed("boom".into()));
    }

    #[test]
    fn list_tasks_returns_recent_first() {
        let _db = TestDb::new();
        register_task("first").expect("register a");
        register_task("second").expect("register b");
        let prompts: Vec<String> = list_tasks(10)
            .expect("list")
            .into_iter()
            .map(|t| t.prompt)
            .collect();
        assert_eq!(prompts.len(), 2);
        assert!(prompts.iter().any(|p| p == "first"));
        assert!(prompts.iter().any(|p| p == "second"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn spawn_task_updates_db_on_completion() {
        let _db = TestDb::new();
        let id = register_task("background").expect("register");
        spawn_task(id.clone(), |_task_id| async { Ok("finished".into()) }).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let task = get_task(&id).expect("get").expect("found");
        assert_eq!(task.status, TaskStatus::Done);
        assert_eq!(task.result.as_deref(), Some("finished"));
    }

    #[test]
    fn get_task_matches_id_prefix() {
        let _db = TestDb::new();
        let id = register_task("prefix test").expect("register");
        let short = id.chars().take(8).collect::<String>();
        let found = get_task(&short).expect("get by prefix").expect("found");
        assert_eq!(found.id, id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancel_pending_task() {
        let _db = TestDb::new();
        let id = register_task("cancel me").expect("register");
        assert!(cancel_task(&id).expect("cancel"));
        let task = get_task(&id).expect("get").expect("found");
        assert_eq!(task.status, TaskStatus::Cancelled);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn wait_task_returns_on_done() {
        let _db = TestDb::new();
        let id = register_task("wait me").expect("register");
        spawn_task(id.clone(), |_task_id| async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok("done".into())
        })
        .await;
        let task = wait_task(&id, Some(Duration::from_secs(5)))
            .await
            .expect("wait")
            .expect("found");
        assert_eq!(task.status, TaskStatus::Done);
    }

    #[test]
    fn status_label_includes_failure_detail() {
        assert_eq!(status_label(&TaskStatus::Done), "done");
        assert_eq!(
            status_label(&TaskStatus::Failed("boom".into())),
            "failed(boom)"
        );
        // multi-byte safe truncation
        let long = "x".repeat(50) + "✨";
        let label = status_label(&TaskStatus::Failed(long));
        assert!(label.starts_with("failed("));
        assert!(label.ends_with(')'));
    }

    #[test]
    fn fmt_ts_epoch_day() {
        // 1970-01-01 00:00:00 UTC
        assert_eq!(fmt_ts(0), "1970-01-01 00:00:00 UTC");
        // 2024-01-01 00:00:00 UTC
        assert_eq!(fmt_ts(1_704_067_200), "2024-01-01 00:00:00 UTC");
    }

    #[test]
    fn gc_reaps_orphan_running_and_purges_old_terminal() {
        let _db = TestDb::new();
        let orphan = register_task("orphan run").expect("register");
        update_status(&orphan, &TaskStatus::Running, None).expect("running");
        // Backdate created_ts so it is stale.
        {
            let conn = open_db().expect("db");
            conn.execute(
                "UPDATE tasks SET created_ts = ?1 WHERE id = ?2",
                params![now_ts() - 10_000, orphan],
            )
            .expect("backdate");
        }
        let keep_me = register_task("keep terminal").expect("register keep");
        update_status(&keep_me, &TaskStatus::Done, Some("ok")).expect("done");
        let drop_me = register_task("drop terminal").expect("register drop");
        update_status(&drop_me, &TaskStatus::Done, Some("old")).expect("done");
        {
            let conn = open_db().expect("db");
            conn.execute(
                "UPDATE tasks SET completed_ts = ?1 WHERE id = ?2",
                params![now_ts() - 86_400 * 40, drop_me],
            )
            .expect("backdate completed");
        }

        // Reap only first — orphan must remain as failed.
        let report = gc(&GcOptions {
            stale_secs: 60,
            keep_terminal: None,
            older_than_secs: None,
            dry_run: false,
        })
        .expect("gc reap");
        assert_eq!(report.reaped.len(), 1);
        assert_eq!(report.reaped[0].0, orphan);
        let orphan_task = get_task(&orphan).expect("get").expect("found");
        assert!(matches!(orphan_task.status, TaskStatus::Failed(_)));

        // Age-based purge removes only the old completed row.
        let report2 = gc(&GcOptions {
            stale_secs: 60,
            keep_terminal: None,
            older_than_secs: Some(86_400 * 30),
            dry_run: false,
        })
        .expect("gc purge");
        assert!(report2.deleted >= 1);
        assert!(get_task(&drop_me).expect("get").is_none());
        assert!(get_task(&keep_me).expect("get").is_some());
        assert!(get_task(&orphan).expect("get").is_some());
    }

    #[test]
    fn count_and_tui_lines() {
        let _db = TestDb::new();
        register_task("a").expect("a");
        let id = register_task("b").expect("b");
        update_status(&id, &TaskStatus::Done, Some("x")).expect("done");
        let counts = count_tasks().expect("count");
        assert_eq!(counts.pending, 1);
        assert_eq!(counts.done, 1);
        let (c2, lines) = tui_lines(10).expect("tui");
        assert_eq!(c2, counts);
        assert!(lines[0].starts_with("swarm "));
        assert!(lines.len() >= 3);
    }

    #[test]
    fn cancel_all_tasks_cancels_non_terminal() {
        let _db = TestDb::new();
        let a = register_task("a").expect("a");
        let b = register_task("b").expect("b");
        update_status(&a, &TaskStatus::Running, None).expect("run");
        update_status(&b, &TaskStatus::Pending, None).expect("pend");
        let done = register_task("done").expect("d");
        update_status(&done, &TaskStatus::Done, Some("ok")).expect("done");
        let n = cancel_all_tasks().expect("cancel all");
        assert_eq!(n, 2);
        assert_eq!(
            get_task(&a).expect("g").expect("f").status,
            TaskStatus::Cancelled
        );
        assert_eq!(
            get_task(&b).expect("g").expect("f").status,
            TaskStatus::Cancelled
        );
        assert_eq!(
            get_task(&done).expect("g").expect("f").status,
            TaskStatus::Done
        );
    }

    #[test]
    fn task_to_json_includes_status_and_result() {
        let _db = TestDb::new();
        let id = register_task("json me").expect("reg");
        update_status(&id, &TaskStatus::Done, Some("hello")).expect("done");
        let task = get_task(&id).expect("g").expect("f");
        let v = task_to_json(&task);
        assert_eq!(v["id"], id);
        assert_eq!(v["status"], "done");
        assert_eq!(v["result"], "hello");
        assert_eq!(v["terminal"], true);
    }

    #[test]
    fn gc_dry_run_does_not_mutate() {
        let _db = TestDb::new();
        let id = register_task("orphan").expect("reg");
        update_status(&id, &TaskStatus::Running, None).expect("run");
        {
            let conn = open_db().expect("db");
            conn.execute(
                "UPDATE tasks SET created_ts = ?1 WHERE id = ?2",
                params![now_ts() - 10_000, id],
            )
            .expect("backdate");
        }
        let report = gc(&GcOptions {
            stale_secs: 60,
            keep_terminal: None,
            older_than_secs: None,
            dry_run: true,
        })
        .expect("dry");
        assert_eq!(report.reaped.len(), 1);
        assert_eq!(
            get_task(&id).expect("g").expect("f").status,
            TaskStatus::Running
        );
    }
}
