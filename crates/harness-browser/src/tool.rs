//! BrowserTool — a single tool exposing all browser actions via an `action` field.
//!
//! The agent picks the action and relevant parameters; the tool dispatches
//! to the BrowserSession. The session is lazily created on first use.

use anyhow::{Context, Result};
use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use harness_tools::registry::Tool;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::session::BrowserSession;

const KNOWN_ACTIONS: &[&str] = &[
    "navigate",
    "click",
    "type",
    "focus",
    "get_text",
    "get_links",
    "evaluate",
    "screenshot",
    "page_info",
];

fn validate_action(action: &str) -> Result<()> {
    if KNOWN_ACTIONS.contains(&action) {
        Ok(())
    } else {
        Err(anyhow::anyhow!("unknown action: {action}"))
    }
}

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
            let s = BrowserSession::connect(&self.devtools_url)
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Browser connect failed (url: {}): {e}\n\
                         Tips:\n\
                         • Start Chrome/Chromium with --remote-debugging-port matching this URL\n\
                         • Verify: curl -s {}/json/version\n\
                         • Use 127.0.0.1 if localhost/IPv6 fails\n\
                         • Full guide: docs/BROWSER_CDP.md",
                        self.devtools_url,
                        self.devtools_url.trim_end_matches('/')
                    )
                })?;
            *lock = Some(s);
        }
        drop(lock);
        Ok(self.session.clone())
    }

    /// Drop a dead session so the next call reconnects.
    async fn reset_session(&self) {
        let mut lock = self.session.lock().await;
        *lock = None;
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
            .ok_or_else(|| anyhow::anyhow!("missing action (one of: {})", KNOWN_ACTIONS.join(", ")))?;
        validate_action(action).with_context(|| {
            format!(
                "unknown browser action `{action}` — valid: {}",
                KNOWN_ACTIONS.join(", ")
            )
        })?;

        let session_arc = self.session().await?;

        let result = {
            let lock = session_arc.lock().await;
            let session = lock.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "browser session not connected — Chrome may have closed; retry the tool call"
                )
            })?;

            match action {
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
                _ => anyhow::bail!(
                    "unknown browser action: {action} — valid: {}",
                    KNOWN_ACTIONS.join(", ")
                ),
            }
        };

        if let Err(ref e) = result {
            let msg = e.to_string();
            if msg.contains("CDP connection closed")
                || msg.contains("WebSocket")
                || msg.contains("connection closed")
            {
                self.reset_session().await;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_tools::registry::Tool;

    #[tokio::test]
    async fn unknown_action_returns_err_before_connect() {
        let tool = BrowserTool::new("http://127.0.0.1:19222");
        let err = tool
            .execute(json!({ "action": "bogus" }))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("unknown action")
                || err.to_string().contains("unknown browser action"),
            "unexpected: {err}"
        );
    }

    #[tokio::test]
    async fn missing_action_returns_err() {
        let tool = BrowserTool::new("http://127.0.0.1:19222");
        let err = tool.execute(json!({})).await.unwrap_err();
        assert!(err.to_string().contains("missing action"));
    }

    #[tokio::test]
    async fn connect_failure_mentions_browser_docs() {
        let tool = BrowserTool::new("http://127.0.0.1:19222");
        let err = tool
            .execute(json!({ "action": "page_info" }))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("Browser connect failed") || err.contains("DevTools"),
            "unexpected: {err}"
        );
        assert!(
            err.contains("BROWSER_CDP") || err.contains("remote-debugging"),
            "expected setup tips: {err}"
        );
    }

    #[test]
    fn validate_action_accepts_known_actions() {
        for action in KNOWN_ACTIONS {
            validate_action(action).unwrap();
        }
    }
}
