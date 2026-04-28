//! Wraps an MCP tool definition as a harness `Tool` so it plugs into
//! the existing `ToolRegistry` without any changes to the core.

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use harness_tools::registry::Tool;
use serde_json::Value;

use crate::client::{McpClient, McpToolDef};

pub struct McpToolAdapter {
    def: McpToolDef,
    client: McpClient,
}

impl McpToolAdapter {
    pub fn new(def: McpToolDef, client: McpClient) -> Self {
        Self { def, client }
    }
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            &self.def.name,
            self.def.description.as_deref().unwrap_or(""),
            self.def.input_schema.clone(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        self.client.call_tool(&self.def.name, args).await
    }
}
