//! Anthropic Claude provider for Harness.
//!
//! Implements the `Provider` trait using Claude's streaming messages API.
//! Supports prompt caching (up to 4 cache breakpoints), tool use, and embeddings
//! via Voyage AI (Anthropic's recommended embedding provider).

use async_trait::async_trait;
use futures::StreamExt;
use harness_provider_core::{
    ChatRequest, Delta, DeltaStream, Pricing, Provider, ProviderError, Role, StopReason, ToolCall,
    ToolCallFunction,
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
            model: "claude-sonnet-4-6".into(),
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
    /// System as a structured array so we can attach cache_control.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    system: Vec<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Value>,
    stream: bool,
    temperature: f32,
    /// Extended thinking config (omitted when None).
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<Value>,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    content: Value,
}

/// Build the system block array with a cache breakpoint on the final block.
fn build_system_blocks(text: &str) -> Vec<Value> {
    vec![json!({
        "type": "text",
        "text": text,
        "cache_control": { "type": "ephemeral" }
    })]
}

fn build_api_messages(req: &ChatRequest) -> Vec<ApiMessage> {
    let mut msgs = Vec::new();
    let total = req.messages.len();

    for (idx, msg) in req.messages.iter().enumerate() {
        let (role, content) = match &msg.role {
            Role::User => {
                let text = msg.content.as_str();
                // Attach a rolling cache breakpoint to the second-to-last user message
                // (Anthropic supports up to 4 cache breakpoints per request).
                // Also cache @file-pinned content and the very last user message.
                let should_cache = idx + 1 == total     // last message
                    || idx + 2 == total                  // second-to-last
                    || text.contains("@file:"); // pinned file reference
                if should_cache {
                    (
                        "user",
                        json!([{
                            "type": "text",
                            "text": text,
                            "cache_control": { "type": "ephemeral" }
                        }]),
                    )
                } else {
                    ("user", json!(text))
                }
            }
            Role::Assistant => {
                let s = msg.content.as_str();
                if let Some(stripped) = s.strip_prefix("__tool_calls__:") {
                    if let Ok(calls) = serde_json::from_str::<Vec<Value>>(stripped) {
                        let content_blocks: Vec<Value> = calls
                            .iter()
                            .map(|c| {
                                let name = c["function"]["name"].as_str().unwrap_or("");
                                let args_str = c["function"]["arguments"].as_str().unwrap_or("{}");
                                let input: Value =
                                    serde_json::from_str(args_str).unwrap_or(json!({}));
                                json!({
                                    "type": "tool_use",
                                    "id": c["id"].as_str().unwrap_or("tool_0"),
                                    "name": name,
                                    "input": input
                                })
                            })
                            .collect();
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
        msgs.push(ApiMessage {
            role: role.into(),
            content,
        });
    }
    msgs
}

/// Build tool schemas with a cache breakpoint on the last tool (so the whole
/// tool list can be cached across turns).
fn build_tool_schemas(tools: &[harness_provider_core::ToolDefinition]) -> Vec<Value> {
    let len = tools.len();
    tools
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut def = json!({
                "name": t.function.name,
                "description": t.function.description,
                "input_schema": t.function.parameters
            });
            // Cache the last tool entry so the whole tool list is cached.
            if i + 1 == len {
                def["cache_control"] = json!({ "type": "ephemeral" });
            }
            def
        })
        .collect()
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
        // Opus 4.7 / 4.6 / 4.5: $5/$25, cached $0.50
        if m.contains("opus-4-7") || m.contains("opus-4-6") || m.contains("opus-4-5") {
            Some(Pricing {
                input_per_m_usd: 5.00,
                cached_input_per_m_usd: 0.50,
                output_per_m_usd: 25.00,
            })
        // Opus 4.1 / 4 (legacy): $15/$75, cached $1.50
        } else if m.contains("opus") {
            Some(Pricing {
                input_per_m_usd: 15.00,
                cached_input_per_m_usd: 1.50,
                output_per_m_usd: 75.00,
            })
        // Sonnet 4.x: $3/$15, cached $0.30
        } else if m.contains("sonnet") {
            Some(Pricing {
                input_per_m_usd: 3.00,
                cached_input_per_m_usd: 0.30,
                output_per_m_usd: 15.00,
            })
        // Haiku 4.5: $1/$5, cached $0.10
        } else if m.contains("haiku-4-5") {
            Some(Pricing {
                input_per_m_usd: 1.00,
                cached_input_per_m_usd: 0.10,
                output_per_m_usd: 5.00,
            })
        // Haiku legacy: $0.80/$4
        } else if m.contains("haiku") {
            Some(Pricing {
                input_per_m_usd: 0.80,
                cached_input_per_m_usd: 0.08,
                output_per_m_usd: 4.00,
            })
        } else {
            None
        }
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError> {
        let messages = build_api_messages(&req);
        let mut tools = build_tool_schemas(&req.tools);
        let system = req
            .system
            .as_deref()
            .map(build_system_blocks)
            .unwrap_or_default();

        // Anthropic structured output: inject a synthetic tool that forces
        // the model to return JSON matching the schema.
        if let Some(rs) = &req.response_schema {
            tools.push(json!({
                "name": format!("respond_{}", rs.name),
                "description": format!("Respond with valid JSON matching the {} schema.", rs.name),
                "input_schema": rs.schema
            }));
        }

        // Append Anthropic native server-side tools when requested.
        if req.native_web_search {
            tools.push(json!({
                "type": "web_search_20250305",
                "name": "web_search"
            }));
        }
        if req.native_code_execution {
            tools.push(json!({
                "type": "bash_20250124",
                "name": "bash"
            }));
        }

        // Extended / adaptive thinking support.
        // Opus 4.7 supports adaptive thinking (no explicit budget needed).
        // Other Claude 4.x models need an explicit budget to activate thinking.
        let model_lower = self.config.model.to_lowercase();
        let supports_thinking = model_lower.contains("opus-4-7")
            || model_lower.contains("sonnet-4-6")
            || model_lower.contains("opus-4-6")
            || model_lower.contains("opus-4-5");

        let (thinking, temperature, betas) = if supports_thinking {
            if let Some(budget) = req.thinking_budget {
                // Explicit budget: extended thinking. Temperature must be 1.0.
                let thinking = json!({
                    "type": "enabled",
                    "budget_tokens": budget
                });
                (
                    Some(thinking),
                    1.0f32,
                    Some(vec!["interleaved-thinking-2025-05-14".to_string()]),
                )
            } else if model_lower.contains("opus-4-7") {
                // Opus 4.7: adaptive thinking (model decides, no explicit config needed)
                (None, self.config.temperature, None)
            } else {
                (None, self.config.temperature, None)
            }
        } else {
            (None, self.config.temperature, None)
        };

        let body = ApiRequest {
            model: self.config.model.clone(),
            max_tokens: self.config.max_tokens,
            messages,
            system,
            tools,
            stream: true,
            temperature,
            thinking,
        };

        let url = format!("{}/messages", self.config.base_url);

        const MAX_RETRIES: u32 = 4;
        let mut attempt = 0u32;

        loop {
            let mut builder = self
                .client
                .post(&url)
                .header("x-api-key", &self.config.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json");

            // Send beta features header if extended thinking is active
            if let Some(beta_list) = &betas {
                builder = builder.header("anthropic-beta", beta_list.join(","));
            }

            let resp = builder
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
                warn!(
                    status = status.as_u16(),
                    attempt, delay_ms, "Anthropic retryable error"
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                attempt += 1;
                continue;
            }

            let msg = resp.text().await.unwrap_or_else(|_| "<unreadable>".into());
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message: msg,
            });
        }
    }
}

#[allow(clippy::collapsible_match)]
fn parse_anthropic_sse(
    byte_stream: impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
) -> impl futures::Stream<Item = Result<Delta, ProviderError>> + Send {
    use std::pin::Pin;
    type ByteStream =
        Pin<Box<dyn futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>;

    struct State {
        stream: ByteStream,
        buf: String,
        tool_id: String,
        tool_name: String,
        tool_args: String,
        in_tool: bool,
        input_tokens: u32,
        output_tokens: u32,
        cache_creation_tokens: u32,
        cache_read_tokens: u32,
        pending_stop_reason: Option<StopReason>,
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
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
        pending_stop_reason: None,
        done: false,
    };

    futures::stream::unfold(state, |mut s| async move {
        if s.done {
            return None;
        }

        // If we already have a pending stop reason (emitted Usage, now emit Done)
        if let Some(sr) = s.pending_stop_reason.take() {
            s.done = true;
            return Some((Ok(Delta::Done { stop_reason: sr }), s));
        }

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
                                    s.tool_id = event["content_block"]["id"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();
                                    s.tool_name = event["content_block"]["name"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();
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
                                    let call =
                                        make_tool_call(&s.tool_id, &s.tool_name, &s.tool_args);
                                    return Some((Ok(Delta::ToolCall(call)), s));
                                }
                            }
                            Some("message_delta") => {
                                if let Some(u) = event["usage"]["output_tokens"].as_u64() {
                                    s.output_tokens = u as u32;
                                }
                                let stop_reason =
                                    event["delta"]["stop_reason"].as_str().unwrap_or("");
                                let sr = match stop_reason {
                                    "tool_use" => StopReason::ToolUse,
                                    "max_tokens" => StopReason::MaxTokens,
                                    _ => StopReason::EndTurn,
                                };
                                let it = s.input_tokens;
                                let ot = s.output_tokens;
                                let cc = s.cache_creation_tokens;
                                let cr = s.cache_read_tokens;
                                if it > 0 || ot > 0 {
                                    // Emit Usage, then Done on the next poll
                                    s.pending_stop_reason = Some(sr);
                                    return Some((
                                        Ok(Delta::Usage {
                                            input_tokens: it,
                                            output_tokens: ot,
                                        }),
                                        s,
                                    ));
                                } else if cc > 0 || cr > 0 {
                                    s.pending_stop_reason = Some(sr);
                                    return Some((
                                        Ok(Delta::CacheUsage {
                                            cache_creation_tokens: cc,
                                            cache_read_tokens: cr,
                                        }),
                                        s,
                                    ));
                                }
                                s.done = true;
                                return Some((Ok(Delta::Done { stop_reason: sr }), s));
                            }
                            Some("message_start") => {
                                let usage = &event["message"]["usage"];
                                if let Some(u) = usage["input_tokens"].as_u64() {
                                    s.input_tokens = u as u32;
                                }
                                // Prompt caching stats
                                if let Some(u) = usage["cache_creation_input_tokens"].as_u64() {
                                    s.cache_creation_tokens = u as u32;
                                }
                                if let Some(u) = usage["cache_read_input_tokens"].as_u64() {
                                    s.cache_read_tokens = u as u32;
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
                    return Some((
                        Ok(Delta::Done {
                            stop_reason: StopReason::EndTurn,
                        }),
                        s,
                    ));
                }
            }
        }
    })
}
