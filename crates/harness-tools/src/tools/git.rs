//! Structured `git` tool — typed operations instead of raw shell strings.
//!
//! Exposes: status, diff, add, commit, branch, push, log, blame, restore.

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::registry::Tool;

pub struct GitTool;

#[async_trait]
impl Tool for GitTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "git",
            "Perform git operations with typed, safe parameters. \
             Actions: status, diff, add, commit, branch, push, log, blame, restore. \
             Prefer this over `shell` for git operations — it validates parameters \
             and prevents force-pushes to protected branches.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["status", "diff", "add", "commit", "branch", "push", "log", "blame", "restore", "stash"],
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
                    }
                },
                "required": ["action"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let action = args["action"].as_str().ok_or_else(|| anyhow::anyhow!("missing action"))?;

        match action {
            "status" => run_git(&["status", "--short"]).await,

            "diff" => {
                let staged = args["staged"].as_bool().unwrap_or(false);
                let paths: Vec<&str> = args["paths"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();

                let mut git_args = vec!["diff"];
                if staged { git_args.push("--cached"); }
                git_args.extend(paths.iter().copied());
                run_git(&git_args).await
            }

            "add" => {
                let paths: Vec<&str> = args["paths"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();

                if paths.is_empty() {
                    run_git(&["add", "."]).await
                } else {
                    let mut git_args = vec!["add", "--"];
                    git_args.extend(paths.iter().copied());
                    run_git(&git_args).await
                }
            }

            "commit" => {
                let msg = args["message"].as_str().ok_or_else(|| anyhow::anyhow!("commit requires a message"))?;
                run_git(&["commit", "-m", msg]).await
            }

            "branch" => {
                match args["branch_name"].as_str() {
                    Some(name) => run_git(&["checkout", "-b", name]).await,
                    None => run_git(&["branch", "--list", "-v"]).await,
                }
            }

            "push" => {
                let remote = args["remote"].as_str().unwrap_or("origin");
                let branch = args["branch_name"].as_str().unwrap_or("HEAD");
                let force = args["force"].as_bool().unwrap_or(false);

                // Block force-push to main/master.
                if force && matches!(branch, "main" | "master") {
                    return Err(anyhow::anyhow!(
                        "Force-push to {branch} is blocked for safety. Use git shell directly if you really need this."
                    ));
                }

                if force {
                    run_git(&["push", "--force-with-lease", remote, branch]).await
                } else {
                    run_git(&["push", remote, branch]).await
                }
            }

            "log" => {
                let limit = args["limit"].as_u64().unwrap_or(10);
                let fmt = "--format=%h %s (%an, %ar)";
                let n_str = format!("-{limit}");
                run_git(&["log", &n_str, fmt]).await
            }

            "blame" => {
                let paths: Vec<&str> = args["paths"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .map(|p| vec![p])
                    .unwrap_or_default();

                if paths.is_empty() {
                    return Err(anyhow::anyhow!("blame requires a path"));
                }

                if let Some(line) = args["line"].as_u64() {
                    let range = format!("-L {line},{line}");
                    run_git(&["blame", &range, paths[0]]).await
                } else {
                    run_git(&["blame", paths[0]]).await
                }
            }

            "restore" => {
                let paths: Vec<&str> = args["paths"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();

                if paths.is_empty() {
                    return Err(anyhow::anyhow!("restore requires at least one path"));
                }

                let mut git_args = vec!["restore", "--"];
                git_args.extend(paths.iter().copied());
                run_git(&git_args).await
            }

            "stash" => {
                let sub = args["stash_action"].as_str().unwrap_or("list");
                match sub {
                    "push" => run_git(&["stash", "push"]).await,
                    "pop" => run_git(&["stash", "pop"]).await,
                    "drop" => run_git(&["stash", "drop"]).await,
                    _ => run_git(&["stash", "list"]).await,
                }
            }

            other => Err(anyhow::anyhow!("unknown git action: {other}")),
        }
    }
}

async fn run_git(args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(args)
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

    Ok(if result.is_empty() { "(no output)".to_string() } else { result })
}
