//! BrowserTool — a single tool exposing all browser actions via an `action` field.
//!
//! The agent picks the action and relevant parameters; the tool dispatches
//! to the BrowserSession. The session is lazily created on first use.

use anyhow::Result;
use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use harness_tools::registry::Tool;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::session::BrowserSession;

pub struct BrowserTool {
    devtools_url: String,
    session: Arc<Mutex<Option<BrowserSession>>>,
}

impl BrowserTool {
    /// `devtools_url` is the Chrome remote debugging HTTP URL,
    /// e.g. "http://localhost:9222".
    pub fn new(devtools_url: impl Into<String>) -> Self {
        Self {
            devtools_url: devtools_url.into(),
            session: Arc::new(Mutex::new(None)),
        }
    }

    async fn session(&self) -> Result<Arc<Mutex<Option<BrowserSession>>>> {
        let mut lock = self.session.lock().await;
        if lock.is_none() {
            let s = BrowserSession::connect(&self.devtools_url).await?;
            *lock = Some(s);
        }
        drop(lock);
        Ok(self.session.clone())
    }
}

#[async_trait]
impl Tool for BrowserTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "browser",
            "Control a Chrome/Chromium browser via CDP. \
             Requires Chrome launched with --remote-debugging-port=9222 (or configured URL). \
             Actions: navigate, click, type, focus, get_text, get_links, evaluate, screenshot, page_info.",
            json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["navigate", "click", "type", "focus", "get_text", "get_links", "evaluate", "screenshot", "page_info"],
                        "description": "The browser action to perform."
                    },
                    "url": {
                        "type": "string",
                        "description": "URL to navigate to (required for navigate)."
                    },
                    "selector": {
                        "type": "string",
                        "description": "CSS selector (required for click, focus, get_text)."
                    },
                    "text": {
                        "type": "string",
                        "description": "Text to type (required for type)."
                    },
                    "expression": {
                        "type": "string",
                        "description": "JavaScript expression to evaluate (required for evaluate)."
                    }
                }
            }),
        )
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing action"))?;

        let session_arc = match self.session().await {
            Ok(s) => s,
            Err(e) => return Ok(format!("Browser connect failed: {e}\nEnsure Chrome is running with --remote-debugging-port=9222")),
        };

        let lock = session_arc.lock().await;
        let session = lock.as_ref().unwrap();

        let result = match action {
            "navigate" => {
                let url = args["url"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("navigate requires url"))?;
                session.navigate(url).await
            }
            "click" => {
                let sel = args["selector"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("click requires selector"))?;
                session.click(sel).await
            }
            "type" => {
                let text = args["text"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("type requires text"))?;
                session.type_text(text).await
            }
            "focus" => {
                let sel = args["selector"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("focus requires selector"))?;
                session.focus(sel).await
            }
            "get_text" => {
                let sel = args["selector"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("get_text requires selector"))?;
                session.get_text(sel).await
            }
            "get_links" => session.get_links().await,
            "evaluate" => {
                let expr = args["expression"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("evaluate requires expression"))?;
                session.evaluate(expr).await
            }
            "screenshot" => session.screenshot().await,
            "page_info" => session.page_info().await,
            other => Err(anyhow::anyhow!("unknown action: {other}")),
        };

        match result {
            Ok(s) => Ok(s),
            Err(e) => Ok(format!("browser error: {e}")),
        }
    }
}
