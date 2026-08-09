//! Fast CLI dispatch for commands that do not need provider, tools, or LSP/MCP init.

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::generate;
use harness_tools::registry::Tool;
use harness_tools::tools::GhTool;

use crate::cli::args::BridgeAction;
use crate::cli::{
    delete_session, export_session, handle_doctor_command, handle_models_command,
    handle_project_command, list_sessions, run_init, run_status, run_update, CheckpointAction, Cli,
    Commands, CostAction, McpAction, SwarmAction, SyncAction,
};
use crate::config::Config;
use crate::provider_build;

pub async fn dispatch_lightweight(cli: &Cli, cfg: &Config) -> Result<()> {
    match &cli.command {
        Some(Commands::Update) => run_update()?,

        Some(Commands::Setup { force }) => {
            crate::cli::run_setup_interactive(cfg, *force)?;
        }

        Some(Commands::Project { action }) => handle_project_command(action)?,

        Some(Commands::Sessions) => {
            let store = open_session_store(cfg)?;
            list_sessions(&store)?;
        }

        Some(Commands::Export { id, output }) => {
            let store = open_session_store(cfg)?;
            export_session(&store, id, output.as_deref())?;
        }

        Some(Commands::Delete { id }) => {
            let store = open_session_store(cfg)?;
            delete_session(&store, id)?;
        }

        Some(Commands::Init { project, force }) => run_init(*project, *force)?,

        Some(Commands::Status) => {
            let store = open_session_store(cfg)?;
            let model = provider_build::resolved_model(cfg, cli.model.as_deref());
            run_status(cfg, &model, &store)?;
        }

        Some(Commands::Doctor) => handle_doctor_command(cfg).await,

        Some(Commands::Bench { pack, json }) => crate::bench::dispatch_bench(pack.clone(), *json)?,

        Some(Commands::Completions { shell }) => {
            let mut cmd = Cli::command();
            let bin_name = cmd.get_name().to_string();
            generate(*shell, &mut cmd, bin_name, &mut std::io::stdout());
        }

        Some(Commands::Models { set }) => handle_models_command(set.clone(), cfg).await?,

        Some(Commands::Trust { tool, pattern }) => {
            let mut store = crate::trust::TrustStore::load();
            if store.add_rule(tool, pattern) {
                store.save()?;
                println!("Trust rule added: {tool} / {pattern}");
            } else {
                println!("Rule already exists: {tool} / {pattern}");
            }
        }

        Some(Commands::Untrust { tool, pattern }) => {
            let mut store = crate::trust::TrustStore::load();
            if store.remove_rule(tool, pattern) {
                store.save()?;
                println!("Trust rule removed: {tool} / {pattern}");
            } else {
                println!("No matching rule: {tool} / {pattern}");
            }
        }

        Some(Commands::TrustList) => {
            let store = crate::trust::TrustStore::load();
            let rules = store.list();
            if rules.is_empty() {
                println!("No trust rules. Use `harness trust <tool> <pattern>` to add one.");
            } else {
                println!("{:<20} {:<40} ADDED", "TOOL", "PATTERN");
                for rule in rules {
                    println!("{:<20} {:<40} {}", rule.tool, rule.pattern, rule.added);
                }
            }
        }

        Some(Commands::DaemonStatus) => dispatch_daemon_status().await?,

        Some(Commands::RunBg { prompt }) => match crate::background::spawn(prompt) {
            Ok(id) => {
                println!("Background run started: {id}");
                println!("Output: ~/.harness/runs/{id}/output.log");
                println!("Status: harness runs");
            }
            Err(e) => eprintln!("run-bg: {e}"),
        },

        Some(Commands::Runs) => dispatch_runs()?,

        Some(Commands::Undo) => match crate::checkpoint::undo() {
            Ok(msg) => println!("{msg}"),
            Err(e) => eprintln!("undo: {e}"),
        },

        Some(Commands::Checkpoint {
            action: CheckpointAction::List,
        }) => dispatch_checkpoint_list()?,

        Some(Commands::Voice {
            duration,
            send,
            realtime,
        }) if !*send && !*realtime => {
            dispatch_voice_record(*duration).await?;
        }

        Some(Commands::Swarm { action }) => dispatch_swarm_readonly(action, cfg).await?,

        Some(Commands::Bridge { action }) => dispatch_bridge(action, cfg).await?,

        Some(Commands::Mcp { action }) => dispatch_mcp(action, cfg).await?,

        Some(Commands::Trace { id }) => dispatch_trace(id.as_deref())?,

        Some(Commands::Sync { action }) => dispatch_sync(action).await?,

        Some(Commands::Cost { action }) => dispatch_cost(action).await?,

        Some(Commands::Memorize { topic, fact }) => {
            match crate::memory_project::remember(topic, fact) {
                Ok(path) => println!("Remembered under '{topic}': {}", path.display()),
                Err(e) => eprintln!("Error saving memory: {e}"),
            }
        }

        Some(Commands::Forget { topic }) => match crate::memory_project::forget(topic) {
            Ok(true) => println!("Forgot topic '{topic}'"),
            Ok(false) => println!("No memory for topic '{topic}'"),
            Err(e) => eprintln!("Error: {e}"),
        },

        Some(Commands::Memories) => {
            let topics = crate::memory_project::list_topics();
            if topics.is_empty() {
                println!("No project memories. Use: harness memorize <topic> <fact>");
            } else {
                println!("{} project memory topic(s):", topics.len());
                for t in &topics {
                    println!("  • {t}");
                }
            }
        }

        Some(Commands::Pr { number, comment }) if comment.is_some() => {
            let body = comment
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("internal: pr comment missing body"))?;
            let out = GhTool
                .execute(serde_json::json!({
                    "action": "pr_comment",
                    "number": number,
                    "message": body,
                }))
                .await?;
            println!("{out}");
        }

        Some(Commands::Connect {
            url,
            prompt,
            session,
        }) => {
            crate::cli::connect_to_server(url, prompt, session.as_deref()).await?;
        }

        _ => anyhow::bail!("internal: command requires agent runtime"),
    }
    Ok(())
}

fn open_session_store(cfg: &Config) -> Result<harness_memory::SessionStore> {
    harness_memory::SessionStore::open(
        cfg.session
            .db_path
            .clone()
            .unwrap_or_else(harness_memory::SessionStore::default_path),
    )
}

async fn dispatch_daemon_status() -> Result<()> {
    if crate::daemon::is_running().await {
        match crate::daemon::send_request(&crate::daemon::DaemonRequest {
            id: 1,
            method: "status".into(),
            token: String::new(),
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
    Ok(())
}

fn dispatch_runs() -> Result<()> {
    let runs = crate::background::list(20)?;
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
    Ok(())
}

fn dispatch_checkpoint_list() -> Result<()> {
    let entries = crate::checkpoint::list()?;
    if entries.is_empty() {
        println!("No harness checkpoint stashes found.");
    } else {
        println!("{:<12} MESSAGE", "STASH");
        for (stash_ref, msg) in entries {
            println!("{:<12} {}", stash_ref, msg);
        }
    }
    Ok(())
}

async fn dispatch_voice_record(duration: u64) -> Result<()> {
    use harness_voice::{record_and_transcribe, WhisperBackend};
    use std::time::Duration;

    let openai_key = std::env::var("OPENAI_API_KEY").ok();
    let backend = WhisperBackend::detect(openai_key.as_deref());
    if !harness_voice::voice_available() && matches!(backend, WhisperBackend::Local { .. }) {
        eprintln!("Warning: no local audio recorder found. Install sox: brew install sox");
    }
    eprintln!("Recording for {duration}s… (speak now)");
    let transcript = record_and_transcribe(Duration::from_secs(duration), &backend).await?;
    println!("{transcript}");
    Ok(())
}

async fn dispatch_swarm_readonly(action: &SwarmAction, cfg: &Config) -> Result<()> {
    crate::swarm::configure(&cfg.swarm);
    match action {
        SwarmAction::List => crate::swarm::print_status()?,
        SwarmAction::Status { id, json } => match crate::swarm::get_task(id)? {
            Some(t) => {
                if *json {
                    crate::swarm::print_task_json(&t);
                } else {
                    crate::swarm::print_task_detail(&t);
                }
            }
            None => {
                if *json {
                    println!("{}", serde_json::json!({"error": "not_found", "id": id}));
                } else {
                    println!("Task {id} not found.");
                }
            }
        },
        SwarmAction::Result { id, json } => match crate::swarm::get_task(id)? {
            Some(t) => {
                if *json {
                    crate::swarm::print_task_json(&t);
                } else {
                    match t.result.as_deref() {
                        Some(r) if !r.is_empty() => println!("{r}"),
                        _ => println!(
                            "(no result yet — status: {})",
                            crate::swarm::status_label(&t.status)
                        ),
                    }
                }
            }
            None => {
                if *json {
                    println!("{}", serde_json::json!({"error": "not_found", "id": id}));
                } else {
                    println!("Task {id} not found.");
                }
            }
        },
        SwarmAction::Cancel { id, all } => {
            if *all {
                let n = crate::swarm::cancel_all_tasks()?;
                println!("Cancelled {n} task(s).");
            } else if let Some(id) = id {
                if crate::swarm::cancel_task(id)? {
                    println!("Cancelled task {id}.");
                } else {
                    println!("Task {id} not found or already finished.");
                }
            } else {
                anyhow::bail!("specify a task id or pass --all");
            }
        }
        SwarmAction::Wait { id, timeout_secs } => {
            use std::time::Duration;
            match crate::swarm::wait_task(id, Some(Duration::from_secs(*timeout_secs))).await? {
                Some(t) => {
                    crate::swarm::print_task_detail(&t);
                    if matches!(t.status, crate::swarm::TaskStatus::Done) {
                        crate::notifications::swarm_complete(&cfg.notifications, 1, 0);
                    } else if matches!(t.status, crate::swarm::TaskStatus::Failed(_)) {
                        crate::notifications::swarm_complete(&cfg.notifications, 1, 1);
                    }
                }
                None => println!("Task {id} not found."),
            }
        }
        SwarmAction::Gc {
            stale_secs,
            keep,
            older_than_secs,
            dry_run,
        } => {
            let opts = crate::swarm::GcOptions {
                stale_secs: *stale_secs,
                keep_terminal: *keep,
                older_than_secs: *older_than_secs,
                dry_run: *dry_run,
            };
            let report = crate::swarm::gc(&opts)?;
            if *dry_run {
                println!("dry-run: {}", report.summary());
            } else {
                println!("{}", report.summary());
            }
            for (id, reason) in &report.reaped {
                println!("  reaped {id}: {reason}");
            }
            if report.deleted > 0 {
                println!("  deleted {} row(s)", report.deleted);
            }
            if report.reaped.is_empty() && report.deleted == 0 {
                println!("Nothing to clean.");
            }
        }
        SwarmAction::Run { .. } => anyhow::bail!("internal: swarm run requires agent runtime"),
    }
    Ok(())
}

async fn dispatch_bridge(action: &BridgeAction, cfg: &Config) -> Result<()> {
    use BridgeAction::*;
    match action {
        Obsidian { title, content } => {
            let body = if content == "-" {
                use std::io::Read;
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                s
            } else {
                content.clone()
            };
            crate::bridges::obsidian_write(&cfg.bridges.obsidian, title, &body).await?;
            println!("Obsidian note queued: {title}");
        }
        Notes { title, content } => {
            crate::bridges::notes_write(&cfg.bridges.notes, title, content).await?;
            println!("Apple Note created: {title}");
        }
        CalendarList { date } => {
            let events = crate::bridges::calendar_query(&cfg.bridges.calendar, date).await?;
            if events.is_empty() {
                println!("No events on {date}.");
            } else {
                for e in events {
                    println!("{e}");
                }
            }
        }
        CalendarCreate { title, start, end } => {
            crate::bridges::calendar_create_event(&cfg.bridges.calendar, title, start, end).await?;
            println!("Calendar event created: {title}");
        }
        GithubProject => {
            let items = crate::bridges::github_project_list(&cfg.bridges.github_projects).await?;
            if items.is_empty() {
                println!("No project items (or bridge disabled / misconfigured).");
            } else {
                for item in items {
                    println!("{item}");
                }
            }
        }
    }
    Ok(())
}

async fn dispatch_mcp(action: &McpAction, cfg: &Config) -> Result<()> {
    match action {
        McpAction::Roots => {
            let roots = harness_mcp::collect_roots();
            if roots.is_empty() {
                println!("No workspace roots advertised (could not resolve CWD or home).");
            } else {
                println!(
                    "MCP roots harness advertises (initialize / notifications/roots/list_changed):"
                );
                for r in &roots {
                    let uri = r.get("uri").and_then(|v| v.as_str()).unwrap_or("?");
                    let name = r.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    println!("  {name:<24} {uri}");
                }
            }
        }
        McpAction::Resources { server } => {
            let Some(path) = resolve_mcp_config_path(cfg) else {
                print_mcp_config_missing();
                return Ok(());
            };
            let servers = load_mcp_servers(&path, cfg, server.as_deref())?;
            if servers.is_empty() {
                println!(
                    "No MCP servers to query (empty mcp.json, filtered by allowlist, or --server not found)."
                );
                println!("Config: {}", path.display());
                return Ok(());
            }

            let mut any = false;
            for (name, server_cfg) in &servers {
                match spawn_mcp_client(name, server_cfg).await {
                    Ok(client) => {
                        let caps = client.capabilities.lock().await.clone();
                        if !caps.has_resources {
                            println!("{name}: (no resources capability)");
                            continue;
                        }
                        match client.list_resources().await {
                            Ok(resources) if resources.is_empty() => {
                                println!("{name}: (no resources listed)");
                            }
                            Ok(resources) => {
                                any = true;
                                println!(
                                    "{name}  (protocol={}) — {} resource(s):",
                                    caps.protocol_version,
                                    resources.len()
                                );
                                for r in resources {
                                    let mime = r.mime_type.as_deref().unwrap_or("-");
                                    let desc = r.description.as_deref().unwrap_or("");
                                    if desc.is_empty() {
                                        println!("  {:<40} {:<28} {}", r.uri, r.name, mime);
                                    } else {
                                        println!(
                                            "  {:<40} {:<28} {}  {}",
                                            r.uri, r.name, mime, desc
                                        );
                                    }
                                }
                            }
                            Err(e) => eprintln!("{name}: resources/list failed: {e}"),
                        }
                    }
                    Err(e) => eprintln!("{name}: failed to spawn/connect: {e}"),
                }
            }
            if !any {
                println!("No resources found across configured MCP servers.");
                println!("Tip: servers must advertise the resources capability.");
            }
        }
        McpAction::Read { uri, server } => {
            let Some(path) = resolve_mcp_config_path(cfg) else {
                print_mcp_config_missing();
                return Ok(());
            };
            let servers = load_mcp_servers(&path, cfg, server.as_deref())?;
            if servers.is_empty() {
                println!(
                    "No MCP servers to query (empty mcp.json, filtered by allowlist, or --server not found)."
                );
                println!("Config: {}", path.display());
                return Ok(());
            }

            let mut last_err: Option<String> = None;
            for (name, server_cfg) in &servers {
                match spawn_mcp_client(name, server_cfg).await {
                    Ok(client) => {
                        let caps = client.capabilities.lock().await.clone();
                        if !caps.has_resources {
                            continue;
                        }
                        match client.read_resource(uri).await {
                            Ok(text) => {
                                println!("# {name} — {uri}");
                                println!("{text}");
                                return Ok(());
                            }
                            Err(e) => {
                                last_err = Some(format!("{name}: {e}"));
                            }
                        }
                    }
                    Err(e) => {
                        last_err = Some(format!("{name}: spawn failed: {e}"));
                    }
                }
            }
            println!("Could not read resource: {uri}");
            if let Some(e) = last_err {
                println!("Last error: {e}");
            } else {
                println!("No configured server advertises the resources capability.");
            }
        }
    }
    Ok(())
}

fn print_mcp_config_missing() {
    println!("No MCP config found.");
    println!("Create `.harness/mcp.json`, `.claude/mcp.json`, or `~/.harness/mcp.json`");
    println!("(Claude Code–compatible `mcpServers` map), or set `[mcp] config_path` in config.");
    println!("Example:");
    println!(
        r#"  {{"mcpServers":{{"filesystem":{{"command":"npx","args":["-y","@modelcontextprotocol/server-filesystem","."]}}}}}}"#
    );
}

/// Load servers from mcp.json, applying command allowlist and optional name filter.
fn load_mcp_servers(
    path: &std::path::Path,
    cfg: &Config,
    server_filter: Option<&str>,
) -> Result<Vec<(String, harness_mcp::McpServerConfig)>> {
    let mcp_cfg = harness_mcp::config::load(path)
        .with_context(|| format!("loading MCP config {}", path.display()))?;
    let allowlist = cfg.mcp.command_allowlist.as_deref();
    let mut out = Vec::new();
    for (name, server_cfg) in mcp_cfg.mcp_servers {
        if let Some(want) = server_filter {
            if name != want {
                continue;
            }
        }
        if !mcp_command_allowed(&server_cfg.command, allowlist) {
            eprintln!(
                "skipping `{name}`: command `{}` not in MCP allowlist (see [mcp] command_allowlist)",
                server_cfg.command
            );
            continue;
        }
        out.push((name, server_cfg));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Mirror harness-mcp default allowlist when config leave it unset.
pub(crate) fn mcp_command_allowed(command: &str, allowlist: Option<&[String]>) -> bool {
    const DEFAULT: &[&str] = &["npx", "node", "python3", "uvx"];
    let effective: Vec<&str> = match allowlist {
        None => DEFAULT.to_vec(),
        Some([]) => return true,
        Some(list) => list.iter().map(String::as_str).collect(),
    };
    let cmd = std::path::Path::new(command)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(command);
    effective
        .iter()
        .any(|allowed| *allowed == command || *allowed == cmd)
}

/// Prefer explicit config path when it exists; else discover default mcp.json.
pub(crate) fn resolve_mcp_config_path_in(
    config_path: Option<&std::path::Path>,
    discovered: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    if let Some(p) = config_path {
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    discovered
}

fn resolve_mcp_config_path(cfg: &Config) -> Option<std::path::PathBuf> {
    resolve_mcp_config_path_in(cfg.mcp.config_path.as_deref(), harness_mcp::find_config())
}

async fn spawn_mcp_client(
    name: &str,
    server_cfg: &harness_mcp::McpServerConfig,
) -> Result<harness_mcp::McpClient> {
    use std::time::Duration;
    let fut = harness_mcp::McpClient::spawn(name, server_cfg);
    match tokio::time::timeout(Duration::from_secs(15), fut).await {
        Ok(Ok(client)) => Ok(client),
        Ok(Err(e)) => Err(e),
        Err(_) => anyhow::bail!("MCP server `{name}` spawn timed out after 15s"),
    }
}

fn dispatch_trace(id: Option<&str>) -> Result<()> {
    match id {
        Some(trace_id) => crate::observability::export_trace(trace_id)?,
        None => {
            let spans = crate::observability::load_last_trace()?;
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
    Ok(())
}

async fn dispatch_sync(action: &SyncAction) -> Result<()> {
    match action {
        SyncAction::Init { git_url } => crate::sync::init(git_url).await?,
        SyncAction::Push => crate::sync::push().await?,
        SyncAction::Pull => crate::sync::pull().await?,
        SyncAction::Status => crate::sync::status().await?,
        SyncAction::Auth => {
            println!("Sync passphrase is stored in the system keychain under 'harness-sync'.");
            println!("To transfer to another machine, run: harness sync init <git-url>");
            println!("Then on the new machine, run: harness sync pull");
            println!("The passphrase will be regenerated and stored on the new machine.");
        }
    }
    Ok(())
}

async fn dispatch_cost(action: &CostAction) -> Result<()> {
    use crate::cost_db::{days_ago, format_usd, CostDb};
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
                print!("\x1B[2J\x1B[H");
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn mcp_command_allowed_defaults_and_empty_allow_all() {
        assert!(mcp_command_allowed("npx", None));
        assert!(mcp_command_allowed("/usr/bin/node", None));
        assert!(mcp_command_allowed("python3", None));
        assert!(mcp_command_allowed("uvx", None));
        assert!(!mcp_command_allowed("bash", None));
        assert!(!mcp_command_allowed("curl", None));

        let empty: &[String] = &[];
        assert!(mcp_command_allowed("anything", Some(empty)));

        let custom = vec!["docker".into(), "cargo".into()];
        assert!(mcp_command_allowed("docker", Some(&custom)));
        assert!(mcp_command_allowed("/bin/cargo", Some(&custom)));
        assert!(!mcp_command_allowed("npx", Some(&custom)));
    }

    #[test]
    fn mcp_command_allowed_path_basename_and_full_match_edges() {
        // Full path matches default by basename only.
        assert!(mcp_command_allowed("/opt/homebrew/bin/npx", None));
        assert!(mcp_command_allowed("/usr/local/bin/python3", None));
        // Relative path with separators — basename wins.
        assert!(mcp_command_allowed("./bin/node", None));
        assert!(!mcp_command_allowed("./bin/bash", None));
        // Empty / odd command strings never match defaults.
        assert!(!mcp_command_allowed("", None));
        assert!(!mcp_command_allowed("NPX", None)); // case-sensitive
        assert!(!mcp_command_allowed("npx.exe", None));
        // Allowlist may match full command path OR basename.
        let full = vec!["/opt/custom/bin/my-mcp".into()];
        assert!(mcp_command_allowed("/opt/custom/bin/my-mcp", Some(&full)));
        assert!(!mcp_command_allowed("my-mcp", Some(&full))); // basename alone not listed
        let base = vec!["my-mcp".into()];
        assert!(mcp_command_allowed("/opt/custom/bin/my-mcp", Some(&base)));
        assert!(mcp_command_allowed("my-mcp", Some(&base)));
        // Single-entry allowlist does not widen to defaults.
        let only_npx = vec!["npx".into()];
        assert!(mcp_command_allowed("npx", Some(&only_npx)));
        assert!(!mcp_command_allowed("node", Some(&only_npx)));
        assert!(!mcp_command_allowed("uvx", Some(&only_npx)));
    }

    #[test]
    fn resolve_mcp_config_path_in_prefers_existing_explicit() {
        let dir = tempfile::tempdir().unwrap();
        let explicit = dir.path().join("mcp.json");
        fs::write(&explicit, "{}").unwrap();
        let discovered = Some(PathBuf::from("/tmp/discovered.json"));
        assert_eq!(
            resolve_mcp_config_path_in(Some(&explicit), discovered.clone()),
            Some(explicit)
        );

        let missing = dir.path().join("nope.json");
        assert_eq!(
            resolve_mcp_config_path_in(Some(&missing), discovered.clone()),
            discovered
        );
        assert_eq!(resolve_mcp_config_path_in(None, None), None);
    }

    #[test]
    fn resolve_mcp_config_path_in_edges_missing_discovered_and_none_explicit() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("absent.json");
        // Explicit missing + no discovery → None (do not invent paths).
        assert_eq!(resolve_mcp_config_path_in(Some(&missing), None), None);
        // No explicit → pass through discovery (even if file does not exist).
        let fake = PathBuf::from("/does/not/exist/mcp.json");
        assert_eq!(
            resolve_mcp_config_path_in(None, Some(fake.clone())),
            Some(fake)
        );
        // Explicit that exists wins over discovery even when discovery is also real.
        let a = dir.path().join("a.json");
        let b = dir.path().join("b.json");
        fs::write(&a, "{}").unwrap();
        fs::write(&b, "{}").unwrap();
        assert_eq!(resolve_mcp_config_path_in(Some(&a), Some(b)), Some(a));
    }

    #[test]
    fn load_mcp_servers_filters_sorts_and_allowlist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        // Unsorted insertion order — output must be name-sorted.
        fs::write(
            &path,
            r#"{
              "mcpServers": {
                "zeta": { "command": "npx", "args": ["-y", "z"] },
                "alpha": { "command": "node", "args": ["a.js"] },
                "blocked": { "command": "bash", "args": ["-c", "echo"] },
                "mid": { "command": "/usr/bin/python3", "args": [] }
              }
            }"#,
        )
        .unwrap();

        let mut cfg = Config::default();
        cfg.mcp.command_allowlist = None; // defaults: npx/node/python3/uvx

        let all = load_mcp_servers(&path, &cfg, None).unwrap();
        let names: Vec<&str> = all.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["alpha", "mid", "zeta"]); // blocked skipped; sorted

        let only_mid = load_mcp_servers(&path, &cfg, Some("mid")).unwrap();
        assert_eq!(only_mid.len(), 1);
        assert_eq!(only_mid[0].0, "mid");
        assert_eq!(only_mid[0].1.command, "/usr/bin/python3");

        let missing = load_mcp_servers(&path, &cfg, Some("nope")).unwrap();
        assert!(missing.is_empty());

        // Empty allowlist = allow all (including bash).
        cfg.mcp.command_allowlist = Some(vec![]);
        let wide = load_mcp_servers(&path, &cfg, None).unwrap();
        assert_eq!(wide.len(), 4);
        assert!(wide.iter().any(|(n, _)| n == "blocked"));

        // Custom allowlist: only bash basename.
        cfg.mcp.command_allowlist = Some(vec!["bash".into()]);
        let bash_only = load_mcp_servers(&path, &cfg, None).unwrap();
        assert_eq!(bash_only.len(), 1);
        assert_eq!(bash_only[0].0, "blocked");
    }

    #[test]
    fn load_mcp_servers_missing_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-mcp.json");
        let cfg = Config::default();
        let err = load_mcp_servers(&missing, &cfg, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("loading MCP config") || msg.contains("no-such-mcp"),
            "unexpected err: {msg}"
        );
    }
}
