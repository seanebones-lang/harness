//! Context window estimation and compaction.

use futures::StreamExt;
use harness_memory::Session;
use harness_provider_core::{ArcProvider, ChatRequest, Delta, Message, Role};

use crate::events::AgentEvent;

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
    } else if m.contains("claude")
        || m.contains("opus")
        || m.contains("sonnet")
        || m.contains("haiku")
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

#[cfg(test)]
mod tests {
    use super::*;
    use harness_provider_core::Message;

    #[test]
    fn estimate_tokens_empty_is_zero() {
        assert_eq!(estimate_tokens(&[]), 0);
    }

    #[test]
    fn estimate_tokens_uses_char_heuristic_plus_one_per_message() {
        // "abcd" → 4/4 + 1 = 2; "efghij" → 6/4 + 1 = 2
        let msgs = vec![Message::user("abcd"), Message::assistant("efghij")];
        assert_eq!(estimate_tokens(&msgs), 4);
    }

    #[test]
    fn context_limit_for_model_covers_families_and_case() {
        assert_eq!(context_limit_for_model("gpt-5.5"), 1_000_000);
        assert_eq!(context_limit_for_model("GPT-4o"), 1_000_000);
        assert_eq!(context_limit_for_model("grok-4.3"), 1_000_000);
        assert_eq!(context_limit_for_model("claude-sonnet-4-6"), 200_000);
        assert_eq!(context_limit_for_model("claude-opus-4"), 200_000);
        assert_eq!(context_limit_for_model("claude-haiku-3-5"), 200_000);
        assert_eq!(context_limit_for_model("Sonnet"), 200_000);
        assert_eq!(context_limit_for_model("qwen3-coder:30b"), 256_000);
        assert_eq!(context_limit_for_model("unknown-model"), 128_000);
    }
}
