//! HTTP server management for the Tauri desktop shell.

use tauri::AppHandle;

const SERVE_ADDR: &str = "127.0.0.1:8787";

fn health_url() -> String {
    format!("http://{SERVE_ADDR}/api/health")
}

/// Ensure `harness serve` is running for the embedded web UI.
pub async fn ensure_daemon_running(_app: &AppHandle) {
    if serve_health_ok().await {
        eprintln!("[harness-desktop] harness serve already running at {SERVE_ADDR}");
        return;
    }

    eprintln!("[harness-desktop] starting harness serve at {SERVE_ADDR}…");
    let _ = start_serve_inner().await;
}

async fn serve_health_ok() -> bool {
    reqwest::get(health_url()).await.map(|r| r.status().is_success()).unwrap_or(false)
}

async fn start_serve_inner() -> Result<String, String> {
    tokio::process::Command::new("harness")
        .args(["serve", "--addr", SERVE_ADDR])
        .spawn()
        .map(|_| format!("Server started at http://{SERVE_ADDR}"))
        .map_err(|e| format!("Failed to start harness serve: {e}"))
}

/// Start the harness HTTP server.
#[tauri::command]
pub async fn start_daemon(_app: AppHandle) -> Result<String, String> {
    start_serve_inner().await
}

/// Get server status.
#[tauri::command]
pub async fn daemon_status() -> Result<String, String> {
    if serve_health_ok().await {
        Ok("running".to_string())
    } else {
        Ok("stopped".to_string())
    }
}
