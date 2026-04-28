//! Wire types for the xAI (Grok) OpenAI-compatible API.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Request ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ApiRequest {
    pub model: String,
    pub messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Value>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ApiMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ApiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ApiToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ApiToolCallFunction,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ApiToolCallFunction {
    pub name: String,
    pub arguments: String,
}

// ── Streaming SSE chunks ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct StreamChunk {
    pub choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
pub struct StreamChoice {
    pub delta: ChunkDelta,
    pub finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ChunkDelta {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<PartialToolCall>>,
    #[allow(dead_code)]
    pub role: Option<String>,
}

/// Tool call fragments arrive across multiple chunks; index tracks assembly.
#[derive(Deserialize, Clone)]
pub struct PartialToolCall {
    pub index: usize,
    pub id: Option<String>,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub kind: Option<String>,
    pub function: Option<PartialFunction>,
}

#[derive(Deserialize, Clone)]
pub struct PartialFunction {
    pub name: Option<String>,
    pub arguments: Option<String>,
}
