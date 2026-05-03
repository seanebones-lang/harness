pub mod client;
pub mod config;
pub mod tool;

pub use client::{McpClient, McpResource, ProgressEvent, ServerCapabilities};
pub use config::{McpConfig, McpServerConfig};
pub use tool::McpToolAdapter;

use anyhow::Result;
use harness_tools::ToolRegistry;
use std::path::Path;
use tokio::sync::mpsc;
use tracing::{info, warn};

use harness_provider_core::ArcProvider;

/// Load all MCP servers from a config file and register their tools into `registry`.
/// Optionally forwards progress events to a channel.
/// `sampling_provider`: when set, MCP `sampling/createMessage` can call the active LLM.
/// Silently skips servers that fail to initialize (logs warnings).
pub async fn load_mcp_tools(
    config_path: &Path,
    registry: &mut ToolRegistry,
    sampling_provider: Option<ArcProvider>,
) -> Result<()> {
    load_mcp_tools_with_progress(config_path, registry, None, sampling_provider).await
}

/// Load MCP tools with optional progress channel and LLM for MCP sampling.
pub async fn load_mcp_tools_with_progress(
    config_path: &Path,
    registry: &mut ToolRegistry,
    progress_tx: Option<mpsc::UnboundedSender<ProgressEvent>>,
    sampling_provider: Option<ArcProvider>,
) -> Result<()> {
    let cfg = match config::load(config_path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // no config is fine
    };

    for (name, server_cfg) in cfg.mcp_servers {
        match McpClient::spawn_with_opts(
            &name,
            &server_cfg,
            progress_tx.clone(),
            sampling_provider.clone(),
        )
        .await
        {
            Ok(client) => {
                match client.list_tools().await {
                    Ok(tools) => {
                        let count = tools.len();
                        for tool_def in tools {
                            registry.register(McpToolAdapter::new(tool_def, client.clone()));
                        }

                        // Also list and log resources if supported
                        let caps = client.capabilities.lock().await.clone();
                        if caps.has_resources {
                            match client.list_resources().await {
                                Ok(resources) => {
                                    info!(server = %name, tools = count, resources = resources.len(), "loaded MCP server (protocol={})", caps.protocol_version);
                                }
                                Err(e) => {
                                    warn!(server = %name, "resources/list failed: {e}");
                                    info!(server = %name, tools = count, "loaded MCP tools");
                                }
                            }
                        } else {
                            info!(server = %name, tools = count, "loaded MCP tools (protocol={})", caps.protocol_version);
                        }
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
