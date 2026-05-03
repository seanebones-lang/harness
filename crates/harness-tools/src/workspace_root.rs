//! Enforce filesystem tool paths under a project workspace root (defense in depth).

use path_clean::PathClean;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::warn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxMode {
    #[default]
    Strict,
    Relaxed,
    Off,
}

impl SandboxMode {
    pub fn from_config(s: Option<&str>) -> Self {
        match s.map(str::to_lowercase).as_deref() {
            Some("off") => SandboxMode::Off,
            Some("relaxed") => SandboxMode::Relaxed,
            _ => SandboxMode::Strict,
        }
    }
}

/// Canonical-ish project root for sandboxing `read_file`, `write_file`, etc.
#[derive(Debug, Clone)]
pub struct WorkspaceRoot {
    root: PathBuf,
    mode: SandboxMode,
}

impl WorkspaceRoot {
    pub fn new(root: PathBuf, mode: SandboxMode) -> anyhow::Result<Self> {
        let root = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clean());
        Ok(Self { root, mode })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn mode(&self) -> SandboxMode {
        self.mode
    }

    /// Resolve a user-supplied path for filesystem access.
    pub fn resolve(&self, path: &str) -> anyhow::Result<PathBuf> {
        let path = path.trim();
        if path.is_empty() {
            anyhow::bail!("empty path");
        }
        if self.mode == SandboxMode::Off {
            return Ok(PathBuf::from(path));
        }

        let p = Path::new(path);
        let joined = if p.is_absolute() {
            PathBuf::from(path)
        } else {
            self.root.join(path)
        }
        .clean();

        let resolved = match std::fs::canonicalize(&joined) {
            Ok(c) => c,
            Err(_) => joined,
        };

        if !resolved.starts_with(&self.root) {
            match self.mode {
                SandboxMode::Relaxed => {
                    warn!(
                        path,
                        root = %self.root.display(),
                        "path outside workspace; allowing (sandbox relaxed)"
                    );
                    Ok(resolved)
                }
                SandboxMode::Strict => anyhow::bail!(
                    "path escapes workspace root {} (got {})",
                    self.root.display(),
                    resolved.display()
                ),
                SandboxMode::Off => Ok(resolved),
            }
        } else {
            Ok(resolved)
        }
    }
}

pub type ArcWorkspace = Arc<WorkspaceRoot>;
