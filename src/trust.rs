//! Learning approval policy — `harness trust` and auto-trust after repeated approvals.
//!
//! Trust rules are stored in `~/.harness/trust.toml`:
//!
//! ```toml
//! [[rules]]
//! tool = "shell"
//! pattern = "cargo check"
//! added = "2026-04-29T12:00:00Z"
//!
//! [[rules]]
//! tool = "write_file"
//! pattern = "*"   # auto-approve all write_file
//! added = "..."
//! ```
//!
//! A pattern of `"*"` means always approve for that tool.
//! Otherwise the pattern is matched as a substring of the tool's first argument.
//!
//! The ConfirmGate in executor uses `TrustStore::is_trusted` to skip confirmation for
//! matched tool calls.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustRule {
    pub tool: String,
    pub pattern: String,
    pub added: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrustStore {
    #[serde(default)]
    pub rules: Vec<TrustRule>,
}

impl TrustStore {
    pub fn path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".harness")
            .join("trust.toml")
    }

    /// Load rules from an explicit path (tests + alternate stores).
    pub fn load_from_path(path: &std::path::Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn load() -> Self {
        Self::load_from_path(&Self::path())
    }

    /// Persist rules to an explicit path (creates parent dirs).
    pub fn save_to_path(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let toml = toml::to_string_pretty(self)?;
        std::fs::write(path, toml)?;
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        self.save_to_path(&Self::path())
    }

    /// Check if a tool call is trusted (skips confirmation gate).
    #[allow(dead_code)]
    pub fn is_trusted(&self, tool: &str, first_arg: &str) -> bool {
        for rule in &self.rules {
            if rule.tool != tool && rule.tool != "*" {
                continue;
            }
            if rule.pattern == "*" || first_arg.contains(&rule.pattern) {
                return true;
            }
        }
        false
    }

    /// Add a trust rule. Returns true if the rule was newly added.
    pub fn add_rule(&mut self, tool: &str, pattern: &str) -> bool {
        // Don't duplicate.
        if self
            .rules
            .iter()
            .any(|r| r.tool == tool && r.pattern == pattern)
        {
            return false;
        }
        self.rules.push(TrustRule {
            tool: tool.to_string(),
            pattern: pattern.to_string(),
            added: chrono::Utc::now().to_rfc3339(),
        });
        true
    }

    /// Remove a trust rule matching tool + pattern. Returns true if removed.
    pub fn remove_rule(&mut self, tool: &str, pattern: &str) -> bool {
        let before = self.rules.len();
        self.rules
            .retain(|r| !(r.tool == tool && r.pattern == pattern));
        self.rules.len() < before
    }

    /// List all rules.
    pub fn list(&self) -> &[TrustRule] {
        &self.rules
    }
}

// ── Approval frequency tracker ────────────────────────────────────────────────
//
// When the user approves the same tool+arg three times in a row, prompt to trust.
// This is stored in-memory only (per session).

use std::collections::HashMap;

#[allow(dead_code)]
pub struct ApprovalTracker {
    counts: HashMap<(String, String), usize>,
}

impl ApprovalTracker {
    #[allow(dead_code)]
    pub fn record(&mut self, tool: &str, first_arg: &str) -> usize {
        let key = (tool.to_string(), first_arg.to_string());
        let count = self.counts.entry(key).or_insert(0);
        *count += 1;
        *count
    }

    #[allow(dead_code)]
    pub fn should_prompt_to_trust(&self, tool: &str, first_arg: &str) -> bool {
        let key = (tool.to_string(), first_arg.to_string());
        self.counts.get(&key).copied().unwrap_or(0) >= 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_trusted_wildcard_tool_and_pattern() {
        let mut store = TrustStore::default();
        assert!(store.add_rule("*", "*"));
        assert!(store.is_trusted("shell", "rm -rf /"));
        assert!(!store.add_rule("*", "*")); // duplicate
    }

    #[test]
    fn is_trusted_substring_pattern() {
        let mut store = TrustStore::default();
        store.add_rule("shell", "cargo check");
        assert!(store.is_trusted("shell", "cargo check --all"));
        assert!(!store.is_trusted("shell", "cargo test"));
        assert!(!store.is_trusted("write_file", "cargo check"));
    }

    #[test]
    fn remove_rule_and_list() {
        let mut store = TrustStore::default();
        store.add_rule("shell", "ls");
        store.add_rule("shell", "pwd");
        assert_eq!(store.list().len(), 2);
        assert!(store.remove_rule("shell", "ls"));
        assert!(!store.remove_rule("shell", "ls"));
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn approval_tracker_prompts_after_three() {
        let mut t = ApprovalTracker {
            counts: Default::default(),
        };
        assert_eq!(t.record("shell", "cargo test"), 1);
        assert!(!t.should_prompt_to_trust("shell", "cargo test"));
        t.record("shell", "cargo test");
        t.record("shell", "cargo test");
        assert!(t.should_prompt_to_trust("shell", "cargo test"));
        assert!(!t.should_prompt_to_trust("shell", "other"));
    }

    #[test]
    fn is_trusted_tool_wildcard_with_pattern() {
        let mut store = TrustStore::default();
        store.add_rule("*", "cargo check");
        assert!(store.is_trusted("shell", "cargo check --all"));
        assert!(store.is_trusted("write_file", "run cargo check"));
        assert!(!store.is_trusted("shell", "cargo test"));
    }

    #[test]
    fn is_trusted_empty_arg_and_empty_store() {
        let store = TrustStore::default();
        assert!(!store.is_trusted("shell", ""));
        let mut store = TrustStore::default();
        store.add_rule("shell", "*");
        assert!(store.is_trusted("shell", ""));
        assert!(!store.is_trusted("write_file", "x"));
    }

    #[test]
    fn load_from_path_missing_and_invalid() {
        let d = tempfile::tempdir().unwrap();
        let missing = d.path().join("nope.toml");
        assert!(TrustStore::load_from_path(&missing).rules.is_empty());

        let bad = d.path().join("bad.toml");
        std::fs::write(&bad, "not = [valid toml").unwrap();
        assert!(TrustStore::load_from_path(&bad).rules.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip_path_inject() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("nested").join("trust.toml");
        let mut store = TrustStore::default();
        assert!(store.add_rule("shell", "cargo test"));
        assert!(store.add_rule("write_file", "*"));
        store.save_to_path(&path).unwrap();
        assert!(path.is_file());

        let loaded = TrustStore::load_from_path(&path);
        assert_eq!(loaded.list().len(), 2);
        assert!(loaded.is_trusted("shell", "cargo test -p harness"));
        assert!(loaded.is_trusted("write_file", "anything"));
        assert!(!loaded.is_trusted("shell", "rm -rf"));
    }

    #[test]
    fn path_ends_with_trust_toml() {
        let p = TrustStore::path();
        assert!(p.ends_with(".harness/trust.toml") || p.ends_with(".harness\\trust.toml"));
    }
}
