//! Daemon management for the Tauri desktop shell.

use tauri::AppHandle;

/// Check if the harness daemon is running and start it if not.
pub async fn ensure_daemon_running(app: &AppHandle) {
    let sock = dirs_next::home_dir()
        .unwrap_or_default()
        .join(".harness/daemon.sock");

    if sock.exists() {
        tracing_log::log::info!("harness daemon already running");
        return;
    }

    tracing_log::log::info!("starting harness daemon…");
    let _ = start_daemon(app.clone()).await;
}

/// Start the harness daemon process.
#[tauri::command]
pub async fn start_daemon(_app: AppHandle) -> Result<String, String> {
    let output = tokio::process::Command::new("harness")
        .arg("daemon")
        .spawn();

    match output {
        Ok(_child) => Ok("Daemon started".to_string()),
        Err(e) => Err(format!("Failed to start daemon: {e}")),
    }
}

/// Get daemon status.
#[tauri::command]
pub async fn daemon_status() -> Result<String, String> {
    let sock = dirs_next::home_dir()
        .unwrap_or_default()
        .join(".harness/daemon.sock");

    if sock.exists() {
        Ok("running".to_string())
    } else {
        Ok("stopped".to_string())
    }
}
