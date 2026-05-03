//! `sessions`, `export`, `delete` CLI helpers.

use anyhow::Result;
use harness_memory::SessionStore;
use harness_provider_core::Role;
use std::fmt::Write as FmtWrite;

pub fn export_session(
    store: &SessionStore,
    id: &str,
    output: Option<&std::path::Path>,
) -> Result<()> {
    let session = store.find(id)?.ok_or_else(|| {
        anyhow::anyhow!(
            "session not found: '{}'. Use 'harness sessions' to list available sessions.",
            id
        )
    })?;

    let mut md = String::new();

    let title = session.name.as_deref().unwrap_or("Untitled session");
    writeln!(md, "# {title}")?;
    writeln!(md)?;
    writeln!(md, "**Session:** `{}`  ", session.id)?;
    writeln!(md, "**Model:** {}  ", session.model)?;
    writeln!(
        md,
        "**Created:** {}  ",
        session.created_at.format("%Y-%m-%d %H:%M UTC")
    )?;
    writeln!(md)?;
    writeln!(md, "---")?;
    writeln!(md)?;

    let mut turn = 0usize;
    for msg in &session.messages {
        match msg.role {
            Role::System => {
                writeln!(md, "> **System:** {}", msg.content.as_str())?;
                writeln!(md)?;
            }
            Role::User => {
                turn += 1;
                writeln!(md, "## Turn {turn} — User")?;
                writeln!(md)?;
                writeln!(md, "{}", msg.content.as_str())?;
                writeln!(md)?;
            }
            Role::Assistant => {
                let content = msg.content.as_str();
                if content.starts_with("__tool_calls__:") {
                    if let Some(json) = content.strip_prefix("__tool_calls__:") {
                        if let Ok(calls) = serde_json::from_str::<serde_json::Value>(json) {
                            if let Some(arr) = calls.as_array() {
                                for call in arr {
                                    let name = call["function"]["name"].as_str().unwrap_or("?");
                                    let args =
                                        call["function"]["arguments"].as_str().unwrap_or("{}");
                                    let pretty = serde_json::from_str::<serde_json::Value>(args)
                                        .map(|v| {
                                            serde_json::to_string_pretty(&v)
                                                .unwrap_or_else(|_| args.to_string())
                                        })
                                        .unwrap_or_else(|_| args.to_string());
                                    writeln!(md, "**→ `{name}`**")?;
                                    writeln!(md, "```json")?;
                                    writeln!(md, "{pretty}")?;
                                    writeln!(md, "```")?;
                                    writeln!(md)?;
                                }
                            }
                        }
                    }
                } else {
                    writeln!(md, "## Turn {turn} — Assistant")?;
                    writeln!(md)?;
                    writeln!(md, "{content}")?;
                    writeln!(md)?;
                }
            }
            Role::Tool => {
                let result = msg.content.as_str();
                let display = if result.len() > 2000 {
                    format!(
                        "{}\n\n_… ({} bytes truncated)_",
                        &result[..2000],
                        result.len() - 2000
                    )
                } else {
                    result.to_string()
                };
                writeln!(md, "**← tool result**")?;
                writeln!(md, "```")?;
                writeln!(md, "{display}")?;
                writeln!(md, "```")?;
                writeln!(md)?;
            }
        }
    }

    match output {
        Some(path) => {
            std::fs::write(path, &md)?;
            eprintln!("Exported {} turns to {}", turn, path.display());
        }
        None => print!("{md}"),
    }

    Ok(())
}

pub fn list_sessions(store: &SessionStore) -> Result<()> {
    let sessions = store.list(20)?;
    if sessions.is_empty() {
        println!("No sessions yet.");
        return Ok(());
    }
    println!("{:<10} {:<24} UPDATED", "ID", "NAME");
    for (id, name, updated) in sessions {
        let short = id.chars().take(8).collect::<String>();
        println!("{:<10} {:<24} {}", short, name.unwrap_or_default(), updated);
    }
    Ok(())
}

pub fn delete_session(store: &SessionStore, id: &str) -> Result<()> {
    if store.delete(id)? {
        println!("Deleted session: {id}");
    } else {
        println!("Session not found: {id}");
    }
    Ok(())
}
