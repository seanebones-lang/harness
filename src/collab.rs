#![allow(dead_code)]
//! Collaborative WebSocket sessions: multiple users sharing a harness session.
//!
//! Adds a `/ws/session/:id` WebSocket route to the server.
//! All connected clients see the same chat stream (agent events broadcast).
//! Events: UserJoined, UserLeft, UserTyping, AgentEvent (rebroadcast).
//!
//! Enable with `[collab]` config block.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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

/// Get or create a session in the registry.
pub fn get_or_create_session(registry: &CollabRegistry, session_id: &str) -> () {
    let mut reg = registry.lock().unwrap();
    reg.entry(session_id.to_string())
        .or_insert_with(|| CollabSession::new(session_id));
}

/// Broadcast an event to all users in a session.
pub fn broadcast_to_session(registry: &CollabRegistry, session_id: &str, event: CollabEvent) {
    let reg = registry.lock().unwrap();
    if let Some(session) = reg.get(session_id) {
        session.broadcast(event);
    }
}

/// List active sessions.
pub fn list_sessions(registry: &CollabRegistry) -> Vec<(String, usize)> {
    let reg = registry.lock().unwrap();
    reg.values()
        .map(|s| (s.session_id.clone(), s.users.len()))
        .collect()
}
