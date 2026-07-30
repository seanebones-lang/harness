# Notifications audit (W3.7) — 2026-07-30

## Platform support

| OS | Backend | Notes |
|----|---------|-------|
| macOS | `notify-rust` → Notification Center | Subtitle via `n.subtitle`; group_id reserved for future |
| Linux | `notify-rust` → libnotify/DBus | Body+summary only |
| Windows | `notify-rust` | Same path; silent fail if toast unavailable |
| Other | warn log only | No panic |

Errors from `n.show()` are logged and swallowed (headless-safe).

## Config (`[notifications]`)

| Key | Default | Effect |
|-----|---------|--------|
| `enabled` | `true` | Master switch for most helpers |
| `on_background_done` | `true` | Gates `background_done` |
| `on_autotest_fail` | `true` | Gates `autotest_failed` |
| `on_budget` | `true` | Gates `budget_alert` |

Helpers that only check `enabled`: PR opened, CI failed, subagent done, voice done, swarm complete, daemon died, update available, custom/test.

## Call sites (wired)

| Kind | Call site |
|------|-----------|
| BackgroundDone | `src/tui/events.rs` (background run finished) |
| SwarmComplete | `src/cli/lightweight.rs` (`swarm wait` Done/Failed) |
| VoiceResponseDone | `src/main.rs` (voice path) |
| DaemonDied | `src/main.rs` (daemon restart path) |
| CiFailed | `src/tui/input.rs` (`/notify` / CI line helper) |
| LongSubagentDone | `src/cli/wiring.rs` (`spawn_agent` runner after drive) |
| Budget / Autotest | helpers exist; call when cost/test integrations fire |
| PrOpened / UpdateAvailable | helpers exist for integrations |

## Gaps / follow-ups

- Optional per-kind flags beyond the three `on_*` keys (swarm/voice/subagent still use master `enabled` only).
- macOS action buttons / grouping IDs are defined on `NotificationKind` but not yet fully pushed into `notify-rust` APIs on all versions.
- Multi-task swarm run completion (batch) should call `swarm_complete(total, failed)` once when the last worker exits — currently per-`wait` task.

## Manual smoke

```bash
# With notifications enabled in ~/.harness/config.toml
./target/debug/harness  # TUI: /notify test if available
# Or trigger swarm wait after a short run
```
