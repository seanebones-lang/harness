use crate::confirm::ConfirmGate;
use crate::confirm::ConfirmResult;
use crate::policy::tool_requires_confirmation;
use crate::registry::Tool as _;
use crate::registry::ToolRegistry;
use crate::tools::TestRunnerTool;
use harness_provider_core::ToolCall;
use std::collections::HashSet;
use tracing::{debug, warn};

/// Tools that modify files (trigger autoformat + optional autotest).
const FILE_WRITE_TOOLS: &[&str] = &["write_file", "patch_file", "apply_patch"];

/// Runs registered tools on behalf of the agent, with optional confirm gate and hooks.
#[derive(Clone)]
pub struct ToolExecutor {
    registry: ToolRegistry,
    /// When set, destructive tools pause and ask for confirmation before executing.
    confirm_gate: Option<ConfirmGate>,
    /// When true, run autoformat on written files after each write.
    autoformat: bool,
    /// When true, run test_runner after each write and append failures to result.
    autotest: bool,
    /// Optional scope to pass to test_runner (package name, file, etc.).
    autotest_scope: Option<String>,
    /// Trust rules: (tool, pattern) pairs that bypass the confirm gate.
    trusted: Vec<(String, String)>,
    /// MCP-adapted tool names (require confirmation in plan mode).
    mcp_tool_names: HashSet<String>,
    /// Always-ask rules from config: (tool, pattern).
    always_ask: Vec<(String, String)>,
}

impl ToolExecutor {
    /// Build an executor over the given tool registry.
    pub fn new(registry: ToolRegistry) -> Self {
        Self {
            registry,
            confirm_gate: None,
            autoformat: true,
            autotest: false,
            autotest_scope: None,
            trusted: Vec::new(),
            mcp_tool_names: HashSet::new(),
            always_ask: Vec::new(),
        }
    }

    /// Set trusted tool/pattern pairs that bypass the confirm gate.
    pub fn with_trusted(mut self, rules: Vec<(String, String)>) -> Self {
        self.trusted = rules;
        self
    }

    /// Attach always-ask rules from `[approval].always_ask`.
    pub fn with_always_ask(mut self, rules: Vec<(String, String)>) -> Self {
        self.always_ask = rules;
        self
    }

    /// Mark MCP tool names that require confirmation in plan mode.
    pub fn with_mcp_tool_names(mut self, names: HashSet<String>) -> Self {
        self.mcp_tool_names = names;
        self
    }

    fn first_arg_preview(args: &serde_json::Value) -> &str {
        args.get("command")
            .or_else(|| args.get("path"))
            .or_else(|| args.get("action"))
            .or_else(|| args.get("patch"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    }

    fn needs_confirmation(&self, tool: &str, args: &serde_json::Value) -> bool {
        if self.mcp_tool_names.contains(tool) {
            return true;
        }
        if tool_requires_confirmation(tool, args) {
            return true;
        }
        let preview = Self::first_arg_preview(args);
        for (t, p) in &self.always_ask {
            let tool_match = t == tool || t == "*";
            let pat_match = p == "*" || preview.contains(p.as_str());
            if tool_match && pat_match {
                return true;
            }
        }
        false
    }

    fn is_trusted(&self, tool: &str, first_arg: &str) -> bool {
        for (t, p) in &self.trusted {
            let tool_match = t == tool || t == "*";
            let pat_match = p == "*" || first_arg.contains(p.as_str());
            if tool_match && pat_match {
                return true;
            }
        }
        false
    }

    /// Attach a confirmation gate (enables plan/approve mode).
    pub fn with_confirm_gate(mut self, gate: ConfirmGate) -> Self {
        self.confirm_gate = Some(gate);
        self
    }

    /// Disable the post-write autoformat hook.
    pub fn without_autoformat(mut self) -> Self {
        self.autoformat = false;
        self
    }

    /// Enable auto-test after file writes.
    pub fn with_autotest(mut self, scope: Option<String>) -> Self {
        self.autotest = true;
        self.autotest_scope = scope;
        self
    }

    /// Execute a provider tool call and return string output for the agent loop.
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
        // Trusted tool/pattern pairs bypass the confirm gate.
        if let Some(gate) = &self.confirm_gate {
            if self.needs_confirmation(&call.function.name, &args) {
                let first_arg = Self::first_arg_preview(&args);
                if !self.is_trusted(&call.function.name, first_arg) {
                    let preview = build_preview(&call.function.name, &args);
                    match gate
                        .request(&call.function.name, preview, Some(args.clone()))
                        .await
                    {
                        ConfirmResult::Deny => {
                            return format!(
                                "[plan mode] '{}' was skipped by user.",
                                call.function.name
                            );
                        }
                        ConfirmResult::ApplyContent { path, content } => {
                            if let Err(e) = tokio::fs::write(&path, &content).await {
                                return format!("Tool error writing {path}: {e}");
                            }
                            let mut result =
                                format!("[plan mode] applied reviewed diff to {path}");
                            if self.autoformat {
                                tokio::spawn(autoformat(path.clone()));
                            }
                            if self.autotest {
                                let scope = self.autotest_scope.clone();
                                let test_args = serde_json::json!({ "scope": scope });
                                if let Ok(report) = TestRunnerTool.execute(test_args).await {
                                    if report.contains("FAIL") {
                                        result = format!("{result}\n\n[autotest]\n{report}");
                                    } else {
                                        result = format!("{result}\n\n[autotest] {report}");
                                    }
                                }
                            }
                            return result;
                        }
                        ConfirmResult::Approve => {}
                    }
                }
            }
        }

        debug!(tool = %call.function.name, "executing tool");
        let result = match tool.execute(args.clone()).await {
            Ok(output) => output,
            Err(e) => return format!("Tool error: {e}"),
        };

        let is_file_write = FILE_WRITE_TOOLS.contains(&call.function.name.as_str());

        // Post-write autoformat hook: best-effort, non-blocking.
        if self.autoformat && is_file_write {
            if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                tokio::spawn(autoformat(path.to_string()));
            }
        }

        // Auto-test loop: run tests and append failures to result so the agent self-corrects.
        if self.autotest && is_file_write {
            let scope = self.autotest_scope.clone();
            let test_args = serde_json::json!({ "scope": scope });
            match TestRunnerTool.execute(test_args).await {
                Ok(report) => {
                    if report.contains("FAIL") {
                        return format!("{result}\n\n[autotest]\n{report}");
                    }
                    // Tests passed — append brief confirmation.
                    return format!("{result}\n\n[autotest] {report}");
                }
                Err(e) => {
                    warn!("autotest failed to run: {e}");
                }
            }
        }

        result
    }

    /// Underlying tool registry.
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    /// Whether plan/approve confirmation is enabled.
    pub fn has_confirm_gate(&self) -> bool {
        self.confirm_gate.is_some()
    }
}

/// Run the appropriate formatter on `path` based on its extension.
/// Best-effort: errors are silently ignored (the formatter may not be installed).
async fn autoformat(path: String) {
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let (prog, args): (&str, Vec<&str>) = match ext.as_str() {
        "rs" => ("rustfmt", vec!["--edition", "2021", &path]),
        "ts" | "tsx" | "js" | "jsx" | "json" | "css" | "html" => {
            ("prettier", vec!["--write", &path])
        }
        "py" => ("ruff", vec!["format", &path]),
        "go" => ("gofmt", vec!["-w", &path]),
        _ => return,
    };

    let _ = tokio::process::Command::new(prog)
        .args(&args)
        .output()
        .await;
}

/// Build a human-readable preview of the proposed action.
fn build_preview(tool_name: &str, args: &serde_json::Value) -> String {
    match tool_name {
        "shell" => {
            let cmd = args["command"].as_str().unwrap_or("(unknown)");
            let cwd = args["cwd"]
                .as_str()
                .map(|c| format!(" (in {c})"))
                .unwrap_or_default();
            format!("$ {cmd}{cwd}")
        }
        "write_file" => {
            let path = args["path"].as_str().unwrap_or("(unknown)");
            let content = args["content"].as_str().unwrap_or("");
            let lines: Vec<&str> = content.lines().take(20).collect();
            let truncated = if content.lines().count() > 20 {
                "\n…(truncated)"
            } else {
                ""
            };
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
