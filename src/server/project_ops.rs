//! Project git/shell helpers for HTTP project actions.

use std::path::Path as FsPath;
use std::process::Command;
use tokio::time::{timeout, Duration};

pub(crate) const PROJECT_GIT_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const PROJECT_TEST_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Default)]
pub(crate) struct ChangeCounts {
    pub(crate) staged: usize,
    pub(crate) unstaged: usize,
    pub(crate) untracked: usize,
}

pub(crate) async fn run_git_in_project(path: &FsPath, args: &[&str]) -> anyhow::Result<String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.current_dir(path).args(args).kill_on_drop(true);
    let cmd_display = format!("git {}", args.join(" "));
    let output = timeout(PROJECT_GIT_TIMEOUT, cmd.output())
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "{cmd_display} timed out after {}s",
                PROJECT_GIT_TIMEOUT.as_secs()
            )
        })??;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub(crate) async fn run_shell_in_project(path: &FsPath, command: &str) -> anyhow::Result<String> {
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(path)
        .kill_on_drop(true);
    let output = timeout(PROJECT_TEST_TIMEOUT, cmd.output())
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "command timed out after {}s: {command}",
                PROJECT_TEST_TIMEOUT.as_secs()
            )
        })??;
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    if !output.status.success() {
        anyhow::bail!("command failed: {command}\n{text}");
    }
    Ok(text)
}

pub(crate) fn current_git_branch(path: &FsPath) -> Option<String> {
    let output = Command::new("git")
        .current_dir(path)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

pub(crate) fn git_output(path: &FsPath, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git").current_dir(path).args(args).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(crate) fn git_ahead_behind(path: &FsPath) -> anyhow::Result<(u64, u64)> {
    let out = git_output(
        path,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    )?;
    let mut parts = out.split_whitespace();
    let ahead = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
    let behind = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
    Ok((ahead, behind))
}

/// Parse `git status --porcelain` into staged/unstaged/untracked counts.
pub(crate) fn parse_porcelain_counts(out: &str) -> ChangeCounts {
    let mut counts = ChangeCounts::default();
    for line in out.lines() {
        if line.starts_with("?? ") {
            counts.untracked += 1;
            continue;
        }
        let bytes = line.as_bytes();
        if bytes.len() < 2 {
            continue;
        }
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        if x != ' ' && x != '?' {
            counts.staged += 1;
        }
        if y != ' ' && y != '?' {
            counts.unstaged += 1;
        }
    }
    counts
}

pub(crate) fn collect_change_counts(path: &FsPath) -> anyhow::Result<ChangeCounts> {
    let out = git_output(path, &["status", "--porcelain"])?;
    Ok(parse_porcelain_counts(&out))
}

pub(crate) fn default_test_command(path: &FsPath) -> String {
    if path.join("Cargo.toml").exists() {
        "cargo test".to_string()
    } else if path.join("package.json").exists() {
        "npm test".to_string()
    } else if path.join("pyproject.toml").exists() || path.join("pytest.ini").exists() {
        "pytest".to_string()
    } else if path.join("go.mod").exists() {
        "go test ./...".to_string()
    } else {
        "echo 'No known test command. Pass command in request.'".to_string()
    }
}

pub(crate) fn is_allowed_test_command(cmd: &str) -> bool {
    const ALLOWED: &[&str] = &[
        "cargo test",
        "npm test",
        "yarn test",
        "pnpm test",
        "go test",
        "pytest",
        "make test",
        "echo ",
    ];
    let cmd = cmd.trim();
    ALLOWED.iter().any(|prefix| cmd.starts_with(prefix))
}

pub(crate) fn collect_files(
    root: &FsPath,
    dir: &FsPath,
    query: &str,
    limit: usize,
    out: &mut Vec<String>,
) -> anyhow::Result<()> {
    if out.len() >= limit {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".git" || name == "node_modules" || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, query, limit, out)?;
            if out.len() >= limit {
                return Ok(());
            }
            continue;
        }
        if let Ok(rel) = path.strip_prefix(root) {
            let rel_s = rel.display().to_string();
            if query.is_empty() || rel_s.to_lowercase().contains(query) {
                out.push(rel_s);
                if out.len() >= limit {
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn parse_porcelain_counts_empty() {
        let c = parse_porcelain_counts("");
        assert_eq!((c.staged, c.unstaged, c.untracked), (0, 0, 0));
    }

    #[test]
    fn parse_porcelain_counts_mixed() {
        let out = "\
M  staged.txt
 M unstaged.txt
MM both.txt
?? untracked.txt
A  added.txt
 D deleted_worktree.txt
";
        let c = parse_porcelain_counts(out);
        assert_eq!(c.untracked, 1);
        // staged: M (col0), M of MM, A  => 3
        assert_eq!(c.staged, 3);
        // unstaged: M col1, M of MM, D => 3
        assert_eq!(c.unstaged, 3);
    }

    #[test]
    fn parse_porcelain_skips_short_lines() {
        let c = parse_porcelain_counts("?\nX\n");
        assert_eq!((c.staged, c.unstaged, c.untracked), (0, 0, 0));
    }

    #[test]
    fn default_test_command_by_markers() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        assert!(default_test_command(root).contains("No known test"));

        fs::write(root.join("go.mod"), "module t\n").unwrap();
        assert_eq!(default_test_command(root), "go test ./...");
        fs::remove_file(root.join("go.mod")).unwrap();

        fs::write(root.join("pytest.ini"), "").unwrap();
        assert_eq!(default_test_command(root), "pytest");
        fs::remove_file(root.join("pytest.ini")).unwrap();

        fs::write(root.join("package.json"), "{}").unwrap();
        assert_eq!(default_test_command(root), "npm test");
        fs::remove_file(root.join("package.json")).unwrap();

        fs::write(root.join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\n").unwrap();
        assert_eq!(default_test_command(root), "cargo test");
    }

    #[test]
    fn is_allowed_test_command_prefixes() {
        assert!(is_allowed_test_command("cargo test"));
        assert!(is_allowed_test_command("  cargo test --bin harness "));
        assert!(is_allowed_test_command("npm test"));
        assert!(is_allowed_test_command("yarn test foo"));
        assert!(is_allowed_test_command("pnpm test"));
        assert!(is_allowed_test_command("go test ./..."));
        assert!(is_allowed_test_command("pytest -q"));
        assert!(is_allowed_test_command("make test"));
        assert!(is_allowed_test_command("echo 'hi'"));
        assert!(!is_allowed_test_command("rm -rf /"));
        assert!(!is_allowed_test_command("curl evil"));
        assert!(!is_allowed_test_command(""));
    }

    #[test]
    fn collect_files_respects_limit_query_and_skips() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::create_dir_all(root.join("node_modules/x")).unwrap();
        fs::write(root.join("src/a.rs"), "").unwrap();
        fs::write(root.join("src/b.txt"), "").unwrap();
        fs::write(root.join("readme.md"), "").unwrap();
        fs::write(root.join("target/debug/x"), "").unwrap();
        fs::write(root.join("node_modules/x/y"), "").unwrap();

        let mut out = Vec::new();
        collect_files(root, root, "a.rs", 10, &mut out).unwrap();
        assert_eq!(out, vec!["src/a.rs".to_string()]);

        out.clear();
        collect_files(root, root, "", 2, &mut out).unwrap();
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|p| !p.contains("target") && !p.contains("node_modules")));

        out.clear();
        collect_files(root, root, "readme", 10, &mut out).unwrap();
        assert_eq!(out, vec!["readme.md".to_string()]);
    }

    #[test]
    fn current_git_branch_none_outside_repo() {
        let dir = tempdir().unwrap();
        assert!(current_git_branch(dir.path()).is_none());
    }
}
