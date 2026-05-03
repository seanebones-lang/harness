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
        let config_contents = r#"[provider]
# api_key = "sk-ant-..."   # or set ANTHROPIC_API_KEY env var
model = "claude-sonnet-4-6"
max_tokens = 8192
temperature = 0.7

[memory]
enabled = true
embed_model = "nomic-embed-text"

[agent]
system_prompt = """
You are a powerful coding assistant running in a terminal.

Available tools:
  read_file, write_file     — read or overwrite files
  patch_file                — surgical old→new text replacement (prefer this over write_file for edits)
  list_dir                  — list directory contents
  shell                     — run shell commands (build, test, git, etc.)
  search_code               — regex search across the codebase
  spawn_agent               — run a sub-agent with base tools for parallel tasks
  browser (when enabled)    — Chrome CDP: navigate, screenshot, click, fill forms
  MCP tools (when loaded)   — any tools registered via .harness/mcp.json

Guidelines:
  - Prefer patch_file over write_file for targeted edits.
  - Always run tests or build commands after changes to verify correctness.
  - Be concise. Prefer making changes over explaining them.
  - When editing multiple files, use spawn_agent for parallelism.
  - In plan mode (--plan flag), destructive calls pause for user approval.
"""
"#;
        std::fs::write(&global_cfg, config_contents)?;
        println!("Created global config at {}", global_cfg.display());
        println!("Edit it any time: {}", global_cfg.display());
        println!("Set ANTHROPIC_API_KEY (or XAI_API_KEY / OPENAI_API_KEY) before running harness.");
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
# Inherits from ~/.harness/config.toml — only override what you need.

[agent]
system_prompt = """
You are a coding assistant working in the {cwd_name} project.
You have access to tools to read/write files, run shell commands, patch files, and search code.
Prefer targeted edits with patch_file over rewriting whole files.
Always run tests after changes to confirm correctness.
"""
"#
            );
            std::fs::write(&project_cfg, project_contents)?;
            println!("Created project config at {}", project_cfg.display());

            let system_md = project_dir.join("SYSTEM.md");
            if !system_md.exists() || force {
                let md = format!(
                    "# Harness system prompt — {cwd_name}\n\nEdit this file to customize the agent's behavior for this project.\nThen copy the contents into `.harness/config.toml` under `[agent] system_prompt`.\n"
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
