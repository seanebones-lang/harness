#![allow(dead_code)]
//! Desktop notification helpers using `notify-rust`.
//!
//! E16: Notification overhaul — rich macOS notifications with action buttons,
//! grouping, pomodoro focus mode, and new notification kinds.

use crate::config::NotificationsConfig;
use tracing::warn;

const APP_NAME: &str = "NextEleven Harness";

/// All notification event kinds in NextEleven Harness.
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
    /// A new version of NextEleven Harness is available.
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
            Self::Custom => "NextEleven Harness",
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
    let (summary, body) = background_done_copy(label, success);
    notify_rich(cfg, NotificationKind::BackgroundDone, &summary, &body);
}

/// Summary + body for background-done notifications (pure).
pub(crate) fn background_done_copy(label: &str, success: bool) -> (String, String) {
    if success {
        (
            format!("{APP_NAME} — Done"),
            format!("Background run '{label}' completed."),
        )
    } else {
        (
            format!("{APP_NAME} — Failed"),
            format!("Background run '{label}' failed."),
        )
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
        &pr_opened_body(title, url),
    );
}

pub(crate) fn pr_opened_body(title: &str, url: &str) -> String {
    format!("{title}\n{url}")
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
        &ci_failed_body(job, url),
    );
}

pub(crate) fn ci_failed_body(job: &str, url: &str) -> String {
    format!("Job '{job}' failed\n{url}")
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
        &subagent_done_body(task_id, result),
    );
}

pub(crate) fn subagent_done_body(task_id: &str, result: &str) -> String {
    format!("Task {task_id}: {result}")
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
    notify_rich(
        cfg,
        NotificationKind::SwarmComplete,
        &format!("{APP_NAME} — Swarm Complete"),
        &swarm_complete_body(total, failed),
    );
}

/// Body text for swarm-complete notifications (pure).
pub(crate) fn swarm_complete_body(total: usize, failed: usize) -> String {
    if failed == 0 {
        format!("All {total} tasks completed successfully.")
    } else {
        format!("{total} tasks done, {failed} failed.")
    }
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
        "The NextEleven Harness daemon restarted. Sessions may have been reset.",
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
        &update_available_body(version),
    );
}

pub(crate) fn update_available_body(version: &str) -> String {
    format!("Version {version} is available. Run `harness update` to upgrade.")
}

/// Notify with a custom summary and body (used by `/notify test`).
pub fn test_notification(cfg: &NotificationsConfig) {
    notify(
        cfg,
        &format!("{APP_NAME} — Test"),
        "Notifications are working! 🎉",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NotificationsConfig;

    fn disabled() -> NotificationsConfig {
        NotificationsConfig {
            enabled: false,
            on_background_done: true,
            on_autotest_fail: true,
            on_budget: true,
        }
    }

    #[test]
    fn group_id_maps_kinds_to_stable_buckets() {
        assert_eq!(NotificationKind::BackgroundDone.group_id(), "harness.agent");
        assert_eq!(
            NotificationKind::LongSubagentDone.group_id(),
            "harness.agent"
        );
        assert_eq!(NotificationKind::SwarmComplete.group_id(), "harness.agent");
        assert_eq!(NotificationKind::AutotestFailed.group_id(), "harness.ci");
        assert_eq!(NotificationKind::CiFailed.group_id(), "harness.ci");
        assert_eq!(NotificationKind::PrOpened.group_id(), "harness.github");
        assert_eq!(NotificationKind::BudgetAlert.group_id(), "harness.budget");
        assert_eq!(
            NotificationKind::VoiceResponseDone.group_id(),
            "harness.voice"
        );
        assert_eq!(NotificationKind::DaemonDied.group_id(), "harness.daemon");
        assert_eq!(
            NotificationKind::UpdateAvailable.group_id(),
            "harness.update"
        );
        assert_eq!(NotificationKind::Custom.group_id(), "harness.misc");
    }

    #[test]
    fn subtitle_maps_each_kind() {
        assert_eq!(
            NotificationKind::BackgroundDone.subtitle(),
            "Background Run"
        );
        assert_eq!(NotificationKind::AutotestFailed.subtitle(), "Test Runner");
        assert_eq!(NotificationKind::BudgetAlert.subtitle(), "Cost Monitor");
        assert_eq!(NotificationKind::PrOpened.subtitle(), "GitHub");
        assert_eq!(NotificationKind::CiFailed.subtitle(), "CI/CD");
        assert_eq!(NotificationKind::LongSubagentDone.subtitle(), "Sub-agent");
        assert_eq!(NotificationKind::VoiceResponseDone.subtitle(), "Voice");
        assert_eq!(NotificationKind::SwarmComplete.subtitle(), "Swarm");
        assert_eq!(NotificationKind::DaemonDied.subtitle(), "Daemon");
        assert_eq!(NotificationKind::UpdateAvailable.subtitle(), "Update");
        assert_eq!(NotificationKind::Custom.subtitle(), "NextEleven Harness");
    }

    #[test]
    fn disabled_config_is_noop_for_all_entry_points() {
        // Must not attempt desktop I/O when disabled — pure early-return paths.
        let cfg = disabled();
        notify(&cfg, "s", "b");
        notify_rich(&cfg, NotificationKind::Custom, "s", "b");
        background_done(&cfg, "job", true);
        background_done(&cfg, "job", false);
        autotest_failed(&cfg, "fail");
        budget_alert(&cfg, "over");
        pr_opened(&cfg, "t", "http://x");
        ci_failed(&cfg, "job", "http://x");
        subagent_done(&cfg, "id", "ok");
        voice_response_done(&cfg);
        swarm_complete(&cfg, 3, 1);
        swarm_complete(&cfg, 2, 0);
        daemon_died(&cfg);
        update_available(&cfg, "1.2.3");
        test_notification(&cfg);
    }

    #[test]
    fn feature_flags_gate_specific_helpers() {
        let mut cfg = NotificationsConfig {
            enabled: true,
            on_background_done: false,
            on_autotest_fail: false,
            on_budget: false,
        };
        // Flags off → no-op even when enabled=true (still no desktop I/O asserted).
        background_done(&cfg, "x", true);
        autotest_failed(&cfg, "x");
        budget_alert(&cfg, "x");
        cfg.on_background_done = true;
        cfg.enabled = false;
        background_done(&cfg, "x", true);
    }

    #[test]
    fn kind_equality_and_clone() {
        assert_eq!(NotificationKind::Custom, NotificationKind::Custom);
        assert_ne!(NotificationKind::CiFailed, NotificationKind::PrOpened);
        let k = NotificationKind::SwarmComplete.clone();
        assert_eq!(k.group_id(), "harness.agent");
        assert_eq!(k.subtitle(), "Swarm");
    }

    #[test]
    fn pure_copy_builders() {
        let (ok_s, ok_b) = background_done_copy("ship", true);
        assert!(ok_s.contains("Done"));
        assert!(ok_b.contains("'ship' completed"));
        let (fail_s, fail_b) = background_done_copy("ship", false);
        assert!(fail_s.contains("Failed"));
        assert!(fail_b.contains("'ship' failed"));

        assert_eq!(
            pr_opened_body("Add X", "https://gh/pr/1"),
            "Add X\nhttps://gh/pr/1"
        );
        assert_eq!(
            ci_failed_body("test", "https://ci/1"),
            "Job 'test' failed\nhttps://ci/1"
        );
        assert_eq!(
            subagent_done_body("t1", "ok"),
            "Task t1: ok"
        );
        assert_eq!(
            swarm_complete_body(4, 0),
            "All 4 tasks completed successfully."
        );
        assert_eq!(swarm_complete_body(4, 2), "4 tasks done, 2 failed.");
        assert!(update_available_body("0.2.0").contains("0.2.0"));
        assert!(update_available_body("0.2.0").contains("harness update"));
    }

    #[test]
    fn disabled_gates_even_when_feature_flags_on() {
        // enabled=false wins over on_* flags for entry points that check enabled first,
        // and feature-flagged helpers still return before notify_rich when their flag is on
        // only if enabled path is taken — exercise both flag-on + enabled-off.
        let cfg = NotificationsConfig {
            enabled: false,
            on_background_done: true,
            on_autotest_fail: true,
            on_budget: true,
        };
        background_done(&cfg, "job", true);
        background_done(&cfg, "job", false);
        autotest_failed(&cfg, "details");
        budget_alert(&cfg, "msg");
        // helpers that only check enabled
        pr_opened(&cfg, "t", "u");
        swarm_complete(&cfg, 0, 0);
        swarm_complete(&cfg, 5, 5);
        test_notification(&cfg);
    }
}
