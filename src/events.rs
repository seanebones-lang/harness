/// Events emitted by the agent loop and consumed by the TUI or stdout printer.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AgentEvent {
    /// A chunk of streamed assistant text.
    TextChunk(String),
    /// Agent is starting a tool call.
    ToolStart { name: String, id: String },
    /// A tool call completed with this result.
    ToolResult {
        name: String,
        id: String,
        result: String,
    },
    /// Agent recalled memories; injected N entries into context.
    MemoryRecall { count: usize },
    /// Sub-agent spawned for a task.
    SubAgentSpawned { task: String },
    /// Sub-agent finished.
    SubAgentDone { task: String, summary: String },
    /// Token usage for the completed request.
    TokenUsage { input: u32, output: u32 },
    /// Anthropic prompt-cache statistics.
    CacheUsage { creation: u32, read: u32 },
    /// Agent turn is complete.
    Done,
    /// An error occurred.
    Error(String),
}

/// Bounded queue so a stalled UI cannot grow agent events without bound.
pub const AGENT_EVENT_CHANNEL_CAP: usize = 1024;

pub type EventTx = tokio::sync::mpsc::Sender<AgentEvent>;
pub type EventRx = tokio::sync::mpsc::Receiver<AgentEvent>;

pub fn channel() -> (EventTx, EventRx) {
    tokio::sync::mpsc::channel(AGENT_EVENT_CHANNEL_CAP)
}

/// Best-effort send from the agent loop (sync closure): never blocks.
/// Drops streaming text chunks first if the buffer is full; then other events.
pub fn try_emit(tx: Option<&EventTx>, event: AgentEvent) {
    let Some(sender) = tx else {
        return;
    };
    match sender.try_send(event) {
        Ok(()) => {}
        Err(tokio::sync::mpsc::error::TrySendError::Full(unsent)) => {
            if matches!(&unsent, AgentEvent::TextChunk(_)) {
                tracing::warn!(
                    "AgentEvent channel full; dropped text chunk (capacity {AGENT_EVENT_CHANNEL_CAP})"
                );
            } else {
                tracing::warn!(
                    ?unsent,
                    "AgentEvent channel full; dropped event (capacity {AGENT_EVENT_CHANNEL_CAP})"
                );
            }
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {}
    }
}
