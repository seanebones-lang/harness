//! Core agent loop: send → stream → execute tools → repeat.
//! Emits AgentEvents so callers (TUI or CLI) can display progress.

use anyhow::Result;
use futures::StreamExt;
use harness_memory::{MemoryStore, Session, SessionStore};
use harness_provider_core::{ChatRequest, Delta, DeltaStream, Message, Provider, Role, StopReason};
use harness_provider_xai::{XaiProvider, tool_calls_to_message};
use harness_tools::ToolExecutor;
use tracing::debug;

use crate::events::{AgentEvent, EventTx};

pub const DEFAULT_SYSTEM: &str = "\
You are a powerful coding assistant running in a terminal. \
You have access to tools to read and write files, run shell commands, and search code. \
Be concise and precise. Prefer making changes over explaining; show diffs when you edit files. \
Always verify your changes work by running relevant tests or build commands.";

/// Drives one full agentic turn (tool loop until EndTurn/MaxTokens).
/// Mutates `session` in place. Sends events through `tx` if provided.
pub async fn drive_agent(
    provider: &XaiProvider,
    tools: &ToolExecutor,
    memory_store: Option<&MemoryStore>,
    embed_model: Option<&str>,
    session: &mut Session,
    system_prompt: &str,
    tx: Option<&EventTx>,
) -> Result<()> {
    let emit = |event: AgentEvent| {
        if let Some(t) = tx {
            let _ = t.send(event);
        }
    };

    // Memory recall: embed the last user message and inject top-k relevant past exchanges.
    let augmented_system = build_augmented_system(
        provider, memory_store, embed_model, session, system_prompt, &emit,
    )
    .await;

    loop {
        let req = ChatRequest::new(&session.model)
            .with_messages(session.messages.clone())
            .with_tools(tools.registry().definitions())
            .with_system(&augmented_system);

        let mut stream: DeltaStream = provider.stream_chat(req).await?;

        let mut text_buf = String::new();
        let mut pending_tool_calls = Vec::new();
        let mut stop_reason = StopReason::EndTurn;

        while let Some(item) = stream.next().await {
            match item? {
                Delta::Text(chunk) => {
                    emit(AgentEvent::TextChunk(chunk.clone()));
                    text_buf.push_str(&chunk);
                }
                Delta::ToolCall(call) => {
                    pending_tool_calls.push(call);
                }
                Delta::Done { stop_reason: sr } => {
                    stop_reason = sr;
                }
            }
        }

        if !text_buf.is_empty() {
            session.push(Message::assistant(&text_buf));
        }

        if !pending_tool_calls.is_empty() {
            session.push(tool_calls_to_message(&pending_tool_calls));

            for call in &pending_tool_calls {
                debug!(tool = %call.function.name, id = %call.id, "executing tool");
                emit(AgentEvent::ToolStart {
                    name: call.function.name.clone(),
                    id: call.id.clone(),
                });
                let result = tools.execute(call).await;
                emit(AgentEvent::ToolResult {
                    name: call.function.name.clone(),
                    id: call.id.clone(),
                    result: result.clone(),
                });
                session.push(Message::tool_result(&call.id, result));
            }

            continue;
        }

        if matches!(stop_reason, StopReason::MaxTokens) {
            emit(AgentEvent::Error("hit max_tokens limit".into()));
        }
        break;
    }

    emit(AgentEvent::Done);
    Ok(())
}

/// Embed the last user message, retrieve top-k memories, and prepend them to the system prompt.
async fn build_augmented_system(
    provider: &XaiProvider,
    memory_store: Option<&MemoryStore>,
    embed_model: Option<&str>,
    session: &Session,
    system_prompt: &str,
    emit: &impl Fn(AgentEvent),
) -> String {
    let (Some(mem), Some(model)) = (memory_store, embed_model) else {
        return system_prompt.to_string();
    };

    let Some(last_user) = session.messages.iter().rev().find(|m| matches!(m.role, Role::User)) else {
        return system_prompt.to_string();
    };

    let user_text = last_user.content.as_str().to_string();
    let Ok(q_emb) = provider.embed(model, &user_text).await else {
        return system_prompt.to_string();
    };

    let Ok(memories) = mem.search(&q_emb, &session.id, 3) else {
        return system_prompt.to_string();
    };

    if memories.is_empty() {
        return system_prompt.to_string();
    }

    emit(AgentEvent::MemoryRecall { count: memories.len() });

    let mem_block = memories
        .iter()
        .map(|(m, score)| format!("[memory relevance={:.2}]\n{}", score, m.text))
        .collect::<Vec<_>>()
        .join("\n\n");

    format!("{system_prompt}\n\n## Relevant past context\n{mem_block}")
}

/// Store the most recent user↔assistant exchange as an embedded memory.
pub async fn store_turn_memory(
    provider: &XaiProvider,
    mem: &MemoryStore,
    embed_model: &str,
    session: &Session,
) {
    let mut user_text = None;
    let mut asst_text = None;

    for msg in session.messages.iter().rev() {
        match msg.role {
            Role::Assistant if asst_text.is_none() => {
                let t = msg.content.as_str();
                if !t.starts_with("__tool_calls__") {
                    asst_text = Some(t.to_string());
                }
            }
            Role::User if user_text.is_none() => {
                user_text = Some(msg.content.as_str().to_string());
            }
            _ => {}
        }
        if user_text.is_some() && asst_text.is_some() {
            break;
        }
    }

    if let (Some(u), Some(a)) = (user_text, asst_text) {
        let combined = format!("Q: {u}\nA: {a}");
        match provider.embed(embed_model, &combined).await {
            Ok(emb) => {
                if let Err(e) = mem.insert(&session.id, &combined, &emb) {
                    debug!("failed to store memory: {e}");
                }
            }
            Err(e) => debug!("failed to embed memory: {e}"),
        }
    }
}

/// Generate a short session name from the first user message.
/// Fires a quick non-streaming chat call; silently returns on failure.
/// Only runs when the session has no name and at least one assistant reply.
pub async fn auto_name_session(provider: &XaiProvider, session: &mut Session) {
    if session.name.is_some() {
        return;
    }

    let first_user = session
        .messages
        .iter()
        .find(|m| matches!(m.role, Role::User))
        .map(|m| m.content.as_str().to_string())
        .unwrap_or_default();

    if first_user.is_empty() {
        return;
    }

    let snippet = &first_user[..first_user.len().min(200)];
    let prompt = format!(
        "Summarise this task in 4 to 6 words. No punctuation, no quotes. \
         Reply with ONLY the title.\n\nTask: {snippet}"
    );

    let req = ChatRequest::new(&session.model)
        .with_messages(vec![Message::user(&prompt)]);

    let Ok(mut stream) = provider.stream_chat(req).await else { return };
    let mut title = String::new();
    while let Some(Ok(Delta::Text(chunk))) = stream.next().await {
        title.push_str(&chunk);
    }

    let title = title.trim().to_string();
    if !title.is_empty() && title.len() < 80 {
        session.name = Some(title);
    }
}

/// Non-interactive single-prompt run. Prints events to stdout/stderr.
pub async fn run_once(
    provider: &XaiProvider,
    store: &SessionStore,
    memory_store: Option<&MemoryStore>,
    embed_model: Option<&str>,
    tools: &ToolExecutor,
    model: &str,
    system_prompt: Option<&str>,
    prompt: &str,
    resume_id: Option<&str>,
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
    let mem2 = memory_store.map(|m| m.clone());
    let em2 = embed_model.map(|s| s.to_string());
    let sys = system_prompt.unwrap_or(DEFAULT_SYSTEM).to_string();

    let handle = tokio::spawn(async move {
        drive_agent(&provider2, &tools2, mem2.as_ref(), em2.as_deref(), &mut session, &sys, Some(&tx)).await?;
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
            AgentEvent::SubAgentSpawned { task } => eprintln!("[swarm] spawning: {task}"),
            AgentEvent::SubAgentDone { task, .. } => eprintln!("[swarm] done: {task}"),
            AgentEvent::Done | AgentEvent::Error(_) => {}
        }
    }

    println!();
    let mut final_session = handle.await??;

    auto_name_session(provider, &mut final_session).await;
    store.save(&final_session)?;

    if let (Some(mem), Some(em)) = (memory_store, embed_model) {
        store_turn_memory(provider, mem, em, &final_session).await;
    }

    eprintln!("[session {}]", final_session.short_id());
    Ok(())
}
