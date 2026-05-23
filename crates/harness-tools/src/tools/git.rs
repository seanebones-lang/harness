//! Structured `git` tool — typed operations instead of raw shell strings.
//!
//! Exposes: status, diff, add, commit, branch, push, log, blame, restore, clone, pull, fetch, checkout, switch.

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tokio::process::Command;

use crate::registry::Tool;
use crate::workspace_root::WorkspaceRoot;

/// Structured git operations tool.
pub struct GitTool {
    /// Workspace root — git runs with this as `current_dir`; clone targets are sandboxed.
    pub workspace: Arc<WorkspaceRoot>,
}

#[async_trait]
impl Tool for GitTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "git",
            "Perform git operations with typed, safe parameters. \
             Actions: status, diff, add, commit, branch, push, log, blame, restore, clone, pull, fetch, checkout, switch. \
             Prefer this over `shell` for git operations — it validates parameters \
             and prevents force-pushes to protected branches.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["status", "diff", "add", "commit", "branch", "push", "log", "blame", "restore", "stash", "clone", "pull", "fetch", "checkout", "switch"],
                        "description": "Git operation to perform."
                    },
                    "paths": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "File paths (for add, restore, blame, diff)."
                    },
                    "message": {
                        "type": "string",
                        "description": "Commit message (for commit)."
                    },
                    "branch_name": {
                        "type": "string",
                        "description": "Branch name (for branch, push)."
                    },
                    "remote": {
                        "type": "string",
                        "description": "Remote name (default: origin)."
                    },
                    "force": {
                        "type": "boolean",
                        "description": "Force push (disabled for main/master)."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Number of log entries (default: 10)."
                    },
                    "line": {
                        "type": "integer",
                        "description": "Line number for blame."
                    },
                    "stash_action": {
                        "type": "string",
                        "enum": ["push", "pop", "list", "drop"],
                        "description": "Stash sub-action."
                    },
                    "staged": {
                        "type": "boolean",
                        "description": "Show staged diff (for diff action)."
                    },
                    "repo": {
                        "type": "string",
                        "description": "Repository URL/path (for clone)."
                    },
                    "directory": {
                        "type": "string",
                        "description": "Destination directory (for clone)."
                    }
                },
                "required": ["action"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing action"))?;
        let cwd = self.workspace.root();

        match action {
            "status" => run_git(cwd, &["status", "--short"]).await,

            "diff" => {
                let staged = args["staged"].as_bool().unwrap_or(false);
                let paths: Vec<String> = args["paths"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str())
                            .map(|p| self.workspace.resolve(p).map(|x| x.to_string_lossy().into_owned()))
                            .collect::<anyhow::Result<_>>()
                    })
                    .transpose()?
                    .unwrap_or_default();

                let mut git_args = vec!["diff"];
                if staged {
                    git_args.push("--cached");
                }
                let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
                git_args.extend(path_refs.iter().copied());
                run_git(cwd, &git_args).await
            }

            "add" => {
                let paths: Vec<String> = args["paths"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str())
                            .map(|p| self.workspace.resolve(p).map(|x| x.to_string_lossy().into_owned()))
                            .collect::<anyhow::Result<_>>()
                    })
                    .transpose()?
                    .unwrap_or_default();

                if paths.is_empty() {
                    run_git(cwd, &["add", "."]).await
                } else {
                    let mut git_args = vec!["add", "--"];
                    let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
                    git_args.extend(path_refs.iter().copied());
                    run_git(cwd, &git_args).await
                }
            }

            "commit" => {
                let msg = args["message"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("commit requires a message"))?;
                run_git(cwd, &["commit", "-m", msg]).await
            }

            "branch" => match args["branch_name"].as_str() {
                Some(name) => run_git(cwd, &["checkout", "-b", name]).await,
                None => run_git(cwd, &["branch", "--list", "-v"]).await,
            },

            "push" => {
                let remote = args["remote"].as_str().unwrap_or("origin");
                let branch_arg = args["branch_name"].as_str().unwrap_or("HEAD");
                let force = args["force"].as_bool().unwrap_or(false);
                let resolved = resolve_branch_name(branch_arg, cwd).await?;

                if force && is_protected_branch(&resolved) {
                    return Err(anyhow::anyhow!(
                        "Force-push to {resolved} is blocked for safety. Use git shell directly if you really need this."
                    ));
                }

                if force {
                    run_git(cwd, &["push", "--force-with-lease", remote, &resolved]).await
                } else {
                    run_git(cwd, &["push", remote, &resolved]).await
                }
            }

            "clone" => {
                let repo = args["repo"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("clone requires `repo`"))?;
                if let Some(directory) = args["directory"].as_str() {
                    let dest = self.workspace.resolve(directory)?;
                    let dest_s = dest.to_string_lossy();
                    run_git(cwd, &["clone", repo, &dest_s]).await
                } else {
                    run_git(cwd, &["clone", repo]).await
                }
            }

            "pull" => {
                let remote = args["remote"].as_str();
                let branch = args["branch_name"].as_str();
                match (remote, branch) {
                    (Some(r), Some(b)) => run_git(cwd, &["pull", "--ff-only", r, b]).await,
                    (Some(r), None) => run_git(cwd, &["pull", "--ff-only", r]).await,
                    (None, Some(b)) => run_git(cwd, &["pull", "--ff-only", "origin", b]).await,
                    (None, None) => run_git(cwd, &["pull", "--ff-only"]).await,
                }
            }

            "fetch" => {
                let remote = args["remote"].as_str();
                let branch = args["branch_name"].as_str();
                match (remote, branch) {
                    (Some(r), Some(b)) => run_git(cwd, &["fetch", r, b]).await,
                    (Some(r), None) => run_git(cwd, &["fetch", r]).await,
                    (None, Some(b)) => run_git(cwd, &["fetch", "origin", b]).await,
                    (None, None) => run_git(cwd, &["fetch", "--all", "--prune"]).await,
                }
            }

            "checkout" => {
                let branch = args["branch_name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("checkout requires `branch_name`"))?;
                run_git(cwd, &["checkout", branch]).await
            }

            "switch" => {
                let branch = args["branch_name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("switch requires `branch_name`"))?;
                run_git(cwd, &["switch", branch]).await
            }

            "log" => {
                let limit = args["limit"].as_u64().unwrap_or(10);
                let fmt = "--format=%h %s (%an, %ar)";
                let n_str = format!("-{limit}");
                run_git(cwd, &["log", &n_str, fmt]).await
            }

            "blame" => {
                let paths: Vec<String> = args["paths"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .map(|p| {
                        self.workspace
                            .resolve(p)
                            .map(|x| x.to_string_lossy().into_owned())
                    })
                    .transpose()?
                    .map(|p| vec![p])
                    .unwrap_or_default();

                if paths.is_empty() {
                    return Err(anyhow::anyhow!("blame requires a path"));
                }

                if let Some(line) = args["line"].as_u64() {
                    let range = format!("-L {line},{line}");
                    run_git(cwd, &["blame", &range, &paths[0]]).await
                } else {
                    run_git(cwd, &["blame", &paths[0]]).await
                }
            }

            "restore" => {
                let paths: Vec<String> = args["paths"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str())
                            .map(|p| self.workspace.resolve(p).map(|x| x.to_string_lossy().into_owned()))
                            .collect::<anyhow::Result<_>>()
                    })
                    .transpose()?
                    .unwrap_or_default();

                if paths.is_empty() {
                    return Err(anyhow::anyhow!("restore requires at least one path"));
                }

                let mut git_args = vec!["restore", "--"];
                let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
                git_args.extend(path_refs.iter().copied());
                run_git(cwd, &git_args).await
            }

            "stash" => {
                let sub = args["stash_action"].as_str().unwrap_or("list");
                match sub {
                    "push" => run_git(cwd, &["stash", "push"]).await,
                    "pop" => run_git(cwd, &["stash", "pop"]).await,
                    "drop" => run_git(cwd, &["stash", "drop"]).await,
                    _ => run_git(cwd, &["stash", "list"]).await,
                }
            }

            other => Err(anyhow::anyhow!("unknown git action: {other}")),
        }
    }
}

fn is_protected_branch(name: &str) -> bool {
    matches!(name, "main" | "master")
}

async fn resolve_branch_name(branch: &str, cwd: &Path) -> anyhow::Result<String> {
    if branch != "HEAD" {
        return Ok(branch.to_string());
    }
    let output = Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(cwd)
        .output()
        .await?;
    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            return Ok(name);
        }
    }
    Ok("HEAD".to_string())
}

async fn run_git(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() && !stderr.is_empty() {
        return Err(anyhow::anyhow!("git {}: {}", args.join(" "), stderr.trim()));
    }

    let result = if stdout.is_empty() && !stderr.is_empty() {
        stderr
    } else {
        stdout
    };

    Ok(if result.is_empty() {
        "(no output)".to_string()
    } else {
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_root::{SandboxMode, WorkspaceRoot};
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn protected_branch_names() {
        assert!(is_protected_branch("main"));
        assert!(is_protected_branch("master"));
        assert!(!is_protected_branch("feature/x"));
    }

    #[tokio::test]
    async fn push_blocks_force_to_main() {
        let dir = tempdir().unwrap();
        let ws = Arc::new(
            WorkspaceRoot::new(dir.path().to_path_buf(), SandboxMode::Strict).expect("workspace"),
        );
        let tool = GitTool { workspace: ws.clone() };

        std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .expect("git init");
        std::fs::write(dir.path().join("README.md"), "init\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(dir.path())
            .output()
            .expect("git add");
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .current_dir(dir.path())
            .output()
            .expect("git commit");

        let err = tool
            .execute(json!({
                "action": "push",
                "branch_name": "main",
                "force": true
            }))
            .await
            .expect_err("force push blocked");
        assert!(err.to_string().contains("blocked"));
    }

    #[tokio::test]
    async fn clone_rejects_directory_outside_workspace() {
        let dir = tempdir().unwrap();
        let ws = Arc::new(
            WorkspaceRoot::new(dir.path().to_path_buf(), SandboxMode::Strict).expect("workspace"),
        );
        let tool = GitTool { workspace: ws };
        let err = tool
            .execute(json!({
                "action": "clone",
                "repo": "https://example.com/repo.git",
                "directory": "../outside"
            }))
            .await
            .expect_err("clone escape");
        assert!(err.to_string().contains("escapes workspace root"));
    }
}
