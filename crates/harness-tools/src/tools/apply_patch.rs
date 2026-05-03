//! `apply_patch` — apply a unified diff to one or more files atomically.
//!
//! The agent sends a standard unified diff (as produced by `git diff` or
//! `diff -u`). This tool parses it and applies each hunk to the corresponding
//! file, reporting per-file results. On any failure the whole operation is
//! rolled back (original files restored).

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

use crate::registry::Tool;
use crate::workspace_root::WorkspaceRoot;

pub struct ApplyPatchTool {
    pub workspace: Arc<WorkspaceRoot>,
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "apply_patch",
            "Apply a unified diff to one or more files. \
             Use this for multi-file edits — pass the complete unified diff \
             (as produced by `diff -u` or `git diff`). \
             Atomic: if any hunk fails, all files are restored.",
            json!({
                "type": "object",
                "properties": {
                    "patch": {
                        "type": "string",
                        "description": "A unified diff string (--- a/file ... +++ b/file ... @@ ... lines)."
                    }
                },
                "required": ["patch"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let patch_text = args["patch"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing patch"))?;

        let changes = parse_unified_diff(patch_text)?;

        if changes.is_empty() {
            return Ok("No changes found in patch.".to_string());
        }

        let changes: Vec<(PathBuf, bool, String)> = changes
            .into_iter()
            .map(|(path_s, new_content, is_deletion)| {
                if path_s.is_empty() || path_s == "/dev/null" {
                    anyhow::bail!("patch has no resolvable file path (---/+++ headers)");
                }
                let path = self.workspace.resolve(&path_s)?;
                Ok((path, is_deletion, new_content))
            })
            .collect::<anyhow::Result<_>>()?;

        // Read originals for rollback.
        let mut originals: Vec<(PathBuf, Option<String>)> = Vec::new();
        for (path, _, _) in &changes {
            let original = tokio::fs::read_to_string(path).await.ok();
            originals.push((path.clone(), original));
        }

        // Apply each file change.
        let mut results: Vec<String> = Vec::new();
        let mut failed = false;

        use std::io::ErrorKind;

        for (path, is_deletion, new_content) in &changes {
            let res = if *is_deletion {
                match tokio::fs::remove_file(path).await {
                    Ok(()) => Ok(format!("✓ deleted {}", path.display())),
                    Err(e) if e.kind() == ErrorKind::NotFound => {
                        Ok(format!("✓ {} (already absent)", path.display()))
                    }
                    Err(e) => Err(e),
                }
            } else {
                match tokio::fs::write(path, new_content).await {
                    Ok(()) => Ok(format!("✓ {}", path.display())),
                    Err(e) => Err(e),
                }
            };
            match res {
                Ok(s) => results.push(s),
                Err(e) => {
                    results.push(format!("✗ {}: {e}", path.display()));
                    failed = true;
                    break;
                }
            }
        }

        if failed {
            // Rollback.
            for (path, original) in &originals {
                match original {
                    Some(content) => {
                        let _ = tokio::fs::write(path, content).await;
                    }
                    None => {
                        let _ = tokio::fs::remove_file(path).await;
                    }
                }
            }
            return Err(anyhow::anyhow!(
                "apply_patch failed (rolled back):\n{}",
                results.join("\n")
            ));
        }

        let file_count = changes.len();
        let hunk_summary: String = results.join("\n");
        Ok(format!(
            "Applied patch to {file_count} file(s):\n{hunk_summary}"
        ))
    }
}

// ── Unified diff parser ───────────────────────────────────────────────────────
//
// Handles the subset of unified diff format that `git diff` produces:
//   --- a/path
//   +++ b/path
//   @@ -L,C +L,C @@
//   [context / - removed / + added lines]
//
// Returns a list of (file_path, new_file_content, is_deletion) triples.
// `is_deletion` is true when the `+++` header is `/dev/null` (file removal).

fn parse_unified_diff(patch: &str) -> anyhow::Result<Vec<(String, String, bool)>> {
    let mut result: Vec<(String, String, bool)> = Vec::new();
    let mut lines: std::iter::Peekable<std::str::Lines> = patch.lines().peekable();

    while let Some(line) = lines.peek() {
        if line.starts_with("--- ") {
            let minus_line = lines
                .next()
                .ok_or_else(|| anyhow::anyhow!("truncated after --- line"))?;
            let plus_line = lines
                .next()
                .ok_or_else(|| anyhow::anyhow!("expected +++ line after ---"))?;
            if !plus_line.starts_with("+++ ") {
                anyhow::bail!("expected +++ line, got: {plus_line}");
            }

            let minus_path = strip_diff_prefix(minus_line.trim_start_matches("--- ").trim());
            let plus_path = strip_diff_prefix(plus_line.trim_start_matches("+++ ").trim());
            let is_deletion = plus_path == "/dev/null";
            let path = if is_deletion { minus_path } else { plus_path };

            // Read current file content (may not exist for new files).
            let original = std::fs::read_to_string(path).unwrap_or_default();
            let mut file_lines: Vec<String> = original.lines().map(|l| l.to_string()).collect();

            // Apply hunks for this file.
            while let Some(hunk_header) = lines.peek() {
                if !hunk_header.starts_with("@@") {
                    break;
                }
                let header = lines.next().unwrap();
                let (orig_start, _orig_count, _new_start, _new_count) = parse_hunk_header(header)?;

                let pos = (orig_start as usize).saturating_sub(1);
                let mut hunk_lines: Vec<(char, String)> = Vec::new();

                while let Some(l) = lines.peek() {
                    if l.starts_with("@@") || l.starts_with("---") || l.starts_with("+++") {
                        break;
                    }
                    let l = lines.next().unwrap();
                    if l.is_empty() {
                        hunk_lines.push((' ', String::new()));
                    } else {
                        let ch = l.chars().next().unwrap_or(' ');
                        let content = l[1..].to_string();
                        hunk_lines.push((ch, content));
                    }
                }

                // Apply the hunk: walk through context/remove/add.
                let mut new_lines: Vec<String> = file_lines[..pos].to_vec();
                let mut orig_ptr = pos;

                for (ch, content) in &hunk_lines {
                    match ch {
                        ' ' => {
                            // Context line — advance orig pointer.
                            new_lines.push(
                                file_lines
                                    .get(orig_ptr)
                                    .cloned()
                                    .unwrap_or_else(|| content.clone()),
                            );
                            orig_ptr += 1;
                        }
                        '-' => {
                            // Remove line — skip it.
                            orig_ptr += 1;
                        }
                        '+' => {
                            // Add line.
                            new_lines.push(content.clone());
                        }
                        _ => {}
                    }
                }

                // Append remaining original lines after the hunk.
                new_lines.extend(file_lines[orig_ptr..].iter().cloned());
                file_lines = new_lines;
            }

            let new_content = file_lines.join("\n");
            // Preserve trailing newline if original had one.
            let new_content = if original.ends_with('\n') && !new_content.ends_with('\n') {
                format!("{new_content}\n")
            } else {
                new_content
            };

            result.push((path.to_string(), new_content, is_deletion));
        } else {
            lines.next();
        }
    }

    Ok(result)
}

fn strip_diff_prefix(path: &str) -> &str {
    // Strip "a/", "b/", or "/dev/null" indicators.
    if path == "/dev/null" {
        return path;
    }
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
}

fn parse_hunk_header(header: &str) -> anyhow::Result<(i64, i64, i64, i64)> {
    // Format: @@ -L[,C] +L[,C] @@[rest]
    let inner = header
        .strip_prefix("@@")
        .and_then(|s| s.split("@@").next())
        .ok_or_else(|| anyhow::anyhow!("malformed hunk header: {header}"))?
        .trim();

    let parts: Vec<&str> = inner.split_whitespace().collect();
    if parts.len() < 2 {
        anyhow::bail!("malformed hunk header: {header}");
    }

    let parse_range = |s: &str| -> anyhow::Result<(i64, i64)> {
        let s = s.trim_start_matches('+').trim_start_matches('-');
        if let Some((l, c)) = s.split_once(',') {
            Ok((l.parse()?, c.parse()?))
        } else {
            Ok((s.parse()?, 1))
        }
    };

    let (ol, oc) = parse_range(parts[0])?;
    let (nl, nc) = parse_range(parts[1])?;
    Ok((ol, oc, nl, nc))
}
