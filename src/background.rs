//! Background run management: `harness run-bg` spawns agents in detached processes.
//!
//! Each background run gets:
//!   ~/.harness/runs/<id>/status.json  — live status (queued/running/done/failed)
//!   ~/.harness/runs/<id>/output.log   — streamed output
//!   ~/.harness/runs/<id>/prompt.txt   — original prompt
//!
//! The TUI reads these files for the background-runs panel.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Queued,
    Running,
    Done,
    Failed,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunStatus::Queued => write!(f, "queued"),
            RunStatus::Running => write!(f, "running"),
            RunStatus::Done => write!(f, "done"),
            RunStatus::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundRun {
    pub id: String,
    pub prompt: String,
    pub status: RunStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub pid: Option<u32>,
}

/// Directory holding all background run state.
pub fn runs_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".harness")
        .join("runs")
}

/// Spawn a background run using the current harness binary.
/// Returns the run ID.
pub fn spawn(prompt: &str) -> Result<String> {
    use std::process::Stdio;

    let id = crate::checkpoint::short_id();
    let dir = runs_dir().join(&id);
    std::fs::create_dir_all(&dir)?;

    let prompt_file = dir.join("prompt.txt");
    std::fs::write(&prompt_file, prompt)?;

    let log_file = dir.join("output.log");
    let log = std::fs::File::create(&log_file)?;
    let log_err = log.try_clone()?;

    let harness_bin = std::env::current_exe()?;
    let child = std::process::Command::new(&harness_bin)
        .arg("run")
        .arg(prompt)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()?;

    let run = BackgroundRun {
        id: id.clone(),
        prompt: prompt.to_string(),
        status: RunStatus::Running,
        started_at: chrono::Utc::now().to_rfc3339(),
        finished_at: None,
        pid: Some(child.id()),
    };

    write_status(&dir, &run)?;
    Ok(id)
}

/// List recent background runs (most recent first, up to `limit`).
pub fn list(limit: usize) -> Result<Vec<BackgroundRun>> {
    let base = runs_dir();
    if !base.exists() {
        return Ok(vec![]);
    }

    let mut runs: Vec<(std::time::SystemTime, BackgroundRun)> = Vec::new();

    for entry in std::fs::read_dir(&base)? {
        let entry = entry?;
        let status_file = entry.path().join("status.json");
        if !status_file.exists() {
            continue;
        }
        if let Ok(data) = std::fs::read_to_string(&status_file) {
            if let Ok(run) = serde_json::from_str::<BackgroundRun>(&data) {
                let mtime = entry.metadata().ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(std::time::UNIX_EPOCH);
                runs.push((mtime, run));
            }
        }
    }

    runs.sort_by_key(|b| Reverse(b.0));
    Ok(runs.into_iter().take(limit).map(|(_, r)| r).collect())
}

/// Read the tail of the output log for a run.
#[allow(dead_code)]
pub fn tail_output(run_id: &str, lines: usize) -> Result<Vec<String>> {
    let log_file = runs_dir().join(run_id).join("output.log");
    let content = std::fs::read_to_string(log_file)?;
    let result: Vec<String> = content.lines()
        .rev()
        .take(lines)
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Ok(result)
}

fn write_status(dir: &std::path::Path, run: &BackgroundRun) -> Result<()> {
    let data = serde_json::to_string_pretty(run)?;
    std::fs::write(dir.join("status.json"), data)?;
    Ok(())
}
