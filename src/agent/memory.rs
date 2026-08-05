//! Memory injection and turn storage.

use harness_memory::{MemoryStore, Session};
use harness_provider_core::{ArcProvider, Role};
use tracing::debug;

use crate::events::AgentEvent;

use super::system::load_project_instructions;

/// Embed the last user message, retrieve top-k memories, and prepend them to the system prompt.
pub(crate) async fn build_augmented_system(
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
