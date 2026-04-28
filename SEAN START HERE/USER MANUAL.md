# SEAN START HERE - Harness User Manual

This guide explains how to use Harness in simple language.

## What This Tool Is

Harness is your coding assistant in the terminal.
You type requests, and it can:

- read files
- edit files
- run commands
- explain codeX
- help debug problems

---

## 1) One-Time Setup

Open a terminal and run:

```bash
cd /Users/nexteleven/harness/harness
set -a; source .env; set +a
```

This loads your API key from `.env`.

---

## 2) Start Interactive Mode (Main Way)

```bash
cd /Users/nexteleven/harness/harness
set -a; source .env; set +a
cargo run
```

You will see the two-panel interface:

- left side = chat
- right side = tools/events
- bottom = message input

Type a request and press `Enter`.

### Example first prompt

`Read CLAUDE.md and summarize the key commands.`

---

## 3) Run a Single Task (No Interactive UI)

```bash
cd /Users/nexteleven/harness/harness
set -a; source .env; set +a
cargo run -- "explain what src/main.rs does"
```

---

## 4) Resume an Old Session

```bash
cd /Users/nexteleven/harness/harness
set -a; source .env; set +a
cargo run -- --resume <session-id-or-name> "continue"
```

Replace `<session-id-or-name>` with what you see from `sessions`.

---

## 5) List, Export, and Delete Sessions

```bash
cd /Users/nexteleven/harness/harness
cargo run -- sessions
```

Export one session:

```bash
cargo run -- export <id> --output my-session.md
```

Delete one session:

```bash
cargo run -- delete <id>
```

---

## 6) Web Mode (Optional)

Start server:

```bash
cd /Users/nexteleven/harness/harness
set -a; source .env; set +a
cargo run -- serve --addr 127.0.0.1:8787
```

Open browser:

`http://127.0.0.1:8787`

---

## 7) Good Prompt Style (Simple Formula)

Use this format for best results:

1. What you want
2. Where to do it
3. What "done" looks like

Example:

`Add error handling in src/server.rs. Keep behavior the same. Run tests after changes.`

---

## 8) Common Problems

### "command not found: cargo"
Install Rust and restart terminal.

### Key/auth errors
Reload `.env`:

```bash
set -a; source .env; set +a
```

### Nothing happens after Enter in UI
Check if app is busy (status line) and wait for current task to finish.

---

## 9) Safety Notes

- Do not commit `.env` or API keys.
- Rotate API keys if they were shared publicly.
- Ask for a dry run if you want review before edits.

---

## 10) Quick Start (Copy/Paste)

```bash
cd /Users/nexteleven/harness/harness
set -a; source .env; set +a
cargo run
```

Then type:

`Read src/main.rs and explain the command structure in plain English.`

---

## 11) Real Example: Work on a QA Repo

This is a full example you can copy and follow.

### Goal

Open a repo named `qa-repo`, fix a failing test, and commit the fix.

### Step A: Open the QA repo in terminal

```bash
cd /Users/nexteleven/path/to/qa-repo
```

If the repo also has a `.env` key file, load it:

```bash
set -a; source .env; set +a
```

### Step B: Start Harness in that repo

```bash
cargo run
```

If you run Harness from a separate install path, use your installed binary instead:

```bash
harness
```

### Step C: Give a clear task prompt

Type this in the Harness message box:

`Run tests, find the first failing QA test, fix the root cause, and re-run tests. Keep changes minimal and explain what changed.`

### Step D: Review what it did

Ask:

`Show me exactly which files were changed and why.`

Then ask:

`Run clippy and tests again and confirm all green.`

### Step E: Commit from Harness

Prompt:

`Create a git commit with a clear message for this fix.`

If you want a specific commit message style, say:

`Use commit title: fix(qa): handle null response in validator`

### Step F: Optional push

Prompt:

`Push this branch to origin.`

---

## 12) Good QA Prompts You Can Reuse

- `Run the full test suite and fix only the first failing test.`
- `Find flaky tests in tests/qa and make them deterministic.`
- `Add a regression test for the bug you just fixed.`
- `Refactor this test helper for readability without behavior changes.`
- `Review this repo for top 3 risk areas and propose fixes.`
