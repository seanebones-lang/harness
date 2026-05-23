//! Local auth tokens for HTTP server and daemon IPC (loopback-only services).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const SERVER_TOKEN_FILE: &str = "server.token";
const DAEMON_TOKEN_FILE: &str = "daemon.token";

/// Harness state directory (`~/.harness`).
pub fn harness_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".harness")
}

/// Load or create the HTTP server bearer token.
pub fn server_token() -> Result<String> {
    load_or_create(SERVER_TOKEN_FILE)
}

/// Load or create the daemon IPC token.
pub fn daemon_token() -> Result<String> {
    load_or_create(DAEMON_TOKEN_FILE)
}

/// Read an existing token file (for clients connecting to server/daemon).
pub fn read_token_file(name: &str) -> Result<String> {
    let path = harness_dir().join(name);
    let t = std::fs::read_to_string(&path)
        .with_context(|| format!("missing auth token at {}", path.display()))?;
    let t = t.trim().to_string();
    if t.is_empty() {
        anyhow::bail!("empty auth token at {}", path.display());
    }
    Ok(t)
}

/// Constant-time-ish compare of bearer tokens (trimmed).
pub fn verify(provided: Option<&str>, expected: &str) -> bool {
    match provided.map(str::trim).filter(|s| !s.is_empty()) {
        Some(p) => p == expected,
        None => false,
    }
}

fn load_or_create(name: &str) -> Result<String> {
    let path = harness_dir().join(name);
    if path.exists() {
        let t = std::fs::read_to_string(&path)?.trim().to_string();
        if !t.is_empty() {
            return Ok(t);
        }
    }
    std::fs::create_dir_all(harness_dir())?;
    let token = uuid::Uuid::new_v4().to_string();
    write_secret_file(&path, &token)?;
    Ok(token)
}

fn write_secret_file(path: &Path, contents: &str) -> Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    std::fs::write(path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
