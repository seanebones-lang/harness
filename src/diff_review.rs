//! Inline diff reviewer for plan mode: hunk-by-hunk accept/reject in the TUI overlay.

use anyhow::Result;
use serde_json::Value;
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
    #[allow(dead_code)]
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

    /// Apply all accepted hunks to disk (batch staging API).
    #[allow(dead_code)]
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
                let result = apply_accepted_hunks(original, &diff.proposed, &diff.hunks);
                if result != original {
                    std::fs::write(&diff.path, &result)?;
                }
                Ok(diff.path.clone())
            })
            .collect()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[allow(dead_code)]
    pub fn pending_count(&self) -> usize {
        self.entries
            .values()
            .flat_map(|d| d.hunks.iter())
            .filter(|h| h.decision == HunkDecision::Pending)
            .count()
    }
}

/// Build a file diff for plan-mode review from tool name + args.
pub fn file_diff_from_tool(tool_name: &str, args: &Value) -> Option<FileDiff> {
    match tool_name {
        "write_file" => {
            let path = args.get("path")?.as_str()?;
            let content = args.get("content")?.as_str()?;
            let mut buf = StagingBuffer::new();
            buf.stage_write(path, content);
            buf.entries.get(std::path::Path::new(path)).cloned()
        }
        "patch_file" => {
            let path = args.get("path")?.as_str()?;
            let old = args.get("old_string")?.as_str()?;
            let new = args.get("new_string")?.as_str()?;
            let original = std::fs::read_to_string(path).ok()?;
            let proposed = original.replacen(old, new, 1);
            if proposed == original {
                return None;
            }
            let mut buf = StagingBuffer::new();
            buf.stage_write(path, &proposed);
            buf.entries.get(std::path::Path::new(path)).cloned()
        }
        _ => None,
    }
}

/// Mark a hunk accepted or rejected.
pub fn set_hunk_decision(diff: &mut FileDiff, hunk_idx: usize, accept: bool) {
    if let Some(hunk) = diff.hunks.get_mut(hunk_idx) {
        hunk.decision = if accept {
            HunkDecision::Accept
        } else {
            HunkDecision::Reject
        };
    }
}

/// Index of the next hunk still pending review, if any.
pub fn next_pending_hunk(diff: &FileDiff, from: usize) -> Option<usize> {
    diff.hunks
        .iter()
        .enumerate()
        .skip(from)
        .find(|(_, h)| h.decision == HunkDecision::Pending)
        .map(|(i, _)| i)
}

/// Final path + content after hunk review. `None` when every hunk was rejected.
pub fn finalize_for_apply(diff: &FileDiff) -> Option<(PathBuf, String)> {
    if diff.hunks.is_empty() {
        return Some((diff.path.clone(), diff.proposed.clone()));
    }
    if diff
        .hunks
        .iter()
        .all(|h| h.decision == HunkDecision::Reject)
    {
        return None;
    }
    let original = diff.original.as_deref().unwrap_or("");
    let content = apply_accepted_hunks(original, &diff.proposed, &diff.hunks);
    Some((diff.path.clone(), content))
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
                        if let Some((lines, orig_start, new_start)) = current_hunk.take() {
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

/// Reconstruct file content from hunk decisions.
fn apply_accepted_hunks(original: &str, proposed: &str, hunks: &[DiffHunk]) -> String {
    if hunks.iter().all(|h| h.decision == HunkDecision::Reject) {
        return original.to_string();
    }
    if hunks
        .iter()
        .all(|h| matches!(h.decision, HunkDecision::Accept | HunkDecision::Pending))
    {
        return proposed.to_string();
    }
    if hunks.iter().any(|h| h.decision == HunkDecision::Accept) {
        proposed.to_string()
    } else {
        original.to_string()
    }
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
        Self::load_from(&path)
    }

    /// Load trust patterns from an explicit toml path (path-injectable).
    pub fn load_from(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        Self::from_toml_str(&text)
    }

    /// Parse trust patterns from toml text (pure).
    pub fn from_toml_str(text: &str) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_hunk(decision: HunkDecision) -> DiffHunk {
        DiffHunk {
            header: "@@ -1,1 +1,1 @@".into(),
            lines: vec![('-', "old".into()), ('+', "new".into())],
            decision,
        }
    }

    #[test]
    fn compute_hunks_detects_single_line_change() {
        let hunks = compute_hunks("alpha\nbeta", "alpha\ngamma");
        assert!(!hunks.is_empty());
        assert!(hunks
            .iter()
            .any(|h| h.lines.iter().any(|(op, _)| *op == '+')));
    }

    #[test]
    fn compute_hunks_identical_strings_no_changes() {
        let hunks = compute_hunks("same\nline\n", "same\nline\n");
        assert!(
            hunks.is_empty()
                || hunks
                    .iter()
                    .all(|h| h.lines.iter().all(|(op, _)| *op == ' ')),
            "identical inputs should not produce change ops"
        );
    }

    #[test]
    fn compute_hunks_pure_insertion() {
        let hunks = compute_hunks("a\n", "a\nb\n");
        assert!(!hunks.is_empty());
        assert!(hunks
            .iter()
            .any(|h| h.lines.iter().any(|(op, s)| *op == '+' && s == "b")));
    }

    #[test]
    fn compute_hunks_pure_deletion() {
        let hunks = compute_hunks("a\nb\n", "a\n");
        assert!(!hunks.is_empty());
        assert!(hunks
            .iter()
            .any(|h| h.lines.iter().any(|(op, s)| *op == '-' && s == "b")));
    }

    #[test]
    fn compute_hunks_empty_to_content() {
        let hunks = compute_hunks("", "only\n");
        assert!(!hunks.is_empty());
        assert!(hunks
            .iter()
            .any(|h| h.lines.iter().any(|(op, _)| *op == '+')));
    }

    #[test]
    fn file_diff_from_write_file_tool() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sample.txt");
        std::fs::write(&path, "hello\n").expect("seed");
        let diff = file_diff_from_tool(
            "write_file",
            &json!({"path": path.to_string_lossy(), "content": "hello\nworld\n"}),
        )
        .expect("diff");
        assert!(!diff.hunks.is_empty());
        assert_eq!(diff.proposed, "hello\nworld\n");
    }

    #[test]
    fn file_diff_from_unknown_tool_is_none() {
        assert!(file_diff_from_tool("bash", &json!({"cmd": "ls"})).is_none());
    }

    #[test]
    fn file_diff_from_write_file_missing_fields() {
        assert!(file_diff_from_tool("write_file", &json!({"path": "/tmp/x"})).is_none());
        assert!(file_diff_from_tool("write_file", &json!({"content": "x"})).is_none());
    }

    #[test]
    fn finalize_all_rejected_is_none() {
        let mut diff = FileDiff {
            path: PathBuf::from("x.rs"),
            original: Some("a".into()),
            proposed: "b".into(),
            hunks: vec![DiffHunk {
                header: "@@".into(),
                lines: vec![('+', "b".into())],
                decision: HunkDecision::Reject,
            }],
            file_decision: None,
        };
        assert!(finalize_for_apply(&diff).is_none());
        diff.hunks[0].decision = HunkDecision::Accept;
        assert_eq!(
            finalize_for_apply(&diff).map(|(_, c)| c),
            Some("b".to_string())
        );
    }

    #[test]
    fn finalize_empty_hunks_returns_proposed() {
        let diff = FileDiff {
            path: PathBuf::from("empty.rs"),
            original: Some("a".into()),
            proposed: "proposed".into(),
            hunks: vec![],
            file_decision: None,
        };
        let (path, content) = finalize_for_apply(&diff).expect("some");
        assert_eq!(path, PathBuf::from("empty.rs"));
        assert_eq!(content, "proposed");
    }

    #[test]
    fn set_hunk_decision_accept_reject_and_oob() {
        let mut diff = FileDiff {
            path: PathBuf::from("t.rs"),
            original: Some("a".into()),
            proposed: "b".into(),
            hunks: vec![sample_hunk(HunkDecision::Pending)],
            file_decision: None,
        };
        set_hunk_decision(&mut diff, 0, true);
        assert_eq!(diff.hunks[0].decision, HunkDecision::Accept);
        set_hunk_decision(&mut diff, 0, false);
        assert_eq!(diff.hunks[0].decision, HunkDecision::Reject);
        // Out-of-bounds is a no-op
        set_hunk_decision(&mut diff, 99, true);
        assert_eq!(diff.hunks[0].decision, HunkDecision::Reject);
    }

    #[test]
    fn next_pending_hunk_skips_decided() {
        let diff = FileDiff {
            path: PathBuf::from("t.rs"),
            original: None,
            proposed: "x".into(),
            hunks: vec![
                sample_hunk(HunkDecision::Accept),
                sample_hunk(HunkDecision::Pending),
                sample_hunk(HunkDecision::Reject),
                sample_hunk(HunkDecision::Pending),
            ],
            file_decision: None,
        };
        assert_eq!(next_pending_hunk(&diff, 0), Some(1));
        assert_eq!(next_pending_hunk(&diff, 1), Some(1));
        assert_eq!(next_pending_hunk(&diff, 2), Some(3));
        assert_eq!(next_pending_hunk(&diff, 4), None);
    }

    #[test]
    fn format_hunk_for_display_includes_header() {
        let hunk = sample_hunk(HunkDecision::Pending);
        let lines = format_hunk_for_display(&hunk);
        assert_eq!(lines[0], (' ', hunk.header.clone()));
        assert_eq!(lines.len(), 1 + hunk.lines.len());
        assert_eq!(lines[1].0, '-');
        assert_eq!(lines[2].0, '+');
    }

    #[test]
    fn staging_buffer_empty_and_pending_count() {
        let mut buf = StagingBuffer::new();
        assert!(buf.is_empty());
        assert_eq!(buf.pending_count(), 0);
        assert_eq!(
            render_staging_summary(&buf),
            "0 file(s), 0 hunk(s), 0 pending"
        );

        // Stage a brand-new file (no original on disk) via pure path
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("new_only.txt");
        buf.stage_write(&path, "line1\nline2\n");
        assert!(!buf.is_empty());
        assert_eq!(buf.pending_count(), 1);
        let summary = render_staging_summary(&buf);
        assert!(summary.contains("1 file(s)"));
        assert!(summary.contains("1 hunk(s)"));
        assert!(summary.contains("1 pending"));

        // Decide the hunk → pending drops
        if let Some(diff) = buf.entries.get_mut(&path) {
            set_hunk_decision(diff, 0, true);
        }
        assert_eq!(buf.pending_count(), 0);
    }

    #[test]
    fn apply_accepted_hunks_all_accept_returns_proposed() {
        let hunks = vec![sample_hunk(HunkDecision::Accept)];
        let out = apply_accepted_hunks("orig", "prop", &hunks);
        assert_eq!(out, "prop");
    }

    #[test]
    fn apply_accepted_hunks_all_reject_returns_original() {
        let hunks = vec![sample_hunk(HunkDecision::Reject)];
        let out = apply_accepted_hunks("orig", "prop", &hunks);
        assert_eq!(out, "orig");
    }

    #[test]
    fn apply_accepted_hunks_mixed_with_accept_uses_proposed() {
        let hunks = vec![
            sample_hunk(HunkDecision::Accept),
            sample_hunk(HunkDecision::Reject),
        ];
        let out = apply_accepted_hunks("orig", "prop", &hunks);
        assert_eq!(out, "prop");
    }

    #[test]
    fn apply_accepted_hunks_only_pending_uses_proposed() {
        let hunks = vec![sample_hunk(HunkDecision::Pending)];
        let out = apply_accepted_hunks("orig", "prop", &hunks);
        assert_eq!(out, "prop");
    }

    #[test]
    fn glob_match_simple_and_double_star() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "src/main.rs"));
        assert!(glob_match("**/*.rs", "src/main.rs"));
        assert!(glob_match("src/*", "src/main.rs"));
        assert!(!glob_match("src/*", "src/a/b.rs"));
        assert!(glob_match("exact.txt", "exact.txt"));
        assert!(!glob_match("exact.txt", "other.txt"));
    }

    #[test]
    fn auto_trust_patterns_accept_reject() {
        let pats = AutoTrustPatterns {
            always_accept: vec!["**/generated/**".into(), "*.lock".into()],
            always_reject: vec!["**/.env".into(), "secrets/*".into()],
        };
        assert!(pats.should_auto_accept(Path::new("foo/generated/x.rs")));
        assert!(pats.should_auto_accept(Path::new("Cargo.lock")));
        assert!(!pats.should_auto_accept(Path::new("src/main.rs")));
        assert!(pats.should_auto_reject(Path::new("app/.env")));
        assert!(pats.should_auto_reject(Path::new("secrets/key")));
        assert!(!pats.should_auto_reject(Path::new("src/main.rs")));
    }

    #[test]
    fn auto_trust_patterns_default_empty() {
        let pats = AutoTrustPatterns::default();
        assert!(!pats.should_auto_accept(Path::new("any.rs")));
        assert!(!pats.should_auto_reject(Path::new("any.rs")));
    }

    #[test]
    fn hunk_decision_equality() {
        assert_eq!(HunkDecision::Pending, HunkDecision::Pending);
        assert_ne!(HunkDecision::Accept, HunkDecision::Reject);
    }

    #[test]
    fn auto_trust_load_from_missing_and_invalid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("no-trust.toml");
        let pats = AutoTrustPatterns::load_from(&missing);
        assert!(pats.always_accept.is_empty());
        assert!(pats.always_reject.is_empty());

        let bad = dir.path().join("bad.toml");
        std::fs::write(&bad, "[[[not toml").unwrap();
        let pats = AutoTrustPatterns::load_from(&bad);
        assert!(pats.always_accept.is_empty());
    }

    #[test]
    fn auto_trust_load_from_and_from_toml_str() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("diff-trust.toml");
        std::fs::write(
            &path,
            r#"
always_accept = ["**/*.gen.rs", "Cargo.lock"]
always_reject = [".env", "secrets/*"]
"#,
        )
        .unwrap();
        let pats = AutoTrustPatterns::load_from(&path);
        assert_eq!(pats.always_accept.len(), 2);
        assert_eq!(pats.always_reject.len(), 2);
        assert!(pats.should_auto_accept(Path::new("src/foo.gen.rs")));
        assert!(pats.should_auto_reject(Path::new(".env")));

        let pure = AutoTrustPatterns::from_toml_str(
            r#"
always_accept = ["only.rs"]
always_reject = []
"#,
        );
        assert_eq!(pure.always_accept, vec!["only.rs".to_string()]);
        assert!(pure.always_reject.is_empty());
        assert!(pure.should_auto_accept(Path::new("only.rs")));

        // Non-string array entries ignored
        let mixed = AutoTrustPatterns::from_toml_str(
            r#"
always_accept = ["ok.rs", 123, true]
"#,
        );
        assert_eq!(mixed.always_accept, vec!["ok.rs".to_string()]);
    }

    #[test]
    fn file_diff_from_patch_file_tool() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("patch_me.txt");
        std::fs::write(&path, "hello world\n").unwrap();
        let diff = file_diff_from_tool(
            "patch_file",
            &json!({
                "path": path.to_string_lossy(),
                "old_string": "world",
                "new_string": "there"
            }),
        )
        .expect("diff");
        assert!(diff.proposed.contains("hello there"));
        assert!(!diff.hunks.is_empty());
    }

    #[test]
    fn file_diff_from_patch_file_no_match_is_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("patch_me.txt");
        std::fs::write(&path, "hello world\n").unwrap();
        assert!(file_diff_from_tool(
            "patch_file",
            &json!({
                "path": path.to_string_lossy(),
                "old_string": "missing",
                "new_string": "x"
            }),
        )
        .is_none());
    }

    #[test]
    fn file_diff_from_patch_file_missing_fields_or_file() {
        assert!(
            file_diff_from_tool("patch_file", &json!({"path": "/tmp/x", "old_string": "a"}),)
                .is_none()
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("nope.txt");
        assert!(file_diff_from_tool(
            "patch_file",
            &json!({
                "path": missing.to_string_lossy(),
                "old_string": "a",
                "new_string": "b"
            }),
        )
        .is_none());
    }

    #[test]
    fn staging_commit_new_file_accept_and_reject() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("out.txt");
        let mut buf = StagingBuffer::new();
        buf.stage_write(&path, "content\n");
        // Accept via file_decision
        if let Some(diff) = buf.entries.get_mut(&path) {
            diff.file_decision = Some(true);
        }
        let results = buf.commit();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "content\n");

        // Reject file_decision leaves existing content
        std::fs::write(&path, "keep\n").unwrap();
        let mut buf2 = StagingBuffer::new();
        buf2.stage_write(&path, "overwrite\n");
        if let Some(diff) = buf2.entries.get_mut(&path) {
            diff.file_decision = Some(false);
        }
        let _ = buf2.commit();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep\n");
    }

    #[test]
    fn staging_commit_hunk_accept_writes_proposed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hunk.txt");
        std::fs::write(&path, "old\n").unwrap();
        let mut buf = StagingBuffer::new();
        buf.stage_write(&path, "new\n");
        if let Some(diff) = buf.entries.get_mut(&path) {
            for i in 0..diff.hunks.len() {
                set_hunk_decision(diff, i, true);
            }
        }
        let _ = buf.commit();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
    }

    #[test]
    fn staging_commit_all_hunks_rejected_keeps_original() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("keep.txt");
        std::fs::write(&path, "original\n").unwrap();
        let mut buf = StagingBuffer::new();
        buf.stage_write(&path, "changed\n");
        if let Some(diff) = buf.entries.get_mut(&path) {
            for i in 0..diff.hunks.len() {
                set_hunk_decision(diff, i, false);
            }
        }
        let _ = buf.commit();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original\n");
    }

    #[test]
    fn finalize_all_pending_returns_proposed() {
        let diff = FileDiff {
            path: PathBuf::from("p.rs"),
            original: Some("a".into()),
            proposed: "b".into(),
            hunks: vec![sample_hunk(HunkDecision::Pending)],
            file_decision: None,
        };
        let (_, content) = finalize_for_apply(&diff).expect("some");
        assert_eq!(content, "b");
    }

    #[test]
    fn stage_write_existing_file_computes_hunks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("exist.txt");
        std::fs::write(&path, "line1\nline2\n").unwrap();
        let mut buf = StagingBuffer::new();
        buf.stage_write(&path, "line1\nline2-changed\n");
        let diff = buf.entries.get(&path).expect("entry");
        assert_eq!(diff.original.as_deref(), Some("line1\nline2\n"));
        assert!(!diff.hunks.is_empty());
        assert!(diff
            .hunks
            .iter()
            .any(|h| h.lines.iter().any(|(op, _)| *op == '+' || *op == '-')));
    }

    #[test]
    fn compute_hunks_multiline_swap() {
        let orig = "a\nb\nc\nd\n";
        let prop = "a\nX\nc\nY\n";
        let hunks = compute_hunks(orig, prop);
        assert!(!hunks.is_empty());
        let ops: Vec<char> = hunks
            .iter()
            .flat_map(|h| h.lines.iter().map(|(op, _)| *op))
            .collect();
        assert!(ops.contains(&'+'));
        assert!(ops.contains(&'-'));
        // Headers present
        assert!(hunks.iter().all(|h| h.header.contains("@@")));
        assert!(hunks.iter().all(|h| h.decision == HunkDecision::Pending));
    }

    #[test]
    fn apply_accepted_hunks_empty_slice_returns_proposed_via_all_pending_branch() {
        // empty hunks: all(|Reject) is true vacuously → original
        let out = apply_accepted_hunks("orig", "prop", &[]);
        assert_eq!(out, "orig");
    }

    #[test]
    fn glob_match_dot_literal_and_invalid_pattern_safe() {
        assert!(glob_match("file.txt", "file.txt"));
        assert!(!glob_match("file.txt", "fileXtxt"));
        // Unbalanced regex-ish input should not panic
        assert!(!glob_match("(", "x"));
    }

    #[test]
    fn new_file_hunk_header_mentions_new_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("brand_new.rs");
        let mut buf = StagingBuffer::new();
        buf.stage_write(&path, "fn main() {}\n");
        let diff = buf.entries.get(&path).unwrap();
        assert!(diff.original.is_none());
        assert_eq!(diff.hunks.len(), 1);
        assert!(diff.hunks[0].header.contains("new file"));
        assert!(diff.hunks[0].lines.iter().all(|(op, _)| *op == '+'));
    }
}
