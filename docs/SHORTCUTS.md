# NextEleven Harness TUI — Keyboard Shortcuts Cheat Sheet

> All shortcuts work in the main TUI (`harness` or `harness serve` + browser).

## Sending & Navigation

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Shift+Enter` / `Alt+Enter` | Insert newline in input |
| `↑` / `↓` | Scroll chat history |
| `PgUp` / `PgDn` | Scroll event log (or swarm panel when open) |
| `F2` | Toggle right panel: Events ↔ Swarm registry |
| `Ctrl+L` | Scroll chat to bottom (latest) |
| `Esc` | Cancel pending confirm / close overlay |

## Input Editing (Readline-style)

| Key | Action |
|-----|--------|
| `Ctrl+A` / `Home` | Move cursor to start of line |
| `Ctrl+E` / `End` | Move cursor to end of line |
| `Ctrl+W` | Delete word backwards |
| `Ctrl+U` | Delete to start of line |
| `Ctrl+K` | Delete to end of line |
| `Alt+←` | Move cursor one word left |
| `Alt+→` | Move cursor one word right |
| `↑` (in input) | Navigate history (previous) |
| `↓` (in input) | Navigate history (next) |

## Session & Agent

| Key | Action |
|-----|--------|
| `Ctrl+C` | Quit |
| `Ctrl+Y` | Copy last assistant response to clipboard |
| `Ctrl+F` | Toggle chat search mode |
| `Ctrl+N` / `Ctrl+P` | Next/prev search match |
| `Tab` | Autocomplete @file or slash command |

## Voice

| Key | Action |
|-----|--------|
| `Ctrl+S` | Toggle voice recording (one-shot Whisper) |

> Note: Ctrl+V was changed to Ctrl+S in Phase E. See `docs/MIGRATION.md`.

## Layout

| Key | Action |
|-----|--------|
| `Ctrl+]` | Widen right panel |
| `Ctrl+[` | Narrow right panel |
| `F2` | Toggle Events ↔ Swarm panel (also `/swarm`) |
| `Enter` (empty, Swarm panel) | Peek selected swarm task result into Events |

## Plan Mode

| Key | Action |
|-----|--------|
| `Y` | Approve pending tool call |
| `N` | Skip pending tool call |
| `A` | Approve all remaining |

## Slash Commands

Type `/` to trigger autocomplete. All commands:

| Command | Description |
|---------|-------------|
| `/clear` | Clear chat and event log |
| `/undo` | Restore last git checkpoint |
| `/diff` | Show `git diff --stat HEAD` |
| `/plan` | Toggle plan mode |
| `/fork` | Fork session into a new branch |
| `/fork <name>` | Fork with a named branch |
| `/test` | Run project test suite |
| `/save <name>` | Save session with a name |
| `/model <name>` | Switch model for this session |
| `/think <N>` | Enable extended thinking with N token budget |
| `/think off` | Disable extended thinking |
| `/focus [N]` | Silence notifications for N minutes (default: 25) |
| `/focus off` | Cancel focus mode |
| `/notify test` | Send a test notification |
| `/obsidian save` | Save last response to Obsidian vault |
| `/schema <name> <json>` | Set strict JSON output schema |
| `/schema clear` | Clear output schema |
| `/swarm` | Toggle swarm panel |
| `/swarm refresh` | Reload swarm registry into panel |
| `/swarm gc […]` | Reap orphan swarm tasks from TUI |
| `/help` / `/?` | Show help |

## @file Mentions

Type `@` followed by a file path in the input box. Press `Tab` to autocomplete.
The file contents are attached to the next message.

## Focus Mode

`/focus 25` starts a 25-minute Pomodoro. The status bar shows `[FOCUS Nm]`.
Notifications are silenced until the timer expires or `/focus off` is typed.
Voice input auto-enables focus mode during recording.

## CLI (related)

| Command | Action |
|---------|--------|
| `harness swarm list\|status\|gc\|…` | Swarm registry ops |
| `harness bench [--json] [--pack DIR]` | Offline microbench pack |
| `harness mcp roots` | Roots advertised to MCP servers |
| `harness mcp resources [--server NAME]` | List MCP resources |
| `harness mcp read <uri> [--server NAME]` | Read one MCP resource |
| `harness bridge obsidian …` | Obsidian bridge write |
| `harness trace [id]` | Dump last / named OTEL-ish trace |
| `harness doctor` | Health: keys, bridges, MCP, observability |
| `harness models [--set …]` | Provider/model catalogue |

## MCP CLI (resources / roots)

Outside the TUI, list or read MCP resources without starting a full agent session:

```bash
harness mcp roots                          # roots harness advertises (CWD + home)
harness mcp resources                      # resources/list across mcp.json servers
harness mcp resources --server filesystem  # one server only
harness mcp read <uri>                     # resources/read (first server that succeeds)
harness mcp read <uri> --server filesystem
```

Requires `.harness/mcp.json`, `.claude/mcp.json`, `~/.harness/mcp.json`, or `[mcp] config_path`.
Respects `[mcp] command_allowlist` (same as tool loading).
