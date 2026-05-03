use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub provider: ProviderConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub session: SessionConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub mcp: McpConfigSection,
    #[serde(default)]
    pub browser: BrowserConfig,
    #[serde(default)]
    pub shell: ShellConfig,
    #[serde(default)]
    pub approval: ApprovalConfig,
    #[serde(default)]
    pub autotest: AutotestConfig,
    #[serde(default)]
    pub native_tools: NativeToolsConfig,
    #[serde(default)]
    pub computer_use: ComputerUseConfig,
    #[serde(default)]
    pub budget: BudgetConfig,
    #[serde(default)]
    pub notifications: NotificationsConfig,
    /// Named provider configs for the multi-provider router.
    #[serde(default)]
    pub providers: std::collections::HashMap<String, harness_provider_router::ProviderEntry>,
    /// Router configuration (fast/heavy/embed model routing, fallback order).
    #[serde(default)]
    pub router: harness_provider_router::RouterConfig,
    /// Tool sandbox / filesystem jail (`[tools]` in config).
    #[serde(default)]
    pub tools: ToolsConfig,
}

/// Tools and sandbox settings.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ToolsConfig {
    /// Filesystem sandbox: `strict` (default), `relaxed`, or `off`.
    pub sandbox: Option<String>,
}

/// Shell execution safety configuration.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ShellConfig {
    /// Commands or prefixes that are unconditionally blocked (returns an error).
    /// Defaults are catastrophic/destructive operations.
    pub denylist: Option<Vec<String>>,
    /// Commands matching these patterns always prompt for confirmation (even without --plan).
    pub confirm_required: Option<Vec<String>>,
    /// Path to the shell command log file. Defaults to ~/.harness/shell.log.
    pub log_path: Option<PathBuf>,
    /// Set to true to disable command logging entirely.
    pub no_log: Option<bool>,
    /// If set, only these command prefixes (e.g. `/usr/bin/git`) may appear as argv0 for absolute-path invocations.
    pub cmd_allowlist: Option<Vec<String>>,
}

impl ShellConfig {
    /// Effective denylist: caller-supplied list, or built-in catastrophic defaults.
    pub fn effective_denylist(&self) -> Vec<String> {
        self.denylist.clone().unwrap_or_else(|| {
            vec![
                "rm -rf /".into(),
                "rm -rf ~/".into(),
                "rm -rf ~".into(),
                "dd if=".into(),
                ":(){:|:&};:".into(),
                "mkfs".into(),
                "git push --force origin main".into(),
                "git push --force origin master".into(),
                "git push -f origin main".into(),
                "git push -f origin master".into(),
            ]
        })
    }

    /// Effective confirm-required patterns.
    pub fn effective_confirm_required(&self) -> Vec<String> {
        self.confirm_required.clone().unwrap_or_else(|| {
            vec![
                "git push".into(),
                "git reset --hard".into(),
                "npm publish".into(),
                "cargo publish".into(),
                "rm -rf".into(),
            ]
        })
    }
}

/// Native provider-managed server-side tools (billed separately per call).
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct NativeToolsConfig {
    /// Enable the provider's native web search tool (Anthropic: web_search, xAI: web_search).
    pub web_search: Option<bool>,
    /// Enable code execution in a sandboxed environment (Anthropic: bash, xAI: code_execution).
    pub code_execution: Option<bool>,
    /// Enable X (Twitter) post search — xAI only.
    pub x_search: Option<bool>,
}

impl NativeToolsConfig {
    #[allow(dead_code)]
    pub fn web_search_enabled(&self) -> bool {
        self.web_search.unwrap_or(false)
    }
    #[allow(dead_code)]
    pub fn code_execution_enabled(&self) -> bool {
        self.code_execution.unwrap_or(false)
    }
    #[allow(dead_code)]
    pub fn x_search_enabled(&self) -> bool {
        self.x_search.unwrap_or(false)
    }
}

/// Budget thresholds — warn when spending approaches limits.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct BudgetConfig {
    /// Maximum daily spend in USD before warnings appear.
    pub daily_usd: Option<f64>,
    /// Maximum monthly spend in USD before warnings appear.
    pub monthly_usd: Option<f64>,
}

/// Desktop notification configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NotificationsConfig {
    /// Enable desktop notifications (default: true on macOS/Linux, false if headless).
    pub enabled: bool,
    /// Notify when a background agent run completes.
    pub on_background_done: bool,
    /// Notify when auto-test fails.
    pub on_autotest_fail: bool,
    /// Notify when budget threshold is hit.
    pub on_budget: bool,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            on_background_done: true,
            on_autotest_fail: true,
            on_budget: true,
        }
    }
}

/// Computer-use configuration (Anthropic computer-use-2025-01-24).
/// DANGER: When enabled, the agent can control your mouse and keyboard.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ComputerUseConfig {
    /// Enable computer use. ONLY enabled when explicitly set to true.
    pub enabled: Option<bool>,
}

impl ComputerUseConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(false)
    }
}

/// Auto-test configuration — run tests after file writes and feed failures back to the agent.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct AutotestConfig {
    /// Enable auto-test after write_file / patch_file / apply_patch.
    pub enabled: bool,
    /// Scope: "package" (default) tries to run tests for the affected package only.
    pub scope: Option<String>,
}

/// Approval policy — controls when the agent pauses for user confirmation.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ApprovalConfig {
    /// "auto" = never pause (default), "smart" = pause for writes to untracked files
    /// and shell denylist matches, "plan" = pause for all destructive operations.
    pub mode: Option<String>,
    /// Tool calls that are always auto-approved (matched by tool name).
    pub auto_approve: Option<Vec<String>>,
    /// Tool calls that always require confirmation (matched by "tool:pattern" or just "tool").
    pub always_ask: Option<Vec<String>>,
}

impl ApprovalConfig {
    /// "auto" (default), "smart", or "plan".
    #[allow(dead_code)]
    pub fn effective_mode(&self) -> &str {
        self.mode.as_deref().unwrap_or("auto")
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct BrowserConfig {
    /// Set to true to enable the browser tool automatically (no --browser flag needed).
    pub enabled: Option<bool>,
    /// Chrome DevTools remote URL (default: http://localhost:9222).
    pub url: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct McpConfigSection {
    /// Path to mcp.json (defaults to .harness/mcp.json or ~/.harness/mcp.json).
    pub config_path: Option<PathBuf>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub base_url: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    pub system_prompt: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct SessionConfig {
    pub db_path: Option<PathBuf>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct MemoryConfig {
    /// Set to false to disable semantic memory entirely.
    pub enabled: Option<bool>,
    /// Embedding model for semantic memory (default: nomic-embed-text via Ollama, or voyage-3.5).
    pub embed_model: Option<String>,
    /// Override path for the memory SQLite DB.
    pub db_path: Option<PathBuf>,
}

pub fn load(path: Option<&Path>) -> Result<Config> {
    let candidates: Vec<PathBuf> = if let Some(p) = path {
        vec![p.to_path_buf()]
    } else {
        vec![
            PathBuf::from(".harness/config.toml"),
            dirs::home_dir()
                .unwrap_or_default()
                .join(".harness/config.toml"),
        ]
    };

    for candidate in &candidates {
        if candidate.exists() {
            let raw = std::fs::read_to_string(candidate)?;
            let cfg: Config = toml::from_str(&raw)?;
            return Ok(cfg);
        }
    }

    Ok(Config::default())
}
