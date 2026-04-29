use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Messages ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
    /// Present when role == Tool; the tool_call_id this result belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(text: impl Into<String>) -> Self {
        Self { role: Role::System, content: MessageContent::Text(text.into()), tool_call_id: None }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self { role: Role::User, content: MessageContent::Text(text.into()), tool_call_id: None }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: MessageContent::Text(text.into()), tool_call_id: None }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, result: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: MessageContent::Text(result.into()),
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl MessageContent {
    pub fn as_str(&self) -> &str {
        match self {
            MessageContent::Text(s) => s,
            MessageContent::Parts(parts) => {
                // Return the first text part if available.
                parts.iter()
                    .find_map(|p| p.text.as_deref())
                    .unwrap_or("")
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
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        let _ = write!(out, "{}", TABLE[(b0 >> 2) & 63] as char);
        let _ = write!(out, "{}", TABLE[((b0 << 4) | (b1 >> 4)) & 63] as char);
        let _ = write!(out, "{}", if chunk.len() > 1 { TABLE[((b1 << 2) | (b2 >> 6)) & 63] as char } else { '=' });
        let _ = write!(out, "{}", if chunk.len() > 2 { TABLE[b2 & 63] as char } else { '=' });
    }
    out
}

fn mime_for_path(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPart {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Base64-encoded image data (for type = "image_url").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<ImageUrl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    /// Data URI: "data:image/png;base64,..." or an https:// URL.
    pub url: String,
}

impl ContentPart {
    pub fn text(t: impl Into<String>) -> Self {
        Self { kind: "text".into(), text: Some(t.into()), image_url: None }
    }

    pub fn image_base64(mime: &str, data: &str) -> Self {
        Self {
            kind: "image_url".into(),
            text: None,
            image_url: Some(ImageUrl { url: format!("data:{mime};base64,{data}") }),
        }
    }
}

// ── Tools ─────────────────────────────────────────────────────────────────────

/// OpenAI-format tool definition (Grok accepts this natively).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: String, // always "function"
    pub function: FunctionDef,
}

impl ToolDefinition {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// A tool call the model wants to make.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String, // JSON string
}

impl ToolCall {
    pub fn args(&self) -> anyhow::Result<Value> {
        Ok(serde_json::from_str(&self.function.arguments)?)
    }
}

// ── Streaming delta ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Delta {
    /// A chunk of assistant text.
    Text(String),
    /// The model wants to call a tool (may arrive in fragments; provider assembles).
    ToolCall(ToolCall),
    /// Token usage for the completed request (emitted just before Done).
    Usage { input_tokens: u32, output_tokens: u32 },
    /// Model stopped generating; stream is done.
    Done { stop_reason: StopReason },
}

#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Other(String),
}

// ── Request ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub system: Option<String>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: 8192,
            temperature: 0.7,
            system: None,
        }
    }

    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }
}
