//! Daemon management for the Tauri desktop shell.

use tauri::AppHandle;

/// Check if the harness daemon is running and start it if not.
pub async fn ensure_daemon_running(_app: &AppHandle) {
    let sock = dirs_next::home_dir()
        .unwrap_or_default()
        .join(".harness/daemon.sock");

    if sock.exists() {
        eprintln!("[harness-desktop] daemon socket present — skipping spawn");
        return;
    }

    eprintln!("[harness-desktop] starting harness daemon…");
    let _ = start_daemon_inner().await;
}

async fn start_daemon_inner() -> Result<String, String> {
    tokio::process::Command::new("harness")
        .arg("daemon")
        .spawn()
        .map(|_| "Daemon started".to_string())
        .map_err(|e| format!("Failed to start daemon: {e}"))
}

/// Start the harness daemon process.
#[tauri::command]
pub async fn start_daemon(_app: AppHandle) -> Result<String, String> {
    start_daemon_inner().await
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
