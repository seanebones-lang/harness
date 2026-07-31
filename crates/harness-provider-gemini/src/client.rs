use async_trait::async_trait;
use harness_provider_core::{Provider, Message, Delta, ResponseSchema};
use reqwest::Client;
use serde_json::json;
use std::env;

use crate::types::GeminiConfig;
use crate::stream::stream_chat;

pub struct GeminiProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl GeminiProvider {
    pub fn new(config: GeminiConfig) -> Self {
        let api_key = config.api_key.unwrap_or_else(|| env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY required"));
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
        }
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    async fn chat(&self, messages: Vec<Message>, schema: Option<ResponseSchema>) -> anyhow::Result<String> {
        // OpenAI-compatible call
        let body = json!({
            "model": "gemini-1.5-pro",
            "messages": messages,
        });
        let resp = self.client.post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let text = resp.text().await?;
        Ok(text)
    }

    async fn stream_chat(&self, messages: Vec<Message>, schema: Option<ResponseSchema>) -> anyhow::Result<impl futures::Stream<Item = Delta>> {
        stream_chat(&self.client, &self.api_key, &self.base_url, messages).await
    }
}