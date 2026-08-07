//! Slash-related helpers: `@file` expansion/completion and auto-detected test commands.

use std::path::{Path, PathBuf};

pub(crate) fn expand_at_files(prompt: &str) -> String {
    expand_at_files_in(prompt, Path::new("."))
}

/// Expand `@path` tokens relative to `root` (tests inject a tempdir).
pub(crate) fn expand_at_files_in(prompt: &str, root: &Path) -> String {
    let mut result = String::new();
    let mut pinned = String::new();
    let mut text_parts = Vec::new();

    for part in prompt.split_whitespace() {
        if let Some(path) = part.strip_prefix('@') {
            let full = resolve_at_path(root, path);
            match std::fs::read_to_string(&full) {
                Ok(contents) => {
                    let ext = full
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("");
                    pinned.push_str(&format!(
                        "<file path=\"{path}\">\n```{ext}\n{contents}\n```\n</file>\n"
                    ));
                }
                Err(e) => {
                    pinned.push_str(&format!("[could not read {path}: {e}]\n"));
                }
            }
        } else {
            text_parts.push(part);
        }
    }

    result.push_str(&text_parts.join(" "));
    if !pinned.is_empty() {
        result.push_str("\n\n");
        result.push_str(&pinned);
    }
    result
}

fn resolve_at_path(root: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

/// Split `@` completion partial into directory prefix (with trailing `/`) + file prefix.
pub(crate) fn at_completion_dir_and_prefix(partial: &str) -> (String, String) {
    if let Some(slash) = partial.rfind('/') {
        (partial[..=slash].to_string(), partial[slash + 1..].to_string())
    } else {
        (String::new(), partial.to_string())
    }
}

pub(crate) fn at_file_completions(partial: &str) -> Vec<String> {
    let (dir, file_prefix) = at_completion_dir_and_prefix(partial);

    let search_dir = if dir.is_empty() {
        ".".to_string()
    } else {
        dir.clone()
    };
    let Ok(entries) = std::fs::read_dir(&search_dir) else {
        return vec![];
    };

    let mut results: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&file_prefix) {
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let full = format!("{}{}{}", dir, name, if is_dir { "/" } else { "" });
                Some(full)
            } else {
                None
            }
        })
        .collect();
    results.sort();
    results.truncate(20);
    results
}

pub(crate) fn detect_test_command() -> String {
    detect_test_command_in(std::path::Path::new("."))
}

/// Detect a project test command for files under `root` (does not depend on process cwd).
pub(crate) fn detect_test_command_in(root: &std::path::Path) -> String {
    if root.join("Cargo.toml").exists() {
        "cargo test 2>&1".into()
    } else if root.join("package.json").exists() {
        "npm test 2>&1".into()
    } else if root.join("pyproject.toml").exists() || root.join("setup.py").exists() {
        "python -m pytest 2>&1".into()
    } else if root.join("go.mod").exists() {
        "go test ./... 2>&1".into()
    } else {
        "make test 2>&1".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detect_test_command_prefers_cargo_in_rust_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"t\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("write Cargo.toml");
        assert!(
            detect_test_command_in(dir.path()).starts_with("cargo test"),
            "expected cargo test stub when Cargo.toml present"
        );
    }

    #[test]
    fn detect_test_command_prefers_npm_when_package_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("package.json"), "{}\n").expect("write package.json");
        assert!(detect_test_command_in(dir.path()).starts_with("npm test"));
    }

    #[test]
    fn detect_test_command_pytest_and_go() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("pyproject.toml"), "[project]\nname=\"t\"\n").unwrap();
        assert!(detect_test_command_in(dir.path()).contains("pytest"));

        let dir2 = tempfile::tempdir().expect("tempdir");
        fs::write(dir2.path().join("go.mod"), "module x\n").unwrap();
        assert!(detect_test_command_in(dir2.path()).starts_with("go test"));
    }

    #[test]
    fn detect_test_command_falls_back_to_make() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(detect_test_command_in(dir.path()).starts_with("make test"));
    }

    #[test]
    fn at_completion_dir_and_prefix_splits() {
        assert_eq!(
            at_completion_dir_and_prefix("src/mai"),
            ("src/".into(), "mai".into())
        );
        assert_eq!(
            at_completion_dir_and_prefix("readme"),
            ("".into(), "readme".into())
        );
        assert_eq!(
            at_completion_dir_and_prefix("a/b/c"),
            ("a/b/".into(), "c".into())
        );
    }

    #[test]
    fn expand_at_files_in_pins_contents_and_keeps_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("note.rs"), "fn main() {}\n").unwrap();
        let out = expand_at_files_in("please review @note.rs carefully", dir.path());
        assert!(out.starts_with("please review carefully"));
        assert!(out.contains("<file path=\"note.rs\">"));
        assert!(out.contains("```rs"));
        assert!(out.contains("fn main() {}"));
    }

    #[test]
    fn expand_at_files_in_missing_path_notes_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = expand_at_files_in("see @missing.txt", dir.path());
        assert!(out.contains("[could not read missing.txt:"));
    }

    #[test]
    fn at_file_completions_lists_prefix_matches() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("alpha.txt"), "a").unwrap();
        fs::write(dir.path().join("alpine.md"), "b").unwrap();
        fs::write(dir.path().join("beta.txt"), "c").unwrap();
        fs::create_dir(dir.path().join("alga")).unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let hits = at_file_completions("al");
        std::env::set_current_dir(prev).unwrap();
        assert!(hits.iter().any(|h| h == "alpha.txt"));
        assert!(hits.iter().any(|h| h == "alpine.md"));
        assert!(hits.iter().any(|h| h == "alga/"));
        assert!(!hits.iter().any(|h| h == "beta.txt"));
    }
}
