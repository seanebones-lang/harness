# Harness prompt cookbook

Real-world prompts and expected tool patterns. Adjust paths and model flags for your project.

## 1. Explore a repo

**Prompt:** `List the top-level directories and summarize what each crate does.`

**Expected tools:** `list_dir`, `read_file`, optionally `search_code`.

## 2. Fix a failing test

**Prompt:** `Run cargo test for harness-memory and fix any failures.`

**Expected tools:** `shell` (`cargo test -p harness-memory`), `read_file`, `patch_file` or `write_file`, re-run tests.

## 3. Safe git status

**Prompt:** `Show git status and a short diff stat for staged files.`

**Expected tools:** `git` with `action: status` and `action: diff` (prefer over raw `shell git`).

## 4. Refactor with review

**Prompt:** `Extract list_sessions formatting into a helper in sessions.rs without changing CLI output.`

**Expected tools:** `read_file`, `patch_file`, `shell` (`cargo test --all`).

## 5. Browser smoke test

**Prompt:** `Navigate to https://example.com and return the page title.`

**Expected tools:** `browser` with `action: navigate` then `page_info` or `evaluate`.

**Prerequisite:** Chrome with `--remote-debugging-port=9222` — see [`BROWSER_CDP.md`](BROWSER_CDP.md).

## 6. Session export

**Prompt:** `Export session abc12345 to markdown.`

**Expected tools:** CLI `harness export abc12345` (agent may use `shell`).

## 7. Memory recall

**Prompt:** `Remember that our default embed model is nomic-embed-text.`

**Expected tools:** project memory via `harness memorize` or agent memory hooks after turn completion.

## 8. PR review

**Prompt:** `/pr 42` or `Review PR #42 — summarize risk and failing checks.`

**Expected tools:** `gh` tool or TUI slash commands when `gh auth login` is configured.

## 9. Structured JSON output

**Prompt:** `Return a JSON object listing all provider crate names.`

**Expected tools:** `list_dir` on `crates/`, response constrained when `--schema` / `response_schema` is set.

## 10. Windows shell note

On native Windows without Git Bash, prefer **`git`** and typed tools over POSIX shell pipelines. The **`shell`** tool uses Git `sh`/`bash` when available, then **PowerShell**, then **`cmd.exe`**.

## 11. Parallel swarm (CLI + TUI)

Queue several background agent workers, poll the registry, collect results, then clean orphans. Task state lives in SQLite at **`~/.harness/swarm.db`** (override with env `HARNESS_SWARM_DB` or `[swarm] db_path` in config).

**Prompt (each worker):** `Review crates/harness-tools and list the three most important tools with one-line why.`

**Expected tools (workers):** `list_dir`, `read_file`, `search_code` (or equivalent); parent process only uses the swarm CLI / TUI panel.

### Queue parallel workers

`--count`, `--agents`, and `-n` are aliases (default count is 1). Optional `--model` overrides the worker model.

```bash
# Three parallel copies of the same prompt
harness swarm run --count 3 \
  "Review crates/harness-tools and list the three most important tools with one-line why."

# Same via aliases
harness swarm run --agents 3 "…"
harness swarm run -n 3 "…"

# Optional model override for workers
harness swarm run -n 2 --model claude-sonnet-4-6 "Summarize src/swarm.rs public API"
```

`run` prints task ids and returns immediately. Workers keep running under the process that spawned them; the registry outlives a crash — use `gc` for leftover `pending`/`running` rows (see below).

### List, status, wait, result

```bash
harness swarm list                 # counts + recent tasks (pending/running/done/failed/cancelled)

harness swarm status <task-id>     # multi-line detail (id, status, prompt, times, preview)
# Task id prefix is enough when unique:
harness swarm status a1b2

harness swarm wait <task-id>                    # block until terminal (default 300s)
harness swarm wait <task-id> --timeout-secs 120

harness swarm status <task-id>     # human detail
harness swarm status <task-id> --json
harness swarm result <task-id>     # full stored output when Done (or status text if not)
harness swarm result <task-id> --json
harness swarm cancel <task-id>     # cancel pending/running when the spawner is still live
harness swarm cancel --all         # cancel every non-terminal task
```

Typical flow after `run --count 3`:

```bash
harness swarm list
# pick an id from the table, then:
harness swarm wait <id>
harness swarm result <id>
```

### GC orphans (dry-run, then real)

After a killed process, rows can sit in `pending`/`running` with no live worker. **Always dry-run first** on the real DB.

```bash
# Preview: what would be reaped / purged (no writes)
harness swarm gc --dry-run

# Reap non-live pending/running older than 1h (default --stale-secs 3600)
harness swarm gc

# Stricter stale window + keep only newest 20 terminal rows
harness swarm gc --stale-secs 900 --keep 20

# Age-purge terminal rows completed more than 7 days ago
harness swarm gc --older-than-secs 604800

# Combine (still prefer a dry-run first)
harness swarm gc --dry-run --stale-secs 1800 --keep 50 --older-than-secs 86400
harness swarm gc --stale-secs 1800 --keep 50 --older-than-secs 86400
```

Flags (clap):

| Flag | Meaning |
|------|---------|
| `--stale-secs <N>` | Mark non-live `pending`/`running` older than N seconds as failed (default `3600`) |
| `--keep <N>` | After reap, keep only the newest N terminal tasks; delete the rest |
| `--older-than-secs <N>` | Delete terminal tasks completed more than N seconds ago |
| `--dry-run` | Report changes only; do not write |

### TUI: F2 and `/swarm`

In the interactive TUI (`harness` with no one-shot prompt):

| Input | Effect |
|-------|--------|
| **F2** | Toggle right panel: Events ↔ Swarm registry |
| `/swarm` | Same toggle (also `/swarm toggle`, `/swarm panel`) |
| `/swarm refresh` | Reload registry into the panel (opens Swarm mode if closed) |
| `/swarm gc` | Reap orphans from the TUI (default stale window) |
| `/swarm gc stale=900` | Custom stale seconds (`keep=N` also accepted) |
| `PgUp` / `PgDn` | Scroll the swarm panel when it is open |
| **Enter** (empty input) | Peek selected swarm task status/result into the Events panel |

Legend in the panel: `*` = live worker, `!` = orphan non-terminal. A status chip `[SWARM N]` appears when tasks are active even if the panel is closed. Full shortcut list: [`SHORTCUTS.md`](SHORTCUTS.md).

### Demo script

End-to-end queue + list without waiting on models: [`demo/scenario_2_swarm.sh`](../demo/scenario_2_swarm.sh).

## 12. Browser tool troubleshooting

**Prompt:** `Open https://example.com, take a screenshot, and summarize the title.`

**Expected tools:** `browser` (`navigate`, `screenshot`, `page_info` / `get_text`).

**Setup:** Chrome/Chromium with remote debugging — see [`BROWSER_CDP.md`](BROWSER_CDP.md).

```bash
# macOS example
/Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome \
  --remote-debugging-port=9222 --user-data-dir=/tmp/harness-chrome

curl -s http://127.0.0.1:9222/json/version | head
harness --browser "navigate to example.com and screenshot"
```

**If the tool fails**, read the full error string. Typical structured messages:

| Error fragment | Meaning / fix |
|----------------|---------------|
| `Browser connect failed` / `Chrome DevTools HTTP unreachable` | Chrome not listening on the configured URL/port |
| `CDP connection closed` / `WebSocket` | Tab/browser died mid-session — retry (harness resets the session) |
| `unknown browser action` | Typo — valid: navigate, click, type, focus, get_text, get_links, evaluate, screenshot, page_info |
| `CDP error N: …` | Page/selector issue; adjust selector or wait for load |

Config:

```toml
[browser]
enabled = true
url = "http://127.0.0.1:9222"
```

## See also

- [`README.md`](../README.md) — platform matrix and troubleshooting
- [`CONTRIBUTING.md`](../CONTRIBUTING.md) — how to add tools
- [`TODO.md`](../TODO.md) — severity-ranked backlog + roadmap (stable blocked on REL-01 manual smoke)
- [`BROWSER_CDP.md`](BROWSER_CDP.md) — browser tool setup
- [`SHORTCUTS.md`](SHORTCUTS.md) — TUI keys including F2 / `/swarm`
- [`CTO_BACKLOG.md`](CTO_BACKLOG.md) — ordered engineering waves
