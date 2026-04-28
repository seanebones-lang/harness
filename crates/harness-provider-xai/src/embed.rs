use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

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
    const MAX_RETRIES: u32 = 4;
    const BASE_DELAY_MS: u64 = 1000;

    let mut attempt = 0u32;
    loop {
        let resp = client
            .post(&url)
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
            .context("embedding request failed")?;

        let status = resp.status();
        let retryable = matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504);

        if status.is_success() {
            let parsed: EmbedResponse = resp.json().await.context("parsing embedding response")?;
            return parsed
                .data
                .into_iter()
                .next()
                .map(|d| d.embedding)
                .context("empty embedding response");
        }

        if retryable && attempt < MAX_RETRIES {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());
            let delay_ms = retry_after
                .map(|s| s * 1000)
                .unwrap_or(BASE_DELAY_MS << attempt);
            warn!(
                status = status.as_u16(),
                attempt,
                delay_ms,
                "xAI embedding retryable error; waiting before retry"
            );
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            attempt += 1;
            continue;
        }

        let msg = resp.text().await.unwrap_or_default();
        anyhow::bail!("embedding API error {status}: {msg}");
    }
}
