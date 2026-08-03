//! Core agentic drive loop.

use anyhow::Result;
use futures::StreamExt;
use harness_memory::{MemoryStore, Session};
use harness_provider_core::{
    tool_calls_to_message, ArcProvider, ChatRequest, Delta, DeltaStream, Message, StopReason,
};
use harness_tools::ToolExecutor;
use tracing::debug;

use crate::events::{try_emit, AgentEvent, EventTx};

use super::compact::maybe_compact;
use super::memory::build_augmented_system;

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
    drive_agent_with_options(
        provider,
        tools,
        memory_store,
        embed_model,
        session,
        system_prompt,
        tx,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn drive_agent_with_options(
    provider: &ArcProvider,
    tools: &ToolExecutor,
    memory_store: Option<&MemoryStore>,
    embed_model: Option<&str>,
    session: &mut Session,
    system_prompt: &str,
    tx: Option<&EventTx>,
    thinking_budget: Option<u32>,
) -> Result<()> {
    drive_agent_full(
        provider,
        tools,
        memory_store,
        embed_model,
        session,
        system_prompt,
        tx,
        thinking_budget,
        false,
        false,
        false,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub async fn drive_agent_with_schema(
    provider: &ArcProvider,
    tools: &ToolExecutor,
    memory_store: Option<&MemoryStore>,
    embed_model: Option<&str>,
    session: &mut Session,
    system_prompt: &str,
    tx: Option<&EventTx>,
    thinking_budget: Option<u32>,
    response_schema: Option<harness_provider_core::ResponseSchema>,
) -> Result<()> {
    drive_agent_full(
        provider,
        tools,
        memory_store,
        embed_model,
        session,
        system_prompt,
        tx,
        thinking_budget,
        false,
        false,
        false,
        response_schema,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn drive_agent_full(
    provider: &ArcProvider,
    tools: &ToolExecutor,
    memory_store: Option<&MemoryStore>,
    embed_model: Option<&str>,
    session: &mut Session,
    system_prompt: &str,
    tx: Option<&EventTx>,
    thinking_budget: Option<u32>,
    native_web_search: bool,
    native_code_execution: bool,
    native_x_search: bool,
    response_schema: Option<harness_provider_core::ResponseSchema>,
) -> Result<()> {
    let emit = |event: AgentEvent| {
        try_emit(tx, event);
    };

    // Auto-checkpoint: stash working tree once per turn before destructive tools.
    let turn_index = session.messages.len();
    let _checkpoint_taken = std::cell::Cell::new(false);

    // Memory recall: embed the last user message and inject top-k relevant past exchanges.
    let augmented_system = build_augmented_system(
        provider,
        memory_store,
        embed_model,
        session,
        system_prompt,
        &emit,
    )
    .await;

    const MAX_TOOL_LOOPS: usize = 50;
    let mut tool_loops = 0usize;

    loop {
        if tool_loops >= MAX_TOOL_LOOPS {
            emit(AgentEvent::Error(format!(
                "stopped after {MAX_TOOL_LOOPS} tool-call rounds (safety limit)"
            )));
            break;
        }
        // Auto-compact context when approaching 70% of the model context window.
        maybe_compact(provider, session, 0.70, Some(&emit)).await;

        let mut req = ChatRequest::new(&session.model)
            .with_messages(session.messages.clone())
            .with_tools(tools.registry().definitions())
            .with_system(&augmented_system)
            .with_thinking(thinking_budget)
            .with_native_tools(native_web_search, native_code_execution, native_x_search);
        if let Some(rs) = response_schema.clone() {
            req = req.with_response_schema(rs);
        }

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
                Delta::Usage {
                    input_tokens,
                    output_tokens,
                } => {
                    emit(AgentEvent::TokenUsage {
                        input: input_tokens,
                        output: output_tokens,
                    });
                }
                Delta::CacheUsage {
                    cache_creation_tokens,
                    cache_read_tokens,
                } => {
                    emit(AgentEvent::CacheUsage {
                        creation: cache_creation_tokens,
                        read: cache_read_tokens,
                    });
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
                c.args()
                    .ok()
                    .map(|args| harness_tools::tool_requires_checkpoint(&c.function.name, &args))
                    .unwrap_or(false)
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

            tool_loops += 1;
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
