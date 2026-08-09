//! Cost tracking database — persists per-turn token usage to SQLite.
//!
//! Schema: `~/.harness/cost.db`
//! Table: `usage (id, session_id, project, provider, model, ts, in_tok, cached_in, out_tok, native_calls, usd)`
//!
//! Used by:
//! - `harness cost today/week/month/all/by-project/by-model/watch`
//! - Budget thresholds with TUI warnings and desktop notifications

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Arc;

/// A single usage record.
#[derive(Debug, Clone)]
pub struct UsageRow {
    pub session_id: String,
    pub project: String,
    pub provider: String,
    pub model: String,
    /// Unix timestamp (seconds).
    pub ts: i64,
    pub in_tok: u32,
    pub cached_in: u32,
    pub out_tok: u32,
    pub native_calls: u32,
    pub usd: f64,
}

/// Thread-safe cost database.
#[derive(Clone)]
pub struct CostDb {
    conn: Arc<Mutex<Connection>>,
}

impl CostDb {
    /// Open (or create) the cost database at `~/.harness/cost.db`.
    pub fn open() -> Result<Self> {
        Self::open_at(db_path())
    }

    /// Open (or create) the cost database at an explicit path (tests / tooling).
    pub fn open_at(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(&path)
            .with_context(|| format!("opening cost.db at {}", path.display()))?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS usage (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id  TEXT NOT NULL,
                project     TEXT NOT NULL DEFAULT '',
                provider    TEXT NOT NULL DEFAULT '',
                model       TEXT NOT NULL DEFAULT '',
                ts          INTEGER NOT NULL DEFAULT 0,
                in_tok      INTEGER NOT NULL DEFAULT 0,
                cached_in   INTEGER NOT NULL DEFAULT 0,
                out_tok     INTEGER NOT NULL DEFAULT 0,
                native_calls INTEGER NOT NULL DEFAULT 0,
                usd         REAL NOT NULL DEFAULT 0.0
            );
            CREATE INDEX IF NOT EXISTS idx_usage_ts ON usage(ts);
            CREATE INDEX IF NOT EXISTS idx_usage_session ON usage(session_id);
        ",
        )?;
        Ok(())
    }

    /// Record one turn's usage.
    pub fn record(&self, row: &UsageRow) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO usage (session_id, project, provider, model, ts, in_tok, cached_in, out_tok, native_calls, usd)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                row.session_id, row.project, row.provider, row.model,
                row.ts, row.in_tok, row.cached_in, row.out_tok, row.native_calls, row.usd
            ],
        )?;
        Ok(())
    }

    /// Total cost in USD for a given time window (seconds since epoch).
    pub fn total_usd_since(&self, since_ts: i64) -> Result<f64> {
        let conn = self.conn.lock();
        let usd: f64 = conn.query_row(
            "SELECT COALESCE(SUM(usd), 0.0) FROM usage WHERE ts >= ?1",
            params![since_ts],
            |row| row.get(0),
        )?;
        Ok(usd)
    }

    /// Cost per model since `ts`.
    pub fn by_model_since(&self, since_ts: i64) -> Result<Vec<(String, f64)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT model, SUM(usd) as total FROM usage WHERE ts >= ?1
             GROUP BY model ORDER BY total DESC",
        )?;
        let rows = stmt.query_map(params![since_ts], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Cost per project since `ts`.
    pub fn by_project_since(&self, since_ts: i64) -> Result<Vec<(String, f64)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT project, SUM(usd) as total FROM usage WHERE ts >= ?1
             GROUP BY project ORDER BY total DESC",
        )?;
        let rows = stmt.query_map(params![since_ts], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Most recent N rows, newest first.
    pub fn recent(&self, limit: u32) -> Result<Vec<UsageRow>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT session_id, project, provider, model, ts, in_tok, cached_in, out_tok, native_calls, usd
             FROM usage ORDER BY ts DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(UsageRow {
                session_id: row.get(0)?,
                project: row.get(1)?,
                provider: row.get(2)?,
                model: row.get(3)?,
                ts: row.get(4)?,
                in_tok: row.get(5)?,
                cached_in: row.get(6)?,
                out_tok: row.get(7)?,
                native_calls: row.get(8)?,
                usd: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn db_path() -> PathBuf {
    let dir = dirs::home_dir().unwrap_or_default().join(".harness");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("cost.db")
}

/// Returns Unix timestamp for N days ago.
pub fn days_ago(n: u64) -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    now - (n * 86400) as i64
}

/// Format USD value for display.
pub fn format_usd(usd: f64) -> String {
    if usd < 0.01 {
        format!("${:.4}", usd)
    } else if usd < 1.0 {
        format!("${:.3}", usd)
    } else {
        format!("${:.2}", usd)
    }
}

/// Check if daily/monthly budget thresholds are exceeded.
/// Returns (daily_pct, monthly_pct) where 100 = 100%.
pub fn check_budget(
    db: &CostDb,
    daily_usd: Option<f64>,
    monthly_usd: Option<f64>,
) -> (Option<f64>, Option<f64>) {
    let daily_pct = daily_usd.and_then(|limit| {
        db.total_usd_since(days_ago(1))
            .ok()
            .map(|spent| spent / limit * 100.0)
    });
    let monthly_pct = monthly_usd.and_then(|limit| {
        db.total_usd_since(days_ago(30))
            .ok()
            .map(|spent| spent / limit * 100.0)
    });
    (daily_pct, monthly_pct)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_row(session: &str, project: &str, model: &str, ts: i64, usd: f64) -> UsageRow {
        UsageRow {
            session_id: session.into(),
            project: project.into(),
            provider: "anthropic".into(),
            model: model.into(),
            ts,
            in_tok: 100,
            cached_in: 0,
            out_tok: 50,
            native_calls: 0,
            usd,
        }
    }

    #[test]
    fn record_and_query_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("cost.db");
        let db = CostDb::open_at(path).expect("open");
        let row = sample_row("abc", "harness", "claude-sonnet-4-6", 1_700_000_000, 0.01);
        db.record(&row).expect("record");
        let total = db.total_usd_since(0).expect("total");
        assert!((total - 0.01).abs() < f64::EPSILON);
        let recent = db.recent(1).expect("recent");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].session_id, "abc");
    }

    #[test]
    fn format_usd_precision_bands() {
        assert_eq!(format_usd(0.0001), "$0.0001");
        assert_eq!(format_usd(0.009), "$0.0090");
        assert_eq!(format_usd(0.05), "$0.050");
        assert_eq!(format_usd(0.999), "$0.999");
        assert_eq!(format_usd(1.2), "$1.20");
        assert_eq!(format_usd(12.345), "$12.35");
    }

    #[test]
    fn days_ago_is_roughly_n_days_before_now() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let d0 = days_ago(0);
        let d1 = days_ago(1);
        let d7 = days_ago(7);
        assert!((now - d0).abs() < 2);
        assert!((now - 86400 - d1).abs() < 2);
        assert!((now - 7 * 86400 - d7).abs() < 2);
    }

    #[test]
    fn by_model_and_project_aggregate() {
        let dir = TempDir::new().expect("tempdir");
        let db = CostDb::open_at(dir.path().join("cost.db")).expect("open");
        db.record(&sample_row("s1", "harness", "grok-4.5", 100, 0.10))
            .unwrap();
        db.record(&sample_row("s2", "harness", "grok-4.5", 101, 0.05))
            .unwrap();
        db.record(&sample_row("s3", "other", "claude", 102, 0.20))
            .unwrap();

        let by_model = db.by_model_since(0).expect("by_model");
        let grok = by_model
            .iter()
            .find(|(m, _)| m == "grok-4.5")
            .expect("grok");
        assert!((grok.1 - 0.15).abs() < 1e-9);
        let claude = by_model
            .iter()
            .find(|(m, _)| m == "claude")
            .expect("claude");
        assert!((claude.1 - 0.20).abs() < 1e-9);

        let by_proj = db.by_project_since(0).expect("by_project");
        let h = by_proj
            .iter()
            .find(|(p, _)| p == "harness")
            .expect("harness");
        assert!((h.1 - 0.15).abs() < 1e-9);
    }

    #[test]
    fn check_budget_none_and_percent() {
        let dir = TempDir::new().expect("tempdir");
        let db = CostDb::open_at(dir.path().join("cost.db")).expect("open");
        // fresh spend so daily window includes it
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        db.record(&sample_row("s", "p", "m", now, 0.50)).unwrap();

        let (d, m) = check_budget(&db, None, None);
        assert!(d.is_none() && m.is_none());

        let (d, m) = check_budget(&db, Some(1.0), Some(10.0));
        let d = d.expect("daily");
        let m = m.expect("monthly");
        assert!((d - 50.0).abs() < 1.0, "daily pct ~50, got {d}");
        assert!((m - 5.0).abs() < 1.0, "monthly pct ~5, got {m}");
    }

    #[test]
    fn total_usd_since_filters_old_rows() {
        let dir = TempDir::new().expect("tempdir");
        let db = CostDb::open_at(dir.path().join("cost.db")).expect("open");
        db.record(&sample_row("old", "p", "m", 1_000, 1.0)).unwrap();
        db.record(&sample_row("new", "p", "m", 2_000, 0.25))
            .unwrap();
        let total = db.total_usd_since(1_500).expect("total");
        assert!((total - 0.25).abs() < 1e-9);
        assert_eq!(db.recent(10).unwrap().len(), 2);
        assert_eq!(db.recent(1).unwrap().len(), 1);
    }
}
