use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Agent-callable tool implemented by each built-in capability.
#[async_trait]
pub trait Tool: Send + Sync {
    /// OpenAI-format tool schema sent to the provider.
    fn definition(&self) -> ToolDefinition;
    /// Execute the tool with JSON arguments from the model.
    async fn execute(&self, args: Value) -> anyhow::Result<String>;
}

/// Registry of named tools available to the agent loop.
#[derive(Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool; its schema `function.name` becomes the lookup key.
    pub fn register(&mut self, tool: impl Tool + 'static) {
        let def = tool.definition();
        self.tools.insert(def.function.name.clone(), Arc::new(tool));
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Collect all tool schemas for the provider request.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.definition()).collect()
    }

    /// Registered tool names.
    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Keep only tools whose names appear in `allow`. Empty allow → no change.
    pub fn retain_allowlist(&mut self, allow: &[String]) {
        if allow.is_empty() {
            return;
        }
        let set: std::collections::HashSet<&str> = allow.iter().map(|s| s.as_str()).collect();
        self.tools.retain(|name, _| set.contains(name.as_str()));
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
