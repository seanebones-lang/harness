pub mod client;
pub mod config;
pub mod tool;

pub use client::{
    collect_roots, McpClient, McpResource, ProgressEvent, SamplingApprovalRequest,
    ServerCapabilities,
};
pub use config::{McpConfig, McpServerConfig};
pub use tool::McpToolAdapter;

use anyhow::Result;
use harness_tools::ToolRegistry;
use std::path::Path;
use std::time::Duration;
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
    command_allowlist: Option<&[String]>,
    sampling_auto_approve: bool,
) -> Result<()> {
    load_mcp_tools_with_progress(
        config_path,
        registry,
        None,
        sampling_provider,
        command_allowlist,
        sampling_auto_approve,
        None,
    )
    .await
}

/// Load MCP tools with optional progress channel and LLM for MCP sampling.
pub async fn load_mcp_tools_with_progress(
    config_path: &Path,
    registry: &mut ToolRegistry,
    progress_tx: Option<mpsc::UnboundedSender<ProgressEvent>>,
    sampling_provider: Option<ArcProvider>,
    command_allowlist: Option<&[String]>,
    sampling_auto_approve: bool,
    sampling_tx: Option<mpsc::UnboundedSender<SamplingApprovalRequest>>,
) -> Result<()> {
    let cfg = match config::load(config_path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // no config is fine
    };

    for (name, server_cfg) in cfg.mcp_servers {
        if !command_is_allowed(&server_cfg.command, command_allowlist) {
            warn!(
                server = %name,
                command = %server_cfg.command,
                "skipping MCP server: command not in allowlist"
            );
            continue;
        }
        let spawn_fut = McpClient::spawn_with_opts(
            &name,
            &server_cfg,
            progress_tx.clone(),
            sampling_provider.clone(),
            sampling_auto_approve,
            sampling_tx.clone(),
        );
        match tokio::time::timeout(Duration::from_secs(15), spawn_fut).await {
            Ok(Ok(client)) => {
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
            Ok(Err(e)) => warn!(server = %name, "failed to spawn MCP server: {e}"),
            Err(_) => warn!(server = %name, "MCP server spawn timed out after 15s"),
        }
    }

    Ok(())
}

/// When `allowlist` is unset, a conservative default list is enforced.
const DEFAULT_COMMAND_ALLOWLIST: &[&str] = &["npx", "node", "python3", "uvx"];

/// When `allowlist` is set and non-empty, only spawn servers whose command basename matches.
fn command_is_allowed(command: &str, allowlist: Option<&[String]>) -> bool {
    let effective: Vec<&str> = match allowlist {
        None => DEFAULT_COMMAND_ALLOWLIST.to_vec(),
        Some([]) => return true,
        Some(list) => list.iter().map(String::as_str).collect(),
    };
    let cmd = std::path::Path::new(command)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(command);
    effective
        .iter()
        .any(|allowed| *allowed == command || *allowed == cmd)
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

#[cfg(test)]
mod allowlist_tests {
    use super::command_is_allowed;

    #[test]
    fn default_allowlist_blocks_unknown_commands() {
        assert!(!command_is_allowed("/usr/bin/evil", None));
        assert!(command_is_allowed("npx", None));
    }

    #[test]
    fn explicit_empty_allowlist_allows_all() {
        assert!(command_is_allowed("/usr/bin/evil", Some(&[])));
    }

    #[test]
    fn allowlist_matches_basename_or_full_path() {
        let list = vec!["npx".to_string(), "/opt/bin/mcp".to_string()];
        assert!(command_is_allowed("npx", Some(&list)));
        assert!(command_is_allowed("/usr/local/bin/npx", Some(&list)));
        assert!(command_is_allowed("/opt/bin/mcp", Some(&list)));
        assert!(!command_is_allowed("python3", Some(&list)));
    }

    #[test]
    fn default_allowlist_includes_common_runners() {
        for cmd in ["npx", "node", "python3", "uvx"] {
            assert!(command_is_allowed(cmd, None), "{cmd}");
            assert!(
                command_is_allowed(&format!("/usr/local/bin/{cmd}"), None),
                "path {cmd}"
            );
        }
        assert!(!command_is_allowed("bash", None));
        assert!(!command_is_allowed("/bin/sh", None));
    }

    #[test]
    fn allowlist_rejects_similar_but_not_exact_basenames() {
        let list = vec!["node".to_string()];
        assert!(!command_is_allowed("nodejs", Some(&list)));
        assert!(!command_is_allowed("node.exe", Some(&list)));
        // Full path must match exactly when listed as full path only
        let full = vec!["/opt/bin/mcp".to_string()];
        assert!(!command_is_allowed("/other/mcp", Some(&full)));
        assert!(command_is_allowed("/opt/bin/mcp", Some(&full)));
    }
}
