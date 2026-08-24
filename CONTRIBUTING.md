# Contributing to NextEleven Harness

Thank you for your interest in contributing. NextEleven Harness is a Rust coding agent — multi-provider, terminal-first, and built to be hacked on. Whether you fix a bug, add a test, improve docs, or ship a whole new feature, you are welcome here.

---

## Quick orientation

| Doc | What it covers |
|-----|---------------|
| [`README.md`](README.md) | Install, run, daily workflow |
| [`CLAUDE.md`](CLAUDE.md) | Module map, key types, agent loop, adding providers/tools |
| [`TODO.md`](TODO.md) | Prioritised backlog — promotion tiers + audit items |
| [`docs/PROMOTION_REPORT.md`](docs/PROMOTION_REPORT.md) | Public beta readiness and launch checklist |
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

Pick a pathway below. Each maps to a **`good first issue`** label on GitHub when maintainers triage the backlog.

| Pathway | Good for | Entry point | Quality gate |
|---------|----------|-------------|--------------|
| **Tools** | Rust + CLI ergonomics | [`crates/harness-tools/`](crates/harness-tools/) | Unit test or smoke in `cargo test --all` |
| **Providers** | HTTP/SSE, API integration | [`crates/harness-provider-*/`](crates/) | Mock-server unit test |
| **Tests & coverage** | No API keys needed | [`tests/`](tests/), crate `#[cfg(test)]` | Uplift toward CI target 60% ([`COVERAGE.md`](COVERAGE.md) ~23%) |
| **Docs** | Writing, screenshots | [`docs/`](docs/), [`README.md`](README.md) | Spell-check + link check |
| **Platform** | Windows, VS Code, Tauri | [`extensions/vscode/`](extensions/vscode/), [`apps/desktop/`](apps/desktop/) | CI matrix green |
| **MCP / LSP** | Protocol work | [`crates/harness-mcp/`](crates/harness-mcp/), [`crates/harness-lsp/`](crates/harness-lsp/) | Framing + handshake tests |

### Good first issues — tests and small fixes

These are well-scoped, self-contained, and unblock the next person reviewing the code. Ask maintainers to label your chosen issue **`good first issue`**.

- **New tools** — `DatabaseTool`, `NotebookTool`, `DockerTool` (see below)
- **New providers** — Mistral, Gemini, Bedrock (four-step guide below)
- **MCP sampling TUI approval** — interactive prompt when MCP servers request `sampling/createMessage`
- **Demo GIF** — 15–30s TUI recording for README (see [`docs/PROMOTION_REPORT.md`](docs/PROMOTION_REPORT.md))
- **Coverage uplift** — voice/mlx/lsp client integration paths (Round 2 added unit tests for detect/availability; deeper paths still welcome)

~~**`ambient.rs` consolidation test**~~ — **Done** (May 2026).

~~**Session-list title lag**~~ — **Done** — first-message fallback + async naming (May 2026).

~~**Generic ambient provider**~~ — **Done** (May 2026). `AmbientProviders` uses router **fast** for summaries and **embed** for vectors; tune via `[ambient]` in config.

### New providers

OpenAI-format services can be registered with `harness route custom` and do not require a new crate. A native provider is appropriate only when protocol, authentication, streaming, or tool semantics differ:

1. Create `crates/harness-provider-<name>/` and implement the `Provider` trait (see `CLAUDE.md` → *Adding a new provider*).
2. Add a build arm and alphabetical preset in `crates/harness-provider-router/src/lib.rs`.
3. Add credential detection for status/setup display only. Detection must never select, rank, or append a provider.
4. Write mock-server construction tests plus an exact-route test.

Interesting native-adapter targets include providers whose APIs are not faithfully OpenAI-compatible. Keep routing policy in the user's config.

### New tools

See `CLAUDE.md` → *Adding a new tool*. The pattern is: implement `Tool` in `crates/harness-tools/src/tools/`, export it, register in `src/cli/wiring.rs`. Ideas:

- **`DatabaseTool`** — query SQLite or Postgres, return results as markdown tables
- **`NotebookTool`** — read/write Jupyter `.ipynb` cells
- **`DockerTool`** — list containers, exec, logs

(`GitTool` — structured git ops — already shipped.)

### Platform coverage

- **Windows:** shell tool prefers Git Bash, then **PowerShell**, then `cmd.exe` — see `crates/harness-tools/src/tools/shell.rs`.
- **VS Code extension** (`extensions/vscode/`): TCP fallback on native Windows (`~/.harness/daemon.port`); packaging/assets still MVP.
- **Tauri desktop app** (`apps/desktop/`): Windows/Linux packaging, tray icon behaviour, auto-update.

### Documentation and UX

- CDP troubleshooting guide and screenshots for the browser tool
- A cookbook of real-world prompts / session transcripts
- Translations of `Start Here/USER MANUAL.md`
- Review the `docs/INSTALL.md` WSL2 section against a fresh machine

### Coverage and property tests

The **≥ 60% line coverage gate** is configured on **pull requests only** via [`.github/workflows/coverage.yml`](.github/workflows/coverage.yml) — it is a **target**, not current measured coverage. Last measured baseline is in [`COVERAGE.md`](COVERAGE.md) (~23% lines). Local: `cargo llvm-cov --workspace --all-features --summary-only`.

Proptest/fuzzing targets:
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

CI runs `fmt`, `clippy --all-features`, `test --all`, `build --all-targets`, `build --profile release-lto`, plus `supply-chain` (audit/deny), `msrv`, and install-script smoke jobs on **Ubuntu**, **macOS**, and **Windows**. PRs need all jobs green before merge.

---

## Code style

- **Rust edition 2021**, stable toolchain.
- `cargo fmt` is enforced in CI — run it before pushing.
- `clippy -D warnings` is enforced — fix warnings rather than suppressing them without explanation.
- No unnecessary comments. If a comment is needed, explain *why*, not *what*.
- Prefer editing existing files to adding new abstractions. Keep diffs reviewable.

---

## Community

- **Issues** — bugs, feature requests, and **`good first issue`** candidates
- **Discussions** — setup help, workflows, provider tips (enable on GitHub if not already)
- **Discord / Matrix** — optional; not required for beta (see [`docs/PROMOTION_REPORT.md`](docs/PROMOTION_REPORT.md) Tier 3)

When opening a PR, link the issue if one exists. For new features, a short design note in the issue helps reviewers.

---

## Reporting bugs

Open an issue with:
- OS and Rust version (`rustc --version`)
- `harness --version`
- The command you ran and the full error output (redact API keys)

---

## License

By contributing, you agree that your contributions are assigned to **NextEleven LLC** and are governed by the project's proprietary [`LICENSE`](LICENSE). This is **not** an open-source / MIT project.

---

## Releasing

See [`docs/RELEASE_PROCESS.md`](docs/RELEASE_PROCESS.md) for how to cut new releases.

Only maintainers with write access should create releases.
