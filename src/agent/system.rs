//! Default system prompt and project instruction loaders.

use std::path::Path;

pub const DEFAULT_SYSTEM: &str = "\
You are a powerful coding assistant running in a terminal.

Available tools:
  read_file, write_file     — read or overwrite files
  patch_file                — surgical old→new text replacement (prefer over write_file for edits)
  apply_patch               — apply a unified diff across multiple files atomically (prefer for multi-file edits)
  list_dir                  — list directory contents
  shell                     — run shell commands (build, test, etc.)
  git                       — typed git operations: status/diff/add/commit/branch/push/log/blame/restore
  test_runner               — run project tests with structured pass/fail output
  search_code               — regex search across the codebase
  spawn_agent               — run a sub-agent with base tools for parallel tasks
  spawn_swarm               — queue parallel background tasks (harness swarm list / result)
  find_definition           — LSP go-to-definition across the codebase
  find_references           — LSP find all references to a symbol
  rename_symbol             — LSP safe rename across files
  diagnostics               — LSP errors/warnings for a file
  browser (when enabled)    — Chrome CDP: navigate, screenshot, click, fill forms
  web_search (when enabled) — provider-native web search (no extra config needed)
  bash (when enabled)       — provider-native sandboxed code execution
  MCP tools (when loaded)   — any tools registered via .harness/mcp.json
  gh                        — GitHub CLI wrapper: PR/issue/CI workflow

Guidelines:
  - Prefer patch_file for single-file edits, apply_patch for multi-file changes.
  - Use the git tool for all git operations instead of shell git commands.
  - Always run test_runner after changes to verify correctness.
  - Use web_search when you need up-to-date information or documentation.
  - Be concise. Prefer making changes over explaining them.
  - When editing multiple files, use spawn_agent for parallelism.
  - In plan mode (--plan flag), destructive calls pause for user approval.";


/// Load a project-specific system prompt prefix from well-known files in CWD.
/// Checks (in order): .harness/SYSTEM.md, AGENTS.md, CLAUDE.md
pub fn load_project_instructions() -> Option<String> {
    load_project_instructions_in(Path::new("."))
}

/// Load project instructions from well-known files under `root` (path-injectable for tests).
/// Checks (in order): `.harness/SYSTEM.md`, `AGENTS.md`, `CLAUDE.md`.
pub fn load_project_instructions_in(root: &Path) -> Option<String> {
    let candidates = [".harness/SYSTEM.md", "AGENTS.md", "CLAUDE.md"];
    for rel in &candidates {
        let path = root.join(rel);
        if let Ok(text) = std::fs::read_to_string(&path) {
            if !text.trim().is_empty() {
                tracing::debug!(file = %path.display(), "loaded project instructions");
                return Some(format!("## Project instructions (from {rel})\n\n{text}"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn default_system_mentions_core_tools_and_guidelines() {
        assert!(DEFAULT_SYSTEM.contains("patch_file"));
        assert!(DEFAULT_SYSTEM.contains("apply_patch"));
        assert!(DEFAULT_SYSTEM.contains("test_runner"));
        assert!(DEFAULT_SYSTEM.contains("spawn_swarm"));
        assert!(DEFAULT_SYSTEM.contains("Prefer patch_file"));
        assert!(!DEFAULT_SYSTEM.trim().is_empty());
    }

    #[test]
    fn load_project_instructions_in_returns_none_when_missing() {
        let dir = tempdir().expect("tempdir");
        assert!(load_project_instructions_in(dir.path()).is_none());
    }

    #[test]
    fn load_project_instructions_in_prefers_harness_system_md() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join(".harness")).expect("mkdir .harness");
        fs::write(dir.path().join(".harness/SYSTEM.md"), "from system\n").expect("write SYSTEM");
        fs::write(dir.path().join("AGENTS.md"), "from agents\n").expect("write AGENTS");
        fs::write(dir.path().join("CLAUDE.md"), "from claude\n").expect("write CLAUDE");

        let loaded = load_project_instructions_in(dir.path()).expect("instructions");
        assert!(loaded.contains("from system"));
        assert!(loaded.contains("from .harness/SYSTEM.md"));
        assert!(!loaded.contains("from agents"));
        assert!(!loaded.contains("from claude"));
    }

    #[test]
    fn load_project_instructions_in_falls_back_to_agents_then_claude() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("AGENTS.md"), "agents body").expect("write AGENTS");
        fs::write(dir.path().join("CLAUDE.md"), "claude body").expect("write CLAUDE");
        let loaded = load_project_instructions_in(dir.path()).expect("agents");
        assert!(loaded.contains("agents body"));
        assert!(loaded.contains("from AGENTS.md"));

        fs::remove_file(dir.path().join("AGENTS.md")).expect("rm AGENTS");
        let loaded = load_project_instructions_in(dir.path()).expect("claude");
        assert!(loaded.contains("claude body"));
        assert!(loaded.contains("from CLAUDE.md"));
    }

    #[test]
    fn load_project_instructions_in_skips_empty_files() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join(".harness")).expect("mkdir");
        fs::write(dir.path().join(".harness/SYSTEM.md"), "   \n\t\n").expect("empty SYSTEM");
        fs::write(dir.path().join("AGENTS.md"), "   ").expect("empty AGENTS");
        fs::write(dir.path().join("CLAUDE.md"), "real instructions").expect("claude");

        let loaded = load_project_instructions_in(dir.path()).expect("claude fallback");
        assert!(loaded.contains("real instructions"));
        assert!(loaded.contains("from CLAUDE.md"));
    }
}
