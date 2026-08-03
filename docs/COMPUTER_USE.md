# Computer use — per-OS sandbox defaults and risks

The `computer` tool lets the agent take screenshots and drive mouse/keyboard on the **local desktop**. It is **off by default** and only registered when both:

1. Config enables it: `[computer_use] enabled = true`
2. The active model is Claude 4.x family (registration checks for `claude-opus-4` / `claude-sonnet-4` / `claude-opus-4-7` in the model id)

When active, the TUI shows a red **`[COMPUTER USE LIVE]`** banner.

## Enable

```toml
# ~/.harness/config.toml or .harness/config.toml
[computer_use]
enabled = true
```

Related filesystem sandbox (does **not** jail the desktop — only path tools):

```toml
[tools]
sandbox = "strict"   # default: read_file/write_file/… stay under workspace root
# sandbox = "relaxed"  # allow outside paths with a warning
# sandbox = "off"      # disable path checks (dangerous)
```

## Platform backends

| OS | Screenshot | Mouse / keyboard | Notes |
|----|------------|------------------|-------|
| **macOS** | `screencapture` (built-in) | `cliclick` (`brew install cliclick`) | Accessibility permission required for input simulation. Prefer a dedicated user session; do not enable on shared login screens. |
| **Linux** | `scrot` or `maim` | `xdotool` (X11) | Wayland support is limited — many setups need an X11 session or XWayland. Install: `scrot`/`maim` + `xdotool`. |
| **Windows** | Best-effort via available tools | Limited / may require extra utilities | Prefer WSL2 + Linux tooling, or keep computer use **disabled** unless you have verified local drivers. Native Windows path is not a full peer of macOS/Linux today. |

Implementation lives in `crates/harness-tools/src/tools/computer.rs` and shells out to system CLIs (no heavy native GUI crates).

## Default safety posture

| Control | Default | Meaning |
|---------|---------|---------|
| `[computer_use] enabled` | **false** / unset | Tool not registered |
| Model gate | Claude 4.x ids only | Other models log a warning and skip registration |
| `[tools] sandbox` | **strict** | Unrelated to mouse/keyboard; keeps file tools in the project root |
| Approval / plan mode | User config | Destructive *file* tools still follow `[approval]`; computer actions are inherently high-risk |

There is **no** OS-level sandbox around mouse/keyboard: if the tool is enabled, the agent can click anywhere the OS allows the harness process to click (other apps, dialogs, browser password fields, etc.).

## Risks

- **Credential exposure** — screenshots may capture secrets; clicks can submit forms or approve prompts.
- **Irreversible UI actions** — send mail, confirm purchases, delete cloud resources in a browser already logged in.
- **Shared machines** — never enable on multi-user or kiosk hosts ([`THREAT_MODEL.md`](THREAT_MODEL.md)).
- **Missing CLIs** — honest failures if `cliclick` / `xdotool` / screenshot tools are absent; install before relying on the tool.

## Recommended practices

1. Use a **throwaway OS user** or VM for computer-use sessions.
2. Close apps with secrets; use a clean browser profile.
3. Prefer **`--plan`** / `[approval] mode = "plan"` so other destructive tools pause (computer use itself is still powerful once enabled).
4. Keep `[tools] sandbox = "strict"` so file edits stay in-repo while exploring the UI.
5. Disable computer use (`enabled = false` or remove the key) when finished.

## Quick verify

```bash
# macOS
which screencapture cliclick

# Linux
which scrot maim xdotool

# Run harness with a Claude 4.x model after enabling config
./target/debug/harness --model claude-sonnet-4-6 "take a screenshot and describe the open window"
```

## See also

- [`COOKBOOK.md`](COOKBOOK.md) — prompts and optional tools (database / notebook / docker)
- [`BROWSER_CDP.md`](BROWSER_CDP.md) — safer alternative for web UI via CDP (no full desktop control)
- [`config/default.toml`](../config/default.toml) — `[computer_use]` and `[tools]` blocks
- [`THREAT_MODEL.md`](THREAT_MODEL.md) — threat notes for computer use
