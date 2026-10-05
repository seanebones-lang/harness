//! ObservationPack: handles + inline threshold for large tool outputs.
use anyhow::Result;
use chrono::{DateTime, Utc};
use harness_memory::SessionId;
use harness_provider_core::Message;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

/// On-disk observation record.
/// On-disk observation record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Observation {
    /// Handle referencing this observation.
    pub handle: ObservationHandle,
    /// Name of the tool that produced this observation.
    pub tool_name: String,
    /// Timestamp when the observation was created.
    pub timestamp: DateTime<Utc>,
    /// Size of the full observation in bytes.
    pub bytes: usize,
    /// SHA-256 hash of the full content.
    pub sha256: String,
    /// Exit code if the tool produced one.
    pub exit_code: Option<i32>,
    /// First N lines (inline_threshold_lines) for inline display.
    pub preview_lines: Vec<String>,
    /// Path to the full body on disk.
    pub body_path: PathBuf,
}

/// Compact handle the model sees in context.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ObservationHandle {
    /// Session identifier.
    pub session_id: SessionId,
    /// Sequence number within the session.
    pub seq: u64,
}

impl ObservationHandle {
    /// Creates a new observation handle.
    pub fn new(session_id: SessionId, seq: u64) -> Self {
        Self { session_id, seq }
    }
    /// Returns the obs:// URI for this handle.
    pub fn uri(&self) -> String {
        format!("obs://{}/{}", self.session_id, self.seq)
    }
    /// Parses an obs:// URI into a handle.
    pub fn from_uri(uri: &str) -> Option<Self> {
        let rest = uri.strip_prefix("obs://")?;
        let (sid, seq_str) = rest.split_once('/')?;
        Some(Self { session_id: sid.into(), seq: seq_str.parse().ok()? })
    }
}

/// Observation store — one per session, persisted alongside session JSON.
#[derive(Debug)]
pub struct ObservationStore {
    inner: Arc<Mutex<Vec<Observation>>>,
    session_id: SessionId,
    obs_dir: PathBuf,
    next_seq: Arc<Mutex<u64>>,
    inline_threshold_bytes: usize,
    inline_threshold_lines: usize,
}

impl ObservationStore {
    /// Creates a new observation store for a session.
    pub fn new(session_id: SessionId, harness_home: &Path) -> Result<Self> {
        let obs_dir = harness_home.join("obs").join(&session_id);
        std::fs::create_dir_all(&obs_dir)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(Vec::new())),
            session_id,
            obs_dir,
            next_seq: Arc::new(Mutex::new(0)),
            inline_threshold_bytes: 2 * 1024,
            inline_threshold_lines: 40,
        })
    }

    /// Pack a tool result. Returns (Message to push, ObservationHandle).
    pub async fn pack(
        &self,
        tool_name: &str,
        result: String,
        exit_code: Option<i32>,
    ) -> Result<(Message, ObservationHandle)> {
        let bytes = result.len();
        let sha256 = format!("{:x}", Sha256::digest(result.as_bytes()));
        let lines: Vec<&str> = result.lines().collect();
        let preview_lines: Vec<String> = lines.iter().take(self.inline_threshold_lines).map(|s| s.to_string()).collect();

        let seq = {
            let mut n = self.next_seq.lock().await;
            *n += 1;
            *n
        };
        let handle = ObservationHandle::new(self.session_id.clone(), seq);
        let body_path = self.obs_dir.join(format!("{}.txt", seq));

        tokio::fs::write(&body_path, &result).await?;

        let obs = Observation {
            handle: handle.clone(),
            tool_name: tool_name.into(),
            timestamp: Utc::now(),
            bytes,
            sha256,
            exit_code,
            preview_lines: preview_lines.clone(),
            body_path,
        };

        self.inner.lock().await.push(obs);

        let is_small = bytes <= self.inline_threshold_bytes && lines.len() <= self.inline_threshold_lines;
        let msg = if is_small {
            Message::tool_result(&handle.uri(), result)
        } else {
            let preview = format!(
                "[{}] exit={} {} bytes\npreview:\n{}",
                tool_name,
                exit_code.unwrap_or(-1),
                bytes,
                preview_lines.join("\n")
            );
            Message::tool_result(&handle.uri(), preview)
        };

        Ok((msg, handle))
    }

    /// Page an observation by line range.
    pub async fn read_range(&self, handle: &ObservationHandle, start: usize, end: Option<usize>) -> Result<String> {
        let obs = self.inner.lock().await.iter().find(|o| o.handle == *handle).cloned();
        let obs = obs.ok_or_else(|| anyhow::anyhow!("observation not found: {}", handle.uri()))?;
        let content = tokio::fs::read_to_string(&obs.body_path).await?;
        let lines: Vec<&str> = content.lines().collect();
        let end = end.unwrap_or(lines.len()).min(lines.len());
        Ok(lines[start..end].join("\n"))
    }

    /// Regex search within one observation.
    pub async fn grep(&self, handle: &ObservationHandle, pattern: &str) -> Result<Vec<(usize, String)>> {
        let obs = self.inner.lock().await.iter().find(|o| o.handle == *handle).cloned();
        let obs = obs.ok_or_else(|| anyhow::anyhow!("observation not found: {}", handle.uri()))?;
        let content = tokio::fs::read_to_string(&obs.body_path).await?;
        let re = Regex::new(pattern)?;
        Ok(content.lines().enumerate().filter_map(|(i, l)| re.find(l).map(|_| (i + 1, l.into()))).collect())
    }

    /// For resume: reload all observations from disk.
    pub async fn reload(&self) -> Result<()> {
        let mut seqs = Vec::new();
        for entry in std::fs::read_dir(&self.obs_dir)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                if let Some(seq_str) = name.strip_suffix(".txt") {
                    if let Ok(seq) = seq_str.parse::<u64>() {
                        seqs.push(seq);
                    }
                }
            }
        }
        seqs.sort();
        let mut inner = self.inner.lock().await;
        inner.clear();
        for seq in &seqs {
            let handle = ObservationHandle::new(self.session_id.clone(), *seq);
            let body_path = self.obs_dir.join(format!("{}.txt", seq));
            let content = tokio::fs::read_to_string(&body_path).await?;
            let bytes = content.len();
            let sha256 = format!("{:x}", Sha256::digest(content.as_bytes()));
            let lines: Vec<&str> = content.lines().collect();
            let preview_lines: Vec<String> = lines.iter().take(self.inline_threshold_lines).map(|s| s.to_string()).collect();
            inner.push(Observation {
                handle,
                tool_name: "unknown".into(),
                timestamp: Utc::now(),
                bytes,
                sha256,
                exit_code: None,
                preview_lines,
                body_path,
            });
        }
        let max_seq = seqs.into_iter().max().unwrap_or(0);
        *self.next_seq.lock().await = max_seq;
        Ok(())
    }
}
