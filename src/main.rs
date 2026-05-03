mod agent;
mod ambient;
mod background;
mod bridges;
mod checkpoint;
mod collab;
mod config;
mod cost;
mod cost_db;
mod daemon;
mod diff_review;
mod events;
mod highlight;
mod memory_project;
mod notifications;
mod observability;
mod projects;
mod server;
mod swarm;
mod sync;
mod trust;
mod tui;

mod cli;

// mimalloc is linked but turso already sets the global allocator.
// We still benefit from mimalloc being in the dependency tree via turso.

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use harness_provider_core::ArcProvider;
use harness_provider_router::ProviderRouter;
use harness_provider_xai::{XaiConfig, XaiProvider};
use harness_tools::registry::Tool;
use harness_tools::tools::{
    GhTool, ListDirTool, PatchFileTool, ReadFileTool, RebuildSelfTool, ReloadSelfTool,
    SearchCodeTool, ShellConfig as ToolShellConfig, ShellTool, WriteFileTool,
};
use harness_tools::{SandboxMode, ToolExecutor, ToolRegistry, WorkspaceRoot};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::{fmt, EnvFilter};

use cli::{build_tools, connect_to_server, graceful_ambient_shutdown};
use cli::{CheckpointAction, Cli, Commands, CostAction, ProjectAction, SwarmAction, SyncAction};

#[tokio::main]
async fn main() -> Result<()> {
    // Auto-load .env from CWD or any parent directory (no-op if not found).
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    let filter = if cli.verbose {
        EnvFilter::new("harness=debug,harness_provider_xai=debug,harness_mcp=debug")
    } else {
        EnvFilter::new("harness=info,harness_mcp=warn")
    };
    fmt().with_env_filter(filter).with_target(false).init();

    let cfg = config::load(cli.config.as_deref())?;

    if let Some(Commands::Project { action }) = &cli.command {
        handle_project_command(action)?;
        return Ok(());
    }

    // Detect available API keys (in priority order: Anthropic > xAI > OpenAI > legacy config)
    let has_anthropic = std::env::var("ANTHROPIC_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    let has_xai = cfg.provider.api_key.is_some()
        || std::env::var("XAI_API_KEY")
            .map(|k| !k.is_empty())
            .unwrap_or(false);
    let has_openai = std::env::var("OPENAI_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    let has_ollama = cfg.providers.contains_key("ollama");

    if !has_anthropic
        && !has_xai
        && !has_openai
        && !has_ollama
        && cfg.providers.is_empty()
        && !harness_provider_mlx::mlx_runtime_available()
    {
        eprintln!("harness: no API key found.\n\nSet one of:\n  ANTHROPIC_API_KEY (recommended — claude-sonnet-4-6)\n  XAI_API_KEY       (grok-4.3)\n  OPENAI_API_KEY    (gpt-5.5)\n\nOr start a local model (Ollama / MLX LM server).\n\nOr run: harness doctor  for a guided setup.");
        std::process::exit(1);
    }

    let model = cli
        .model
        .or_else(|| cfg.provider.model.clone())
        .unwrap_or_else(|| {
            if has_anthropic {
                "claude-sonnet-4-6".to_string()
            } else if has_xai {
                "grok-4.3".to_string()
            } else if has_openai {
                "gpt-5.5".to_string()
            } else {
                "qwen3-coder:30b".to_string()
            }
        });

    let provider: ArcProvider = if !cfg.providers.is_empty() || has_anthropic || has_openai {
        // Smart router: builds from env vars + config
        let router = ProviderRouter::from_config(&cfg.providers, &cfg.router)
            .context("failed to build provider router")?;
        router.into_arc()
    } else if has_xai {
        let api_key = cfg
            .provider
            .api_key
            .clone()
            .or_else(|| std::env::var("XAI_API_KEY").ok())
            .unwrap();
        let xai_cfg = XaiConfig::new(&api_key)
            .with_model(&model)
            .with_max_tokens(cfg.provider.max_tokens.unwrap_or(8192))
            .with_temperature(cfg.provider.temperature.unwrap_or(0.7));
        std::sync::Arc::new(XaiProvider::new(xai_cfg)?)
    } else {
        // Fallback to router (handles Ollama etc.)
        let router = ProviderRouter::from_config(&cfg.providers, &cfg.router)
            .context("failed to build provider router")?;
        router.into_arc()
    };

    let session_store = harness_memory::SessionStore::open(
        cfg.session
            .db_path
            .clone()
            .unwrap_or_else(harness_memory::SessionStore::default_path),
    )?;

    let memory_db = cfg
        .memory
        .db_path
        .clone()
        .or_else(|| cfg.session.db_path.clone())
        .unwrap_or_else(harness_memory::SessionStore::default_path);

    let memory_store = if !cli.no_memory && cfg.memory.enabled.unwrap_or(true) {
        Some(harness_memory::MemoryStore::open(memory_db)?)
    } else {
        None
    };

    let embed_model = if memory_store.is_some() {
        Some(
            cfg.memory
                .embed_model
                .clone()
                .unwrap_or_else(|| "nomic-embed-text".into()),
        )
    } else {
        None
    };

    // Start ambient memory consolidation if memory is enabled.
    // ambient_shutdown is (sender, join_handle); send () then await handle for a clean exit.
    let ambient_shutdown: Option<(tokio::sync::watch::Sender<()>, tokio::task::JoinHandle<()>)> =
        if let (Some(mem), Some(em)) = (&memory_store, &embed_model) {
            let mem_arc = std::sync::Arc::new(mem.clone());
            Some(ambient::spawn(provider.clone(), mem_arc, em.clone()))
        } else {
            None
        };

    // CLI --browser flag overrides config; config.browser.enabled is the opt-in default.
    let browser_enabled = cli.browser || cfg.browser.enabled.unwrap_or(false);
    let browser_url = cfg.browser.url.clone().unwrap_or(cli.browser_url);

    // Build tools (including MCP servers if config exists).
    let tools = build_tools(
        provider.clone(),
        model.clone(),
        &cfg,
        browser_enabled,
        &browser_url,
        memory_store.clone(),
        embed_model.clone(),
    )
    .await;

    match cli.command {
        Some(Commands::Sessions) => {
            list_sessions(&session_store)?;
        }

        Some(Commands::Project { action }) => {
            handle_project_command(&action)?;
            return Ok(());
        }

        Some(Commands::Run { prompt }) => {
            let effective_prompt = build_prompt_with_image(&prompt, cli.image.as_deref())?;
            agent::run_once(
                &provider,
                &session_store,
                memory_store.as_ref(),
                embed_model.as_deref(),
                &tools,
                &model,
                cfg.agent.system_prompt.as_deref(),
                &effective_prompt,
                cli.resume.as_deref(),
            )
            .await?;
        }

        Some(Commands::Pr { number, comment }) => {
            if let Some(body) = comment {
                let out = GhTool
                    .execute(serde_json::json!({
                        "action": "pr_comment",
                        "number": number,
                        "message": body,
                    }))
                    .await?;
                println!("{out}");
                return Ok(());
            }
            use harness_tools::tools::gh::pr_context;
            eprintln!("Fetching PR #{number} context…");
            let context = pr_context(number)
                .await
                .unwrap_or_else(|e| format!("Error fetching PR: {e}"));
            let system_pr = format!(
                "{}\n\n# Reviewing PR #{number}\nYou are helping review and babysit this pull request. \
                 Check the CI status, review the diff, and help address any review comments or failures.",
                cfg.agent.system_prompt.as_deref().unwrap_or(agent::DEFAULT_SYSTEM)
            );
            agent::run_once(
                &provider,
                &session_store,
                memory_store.as_ref(),
                embed_model.as_deref(),
                &tools,
                &model,
                Some(&system_pr),
                &context,
                None,
            )
            .await?;
        }

        Some(Commands::Serve { addr }) => {
            let addr: std::net::SocketAddr = addr.parse().context("invalid address")?;
            let state = server::ServerState {
                provider,
                session_store: Arc::new(session_store),
                memory_store: memory_store.map(Arc::new),
                embed_model,
                tools,
                model,
                system_prompt: cfg
                    .agent
                    .system_prompt
                    .unwrap_or_else(|| agent::DEFAULT_SYSTEM.to_string()),
            };
            server::serve(state, addr).await?;
        }

        Some(Commands::Connect {
            url,
            prompt,
            session,
        }) => {
            connect_to_server(&url, &prompt, session.as_deref()).await?;
        }

        Some(Commands::Export { id, output }) => {
            export_session(&session_store, &id, output.as_deref())?;
        }

        Some(Commands::Delete { id }) => {
            delete_session(&session_store, &id)?;
        }

        Some(Commands::Init { project, force }) => {
            run_init(project, force)?;
            return Ok(());
        }

        Some(Commands::Status) => {
            let display_key = std::env::var("ANTHROPIC_API_KEY")
                .or_else(|_| std::env::var("XAI_API_KEY"))
                .or_else(|_| std::env::var("OPENAI_API_KEY"))
                .unwrap_or_default();
            run_status(&cfg, &model, &session_store, &display_key)?;
            return Ok(());
        }

        Some(Commands::Trust { tool, pattern }) => {
            let mut store = trust::TrustStore::load();
            if store.add_rule(&tool, &pattern) {
                store.save()?;
                println!("Trust rule added: {tool} / {pattern}");
            } else {
                println!("Rule already exists: {tool} / {pattern}");
            }
            return Ok(());
        }

        Some(Commands::Untrust { tool, pattern }) => {
            let mut store = trust::TrustStore::load();
            if store.remove_rule(&tool, &pattern) {
                store.save()?;
                println!("Trust rule removed: {tool} / {pattern}");
            } else {
                println!("No matching rule: {tool} / {pattern}");
            }
            return Ok(());
        }

        Some(Commands::TrustList) => {
            let store = trust::TrustStore::load();
            let rules = store.list();
            if rules.is_empty() {
                println!("No trust rules. Use `harness trust <tool> <pattern>` to add one.");
            } else {
                println!("{:<20} {:<40} ADDED", "TOOL", "PATTERN");
                for rule in rules {
                    println!("{:<20} {:<40} {}", rule.tool, rule.pattern, rule.added);
                }
            }
            return Ok(());
        }

        Some(Commands::Daemon) => {
            println!("Starting harness daemon…");
            println!("Socket: {}", daemon::socket_path().display());
            println!("Press Ctrl+C to stop.");
            let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());
            let system = cfg
                .agent
                .system_prompt
                .clone()
                .unwrap_or_else(|| agent::DEFAULT_SYSTEM.to_string());
            tokio::select! {
                res = daemon::run_daemon(
                    provider, session_store, memory_store, embed_model,
                    tools, model, system, shutdown_rx
                ) => {
                    if let Err(e) = res { eprintln!("daemon: {e}"); }
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\nDaemon stopped.");
                    let _ = shutdown_tx.send(());
                }
            }
            return Ok(());
        }

        Some(Commands::DaemonStatus) => {
            if daemon::is_running().await {
                match daemon::send_request(&daemon::DaemonRequest {
                    id: 1,
                    method: "status".into(),
                    params: serde_json::json!({}),
                })
                .await
                {
                    Ok(resp) => {
                        if let Some(result) = resp.result {
                            println!("Daemon running: {}", serde_json::to_string_pretty(&result)?);
                        }
                    }
                    Err(e) => eprintln!("daemon status: {e}"),
                }
            } else {
                println!("Daemon is not running.");
                println!("Start with: harness daemon");
            }
            return Ok(());
        }

        Some(Commands::RunBg { prompt }) => {
            match background::spawn(&prompt) {
                Ok(id) => {
                    println!("Background run started: {id}");
                    println!("Output: ~/.harness/runs/{id}/output.log");
                    println!("Status: harness runs");
                }
                Err(e) => eprintln!("run-bg: {e}"),
            }
            return Ok(());
        }

        Some(Commands::Runs) => {
            let runs = background::list(20)?;
            if runs.is_empty() {
                println!("No background runs yet.");
            } else {
                println!("{:<10} {:<8} {:<25} PROMPT", "ID", "STATUS", "STARTED");
                for run in runs {
                    let prompt_preview = if run.prompt.len() > 40 {
                        format!("{}…", &run.prompt[..40])
                    } else {
                        run.prompt.clone()
                    };
                    println!(
                        "{:<10} {:<8} {:<25} {}",
                        run.id, run.status, run.started_at, prompt_preview
                    );
                }
            }
            return Ok(());
        }

        Some(Commands::Undo) => {
            match checkpoint::undo() {
                Ok(msg) => println!("{msg}"),
                Err(e) => eprintln!("undo: {e}"),
            }
            return Ok(());
        }

        Some(Commands::Checkpoint {
            action: CheckpointAction::List,
        }) => {
            let entries = checkpoint::list()?;
            if entries.is_empty() {
                println!("No harness checkpoint stashes found.");
            } else {
                println!("{:<12} MESSAGE", "STASH");
                for (stash_ref, msg) in entries {
                    println!("{:<12} {}", stash_ref, msg);
                }
            }
            return Ok(());
        }

        Some(Commands::Voice {
            duration,
            send,
            realtime,
        }) => {
            use harness_voice::{record_and_transcribe, WhisperBackend};
            use std::time::Duration;

            let openai_key = std::env::var("OPENAI_API_KEY").ok();

            if realtime {
                let key = openai_key
                    .clone()
                    .context("OPENAI_API_KEY required for realtime voice")?;
                eprintln!("Starting realtime voice session (Ctrl+C to stop)…");
                eprintln!("Connect to the OpenAI Realtime API — speak naturally.");
                let mut session = harness_voice::RealtimeVoiceSession::connect(
                    &key,
                    "You are a helpful coding assistant. Be concise and technical.",
                )
                .await?;
                // Simple: capture 5s chunks, send to API, print transcripts
                let dur = Duration::from_secs(duration);
                loop {
                    let backend = WhisperBackend::OpenAI {
                        api_key: key.clone(),
                        base_url: "https://api.openai.com/v1".to_string(),
                    };
                    let wav = harness_voice::record_and_transcribe(dur, &backend).await;
                    match wav {
                        Ok(t) if !t.is_empty() => {
                            eprintln!("You: {t}");
                            if t.to_lowercase().contains("goodbye")
                                || t.to_lowercase().contains("quit")
                            {
                                break;
                            }
                        }
                        _ => {}
                    }
                    if let Ok(ev) = session.event_rx.try_recv() {
                        match ev {
                            harness_voice::RealtimeEvent::TurnComplete(text) => {
                                eprintln!("AI: {text}")
                            }
                            harness_voice::RealtimeEvent::Error(e) => {
                                eprintln!("Error: {e}");
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                return Ok(());
            }

            let backend = WhisperBackend::detect(openai_key.as_deref());
            if !harness_voice::voice_available() && matches!(backend, WhisperBackend::Local { .. })
            {
                eprintln!("Warning: no local audio recorder found. Install sox: brew install sox");
            }

            eprintln!("Recording for {duration}s… (speak now)");
            let transcript = record_and_transcribe(Duration::from_secs(duration), &backend).await?;
            println!("{transcript}");

            if send && !transcript.is_empty() {
                agent::run_once(
                    &provider,
                    &session_store,
                    memory_store.as_ref(),
                    embed_model.as_deref(),
                    &tools,
                    &model,
                    cfg.agent.system_prompt.as_deref(),
                    &transcript,
                    cli.resume.as_deref(),
                )
                .await?;
            }
            return Ok(());
        }

        Some(Commands::Swarm { action }) => {
            match action {
                SwarmAction::Run {
                    prompt,
                    model: run_model,
                    count,
                } => {
                    let n = count.unwrap_or(1).clamp(1, 32);
                    let worker_model = run_model.unwrap_or_else(|| model.clone());
                    for i in 0..n {
                        let label = if n > 1 {
                            format!("{prompt} [swarm {}/{}]", i + 1, n)
                        } else {
                            prompt.clone()
                        };
                        let id = swarm::register_task(&label)?;
                        let p = provider.clone();
                        let t = tools.clone();
                        let mem = memory_store.clone();
                        let emb = embed_model.clone();
                        let sys = cfg.agent.system_prompt.clone();
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
                                Ok(reply)
                            }
                        })
                        .await;
                    }
                    println!("Queued {n} swarm task(s). Use: harness swarm list");
                }
                SwarmAction::List => swarm::print_status()?,
                SwarmAction::Status { id } => match swarm::get_task(&id)? {
                    Some(t) => println!("{} [{}] {}", t.id, t.status.as_str(), t.prompt),
                    None => println!("Task {id} not found."),
                },
                SwarmAction::Result { id } => match swarm::get_task(&id)? {
                    Some(t) => println!("{}", t.result.as_deref().unwrap_or("(no result)")),
                    None => println!("Task {id} not found."),
                },
            }
            return Ok(());
        }

        Some(Commands::Trace { id }) => {
            match id {
                Some(trace_id) => observability::export_trace(&trace_id)?,
                None => {
                    let spans = observability::load_last_trace()?;
                    if spans.is_empty() {
                        println!("No traces found. Enable [observability] in config.");
                    } else {
                        println!("Trace {} — {} spans:", spans[0].trace_id, spans.len());
                        for s in &spans {
                            println!("  {:<40} {}ms", s.name, s.duration_ms);
                        }
                    }
                }
            }
            return Ok(());
        }

        Some(Commands::Doctor) => {
            handle_doctor_command(&cfg).await;
            return Ok(());
        }

        Some(Commands::Completions { shell }) => {
            let mut cmd = Cli::command();
            let bin_name = cmd.get_name().to_string();
            generate(shell, &mut cmd, bin_name, &mut std::io::stdout());
            return Ok(());
        }

        Some(Commands::Models { set }) => {
            handle_models_command(set, &cfg).await?;
            return Ok(());
        }

        Some(Commands::Sync { action }) => {
            match action {
                SyncAction::Init { git_url } => sync::init(&git_url).await?,
                SyncAction::Push => sync::push().await?,
                SyncAction::Pull => sync::pull().await?,
                SyncAction::Status => sync::status().await?,
                SyncAction::Auth => {
                    println!(
                        "Sync passphrase is stored in the system keychain under 'harness-sync'."
                    );
                    println!("To transfer to another machine, run: harness sync init <git-url>");
                    println!("Then on the new machine, run: harness sync pull");
                    println!("The passphrase will be regenerated and stored on the new machine.");
                }
            }
            return Ok(());
        }

        Some(Commands::Cost { action }) => {
            use cost_db::{days_ago, format_usd, CostDb};
            let db = CostDb::open().context("opening cost.db")?;
            match action {
                CostAction::Today => {
                    let usd = db.total_usd_since(days_ago(1))?;
                    println!("Today: {}", format_usd(usd));
                }
                CostAction::Week => {
                    let usd = db.total_usd_since(days_ago(7))?;
                    println!("Past 7 days: {}", format_usd(usd));
                }
                CostAction::Month => {
                    let usd = db.total_usd_since(days_ago(30))?;
                    println!("Past 30 days: {}", format_usd(usd));
                }
                CostAction::All => {
                    let usd = db.total_usd_since(0)?;
                    println!("All time: {}", format_usd(usd));
                }
                CostAction::ByModel => {
                    let rows = db.by_model_since(0)?;
                    if rows.is_empty() {
                        println!("No usage data yet.");
                    } else {
                        println!("{:<35} Cost", "Model");
                        println!("{}", "-".repeat(45));
                        for (model, usd) in rows {
                            println!("{:<35} {}", model, format_usd(usd));
                        }
                    }
                }
                CostAction::ByProject => {
                    let rows = db.by_project_since(0)?;
                    if rows.is_empty() {
                        println!("No usage data yet.");
                    } else {
                        println!("{:<35} Cost", "Project");
                        println!("{}", "-".repeat(45));
                        for (project, usd) in rows {
                            let display = if project.is_empty() {
                                "(unnamed)".to_string()
                            } else {
                                project
                            };
                            println!("{:<35} {}", display, format_usd(usd));
                        }
                    }
                }
                CostAction::Watch => {
                    println!("Watching cost.db (Ctrl+C to stop)…\n");
                    loop {
                        let rows = db.recent(5)?;
                        let today = db.total_usd_since(days_ago(1))?;
                        let month = db.total_usd_since(days_ago(30))?;
                        print!("\x1B[2J\x1B[H"); // clear screen
                        println!(
                            "  Today: {}  |  30 days: {}",
                            format_usd(today),
                            format_usd(month)
                        );
                        println!("\nRecent turns:");
                        for r in &rows {
                            println!(
                                "  {} │ {} │ ↑{} ↓{} │ {}",
                                r.model,
                                &r.session_id[..8.min(r.session_id.len())],
                                r.in_tok,
                                r.out_tok,
                                format_usd(r.usd)
                            );
                        }
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                }
            }
            return Ok(());
        }

        Some(Commands::Memorize { topic, fact }) => {
            match memory_project::remember(&topic, &fact) {
                Ok(path) => println!("Remembered under '{}': {}", topic, path.display()),
                Err(e) => eprintln!("Error saving memory: {e}"),
            }
            return Ok(());
        }

        Some(Commands::Forget { topic }) => {
            match memory_project::forget(&topic) {
                Ok(true) => println!("Forgot topic '{topic}'"),
                Ok(false) => println!("No memory for topic '{topic}'"),
                Err(e) => eprintln!("Error: {e}"),
            }
            return Ok(());
        }

        Some(Commands::Memories) => {
            let topics = memory_project::list_topics();
            if topics.is_empty() {
                println!("No project memories. Use: harness memorize <topic> <fact>");
            } else {
                println!("{} project memory topic(s):", topics.len());
                for t in &topics {
                    println!("  • {t}");
                }
            }
            return Ok(());
        }

        Some(Commands::SelfDev {
            src,
            model: sd_model,
        }) => {
            let src_dir = match src {
                Some(path) => path,
                None => std::env::current_dir().context("failed to get current directory")?,
            };
            let sd_model = sd_model.unwrap_or_else(|| model.clone());
            run_self_dev(
                provider,
                session_store,
                memory_store,
                embed_model,
                src_dir,
                sd_model,
                &cfg,
            )
            .await?;
        }

        None => {
            if let Some(prompt) = cli.prompt {
                let effective_prompt = build_prompt_with_image(&prompt, cli.image.as_deref())?;
                agent::run_once(
                    &provider,
                    &session_store,
                    memory_store.as_ref(),
                    embed_model.as_deref(),
                    &tools,
                    &model,
                    cfg.agent.system_prompt.as_deref(),
                    &effective_prompt,
                    cli.resume.as_deref(),
                )
                .await?;
            } else {
                let ambient_tx = ambient_shutdown.as_ref().map(|(tx, _)| tx.clone());
                // In plan mode, create a confirm gate channel and pass it to both the
                // tools executor and the TUI (which will handle the confirmation prompts).
                let (tools, confirm_rx) = if cli.plan {
                    let (gate, rx) = harness_tools::confirm::channel();
                    (tools.with_confirm_gate(gate), Some(rx))
                } else {
                    (tools, None)
                };
                let result = tui::run(
                    provider,
                    session_store,
                    memory_store,
                    embed_model,
                    tools,
                    model,
                    cfg,
                    cli.resume.as_deref(),
                    ambient_tx,
                    confirm_rx,
                )
                .await;
                graceful_ambient_shutdown(ambient_shutdown).await;
                result?;
                return Ok(());
            }
        }
    }

    graceful_ambient_shutdown(ambient_shutdown).await;
    Ok(())
}

/// Signal the ambient consolidation task to stop and wait up to 5 s for it.
/// Self-dev mode: builds a tool executor with rebuild/reload tools, then launches
/// the TUI with a self-dev system prompt so the agent can modify its own source.
async fn run_self_dev(
    provider: ArcProvider,
    session_store: harness_memory::SessionStore,
    memory_store: Option<harness_memory::MemoryStore>,
    embed_model: Option<String>,
    src_dir: PathBuf,
    model: String,
    cfg: &config::Config,
) -> Result<()> {
    tracing::info!(src = %src_dir.display(), model = %model, "starting self-dev mode");

    let workspace = Arc::new(WorkspaceRoot::new(
        src_dir.clone(),
        SandboxMode::from_config(cfg.tools.sandbox.as_deref()),
    )?);

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
    registry.register(ListDirTool {
        workspace: workspace.clone(),
    });
    registry.register(ShellTool::new(shell_cfg, workspace.clone()));
    registry.register(SearchCodeTool);
    registry.register(RebuildSelfTool::new(src_dir.clone()));
    registry.register(ReloadSelfTool::new(src_dir.clone()));

    let tools = ToolExecutor::new(registry);

    // Tailor the config for self-dev
    let mut sd_cfg = config::Config {
        provider: config::ProviderConfig {
            model: Some(model.clone()),
            ..Default::default()
        },
        agent: config::AgentConfig {
            system_prompt: Some(SELF_DEV_SYSTEM.to_string()),
        },
        ..Default::default()
    };
    // Keep memory/session + tool sandbox settings from user config
    sd_cfg.session = cfg.session.clone();
    sd_cfg.memory = cfg.memory.clone();
    sd_cfg.tools = cfg.tools.clone();

    tui::run(
        provider,
        session_store,
        memory_store,
        embed_model,
        tools,
        model,
        sd_cfg,
        None,
        None,
        None,
    )
    .await
}

const SELF_DEV_SYSTEM: &str = "\
You are harness, a Rust coding agent, running in self-development mode.
You have access to your own source code and can modify it.
You can also use browser automation (when enabled), MCP tools (when configured),
and spawn sub-agents for parallel tasks.

Source layout:
  src/main.rs          — CLI, tool wiring, self-dev entry point
  src/agent.rs         — core agent loop, memory injection
  src/tui.rs           — two-panel ratatui TUI
  src/highlight.rs     — syntect syntax highlighting
  src/server.rs        — axum HTTP/SSE server
  src/events.rs        — AgentEvent enum
  src/config.rs        — TOML config structs
  crates/harness-provider-core/  — Provider trait, Message/Delta types
  crates/harness-provider-xai/   — Grok streaming client, embeddings
  crates/harness-tools/          — Tool trait, built-in tools (file/shell/search/spawn/selfdev)
  crates/harness-memory/         — SQLite session & vector memory store
  crates/harness-mcp/            — MCP stdio protocol client

Workflow:
1. Read relevant source files before editing.
2. Make targeted edits with write_file or shell (use patch/sed for small changes).
3. Call rebuild_self to check your changes compile (use check_only=true for a fast check).
4. Fix any compiler errors, then rebuild.
5. Once the build succeeds, call reload_self to hot-swap to the new binary.

Be methodical: one change at a time, verify compilation before proceeding.
Prefer small, well-understood edits over large rewrites.";

fn export_session(
    store: &harness_memory::SessionStore,
    id: &str,
    output: Option<&std::path::Path>,
) -> Result<()> {
    use harness_provider_core::Role;
    use std::fmt::Write as FmtWrite;

    let session = store.find(id)?.ok_or_else(|| {
        anyhow::anyhow!(
            "session not found: '{}'. Use 'harness sessions' to list available sessions.",
            id
        )
    })?;

    let mut md = String::new();

    // Front matter
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
                    // Decode tool calls for readability
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
                // Truncate very long tool outputs
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

fn list_sessions(store: &harness_memory::SessionStore) -> Result<()> {
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

fn run_init(project: bool, force: bool) -> Result<()> {
    // Match `scripts/install.sh`: Anthropic-oriented template, no secrets in file.
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

    // ── Project config ─────────────────────────────────────────────────────────
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

            // Write system prompt as editable markdown too
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

fn run_status(
    cfg: &config::Config,
    model: &str,
    store: &harness_memory::SessionStore,
    api_key: &str,
) -> Result<()> {
    println!("harness status\n");

    // API key source
    let key_source = if cfg.provider.api_key.is_some() {
        "config file"
    } else if std::env::var("XAI_API_KEY").is_ok() {
        "XAI_API_KEY env var"
    } else {
        "unknown"
    };
    let key_preview = if api_key.len() > 8 {
        format!("{}…{}", &api_key[..6], &api_key[api_key.len() - 4..])
    } else {
        "(too short)".to_string()
    };
    println!("  API key : {} ({})", key_preview, key_source);
    println!("  Model   : {model}");

    // Config file in use
    let cfg_path = {
        let local = std::path::PathBuf::from(".harness/config.toml");
        let global = dirs::home_dir()
            .unwrap_or_default()
            .join(".harness/config.toml");
        if local.exists() {
            format!("{} (project)", local.display())
        } else if global.exists() {
            format!("{} (global)", global.display())
        } else {
            "defaults (no config file found)".to_string()
        }
    };
    println!("  Config  : {cfg_path}");

    // MCP config
    let mcp_path = harness_mcp::find_config();
    println!(
        "  MCP     : {}",
        mcp_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "not configured".to_string())
    );

    // Recent sessions
    println!();
    println!("Recent sessions:");
    match store.list(5) {
        Ok(sessions) if !sessions.is_empty() => {
            for (id, name, updated) in sessions {
                let short = id.chars().take(8).collect::<String>();
                println!(
                    "  {} · {} · {}",
                    short,
                    name.unwrap_or_else(|| "(unnamed)".to_string()),
                    updated
                );
            }
        }
        Ok(_) => println!("  (none yet)"),
        Err(e) => println!("  (error reading sessions: {e})"),
    }

    println!();
    println!("Run `harness` to start a new session.");
    Ok(())
}

/// If an image path is provided, attach it to the prompt text as a note.
/// The actual image content is embedded in the message when the provider supports it.
fn build_prompt_with_image(prompt: &str, image: Option<&std::path::Path>) -> Result<String> {
    match image {
        None => Ok(prompt.to_string()),
        Some(path) => {
            // Embed image as a base64 data URI annotation that vision-capable providers understand.
            let _content =
                harness_provider_core::MessageContent::with_image(prompt, &path.to_string_lossy())?;
            // For now, return the prompt with a note about the image.
            // Providers that support vision (Anthropic, OpenAI, xAI) will be updated to use
            // the Parts variant directly when MessageContent wiring is complete.
            Ok(format!("{prompt}\n\n[image attached: {}]", path.display()))
        }
    }
}

/// Handle `harness models [--set provider:model]`.
async fn handle_models_command(set: Option<String>, cfg: &config::Config) -> Result<()> {
    use harness_provider_router::ProviderEntry;
    use std::collections::HashMap;

    // Model catalogue: provider → [(model, description)]
    let catalogue: &[(&str, &[(&str, &str)])] = &[
        (
            "anthropic",
            &[
                ("claude-opus-4-7", "$5/$25 · 1M ctx · adaptive thinking"),
                ("claude-sonnet-4-6", "$3/$15 · 1M ctx · default ★"),
                ("claude-haiku-4-5", "$1/$5  · fast / cheap"),
            ],
        ),
        (
            "openai",
            &[
                ("gpt-5.5", "$5/$30  · 1M ctx"),
                ("gpt-5.4", "$2.50/$15"),
                ("gpt-5.4-mini", "$0.75/$4.50 · fast"),
                ("gpt-5.4-nano", "$0.20/$1.25 · ultra-cheap"),
                ("o4-mini", "$1.10/$4.40 · reasoning"),
            ],
        ),
        (
            "xai",
            &[
                ("grok-4.3", "$1.25/$2.50 · 1M ctx · flagship ★"),
                (
                    "grok-4.20-0309-reasoning",
                    "$2/$6   · pinned 2M ctx snapshot",
                ),
                ("grok-4-1-fast-reasoning", "$0.20/$0.50 · fast"),
            ],
        ),
        (
            "ollama",
            &[
                ("qwen3-coder:30b", "local · 256K ctx · agentic ★"),
                ("qwen2.5-coder:32b", "local · 92.7% HumanEval"),
                ("nomic-embed-text", "local · embed"),
            ],
        ),
    ];

    if let Some(ref model_spec) = set {
        // Use toml_edit for clean, idempotent TOML manipulation
        let local_cfg = std::path::PathBuf::from(".harness").join("config.toml");
        let _ = std::fs::create_dir_all(".harness");
        let text = if local_cfg.exists() {
            std::fs::read_to_string(&local_cfg).unwrap_or_default()
        } else {
            String::new()
        };

        let (provider_part, model_part) = if model_spec.contains(':') {
            let mut parts = model_spec.splitn(2, ':');
            (
                parts.next().unwrap_or("").to_string(),
                parts.next().unwrap_or("").to_string(),
            )
        } else {
            (String::new(), model_spec.clone())
        };

        let mut doc: toml_edit::DocumentMut = text.parse().unwrap_or_default();

        // Set [provider].model
        if !doc.contains_key("provider") {
            doc["provider"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        doc["provider"]["model"] = toml_edit::value(model_part.as_str());

        if !provider_part.is_empty() {
            // Also set [router].default to the provider name
            if !doc.contains_key("router") {
                doc["router"] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            doc["router"]["default"] = toml_edit::value(provider_part.as_str());
        }

        std::fs::write(&local_cfg, doc.to_string())?;
        println!(
            "✓ Default model set to '{model_spec}' in {}",
            local_cfg.display()
        );
        return Ok(());
    }

    // List all providers + models
    println!("Available models (May 2026):");
    println!();
    for (provider, models) in catalogue {
        let env_key = match *provider {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openai" => "OPENAI_API_KEY",
            "xai" => "XAI_API_KEY",
            _ => "",
        };
        let available = if env_key.is_empty() {
            "local".to_string()
        } else if std::env::var(env_key)
            .map(|k| !k.is_empty())
            .unwrap_or(false)
        {
            "✓ key set".to_string()
        } else {
            format!("✗ {} not set", env_key)
        };
        println!("  {provider} ({available})");
        for (model, desc) in *models {
            let current = cfg.provider.model.as_deref() == Some(model);
            let marker = if current { " ◀ current" } else { "" };
            println!("    {:42} {desc}{marker}", model);
        }
        println!();
    }

    let current = cfg.provider.model.as_deref().unwrap_or("claude-sonnet-4-6");
    println!("Current default: {current}");
    println!();
    println!("To switch: harness models --set <provider:model>");
    println!("Example:   harness models --set anthropic:claude-opus-4-7");

    let _ = cfg;
    let _ = HashMap::<String, ProviderEntry>::new();
    Ok(())
}

async fn handle_doctor_command(cfg: &config::Config) {
    println!("harness doctor — system health check\n");

    // API keys
    let checks: &[(&str, &str, &str)] = &[
        (
            "ANTHROPIC_API_KEY",
            "Anthropic Claude 4.x",
            "claude-sonnet-4-6",
        ),
        ("XAI_API_KEY", "xAI Grok 4.x", "grok-4.3"),
        ("OPENAI_API_KEY", "OpenAI GPT-5.x", "gpt-5.5"),
    ];
    println!("  API Keys:");
    let mut any_key = false;
    for (env, name, model) in checks {
        let set = std::env::var(env).map(|k| !k.is_empty()).unwrap_or(false);
        if set {
            any_key = true;
        }
        println!(
            "  {} {} → {}",
            if set { "✓" } else { "✗" },
            name,
            if set {
                format!("key set, will use {model}")
            } else {
                format!("set {env} to enable")
            }
        );
    }
    if !any_key {
        println!("\n  ⚠ No API key found! Set ANTHROPIC_API_KEY to get started.");
        println!("  Export it in your shell: export ANTHROPIC_API_KEY=sk-ant-...");
    }

    // Ollama
    let ollama_running = tokio::process::Command::new("ollama")
        .arg("list")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    println!(
        "  {} Ollama local models: {}",
        if ollama_running { "✓" } else { "○" },
        if ollama_running {
            "running"
        } else {
            "not running (optional)"
        }
    );

    let mlx_ok = harness_provider_mlx::mlx_runtime_available();
    println!(
        "  {} MLX LM server (OpenAI-compatible HTTP): {}",
        if mlx_ok { "✓" } else { "○" },
        if mlx_ok {
            "mlx_lm.server on PATH or :8080 accepting connections"
        } else {
            "not detected (optional — Apple Silicon: mlx_lm.server, default http://127.0.0.1:8080/v1)"
        }
    );

    // Tools
    println!("\n  External tools:");
    let tools: &[(&str, &str)] = &[
        ("git", "version control"),
        ("gh", "GitHub CLI (PR/issues)"),
        ("rg", "ripgrep code search"),
        ("cargo", "Rust builds"),
        ("node", "Node.js (TypeScript LSP)"),
        ("sox", "audio recording (voice)"),
    ];
    for (tool, desc) in tools {
        let found = tokio::process::Command::new(tool)
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        println!("  {} {} — {}", if found { "✓" } else { "○" }, tool, desc);
    }

    // Config files
    println!("\n  Config:");
    let user_cfg = dirs::home_dir()
        .unwrap_or_default()
        .join(".harness/config.toml");
    let local_cfg = std::path::PathBuf::from(".harness/config.toml");
    println!(
        "  {} ~/.harness/config.toml",
        if user_cfg.exists() {
            "✓"
        } else {
            "○ (optional)"
        }
    );
    println!(
        "  {} .harness/config.toml",
        if local_cfg.exists() {
            "✓"
        } else {
            "○ (optional — run harness init --project to create)"
        }
    );
    println!(
        "  Current model: {}",
        cfg.provider.model.as_deref().unwrap_or("(auto)")
    );

    // Memory + cost DB
    let mem_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".harness/sessions.db");
    let cost_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".harness/cost.db");
    println!("\n  Data:");
    println!(
        "  {} sessions DB: {}",
        if mem_path.exists() { "✓" } else { "○" },
        mem_path.display()
    );
    println!(
        "  {} cost DB: {}",
        if cost_path.exists() { "✓" } else { "○" },
        cost_path.display()
    );

    // Daemon
    let sock = dirs::home_dir()
        .unwrap_or_default()
        .join(".harness/daemon.sock");
    println!("\n  Daemon:");
    println!(
        "  {} daemon socket: {}",
        if sock.exists() {
            "✓ running"
        } else {
            "○ not running (optional — run harness daemon)"
        },
        sock.display()
    );

    println!("\nRun `harness init` to create a default config.");
    println!("Run `harness completions zsh > ~/.zfunc/_harness` to add shell completions.");
}

fn handle_project_command(action: &ProjectAction) -> Result<()> {
    let mut store = projects::ProjectStore::load();

    match action {
        ProjectAction::Init {
            name,
            path,
            default_branch,
        } => {
            let parent = path
                .clone()
                .unwrap_or(std::env::current_dir().context("reading current directory")?);
            if !parent.exists() {
                anyhow::bail!("path does not exist: {}", parent.display());
            }
            let project_dir = parent.join(name);
            if project_dir.exists() {
                anyhow::bail!(
                    "project directory already exists: {}",
                    project_dir.display()
                );
            }
            std::fs::create_dir_all(&project_dir)
                .with_context(|| format!("creating {}", project_dir.display()))?;

            init_git_repo(&project_dir, default_branch)?;
            let readme_path = project_dir.join("README.md");
            if !readme_path.exists() {
                std::fs::write(&readme_path, format!("# {name}\n"))
                    .with_context(|| format!("writing {}", readme_path.display()))?;
            }

            let outcome = store.add(
                Some(name.clone()),
                Some(project_dir.clone()),
                None,
                Some(default_branch.clone()),
            )?;
            store.save()?;
            let entry = match outcome {
                projects::AddOutcome::Added(entry) | projects::AddOutcome::Updated(entry) => entry,
            };
            println!("Initialized and linked project '{}'", entry.name);
            println!("  path: {}", entry.path.display());
            println!("  branch: {default_branch}");
            println!(
                "Next: harness project publish {} --public|--private",
                entry.name
            );
        }
        ProjectAction::Add {
            name,
            path,
            remote,
            default_branch,
        } => {
            let outcome = store.add(
                name.clone(),
                path.clone(),
                remote.clone(),
                default_branch.clone(),
            )?;
            store.save()?;
            match outcome {
                projects::AddOutcome::Added(entry) => {
                    println!("Added project '{}'", entry.name);
                    println!("  path: {}", entry.path.display());
                    if let Some(remote) = entry.remote {
                        println!("  remote: {remote}");
                    }
                }
                projects::AddOutcome::Updated(entry) => {
                    println!("Updated project '{}'", entry.name);
                    println!("  path: {}", entry.path.display());
                    if let Some(remote) = entry.remote {
                        println!("  remote: {remote}");
                    }
                }
            }
        }
        ProjectAction::Clone {
            repo,
            name,
            directory,
            default_branch,
        } => {
            let clone_dir = directory
                .clone()
                .unwrap_or_else(|| PathBuf::from(infer_clone_directory(repo)));

            let status = std::process::Command::new("git")
                .arg("clone")
                .arg(repo)
                .arg(&clone_dir)
                .status()
                .context("running git clone")?;
            if !status.success() {
                anyhow::bail!("git clone failed with status {status}");
            }

            let outcome = store.add(
                name.clone(),
                Some(clone_dir),
                Some(repo.clone()),
                default_branch.clone(),
            )?;
            store.save()?;
            let entry = match outcome {
                projects::AddOutcome::Added(entry) | projects::AddOutcome::Updated(entry) => entry,
            };
            println!("Cloned and linked project '{}'", entry.name);
            println!("  path: {}", entry.path.display());
            if let Some(remote) = entry.remote {
                println!("  remote: {remote}");
            }
        }
        ProjectAction::List => {
            let projects = store.list_sorted();
            if projects.is_empty() {
                println!("No linked projects yet. Use `harness project add`.");
                return Ok(());
            }

            println!("Linked projects: {}\n", projects.len());
            println!("{:<22} {:<52} {:<22} BRANCH", "NAME", "PATH", "REMOTE");
            for p in projects {
                let remote = p.remote.unwrap_or_else(|| "-".to_string());
                let branch = p.default_branch.unwrap_or_else(|| "-".to_string());
                println!(
                    "{:<22} {:<52} {:<22} {}",
                    p.name,
                    p.path.display(),
                    remote,
                    branch
                );
            }
        }
        ProjectAction::Dashboard => {
            let projects = store.list_sorted();
            if projects.is_empty() {
                println!("No linked projects yet. Use `harness project add`.");
                return Ok(());
            }

            println!("Project dashboard: {}\n", projects.len());
            println!(
                "{:<20} {:<18} {:<17} {:<16} STATUS",
                "PROJECT", "BRANCH", "AHEAD/BEHIND", "CHANGES"
            );
            println!("{}", "-".repeat(88));
            for p in projects {
                match project_health_row(&p.path) {
                    Ok(row) => {
                        println!(
                            "{:<20} {:<18} {:<17} {:<16} {}",
                            p.name,
                            row.branch,
                            format!("+{} / -{}", row.ahead, row.behind),
                            format!(
                                "S:{} U:{} ?:{}",
                                row.changes.staged, row.changes.unstaged, row.changes.untracked
                            ),
                            status_badge(&row.status)
                        );
                    }
                    Err(err) => {
                        println!(
                            "{:<20} {:<18} {:<17} {:<16} error: {}",
                            p.name, "-", "-", "-", err
                        );
                    }
                }
            }
        }
        ProjectAction::Remove { target } => {
            if let Some(removed) = store.remove(target) {
                store.save()?;
                println!("Removed project '{}'", removed.name);
                println!("  path: {}", removed.path.display());
            } else {
                anyhow::bail!("project '{target}' not found. Run `harness project list`.");
            }
        }
        ProjectAction::Sync { target, all } => {
            let targets = if *all {
                let projects = store.list_sorted();
                if projects.is_empty() {
                    println!("No linked projects yet. Use `harness project add`.");
                    return Ok(());
                }
                projects
            } else if let Some(name_or_path) = target {
                vec![store.find(name_or_path).with_context(|| {
                    format!("project '{name_or_path}' not found. Run `harness project list`.")
                })?]
            } else {
                anyhow::bail!("provide <target> or use --all");
            };

            let mut synced = 0usize;
            for entry in targets {
                sync_project(&entry)?;
                let _ = store.add(
                    Some(entry.name.clone()),
                    Some(entry.path.clone()),
                    entry.remote.clone(),
                    entry.default_branch.clone(),
                )?;
                println!("Synced project '{}'", entry.name);
                println!("  path: {}", entry.path.display());
                synced += 1;
            }
            store.save()?;
            println!("Synced {synced} project(s).");
        }
        ProjectAction::Push {
            target,
            remote,
            branch,
            force,
        } => {
            let entry = store.find(target).with_context(|| {
                format!("project '{target}' not found. Run `harness project list`.")
            })?;

            let resolved_branch = if let Some(override_branch) = branch.clone() {
                override_branch
            } else {
                current_git_branch(&entry.path)
                    .or(entry.default_branch.clone())
                    .with_context(|| {
                        format!(
                            "could not determine branch for '{}'; pass --branch explicitly",
                            entry.name
                        )
                    })?
            };

            if *force && matches!(resolved_branch.as_str(), "main" | "master") {
                anyhow::bail!(
                    "force push to '{}' is blocked for safety. Push without --force.",
                    resolved_branch
                );
            }

            let mut cmd = std::process::Command::new("git");
            cmd.current_dir(&entry.path).arg("push");
            if *force {
                cmd.arg("--force-with-lease");
            }
            cmd.arg(remote).arg(&resolved_branch);

            let status = cmd.status().context("running git push")?;
            if !status.success() {
                anyhow::bail!("git push failed with status {status}");
            }

            let _ = store.add(
                Some(entry.name.clone()),
                Some(entry.path.clone()),
                entry.remote.clone(),
                Some(resolved_branch.clone()),
            )?;
            store.save()?;
            println!("Pushed '{}' to {remote}/{resolved_branch}", entry.name);
            println!("  path: {}", entry.path.display());
        }
        ProjectAction::Status { target } => {
            let entry = store.find(target).with_context(|| {
                format!("project '{target}' not found. Run `harness project list`.")
            })?;

            let branch =
                current_git_branch(&entry.path).unwrap_or_else(|| "(detached HEAD)".to_string());
            let upstream = git_output(
                &entry.path,
                &[
                    "rev-parse",
                    "--abbrev-ref",
                    "--symbolic-full-name",
                    "@{upstream}",
                ],
            )
            .ok();
            let remote_url = git_output(&entry.path, &["remote", "get-url", "origin"]).ok();
            let changes = collect_change_counts(&entry.path)?;
            let (ahead, behind) = if upstream.is_some() {
                git_ahead_behind(&entry.path)?
            } else {
                (0, 0)
            };

            println!("Project: {}", entry.name);
            println!("Path: {}", entry.path.display());
            println!("Branch: {branch}");
            println!(
                "Upstream: {}",
                upstream.unwrap_or_else(|| "(not configured)".to_string())
            );
            println!(
                "Remote: {}",
                remote_url.unwrap_or_else(|| "(origin not configured)".to_string())
            );
            println!("Ahead/Behind: +{ahead} / -{behind}");
            println!(
                "Changes: {} staged, {} unstaged, {} untracked",
                changes.staged, changes.unstaged, changes.untracked
            );
        }
        ProjectAction::Import { root, recursive } => {
            let scan_root = root
                .clone()
                .unwrap_or(std::env::current_dir().context("reading current directory")?);
            let repos = find_git_repos(&scan_root, *recursive)?;
            if repos.is_empty() {
                println!("No git repositories found under {}", scan_root.display());
                return Ok(());
            }

            let mut added = 0usize;
            let mut updated = 0usize;
            for repo_path in repos {
                let outcome = store.add(None, Some(repo_path), None, None)?;
                match outcome {
                    projects::AddOutcome::Added(entry) => {
                        println!("Added '{}': {}", entry.name, entry.path.display());
                        added += 1;
                    }
                    projects::AddOutcome::Updated(entry) => {
                        println!("Updated '{}': {}", entry.name, entry.path.display());
                        updated += 1;
                    }
                }
            }
            store.save()?;
            println!("Import complete: {added} added, {updated} updated.");
        }
        ProjectAction::Prune => {
            let before = store.projects.len();
            store.projects.retain(|p| p.path.exists());
            let removed = before.saturating_sub(store.projects.len());
            if removed > 0 {
                store.save()?;
            }
            println!("Pruned {removed} missing project link(s).");
        }
        ProjectAction::Exec { target, command } => {
            let entry = store.find(target).with_context(|| {
                format!("project '{target}' not found. Run `harness project list`.")
            })?;
            let program = &command[0];
            let args = &command[1..];
            let status = std::process::Command::new(program)
                .args(args)
                .current_dir(&entry.path)
                .status()
                .with_context(|| format!("running command in {}", entry.path.display()))?;
            if !status.success() {
                anyhow::bail!("command failed with status {status}");
            }
        }
        ProjectAction::Publish {
            target,
            repo,
            remote,
            public,
            private: _,
            push,
        } => {
            let entry = store.find(target).with_context(|| {
                format!("project '{target}' not found. Run `harness project list`.")
            })?;
            let repo_name = repo.clone().unwrap_or_else(|| entry.name.clone());

            let mut cmd = std::process::Command::new("gh");
            cmd.current_dir(&entry.path)
                .args(["repo", "create"])
                .arg(&repo_name)
                .args(["--source", ".", "--remote"])
                .arg(remote);
            if *public {
                cmd.arg("--public");
            } else {
                cmd.arg("--private");
            }
            if *push {
                cmd.arg("--push");
            }
            let status = cmd.status().context("running gh repo create")?;
            if !status.success() {
                anyhow::bail!("gh repo create failed with status {status}");
            }

            let _ = store.add(
                Some(entry.name.clone()),
                Some(entry.path.clone()),
                Some(repo_name.clone()),
                current_git_branch(&entry.path).or(entry.default_branch.clone()),
            )?;
            store.save()?;
            println!("Published '{}' to GitHub repo '{}'", entry.name, repo_name);
            println!("  path: {}", entry.path.display());
            println!("  remote: {remote}");
        }
        ProjectAction::Open { target, run } => {
            let entry = store.find(target).with_context(|| {
                format!("project '{target}' not found. Run `harness project list`.")
            })?;

            if *run {
                let exe = std::env::current_exe().context("resolving harness executable path")?;
                let status = std::process::Command::new(exe)
                    .current_dir(&entry.path)
                    .status()
                    .context("starting harness in project directory")?;
                if !status.success() {
                    anyhow::bail!("harness exited with status {status}");
                }
            } else {
                println!("{}", entry.path.display());
            }
        }
    }

    Ok(())
}

fn infer_clone_directory(repo: &str) -> String {
    let trimmed = repo.trim_end_matches('/');
    let last = trimmed.rsplit('/').next().unwrap_or(trimmed);
    let without_git = last.strip_suffix(".git").unwrap_or(last);
    if without_git.is_empty() {
        "repo".to_string()
    } else {
        without_git.to_string()
    }
}

fn init_git_repo(project_dir: &std::path::Path, default_branch: &str) -> Result<()> {
    let init_with_branch = std::process::Command::new("git")
        .current_dir(project_dir)
        .args(["init", "-b", default_branch])
        .status()
        .context("running git init -b")?;
    if init_with_branch.success() {
        return Ok(());
    }

    let init_basic = std::process::Command::new("git")
        .current_dir(project_dir)
        .arg("init")
        .status()
        .context("running git init")?;
    if !init_basic.success() {
        anyhow::bail!("git init failed with status {init_basic}");
    }
    let checkout = std::process::Command::new("git")
        .current_dir(project_dir)
        .args(["checkout", "-b", default_branch])
        .status()
        .context("running git checkout -b")?;
    if !checkout.success() {
        anyhow::bail!("git checkout -b failed with status {checkout}");
    }

    Ok(())
}

fn sync_project(entry: &projects::ProjectEntry) -> Result<()> {
    let fetch_status = std::process::Command::new("git")
        .current_dir(&entry.path)
        .args(["fetch", "--all", "--prune"])
        .status()
        .context("running git fetch --all --prune")?;
    if !fetch_status.success() {
        anyhow::bail!("git fetch failed with status {fetch_status}");
    }

    let mut pull_cmd = std::process::Command::new("git");
    pull_cmd
        .current_dir(&entry.path)
        .args(["pull", "--ff-only"]);
    if let Some(branch) = &entry.default_branch {
        pull_cmd.arg("origin").arg(branch);
    }
    let pull_status = pull_cmd.status().context("running git pull --ff-only")?;
    if !pull_status.success() {
        anyhow::bail!("git pull failed with status {pull_status}");
    }

    Ok(())
}

fn find_git_repos(root: &std::path::Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut repos = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(root.to_path_buf());

    while let Some(dir) = queue.pop_front() {
        if dir.join(".git").exists() {
            repos.push(dir.clone());
            // If this is already a git repo, do not recurse into children.
            continue;
        }

        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("reading directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name == ".git" {
                    continue;
                }
            }
            if recursive {
                queue.push_back(path);
            } else if path.join(".git").exists() {
                repos.push(path);
            }
        }
    }

    repos.sort();
    repos.dedup();
    Ok(repos)
}

fn current_git_branch(path: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .current_dir(path)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

#[derive(Debug, Default)]
struct ChangeCounts {
    staged: usize,
    unstaged: usize,
    untracked: usize,
}

#[derive(Debug)]
struct ProjectHealthRow {
    branch: String,
    ahead: u64,
    behind: u64,
    changes: ChangeCounts,
    status: String,
}

fn project_health_row(path: &std::path::Path) -> Result<ProjectHealthRow> {
    let branch = current_git_branch(path).unwrap_or_else(|| "(detached HEAD)".to_string());
    let upstream = git_output(
        path,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .ok();
    let changes = collect_change_counts(path)?;
    let (ahead, behind) = if upstream.is_some() {
        git_ahead_behind(path)?
    } else {
        (0, 0)
    };

    let dirty = changes.staged + changes.unstaged + changes.untracked;
    let status = if dirty == 0 && ahead == 0 && behind == 0 {
        "clean".to_string()
    } else if behind > 0 && ahead == 0 {
        "behind".to_string()
    } else if ahead > 0 && behind == 0 {
        "ahead".to_string()
    } else if ahead > 0 && behind > 0 {
        "diverged".to_string()
    } else {
        "dirty".to_string()
    };

    Ok(ProjectHealthRow {
        branch,
        ahead,
        behind,
        changes,
        status,
    })
}

fn collect_change_counts(path: &std::path::Path) -> Result<ChangeCounts> {
    let out = git_output(path, &["status", "--porcelain"])?;
    let mut counts = ChangeCounts::default();
    for line in out.lines() {
        if line.starts_with("?? ") {
            counts.untracked += 1;
            continue;
        }
        let bytes = line.as_bytes();
        if bytes.len() < 2 {
            continue;
        }
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        if x != ' ' && x != '?' {
            counts.staged += 1;
        }
        if y != ' ' && y != '?' {
            counts.unstaged += 1;
        }
    }
    Ok(counts)
}

fn git_output(path: &std::path::Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_ahead_behind(path: &std::path::Path) -> Result<(u64, u64)> {
    let out = git_output(
        path,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    )?;
    let mut parts = out.split_whitespace();
    let ahead = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
    let behind = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
    Ok((ahead, behind))
}

fn status_badge(status: &str) -> &'static str {
    match status {
        "clean" => "OK",
        "ahead" => "AHEAD",
        "behind" => "BEHIND",
        "diverged" => "DIVERGED",
        "dirty" => "DIRTY",
        _ => "UNKNOWN",
    }
}

fn delete_session(store: &harness_memory::SessionStore, id: &str) -> Result<()> {
    if store.delete(id)? {
        println!("Deleted session: {id}");
    } else {
        println!("Session not found: {id}");
    }
    Ok(())
}
