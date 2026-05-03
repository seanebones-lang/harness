//! Local [MLX LM](https://github.com/ml-explore/mlx-examples) HTTP server provider.
//!
//! Wraps `mlx_lm.server`'s OpenAI-compatible `/v1/chat/completions` endpoint via
//! [`harness_provider_openai::OpenAIProvider`].

use harness_provider_core::ArcProvider;
use harness_provider_openai::{OpenAIConfig, OpenAIProvider};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

/// Default chat model when using Apple's MLX examples / common setups.
pub const DEFAULT_MODEL: &str = "mlx-community/Qwen3-Coder-30B";

/// Default base URL (`mlx_lm.server` uses port 8080 by default).
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8080/v1";

fn normalize_base_url(url: &str) -> String {
    let u = url.trim_end_matches('/');
    if u.ends_with("/v1") {
        u.to_string()
    } else {
        format!("{u}/v1")
    }
}

/// Build an [`ArcProvider`] pointed at a running `mlx_lm.server` (or compatible) endpoint.
pub fn build_arc(model: Option<String>, base_url: Option<String>) -> anyhow::Result<ArcProvider> {
    let base = normalize_base_url(
        base_url
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_BASE_URL),
    );
    let model = model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let cfg = OpenAIConfig::new("mlx-local")
        .with_model(model)
        .with_base_url(base);
    Ok(Arc::new(OpenAIProvider::new(cfg)?))
}

/// `true` when `mlx_lm.server` is on `PATH` or TCP `127.0.0.1:8080` accepts a connection.
pub fn mlx_runtime_available() -> bool {
    mlx_server_on_path() || mlx_port_open(8080)
}

fn mlx_server_on_path() -> bool {
    which::which("mlx_lm.server").is_ok()
}

fn mlx_port_open(port: u16) -> bool {
    let addr = format!("127.0.0.1:{port}");
    let Ok(mut it) = addr.to_socket_addrs() else {
        return false;
    };
    let Some(sock) = it.next() else {
        return false;
    };
    TcpStream::connect_timeout(&sock, Duration::from_millis(250)).is_ok()
}
