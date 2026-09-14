//! Bounded readiness and startup for the desktop shell's local HTTP server.
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const SERVE_ADDR: &str = "127.0.0.1:8787";

fn harness_executable() -> PathBuf {
    // Finder-launched applications often omit ~/.local/bin from PATH.
    if let Some(home) = dirs_next::home_dir() {
        let filename = if cfg!(windows) {
            "harness.exe"
        } else {
            "harness"
        };
        for directory in [".local/bin", ".cargo/bin"] {
            let candidate = home.join(directory).join(filename);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from("harness")
}

async fn serve_health_ok() -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return false;
    };
    let Ok(response) = client
        .get(format!("http://{SERVE_ADDR}/api/health"))
        .send()
        .await
    else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    let Ok(value) = response.json::<serde_json::Value>().await else {
        return false;
    };
    value["status"] == "ok" && value["provider_model"].is_string()
}

async fn start_serve_inner() -> Result<String, String> {
    if serve_health_ok().await {
        return Ok(format!("Server ready at http://{SERVE_ADDR}"));
    }
    let mut child = tokio::process::Command::new(harness_executable())
        .args(["serve", "--addr", SERVE_ADDR])
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Cannot start Harness. Install the CLI and run harness setup: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!(
                "Harness exited before readiness ({status}); run harness doctor and harness setup"
            ));
        }
        if serve_health_ok().await {
            return Ok(format!("Server ready at http://{SERVE_ADDR}"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = child.kill().await;
    Err("Harness did not become ready within 15 seconds; run harness doctor".into())
}

/// Reload after readiness so a cold start cannot leave the webview on a failed load.
pub async fn ensure_daemon_running(app: &AppHandle) {
    match start_serve_inner().await {
        Ok(_) => {
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(url) = format!("http://{SERVE_ADDR}").parse() {
                    let _ = window.navigate(url);
                }
            }
        }
        Err(error) => {
            eprintln!("[harness-desktop] {error}");
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title("Harness server unavailable — run harness doctor / setup");
                let page = "data:text/html,<html><head><title>Harness setup required</title></head><body style='font:18px system-ui;padding:48px;color:white;background:rgb(18,22,30)'><h1>Harness could not start</h1><p>Install the Harness CLI, then run <code>harness setup</code> and <code>harness doctor</code> in a terminal.</p><p>After fixing the reported issue, reopen this app.</p></body></html>";
                if let Ok(url) = page.parse() {
                    let _ = window.navigate(url);
                }
            }
        }
    }
}

#[tauri::command]
pub async fn start_daemon(_app: AppHandle) -> Result<String, String> {
    start_serve_inner().await
}

#[tauri::command]
pub async fn daemon_status() -> Result<String, String> {
    Ok(if serve_health_ok().await {
        "running"
    } else {
        "stopped"
    }
    .to_string())
}
