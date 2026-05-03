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
mod provider_build;
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
use harness_tools::registry::Tool;
use harness_tools::tools::GhTool;
use std::sync::Arc;
use tracing_subscriber::{fmt, EnvFilter};

use cli::{
    build_prompt_with_image, build_tools, connect_to_server, delete_session, export_session,
    graceful_ambient_shutdown, handle_doctor_command, handle_models_command,
    handle_project_command, list_sessions, run_init, run_self_dev, run_status,
};
use cli::{CheckpointAction, Cli, Commands, CostAction, SwarmAction, SyncAction};

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
        .clone()
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

    let provider: ArcProvider = provider_build::build_arc_provider(&cfg, cli.model.as_deref())?;

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
            let cfg_for_serve = cfg.clone();
            let inner = server::ServeRuntimeState {
                provider,
                tools,
                model: model.clone(),
                system_prompt: cfg_for_serve
                    .agent
                    .system_prompt
                    .clone()
                    .unwrap_or_else(|| agent::DEFAULT_SYSTEM.to_string()),
                config: cfg_for_serve,
            };
            let state = server::ServerState {
                inner: Arc::new(tokio::sync::RwLock::new(inner)),
                session_store: Arc::new(session_store),
                memory_store: memory_store.map(Arc::new),
                embed_model,
                browser_enabled,
                browser_url,
                config_active_path: Arc::new(config::active_config_toml_path()),
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
