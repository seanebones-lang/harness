//! OpenAI provider for Harness.
//!
//! Implements the `Provider` trait using OpenAI's streaming chat completions API
//! (compatible with any OpenAI-format endpoint).

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

const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub base_url: String,
    /// Reported by `Provider::name` (e.g. `openai`, `mistral`, custom compatible id).
    pub provider_name: String,
}

impl OpenAIConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "gpt-5.5".into(),
            max_tokens: 8192,
            temperature: 0.7,
            base_url: OPENAI_BASE_URL.into(),
            provider_name: "openai".into(),
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

    pub fn with_provider_name(mut self, name: impl Into<String>) -> Self {
        self.provider_name = name.into();
        self
    }

    pub fn with_max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = n;
        self
    }

    pub fn with_temperature(mut self, t: f32) -> Self {
        self.temperature = t;
        self
    }

    /// Mistral AI OpenAI-compatible API defaults.
    pub fn mistral(api_key: impl Into<String>) -> Self {
        Self::new(api_key)
            .with_provider_name("mistral")
            .with_base_url("https://api.mistral.ai/v1")
            .with_model("mistral-large-latest")
    }

    /// Google Gemini via the OpenAI-compatible Generative Language API.
    ///
    /// Base: `https://generativelanguage.googleapis.com/v1beta/openai`
    /// Auth: `GEMINI_API_KEY` or `GOOGLE_API_KEY` (Bearer).
    pub fn gemini(api_key: impl Into<String>) -> Self {
        Self::new(api_key)
            .with_provider_name("gemini")
            .with_base_url("https://generativelanguage.googleapis.com/v1beta/openai")
            .with_model("gemini-2.0-flash")
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
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
    stream_options: StreamOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<Value>,
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
                msgs.push(json!({"role": "user", "content": msg.content}));
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
    tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.function.name,
                    "description": t.function.description,
                    "parameters": t.function.parameters
                }
            })
        })
        .collect()
}

fn build_api_request(req: &ChatRequest, config: &OpenAIConfig) -> ApiRequest {
    let model = req.effective_model(&config.model);
    let reasoning_model = ["gpt-5", "gpt-6", "o1", "o3", "o4"]
        .iter()
        .any(|prefix| model.starts_with(prefix));
    // Official OpenAI reasoning models reject legacy token/sampling parameters.
    // Compatible services retain their own established request format.
    let modern_limits = config.base_url.trim_end_matches('/') == OPENAI_BASE_URL && reasoning_model;
    ApiRequest {
        model: model.to_owned(),
        messages: build_api_messages(req),
        tools: build_tool_schemas(&req.tools),
        max_tokens: (!modern_limits).then_some(config.max_tokens),
        max_completion_tokens: modern_limits.then_some(config.max_tokens),
        temperature: (!modern_limits).then_some(config.temperature),
        stream: true,
        stream_options: StreamOptions { include_usage: true },
        response_format: req.response_schema.as_ref().map(|schema| json!({
            "type": "json_schema",
            "json_schema": { "name": schema.name, "schema": schema.schema, "strict": schema.strict }
        })),
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    fn name(&self) -> &str {
        &self.config.provider_name
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    fn pricing(&self) -> Option<Pricing> {
        let m = self.config.model.to_lowercase();
        // May 2026 GPT-5.x family
        if m.contains("gpt-5.5") {
            Some(Pricing {
                input_per_m_usd: 5.00,
                cached_input_per_m_usd: 0.50,
                output_per_m_usd: 30.00,
            })
        } else if m.contains("gpt-5.4-nano") {
            Some(Pricing {
                input_per_m_usd: 0.20,
                cached_input_per_m_usd: 0.02,
                output_per_m_usd: 1.25,
            })
        } else if m.contains("gpt-5.4-mini") {
            Some(Pricing {
                input_per_m_usd: 0.75,
                cached_input_per_m_usd: 0.075,
                output_per_m_usd: 4.50,
            })
        } else if m.contains("gpt-5.4") {
            Some(Pricing {
                input_per_m_usd: 2.50,
                cached_input_per_m_usd: 0.25,
                output_per_m_usd: 15.00,
            })
        } else if m.contains("gpt-5") {
            Some(Pricing {
                input_per_m_usd: 1.25,
                cached_input_per_m_usd: 0.125,
                output_per_m_usd: 10.00,
            })
        } else if m.contains("o4-mini") {
            Some(Pricing {
                input_per_m_usd: 1.10,
                cached_input_per_m_usd: 0.275,
                output_per_m_usd: 4.40,
            })
        } else if m.contains("o4") {
            Some(Pricing {
                input_per_m_usd: 2.00,
                cached_input_per_m_usd: 0.50,
                output_per_m_usd: 8.00,
            })
        } else if m.contains("o3") {
            Some(Pricing {
                input_per_m_usd: 1.00,
                cached_input_per_m_usd: 0.25,
                output_per_m_usd: 4.00,
            })
        // Legacy GPT-4o
        } else if m.contains("gpt-4o") && m.contains("mini") {
            Some(Pricing {
                input_per_m_usd: 0.15,
                cached_input_per_m_usd: 0.075,
                output_per_m_usd: 0.60,
            })
        } else if m.contains("gpt-4o") || m.contains("gpt-4") {
            Some(Pricing {
                input_per_m_usd: 2.50,
                cached_input_per_m_usd: 1.25,
                output_per_m_usd: 10.00,
            })
        } else if m.contains("gpt-3.5") {
            Some(Pricing {
                input_per_m_usd: 0.50,
                cached_input_per_m_usd: 0.0,
                output_per_m_usd: 1.50,
            })
        } else {
            None
        }
    }

    async fn embed(&self, model: &str, text: &str) -> Result<Vec<f32>, ProviderError> {
        let url = format!("{}/embeddings", self.config.base_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.config.api_key)
            .json(&json!({
                "model": model,
                "input": text
            }))
            .send()
            .await
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let msg = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                status,
                message: msg,
            });
        }

        let body: Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Other(e.to_string()))?;
        let emb: Vec<f32> = body["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| ProviderError::Other("missing embedding".into()))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        Ok(emb)
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError> {
        let body = build_api_request(&req, &self.config);

        let url = format!("{}/chat/completions", self.config.base_url);

        const MAX_RETRIES: u32 = 4;
        let mut attempt = 0u32;

        loop {
            let resp = self
                .client
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
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message: msg,
            });
        }
    }
}

fn parse_openai_sse(
    byte_stream: impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
) -> impl futures::Stream<Item = Result<Delta, ProviderError>> + Send {
    use std::{
        collections::{HashMap, VecDeque},
        pin::Pin,
    };
    type ByteStream =
        Pin<Box<dyn futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>;
    struct State {
        stream: ByteStream,
        lines: harness_provider_core::LineBuffer,
        tools: HashMap<u32, (String, String, String)>,
        queue: VecDeque<Delta>,
        finished: bool,
        eof: bool,
    }
    let state = State {
        stream: Box::pin(byte_stream),
        lines: Default::default(),
        tools: HashMap::new(),
        queue: VecDeque::new(),
        finished: false,
        eof: false,
    };
    futures::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(delta) = state.queue.pop_front() {
                return Some((Ok(delta), state));
            }
            if state.eof {
                return None;
            }
            let line = match state.lines.next_line() {
                Ok(line) => line,
                Err(error) => {
                    state.eof = true;
                    return Some((Err(error), state));
                }
            };
            if let Some(line) = line {
                let Some(data) = line.strip_prefix("data:").map(str::trim_start) else {
                    continue;
                };
                if data == "[DONE]" {
                    state.eof = true;
                    // Some compatible servers use only the explicit terminator.
                    if !state.finished {
                        if !state.tools.is_empty() {
                            return Some((Err(ProviderError::StreamEnded), state));
                        }
                        state.finished = true;
                        return Some((
                            Ok(Delta::Done {
                                stop_reason: StopReason::EndTurn,
                            }),
                            state,
                        ));
                    }
                    return None;
                }
                let value: Value = match serde_json::from_str(data) {
                    Ok(value) => value,
                    Err(error) => {
                        state.eof = true;
                        return Some((Err(error.into()), state));
                    }
                };
                if let Some(error) = value.get("error") {
                    state.eof = true;
                    return Some((
                        Err(ProviderError::Other(format!(
                            "provider stream error: {error}"
                        ))),
                        state,
                    ));
                }
                if let Some(usage) = value.get("usage") {
                    let input_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                    let output_tokens = usage["completion_tokens"].as_u64().unwrap_or(0) as u32;
                    if input_tokens > 0 || output_tokens > 0 {
                        state.queue.push_back(Delta::Usage {
                            input_tokens,
                            output_tokens,
                        });
                    }
                }
                if let Some(choice) = value["choices"]
                    .as_array()
                    .and_then(|choices| choices.first())
                {
                    let delta = &choice["delta"];
                    if let Some(text) = delta["content"].as_str().filter(|text| !text.is_empty()) {
                        state.queue.push_back(Delta::Text(text.to_owned()));
                    }
                    if let Some(calls) = delta["tool_calls"].as_array() {
                        for call in calls {
                            let index = call["index"].as_u64().unwrap_or(0) as u32;
                            let entry = state.tools.entry(index).or_default();
                            if let Some(id) = call["id"].as_str() {
                                entry.0 = id.to_owned();
                            }
                            if let Some(name) = call["function"]["name"].as_str() {
                                entry.1 = name.to_owned();
                            }
                            if let Some(arguments) = call["function"]["arguments"].as_str() {
                                entry.2.push_str(arguments);
                            }
                        }
                    }
                    if let Some(reason) = choice["finish_reason"].as_str() {
                        let mut calls: Vec<_> = state.tools.drain().collect();
                        calls.sort_by_key(|(index, _)| *index);
                        for (_, (id, name, arguments)) in calls {
                            state.queue.push_back(Delta::ToolCall(ToolCall {
                                id,
                                kind: "function".into(),
                                function: ToolCallFunction { name, arguments },
                            }));
                        }
                        let stop_reason = match reason {
                            "tool_calls" => StopReason::ToolUse,
                            "length" => StopReason::MaxTokens,
                            "stop" => StopReason::EndTurn,
                            _ => {
                                state.eof = true;
                                return Some((
                                    Err(ProviderError::Other(format!(
                                        "provider stopped with {reason}"
                                    ))),
                                    state,
                                ));
                            }
                        };
                        state.finished = true;
                        state.queue.push_back(Delta::Done { stop_reason });
                    }
                }
                continue;
            }
            if state.eof {
                return None;
            }
            match state.stream.next().await {
                Some(Ok(chunk)) => state.lines.push(&chunk),
                Some(Err(error)) => {
                    state.eof = true;
                    return Some((Err(ProviderError::Other(error.to_string())), state));
                }
                None => {
                    state.eof = true;
                    if !state.finished {
                        return Some((Err(ProviderError::StreamEnded), state));
                    }
                    return None;
                }
            }
        }
    })
}

#[cfg(test)]
mod openai_sse_tests {
    use super::*;
    use futures::StreamExt;

    #[test]
    fn request_limits_follow_the_effective_model_and_endpoint() {
        let config = OpenAIConfig::new("test").with_max_tokens(1234);
        for model in ["gpt-5.5", "gpt-6-sol", "o3", "o4-mini"] {
            let body =
                serde_json::to_value(build_api_request(&ChatRequest::new(model), &config)).unwrap();
            assert_eq!(body["model"], model);
            assert_eq!(body["max_completion_tokens"], 1234);
            assert!(body.get("max_tokens").is_none());
            assert!(body.get("temperature").is_none());
        }
        let compatible = config.with_base_url("http://localhost:8080/v1");
        let body =
            serde_json::to_value(build_api_request(&ChatRequest::new("gpt-5.5"), &compatible))
                .unwrap();
        assert_eq!(body["max_tokens"], 1234);
        assert!(body.get("temperature").is_some());
        assert!(body.get("max_completion_tokens").is_none());
    }

    #[tokio::test]
    async fn preserves_unicode_at_every_network_split_and_trailing_usage() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hé中🦀\"},\"finish_reason\":\"stop\"}]}\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7}}\n",
            "data: [DONE]\n"
        );
        for split in 0..=payload.len() {
            let chunks = vec![
                Ok(bytes::Bytes::copy_from_slice(&payload.as_bytes()[..split])),
                Ok(bytes::Bytes::copy_from_slice(&payload.as_bytes()[split..])),
            ];
            let parsed = parse_openai_sse(futures::stream::iter(chunks));
            tokio::pin!(parsed);
            let mut text = String::new();
            let mut usage = None;
            let mut done = 0;
            while let Some(delta) = parsed.next().await {
                match delta.unwrap() {
                    Delta::Text(part) => text.push_str(&part),
                    Delta::Usage {
                        input_tokens,
                        output_tokens,
                    } => usage = Some((input_tokens, output_tokens)),
                    Delta::Done { .. } => done += 1,
                    _ => {}
                }
            }
            assert_eq!(text, "hé中🦀");
            assert_eq!(usage, Some((11, 7)));
            assert_eq!(done, 1);
        }
    }

    #[tokio::test]
    async fn reports_unexpected_eof() {
        let body = sse_bytes(&[r#"data: {"choices":[{"delta":{"content":"partial"}}]}"#]);
        let parsed = parse_openai_sse(futures::stream::once(async move { Ok(body) }));
        tokio::pin!(parsed);
        assert!(matches!(parsed.next().await, Some(Ok(Delta::Text(_)))));
        assert!(matches!(
            parsed.next().await,
            Some(Err(ProviderError::StreamEnded))
        ));
        assert!(parsed.next().await.is_none());
    }

    fn sse_bytes(lines: &[&str]) -> bytes::Bytes {
        let mut s = String::new();
        for line in lines {
            s.push_str(line);
            if !line.ends_with('\n') {
                s.push('\n');
            }
        }
        bytes::Bytes::from(s)
    }

    async fn collect(body: bytes::Bytes) -> Vec<Delta> {
        let stream = futures::stream::once(async move { Ok::<_, reqwest::Error>(body) });
        let parsed = parse_openai_sse(stream);
        tokio::pin!(parsed);
        let mut out = Vec::new();
        while let Some(item) = parsed.next().await {
            out.push(item.expect("delta"));
        }
        out
    }

    #[tokio::test]
    async fn emits_all_parallel_tool_calls_before_done() {
        let body = sse_bytes(&[
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"tool_a","arguments":""}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_b","function":{"name":"tool_b","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#,
        ]);
        let mut tools = Vec::new();
        let mut last_done = None;
        for d in collect(body).await {
            match d {
                Delta::ToolCall(tc) => tools.push((tc.function.name, tc.id)),
                Delta::Done { stop_reason } => last_done = Some(stop_reason),
                _ => {}
            }
        }
        assert_eq!(
            tools,
            vec![
                ("tool_a".into(), "call_a".into()),
                ("tool_b".into(), "call_b".into())
            ]
        );
        assert_eq!(last_done, Some(StopReason::ToolUse));
    }

    #[tokio::test]
    async fn three_parallel_tool_calls_emitted_in_index_order() {
        // Send the indices out of order to confirm we sort by index.
        let body = sse_bytes(&[
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":2,"id":"c","function":{"name":"t_c","arguments":"{}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"t_a","arguments":"{}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"b","function":{"name":"t_b","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#,
        ]);
        let names: Vec<String> = collect(body)
            .await
            .into_iter()
            .filter_map(|d| match d {
                Delta::ToolCall(tc) => Some(tc.function.name),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["t_a", "t_b", "t_c"]);
    }

    #[tokio::test]
    async fn streams_text_content_chunks_in_order() {
        let body = sse_bytes(&[
            r#"data: {"choices":[{"delta":{"content":"Hello"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":", world"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"!"},"finish_reason":"stop"}]}"#,
        ]);
        let mut text = String::new();
        let mut done = None;
        for d in collect(body).await {
            match d {
                Delta::Text(t) => text.push_str(&t),
                Delta::Done { stop_reason } => done = Some(stop_reason),
                _ => {}
            }
        }
        assert_eq!(text, "Hello, world!");
        assert_eq!(done, Some(StopReason::EndTurn));
    }

    #[tokio::test]
    async fn emits_usage_delta_when_present() {
        let body = sse_bytes(&[
            r#"data: {"choices":[{"delta":{"content":"hi"}}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":42,"completion_tokens":7}}"#,
        ]);
        let mut usage = None;
        for d in collect(body).await {
            if let Delta::Usage {
                input_tokens,
                output_tokens,
            } = d
            {
                usage = Some((input_tokens, output_tokens));
            }
        }
        assert_eq!(usage, Some((42, 7)));
    }

    #[tokio::test]
    async fn done_terminator_stops_stream_with_endturn() {
        let body = sse_bytes(&[
            r#"data: {"choices":[{"delta":{"content":"ok"}}]}"#,
            r#"data: [DONE]"#,
            // Anything after [DONE] must be ignored.
            r#"data: {"choices":[{"delta":{"content":"after"}}]}"#,
        ]);
        let collected = collect(body).await;
        let text: String = collected
            .iter()
            .filter_map(|d| match d {
                Delta::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "ok", "must not stream content after [DONE]");
        assert!(matches!(
            collected.last(),
            Some(Delta::Done {
                stop_reason: StopReason::EndTurn
            })
        ));
    }

    #[tokio::test]
    async fn malformed_json_is_a_reported_failure() {
        let body = sse_bytes(&["data: {not valid json"]);
        let stream = parse_openai_sse(futures::stream::once(async move { Ok(body) }));
        tokio::pin!(stream);
        assert!(matches!(
            stream.next().await,
            Some(Err(ProviderError::Json(_)))
        ));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn multi_chunk_tool_arguments_concatenate() {
        // OpenAI streams `function.arguments` in tiny pieces; they must concatenate.
        let body = sse_bytes(&[
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"x","function":{"name":"do_thing","arguments":"{\"a\":"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"1,\"b\":"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"2}"}}]},"finish_reason":"tool_calls"}]}"#,
        ]);
        let calls: Vec<ToolCall> = collect(body)
            .await
            .into_iter()
            .filter_map(|d| match d {
                Delta::ToolCall(tc) => Some(tc),
                _ => None,
            })
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "do_thing");
        assert_eq!(calls[0].function.arguments, r#"{"a":1,"b":2}"#);
    }

    #[tokio::test]
    async fn empty_content_chunks_are_skipped() {
        // OpenAI sometimes emits `delta.content = ""` keep-alives; do not surface them.
        let body = sse_bytes(&[
            r#"data: {"choices":[{"delta":{"content":""}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"real"}}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ]);
        let texts: Vec<String> = collect(body)
            .await
            .into_iter()
            .filter_map(|d| match d {
                Delta::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["real".to_string()]);
    }

    mod sse_proptest {
        use super::*;
        use proptest::prelude::*;

        fn collect_sync(body: bytes::Bytes) -> Vec<Delta> {
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            rt.block_on(async { collect(body).await })
        }

        proptest! {
            #[test]
            fn text_sse_chunk_invariance(text in prop::collection::vec(0x20u8..=0x7e, 0..80)
                .prop_map(|bytes| String::from_utf8(bytes).expect("ascii"))) {
                let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
                let line = format!(
                    r#"data: {{"choices":[{{"delta":{{"content":"{escaped}"}}}}]}}"#
                );
                let payload = sse_bytes(&[&line, "data: [DONE]"]);
                let full = collect_sync(payload.clone());
                let texts: String = full
                    .into_iter()
                    .filter_map(|d| match d {
                        Delta::Text(t) => Some(t),
                        _ => None,
                    })
                    .collect();
                prop_assert_eq!(texts, text.clone());

                if payload.len() > 1 {
                    for split in 1..payload.len() {
                        let (a, b) = payload.split_at(split);
                        let stream = futures::stream::iter(vec![
                            Ok::<_, reqwest::Error>(bytes::Bytes::copy_from_slice(a)),
                            Ok(bytes::Bytes::copy_from_slice(b)),
                        ]);
                        let rt = tokio::runtime::Runtime::new().expect("runtime");
                        let chunked: String = rt.block_on(async {
                            let parsed = parse_openai_sse(stream);
                            tokio::pin!(parsed);
                            let mut out = String::new();
                            while let Some(item) = parsed.next().await {
                                if let Ok(Delta::Text(t)) = item {
                                    out.push_str(&t);
                                }
                            }
                            out
                        });
                        prop_assert_eq!(chunked, text.clone());
                    }
                }
            }
        }
    }
}
