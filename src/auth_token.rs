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
    load_or_create_in(&harness_dir(), SERVER_TOKEN_FILE)
}

/// Load or create the daemon IPC token.
pub fn daemon_token() -> Result<String> {
    load_or_create_in(&harness_dir(), DAEMON_TOKEN_FILE)
}

/// Read an existing token file (for clients connecting to server/daemon).
pub fn read_token_file(name: &str) -> Result<String> {
    read_token_file_in(&harness_dir(), name)
}

/// Constant-time compare of bearer tokens (trimmed).
pub fn verify(provided: Option<&str>, expected: &str) -> bool {
    let Some(p) = provided.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    if p.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in p.bytes().zip(expected.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// Read token from a specific harness state directory.
pub fn read_token_file_in(dir: &Path, name: &str) -> Result<String> {
    let path = dir.join(name);
    let t = std::fs::read_to_string(&path)
        .with_context(|| format!("missing auth token at {}", path.display()))?;
    let t = t.trim().to_string();
    if t.is_empty() {
        anyhow::bail!("empty auth token at {}", path.display());
    }
    Ok(t)
}

/// Load or create token under `dir` (used by server/daemon; tests pass temp dirs).
pub fn load_or_create_in(dir: &Path, name: &str) -> Result<String> {
    let path = dir.join(name);
    if path.exists() {
        let t = std::fs::read_to_string(&path)?.trim().to_string();
        if !t.is_empty() {
            return Ok(t);
        }
    }
    std::fs::create_dir_all(dir)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn verify_accepts_matching_token() {
        assert!(verify(Some("secret-token"), "secret-token"));
        assert!(verify(Some("  secret-token  "), "secret-token"));
    }

    #[test]
    fn verify_rejects_wrong_or_missing_token() {
        assert!(!verify(Some("wrong"), "secret-token"));
        assert!(!verify(None, "secret-token"));
        assert!(!verify(Some(""), "secret-token"));
        assert!(!verify(Some("   "), "secret-token"));
        assert!(!verify(Some("secret-token-extra"), "secret-token"));
    }

    #[test]
    fn verify_is_constant_time_for_equal_length() {
        assert!(!verify(Some("aaaa"), "bbbb"));
    }

    #[test]
    fn load_or_create_roundtrip_and_stable() {
        let dir = tempdir().unwrap();
        let t1 = load_or_create_in(dir.path(), "server.token").unwrap();
        assert!(!t1.is_empty());
        let t2 = load_or_create_in(dir.path(), "server.token").unwrap();
        assert_eq!(t1, t2);
        let read = read_token_file_in(dir.path(), "server.token").unwrap();
        assert_eq!(read, t1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join("server.token"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn read_token_file_rejects_missing_and_empty() {
        let dir = tempdir().unwrap();
        assert!(read_token_file_in(dir.path(), "nope.token").is_err());
        fs::write(dir.path().join("empty.token"), "   \n").unwrap();
        assert!(read_token_file_in(dir.path(), "empty.token").is_err());
    }

    #[test]
    fn empty_existing_file_is_regenerated() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("daemon.token"), "").unwrap();
        let t = load_or_create_in(dir.path(), "daemon.token").unwrap();
        assert!(!t.is_empty());
        assert_eq!(read_token_file_in(dir.path(), "daemon.token").unwrap(), t);
    }
}
