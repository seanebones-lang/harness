//! Shared policy for destructive tools: plan-mode confirmation and git checkpoints.

use serde_json::Value;

/// Built-in tools that mutate the workspace without going through `git`.
pub const BUILTIN_DESTRUCTIVE_TOOLS: &[&str] =
    &["write_file", "patch_file", "shell", "apply_patch"];

/// Returns true when a tool call should trigger a git checkpoint stash before execution.
pub fn tool_requires_checkpoint(name: &str, args: &Value) -> bool {
    if BUILTIN_DESTRUCTIVE_TOOLS.contains(&name) {
        return true;
    }
    if name == "git" {
        return git_action_is_mutating(args);
    }
    false
}

/// Returns true when plan mode should pause for user confirmation (before MCP / always_ask rules).
pub fn tool_requires_confirmation(name: &str, args: &Value) -> bool {
    tool_requires_checkpoint(name, args)
}

fn git_action_is_mutating(args: &Value) -> bool {
    match args.get("action").and_then(Value::as_str) {
        Some("status" | "diff" | "log" | "blame" | "fetch") => false,
        Some("stash") => matches!(
            args.get("stash_action").and_then(Value::as_str),
            Some("push" | "pop" | "drop")
        ),
        Some(
            "add" | "commit" | "branch" | "push" | "restore" | "clone" | "pull" | "checkout"
            | "switch",
        ) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builtin_destructive_tools_require_checkpoint() {
        for name in BUILTIN_DESTRUCTIVE_TOOLS {
            assert!(tool_requires_checkpoint(name, &json!({})));
        }
    }

    #[test]
    fn git_readonly_skips_checkpoint() {
        assert!(!tool_requires_checkpoint(
            "git",
            &json!({"action": "status"})
        ));
        assert!(!tool_requires_checkpoint("git", &json!({"action": "diff"})));
        assert!(!tool_requires_checkpoint(
            "git",
            &json!({"action": "stash", "stash_action": "list"})
        ));
    }

    #[test]
    fn git_mutating_requires_checkpoint() {
        assert!(tool_requires_checkpoint(
            "git",
            &json!({"action": "commit", "message": "x"})
        ));
        assert!(tool_requires_checkpoint("git", &json!({"action": "push"})));
        assert!(tool_requires_checkpoint(
            "git",
            &json!({"action": "stash", "stash_action": "push"})
        ));
    }
}
