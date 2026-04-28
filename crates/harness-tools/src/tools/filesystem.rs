use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};
use walkdir::WalkDir;

use crate::registry::Tool;

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "read_file",
            "Read the contents of a file at the given path.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute or relative file path to read." },
                    "start_line": { "type": "integer", "description": "Optional 1-based line to start reading from." },
                    "end_line": { "type": "integer", "description": "Optional 1-based line to stop reading at (inclusive)." }
                },
                "required": ["path"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let path = args["path"].as_str().ok_or_else(|| anyhow::anyhow!("missing path"))?;
        let content = tokio::fs::read_to_string(path).await?;

        let start = args["start_line"].as_u64().map(|n| n as usize).unwrap_or(1);
        let end = args["end_line"].as_u64().map(|n| n as usize);

        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        let from = start.saturating_sub(1).min(total);
        let to = end.unwrap_or(total).min(total);

        let selected: Vec<String> = lines[from..to]
            .iter()
            .enumerate()
            .map(|(i, l)| format!("{:>4} | {}", from + i + 1, l))
            .collect();

        Ok(selected.join("\n"))
    }
}

pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "write_file",
            "Write content to a file, creating it or overwriting it.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to write to." },
                    "content": { "type": "string", "description": "Content to write." }
                },
                "required": ["path", "content"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let path = args["path"].as_str().ok_or_else(|| anyhow::anyhow!("missing path"))?;
        let content = args["content"].as_str().ok_or_else(|| anyhow::anyhow!("missing content"))?;

        if let Some(parent) = std::path::Path::new(path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, content).await?;
        Ok(format!("Wrote {} bytes to {path}", content.len()))
    }
}

/// Apply a unified diff (`--- / +++ / @@ … @@`) to a file in place.
///
/// The agent supplies the original and new content of a specific region as a
/// unified diff block. This tool locates the context lines in the file,
/// validates the match, and performs a surgical line-level replacement.
///
/// Why not just use WriteFileTool? Diffs are far more token-efficient for
/// targeted edits in large files, and they make the agent's intent explicit.
pub struct PatchFileTool;

#[async_trait]
impl Tool for PatchFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "patch_file",
            "Apply a targeted edit to a file using an old/new pair. \
             Supply the exact lines to replace (old_content) and what to replace them with \
             (new_content). The tool finds old_content in the file and replaces it. \
             old_content must be an exact substring of the current file content.",
            json!({
                "type": "object",
                "required": ["path", "old_content", "new_content"],
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path of the file to edit."
                    },
                    "old_content": {
                        "type": "string",
                        "description": "Exact text to find in the file (must be unique)."
                    },
                    "new_content": {
                        "type": "string",
                        "description": "Text to replace old_content with."
                    },
                    "dry_run": {
                        "type": "boolean",
                        "description": "If true, show the diff but do not write the file."
                    }
                }
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing path"))?;
        let old = args["old_content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing old_content"))?;
        let new = args["new_content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing new_content"))?;
        let dry_run = args["dry_run"].as_bool().unwrap_or(false);

        let original = tokio::fs::read_to_string(path).await
            .map_err(|e| anyhow::anyhow!("read {path}: {e}"))?;

        // Count occurrences to guard against ambiguous matches.
        let count = original.matches(old).count();
        if count == 0 {
            // Show a diff hint to help the agent correct the old_content.
            let diff = TextDiff::from_lines(old, &original);
            let hint: String = diff
                .unified_diff()
                .context_radius(3)
                .header("old_content (provided)", "file content")
                .to_string();
            return Ok(format!(
                "patch_file: old_content not found in {path}.\n\
                 Diff of provided old_content vs file:\n{hint}"
            ));
        }
        if count > 1 {
            return Ok(format!(
                "patch_file: old_content appears {count} times in {path} — \
                 add more context lines to make it unique."
            ));
        }

        let patched = original.replacen(old, new, 1);

        // Build a human-readable diff for the return value.
        let diff = TextDiff::from_lines(&original, &patched);
        let mut diff_lines = Vec::new();
        for change in diff.iter_all_changes() {
            let prefix = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal  => " ",
            };
            diff_lines.push(format!("{prefix}{}", change.value()));
        }
        // Trim equal context lines in the middle to keep output short.
        let trimmed = trim_context(&diff_lines, 3);

        if dry_run {
            return Ok(format!("--- dry run: {path} ---\n{}", trimmed.join("")));
        }

        tokio::fs::write(path, &patched).await
            .map_err(|e| anyhow::anyhow!("write {path}: {e}"))?;

        let added: usize = diff_lines.iter().filter(|l| l.starts_with('+')).count();
        let removed: usize = diff_lines.iter().filter(|l| l.starts_with('-')).count();

        Ok(format!(
            "Patched {path}: +{added} -{removed} lines.\n{}",
            trimmed.join("")
        ))
    }
}

/// Keep at most `ctx` equal lines before/after each change group.
fn trim_context(lines: &[String], ctx: usize) -> Vec<String> {
    // Find indices with actual changes.
    let changed: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with('+') || l.starts_with('-'))
        .map(|(i, _)| i)
        .collect();

    if changed.is_empty() {
        return vec!["(no changes)".to_string()];
    }

    let mut keep = vec![false; lines.len()];
    for &ci in &changed {
        let lo = ci.saturating_sub(ctx);
        let hi = (ci + ctx + 1).min(lines.len());
        for k in lo..hi {
            keep[k] = true;
        }
    }

    let mut out = Vec::new();
    let mut skipping = false;
    for (i, line) in lines.iter().enumerate() {
        if keep[i] {
            skipping = false;
            out.push(line.clone());
        } else if !skipping {
            out.push("@@ …\n".to_string());
            skipping = true;
        }
    }
    out
}

pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "list_dir",
            "List files and directories at a given path, optionally recursively.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path to list." },
                    "recursive": { "type": "boolean", "description": "If true, list recursively. Default false." },
                    "max_depth": { "type": "integer", "description": "Max recursion depth. Default 3." }
                },
                "required": ["path"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let path = args["path"].as_str().ok_or_else(|| anyhow::anyhow!("missing path"))?;
        let recursive = args["recursive"].as_bool().unwrap_or(false);
        let max_depth = args["max_depth"].as_u64().unwrap_or(3) as usize;

        let depth = if recursive { max_depth } else { 1 };

        let mut entries: Vec<String> = WalkDir::new(path)
            .max_depth(depth)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().to_str().map(|p| p != path).unwrap_or(false))
            .map(|e| {
                let suffix = if e.file_type().is_dir() { "/" } else { "" };
                format!("{}{suffix}", e.path().display())
            })
            .collect();

        entries.sort();
        if entries.is_empty() {
            Ok("(empty directory)".into())
        } else {
            Ok(entries.join("\n"))
        }
    }
}
