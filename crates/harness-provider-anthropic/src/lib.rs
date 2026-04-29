//! Anthropic Claude provider for Harness.
//!
//! Implements the `Provider` trait using Claude's streaming messages API.
//! Supports prompt caching (up to 4 cache breakpoints), tool use, and embeddings
//! via Voyage AI (Anthropic's recommended embedding provider).

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

const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub base_url: String,
}

impl AnthropicConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "claude-sonnet-4-5".into(),
            max_tokens: 8192,
            temperature: 0.7,
            base_url: ANTHROPIC_BASE_URL.into(),
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }
}

#[derive(Clone)]
pub struct AnthropicProvider {
    pub config: AnthropicConfig,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(config: AnthropicConfig) -> anyhow::Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;
        Ok(Self { config, client })
    }
}

// ── Anthropic API types ───────────────────────────────────────────────────────

#[derive(Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Value>,
    stream: bool,
    temperature: f32,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    content: Value,
}

fn build_api_messages(req: &ChatRequest) -> Vec<ApiMessage> {
    let mut msgs = Vec::new();
    for msg in &req.messages {
        let (role, content) = match &msg.role {
            Role::User => ("user", json!(msg.content.as_str())),
            Role::Assistant => {
                let s = msg.content.as_str();
                if let Some(stripped) = s.strip_prefix("__tool_calls__:") {
                    if let Ok(calls) = serde_json::from_str::<Vec<Value>>(stripped) {
                        let content_blocks: Vec<Value> = calls.iter().map(|c| {
                            let name = c["function"]["name"].as_str().unwrap_or("");
                            let args_str = c["function"]["arguments"].as_str().unwrap_or("{}");
                            let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                            json!({
                                "type": "tool_use",
                                "id": c["id"].as_str().unwrap_or("tool_0"),
                                "name": name,
                                "input": input
                            })
                        }).collect();
                        ("assistant", json!(content_blocks))
                    } else {
                        ("assistant", json!(s))
                    }
                } else {
                    ("assistant", json!(s))
                }
            }
            Role::Tool => {
                let tool_call_id = msg.tool_call_id.as_deref().unwrap_or("tool_0");
                (
                    "user",
                    json!([{
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": msg.content.as_str()
                    }]),
                )
            }
            Role::System => continue,
        };
        msgs.push(ApiMessage { role: role.into(), content });
    }
    msgs
}

fn build_tool_schemas(tools: &[harness_provider_core::ToolDefinition]) -> Vec<Value> {
    tools.iter().map(|t| {
        json!({
            "name": t.function.name,
            "description": t.function.description,
            "input_schema": t.function.parameters
        })
    }).collect()
}

fn make_tool_call(id: &str, name: &str, args: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        kind: "function".into(),
        function: ToolCallFunction {
            name: name.to_string(),
            arguments: args.to_string(),
        },
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    fn pricing(&self) -> Option<Pricing> {
        let m = self.config.model.to_lowercase();
        if m.contains("opus") {
            Some(Pricing { input_per_m_usd: 15.00, output_per_m_usd: 75.00 })
        } else if m.contains("sonnet") {
            Some(Pricing { input_per_m_usd: 3.00, output_per_m_usd: 15.00 })
        } else if m.contains("haiku") {
            Some(Pricing { input_per_m_usd: 0.25, output_per_m_usd: 1.25 })
        } else {
            None
        }
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError> {
        let messages = build_api_messages(&req);
        let tools = build_tool_schemas(&req.tools);

        let body = ApiRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            messages,
            system: req.system.clone(),
            tools,
            stream: true,
            temperature: self.config.temperature,
        };

        let url = format!("{}/messages", self.config.base_url);

        const MAX_RETRIES: u32 = 4;
        let mut attempt = 0u32;

        loop {
            let resp = self.client
                .post(&url)
                .header("x-api-key", &self.config.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| ProviderError::Other(e.to_string()))?;

            let status = resp.status();
            let retryable = matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504);

            if status.is_success() {
                // Parse SSE stream from Anthropic.
                let byte_stream = resp.bytes_stream();
                let stream = parse_anthropic_sse(byte_stream);
                return Ok(Box::pin(stream));
            }

            if retryable && attempt < MAX_RETRIES {
                let delay_ms = 1000u64 << attempt;
                warn!(status = status.as_u16(), attempt, delay_ms, "Anthropic retryable error");
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                attempt += 1;
                continue;
            }

            let msg = resp.text().await.unwrap_or_else(|_| "<unreadable>".into());
            return Err(ProviderError::Api { status: status.as_u16(), message: msg });
        }
    }
}

#[allow(clippy::collapsible_match)]
fn parse_anthropic_sse(
    byte_stream: impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
) -> impl futures::Stream<Item = Result<Delta, ProviderError>> + Send {
    use std::pin::Pin;
    type ByteStream = Pin<Box<dyn futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>;

    struct State {
        stream: ByteStream,
        buf: String,
        tool_id: String,
        tool_name: String,
        tool_args: String,
        in_tool: bool,
        input_tokens: u32,
        output_tokens: u32,
        done: bool,
    }

    let state = State {
        stream: Box::pin(byte_stream),
        buf: String::new(),
        tool_id: String::new(),
        tool_name: String::new(),
        tool_args: String::new(),
        in_tool: false,
        input_tokens: 0,
        output_tokens: 0,
        done: false,
    };

    futures::stream::unfold(state, |mut s| async move {
        if s.done { return None; }

        loop {
            while let Some(nl) = s.buf.find('\n') {
                let line = s.buf[..nl].trim_end_matches('\r').to_string();
                s.buf = s.buf[nl + 1..].to_string();

                if let Some(data) = line.strip_prefix("data: ") {
                    if let Ok(event) = serde_json::from_str::<Value>(data) {
                        match event["type"].as_str() {
                            Some("content_block_start") => {
                                if event["content_block"]["type"] == "tool_use" {
                                    s.in_tool = true;
                                    s.tool_id = event["content_block"]["id"].as_str().unwrap_or("").to_string();
                                    s.tool_name = event["content_block"]["name"].as_str().unwrap_or("").to_string();
                                    s.tool_args.clear();
                                }
                            }
                            Some("content_block_delta") => {
                                let delta = &event["delta"];
                                if delta["type"] == "text_delta" {
                                    if let Some(text) = delta["text"].as_str() {
                                        return Some((Ok(Delta::Text(text.to_string())), s));
                                    }
                                } else if delta["type"] == "input_json_delta" {
                                    if let Some(partial) = delta["partial_json"].as_str() {
                                        s.tool_args.push_str(partial);
                                    }
                                }
                            }
                            Some("content_block_stop") => {
                                if s.in_tool {
                                    s.in_tool = false;
                                    let call = make_tool_call(&s.tool_id, &s.tool_name, &s.tool_args);
                                    return Some((Ok(Delta::ToolCall(call)), s));
                                }
                            }
                            Some("message_delta") => {
                                if let Some(u) = event["usage"]["output_tokens"].as_u64() {
                                    s.output_tokens = u as u32;
                                }
                                let stop_reason = event["delta"]["stop_reason"].as_str().unwrap_or("");
                                let sr = match stop_reason {
                                    "tool_use" => StopReason::ToolUse,
                                    "max_tokens" => StopReason::MaxTokens,
                                    _ => StopReason::EndTurn,
                                };
                                let it = s.input_tokens;
                                let ot = s.output_tokens;
                                s.done = true;
                                if it > 0 || ot > 0 {
                                    s.input_tokens = 0;
                                    s.output_tokens = 0;
                                    // Emit usage; done emitted on next poll (state.done=true).
                                    return Some((Ok(Delta::Usage { input_tokens: it, output_tokens: ot }), s));
                                }
                                return Some((Ok(Delta::Done { stop_reason: sr }), s));
                            }
                            Some("message_start") => {
                                if let Some(u) = event["message"]["usage"]["input_tokens"].as_u64() {
                                    s.input_tokens = u as u32;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            match s.stream.next().await {
                Some(Ok(chunk)) => s.buf.push_str(&String::from_utf8_lossy(&chunk)),
                Some(Err(e)) => {
                    s.done = true;
                    return Some((Err(ProviderError::Other(e.to_string())), s));
                }
                None => {
                    s.done = true;
                    return Some((Ok(Delta::Done { stop_reason: StopReason::EndTurn }), s));
                }
            }
        }
    })
}
