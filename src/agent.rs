//! Core agent loop: send → stream → execute tools → repeat.
//! Emits AgentEvents so callers (TUI or CLI) can display progress.

use anyhow::Result;
use futures::StreamExt;
use harness_memory::{MemoryStore, Session, SessionStore};
use harness_provider_core::{
    ArcProvider, ChatRequest, Delta, DeltaStream, Message, Role, StopReason,
};
use harness_provider_xai::tool_calls_to_message;
use harness_tools::ToolExecutor;
use tracing::debug;

use crate::events::{try_emit, AgentEvent, EventTx};

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
  spawn_swarm               — queue parallel background tasks (harness swarm list / result)
  find_definition           — LSP go-to-definition across the codebase
  find_references           — LSP find all references to a symbol
  rename_symbol             — LSP safe rename across files
  diagnostics               — LSP errors/warnings for a file
  browser (when enabled)    — Chrome CDP: navigate, screenshot, click, fill forms
  web_search (when enabled) — provider-native web search (no extra config needed)
  bash (when enabled)       — provider-native sandboxed code execution
  MCP tools (when loaded)   — any tools registered via .harness/mcp.json
  gh                        — GitHub CLI wrapper: PR/issue/CI workflow

Guidelines:
  - Prefer patch_file for single-file edits, apply_patch for multi-file changes.
  - Use the git tool for all git operations instead of shell git commands.
  - Always run test_runner after changes to verify correctness.
  - Use web_search when you need up-to-date information or documentation.
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
                matches!(
                    c.function.name.as_str(),
                    "write_file" | "patch_file" | "shell" | "apply_patch"
                )
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

/// Load a project-specific system prompt prefix from well-known files in CWD.
/// Checks (in order): .harness/SYSTEM.md, AGENTS.md, CLAUDE.md
pub fn load_project_instructions() -> Option<String> {
    let candidates = [".harness/SYSTEM.md", "AGENTS.md", "CLAUDE.md"];
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
    // Inject project semantic memory from .harness/memory/
    let base = crate::memory_project::augment_system(&base);
    let system_prompt = base.as_str();

    let (Some(mem), Some(model)) = (memory_store, embed_model) else {
        return system_prompt.to_string();
    };

    let Some(last_user) = session
        .messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, Role::User))
    else {
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

    emit(AgentEvent::MemoryRecall {
        count: memories.len(),
    });

    let mem_block = memories
        .iter()
        .map(|(m, score)| format!("[memory relevance={:.2}]\n{}", score, m.text))
        .collect::<Vec<_>>()
        .join("\n\n");

    format!("{system_prompt}\n\n## Relevant past context\n{mem_block}")
}

/// Return a rough token count for a slice of messages (character heuristic: 4 chars/token).
pub fn estimate_tokens(messages: &[harness_provider_core::Message]) -> usize {
    messages
        .iter()
        .map(|m| m.content.as_str().len() / 4 + 1)
        .sum()
}

/// Rough context window (tokens) for compaction heuristics.
pub fn context_limit_for_model(model: &str) -> usize {
    let m = model.to_lowercase();
    if m.contains("gpt") || m.contains("grok") {
        1_000_000
    } else if m.contains("claude") || m.contains("opus") || m.contains("sonnet") || m.contains("haiku")
    {
        200_000
    } else if m.contains("qwen") || m.contains("coder") {
        256_000
    } else {
        128_000
    }
}

/// Context compaction: when the session exceeds `threshold` fraction of the model context
/// window, summarise the oldest non-system messages and replace them with a compact block.
pub async fn maybe_compact(
    provider: &ArcProvider,
    session: &mut Session,
    threshold: f32,
    emit: Option<&impl Fn(AgentEvent)>,
) {
    let limit = context_limit_for_model(&session.model);
    let total = estimate_tokens(&session.messages);
    if (total as f32) < limit as f32 * threshold {
        return;
    }

    tracing::debug!(tokens = total, limit, "compacting context");
    let before = session.messages.len();
    if compact_context(provider, session).await {
        let after = session.messages.len();
        if let Some(emit) = emit {
            emit(AgentEvent::ContextCompacted {
                messages_before: before,
                messages_after: after,
            });
        }
    }
}

/// Force-compact the oldest half of non-system messages into a summary block.
/// Returns `true` if messages were replaced with a summary.
pub async fn compact_context(provider: &ArcProvider, session: &mut Session) -> bool {
    // Separate system messages from the rest.
    let (system_msgs, mut conv_msgs): (Vec<_>, Vec<_>) = session
        .messages
        .drain(..)
        .partition(|m| matches!(m.role, Role::System));

    if conv_msgs.len() < 4 {
        // Nothing worth compacting.
        session.messages.extend(system_msgs);
        session.messages.extend(conv_msgs);
        return false;
    }

    // Take the oldest half for summarisation.
    let mid = conv_msgs.len() / 2;
    let to_compact = conv_msgs.drain(..mid).collect::<Vec<_>>();
    let remaining = conv_msgs;

    // Build a summarisation prompt.
    let segment: String = to_compact
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
                Role::Tool => "Tool",
                Role::System => "System",
            };
            format!("{role}: {}\n", m.content.as_str())
        })
        .collect();

    let summary_prompt = format!(
        "Summarise this conversation segment concisely. \
         Preserve all file paths, tool names, decisions made, errors encountered, and current state. \
         Output only the summary — no preamble.\n\n{segment}"
    );

    let summary_req =
        ChatRequest::new(&session.model).with_messages(vec![Message::user(&summary_prompt)]);

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
            return false;
        }
    };

    let compact_msg = Message::system(format!("[compacted: {}]", summary.trim()));

    session.messages.extend(system_msgs);
    session.messages.push(compact_msg);
    session.messages.extend(remaining);

    tracing::info!(
        "context compacted: {} messages → summary + {}",
        mid,
        session.messages.len()
    );
    true
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

    let req = ChatRequest::new(&session.model).with_messages(vec![Message::user(&prompt)]);

    let Ok(mut stream) = provider.stream_chat(req).await else {
        return None;
    };
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
            } => eprintln!(
                "[compact] {messages_before} → {messages_after} messages"
            ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream;
    use harness_provider_core::{
        ChatRequest, DeltaStream, Pricing, Provider, ProviderError, ToolCall, ToolCallFunction,
        ToolDefinition,
    };
    use harness_tools::registry::{Tool, ToolRegistry};
    use harness_tools::ToolExecutor;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// Returns scripted delta batches on each `stream_chat` call (in order).
    struct ScriptProvider {
        scripts: Mutex<Vec<Vec<Delta>>>,
        calls: AtomicUsize,
        embed: Vec<f32>,
    }

    impl ScriptProvider {
        fn new(scripts: Vec<Vec<Delta>>) -> Self {
            Self {
                scripts: Mutex::new(scripts),
                calls: AtomicUsize::new(0),
                embed: vec![1.0, 0.0, 0.0],
            }
        }

        fn with_embed(mut self, embed: Vec<f32>) -> Self {
            self.embed = embed;
            self
        }
    }

    #[async_trait]
    impl Provider for ScriptProvider {
        fn name(&self) -> &str {
            "script"
        }

        fn model(&self) -> &str {
            "script-model"
        }

        async fn stream_chat(&self, _req: ChatRequest) -> Result<DeltaStream, ProviderError> {
            let idx = self.calls.fetch_add(1, Ordering::SeqCst);
            let batch = {
                let scripts = self.scripts.lock().expect("scripts lock");
                scripts.get(idx).cloned().unwrap_or_else(|| {
                    vec![Delta::Done {
                        stop_reason: StopReason::EndTurn,
                    }]
                })
            };
            Ok(Box::pin(stream::iter(batch.into_iter().map(Ok))))
        }

        fn pricing(&self) -> Option<Pricing> {
            None
        }

        async fn embed(&self, _model: &str, _text: &str) -> Result<Vec<f32>, ProviderError> {
            Ok(self.embed.clone())
        }
    }

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new(
                "echo",
                "Echo a message",
                json!({
                    "type": "object",
                    "properties": { "msg": { "type": "string" } },
                    "required": ["msg"]
                }),
            )
        }

        async fn execute(&self, args: serde_json::Value) -> anyhow::Result<String> {
            Ok(format!(
                "echo:{}",
                args.get("msg").and_then(|v| v.as_str()).unwrap_or("")
            ))
        }
    }

    fn echo_call(id: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            kind: "function".into(),
            function: ToolCallFunction {
                name: "echo".into(),
                arguments: r#"{"msg":"hi"}"#.into(),
            },
        }
    }

    fn echo_executor() -> ToolExecutor {
        let mut registry = ToolRegistry::new();
        registry.register(EchoTool);
        ToolExecutor::new(registry)
    }

    async fn run_and_collect(
        provider: ArcProvider,
        tools: &ToolExecutor,
        session: &mut Session,
    ) -> (Vec<AgentEvent>, Result<()>) {
        let (tx, mut rx) = crate::events::channel();
        let result = drive_agent(
            &provider,
            tools,
            None,
            None,
            session,
            "test system",
            Some(&tx),
        )
        .await;
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        (events, result)
    }

    #[tokio::test]
    async fn drive_agent_streams_text_and_completes() {
        let mut session = Session::new("script-model");
        session.push(Message::user("hello"));

        let provider: ArcProvider = Arc::new(ScriptProvider::new(vec![vec![
            Delta::Text("world".into()),
            Delta::Done {
                stop_reason: StopReason::EndTurn,
            },
        ]]));

        let (events, result) = run_and_collect(provider, &echo_executor(), &mut session).await;

        result.expect("drive_agent should succeed");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AgentEvent::TextChunk(s) if s == "world")),
            "expected text chunk"
        );
        assert!(events.iter().any(|e| matches!(e, AgentEvent::Done)));
        assert_eq!(session.messages.len(), 2);
        assert!(session.messages[1].content.as_str().contains("world"));
    }

    #[tokio::test]
    async fn drive_agent_executes_tool_then_continues() {
        let mut session = Session::new("script-model");
        session.push(Message::user("use echo"));

        let script = Arc::new(ScriptProvider::new(vec![
            vec![
                Delta::ToolCall(echo_call("tc1")),
                Delta::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ],
            vec![
                Delta::Text("finished".into()),
                Delta::Done {
                    stop_reason: StopReason::EndTurn,
                },
            ],
        ]));
        let provider: ArcProvider = script.clone();
        let (tx, mut rx) = crate::events::channel();
        let result = drive_agent(
            &provider,
            &echo_executor(),
            None,
            None,
            &mut session,
            "test system",
            Some(&tx),
        )
        .await;
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }

        result.expect("drive_agent should succeed");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AgentEvent::ToolStart { name, .. } if name == "echo"))
        );
        assert!(
            events.iter().any(
                |e| matches!(e, AgentEvent::ToolResult { result, .. } if result.contains("echo:hi"))
            )
        );
        assert_eq!(script.calls.load(Ordering::SeqCst), 2);
        assert!(session.messages.len() >= 4);
    }

    #[tokio::test]
    async fn drive_agent_stops_at_tool_loop_limit() {
        let mut session = Session::new("script-model");
        session.push(Message::user("loop forever"));

        let always_tool = vec![Delta::ToolCall(echo_call("tc")), Delta::Done {
            stop_reason: StopReason::ToolUse,
        }];
        let script = Arc::new(ScriptProvider::new(vec![always_tool; 60]));
        let provider: ArcProvider = script.clone();
        let (tx, mut rx) = crate::events::channel();
        let result = drive_agent(
            &provider,
            &echo_executor(),
            None,
            None,
            &mut session,
            "test system",
            Some(&tx),
        )
        .await;
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }

        result.expect("drive_agent should return Ok after safety stop");
        assert!(
            events.iter().any(|e| {
                matches!(e, AgentEvent::Error(msg) if msg.contains("50") && msg.contains("safety limit"))
            }),
            "expected safety-limit error event"
        );
        assert_eq!(script.calls.load(Ordering::SeqCst), 50);
    }

    #[test]
    fn estimate_tokens_uses_char_heuristic() {
        let msgs = vec![
            Message::user("abcd"),
            Message::assistant("efgh"),
        ];
        assert_eq!(estimate_tokens(&msgs), 4);
    }

    #[tokio::test]
    async fn maybe_compact_skips_small_sessions() {
        let provider: ArcProvider = Arc::new(ScriptProvider::new(vec![]));
        let mut session = Session::new("script-model");
        session.push(Message::user("short"));
        maybe_compact(&provider, &mut session, 0.7, None::<&fn(AgentEvent)>).await;
        assert_eq!(session.messages.len(), 1);
    }

    #[tokio::test]
    async fn compact_context_replaces_old_messages_with_summary() {
        let provider: ArcProvider = Arc::new(ScriptProvider::new(vec![vec![
            Delta::Text("older turns summary".into()),
            Delta::Done {
                stop_reason: StopReason::EndTurn,
            },
        ]]));
        let mut session = Session::new("script-model");
        for i in 0..8 {
            session.push(Message::user(format!("question {i}")));
            session.push(Message::assistant(format!("answer {i}")));
        }
        let before = session.messages.len();
        compact_context(&provider, &mut session).await;
        assert!(session.messages.len() < before);
        assert!(
            session
                .messages
                .iter()
                .any(|m| m.content.as_str().contains("[compacted:"))
        );
    }

    #[tokio::test]
    async fn build_augmented_system_injects_matching_memories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = MemoryStore::open(dir.path().join("mem.db")).expect("memory db");
        let embedding = vec![1.0, 0.0, 0.0];
        store
            .insert("other", "prior architecture fact", &embedding)
            .expect("insert memory");

        let provider: ArcProvider =
            Arc::new(ScriptProvider::new(vec![]).with_embed(embedding.clone()));
        let mut session = Session::new("script-model");
        session.id = "current".to_string();
        session.push(Message::user("tell me about architecture"));

        use std::cell::Cell;

        let recalled = Cell::new(0usize);
        let emit = |event: AgentEvent| {
            if let AgentEvent::MemoryRecall { count } = event {
                recalled.set(count);
            }
        };

        let augmented = build_augmented_system(
            &provider,
            Some(&store),
            Some("embed-model"),
            &session,
            "base prompt",
            &emit,
        )
        .await;

        assert!(recalled.get() >= 1, "expected MemoryRecall event");
        assert!(augmented.contains("prior architecture fact"));
        assert!(augmented.contains("Relevant past context"));
    }

    #[test]
    fn context_limit_varies_by_model_family() {
        assert_eq!(context_limit_for_model("gpt-5.5"), 1_000_000);
        assert_eq!(context_limit_for_model("claude-sonnet-4-6"), 200_000);
        assert_eq!(context_limit_for_model("qwen3-coder:30b"), 256_000);
        assert_eq!(context_limit_for_model("unknown-model"), 128_000);
    }

    #[tokio::test]
    async fn maybe_compact_emits_context_compacted_event() {
        use std::cell::Cell;

        let provider: ArcProvider = Arc::new(ScriptProvider::new(vec![vec![
            Delta::Text("summary block".into()),
            Delta::Done {
                stop_reason: StopReason::EndTurn,
            },
        ]]));
        let mut session = Session::new("claude-sonnet-4-6");
        for i in 0..8 {
            session.push(Message::user(format!("question {i}")));
            session.push(Message::assistant(format!("answer {i}")));
        }
        let saw = Cell::new(false);
        let emit = |event: AgentEvent| {
            if let AgentEvent::ContextCompacted {
                messages_before,
                messages_after,
            } = event
            {
                assert!(messages_before > messages_after);
                saw.set(true);
            }
        };
        maybe_compact(&provider, &mut session, 0.0, Some(&emit)).await;
        assert!(saw.get(), "expected ContextCompacted event");
    }
}
