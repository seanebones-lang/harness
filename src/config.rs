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
    /// Named provider configs for the multi-provider router.
    #[serde(default)]
    pub providers: std::collections::HashMap<String, harness_provider_router::ProviderEntry>,
    /// Router configuration (fast/heavy/embed model routing, fallback order).
    #[serde(default)]
    pub router: harness_provider_router::RouterConfig,
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
    /// xAI embedding model (default: grok-3-embed-english).
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
