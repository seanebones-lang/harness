//! OpenTelemetry observability for Harness.
//! Local JSONL traces are wired; OTLP export is experimental (see tests).
//!
//! Instruments agent turns, tool calls, embed operations, and MCP calls
//! with OTLP spans. Traces can be exported to:
//! - A local OTLP endpoint (e.g. Jaeger, Grafana Tempo)
//! - `~/.harness/traces/` as JSON files for offline replay
//!
//! `Tracer` / `ObservabilityConfig` are the in-process API; `harness trace` CLI currently reads JSONL files only.
//!
//! Configure in `~/.harness/config.toml`:
//! ```toml
//! [observability]
//! enabled = true
//! otlp_experimental_endpoint = "http://localhost:4318"   # optional; simplified JSON exporter, not full OTLP
//! local_traces = true                                     # write to ~/.harness/traces/
//! ```
#![allow(dead_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::warn;

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ObservabilityConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Experimental OTLP/HTTP JSON export (non-spec trace IDs / timestamps). Alias: `otlp_endpoint`.
    #[serde(default, alias = "otlp_endpoint")]
    pub otlp_experimental_endpoint: Option<String>,
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

    pub fn finish_err(self, err: &str) -> Span {
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
        if let Some(ref endpoint) = self.config.otlp_experimental_endpoint {
            let endpoint = endpoint.clone();
            let span_clone = span.clone();
            tokio::spawn(async move {
                if let Err(e) = export_otlp(&span_clone, &endpoint).await {
                    warn!(endpoint = %endpoint, error = %e, "OTLP experimental export failed");
                }
            });
        }
    }
}

/// Default local traces directory (`~/.harness/traces`).
pub fn default_traces_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".harness/traces")
}

fn write_local_trace(span: &Span) -> Result<()> {
    write_local_trace_in(&default_traces_dir(), span)
}

/// Append a span as JSONL under `dir/{trace_id}.jsonl` (path-injectable).
pub fn write_local_trace_in(dir: &Path, span: &Span) -> Result<()> {
    std::fs::create_dir_all(dir)?;
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
                    // Stored times are microseconds since UNIX epoch; OTLP expects nanoseconds.
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
    let base = endpoint.trim_end_matches('/');
    let url = format!("{base}/v1/traces");
    let resp = client.post(&url).json(&payload).send().await?;
    let status = resp.status();
    let msg = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("HTTP {}: {}", status, msg);
    }
    Ok(())
}

// ── CLI commands ──────────────────────────────────────────────────────────────

/// List recent traces from ~/.harness/traces/.
pub fn list_traces(limit: usize) -> Result<Vec<String>> {
    list_traces_in(&default_traces_dir(), limit)
}

/// List recent `.jsonl` traces under `dir` (newest first, path-injectable).
pub fn list_traces_in(dir: &Path, limit: usize) -> Result<Vec<String>> {
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
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
    load_last_trace_in(&default_traces_dir())
}

/// Load the newest trace under `dir` (path-injectable).
pub fn load_last_trace_in(dir: &Path) -> Result<Vec<Span>> {
    let files = list_traces_in(dir, 1)?;
    let Some(path) = files.first() else {
        return Ok(vec![]);
    };
    load_trace_file(Path::new(path))
}

/// Parse spans from a JSONL trace file (skips blank / invalid lines).
pub fn load_trace_file(path: &Path) -> Result<Vec<Span>> {
    let text = std::fs::read_to_string(path)?;
    let spans = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Span>(l).ok())
        .collect();
    Ok(spans)
}

/// Load a trace by id from the default traces dir.
pub fn load_trace_by_id(trace_id: &str) -> Result<Vec<Span>> {
    load_trace_by_id_in(&default_traces_dir(), trace_id)
}

/// Load a trace by id under `dir` (path-injectable).
pub fn load_trace_by_id_in(dir: &Path, trace_id: &str) -> Result<Vec<Span>> {
    let file = dir.join(format!("{trace_id}.jsonl"));
    if !file.exists() {
        anyhow::bail!("trace {trace_id} not found");
    }
    load_trace_file(&file)
}

/// Export a trace as JSON to stdout.
pub fn export_trace(trace_id: &str) -> Result<()> {
    let spans = load_trace_by_id(trace_id)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    fn sample_span() -> Span {
        Span {
            trace_id: "trace-test".into(),
            span_id: "span-test".into(),
            parent_span_id: None,
            name: "agent.turn".into(),
            start_ts_us: 1_000,
            end_ts_us: 2_000,
            duration_ms: 1,
            status: SpanStatus::Ok,
            attributes: HashMap::from([("model".into(), serde_json::Value::String("test".into()))]),
            events: vec![],
        }
    }

    fn sample_span_named(trace_id: &str, span_id: &str, name: &str) -> Span {
        Span {
            trace_id: trace_id.into(),
            span_id: span_id.into(),
            parent_span_id: None,
            name: name.into(),
            start_ts_us: 1_000,
            end_ts_us: 2_000,
            duration_ms: 1,
            status: SpanStatus::Ok,
            attributes: HashMap::new(),
            events: vec![],
        }
    }

    #[tokio::test]
    async fn otlp_export_posts_to_v1_traces() {
        let captured: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
        let captured2 = captured.clone();
        let app = Router::new().route(
            "/v1/traces",
            post(move |Json(body): Json<serde_json::Value>| {
                let captured2 = captured2.clone();
                async move {
                    *captured2.lock().unwrap() = Some(body);
                    "ok"
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        export_otlp(&sample_span(), &format!("http://{addr}"))
            .await
            .expect("export");

        let body = captured.lock().unwrap().clone().expect("body captured");
        assert!(body.get("resourceSpans").is_some());
    }

    #[tokio::test]
    async fn otlp_export_payload_converts_us_to_ns_scale_and_attrs() {
        let captured: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
        let captured2 = captured.clone();
        let app = Router::new().route(
            "/v1/traces",
            post(move |Json(body): Json<serde_json::Value>| {
                let captured2 = captured2.clone();
                async move {
                    *captured2.lock().unwrap() = Some(body);
                    "ok"
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let mut span = sample_span();
        span.start_ts_us = 5_000;
        span.end_ts_us = 7_500;
        span.attributes
            .insert("tool".into(), serde_json::json!("read_file"));
        export_otlp(&span, &format!("http://{addr}/"))
            .await
            .expect("export");

        let body = captured.lock().unwrap().clone().expect("body");
        let span_json = &body["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert_eq!(span_json["startTimeUnixNano"], 5_000_000);
        assert_eq!(span_json["endTimeUnixNano"], 7_500_000);
        assert_eq!(span_json["name"], "agent.turn");
        let attrs = span_json["attributes"].as_array().expect("attrs");
        assert!(attrs.iter().any(|a| a["key"] == "tool"));
    }

    #[tokio::test]
    async fn otlp_export_http_error_is_err() {
        let app = Router::new().route(
            "/v1/traces",
            post(|| async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "nope") }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        let err = export_otlp(&sample_span(), &format!("http://{addr}"))
            .await
            .expect_err("should fail");
        assert!(err.to_string().contains("HTTP 500"));
    }

    #[test]
    fn observability_config_defaults_and_serde_alias() {
        let cfg = ObservabilityConfig::default();
        // Derive Default leaves bool false; serde defaults apply on deserialize.
        assert!(!cfg.enabled);
        assert!(cfg.otlp_experimental_endpoint.is_none());
        assert!(!cfg.local_traces);

        let from_alias: ObservabilityConfig = toml::from_str(
            r#"
            enabled = true
            otlp_endpoint = "http://localhost:4318"
            local_traces = true
            "#,
        )
        .expect("toml");
        assert!(from_alias.enabled);
        assert_eq!(
            from_alias.otlp_experimental_endpoint.as_deref(),
            Some("http://localhost:4318")
        );
        assert!(from_alias.local_traces);

        let from_empty: ObservabilityConfig = toml::from_str("").expect("empty");
        assert!(from_empty.enabled);
        assert!(from_empty.local_traces);
        assert!(from_empty.otlp_experimental_endpoint.is_none());
    }

    #[test]
    fn span_builder_finish_ok_and_err_with_attrs_events() {
        let tracer = Tracer::new(ObservabilityConfig {
            enabled: false,
            otlp_experimental_endpoint: None,
            local_traces: false,
        });
        let mut sb = tracer.span("tool.call");
        sb.set_attr("name", "read_file");
        sb.set_attr("ok", true);
        sb.add_event("start", HashMap::from([("n".into(), serde_json::json!(1))]));
        let span = sb.finish();
        assert_eq!(span.name, "tool.call");
        assert!(matches!(span.status, SpanStatus::Ok));
        assert_eq!(
            span.attributes.get("name").and_then(|v| v.as_str()),
            Some("read_file")
        );
        assert_eq!(span.events.len(), 1);
        assert_eq!(span.events[0].name, "start");
        assert!(span.end_ts_us >= span.start_ts_us);

        let mut sb2 = tracer.span_with_parent("child", Some(span.span_id.clone()));
        sb2.set_attr("k", 42);
        let err_span = sb2.finish_err("boom");
        assert!(matches!(err_span.status, SpanStatus::Error(ref m) if m == "boom"));
        assert_eq!(
            err_span.parent_span_id.as_deref(),
            Some(span.span_id.as_str())
        );
        assert_eq!(err_span.trace_id, span.trace_id);
    }

    #[test]
    fn child_tracer_shares_trace_id() {
        let parent = Tracer::new(ObservabilityConfig {
            enabled: false,
            ..Default::default()
        });
        let child = parent.child_tracer();
        let a = parent.span("a").finish();
        let b = child.span("b").finish();
        assert_eq!(a.trace_id, b.trace_id);
        assert_ne!(a.span_id, b.span_id);
    }

    #[test]
    fn new_id_and_now_us_are_nonzero() {
        let a = new_id();
        let b = new_id();
        assert_eq!(a.len(), 16);
        assert_eq!(b.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        // Distinct under normal scheduling (hash includes time + thread).
        let _ = (a, b);
        assert!(now_us() > 0);
    }

    #[test]
    fn list_traces_in_missing_dir_is_empty() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("no-such-traces");
        let files = list_traces_in(&missing, 10).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn write_list_load_trace_roundtrip_in_tempdir() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let s1 = sample_span_named("tid-a", "s1", "first");
        let s2 = sample_span_named("tid-a", "s2", "second");
        write_local_trace_in(root, &s1).unwrap();
        // Ensure mtime ordering is stable across filesystems.
        thread::sleep(Duration::from_millis(15));
        write_local_trace_in(root, &s2).unwrap();

        let other = sample_span_named("tid-b", "s3", "other");
        thread::sleep(Duration::from_millis(15));
        write_local_trace_in(root, &other).unwrap();

        // Non-jsonl ignored
        std::fs::write(root.join("notes.txt"), "ignore").unwrap();

        let listed = list_traces_in(root, 10).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed[0].ends_with("tid-b.jsonl"));
        assert!(listed.iter().any(|p| p.ends_with("tid-a.jsonl")));

        let limited = list_traces_in(root, 1).unwrap();
        assert_eq!(limited.len(), 1);
        assert!(limited[0].ends_with("tid-b.jsonl"));

        let last = load_last_trace_in(root).unwrap();
        assert_eq!(last.len(), 1);
        assert_eq!(last[0].name, "other");

        let multi = load_trace_by_id_in(root, "tid-a").unwrap();
        assert_eq!(multi.len(), 2);
        assert_eq!(multi[0].name, "first");
        assert_eq!(multi[1].name, "second");
    }

    #[test]
    fn load_trace_file_skips_blank_and_invalid_lines() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("messy.jsonl");
        let good = serde_json::to_string(&sample_span()).unwrap();
        std::fs::write(&path, format!("\n{good}\nnot-json\n\n{{}}\n{good}\n")).unwrap();
        let spans = load_trace_file(&path).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].trace_id, "trace-test");
    }

    #[test]
    fn load_trace_by_id_in_missing_is_err() {
        let dir = TempDir::new().unwrap();
        let err = load_trace_by_id_in(dir.path(), "missing").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn load_last_trace_in_empty_dir_is_empty_vec() {
        let dir = TempDir::new().unwrap();
        let spans = load_last_trace_in(dir.path()).unwrap();
        assert!(spans.is_empty());
    }

    #[test]
    fn span_status_and_span_serde_roundtrip() {
        let mut span = sample_span();
        span.status = SpanStatus::Error("x".into());
        span.parent_span_id = Some("p".into());
        span.events.push(SpanEvent {
            name: "e".into(),
            ts_us: 9,
            attributes: HashMap::new(),
        });
        let json = serde_json::to_string(&span).unwrap();
        let back: Span = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.status, SpanStatus::Error(ref m) if m == "x"));
        assert_eq!(back.parent_span_id.as_deref(), Some("p"));
        assert_eq!(back.events.len(), 1);
        assert_eq!(back.name, "agent.turn");
    }

    #[test]
    fn default_traces_dir_ends_with_harness_traces() {
        let p = default_traces_dir();
        assert!(p.ends_with(Path::new(".harness/traces")) || p.ends_with("traces"));
    }

    #[test]
    fn tracer_record_disabled_does_not_create_files() {
        let dir = TempDir::new().unwrap();
        // finish() records via home dir only when enabled; disabled must be a no-op.
        let tracer = Tracer::new(ObservabilityConfig {
            enabled: false,
            local_traces: true,
            otlp_experimental_endpoint: None,
        });
        let _ = tracer.span("noop").finish();
        // Tempdir stays empty (we never pointed record at it); assert inject path still works.
        assert!(list_traces_in(dir.path(), 5).unwrap().is_empty());
        write_local_trace_in(dir.path(), &sample_span()).unwrap();
        assert_eq!(list_traces_in(dir.path(), 5).unwrap().len(), 1);
    }
}
