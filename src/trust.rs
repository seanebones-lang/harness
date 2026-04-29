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

    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let toml = toml::to_string_pretty(self)?;
        std::fs::write(&path, toml)?;
        Ok(())
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
        if self.rules.iter().any(|r| r.tool == tool && r.pattern == pattern) {
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
        self.rules.retain(|r| !(r.tool == tool && r.pattern == pattern));
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
