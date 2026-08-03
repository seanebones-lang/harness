//! `docker` tool — allowlisted read-heavy Docker CLI actions.
//!
//! No docker.sock mounting, no run/build/rm by default. Optional `compose_up` only when
//! `allow_mutating` is enabled in config.

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

use crate::registry::Tool;

/// Default CLI timeout for docker invocations.
pub const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Configuration for [`DockerTool`].
#[derive(Debug, Clone)]
pub struct DockerToolConfig {
    /// When true, allow `compose_up` (still no run/build/rm).
    pub allow_mutating: bool,
    /// Process timeout in seconds.
    pub timeout_secs: u64,
    /// Override path to the `docker` binary (tests can point at a missing path).
    pub docker_bin: PathBuf,
}

impl Default for DockerToolConfig {
    fn default() -> Self {
        Self {
            allow_mutating: false,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            docker_bin: PathBuf::from("docker"),
        }
    }
}

/// Allowlisted Docker CLI wrapper.
pub struct DockerTool {
    /// Tool options.
    pub config: DockerToolConfig,
}

impl DockerTool {
    /// Create with the given config.
    pub fn new(config: DockerToolConfig) -> Self {
        Self { config }
    }
}

/// Actions permitted when `allow_mutating` is false.
pub const READONLY_ACTIONS: &[&str] = &["ps", "logs", "compose_ps", "compose_logs", "images"];

/// Actions that require `allow_mutating = true`.
pub const MUTATING_ACTIONS: &[&str] = &["compose_up"];

/// Validate action name against allowlists (does not run docker).
pub fn validate_docker_action(action: &str, allow_mutating: bool) -> Result<(), String> {
    if READONLY_ACTIONS.contains(&action) {
        return Ok(());
    }
    if MUTATING_ACTIONS.contains(&action) {
        if allow_mutating {
            return Ok(());
        }
        return Err(format!(
            "docker action '{action}' requires [tools.docker] allow_mutating = true"
        ));
    }
    Err(format!(
        "unknown or disallowed docker action: {action}. Allowed: {}{}",
        READONLY_ACTIONS.join(", "),
        if allow_mutating {
            format!(", {}", MUTATING_ACTIONS.join(", "))
        } else {
            String::new()
        }
    ))
}

/// True when the docker call mutates runtime state (checkpoint/confirm).
pub fn docker_action_is_mutating(args: &Value) -> bool {
    matches!(
        args.get("action").and_then(Value::as_str),
        Some(a) if MUTATING_ACTIONS.contains(&a)
    )
}

/// Build argv for the docker CLI (without the binary path). Public for unit tests.
pub fn build_docker_args(action: &str, args: &Value) -> Result<Vec<String>, String> {
    match action {
        "ps" => {
            let mut a = vec!["ps".into(), "--format".into(), "{{.ID}}\t{{.Image}}\t{{.Status}}\t{{.Names}}".into()];
            if args.get("all").and_then(Value::as_bool).unwrap_or(false) {
                a.insert(1, "-a".into());
            }
            Ok(a)
        }
        "logs" => {
            let id = args
                .get("container")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "logs requires `container`".to_string())?;
            validate_safe_token(id, "container")?;
            let mut a = vec!["logs".into()];
            if let Some(n) = args.get("tail").and_then(Value::as_u64) {
                a.push("--tail".into());
                a.push(n.min(5000).to_string());
            } else {
                a.push("--tail".into());
                a.push("200".into());
            }
            a.push(id.into());
            Ok(a)
        }
        "images" => Ok(vec![
            "images".into(),
            "--format".into(),
            "{{.Repository}}:{{.Tag}}\t{{.ID}}\t{{.Size}}".into(),
        ]),
        "compose_ps" => {
            let mut a = compose_prefix(args)?;
            a.push("ps".into());
            Ok(a)
        }
        "compose_logs" => {
            let mut a = compose_prefix(args)?;
            a.push("logs".into());
            a.push("--no-color".into());
            if let Some(n) = args.get("tail").and_then(Value::as_u64) {
                a.push("--tail".into());
                a.push(n.min(5000).to_string());
            } else {
                a.push("--tail".into());
                a.push("200".into());
            }
            if let Some(svc) = args.get("service").and_then(Value::as_str) {
                validate_safe_token(svc, "service")?;
                a.push(svc.into());
            }
            Ok(a)
        }
        "compose_up" => {
            let mut a = compose_prefix(args)?;
            a.push("up".into());
            a.push("-d".into());
            if let Some(svc) = args.get("service").and_then(Value::as_str) {
                validate_safe_token(svc, "service")?;
                a.push(svc.into());
            }
            Ok(a)
        }
        other => Err(format!("cannot build args for action: {other}")),
    }
}

fn compose_prefix(args: &Value) -> Result<Vec<String>, String> {
    let mut a = vec!["compose".into()];
    if let Some(file) = args.get("file").and_then(Value::as_str) {
        // Allow relative compose file names only (no shell metacharacters).
        validate_compose_file(file)?;
        a.push("-f".into());
        a.push(file.into());
    }
    if let Some(project) = args.get("project").and_then(Value::as_str) {
        validate_safe_token(project, "project")?;
        a.push("-p".into());
        a.push(project.into());
    }
    Ok(a)
}

fn validate_safe_token(s: &str, field: &str) -> Result<(), String> {
    if s.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '/'))
    {
        return Err(format!(
            "invalid {field}: {s:?} (allowed: alnum, _ - . : /)"
        ));
    }
    Ok(())
}

fn validate_compose_file(s: &str) -> Result<(), String> {
    if s.is_empty() || s.contains("..") || s.starts_with('/') {
        return Err(format!(
            "invalid compose file path: {s:?} (use a relative path without ..)"
        ));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
    {
        return Err(format!("invalid compose file path characters: {s:?}"));
    }
    Ok(())
}

#[async_trait]
impl Tool for DockerTool {
    fn definition(&self) -> ToolDefinition {
        let mut actions = READONLY_ACTIONS.to_vec();
        if self.config.allow_mutating {
            actions.extend_from_slice(MUTATING_ACTIONS);
        }
        ToolDefinition::new(
            "docker",
            "Allowlisted Docker CLI (read-heavy). Actions: ps, logs, compose_ps, compose_logs, images. \
             Optional compose_up only when config allow_mutating=true. \
             Does NOT mount docker.sock, run, build, or rm containers by default.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": actions,
                        "description": "Docker operation."
                    },
                    "container": {
                        "type": "string",
                        "description": "Container id or name (logs)."
                    },
                    "service": {
                        "type": "string",
                        "description": "Compose service name (compose_logs / compose_up)."
                    },
                    "file": {
                        "type": "string",
                        "description": "Relative compose file path (default: docker-compose.yml discovery)."
                    },
                    "project": {
                        "type": "string",
                        "description": "Compose project name (-p)."
                    },
                    "tail": {
                        "type": "integer",
                        "description": "Log tail lines (default 200, max 5000)."
                    },
                    "all": {
                        "type": "boolean",
                        "description": "For ps: include stopped containers (-a)."
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

        validate_docker_action(action, self.config.allow_mutating)
            .map_err(|e| anyhow::anyhow!(e))?;

        let cli_args = build_docker_args(action, &args).map_err(|e| anyhow::anyhow!(e))?;

        run_docker(&self.config.docker_bin, &cli_args, self.config.timeout_secs).await
    }
}

async fn run_docker(bin: &std::path::Path, args: &[String], timeout_secs: u64) -> anyhow::Result<String> {
    // Honest error if binary missing.
    if bin.as_os_str() != "docker" {
        if !bin.exists() {
            anyhow::bail!(
                "docker binary not found at {}. Install Docker and ensure `docker` is on PATH.",
                bin.display()
            );
        }
    } else {
        // Quick which-style probe without depending on `which` crate in tests for "docker".
        let probe = Command::new(bin)
            .arg("version")
            .arg("--format")
            .arg("{{.Client.Version}}")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await;
        match probe {
            Ok(out) if out.status.success() => {}
            Ok(_) | Err(_) => {
                // Fall through and try the real command; if still fails, report honestly.
                // Distinguish "not found" from daemon errors via spawn error kind.
            }
        }
    }

    let child = match Command::new(bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!(
                "docker CLI not found (looked for {}). Install Docker Desktop / docker engine \
                 and ensure the `docker` binary is on PATH.",
                bin.display()
            );
        }
        Err(e) => return Err(e.into()),
    };

    let output = tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("docker timed out after {timeout_secs}s"))??;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        anyhow::bail!("docker {} failed: {msg}", args.join(" "));
    }

    let result = if stdout.is_empty() && !stderr.is_empty() {
        stderr
    } else {
        stdout
    };
    Ok(if result.is_empty() {
        "(no output)".into()
    } else {
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validate_readonly_actions() {
        for a in READONLY_ACTIONS {
            assert!(validate_docker_action(a, false).is_ok());
        }
        assert!(validate_docker_action("compose_up", false).is_err());
        assert!(validate_docker_action("compose_up", true).is_ok());
        assert!(validate_docker_action("run", false).is_err());
        assert!(validate_docker_action("build", true).is_err());
        assert!(validate_docker_action("rm", false).is_err());
    }

    #[test]
    fn build_args_ps_logs_images() {
        let ps = build_docker_args("ps", &json!({})).unwrap();
        assert_eq!(ps[0], "ps");

        let ps_all = build_docker_args("ps", &json!({"all": true})).unwrap();
        assert!(ps_all.iter().any(|s| s == "-a"));

        let logs = build_docker_args("logs", &json!({"container": "abc", "tail": 10})).unwrap();
        assert_eq!(logs[0], "logs");
        assert!(logs.iter().any(|s| s == "abc"));
        assert!(logs.iter().any(|s| s == "10"));

        assert!(build_docker_args("logs", &json!({})).is_err());

        let images = build_docker_args("images", &json!({})).unwrap();
        assert_eq!(images[0], "images");
    }

    #[test]
    fn build_args_compose_and_reject_bad_tokens() {
        let cps = build_docker_args(
            "compose_ps",
            &json!({"file": "docker-compose.yml", "project": "demo"}),
        )
        .unwrap();
        assert_eq!(cps[0], "compose");
        assert!(cps.windows(2).any(|w| w[0] == "-f" && w[1] == "docker-compose.yml"));

        let cup = build_docker_args("compose_up", &json!({"service": "web"})).unwrap();
        assert!(cup.iter().any(|s| s == "up"));
        assert!(cup.iter().any(|s| s == "-d"));

        assert!(build_docker_args("logs", &json!({"container": "a;rm -rf"})).is_err());
        assert!(build_docker_args(
            "compose_ps",
            &json!({"file": "../etc/passwd"})
        )
        .is_err());
    }

    #[test]
    fn mutating_policy_helper() {
        assert!(!docker_action_is_mutating(&json!({"action": "ps"})));
        assert!(!docker_action_is_mutating(&json!({"action": "logs"})));
        assert!(docker_action_is_mutating(
            &json!({"action": "compose_up"})
        ));
    }

    #[tokio::test]
    async fn missing_binary_is_honest_error() {
        let tool = DockerTool::new(DockerToolConfig {
            allow_mutating: false,
            timeout_secs: 5,
            docker_bin: PathBuf::from("/nonexistent/path/to/docker-binary-xyz"),
        });
        let err = tool
            .execute(json!({"action": "ps"}))
            .await
            .expect_err("missing bin");
        let msg = err.to_string();
        assert!(
            msg.contains("not found") || msg.contains("docker"),
            "unexpected error: {msg}"
        );
    }

    #[tokio::test]
    async fn rejects_disallowed_action_without_running() {
        let tool = DockerTool::new(DockerToolConfig {
            allow_mutating: false,
            timeout_secs: 5,
            docker_bin: PathBuf::from("/nonexistent/docker"),
        });
        let err = tool
            .execute(json!({"action": "run"}))
            .await
            .expect_err("run blocked");
        assert!(err.to_string().contains("disallowed") || err.to_string().contains("unknown"));
    }
}
