//! Git-based checkpoint: stash before every destructive agent turn,
//! expose `harness undo` to pop the latest harness stash.

use anyhow::{Context, Result};
use std::process::Command;

const STASH_PREFIX: &str = "harness-checkpoint";

/// True if the CWD is inside a git repository.
pub fn in_git_repo() -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Create a stash snapshot before destructive tool calls in a session turn.
/// Returns the stash entry name on success, or `None` if there is nothing to stash.
pub fn create(session_id: &str, turn: usize) -> Option<String> {
    if !in_git_repo() {
        return None;
    }
    let msg = format!("{STASH_PREFIX}:{session_id}:{turn}");
    // `git stash push -u` includes untracked files.
    let out = Command::new("git")
        .args(["stash", "push", "-u", "-m", &msg])
        .output()
        .ok()?;

    if out.status.success() {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if combined.contains("No local changes") {
            None
        } else {
            Some(msg)
        }
    } else {
        None
    }
}

/// Pop (restore) the most recent harness checkpoint stash.
pub fn undo() -> Result<String> {
    if !in_git_repo() {
        anyhow::bail!("Not inside a git repository — nothing to undo.");
    }

    // List all stashes and find the most recent harness one.
    let stash_ref = find_latest_harness_stash()
        .context("No harness checkpoint stash found. Nothing to undo.")?;

    let out = Command::new("git")
        .args(["stash", "pop", &stash_ref])
        .output()
        .context("git stash pop failed")?;

    if out.status.success() {
        let msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(format!("Restored checkpoint ({})\n{msg}", stash_ref))
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("git stash pop failed: {err}")
    }
}

/// List all harness checkpoint stashes.
pub fn list() -> Result<Vec<(String, String)>> {
    if !in_git_repo() {
        return Ok(vec![]);
    }

    let out = Command::new("git")
        .args(["stash", "list", "--format=%gd %s"])
        .output()
        .context("git stash list failed")?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let entries = stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, ' ');
            let stash_ref = parts.next()?.to_string();
            let msg = parts.next()?.to_string();
            if msg.contains(STASH_PREFIX) {
                Some((stash_ref, msg))
            } else {
                None
            }
        })
        .collect();

    Ok(entries)
}

/// Generate a short time-based ID (8 hex chars).
pub fn short_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Use low 32 bits XOR'd with nanos for pseudo-uniqueness.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let mixed = (secs as u32) ^ nanos;
    format!("{mixed:08x}")
}

fn find_latest_harness_stash() -> Option<String> {
    let out = Command::new("git")
        .args(["stash", "list", "--format=%gd %s"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        if line.contains(STASH_PREFIX) {
            return line.split_whitespace().next().map(|s| s.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Mutex;

    static GIT_REPO_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct CwdGuard {
        previous: PathBuf,
    }

    impl CwdGuard {
        fn chdir(path: &Path) -> Self {
            let previous = std::env::current_dir().expect("cwd");
            std::env::set_current_dir(path).expect("chdir");
            Self { previous }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(dir)
            .args(args)
            .status()
            .expect("spawn git");
        assert!(status.success(), "git {} failed", args.join(" "));
    }

    fn init_repo(dir: &Path) {
        git(dir, &["init"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "test"]);
        git(dir, &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.join("tracked.txt"), "v1").unwrap();
        git(dir, &["add", "tracked.txt"]);
        git(dir, &["commit", "-m", "init"]);
    }

    #[test]
    fn short_id_is_eight_hex_chars() {
        let id = short_id();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn create_list_and_undo_roundtrip() {
        let _lock = GIT_REPO_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tmp = tempfile::tempdir().expect("tempdir");
        init_repo(tmp.path());
        let _guard = CwdGuard::chdir(tmp.path());

        std::fs::write(tmp.path().join("tracked.txt"), "v2").unwrap();
        let stash_name = create("sess1", 1).expect("stash created");
        assert!(stash_name.contains(STASH_PREFIX));
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("tracked.txt")).unwrap(),
            "v1"
        );

        let entries = list().expect("list stashes");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].1.contains("sess1"));

        undo().expect("undo");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("tracked.txt")).unwrap(),
            "v2"
        );
    }

    #[test]
    fn create_returns_none_when_clean() {
        let _lock = GIT_REPO_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tmp = tempfile::tempdir().expect("tempdir");
        init_repo(tmp.path());
        let _guard = CwdGuard::chdir(tmp.path());
        assert!(create("sess2", 0).is_none());
    }
}
