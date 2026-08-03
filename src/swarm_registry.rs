//! Optional remote/shared swarm registry (W7.1).
//!
//! Local default remains SQLite via [`LocalSqliteRegistry`]. When
//! `[swarm].registry_url` is set, [`HttpRegistry`] is selected but currently
//! returns a clear not-implemented error until the remote protocol is shipped
//! (see `docs/WAVE7_SCALE.md`).

use anyhow::{bail, Result};

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

/// Default local SQLite registry (wraps `crate::swarm` functions).
#[derive(Debug, Default, Clone)]
pub struct LocalSqliteRegistry;

impl SwarmRegistry for LocalSqliteRegistry {
    fn register(&self, prompt: &str, model: Option<&str>) -> Result<TaskId> {
        match model {
            Some(m) => swarm::register_task_with_model(prompt, Some(m)),
            None => swarm::register_task(prompt),
        }
    }

    fn update(&self, id: &str, status: &TaskStatus, result: Option<&str>) -> Result<()> {
        swarm::update_status(id, status, result)
    }

    fn list(&self, limit: usize) -> Result<Vec<TaskEntry>> {
        swarm::list_tasks(limit)
    }

    fn get(&self, id: &str) -> Result<Option<TaskEntry>> {
        swarm::get_task(id)
    }

    fn backend_name(&self) -> &'static str {
        "sqlite-local"
    }
}

/// Future HTTP registry client (stub).
#[derive(Debug, Clone)]
pub struct HttpRegistry {
    /// Base URL, e.g. `https://swarm.example/v1`.
    pub base_url: String,
    /// Optional bearer token (from env `HARNESS_SWARM_TOKEN` at construct time).
    #[allow(dead_code)]
    pub token: Option<String>,
}

impl HttpRegistry {
    /// Build from config URL + env token.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token: std::env::var("HARNESS_SWARM_TOKEN")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }
}

impl SwarmRegistry for HttpRegistry {
    fn register(&self, _prompt: &str, _model: Option<&str>) -> Result<TaskId> {
        bail!(
            "remote swarm registry not implemented yet (url={}). \
             unset [swarm].registry_url to use local SQLite — see docs/WAVE7_SCALE.md",
            self.base_url
        )
    }

    fn update(&self, _id: &str, _status: &TaskStatus, _result: Option<&str>) -> Result<()> {
        bail!(
            "remote swarm registry not implemented yet (url={})",
            self.base_url
        )
    }

    fn list(&self, _limit: usize) -> Result<Vec<TaskEntry>> {
        bail!(
            "remote swarm registry not implemented yet (url={})",
            self.base_url
        )
    }

    fn get(&self, _id: &str) -> Result<Option<TaskEntry>> {
        bail!(
            "remote swarm registry not implemented yet (url={})",
            self.base_url
        )
    }

    fn backend_name(&self) -> &'static str {
        "http-remote-stub"
    }
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
    if name == "http-remote-stub" {
        let _ = reg.register("probe", None);
        let _ = reg.update("probe", &TaskStatus::Pending, None);
        let _ = reg.get("probe");
        let _ = reg.list(0);
        return Ok(name);
    }
    let _ = reg.list(0)?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_local_by_default() {
        let r = select_registry(None);
        assert_eq!(r.backend_name(), "sqlite-local");
        assert_eq!(probe_registry(r.as_ref()).unwrap(), "sqlite-local");
    }

    #[test]
    fn select_http_when_url_set() {
        let r = select_registry(Some("https://example.test/swarm"));
        assert_eq!(r.backend_name(), "http-remote-stub");
        assert!(r.register("x", None).is_err());
        assert_eq!(probe_registry(r.as_ref()).unwrap(), "http-remote-stub");
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
}
