//! Slash-related helpers: `@file` expansion/completion and auto-detected test commands.

pub(crate) fn expand_at_files(prompt: &str) -> String {
    let mut result = String::new();
    let mut pinned = String::new();
    let mut text_parts = Vec::new();

    for part in prompt.split_whitespace() {
        if let Some(path) = part.strip_prefix('@') {
            match std::fs::read_to_string(path) {
                Ok(contents) => {
                    let ext = std::path::Path::new(path)
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

pub(crate) fn at_file_completions(partial: &str) -> Vec<String> {
    let dir = if let Some(slash) = partial.rfind('/') {
        partial[..=slash].to_string()
    } else {
        String::new()
    };
    let file_prefix = if let Some(slash) = partial.rfind('/') {
        partial[slash + 1..].to_string()
    } else {
        partial.to_string()
    };

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
    if std::path::Path::new("Cargo.toml").exists() {
        "cargo test 2>&1".into()
    } else if std::path::Path::new("package.json").exists() {
        "npm test 2>&1".into()
    } else if std::path::Path::new("pyproject.toml").exists()
        || std::path::Path::new("setup.py").exists()
    {
        "python -m pytest 2>&1".into()
    } else if std::path::Path::new("go.mod").exists() {
        "go test ./... 2>&1".into()
    } else {
        "make test 2>&1".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_test_command_prefers_cargo_in_rust_workspace() {
        // This workspace root has Cargo.toml
        assert!(
            detect_test_command().starts_with("cargo test"),
            "expected cargo test stub in harness repo root"
        );
    }
}
