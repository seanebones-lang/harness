//! Plan/approve mode: a gate that pauses destructive tool calls for user confirmation.
//!
//! The `ConfirmGate` wraps a bounded sender. Before executing a write_file,
//! patch_file, or shell call, the executor sends a `ConfirmRequest` down the channel.
//! The UI holds the receiver, shows a preview, and responds via the oneshot channel
//! embedded in the request.

use tokio::sync::{mpsc, oneshot};

const CONFIRM_CHANNEL_CAP: usize = 256;

/// A single confirmation request sent from the executor to the UI.
pub struct ConfirmRequest {
    /// Short tool name, e.g. "write_file".
    pub tool_name: String,
    /// Human-readable preview of the proposed action (diff, command, etc.).
    pub preview: String,
    /// Send `true` to approve, `false` to deny.
    pub reply: oneshot::Sender<bool>,
}

/// Confirmation gate sender for plan/approve mode.
#[derive(Clone)]
pub struct ConfirmGate(pub mpsc::Sender<ConfirmRequest>);

impl ConfirmGate {
    /// Request confirmation for a destructive action.
    /// Returns `true` if approved, `false` if denied or the channel is unavailable.
    pub async fn request(&self, tool_name: &str, preview: String) -> bool {
        let (tx, rx) = oneshot::channel();
        let req = ConfirmRequest {
            tool_name: tool_name.to_string(),
            preview,
            reply: tx,
        };
        match self.0.try_send(req) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!("confirm channel full; defaulting to deny");
                return false;
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!("confirm channel closed; defaulting to deny");
                return false;
            }
        }
        match rx.await {
            Ok(true) => true,
            Ok(false) => false,
            Err(_) => {
                tracing::warn!("confirm reply dropped; defaulting to deny");
                false
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
        assert!(
            !gate.request("shell", "rm -rf .".into()).await,
            "closed channel must deny"
        );
    }

    #[tokio::test]
    async fn explicit_deny_returns_false() {
        let (gate, mut rx) = channel();
        let gate2 = gate.clone();
        tokio::spawn(async move {
            if let Some(req) = rx.recv().await {
                let _ = req.reply.send(false);
            }
        });
        assert!(
            !gate2.request("write_file", "write foo".into()).await,
            "explicit deny must return false"
        );
    }

    #[tokio::test]
    async fn explicit_approve_returns_true() {
        let (gate, mut rx) = channel();
        let gate2 = gate.clone();
        tokio::spawn(async move {
            if let Some(req) = rx.recv().await {
                let _ = req.reply.send(true);
            }
        });
        assert!(
            gate2.request("shell", "git push".into()).await,
            "explicit approve must return true"
        );
    }
}
