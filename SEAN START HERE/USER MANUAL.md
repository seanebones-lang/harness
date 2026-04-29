# SEAN START HERE — Harness User Manual

This guide explains how to use Harness in plain English.

---

## What Harness Does

Harness is an AI coding assistant you run in your terminal. You type a request; it reads files, writes code, runs shell commands, fixes tests, commits — whatever you ask. It supports multiple AI providers (xAI Grok, Anthropic Claude, OpenAI, local Ollama models), has a full TUI with syntax highlighting, remembers past sessions semantically, and integrates with your language server.

---

## One-Time Setup (do this once)

### Step 1 — Install

From inside the harness source directory:

```bash
cargo build --profile release-lto
install -m 755 target/release-lto/harness ~/.local/bin/harness
```

Then add `~/.local/bin` to your PATH permanently. In `~/.zshrc`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

Reload your shell:

```bash
source ~/.zshrc
```

Confirm it works:

```bash
harness --version
```

### Step 2 — Initialize

Run this once to create your global config and store your API key:

```bash
harness init
```

It will prompt you for your xAI API key (get one at https://console.x.ai).

---

## Daily Use (normal flow)

```bash
cd /path/to/your/project
harness
```

That's it. No `source .env`, no `cargo run`.

You will see a two-panel interface:
- **Left panel** — conversation
- **Right panel** — tool calls and events
- **Bottom bar** — type your message here, press `Enter` to send

### Good first prompts

```
Read the README and summarize what this project does.
Run the tests and tell me which ones are failing.
Find all TODO comments and list them by file.
```

---

## Approve Mode (review before changes apply)

If you want to see what Harness is about to do before it writes files or runs commands:

```bash
harness --plan
```

Or type `/plan` inside the TUI to toggle it on. You'll be shown a preview and asked to confirm before any write, patch, or shell command executes.

---

## TUI Slash Commands

Type any of these in the input bar and press Enter:

| Command | What it does |
|---|---|
| `/help` | Show this list in the event log |
| `/clear` | Clear the chat panel (keeps session) |
| `/undo` | Restore files from the last git checkpoint |
| `/diff` | Show `git diff` in the event log |
| `/test` | Run the test suite and stream output |
| `/compact` | Summarise old messages to free up context |
| `/cost` | Show running token count and dollar estimate |
| `/plan` | Toggle plan mode on/off |
| `/model <name>` | Switch model for new turns (e.g. `/model grok-3`) |
| `/runs` | List background agent runs |
| `/fork` | Note about session forking (use Ctrl+E) |

---

## TUI Keybindings

| Key | Action |
|---|---|
| `Enter` | Send message |
| `Tab` | Autocomplete `@file` paths in input |
| `↑ / ↓` | Scroll chat panel |
| `PgUp / PgDn` | Scroll event log |
| `Ctrl+E` | Fork session — enter a turn number to branch from that point |
| `Ctrl+C` | Quit |

---

## Pinning Files into Messages (`@file`)

Prefix any file path with `@` in your message to attach its full contents before sending:

```
@src/api.rs Add input validation to reject empty strings with a 400 error.
```

Press `Tab` after `@` to autocomplete paths from the current directory. You can pin multiple files in one message.

---

## Undo / Checkpoints

Before every destructive tool call (write, patch, shell command), Harness automatically creates a git stash checkpoint. To roll back:

```bash
harness undo           # restore from the most recent checkpoint
harness checkpoint list  # see all saved checkpoints
```

Or type `/undo` in the TUI.

---

## Resume a Previous Session

```bash
harness --resume <session-id>
```

Find session IDs with:

```bash
harness sessions
```

---

## Per-Project Setup

To give Harness a custom system prompt tuned for one repo:

```bash
cd my-project
harness init --project
```

This writes `.harness/config.toml` in the current directory. Edit the `system_prompt` there to describe the project, conventions, testing approach, etc.

You can also create `.harness/SYSTEM.md`, `AGENTS.md`, or `CLAUDE.md` in your project root — Harness reads whichever it finds first and prepends it to every session automatically.

---

## Check Your Setup

```bash
harness status
```

Shows: which API key is loaded, which config file is active, which MCP servers are configured, and your last 5 sessions.

---

## Session Management

```bash
harness sessions                     # list recent sessions
harness export <id>                  # print a session as Markdown
harness export <id> --output out.md  # save to file
harness delete <id>                  # delete a session
```

---

## Background Runs

Run the agent on a task without tying up your terminal:

```bash
harness run-bg "fix all clippy warnings in the codebase"
```

Output streams to `~/.harness/runs/<id>/output.log`. Check status with:

```bash
harness runs
```

Or type `/runs` in any TUI session.

---

## Fork a Past Turn (Ctrl+E)

If the agent went off-rails three turns ago, you can branch from that point instead of starting over:

1. Press `Ctrl+E` in the TUI
2. Type the turn number you want to fork at (e.g. `3`)
3. Press `Enter`

A new session is created with messages up to that turn. Continue from there with a corrected prompt.

---

## Multi-Provider Support

Harness supports xAI Grok (default), Anthropic Claude, OpenAI, and local Ollama models. Configure in `~/.harness/config.toml`:

```toml
[providers.anthropic]
api_key = "sk-ant-..."
model = "claude-sonnet-4-5"

[providers.xai]
api_key = "xai-..."
model = "grok-3-fast"

[providers.ollama]
base_url = "http://localhost:11434"
model = "qwen2.5-coder:7b"

[router]
default = "anthropic"
fast_model = "xai:grok-3-mini-fast"
heavy_model = "anthropic:claude-sonnet-4-5"
embed_model = "ollama:nomic-embed-text"
fallback = ["anthropic", "xai", "openai", "ollama"]
```

Switch models mid-session with `/model <name>`.

---

## Attach an Image

Pass a screenshot or diagram to support vision-capable models:

```bash
harness "what's wrong in this screenshot?" --image error.png
```

In the TUI, paste an image file path (the TUI detects `.png`, `.jpg`, etc. from bracketed paste and converts it to an `@file` reference automatically).

---

## Auto-Test Loop

Enable in config to automatically run tests after every file write and feed failures straight back to the agent:

```toml
[autotest]
enabled = true
scope = "package"   # "package" or omit for full suite
```

The agent sees test failures immediately and self-corrects without you needing to prompt it again.

---

## Auto-Format

After every file write, Harness automatically runs the appropriate formatter:
- `.rs` → `rustfmt`
- `.ts/.tsx/.js/.jsx/.json` → `prettier`
- `.py` → `ruff format`
- `.go` → `gofmt`

Best-effort: if the formatter isn't installed, the file is written as-is.

---

## Trust Rules (skip confirmation for known-safe commands)

In `--plan` mode, you can permanently skip confirmation for commands you always approve:

```bash
harness trust shell "cargo check"       # never ask about cargo check
harness trust write_file "*"            # never ask about any file write
harness trust-list                      # show all rules
harness untrust shell "cargo check"     # remove a rule
```

After approving the same command 3 times in a row, the TUI will suggest the matching `harness trust` command for you.

---

## LSP Integration (find definition, references, rename)

If `rust-analyzer`, `typescript-language-server`, `pyright`, or `gopls` is installed, Harness auto-detects and connects to it. The agent gains four tools:

- **`find_definition`** — jump to where a symbol is defined
- **`find_references`** — list every usage site
- **`rename_symbol`** — project-wide rename (returns a diff to review)
- **`diagnostics`** — live errors and warnings

No configuration required — it just works if the language server binary is on your PATH.

---

## Daemon Mode (fast startup)

Start a long-lived background process that holds all resources (SQLite, LSP, provider clients):

```bash
harness daemon
```

Other `harness` invocations auto-connect to it via `~/.harness/daemon.sock`. This eliminates the 1-2 second cold start on every launch. Check status with:

```bash
harness daemon-status
```

Stop it with `Ctrl+C` in the terminal running `harness daemon`.

---

## Context Compaction

Harness automatically summarises the oldest half of the conversation when it approaches 70% of the model's context window — preserving all file paths, decisions, and tool results in a compact form.

Force it manually at any time with `/compact` in the TUI.

---

## Web UI (optional)

```bash
harness serve --addr 127.0.0.1:8787
```

Then open `http://127.0.0.1:8787` in a browser. Your session is remembered across refreshes. Use the "New session" button to start fresh.

---

## Common Problems

### "command not found: harness"

Install Rust, build, copy the binary to `~/.local/bin`, add that to `$PATH`. See One-Time Setup above.

### API key errors

Run `harness status` to see which key is being used. Re-run `harness init --force` to update it.

### Nothing happens after pressing Enter

Check the bottom status bar — the agent may still be running. Wait for it to finish.

### Checkpoint/undo fails with "not a git repo"

Checkpoints require a git repository. Run `git init` in your project root if you haven't already.

---

## Writing Good Prompts

Use this formula:

1. **What** you want done
2. **Where** to do it (file/directory)
3. What **done** looks like

Example:

```
Add input validation to src/api.rs. Reject empty strings with a 400 error.
Run tests after. Keep all existing tests passing.
```

---

## Example: Full QA Fix Workflow

```bash
cd my-project
harness
```

Then type these prompts in sequence:

```
Run the full test suite and show me only the failing tests.
```

```
Fix the first failing test. Keep the fix minimal and explain what changed.
```

```
Run clippy and tests again and confirm everything is green.
```

```
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
