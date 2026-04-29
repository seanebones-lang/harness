//! Harness Desktop — Tauri 2 shell around the Harness HTTP server.
//!
//! Features:
//! - System tray with status indicator
//! - Global hotkey Cmd+Shift+H to show/hide
//! - Auto-spawns harness daemon on launch
//! - Wraps the web UI served at http://127.0.0.1:8787

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

mod daemon;

fn toggle_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
        } else {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}

fn main() {
    let hotkey: Shortcut = "CmdOrCtrl+Shift+H"
        .parse()
        .expect("global shortcut must parse");
    let hotkey_handler = hotkey.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if shortcut == &hotkey_handler && event.state() == ShortcutState::Pressed {
                        toggle_main_window(app);
                    }
                })
                .build(),
        )
        .setup(move |app| {
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Harness AI")
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { .. } = event {
                        let app = tray.app_handle();
                        toggle_main_window(app);
                    }
                })
                .build(app)?;

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                daemon::ensure_daemon_running(&app_handle).await;
            });

            app.global_shortcut().register(hotkey)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
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
