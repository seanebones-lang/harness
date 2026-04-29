//! Core agent loop: send → stream → execute tools → repeat.
//! Emits AgentEvents so callers (TUI or CLI) can display progress.

use anyhow::Result;
use futures::StreamExt;
use harness_memory::{MemoryStore, Session, SessionStore};
use harness_provider_core::{ArcProvider, ChatRequest, Delta, DeltaStream, Message, Role, StopReason};
use harness_provider_xai::tool_calls_to_message;
use harness_tools::ToolExecutor;
use tracing::debug;

use crate::events::{AgentEvent, EventTx};

pub const DEFAULT_SYSTEM: &str = "\
You are a powerful coding assistant running in a terminal.

Available tools:
  read_file, write_file     — read or overwrite files
  patch_file                — surgical old→new text replacement (prefer over write_file for edits)
  apply_patch               — apply a unified diff across multiple files atomically (prefer for multi-file edits)
  list_dir                  — list directory contents
  shell                     — run shell commands (build, test, etc.)
  git                       — typed git operations: status/diff/add/commit/branch/push/log/blame/restore
  test_runner               — run project tests with structured pass/fail output
  search_code               — regex search across the codebase
  spawn_agent               — run a sub-agent with base tools for parallel tasks
  browser (when enabled)    — Chrome CDP: navigate, screenshot, click, fill forms
  MCP tools (when loaded)   — any tools registered via .harness/mcp.json

Guidelines:
  - Prefer patch_file for single-file edits, apply_patch for multi-file changes.
  - Use the git tool for all git operations instead of shell git commands.
  - Always run test_runner after changes to verify correctness.
  - Be concise. Prefer making changes over explaining them.
  - When editing multiple files, use spawn_agent for parallelism.
  - In plan mode (--plan flag), destructive calls pause for user approval.";

/// Drives one full agentic turn (tool loop until EndTurn/MaxTokens).
/// Mutates `session` in place. Sends events through `tx` if provided.
pub async fn drive_agent(
    provider: &ArcProvider,
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

    // Auto-checkpoint: stash working tree once per turn before destructive tools.
    let turn_index = session.messages.len();
    let _checkpoint_taken = std::cell::Cell::new(false);

    // Memory recall: embed the last user message and inject top-k relevant past exchanges.
    let augmented_system = build_augmented_system(
        provider, memory_store, embed_model, session, system_prompt, &emit,
    )
    .await;

    loop {
        // Auto-compact context when approaching 70% of the model context window.
        maybe_compact(provider, session, 0.70).await;

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
                Delta::Usage { input_tokens, output_tokens } => {
                    emit(AgentEvent::TokenUsage { input: input_tokens, output: output_tokens });
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
            // Create a git checkpoint stash on the first destructive tool call of this turn.
            let has_destructive = pending_tool_calls.iter().any(|c| {
                matches!(c.function.name.as_str(), "write_file" | "patch_file" | "shell")
            });
            if has_destructive && !_checkpoint_taken.get() {
                _checkpoint_taken.set(true);
                let sid = session.id.chars().take(8).collect::<String>();
                if let Some(stash_name) = crate::checkpoint::create(&sid, turn_index) {
                    emit(AgentEvent::ToolStart {
                        name: "checkpoint".into(),
                        id: "checkpoint".into(),
                    });
                    emit(AgentEvent::ToolResult {
                        name: "checkpoint".into(),
                        id: "checkpoint".into(),
                        result: format!("Checkpoint created: {stash_name}"),
                    });
                }
            }

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

/// Load a project-specific system prompt prefix from well-known files in CWD.
/// Checks (in order): .harness/SYSTEM.md, AGENTS.md, CLAUDE.md
pub fn load_project_instructions() -> Option<String> {
    let candidates = [
        ".harness/SYSTEM.md",
        "AGENTS.md",
        "CLAUDE.md",
    ];
    for path in &candidates {
        if let Ok(text) = std::fs::read_to_string(path) {
            if !text.trim().is_empty() {
                tracing::debug!(file = path, "loaded project instructions");
                return Some(format!("## Project instructions (from {path})\n\n{text}"));
            }
        }
    }
    None
}

/// Embed the last user message, retrieve top-k memories, and prepend them to the system prompt.
async fn build_augmented_system(
    provider: &ArcProvider,
    memory_store: Option<&MemoryStore>,
    embed_model: Option<&str>,
    session: &Session,
    system_prompt: &str,
    emit: &impl Fn(AgentEvent),
) -> String {
    // Prepend project instructions if available.
    let base = if let Some(proj) = load_project_instructions() {
        format!("{system_prompt}\n\n{proj}")
    } else {
        system_prompt.to_string()
    };
    let system_prompt = base.as_str();

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

/// Return a rough token count for a slice of messages (character heuristic: 4 chars/token).
pub fn estimate_tokens(messages: &[harness_provider_core::Message]) -> usize {
    messages.iter().map(|m| m.content.as_str().len() / 4 + 1).sum()
}

/// Context compaction: when the session exceeds `threshold` fraction of the model context
/// window, summarise the oldest non-system messages and replace them with a compact block.
///
/// Uses the provider's fast model when available (falls back to the session model).
/// Context limit is approximated at 128k tokens for modern models.
pub async fn maybe_compact(
    provider: &ArcProvider,
    session: &mut Session,
    threshold: f32,
) {
    const CONTEXT_LIMIT: usize = 128_000;

    let total = estimate_tokens(&session.messages);
    if (total as f32) < CONTEXT_LIMIT as f32 * threshold {
        return;
    }

    tracing::debug!(tokens = total, "compacting context");
    compact_context(provider, session).await;
}

/// Force-compact the oldest half of non-system messages into a summary block.
pub async fn compact_context(provider: &ArcProvider, session: &mut Session) {
    // Separate system messages from the rest.
    let (system_msgs, mut conv_msgs): (Vec<_>, Vec<_>) = session.messages
        .drain(..)
        .partition(|m| matches!(m.role, Role::System));

    if conv_msgs.len() < 4 {
        // Nothing worth compacting.
        session.messages.extend(system_msgs);
        session.messages.extend(conv_msgs);
        return;
    }

    // Take the oldest half for summarisation.
    let mid = conv_msgs.len() / 2;
    let to_compact = conv_msgs.drain(..mid).collect::<Vec<_>>();
    let remaining = conv_msgs;

    // Build a summarisation prompt.
    let segment: String = to_compact.iter().map(|m| {
        let role = match m.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::Tool => "Tool",
            Role::System => "System",
        };
        format!("{role}: {}\n", m.content.as_str())
    }).collect();

    let summary_prompt = format!(
        "Summarise this conversation segment concisely. \
         Preserve all file paths, tool names, decisions made, errors encountered, and current state. \
         Output only the summary — no preamble.\n\n{segment}"
    );

    let summary_req = ChatRequest::new(&session.model)
        .with_messages(vec![Message::user(&summary_prompt)]);

    let summary = match provider.stream_chat(summary_req).await {
        Ok(mut stream) => {
            let mut text = String::new();
            while let Some(Ok(Delta::Text(chunk))) = stream.next().await {
                text.push_str(&chunk);
            }
            text
        }
        Err(e) => {
            tracing::warn!("compaction failed: {e}");
            // On failure, put messages back.
            session.messages.extend(system_msgs);
            session.messages.extend(to_compact);
            session.messages.extend(remaining);
            return;
        }
    };

    let compact_msg = Message::system(format!("[compacted: {}]", summary.trim()));

    session.messages.extend(system_msgs);
    session.messages.push(compact_msg);
    session.messages.extend(remaining);

    tracing::info!("context compacted: {} messages → summary + {}", mid, session.messages.len());
}

/// Store the most recent user↔assistant exchange as an embedded memory.
pub async fn store_turn_memory(
    provider: &ArcProvider,
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

pub async fn suggest_session_name(provider: &ArcProvider, session: &Session) -> Option<String> {
    if session.name.is_some() {
        return None;
    }

    let first_user = session
        .messages
        .iter()
        .find(|m| matches!(m.role, Role::User))
        .map(|m| m.content.as_str().to_string())
        .unwrap_or_default();

    if first_user.is_empty() {
        return None;
    }

    let snippet = &first_user[..first_user.len().min(200)];
    let prompt = format!(
        "Summarise this task in 4 to 6 words. No punctuation, no quotes. \
         Reply with ONLY the title.\n\nTask: {snippet}"
    );

    let req = ChatRequest::new(&session.model)
        .with_messages(vec![Message::user(&prompt)]);

    let Ok(mut stream) = provider.stream_chat(req).await else { return None };
    let mut title = String::new();
    while let Some(Ok(Delta::Text(chunk))) = stream.next().await {
        title.push_str(&chunk);
    }

    let title = title.trim().to_string();
    if !title.is_empty() && title.len() < 80 {
        return Some(title);
    }
    None
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
            AgentEvent::TokenUsage { input, output } => {
                eprintln!("[tokens] in={input} out={output}");
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