//! Bearer auth middleware and token extraction.

use axum::extract::ConnectInfo;
use axum::extract::State;
use axum::http::{header::AUTHORIZATION, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use std::net::SocketAddr;
use std::sync::Arc;

use super::state::ServerState;

pub(crate) async fn require_auth(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<ServerState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if !crate::rate_limit::allow(addr.ip()) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    let token = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").map(str::trim));
    if !crate::auth_token::verify(token, &state.auth_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(req).await)
}



pub(crate) fn extract_bearer(headers: &axum::http::HeaderMap, body_token: Option<&str>) -> Option<String> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").map(str::trim))
        .map(|s| s.to_string())
        .or_else(|| body_token.map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
}

