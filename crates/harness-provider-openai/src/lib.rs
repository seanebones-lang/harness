//! OpenAI provider for Harness.
//!
//! Implements the `Provider` trait using OpenAI's streaming chat completions API
//! (compatible with any OpenAI-format endpoint).

use async_trait::async_trait;
use futures::StreamExt;
use harness_provider_core::{
    ChatRequest, Delta, DeltaStream, Pricing, Provider, ProviderError,
    Role, StopReason, ToolCall, ToolCallFunction,
};
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use tracing::warn;

const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub base_url: String,
}

impl OpenAIConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "gpt-4o".into(),
            max_tokens: 8192,
            temperature: 0.7,
            base_url: OPENAI_BASE_URL.into(),
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
}

#[derive(Clone)]
pub struct OpenAIProvider {
    pub config: OpenAIConfig,
    client: Client,
}

impl OpenAIProvider {
    pub fn new(config: OpenAIConfig) -> anyhow::Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;
        Ok(Self { config, client })
    }
}

// ── API types ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ApiRequest {
    model: String,
    messages: Vec<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Value>,
    max_tokens: u32,
    temperature: f32,
    stream: bool,
    stream_options: StreamOptions,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

fn build_api_messages(req: &ChatRequest) -> Vec<Value> {
    let mut msgs: Vec<Value> = Vec::new();

    if let Some(sys) = &req.system {
        msgs.push(json!({"role": "system", "content": sys}));
    }

    for msg in &req.messages {
        match &msg.role {
            Role::System => {
                msgs.push(json!({"role": "system", "content": msg.content.as_str()}));
            }
            Role::User => {
                msgs.push(json!({"role": "user", "content": msg.content.as_str()}));
            }
            Role::Assistant => {
                let s = msg.content.as_str();
                if let Some(stripped) = s.strip_prefix("__tool_calls__:") {
                    if let Ok(calls) = serde_json::from_str::<Vec<Value>>(stripped) {
                        msgs.push(json!({
                            "role": "assistant",
                            "content": null,
                            "tool_calls": calls
                        }));
                    } else {
                        msgs.push(json!({"role": "assistant", "content": s}));
                    }
                } else {
                    msgs.push(json!({"role": "assistant", "content": s}));
                }
            }
            Role::Tool => {
                msgs.push(json!({
                    "role": "tool",
                    "tool_call_id": msg.tool_call_id.as_deref().unwrap_or(""),
                    "content": msg.content.as_str()
                }));
            }
        }
    }
    msgs
}

fn build_tool_schemas(tools: &[harness_provider_core::ToolDefinition]) -> Vec<Value> {
    tools.iter().map(|t| {
        json!({
            "type": "function",
            "function": {
                "name": t.function.name,
                "description": t.function.description,
                "parameters": t.function.parameters
            }
        })
    }).collect()
}

#[async_trait]
impl Provider for OpenAIProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    fn pricing(&self) -> Option<Pricing> {
        let m = self.config.model.to_lowercase();
        if m.contains("gpt-4o") && m.contains("mini") {
            Some(Pricing { input_per_m_usd: 0.15, output_per_m_usd: 0.60 })
        } else if m.contains("gpt-4o") || m.contains("gpt-4") {
            Some(Pricing { input_per_m_usd: 2.50, output_per_m_usd: 10.00 })
        } else if m.contains("gpt-3.5") {
            Some(Pricing { input_per_m_usd: 0.50, output_per_m_usd: 1.50 })
        } else if m.contains("o3") || m.contains("o4") {
            Some(Pricing { input_per_m_usd: 10.00, output_per_m_usd: 40.00 })
        } else {
            None
        }
    }

    async fn embed(&self, _model: &str, text: &str) -> Result<Vec<f32>, ProviderError> {
        let url = format!("{}/embeddings", self.config.base_url);
        let resp = self.client
            .post(&url)
            .bearer_auth(&self.config.api_key)
            .json(&json!({
                "model": "text-embedding-3-small",
                "input": text
            }))
            .send()
            .await
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        if !resp.status().is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api { status: 0, message: msg });
        }

        let body: Value = resp.json().await.map_err(|e| ProviderError::Other(e.to_string()))?;
        let emb: Vec<f32> = body["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| ProviderError::Other("missing embedding".into()))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        Ok(emb)
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError> {
        let messages = build_api_messages(&req);
        let tools = build_tool_schemas(&req.tools);

        let body = ApiRequest {
            model: self.config.model.clone(),
            messages,
            tools,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
            stream: true,
            stream_options: StreamOptions { include_usage: true },
        };

        let url = format!("{}/chat/completions", self.config.base_url);

        const MAX_RETRIES: u32 = 4;
        let mut attempt = 0u32;

        loop {
            let resp = self.client
                .post(&url)
                .bearer_auth(&self.config.api_key)
                .json(&body)
                .send()
                .await
                .map_err(|e| ProviderError::Other(e.to_string()))?;

            let status = resp.status();
            if status.is_success() {
                let stream = parse_openai_sse(resp.bytes_stream());
                return Ok(Box::pin(stream));
            }

            let retryable = matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504);
            if retryable && attempt < MAX_RETRIES {
                let delay_ms = 1000u64 << attempt;
                warn!(status = status.as_u16(), attempt, "OpenAI retryable error");
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                attempt += 1;
                continue;
            }

            let msg = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api { status: status.as_u16(), message: msg });
        }
    }
}

fn parse_openai_sse(
    byte_stream: impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
) -> impl futures::Stream<Item = Result<Delta, ProviderError>> + Send {
    use std::pin::Pin;
    type ByteStream = Pin<Box<dyn futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>;

    struct State {
        stream: ByteStream,
        buf: String,
        // index → (id, name, accumulated_args)
        tool_calls: std::collections::HashMap<u32, (String, String, String)>,
        done: bool,
    }

    let state = State {
        stream: Box::pin(byte_stream),
        buf: String::new(),
        tool_calls: std::collections::HashMap::new(),
        done: false,
    };

    futures::stream::unfold(state, |mut s| async move {
        if s.done { return None; }

        loop {
            while let Some(nl) = s.buf.find('\n') {
                let line = s.buf[..nl].trim_end_matches('\r').to_string();
                s.buf = s.buf[nl + 1..].to_string();

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        s.done = true;
                        return Some((Ok(Delta::Done { stop_reason: StopReason::EndTurn }), s));
                    }

                    if let Ok(v) = serde_json::from_str::<Value>(data) {
                        // Usage (may appear in a separate chunk or on the last choice)
                        if let Some(usage) = v.get("usage") {
                            let in_tok = usage["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                            let out_tok = usage["completion_tokens"].as_u64().unwrap_or(0) as u32;
                            if in_tok > 0 || out_tok > 0 {
                                return Some((Ok(Delta::Usage { input_tokens: in_tok, output_tokens: out_tok }), s));
                            }
                        }

                        if let Some(choices) = v["choices"].as_array() {
                            for choice in choices {
                                let delta = &choice["delta"];
                                let finish_reason = choice["finish_reason"].as_str();

                                if let Some(text) = delta["content"].as_str() {
                                    if !text.is_empty() {
                                        return Some((Ok(Delta::Text(text.to_string())), s));
                                    }
                                }

                                if let Some(tc_arr) = delta["tool_calls"].as_array() {
                                    for tc in tc_arr {
                                        let idx = tc["index"].as_u64().unwrap_or(0) as u32;
                                        let entry = s.tool_calls.entry(idx).or_default();
                                        if let Some(id) = tc["id"].as_str() { entry.0 = id.to_string(); }
                                        if let Some(name) = tc["function"]["name"].as_str() { entry.1 = name.to_string(); }
                                        if let Some(args) = tc["function"]["arguments"].as_str() { entry.2.push_str(args); }
                                    }
                                }

                                if let Some(reason) = finish_reason {
                                    // Flush accumulated tool calls.
                                    if !s.tool_calls.is_empty() {
                                        let mut sorted: Vec<_> = s.tool_calls.drain().collect();
                                        sorted.sort_by_key(|(k, _)| *k);
                                        // Return the first one; downstream agent will call again.
                                        if let Some((_, (id, name, args))) = sorted.into_iter().next() {
                                            let call = ToolCall { id, kind: "function".into(), function: ToolCallFunction { name, arguments: args } };
                                            return Some((Ok(Delta::ToolCall(call)), s));
                                        }
                                    }

                                    let sr = match reason {
                                        "tool_calls" => StopReason::ToolUse,
                                        "length" => StopReason::MaxTokens,
                                        _ => StopReason::EndTurn,
                                    };
                                    s.done = true;
                                    return Some((Ok(Delta::Done { stop_reason: sr }), s));
                                }
                            }
                        }
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
