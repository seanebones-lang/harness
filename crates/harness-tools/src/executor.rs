use crate::registry::ToolRegistry;
use harness_provider_core::ToolCall;
use tracing::{debug, warn};

pub struct ToolExecutor {
    registry: ToolRegistry,
}

impl ToolExecutor {
    pub fn new(registry: ToolRegistry) -> Self {
        Self { registry }
    }

    pub async fn execute(&self, call: &ToolCall) -> String {
        let args = match call.args() {
            Ok(v) => v,
            Err(e) => return format!("Error parsing tool arguments: {e}"),
        };

        let Some(tool) = self.registry.get(&call.function.name) else {
            warn!(name = %call.function.name, "unknown tool requested");
            return format!("Unknown tool: {}", call.function.name);
        };

        debug!(tool = %call.function.name, "executing tool");
        match tool.execute(args).await {
            Ok(output) => output,
            Err(e) => format!("Tool error: {e}"),
        }
    }

    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }
}
