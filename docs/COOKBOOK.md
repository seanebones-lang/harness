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

## See also

- [`README.md`](../README.md) — platform matrix and troubleshooting
- [`CONTRIBUTING.md`](../CONTRIBUTING.md) — how to add tools
- [`TODO.md`](../TODO.md) — severity-ranked backlog + roadmap (stable blocked on REL-01 manual smoke)
- [`BROWSER_CDP.md`](BROWSER_CDP.md) — browser tool setup
