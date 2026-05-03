//! Tool wiring, SSE connect, ambient shutdown helpers.

use crate::agent;
use crate::swarm;
use crate::trust;
use anyhow::{Context, Result};
use harness_browser::BrowserTool;
use harness_lsp::{
    detect_and_spawn, DiagnosticsTool, FindDefinitionTool, FindReferencesTool, RenameSymbolTool,
};
use harness_mcp;
use harness_provider_core::ArcProvider;
use harness_tools::tools::{
    ApplyPatchTool, ComputerUseTool, GhTool, GitTool, ListDirTool, PatchFileTool, ReadFileTool,
    SearchCodeTool, ShellConfig as ToolShellConfig, ShellTool, SpawnAgentTool, SpawnSwarmTool,
    SwarmEnqueueRunner, TestRunnerTool, WriteFileTool,
};
use harness_tools::{SandboxMode, ToolExecutor, ToolRegistry, WorkspaceRoot};
use std::path::PathBuf;
use std::sync::Arc;

pub async fn graceful_ambient_shutdown(
    ambient: Option<(tokio::sync::watch::Sender<()>, tokio::task::JoinHandle<()>)>,
) {
    if let Some((tx, handle)) = ambient {
        let _ = tx.send(());
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;
    }
}

pub fn tool_workspace(cfg: &crate::config::Config) -> Arc<WorkspaceRoot> {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mode = SandboxMode::from_config(cfg.tools.sandbox.as_deref());
    Arc::new(
        WorkspaceRoot::new(root, mode).expect("failed to resolve workspace root for tool sandbox"),
    )
}

/// Build the full tool executor: base tools + SpawnAgentTool + SpawnSwarmTool + MCP tools.
pub async fn build_tools(
    provider: ArcProvider,
    model: String,
    cfg: &crate::config::Config,
    browser_enabled: bool,
    browser_url: &str,
    memory_store: Option<harness_memory::MemoryStore>,
    embed_model: Option<String>,
) -> ToolExecutor {
    let browser_url_owned = browser_url.to_string();
    let cfg_clone = cfg.clone();
    let swarm_enqueue: SwarmEnqueueRunner = Arc::new({
        let provider = provider.clone();
        let model = model.clone();
        let cfg_clone = cfg_clone.clone();
        let memory_store = memory_store.clone();
        let embed_model = embed_model.clone();
        let browser_url_owned = browser_url_owned.clone();
        move |prompt: String, count: usize| {
            let provider = provider.clone();
            let model = model.clone();
            let cfg_clone = cfg_clone.clone();
            let memory_store = memory_store.clone();
            let embed_model = embed_model.clone();
            let browser_url_owned = browser_url_owned.clone();
            Box::pin(async move {
                let n = count.clamp(1, 32);
                let tools = build_tools_inner(
                    provider.clone(),
                    model.clone(),
                    &cfg_clone,
                    browser_enabled,
                    &browser_url_owned,
                    None,
                )
                .await;
                let mut ids = Vec::new();
                for i in 0..n {
                    let label = if n > 1 {
                        format!("{prompt} [swarm {}/{n}]", i + 1)
                    } else {
                        prompt.clone()
                    };
                    let id = swarm::register_task(&label)?;
                    ids.push(id.clone());
                    let p = provider.clone();
                    let t = tools.clone();
                    let mem = memory_store.clone();
                    let emb = embed_model.clone();
                    let sys = cfg_clone.agent.system_prompt.clone();
                    let m2 = model.clone();
                    swarm::spawn_task(id, move |_tid| {
                        let p = p.clone();
                        let t = t.clone();
                        let mem = mem.clone();
                        let emb = emb.clone();
                        let label = label.clone();
                        let m2 = m2.clone();
                        let sys = sys.clone();
                        async move {
                            use harness_memory::Session;
                            use harness_provider_core::Message;
                            let mut session = Session::new(&m2);
                            session.push(Message::user(&label));
                            agent::drive_agent(
                                &p,
                                &t,
                                mem.as_ref(),
                                emb.as_deref(),
                                &mut session,
                                sys.as_deref().unwrap_or(agent::DEFAULT_SYSTEM),
                                None,
                            )
                            .await?;
                            let reply = session
                                .messages
                                .iter()
                                .rev()
                                .find(|m| matches!(m.role, harness_provider_core::Role::Assistant))
                                .map(|m| m.content.as_str().to_string())
                                .unwrap_or_else(|| "(no response)".into());
                            Ok(reply)
                        }
                    })
                    .await;
                }
                Ok(format!(
                    "Queued swarm task(s): {} (parallel={n})",
                    ids.join(", ")
                ))
            })
        }
    });
    build_tools_inner(
        provider,
        model,
        cfg,
        browser_enabled,
        browser_url,
        Some(swarm_enqueue),
    )
    .await
}

pub async fn build_tools_inner(
    provider: ArcProvider,
    model: String,
    cfg: &crate::config::Config,
    browser_enabled: bool,
    browser_url: &str,
    swarm_enqueue: Option<SwarmEnqueueRunner>,
) -> ToolExecutor {
    let workspace = tool_workspace(cfg);

    let shell_cfg = ToolShellConfig {
        denylist: cfg.shell.effective_denylist(),
        confirm_required: cfg.shell.effective_confirm_required(),
        log_path: cfg
            .shell
            .log_path
            .clone()
            .or_else(|| dirs::home_dir().map(|h| h.join(".harness").join("shell.log"))),
        cmd_allowlist: cfg.shell.cmd_allowlist.clone(),
    };

    // Sub-agent runner: runs a prompt through a fresh session with base tools only.
    let sub_provider: ArcProvider = provider.clone();
    let sub_model = model.clone();
    let sub_shell_cfg = shell_cfg.clone();
    let sub_workspace = workspace.clone();
    let runner: harness_tools::tools::agent::SubAgentRunner = Arc::new(move |task: String| {
        let p: ArcProvider = sub_provider.clone();
        let m = sub_model.clone();
        let scfg = sub_shell_cfg.clone();
        let ws = sub_workspace.clone();
        let sub_tools = {
            let mut r = ToolRegistry::new();
            r.register(ReadFileTool {
                workspace: ws.clone(),
            });
            r.register(WriteFileTool {
                workspace: ws.clone(),
            });
            r.register(PatchFileTool {
                workspace: ws.clone(),
            });
            r.register(ListDirTool {
                workspace: ws.clone(),
            });
            r.register(ShellTool::new(scfg, ws));
            r.register(SearchCodeTool);
            ToolExecutor::new(r)
        };
        Box::pin(async move {
            use harness_memory::Session;
            use harness_provider_core::Message;
            let mut session = Session::new(&m);
            session.push(Message::user(&task));
            agent::drive_agent(
                &p,
                &sub_tools,
                None,
                None,
                &mut session,
                agent::DEFAULT_SYSTEM,
                None,
            )
            .await?;
            let reply = session
                .messages
                .iter()
                .rev()
                .find(|m| matches!(m.role, harness_provider_core::Role::Assistant))
                .map(|m| m.content.as_str().to_string())
                .unwrap_or_else(|| "(no response)".into());
            Ok(reply)
        })
    });

    let mut registry = ToolRegistry::new();
    registry.register(ReadFileTool {
        workspace: workspace.clone(),
    });
    registry.register(WriteFileTool {
        workspace: workspace.clone(),
    });
    registry.register(PatchFileTool {
        workspace: workspace.clone(),
    });
    registry.register(ApplyPatchTool {
        workspace: workspace.clone(),
    });
    registry.register(ListDirTool {
        workspace: workspace.clone(),
    });
    registry.register(ShellTool::new(shell_cfg, workspace.clone()));
    registry.register(SearchCodeTool);
    registry.register(GitTool);
    registry.register(GhTool);
    registry.register(TestRunnerTool);
    registry.register(SpawnAgentTool::new(runner));
    if let Some(enqueue) = swarm_enqueue {
        registry.register(SpawnSwarmTool::new(enqueue));
    }

    if browser_enabled {
        registry.register(BrowserTool::new(browser_url));
        tracing::info!(url = %browser_url, "browser tool enabled");
    }

    // Computer use: gated, only enable if explicitly configured
    if cfg.computer_use.is_enabled() {
        let model_lower = model.to_lowercase();
        if model_lower.contains("claude-opus-4-7")
            || model_lower.contains("claude-opus-4")
            || model_lower.contains("claude-sonnet-4")
        {
            registry.register(ComputerUseTool);
            tracing::warn!("⚠️  COMPUTER USE ENABLED — agent can control mouse/keyboard");
        } else {
            tracing::warn!("computer_use enabled in config but model {} does not support it (requires Claude 4.7+)", model);
        }
    }

    // Lazy LSP: only spawn if a supported project type is detected in the cwd.
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let has_supported_project = cwd.join("Cargo.toml").exists()
        || cwd.join("tsconfig.json").exists()
        || cwd.join("package.json").exists()
        || cwd.join("pyproject.toml").exists()
        || cwd.join("setup.py").exists()
        || cwd.join("go.mod").exists();

    if has_supported_project {
        if let Some(lsp) = detect_and_spawn(&cwd).await {
            registry.register(FindDefinitionTool {
                client: lsp.clone(),
            });
            registry.register(FindReferencesTool {
                client: lsp.clone(),
            });
            registry.register(RenameSymbolTool {
                client: lsp.clone(),
            });
            registry.register(DiagnosticsTool { client: lsp });
        }
    }

    // Load MCP tools.
    if let Some(mcp_path) = harness_mcp::find_config() {
        if let Err(e) =
            harness_mcp::load_mcp_tools(&mcp_path, &mut registry, Some(provider.clone())).await
        {
            tracing::warn!("MCP load failed: {e}");
        }
    }
    if let Some(mcp_path) = &cfg.mcp.config_path {
        if mcp_path.exists() {
            if let Err(e) =
                harness_mcp::load_mcp_tools(mcp_path, &mut registry, Some(provider.clone())).await
            {
                tracing::warn!("MCP config load failed: {e}");
            }
        }
    }

    let executor = ToolExecutor::new(registry);

    // Wire autotest if enabled in config.
    let executor = if cfg.autotest.enabled {
        executor.with_autotest(cfg.autotest.scope.clone())
    } else {
        executor
    };

    // Load trust rules.
    let trust_store = trust::TrustStore::load();
    let trusted_rules: Vec<(String, String)> = trust_store
        .list()
        .iter()
        .map(|r| (r.tool.clone(), r.pattern.clone()))
        .collect();

    if trusted_rules.is_empty() {
        executor
    } else {
        executor.with_trusted(trusted_rules)
    }
}

/// Minimal SSE client for `harness connect`: streams events from server to stdout.
pub async fn connect_to_server(
    base_url: &str,
    prompt: &str,
    session_id: Option<&str>,
) -> Result<()> {
    let client = reqwest::Client::new();
    let mut body = serde_json::json!({ "prompt": prompt });
    if let Some(id) = session_id {
        body["session_id"] = serde_json::Value::String(id.to_string());
    }

    let resp = client
        .post(format!("{base_url}/api/chat"))
        .json(&body)
        .send()
        .await
        .context("connecting to harness server")?;

    if !resp.status().is_success() {
        let msg = resp.text().await.context("reading error body")?;
        anyhow::bail!("server error: {msg}");
    }

    use futures::StreamExt;
    let mut byte_stream = resp.bytes_stream();
    let mut buf = String::new();

    while let Some(chunk) = byte_stream.next().await {
        let bytes: bytes::Bytes = chunk.context("reading SSE stream")?;
        buf.push_str(&String::from_utf8_lossy(&bytes));

        // Process complete SSE lines
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim_end_matches('\r').to_string();
            buf = buf[pos + 1..].to_string();

            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                    match event["type"].as_str() {
                        Some("text_chunk") => {
                            print!("{}", event["content"].as_str().unwrap_or(""));
                            use std::io::Write;
                            std::io::stdout().flush().ok();
                        }
                        Some("tool_start") => {
                            eprintln!("\n[→ {}]", event["name"].as_str().unwrap_or(""))
                        }
                        Some("tool_result") => {
                            eprintln!("[← {}]", event["name"].as_str().unwrap_or(""))
                        }
                        Some("done") => {
                            println!();
                            break;
                        }
                        Some("error") => {
                            eprintln!("error: {}", event["message"].as_str().unwrap_or("unknown"));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}
