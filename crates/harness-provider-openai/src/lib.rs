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
}

impl OpenAIConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "gpt-5.5".into(),
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

    async fn embed(&self, _model: &str, text: &str) -> Result<Vec<f32>, ProviderError> {
        let url = format!("{}/embeddings", self.config.base_url);
        let resp = self
            .client
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
            return Err(ProviderError::Api {
                status: 0,
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
        let messages = build_api_messages(&req);
        let tools = build_tool_schemas(&req.tools);

        // Build response_format for strict JSON schema if requested
        let response_format = req.response_schema.as_ref().map(|rs| {
            json!({
                "type": "json_schema",
                "json_schema": {
                    "name": rs.name,
                    "schema": rs.schema,
                    "strict": rs.strict
                }
            })
        });

        let body = ApiRequest {
            model: self.config.model.clone(),
            messages,
            tools,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
            stream: true,
            stream_options: StreamOptions {
                include_usage: true,
            },
            response_format,
        };

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
    use std::pin::Pin;
    type ByteStream =
        Pin<Box<dyn futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>;

    struct State {
        stream: ByteStream,
        buf: String,
        // index → (id, name, accumulated_args)
        tool_calls: std::collections::HashMap<u32, (String, String, String)>,
        /// Tool calls accumulated for the current assistant turn; emitted one `Delta::ToolCall` at a time.
        pending_tool_calls: std::collections::VecDeque<ToolCall>,
        /// After all `pending_tool_calls` are emitted, send this `Done` (e.g. `ToolUse`).
        pending_stop_after_tools: Option<StopReason>,
        done: bool,
    }

    let state = State {
        stream: Box::pin(byte_stream),
        buf: String::new(),
        tool_calls: std::collections::HashMap::new(),
        pending_tool_calls: std::collections::VecDeque::new(),
        pending_stop_after_tools: None,
        done: false,
    };

    futures::stream::unfold(state, |mut s| async move {
        if s.done {
            return None;
        }

        // Drain multi-tool batches: emit every tool call before the final `Done`.
        if let Some(call) = s.pending_tool_calls.pop_front() {
            return Some((Ok(Delta::ToolCall(call)), s));
        }
        if let Some(sr) = s.pending_stop_after_tools.take() {
            s.done = true;
            return Some((Ok(Delta::Done { stop_reason: sr }), s));
        }

        loop {
            while let Some(nl) = s.buf.find('\n') {
                let line = s.buf[..nl].trim_end_matches('\r').to_string();
                s.buf = s.buf[nl + 1..].to_string();

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        s.done = true;
                        return Some((
                            Ok(Delta::Done {
                                stop_reason: StopReason::EndTurn,
                            }),
                            s,
                        ));
                    }

                    if let Ok(v) = serde_json::from_str::<Value>(data) {
                        // Usage (may appear in a separate chunk or on the last choice)
                        if let Some(usage) = v.get("usage") {
                            let in_tok = usage["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                            let out_tok = usage["completion_tokens"].as_u64().unwrap_or(0) as u32;
                            if in_tok > 0 || out_tok > 0 {
                                return Some((
                                    Ok(Delta::Usage {
                                        input_tokens: in_tok,
                                        output_tokens: out_tok,
                                    }),
                                    s,
                                ));
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
                                        if let Some(id) = tc["id"].as_str() {
                                            entry.0 = id.to_string();
                                        }
                                        if let Some(name) = tc["function"]["name"].as_str() {
                                            entry.1 = name.to_string();
                                        }
                                        if let Some(args) = tc["function"]["arguments"].as_str() {
                                            entry.2.push_str(args);
                                        }
                                    }
                                }

                                if let Some(reason) = finish_reason {
                                    // Flush every accumulated tool call (OpenAI may batch several).
                                    if !s.tool_calls.is_empty() {
                                        let mut sorted: Vec<_> = s.tool_calls.drain().collect();
                                        sorted.sort_by_key(|(k, _)| *k);
                                        for (_, (id, name, args)) in sorted {
                                            s.pending_tool_calls.push_back(ToolCall {
                                                id,
                                                kind: "function".into(),
                                                function: ToolCallFunction {
                                                    name,
                                                    arguments: args,
                                                },
                                            });
                                        }
                                        let sr = match reason {
                                            "tool_calls" => StopReason::ToolUse,
                                            "length" => StopReason::MaxTokens,
                                            _ => StopReason::EndTurn,
                                        };
                                        s.pending_stop_after_tools = Some(sr);
                                        if let Some(call) = s.pending_tool_calls.pop_front() {
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

#[cfg(test)]
mod openai_sse_tests {
    use super::*;
    use futures::StreamExt;

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
    async fn malformed_json_lines_are_skipped_not_panicked() {
        let body = sse_bytes(&[
            r#"data: {not valid json"#,
            r#"data: {"choices":[{"delta":{"content":"survived"}}]}"#,
            r#"data: also garbage"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        ]);
        let collected = collect(body).await;
        let text: String = collected
            .iter()
            .filter_map(|d| match d {
                Delta::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "survived");
        assert!(matches!(collected.last(), Some(Delta::Done { .. })));
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
}
