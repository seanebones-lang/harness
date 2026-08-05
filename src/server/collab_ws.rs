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
    let user_id = query
        .user_id
        .filter(|s| !s.is_empty())
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
            let preview = if result.len() > 120 {
                format!("{}…", &result[..120])
            } else {
                result.clone()
            };
            Some(CollabEvent::AgentToolResult {
                name: name.clone(),
                preview,
            })
        }
        AgentEvent::Done => Some(CollabEvent::AgentDone),
        _ => None,
    }
}
