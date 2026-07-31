use futures::Stream;
use harness_provider_core::Delta;
use reqwest::Client;
use serde_json::json;

pub async fn stream_chat(
    client: &Client,
    api_key: &str,
    base_url: &str,
    messages: Vec<harness_provider_core::Message>,
) -> anyhow::Result<impl Stream<Item = Delta>> {
    // Stub SSE stream (match OpenAI pattern)
    let body = json!({ "model": "gemini-1.5-pro", "messages": messages, "stream": true });
    // TODO: full SSE parsing
    let _ = client.post(format!("{}/chat/completions", base_url))
        .bearer_auth(api_key)
        .json(&body);
    Ok(futures::stream::empty::<Delta>())
}