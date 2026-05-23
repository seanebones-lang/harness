//! Plan/approve mode: a gate that pauses destructive tool calls for user confirmation.
//!
//! The `ConfirmGate` wraps a bounded sender. Before executing a write_file,
//! patch_file, or shell call, the executor sends a `ConfirmRequest` down the channel.
//! The UI holds the receiver, shows a preview, and responds via the oneshot channel
//! embedded in the request.

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

const CONFIRM_CHANNEL_CAP: usize = 256;

/// Outcome of a plan-mode confirmation prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmResult {
    /// User denied the action; skip the tool call.
    Deny,
    /// User approved; run the tool normally.
    Approve,
    /// User approved a partial diff; write `content` to `path` instead of running the tool.
    ApplyContent {
        /// Target file path.
        path: String,
        /// Final content after hunk review.
        content: String,
    },
}

/// A single confirmation request sent from the executor to the UI.
pub struct ConfirmRequest {
    /// Short tool name, e.g. "write_file".
    pub tool_name: String,
    /// Human-readable preview of the proposed action (diff, command, etc.).
    pub preview: String,
    /// Raw tool arguments (for inline diff review in the TUI).
    pub args: Option<Value>,
    /// Send the user's decision back to the executor.
    pub reply: oneshot::Sender<ConfirmResult>,
}

/// Confirmation gate sender for plan/approve mode.
#[derive(Clone)]
pub struct ConfirmGate(pub mpsc::Sender<ConfirmRequest>);

impl ConfirmGate {
    /// Request confirmation for a destructive action.
    pub async fn request(
        &self,
        tool_name: &str,
        preview: String,
        args: Option<Value>,
    ) -> ConfirmResult {
        let (tx, rx) = oneshot::channel();
        let req = ConfirmRequest {
            tool_name: tool_name.to_string(),
            preview,
            args,
            reply: tx,
        };
        match self.0.try_send(req) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!("confirm channel full; defaulting to deny");
                return ConfirmResult::Deny;
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!("confirm channel closed; defaulting to deny");
                return ConfirmResult::Deny;
            }
        }
        match rx.await {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!("confirm reply dropped; defaulting to deny");
                ConfirmResult::Deny
            }
        }
    }
}

/// Create a linked (gate, receiver) pair for TUI integration.
pub fn channel() -> (ConfirmGate, mpsc::Receiver<ConfirmRequest>) {
    let (tx, rx) = mpsc::channel(CONFIRM_CHANNEL_CAP);
    (ConfirmGate(tx), rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn closed_channel_denies() {
        let (gate, rx) = channel();
        drop(rx);
        assert_eq!(
            gate.request("shell", "rm -rf .".into(), None).await,
            ConfirmResult::Deny
        );
    }

    #[tokio::test]
    async fn explicit_deny_returns_deny() {
        let (gate, mut rx) = channel();
        let gate2 = gate.clone();
        tokio::spawn(async move {
            if let Some(req) = rx.recv().await {
                let _ = req.reply.send(ConfirmResult::Deny);
            }
        });
        assert_eq!(
            gate2.request("write_file", "write foo".into(), None).await,
            ConfirmResult::Deny
        );
    }

    #[tokio::test]
    async fn explicit_approve_returns_approve() {
        let (gate, mut rx) = channel();
        let gate2 = gate.clone();
        tokio::spawn(async move {
            if let Some(req) = rx.recv().await {
                let _ = req.reply.send(ConfirmResult::Approve);
            }
        });
        assert_eq!(
            gate2.request("shell", "git push".into(), None).await,
            ConfirmResult::Approve
        );
    }

    #[tokio::test]
    async fn apply_content_short_circuits_tool() {
        let (gate, mut rx) = channel();
        let gate2 = gate.clone();
        tokio::spawn(async move {
            if let Some(req) = rx.recv().await {
                let _ = req.reply.send(ConfirmResult::ApplyContent {
                    path: "foo.rs".into(),
                    content: "fn main() {}".into(),
                });
            }
        });
        assert_eq!(
            gate2.request("write_file", "write foo.rs".into(), None).await,
            ConfirmResult::ApplyContent {
                path: "foo.rs".into(),
                content: "fn main() {}".into(),
            }
        );
    }
}
