# harness — Codebase Guide (May 2026)

Rust coding agent. Multi-provider (Anthropic Claude 4.x, xAI Grok 4.x, OpenAI GPT-5.x, Ollama Qwen3-Coder). Fast, low-memory, multi-agent.

**Release:** Public **beta** GO — **218 tests**, P0 security closed. **Stable** blocked on manual smoke §3 ([`TODO.md`](TODO.md) **REL-01**).

## Build & Test

```bash
cargo build                        # dev build
cargo build --profile selfdev      # fast self-modification build
cargo build --profile release-lto  # distribution build (thin LTO, stripped)
cargo test --all                   # workspace integration + crates (root + tests/* ; no API keys)
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all

# PR coverage gate (see .github/workflows/coverage.yml): ≥ 60% line coverage

# Optional: [.githooks/commit-msg](.githooks/commit-msg) drops `Co-authored-by`, `Co-developed-by`,
# and `Made-with:` trailer lines locally (avoid polluting attribution when IDE aids compose messages).
git config core.hooksPath .githooks
```

## Running

```bash
# Interactive TUI (claude-sonnet-4-6 is the default)
ANTHROPIC_API_KEY=sk-ant-... harness

# With extended thinking
ANTHROPIC_API_KEY=sk-ant-... harness --think 10000

# xAI Grok 4.3 (flagship; see https://docs.x.ai/docs/models )
XAI_API_KEY=xai-... harness --model grok-4.3

# One-shot
ANTHROPIC_API_KEY=sk-ant-... harness "refactor src/agent.rs to use a state machine"

# Resume a session
harness --resume abc12345 "continue where we left off"

# Export session transcript to Markdown
harness export abc12345
harness export abc12345 --output session.md

# Browser tool against Chrome DevTools Protocol (requires Chrome with debugging port — see `[browser]` in config/default.toml or use --browser flag)
ANTHROPIC_API_KEY=sk-ant-... harness --browser "navigate to example.com and take a screenshot"

# HTTP server
harness serve --addr 127.0.0.1:8787

# Cost dashboard
harness cost today
harness cost by-model

# Cross-machine sync
harness sync init git@github.com:user/harness-state.git
harness sync push

# Models picker
harness models
harness models --set anthropic:claude-opus-4-7

# GitHub PR review
harness pr 123

# Project memory
harness memorize architecture "monorepo: crates/ + src/"
harness memories

# Voice input (one-shot)
harness voice

# Real-time voice (duplex WebSocket)
harness voice --realtime

# Self-development mode (agent edits itself)
harness self-dev --src . --model claude-sonnet-4-6

# Diagnostics
harness doctor

# VS Code / editor integration (Unix socket on macOS/Linux; TCP on Windows)
harness daemon

# Shell completions
harness completions bash > ~/.bash_completion.d/harness
harness completions zsh > ~/.zsh/completions/_harness
harness completions fish > ~/.config/fish/completions/harness.fish

# Parallel swarm (SQLite registry)
harness swarm list
harness swarm status <task-id>
harness swarm result <task-id>
harness swarm cancel <task-id>
harness swarm wait <task-id>
harness swarm gc                 # reap orphans; --keep / --older-than-secs / --dry-run
# TUI: F2 or /swarm toggles the swarm panel

# External bridges (Obsidian, Notes, Calendar, GitHub Projects)
harness bridge obsidian "Title" "content"

# Observability traces
harness trace
harness trace <trace-id>

# Collaborative sessions (when server exposes collab — see server/collab wiring)
```

## Workspace layout

```
harness/
├── src/                            root binary
│   ├── main.rs                     entry (`tokio::main`), command dispatch
│   ├── cli/                        clap args (`args.rs`), tool wiring (`wiring.rs`)
│   ├── agent.rs                    core agentic loop + memory injection
│   ├── tui/                        two-panel ratatui TUI (`mod.rs`, `theme.rs`)
│   ├── highlight.rs                syntect → ratatui syntax highlighting
│   ├── server.rs                   axum HTTP/SSE server (harness serve)
│   ├── events.rs                   AgentEvent enum + channel helpers
│   ├── config.rs                   TOML config structs
│   ├── cost_db.rs                  SQLite cost tracking (~/.harness/cost.db)
│   ├── memory_project.rs           .harness/memory/ project facts
│   ├── sync.rs                     age-encrypted cross-machine git sync
│   ├── notifications.rs            desktop notification helpers (notify-rust), E16 rich kinds
│   ├── diff_review.rs              inline diff review + staging buffer (E4)
│   ├── observability.rs            OpenTelemetry tracing + OTLP export (E7)
│   ├── swarm.rs                    parallel sub-agent swarm + SQLite registry (E9)
│   ├── ambient.rs                  background memory consolidation (`AmbientProviders`, `[ambient]` config)
│   ├── daemon.rs                   editor IPC: Unix socket (macOS/Linux) or loopback TCP (Windows)
│   ├── bridges.rs                  Obsidian / Apple Notes / Calendar / GitHub Projects (E12)
│   └── collab.rs                   collaborative WebSocket sessions (E13)
├── crates/
│   ├── harness-provider-core/      Provider trait, Message/Delta/Tool types, ResponseSchema
│   ├── harness-provider-anthropic/ Claude Sonnet/Opus/Haiku + prompt caching + thinking
│   ├── harness-provider-openai/    GPT-5.x streaming SSE client + strict JSON schema
│   ├── harness-provider-xai/       Grok 4.x streaming + native tools + strict JSON schema
│   ├── harness-provider-ollama/    Local Ollama (Qwen3-Coder 30B default)
│   ├── harness-provider-mlx/       MLX via `mlx_lm.server` (macOS/aarch64; added in remediation)
│   ├── harness-provider-router/    Smart multi-provider router (env-key detection)
│   ├── harness-lsp/                LSP client integration
│   ├── harness-tools/              Tool trait + built-ins (shell/gh/computer/file/search)
│   ├── harness-memory/             SQLite session store + vector memory store
│   ├── harness-mcp/                MCP 2025-03-26 client: tools, resources, sampling, roots, progress (E8)
│   ├── harness-browser/            Chrome CDP browser tool
│   ├── harness-voice/              Whisper transcription + OpenAI Realtime API duplex (E5)
│   └── harness-term-graphics/      Inline image rendering (Kitty/iTerm2/Sixel, E6)
├── extensions/vscode/              VS Code extension (TypeScript, E14)
├── apps/desktop/                   Tauri 2 desktop shell (macOS .app, E15)
├── config/default.toml             Annotated default configuration
├── docs/BROWSER_CDP.md             Chrome CDP / browser tool setup
├── docs/COOKBOOK.md                Example prompts and tool patterns
├── docs/INSTALL.md                 Per-OS install guide
├── docs/PUBLIC_RELEASE.md          Release checklist
├── docs/RELEASE_STATUS.md          Latest go/no-go log
├── docs/SHORTCUTS.md               TUI keyboard shortcuts cheat sheet
├── docs/MIGRATION.md               Phase D→E breaking changes
├── docs/i18n/USER_MANUAL.es.md     Spanish manual (partial)
├── TODO.md                         Open backlog + recently completed items
├── CONTRIBUTING.md                 Contributor guide
├── tests/smoke_test.rs             Integration tests (no API key required)
└── scripts/install.sh              Install from source to ~/.local/bin
```

## May 2026 Model Defaults

| Provider  | Default model                    | Notes                         |
|-----------|----------------------------------|-------------------------------|
| Anthropic | `claude-sonnet-4-6`              | Prompt caching on by default  |
| OpenAI    | `gpt-5.5`                        | 1M context, Dec 2025 cutoff   |
| xAI       | `grok-4.3`                       | 1M context, native tools, flagship |
| Ollama    | `qwen3-coder:30b`                | Local, 256K ctx, SWE-bench RL |
| MLX       | `mlx-community/Qwen3-Coder-30B`  | Apple Silicon native          |
| Embed     | `nomic-embed-text` (Ollama)      | RAG default                   |

Smart router picks the best available provider based on which `*_API_KEY` env vars are set:
`ANTHROPIC_API_KEY` → `XAI_API_KEY` → `OPENAI_API_KEY` → `ollama` (local).

## Key types

### `harness-provider-core`

| Type | Description |
|------|-------------|
| `Provider` trait | `async fn stream_chat(req: ChatRequest) -> DeltaStream` |
| `Message` | `{role: Role, content: MessageContent, tool_call_id?}` |
| `Delta` | `Text(String)` \| `ToolCall(ToolCall)` \| `Done{stop_reason}` \| `CacheUsage{creation, read}` \| `Usage{input, output}` |
| `ToolDefinition` | OpenAI-format function schema |
| `ChatRequest` | Builder: `.with_messages()`, `.with_tools()`, `.with_system()`, `.thinking_budget`, `.native_web_search`, `.response_schema` |
| `ResponseSchema` | Strict JSON schema constraint (`name`, `schema: Value`, `strict: bool`) |

`ChatRequest` fields (May 2026):
- `thinking_budget: Option<u32>` — adaptive thinking budget (Anthropic)
- `native_web_search: bool` — enable provider-native web search
- `native_code_execution: bool` — enable sandboxed code execution
- `native_x_search: bool` — enable xAI X (Twitter) search
- `response_schema: Option<ResponseSchema>` — strict structured JSON output (E10)

### `harness-provider-anthropic`

Prompt caching: attaches `cache_control: {type: "ephemeral"}` to system, tools, pinned `@file` messages, and the second-to-last user message. Parses `cache_creation_input_tokens` / `cache_read_input_tokens` from SSE and emits `Delta::CacheUsage`.

Extended thinking: if `thinking_budget` > 0 on the request, sends `thinking: {type: "enabled", budget_tokens: N}` in the body + `anthropic-beta: interleaved-thinking-2025-05-14` header.

Structured output: injects a synthetic tool named `respond_<schema-name>` so the model is forced to call it with valid JSON matching the schema.

### `harness-provider-openai` / `harness-provider-xai`

Structured output: sets `response_format: {type: "json_schema", json_schema: {name, schema, strict: true}}` on the request when `response_schema` is set.

### `harness-provider-router`

Auto-builds providers from env keys when no `[providers]` block is configured. Smart routing:
- `default` → anthropic > xai > openai > ollama > mlx
- `fast` → same priority, uses fast/cheap model
- `heavy` → same priority, uses opus / grok-4.3 (when xAI is the heavy provider)
- `embed` → ollama:nomic-embed-text if available

### `harness-tools`

`Tool` trait: `fn definition() -> ToolDefinition` + `async fn execute(args: Value) -> Result<String>`.

Built-in tools:
- `ReadFileTool`, `WriteFileTool`, `ListDirTool`
- `PatchFileTool` — surgical old→new text replacement with diff output
- `ShellTool` — runs `sh -c <command>` on Unix; on Windows tries Git `sh`/`bash`, then **PowerShell**, then `cmd.exe /C`
- `SearchCodeTool` — regex over gitignore-aware file walk
- `GitTool` — structured git ops (`status`, `diff`, `log`, `commit`, …) — prefer over raw shell git
- `SpawnAgentTool` — runs a sub-agent with base tools only
- `RebuildSelfTool`, `ReloadSelfTool` — self-modification
- `GhTool` — `gh` CLI wrapper (pr_list, pr_view, pr_diff, pr_checks, pr_comment, issue_list, run_view, run_logs)
- `ComputerUseTool` — Anthropic computer-use-2025-01-24 spec (screenshot, mouse, keyboard) — only registered when `[computer_use] enabled = true`

### `harness-browser`

Chrome/Chromium automation over **Chrome DevTools Protocol**.

| Type | Description |
|------|-------------|
| `BrowserSession` | Connects to a running browser (`BrowserSession::connect(url)`); finds pages/targets (`find_or_open_target`). |
| `BrowserTool` | Provider-facing `Tool` (`name: "browser"`); exposes CDP actions via an `action` enum (navigate, screenshot, click, …). Lazily connects `BrowserSession` on first use. |

Requirements: Chrome (or Chromium) launched with `--remote-debugging-port=9222` (see `config/default.toml` `[browser]`). Configure the CDP endpoint in `[browser].url` or CLI `--browser-url` (defaults in `Cli` mirror local dev setups). User-facing setup: [`docs/BROWSER_CDP.md`](docs/BROWSER_CDP.md).

**Error semantics:** `BrowserTool::execute` returns `Err` when Chrome is unreachable or the action is unknown (validated before connect). Unit tests in `crates/harness-browser/` cover connect failures, unknown actions, and CDP JSON serde.

### `harness-voice`

`record_and_transcribe()` — captures audio via `sox rec` / `afrecord`, transcribes via OpenAI Whisper API or local `whisper-cli`. `WhisperBackend::detect()` picks the best available backend.

`RealtimeVoiceSession` — OpenAI Realtime API over WebSocket, bidirectional audio/text streaming (E5).

### `harness-memory`

`SessionStore` — SQLite WAL, sessions table. `save/load/find(prefix-or-name)/list`.
`MemoryStore` — memories table with JSON float embeddings. `insert/search(cosine-similarity, top-k)`.

`memory_project` module — `.harness/memory/<topic>.md` project facts. `remember/forget/list_topics/augment_system`.

### `cost_db`

`CostDb` — SQLite at `~/.harness/cost.db`. Schema: `(session_id, project, provider, model, ts, in_tok, cached_in, out_tok, native_calls, usd)`. `check_budget(db, daily_usd, monthly_usd)` returns `(Option<pct>, Option<pct>)`.

### `sync`

`init(git_url)` / `push()` / `pull()` / `status()` — age-encrypted sync of `~/.harness/{sessions.db, memory.db, trust.json, cost.db, memory/}` to a private git repo. Passphrase stored in macOS Keychain via `security` CLI, falls back to `~/.harness/.sync-key` (mode 0600).

### `notifications` (E16)

Rich notification system with kinds: `BackgroundDone`, `AutotestFailed`, `BudgetAlert`, `PrOpened`, `CiFailed`, `LongSubagentDone`, `VoiceResponseDone`, `SwarmComplete`, `DaemonDied`, `UpdateAvailable`.

macOS notifications include subtitle and group_id for grouping. Focus mode (`/focus N`) silences notifications for N minutes.

### `harness-mcp` (E8 — MCP 2025-03-26)

`McpClient::spawn(name, config)` — forks process, runs initialize handshake with full capabilities negotiation.

New MCP 2.0 features:
- `resources/list` + `resources/read` — fetch server-exposed resources
- `sampling/createMessage` — with user approval; approved requests run through the same `ArcProvider` used for chat (`load_mcp_tools(..., Some(provider))`)
- Roots advertisement in `initialize` (CWD + home)
- Progress notifications forwarded to `mpsc::UnboundedSender<ProgressEvent>`
- `ServerCapabilities` struct captures `has_resources`, `has_sampling`, `has_logging`, `has_prompts`, `protocol_version`

### `observability` (E7)

`ObservabilityConfig`, `Span`, `Tracer` — local JSONL traces at `~/.harness/traces/`. Optional OTLP/HTTP export.

### `swarm` (E9)

`TaskEntry`, `TaskStatus`, SQLite at `~/.harness/swarm.db`, `[swarm]` config (`max_concurrency`, `db_path`). CLI: `harness swarm run|list|status|result|cancel|wait|gc`. TUI: F2 / `/swarm` panel; `gc` reaps orphan pending/running tasks.

### `bridges` (E12)

`BridgesConfig` — Obsidian, Apple Notes, Calendar, GitHub Projects. CLI: `harness bridge …` when `[bridges.*]` enabled.

### `collab` (E13)

`CollabConfig`, `CollabEvent`, `CollabRegistry` — WebSocket at `/ws/session/:id?token=…` when `[collab].enabled` on `harness serve`.

### `diff_review` (E4)

`StagingBuffer`, `FileDiff`, `DiffHunk` — LCS hunks; plan-mode TUI overlay (`y`/`n` per hunk, `[`/`]` nav) for `write_file` / `patch_file`. Auto-trust via `~/.harness/diff-trust.toml`.

### `ambient` (`src/ambient.rs`)

Background memory consolidation after turns. Uses **`AmbientProviders`** (router **fast** route for summaries, **embed** route for vectors), **`AmbientConfig`** / **`[ambient]`** in config (interval, thresholds, optional `consolidation_model`), and **`consolidate()`** to merge new facts into vector memory. Embed failures are skipped rather than aborting the loop. Tests use **`FakeProvider`** in-process.

### `daemon` (`src/daemon.rs`)

Editor IPC for the VS Code extension. **macOS/Linux:** Unix domain socket at `~/.harness/daemon.sock` (default). **Windows:** loopback **TCP** with port written to `~/.harness/daemon.port`. `[daemon].transport` in config selects `auto` (platform default), `unix`, or `tcp`; Unix can force TCP via `transport = "tcp"`.

## Agent loop (`src/agent.rs`)

```
drive_agent(provider, tools, memory?, embed_model?, session, system_prompt, events?) -> Result<()>
  │
  ├─ build_augmented_system()   embed last user msg → cosine search → inject top-3 memories
  │                             + augment with .harness/memory/ project facts
  │
  └─ loop:
       stream_chat(req) → Delta stream
         TextChunk    → emit event, buffer text
         ToolCall     → collect into pending list
         Done         → break or set stop_reason
         CacheUsage   → emit AgentEvent::CacheUsage
         Usage        → emit AgentEvent::TokenUsage → record to cost.db
       push assistant message to session
       for each tool call:
         emit ToolStart
         executor.execute(call)
         emit ToolResult
         push tool_result message to session
       if no tool calls → break
```

New entry points:
- `drive_agent_with_options(…, thinking_budget)` — extended thinking
- `drive_agent_with_schema(…, thinking_budget, response_schema)` — structured output (E10)
- `drive_agent_full(…, native_web_search, native_code_execution, native_x_search, response_schema)` — all flags

## TUI layout (`src/tui/mod.rs`)

```
┌──────────────── Chat (62%) ──────────────────┬─── Tools & Events (38%) ──┐
│ ┌ [you]                                       │  → read_file              │
│ │ refactor the agent loop                     │  ← read_file: pub async f…│
│                                               │  → shell                  │
│ ┌ [claude] ●  (streaming, yellow)             │  ← shell: Build succeeded │
│ │ Here's the refactored version…              │  memory: recalled 2 entries│
├─────────────────────────────────────────────┴───────────────────────────┤
│  Message: _                                                               │
├───────────────────────────────────────────────────────────────────────────┤
│  Session abc · claude-sonnet-4-6 · 4 turns · $0.003 · cache:73%↑        │
└───────────────────────────────────────────────────────────────────────────┘
```

Status bar shows: session ID · model · turns · cost · cache hit rate · `[FOCUS Nm]` when in focus mode · `[COMPUTER USE LIVE]` / `[🎙 REC]` when active.

See `docs/SHORTCUTS.md` for full keyboard reference.

## HTTP API (`src/server.rs`)

```
POST /api/chat          body: {prompt, session_id?}   → SSE AgentEvent stream (Bearer auth)
GET  /api/sessions      → [{id, name, updated_at}]    (Bearer auth)
GET  /api/sessions/:id  → full Session JSON           (Bearer auth)
GET  /api/health        → {status, model, auth_token} (loopback bootstrap)
WS   /ws/session/:id    → collaborative events when `[collab].enabled` (Bearer token via `?token=`)
```

Protected routes require `Authorization: Bearer $(cat ~/.harness/server.token)`. See [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md).

SSE event types: `text_chunk`, `tool_start`, `tool_result`, `memory_recall`,
`cache_usage`, `token_usage`, `sub_agent_spawned`, `sub_agent_done`, `done`, `error`.

## Configuration (`~/.harness/config.toml`)

```toml
[provider]
api_key = "sk-ant-..."             # or ANTHROPIC_API_KEY env var
model = "claude-sonnet-4-6"        # current default
max_tokens = 8192

[memory]
enabled = true
embed_model = "nomic-embed-text"   # or voyage-3.5 (VOYAGE_API_KEY)

[approval]
mode = "auto"                      # auto | smart | plan (plan ≡ harness --plan in TUI)
# auto_approve = ["read_file"]
# always_ask = ["shell:git push"]

[budget]
daily_usd = 5.00
monthly_usd = 50.00

[notifications]
enabled = true
on_background_done = true
on_autotest_fail = true
on_budget = true

[native_tools]
web_search = false
code_execution = false
x_search = false

[browser]
enabled = false                    # CDP browser tool — also toggled via `harness --browser`
url = "http://127.0.0.1:9222"      # Chrome when started with --remote-debugging-port=9222

[computer_use]
enabled = false   # DANGER: only with claude-opus-4-7+

[agent]
system_prompt = "..."

[router]
default = "anthropic"
fast_model = "anthropic:claude-haiku-4-5"
heavy_model = "anthropic:claude-opus-4-7"
embed_model = "ollama:nomic-embed-text"
fallback = ["anthropic", "xai", "openai", "ollama"]

[observability]
enabled = true
traces_dir = "~/.harness/traces"
otlp_experimental_endpoint = ""   # optional base URL, e.g. "http://localhost:4318" (appends /v1/traces); legacy key: otlp_endpoint

[swarm]
max_concurrency = 4
db_path = "~/.harness/swarm.db"

[bridges]
obsidian_vault = ""            # e.g. "/Users/you/Documents/Obsidian/Vault"
apple_notes_folder = "Harness"
github_project_number = 0
github_owner = ""

[ambient]
# interval_secs = 300
# min_new = 5
# top_k = 20
# consolidation_model = "grok-4.1-fast"

[collab]
enabled = false
bind = "127.0.0.1:9090"

# Optional — commented in config/default.toml today:
# [daemon]
# transport = "auto"   # auto | unix | tcp
```

## Documentation map

| Doc | Purpose |
|-----|---------|
| [`README.md`](README.md) | Install, daily workflow, platform matrix |
| [`TODO.md`](TODO.md) | **Open backlog (severity-ranked) + roadmap** |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | How to contribute, CI gates |
| [`docs/PEER_REVIEW_AUDIT.md`](docs/PEER_REVIEW_AUDIT.md) | Security audit + remediation log |
| [`docs/INSTALL.md`](docs/INSTALL.md) | Per-OS install + troubleshooting |
| [`docs/BROWSER_CDP.md`](docs/BROWSER_CDP.md) | Browser tool setup |
| [`docs/COOKBOOK.md`](docs/COOKBOOK.md) | Example prompts |
| [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) | Release checklist + manual smoke §3 |
| [`docs/RELEASE_STATUS.md`](docs/RELEASE_STATUS.md) | Latest verification log |
| [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md) | HTTP/daemon/tool trust boundaries |

Public API crates enforce **`#![deny(missing_docs)]`**: `harness-provider-core`, `harness-tools`.

## Adding a new tool

1. Create `crates/harness-tools/src/tools/mytool.rs`, implement `Tool` trait.
2. Export from `crates/harness-tools/src/tools/mod.rs`.
3. Register in `build_tools()` in `src/cli/wiring.rs`.
4. Done — tool schema is automatically sent to the provider.

## Adding a new provider

1. Create `crates/harness-provider-<name>/`, implement `Provider` trait from `harness-provider-core`.
2. Add a new `build_provider` arm in `crates/harness-provider-router/src/lib.rs`.
3. Add env-key detection in the smart-defaults block of `ProviderRouter::from_config`.

## Self-dev mode

```bash
harness self-dev --src . --model claude-sonnet-4-6
```

The agent gets `RebuildSelfTool` and `ReloadSelfTool`. Workflow: read source → edit → `rebuild_self` (check_only=true for fast check) → fix errors → `rebuild_self` → `reload_self`.
On Unix, `reload_self` calls `exec()` to hot-swap the binary in-place.
