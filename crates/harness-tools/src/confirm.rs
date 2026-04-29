//! Plan/approve mode: a gate that pauses destructive tool calls for user confirmation.
//!
//! The `ConfirmGate` wraps an unbounded sender. Before executing a write_file,
//! patch_file, or shell call, the executor sends a `ConfirmRequest` down the channel.
//! The UI holds the receiver, shows a preview, and responds via the oneshot channel
//! embedded in the request.

use tokio::sync::{mpsc, oneshot};

/// A single confirmation request sent from the executor to the UI.
pub struct ConfirmRequest {
    /// Short tool name, e.g. "write_file".
    pub tool_name: String,
    /// Human-readable preview of the proposed action (diff, command, etc.).
    pub preview: String,
    /// Send `true` to approve, `false` to deny.
    pub reply: oneshot::Sender<bool>,
}

/// Sender half — held by `ToolExecutor`.
#[derive(Clone)]
pub struct ConfirmGate(pub mpsc::UnboundedSender<ConfirmRequest>);

impl ConfirmGate {
    /// Request confirmation for a destructive action.
    /// Returns `true` if approved, `false` if denied or the channel is closed.
    pub async fn request(&self, tool_name: &str, preview: String) -> bool {
        let (tx, rx) = oneshot::channel();
        let req = ConfirmRequest {
            tool_name: tool_name.to_string(),
            preview,
            reply: tx,
        };
        if self.0.send(req).is_err() {
            return true; // channel closed → default to allow (non-TUI fallback)
        }
        rx.await.unwrap_or(true)
    }
}

/// Create a linked (gate, receiver) pair for TUI integration.
pub fn channel() -> (ConfirmGate, mpsc::UnboundedReceiver<ConfirmRequest>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (ConfirmGate(tx), rx)
}
