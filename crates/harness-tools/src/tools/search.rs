use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use ignore::WalkBuilder;
use regex::Regex;
use serde_json::{json, Value};

use crate::registry::Tool;

pub struct SearchCodeTool;

#[async_trait]
impl Tool for SearchCodeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "search_code",
            "Search for a regex pattern across source files in a directory, respecting .gitignore.",
            json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern to search for." },
                    "path": { "type": "string", "description": "Directory to search. Defaults to current directory." },
                    "file_glob": { "type": "string", "description": "Optional glob to filter files, e.g. '*.rs'." },
                    "max_results": { "type": "integer", "description": "Max number of matches to return. Default 50." }
                },
                "required": ["pattern"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let pattern = args["pattern"].as_str().ok_or_else(|| anyhow::anyhow!("missing pattern"))?;
        let root = args["path"].as_str().unwrap_or(".");
        let file_glob = args["file_glob"].as_str();
        let max_results = args["max_results"].as_u64().unwrap_or(50) as usize;

        let re = Regex::new(pattern)?;
        let mut results: Vec<String> = Vec::new();

        let mut builder = WalkBuilder::new(root);
        builder.hidden(false).git_ignore(true);

        for entry in builder.build().flatten() {
            if results.len() >= max_results {
                break;
            }
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();

            if let Some(glob) = file_glob {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if !glob_match(glob, &name) {
                    continue;
                }
            }

            let Ok(content) = std::fs::read_to_string(path) else { continue };

            for (line_no, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    results.push(format!("{}:{}: {}", path.display(), line_no + 1, line.trim()));
                    if results.len() >= max_results {
                        break;
                    }
                }
            }
        }

        if results.is_empty() {
            Ok(format!("No matches for `{pattern}`"))
        } else {
            let count = results.len();
            let mut out = results.join("\n");
            if count >= max_results {
                out.push_str(&format!("\n... ({max_results} results, limit reached)"));
            }
            Ok(out)
        }
    }
}

fn glob_match(pattern: &str, name: &str) -> bool {
    if let Some(ext) = pattern.strip_prefix("*.") {
        name.ends_with(&format!(".{ext}"))
    } else {
        name == pattern
    }
}
