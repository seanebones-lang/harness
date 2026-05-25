#![allow(dead_code)]
//! Collaborative multi-user sessions over WebSocket when `[collab].enabled = true`.

use anyhow::Result;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CollabConfig {
    pub enabled: bool,
    /// Maximum concurrent users per session.
    #[serde(default = "default_max_users")]
    pub max_users: usize,
}

fn default_max_users() -> usize {
    10
}

// ── Event types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CollabEvent {
    UserJoined {
        user_id: String,
    },
    UserLeft {
        user_id: String,
    },
    UserTyping {
        user_id: String,
        partial: String,
    },
    AgentTextChunk {
        content: String,
    },
    AgentToolStart {
        name: String,
    },
    AgentToolResult {
        name: String,
        preview: String,
    },
    AgentDone,
    SessionInfo {
        session_id: String,
        user_count: usize,
    },
}

// ── Session registry ──────────────────────────────────────────────────────────

pub struct CollabSession {
    pub session_id: String,
    pub tx: broadcast::Sender<CollabEvent>,
    pub users: Vec<String>,
}

pub type CollabRegistry = Arc<Mutex<HashMap<String, CollabSession>>>;

pub fn new_registry() -> CollabRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

impl CollabSession {
    pub fn new(session_id: &str) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            session_id: session_id.to_string(),
            tx,
            users: Vec::new(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CollabEvent> {
        self.tx.subscribe()
    }

    pub fn broadcast(&self, event: CollabEvent) {
        let _ = self.tx.send(event);
    }

    pub fn user_joined(&mut self, user_id: &str) {
        self.users.push(user_id.to_string());
        self.broadcast(CollabEvent::UserJoined {
            user_id: user_id.to_string(),
        });
    }

    pub fn user_left(&mut self, user_id: &str) {
        self.users.retain(|u| u != user_id);
        self.broadcast(CollabEvent::UserLeft {
            user_id: user_id.to_string(),
        });
    }
}

/// Join a session and return a broadcast receiver for live events.
pub fn join_session(
    registry: &CollabRegistry,
    session_id: &str,
    user_id: &str,
    max_users: usize,
) -> Result<broadcast::Receiver<CollabEvent>> {
    let mut reg = registry.lock();
    let session = reg
        .entry(session_id.to_string())
        .or_insert_with(|| CollabSession::new(session_id));
    if session.users.len() >= max_users && !session.users.iter().any(|u| u == user_id) {
        anyhow::bail!("collab session full (max {max_users} users)");
    }
    session.user_joined(user_id);
    Ok(session.subscribe())
}

/// Leave a session and notify peers.
pub fn leave_session(registry: &CollabRegistry, session_id: &str, user_id: &str) {
    let mut reg = registry.lock();
    if let Some(session) = reg.get_mut(session_id) {
        session.user_left(user_id);
    }
}

/// Get or create a session in the registry.
pub fn get_or_create_session(registry: &CollabRegistry, session_id: &str) {
    let mut reg = registry.lock();
    reg.entry(session_id.to_string())
        .or_insert_with(|| CollabSession::new(session_id));
}

/// Broadcast an event to all users in a session.
pub fn broadcast_to_session(registry: &CollabRegistry, session_id: &str, event: CollabEvent) {
    let reg = registry.lock();
    if let Some(session) = reg.get(session_id) {
        session.broadcast(event);
    }
}

/// List active sessions.
pub fn list_sessions(registry: &CollabRegistry) -> Vec<(String, usize)> {
    let reg = registry.lock();
    reg.values()
        .map(|s| (s.session_id.clone(), s.users.len()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_session_enforces_max_users() {
        let reg = new_registry();
        for i in 0..2 {
            join_session(&reg, "sess1", &format!("user{i}"), 2).expect("join");
        }
        let err = join_session(&reg, "sess1", "user3", 2).unwrap_err();
        assert!(err.to_string().contains("full"));
    }

    #[test]
    fn join_session_allows_rejoin_for_existing_user() {
        let reg = new_registry();
        join_session(&reg, "sess1", "alice", 1).expect("first join");
        join_session(&reg, "sess1", "alice", 1).expect("rejoin");
    }

    #[test]
    fn list_sessions_reports_user_counts() {
        let reg = new_registry();
        join_session(&reg, "a", "u1", 10).unwrap();
        join_session(&reg, "a", "u2", 10).unwrap();
        join_session(&reg, "b", "u3", 10).unwrap();
        let mut sessions = list_sessions(&reg);
        sessions.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(sessions, vec![("a".into(), 2), ("b".into(), 1)]);
    }
}
