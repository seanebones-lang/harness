# Migration Guide: Phase D → Phase E

This document covers breaking changes introduced in Phase E (April 2026).

## Breaking Changes

### 1. Voice Recording: `Ctrl+V` → `Ctrl+S`

**Phase D:** `Ctrl+V` triggered voice recording.  
**Phase E:** `Ctrl+V` is reserved for paste operations. Voice recording is now `Ctrl+S`.

**Why:** `Ctrl+V` is the standard paste shortcut; using it for voice caused conflicts when pasting text that started with `v`. `Ctrl+S` ("speak") is now the dedicated voice key.

### 2. Provider Initialization — No Longer XAI-gated

**Phase D:** Harness required `XAI_API_KEY` to be set at startup.  
**Phase E:** Smart provider detection — any of `ANTHROPIC_API_KEY`, `XAI_API_KEY`, or `OPENAI_API_KEY` will work. Ollama is tried as a final fallback.

**What to do:** Remove any workaround scripts that exported a dummy `XAI_API_KEY`. Just set the key for the provider you use.

### 3. `drive_agent_full` Signature Change

**Phase D:** `drive_agent_full(…, native_web_search, native_code_execution, native_x_search)`  
**Phase E:** `drive_agent_full(…, native_web_search, native_code_execution, native_x_search, response_schema)`

If you have any downstream code calling `drive_agent_full` directly, add `None` as the last argument.

### 4. MCP Protocol Version Upgrade

**Phase D:** MCP `protocolVersion: "2024-11-05"`  
**Phase E:** MCP `protocolVersion: "2025-03-26"`

Harness now negotiates full MCP 2.0 capabilities including `resources`, `sampling`, and `roots`. Old MCP servers that do not support 2025-03-26 will still work — the server's advertised capabilities are respected and missing features are skipped gracefully.

### 5. Config: New Sections

Phase E adds several new config sections to `~/.harness/config.toml`:

```toml
[observability]
enabled = true
traces_dir = "~/.harness/traces"
otlp_endpoint = ""

[swarm]
max_concurrency = 4
db_path = "~/.harness/swarm.db"

[bridges]
obsidian_vault = ""
apple_notes_folder = "Harness"
github_project_number = 0
github_owner = ""

[collab]
enabled = false
bind = "127.0.0.1:9090"
```

None of these are required — all Phase E features are opt-in via config. Existing configs continue to work unchanged.

### 6. New CLI Commands

Phase E adds the following new subcommands:

| Command | Description |
|---------|-------------|
| `harness doctor` | Self-diagnostic: check API keys, paths, services |
| `harness completions <shell>` | Generate shell completions (bash, zsh, fish, …) |
| `harness swarm list` | List recent swarm tasks |
| `harness swarm status <id>` | Show one task’s state |
| `harness swarm result <id>` | Print stored result when complete |
| `harness trace` | Summarize last local trace (when observability enabled) |
| `harness trace <id>` | Export a specific trace id |
| `harness voice --realtime` | Real-time duplex voice (OpenAI Realtime API) |

### 7. Notification System Overhaul

**Phase D:** Three notification hooks: `background_done`, `autotest_failed`, `budget_alert`.  
**Phase E:** Extended to 10 kinds. New hooks: `pr_opened`, `ci_failed`, `subagent_done`, `voice_response_done`, `swarm_complete`, `daemon_died`, `update_available`.

No config changes needed — all new kinds respect the existing `[notifications] enabled` flag.

## New Features Summary

| Feature | Details |
|---------|---------|
| E4 — Inline diff reviewer | `StagingBuffer` + LCS diff + auto-trust globs |
| E5 — Realtime voice | OpenAI Realtime API WebSocket duplex |
| E6 — Inline images | Kitty/iTerm2/Sixel terminal image rendering |
| E7 — Observability | Local JSONL traces + optional OTLP export |
| E8 — MCP 2.0 | Resources, sampling, roots, progress |
| E9 — Swarm | Parallel sub-agent tasks with SQLite registry |
| E10 — Strict JSON | `response_schema` on all providers |
| E11 — MLX | Native Apple Silicon local inference |
| E12 — Bridges | Obsidian, Apple Notes, Calendar, GitHub Projects |
| E13 — Collab | Multi-user shared sessions over WebSocket |
| E14 — VS Code ext | Side panel chat, Cmd+I inline edit |
| E15 — Tauri desktop | macOS .app with tray icon, Cmd+Shift+H |
| E16 — Notifications | 10 kinds, macOS grouping, focus/pomodoro mode |
