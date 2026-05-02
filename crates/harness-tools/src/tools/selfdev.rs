//! Self-dev mode tools: let the agent modify its own source and trigger rebuilds.
//!
//! RebuildSelfTool  — runs `cargo build --profile selfdev` in the source dir.
//!                    Returns compiler output so the agent can fix errors.
//! ReloadSelfTool   — on Unix, execs the freshly-built binary (hot-reload).
//!                    On other platforms, prints the binary path and exits.

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::registry::Tool;

// ── RebuildSelfTool ───────────────────────────────────────────────────────────

pub struct RebuildSelfTool {
    src_dir: PathBuf,
    profile: String,
}

impl RebuildSelfTool {
    pub fn new(src_dir: PathBuf) -> Self {
        Self {
            src_dir,
            profile: "selfdev".into(),
        }
    }

    pub fn with_profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = profile.into();
        self
    }
}

#[async_trait]
impl Tool for RebuildSelfTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "rebuild_self",
            "Rebuild the harness binary from its Rust source. \
             Returns compiler output including any errors. \
             Call this after editing source files to check if the changes compile.",
            json!({
                "type": "object",
                "properties": {
                    "check_only": {
                        "type": "boolean",
                        "description": "If true, run `cargo check` only (fast, no binary produced)."
                    }
                }
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let check_only = args["check_only"].as_bool().unwrap_or(false);

        let subcmd = if check_only { "check" } else { "build" };
        let profile_arg = if check_only {
            vec![]
        } else {
            vec!["--profile".to_string(), self.profile.clone()]
        };

        let mut cmd = tokio::process::Command::new("cargo");
        cmd.arg(subcmd);
        cmd.args(&profile_arg);
        cmd.current_dir(&self.src_dir);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            cmd.spawn()?.wait_with_output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("cargo build timed out after 120s"))??;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let combined = format!("{stdout}{stderr}").trim().to_string();

        if output.status.success() {
            let binary = self
                .src_dir
                .join("target")
                .join(&self.profile)
                .join("harness");
            Ok(format!(
                "Build succeeded.\nBinary: {}\n\n{}",
                binary.display(),
                combined
            ))
        } else {
            Ok(format!(
                "Build FAILED (exit {}):\n\n{}",
                output.status, combined
            ))
        }
    }
}

// ── ReloadSelfTool ────────────────────────────────────────────────────────────

pub struct ReloadSelfTool {
    src_dir: PathBuf,
    profile: String,
}

impl ReloadSelfTool {
    pub fn new(src_dir: PathBuf) -> Self {
        Self {
            src_dir,
            profile: "selfdev".into(),
        }
    }
}

#[async_trait]
impl Tool for ReloadSelfTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "reload_self",
            "Hot-reload the harness by exec()-ing the freshly-built binary. \
             The current process is replaced in-place; the TUI/CLI session continues \
             with the new binary. Only call this after a successful rebuild.",
            json!({ "type": "object", "properties": {} }),
        )
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<String> {
        let binary = self
            .src_dir
            .join("target")
            .join(&self.profile)
            .join("harness");

        if !binary.exists() {
            anyhow::bail!(
                "Binary not found at {}. Run rebuild_self first.",
                binary.display()
            );
        }

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let err = std::process::Command::new(&binary)
                .args(std::env::args().skip(1))
                .exec(); // replaces current process
                         // exec() only returns on error
            anyhow::bail!("exec failed: {err}");
        }

        #[cfg(not(unix))]
        {
            Ok(format!(
                "Reload not supported on this platform. \
                 Restart manually with: {}",
                binary.display()
            ))
        }
    }
}
