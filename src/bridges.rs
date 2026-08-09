#![allow(dead_code)]
//! External app bridges — Obsidian, Apple Notes, Calendar, GitHub Projects.
//! CLI: `harness bridge …` when `[bridges.*]` enabled in config.

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
    let folder = cfg.folder.as_deref().unwrap_or("NextEleven Harness");
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

    // Escape for AppleScript (same helper as Calendar bridge).
    let escaped_title = escape_applescript(title);
    let escaped_content = escape_applescript(content).replace('\n', "\\n");

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

fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Query calendar events for a given day.
pub async fn calendar_query(cfg: &CalendarConfig, date: &str) -> Result<Vec<String>> {
    if !cfg.enabled {
        anyhow::bail!("Calendar bridge not enabled.");
    }

    let escaped_date = escape_applescript(date);
    let script = format!(
        r#"tell application "Calendar"
    set d to date "{escaped_date}"
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

    let calendar = escape_applescript(cfg.calendar.as_deref().unwrap_or("NextEleven Harness"));
    let escaped_title = escape_applescript(title);
    let escaped_start = escape_applescript(start);
    let escaped_end = escape_applescript(end);
    let script = format!(
        r#"tell application "Calendar"
    tell calendar "{calendar}"
        make new event with properties {{summary:"{escaped_title}", start date:(date "{escaped_start}"), end date:(date "{escaped_end}")}}
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

/// Build a parameterized GraphQL body for GitHub Project V2 item listing.
pub(crate) fn github_projects_graphql_body(owner: &str, project_number: u64) -> serde_json::Value {
    serde_json::json!({
        "query": "query($login: String!, $num: Int!) { user(login: $login) { projectV2(number: $num) { items(first: 20) { nodes { id content { ... on Issue { title number } ... on PullRequest { title number } } } } } } } }",
        "variables": {
            "login": owner,
            "num": project_number
        }
    })
}

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

    let body = github_projects_graphql_body(owner, project_number);
    let payload = serde_json::to_string(&body)?;

    let mut child = tokio::process::Command::new("gh")
        .args(["api", "graphql", "--input", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("spawning gh")?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(payload.as_bytes()).await?;
        stdin.shutdown().await.ok();
    }

    let out = child.wait_with_output().await?;
    if !out.status.success() {
        anyhow::bail!(
            "gh api graphql failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let val: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    let items = val["data"]["user"]["projectV2"]["items"]["nodes"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item["content"]["title"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::{escape_applescript, github_projects_graphql_body};

    #[test]
    fn escape_applescript_quotes_and_backslashes() {
        assert_eq!(escape_applescript(r#"say "hi""#), r#"say \"hi\""#);
        assert_eq!(escape_applescript(r"path\to"), r"path\\to");
    }

    #[test]
    fn notes_escape_uses_applescript_helper() {
        let title = escape_applescript(r#"Note "title""#);
        assert!(title.contains(r#"\"#));
        let body = escape_applescript("line1\nline2").replace('\n', "\\n");
        assert!(body.contains("\\n"));
    }

    #[test]
    fn github_projects_graphql_uses_variables() {
        let body = github_projects_graphql_body("acme-corp", 42);
        let vars = &body["variables"];
        assert_eq!(vars["login"], "acme-corp");
        assert_eq!(vars["num"], 42);
        let query = body["query"].as_str().expect("query");
        assert!(query.contains("$login"));
        assert!(!query.contains("acme-corp"));
    }

    #[test]
    fn escape_applescript_empty_and_plain() {
        assert_eq!(escape_applescript(""), "");
        assert_eq!(escape_applescript("plain"), "plain");
        // single backslash doubles
        assert_eq!(escape_applescript(r"\"), r"\\");
        // quote alone becomes escaped quote
        assert_eq!(escape_applescript(r#"""#), r#"\""#);
    }

    #[test]
    fn github_projects_graphql_zero_project_number() {
        let body = github_projects_graphql_body("o", 0);
        assert_eq!(body["variables"]["num"], 0);
        assert_eq!(body["variables"]["login"], "o");
    }
}
