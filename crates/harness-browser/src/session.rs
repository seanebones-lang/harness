//! CDP WebSocket session for a single browser tab.

use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::cdp::{CdpRequest, CdpResponse, ChromeTarget};

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub struct BrowserSession {
    ws: Arc<Mutex<WsStream>>,
    next_id: AtomicU64,
    /// HTTP base URL for the Chrome DevTools remote, e.g. "http://localhost:9222"
    devtools_url: String,
}

impl BrowserSession {
    /// Connect to a running Chrome/Chromium with `--remote-debugging-port=<port>`.
    /// Opens (or reuses) the first available page target.
    pub async fn connect(devtools_url: &str) -> Result<Self> {
        let target = Self::find_or_open_target(devtools_url).await?;
        let ws_url = target
            .web_socket_debugger_url
            .context("target has no WebSocket URL")?;

        let (ws, _) = connect_async(&ws_url)
            .await
            .with_context(|| format!("WebSocket connect failed: {ws_url}"))?;

        Ok(Self {
            ws: Arc::new(Mutex::new(ws)),
            next_id: AtomicU64::new(1),
            devtools_url: devtools_url.to_string(),
        })
    }

    async fn find_or_open_target(base: &str) -> Result<ChromeTarget> {
        let client = reqwest::Client::new();

        let targets: Vec<ChromeTarget> = client
            .get(format!("{base}/json/list"))
            .send()
            .await
            .context("Chrome DevTools HTTP unreachable")?
            .json()
            .await
            .context("parsing /json/list")?;

        // Prefer an existing page target.
        if let Some(t) = targets
            .into_iter()
            .find(|t| t.r#type == "page" && t.web_socket_debugger_url.is_some())
        {
            return Ok(t);
        }

        // Open a new tab.
        let target: ChromeTarget = client
            .get(format!("{base}/json/new"))
            .send()
            .await
            .context("creating new Chrome tab")?
            .json()
            .await
            .context("parsing /json/new")?;

        Ok(target)
    }

    // ── Low-level send/recv ─────────────────────────────────────────────────

    async fn send(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = CdpRequest {
            id,
            method: method.to_string(),
            params,
        };

        let text = serde_json::to_string(&req)?;
        {
            let mut ws = self.ws.lock().await;
            ws.send(Message::Text(text.into()))
                .await
                .context("CDP send")?;
        }

        // Read frames until we find the response matching our id.
        loop {
            let frame = {
                let mut ws = self.ws.lock().await;
                ws.next().await.context("CDP connection closed")?
            };
            let msg = frame.context("CDP frame error")?;
            if let Message::Text(txt) = msg {
                let resp: CdpResponse =
                    serde_json::from_str(&txt).context("CDP response parse")?;
                if resp.id == Some(id) {
                    if let Some(err) = resp.error {
                        anyhow::bail!("CDP error {}: {}", err.code, err.message);
                    }
                    return Ok(resp.result.unwrap_or(Value::Null));
                }
                // Drop events that don't match our id.
            }
        }
    }

    // ── High-level actions ──────────────────────────────────────────────────

    /// Navigate to `url` and wait for load.
    pub async fn navigate(&self, url: &str) -> Result<String> {
        self.send("Page.enable", json!({})).await.ok();
        let _result = self
            .send("Page.navigate", json!({ "url": url }))
            .await?;
        // Wait for loadEventFired or a short settle period.
        self.wait_for_load().await.ok();
        Ok(format!("Navigated to {url}"))
    }

    async fn wait_for_load(&self) -> Result<()> {
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_secs(15);
        loop {
            if tokio::time::Instant::now() > deadline {
                break;
            }
            let frame = {
                let mut ws = self.ws.lock().await;
                tokio::time::timeout(
                    std::time::Duration::from_millis(200),
                    ws.next(),
                )
                .await
            };
            if let Ok(Some(Ok(Message::Text(txt)))) = frame {
                let resp: CdpResponse = serde_json::from_str(&txt).unwrap_or(CdpResponse {
                    id: None,
                    result: None,
                    error: None,
                    method: None,
                    params: None,
                });
                if resp.method.as_deref() == Some("Page.loadEventFired") {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Evaluate JS in the page and return the result as a string.
    pub async fn evaluate(&self, expression: &str) -> Result<String> {
        let result = self
            .send(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": true,
                }),
            )
            .await?;

        let value = &result["result"]["value"];
        if value.is_null() {
            Ok(result["result"]["description"]
                .as_str()
                .unwrap_or("undefined")
                .to_string())
        } else {
            Ok(match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
        }
    }

    /// Click the first element matching a CSS selector.
    pub async fn click(&self, selector: &str) -> Result<String> {
        let escaped = selector.replace('\'', "\\'");
        let js = format!(
            r#"
            (function() {{
                const el = document.querySelector('{escaped}');
                if (!el) return 'no element matching {escaped}';
                el.click();
                return 'clicked';
            }})()
            "#
        );
        self.evaluate(&js).await
    }

    /// Type text into the focused element (sends key events).
    pub async fn type_text(&self, text: &str) -> Result<String> {
        for ch in text.chars() {
            self.send(
                "Input.dispatchKeyEvent",
                json!({
                    "type": "char",
                    "text": ch.to_string(),
                }),
            )
            .await?;
        }
        Ok(format!("Typed {} characters", text.len()))
    }

    /// Focus the first element matching a CSS selector.
    pub async fn focus(&self, selector: &str) -> Result<String> {
        let escaped = selector.replace('\'', "\\'");
        let js = format!(
            r#"
            (function() {{
                const el = document.querySelector('{escaped}');
                if (!el) return 'no element matching {escaped}';
                el.focus();
                return 'focused';
            }})()
            "#
        );
        self.evaluate(&js).await
    }

    /// Get text content of elements matching `selector`.
    pub async fn get_text(&self, selector: &str) -> Result<String> {
        let escaped = selector.replace('\'', "\\'");
        let js = format!(
            r#"
            (function() {{
                const els = [...document.querySelectorAll('{escaped}')];
                return els.map(e => e.innerText || e.textContent).join('\n---\n');
            }})()
            "#
        );
        let text = self.evaluate(&js).await?;
        if text.is_empty() {
            Ok(format!("No elements matching {selector}"))
        } else {
            Ok(text)
        }
    }

    /// Get all anchor hrefs and their link text from the page.
    pub async fn get_links(&self) -> Result<String> {
        let js = r#"
            (function() {
                const links = [...document.querySelectorAll('a[href]')];
                return links.map(a => `${a.href}  ${a.innerText.trim()}`).join('\n');
            })()
        "#;
        self.evaluate(js).await
    }

    /// Take a screenshot and return base64-encoded PNG.
    pub async fn screenshot(&self) -> Result<String> {
        let result = self
            .send(
                "Page.captureScreenshot",
                json!({ "format": "png", "quality": 80 }),
            )
            .await?;

        let data = result["data"]
            .as_str()
            .context("screenshot: no data field")?;
        Ok(format!("screenshot:base64:{data}"))
    }

    /// Return the page title and current URL.
    pub async fn page_info(&self) -> Result<String> {
        let title = self
            .evaluate("document.title")
            .await
            .unwrap_or_default();
        let url = self
            .evaluate("location.href")
            .await
            .unwrap_or_default();
        Ok(format!("URL: {url}\nTitle: {title}"))
    }

    pub fn devtools_url(&self) -> &str {
        &self.devtools_url
    }
}
