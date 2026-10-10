//! `harness init` — global + optional project config scaffolding.

use anyhow::{Context, Result};

pub fn run_init(project: bool, force: bool) -> Result<()> {
    let global_dir = dirs::home_dir()
        .context("cannot determine home directory")?
        .join(".harness");
    std::fs::create_dir_all(&global_dir)?;
    let global_cfg = global_dir.join("config.toml");

    if global_cfg.exists() && !force {
        println!("Global config already exists at {}", global_cfg.display());
        println!("Run `harness init --force` to overwrite it.");
    } else {
        let config_contents = r#"# Choose providers and models with `harness setup` or `harness route set`.
# Harness does not choose a vendor, model, or fallback order for you.

[provider]
max_tokens = 8192
temperature = 0.7

[memory]
enabled = true
embed_model = "nomic-embed-text"

[agent]
# Uses the current built-in instructions; add system_prompt only for an explicit override.

"#;
        std::fs::write(&global_cfg, config_contents)?;
        println!("Created global config at {}", global_cfg.display());
        println!("Edit it any time: {}", global_cfg.display());
        println!("Run `harness setup` to choose the exact provider/model route.");
    }

    if project {
        let project_dir = std::env::current_dir()?.join(".harness");
        std::fs::create_dir_all(&project_dir)?;
        let project_cfg = project_dir.join("config.toml");

        if project_cfg.exists() && !force {
            println!("Project config already exists at {}", project_cfg.display());
            println!("Run `harness init --project --force` to overwrite it.");
        } else {
            let cwd_name = std::env::current_dir()?
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "this project".to_string());
            let project_contents = format!(
                r#"# Project-level config for {cwd_name}
# Project config is authoritative when present; copy any global route you want to keep.

[agent]
# Uses the current built-in instructions plus .harness/SYSTEM.md and project skills.

"#
            );
            std::fs::write(&project_cfg, project_contents)?;
            println!("Created project config at {}", project_cfg.display());

            let system_md = project_dir.join("SYSTEM.md");
            if !system_md.exists() || force {
                let md = format!(
                    "# NextEleven Harness system prompt — {cwd_name}\n\nEdit this file to customize the agent's behavior for this project.\nHarness loads this file automatically alongside the active brief and project skills.\n"
                );
                std::fs::write(&system_md, md)?;
                println!("Created system prompt template at {}", system_md.display());
            }
        }
    }

    println!();
    println!("All done. Start a session with:  harness");
    println!("Resume a session with:           harness --resume <id>");
    println!("Approve changes before writing:  harness --plan");
    Ok(())
}
