//! Shared policy for destructive tools: plan-mode confirmation and git checkpoints.

use serde_json::Value;

use crate::tools::database::database_action_is_mutating;
use crate::tools::docker::docker_action_is_mutating;
use crate::tools::notebook::notebook_action_is_mutating;

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
    if name == "notebook" {
        return notebook_action_is_mutating(args);
    }
    if name == "docker" {
        return docker_action_is_mutating(args);
    }
    if name == "database" {
        // When the tool is configured readonly (default), execute rejects writes.
        // If readonly was disabled, treat non-SELECT queries as mutating.
        // We cannot read runtime config here; use SQL shape: non-readonly SQL → checkpoint.
        return database_action_is_mutating(args, false);
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

    #[test]
    fn unknown_and_readonly_tools_skip_checkpoint() {
        assert!(!tool_requires_checkpoint("read_file", &json!({})));
        assert!(!tool_requires_checkpoint("search_code", &json!({})));
        assert!(!tool_requires_checkpoint("spawn_swarm", &json!({})));
        assert!(!tool_requires_checkpoint("git", &json!({"action": "log"})));
        assert!(!tool_requires_checkpoint(
            "git",
            &json!({"action": "blame"})
        ));
        assert!(!tool_requires_checkpoint(
            "git",
            &json!({"action": "fetch"})
        ));
        assert!(!tool_requires_checkpoint("git", &json!({})));
        assert!(!tool_requires_checkpoint(
            "git",
            &json!({"action": "unknown_action"})
        ));
    }

    #[test]
    fn confirmation_mirrors_checkpoint_policy() {
        assert_eq!(
            tool_requires_confirmation("write_file", &json!({})),
            tool_requires_checkpoint("write_file", &json!({}))
        );
        assert_eq!(
            tool_requires_confirmation("git", &json!({"action": "status"})),
            tool_requires_checkpoint("git", &json!({"action": "status"}))
        );
        assert_eq!(
            tool_requires_confirmation("git", &json!({"action": "checkout"})),
            tool_requires_checkpoint("git", &json!({"action": "checkout"}))
        );
    }

    #[test]
    fn git_stash_list_is_readonly_pop_is_mutating() {
        assert!(!tool_requires_checkpoint(
            "git",
            &json!({"action": "stash", "stash_action": "list"})
        ));
        assert!(tool_requires_checkpoint(
            "git",
            &json!({"action": "stash", "stash_action": "pop"})
        ));
        assert!(tool_requires_checkpoint(
            "git",
            &json!({"action": "stash", "stash_action": "drop"})
        ));
        // stash without stash_action is not treated as mutating
        assert!(!tool_requires_checkpoint(
            "git",
            &json!({"action": "stash"})
        ));
    }

    #[test]
    fn notebook_write_requires_checkpoint_read_skips() {
        assert!(tool_requires_checkpoint(
            "notebook",
            &json!({"action": "write_cell", "path": "a.ipynb", "index": 0})
        ));
        assert!(tool_requires_checkpoint(
            "notebook",
            &json!({"action": "add_cell", "path": "a.ipynb"})
        ));
        assert!(!tool_requires_checkpoint(
            "notebook",
            &json!({"action": "list_cells", "path": "a.ipynb"})
        ));
        assert!(!tool_requires_checkpoint(
            "notebook",
            &json!({"action": "read_cell", "path": "a.ipynb", "index": 0})
        ));
        assert!(!tool_requires_checkpoint(
            "notebook",
            &json!({"action": "metadata", "path": "a.ipynb"})
        ));
    }

    #[test]
    fn docker_mutating_requires_checkpoint_readonly_skips() {
        assert!(tool_requires_checkpoint(
            "docker",
            &json!({"action": "compose_up"})
        ));
        assert!(!tool_requires_checkpoint(
            "docker",
            &json!({"action": "ps"})
        ));
        assert!(!tool_requires_checkpoint(
            "docker",
            &json!({"action": "logs", "container": "x"})
        ));
        assert!(!tool_requires_checkpoint(
            "docker",
            &json!({"action": "images"})
        ));
    }

    #[test]
    fn database_write_sql_requires_checkpoint_select_skips() {
        assert!(!tool_requires_checkpoint(
            "database",
            &json!({"action": "query", "sql": "SELECT 1", "path": "a.db"})
        ));
        assert!(!tool_requires_checkpoint(
            "database",
            &json!({"action": "list_tables", "path": "a.db"})
        ));
        assert!(!tool_requires_checkpoint(
            "database",
            &json!({"action": "schema", "path": "a.db", "table": "t"})
        ));
        assert!(tool_requires_checkpoint(
            "database",
            &json!({"action": "query", "sql": "DELETE FROM t", "path": "a.db"})
        ));
    }
}
