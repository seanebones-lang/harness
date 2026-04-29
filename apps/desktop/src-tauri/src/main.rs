//! Harness Desktop — Tauri 2 shell around the Harness HTTP server.
//!
//! Features:
//! - System tray with status indicator
//! - Global hotkey Cmd+Shift+H to show/hide
//! - Auto-spawns harness daemon on launch
//! - Native macOS notifications (forwarded from daemon events)
//! - Wraps the web UI served at http://127.0.0.1:8787

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{
    tray::{TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent, AppHandle,
};

mod daemon;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            // Build the tray icon
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Harness AI")
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { .. } = event {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            if win.is_visible().unwrap_or(false) {
                                let _ = win.hide();
                            } else {
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // Auto-spawn the harness daemon if not already running
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                daemon::ensure_daemon_running(&app_handle).await;
            });

            // Register global hotkey: Cmd+Shift+H
            use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
            let shortcut: Shortcut = "CmdOrCtrl+Shift+H".parse().unwrap();
            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(move |app, s, event| {
                        if s == &shortcut && event.state() == ShortcutState::Pressed {
                            if let Some(win) = app.get_webview_window("main") {
                                if win.is_visible().unwrap_or(false) {
                                    let _ = win.hide();
                                } else {
                                    let _ = win.show();
                                    let _ = win.set_focus();
                                }
                            }
                        }
                    })
                    .build()
            )?;
            app.global_shortcut().register("CmdOrCtrl+Shift+H")?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Hide instead of close (keep daemon alive)
                window.hide().ok();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            daemon::start_daemon,
            daemon::daemon_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Harness Desktop");
}
