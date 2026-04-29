//! Ambient memory consolidation: a background tokio task that periodically
//! compacts the vector memory store by asking Grok to summarise clusters of
//! related memories into a single higher-level memory.
//!
//! The task wakes every INTERVAL seconds and only runs a consolidation pass
//! when at least MIN_NEW_SINCE_LAST new memories have been inserted since the
//! previous pass.

use anyhow::Result;
use futures::StreamExt;
use harness_memory::{Memory, MemoryStore};
use harness_provider_core::{ChatRequest, Delta, Message, Provider};
use harness_provider_xai::XaiProvider;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

const INTERVAL: Duration = Duration::from_secs(300); // 5 minutes
const MIN_NEW: usize = 5;
const TOP_K: usize = 20;

pub trait AmbientProvider: Provider + Clone + Send + Sync + 'static {
    fn embed_for_memory<'a>(
        &'a self,
        model: &'a str,
        text: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<f32>>> + Send + 'a>>;
}

impl AmbientProvider for XaiProvider {
    fn embed_for_memory<'a>(
        &'a self,
        model: &'a str,
        text: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<f32>>> + Send + 'a>> {
        Box::pin(async move { self.embed(model, text).await })
    }
}

/// Spawn the ambient consolidation task.
///
/// Returns a `(shutdown_tx, join_handle)` pair.
/// Send `()` on `shutdown_tx` (or drop it) to request a clean stop;
/// `await` the `JoinHandle` to confirm the task has exited.
pub fn spawn(
    provider: impl AmbientProvider,
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
    provider: &impl AmbientProvider,
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

    // Embed the summary.
    let embedding = provider.embed_for_memory(embed_model, &summary).await?;

    // Store the consolidated memory under a synthetic session id.
    memory.insert("__consolidated__", &summary, &embedding)?;

    // Delete the originals.
    let ids: Vec<String> = memories.iter().map(|m| m.id.clone()).collect();
    memory.delete_memories(&ids)?;

    Ok(memories.len())
}

