#![allow(dead_code)]
//! Inline diff reviewer: shows hunk-by-hunk diffs with per-hunk accept/reject/edit in the TUI.
//!
//! Usage:
//! - Agent writes to `staging_buffer` instead of directly to disk in plan mode
//! - The TUI overlay shows colored diffs (red = removed, green = added)
//! - User presses y/n/e per hunk, or Y/N for entire file
//! - Auto-trust patterns skip confirmation for known-safe patterns

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkDecision {
    Accept,
    Reject,
    Pending,
}

#[derive(Debug, Clone)]
pub struct DiffHunk {
    /// Context lines (shown but not changed).
    pub header: String,
    /// Lines in this hunk: ('+'/'-'/' ', content).
    pub lines: Vec<(char, String)>,
    pub decision: HunkDecision,
}

#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: PathBuf,
    /// Original file content (None = new file).
    pub original: Option<String>,
    /// Proposed content after applying all accepted hunks.
    pub proposed: String,
    pub hunks: Vec<DiffHunk>,
    /// Whether the entire file has been decided.
    pub file_decision: Option<bool>,
}

/// The staging buffer holds pending writes before user review.
#[derive(Debug, Default)]
pub struct StagingBuffer {
    pub entries: HashMap<PathBuf, FileDiff>,
}

impl StagingBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stage a write: compute diff vs current file content.
    pub fn stage_write(&mut self, path: impl AsRef<Path>, new_content: &str) {
        let path = path.as_ref().to_path_buf();
        let original = std::fs::read_to_string(&path).ok();
        let hunks = if let Some(ref orig) = original {
            compute_hunks(orig, new_content)
        } else {
            // New file: single hunk with all additions
            let lines: Vec<(char, String)> =
                new_content.lines().map(|l| ('+', l.to_string())).collect();
            vec![DiffHunk {
                header: "@@ -0,0 +1 @@ (new file)".to_string(),
                lines,
                decision: HunkDecision::Pending,
            }]
        };

        let diff = FileDiff {
            path: path.clone(),
            original,
            proposed: new_content.to_string(),
            hunks,
            file_decision: None,
        };
        self.entries.insert(path, diff);
    }

    /// Apply all accepted hunks to disk.
    pub fn commit(&self) -> Vec<Result<PathBuf>> {
        self.entries
            .values()
            .map(|diff| {
                // If file-level decision: apply or skip entirely.
                if let Some(accept) = diff.file_decision {
                    if accept {
                        std::fs::write(&diff.path, &diff.proposed)?;
                        return Ok(diff.path.clone());
                    } else {
                        return Ok(diff.path.clone()); // rejected, no change
                    }
                }
                // Apply accepted hunks only (reconstruct file)
                let original = diff.original.as_deref().unwrap_or("");
                let result = apply_accepted_hunks(original, &diff.hunks);
                if result != original {
                    std::fs::write(&diff.path, &result)?;
                }
                Ok(diff.path.clone())
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn pending_count(&self) -> usize {
        self.entries
            .values()
            .flat_map(|d| d.hunks.iter())
            .filter(|h| h.decision == HunkDecision::Pending)
            .count()
    }
}

// ── Diff computation ──────────────────────────────────────────────────────────

/// Compute unified diff hunks between two strings.
pub fn compute_hunks(original: &str, proposed: &str) -> Vec<DiffHunk> {
    // Simple line-by-line diff using LCS
    let orig_lines: Vec<&str> = original.lines().collect();
    let new_lines: Vec<&str> = proposed.lines().collect();

    let lcs = lcs_diff(&orig_lines, &new_lines);
    let edits = diff_to_edits(&orig_lines, &new_lines, &lcs);

    group_edits_into_hunks(&orig_lines, &new_lines, edits, 3)
}

/// LCS-based diff: returns longest-common-subsequence indices.
fn lcs_diff(a: &[&str], b: &[&str]) -> Vec<Vec<usize>> {
    let m = a.len();
    let n = b.len();
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if a[i - 1] == b[j - 1] {
                dp[i - 1][j - 1] + 1
            } else {
                dp[i - 1][j].max(dp[i][j - 1])
            };
        }
    }
    dp
}

#[derive(Debug, Clone, PartialEq)]
enum EditOp {
    Keep(usize, usize),
    Delete(usize),
    Insert(usize),
}

/// In-progress hunk: context/change lines plus (orig_start, new_start).
type PendingHunk = (Vec<(char, String)>, usize, usize);

fn diff_to_edits(a: &[&str], b: &[&str], dp: &[Vec<usize>]) -> Vec<EditOp> {
    let mut ops = Vec::new();
    let mut i = a.len();
    let mut j = b.len();
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
            ops.push(EditOp::Keep(i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(EditOp::Insert(j - 1));
            j -= 1;
        } else {
            ops.push(EditOp::Delete(i - 1));
            i -= 1;
        }
    }
    ops.reverse();
    ops
}

fn group_edits_into_hunks(
    orig: &[&str],
    new: &[&str],
    edits: Vec<EditOp>,
    context: usize,
) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut current_hunk: Option<PendingHunk> = None;
    let mut last_change_idx = 0usize;

    for (edit_idx, edit) in edits.iter().enumerate() {
        match edit {
            EditOp::Keep(oi, _ni) => {
                if let Some(ref mut hunk) = current_hunk {
                    let dist_to_next_change = edits[edit_idx..]
                        .iter()
                        .position(|e| !matches!(e, EditOp::Keep(..)))
                        .unwrap_or(usize::MAX);
                    hunk.0.push((' ', orig[*oi].to_string()));
                    if dist_to_next_change > context * 2 {
                        // Flush hunk
                        let (lines, orig_start, new_start) = current_hunk.take().unwrap();
                        let header = format!(
                            "@@ -{},{} +{},{} @@",
                            orig_start + 1,
                            lines.len(),
                            new_start + 1,
                            lines.len()
                        );
                        hunks.push(DiffHunk {
                            header,
                            lines,
                            decision: HunkDecision::Pending,
                        });
                    }
                }
            }
            EditOp::Delete(oi) => {
                if current_hunk.is_none() {
                    let start = (*oi).saturating_sub(context);
                    let mut lines = Vec::new();
                    for line in orig.iter().copied().take(*oi).skip(start) {
                        lines.push((' ', line.to_string()));
                    }
                    current_hunk = Some((lines, start, start));
                }
                if let Some(ref mut hunk) = current_hunk {
                    hunk.0.push(('-', orig[*oi].to_string()));
                    last_change_idx = edit_idx;
                }
            }
            EditOp::Insert(ni) => {
                if current_hunk.is_none() {
                    let start_orig = if *ni > 0 {
                        ni.saturating_sub(context)
                    } else {
                        0
                    };
                    let mut lines = Vec::new();
                    let end = (*ni).min(new.len());
                    for line in new.iter().copied().take(end).skip(start_orig) {
                        lines.push((' ', line.to_string()));
                    }
                    current_hunk = Some((lines, start_orig, start_orig));
                }
                if let Some(ref mut hunk) = current_hunk {
                    hunk.0.push(('+', new[*ni].to_string()));
                    last_change_idx = edit_idx;
                }
            }
        }
    }

    if let Some((lines, orig_start, new_start)) = current_hunk {
        let header = format!(
            "@@ -{},{} +{},{} @@",
            orig_start + 1,
            lines.len(),
            new_start + 1,
            lines.len()
        );
        hunks.push(DiffHunk {
            header,
            lines,
            decision: HunkDecision::Pending,
        });
    }

    let _ = last_change_idx;
    hunks
}

/// Reconstruct file content by applying only accepted hunks.
fn apply_accepted_hunks(original: &str, hunks: &[DiffHunk]) -> String {
    // Simple strategy: start with proposed changes for accepted hunks,
    // original for rejected hunks.
    // For a real implementation, this would use patch application.
    // For now: if any hunk is accepted, apply full diff; if all rejected, return original.
    let all_rejected = hunks.iter().all(|h| h.decision == HunkDecision::Reject);
    let all_accepted = hunks.iter().all(|h| h.decision == HunkDecision::Accept);

    if all_rejected {
        return original.to_string();
    }
    if all_accepted {
        // The full proposed content is stored in FileDiff, but we only have hunks here.
        // Reconstruct from hunk lines.
    }

    // Reconstruct from hunk lines (accepted = apply changes, rejected = use original)
    let mut result = original.to_string();
    // For simplicity, apply all accepted hunks by filtering lines
    let accepted_lines: Vec<String> = hunks
        .iter()
        .filter(|h| h.decision == HunkDecision::Accept || h.decision == HunkDecision::Pending)
        .flat_map(|h| {
            h.lines
                .iter()
                .filter(|(op, _)| *op != '-')
                .map(|(_, l)| l.clone())
        })
        .collect();

    if !accepted_lines.is_empty() {
        result = accepted_lines.join("\n");
    }
    result
}

// ── Auto-trust patterns ───────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct AutoTrustPatterns {
    /// Glob patterns for paths that are always auto-accepted.
    pub always_accept: Vec<String>,
    /// Glob patterns for paths that are always auto-rejected.
    pub always_reject: Vec<String>,
}

impl AutoTrustPatterns {
    pub fn load() -> Self {
        let path = dirs::home_dir()
            .unwrap_or_default()
            .join(".harness/diff-trust.toml");
        if !path.exists() {
            return Self::default();
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        let Ok(val) = text.parse::<toml::Value>() else {
            return Self::default();
        };

        let get_list = |key: &str| -> Vec<String> {
            val.get(key)
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };

        Self {
            always_accept: get_list("always_accept"),
            always_reject: get_list("always_reject"),
        }
    }

    pub fn should_auto_accept(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        self.always_accept
            .iter()
            .any(|pat| glob_match(pat, &path_str))
    }

    pub fn should_auto_reject(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        self.always_reject
            .iter()
            .any(|pat| glob_match(pat, &path_str))
    }
}

fn glob_match(pattern: &str, path: &str) -> bool {
    // Simple glob: * = any chars, ** = path sep too
    let re_pat = pattern
        .replace('.', "\\.")
        .replace("**", "\x00")
        .replace('*', "[^/]*")
        .replace('\x00', ".*");
    regex::Regex::new(&format!("^{re_pat}$"))
        .map(|r| r.is_match(path))
        .unwrap_or(false)
}

// ── TUI overlay rendering helpers ─────────────────────────────────────────────

/// Format a hunk for display in the TUI confirm overlay.
pub fn format_hunk_for_display(hunk: &DiffHunk) -> Vec<(char, String)> {
    let mut lines = vec![(' ', hunk.header.clone())];
    lines.extend(hunk.lines.clone());
    lines
}

/// Render a diff summary: X files, Y hunks pending.
pub fn render_staging_summary(buf: &StagingBuffer) -> String {
    let file_count = buf.entries.len();
    let hunk_count = buf.entries.values().flat_map(|d| d.hunks.iter()).count();
    let pending = buf.pending_count();
    format!("{file_count} file(s), {hunk_count} hunk(s), {pending} pending")
}
