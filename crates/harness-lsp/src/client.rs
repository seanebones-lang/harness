//! LSP client: spawns the language server process and performs typed requests.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::process::Command;
use tracing::debug;

use crate::detect::LspKind;
use crate::jsonrpc::{LspTransport, Notification, Request};

static REQ_ID: AtomicU64 = AtomicU64::new(1);

pub struct LspClient {
    transport: LspTransport,
    root_uri: String,
    initialized: bool,
}

impl LspClient {
    /// Spawn a language server for the given project root and kind.
    pub async fn spawn(kind: &LspKind, root: &Path) -> Result<Self> {
        let root_abs = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let root_uri = path_to_uri(&root_abs);

        let mut child = Command::new(kind.binary())
            .args(kind.args())
            .current_dir(&root_abs)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("failed to spawn language server: {}", kind.binary()))?;

        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;

        // Intentionally leak the child handle — the LSP server lives for the session.
        std::mem::forget(child);

        let transport = LspTransport::new(stdin, stdout);
        let mut client = Self {
            transport,
            root_uri,
            initialized: false,
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&mut self) -> Result<()> {
        let id = next_id();
        let req = Request::new(
            id,
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": self.root_uri,
                "capabilities": {
                    "textDocument": {
                        "definition": { "dynamicRegistration": false },
                        "references": { "dynamicRegistration": false },
                        "rename": { "dynamicRegistration": false },
                        "publishDiagnostics": { "dynamicRegistration": false }
                    }
                },
                "initializationOptions": {}
            }),
        );

        self.transport.send_request(&req).await?;
        self.transport.read_response_for(id).await?;

        let notif = Notification::new("initialized", json!({}));
        self.transport.send_notification(&notif).await?;
        self.initialized = true;
        debug!("LSP initialized");
        Ok(())
    }

    /// Notify the server that a file is open (required before requests on that file).
    async fn open_file(&mut self, path: &str) -> Result<()> {
        let uri = path_to_uri(Path::new(path));
        let content = std::fs::read_to_string(path).unwrap_or_default();
        let language_id = guess_language_id(path);

        let notif = Notification::new(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": 1,
                    "text": content
                }
            }),
        );
        self.transport.send_notification(&notif).await?;
        Ok(())
    }

    pub async fn goto_definition(&mut self, file: &str, line: u32, col: u32) -> Result<String> {
        self.open_file(file).await?;
        let id = next_id();
        let uri = path_to_uri(Path::new(file));
        let req = Request::new(
            id,
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": col }
            }),
        );

        self.transport.send_request(&req).await?;
        let resp = self.transport.read_response_for(id).await?;

        if let Some(err) = resp.error {
            return Err(anyhow::anyhow!("LSP error: {}", err.message));
        }

        format_location_response(resp.result)
    }

    pub async fn references(&mut self, file: &str, line: u32, col: u32) -> Result<String> {
        self.open_file(file).await?;
        let id = next_id();
        let uri = path_to_uri(Path::new(file));
        let req = Request::new(
            id,
            "textDocument/references",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": col },
                "context": { "includeDeclaration": true }
            }),
        );

        self.transport.send_request(&req).await?;
        let resp = self.transport.read_response_for(id).await?;

        if let Some(err) = resp.error {
            return Err(anyhow::anyhow!("LSP error: {}", err.message));
        }

        match resp.result {
            Some(Value::Array(locs)) => {
                if locs.is_empty() {
                    return Ok("No references found.".into());
                }
                let lines: Vec<String> = locs
                    .iter()
                    .filter_map(|loc| {
                        let uri = loc["uri"].as_str()?;
                        let line = loc["range"]["start"]["line"].as_u64()? + 1;
                        let path = uri_to_path(uri);
                        Some(format!("{path}:{line}"))
                    })
                    .collect();
                Ok(lines.join("\n"))
            }
            _ => Ok("No references found.".into()),
        }
    }

    pub async fn rename(
        &mut self,
        file: &str,
        line: u32,
        col: u32,
        new_name: &str,
    ) -> Result<String> {
        self.open_file(file).await?;
        let id = next_id();
        let uri = path_to_uri(Path::new(file));
        let req = Request::new(
            id,
            "textDocument/rename",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": col },
                "newName": new_name
            }),
        );

        self.transport.send_request(&req).await?;
        let resp = self.transport.read_response_for(id).await?;

        if let Some(err) = resp.error {
            return Err(anyhow::anyhow!("LSP error: {}", err.message));
        }

        match resp.result {
            Some(Value::Object(workspace_edit)) => format_workspace_edit(&workspace_edit),
            Some(Value::Null) | None => Ok("No changes needed for rename.".into()),
            _ => Ok(format!(
                "Rename result: {}",
                resp.result.unwrap_or_default()
            )),
        }
    }

    pub async fn diagnostics(&mut self, file: Option<&str>) -> Result<String> {
        // LSP sends diagnostics via notifications; we do a lightweight workaround:
        // open the file and wait briefly for publishDiagnostics.
        if let Some(f) = file {
            self.open_file(f).await?;
        }

        // Request pull-model diagnostics (LSP 3.17+).
        let id = next_id();
        let params = if let Some(f) = file {
            let uri = path_to_uri(Path::new(f));
            json!({ "identifier": null, "textDocument": { "uri": uri } })
        } else {
            json!({ "identifier": null })
        };

        let req = Request::new(id, "textDocument/diagnostic", params);
        self.transport.send_request(&req).await?;

        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.transport.read_response_for(id),
        )
        .await
        {
            Ok(Ok(resp)) => {
                if let Some(err) = resp.error {
                    // Method not found is expected for servers that don't support pull diagnostics.
                    if err.code == -32601 {
                        return Ok("This language server doesn't support pull diagnostics. Errors will appear inline as you edit files.".into());
                    }
                    return Err(anyhow::anyhow!("LSP error: {}", err.message));
                }
                format_diagnostics_response(resp.result, file)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Ok("Diagnostics request timed out (server may still be indexing).".into()),
        }
    }
}

fn next_id() -> u64 {
    REQ_ID.fetch_add(1, Ordering::SeqCst)
}

fn path_to_uri(path: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    format!("file://{}", abs.display())
}

fn uri_to_path(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_string()
}

fn guess_language_id(path: &str) -> &'static str {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        _ => "plaintext",
    }
}

fn format_location_response(result: Option<Value>) -> Result<String> {
    match result {
        Some(Value::Array(locs)) if !locs.is_empty() => {
            let loc = &locs[0];
            let uri = loc["uri"].as_str().unwrap_or("?");
            let line = loc["range"]["start"]["line"].as_u64().unwrap_or(0) + 1;
            let col = loc["range"]["start"]["character"].as_u64().unwrap_or(0) + 1;
            let path = uri_to_path(uri);
            Ok(format!("{path}:{line}:{col}"))
        }
        Some(Value::Object(loc)) => {
            let uri = loc["uri"].as_str().unwrap_or("?");
            let line = loc["range"]["start"]["line"].as_u64().unwrap_or(0) + 1;
            let col = loc["range"]["start"]["character"].as_u64().unwrap_or(0) + 1;
            let path = uri_to_path(uri);
            Ok(format!("{path}:{line}:{col}"))
        }
        _ => Ok("Definition not found.".into()),
    }
}

fn format_workspace_edit(edit: &serde_json::Map<String, Value>) -> Result<String> {
    let mut output = String::from("Rename edits (apply with apply_patch):\n");

    // Handle both `changes` (old format) and `documentChanges` (new format).
    if let Some(Value::Object(changes)) = edit.get("changes") {
        for (uri, edits) in changes {
            let path = uri_to_path(uri);
            output.push_str(&format!("\n{path}:\n"));
            if let Some(edits_arr) = edits.as_array() {
                for e in edits_arr {
                    let new_text = e["newText"].as_str().unwrap_or("?");
                    let line = e["range"]["start"]["line"].as_u64().unwrap_or(0) + 1;
                    output.push_str(&format!("  line {line}: → {new_text}\n"));
                }
            }
        }
    } else if let Some(Value::Array(doc_changes)) = edit.get("documentChanges") {
        for change in doc_changes {
            let uri = change["textDocument"]["uri"].as_str().unwrap_or("?");
            let path = uri_to_path(uri);
            output.push_str(&format!("\n{path}:\n"));
            if let Some(edits_arr) = change["edits"].as_array() {
                for e in edits_arr {
                    let new_text = e["newText"].as_str().unwrap_or("?");
                    let line = e["range"]["start"]["line"].as_u64().unwrap_or(0) + 1;
                    output.push_str(&format!("  line {line}: → {new_text}\n"));
                }
            }
        }
    }

    Ok(output)
}

fn format_diagnostics_response(result: Option<Value>, file: Option<&str>) -> Result<String> {
    let label = file.unwrap_or("project");
    match result {
        Some(v) => {
            let items = v["items"].as_array().cloned().unwrap_or_default();
            if items.is_empty() {
                return Ok(format!("No diagnostics for {label}."));
            }
            let mut out = format!("{} diagnostic(s) for {label}:\n", items.len());
            for d in &items {
                let severity = match d["severity"].as_u64() {
                    Some(1) => "error",
                    Some(2) => "warning",
                    Some(3) => "info",
                    _ => "hint",
                };
                let line = d["range"]["start"]["line"].as_u64().unwrap_or(0) + 1;
                let msg = d["message"].as_str().unwrap_or("?");
                out.push_str(&format!("  [{severity}] line {line}: {msg}\n"));
            }
            Ok(out)
        }
        None => Ok(format!("No diagnostics for {label}.")),
    }
}
