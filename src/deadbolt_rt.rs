//! Process-local Deadbolt handle and out-of-band CLI dispatch.
//!
//! Deadbolt commands are not model tools. `kill` is bound to one `agent_id`.

use std::sync::OnceLock;

use anyhow::Result;
use harness_deadbolt::{Deadbolt, DeadboltConfig, PolicyPatch};

use crate::cli::args::DeadboltAction;
use crate::config::Config;

static HANDLE: OnceLock<Deadbolt> = OnceLock::new();
static AGENT: OnceLock<String> = OnceLock::new();

/// Stable id for this process. Not a vendor API key.
pub fn process_agent_id() -> String {
    AGENT
        .get_or_init(|| {
            if let Ok(raw) = std::env::var("HARNESS_AGENT_ID") {
                let v = raw.trim();
                if is_agent_token(v) {
                    return v.to_string();
                }
            }
            let n = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            format!("h-{}-{n:x}", std::process::id())
        })
        .clone()
}

/// Shared gate for this process. CLI commands in other processes open the same files.
pub fn handle(cfg: &DeadboltConfig) -> Deadbolt {
    HANDLE.get_or_init(|| Deadbolt::open(cfg)).clone()
}

/// `harness deadbolt …`. Drill uses a temp store and does not need API keys.
pub fn dispatch(action: &DeadboltAction, cfg: &Config) -> Result<()> {
    if matches!(action, DeadboltAction::Drill) {
        Deadbolt::drill()?;
        println!("deadbolt drill ok");
        return Ok(());
    }
    let db = Deadbolt::open(&cfg.deadbolt);
    match action {
        DeadboltAction::Status { agent } => {
            let rows = db.status(agent.as_deref())?;
            if rows.is_empty() {
                println!("deadbolt status none");
            }
            for row in rows {
                println!(
                    "agent={} state={} expires_at={} parent={} clips={} swarm_task={}",
                    row.agent_id,
                    row.state,
                    row.expires_at,
                    row.parent_id.as_deref().unwrap_or("-"),
                    if row.clips.is_empty() {
                        "-".to_string()
                    } else {
                        row.clips.join(",")
                    },
                    row.swarm_task_id.as_deref().unwrap_or("-")
                );
            }
        }
        DeadboltAction::Pause { agent } => {
            db.pause(agent)?;
            println!("deadbolt paused {agent}");
        }
        DeadboltAction::Clip { tool, agent } => {
            db.clip(agent, tool)?;
            println!("deadbolt clipped {agent} {tool}");
        }
        DeadboltAction::Resume { agent } => {
            db.resume(agent)?;
            println!("deadbolt resumed {agent}");
        }
        DeadboltAction::Kill { agent } => {
            let report = db.kill(agent)?;
            for id in &report.swarm_task_ids {
                let _ = crate::swarm::cancel_task(id);
            }
            println!(
                "deadbolt killed {agent} revoked={} swarm={}",
                report.revoked.len(),
                report.swarm_task_ids.len()
            );
        }
        DeadboltAction::Serve { bind } => {
            let path = bind.clone().unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join(".harness")
                    .join("deadbolt.sock")
            });
            if harness_deadbolt::bind_refused(&path) {
                anyhow::bail!("deadbolt:bind_refused");
            }
            println!("deadbolt serve {}", path.display());
            harness_deadbolt::serve(&cfg.deadbolt, &path)?;
        }
        DeadboltAction::Export {
            agent,
            out,
            json,
            children,
        } => {
            let rows = db.export(agent, *children)?;
            let text = harness_deadbolt::format_export(&rows, *json);
            if let Some(path) = out {
                std::fs::write(path, text)?;
            } else {
                print!("{text}");
            }
        }
        DeadboltAction::Policy {
            agent,
            tools,
            dest,
            spend_cap,
            irreversible,
        } => {
            db.set_policy(
                agent,
                PolicyPatch {
                    tools_allow: split_list(tools.as_deref()),
                    dest_allow: split_list(dest.as_deref()),
                    spend_cap_usd: *spend_cap,
                    irreversible: split_list(irreversible.as_deref()),
                },
            )?;
            println!("deadbolt policy {agent}");
        }
        DeadboltAction::Approve { agent, tool } => {
            db.approve(agent, tool)?;
            println!("deadbolt approve {agent} {tool}");
        }
        DeadboltAction::Incident {
            agent,
            out,
            json,
            children: _,
        } => {
            let raw = db.incident(agent)?;
            let value: serde_json::Value = serde_json::from_str(&raw)?;
            let text = if *json {
                format!("{value}\n")
            } else {
                incident_tokens(&value)
            };
            if let Some(path) = out {
                std::fs::write(path, text)?;
            } else {
                print!("{text}");
            }
        }
        DeadboltAction::Drill => unreachable!("drill handled above"),
    }
    Ok(())
}

use harness_tools::ToolExecutor;
use std::sync::Arc;

/// Attach the process gate when `[deadbolt] enabled`. No-op when disabled.
pub fn bind(exec: ToolExecutor, cfg: &DeadboltConfig, agent_id: &str) -> ToolExecutor {
    if !cfg.enabled {
        return exec;
    }
    let gate = handle(cfg);
    if !gate.should_attach() {
        return exec;
    }
    let _ = gate.ensure_agent(agent_id);
    exec.with_deadbolt(Arc::new(gate), agent_id.to_string())
}

/// Register a swarm worker under `parent`.
///
/// `None` means do not spawn: Deadbolt is off, or the parent is already dead
/// (the swarm task is cancelled). `Some` is the child lease id to bind.
pub fn bind_swarm_child(cfg: &DeadboltConfig, parent: &str, swarm_task_id: &str) -> Option<String> {
    if !cfg.enabled {
        return None;
    }
    let gate = handle(cfg);
    if !gate.should_attach() {
        return None;
    }
    let _ = gate.ensure_agent(parent);
    let child = child_for(parent, swarm_task_id);
    match gate.register_child(parent, &child, Some(swarm_task_id)) {
        Ok(true) => Some(child),
        Ok(false) => {
            let _ = crate::swarm::cancel_task(swarm_task_id);
            None
        }
        Err(_) if cfg.fail_closed => Some(child),
        Err(_) => None,
    }
}

fn child_for(parent: &str, swarm_task_id: &str) -> String {
    let raw = format!("{parent}-{swarm_task_id}");
    if raw.len() <= 128 && is_agent_token(&raw) {
        raw
    } else {
        format!("h-sw-{swarm_task_id}")
    }
}

fn split_list(raw: Option<&str>) -> Option<Vec<String>> {
    raw.map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect()
    })
}

fn incident_tokens(value: &serde_json::Value) -> String {
    let agent = value.get("agent").and_then(|v| v.as_str()).unwrap_or("-");
    let killed = value
        .get("killed_at")
        .and_then(|v| v.as_i64())
        .map(|n| n.to_string())
        .unwrap_or_else(|| "-".into());
    let children = value
        .get("children")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let codes = value
        .get("decisions")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("code").and_then(|c| c.as_str()))
                .collect::<Vec<_>>()
                .join(",")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "-".into());
    format!("agent={agent} killed_at={killed} children={children} codes={codes}\n")
}

fn is_agent_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '/' | '-'))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{command_needs_agent_runtime, Cli};

    #[test]
    fn deadbolt_drill_is_not_agent_runtime() {
        let cli = Cli::try_parse_from(["harness", "deadbolt", "drill"]).expect("parse");
        assert!(!command_needs_agent_runtime(&cli));
    }

    #[test]
    fn kill_requires_agent_and_rejects_fleet_halt() {
        assert!(Cli::try_parse_from(["harness", "deadbolt", "kill"]).is_err());
        assert!(
            Cli::try_parse_from(["harness", "deadbolt", "kill", "--all", "--agent", "a"]).is_err()
        );
        let cli =
            Cli::try_parse_from(["harness", "deadbolt", "kill", "--agent", "h-1"]).expect("parse");
        assert!(!command_needs_agent_runtime(&cli));
    }

    #[test]
    fn policy_approve_incident_are_not_agent_runtime() {
        for argv in [
            vec![
                "harness", "deadbolt", "policy", "--agent", "h-1", "--tools", "shell",
            ],
            vec![
                "harness", "deadbolt", "approve", "--agent", "h-1", "--tool", "shell",
            ],
            vec!["harness", "deadbolt", "incident", "--agent", "h-1"],
        ] {
            let cli = Cli::try_parse_from(argv).expect("parse");
            assert!(!command_needs_agent_runtime(&cli));
        }
    }
}
