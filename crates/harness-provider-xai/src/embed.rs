use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedDatum>,
}

#[derive(Deserialize)]
struct EmbedDatum {
    embedding: Vec<f32>,
}

/// Calls the xAI (OpenAI-compatible) embeddings endpoint.
/// Returns the raw float vector for the given text.
pub async fn embed_text(
    client: &Client,
    api_key: &str,
    base_url: &str,
    model: &str,
    text: &str,
) -> Result<Vec<f32>> {
    let url = format!("{base_url}/embeddings");
    debug!(model, chars = text.len(), "embedding text");

    let body = EmbedRequest { model, input: text };
    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("embedding request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let msg = resp.text().await.unwrap_or_default();
        anyhow::bail!("embedding API error {status}: {msg}");
    }

    let parsed: EmbedResponse = resp.json().await.context("parsing embedding response")?;
    parsed
        .data
        .into_iter()
        .next()
        .map(|d| d.embedding)
        .context("empty embedding response")
}
