//! Project-level persistent memory.
//!
//! Stores named facts in `.harness/memory/<topic>.md` files.
//! On startup, all files are loaded and appended to the system prompt.
//! `/remember <topic>: <fact>` adds a new entry.
//! `/forget <topic>` removes an entry.
//! `harness memorize <topic> <fact>` is the CLI equivalent.

use anyhow::Result;
use std::path::{Path, PathBuf};

/// Return the project-level memory directory, creating it if needed.
pub fn memory_dir() -> PathBuf {
    let base = std::env::current_dir()
        .unwrap_or_default()
        .join(".harness/memory");
    let _ = std::fs::create_dir_all(&base);
    base
}

/// Sanitize a topic into a safe lowercase filename stem.
pub fn sanitize_filename(name: &str) -> String {
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

/// Build the system-prompt memory section from topic/content pairs (pure).
pub fn format_memory_section(parts: &[(String, String)]) -> String {
    if parts.is_empty() {
        return String::new();
    }
    let body = parts
        .iter()
        .map(|(topic, content)| format!("### {topic}\n{content}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("## Project Memory\n\n{body}\n")
}

/// Load all `*.md` files under `dir` into a system-prompt section.
pub fn load_all_in(dir: &Path) -> String {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return String::new();
    };

    let mut parts: Vec<(String, String)> = Vec::new();
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false))
        .collect();
    paths.sort();

    for path in &paths {
        let topic = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if let Ok(content) = std::fs::read_to_string(path) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                parts.push((topic, trimmed.to_string()));
            }
        }
    }

    format_memory_section(&parts)
}

/// Load all `*.md` files from `.harness/memory/` and return them as a single
/// concatenated string suitable for injecting into the system prompt.
pub fn load_all() -> String {
    load_all_in(&memory_dir())
}

/// Append a fact under `dir/<topic>.md`.
pub fn remember_in(dir: &Path, topic: &str, fact: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let safe_topic = sanitize_filename(topic);
    let path = dir.join(format!("{safe_topic}.md"));
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

/// Append a fact to `.harness/memory/<topic>.md`, creating it if necessary.
pub fn remember(topic: &str, fact: &str) -> Result<PathBuf> {
    remember_in(&memory_dir(), topic, fact)
}

/// Remove the topic file under `dir`.
pub fn forget_in(dir: &Path, topic: &str) -> Result<bool> {
    let safe_topic = sanitize_filename(topic);
    let path = dir.join(format!("{safe_topic}.md"));
    if path.exists() {
        std::fs::remove_file(&path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Remove the topic file entirely.
pub fn forget(topic: &str) -> Result<bool> {
    forget_in(&memory_dir(), topic)
}

/// List memory topics under `dir`.
pub fn list_topics_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
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

/// List all memory topics.
pub fn list_topics() -> Vec<String> {
    list_topics_in(&memory_dir())
}

/// Inject project memory into a system prompt string.
/// Returns the original prompt unchanged if no memory exists.
pub fn augment_system(system: &str) -> String {
    augment_system_with(system, &load_all())
}

/// Pure: append memory section when non-empty.
pub fn augment_system_with(system: &str, mem: &str) -> String {
    if mem.is_empty() {
        system.to_string()
    } else {
        format!("{system}\n\n{mem}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn sanitize_filename_lowercases_and_replaces() {
        assert_eq!(sanitize_filename("Arch/Decision"), "arch_decision");
        assert_eq!(sanitize_filename("OK-Topic_1"), "ok-topic_1");
        assert_eq!(sanitize_filename("  spaced  "), "__spaced__");
    }

    #[test]
    fn format_memory_section_empty_and_joined() {
        assert_eq!(format_memory_section(&[]), "");
        let s =
            format_memory_section(&[("a".into(), "- one".into()), ("b".into(), "- two".into())]);
        assert!(s.starts_with("## Project Memory\n"));
        assert!(s.contains("### a\n- one"));
        assert!(s.contains("### b\n- two"));
    }

    #[test]
    fn remember_forget_list_roundtrip_in_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let p = remember_in(dir.path(), "API Keys", "use vault").unwrap();
        assert!(p.ends_with("api_keys.md"));
        remember_in(dir.path(), "API Keys", "rotate quarterly").unwrap();
        let topics = list_topics_in(dir.path());
        assert_eq!(topics, vec!["api_keys".to_string()]);
        let all = load_all_in(dir.path());
        assert!(all.contains("use vault"));
        assert!(all.contains("rotate quarterly"));
        assert!(forget_in(dir.path(), "API Keys").unwrap());
        assert!(!forget_in(dir.path(), "API Keys").unwrap());
        assert!(list_topics_in(dir.path()).is_empty());
        assert_eq!(load_all_in(dir.path()), "");
    }

    #[test]
    fn load_all_in_skips_empty_and_non_md() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("empty.md"), "  \n").unwrap();
        fs::write(dir.path().join("note.txt"), "nope").unwrap();
        fs::write(dir.path().join("real.md"), "- fact\n").unwrap();
        let all = load_all_in(dir.path());
        assert!(all.contains("### real"));
        assert!(all.contains("- fact"));
        assert!(!all.contains("note"));
    }

    #[test]
    fn augment_system_with_memory() {
        assert_eq!(augment_system_with("SYS", ""), "SYS");
        let out = augment_system_with("SYS", "## Project Memory\n\nx\n");
        assert!(out.starts_with("SYS\n\n## Project Memory"));
    }
}
