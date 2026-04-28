//! Core agent loop: send → stream → execute tools → repeat.

use anyhow::Result;
use futures::StreamExt;
use harness_memory::{Session, SessionStore};
use harness_provider_core::{ChatRequest, Delta, DeltaStream, Message, Provider, StopReason};
use harness_provider_xai::{XaiProvider, tool_calls_to_message};
use harness_tools::ToolExecutor;
use tracing::debug;

/// Run one prompt to completion (non-interactive / `harness run`).
pub async fn run_once(
    provider: XaiProvider,
    store: SessionStore,
    tools: ToolExecutor,
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

    let response = drive_agent(&provider, &tools, &mut session, system_prompt).await?;
    println!("{response}");

    store.save(&session)?;
    eprintln!("[session {}]", session.short_id());
    Ok(())
}

/// Run the full agentic loop for one turn: send, stream, handle tool calls,
/// repeat until the model stops with EndTurn or MaxTokens.
/// Returns the final assistant text.
pub async fn drive_agent(
    provider: &XaiProvider,
    tools: &ToolExecutor,
    session: &mut Session,
    system_prompt: Option<&str>,
) -> Result<String> {
    let mut final_text = String::new();

    loop {
        let req = ChatRequest::new(&session.model)
            .with_messages(session.messages.clone())
            .with_tools(tools.registry().definitions())
            .with_system(system_prompt.unwrap_or(DEFAULT_SYSTEM));

        let mut stream: DeltaStream = provider.stream_chat(req).await?;

        let mut text_buf = String::new();
        let mut pending_tool_calls = Vec::new();
        let mut stop_reason = StopReason::EndTurn;

        while let Some(item) = stream.next().await {
            match item? {
                Delta::Text(chunk) => {
                    print!("{chunk}");
                    use std::io::Write;
                    std::io::stdout().flush().ok();
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
            println!(); // newline after streamed text
            session.push(Message::assistant(&text_buf));
            final_text = text_buf.clone();
        }

        if !pending_tool_calls.is_empty() {
            // Record the assistant's tool call intent
            session.push(tool_calls_to_message(&pending_tool_calls));

            // Execute all tool calls and record results
            for call in &pending_tool_calls {
                debug!(tool = %call.function.name, id = %call.id, "executing tool call");
                let result = tools.execute(call).await;
                eprintln!("[tool:{}] {}", call.function.name, &result[..result.len().min(120)]);
                session.push(Message::tool_result(&call.id, result));
            }

            // Continue the loop — model will respond to tool results
            continue;
        }

        // No tool calls — we're done
        match stop_reason {
            StopReason::MaxTokens => {
                eprintln!("[warning] hit max_tokens limit");
            }
            _ => {}
        }
        break;
    }

    Ok(final_text)
}

const DEFAULT_SYSTEM: &str = "\
You are a powerful coding assistant running in a terminal. \
You have access to tools to read and write files, run shell commands, and search code. \
Be concise and precise. Prefer making changes over explaining; show diffs when you edit files. \
Always verify your changes work by running relevant tests or build commands.";
