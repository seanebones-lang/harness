//! Message, tool, and chat request types for provider implementations.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Messages ──────────────────────────────────────────────────────────────────

/// Conversation role for a chat message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System prompt message.
    System,
    /// User turn.
    User,
    /// Assistant model output.
    Assistant,
    /// Tool execution result.
    Tool,
}

/// A single chat message in provider wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message author role.
    pub role: Role,
    /// Text or multipart content.
    pub content: MessageContent,
    /// Present when role == Tool; the tool_call_id this result belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// Build a system message.
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: MessageContent::Text(text.into()),
            tool_call_id: None,
        }
    }

    /// Build a user message.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Text(text.into()),
            tool_call_id: None,
        }
    }

    /// Build an assistant message.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Text(text.into()),
            tool_call_id: None,
        }
    }

    /// Build a tool result message tied to a tool call id.
    pub fn tool_result(tool_call_id: impl Into<String>, result: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: MessageContent::Text(result.into()),
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// Plain text or multipart message body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Single text block.
    Text(String),
    /// Text + image parts.
    Parts(Vec<ContentPart>),
}

impl MessageContent {
    /// Return the primary text content, if any.
    pub fn as_str(&self) -> &str {
        match self {
            MessageContent::Text(s) => s,
            MessageContent::Parts(parts) => {
                // Return the first text part if available.
                parts.iter().find_map(|p| p.text.as_deref()).unwrap_or("")
            }
        }
    }

    /// Build a multipart message with text and an image from a file path.
    pub fn with_image(text: impl Into<String>, image_path: &str) -> anyhow::Result<Self> {
        use std::io::Read;
        let mut f = std::fs::File::open(image_path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        let b64 = base64_encode(&buf);
        let mime = mime_for_path(image_path);
        Ok(Self::Parts(vec![
            ContentPart::text(text),
            ContentPart::image_base64(mime, &b64),
        ]))
    }
}

fn base64_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 {
            chunk[1] as usize
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            chunk[2] as usize
        } else {
            0
        };
        let _ = write!(out, "{}", TABLE[(b0 >> 2) & 63] as char);
        let _ = write!(out, "{}", TABLE[((b0 << 4) | (b1 >> 4)) & 63] as char);
        let _ = write!(
            out,
            "{}",
            if chunk.len() > 1 {
                TABLE[((b1 << 2) | (b2 >> 6)) & 63] as char
            } else {
                '='
            }
        );
        let _ = write!(
            out,
            "{}",
            if chunk.len() > 2 {
                TABLE[b2 & 63] as char
            } else {
                '='
            }
        );
    }
    out
}

fn mime_for_path(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

/// One part of a multipart message (text or image).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPart {
    /// Part type (`text` or `image_url`).
    #[serde(rename = "type")]
    pub kind: String,
    /// Text payload when `kind == "text"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Base64-encoded image data (for type = "image_url").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<ImageUrl>,
}

/// Image reference embedded in a multipart message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    /// Data URI: "data:image/png;base64,..." or an https:// URL.
    pub url: String,
}

impl ContentPart {
    /// Text content part.
    pub fn text(t: impl Into<String>) -> Self {
        Self {
            kind: "text".into(),
            text: Some(t.into()),
            image_url: None,
        }
    }

    /// Base64 image content part.
    pub fn image_base64(mime: &str, data: &str) -> Self {
        Self {
            kind: "image_url".into(),
            text: None,
            image_url: Some(ImageUrl {
                url: format!("data:{mime};base64,{data}"),
            }),
        }
    }
}

// ── Tools ─────────────────────────────────────────────────────────────────────

/// OpenAI-format tool definition (Grok accepts this natively).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Always `"function"` for Harness tools.
    #[serde(rename = "type")]
    pub kind: String, // always "function"
    /// Function name, description, and JSON Schema parameters.
    pub function: FunctionDef,
}

impl ToolDefinition {
    /// Build an OpenAI-format function tool definition.
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            kind: "function".into(),
            function: FunctionDef {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

/// Function metadata inside a [`ToolDefinition`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    /// Tool name sent to the model.
    pub name: String,
    /// Human-readable tool description.
    pub description: String,
    /// JSON Schema for tool arguments.
    pub parameters: Value,
}

/// A tool call the model wants to make.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned tool call id.
    pub id: String,
    /// Always `"function"` for Harness.
    #[serde(rename = "type")]
    pub kind: String,
    /// Function name and JSON arguments string.
    pub function: ToolCallFunction,
}

/// Function invocation requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    /// Function/tool name.
    pub name: String,
    /// JSON-encoded arguments object.
    pub arguments: String, // JSON string
}

impl ToolCall {
    /// Parse `function.arguments` as JSON.
    pub fn args(&self) -> anyhow::Result<Value> {
        Ok(serde_json::from_str(&self.function.arguments)?)
    }
}

/// Encode pending tool calls as an assistant message for conversation history.
///
/// Wire format: `__tool_calls__:` + JSON array of [`ToolCall`] (OpenAI-compatible shape).
/// Providers decode this prefix when building API requests.
pub fn tool_calls_to_message(calls: &[ToolCall]) -> Message {
    let encoded = format!(
        "__tool_calls__:{}",
        serde_json::to_string(calls).unwrap_or_default()
    );
    Message {
        role: Role::Assistant,
        content: MessageContent::Text(encoded),
        tool_call_id: None,
    }
}

// ── Streaming delta ───────────────────────────────────────────────────────────

/// Incremental item emitted while streaming a chat completion.
#[derive(Debug, Clone)]
pub enum Delta {
    /// A chunk of assistant text.
    Text(String),
    /// The model wants to call a tool (may arrive in fragments; provider assembles).
    ToolCall(ToolCall),
    /// Token usage for the completed request (emitted just before Done).
    Usage {
        /// Prompt/input tokens billed.
        input_tokens: u32,
        /// Completion/output tokens billed.
        output_tokens: u32,
    },
    /// Prompt-cache statistics (Anthropic only). Emitted alongside Usage.
    CacheUsage {
        /// Tokens written to cache this request (incurs a write surcharge).
        cache_creation_tokens: u32,
        /// Tokens read from cache (cheapest — ~10% of normal input price).
        cache_read_tokens: u32,
    },
    /// Model stopped generating; stream is done.
    Done {
        /// Why generation stopped.
        stop_reason: StopReason,
    },
}

/// Reason the model stopped generating tokens.
#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    /// Natural end of turn.
    EndTurn,
    /// Model requested tool calls.
    ToolUse,
    /// Hit `max_tokens` limit.
    MaxTokens,
    /// Provider-specific stop reason string.
    Other(String),
}

// ── Request ───────────────────────────────────────────────────────────────────

/// Outbound chat completion request.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    /// Target model id.
    pub model: String,
    /// Conversation history.
    pub messages: Vec<Message>,
    /// Tool definitions available to the model.
    pub tools: Vec<ToolDefinition>,
    /// Maximum tokens to generate.
    pub max_tokens: u32,
    /// Sampling temperature.
    pub temperature: f32,
    /// Optional system prompt (may also be sent as a system message).
    pub system: Option<String>,
    /// Enable extended thinking. When Some(budget), the model will use up to
    /// `budget` tokens for internal reasoning (billed as output tokens).
    /// When None, adaptive thinking is used on Opus 4.7 (model decides).
    pub thinking_budget: Option<u32>,
    /// Enable provider-side native web search tool.
    pub native_web_search: bool,
    /// Enable provider-side sandboxed code execution tool.
    pub native_code_execution: bool,
    /// Enable provider-side X (Twitter) search tool.
    pub native_x_search: bool,
    /// Constrain response to a JSON Schema (strict structured output).
    /// When set, providers use their native structured-output mechanism:
    /// - OpenAI: `response_format` with `json_schema` + `strict: true`
    /// - Anthropic: synthetic tool trick (forces JSON tool call)
    /// - xAI: same as OpenAI
    /// - Ollama: `format` field
    pub response_schema: Option<ResponseSchema>,
}

/// A JSON Schema constraint for structured output responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseSchema {
    /// A name for the schema (used by OpenAI/xAI).
    pub name: String,
    /// The JSON Schema object (type, properties, required, etc.).
    pub schema: Value,
    /// If true, the provider must strictly follow the schema.
    #[serde(default = "default_strict")]
    pub strict: bool,
}

fn default_strict() -> bool {
    true
}

impl ResponseSchema {
    /// Build a strict JSON schema constraint.
    pub fn new(name: impl Into<String>, schema: Value) -> Self {
        Self {
            name: name.into(),
            schema,
            strict: true,
        }
    }
}

impl ChatRequest {
    /// Create a request with Harness defaults for the given model.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: 8192,
            temperature: 0.7,
            system: None,
            thinking_budget: None,
            native_web_search: false,
            native_code_execution: false,
            native_x_search: false,
            response_schema: None,
        }
    }

    /// Require structured JSON output matching the given schema.
    pub fn with_response_schema(mut self, schema: ResponseSchema) -> Self {
        self.response_schema = Some(schema);
        self
    }

    /// Model id to use for this request, preferring `self.model` when set.
    pub fn effective_model<'a>(&'a self, provider_default: &'a str) -> &'a str {
        if self.model.is_empty() {
            provider_default
        } else {
            &self.model
        }
    }

    /// Attach conversation messages.
    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    /// Attach tool definitions.
    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    /// Set the system prompt string.
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Enable extended thinking with a token budget.
    /// Pass `None` for Opus 4.7's adaptive thinking (model decides when to think).
    pub fn with_thinking(mut self, budget: Option<u32>) -> Self {
        self.thinking_budget = budget;
        self
    }

    /// Enable provider-managed native tools for this request.
    pub fn with_native_tools(
        mut self,
        web_search: bool,
        code_execution: bool,
        x_search: bool,
    ) -> Self {
        self.native_web_search = web_search;
        self.native_code_execution = code_execution;
        self.native_x_search = x_search;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_user_roundtrip() {
        let msg = Message::user("hello");
        let json = serde_json::to_string(&msg).expect("serialize");
        let back: Message = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.role, Role::User);
        assert_eq!(back.content.as_str(), "hello");
        assert!(back.tool_call_id.is_none());
    }

    #[test]
    fn message_tool_result_roundtrip() {
        let msg = Message::tool_result("call_1", "ok");
        let back: Message = serde_json::from_str(&serde_json::to_string(&msg).unwrap()).unwrap();
        assert_eq!(back.role, Role::Tool);
        assert_eq!(back.tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn tool_call_args_parse() {
        let call = ToolCall {
            id: "tc1".into(),
            kind: "function".into(),
            function: ToolCallFunction {
                name: "shell".into(),
                arguments: r#"{"command":"ls"}"#.into(),
            },
        };
        let args = call.args().expect("parse args");
        assert_eq!(args["command"], "ls");
    }

    #[test]
    fn tool_definition_roundtrip() {
        let def = ToolDefinition::new(
            "read_file",
            "Read a file",
            json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        );
        let back: ToolDefinition =
            serde_json::from_str(&serde_json::to_string(&def).unwrap()).unwrap();
        assert_eq!(back.function.name, "read_file");
        assert_eq!(back.kind, "function");
    }

    #[test]
    fn response_schema_roundtrip() {
        let schema = ResponseSchema::new(
            "answer",
            json!({
                "type": "object",
                "properties": { "ok": { "type": "boolean" } },
                "required": ["ok"]
            }),
        );
        let back: ResponseSchema =
            serde_json::from_str(&serde_json::to_string(&schema).unwrap()).unwrap();
        assert_eq!(back.name, "answer");
        assert!(back.strict);
    }

    #[test]
    fn role_deserializes_lowercase() {
        let role: Role = serde_json::from_str("\"assistant\"").unwrap();
        assert_eq!(role, Role::Assistant);
    }

    #[test]
    fn tool_calls_to_message_roundtrip_prefix() {
        let calls = vec![ToolCall {
            id: "call_1".into(),
            kind: "function".into(),
            function: ToolCallFunction {
                name: "read_file".into(),
                arguments: r#"{"path":"src/main.rs"}"#.into(),
            },
        }];
        let msg = tool_calls_to_message(&calls);
        let text = msg.content.as_str();
        assert!(text.starts_with("__tool_calls__:"));
        let json = text.strip_prefix("__tool_calls__:").unwrap();
        let back: Vec<ToolCall> = serde_json::from_str(json).unwrap();
        assert_eq!(back[0].function.name, "read_file");
    }

    #[test]
    fn effective_model_prefers_request_override() {
        let req = ChatRequest::new("override-model");
        assert_eq!(req.effective_model("default-model"), "override-model");
        let empty = ChatRequest::new("");
        assert_eq!(empty.effective_model("default-model"), "default-model");
    }
}
