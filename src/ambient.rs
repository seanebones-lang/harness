//! Ambient memory consolidation: a background tokio task that periodically
//! compacts the vector memory store by asking the model to summarise clusters of
//! related memories into a single higher-level memory.

use anyhow::Result;
use futures::StreamExt;
use harness_memory::{Memory, MemoryStore};
use harness_provider_core::{ArcProvider, ChatRequest, Delta, Message};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

const INTERVAL: Duration = Duration::from_secs(300); // 5 minutes
const MIN_NEW: usize = 5;
const TOP_K: usize = 20;

/// Spawn the ambient consolidation task.
///
/// Returns a `(shutdown_tx, join_handle)` pair.
/// Send `()` on `shutdown_tx` (or drop it) to request a clean stop;
/// `await` the `JoinHandle` to confirm the task has exited.
pub fn spawn(
    provider: ArcProvider,
    memory: Arc<MemoryStore>,
    embed_model: String,
) -> (watch::Sender<()>, tokio::task::JoinHandle<()>) {
    let (tx, mut rx) = watch::channel(());

    let handle = tokio::spawn(async move {
        let mut last_count: usize = memory.count_all().unwrap_or(0);
        let mut interval = tokio::time::interval(INTERVAL);
        interval.tick().await; // skip the immediate first tick

        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = rx.changed() => break,
            }

            let current = memory.count_all().unwrap_or(0);
            let new_since = current.saturating_sub(last_count);
            if new_since < MIN_NEW {
                tracing::debug!(new_since, "ambient: not enough new memories, skipping");
                continue;
            }

            tracing::info!(new_since, total = current, "ambient: running consolidation pass");

            match consolidate(&provider, &memory, &embed_model).await {
                Ok(n) => {
                    tracing::info!(n, "ambient: consolidated memories");
                    last_count = memory.count_all().unwrap_or(0);
                }
                Err(e) => tracing::warn!("ambient: consolidation failed: {e}"),
            }
        }
    });

    (tx, handle)
}

/// Pull the most recent TOP_K memories, ask the model to summarise them,
/// store the summary as a new memory, and delete the originals.
async fn consolidate(
    provider: &ArcProvider,
    memory: &MemoryStore,
    embed_model: &str,
) -> Result<usize> {
    let memories: Vec<Memory> = memory.recent_memories(TOP_K)?;
    if memories.len() < MIN_NEW {
        return Ok(0);
    }

    let text_block = memories
        .iter()
        .enumerate()
        .map(|(i, m)| format!("[{}] {}", i + 1, m.text))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "The following are memory fragments from a coding agent session. \
         Please consolidate them into a single concise paragraph that captures \
         the key facts, decisions, and context. Omit trivial or redundant details.\n\n{text_block}"
    );

    let model = provider.model().to_string();
    let req = ChatRequest::new(model).with_messages(vec![Message::user(&prompt)]);
    let mut stream = provider.stream_chat(req).await?;
    let mut summary = String::new();

    while let Some(delta) = stream.next().await {
        if let Ok(Delta::Text(chunk)) = delta {
            summary.push_str(&chunk);
        }
    }

    if summary.trim().is_empty() {
        anyhow::bail!("consolidation produced empty summary");
    }

    let embedding = provider.embed(embed_model, &summary).await
        .map_err(|e| anyhow::anyhow!("embed failed: {e}"))?;

    memory.insert("__consolidated__", &summary, &embedding)?;

    let ids: Vec<String> = memories.iter().map(|m| m.id.clone()).collect();
    memory.delete_memories(&ids)?;

    Ok(memories.len())
}
