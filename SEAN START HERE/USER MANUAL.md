# SEAN START HERE — Harness User Manual

This guide explains how to use Harness in plain English.

---

## What Harness Does

Harness is an AI coding assistant you run in your terminal. You type a request; it reads files, writes code, runs shell commands, fixes tests, commits — whatever you ask.

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

## Web UI (optional)

```bash
harness serve --addr 127.0.0.1:8787
```

Then open `http://127.0.0.1:8787` in a browser. Your session is remembered across refreshes. Use the "New session" button to start fresh.

---

## TUI Keybindings

| Key | Action |
|---|---|
| `Enter` | Send message |
| `↑ / ↓` | Scroll chat |
| `PgUp / PgDn` | Scroll event log |
| `Ctrl+C` | Quit |

---

## Common Problems

### "command not found: harness"

Install Rust, build, copy the binary to `~/.local/bin`, add that to `$PATH`. See One-Time Setup above.

### API key errors

Run `harness status` to see which key is being used. Re-run `harness init --force` to update it.

### Nothing happens after pressing Enter

Check if the agent is mid-task (look at the bottom status bar). Wait for it to finish.

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
