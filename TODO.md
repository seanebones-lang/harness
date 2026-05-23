# Harness — Open Tasks

**New here?** Start with [`CONTRIBUTING.md`](CONTRIBUTING.md) — it explains the codebase, how to run tests, and how to open a PR. The items below are good places to pick up work.

Canonical user docs: [`README.md`](README.md), [`Start Here/USER MANUAL.md`](Start%20Here/USER%20MANUAL.md).  
Developer detail: [`CLAUDE.md`](CLAUDE.md), [`config/default.toml`](config/default.toml).

Implemented and closed: xAI `stream_options` + usage, multi-tool-call streaming tests, embedding retries, TUI `--resume`, ambient `ctrl_c` shutdown, web UI session persistence, session delete CLI, clippy cleanliness.

Release readiness: [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) · latest verdict: [`docs/RELEASE_STATUS.md`](docs/RELEASE_STATUS.md)

---

## Good first contributions

### Unit tests — `harness-browser` (`crates/harness-browser/`)

Three targeted tests, no API key needed:

- No-Chrome error path: connect with nothing listening on the CDP port → clean error, no panic.
- Unknown `action` passed to `BrowserTool::execute` → returns `Err`, not `unreachable!`.
- CDP request/response JSON round-trip: mock the socket, assert serialization is stable.

### Unit tests — `ambient.rs` consolidation (`src/ambient.rs` or `tests/smoke_test.rs`)

Spin up a mock `MemoryStore` with ≥ 5 entries, trigger consolidation, assert merged / `__consolidated__` entries are written correctly.

### Swarm module tests and CLI integration

The swarm system (src/swarm.rs) has solid core logic but lacks:
- Unit tests for register/update/list flows
- Integration with the main CLI (`harness swarm spawn`, `harness swarm status`)
- Background watcher that auto-updates task status when sub-agents finish

Add these as high-priority Polish items.

### Session-list title lag (`src/main.rs` → `list_sessions()`)

Session names are generated async; titles can be missing on the first `harness sessions` call right after save. Options: re-query after rename completes, or add a note in UX copy. Small, contained change.

---

## Architecture

### Generic `ambient` provider (`src/ambient.rs`)

`spawn()` and `consolidate()` take `XaiProvider` directly. Replace with `P: Provider + Clone + 'static` so any backend (Anthropic, Ollama, …) can drive ambient consolidation. See the `Provider` trait in `crates/harness-provider-core/`. Medium-sized generics change with a clear interface contract.

### Coverage and property testing

Current coverage target: ≥ 60 % on library crates. Proptest / fuzzing targets:

- MCP message framing (`crates/harness-mcp/`)
- LSP framing (`crates/harness-lsp/`)
- Provider SSE parsing (`crates/harness-provider-openai/`, `crates/harness-provider-xai/`)

### `#![deny(missing_docs)]` on public crates

`harness-provider-core` and `harness-tools` are the public API surface. Doc comments + `missing_docs` help downstream consumers and IDEs. Pair with `cargo doc --open` to verify output.

---

## New providers

See [`CLAUDE.md`](CLAUDE.md) → *Adding a new provider* for the three-step pattern. Interesting targets: **Mistral**, **Cohere**, **Google Gemini**, **AWS Bedrock**.

## New tools

See [`CLAUDE.md`](CLAUDE.md) → *Adding a new tool*. Ideas: `GitTool` (structured git ops), `DatabaseTool` (SQLite/Postgres → markdown tables), `NotebookTool` (Jupyter `.ipynb`), `DockerTool` (container list/exec/logs).

## Platform coverage

- **Windows:** shell tool falls back to `cmd.exe` when Git for Windows is absent; a PowerShell native path would improve the experience.
- **VS Code extension** (`extensions/vscode/`): currently Unix socket only; a named-pipe or TCP fallback makes it first-class on Windows.
- **Tauri desktop app** (`apps/desktop/`): Windows/Linux packaging, tray-icon polish, auto-update.

## Documentation polish

- Screenshots and a CDP troubleshooting guide for the browser tool
- Cookbook of real-world prompts / session transcripts
- Translations of `Start Here/USER MANUAL.md`

---

## Release checklist (maintainers)

Automated gates — **CI runs these on Ubuntu, macOS, Windows**:

- [x] `cargo test --all` — workspace integration + doctests (no API keys)
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [x] `cargo fmt --all -- --check`
- [x] `cargo build --profile release-lto`

Manual smoke (needs API keys / local GUI):

- [ ] `XAI_API_KEY=... harness "list files in ."` — one-shot works
- [ ] `XAI_API_KEY=... harness` — TUI, token counts in status bar
- [ ] `XAI_API_KEY=... harness serve` + `http://127.0.0.1:8787` — web UI chat
- [ ] `harness export <id>` — Markdown readable in a viewer
- [ ] `harness sessions` — lists sessions including auto-named titles

Full checklist: [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md).

---

## High Priority Polish (v0.1.1)

### Safety & Approval Flows
- Improve visual diff previews in TUI (unified diff rendering)
- Make "Smart mode" vs "Plan mode" differences more obvious to users
- Add one-line "what will change" summary before multi-file edits

### Swarm Reliability & Cost Control
- Define clear "Fast" vs "Smart" sub-agent profiles with explicit cost/time budgets
- Add swarm-level cost caps and easy cancellation
- Improve task status visibility and background watcher reliability

### Session Management Polish
- Fix session-list title lag (async name generation)
- Better session naming and recent session UX

### Context Optimization
- Improve compaction strategy and token estimation accuracy
- Reduce unnecessary context bloat in long sessions

### Windows Experience
- Improve PowerShell install experience
- Add clearer Git for Windows requirement messaging and troubleshooting

