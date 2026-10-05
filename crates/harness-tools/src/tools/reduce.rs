//! reduce_obs — evidence-preserving reducer with forced schema.
use crate::observation::{ObservationHandle, ObservationStore};
use crate::registry::Tool;
use harness_provider_core::{ChatRequest, Delta, Message, ResponseSchema, ToolDefinition};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use futures::StreamExt;
use std::sync::Arc;

/// Input for reduce_obs tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReduceObsInput {
    /// obs:// handle or file path to reduce.
    pub handle: String,
    /// Question to answer from the observation.
    pub question: String,
    /// Optional max lines to read from the observation.
    pub max_lines: Option<usize>,
}

/// Citation linking an answer to source observation lines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceCitation {
    /// obs:// handle of the source observation.
    pub handle: String,
    /// Inclusive line range [start, end] within the observation.
    pub lines: [usize; 2],
    /// Verbatim quote from the observation.
    pub quote: String,
}

/// Structured output from reduce_obs: answer + citations + uncertainty.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReduceObsOutput {
    /// The answer to the question.
    pub answer: String,
    /// Evidence citations supporting the answer.
    pub evidence: Vec<EvidenceCitation>,
    /// Self-assessed uncertainty: "low", "medium", or "high".
    pub uncertainty: String,
}

/// Tool for reducing observations to structured answers with evidence.
pub struct ReduceObsTool {
    /// Fast-model router for the reduction LLM call.
    pub router: Arc<harness_provider_router::ProviderRouter>,
    /// Observation store to read from.
    pub obs_store: Arc<ObservationStore>,
}

#[async_trait::async_trait]
impl Tool for ReduceObsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "reduce_obs",
            "Read an observation and return a structured answer with cited evidence. Uses fast model.",
            json!({
                "type": "object",
                "properties": {
                    "handle": { "type": "string", "description": "obs:// handle or file path" },
                    "question": { "type": "string", "description": "Specific question to answer from the observation" },
                    "max_lines": { "type": "integer", "minimum": 10, "maximum": 500, "default": 200 }
                },
                "required": ["handle", "question"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let input: ReduceObsInput = serde_json::from_value(args)?;

        let content = if let Some(handle) = ObservationHandle::from_uri(&input.handle) {
            self.obs_store.read_range(&handle, 0, Some(input.max_lines.unwrap_or(200))).await?
        } else {
            tokio::fs::read_to_string(&input.handle).await?
        };

        let child_prompt = format!(
            "You are a precise evidence extractor. Read the observation below and answer the question.\n\
             Return ONLY valid JSON matching the schema. Every claim in 'answer' MUST have a corresponding entry in 'evidence' quoting exact lines.\n\
             If the observation does not contain the answer, set uncertainty=\"high\" and evidence=[]; do not hallucinate.\n\n\
             OBSERVATION:\n{}\n\nQUESTION: {}\n",
            content, input.question
        );

        let fast_model = self.router.fast_model_id().ok_or_else(|| anyhow::anyhow!("no fast model"))?.to_string();
        let schema = ResponseSchema::new("reduce_obs_output", json!({
            "type": "object",
            "properties": {
                "answer": { "type": "string" },
                "evidence": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "handle": { "type": "string" },
                            "lines": { "type": "array", "items": { "type": "integer" }, "minItems": 2, "maxItems": 2 },
                            "quote": { "type": "string" }
                        },
                        "required": ["handle", "lines", "quote"]
                    }
                },
                "uncertainty": { "type": "string", "enum": ["low", "medium", "high"] }
            },
            "required": ["answer", "evidence", "uncertainty"]
        }));

        let req = ChatRequest::new(&fast_model)
            .with_messages(vec![Message::user(&child_prompt)])
            .with_response_schema(schema);

        let mut stream = self.router.fast_provider().cloned().ok_or_else(|| anyhow::anyhow!("no fast provider configured"))?.stream_chat(req).await?;
        let mut json_text = String::new();
        while let Some(Ok(Delta::Text(chunk))) = stream.next().await {
            json_text.push_str(&chunk);
        }

        let output: ReduceObsOutput = serde_json::from_str(&json_text)
            .map_err(|e| anyhow::anyhow!("reduce_obs child returned invalid JSON: {}", e))?;

        if output.evidence.is_empty() && output.uncertainty != "high" {
            return Err(anyhow::anyhow!("reduce_obs: evidence empty but uncertainty not high"));
        }

        serde_json::to_string_pretty(&output).map_err(Into::into)
    }
}
