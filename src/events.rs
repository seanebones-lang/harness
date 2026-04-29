/// Events emitted by the agent loop and consumed by the TUI or stdout printer.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AgentEvent {
    /// A chunk of streamed assistant text.
    TextChunk(String),
    /// Agent is starting a tool call.
    ToolStart { name: String, id: String },
    /// A tool call completed with this result.
    ToolResult { name: String, id: String, result: String },
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

pub type EventTx = tokio::sync::mpsc::UnboundedSender<AgentEvent>;
pub type EventRx = tokio::sync::mpsc::UnboundedReceiver<AgentEvent>;

pub fn channel() -> (EventTx, EventRx) {
    tokio::sync::mpsc::unbounded_channel()
}
