//! LSP client integration for harness.
//!
//! Exposes four agent tools: find_definition, find_references, rename_symbol, diagnostics.
//!
//! Use `LspSession::detect_and_spawn` to auto-detect and launch the right language server,
//! then call `LspSession::register_tools` to add the tools to a `ToolRegistry`.

mod client;
mod detect;
mod jsonrpc;

pub use client::LspClient;
pub use detect::{detect_language_server, LspKind};

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use harness_tools::registry::Tool;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A shared handle to a running LSP client.
pub type SharedLspClient = Arc<Mutex<LspClient>>;

/// Auto-detect the language server for `root`, spawn it, and return a shared client.
/// Returns `None` if no supported language server is available for this project.
pub async fn detect_and_spawn(root: &Path) -> Option<SharedLspClient> {
    let kind = detect_language_server(root)?;
    match LspClient::spawn(&kind, root).await {
        Ok(client) => {
            tracing::info!(server = kind.binary(), "LSP server started");
            Some(Arc::new(Mutex::new(client)))
        }
        Err(e) => {
            tracing::warn!("LSP spawn failed: {e}");
            None
        }
    }
}

// ── find_definition ──────────────────────────────────────────────────────────

pub struct FindDefinitionTool {
    pub client: SharedLspClient,
}

#[async_trait]
impl Tool for FindDefinitionTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "find_definition",
            "Jump to the definition of a symbol at the given file/line/col. \
             Returns the definition location as 'file:line:col'. \
             Use this instead of grepping — it is accurate for any language with a running LSP.",
            json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "File path." },
                    "line": { "type": "integer", "description": "1-indexed line number." },
                    "col":  { "type": "integer", "description": "1-indexed column." }
                },
                "required": ["file", "line", "col"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let file = args["file"].as_str().ok_or_else(|| anyhow::anyhow!("missing file"))?;
        let line = args["line"].as_u64().ok_or_else(|| anyhow::anyhow!("missing line"))? as u32;
        let col  = args["col"].as_u64().ok_or_else(|| anyhow::anyhow!("missing col"))? as u32;
        let mut c = self.client.lock().await;
        c.goto_definition(file, line.saturating_sub(1), col.saturating_sub(1)).await
    }
}

// ── find_references ──────────────────────────────────────────────────────────

pub struct FindReferencesTool {
    pub client: SharedLspClient,
}

#[async_trait]
impl Tool for FindReferencesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "find_references",
            "Find all usage sites of a symbol across the project. \
             Returns a list of 'file:line' locations.",
            json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string" },
                    "line": { "type": "integer" },
                    "col":  { "type": "integer" }
                },
                "required": ["file", "line", "col"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let file = args["file"].as_str().ok_or_else(|| anyhow::anyhow!("missing file"))?;
        let line = args["line"].as_u64().ok_or_else(|| anyhow::anyhow!("missing line"))? as u32;
        let col  = args["col"].as_u64().ok_or_else(|| anyhow::anyhow!("missing col"))? as u32;
        let mut c = self.client.lock().await;
        c.references(file, line.saturating_sub(1), col.saturating_sub(1)).await
    }
}

// ── rename_symbol ────────────────────────────────────────────────────────────

pub struct RenameSymbolTool {
    pub client: SharedLspClient,
}

#[async_trait]
impl Tool for RenameSymbolTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "rename_symbol",
            "Rename a symbol across the whole project using the language server. \
             Returns a summary of all edits. \
             IMPORTANT: this is a dry-run — apply the returned changes with apply_patch.",
            json!({
                "type": "object",
                "properties": {
                    "file":     { "type": "string" },
                    "line":     { "type": "integer" },
                    "col":      { "type": "integer" },
                    "new_name": { "type": "string" }
                },
                "required": ["file", "line", "col", "new_name"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let file     = args["file"].as_str().ok_or_else(|| anyhow::anyhow!("missing file"))?;
        let line     = args["line"].as_u64().ok_or_else(|| anyhow::anyhow!("missing line"))? as u32;
        let col      = args["col"].as_u64().ok_or_else(|| anyhow::anyhow!("missing col"))? as u32;
        let new_name = args["new_name"].as_str().ok_or_else(|| anyhow::anyhow!("missing new_name"))?;
        let mut c = self.client.lock().await;
        c.rename(file, line.saturating_sub(1), col.saturating_sub(1), new_name).await
    }
}

// ── diagnostics ──────────────────────────────────────────────────────────────

pub struct DiagnosticsTool {
    pub client: SharedLspClient,
}

#[async_trait]
impl Tool for DiagnosticsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "diagnostics",
            "Get live errors and warnings from the language server. \
             Pass a file path for per-file results, or omit for project-wide diagnostics.",
            json!({
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "Optional: file path. Omit for all diagnostics."
                    }
                }
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let file = args["file"].as_str();
        let mut c = self.client.lock().await;
        c.diagnostics(file).await
    }
}
