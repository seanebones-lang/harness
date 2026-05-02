use async_trait::async_trait;
use harness_provider_core::{
    ChatRequest, DeltaStream, Message, MessageContent, Pricing, Provider, ProviderError, Role,
    ToolDefinition,
};
use reqwest::Client;
use serde_json::Value;
use tracing::{debug, warn};

use crate::stream::SseStream;
use crate::types::{ApiMessage, ApiRequest, ApiToolCall, ApiToolCallFunction, StreamOptions};

const XAI_BASE_URL: &str = "https://api.x.ai/v1";

#[derive(Debug, Clone)]
pub struct XaiConfig {
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub base_url: String,
}

impl XaiConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "grok-4.20-0309-reasoning".into(),
            max_tokens: 8192,
            temperature: 0.7,
            base_url: XAI_BASE_URL.into(),
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

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }
}

#[derive(Clone)]
pub struct XaiProvider {
    pub config: XaiConfig,
    client: Client,
}

impl XaiProvider {
    pub fn new(config: XaiConfig) -> anyhow::Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;
        Ok(Self { config, client })
    }

    fn build_api_messages(&self, req: &ChatRequest) -> Vec<ApiMessage> {
        let mut api_msgs: Vec<ApiMessage> = Vec::new();

        // Inject system message at the front if present
        if let Some(sys) = &req.system {
            api_msgs.push(ApiMessage {
                role: "system".into(),
                content: Some(sys.clone()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });
        }

        for msg in &req.messages {
            match &msg.role {
                Role::System => {
                    api_msgs.push(ApiMessage {
                        role: "system".into(),
                        content: Some(msg.content.as_str().to_string()),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                }
                Role::User => {
                    api_msgs.push(ApiMessage {
                        role: "user".into(),
                        content: Some(msg.content.as_str().to_string()),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                }
                Role::Assistant => {
                    // Assistant message may carry pending tool_calls stored as JSON in content
                    // Convention: if content starts with __tool_calls__, parse them back
                    let content_str = msg.content.as_str();
                    if let Some(stripped) = content_str.strip_prefix("__tool_calls__:") {
                        let calls: Vec<ApiToolCall> =
                            serde_json::from_str(stripped).unwrap_or_default();
                        api_msgs.push(ApiMessage {
                            role: "assistant".into(),
                            content: None,
                            tool_calls: Some(calls),
                            tool_call_id: None,
                            name: None,
                        });
                    } else {
                        api_msgs.push(ApiMessage {
                            role: "assistant".into(),
                            content: Some(content_str.to_string()),
                            tool_calls: None,
                            tool_call_id: None,
                            name: None,
                        });
                    }
                }
                Role::Tool => {
                    api_msgs.push(ApiMessage {
                        role: "tool".into(),
                        content: Some(msg.content.as_str().to_string()),
                        tool_calls: None,
                        tool_call_id: msg.tool_call_id.clone(),
                        name: None,
                    });
                }
            }
        }

        api_msgs
    }

    fn build_tool_schemas(&self, tools: &[ToolDefinition]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| serde_json::to_value(t).unwrap_or(Value::Null))
            .collect()
    }
}

#[async_trait]
impl Provider for XaiProvider {
    fn name(&self) -> &str {
        "xai"
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    async fn embed(&self, model: &str, text: &str) -> Result<Vec<f32>, ProviderError> {
        crate::embed::embed_text(
            &self.client,
            &self.config.api_key,
            &self.config.base_url,
            model,
            text,
        )
        .await
        .map_err(ProviderError::from)
    }

    fn pricing(&self) -> Option<Pricing> {
        let m = self.config.model.to_lowercase();
        // April 2026 Grok 4.x SKUs
        if m.contains("grok-4.20") || m.contains("grok-4-20") {
            // Grok 4.20: $2/$6, 90% cached discount → $0.20 cached
            Some(Pricing {
                input_per_m_usd: 2.00,
                cached_input_per_m_usd: 0.20,
                output_per_m_usd: 6.00,
            })
        } else if m.contains("grok-4-1-fast") || m.contains("grok-4.1-fast") {
            Some(Pricing {
                input_per_m_usd: 0.20,
                cached_input_per_m_usd: 0.05,
                output_per_m_usd: 0.50,
            })
        } else if m.contains("grok-4") {
            Some(Pricing {
                input_per_m_usd: 3.00,
                cached_input_per_m_usd: 0.0,
                output_per_m_usd: 15.00,
            })
        // Legacy Grok 3
        } else if m.contains("grok-3-mini") {
            Some(Pricing {
                input_per_m_usd: 0.30,
                cached_input_per_m_usd: 0.0,
                output_per_m_usd: 0.50,
            })
        } else if m.contains("grok-3") {
            Some(Pricing {
                input_per_m_usd: 2.00,
                cached_input_per_m_usd: 0.0,
                output_per_m_usd: 10.00,
            })
        } else {
            None
        }
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError> {
        let messages = self.build_api_messages(&req);
        let mut tools = self.build_tool_schemas(&req.tools);

        // Append xAI native server-side tools when requested.
        // xAI tools use a special "type" field distinct from "function".
        if req.native_web_search {
            tools.push(serde_json::json!({
                "type": "web_search"
            }));
        }
        if req.native_x_search {
            tools.push(serde_json::json!({
                "type": "x_search"
            }));
        }
        if req.native_code_execution {
            tools.push(serde_json::json!({
                "type": "code_execution"
            }));
        }

        let has_tools = !tools.is_empty();

        // Build response_format for strict JSON schema if requested
        let response_format = req.response_schema.as_ref().map(|rs| {
            serde_json::json!({
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
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
            tool_choice: if has_tools { Some("auto".into()) } else { None },
            response_format,
        };

        debug!(model = %body.model, "sending chat request to xAI");

        let url = format!("{}/chat/completions", self.config.base_url);

        // Retry loop: up to MAX_RETRIES attempts with exponential backoff.
        // Retryable: 429 (rate limit), 500/502/503/504 (transient server errors).
        const MAX_RETRIES: u32 = 4;
        const BASE_DELAY_MS: u64 = 1000;

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

            let retryable = matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504);

            if status.is_success() {
                let byte_stream = resp.bytes_stream();
                let sse = SseStream::new(byte_stream);
                return Ok(Box::pin(sse));
            }

            if retryable && attempt < MAX_RETRIES {
                // Honour Retry-After header if present (value in seconds).
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok());

                let delay_ms = retry_after
                    .map(|s| s * 1000)
                    .unwrap_or(BASE_DELAY_MS << attempt); // exponential: 1s, 2s, 4s, 8s

                warn!(
                    status = status.as_u16(),
                    attempt, delay_ms, "xAI API retryable error; waiting before retry"
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

impl XaiProvider {
    /// Embed a text string using the xAI embeddings endpoint (inherent method).
    /// Prefer using the `Provider::embed` trait method for generic code.
    pub async fn embed_direct(&self, model: &str, text: &str) -> anyhow::Result<Vec<f32>> {
        crate::embed::embed_text(
            &self.client,
            &self.config.api_key,
            &self.config.base_url,
            model,
            text,
        )
        .await
    }
}

/// Encode tool calls into a Message for conversation history.
pub fn tool_calls_to_message(calls: &[harness_provider_core::ToolCall]) -> Message {
    let api_calls: Vec<ApiToolCall> = calls
        .iter()
        .map(|c| ApiToolCall {
            id: c.id.clone(),
            kind: c.kind.clone(),
            function: ApiToolCallFunction {
                name: c.function.name.clone(),
                arguments: c.function.arguments.clone(),
            },
        })
        .collect();
    let encoded = format!(
        "__tool_calls__:{}",
        serde_json::to_string(&api_calls).unwrap_or_default()
    );
    Message {
        role: Role::Assistant,
        content: MessageContent::Text(encoded),
        tool_call_id: None,
    }
}
