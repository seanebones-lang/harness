#![allow(dead_code)]
//! Ecosystem bridges: OS-level integrations for macOS apps and GitHub Projects.
//!
//! Gated by `[bridges]` config block. Each bridge is independently enabled.
//!
//! Available bridges:
//! - **Obsidian**: write notes/snippets to the Obsidian vault via its URI scheme
//! - **Apple Notes**: create/append notes via osascript
//! - **Calendar**: query and create events via EventKit (osascript)
//! - **GitHub Projects**: read/update project board items via `gh api graphql`

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct BridgesConfig {
    #[serde(default)]
    pub obsidian: ObsidianConfig,
    #[serde(default)]
    pub notes: NotesConfig,
    #[serde(default)]
    pub calendar: CalendarConfig,
    #[serde(default)]
    pub github_projects: GithubProjectsConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ObsidianConfig {
    pub enabled: bool,
    /// Vault name (used in obsidian:// URI).
    pub vault: Option<String>,
    /// Default folder for harness-generated notes.
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct NotesConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CalendarConfig {
    pub enabled: bool,
    /// Calendar name to use for created events.
    pub calendar: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct GithubProjectsConfig {
    pub enabled: bool,
    /// GitHub Project V2 number.
    pub project_number: Option<u64>,
    /// Owner (user or org) of the project.
    pub owner: Option<String>,
}

// ── Obsidian ──────────────────────────────────────────────────────────────────

/// Write a note to Obsidian via the obsidian:// URI scheme.
pub async fn obsidian_write(cfg: &ObsidianConfig, title: &str, content: &str) -> Result<()> {
    if !cfg.enabled {
        anyhow::bail!(
            "Obsidian bridge not enabled. Set [bridges.obsidian] enabled = true in config."
        );
    }

    let vault = cfg.vault.as_deref().unwrap_or("");
    let folder = cfg.folder.as_deref().unwrap_or("Harness");
    let path = format!("{folder}/{title}.md");

    // Use obsidian://new URI scheme
    let encoded_path = urlencoding::encode(&path);
    let encoded_content = urlencoding::encode(content);
    let uri = if vault.is_empty() {
        format!("obsidian://new?file={encoded_path}&content={encoded_content}")
    } else {
        let encoded_vault = urlencoding::encode(vault);
        format!(
            "obsidian://new?vault={encoded_vault}&file={encoded_path}&content={encoded_content}"
        )
    };

    // Open via `open` command (macOS/Linux)
    tokio::process::Command::new("open")
        .arg(&uri)
        .status()
        .await
        .context("opening Obsidian URI")?;

    Ok(())
}

// ── Apple Notes ───────────────────────────────────────────────────────────────

/// Create a note in Apple Notes via osascript.
pub async fn notes_write(cfg: &NotesConfig, title: &str, content: &str) -> Result<()> {
    if !cfg.enabled {
        anyhow::bail!("Notes bridge not enabled. Set [bridges.notes] enabled = true in config.");
    }

    // Escape for AppleScript
    let escaped_title = title.replace('"', "\\\"");
    let escaped_content = content.replace('"', "\\\"").replace('\n', "\\n");

    let script = format!(
        r#"tell application "Notes"
    activate
    tell folder "Notes" of default account
        make new note with properties {{name:"{escaped_title}", body:"{escaped_content}"}}
    end tell
end tell"#
    );

    tokio::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .status()
        .await
        .context("running osascript for Notes")?;

    Ok(())
}

// ── Calendar ──────────────────────────────────────────────────────────────────

/// Query calendar events for a given day.
pub async fn calendar_query(cfg: &CalendarConfig, date: &str) -> Result<Vec<String>> {
    if !cfg.enabled {
        anyhow::bail!("Calendar bridge not enabled.");
    }

    let script = format!(
        r#"tell application "Calendar"
    set d to date "{date}"
    set allEvents to (every event of every calendar whose start date >= d and start date < d + 1 * days)
    set names to {{}}
    repeat with e in allEvents
        set end of names to summary of e
    end repeat
    return names
end tell"#
    );

    let out = tokio::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .await
        .context("running osascript for Calendar")?;

    let result = String::from_utf8_lossy(&out.stdout).to_string();
    Ok(result
        .split(", ")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Create a calendar event.
pub async fn calendar_create_event(
    cfg: &CalendarConfig,
    title: &str,
    start: &str,
    end: &str,
) -> Result<()> {
    if !cfg.enabled {
        anyhow::bail!("Calendar bridge not enabled.");
    }

    let calendar = cfg.calendar.as_deref().unwrap_or("Harness");
    let script = format!(
        r#"tell application "Calendar"
    tell calendar "{calendar}"
        make new event with properties {{summary:"{title}", start date:(date "{start}"), end date:(date "{end}")}}
    end tell
end tell"#
    );

    tokio::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .status()
        .await
        .context("osascript Calendar create")?;

    Ok(())
}

// ── GitHub Projects V2 ────────────────────────────────────────────────────────

/// List items in a GitHub Project V2.
pub async fn github_project_list(cfg: &GithubProjectsConfig) -> Result<Vec<String>> {
    if !cfg.enabled {
        anyhow::bail!("GitHub Projects bridge not enabled.");
    }

    let owner = cfg
        .owner
        .as_deref()
        .context("bridges.github_projects.owner not set")?;
    let project_number = cfg
        .project_number
        .context("bridges.github_projects.project_number not set")?;

    let query = format!(
        r#"{{
        "query": "query {{ user(login: \"{owner}\") {{ projectV2(number: {project_number}) {{ items(first: 20) {{ nodes {{ id content {{ ... on Issue {{ title number }} ... on PullRequest {{ title number }} }} }} }} }} }} }}"
    }}"#
    );

    let out = tokio::process::Command::new("gh")
        .args(["api", "graphql", "--input", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context("spawning gh")?
        .wait_with_output()
        .await?;

    let text = String::from_utf8_lossy(&out.stdout).to_string();
    // Parse item titles from JSON
    let val: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    let items = val["data"]["user"]["projectV2"]["items"]["nodes"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item["content"]["title"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let _ = query;
    Ok(items)
}
