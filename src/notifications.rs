//! Desktop notification helpers using `notify-rust`.
//!
//! Notifications are disabled automatically in CI / headless environments.
//! Enable / disable globally via `[notifications] enabled = true/false` in config.

use crate::config::NotificationsConfig;
use tracing::warn;

const APP_NAME: &str = "Harness AI";

/// Try to show a desktop notification. Silently swallows errors (headless / disabled).
pub fn notify(cfg: &NotificationsConfig, summary: &str, body: &str) {
    if !cfg.enabled {
        return;
    }
    send(summary, body);
}

fn send(summary: &str, body: &str) {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        use notify_rust::Notification;
        if let Err(e) = Notification::new()
            .appname(APP_NAME)
            .summary(summary)
            .body(body)
            .timeout(notify_rust::Timeout::Milliseconds(5000))
            .show()
        {
            warn!("desktop notification failed: {e}");
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        warn!("desktop notifications not supported on this platform (summary={summary:?})");
    }
}

/// Notify that a background run completed.
pub fn background_done(cfg: &NotificationsConfig, label: &str, success: bool) {
    if !cfg.on_background_done {
        return;
    }
    if success {
        notify(cfg, &format!("{APP_NAME} — Done"), &format!("Background run '{label}' completed."));
    } else {
        notify(cfg, &format!("{APP_NAME} — Failed"), &format!("Background run '{label}' failed."));
    }
}

/// Notify that auto-test failed.
pub fn autotest_failed(cfg: &NotificationsConfig, details: &str) {
    if !cfg.on_autotest_fail {
        return;
    }
    notify(cfg, &format!("{APP_NAME} — Test Failure"), details);
}

/// Notify that a budget threshold has been crossed.
pub fn budget_alert(cfg: &NotificationsConfig, message: &str) {
    if !cfg.on_budget {
        return;
    }
    notify(cfg, &format!("{APP_NAME} — Budget Alert"), message);
}

/// Notify with a custom summary and body (used by `/notify test`).
pub fn test_notification(cfg: &NotificationsConfig) {
    notify(cfg, &format!("{APP_NAME} — Test"), "Notifications are working.");
}
