//! Ambient memory consolidation: a background tokio task that periodically
//! compacts the vector memory store by asking the model to summarise clusters of
//! related memories into a single higher-level memory.

use anyhow::Result;
use futures::StreamExt;
use harness_memory::{Memory, MemoryStore};
use harness_provider_core::{ArcProvider, ChatRequest, Delta, Message, ProviderError};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Tunable parameters for the ambient consolidation loop.
#[derive(Debug, Clone)]
pub struct AmbientConfig {
    pub interval: Duration,
    pub min_new: usize,
    pub top_k: usize,
    /// Model used for consolidation summaries; defaults to the provider's model when unset.
    pub consolidation_model: Option<String>,
}

impl Default for AmbientConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(300),
            min_new: 5,
            top_k: 20,
            consolidation_model: None,
        }
    }
}

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
    spawn_with_config(provider, memory, embed_model, AmbientConfig::default())
}

pub fn spawn_with_config(
    provider: ArcProvider,
    memory: Arc<MemoryStore>,
    embed_model: String,
    config: AmbientConfig,
) -> (watch::Sender<()>, tokio::task::JoinHandle<()>) {
    let AmbientConfig {
        interval,
        min_new,
        top_k,
        consolidation_model,
    } = config;
    let (tx, mut rx) = watch::channel(());

    let handle = tokio::spawn(async move {
        let mut last_count: usize = memory.count_all().unwrap_or(0);
        let mut tick = tokio::time::interval(interval);
        tick.tick().await; // skip the immediate first tick

        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = rx.changed() => break,
            }

            let current = memory.count_all().unwrap_or(0);
            let new_since = current.saturating_sub(last_count);
            if new_since < min_new {
                tracing::debug!(new_since, "ambient: not enough new memories, skipping");
                continue;
            }

            tracing::info!(
                new_since,
                total = current,
                "ambient: running consolidation pass"
            );

            match consolidate(
                &provider,
                &memory,
                &embed_model,
                min_new,
                top_k,
                consolidation_model.as_deref(),
            )
            .await
            {
                Ok(n) => {
                    if n > 0 {
                        tracing::info!(n, "ambient: consolidated memories");
                    }
                    last_count = memory.count_all().unwrap_or(0);
                }
                Err(ConsolidateError::UnsupportedEmbed) => {
                    tracing::debug!("ambient: provider does not support embeddings, skipping");
                }
                Err(ConsolidateError::Other(e)) => {
                    tracing::warn!("ambient: consolidation failed: {e}");
                }
            }
        }
    });

    (tx, handle)
}

#[derive(Debug)]
pub(crate) enum ConsolidateError {
    UnsupportedEmbed,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for ConsolidateError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(e)
    }
}

/// Pull the most recent memories, ask the model to summarise them,
/// store the summary as a new memory, and delete the originals.
pub(crate) async fn consolidate(
    provider: &ArcProvider,
    memory: &MemoryStore,
    embed_model: &str,
    min_new: usize,
    top_k: usize,
    consolidation_model: Option<&str>,
) -> Result<usize, ConsolidateError> {
    let memories: Vec<Memory> = memory.recent_memories(top_k)?;
    if memories.len() < min_new {
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

    let model = consolidation_model
        .map(str::to_string)
        .unwrap_or_else(|| provider.model().to_string());
    let req = ChatRequest::new(model).with_messages(vec![Message::user(&prompt)]);
    let mut stream = provider
        .stream_chat(req)
        .await
        .map_err(|e| ConsolidateError::Other(anyhow::anyhow!("stream_chat failed: {e}")))?;
    let mut summary = String::new();

    while let Some(delta) = stream.next().await {
        if let Ok(Delta::Text(chunk)) = delta {
            summary.push_str(&chunk);
        }
    }

    if summary.trim().is_empty() {
        return Err(ConsolidateError::Other(anyhow::anyhow!(
            "consolidation produced empty summary"
        )));
    }

    let embedding = match provider.embed(embed_model, &summary).await {
        Ok(v) => v,
        Err(ProviderError::Unsupported(_)) => return Err(ConsolidateError::UnsupportedEmbed),
        Err(e) => {
            return Err(ConsolidateError::Other(anyhow::anyhow!(
                "embed failed: {e}"
            )));
        }
    };

    memory.insert("__consolidated__", &summary, &embedding)?;

    let ids: Vec<String> = memories.iter().map(|m| m.id.clone()).collect();
    memory.delete_memories(&ids)?;

    Ok(memories.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream;
    use harness_provider_core::{ChatRequest, DeltaStream, Pricing, Provider, StopReason};
    use tempfile::tempdir;

    struct FakeProvider {
        summary: String,
        embed: Vec<f32>,
    }

    #[async_trait]
    impl Provider for FakeProvider {
        fn name(&self) -> &str {
            "fake"
        }

        fn model(&self) -> &str {
            "fake-model"
        }

        async fn stream_chat(&self, _req: ChatRequest) -> Result<DeltaStream, ProviderError> {
            let summary = self.summary.clone();
            let s = stream::iter(vec![
                Ok(Delta::Text(summary)),
                Ok(Delta::Done {
                    stop_reason: StopReason::EndTurn,
                }),
            ]);
            Ok(Box::pin(s))
        }

        async fn embed(&self, _model: &str, _text: &str) -> Result<Vec<f32>, ProviderError> {
            Ok(self.embed.clone())
        }

        fn pricing(&self) -> Option<Pricing> {
            None
        }
    }

    fn sample_embedding(seed: f32) -> Vec<f32> {
        vec![seed, 1.0 - seed, 0.5]
    }

    #[tokio::test]
    async fn consolidate_merges_memories_into_one() {
        let dir = tempdir().unwrap();
        let store = MemoryStore::open(dir.path().join("mem.db")).unwrap();
        let mut ids = Vec::new();
        for i in 0..5 {
            let id = store
                .insert(
                    "sess",
                    &format!("memory fragment {i}"),
                    &sample_embedding(i as f32),
                )
                .unwrap();
            ids.push(id);
        }

        let provider: ArcProvider = Arc::new(FakeProvider {
            summary: "consolidated summary paragraph".into(),
            embed: sample_embedding(0.9),
        });

        let deleted = consolidate(&provider, &store, "embed-model", 5, 20, None)
            .await
            .expect("consolidate");
        assert_eq!(deleted, 5);
        assert_eq!(store.count_all().unwrap(), 1);

        let recent = store.recent_memories(5).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].session_id, "__consolidated__");
        assert_eq!(recent[0].text, "consolidated summary paragraph");
        for id in ids {
            assert!(!recent.iter().any(|m| m.id == id));
        }
    }

    #[tokio::test]
    async fn consolidate_skips_when_fewer_than_min_new() {
        let dir = tempdir().unwrap();
        let store = MemoryStore::open(dir.path().join("mem.db")).unwrap();
        store
            .insert("sess", "only one", &sample_embedding(0.1))
            .unwrap();

        let provider: ArcProvider = Arc::new(FakeProvider {
            summary: "unused".into(),
            embed: sample_embedding(0.2),
        });

        let deleted = consolidate(&provider, &store, "embed-model", 5, 20, None)
            .await
            .expect("consolidate");
        assert_eq!(deleted, 0);
        assert_eq!(store.count_all().unwrap(), 1);
    }

    #[tokio::test]
    async fn consolidate_returns_unsupported_when_embed_missing() {
        struct NoEmbedProvider;

        #[async_trait]
        impl Provider for NoEmbedProvider {
            fn name(&self) -> &str {
                "no-embed"
            }
            fn model(&self) -> &str {
                "m"
            }
            async fn stream_chat(&self, _req: ChatRequest) -> Result<DeltaStream, ProviderError> {
                let s = stream::iter(vec![
                    Ok(Delta::Text("summary".into())),
                    Ok(Delta::Done {
                        stop_reason: StopReason::EndTurn,
                    }),
                ]);
                Ok(Box::pin(s))
            }
        }

        let dir = tempdir().unwrap();
        let store = MemoryStore::open(dir.path().join("mem.db")).unwrap();
        for i in 0..5 {
            store
                .insert("sess", &format!("m{i}"), &sample_embedding(i as f32))
                .unwrap();
        }

        let provider: ArcProvider = Arc::new(NoEmbedProvider);
        let err = consolidate(&provider, &store, "embed-model", 5, 20, None)
            .await
            .unwrap_err();
        assert!(matches!(err, ConsolidateError::UnsupportedEmbed));
    }
}
