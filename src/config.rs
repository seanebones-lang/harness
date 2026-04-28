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
