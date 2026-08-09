# NextEleven Harness — Rust Coding Agent

[![CI](https://github.com/seanebones-lang/harness/actions/workflows/ci.yml/badge.svg)](https://github.com/seanebones-lang/harness/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.76%2B-orange)](rust-toolchain.toml)
[![Toolchain](https://img.shields.io/badge/pinned-1.95.0-blue)](rust-toolchain.toml)
[![Coverage](https://img.shields.io/badge/coverage-~62%25%20(gate%2060%25%20met)-brightgreen)](COVERAGE.md)
[![Version](https://img.shields.io/badge/version-0.1.2--beta-informational)](Cargo.toml)

**NextEleven Harness** is a terminal-native AI coding agent written in Rust by **NextEleven LLC**. It edits your repo with sandboxed tools, tracks cost and sessions, runs parallel swarm workers, speaks MCP, and can serve a local HTTP/SSE UI — multi-provider, multi-agent, local-first.

Default chat model: **claude-sonnet-4-6** (Anthropic). Smart router falls through **Anthropic → xAI → OpenAI → Mistral → Gemini → Bedrock → Ollama/MLX** based on configured keys and `[providers]` tables.

**Status:** public **beta** (daily-driver capable). **Stable** is blocked on full REL-01 smoke matrix + release artifact billing (see [`docs/CTO_BACKLOG.md`](docs/CTO_BACKLOG.md)).  
**Branch:** ship on **`main`** only.  
**License:** proprietary — NextEleven LLC ([`LICENSE`](LICENSE)). Not open source.

| Doc | Purpose |
|-----|---------|
| [`docs/INSTALL.md`](docs/INSTALL.md) | Per-OS install, PATH, FAQ |
| [`Start Here/USER MANUAL.md`](Start%20Here/USER%20MANUAL.md) | Plain-language first run |
| [`docs/COOKBOOK.md`](docs/COOKBOOK.md) | Worked prompts + tool recipes |
| [`docs/SHORTCUTS.md`](docs/SHORTCUTS.md) | TUI keys + slash + CLI |
| [`docs/CTO_BACKLOG.md`](docs/CTO_BACKLOG.md) | Ordered engineering backlog |
| [`docs/RELEASE_STATUS.md`](docs/RELEASE_STATUS.md) | Go / no-go log |
| [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md) | Trust boundaries (v2) |
| [`CLAUDE.md`](CLAUDE.md) | Developer map of the tree |
| [`COVERAGE.md`](COVERAGE.md) | Measured coverage SoT |

---

## What you get

- **Multi-provider streaming** — Anthropic (caching + thinking), OpenAI / OpenAI-compatible / Mistral, xAI Grok, Google Gemini (OpenAI-compat endpoint), AWS Bedrock Converse, local Ollama + MLX
- **Agentic tools** — `read_file` / `write_file` / `patch_file` / `apply_patch` / `list_dir` / `search_code` / `shell` / `git` / `gh` / `test_runner` / LSP (`find_definition`, …) / `spawn_agent` / `spawn_swarm`
- **Config-gated extras** (default **off**) — `database` (SQLite readonly), `notebook` (`.ipynb`), `docker` (allowlisted CLI), `browser` (Chrome CDP), `computer_use` (see [`docs/COMPUTER_USE.md`](docs/COMPUTER_USE.md))
- **Parallel swarm** — SQLite registry (`~/.harness/swarm.db`), CLI + TUI panel (F2 / `/swarm`), cancel-all, auto-GC, `--json`, worker tool allowlist + wall timeout, optional remote registry hook
- **Sessions + memory** — SQLite sessions, semantic recall, project memory (`.harness/memory/`), ambient consolidation
- **Plan mode** — `--plan` pauses destructive tools for y/n
- **Serve / daemon** — local HTTP+SSE (`harness serve`), Unix-socket daemon, collab WS when enabled
- **MCP** — tools + resources/roots CLI + inbound sampling approval (TUI y/n or auto)
- **Ops** — `doctor`, `cost`, `sync` (age-encrypted), `bench` (offline pack), `trace` / OTLP notes, bridges (Obsidian/Notes/Calendar/Projects)

---

## Prerequisites

- **Rust** via [rustup](https://rustup.rs) — pinned channel **1.95.0** in `rust-toolchain.toml`; MSRV **1.76** in workspace `Cargo.toml`
- **Git**
- **macOS / Linux / Windows** — CI runs fmt, clippy, test, build on all three

---

## Quick start

### macOS / Linux

```bash
git clone https://github.com/seanebones-lang/harness.git
cd harness
cargo build --profile release-lto
install -m 755 target/release-lto/harness ~/.local/bin/harness
export PATH="$HOME/.local/bin:$PATH"

export ANTHROPIC_API_KEY="sk-ant-..."   # or XAI_API_KEY / OPENAI_API_KEY / GEMINI_API_KEY / …
cd /path/to/your/project
harness init        # optional: seed ~/.harness/config.toml
harness             # interactive TUI
```

Installer script: [`scripts/install.sh`](scripts/install.sh).

### Windows (PowerShell)

```powershell
git clone https://github.com/seanebones-lang/harness.git
cd harness
cargo build --profile release-lto
New-Item -ItemType Directory -Force -Path "$HOME\.local\bin" | Out-Null
Copy-Item -Force .\target\release-lto\harness.exe "$HOME\.local\bin\harness.exe"
# add %USERPROFILE%\.local\bin to User PATH, new terminal

$env:ANTHROPIC_API_KEY = "sk-ant-..."
cd C:\path\to\your\project
harness
```

Installer: [`scripts/install.ps1`](scripts/install.ps1). Prefer **MSVC** toolchain + Git for Windows.

### One-shot & common commands

```bash
# Prefer freshly built binary while developing
./target/debug/harness --help

harness "summarize this crate layout"
harness --plan "refactor src/agent into modules"
harness --model grok-4.3 --think 8000 "design a migration"
harness --resume <session-id-prefix> "continue"

harness doctor
harness models
harness models --set anthropic:claude-opus-4-7
harness cost today

harness swarm run "audit auth" -n 3
harness swarm list
harness swarm status <id> --json
harness swarm gc --dry-run

harness mcp roots
harness mcp resources
harness bench                 # offline pack; no API keys
harness bench --json

harness serve --addr 127.0.0.1:8787
harness bridge obsidian "Title" "body"
harness completions zsh > ~/.zsh/completions/_harness
```

**PATH pitfall:** install under `~/.local/bin` can lag the tree. After CLI changes, smoke with **`./target/debug/harness`**.

---

## Providers & keys

| Provider | Typical env | Notes |
|----------|-------------|--------|
| Anthropic | `ANTHROPIC_API_KEY` | Default models; prompt cache + thinking |
| xAI | `XAI_API_KEY` | Grok 4.x flagship / fast |
| OpenAI | `OPENAI_API_KEY` | GPT-5.x family |
| Mistral | `MISTRAL_API_KEY` | OpenAI-compatible client |
| Gemini | `GEMINI_API_KEY` or `GOOGLE_API_KEY` | OpenAI-compat Generative Language API — [`docs/PROVIDERS_GEMINI_BEDROCK.md`](docs/PROVIDERS_GEMINI_BEDROCK.md) |
| Bedrock | `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` (+ region / `BEDROCK_MODEL_ID`) | Converse + SigV4 |
| Ollama | local daemon | Default `qwen3-coder:30b` |
| MLX | macOS Apple Silicon | `mlx_lm.server` OpenAI-compat |
| Generic | any OpenAI-format `base_url` | `[providers.*]` or `openai-compatible` kind |

```bash
export GEMINI_API_KEY=...
harness --model gemini-2.0-flash "ping"

export AWS_REGION=us-east-1
export BEDROCK_MODEL_ID=anthropic.claude-3-5-sonnet-20241022-v2:0
# AWS_* keys as usual
```

Router policy + catalogue tests live in `crates/harness-provider-router`.

---

## Optional tools (config-gated)

All default **disabled**. Enable in `~/.harness/config.toml` or project `.harness/config.toml`:

```toml
[tools.database]
enabled = true
readonly = true      # SELECT / WITH / PRAGMA / EXPLAIN only
max_rows = 500

[tools.notebook]
enabled = true

[tools.docker]
enabled = true
allow_mutating = false   # compose_up only if true
timeout_secs = 60

[computer_use]
# enabled = true         # DANGER: mouse/keyboard — Claude 4.x models; see docs/COMPUTER_USE.md

[browser]
# use CLI --browser or config; needs Chrome CDP — docs/BROWSER_CDP.md
```

Cookbook sections 14–16: [`docs/COOKBOOK.md`](docs/COOKBOOK.md).

### Swarm worker gates

```toml
[swarm]
max_concurrency = 4
# auto_gc_stale_secs = 86400
# worker_tool_allowlist = ["read_file", "list_dir", "search_code", "test_runner"]
# worker_max_wall_secs = 600
# registry_url = ""    # optional remote registry hook (see docs/WAVE7_SCALE.md)
```

TUI: **F2** or `/swarm` dumps swarm registry lines into the single-panel transcript (Hermes-style layout; no side panel).

---

## CLI surface (authoritative: `./target/debug/harness --help`)

| Area | Commands |
|------|----------|
| Chat | (default TUI), `run`, `--resume`, `--plan`, `--think`, `--image`, `--browser` |
| Sessions | `sessions`, `export`, `delete`, `undo`, `checkpoint` |
| Swarm | `swarm run\|list\|status\|result\|cancel\|wait\|gc` |
| MCP | `mcp resources\|roots\|read` |
| Ops | `doctor`, `status`, `init`, `setup`, `models`, `cost`, `update` |
| Memory | `memorize`, `forget`, `memories` |
| Network | `serve`, `connect`, `daemon`, `daemon-status` |
| Bridges | `bridge …` |
| Misc | `pr`, `voice`, `sync`, `trace`, `bench`, `trust` / `untrust`, `completions`, `self-dev`, `project` |

---

## Build, test, quality

```bash
cargo build
cargo build --profile release-lto
cargo test --bin harness          # 363 tests (2026-08-09 cont; no API keys)
cargo test -p harness-tools       # 179 tests (Swarm-51)
cargo test -p harness-provider-router
cargo clippy -p harness --bin harness -- -D warnings
cargo fmt --all -- --check

# Coverage SoT (badge = measured; CI fail-under 60% met)
cargo llvm-cov --workspace --all-features --summary-only
# Last measured: **61.65%** lines (2026-08-09 Swarm-51) — see COVERAGE.md

# Offline microbench pack
./target/debug/harness bench
cargo bench                       # criterion (memory search, JSON-RPC)

# Offline REL smoke helpers
bash scripts/smoke_rel01.sh
# bash scripts/smoke_linux_docker.sh   # needs Docker
```
Root package is a **binary** — use `cargo test --bin harness <filter>`, not `--lib`. One test filter only per invocation.

---

## Workspace layout

```
harness/
├── src/                    # binary: agent/, server/, tui/, swarm, bench, CLI
├── crates/
│   ├── harness-provider-*  # core, anthropic, openai, xai, ollama, mlx, gemini, bedrock, router
│   ├── harness-tools       # tool trait + builtins (+ database/notebook/docker)
│   ├── harness-memory      # sessions + vector memory
│   ├── harness-mcp         # MCP client
│   ├── harness-browser     # CDP
│   ├── harness-lsp / voice / term-graphics
├── config/default.toml
├── demo/                   # scenarios + bench_tasks pack + DEMO_SCRIPT
├── docs/                   # user + eng docs
├── apps/desktop            # Tauri shell
├── extensions/vscode
└── scripts/                # install, smoke, vendor, homebrew SHA
```

Developer narrative: [`CLAUDE.md`](CLAUDE.md) · architecture: [`ARCHITECTURE.md`](ARCHITECTURE.md).

---

## Security

- Workspace path jail (strict by default)
- Confirm gate / plan mode for destructive tools
- HTTP bearer auth on `serve`; daemon token on loopback socket
- MCP command allowlist; sampling approval path
- Optional tools off by default
- Threat model v2 + audit checklist: [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md)
- Report vulnerabilities per [`SECURITY.md`](SECURITY.md) — do not open public issues for sensitive reports

---

## Release posture

| Item | State |
|------|--------|
| Public beta | **GO** |
| Stable 0.2.0 | Blocked — REL-01 full OS smoke + prebuilt matrix |
| Coverage CI gate | **Met** — measured **61.65%** lines (llvm-cov 2026-08-09 Swarm-51); badge ~62% |
| Billing / full Release matrix | 📌 pinned (maintainer) |
| Branch | **`main`** |

Details: [`docs/RELEASE_STATUS.md`](docs/RELEASE_STATUS.md) · [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) · ordered work: [`docs/CTO_BACKLOG.md`](docs/CTO_BACKLOG.md).

---

## Contributing

1. Branch from **`main`**
2. `cargo fmt` · `cargo clippy -p harness --bin harness -- -D warnings` · `cargo test --bin harness`
3. Keep docs honest (coverage badge = measured; no vaporware flags)
4. See [`CONTRIBUTING.md`](CONTRIBUTING.md)

---

## Demo & evaluation

```bash
docker compose up                 # judge path + Ollama when configured
demo/DEMO_SCRIPT_5-10min.md       # doctor → one-shot → tools → swarm → gc
./target/debug/harness bench      # offline pack under demo/bench_tasks/
```

Competition / submission notes: [`docs/SUBMISSION_MANIFEST.md`](docs/SUBMISSION_MANIFEST.md), [`docs/TECHNICAL_REPORT.md`](docs/TECHNICAL_REPORT.md).

---

## License

**Proprietary — NextEleven LLC.** See [`LICENSE`](LICENSE). Unauthorized use, copying, distribution, or derivative works are prohibited.

---

## Links

- Issues / PRs: https://github.com/seanebones-lang/harness  
- Releases: https://github.com/seanebones-lang/harness/releases  
- Comparison notes: [`docs/COMPARISON.md`](docs/COMPARISON.md)
