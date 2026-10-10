//! Ollama local model provider for Harness.
//!
//! Uses Ollama's OpenAI-compatible `/api/chat` endpoint for streaming chat
//! and `/api/embeddings` for text embeddings.

use async_trait::async_trait;
use futures::StreamExt;
use harness_provider_core::{
    ChatRequest, Delta, DeltaStream, Pricing, Provider, ProviderError, Role, StopReason, ToolCall,
    ToolCallFunction,
};
use reqwest::Client;
use serde_json::{json, Value};
use tracing::debug;

const OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434";

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
            Role::Assistant => {
                msgs.push(json!({"role": "assistant", "content": msg.content.as_str()}))
            }
            Role::Tool => msgs.push(json!({
                "role": "tool",
                "content": msg.content.as_str()
            })),
        }
    }
    msgs
}

fn build_tools(tools: &[harness_provider_core::ToolDefinition]) -> Vec<Value> {
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
impl Provider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    fn pricing(&self) -> Option<Pricing> {
        // Local models are free.
        Some(Pricing {
            input_per_m_usd: 0.0,
            cached_input_per_m_usd: 0.0,
            output_per_m_usd: 0.0,
        })
    }

    async fn embed(&self, model: &str, text: &str) -> Result<Vec<f32>, ProviderError> {
        let url = format!("{}/api/embeddings", self.config.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&json!({"model": model, "prompt": text}))
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
        let emb: Vec<f32> = body["embedding"]
            .as_array()
            .ok_or_else(|| ProviderError::Other("missing embedding in Ollama response".into()))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        Ok(emb)
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError> {
        req.ensure_text_only("ollama")?;
        let messages = build_messages(&req);
        let tools = build_tools(&req.tools);
        let has_tools = !tools.is_empty();

        // Use OpenAI-compat endpoint if Ollama version >= 0.5.
        let url = format!("{}/api/chat", self.config.base_url);

        let model = req.effective_model(&self.config.model);
        let mut body = json!({
            "model": model,
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

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message: msg,
            });
        }

        let stream = parse_ollama_stream(resp.bytes_stream());
        Ok(Box::pin(stream))
    }
}

fn parse_ollama_stream(
    byte_stream: impl futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
) -> impl futures::Stream<Item = Result<Delta, ProviderError>> + Send {
    use std::{collections::VecDeque, pin::Pin};
    type ByteStream =
        Pin<Box<dyn futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>;
    struct State {
        stream: ByteStream,
        lines: harness_provider_core::LineBuffer,
        queue: VecDeque<Delta>,
        done: bool,
        eof: bool,
        tool_count: usize,
    }
    let state = State {
        stream: Box::pin(byte_stream),
        lines: Default::default(),
        queue: VecDeque::new(),
        done: false,
        eof: false,
        tool_count: 0,
    };
    futures::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(delta) = state.queue.pop_front() {
                return Some((Ok(delta), state));
            }
            if state.done {
                return None;
            }
            let line = match state.lines.next_line() {
                Ok(line) => line,
                Err(error) => {
                    state.done = true;
                    return Some((Err(error), state));
                }
            };
            if let Some(line) = line {
                if line.trim().is_empty() {
                    continue;
                }
                let value: Value = match serde_json::from_str(&line) {
                    Ok(value) => value,
                    Err(error) => {
                        state.done = true;
                        return Some((Err(error.into()), state));
                    }
                };
                if let Some(error) = value.get("error") {
                    state.done = true;
                    return Some((
                        Err(ProviderError::Other(format!(
                            "Ollama stream error: {error}"
                        ))),
                        state,
                    ));
                }
                if let Some(content) = value["message"]["content"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                {
                    state.queue.push_back(Delta::Text(content.to_owned()));
                }
                if let Some(calls) = value["message"]["tool_calls"].as_array() {
                    for call in calls {
                        state.tool_count += 1;
                        let id = call["id"]
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("ollama-call-{}", state.tool_count));
                        let name = call["function"]["name"].as_str().unwrap_or("").to_owned();
                        let arguments = match &call["function"]["arguments"] {
                            Value::String(text) => text.clone(),
                            value => value.to_string(),
                        };
                        state.queue.push_back(Delta::ToolCall(ToolCall {
                            id,
                            kind: "function".into(),
                            function: ToolCallFunction { name, arguments },
                        }));
                    }
                }
                if value["done"].as_bool() == Some(true) {
                    let input_tokens = value["prompt_eval_count"].as_u64().unwrap_or(0) as u32;
                    let output_tokens = value["eval_count"].as_u64().unwrap_or(0) as u32;
                    if input_tokens > 0 || output_tokens > 0 {
                        state.queue.push_back(Delta::Usage {
                            input_tokens,
                            output_tokens,
                        });
                    }
                    let stop_reason = if value["done_reason"].as_str() == Some("length") {
                        StopReason::MaxTokens
                    } else if state.tool_count > 0 {
                        StopReason::ToolUse
                    } else {
                        StopReason::EndTurn
                    };
                    state.queue.push_back(Delta::Done { stop_reason });
                    state.done = true;
                }
                continue;
            }
            if state.eof {
                state.done = true;
                return Some((Err(ProviderError::StreamEnded), state));
            }
            match state.stream.next().await {
                Some(Ok(chunk)) => state.lines.push(&chunk),
                Some(Err(error)) => {
                    state.done = true;
                    return Some((Err(ProviderError::Other(error.to_string())), state));
                }
                None => {
                    state.eof = true;
                    state.lines.finish();
                }
            }
        }
    })
}

#[cfg(test)]
mod ollama_stream_tests {
    use super::*;
    use futures::StreamExt;

    fn ndjson(body: &str) -> bytes::Bytes {
        bytes::Bytes::from(body.to_string())
    }

    async fn collect(body: bytes::Bytes) -> Vec<Delta> {
        let stream = futures::stream::once(async move { Ok::<_, reqwest::Error>(body) });
        let parsed = parse_ollama_stream(stream);
        tokio::pin!(parsed);
        let mut out = Vec::new();
        while let Some(item) = parsed.next().await {
            out.push(item.expect("delta"));
        }
        out
    }

    #[tokio::test]
    async fn emits_all_tool_calls_from_one_line() {
        let body = ndjson(
            r#"{"message":{"tool_calls":[{"id":"a","function":{"name":"read_file","arguments":"{}"}},{"id":"b","function":{"name":"shell","arguments":"{\"command\":\"ls\"}"}}]}}"#,
        );
        let body = bytes::Bytes::from(format!(
            "{}\n{{\"done\":true}}",
            String::from_utf8_lossy(&body)
        ));
        let names: Vec<String> = collect(body)
            .await
            .into_iter()
            .filter_map(|d| match d {
                Delta::ToolCall(tc) => Some(tc.function.name),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["read_file", "shell"]);
    }

    #[tokio::test]
    async fn streams_text_then_done() {
        let body = ndjson(
            "{\"message\":{\"content\":\"hello\"}}\n{\"done\":true,\"prompt_eval_count\":1,\"eval_count\":2}\n",
        );
        let mut text = String::new();
        let mut usage = None;
        for d in collect(body).await {
            match d {
                Delta::Text(t) => text.push_str(&t),
                Delta::Usage {
                    input_tokens,
                    output_tokens,
                } => usage = Some((input_tokens, output_tokens)),
                _ => {}
            }
        }
        assert_eq!(text, "hello");
        assert_eq!(usage, Some((1, 2)));
    }
    #[tokio::test]
    async fn combined_final_line_preserves_unicode_usage_and_completion() {
        let payload =
            r#"{"message":{"content":"hé中🦀"},"done":true,"prompt_eval_count":3,"eval_count":2}"#;
        for split in 0..=payload.len() {
            let chunks = vec![
                Ok(bytes::Bytes::copy_from_slice(&payload.as_bytes()[..split])),
                Ok(bytes::Bytes::copy_from_slice(&payload.as_bytes()[split..])),
            ];
            let parsed = parse_ollama_stream(futures::stream::iter(chunks));
            tokio::pin!(parsed);
            assert!(matches!(parsed.next().await, Some(Ok(Delta::Text(text))) if text == "hé中🦀"));
            assert!(matches!(
                parsed.next().await,
                Some(Ok(Delta::Usage {
                    input_tokens: 3,
                    output_tokens: 2
                }))
            ));
            assert!(matches!(
                parsed.next().await,
                Some(Ok(Delta::Done {
                    stop_reason: StopReason::EndTurn
                }))
            ));
            assert!(parsed.next().await.is_none());
        }
    }
}
