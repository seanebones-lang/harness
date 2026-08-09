//! Tool wiring, SSE connect, ambient shutdown helpers.

use crate::agent;
use crate::swarm;
use crate::trust;
use anyhow::{Context, Result};
use harness_browser::BrowserTool;
use harness_lsp::{
    DiagnosticsTool, FindDefinitionTool, FindReferencesTool, LazyLspClient, RenameSymbolTool,
};
use harness_mcp;
use harness_provider_core::ArcProvider;
use harness_tools::tools::{
    ApplyPatchTool, ComputerUseTool, DatabaseTool, DatabaseToolConfig, DockerTool,
    DockerToolConfig, GhTool, GitTool, ListDirTool, NotebookTool, PatchFileTool, ReadFileTool,
    SearchCodeTool, ShellConfig as ToolShellConfig, ShellTool, SpawnAgentTool, SpawnSwarmTool,
    SwarmEnqueueRunner, TestRunnerTool, WriteFileTool,
};
use harness_tools::{ConfirmGate, SandboxMode, ToolExecutor, ToolRegistry, WorkspaceRoot};
use std::collections::HashSet;
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

/// Map plan-mode gate + approval mode string → executor confirm policy.
pub(crate) fn confirm_policy_for_gate(
    has_gate: bool,
    approval_mode: &str,
) -> harness_tools::ConfirmPolicy {
    if !has_gate {
        harness_tools::ConfirmPolicy::Off
    } else if approval_mode == "smart" {
        harness_tools::ConfirmPolicy::Smart
    } else {
        harness_tools::ConfirmPolicy::Plan
    }
}

/// Models allowed to register computer-use when config enables it.
pub(crate) fn computer_use_model_supported(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("claude-opus-4-7") || m.contains("claude-opus-4") || m.contains("claude-sonnet-4")
}

/// LSP tools only when cwd looks like a supported project tree.
pub(crate) fn has_supported_lsp_project(root: &std::path::Path) -> bool {
    root.join("Cargo.toml").exists()
        || root.join("tsconfig.json").exists()
        || root.join("package.json").exists()
        || root.join("pyproject.toml").exists()
        || root.join("setup.py").exists()
        || root.join("go.mod").exists()
}

/// Label for a swarm worker registration (`prompt [swarm i/n]` when n>1).
pub(crate) fn format_swarm_worker_label(prompt: &str, index_1based: usize, n: usize) -> String {
    if n > 1 {
        format!("{prompt} [swarm {index_1based}/{n}]")
    } else {
        prompt.to_string()
    }
}

/// Tool names present after MCP load that were not in the builtin set.
pub(crate) fn mcp_names_added(
    before: &HashSet<String>,
    after_names: impl IntoIterator<Item = String>,
) -> HashSet<String> {
    after_names
        .into_iter()
        .filter(|n| !before.contains(n))
        .collect()
}

/// JSON body for `POST /api/chat` used by `connect_to_server`.
pub(crate) fn connect_chat_json_body(prompt: &str, session_id: Option<&str>) -> serde_json::Value {
    let mut body = serde_json::json!({ "prompt": prompt });
    if let Some(id) = session_id {
        body["session_id"] = serde_json::Value::String(id.to_string());
    }
    body
}

/// One SSE `data:` line → display action for the connect client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SseConnectAction {
    Text(String),
    ToolStart(String),
    ToolResult(String),
    Done,
    Error(String),
    Ignore,
}

pub(crate) fn parse_sse_connect_line(line: &str) -> SseConnectAction {
    let Some(data) = line.strip_prefix("data: ") else {
        return SseConnectAction::Ignore;
    };
    let Ok(event) = serde_json::from_str::<serde_json::Value>(data) else {
        return SseConnectAction::Ignore;
    };
    match event.get("type").and_then(|t| t.as_str()) {
        Some("text_chunk") => SseConnectAction::Text(
            event
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string(),
        ),
        Some("tool_start") => SseConnectAction::ToolStart(
            event
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string(),
        ),
        Some("tool_result") => SseConnectAction::ToolResult(
            event
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string(),
        ),
        Some("done") => SseConnectAction::Done,
        Some("error") => SseConnectAction::Error(
            event
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
                .to_string(),
        ),
        _ => SseConnectAction::Ignore,
    }
}

pub fn tool_workspace(cfg: &crate::config::Config) -> Result<Arc<WorkspaceRoot>> {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mode = SandboxMode::from_config(cfg.tools.sandbox.as_deref());
    Ok(Arc::new(WorkspaceRoot::new(root, mode).context(
        "failed to resolve workspace root for tool sandbox",
    )?))
}

/// Build the full tool executor: base tools + SpawnAgentTool + SpawnSwarmTool + MCP tools.
#[allow(clippy::too_many_arguments)]
pub async fn build_tools(
    provider: ArcProvider,
    model: String,
    cfg: &crate::config::Config,
    browser_enabled: bool,
    browser_url: &str,
    memory_store: Option<harness_memory::MemoryStore>,
    embed_model: Option<String>,
    confirm_gate: Option<ConfirmGate>,
    sampling_tx: Option<tokio::sync::mpsc::UnboundedSender<harness_mcp::SamplingApprovalRequest>>,
) -> Result<ToolExecutor> {
    let browser_url_owned = browser_url.to_string();
    let cfg_clone = cfg.clone();
    let swarm_enqueue: SwarmEnqueueRunner = Arc::new({
        let cfg_clone = cfg_clone.clone();
        let memory_store = memory_store.clone();
        let embed_model = embed_model.clone();
        let browser_url_owned = browser_url_owned.clone();
        let orch_model = model.clone();
        move |prompt: String, count: usize| {
            let cfg_clone = cfg_clone.clone();
            let memory_store = memory_store.clone();
            let embed_model = embed_model.clone();
            let browser_url_owned = browser_url_owned.clone();
            let orch_model = orch_model.clone();
            Box::pin(async move {
                let n = count.clamp(1, 32);
                let worker_spec = cfg_clone.swarm.effective_worker_model(&orch_model);
                let (worker_provider, worker_model) =
                    crate::provider_build::build_worker_provider(&cfg_clone, &worker_spec)?;
                let tools = build_tools_inner(
                    worker_provider.clone(),
                    worker_model.clone(),
                    &cfg_clone,
                    browser_enabled,
                    &browser_url_owned,
                    None,
                    None,
                    None, // sub-agents: no interactive sampling UI
                )
                .await?;
                let tools = if let Some(allow) = cfg_clone.swarm.effective_worker_allowlist() {
                    tools.with_tool_allowlist(&allow)
                } else {
                    tools
                };
                let wall = cfg_clone.swarm.worker_wall_timeout();
                let mut ids = Vec::new();
                for i in 0..n {
                    let label = format_swarm_worker_label(&prompt, i + 1, n);
                    let id = swarm::register_task_with_model(&label, Some(worker_model.as_str()))?;
                    ids.push(id.clone());
                    let p = worker_provider.clone();
                    let t = tools.clone();
                    let mem = memory_store.clone();
                    let emb = embed_model.clone();
                    let sys = cfg_clone.agent.system_prompt.clone();
                    let m2 = worker_model.clone();
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
                            let work = async {
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
                                    .find(|m| {
                                        matches!(m.role, harness_provider_core::Role::Assistant)
                                    })
                                    .map(|m| m.content.as_str().to_string())
                                    .unwrap_or_else(|| "(no response)".into());
                                Ok::<String, anyhow::Error>(reply)
                            };
                            match wall {
                                Some(d) => match tokio::time::timeout(d, work).await {
                                    Ok(r) => r,
                                    Err(_) => Err(anyhow::anyhow!(
                                        "swarm worker exceeded wall timeout ({d:?})"
                                    )),
                                },
                                None => work.await,
                            }
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
        confirm_gate,
        sampling_tx,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn build_tools_inner(
    provider: ArcProvider,
    model: String,
    cfg: &crate::config::Config,
    browser_enabled: bool,
    browser_url: &str,
    swarm_enqueue: Option<SwarmEnqueueRunner>,
    confirm_gate: Option<ConfirmGate>,
    sampling_tx: Option<tokio::sync::mpsc::UnboundedSender<harness_mcp::SamplingApprovalRequest>>,
) -> Result<ToolExecutor> {
    let workspace = tool_workspace(cfg)?;

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

    // Sub-agent runner: slaves use [swarm].worker_model when set (else orchestrator).
    let worker_spec = cfg.swarm.effective_worker_model(&model);
    let (sub_provider, sub_model) =
        match crate::provider_build::build_worker_provider(cfg, &worker_spec) {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    worker = %worker_spec,
                    "worker provider build failed — spawn_agent falls back to orchestrator"
                );
                (provider.clone(), model.clone())
            }
        };
    let sub_shell_cfg = shell_cfg.clone();
    let sub_workspace = workspace.clone();
    let sub_confirm = confirm_gate.clone();
    let sub_confirm_policy =
        confirm_policy_for_gate(confirm_gate.is_some(), cfg.approval.effective_mode());
    let sub_notifications = cfg.notifications.clone();
    let runner: harness_tools::tools::agent::SubAgentRunner = Arc::new(move |task: String| {
        let p: ArcProvider = sub_provider.clone();
        let m = sub_model.clone();
        let scfg = sub_shell_cfg.clone();
        let ws = sub_workspace.clone();
        let gate = sub_confirm.clone();
        let notif = sub_notifications.clone();
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
            r.register(ShellTool::new(scfg, ws.clone()));
            r.register(SearchCodeTool {
                workspace: ws.clone(),
            });
            let mut exec = ToolExecutor::new(r);
            if let Some(g) = gate {
                exec = exec
                    .with_confirm_gate(g)
                    .with_confirm_policy(sub_confirm_policy);
            }
            exec
        };
        Box::pin(async move {
            use harness_memory::Session;
            use harness_provider_core::Message;
            let mut session = Session::new(&m);
            session.push(Message::user(&task));
            let drive_result = agent::drive_agent(
                &p,
                &sub_tools,
                None,
                None,
                &mut session,
                agent::DEFAULT_SYSTEM,
                None,
            )
            .await;
            let reply = session
                .messages
                .iter()
                .rev()
                .find(|m| matches!(m.role, harness_provider_core::Role::Assistant))
                .map(|m| m.content.as_str().to_string())
                .unwrap_or_else(|| "(no response)".into());
            let preview: String = reply.chars().take(160).collect();
            match &drive_result {
                Ok(()) => {
                    crate::notifications::subagent_done(&notif, "spawn_agent", &preview);
                }
                Err(e) => {
                    crate::notifications::subagent_done(
                        &notif,
                        "spawn_agent",
                        &format!("failed: {e}"),
                    );
                }
            }
            drive_result?;
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
    registry.register(SearchCodeTool {
        workspace: workspace.clone(),
    });
    registry.register(GitTool {
        workspace: workspace.clone(),
    });
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
        if computer_use_model_supported(&model) {
            registry.register(ComputerUseTool);
            tracing::warn!("⚠️  COMPUTER USE ENABLED — agent can control mouse/keyboard");
        } else {
            tracing::warn!("computer_use enabled in config but model {} does not support it (requires Claude 4.7+)", model);
        }
    }

    // Optional tools (off by default — see [tools.database|notebook|docker] in config).
    if cfg.tools.database.is_enabled() {
        registry.register(DatabaseTool::new(
            workspace.clone(),
            DatabaseToolConfig {
                readonly: cfg.tools.database.is_readonly(),
                max_rows: cfg.tools.database.effective_max_rows(),
            },
        ));
        tracing::info!(
            readonly = cfg.tools.database.is_readonly(),
            max_rows = cfg.tools.database.effective_max_rows(),
            "database tool enabled"
        );
    }
    if cfg.tools.notebook.is_enabled() {
        registry.register(NotebookTool {
            workspace: workspace.clone(),
        });
        tracing::info!("notebook tool enabled");
    }
    if cfg.tools.docker.is_enabled() {
        registry.register(DockerTool::new(DockerToolConfig {
            allow_mutating: cfg.tools.docker.allow_mutating(),
            timeout_secs: cfg.tools.docker.effective_timeout_secs(),
            docker_bin: std::path::PathBuf::from("docker"),
        }));
        tracing::info!(
            allow_mutating = cfg.tools.docker.allow_mutating(),
            "docker tool enabled"
        );
    }

    // Lazy LSP: only spawn if a supported project type is detected in the cwd.
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let has_supported_project = has_supported_lsp_project(&cwd);

    if has_supported_project {
        let lsp = LazyLspClient::new(cwd);
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

    // Load MCP tools.
    let mcp_allowlist = cfg.mcp.command_allowlist.as_deref();
    let mcp_sampling_auto = cfg.approval.effective_mode() == "auto";
    let builtin_before: HashSet<String> = registry.names().into_iter().collect();
    if let Some(mcp_path) = harness_mcp::find_config() {
        if let Err(e) = harness_mcp::load_mcp_tools_with_progress(
            &mcp_path,
            &mut registry,
            None,
            Some(provider.clone()),
            mcp_allowlist,
            mcp_sampling_auto,
            sampling_tx.clone(),
        )
        .await
        {
            tracing::warn!("MCP load failed: {e}");
        }
    }
    if let Some(mcp_path) = &cfg.mcp.config_path {
        if mcp_path.exists() {
            if let Err(e) = harness_mcp::load_mcp_tools_with_progress(
                mcp_path,
                &mut registry,
                None,
                Some(provider.clone()),
                mcp_allowlist,
                mcp_sampling_auto,
                sampling_tx.clone(),
            )
            .await
            {
                tracing::warn!("MCP config load failed: {e}");
            }
        }
    }
    let mcp_tool_names: HashSet<String> = mcp_names_added(&builtin_before, registry.names());

    let confirm_policy =
        confirm_policy_for_gate(confirm_gate.is_some(), cfg.approval.effective_mode());

    let executor = ToolExecutor::new(registry);
    let executor = if let Some(gate) = confirm_gate {
        executor.with_confirm_gate(gate)
    } else {
        executor
    };

    let executor = executor
        .with_mcp_tool_names(mcp_tool_names)
        .with_always_ask(cfg.approval.parsed_always_ask())
        .with_auto_approve(cfg.approval.auto_approve.clone().unwrap_or_default())
        .with_shell_confirm_patterns(cfg.shell.effective_confirm_required())
        .with_confirm_policy(confirm_policy);

    // Wire autotest if enabled in config.
    let mut executor = if cfg.autotest.enabled {
        executor.with_autotest(cfg.autotest.scope.clone())
    } else {
        executor
    };

    if cfg.autotest.enabled && cfg.notifications.on_autotest_fail {
        let notif = cfg.notifications.clone();
        executor = executor.with_autotest_fail_hook(Arc::new(move |report| {
            crate::notifications::autotest_failed(&notif, report);
        }));
    }

    if cfg.notifications.enabled {
        let notif = cfg.notifications.clone();
        executor = executor.with_gh_pr_opened_hook(Arc::new(move |title, url| {
            crate::notifications::pr_opened(&notif, title, url);
        }));
    }

    // Load trust rules.
    let trust_store = trust::TrustStore::load();
    let trusted_rules: Vec<(String, String)> = trust_store
        .list()
        .iter()
        .map(|r| (r.tool.clone(), r.pattern.clone()))
        .collect();

    if trusted_rules.is_empty() {
        Ok(executor)
    } else {
        Ok(executor.with_trusted(trusted_rules))
    }
}

/// Minimal SSE client for `harness connect`: streams events from server to stdout.
pub async fn connect_to_server(
    base_url: &str,
    prompt: &str,
    session_id: Option<&str>,
) -> Result<()> {
    let token = crate::auth_token::read_token_file("server.token")
        .or_else(|_| std::env::var("HARNESS_SERVER_TOKEN").map_err(anyhow::Error::msg))?;
    let client = reqwest::Client::new();
    let body = connect_chat_json_body(prompt, session_id);

    let resp = client
        .post(format!("{base_url}/api/chat"))
        .header("Authorization", format!("Bearer {token}"))
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

            match parse_sse_connect_line(&line) {
                SseConnectAction::Text(content) => {
                    print!("{content}");
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }
                SseConnectAction::ToolStart(name) => {
                    eprintln!("\n[→ {name}]");
                }
                SseConnectAction::ToolResult(name) => {
                    eprintln!("[← {name}]");
                }
                SseConnectAction::Done => {
                    println!();
                    break;
                }
                SseConnectAction::Error(msg) => {
                    eprintln!("error: {msg}");
                }
                SseConnectAction::Ignore => {}
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_tools::ConfirmPolicy;
    use std::collections::HashSet;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn confirm_policy_for_gate_matrix() {
        assert_eq!(confirm_policy_for_gate(false, "smart"), ConfirmPolicy::Off);
        assert_eq!(confirm_policy_for_gate(true, "smart"), ConfirmPolicy::Smart);
        assert_eq!(confirm_policy_for_gate(true, "plan"), ConfirmPolicy::Plan);
        assert_eq!(confirm_policy_for_gate(true, "auto"), ConfirmPolicy::Plan);
        assert_eq!(confirm_policy_for_gate(true, ""), ConfirmPolicy::Plan);
    }

    #[test]
    fn computer_use_model_gate() {
        assert!(computer_use_model_supported("claude-sonnet-4-6"));
        assert!(computer_use_model_supported("Claude-Opus-4-7"));
        assert!(computer_use_model_supported("claude-opus-4"));
        assert!(!computer_use_model_supported("grok-4.5"));
        assert!(!computer_use_model_supported("gpt-5.5"));
        assert!(!computer_use_model_supported("claude-3-5-sonnet"));
    }

    #[test]
    fn lsp_project_markers() {
        let d = tempdir().unwrap();
        assert!(!has_supported_lsp_project(d.path()));
        fs::write(d.path().join("go.mod"), "module x\n").unwrap();
        assert!(has_supported_lsp_project(d.path()));

        let d2 = tempdir().unwrap();
        fs::write(d2.path().join("package.json"), "{}").unwrap();
        assert!(has_supported_lsp_project(d2.path()));
    }

    #[test]
    fn swarm_worker_label_formats() {
        assert_eq!(format_swarm_worker_label("fix foo", 1, 1), "fix foo");
        assert_eq!(
            format_swarm_worker_label("fix foo", 2, 3),
            "fix foo [swarm 2/3]"
        );
    }

    #[test]
    fn mcp_names_added_diff() {
        let before: HashSet<_> = ["read_file", "shell"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let after = vec![
            "read_file".into(),
            "shell".into(),
            "mcp_weather".into(),
            "mcp_docs".into(),
        ];
        let added = mcp_names_added(&before, after);
        assert_eq!(added.len(), 2);
        assert!(added.contains("mcp_weather"));
        assert!(added.contains("mcp_docs"));
    }

    #[test]
    fn connect_chat_json_body_optional_session() {
        let b = connect_chat_json_body("hi", None);
        assert_eq!(b["prompt"], "hi");
        assert!(b.get("session_id").is_none());
        let b2 = connect_chat_json_body("hi", Some("abc"));
        assert_eq!(b2["session_id"], "abc");
    }

    #[test]
    fn parse_sse_connect_line_variants() {
        assert_eq!(
            parse_sse_connect_line(r#"data: {"type":"text_chunk","content":"yo"}"#),
            SseConnectAction::Text("yo".into())
        );
        assert_eq!(
            parse_sse_connect_line(r#"data: {"type":"tool_start","name":"shell"}"#),
            SseConnectAction::ToolStart("shell".into())
        );
        assert_eq!(
            parse_sse_connect_line(r#"data: {"type":"tool_result","name":"read_file"}"#),
            SseConnectAction::ToolResult("read_file".into())
        );
        assert_eq!(
            parse_sse_connect_line(r#"data: {"type":"done"}"#),
            SseConnectAction::Done
        );
        assert_eq!(
            parse_sse_connect_line(r#"data: {"type":"error","message":"boom"}"#),
            SseConnectAction::Error("boom".into())
        );
        assert_eq!(
            parse_sse_connect_line(r#"data: {"type":"error"}"#),
            SseConnectAction::Error("unknown".into())
        );
        assert_eq!(
            parse_sse_connect_line(": comment"),
            SseConnectAction::Ignore
        );
        assert_eq!(
            parse_sse_connect_line(r#"data: not-json"#),
            SseConnectAction::Ignore
        );
        assert_eq!(
            parse_sse_connect_line(r#"data: {"type":"other"}"#),
            SseConnectAction::Ignore
        );
    }
}
