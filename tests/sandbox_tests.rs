//! Sandbox-boundary integration tests.
//!
//! These tests construct each tool with `WorkspaceRoot` in `Strict` mode (the
//! security default) and verify that:
//!
//!   * Operations under the workspace root succeed.
//!   * Operations that try to escape the workspace root via `..`, an absolute
//!     path outside the root, or (on Unix) a symlink, are rejected.
//!   * Existing safety nets (`ShellConfig::denylist`, `cmd_allowlist`) still
//!     fire under the new sandbox.
//!
//! These tests cost nothing at runtime, require no API keys, and gate the
//! single most security-sensitive surface in the agent: filesystem and shell
//! tools driven by an LLM.

use harness_provider_core::{ToolCall, ToolCallFunction};
use harness_tools::tools::{
    ListDirTool, PatchFileTool, ReadFileTool, ShellConfig, ShellTool, WriteFileTool,
};
use harness_tools::{SandboxMode, ToolExecutor, ToolRegistry, WorkspaceRoot};
use serde_json::json;
use std::sync::Arc;
use tempfile::tempdir;

fn strict(dir: &std::path::Path) -> Arc<WorkspaceRoot> {
    Arc::new(WorkspaceRoot::new(dir.to_path_buf(), SandboxMode::Strict).unwrap())
}

fn call(name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall {
        id: format!("{name}-call"),
        kind: "function".into(),
        function: ToolCallFunction {
            name: name.into(),
            arguments: args.to_string(),
        },
    }
}

// ── filesystem tools ──────────────────────────────────────────────────────────

#[tokio::test]
async fn read_file_strict_allows_inside_root() {
    let dir = tempdir().unwrap();
    let inside = dir.path().join("ok.txt");
    std::fs::write(&inside, b"hello").unwrap();

    let ws = strict(dir.path());
    let mut reg = ToolRegistry::new();
    reg.register(ReadFileTool { workspace: ws });
    let exec = ToolExecutor::new(reg);

    let out = exec
        .execute(&call("read_file", json!({"path": "ok.txt"})))
        .await;
    assert!(
        out.contains("hello"),
        "read should succeed inside root: {out}"
    );
}

#[tokio::test]
async fn read_file_strict_rejects_dot_dot_escape() {
    let outside_dir = tempdir().unwrap();
    // Distinctive canary — confirms no actual file content leaks back through the tool.
    const CANARY: &str = "OUTSIDE-ONLY-CANARY-VALUE-XYZZY";
    let outside_file = outside_dir.path().join("victim.txt");
    std::fs::write(&outside_file, CANARY).unwrap();

    let inside = tempdir().unwrap();
    let ws = strict(inside.path());
    let mut reg = ToolRegistry::new();
    reg.register(ReadFileTool { workspace: ws });
    let exec = ToolExecutor::new(reg);

    // Try to escape: relative path that climbs out of the workspace.
    let attack = "../../../../../../../etc/passwd";
    let out = exec
        .execute(&call("read_file", json!({"path": attack})))
        .await;
    assert!(
        out.contains("escapes workspace root") || out.contains("error"),
        "dot-dot escape must be rejected, got: {out}"
    );
    assert!(!out.contains(CANARY), "leaked canary content: {out}");

    // Try to escape: absolute path outside root.
    let out = exec
        .execute(&call(
            "read_file",
            json!({"path": outside_file.to_str().unwrap()}),
        ))
        .await;
    assert!(
        out.contains("escapes workspace root") || out.contains("error"),
        "absolute outside-root path must be rejected, got: {out}"
    );
    assert!(!out.contains(CANARY), "leaked canary content: {out}");
}

#[tokio::test]
async fn write_file_strict_rejects_dot_dot_escape() {
    let dir = tempdir().unwrap();
    let ws = strict(dir.path());
    let mut reg = ToolRegistry::new();
    reg.register(WriteFileTool { workspace: ws });
    let exec = ToolExecutor::new(reg);

    let out = exec
        .execute(&call(
            "write_file",
            json!({"path": "../escape.txt", "content": "pwned"}),
        ))
        .await;
    assert!(
        out.contains("escapes workspace root") || out.contains("error"),
        "write_file must reject escape, got: {out}"
    );
}

#[tokio::test]
async fn write_file_strict_rejects_absolute_outside() {
    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let ws = strict(dir.path());
    let mut reg = ToolRegistry::new();
    reg.register(WriteFileTool { workspace: ws });
    let exec = ToolExecutor::new(reg);

    let target = outside.path().join("pwned.txt");
    let out = exec
        .execute(&call(
            "write_file",
            json!({"path": target.to_str().unwrap(), "content": "pwned"}),
        ))
        .await;
    assert!(
        out.contains("escapes workspace root") || out.contains("error"),
        "absolute outside-root write must be rejected, got: {out}"
    );
    assert!(!target.exists(), "file was created outside the sandbox!");
}

#[tokio::test]
async fn list_dir_strict_rejects_escape() {
    let dir = tempdir().unwrap();
    let ws = strict(dir.path());
    let mut reg = ToolRegistry::new();
    reg.register(ListDirTool { workspace: ws });
    let exec = ToolExecutor::new(reg);

    let out = exec
        .execute(&call("list_dir", json!({"path": "../"})))
        .await;
    assert!(
        out.contains("escapes workspace root") || out.contains("error"),
        "list_dir must reject ../, got: {out}"
    );
}

#[tokio::test]
async fn patch_file_strict_rejects_escape() {
    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let outside_file = outside.path().join("victim.txt");
    std::fs::write(&outside_file, b"hello world\n").unwrap();

    let ws = strict(dir.path());
    let mut reg = ToolRegistry::new();
    reg.register(PatchFileTool { workspace: ws });
    let exec = ToolExecutor::new(reg);

    let out = exec
        .execute(&call(
            "patch_file",
            json!({
                "path": outside_file.to_str().unwrap(),
                "old_content": "hello",
                "new_content": "PWNED"
            }),
        ))
        .await;
    assert!(
        out.contains("escapes workspace root") || out.contains("error"),
        "patch_file must reject escape, got: {out}"
    );
    let after = std::fs::read_to_string(&outside_file).unwrap();
    assert_eq!(after, "hello world\n", "file outside sandbox was modified!");
}

// ── shell tool ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn shell_cwd_strict_rejects_escape() {
    let dir = tempdir().unwrap();
    let ws = strict(dir.path());
    let mut reg = ToolRegistry::new();
    reg.register(ShellTool::new(ShellConfig::default(), ws));
    let exec = ToolExecutor::new(reg);

    let out = exec
        .execute(&call(
            "shell",
            json!({"command": "echo from-escape", "cwd": "../"}),
        ))
        .await;
    assert!(
        out.contains("escapes workspace root") || out.contains("error"),
        "shell cwd escape must be rejected, got: {out}"
    );
}

#[tokio::test]
async fn shell_denylist_blocks_destructive_command() {
    let dir = tempdir().unwrap();
    let ws = strict(dir.path());
    let mut reg = ToolRegistry::new();
    reg.register(ShellTool::new(ShellConfig::default(), ws));
    let exec = ToolExecutor::new(reg);

    let out = exec
        .execute(&call("shell", json!({"command": "rm -rf /"})))
        .await;
    assert!(
        out.contains("denylist") || out.contains("blocked") || out.contains("error"),
        "rm -rf / must be blocked, got: {out}"
    );
}

#[tokio::test]
async fn shell_cmd_allowlist_blocks_unlisted_absolute_command() {
    let dir = tempdir().unwrap();
    let ws = strict(dir.path());

    let cfg = ShellConfig {
        cmd_allowlist: Some(vec!["/bin/echo".into()]),
        ..ShellConfig::default()
    };
    let mut reg = ToolRegistry::new();
    reg.register(ShellTool::new(cfg, ws));
    let exec = ToolExecutor::new(reg);

    // Allowed
    let allowed = exec
        .execute(&call("shell", json!({"command": "/bin/echo allowed"})))
        .await;
    assert!(
        allowed.contains("allowed"),
        "allowlisted command should run, got: {allowed}"
    );

    // Denied: a different absolute path that is not in the allowlist.
    let denied = exec
        .execute(&call("shell", json!({"command": "/usr/bin/whoami"})))
        .await;
    assert!(
        denied.contains("not in") || denied.contains("allowlist") || denied.contains("error"),
        "non-allowlisted absolute command must be blocked, got: {denied}"
    );
}

#[tokio::test]
async fn shell_cwd_inside_root_succeeds() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    let ws = strict(dir.path());
    let mut reg = ToolRegistry::new();
    reg.register(ShellTool::new(ShellConfig::default(), ws));
    let exec = ToolExecutor::new(reg);

    let out = exec
        .execute(&call("shell", json!({"command": "echo ok", "cwd": "sub"})))
        .await;
    assert!(
        out.contains("ok"),
        "cwd inside root must succeed, got: {out}"
    );
}
