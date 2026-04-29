# SEAN START HERE — Harness User Manual (April 2026)

This guide explains how to use Harness in plain English.

---

## What Harness Does

Harness is an AI coding assistant you run in your terminal. You type a request; it reads files, writes code, runs shell commands, fixes tests, commits — whatever you ask. It supports multiple AI providers (Anthropic Claude, xAI Grok, OpenAI, local Ollama), has a full TUI with syntax highlighting, remembers past sessions semantically, and integrates with your language server.

**Default model: `claude-sonnet-4-6`** — 10x cheaper than base price on repeated context thanks to Anthropic prompt caching. Falls back to xAI → OpenAI → local Ollama based on which API keys are set.

---

## One-Time Setup (do this once)

### Step 1 — Install

From inside the harness source directory:

```bash
cargo build --profile release-lto
install -m 755 target/release-lto/harness ~/.local/bin/harness
```

Add `~/.local/bin` to your PATH in `~/.zshrc`:

```bash
export PATH="$HOME/.local/bin:$PATH"
source ~/.zshrc
harness --version  # confirm it works
```

### Step 2 — Set your API key

```bash
export ANTHROPIC_API_KEY="sk-ant-..."   # preferred (get at console.anthropic.com)
export XAI_API_KEY="xai-..."            # fallback (console.x.ai)
export OPENAI_API_KEY="sk-..."          # fallback (platform.openai.com)
```

Add to `~/.zshrc` to make permanent. Harness auto-detects which keys are set.

### Step 3 — Initialize

```bash
harness init
```

---

## Daily Use (normal flow)

```bash
cd /path/to/your/project
harness
```

That's it. No `source .env`, no `cargo run`.

Two-panel interface:
- **Left panel** — conversation
- **Right panel** — tool calls and events
- **Bottom bar** — type your message, press `Enter`

**Keyboard reference:** all Phase E shortcuts (search, paste-friendly voice key, panel resize, `/focus`, etc.) are listed in [`docs/SHORTCUTS.md`](../docs/SHORTCUTS.md). Breaking changes from older builds: [`docs/MIGRATION.md`](../docs/MIGRATION.md).

### Good first prompts

```
Read the README and summarize what this project does.
Run the tests and tell me which ones are failing.
Find all TODO comments and list them by file.
```

---

## April 2026 Models

Pick the right model for the job:

| Model | Use When |
|-------|----------|
| `claude-sonnet-4-6` (default) | Most tasks — fast, cheap with caching |
| `claude-opus-4-7` | Complex architecture, long tasks, adaptive thinking |
| `claude-haiku-4-5` | Summaries, quick lookups — ultra-fast |
| `grok-4.20-0309-reasoning` | Code reasoning, 2M context window |
| `grok-4-1-fast-reasoning` | Real-time low-latency tasks |
| `gpt-5.5` | When you want OpenAI's latest |
| `qwen3-coder:30b` | Fully local (no API key), 256K context |

Switch models:
```bash
harness models                                 # list all available
harness models --set anthropic:claude-opus-4-7 # set default for this project
```

Or mid-session: `/model claude-opus-4-7`

---

## Extended Thinking (Opus 4.7 + Sonnet 4.6)

Enable the model to "think aloud" before answering — great for complex architecture tasks:

```bash
harness --think 10000    # 10k token thinking budget
harness --think 0        # disable (default)
```

Or in TUI: `/think 10000`

---

## Approve Mode (review before changes apply)

```bash
harness --plan
```

Or type `/plan` inside the TUI. Shows a preview and asks for confirmation before any write, patch, or shell command.

---

## Structured JSON responses (`/schema`)

For APIs, configs, or any workflow where the model must return **valid JSON** matching a schema:

```
/schema my_output {"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]}
```

Clears with `/schema clear`. While set, the agent attaches strict structured-output instructions for your current provider (OpenAI/xAI JSON Schema mode; Anthropic synthetic tool). See [`CLAUDE.md`](../CLAUDE.md) for technical detail.

---

## TUI Slash Commands

Type any of these in the input bar and press Enter:

| Command              | What it does                                              |
|----------------------|-----------------------------------------------------------|
| `/help`              | Show this list in the event log                           |
| `/clear`             | Clear the chat panel (keeps session)                      |
| `/undo`              | Restore files from the last git checkpoint                |
| `/diff`              | Show `git diff` in the event log                          |
| `/test`              | Run the test suite and stream output                      |
| `/compact`           | Summarise old messages to free up context                 |
| `/cost`              | Show running token count + dollar estimate + cache hit %  |
| `/plan`              | Toggle plan mode on/off                                   |
| `/model <name>`      | Switch model for new turns                                |
| `/think [N]`         | Enable extended thinking with N-token budget              |
| `/remember t: fact`  | Store fact under topic t in `.harness/memory/`            |
| `/forget t`          | Delete memory topic t                                     |
| `/memories`          | List all memory topics                                    |
| `/pr [N]`            | List open PRs or load PR #N for review                    |
| `/issues`            | List open GitHub issues                                   |
| `/ci`                | Show recent CI workflow runs                              |
| `/notify test`       | Send a test desktop notification                          |
| `/runs`              | List background agent runs                                |
| `/fork`              | Note about session forking (use Ctrl+E)                   |
| `/focus [N]`         | Pomodoro-style quiet: silence notifications for N minutes (default 25); `/focus off` clears |
| `/focus off`         | Exit focus / notification quiet mode                      |
| `/schema …`          | Strict JSON output: `/schema clear` or `/schema name {"type":"object",…}` |
| `/obsidian save`     | Save last assistant reply to Obsidian (needs `[bridges]` vault path) |

See [`docs/SHORTCUTS.md`](../docs/SHORTCUTS.md) for the full list.

---

## TUI Keybindings

| Key       | Action                                          |
|-----------|-------------------------------------------------|
| `Enter`   | Send message                                    |
| `Shift+Enter` / `Alt+Enter` | Newline inside the input box              |
| `Tab`     | Autocomplete `@file` paths or slash commands    |
| `↑ / ↓`   | Scroll chat (or navigate input history at line start) |
| `PgUp/Dn` | Scroll event log                                |
| `Ctrl+S`  | Voice record → transcribe (Whisper); **Phase E default** |
| `Ctrl+Y`  | Copy last assistant reply to system clipboard   |
| `Ctrl+F`  | Search chat; `Ctrl+N` / `Ctrl+P` next/prev match |
| `Ctrl+L`  | Jump chat to bottom (latest messages)           |
| `Ctrl+]` / `Ctrl+[` | Widen / narrow the right (events) panel   |
| `Ctrl+E`  | Fork session — branch from a past turn          |
| `Ctrl+C`  | Quit                                            |

> **Migration:** voice was moved from **Ctrl+V** to **Ctrl+S** so **Ctrl+V** can paste normally. Details in [`docs/MIGRATION.md`](../docs/MIGRATION.md).

---

## Pinning Files into Messages (`@file`)

```
@src/api.rs Add input validation to reject empty strings with a 400 error.
```

Press `Tab` after `@` to autocomplete. Pin multiple files in one message.

---

## Voice Input (Ctrl+S / `harness voice`)

In the TUI, use **`Ctrl+S`** for voice capture → Whisper transcription → text inserted into the input (same flow as before; key changed in Phase E).

One-shot CLI:
```bash
harness voice            # record + print transcript
harness voice --send     # record + send to agent immediately
harness voice --realtime # duplex conversation via OpenAI Realtime API (requires OPENAI_API_KEY)
```

Backends (auto-detected):
- **OpenAI Whisper** — if `OPENAI_API_KEY` is set, uses `gpt-4o-transcribe`
- **Local** — if `whisper-cli` is on PATH, uses whisper.cpp offline

---

## Project Memory

Store persistent facts about your project — automatically injected into every session:

```bash
harness memorize architecture "Monorepo: crates/ + src/. Provider abstraction in harness-provider-core."
harness memorize tests "Run cargo test --workspace. Lints via cargo clippy."
harness memorize deploy "Deploy with cargo build --release + scp to prod."
harness memories         # list all topics
harness forget tests     # delete a topic
```

Or in TUI:
```
/remember tests: run cargo nextest for speed
/forget tests
/memories
```

Memory files live in `.harness/memory/<topic>.md` and are version-controlled with your project.

---

## GitHub Workflow

```bash
harness pr 123                   # load PR #123 diff + comments into session
harness pr 123 --comment "LGTM"  # post a review comment
```

In TUI:
```
/pr           → list open PRs for this branch
/pr 123       → load PR #123 context
/issues       → list open issues
/ci           → show recent CI run status
```

Requires `gh` CLI installed and authenticated (`gh auth login`).

---

## Cost Tracking

```bash
harness cost today               # today's spend
harness cost week                # last 7 days
harness cost month               # last 30 days
harness cost by-model            # breakdown by model
harness cost by-project          # breakdown by project directory
harness cost watch               # live tail (updates every 5s)
```

Set budget limits in `~/.harness/config.toml`:
```toml
[budget]
daily_usd = 5.00
monthly_usd = 50.00
```

Status bar shows cost in real time. Turns yellow at 80%, red at 100%.
Desktop notification fires at each threshold.

The status bar also shows **prompt cache hit rate** — when using Claude, repeated context (system prompt, pinned files) is served at 10x discount automatically.

---

## Desktop Notifications

Harness sends macOS Notification Center / libnotify alerts for:
- Background agent runs completing (done or failed)
- Auto-test failures
- Budget threshold crossings (80%, 100%)
- Additional kinds are available in code for PRs, CI, swarm completion, daemon lifecycle, etc., as those integrations fire.

Use **`/focus 25`** in the TUI for a 25-minute “quiet” window (notifications suppressed); the status bar shows **`[FOCUS Nm]`**. **`/focus off`** clears it.

Test alerts with `/notify test`, or disable globally:
```toml
[notifications]
enabled = false
```

---

## Cross-Machine Sync

Encrypt and sync `~/.harness` state (sessions, memory, cost DB) to a private git repo:

```bash
harness sync init git@github.com:you/harness-state.git
harness sync push      # encrypt with age + push
harness sync pull      # pull + decrypt (on another machine)
harness sync status    # show recent syncs
harness sync auth      # show passphrase storage info
```

Passphrase is stored in macOS Keychain (or `~/.harness/.sync-key` as fallback).
Encryption uses `age` with scrypt — no plaintext secrets ever leave your machine.

---

## Undo / Checkpoints

Before every destructive tool call, Harness automatically creates a git stash checkpoint:

```bash
harness undo               # restore from most recent checkpoint
harness checkpoint list    # see all saved checkpoints
```

Or type `/undo` in TUI.

---

## Resume a Previous Session

```bash
harness --resume <session-id>
harness sessions    # find session IDs
```

---

## Per-Project Setup

```bash
cd my-project
harness init --project   # writes .harness/config.toml
```

You can also create `.harness/SYSTEM.md`, `AGENTS.md`, or `CLAUDE.md` in your project root — Harness reads whichever it finds first and prepends it to every session automatically.

---

## Computer Use (macOS, opt-in)

Enable the agent to control your mouse and keyboard:

```toml
[computer_use]
enabled = true   # DANGER: agent can move mouse, type, take screenshots
```

Requires `cliclick` (`brew install cliclick`). Only works with `claude-opus-4-7` or newer.
TUI shows a red `[COMPUTER USE LIVE]` banner whenever active.

---

## Native Server-Side Tools

Enable provider-managed tools (billed per call, no local plumbing needed):

```toml
[native_tools]
web_search = true       # Anthropic/xAI native web search
code_execution = false  # sandboxed code execution
x_search = false        # xAI X (Twitter) post search
```

---

## Session Management

```bash
harness sessions                     # list recent sessions
harness export <id>                  # print as Markdown
harness export <id> --output out.md  # save to file
harness delete <id>                  # delete a session
```

---

## Background Runs

```bash
harness run-bg "fix all clippy warnings in the codebase"
harness runs                         # show status of all background runs
```

Output streams to `~/.harness/runs/<id>/output.log`. Desktop notification fires on completion.

---

## Fork a Past Turn (Ctrl+E)

1. Press `Ctrl+E` in the TUI
2. Type the turn number to branch from (e.g. `3`)
3. Press `Enter`

A new session is created with messages up to that turn. Continue with a corrected prompt.

---

## Auto-Test Loop

```toml
[autotest]
enabled = true
scope = "package"   # "package" or omit for full suite
```

Runs tests automatically after every file write. Agent self-corrects on failures. Desktop notification fires when tests fail.

---

## Auto-Format

After every file write, Harness automatically runs:
- `.rs` → `rustfmt`
- `.ts/.tsx/.js/.jsx/.json` → `prettier`
- `.py` → `ruff format`
- `.go` → `gofmt`

---

## Check Your Setup

```bash
harness status   # config summary, recent sessions, MCP hints
harness doctor   # deeper checks: keys, paths, daemon socket, toolchain hints
```

### Shell completions

```bash
harness completions bash
harness completions zsh
harness completions fish
```

Redirect output into your shell’s completion directory (see `harness completions --help`).

### Swarm & traces (Phase E)

Parallel sub-agent tasks (when you use swarm tooling):

```bash
harness swarm list              # recent tasks
harness swarm status <task-id> # one task state
harness swarm result <task-id> # captured output when finished
```

Local observability spans (when enabled in config):

```bash
harness trace          # summarize last trace
harness trace <id>     # export a specific trace id
```

### Desktop app & VS Code

- **Tauri shell:** [`apps/desktop/README.md`](../apps/desktop/README.md) — tray icon, **Cmd+Shift+H** (Ctrl+Shift+H on Windows/Linux) to show/hide, tries to run `harness daemon` if no socket exists.
- **VS Code:** [`extensions/vscode/`](../extensions/vscode/) — install deps with `npm install`, then open in VS Code and run the extension (or package with `vsce`).

---

## Common Problems

### "command not found: harness"

Install Rust, build, copy binary to `~/.local/bin`, add to `$PATH`. See One-Time Setup above.

### API key errors

Run `harness status`. Re-run `harness init --force` to update the stored key.
Or set the env var: `export ANTHROPIC_API_KEY="..."`

### Nothing happens after pressing Enter

Check the bottom status bar — the agent may still be running. Wait for it to finish.

### Checkpoint/undo fails with "not a git repo"

Run `git init` in your project root.

---

## Writing Good Prompts

Formula: **What** you want + **Where** + What **done** looks like

```
Add input validation to src/api.rs.
Reject empty strings with a 400 error.
Run tests after. Keep all existing tests passing.
```

---

## Example: Full QA Fix Workflow

```bash
cd my-project
harness
```

Then type in sequence:

```
Run the full test suite and show me only the failing tests.
Fix the first failing test. Keep the fix minimal and explain what changed.
Run clippy and tests again and confirm everything is green.
Create a git commit with a clear message for this fix.
```

---

## Reusable Prompts

- `Run the full test suite and fix only the first failing test.`
- `Find flaky tests and make them deterministic.`
- `Add a regression test for the bug you just fixed.`
- `Review this repo for the top 3 risk areas and propose fixes.`
- `Refactor this test helper for readability without behavior changes.`
- `Show me all files changed in the last commit and explain each change.`
- `Find all usages of <function> and rename it to <new_name> across the project.`
- `Run diagnostics on src/main.rs and fix every error and warning.`
- `Summarize today's work as a standup update.`
- `Review PR 123 and suggest improvements.`
