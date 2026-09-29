use crate::confirm::ConfirmGate;
use crate::confirm::ConfirmResult;
use crate::policy::tool_requires_confirmation;
use crate::registry::Tool as _;
use crate::registry::ToolRegistry;
use crate::tools::TestRunnerTool;
use harness_deadbolt::Deadbolt;
use harness_provider_core::ToolCall;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, warn};

/// When the confirm gate is active, which tool calls require user approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfirmPolicy {
    /// Confirm gate disabled (default).
    #[default]
    Off,
    /// Pause for shell confirm patterns, new-file writes, MCP tools, and `always_ask` rules.
    Smart,
    /// Pause for all destructive operations (same as `--plan`).
    Plan,
}

/// Tools that modify files (trigger autoformat + optional autotest).
const FILE_WRITE_TOOLS: &[&str] = &["write_file", "patch_file", "apply_patch"];

/// Hook invoked when post-write autotest reports failures.
pub type AutotestFailHook = Arc<dyn Fn(&str) + Send + Sync>;

/// Hook invoked after a successful `gh pr_create` (title, url).
pub type GhPrOpenedHook = Arc<dyn Fn(&str, &str) + Send + Sync>;

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
    /// Called when autotest reports failures (desktop notification hook).
    autotest_fail_hook: Option<AutotestFailHook>,
    gh_pr_opened_hook: Option<GhPrOpenedHook>,
    /// Trust rules: (tool, pattern) pairs that bypass the confirm gate.
    trusted: Vec<(String, String)>,
    /// MCP-adapted tool names (require confirmation in plan mode).
    mcp_tool_names: HashSet<String>,
    /// Always-ask rules from config: (tool, pattern).
    always_ask: Vec<(String, String)>,
    /// Smart-mode shell patterns from `[shell].confirm_required`.
    shell_confirm_patterns: Vec<String>,
    /// Tool names that bypass confirmation even in plan/smart mode.
    auto_approve: HashSet<String>,
    /// Which calls require confirmation when the gate is enabled.
    confirm_policy: ConfirmPolicy,
    /// Out-of-band Deadbolt lease. Absent means the gate is not attached.
    admit: Option<Arc<Deadbolt>>,
    /// Agent the lease is bound to. Not a vendor key.
    agent_id: Option<String>,
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
            autotest_fail_hook: None,
            gh_pr_opened_hook: None,
            trusted: Vec::new(),
            mcp_tool_names: HashSet::new(),
            always_ask: Vec::new(),
            shell_confirm_patterns: Vec::new(),
            auto_approve: HashSet::new(),
            confirm_policy: ConfirmPolicy::Off,
            admit: None,
            agent_id: None,
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

    /// Shell patterns that trigger confirmation in smart mode.
    pub fn with_shell_confirm_patterns(mut self, patterns: Vec<String>) -> Self {
        self.shell_confirm_patterns = patterns;
        self
    }

    /// Tool names from `[approval].auto_approve` that skip confirmation.
    pub fn with_auto_approve(mut self, tools: Vec<String>) -> Self {
        self.auto_approve = tools.into_iter().collect();
        self
    }

    /// Set smart vs plan confirmation policy (requires `with_confirm_gate`).
    pub fn with_confirm_policy(mut self, policy: ConfirmPolicy) -> Self {
        self.confirm_policy = policy;
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

    fn matches_always_ask(&self, tool: &str, preview: &str) -> bool {
        self.always_ask.iter().any(|(t, p)| {
            let tool_match = t == tool || t == "*";
            let pat_match = p == "*" || preview.contains(p.as_str());
            tool_match && pat_match
        })
    }

    async fn needs_confirmation(&self, tool: &str, args: &serde_json::Value) -> bool {
        if self.auto_approve.contains(tool) {
            return false;
        }
        if self.confirm_policy == ConfirmPolicy::Off {
            return false;
        }

        let preview = Self::first_arg_preview(args);
        if self.matches_always_ask(tool, preview) {
            return true;
        }

        match self.confirm_policy {
            ConfirmPolicy::Off => false,
            ConfirmPolicy::Plan => {
                self.mcp_tool_names.contains(tool) || tool_requires_confirmation(tool, args)
            }
            ConfirmPolicy::Smart => {
                if self.mcp_tool_names.contains(tool) {
                    return true;
                }
                if tool == "shell" {
                    let cmd = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                    if self
                        .shell_confirm_patterns
                        .iter()
                        .any(|pat| cmd.contains(pat.as_str()))
                    {
                        return true;
                    }
                }
                if matches!(tool, "write_file" | "patch_file") {
                    if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                        if matches!(tokio::fs::try_exists(path).await, Ok(false)) {
                            return true;
                        }
                    }
                }
                false
            }
        }
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

    /// Attach Deadbolt. `admit` runs before any tool body, including MCP adapters.
    pub fn with_deadbolt(mut self, gate: Arc<Deadbolt>, agent_id: impl Into<String>) -> Self {
        self.admit = Some(gate);
        self.agent_id = Some(agent_id.into());
        self
    }

    /// Rebind the lease id. Used when a swarm worker shares a parent executor.
    pub fn with_agent_id(&self, agent_id: impl Into<String>) -> Self {
        let mut out = self.clone();
        out.agent_id = Some(agent_id.into());
        out
    }

    fn deadbolt_deny(&self, tool: &str, args: &serde_json::Value) -> Option<String> {
        let (Some(gate), Some(agent_id)) = (&self.admit, &self.agent_id) else {
            return None;
        };
        if let Some(usd) = spend_of(args) {
            if gate.spend_add(agent_id, usd).is_err() {
                return Some(format!("[deadbolt] denied {tool}: store_unavailable"));
            }
        }
        let dest = dest_of(args);
        match gate.admit_dest(agent_id, tool, dest.as_deref()) {
            harness_deadbolt::AdmitDecision::Allow => None,
            harness_deadbolt::AdmitDecision::Deny { code } => {
                Some(format!("[deadbolt] denied {tool}: {}", code.as_str()))
            }
        }
    }

    /// Kill or pause between confirm and the body. Does not re-admit.
    /// A second admit would consume an irreversible one-shot.
    fn deadbolt_recheck(&self, tool: &str) -> Option<String> {
        let (Some(gate), Some(agent_id)) = (&self.admit, &self.agent_id) else {
            return None;
        };
        match gate.status(Some(agent_id)) {
            Ok(rows) => match rows.first().map(|r| r.state.as_str()).unwrap_or("") {
                "killed" => Some(format!("[deadbolt] denied {tool}: killed")),
                "paused" => Some(format!("[deadbolt] denied {tool}: paused")),
                _ => None,
            },
            Err(_) => Some(format!("[deadbolt] denied {tool}: store_unavailable")),
        }
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

    /// Desktop notification hook when autotest reports failures.
    pub fn with_autotest_fail_hook(mut self, hook: AutotestFailHook) -> Self {
        self.autotest_fail_hook = Some(hook);
        self
    }

    /// Desktop notification hook when `gh pr_create` succeeds.
    pub fn with_gh_pr_opened_hook(mut self, hook: GhPrOpenedHook) -> Self {
        self.gh_pr_opened_hook = Some(hook);
        self
    }

    fn notify_autotest_fail(&self, report: &str) {
        if let Some(hook) = &self.autotest_fail_hook {
            hook(report);
        }
    }

    fn notify_gh_pr_opened(&self, args: &serde_json::Value, output: &str) {
        if args.get("action").and_then(|v| v.as_str()) != Some("pr_create") {
            return;
        }
        let Some(hook) = &self.gh_pr_opened_hook else {
            return;
        };
        let parsed = serde_json::from_str::<serde_json::Value>(output.trim()).ok();
        let title = parsed
            .as_ref()
            .and_then(|v| v.get("title").and_then(|t| t.as_str()))
            .or_else(|| args.get("title").and_then(|v| v.as_str()))
            .unwrap_or("Pull request");
        let url = parsed
            .as_ref()
            .and_then(|v| v.get("url").and_then(|u| u.as_str()))
            .unwrap_or("");
        hook(title, url);
    }

    /// Execute a provider tool call and return string output for the agent loop.
    pub async fn execute(&self, call: &ToolCall) -> String {
        let args = match call.args() {
            Ok(v) => v,
            Err(e) => return format!("Error parsing tool arguments: {e}"),
        };

        // Deadbolt admit is out-of-band and runs before any tool body, confirm
        // write, or MCP adapter execute. Confirm-gate and the workspace jail stay.
        if let Some(denied) = self.deadbolt_deny(&call.function.name, &args) {
            return denied;
        }

        let Some(tool) = self.registry.get(&call.function.name) else {
            warn!(name = %call.function.name, "unknown tool requested");
            return format!("Unknown tool: {}", call.function.name);
        };

        // In plan mode, pause and wait for confirmation before destructive calls.
        // Trusted tool/pattern pairs bypass the confirm gate.
        if let Some(gate) = &self.confirm_gate {
            if self.needs_confirmation(&call.function.name, &args).await {
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
                            if let Some(denied) = self.deadbolt_recheck(&call.function.name) {
                                return denied;
                            }
                            if let Err(e) = tokio::fs::write(&path, &content).await {
                                return format!("Tool error writing {path}: {e}");
                            }
                            let mut result = format!("[plan mode] applied reviewed diff to {path}");
                            if self.autoformat {
                                tokio::spawn(autoformat(path.clone()));
                            }
                            if self.autotest {
                                let scope = self.autotest_scope.clone();
                                let test_args = serde_json::json!({ "scope": scope });
                                if let Ok(report) = TestRunnerTool.execute(test_args).await {
                                    if report.contains("FAIL") {
                                        self.notify_autotest_fail(&report);
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
        if let Some(denied) = self.deadbolt_recheck(&call.function.name) {
            return denied;
        }
        let result = match tool.execute(args.clone()).await {
            Ok(output) => output,
            Err(e) => return format!("Tool error: {e}"),
        };

        if call.function.name == "gh" {
            self.notify_gh_pr_opened(&args, &result);
        }

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
                        self.notify_autotest_fail(&report);
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

    /// Clone executor with tools restricted to `allow` (by tool name).
    /// Empty allowlist leaves the registry unchanged.
    pub fn with_tool_allowlist(&self, allow: &[String]) -> Self {
        let mut out = self.clone();
        out.registry.retain_allowlist(allow);
        out
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

fn dest_of(args: &serde_json::Value) -> Option<String> {
    find_dest(args)
}

fn find_dest(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            for key in ["url", "uri", "href", "endpoint", "host"] {
                if let Some(raw) = map.get(key).and_then(|v| v.as_str()) {
                    if let Some(host) = host_from_field(key, raw) {
                        return Some(host);
                    }
                }
            }
            map.values().find_map(find_dest)
        }
        serde_json::Value::Array(items) => items.iter().find_map(find_dest),
        _ => None,
    }
}

fn host_from_field(key: &str, raw: &str) -> Option<String> {
    if let Some(host) = host_in(raw) {
        return Some(host);
    }
    if key == "host" {
        bare_host(raw)
    } else {
        None
    }
}

fn bare_host(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains('/') || raw.contains(' ') || raw.contains('@') {
        return None;
    }
    let host = raw.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn host_in(raw: &str) -> Option<String> {
    let rest = raw
        .strip_prefix("https://")
        .or_else(|| raw.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    let host = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next()?
    } else {
        host.split(':').next()?
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn spend_of(args: &serde_json::Value) -> Option<f64> {
    find_spend(args)
}

fn find_spend(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Object(map) => {
            for key in ["amount", "usd", "cost"] {
                if let Some(n) = map.get(key).and_then(as_usd) {
                    return Some(n);
                }
            }
            map.values().find_map(find_spend)
        }
        serde_json::Value::Array(items) => items.iter().find_map(find_spend),
        _ => None,
    }
}

fn as_usd(value: &serde_json::Value) -> Option<f64> {
    if let Some(n) = value.as_f64() {
        return n.is_finite().then_some(n);
    }
    let raw = value.as_str()?.trim();
    let n = raw.parse::<f64>().ok()?;
    n.is_finite().then_some(n)
}

#[cfg(test)]
impl ToolExecutor {
    async fn test_needs_confirmation(&self, tool: &str, args: &serde_json::Value) -> bool {
        self.needs_confirmation(tool, args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolRegistry;
    use serde_json::json;
    use std::collections::HashSet;

    fn executor_with_policy(policy: ConfirmPolicy) -> ToolExecutor {
        ToolExecutor::new(ToolRegistry::new())
            .with_confirm_policy(policy)
            .with_shell_confirm_patterns(vec!["git push".into()])
            .with_auto_approve(vec!["read_file".into()])
    }

    #[test]
    fn first_arg_preview_prefers_command_path_action_patch() {
        assert_eq!(
            ToolExecutor::first_arg_preview(&json!({"command": "ls", "path": "x"})),
            "ls"
        );
        assert_eq!(
            ToolExecutor::first_arg_preview(&json!({"path": "a.rs"})),
            "a.rs"
        );
        assert_eq!(
            ToolExecutor::first_arg_preview(&json!({"action": "status"})),
            "status"
        );
        assert_eq!(
            ToolExecutor::first_arg_preview(&json!({"patch": "@@"})),
            "@@"
        );
        assert_eq!(ToolExecutor::first_arg_preview(&json!({})), "");
    }

    #[test]
    fn is_trusted_matches_tool_and_pattern() {
        let ex = ToolExecutor::new(ToolRegistry::new())
            .with_trusted(vec![("shell".into(), "cargo test".into())]);
        assert!(ex.is_trusted("shell", "cargo test -p harness"));
        assert!(!ex.is_trusted("shell", "rm -rf /"));
        assert!(!ex.is_trusted("write_file", "cargo test"));
        let star =
            ToolExecutor::new(ToolRegistry::new()).with_trusted(vec![("*".into(), "*".into())]);
        assert!(star.is_trusted("anything", "x"));
    }

    #[test]
    fn matches_always_ask_wildcard_and_substring() {
        let ex = ToolExecutor::new(ToolRegistry::new()).with_always_ask(vec![
            ("shell".into(), "rm -rf".into()),
            ("*".into(), "prod".into()),
        ]);
        assert!(ex.matches_always_ask("shell", "sudo rm -rf /tmp"));
        assert!(!ex.matches_always_ask("shell", "echo hi"));
        assert!(ex.matches_always_ask("write_file", "deploy-prod.yaml"));
    }

    #[test]
    fn build_preview_shapes() {
        let shell = build_preview("shell", &json!({"command": "echo hi", "cwd": "/tmp"}));
        assert!(shell.contains("$ echo hi"));
        assert!(shell.contains("/tmp"));

        let write = build_preview(
            "write_file",
            &json!({"path": "a.txt", "content": "one\ntwo\nthree"}),
        );
        assert!(write.contains("write a.txt"));
        assert!(write.contains("one"));

        let patch = build_preview(
            "patch_file",
            &json!({"path": "a.rs", "old_string": "a", "new_string": "b"}),
        );
        assert!(patch.contains("patch a.rs"));
        assert!(patch.contains("- a"));
        assert!(patch.contains("+ b"));

        let other = build_preview("git", &json!({"action": "status"}));
        assert!(other.contains("status"));
    }

    #[tokio::test]
    async fn plan_mode_confirms_destructive_shell() {
        let ex = executor_with_policy(ConfirmPolicy::Plan);
        assert!(
            ex.test_needs_confirmation("shell", &json!({"command": "echo hello"}))
                .await
        );
    }

    #[tokio::test]
    async fn smart_mode_skips_benign_shell() {
        let ex = executor_with_policy(ConfirmPolicy::Smart);
        assert!(
            !ex.test_needs_confirmation("shell", &json!({"command": "echo hello"}))
                .await
        );
    }

    #[tokio::test]
    async fn smart_mode_confirms_shell_patterns() {
        let ex = executor_with_policy(ConfirmPolicy::Smart);
        assert!(
            ex.test_needs_confirmation("shell", &json!({"command": "git push origin main"}))
                .await
        );
    }

    #[tokio::test]
    async fn auto_approve_skips_even_in_plan_mode() {
        let ex = executor_with_policy(ConfirmPolicy::Plan);
        assert!(
            !ex.test_needs_confirmation("read_file", &json!({"path": "x"}))
                .await
        );
    }

    #[tokio::test]
    async fn off_policy_never_confirms() {
        let ex = executor_with_policy(ConfirmPolicy::Off)
            .with_always_ask(vec![("*".into(), "*".into())]);
        // Off short-circuits after auto_approve check — always_ask still runs before policy match
        // Actually: Off returns false after always_ask? Looking at code:
        // auto_approve first, then Off check at top after auto_approve... wait:
        // if confirm_policy == Off { return false } comes BEFORE always_ask.
        assert!(
            !ex.test_needs_confirmation("shell", &json!({"command": "rm -rf /"}))
                .await
        );
    }

    #[tokio::test]
    async fn plan_mode_confirms_mcp_tools() {
        let mut names = HashSet::new();
        names.insert("mcp_foo".into());
        let ex = executor_with_policy(ConfirmPolicy::Plan).with_mcp_tool_names(names);
        assert!(ex.test_needs_confirmation("mcp_foo", &json!({})).await);
    }

    #[tokio::test]
    async fn always_ask_forces_confirm_in_smart() {
        let ex = executor_with_policy(ConfirmPolicy::Smart)
            .with_always_ask(vec![("shell".into(), "danger".into())]);
        assert!(
            ex.test_needs_confirmation("shell", &json!({"command": "do danger thing"}))
                .await
        );
    }

    fn call(name: &str) -> ToolCall {
        ToolCall {
            id: "t".into(),
            kind: "function".into(),
            function: harness_provider_core::ToolCallFunction {
                name: name.into(),
                arguments: "{}".into(),
            },
        }
    }

    #[tokio::test]
    async fn deadbolt_deny_after_kill_blocks_next_admit() {
        let dir = tempfile::tempdir().expect("tmp");
        let gate = harness_deadbolt::Deadbolt::open_at(dir.path(), true, 60);
        gate.ensure_agent("agent-a").expect("lease");
        let exec =
            ToolExecutor::new(ToolRegistry::new()).with_deadbolt(Arc::new(gate.clone()), "agent-a");
        let allowed = exec.execute(&call("read_file")).await;
        assert!(!allowed.contains("[deadbolt]"));
        gate.kill("agent-a").expect("kill");
        let denied = exec.execute(&call("read_file")).await;
        assert!(denied.contains("[deadbolt]"));
        assert!(denied.contains("killed"));
        let other =
            ToolExecutor::new(ToolRegistry::new()).with_deadbolt(Arc::new(gate.clone()), "agent-b");
        gate.ensure_agent("agent-b").expect("b lease");
        let b = other.execute(&call("read_file")).await;
        assert!(!b.contains("[deadbolt]"), "killing A must not block B: {b}");
    }

    #[test]
    fn model_definitions_have_no_shutdown_tool() {
        let names = [
            "read_file",
            "shell",
            "spawn_agent",
            "spawn_swarm",
            "write_file",
        ];
        for name in names {
            assert!(!harness_deadbolt::is_shutdown_tool(name));
        }
        assert!(harness_deadbolt::is_shutdown_tool("shutdown"));
        assert!(harness_deadbolt::is_shutdown_tool("deadbolt"));
    }

    struct FlagTool {
        ran: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait::async_trait]
    impl crate::registry::Tool for FlagTool {
        fn definition(&self) -> harness_provider_core::ToolDefinition {
            harness_provider_core::ToolDefinition::new("shell", "s", json!({}))
        }

        async fn execute(&self, _: serde_json::Value) -> anyhow::Result<String> {
            self.ran.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok("ran".into())
        }
    }

    fn flag_exec(
        gate: &harness_deadbolt::Deadbolt,
        agent: &str,
    ) -> (ToolExecutor, std::sync::Arc<std::sync::atomic::AtomicBool>) {
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut reg = ToolRegistry::new();
        reg.register(FlagTool { ran: ran.clone() });
        let exec = ToolExecutor::new(reg).with_deadbolt(Arc::new(gate.clone()), agent);
        (exec, ran)
    }

    fn call_args(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "t".into(),
            kind: "function".into(),
            function: harness_provider_core::ToolCallFunction {
                name: name.into(),
                arguments: args.to_string(),
            },
        }
    }

    #[tokio::test]
    async fn dest_policy_denies_foreign_host_and_missing_host() {
        let dir = tempfile::tempdir().expect("tmp");
        let gate = harness_deadbolt::Deadbolt::open_at(dir.path(), true, 60);
        gate.ensure_agent("agent-a").expect("lease");
        gate.set_policy(
            "agent-a",
            harness_deadbolt::PolicyPatch {
                dest_allow: Some(vec!["api.stripe.com".into()]),
                ..harness_deadbolt::PolicyPatch::default()
            },
        )
        .expect("policy");
        let (exec, ran) = flag_exec(&gate, "agent-a");
        let foreign = exec
            .execute(&call_args(
                "shell",
                json!({"url": "https://github.com/acme"}),
            ))
            .await;
        assert!(foreign.contains("purpose_exceeded"));
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
        let missing = exec
            .execute(&call_args("shell", json!({"note": "no host"})))
            .await;
        assert!(missing.contains("purpose_exceeded"));
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn irreversible_needs_human_does_not_run_until_approve() {
        let dir = tempfile::tempdir().expect("tmp");
        let gate = harness_deadbolt::Deadbolt::open_at(dir.path(), true, 60);
        gate.ensure_agent("agent-a").expect("lease");
        gate.set_policy(
            "agent-a",
            harness_deadbolt::PolicyPatch {
                irreversible: Some(vec!["shell".into()]),
                ..harness_deadbolt::PolicyPatch::default()
            },
        )
        .expect("policy");
        let (exec, ran) = flag_exec(&gate, "agent-a");
        let denied = exec.execute(&call("shell")).await;
        assert!(denied.contains("needs_human"));
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
        gate.approve("agent-a", "shell").expect("approve");
        let allowed = exec.execute(&call("shell")).await;
        assert_eq!(allowed, "ran");
        assert!(ran.load(std::sync::atomic::Ordering::SeqCst));
        ran.store(false, std::sync::atomic::Ordering::SeqCst);
        let again = exec.execute(&call("shell")).await;
        assert!(again.contains("needs_human"));
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn spend_cap_does_not_run_body() {
        let dir = tempfile::tempdir().expect("tmp");
        let gate = harness_deadbolt::Deadbolt::open_at(dir.path(), true, 60);
        gate.ensure_agent("agent-a").expect("lease");
        gate.set_policy(
            "agent-a",
            harness_deadbolt::PolicyPatch {
                spend_cap_usd: Some(5.0),
                ..harness_deadbolt::PolicyPatch::default()
            },
        )
        .expect("policy");
        gate.spend_add("agent-a", 6.0).expect("spend");
        let (exec, ran) = flag_exec(&gate, "agent-a");
        let denied = exec.execute(&call("shell")).await;
        assert!(denied.contains("spend_cap"), "{denied}");
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
    }
}
