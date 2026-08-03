//! Offline benchmark pack runner (W7.3) — no API keys required.
//!
//! Measures local hot paths: swarm DB ops, workspace sandbox, policy helpers,
//! and serde JSON round-trips. Emits human or JSON summary.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::swarm::{self, GcOptions};

/// One timed measurement.
#[derive(Debug, Clone, Serialize)]
pub struct BenchCaseResult {
    /// Case id from pack or built-in name.
    pub name: String,
    /// Wall duration in milliseconds.
    pub duration_ms: f64,
    /// Optional detail string.
    pub detail: String,
    /// True when the case completed without error.
    pub ok: bool,
}

/// Full offline bench report.
#[derive(Debug, Clone, Serialize)]
pub struct BenchReport {
    /// ISO-ish local timestamp string.
    pub started: String,
    /// Pack path used (or "builtin").
    pub pack: String,
    /// Individual cases.
    pub cases: Vec<BenchCaseResult>,
    /// Sum of case durations.
    pub total_ms: f64,
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn run_timed(
    name: &str,
    detail: impl Into<String>,
    f: impl FnOnce() -> Result<()>,
) -> BenchCaseResult {
    let start = Instant::now();
    match f() {
        Ok(()) => BenchCaseResult {
            name: name.into(),
            duration_ms: elapsed_ms(start),
            detail: detail.into(),
            ok: true,
        },
        Err(e) => BenchCaseResult {
            name: name.into(),
            duration_ms: elapsed_ms(start),
            detail: format!("error: {e:#}"),
            ok: false,
        },
    }
}

fn unique_temp_dir(prefix: &str) -> Result<PathBuf> {
    let base = std::env::temp_dir();
    let dir = base.join(format!(
        "harness-bench-{}-{}",
        prefix,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Built-in offline cases (always available).
pub fn run_builtin_cases() -> Result<Vec<BenchCaseResult>> {
    let mut out = Vec::new();

    out.push(run_timed(
        "json_roundtrip",
        "1000 serde_json RPC shapes",
        || {
            for i in 0..1000 {
                let req = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": i,
                    "method": "tools/call",
                    "params": { "name": "read_file", "arguments": { "path": "src/main.rs" } }
                });
                let s = serde_json::to_string(&req)?;
                let _: serde_json::Value = serde_json::from_str(&s)?;
            }
            Ok(())
        },
    ));

    out.push(run_timed(
        "policy_checkpoint",
        "10k tool_requires_checkpoint lookups",
        || {
            use harness_tools::{tool_requires_checkpoint, tool_requires_confirmation};
            let args = serde_json::json!({"action": "status"});
            for _ in 0..10_000 {
                let _ = tool_requires_checkpoint("git", &args);
                let _ = tool_requires_confirmation("write_file", &serde_json::json!({}));
                let _ = tool_requires_checkpoint("read_file", &serde_json::json!({}));
            }
            Ok(())
        },
    ));

    out.push(run_timed(
        "workspace_strict",
        "5k WorkspaceRoot resolves under temp dir",
        || {
            use harness_tools::{SandboxMode, WorkspaceRoot};
            let dir = unique_temp_dir("ws")?;
            let root = WorkspaceRoot::new(dir.clone(), SandboxMode::Strict)?;
            for i in 0..5_000 {
                let rel = format!("f{i}.txt");
                let _ = root.resolve(&rel)?;
            }
            let _ = std::fs::remove_dir_all(&dir);
            Ok(())
        },
    ));

    out.push(run_timed(
        "swarm_register_list_gc",
        "register 20 + list + gc dry-run on temp DB",
        || {
            let dir = unique_temp_dir("swarm")?;
            let db = dir.join("bench-swarm.db");
            swarm::with_db_path_override(db, || {
                for i in 0..20 {
                    swarm::register_task_with_model(&format!("bench-{i}"), Some("bench-model"))?;
                }
                let listed = swarm::list_tasks(50)?;
                if listed.len() < 20 {
                    anyhow::bail!("expected >=20 tasks, got {}", listed.len());
                }
                let _ = swarm::gc(&GcOptions {
                    stale_secs: 3600,
                    keep_terminal: None,
                    older_than_secs: None,
                    dry_run: true,
                })?;
                Ok::<(), anyhow::Error>(())
            })?;
            let _ = std::fs::remove_dir_all(&dir);
            Ok(())
        },
    ));

    out.push(run_timed(
        "swarm_task_json",
        "serialize 500 task_to_json",
        || {
            let dir = unique_temp_dir("swarm-json")?;
            let db = dir.join("bench-swarm-json.db");
            swarm::with_db_path_override(db, || {
                let id = swarm::register_task_with_model("json-bench", Some("m"))?;
                let task = swarm::get_task(&id)?.context("missing task")?;
                for _ in 0..500 {
                    let _ = swarm::task_to_json(&task);
                }
                Ok::<(), anyhow::Error>(())
            })?;
            let _ = std::fs::remove_dir_all(&dir);
            Ok(())
        },
    ));

    Ok(out)
}

/// Load optional pack.json listing extra case names (currently only builtin ids).
fn load_pack_names(pack: &Path) -> Result<Option<Vec<String>>> {
    let pack_json = pack.join("pack.json");
    if !pack_json.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&pack_json)
        .with_context(|| format!("read {}", pack_json.display()))?;
    let v: serde_json::Value = serde_json::from_str(&text)?;
    let names = v
        .get("cases")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(Some(names))
}

/// Run offline bench pack. `pack` may be None for built-in defaults.
pub fn run_offline(pack: Option<&Path>) -> Result<BenchReport> {
    let started = chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S %z")
        .to_string();
    let pack_label = pack
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "builtin".into());

    let mut cases = run_builtin_cases()?;
    if let Some(p) = pack {
        if let Some(filter) = load_pack_names(p)? {
            if !filter.is_empty() {
                cases.retain(|c| filter.iter().any(|n| n == &c.name));
            }
        }
    }

    let total_ms = cases.iter().map(|c| c.duration_ms).sum();
    Ok(BenchReport {
        started,
        pack: pack_label,
        cases,
        total_ms,
    })
}

/// CLI entry: print human or JSON report.
pub fn dispatch_bench(pack: Option<PathBuf>, json: bool) -> Result<()> {
    let pack_path = pack.or_else(|| {
        let default = PathBuf::from("demo/bench_tasks");
        if default.is_dir() {
            Some(default)
        } else {
            None
        }
    });
    let report = run_offline(pack_path.as_deref())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("harness bench (offline) — pack={}", report.pack);
        println!("started: {}", report.started);
        println!();
        for c in &report.cases {
            let mark = if c.ok { "ok" } else { "FAIL" };
            println!(
                "  [{mark}] {:24} {:>8.2} ms  {}",
                c.name, c.duration_ms, c.detail
            );
        }
        println!();
        println!("total: {:.2} ms", report.total_ms);
        if report.cases.iter().any(|c| !c.ok) {
            anyhow::bail!("one or more bench cases failed");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_cases_all_ok() {
        let cases = run_builtin_cases().expect("builtin");
        assert!(cases.len() >= 4);
        for c in &cases {
            assert!(c.ok, "{}: {}", c.name, c.detail);
            assert!(c.duration_ms >= 0.0);
        }
    }

    #[test]
    fn report_json_serializes() {
        let r = run_offline(None).expect("run");
        let s = serde_json::to_string(&r).expect("json");
        assert!(s.contains("cases"));
        assert!(s.contains("total_ms"));
    }
}
