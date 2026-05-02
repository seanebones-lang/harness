#![allow(dead_code, unused_mut)]
//! OpenTelemetry observability for Harness.
//!
//! Instruments agent turns, tool calls, embed operations, and MCP calls
//! with OTLP spans. Traces can be exported to:
//! - A local OTLP endpoint (e.g. Jaeger, Grafana Tempo)
//! - `~/.harness/traces/` as JSON files for offline replay
//!
//! Configure in `~/.harness/config.toml`:
//! ```toml
//! [observability]
//! enabled = true
//! otlp_endpoint = "http://localhost:4318"   # optional OTLP/HTTP export
//! local_traces = true                        # write to ~/.harness/traces/
//! ```

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ObservabilityConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// OTLP/HTTP endpoint (e.g. http://localhost:4318).
    pub otlp_endpoint: Option<String>,
    /// Write traces to ~/.harness/traces/ as JSONL.
    #[serde(default = "default_true")]
    pub local_traces: bool,
}

fn default_true() -> bool {
    true
}

// ── Trace types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub start_ts_us: u64,
    pub end_ts_us: u64,
    pub duration_ms: u64,
    pub status: SpanStatus,
    pub attributes: HashMap<String, serde_json::Value>,
    pub events: Vec<SpanEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpanStatus {
    Ok,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    pub name: String,
    pub ts_us: u64,
    pub attributes: HashMap<String, serde_json::Value>,
}

/// An in-progress span builder.
pub struct SpanBuilder {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub start: Instant,
    pub start_ts_us: u64,
    pub attributes: HashMap<String, serde_json::Value>,
    pub events: Vec<SpanEvent>,
    pub tracer: Tracer,
}

impl SpanBuilder {
    pub fn set_attr(&mut self, key: &str, value: impl Into<serde_json::Value>) {
        self.attributes.insert(key.to_string(), value.into());
    }

    pub fn add_event(&mut self, name: &str, attrs: HashMap<String, serde_json::Value>) {
        self.events.push(SpanEvent {
            name: name.to_string(),
            ts_us: now_us(),
            attributes: attrs,
        });
    }

    pub fn finish(self) -> Span {
        let end_ts_us = now_us();
        let duration_ms = self.start.elapsed().as_millis() as u64;
        let span = Span {
            trace_id: self.trace_id,
            span_id: self.span_id.clone(),
            parent_span_id: self.parent_span_id,
            name: self.name,
            start_ts_us: self.start_ts_us,
            end_ts_us,
            duration_ms,
            status: SpanStatus::Ok,
            attributes: self.attributes,
            events: self.events,
        };
        self.tracer.record(span.clone());
        span
    }

    pub fn finish_err(mut self, err: &str) -> Span {
        let end_ts_us = now_us();
        let duration_ms = self.start.elapsed().as_millis() as u64;
        let span = Span {
            trace_id: self.trace_id,
            span_id: self.span_id.clone(),
            parent_span_id: self.parent_span_id,
            name: self.name,
            start_ts_us: self.start_ts_us,
            end_ts_us,
            duration_ms,
            status: SpanStatus::Error(err.to_string()),
            attributes: self.attributes,
            events: self.events,
        };
        self.tracer.record(span.clone());
        span
    }
}

// ── Tracer ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Tracer {
    config: ObservabilityConfig,
    trace_id: String,
}

impl Tracer {
    pub fn new(cfg: ObservabilityConfig) -> Self {
        Self {
            config: cfg,
            trace_id: new_id(),
        }
    }

    pub fn span(&self, name: &str) -> SpanBuilder {
        self.span_with_parent(name, None)
    }

    pub fn span_with_parent(&self, name: &str, parent_id: Option<String>) -> SpanBuilder {
        SpanBuilder {
            trace_id: self.trace_id.clone(),
            span_id: new_id(),
            parent_span_id: parent_id,
            name: name.to_string(),
            start: Instant::now(),
            start_ts_us: now_us(),
            attributes: HashMap::new(),
            events: Vec::new(),
            tracer: self.clone(),
        }
    }

    pub fn child_tracer(&self) -> Self {
        Self {
            config: self.config.clone(),
            trace_id: self.trace_id.clone(),
        }
    }

    fn record(&self, span: Span) {
        if !self.config.enabled {
            return;
        }
        if self.config.local_traces {
            let _ = write_local_trace(&span);
        }
        if let Some(ref endpoint) = self.config.otlp_endpoint {
            let endpoint = endpoint.clone();
            let span_clone = span.clone();
            tokio::spawn(async move {
                let _ = export_otlp(&span_clone, &endpoint).await;
            });
        }
    }
}

fn write_local_trace(span: &Span) -> Result<()> {
    let dir = dirs::home_dir().unwrap_or_default().join(".harness/traces");
    std::fs::create_dir_all(&dir)?;
    // One file per trace, append spans as JSONL
    let file = dir.join(format!("{}.jsonl", span.trace_id));
    let line = serde_json::to_string(span)?;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)?;
    writeln!(f, "{line}")?;
    Ok(())
}

async fn export_otlp(span: &Span, endpoint: &str) -> Result<()> {
    // OTLP/HTTP JSON format (simplified)
    let payload = serde_json::json!({
        "resourceSpans": [{
            "resource": { "attributes": [{"key": "service.name", "value": {"stringValue": "harness"}}] },
            "scopeSpans": [{
                "scope": {"name": "harness-agent"},
                "spans": [{
                    "traceId": span.trace_id,
                    "spanId": span.span_id,
                    "parentSpanId": span.parent_span_id,
                    "name": span.name,
                    "startTimeUnixNano": span.start_ts_us * 1000,
                    "endTimeUnixNano": span.end_ts_us * 1000,
                    "attributes": span.attributes.iter().map(|(k, v)| {
                        serde_json::json!({"key": k, "value": {"stringValue": v.to_string()}})
                    }).collect::<Vec<_>>(),
                }]
            }]
        }]
    });

    let client = reqwest::Client::new();
    let url = format!("{endpoint}/v1/traces");
    let _ = client.post(&url).json(&payload).send().await;
    Ok(())
}

// ── CLI commands ──────────────────────────────────────────────────────────────

/// List recent traces from ~/.harness/traces/.
pub fn list_traces(limit: usize) -> Result<Vec<String>> {
    let dir = dirs::home_dir().unwrap_or_default().join(".harness/traces");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "jsonl").unwrap_or(false))
        .collect();
    files.sort_by(|a, b| {
        b.metadata()
            .and_then(|m| m.modified())
            .ok()
            .cmp(&a.metadata().and_then(|m| m.modified()).ok())
    });
    files.truncate(limit);
    Ok(files.iter().map(|p| p.display().to_string()).collect())
}

/// Load the last trace file and return its spans.
pub fn load_last_trace() -> Result<Vec<Span>> {
    let files = list_traces(1)?;
    let Some(path) = files.first() else {
        return Ok(vec![]);
    };
    let text = std::fs::read_to_string(path)?;
    let spans = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Span>(l).ok())
        .collect();
    Ok(spans)
}

/// Export a trace as JSON to stdout.
pub fn export_trace(trace_id: &str) -> Result<()> {
    let dir = dirs::home_dir().unwrap_or_default().join(".harness/traces");
    let file = dir.join(format!("{trace_id}.jsonl"));
    if !file.exists() {
        anyhow::bail!("trace {trace_id} not found");
    }
    let text = std::fs::read_to_string(&file)?;
    let spans: Vec<Span> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    println!("{}", serde_json::to_string_pretty(&spans)?);
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn new_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    now_us().hash(&mut h);
    std::thread::current().id().hash(&mut h);
    format!("{:016x}", h.finish())
}

fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}
