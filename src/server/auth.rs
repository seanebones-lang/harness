//! Bearer auth middleware and token extraction.

use axum::extract::ConnectInfo;
use axum::extract::State;
use axum::http::{header::AUTHORIZATION, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use std::net::SocketAddr;
use std::sync::Arc;

use super::state::ServerState;

/// The loopback bootstrap exposes a bearer token to the bundled UI. Reject
/// foreign Host values before that bootstrap can be reached via DNS rebinding,
/// and reject cross-origin browser requests on every route (including WebSocket).
pub(crate) async fn browser_boundary(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<axum::http::uri::Authority>().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if addr.ip().is_loopback() && !is_loopback_host(host.host()) {
        return Err(StatusCode::FORBIDDEN);
    }
    if let Some(origin) = req.headers().get(axum::http::header::ORIGIN) {
        let origin = origin
            .to_str()
            .ok()
            .and_then(|value| value.parse::<axum::http::Uri>().ok())
            .ok_or(StatusCode::FORBIDDEN)?;
        if !matches!(origin.scheme_str(), Some("http" | "https"))
            || origin.authority() != Some(&host)
        {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    let mut response = next.run(req).await;
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        axum::http::HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        "x-frame-options",
        axum::http::HeaderValue::from_static("DENY"),
    );
    Ok(response)
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

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

pub(crate) fn extract_bearer(
    headers: &axum::http::HeaderMap,
    body_token: Option<&str>,
) -> Option<String> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").map(str::trim))
        .map(|s| s.to_string())
        .or_else(|| body_token.map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header::AUTHORIZATION, HeaderMap, HeaderValue};

    #[test]
    fn extract_bearer_from_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer secret-token"),
        );
        assert_eq!(
            extract_bearer(&headers, None).as_deref(),
            Some("secret-token")
        );
    }

    #[test]
    fn extract_bearer_falls_back_to_body() {
        let headers = HeaderMap::new();
        assert_eq!(
            extract_bearer(&headers, Some("  bodytok  ")).as_deref(),
            Some("bodytok")
        );
        assert_eq!(extract_bearer(&headers, Some("   ")), None);
        assert_eq!(extract_bearer(&headers, None), None);
    }

    #[test]
    fn extract_bearer_ignores_non_bearer_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic abc"));
        assert_eq!(
            extract_bearer(&headers, Some("fallback")).as_deref(),
            Some("fallback")
        );
    }

    #[test]
    fn extract_bearer_prefers_header_over_body_and_trims() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer   header-tok  "),
        );
        assert_eq!(
            extract_bearer(&headers, Some("body-tok")).as_deref(),
            Some("header-tok")
        );
    }

    #[test]
    fn extract_bearer_rejects_empty_bearer_value() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer "));
        // Empty Bearer value becomes Some("") before filter; body is not consulted.
        assert_eq!(extract_bearer(&headers, None), None);
        assert_eq!(extract_bearer(&headers, Some("body-tok")), None);
    }
}
