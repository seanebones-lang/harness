use super::*;
use anyhow::Result;
use async_trait::async_trait;
use futures::stream;
use harness_memory::{MemoryStore, Session};
use harness_provider_core::{
    ArcProvider, ChatRequest, Delta, DeltaStream, Message, Pricing, Provider, ProviderError,
    StopReason, ToolCall, ToolCallFunction, ToolDefinition,
};
use harness_tools::registry::{Tool, ToolRegistry};
use harness_tools::ToolExecutor;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::memory::build_augmented_system;
use crate::events::AgentEvent;

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
            let scripts = self.scripts.lock().unwrap_or_else(|e| e.into_inner());
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
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolStart { name, .. } if name == "echo")));
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolResult { result, .. } if result.contains("echo:hi"))));
    assert_eq!(script.calls.load(Ordering::SeqCst), 2);
    assert!(session.messages.len() >= 4);
}

#[tokio::test]
async fn drive_agent_stops_at_tool_loop_limit() {
    let mut session = Session::new("script-model");
    session.push(Message::user("loop forever"));

    let always_tool = vec![
        Delta::ToolCall(echo_call("tc")),
        Delta::Done {
            stop_reason: StopReason::ToolUse,
        },
    ];
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
    let msgs = vec![Message::user("abcd"), Message::assistant("efgh")];
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
    assert!(session
        .messages
        .iter()
        .any(|m| m.content.as_str().contains("[compacted:")));
}

#[tokio::test]
async fn build_augmented_system_injects_matching_memories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = MemoryStore::open(dir.path().join("mem.db")).expect("memory db");
    let embedding = vec![1.0, 0.0, 0.0];
    store
        .insert("other", "prior architecture fact", &embedding)
        .expect("insert memory");

    let provider: ArcProvider = Arc::new(ScriptProvider::new(vec![]).with_embed(embedding.clone()));
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
    assert_eq!(context_limit_for_model("grok-4.3"), 1_000_000);
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
    let mut session = Session::new("grok-4.3");
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
