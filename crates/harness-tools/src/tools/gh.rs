//! GitHub CLI tool — wraps `gh` for PR, issue, and CI workflow operations.
//! Requires `gh` to be installed and authenticated on the system.

use anyhow::Result;
use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
use tokio::process::Command;
use tracing::debug;

use crate::registry::Tool;

/// Wrapper around the `gh` CLI for PR, issue, and CI workflow queries.
///
/// Actions:
///   pr_list        — list open PRs
///   pr_view N      — view PR #N (diff + description)
///   pr_diff N      — full diff for PR #N
///   pr_checks N    — CI status for PR #N
///   pr_comment N msg — post a comment on PR #N
///   pr_create      — open a PR (title required; optional body, base, head)
///   issue_list     — list open issues
///   issue_view N   — view issue #N
///   run_view N     — view workflow run #N
///   run_logs N     — fetch logs for workflow run #N
/// GitHub CLI wrapper tool.
pub struct GhTool;

#[async_trait]
impl Tool for GhTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "gh",
            "GitHub CLI wrapper. Actions: pr_list, pr_view <n>, pr_diff <n>, pr_checks <n>, \
             pr_comment <n> <msg>, pr_create (title, body?, base?, head?), issue_list, \
             issue_view <n>, run_view <n>, run_logs <n>.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "pr_list", "pr_view", "pr_diff", "pr_checks", "pr_comment",
                            "pr_create", "issue_list", "issue_view", "run_view", "run_logs"
                        ]
                    },
                    "number": {
                        "type": "integer",
                        "description": "PR, issue, or run number"
                    },
                    "title": {
                        "type": "string",
                        "description": "PR title (for pr_create)"
                    },
                    "body": {
                        "type": "string",
                        "description": "PR or comment body"
                    },
                    "base": {
                        "type": "string",
                        "description": "Base branch (for pr_create)"
                    },
                    "head": {
                        "type": "string",
                        "description": "Head branch (for pr_create)"
                    },
                    "message": {
                        "type": "string",
                        "description": "Comment body (for pr_comment; alias for body)"
                    }
                },
                "required": ["action"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("pr_list");
        let num = args["number"].as_u64();
        debug!(action, "gh tool execute");

        match action {
            "pr_list" => {
                run_gh(&[
                    "pr",
                    "list",
                    "--json",
                    "number,title,headRefName,state,updatedAt",
                    "--limit",
                    "20",
                ])
                .await
            }
            "pr_view" => {
                let n = require_number(num)?;
                run_gh(&[
                    "pr",
                    "view",
                    &n,
                    "--json",
                    "number,title,body,state,reviews,assignees,labels",
                ])
                .await
            }
            "pr_diff" => {
                let n = require_number(num)?;
                run_gh(&["pr", "diff", &n]).await
            }
            "pr_checks" => {
                let n = require_number(num)?;
                run_gh(&["pr", "checks", &n]).await
            }
            "pr_comment" => {
                let n = require_number(num)?;
                let msg = args["message"]
                    .as_str()
                    .or_else(|| args["body"].as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if msg.is_empty() {
                    anyhow::bail!("message is required for pr_comment");
                }
                run_gh(&["pr", "comment", &n, "--body", &msg]).await
            }
            "pr_create" => {
                let title = args["title"].as_str().unwrap_or("").trim().to_string();
                if title.is_empty() {
                    anyhow::bail!("title is required for pr_create");
                }
                let body = args["body"].as_str().unwrap_or("").trim();
                let mut gh_args = vec![
                    "pr",
                    "create",
                    "--title",
                    &title,
                    "--json",
                    "url,title,number",
                ];
                if !body.is_empty() {
                    gh_args.push("--body");
                    gh_args.push(body);
                }
                if let Some(base) = args["base"].as_str().filter(|s| !s.is_empty()) {
                    gh_args.push("--base");
                    gh_args.push(base);
                }
                if let Some(head) = args["head"].as_str().filter(|s| !s.is_empty()) {
                    gh_args.push("--head");
                    gh_args.push(head);
                }
                run_gh(&gh_args).await
            }
            "issue_list" => {
                run_gh(&[
                    "issue",
                    "list",
                    "--json",
                    "number,title,state,labels,updatedAt",
                    "--limit",
                    "20",
                ])
                .await
            }
            "issue_view" => {
                let n = require_number(num)?;
                run_gh(&[
                    "issue",
                    "view",
                    &n,
                    "--json",
                    "number,title,body,state,comments",
                ])
                .await
            }
            "run_view" => {
                let n = require_number(num)?;
                run_gh(&["run", "view", &n, "--json", "status,conclusion,name,jobs"]).await
            }
            "run_logs" => {
                let n = require_number(num)?;
                run_gh(&["run", "view", "--log", &n]).await
            }
            _ => anyhow::bail!("Unknown gh action: {action}"),
        }
    }
}

fn require_number(num: Option<u64>) -> Result<String> {
    num.map(|n| n.to_string())
        .ok_or_else(|| anyhow::anyhow!("'number' is required for this action"))
}

async fn run_gh(args: &[&str]) -> Result<String> {
    // Check gh is available
    let which = Command::new("which").arg("gh").output().await;
    if which.map(|o| !o.status.success()).unwrap_or(true) {
        return Ok("gh CLI not found. Install from https://cli.github.com/".to_string());
    }

    let out = Command::new("gh").args(args).output().await?;

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    if !out.status.success() {
        if stderr.trim().is_empty() {
            anyhow::bail!("gh exited with status {}: {stdout}", out.status);
        }
        anyhow::bail!("gh error: {stderr}");
    }

    // Try to pretty-print JSON output for readability
    if stdout.trim_start().starts_with('[') || stdout.trim_start().starts_with('{') {
        if let Ok(val) = serde_json::from_str::<Value>(&stdout) {
            return Ok(serde_json::to_string_pretty(&val).unwrap_or(stdout));
        }
    }

    Ok(stdout)
}

/// List open PRs. Convenience for TUI /pr command.
pub async fn pr_list() -> Result<String> {
    run_gh(&[
        "pr",
        "list",
        "--json",
        "number,title,headRefName,state,updatedAt",
        "--limit",
        "20",
    ])
    .await
}

/// Fetch PR diff + comments and return as a context string.
/// Used by `harness pr <num>` CLI to pre-load agent context.
pub async fn pr_context(number: u64) -> Result<String> {
    let n = number.to_string();
    let view = run_gh(&[
        "pr",
        "view",
        &n,
        "--json",
        "number,title,body,state,reviews,comments",
    ])
    .await
    .unwrap_or_default();
    let diff = run_gh(&["pr", "diff", &n]).await.unwrap_or_default();
    let checks = run_gh(&["pr", "checks", &n]).await.unwrap_or_default();

    Ok(format!(
        "# PR #{number} — Context\n\n## PR Details\n{view}\n\n## Diff\n```diff\n{diff}\n```\n\n## CI Checks\n{checks}"
    ))
}
