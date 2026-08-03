//! `notebook` tool — list/read/edit Jupyter `.ipynb` cells under the workspace sandbox.
//!
//! Edits preserve nbformat structure via `serde_json` (does not re-execute kernels).

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Map, Value};
use std::path::Path;
use std::sync::Arc;

use crate::registry::Tool;
use crate::workspace_root::WorkspaceRoot;

/// Jupyter notebook cell editor within the workspace.
pub struct NotebookTool {
    /// Workspace root for path resolution.
    pub workspace: Arc<WorkspaceRoot>,
}

#[async_trait]
impl Tool for NotebookTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "notebook",
            "Read and edit Jupyter notebook (.ipynb) cells under the workspace. \
             Actions: list_cells, read_cell, write_cell, add_cell, metadata. \
             Does not execute cells — structure-only JSON edits preserving nbformat.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list_cells", "read_cell", "write_cell", "add_cell", "metadata"],
                        "description": "Notebook operation."
                    },
                    "path": {
                        "type": "string",
                        "description": "Path to .ipynb file under the workspace."
                    },
                    "index": {
                        "type": "integer",
                        "description": "0-based cell index (read_cell, write_cell)."
                    },
                    "source": {
                        "type": "string",
                        "description": "Cell source text (write_cell, add_cell)."
                    },
                    "cell_type": {
                        "type": "string",
                        "enum": ["code", "markdown", "raw"],
                        "description": "Cell type for add_cell (default: code)."
                    },
                    "position": {
                        "type": "integer",
                        "description": "Insert position for add_cell (default: append)."
                    }
                },
                "required": ["action", "path"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing action"))?;
        let path_raw = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing path"))?;
        let path = self.workspace.resolve(path_raw)?;

        if !path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("ipynb"))
            .unwrap_or(false)
        {
            anyhow::bail!("notebook path must end in .ipynb (got {})", path.display());
        }

        match action {
            "list_cells" => list_cells(&path).await,
            "read_cell" => {
                let index = require_index(&args)?;
                read_cell(&path, index).await
            }
            "write_cell" => {
                let index = require_index(&args)?;
                let source = args["source"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("write_cell requires `source`"))?;
                write_cell(&path, index, source).await
            }
            "add_cell" => {
                let source = args["source"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("add_cell requires `source`"))?;
                let cell_type = args["cell_type"].as_str().unwrap_or("code");
                let position = args["position"].as_u64().map(|n| n as usize);
                add_cell(&path, source, cell_type, position).await
            }
            "metadata" => notebook_metadata(&path).await,
            other => anyhow::bail!("unknown notebook action: {other}"),
        }
    }
}

fn require_index(args: &Value) -> anyhow::Result<usize> {
    args["index"]
        .as_u64()
        .map(|n| n as usize)
        .ok_or_else(|| anyhow::anyhow!("missing index"))
}

/// True when the notebook action mutates the file on disk.
pub fn notebook_action_is_mutating(args: &Value) -> bool {
    matches!(
        args.get("action").and_then(Value::as_str),
        Some("write_cell" | "add_cell")
    )
}

async fn load_notebook(path: &Path) -> anyhow::Result<Value> {
    let raw = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    let nb: Value = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("invalid notebook JSON {}: {e}", path.display()))?;
    if !nb.get("cells").map(|c| c.is_array()).unwrap_or(false) {
        anyhow::bail!("notebook missing cells array: {}", path.display());
    }
    Ok(nb)
}

async fn save_notebook(path: &Path, nb: &Value) -> anyhow::Result<()> {
    let pretty = serde_json::to_string_pretty(nb)?;
    // Jupyter often ends files with a trailing newline.
    let mut out = pretty;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    tokio::fs::write(path, out)
        .await
        .map_err(|e| anyhow::anyhow!("write {}: {e}", path.display()))?;
    Ok(())
}

fn cell_source_text(cell: &Value) -> String {
    match cell.get("source") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn set_cell_source(cell: &mut Value, source: &str) {
    // Store as a single-string array entry list split by lines ending with \n (nbformat common).
    let lines: Vec<Value> = if source.is_empty() {
        vec![]
    } else {
        let mut out = Vec::new();
        let mut rest = source;
        while let Some(idx) = rest.find('\n') {
            let (line, tail) = rest.split_at(idx + 1);
            out.push(Value::String(line.to_string()));
            rest = tail;
        }
        if !rest.is_empty() {
            out.push(Value::String(rest.to_string()));
        }
        out
    };
    if let Some(obj) = cell.as_object_mut() {
        obj.insert("source".into(), Value::Array(lines));
    }
}

async fn list_cells(path: &Path) -> anyhow::Result<String> {
    let nb = load_notebook(path).await?;
    let cells = nb["cells"].as_array().cloned().unwrap_or_default();
    let mut out = format!("{} cell(s) in {}\n", cells.len(), path.display());
    for (i, cell) in cells.iter().enumerate() {
        let ty = cell["cell_type"].as_str().unwrap_or("?");
        let src = cell_source_text(cell);
        let preview: String = src.chars().take(80).collect();
        let preview = preview.replace('\n', "⏎");
        out.push_str(&format!("[{i}] {ty}: {preview}\n"));
    }
    Ok(out)
}

async fn read_cell(path: &Path, index: usize) -> anyhow::Result<String> {
    let nb = load_notebook(path).await?;
    let cells = nb["cells"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing cells"))?;
    let cell = cells
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("cell index {index} out of range (len={})", cells.len()))?;
    let ty = cell["cell_type"].as_str().unwrap_or("?");
    let src = cell_source_text(cell);
    Ok(format!("cell[{index}] type={ty}\n---\n{src}"))
}

async fn write_cell(path: &Path, index: usize, source: &str) -> anyhow::Result<String> {
    let mut nb = load_notebook(path).await?;
    let cell_len = nb["cells"].as_array().map(|a| a.len()).unwrap_or(0);
    if index >= cell_len {
        anyhow::bail!("cell index {index} out of range (len={cell_len})");
    }
    let cells = nb
        .get_mut("cells")
        .and_then(|c| c.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("missing cells"))?;
    let cell = &mut cells[index];
    set_cell_source(cell, source);
    // Clear outputs for code cells after edit (stale results).
    if cell["cell_type"].as_str() == Some("code") {
        if let Some(obj) = cell.as_object_mut() {
            obj.insert("outputs".into(), Value::Array(vec![]));
            obj.insert("execution_count".into(), Value::Null);
        }
    }
    save_notebook(path, &nb).await?;
    Ok(format!(
        "Updated cell[{index}] in {} ({} bytes source)",
        path.display(),
        source.len()
    ))
}

async fn add_cell(
    path: &Path,
    source: &str,
    cell_type: &str,
    position: Option<usize>,
) -> anyhow::Result<String> {
    match cell_type {
        "code" | "markdown" | "raw" => {}
        other => anyhow::bail!("invalid cell_type: {other}"),
    }

    let mut nb = load_notebook(path).await?;
    let total = {
        let cells = nb
            .get_mut("cells")
            .and_then(|c| c.as_array_mut())
            .ok_or_else(|| anyhow::anyhow!("missing cells"))?;

        let mut cell_map = Map::new();
        cell_map.insert("cell_type".into(), Value::String(cell_type.into()));
        cell_map.insert("metadata".into(), json!({}));
        cell_map.insert("source".into(), Value::Array(vec![]));
        if cell_type == "code" {
            cell_map.insert("outputs".into(), Value::Array(vec![]));
            cell_map.insert("execution_count".into(), Value::Null);
        }
        let mut cell = Value::Object(cell_map);
        set_cell_source(&mut cell, source);

        let insert_at = position.unwrap_or(cells.len()).min(cells.len());
        cells.insert(insert_at, cell);
        let total = cells.len();
        (insert_at, total)
    };
    let (insert_at, total) = total;
    save_notebook(path, &nb).await?;
    Ok(format!(
        "Added {cell_type} cell at index {insert_at} in {} (total {total} cells)",
        path.display()
    ))
}

async fn notebook_metadata(path: &Path) -> anyhow::Result<String> {
    let nb = load_notebook(path).await?;
    let meta = nb.get("metadata").cloned().unwrap_or(json!({}));
    let nbformat = nb.get("nbformat").cloned().unwrap_or(Value::Null);
    let nbformat_minor = nb.get("nbformat_minor").cloned().unwrap_or(Value::Null);
    let cell_count = nb["cells"].as_array().map(|a| a.len()).unwrap_or(0);
    Ok(serde_json::to_string_pretty(&json!({
        "path": path.display().to_string(),
        "nbformat": nbformat,
        "nbformat_minor": nbformat_minor,
        "cell_count": cell_count,
        "metadata": meta
    }))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_root::{SandboxMode, WorkspaceRoot};
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn fixture_nb() -> Value {
        json!({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {
                "kernelspec": {
                    "display_name": "Python 3",
                    "language": "python",
                    "name": "python3"
                }
            },
            "cells": [
                {
                    "cell_type": "markdown",
                    "metadata": {},
                    "source": ["# Title\n", "Hello"]
                },
                {
                    "cell_type": "code",
                    "metadata": {},
                    "execution_count": 1,
                    "outputs": [{"output_type": "stream", "name": "stdout", "text": ["hi\n"]}],
                    "source": ["print('hi')\n"]
                }
            ]
        })
    }

    fn setup() -> (tempfile::TempDir, Arc<WorkspaceRoot>) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("demo.ipynb");
        std::fs::write(&path, serde_json::to_string_pretty(&fixture_nb()).unwrap()).unwrap();
        let ws = Arc::new(
            WorkspaceRoot::new(dir.path().to_path_buf(), SandboxMode::Strict).expect("ws"),
        );
        (dir, ws)
    }

    #[tokio::test]
    async fn list_read_metadata() {
        let (_dir, ws) = setup();
        let tool = NotebookTool {
            workspace: ws.clone(),
        };

        let list = tool
            .execute(json!({"action": "list_cells", "path": "demo.ipynb"}))
            .await
            .unwrap();
        assert!(list.contains("2 cell"));
        assert!(list.contains("markdown"));
        assert!(list.contains("code"));

        let cell0 = tool
            .execute(json!({"action": "read_cell", "path": "demo.ipynb", "index": 0}))
            .await
            .unwrap();
        assert!(cell0.contains("# Title"));

        let meta = tool
            .execute(json!({"action": "metadata", "path": "demo.ipynb"}))
            .await
            .unwrap();
        assert!(meta.contains("nbformat"));
        assert!(meta.contains("python3") || meta.contains("kernelspec"));
    }

    #[tokio::test]
    async fn write_and_add_cell_preserve_structure() {
        let (dir, ws) = setup();
        let tool = NotebookTool {
            workspace: ws.clone(),
        };

        tool.execute(json!({
            "action": "write_cell",
            "path": "demo.ipynb",
            "index": 1,
            "source": "print('updated')\n"
        }))
        .await
        .unwrap();

        let raw = std::fs::read_to_string(dir.path().join("demo.ipynb")).unwrap();
        let nb: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(nb["nbformat"], 4);
        assert!(nb["cells"].as_array().unwrap().len() >= 2);
        let src = cell_source_text(&nb["cells"][1]);
        assert!(src.contains("updated"));
        // outputs cleared
        assert_eq!(nb["cells"][1]["outputs"], json!([]));

        tool.execute(json!({
            "action": "add_cell",
            "path": "demo.ipynb",
            "cell_type": "markdown",
            "source": "## New\n",
            "position": 0
        }))
        .await
        .unwrap();

        let list = tool
            .execute(json!({"action": "list_cells", "path": "demo.ipynb"}))
            .await
            .unwrap();
        assert!(list.contains("3 cell"));
        assert!(list.contains("## New") || list.contains("markdown"));
    }

    #[tokio::test]
    async fn rejects_non_ipynb_and_bad_index() {
        let (dir, ws) = setup();
        std::fs::write(dir.path().join("notes.txt"), "nope").unwrap();
        let tool = NotebookTool {
            workspace: ws.clone(),
        };

        let err = tool
            .execute(json!({"action": "list_cells", "path": "notes.txt"}))
            .await
            .expect_err("ext");
        assert!(err.to_string().contains(".ipynb"));

        let err = tool
            .execute(json!({"action": "read_cell", "path": "demo.ipynb", "index": 99}))
            .await
            .expect_err("oob");
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn mutating_policy_helper() {
        assert!(notebook_action_is_mutating(
            &json!({"action": "write_cell"})
        ));
        assert!(notebook_action_is_mutating(&json!({"action": "add_cell"})));
        assert!(!notebook_action_is_mutating(
            &json!({"action": "list_cells"})
        ));
        assert!(!notebook_action_is_mutating(
            &json!({"action": "read_cell"})
        ));
        assert!(!notebook_action_is_mutating(
            &json!({"action": "metadata"})
        ));
    }
}
