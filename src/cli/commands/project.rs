//! `harness project ...` subcommand: linked-project management (init/add/clone/list/
//! dashboard/sync/push/status/import/prune/exec/publish/open).
//!
//! Extracted from `main.rs` (May 2026) as part of the god-file decomposition.
//! Pure git/CLI orchestration — no agent or LLM coupling.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::cli::ProjectAction;
use crate::projects;

pub fn handle_project_command(action: &ProjectAction) -> Result<()> {
    let mut store = projects::ProjectStore::load();

    match action {
        ProjectAction::Init {
            name,
            path,
            default_branch,
        } => {
            let parent = path
                .clone()
                .unwrap_or(std::env::current_dir().context("reading current directory")?);
            if !parent.exists() {
                anyhow::bail!("path does not exist: {}", parent.display());
            }
            let project_dir = parent.join(name);
            if project_dir.exists() {
                anyhow::bail!(
                    "project directory already exists: {}",
                    project_dir.display()
                );
            }
            std::fs::create_dir_all(&project_dir)
                .with_context(|| format!("creating {}", project_dir.display()))?;

            init_git_repo(&project_dir, default_branch)?;
            let readme_path = project_dir.join("README.md");
            if !readme_path.exists() {
                std::fs::write(&readme_path, format!("# {name}\n"))
                    .with_context(|| format!("writing {}", readme_path.display()))?;
            }

            let outcome = store.add(
                Some(name.clone()),
                Some(project_dir.clone()),
                None,
                Some(default_branch.clone()),
            )?;
            store.save()?;
            let entry = match outcome {
                projects::AddOutcome::Added(entry) | projects::AddOutcome::Updated(entry) => entry,
            };
            println!("Initialized and linked project '{}'", entry.name);
            println!("  path: {}", entry.path.display());
            println!("  branch: {default_branch}");
            println!(
                "Next: harness project publish {} --public|--private",
                entry.name
            );
        }
        ProjectAction::Add {
            name,
            path,
            remote,
            default_branch,
        } => {
            let outcome = store.add(
                name.clone(),
                path.clone(),
                remote.clone(),
                default_branch.clone(),
            )?;
            store.save()?;
            match outcome {
                projects::AddOutcome::Added(entry) => {
                    println!("Added project '{}'", entry.name);
                    println!("  path: {}", entry.path.display());
                    if let Some(remote) = entry.remote {
                        println!("  remote: {remote}");
                    }
                }
                projects::AddOutcome::Updated(entry) => {
                    println!("Updated project '{}'", entry.name);
                    println!("  path: {}", entry.path.display());
                    if let Some(remote) = entry.remote {
                        println!("  remote: {remote}");
                    }
                }
            }
        }
        ProjectAction::Clone {
            repo,
            name,
            directory,
            default_branch,
        } => {
            let clone_dir = directory
                .clone()
                .unwrap_or_else(|| PathBuf::from(infer_clone_directory(repo)));

            let status = std::process::Command::new("git")
                .arg("clone")
                .arg(repo)
                .arg(&clone_dir)
                .status()
                .context("running git clone")?;
            if !status.success() {
                anyhow::bail!("git clone failed with status {status}");
            }

            let outcome = store.add(
                name.clone(),
                Some(clone_dir),
                Some(repo.clone()),
                default_branch.clone(),
            )?;
            store.save()?;
            let entry = match outcome {
                projects::AddOutcome::Added(entry) | projects::AddOutcome::Updated(entry) => entry,
            };
            println!("Cloned and linked project '{}'", entry.name);
            println!("  path: {}", entry.path.display());
            if let Some(remote) = entry.remote {
                println!("  remote: {remote}");
            }
        }
        ProjectAction::List => {
            let projects = store.list_sorted();
            if projects.is_empty() {
                println!("No linked projects yet. Use `harness project add`.");
                return Ok(());
            }

            println!("Linked projects: {}\n", projects.len());
            println!("{:<22} {:<52} {:<22} BRANCH", "NAME", "PATH", "REMOTE");
            for p in projects {
                let remote = p.remote.unwrap_or_else(|| "-".to_string());
                let branch = p.default_branch.unwrap_or_else(|| "-".to_string());
                println!(
                    "{:<22} {:<52} {:<22} {}",
                    p.name,
                    p.path.display(),
                    remote,
                    branch
                );
            }
        }
        ProjectAction::Dashboard => {
            let projects = store.list_sorted();
            if projects.is_empty() {
                println!("No linked projects yet. Use `harness project add`.");
                return Ok(());
            }

            println!("Project dashboard: {}\n", projects.len());
            println!(
                "{:<20} {:<18} {:<17} {:<16} STATUS",
                "PROJECT", "BRANCH", "AHEAD/BEHIND", "CHANGES"
            );
            println!("{}", "-".repeat(88));
            for p in projects {
                match project_health_row(&p.path) {
                    Ok(row) => {
                        println!(
                            "{:<20} {:<18} {:<17} {:<16} {}",
                            p.name,
                            row.branch,
                            format!("+{} / -{}", row.ahead, row.behind),
                            format!(
                                "S:{} U:{} ?:{}",
                                row.changes.staged, row.changes.unstaged, row.changes.untracked
                            ),
                            status_badge(&row.status)
                        );
                    }
                    Err(err) => {
                        println!(
                            "{:<20} {:<18} {:<17} {:<16} error: {}",
                            p.name, "-", "-", "-", err
                        );
                    }
                }
            }
        }
        ProjectAction::Remove { target } => {
            if let Some(removed) = store.remove(target) {
                store.save()?;
                println!("Removed project '{}'", removed.name);
                println!("  path: {}", removed.path.display());
            } else {
                anyhow::bail!("project '{target}' not found. Run `harness project list`.");
            }
        }
        ProjectAction::Sync { target, all } => {
            let targets = if *all {
                let projects = store.list_sorted();
                if projects.is_empty() {
                    println!("No linked projects yet. Use `harness project add`.");
                    return Ok(());
                }
                projects
            } else if let Some(name_or_path) = target {
                vec![store.find(name_or_path).with_context(|| {
                    format!("project '{name_or_path}' not found. Run `harness project list`.")
                })?]
            } else {
                anyhow::bail!("provide <target> or use --all");
            };

            let mut synced = 0usize;
            for entry in targets {
                sync_project(&entry)?;
                let _ = store.add(
                    Some(entry.name.clone()),
                    Some(entry.path.clone()),
                    entry.remote.clone(),
                    entry.default_branch.clone(),
                )?;
                println!("Synced project '{}'", entry.name);
                println!("  path: {}", entry.path.display());
                synced += 1;
            }
            store.save()?;
            println!("Synced {synced} project(s).");
        }
        ProjectAction::Push {
            target,
            remote,
            branch,
            force,
        } => {
            let entry = store.find(target).with_context(|| {
                format!("project '{target}' not found. Run `harness project list`.")
            })?;

            let resolved_branch = if let Some(override_branch) = branch.clone() {
                override_branch
            } else {
                current_git_branch(&entry.path)
                    .or(entry.default_branch.clone())
                    .with_context(|| {
                        format!(
                            "could not determine branch for '{}'; pass --branch explicitly",
                            entry.name
                        )
                    })?
            };

            if *force && matches!(resolved_branch.as_str(), "main" | "master") {
                anyhow::bail!(
                    "force push to '{}' is blocked for safety. Push without --force.",
                    resolved_branch
                );
            }

            let mut cmd = std::process::Command::new("git");
            cmd.current_dir(&entry.path).arg("push");
            if *force {
                cmd.arg("--force-with-lease");
            }
            cmd.arg(remote).arg(&resolved_branch);

            let status = cmd.status().context("running git push")?;
            if !status.success() {
                anyhow::bail!("git push failed with status {status}");
            }

            let _ = store.add(
                Some(entry.name.clone()),
                Some(entry.path.clone()),
                entry.remote.clone(),
                Some(resolved_branch.clone()),
            )?;
            store.save()?;
            println!("Pushed '{}' to {remote}/{resolved_branch}", entry.name);
            println!("  path: {}", entry.path.display());
        }
        ProjectAction::Status { target } => {
            let entry = store.find(target).with_context(|| {
                format!("project '{target}' not found. Run `harness project list`.")
            })?;

            let branch =
                current_git_branch(&entry.path).unwrap_or_else(|| "(detached HEAD)".to_string());
            let upstream = git_output(
                &entry.path,
                &[
                    "rev-parse",
                    "--abbrev-ref",
                    "--symbolic-full-name",
                    "@{upstream}",
                ],
            )
            .ok();
            let remote_url = git_output(&entry.path, &["remote", "get-url", "origin"]).ok();
            let changes = collect_change_counts(&entry.path)?;
            let (ahead, behind) = if upstream.is_some() {
                git_ahead_behind(&entry.path)?
            } else {
                (0, 0)
            };

            println!("Project: {}", entry.name);
            println!("Path: {}", entry.path.display());
            println!("Branch: {branch}");
            println!(
                "Upstream: {}",
                upstream.unwrap_or_else(|| "(not configured)".to_string())
            );
            println!(
                "Remote: {}",
                remote_url.unwrap_or_else(|| "(origin not configured)".to_string())
            );
            println!("Ahead/Behind: +{ahead} / -{behind}");
            println!(
                "Changes: {} staged, {} unstaged, {} untracked",
                changes.staged, changes.unstaged, changes.untracked
            );
        }
        ProjectAction::Import { root, recursive } => {
            let scan_root = root
                .clone()
                .unwrap_or(std::env::current_dir().context("reading current directory")?);
            let repos = find_git_repos(&scan_root, *recursive)?;
            if repos.is_empty() {
                println!("No git repositories found under {}", scan_root.display());
                return Ok(());
            }

            let mut added = 0usize;
            let mut updated = 0usize;
            for repo_path in repos {
                let outcome = store.add(None, Some(repo_path), None, None)?;
                match outcome {
                    projects::AddOutcome::Added(entry) => {
                        println!("Added '{}': {}", entry.name, entry.path.display());
                        added += 1;
                    }
                    projects::AddOutcome::Updated(entry) => {
                        println!("Updated '{}': {}", entry.name, entry.path.display());
                        updated += 1;
                    }
                }
            }
            store.save()?;
            println!("Import complete: {added} added, {updated} updated.");
        }
        ProjectAction::Prune => {
            let before = store.projects.len();
            store.projects.retain(|p| p.path.exists());
            let removed = before.saturating_sub(store.projects.len());
            if removed > 0 {
                store.save()?;
            }
            println!("Pruned {removed} missing project link(s).");
        }
        ProjectAction::Exec { target, command } => {
            let entry = store.find(target).with_context(|| {
                format!("project '{target}' not found. Run `harness project list`.")
            })?;
            let program = &command[0];
            let args = &command[1..];
            let status = std::process::Command::new(program)
                .args(args)
                .current_dir(&entry.path)
                .status()
                .with_context(|| format!("running command in {}", entry.path.display()))?;
            if !status.success() {
                anyhow::bail!("command failed with status {status}");
            }
        }
        ProjectAction::Publish {
            target,
            repo,
            remote,
            public,
            private: _,
            push,
        } => {
            let entry = store.find(target).with_context(|| {
                format!("project '{target}' not found. Run `harness project list`.")
            })?;
            let repo_name = repo.clone().unwrap_or_else(|| entry.name.clone());

            let mut cmd = std::process::Command::new("gh");
            cmd.current_dir(&entry.path)
                .args(["repo", "create"])
                .arg(&repo_name)
                .args(["--source", ".", "--remote"])
                .arg(remote);
            if *public {
                cmd.arg("--public");
            } else {
                cmd.arg("--private");
            }
            if *push {
                cmd.arg("--push");
            }
            let status = cmd.status().context("running gh repo create")?;
            if !status.success() {
                anyhow::bail!("gh repo create failed with status {status}");
            }

            let _ = store.add(
                Some(entry.name.clone()),
                Some(entry.path.clone()),
                Some(repo_name.clone()),
                current_git_branch(&entry.path).or(entry.default_branch.clone()),
            )?;
            store.save()?;
            println!("Published '{}' to GitHub repo '{}'", entry.name, repo_name);
            println!("  path: {}", entry.path.display());
            println!("  remote: {remote}");
        }
        ProjectAction::Open { target, run } => {
            let entry = store.find(target).with_context(|| {
                format!("project '{target}' not found. Run `harness project list`.")
            })?;

            if *run {
                let exe = std::env::current_exe().context("resolving harness executable path")?;
                let status = std::process::Command::new(exe)
                    .current_dir(&entry.path)
                    .status()
                    .context("starting harness in project directory")?;
                if !status.success() {
                    anyhow::bail!("harness exited with status {status}");
                }
            } else {
                println!("{}", entry.path.display());
            }
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn infer_clone_directory(repo: &str) -> String {
    let trimmed = repo.trim_end_matches('/');
    let last = trimmed.rsplit('/').next().unwrap_or(trimmed);
    let without_git = last.strip_suffix(".git").unwrap_or(last);
    if without_git.is_empty() {
        "repo".to_string()
    } else {
        without_git.to_string()
    }
}

fn init_git_repo(project_dir: &std::path::Path, default_branch: &str) -> Result<()> {
    let init_with_branch = std::process::Command::new("git")
        .current_dir(project_dir)
        .args(["init", "-b", default_branch])
        .status()
        .context("running git init -b")?;
    if init_with_branch.success() {
        return Ok(());
    }

    let init_basic = std::process::Command::new("git")
        .current_dir(project_dir)
        .arg("init")
        .status()
        .context("running git init")?;
    if !init_basic.success() {
        anyhow::bail!("git init failed with status {init_basic}");
    }
    let checkout = std::process::Command::new("git")
        .current_dir(project_dir)
        .args(["checkout", "-b", default_branch])
        .status()
        .context("running git checkout -b")?;
    if !checkout.success() {
        anyhow::bail!("git checkout -b failed with status {checkout}");
    }

    Ok(())
}

fn sync_project(entry: &projects::ProjectEntry) -> Result<()> {
    let fetch_status = std::process::Command::new("git")
        .current_dir(&entry.path)
        .args(["fetch", "--all", "--prune"])
        .status()
        .context("running git fetch --all --prune")?;
    if !fetch_status.success() {
        anyhow::bail!("git fetch failed with status {fetch_status}");
    }

    let mut pull_cmd = std::process::Command::new("git");
    pull_cmd
        .current_dir(&entry.path)
        .args(["pull", "--ff-only"]);
    if let Some(branch) = &entry.default_branch {
        pull_cmd.arg("origin").arg(branch);
    }
    let pull_status = pull_cmd.status().context("running git pull --ff-only")?;
    if !pull_status.success() {
        anyhow::bail!("git pull failed with status {pull_status}");
    }

    Ok(())
}

fn find_git_repos(root: &std::path::Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut repos = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(root.to_path_buf());

    while let Some(dir) = queue.pop_front() {
        if dir.join(".git").exists() {
            repos.push(dir.clone());
            // If this is already a git repo, do not recurse into children.
            continue;
        }

        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("reading directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name == ".git" {
                    continue;
                }
            }
            if recursive {
                queue.push_back(path);
            } else if path.join(".git").exists() {
                repos.push(path);
            }
        }
    }

    repos.sort();
    repos.dedup();
    Ok(repos)
}

fn current_git_branch(path: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("git")
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

#[derive(Debug, Default)]
struct ChangeCounts {
    staged: usize,
    unstaged: usize,
    untracked: usize,
}

#[derive(Debug)]
struct ProjectHealthRow {
    branch: String,
    ahead: u64,
    behind: u64,
    changes: ChangeCounts,
    status: String,
}

fn project_health_row(path: &std::path::Path) -> Result<ProjectHealthRow> {
    let branch = current_git_branch(path).unwrap_or_else(|| "(detached HEAD)".to_string());
    let upstream = git_output(
        path,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .ok();
    let changes = collect_change_counts(path)?;
    let (ahead, behind) = if upstream.is_some() {
        git_ahead_behind(path)?
    } else {
        (0, 0)
    };

    let dirty = changes.staged + changes.unstaged + changes.untracked;
    let status = if dirty == 0 && ahead == 0 && behind == 0 {
        "clean".to_string()
    } else if behind > 0 && ahead == 0 {
        "behind".to_string()
    } else if ahead > 0 && behind == 0 {
        "ahead".to_string()
    } else if ahead > 0 && behind > 0 {
        "diverged".to_string()
    } else {
        "dirty".to_string()
    };

    Ok(ProjectHealthRow {
        branch,
        ahead,
        behind,
        changes,
        status,
    })
}

fn collect_change_counts(path: &std::path::Path) -> Result<ChangeCounts> {
    let out = git_output(path, &["status", "--porcelain"])?;
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
    Ok(counts)
}

fn git_output(path: &std::path::Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_ahead_behind(path: &std::path::Path) -> Result<(u64, u64)> {
    let out = git_output(
        path,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    )?;
    let mut parts = out.split_whitespace();
    let ahead = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
    let behind = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
    Ok((ahead, behind))
}

fn status_badge(status: &str) -> &'static str {
    match status {
        "clean" => "OK",
        "ahead" => "AHEAD",
        "behind" => "BEHIND",
        "diverged" => "DIVERGED",
        "dirty" => "DIRTY",
        _ => "UNKNOWN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_clone_directory_strips_git_suffix() {
        assert_eq!(
            infer_clone_directory("https://github.com/u/repo.git"),
            "repo"
        );
    }

    #[test]
    fn infer_clone_directory_handles_trailing_slash() {
        assert_eq!(infer_clone_directory("https://github.com/u/repo/"), "repo");
    }

    #[test]
    fn infer_clone_directory_handles_ssh_url() {
        assert_eq!(
            infer_clone_directory("git@github.com:u/myproj.git"),
            "myproj"
        );
    }

    #[test]
    fn infer_clone_directory_handles_no_slash() {
        assert_eq!(infer_clone_directory("local-repo"), "local-repo");
    }

    #[test]
    fn infer_clone_directory_falls_back_to_repo_for_empty_basename() {
        assert_eq!(infer_clone_directory(".git"), "repo");
    }

    #[test]
    fn status_badge_maps_known_statuses() {
        assert_eq!(status_badge("clean"), "OK");
        assert_eq!(status_badge("ahead"), "AHEAD");
        assert_eq!(status_badge("behind"), "BEHIND");
        assert_eq!(status_badge("diverged"), "DIVERGED");
        assert_eq!(status_badge("dirty"), "DIRTY");
    }

    #[test]
    fn status_badge_falls_back_for_unknown() {
        assert_eq!(status_badge("anything-else"), "UNKNOWN");
        assert_eq!(status_badge(""), "UNKNOWN");
    }
}
