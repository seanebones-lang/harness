//! Optional remote/shared swarm registry (W7.1).
//!
//! Local default remains SQLite via [`LocalSqliteRegistry`]. When
//! `[swarm].registry_url` is set, [`HttpRegistry`] talks REST:
//!
//! - `POST {base}/tasks` — register `{prompt, model?}` → task JSON
//! - `GET {base}/tasks?limit=N` — list `{tasks:[…]}` or bare array
//! - `GET {base}/tasks/:id` — one task (404 → None)
//! - `PUT {base}/tasks/:id` — update `{status, result?, error?}`
//!
//! Auth: optional `Authorization: Bearer $HARNESS_SWARM_TOKEN`.
//! Unreachable remote → hard error (no silent split-brain). See `docs/WAVE7_SCALE.md`.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::swarm::{self, TaskEntry, TaskId, TaskStatus};

/// Pluggable swarm task registry.
pub trait SwarmRegistry: Send + Sync {
    /// Register a pending task.
    fn register(&self, prompt: &str, model: Option<&str>) -> Result<TaskId>;
    /// Update status/result.
    fn update(&self, id: &str, status: &TaskStatus, result: Option<&str>) -> Result<()>;
    /// List recent tasks.
    fn list(&self, limit: usize) -> Result<Vec<TaskEntry>>;
    /// Fetch one task by id/prefix.
    fn get(&self, id: &str) -> Result<Option<TaskEntry>>;
    /// Backend name for diagnostics.
    fn backend_name(&self) -> &'static str;
}

/// Default local SQLite registry (wraps `crate::swarm` local helpers).
#[derive(Debug, Default, Clone)]
pub struct LocalSqliteRegistry;

impl SwarmRegistry for LocalSqliteRegistry {
    fn register(&self, prompt: &str, model: Option<&str>) -> Result<TaskId> {
        // Call local_* to avoid re-entering remote routing.
        match model {
            Some(m) => swarm::register_task_local(prompt, Some(m)),
            None => swarm::register_task_local(prompt, None),
        }
    }

    fn update(&self, id: &str, status: &TaskStatus, result: Option<&str>) -> Result<()> {
        swarm::update_status_local(id, status, result)
    }

    fn list(&self, limit: usize) -> Result<Vec<TaskEntry>> {
        swarm::list_tasks_local(limit)
    }

    fn get(&self, id: &str) -> Result<Option<TaskEntry>> {
        swarm::get_task_local(id)
    }

    fn backend_name(&self) -> &'static str {
        "sqlite-local"
    }
}

/// HTTP registry client (sync; uses reqwest blocking).
#[derive(Debug, Clone)]
pub struct HttpRegistry {
    /// Base URL, e.g. `https://swarm.example/v1` (no trailing slash required).
    pub base_url: String,
    /// Optional bearer token (from env `HARNESS_SWARM_TOKEN` at construct time).
    pub token: Option<String>,
    /// Request timeout seconds.
    pub timeout_secs: u64,
}

impl HttpRegistry {
    /// Build from config URL + env token.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: std::env::var("HARNESS_SWARM_TOKEN")
                .ok()
                .filter(|s| !s.is_empty()),
            timeout_secs: 15,
        }
    }

    fn client(&self) -> Result<reqwest::blocking::Client> {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .build()
            .context("build HTTP client for swarm registry")
    }

    fn apply_auth(
        &self,
        req: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        match &self.token {
            Some(t) => req.header("Authorization", format!("Bearer {t}")),
            None => req,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn map_transport(err: reqwest::Error, base: &str) -> anyhow::Error {
        anyhow!(
            "remote swarm registry unreachable (url={base}): {err}. \
             unset [swarm].registry_url to use local SQLite — see docs/WAVE7_SCALE.md"
        )
    }
}

impl SwarmRegistry for HttpRegistry {
    fn register(&self, prompt: &str, model: Option<&str>) -> Result<TaskId> {
        let client = self.client()?;
        let body = json!({
            "prompt": prompt,
            "model": model,
        });
        let req = self.apply_auth(client.post(self.url("/tasks")).json(&body));
        let resp = req
            .send()
            .map_err(|e| Self::map_transport(e, &self.base_url))?;
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        if !status.is_success() {
            bail!(
                "remote swarm register failed HTTP {status}: {}",
                trunc(&text, 200)
            );
        }
        let v: Value = serde_json::from_str(&text)
            .with_context(|| format!("register response not JSON: {}", trunc(&text, 120)))?;
        let task = task_from_json(&v).context("register response missing task fields")?;
        Ok(task.id)
    }

    fn update(&self, id: &str, status: &TaskStatus, result: Option<&str>) -> Result<()> {
        let client = self.client()?;
        let (status_s, error) = match status {
            TaskStatus::Failed(msg) => ("failed", Some(msg.as_str())),
            other => (other.as_str(), None),
        };
        let body = json!({
            "status": status_s,
            "result": result,
            "error": error,
        });
        let path = format!("/tasks/{}", urlencoding_path(id));
        let req = self.apply_auth(client.put(self.url(&path)).json(&body));
        let resp = req
            .send()
            .map_err(|e| Self::map_transport(e, &self.base_url))?;
        let code = resp.status();
        if !code.is_success() {
            let text = resp.text().unwrap_or_default();
            bail!(
                "remote swarm update failed HTTP {code}: {}",
                trunc(&text, 200)
            );
        }
        Ok(())
    }

    fn list(&self, limit: usize) -> Result<Vec<TaskEntry>> {
        let client = self.client()?;
        let req = self.apply_auth(
            client
                .get(self.url("/tasks"))
                .query(&[("limit", limit.to_string())]),
        );
        let resp = req
            .send()
            .map_err(|e| Self::map_transport(e, &self.base_url))?;
        let code = resp.status();
        let text = resp.text().unwrap_or_default();
        if !code.is_success() {
            bail!(
                "remote swarm list failed HTTP {code}: {}",
                trunc(&text, 200)
            );
        }
        let v: Value = serde_json::from_str(&text)
            .with_context(|| format!("list response not JSON: {}", trunc(&text, 120)))?;
        let arr = if let Some(a) = v.as_array() {
            a.clone()
        } else if let Some(a) = v.get("tasks").and_then(|x| x.as_array()) {
            a.clone()
        } else {
            bail!("list response needs array or {{tasks:[…]}}");
        };
        arr.iter().map(task_from_json).collect()
    }

    fn get(&self, id: &str) -> Result<Option<TaskEntry>> {
        let client = self.client()?;
        let path = format!("/tasks/{}", urlencoding_path(id));
        let req = self.apply_auth(client.get(self.url(&path)));
        let resp = req
            .send()
            .map_err(|e| Self::map_transport(e, &self.base_url))?;
        let code = resp.status();
        if code.as_u16() == 404 {
            return Ok(None);
        }
        let text = resp.text().unwrap_or_default();
        if !code.is_success() {
            bail!("remote swarm get failed HTTP {code}: {}", trunc(&text, 200));
        }
        let v: Value = serde_json::from_str(&text)
            .with_context(|| format!("get response not JSON: {}", trunc(&text, 120)))?;
        Ok(Some(task_from_json(&v)?))
    }

    fn backend_name(&self) -> &'static str {
        "http-remote"
    }
}

/// Parse task JSON (compatible with `swarm::task_to_json` + register response).
pub fn task_from_json(v: &Value) -> Result<TaskEntry> {
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("task missing id"))?
        .to_string();
    let prompt = v
        .get("prompt")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let status_s = v
        .get("status")
        .and_then(|x| x.as_str())
        .unwrap_or("pending");
    let error = v.get("error").and_then(|x| x.as_str());
    let status = match status_s {
        "pending" => TaskStatus::Pending,
        "running" => TaskStatus::Running,
        "done" => TaskStatus::Done,
        "cancelled" => TaskStatus::Cancelled,
        "failed" => TaskStatus::Failed(error.unwrap_or("failed").to_string()),
        other => TaskStatus::Failed(format!("unknown status: {other}")),
    };
    let result = v.get("result").and_then(|x| {
        if x.is_null() {
            None
        } else {
            x.as_str().map(|s| s.to_string())
        }
    });
    let model = v.get("model").and_then(|x| {
        if x.is_null() {
            None
        } else {
            x.as_str().map(|s| s.to_string())
        }
    });
    let created_ts = v.get("created_ts").and_then(|x| x.as_i64()).unwrap_or(0);
    let completed_ts =
        v.get("completed_ts")
            .and_then(|x| if x.is_null() { None } else { x.as_i64() });
    Ok(TaskEntry {
        id,
        prompt,
        status,
        result,
        model,
        created_ts,
        completed_ts,
    })
}

fn trunc(s: &str, n: usize) -> String {
    let mut t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        t.push('…');
    }
    t
}

fn urlencoding_path(id: &str) -> String {
    // Task ids are hex-like; still escape path-sensitive chars.
    id.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

/// Choose registry implementation from optional URL.
pub fn select_registry(registry_url: Option<&str>) -> Box<dyn SwarmRegistry> {
    match registry_url.map(str::trim).filter(|s| !s.is_empty()) {
        Some(url) => Box::new(HttpRegistry::new(url)),
        None => Box::new(LocalSqliteRegistry),
    }
}

/// Smoke-call registry methods so clippy sees the trait surface as live API.
pub fn probe_registry(reg: &dyn SwarmRegistry) -> Result<&'static str> {
    let name = reg.backend_name();
    if name == "http-remote" {
        // No network I/O during configure — client is exercised in unit tests.
        return Ok(name);
    }
    // Safe read-only touch of local backend.
    let _ = reg.get("__harness_registry_probe__")?;
    let _ = reg.list(0)?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Path, Query, State};
    use axum::http::StatusCode;
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use serde::Deserialize;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn select_local_by_default() {
        let r = select_registry(None);
        assert_eq!(r.backend_name(), "sqlite-local");
        assert_eq!(probe_registry(r.as_ref()).unwrap(), "sqlite-local");
    }

    #[test]
    fn select_http_when_url_set() {
        let r = select_registry(Some("https://example.test/swarm"));
        assert_eq!(r.backend_name(), "http-remote");
        assert_eq!(probe_registry(r.as_ref()).unwrap(), "http-remote");
    }

    #[test]
    fn task_from_json_roundtrip_shape() {
        let v = json!({
            "id": "swabc",
            "status": "failed",
            "error": "boom",
            "prompt": "p",
            "result": null,
            "model": "m",
            "created_ts": 1,
            "completed_ts": 2
        });
        let t = task_from_json(&v).unwrap();
        assert_eq!(t.id, "swabc");
        assert_eq!(t.model.as_deref(), Some("m"));
        assert!(matches!(t.status, TaskStatus::Failed(ref m) if m == "boom"));
    }

    #[test]
    fn local_register_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("reg.db");
        swarm::with_db_path_override(db, || {
            let r = LocalSqliteRegistry;
            let id = r.register("hi", Some("m1")).unwrap();
            let t = r.get(&id).unwrap().unwrap();
            assert_eq!(t.model.as_deref(), Some("m1"));
            r.update(&id, &TaskStatus::Done, Some("ok")).unwrap();
            assert_eq!(r.list(5).unwrap().len(), 1);
        });
    }

    #[derive(Clone, Default)]
    struct Mem {
        tasks: Arc<Mutex<HashMap<String, TaskEntry>>>,
    }

    #[derive(Deserialize)]
    struct RegBody {
        prompt: String,
        model: Option<String>,
    }

    #[derive(Deserialize)]
    struct UpdBody {
        status: String,
        result: Option<String>,
        error: Option<String>,
    }

    #[derive(Deserialize)]
    struct ListQ {
        limit: Option<usize>,
    }

    async fn post_task(State(mem): State<Mem>, Json(b): Json<RegBody>) -> Json<Value> {
        let id = format!(
            "sw{:x}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let entry = TaskEntry {
            id: id.clone(),
            prompt: b.prompt,
            status: TaskStatus::Pending,
            result: None,
            model: b.model,
            created_ts: 1,
            completed_ts: None,
        };
        mem.tasks.lock().unwrap().insert(id.clone(), entry.clone());
        Json(swarm::task_to_json(&entry))
    }

    async fn list_tasks(State(mem): State<Mem>, Query(q): Query<ListQ>) -> Json<Value> {
        let limit = q.limit.unwrap_or(50);
        let mut items: Vec<_> = mem.tasks.lock().unwrap().values().cloned().collect();
        items.truncate(limit);
        let arr: Vec<Value> = items.iter().map(swarm::task_to_json).collect();
        Json(json!({ "tasks": arr }))
    }

    async fn get_task(
        State(mem): State<Mem>,
        Path(id): Path<String>,
    ) -> Result<Json<Value>, StatusCode> {
        match mem.tasks.lock().unwrap().get(&id) {
            Some(t) => Ok(Json(swarm::task_to_json(t))),
            None => Err(StatusCode::NOT_FOUND),
        }
    }

    async fn put_task(
        State(mem): State<Mem>,
        Path(id): Path<String>,
        Json(b): Json<UpdBody>,
    ) -> Result<StatusCode, StatusCode> {
        let mut g = mem.tasks.lock().unwrap();
        let t = g.get_mut(&id).ok_or(StatusCode::NOT_FOUND)?;
        t.status = match b.status.as_str() {
            "pending" => TaskStatus::Pending,
            "running" => TaskStatus::Running,
            "done" => TaskStatus::Done,
            "cancelled" => TaskStatus::Cancelled,
            "failed" => TaskStatus::Failed(b.error.unwrap_or_else(|| "failed".into())),
            _ => return Err(StatusCode::BAD_REQUEST),
        };
        t.result = b.result;
        Ok(StatusCode::NO_CONTENT)
    }

    fn spawn_test_server() -> (String, thread::JoinHandle<()>) {
        let mem = Mem::default();
        let app = Router::new()
            .route("/tasks", post(post_task).get(list_tasks))
            .route("/tasks/:id", get(get_task).put(put_task))
            .with_state(mem);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let listener =
            rt.block_on(async { tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap() });
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            rt.block_on(async move {
                axum::serve(listener, app).await.unwrap();
            });
        });
        // tiny wait for accept loop
        thread::sleep(std::time::Duration::from_millis(50));
        (format!("http://{addr}"), handle)
    }

    #[test]
    fn http_registry_roundtrip_against_local_server() {
        let (base, _h) = spawn_test_server();
        let reg = HttpRegistry {
            base_url: base,
            token: None,
            timeout_secs: 5,
        };
        assert_eq!(reg.backend_name(), "http-remote");
        let id = reg.register("hello remote", Some("grok")).unwrap();
        assert!(id.starts_with("sw"));
        let got = reg.get(&id).unwrap().unwrap();
        assert_eq!(got.prompt, "hello remote");
        assert_eq!(got.model.as_deref(), Some("grok"));
        reg.update(&id, &TaskStatus::Done, Some("ok")).unwrap();
        let done = reg.get(&id).unwrap().unwrap();
        assert_eq!(done.status, TaskStatus::Done);
        assert_eq!(done.result.as_deref(), Some("ok"));
        let list = reg.list(10).unwrap();
        assert_eq!(list.len(), 1);
        assert!(reg.get("missing").unwrap().is_none());
    }

    #[test]
    fn http_registry_unreachable_is_hard_error() {
        let reg = HttpRegistry {
            base_url: "http://127.0.0.1:1".into(), // nothing listening
            token: None,
            timeout_secs: 1,
        };
        let err = reg.register("x", None).unwrap_err().to_string();
        assert!(
            err.contains("unreachable") || err.contains("Connection"),
            "{err}"
        );
    }

    #[test]
    fn public_api_routes_through_registry_url_override() {
        let (base, _h) = spawn_test_server();
        swarm::with_registry_url_override(Some(&base), || {
            assert_eq!(swarm::registry_backend_name(), "http-remote");
            let id = swarm::register_task_with_model("via public api", Some("m")).unwrap();
            let t = swarm::get_task(&id).unwrap().unwrap();
            assert_eq!(t.prompt, "via public api");
            swarm::update_status(&id, &TaskStatus::Done, Some("done")).unwrap();
            let listed = swarm::list_tasks(10).unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].status, TaskStatus::Done);
        });
        // override cleared
        assert_eq!(swarm::registry_backend_name(), "sqlite-local");
    }
}
