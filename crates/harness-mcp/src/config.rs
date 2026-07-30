use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Top-level MCP config file format (compatible with Claude Code's mcp.json).
#[derive(Debug, Deserialize, Serialize)]
pub struct McpConfig {
    #[serde(rename = "mcpServers")]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerConfig {
    /// Executable to run (e.g. "npx", "python", "/usr/local/bin/my-mcp-server").
    pub command: String,
    /// Arguments passed to the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables to set.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

pub fn load(path: &Path) -> Result<McpConfig> {
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_path(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("harness-mcp-cfg-{name}-{nanos}.json"))
    }

    #[test]
    fn load_parses_mcp_servers_map() {
        let path = scratch_path("demo");
        std::fs::write(
            &path,
            r#"{
              "mcpServers": {
                "demo": {
                  "command": "npx",
                  "args": ["-y", "server"],
                  "env": {"FOO": "bar"}
                }
              }
            }"#,
        )
        .unwrap();
        let cfg = load(&path).expect("load");
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.mcp_servers.len(), 1);
        let demo = cfg.mcp_servers.get("demo").expect("demo");
        assert_eq!(demo.command, "npx");
        assert_eq!(demo.args, vec!["-y", "server"]);
        assert_eq!(demo.env.get("FOO").map(String::as_str), Some("bar"));
    }

    #[test]
    fn load_defaults_missing_args_and_env() {
        let path = scratch_path("bare");
        std::fs::write(&path, r#"{"mcpServers":{"bare":{"command":"node"}}}"#).unwrap();
        let cfg = load(&path).expect("load");
        let _ = std::fs::remove_file(&path);
        let bare = cfg.mcp_servers.get("bare").unwrap();
        assert!(bare.args.is_empty());
        assert!(bare.env.is_empty());
    }

    #[test]
    fn load_rejects_invalid_json() {
        let path = scratch_path("bad");
        std::fs::write(&path, "{not json").unwrap();
        assert!(load(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_missing_file_errors() {
        let path = scratch_path("missing");
        let _ = std::fs::remove_file(&path);
        assert!(load(&path).is_err());
    }
}
