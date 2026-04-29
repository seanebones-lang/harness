use crate::confirm::ConfirmGate;
use crate::registry::ToolRegistry;
use harness_provider_core::ToolCall;
use tracing::{debug, warn};

/// Tools that require explicit confirmation in plan mode.
const DESTRUCTIVE_TOOLS: &[&str] = &["write_file", "patch_file", "shell"];

#[derive(Clone)]
pub struct ToolExecutor {
    registry: ToolRegistry,
    /// When set, destructive tools pause and ask for confirmation before executing.
    confirm_gate: Option<ConfirmGate>,
}

impl ToolExecutor {
    pub fn new(registry: ToolRegistry) -> Self {
        Self { registry, confirm_gate: None }
    }

    /// Attach a confirmation gate (enables plan/approve mode).
    pub fn with_confirm_gate(mut self, gate: ConfirmGate) -> Self {
        self.confirm_gate = Some(gate);
        self
    }

    pub async fn execute(&self, call: &ToolCall) -> String {
        let args = match call.args() {
            Ok(v) => v,
            Err(e) => return format!("Error parsing tool arguments: {e}"),
        };

        let Some(tool) = self.registry.get(&call.function.name) else {
            warn!(name = %call.function.name, "unknown tool requested");
            return format!("Unknown tool: {}", call.function.name);
        };

        // In plan mode, pause and wait for confirmation before destructive calls.
        if let Some(gate) = &self.confirm_gate {
            if DESTRUCTIVE_TOOLS.contains(&call.function.name.as_str()) {
                let preview = build_preview(&call.function.name, &args);
                let approved = gate.request(&call.function.name, preview).await;
                if !approved {
                    return format!("[plan mode] '{}' was skipped by user.", call.function.name);
                }
            }
        }

        debug!(tool = %call.function.name, "executing tool");
        match tool.execute(args).await {
            Ok(output) => output,
            Err(e) => format!("Tool error: {e}"),
        }
    }

    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    pub fn has_confirm_gate(&self) -> bool {
        self.confirm_gate.is_some()
    }
}

/// Build a human-readable preview of the proposed action.
fn build_preview(tool_name: &str, args: &serde_json::Value) -> String {
    match tool_name {
        "shell" => {
            let cmd = args["command"].as_str().unwrap_or("(unknown)");
            let cwd = args["cwd"].as_str().map(|c| format!(" (in {c})")).unwrap_or_default();
            format!("$ {cmd}{cwd}")
        }
        "write_file" => {
            let path = args["path"].as_str().unwrap_or("(unknown)");
            let content = args["content"].as_str().unwrap_or("");
            let lines: Vec<&str> = content.lines().take(20).collect();
            let truncated = if content.lines().count() > 20 { "\n…(truncated)" } else { "" };
            format!("write {path}\n{}{}", lines.join("\n"), truncated)
        }
        "patch_file" => {
            let path = args["path"].as_str().unwrap_or("(unknown)");
            let old = args["old_string"].as_str().unwrap_or("(none)");
            let new = args["new_string"].as_str().unwrap_or("(none)");
            let old_preview: String = old.lines().take(8).map(|l| format!("- {l}\n")).collect();
            let new_preview: String = new.lines().take(8).map(|l| format!("+ {l}\n")).collect();
            format!("patch {path}\n{old_preview}{new_preview}")
        }
        _ => serde_json::to_string_pretty(args).unwrap_or_default(),
    }
}
