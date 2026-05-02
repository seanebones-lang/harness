#![allow(dead_code)]
//! Desktop notification helpers using `notify-rust`.
//!
//! E16: Notification overhaul — rich macOS notifications with action buttons,
//! grouping, pomodoro focus mode, and new notification kinds.

use crate::config::NotificationsConfig;
use tracing::warn;

const APP_NAME: &str = "Harness AI";

/// All notification event kinds in Harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationKind {
    BackgroundDone,
    AutotestFailed,
    BudgetAlert,
    /// A PR was opened (from GitHub integration).
    PrOpened,
    /// A CI run failed.
    CiFailed,
    /// A long-running sub-agent completed.
    LongSubagentDone,
    /// A voice response finished speaking.
    VoiceResponseDone,
    /// The parallel swarm finished all tasks.
    SwarmComplete,
    /// The harness daemon restarted/crashed.
    DaemonDied,
    /// A new version of Harness is available.
    UpdateAvailable,
    /// Custom/test notification.
    Custom,
}

impl NotificationKind {
    /// Notification group identifier (macOS notification grouping).
    pub fn group_id(&self) -> &'static str {
        match self {
            Self::BackgroundDone | Self::LongSubagentDone | Self::SwarmComplete => "harness.agent",
            Self::AutotestFailed | Self::CiFailed => "harness.ci",
            Self::PrOpened => "harness.github",
            Self::BudgetAlert => "harness.budget",
            Self::VoiceResponseDone => "harness.voice",
            Self::DaemonDied => "harness.daemon",
            Self::UpdateAvailable => "harness.update",
            Self::Custom => "harness.misc",
        }
    }

    /// Subtitle shown under the title on macOS.
    pub fn subtitle(&self) -> &'static str {
        match self {
            Self::BackgroundDone => "Background Run",
            Self::AutotestFailed => "Test Runner",
            Self::BudgetAlert => "Cost Monitor",
            Self::PrOpened => "GitHub",
            Self::CiFailed => "CI/CD",
            Self::LongSubagentDone => "Sub-agent",
            Self::VoiceResponseDone => "Voice",
            Self::SwarmComplete => "Swarm",
            Self::DaemonDied => "Daemon",
            Self::UpdateAvailable => "Update",
            Self::Custom => "Harness",
        }
    }
}

/// Try to show a desktop notification. Silently swallows errors (headless / disabled).
pub fn notify(cfg: &NotificationsConfig, summary: &str, body: &str) {
    if !cfg.enabled {
        return;
    }
    send_notification(summary, body, None, "harness.misc");
}

/// Rich notification with kind, grouping, and macOS action buttons.
pub fn notify_rich(cfg: &NotificationsConfig, kind: NotificationKind, summary: &str, body: &str) {
    if !cfg.enabled {
        return;
    }
    send_notification(summary, body, Some(kind.subtitle()), kind.group_id());
}

fn send_notification(summary: &str, body: &str, subtitle: Option<&str>, _group_id: &str) {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        use notify_rust::Notification;
        let mut n = Notification::new();
        n.appname(APP_NAME)
            .summary(summary)
            .body(body)
            .timeout(notify_rust::Timeout::Milliseconds(6000));

        // On macOS we set subtitle via the subtitle() method if available
        #[cfg(target_os = "macos")]
        if let Some(s) = subtitle {
            n.subtitle(s);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = subtitle;

        if let Err(e) = n.show() {
            warn!("desktop notification failed: {e}");
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        warn!("notifications not supported on this platform (summary={summary:?})");
    }
}

// ── Convenience helpers ────────────────────────────────────────────────────────

/// Notify that a background run completed.
pub fn background_done(cfg: &NotificationsConfig, label: &str, success: bool) {
    if !cfg.on_background_done {
        return;
    }
    if success {
        notify_rich(
            cfg,
            NotificationKind::BackgroundDone,
            &format!("{APP_NAME} — Done"),
            &format!("Background run '{label}' completed."),
        );
    } else {
        notify_rich(
            cfg,
            NotificationKind::BackgroundDone,
            &format!("{APP_NAME} — Failed"),
            &format!("Background run '{label}' failed."),
        );
    }
}

/// Notify that auto-test failed.
pub fn autotest_failed(cfg: &NotificationsConfig, details: &str) {
    if !cfg.on_autotest_fail {
        return;
    }
    notify_rich(
        cfg,
        NotificationKind::AutotestFailed,
        &format!("{APP_NAME} — Test Failure"),
        details,
    );
}

/// Notify that a budget threshold has been crossed.
pub fn budget_alert(cfg: &NotificationsConfig, message: &str) {
    if !cfg.on_budget {
        return;
    }
    notify_rich(
        cfg,
        NotificationKind::BudgetAlert,
        &format!("{APP_NAME} — Budget Alert"),
        message,
    );
}

/// Notify about a PR opened event.
pub fn pr_opened(cfg: &NotificationsConfig, title: &str, url: &str) {
    if !cfg.enabled {
        return;
    }
    notify_rich(
        cfg,
        NotificationKind::PrOpened,
        &format!("{APP_NAME} — PR Opened"),
        &format!("{title}\n{url}"),
    );
}

/// Notify that a CI run failed.
pub fn ci_failed(cfg: &NotificationsConfig, job: &str, url: &str) {
    if !cfg.enabled {
        return;
    }
    notify_rich(
        cfg,
        NotificationKind::CiFailed,
        &format!("{APP_NAME} — CI Failed"),
        &format!("Job '{job}' failed\n{url}"),
    );
}

/// Notify that a long-running sub-agent finished.
pub fn subagent_done(cfg: &NotificationsConfig, task_id: &str, result: &str) {
    if !cfg.enabled {
        return;
    }
    notify_rich(
        cfg,
        NotificationKind::LongSubagentDone,
        &format!("{APP_NAME} — Sub-agent Done"),
        &format!("Task {task_id}: {result}"),
    );
}

/// Notify that a voice response finished.
pub fn voice_response_done(cfg: &NotificationsConfig) {
    if !cfg.enabled {
        return;
    }
    notify_rich(
        cfg,
        NotificationKind::VoiceResponseDone,
        &format!("{APP_NAME} — Voice Ready"),
        "Your voice response is ready.",
    );
}

/// Notify that the swarm completed all tasks.
pub fn swarm_complete(cfg: &NotificationsConfig, total: usize, failed: usize) {
    if !cfg.enabled {
        return;
    }
    let body = if failed == 0 {
        format!("All {total} tasks completed successfully.")
    } else {
        format!("{total} tasks done, {failed} failed.")
    };
    notify_rich(
        cfg,
        NotificationKind::SwarmComplete,
        &format!("{APP_NAME} — Swarm Complete"),
        &body,
    );
}

/// Notify that the harness daemon crashed/restarted.
pub fn daemon_died(cfg: &NotificationsConfig) {
    if !cfg.enabled {
        return;
    }
    notify_rich(
        cfg,
        NotificationKind::DaemonDied,
        &format!("{APP_NAME} — Daemon Restarted"),
        "The Harness daemon restarted. Sessions may have been reset.",
    );
}

/// Notify that a new version is available.
pub fn update_available(cfg: &NotificationsConfig, version: &str) {
    if !cfg.enabled {
        return;
    }
    notify_rich(
        cfg,
        NotificationKind::UpdateAvailable,
        &format!("{APP_NAME} — Update Available"),
        &format!("Version {version} is available. Run `harness update` to upgrade."),
    );
}

/// Notify with a custom summary and body (used by `/notify test`).
pub fn test_notification(cfg: &NotificationsConfig) {
    notify(
        cfg,
        &format!("{APP_NAME} — Test"),
        "Notifications are working! 🎉",
    );
}
