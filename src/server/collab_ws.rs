//! Collaborative WebSocket endpoint.

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use std::sync::Arc;

use crate::collab::{self, CollabEvent, CollabRegistry};
use crate::events::AgentEvent;

use super::state::ServerState;

#[derive(Deserialize)]
pub(crate) struct CollabWsQuery {
    token: Option<String>,
    user_id: Option<String>,
}

pub(crate) async fn collab_ws(
    ws: WebSocketUpgrade,
    Path(session_id): Path<String>,
    Query(query): Query<CollabWsQuery>,
    State(state): State<Arc<ServerState>>,
) -> Result<impl IntoResponse, StatusCode> {
    let Some(registry) = state.collab.clone() else {
        return Err(StatusCode::NOT_FOUND);
    };
    if !crate::auth_token::verify(query.token.as_deref(), &state.auth_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let max_users = {
        let g = state.inner.read().await;
        g.config.collab.max_users
    };
    let user_id = collab_user_id_or_none(query.user_id.as_deref())
        .unwrap_or_else(|| format!("u{:08x}", uuid::Uuid::new_v4().as_u128() as u32));
    Ok(ws.on_upgrade(move |socket| {
        handle_collab_ws(socket, session_id, user_id, registry, max_users)
    }))
}

async fn handle_collab_ws(
    mut socket: WebSocket,
    session_id: String,
    user_id: String,
    registry: CollabRegistry,
    max_users: usize,
) {
    let mut rx = match collab::join_session(&registry, &session_id, &user_id, max_users) {
        Ok(rx) => rx,
        Err(e) => {
            let _ = socket.send(WsMessage::Text(format!("error: {e}"))).await;
            return;
        }
    };
    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(WsMessage::Text(text))) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            if v.get("type").and_then(|t| t.as_str()) == Some("typing") {
                                let partial = v
                                    .get("partial")
                                    .and_then(|p| p.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                collab::broadcast_to_session(
                                    &registry,
                                    &session_id,
                                    CollabEvent::UserTyping {
                                        user_id: user_id.clone(),
                                        partial,
                                    },
                                );
                            }
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    _ => {}
                }
            }
            ev = rx.recv() => {
                match ev {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            if socket.send(WsMessage::Text(json)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
    collab::leave_session(&registry, &session_id, &user_id);
}

pub(crate) fn agent_event_to_collab(event: &AgentEvent) -> Option<CollabEvent> {
    match event {
        AgentEvent::TextChunk(content) => Some(CollabEvent::AgentTextChunk {
            content: content.clone(),
        }),
        AgentEvent::ToolStart { name, .. } => {
            Some(CollabEvent::AgentToolStart { name: name.clone() })
        }
        AgentEvent::ToolResult { name, result, .. } => {
            let preview = tool_result_preview(result);
            Some(CollabEvent::AgentToolResult {
                name: name.clone(),
                preview,
            })
        }
        AgentEvent::Done => Some(CollabEvent::AgentDone),
        _ => None,
    }
}

/// Truncate tool results for collab WS payloads (ellipsis after 120 chars).
pub(crate) fn tool_result_preview(result: &str) -> String {
    if result.len() > 120 {
        format!("{}…", &result[..120])
    } else {
        result.to_string()
    }
}

/// Prefer explicit non-empty `user_id`; otherwise caller generates a random id.
pub(crate) fn collab_user_id_or_none(user_id: Option<&str>) -> Option<String> {
    user_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collab::CollabEvent;
    use crate::events::AgentEvent;

    #[test]
    fn tool_result_preview_truncates_at_120() {
        let short = "hello";
        assert_eq!(tool_result_preview(short), "hello");
        let long = "x".repeat(121);
        let p = tool_result_preview(&long);
        assert!(p.ends_with('…'));
        assert_eq!(p.chars().count(), 121); // 120 + ellipsis char
        assert_eq!(&p[..120], &"x".repeat(120));
    }

    #[test]
    fn collab_user_id_filters_empty() {
        assert_eq!(collab_user_id_or_none(Some("alice")), Some("alice".into()));
        assert_eq!(collab_user_id_or_none(Some("  bob  ")), Some("bob".into()));
        assert_eq!(collab_user_id_or_none(Some("")), None);
        assert_eq!(collab_user_id_or_none(Some("   ")), None);
        assert_eq!(collab_user_id_or_none(None), None);
    }

    #[test]
    fn agent_event_to_collab_maps_known_variants() {
        match agent_event_to_collab(&AgentEvent::TextChunk("hi".into())) {
            Some(CollabEvent::AgentTextChunk { content }) => assert_eq!(content, "hi"),
            other => panic!("{other:?}"),
        }
        match agent_event_to_collab(&AgentEvent::ToolStart {
            name: "read_file".into(),
            id: "1".into(),
        }) {
            Some(CollabEvent::AgentToolStart { name }) => assert_eq!(name, "read_file"),
            other => panic!("{other:?}"),
        }
        match agent_event_to_collab(&AgentEvent::ToolResult {
            name: "shell".into(),
            id: "2".into(),
            result: "ok".into(),
        }) {
            Some(CollabEvent::AgentToolResult { name, preview }) => {
                assert_eq!(name, "shell");
                assert_eq!(preview, "ok");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            agent_event_to_collab(&AgentEvent::Done),
            Some(CollabEvent::AgentDone)
        ));
    }

    #[test]
    fn agent_event_to_collab_drops_noise() {
        assert!(agent_event_to_collab(&AgentEvent::Error("x".into())).is_none());
        assert!(agent_event_to_collab(&AgentEvent::TokenUsage {
            input: 1,
            output: 2
        })
        .is_none());
        assert!(agent_event_to_collab(&AgentEvent::MemoryRecall { count: 3 }).is_none());
        assert!(agent_event_to_collab(&AgentEvent::ContextCompacted {
            messages_before: 10,
            messages_after: 4
        })
        .is_none());
    }
}
