mod agent;
mod ambient;
mod auth_token;
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
mod rate_limit;
mod server;
mod swarm;
mod sync;
mod trust;
mod tui;

mod cli;

// mimalloc is linked but turso already sets the global allocator.
// We still benefit from mimalloc being in the dependency tree via turso.

use anyhow::{Context, Result};
use clap::Parser;
use harness_provider_core::ArcProvider;
use harness_tools::registry::Tool;
use harness_tools::tools::GhTool;
use std::sync::Arc;
use tracing_subscriber::{fmt, EnvFilter};

use cli::{
    build_prompt_with_image, build_tools, command_needs_agent_runtime, dispatch_lightweight,
    graceful_ambient_shutdown, maybe_run_first_time_wizard, run_self_dev,
};
use cli::{Cli, Commands, SwarmAction};

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

    let mut cfg = config::load(cli.config.as_deref())?;
    swarm::configure(&cfg.swarm);
    daemon::configure(&cfg.daemon);

    if !command_needs_agent_runtime(&cli) {
        return dispatch_lightweight(&cli, &cfg).await;
    }

    maybe_run_first_time_wizard(&cfg)?;

    // Detect available API keys (router priority: anthropic > xai > openai > ollama > mlx)
    let has_xai = cfg.provider.api_key.is_some()
        || std::env::var("XAI_API_KEY")
            .map(|k| !k.is_empty())
            .unwrap_or(false);
    let has_anthropic = std::env::var("ANTHROPIC_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    let has_openai = std::env::var("OPENAI_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    let has_ollama = cfg.providers.contains_key("ollama");

    cfg = config::load(cli.config.as_deref())?;
    swarm::configure(&cfg.swarm);
    daemon::configure(&cfg.daemon);

    let router_default = cfg.router.default.as_deref().unwrap_or("xai");
    if !has_xai && router_default == "xai" && (has_anthropic || has_openai || has_ollama) {
        eprintln!("Note: xAI key not found. Using another configured provider.");
    }

    let model = provider_build::resolved_model(&cfg, cli.model.as_deref());

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
            if cfg.ambient.is_enabled() {
                let mem_arc = std::sync::Arc::new(mem.clone());
                match crate::provider_build::build_ambient_providers(&cfg, cli.model.as_deref()) {
                    Ok(providers) => {
                        let mut ambient_cfg = ambient::AmbientConfig::from_section(&cfg.ambient);
                        if ambient_cfg.consolidation_model.is_none() {
                            if let Ok(router) = provider_build::build_router(&cfg) {
                                if let Some(fast) = router.fast_model_id() {
                                    ambient_cfg.consolidation_model = Some(fast.to_string());
                                }
                            }
                        }
                        Some(ambient::spawn_with_config(
                            providers,
                            mem_arc,
                            em.clone(),
                            ambient_cfg,
                        ))
                    }
                    Err(e) => {
                        tracing::warn!("ambient: failed to build providers, skipping: {e}");
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

    // CLI --browser flag overrides config; config.browser.enabled is the opt-in default.
    let browser_enabled = cli.browser || cfg.browser.enabled.unwrap_or(false);
    let browser_url = cfg.browser.url.clone().unwrap_or(cli.browser_url);

    // Plan/approve mode: CLI flag or `[approval].mode = "plan" | "smart"`.
    let approval_mode = cfg.approval.effective_mode();
    let confirm_active = cli.plan || approval_mode == "plan" || approval_mode == "smart";
    let interactive_tui = cli.command.is_none() && cli.prompt.is_none();
    let (confirm_gate, confirm_rx) = if confirm_active && interactive_tui {
        let (gate, rx) = harness_tools::confirm::channel();
        (Some(gate), Some(rx))
    } else {
        (None, None)
    };
    let confirm_bar_label = if confirm_active && interactive_tui {
        Some(if cli.plan || approval_mode == "plan" {
            "PLAN"
        } else {
            "SMART"
        })
    } else {
        None
    };

    // Build tools (including MCP servers if config exists).
    let tools = build_tools(
        provider.clone(),
        model.clone(),
        &cfg,
        browser_enabled,
        &browser_url,
        memory_store.clone(),
        embed_model.clone(),
        confirm_gate,
    )
    .await?;

    let run_opts = agent::RunOnceOptions::from_config(&cfg, cli.think);

    match cli.command {
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
                run_opts.clone(),
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
                run_opts.clone(),
            )
            .await?;
        }

        Some(Commands::Serve { addr }) => {
            let addr: std::net::SocketAddr = addr.parse().context("invalid address")?;
            if !addr.ip().is_loopback() {
                tracing::warn!(
                    %addr,
                    "binding harness serve to a non-loopback address exposes the agent API — bearer token auth is required"
                );
            }
            let auth_token = auth_token::server_token()?;
            tracing::info!(
                "HTTP auth token loaded from ~/.harness/server.token (send as Authorization: Bearer <token>)"
            );
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
            let collab_registry = if cfg.collab.enabled {
                Some(crate::collab::new_registry())
            } else {
                None
            };
            let state = server::ServerState {
                inner: Arc::new(tokio::sync::RwLock::new(inner)),
                session_store: Arc::new(session_store),
                memory_store: memory_store.map(Arc::new),
                embed_model,
                browser_enabled,
                browser_url,
                config_active_path: Arc::new(config::active_config_toml_path()),
                auth_token,
                collab: collab_registry,
            };
            server::serve(state, addr).await?;
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
                    if let Err(e) = res {
                        notifications::daemon_died(&cfg.notifications);
                        eprintln!("daemon: {e}");
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\nDaemon stopped.");
                    let _ = shutdown_tx.send(());
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
            if send {
                if !harness_voice::voice_available()
                    && matches!(backend, WhisperBackend::Local { .. })
                {
                    eprintln!(
                        "Warning: no local audio recorder found. Install sox: brew install sox"
                    );
                }
                eprintln!("Recording for {duration}s… (speak now)");
                let transcript =
                    record_and_transcribe(Duration::from_secs(duration), &backend).await?;
                println!("{transcript}");
                if !transcript.is_empty() {
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
                        run_opts.clone(),
                    )
                    .await?;
                    notifications::voice_response_done(&cfg.notifications);
                }
            }
            return Ok(());
        }

        Some(Commands::Swarm {
            action:
                SwarmAction::Run {
                    prompt,
                    model: run_model,
                    count,
                },
        }) => {
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
                            .find(|m| matches!(m.role, harness_provider_core::Role::Assistant))
                            .map(|m| m.content.as_str().to_string())
                            .unwrap_or_else(|| "(no response)".into());
                        Ok(reply)
                    }
                })
                .await;
            }
            println!("Queued {n} swarm task(s). Use: harness swarm list");
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
                    run_opts,
                )
                .await?;
            } else {
                let ambient_tx = ambient_shutdown.as_ref().map(|(tx, _)| tx.clone());
                let result = tui::run(
                    provider,
                    session_store,
                    memory_store,
                    embed_model,
                    tools,
                    model,
                    cfg,
                    cli.resume.as_deref(),
                    cli.think,
                    ambient_tx,
                    confirm_rx,
                    confirm_bar_label,
                )
                .await;
                graceful_ambient_shutdown(ambient_shutdown).await;
                result?;
                return Ok(());
            }
        }

        _ => {}
    }

    graceful_ambient_shutdown(ambient_shutdown).await;
    Ok(())
}
