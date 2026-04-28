use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
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
