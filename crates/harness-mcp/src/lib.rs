pub mod client;
pub mod config;
pub mod tool;

pub use client::McpClient;
pub use config::{McpConfig, McpServerConfig};
pub use tool::McpToolAdapter;

use anyhow::Result;
use harness_tools::ToolRegistry;
use std::path::Path;
use tracing::{info, warn};

/// Load all MCP servers from a config file and register their tools into `registry`.
/// Silently skips servers that fail to initialize (logs warnings).
pub async fn load_mcp_tools(config_path: &Path, registry: &mut ToolRegistry) -> Result<()> {
    let cfg = match config::load(config_path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // no config is fine
    };

    for (name, server_cfg) in cfg.mcp_servers {
        match McpClient::spawn(&name, &server_cfg).await {
            Ok(client) => {
                match client.list_tools().await {
                    Ok(tools) => {
                        let count = tools.len();
                        for tool_def in tools {
                            registry.register(McpToolAdapter::new(
                                tool_def,
                                client.clone(),
                            ));
                        }
                        info!(server = %name, tools = count, "loaded MCP tools");
                    }
                    Err(e) => warn!(server = %name, "failed to list tools: {e}"),
                }
            }
            Err(e) => warn!(server = %name, "failed to spawn MCP server: {e}"),
        }
    }

    Ok(())
}

/// Returns the first existing MCP config path found.
pub fn find_config() -> Option<std::path::PathBuf> {
    let candidates = [
        std::path::PathBuf::from(".harness/mcp.json"),
        std::path::PathBuf::from(".claude/mcp.json"),
        dirs::home_dir()?.join(".harness/mcp.json"),
    ];
    candidates.into_iter().find(|p| p.exists())
}
