//! Ollama local model provider for Harness.
//!
//! Uses Ollama's OpenAI-compatible `/api/chat` endpoint for streaming chat
//! and `/api/embeddings` for text embeddings.

use async_trait::async_trait;
use futures::StreamExt;
use harness_provider_core::{
    ChatRequest, Delta, DeltaStream, Pricing, Provider, ProviderError,
    Role, StopReason, ToolCall, ToolCallFunction,
};
use reqwest::Client;
use serde_json::{json, Value};
use tracing::debug;

const OLLAMA_BASE_URL: &str = "http://localhost:11434";

#[derive(Debug, Clone)]
pub struct OllamaConfig {
    pub model: String,
    pub embed_model: String,
    pub base_url: String,
    pub max_tokens: u32,
    pub temperature: f32,
}

impl OllamaConfig {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            embed_model: "nomic-embed-text".into(),
            base_url: OLLAMA_BASE_URL.into(),
            max_tokens: 8192,
            temperature: 0.7,
        }
    }

    pub fn with_embed_model(mut self, model: impl Into<String>) -> Self {
        self.embed_model = model.into();
        self
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
}

#[derive(Clone)]
pub struct OllamaProvider {
    pub config: OllamaConfig,
    client: Client,
}

impl OllamaProvider {
    pub fn new(config: OllamaConfig) -> anyhow::Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()?;
        Ok(Self { config, client })
    }
}

fn build_messages(req: &ChatRequest) -> Vec<Value> {
    let mut msgs: Vec<Value> = Vec::new();

    if let Some(sys) = &req.system {
        msgs.push(json!({"role": "system", "content": sys}));
    }

    for msg in &req.messages {
        match &msg.role {
            Role::System => msgs.push(json!({"role": "system", "content": msg.content.as_str()})),
            Role::User => msgs.push(json!({"role": "user", "content": msg.content.as_str()})),
            Role::Assistant => msgs.push(json!({"role": "assistant", "content": msg.content.as_str()})),
            Role::Tool => msgs.push(json!({
                "role": "tool",
                "content": msg.content.as_str()
            })),
        }
    }
    msgs
}

fn build_tools(tools: &[harness_provider_core::ToolDefinition]) -> Vec<Value> {
    tools.iter().map(|t| json!({
        "type": "function",
        "function": {
            "name": t.function.name,
            "description": t.function.description,
            "parameters": t.function.parameters
        }
    })).collect()
}

#[async_trait]
impl Provider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    fn pricing(&self) -> Option<Pricing> {
        // Local models are free.
        Some(Pricing { input_per_m_usd: 0.0, cached_input_per_m_usd: 0.0, output_per_m_usd: 0.0 })
    }

    async fn embed(&self, model: &str, text: &str) -> Result<Vec<f32>, ProviderError> {
        let url = format!("{}/api/embeddings", self.config.base_url);
        let resp = self.client
            .post(&url)
            .json(&json!({"model": model, "prompt": text}))
            .send()
            .await
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        if !resp.status().is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api { status: 0, message: msg });
        }

        let body: Value = resp.json().await.map_err(|e| ProviderError::Other(e.to_string()))?;
        let emb: Vec<f32> = body["embedding"]
            .as_array()
            .ok_or_else(|| ProviderError::Other("missing embedding in Ollama response".into()))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        Ok(emb)
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError> {
        let messages = build_messages(&req);
        let tools = build_tools(&req.tools);
        let has_tools = !tools.is_empty();

        // Use OpenAI-compat endpoint if Ollama version >= 0.5.
        let url = format!("{}/api/chat", self.config.base_url);

        let mut body = json!({
            "model": self.config.model,
            "messages": messages,
            "stream": true,
            "options": {
                "num_predict": self.config.max_tokens,
                "temperature": self.config.temperature
            }
        });

        if has_tools {
            body["tools"] = json!(tools);
        }

        debug!(model = %self.config.model, "sending Ollama chat request");

        let resp = self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api { status: status.as_u16(), message: msg });
        }

        let stream = parse_ollama_stream(resp.bytes_stream());
        Ok(Box::pin(stream))
    }
}

fn parse_ollama_stream(
    byte_stream: impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
) -> impl futures::Stream<Item = Result<Delta, ProviderError>> + Send {
    use std::pin::Pin;
    type ByteStream = Pin<Box<dyn futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>;

    struct State {
        stream: ByteStream,
        buf: String,
        done: bool,
    }

    let state = State {
        stream: Box::pin(byte_stream),
        buf: String::new(),
        done: false,
    };

    futures::stream::unfold(state, |mut s| async move {
        if s.done { return None; }

        loop {
            while let Some(nl) = s.buf.find('\n') {
                let line = s.buf[..nl].trim().to_string();
                s.buf = s.buf[nl + 1..].to_string();

                if line.is_empty() { continue; }

                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    // Tool calls — emit first one; subsequent calls handled next iteration
                    if let Some(call) = v["message"]["tool_calls"].as_array().and_then(|a| a.first()) {
                        let id = call["id"].as_str().unwrap_or("tool_0").to_string();
                        let name = call["function"]["name"].as_str().unwrap_or("").to_string();
                        let args = call["function"]["arguments"].to_string();
                        return Some((Ok(Delta::ToolCall(ToolCall {
                            id, kind: "function".into(),
                            function: ToolCallFunction { name, arguments: args },
                        })), s));
                    }

                    // Text content
                    if let Some(content) = v["message"]["content"].as_str() {
                        if !content.is_empty() {
                            return Some((Ok(Delta::Text(content.to_string())), s));
                        }
                    }

                    // Done
                    if v["done"].as_bool() == Some(true) {
                        let in_tok = v["prompt_eval_count"].as_u64().unwrap_or(0) as u32;
                        let out_tok = v["eval_count"].as_u64().unwrap_or(0) as u32;
                        s.done = true;
                        if in_tok > 0 || out_tok > 0 {
                            return Some((Ok(Delta::Usage { input_tokens: in_tok, output_tokens: out_tok }), s));
                        }
                        return Some((Ok(Delta::Done { stop_reason: StopReason::EndTurn }), s));
                    }
                }
            }

            match s.stream.next().await {
                Some(Ok(chunk)) => s.buf.push_str(&String::from_utf8_lossy(&chunk)),
                Some(Err(e)) => { s.done = true; return Some((Err(ProviderError::Other(e.to_string())), s)); }
                None => { s.done = true; return Some((Ok(Delta::Done { stop_reason: StopReason::EndTurn }), s)); }
            }
        }
    })
}
