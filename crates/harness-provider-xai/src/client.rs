use async_trait::async_trait;
use harness_provider_core::{
    ChatRequest, DeltaStream, Message, MessageContent, Provider, ProviderError, Role,
    ToolDefinition,
};
use reqwest::Client;
use serde_json::Value;
use tracing::debug;

use crate::stream::SseStream;
use crate::types::{ApiMessage, ApiRequest, ApiToolCall, ApiToolCallFunction};

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
            model: "grok-3-fast".into(),
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

pub struct XaiProvider {
    config: XaiConfig,
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

    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError> {
        let messages = self.build_api_messages(&req);
        let tools = self.build_tool_schemas(&req.tools);
        let has_tools = !tools.is_empty();

        let body = ApiRequest {
            model: self.config.model.clone(),
            messages,
            tools,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
            stream: true,
            tool_choice: if has_tools { Some("auto".into()) } else { None },
        };

        debug!(model = %body.model, "sending chat request to xAI");

        let url = format!("{}/chat/completions", self.config.base_url);
        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.config.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".into());
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let byte_stream = response.bytes_stream();
        let sse = SseStream::new(byte_stream);
        Ok(Box::pin(sse))
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
