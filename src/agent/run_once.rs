//! Non-interactive single-prompt entrypoint.

use anyhow::Result;
use harness_memory::{MemoryStore, Session, SessionStore};
use harness_provider_core::{ArcProvider, Message};
use harness_tools::ToolExecutor;

use crate::events::AgentEvent;

use super::drive::drive_agent_full;
use super::memory::store_turn_memory;
use super::naming::suggest_session_name;
use super::system::DEFAULT_SYSTEM;

/// Optional flags for non-interactive `run_once`.
#[derive(Debug, Clone, Default)]
pub struct RunOnceOptions {
    /// Extended thinking token budget.
    pub thinking_budget: Option<u32>,
    /// Enable provider-native web search.
    pub native_web_search: bool,
    /// Enable provider-native code execution.
    pub native_code_execution: bool,
    /// Enable provider-native X search (xAI).
    pub native_x_search: bool,
}

impl RunOnceOptions {
    /// Build options from harness config and optional CLI thinking budget.
    pub fn from_config(cfg: &crate::config::Config, thinking_budget: Option<u32>) -> Self {
        Self {
            thinking_budget,
            native_web_search: cfg.native_tools.web_search_enabled(),
            native_code_execution: cfg.native_tools.code_execution_enabled(),
            native_x_search: cfg.native_tools.x_search_enabled(),
        }
    }
}

/// Non-interactive single-prompt run. Prints events to stdout/stderr.
#[allow(clippy::too_many_arguments)]
pub async fn run_once(
    provider: &ArcProvider,
    store: &SessionStore,
    memory_store: Option<&MemoryStore>,
    embed_model: Option<&str>,
    tools: &ToolExecutor,
    model: &str,
    system_prompt: Option<&str>,
    prompt: &str,
    resume_id: Option<&str>,
    opts: RunOnceOptions,
) -> Result<()> {
    let mut session = match resume_id {
        Some(id) => store
            .find(id)?
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?,
        None => Session::new(model),
    };

    session.push(Message::user(prompt));

    let (tx, mut rx) = crate::events::channel();

    // Drive agent in the background so we can print events as they arrive.
    let provider2 = provider.clone();
    let tools2 = tools.clone();
    let mem2 = memory_store.cloned();
    let em2 = embed_model.map(|s| s.to_string());
    let sys = system_prompt.unwrap_or(DEFAULT_SYSTEM).to_string();

    let handle = tokio::spawn(async move {
        drive_agent_full(
            &provider2,
            &tools2,
            mem2.as_ref(),
            em2.as_deref(),
            &mut session,
            &sys,
            Some(&tx),
            opts.thinking_budget,
            opts.native_web_search,
            opts.native_code_execution,
            opts.native_x_search,
            None,
        )
        .await?;
        Ok::<Session, anyhow::Error>(session)
    });

    while let Some(event) = rx.recv().await {
        match &event {
            AgentEvent::TextChunk(s) => {
                print!("{s}");
                use std::io::Write;
                std::io::stdout().flush().ok();
            }
            AgentEvent::ToolStart { name, .. } => eprintln!("\n[→ {name}]"),
            AgentEvent::ToolResult { name, result, .. } => {
                let preview = &result[..result.len().min(100)];
                eprintln!("[← {name}] {preview}");
            }
            AgentEvent::MemoryRecall { count } => eprintln!("[memory] recalled {count} entries"),
            AgentEvent::ContextCompacted {
                messages_before,
                messages_after,
            } => eprintln!("[compact] {messages_before} → {messages_after} messages"),
            AgentEvent::SubAgentSpawned { task } => eprintln!("[swarm] spawning: {task}"),
            AgentEvent::SubAgentDone { task, .. } => eprintln!("[swarm] done: {task}"),
            AgentEvent::TokenUsage { input, output } => {
                eprintln!("[tokens] in={input} out={output}");
            }
            AgentEvent::CacheUsage { creation, read } => {
                if *read > 0 {
                    eprintln!("[cache] write={creation} read={read}");
                }
            }
            AgentEvent::Done | AgentEvent::Error(_) => {}
        }
    }

    println!();
    let mut final_session = handle.await??;

    if let Some(title) = suggest_session_name(provider, &final_session).await {
        final_session.name = Some(title.clone());
        let _ = store.set_name_if_missing(&final_session.id, &title)?;
    }
    store.save(&final_session)?;

    if let (Some(mem), Some(em)) = (memory_store, embed_model) {
        store_turn_memory(provider, mem, em, &final_session).await;
    }

    eprintln!("[session {}]", final_session.short_id());
    Ok(())
}
