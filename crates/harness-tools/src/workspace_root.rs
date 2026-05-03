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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn ws(mode: SandboxMode) -> (tempfile::TempDir, WorkspaceRoot) {
        let dir = tempdir().unwrap();
        let root = WorkspaceRoot::new(dir.path().to_path_buf(), mode).unwrap();
        (dir, root)
    }

    #[test]
    fn sandbox_mode_from_config_defaults_to_strict() {
        assert_eq!(SandboxMode::from_config(None), SandboxMode::Strict);
        assert_eq!(SandboxMode::from_config(Some("")), SandboxMode::Strict);
        assert_eq!(
            SandboxMode::from_config(Some("garbage")),
            SandboxMode::Strict
        );
        assert_eq!(
            SandboxMode::from_config(Some("strict")),
            SandboxMode::Strict
        );
        assert_eq!(
            SandboxMode::from_config(Some("STRICT")),
            SandboxMode::Strict
        );
        assert_eq!(
            SandboxMode::from_config(Some("relaxed")),
            SandboxMode::Relaxed
        );
        assert_eq!(SandboxMode::from_config(Some("off")), SandboxMode::Off);
    }

    #[test]
    fn empty_path_rejected_in_every_mode() {
        for m in [SandboxMode::Strict, SandboxMode::Relaxed, SandboxMode::Off] {
            let (_d, r) = ws(m);
            assert!(r.resolve("").is_err(), "empty path must err in {m:?}");
            assert!(
                r.resolve("   ").is_err(),
                "whitespace path must err in {m:?}"
            );
            assert!(
                r.resolve("\t\n").is_err(),
                "control-only path must err in {m:?}"
            );
        }
    }

    #[test]
    fn off_mode_passes_paths_through_unchanged() {
        let (_d, r) = ws(SandboxMode::Off);
        // Off mode is a hatch for special workflows — it must not reject anything,
        // including absolute paths that are nowhere near the workspace.
        let p = r.resolve("/etc/passwd").unwrap();
        assert_eq!(p, PathBuf::from("/etc/passwd"));
        let p = r.resolve("../../../escape").unwrap();
        assert_eq!(p, PathBuf::from("../../../escape"));
    }

    #[test]
    fn strict_allows_relative_under_root() {
        let (_d, r) = ws(SandboxMode::Strict);
        let p = r.resolve("hello.txt").unwrap();
        assert!(p.starts_with(r.root()), "{p:?} not under {:?}", r.root());
        assert!(p.ends_with("hello.txt"));
    }

    #[test]
    fn strict_allows_nested_relative_under_root() {
        let (_d, r) = ws(SandboxMode::Strict);
        let p = r.resolve("a/b/c.txt").unwrap();
        assert!(p.starts_with(r.root()));
        assert!(p.ends_with("a/b/c.txt"));
    }

    #[test]
    fn strict_allows_dot_dot_that_stays_inside_root() {
        let (_d, r) = ws(SandboxMode::Strict);
        // `a/../b.txt` cleans to `b.txt` — inside root.
        let p = r.resolve("a/../b.txt").unwrap();
        assert!(p.starts_with(r.root()));
        assert!(p.ends_with("b.txt"));
    }

    #[test]
    fn strict_rejects_dot_dot_escape() {
        let (_d, r) = ws(SandboxMode::Strict);
        let err = r.resolve("../escape.txt").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("escapes workspace root"),
            "expected workspace-escape error, got: {msg}"
        );
    }

    #[test]
    fn strict_rejects_deep_dot_dot_escape() {
        let (_d, r) = ws(SandboxMode::Strict);
        let err = r.resolve("a/b/../../../../etc/passwd").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("escapes workspace root"), "got: {msg}");
    }

    #[test]
    fn strict_rejects_absolute_outside_root() {
        let (_d, r) = ws(SandboxMode::Strict);
        let err = r.resolve("/etc/passwd").unwrap_err();
        assert!(err.to_string().contains("escapes workspace root"));
    }

    #[test]
    fn strict_allows_canonical_absolute_under_root() {
        let (d, r) = ws(SandboxMode::Strict);
        // The root path itself, after canonicalization, must of course be allowed.
        let canon = std::fs::canonicalize(d.path()).unwrap();
        let target = canon.join("nested.txt");
        let p = r.resolve(target.to_str().unwrap()).unwrap();
        assert!(p.starts_with(r.root()));
    }

    #[test]
    fn relaxed_allows_escape_with_warning() {
        let (_d, r) = ws(SandboxMode::Relaxed);
        // In relaxed mode the bad path is allowed (and a warn! is emitted).
        let p = r.resolve("../escape.txt").unwrap();
        // We can't easily assert on the path without canonicalization quirks, just that it didn't err.
        assert!(!p.as_os_str().is_empty());
    }

    #[test]
    fn relaxed_allows_absolute_outside_root() {
        let (_d, r) = ws(SandboxMode::Relaxed);
        let p = r.resolve("/tmp/whatever.txt").unwrap();
        assert_eq!(p, PathBuf::from("/tmp/whatever.txt").clean());
    }

    #[cfg(unix)]
    #[test]
    fn strict_rejects_symlink_escape() {
        let (d, r) = ws(SandboxMode::Strict);
        // Create an outside target dir, symlink to it from inside root, then try to
        // resolve a path that goes through the symlink. canonicalize() must follow
        // the symlink and the resulting path must fail the starts_with(root) check.
        let outside = tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"shh").unwrap();
        let link = d.path().join("link_to_outside");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();

        let err = r
            .resolve(link.join("secret.txt").to_str().unwrap())
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("escapes workspace root"),
            "symlink escape must be rejected, got: {msg}"
        );
    }

    #[test]
    fn root_is_canonicalized_on_construction() {
        let dir = tempdir().unwrap();
        let r = WorkspaceRoot::new(dir.path().to_path_buf(), SandboxMode::Strict).unwrap();
        // After construction, root must be the canonical form (resolves /tmp -> /private/tmp on macOS,
        // for instance). Subsequent resolves of paths through that canonical root must succeed.
        let p = r.resolve("inside.txt").unwrap();
        assert!(p.starts_with(r.root()));
    }
}
