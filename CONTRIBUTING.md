# Contributing to Harness

Thank you for your interest in contributing. Harness is a Rust coding agent — multi-provider, terminal-first, and built to be hacked on. Whether you fix a bug, add a test, improve docs, or ship a whole new feature, you are welcome here.

---

## Quick orientation

| Doc | What it covers |
|-----|---------------|
| [`README.md`](README.md) | Install, run, daily workflow |
| [`CLAUDE.md`](CLAUDE.md) | Module map, key types, agent loop, adding providers/tools |
| [`TODO.md`](TODO.md) | Prioritised backlog — great place to pick up work |
| [`docs/SHORTCUTS.md`](docs/SHORTCUTS.md) | TUI keyboard reference |
| [`docs/MIGRATION.md`](docs/MIGRATION.md) | Breaking changes across phases |
| [`config/default.toml`](config/default.toml) | Annotated config reference |

---

## Setting up

```bash
git clone https://github.com/seanebones-lang/harness.git
cd harness
cargo build                          # dev build
cargo test --all                     # full test suite (no API keys required)
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all

# Optional: drop AI attribution trailers from commits
git config core.hooksPath .githooks
```

No API key is needed to build or run the automated test suite.

---

## Where to contribute

### Good first issues — tests and small fixes

These are well-scoped, self-contained, and unblock the next person reviewing the code.

- **`harness-browser` unit tests** (`crates/harness-browser/`)
  - No-Chrome error path (connect attempt with nothing listening on the CDP port)
  - Unknown `action` passed to `BrowserTool::execute` returns a clean error
  - CDP request/response JSON round-trip (mock the socket)
- **`ambient.rs` consolidation test** (`src/ambient.rs` or `tests/smoke_test.rs`)
  - Spin up a mock `MemoryStore` with ≥ 5 entries, trigger consolidation, assert merged/`__consolidated__` entries look right
- **Session-list title lag** (`src/main.rs` → `list_sessions()`)
  - Titles can lag the first `harness sessions` call because naming is async; a follow-up re-query or a UX note fixes it

### Architecture improvement — generic `ambient` provider

`src/ambient.rs` currently takes `XaiProvider` directly. Replacing it with a `P: Provider + Clone + 'static` bound lets any backend (Anthropic, Ollama, …) drive ambient consolidation. See the `Provider` trait in `crates/harness-provider-core/`. This is a medium-sized Rust generics change with a clear goal and no API keys needed to validate it.

### New providers

The router auto-selects providers from env keys. Adding a new one is four steps:

1. Create `crates/harness-provider-<name>/` and implement the `Provider` trait (see `CLAUDE.md` → *Adding a new provider*).
2. Add a build arm in `crates/harness-provider-router/src/lib.rs`.
3. Add env-key detection in the smart-defaults block.
4. Write at least one unit test with a mock HTTP server.

Interesting targets: **Mistral**, **Cohere**, **Google Gemini**, **AWS Bedrock**.

### New tools

See `CLAUDE.md` → *Adding a new tool*. The pattern is: implement `Tool` in `crates/harness-tools/src/tools/`, export it, register in `src/cli/wiring.rs`. Ideas:

- **`GitTool`** — structured git operations (status, diff, commit) without shelling out raw commands
- **`DatabaseTool`** — query SQLite or Postgres, return results as markdown tables
- **`NotebookTool`** — read/write Jupyter `.ipynb` cells
- **`DockerTool`** — list containers, exec, logs

### Platform coverage

- **Windows:** the shell tool falls back to `cmd.exe` when Git for Windows is absent; a richer Windows-native fallback (PowerShell) would help many users.
- **VS Code extension** (`extensions/vscode/`): Windows transport (currently Unix socket only); a named-pipe or TCP fallback would make it first-class on Windows.
- **Tauri desktop app** (`apps/desktop/`): Windows/Linux packaging, tray icon behaviour, auto-update.

### Documentation and UX

- CDP troubleshooting guide and screenshots for the browser tool
- A cookbook of real-world prompts / session transcripts
- Translations of `Start Here/USER MANUAL.md`
- Review the `docs/INSTALL.md` WSL2 section against a fresh machine

### Coverage and property tests

Current coverage target is ≥ 60 % on library crates. Proptest/fuzzing on:
- MCP message framing (`crates/harness-mcp/`)
- LSP framing (`crates/harness-lsp/`)
- Provider SSE parsing (`crates/harness-provider-openai/`, `crates/harness-provider-xai/`)

### `#![deny(missing_docs)]` on public crates

`harness-provider-core` and `harness-tools` are the public API surface. Adding doc comments and enabling `missing_docs` helps downstream users and IDEs.

---

## Submitting changes

1. **Fork** the repo, create a branch (`git checkout -b my-feature`).
2. Make your changes, keeping commits focused and the message clear.
3. Run the quality gates locally:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all
   ```
4. Open a **pull request** against `main`. Describe what the change does and why. For anything non-trivial, include a short test plan or before/after diff.

CI runs `fmt`, `clippy --all-features`, `test --all`, and `build` on **Ubuntu**, **macOS**, and **Windows**. PRs need all three to be green before merge.

---

## Code style

- **Rust edition 2021**, stable toolchain.
- `cargo fmt` is enforced in CI — run it before pushing.
- `clippy -D warnings` is enforced — fix warnings rather than suppressing them without explanation.
- No unnecessary comments. If a comment is needed, explain *why*, not *what*.
- Prefer editing existing files to adding new abstractions. Keep diffs reviewable.

---

## Reporting bugs

Open an issue with:
- OS and Rust version (`rustc --version`)
- `harness --version`
- The command you ran and the full error output (redact API keys)

---

## License

By contributing you agree your code is released under the project's [MIT License](LICENSE).

---

## Releasing

See [`docs/RELEASE_PROCESS.md`](docs/RELEASE_PROCESS.md) for how to cut new releases.

Only maintainers with write access should create releases.
