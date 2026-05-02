//! Project-level persistent memory.
//!
//! Stores named facts in `.harness/memory/<topic>.md` files.
//! On startup, all files are loaded and appended to the system prompt.
//! `/remember <topic>: <fact>` adds a new entry.
//! `/forget <topic>` removes an entry.
//! `harness memorize <topic> <fact>` is the CLI equivalent.

use anyhow::Result;
use std::path::PathBuf;

/// Return the project-level memory directory, creating it if needed.
pub fn memory_dir() -> PathBuf {
    let base = std::env::current_dir()
        .unwrap_or_default()
        .join(".harness/memory");
    let _ = std::fs::create_dir_all(&base);
    base
}

/// Load all `*.md` files from `.harness/memory/` and return them as a single
/// concatenated string suitable for injecting into the system prompt.
pub fn load_all() -> String {
    let dir = memory_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return String::new();
    };

    let mut parts: Vec<String> = Vec::new();
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false))
        .collect();
    paths.sort();

    for path in &paths {
        let topic = path.file_stem().unwrap_or_default().to_string_lossy();
        if let Ok(content) = std::fs::read_to_string(path) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                parts.push(format!("### {topic}\n{trimmed}"));
            }
        }
    }

    if parts.is_empty() {
        return String::new();
    }

    format!("## Project Memory\n\n{}\n", parts.join("\n\n"))
}

/// Append a fact to `.harness/memory/<topic>.md`, creating it if necessary.
pub fn remember(topic: &str, fact: &str) -> Result<PathBuf> {
    let safe_topic = sanitize_filename(topic);
    let path = memory_dir().join(format!("{safe_topic}.md"));
    let entry = format!("- {}\n", fact.trim());
    let mut content = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };
    content.push_str(&entry);
    std::fs::write(&path, &content)?;
    Ok(path)
}

/// Remove the topic file entirely.
pub fn forget(topic: &str) -> Result<bool> {
    let safe_topic = sanitize_filename(topic);
    let path = memory_dir().join(format!("{safe_topic}.md"));
    if path.exists() {
        std::fs::remove_file(&path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// List all memory topics.
pub fn list_topics() -> Vec<String> {
    let dir = memory_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut topics: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
        .filter_map(|e| {
            e.path()
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
        })
        .collect();
    topics.sort();
    topics
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .to_lowercase()
}

/// Inject project memory into a system prompt string.
/// Returns the original prompt unchanged if no memory exists.
pub fn augment_system(system: &str) -> String {
    let mem = load_all();
    if mem.is_empty() {
        system.to_string()
    } else {
        format!("{system}\n\n{mem}")
    }
}
