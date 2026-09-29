//! Format-compatible Witness adapter. Does not link `witness-core`.
//!
//! Records use Witness field names: `epistemic_type` (`observed`|`inferred`),
//! `cid`, and `premises`. Generated rows are refused. This module does not
//! copy Witness source.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection};

use crate::{EpistemicClass, EvidenceRecord, EvidenceSink, SinkError};

const BUSY_BUDGET: Duration = Duration::from_millis(5000);

struct Inner {
    conn: Connection,
    events: std::fs::File,
}

/// Second evidence store. Same [`EvidenceSink`] contract as the lease JSONL.
pub struct WitnessSink {
    inner: Mutex<Inner>,
    jsonl: PathBuf,
}

impl WitnessSink {
    /// Open `db` plus a sibling `.jsonl`. Creates parents. WAL and busy timeout.
    pub fn open(db: &Path) -> Result<Self, SinkError> {
        if let Some(parent) = db.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|_| SinkError::Unavailable)?;
            }
        }
        let jsonl = jsonl_beside(db);
        if let Some(parent) = jsonl.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|_| SinkError::Unavailable)?;
            }
        }
        let conn = Connection::open(db).map_err(|_| SinkError::Unavailable)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             CREATE TABLE IF NOT EXISTS witness_nodes (
               seq INTEGER PRIMARY KEY,
               cid TEXT NOT NULL UNIQUE,
               epistemic_type TEXT NOT NULL,
               kind TEXT NOT NULL,
               payload TEXT NOT NULL,
               premises TEXT NOT NULL,
               ts INTEGER NOT NULL
             );",
        )
        .map_err(|_| SinkError::Unavailable)?;
        let events = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&jsonl)
            .map_err(|_| SinkError::Unavailable)?;
        Ok(Self {
            inner: Mutex::new(Inner { conn, events }),
            jsonl,
        })
    }

    /// JSONL path written beside the sqlite file.
    pub fn jsonl_path(&self) -> &Path {
        &self.jsonl
    }
}

impl EvidenceSink for WitnessSink {
    fn append(&self, record: &EvidenceRecord) -> Result<(), SinkError> {
        let epistemic_type = match record.class {
            EpistemicClass::Observed => "observed",
            EpistemicClass::Inferred => "inferred",
            EpistemicClass::Generated => return Err(SinkError::Prose),
        };
        if record.class == EpistemicClass::Inferred && record.premises.is_empty() {
            return Err(SinkError::PremisesRequired);
        }
        let payload = serde_json::to_string(&record.payload).map_err(|_| SinkError::Unavailable)?;
        let premises =
            serde_json::to_string(&record.premises).map_err(|_| SinkError::Unavailable)?;
        let line = serde_json::json!({
            "epistemic_type": epistemic_type,
            "class": epistemic_type,
            "cid": record.cid,
            "premises": record.premises,
            "kind": record.kind,
            "payload": record.payload,
        });
        let encoded = serde_json::to_string(&line).map_err(|_| SinkError::Unavailable)?;
        let mut g = self.inner.lock().map_err(|_| SinkError::Unavailable)?;
        let start = Instant::now();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        loop {
            match g.conn.execute(
                "INSERT INTO witness_nodes (seq, cid, epistemic_type, kind, payload, premises, ts)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    record.seq as i64,
                    record.cid,
                    epistemic_type,
                    record.kind,
                    payload,
                    premises,
                    ts
                ],
            ) {
                Ok(_) => break,
                Err(e) if sqlite_busy(&e) && start.elapsed() < BUSY_BUDGET => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return Err(SinkError::Unavailable),
            }
        }
        g.events
            .write_all(encoded.as_bytes())
            .map_err(|_| SinkError::Unavailable)?;
        g.events
            .write_all(b"\n")
            .map_err(|_| SinkError::Unavailable)?;
        g.events.flush().map_err(|_| SinkError::Unavailable)?;
        Ok(())
    }
}

fn jsonl_beside(db: &Path) -> PathBuf {
    let mut path = db.to_path_buf();
    path.set_extension("jsonl");
    path
}

fn sqlite_busy(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(e, _) => matches!(
            e.code,
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EvidenceRecord;
    use serde_json::json;

    fn record(class: EpistemicClass, premises: Vec<String>) -> EvidenceRecord {
        EvidenceRecord {
            cid: "sha256:abc".into(),
            payload_sha256: "sha256:def".into(),
            seq: 1,
            class,
            kind: "tool_attempt".into(),
            payload: json!({"agent_id": "A", "tool": "shell"}),
            premises,
        }
    }

    #[test]
    fn witness_refuses_generated() {
        let dir = tempfile::tempdir().unwrap();
        let sink = WitnessSink::open(&dir.path().join("witness.db")).unwrap();
        let err = sink
            .append(&record(EpistemicClass::Generated, Vec::new()))
            .unwrap_err();
        assert_eq!(err, SinkError::Prose);
        let text = fs::read_to_string(sink.jsonl_path()).unwrap_or_default();
        assert!(!text.contains("generated"));
    }
}
