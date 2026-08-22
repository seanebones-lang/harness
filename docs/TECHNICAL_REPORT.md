# Harness: A Multi-Provider Rust Coding Agent with Semantic Memory and Parallel Execution

**Technical Report — International Engineering Competition Submission**
*May 2026*

---

## 1. Abstract

Large language model (LLM) coding agents have emerged as productivity multipliers for software engineers, yet existing solutions suffer from one or more critical deficiencies: they are locked to a single LLM provider, consume excessive memory due to runtime overhead, exhibit startup latency incompatible with interactive workflows, or provide insufficient tool composability. Harness addresses these gaps with a public-beta, proprietary source-available coding agent written in Rust. The system implements a trait-based provider abstraction supporting Anthropic Claude 4.x, xAI Grok 4.x, OpenAI GPT-5.x, local Ollama (Qwen3-Coder), and Apple Silicon MLX backends under a single unified interface. A 14-crate Cargo workspace separates concerns cleanly while enabling thin-LTO release builds that start in under 100 milliseconds. The agent loop incorporates cosine-similarity semantic memory retrieval, plan-mode diff review with LCS-based hunks, and a parallel sub-agent swarm backed by SQLite. Full MCP 2025-03-26 protocol support — including sampling, resources, roots advertisement, and progress notifications — enables deep interoperability with any compliant tool server. At this report's May 2026 snapshot, the test suite comprised 218 tests exercised on Ubuntu, macOS, and Windows in continuous integration, with a line coverage gate of at least 60 percent. A P0 security audit was completed with all seven findings closed before public beta release. Current status and gate results are recorded in `docs/RELEASE_STATUS.md`.

---

## 2. Motivation and Problem Statement

### 2.1 The Current Landscape

The software development tooling space has converged on several AI-assisted coding products, each with meaningful limitations:

**Aider** is a mature open-source tool with strong git integration, but it is architecturally single-provider (primarily OpenAI-format APIs) and lacks semantic memory: every session starts cold. Its Python runtime adds measurable overhead and makes it unsuitable as a subprocess invoked from other tools.

**Claude Code** (Anthropic's official CLI) is tightly coupled to the Anthropic API. It offers no local model support, no cross-machine encrypted state sync, and no parallel sub-agent execution model. Engineers working in air-gapped environments or on cost-sensitive workloads cannot use it effectively.

**Cursor and Windsurf** are IDE-embedded tools that require the Visual Studio Code ecosystem. They are not composable as CLI tools, cannot be invoked headlessly in CI pipelines, and offer no programmatic API for integration into larger workflows.

**Devin** and similar autonomous agent products are hosted SaaS offerings that transmit full codebases to remote infrastructure. This is incompatible with proprietary code requirements and introduces network latency into every tool call.

### 2.2 The Gap Harness Fills

The common thread across these tools is that none simultaneously satisfies: (1) multi-provider portability, (2) low-latency startup for interactive and scripted use, (3) persistent semantic memory across sessions, (4) parallel sub-agent execution, (5) local-first operation with optional cloud providers, and (6) a composable extension model (MCP). NextEleven Harness is designed around all six properties.

---

## 3. System Architecture

### 3.1 Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          harness binary (src/)                          │
│  CLI (clap) ──► Agent Loop ──► Provider Router ──► Provider Crates      │
│      │               │                                    │             │
│      │         Memory Pipeline                   Anthropic / xAI /      │
│      │         (embed → cosine → inject)         OpenAI / Ollama / MLX  │
│      │               │                                                  │
│      ├── TUI (ratatui) ◄── AgentEvent channel                           │
│      ├── HTTP/SSE server (axum)                                         │
│      ├── Swarm (SQLite + Semaphore)                                     │
│      └── Cost DB (SQLite)                                               │
│                                                                         │
│  crates/                                                                │
│  ├── harness-provider-core    (Provider trait, Delta, ChatRequest)      │
│  ├── harness-provider-*       (one crate per backend)                   │
│  ├── harness-provider-router  (env-key auto-detection, routing)         │
│  ├── harness-tools            (Tool trait + built-ins)                  │
│  ├── harness-memory           (SQLite session + vector store)           │
│  ├── harness-mcp              (MCP 2025-03-26 full protocol)            │
│  ├── harness-browser          (Chrome CDP automation)                   │
│  ├── harness-voice            (Whisper + Realtime API)                  │
│  ├── harness-lsp              (LSP client integration)                  │
│  └── harness-term-graphics    (Kitty/iTerm2/Sixel inline images)        │
└─────────────────────────────────────────────────────────────────────────┘
```

### 3.2 The 14-Crate Workspace

The workspace separates the provider abstraction, tool execution, memory, and protocol layers into independent crates with well-defined dependency edges. This achieves two goals: compile-time isolation (a bug in `harness-voice` cannot introduce undefined behavior in `harness-provider-core`) and selective compilation (downstream embedders can depend only on what they need).

**`harness-provider-core`** defines the canonical `Provider` trait, `ChatRequest` builder, `Delta` stream enum, `Message`, and `ResponseSchema`. All other provider crates depend only on this crate, never on each other.

**`harness-provider-router`** inspects the environment at runtime (checking `ANTHROPIC_API_KEY`, `XAI_API_KEY`, `OPENAI_API_KEY`, and Ollama reachability) and constructs the best available provider chain, with configurable fallback ordering.

**`harness-tools`** defines the `Tool` trait (`definition() -> ToolDefinition` plus `async fn execute(args: Value) -> Result<String>`) and ships all built-in tools. The `ToolDefinition` uses OpenAI-format JSON Schema, which is compatible with all supported providers.

**`harness-memory`** manages two SQLite databases: `sessions.db` for conversation history (WAL mode for concurrent readers) and `memory.db` for vector embeddings stored as JSON float arrays.

**`harness-mcp`** implements the full MCP 2025-03-26 specification as a stdio client, covering tools, resources, sampling, roots, and progress notifications.

### 3.3 Agent Loop

The core loop in `src/agent.rs` operates as follows:

```
drive_agent(provider, tools, memory?, session, system_prompt) -> Result<()>
  │
  ├─ build_augmented_system():
  │     embed(last_user_message) → cosine_search(top_k=3) → prepend_to_system
  │     + load .harness/memory/<topic>.md files → append project facts
  │
  └─ loop:
       req = ChatRequest::new(messages, tools, augmented_system)
       stream = provider.stream_chat(req).await
       for delta in stream:
         Delta::Text(s)         → buffer, emit AgentEvent::TextChunk
         Delta::ToolCall(tc)    → push to pending_calls
         Delta::Done            → record stop_reason, break
         Delta::CacheUsage      → emit AgentEvent::CacheUsage
         Delta::Usage           → record to cost.db, emit TokenUsage
       session.push(assistant_message)
       for call in pending_calls:
         emit AgentEvent::ToolStart
         result = tool_executor.execute(call)
         emit AgentEvent::ToolResult
         session.push(tool_result_message)
       if pending_calls.is_empty() → break
```

This loop is intentionally provider-agnostic. The `Delta` enum abstracts over SSE frames (Anthropic, OpenAI), chunked JSON (Ollama), and structured tool-call events.

### 3.4 Tool Execution Pipeline

Tools are registered at startup via `build_tools()` in `src/cli/wiring.rs`. Each tool's `ToolDefinition` is serialized to JSON Schema and transmitted to the provider in the `ChatRequest`. When the provider returns a `Delta::ToolCall`, the executor dispatches to the matching tool by name using a `HashMap<String, Arc<dyn Tool>>`. Results are UTF-8 strings returned to the session as tool-result messages.

MCP tools are loaded via `harness-mcp` and wrapped in a `McpToolAdapter` that implements the same `Tool` trait, making them indistinguishable from built-in tools from the agent loop's perspective.

### 3.5 Memory Pipeline

On each agent turn, `build_augmented_system()` embeds the user's message using the configured embedding model (`nomic-embed-text` via Ollama by default, or Voyage 3.5 with a `VOYAGE_API_KEY`). The resulting vector is compared against all stored memories using cosine similarity, and the top-three results are prepended to the system prompt. Separately, project memory files under `.harness/memory/` are appended verbatim, providing persistent project-level context that survives embedding failures.

Background memory consolidation (`src/ambient.rs`) runs after each turn: when the new-memory count exceeds a configured threshold, the ambient consolidation loop summarizes recent entries using the router's fast model and merges them into the vector store.

---

## 4. Novel Contributions

### 4.1 Full MCP 2025-03-26 Protocol Implementation

Most MCP clients in 2025-2026 implement only the tools subset of the protocol. `harness-mcp` implements the complete 2025-03-26 specification: `tools/list` and `tools/call`, `resources/list` and `resources/read`, `sampling/createMessage` with user-approval gating (approved requests are forwarded through the same `ArcProvider` used for chat), `initialize` with roots advertisement (CWD and home directory), and progress notifications forwarded to a `tokio::sync::mpsc::UnboundedSender<ProgressEvent>`. The `ServerCapabilities` struct captures all optional capability flags at handshake time.

### 4.2 Age-Encrypted Cross-Machine Sync

The `sync` module (`src/sync.rs`) synchronizes the full `~/.harness` state — sessions, vector memory, cost database, trust configuration, and project facts — across machines using a private git repository as transport. All content is encrypted with `age` before commit. The passphrase is stored in the macOS Keychain via the `security` CLI on supported platforms and falls back to a mode-0600 file on Linux and Windows. This design allows a developer to resume any session on any machine without transmitting plaintext conversation history to a third-party service.

### 4.3 Adaptive Thinking Budget

For Anthropic providers, `ChatRequest.thinking_budget` triggers extended thinking mode: the serialized request includes `thinking: {type: "enabled", budget_tokens: N}` and the `anthropic-beta: interleaved-thinking-2025-05-14` header. The TUI exposes this as `--think N` on the CLI and `/think N` as a slash command, allowing per-session budget tuning. Thinking tokens are counted separately in the cost ledger via `Delta::CacheUsage`.

### 4.4 Ambient Memory Consolidation

`AmbientProviders` (in `src/ambient.rs`) runs as a background tokio task, waking on a configurable interval (default 300 seconds) or when the pending-memory count crosses a threshold. It summarizes new memory entries using the router's fast model, embeds the summary, and writes it to the vector store with a `__consolidated__` tag. This prevents unbounded growth of the memories table while preserving semantically compressed history. The embed path degrades gracefully: failures are logged and skipped, so a missing Ollama instance does not abort the agent loop.

### 4.5 Parallel Sub-Agent Swarm with SQLite Registry

The swarm subsystem (`src/swarm.rs`) maintains a task registry in `~/.harness/swarm.db` with schema `(id, prompt, status, result, created_at, updated_at)`. `harness swarm run` enqueues a task; a `tokio::sync::Semaphore` gate limits concurrent agent executions to the configured `max_concurrency` (default 4). Each sub-agent runs `drive_agent` with the base tool set in an isolated tokio task. Status transitions (`Queued → Running → Done | Failed`) are written atomically via SQLite transactions. This gives users a persistent, inspectable background compute queue without requiring an external job scheduler.

### 4.6 Plan-Mode Diff Review with LCS Hunks

`diff_review.rs` implements a `StagingBuffer` that intercepts writes from `WriteFileTool` and `PatchFileTool` when the agent is invoked with `--plan`. Diffs are computed using the Longest Common Subsequence algorithm, producing typed `DiffHunk` values (`Added`, `Removed`, `Context`). The TUI overlays a diff viewer where the user navigates hunks with `[`/`]` and approves or rejects with `y`/`n`. Auto-trust patterns in `~/.harness/diff-trust.toml` allow glob-based bypass for known-safe paths (e.g., test fixtures).

### 4.7 Multi-Provider Router with Environment-Key Auto-Detection

`harness-provider-router` eliminates configuration boilerplate by inspecting environment variables at startup. The priority chain — `ANTHROPIC_API_KEY` → `XAI_API_KEY` → `OPENAI_API_KEY` → Ollama (reachability probe) → MLX (platform check) — means a binary drop-in on any machine picks the best available provider without touching a config file. The router supports three named routes: `default`, `fast` (cost-optimized models), and `heavy` (highest-capability models), and an `embed` route for vector operations. Routes are resolved at request time, allowing the agent loop and ambient consolidation to use different cost/capability trade-offs from the same binary.

---

## 5. Implementation Details

### 5.1 SSE Delta Parsing

The Anthropic and OpenAI SSE streams differ significantly in event shape. `harness-provider-anthropic` parses `content_block_delta` events of type `text_delta` and `input_json_delta` (for tool calls), assembling partial JSON across frames. `harness-provider-openai` and `harness-provider-xai` parse the OpenAI-format `choices[0].delta` structure. Both normalize to the same `Delta` enum before returning from `stream_chat`. This normalization is what makes the agent loop fully provider-agnostic.

### 5.2 Cosine Memory Search

Memory retrieval in `harness-memory` stores embeddings as `Vec<f32>` serialized to JSON in SQLite. At query time, all stored vectors are loaded, and cosine similarity is computed:

```
similarity(a, b) = dot(a, b) / (norm(a) * norm(b))
```

The top-k results by similarity are returned. This is a linear scan over the memories table, which is acceptable for personal-scale memory stores (typically hundreds to low thousands of entries). Future work includes approximate nearest-neighbor indexing.

### 5.3 LCS Diff Algorithm

`DiffHunk` computation in `diff_review.rs` uses a standard dynamic-programming LCS over lines. The DP table is built in O(mn) time and O(mn) space for files of m and n lines. For large files the diff is truncated at a configurable line limit to keep the TUI overlay responsive. The LCS backbone is then annotated with `Added` and `Removed` spans to produce the final hunk list.

### 5.4 Swarm Scheduling with SQLite

Task state is managed with SQLite WAL mode and explicit `BEGIN IMMEDIATE` transactions to prevent lost updates under concurrent tokio tasks. The semaphore acquisition happens before the status transition to `Running`, so a task is never marked running if the slot could not be obtained. Crashed sub-agents (panics, OOM) are caught via `tokio::task::JoinHandle` error propagation and written as `Failed` with the error message stored in the result column.

---

## 6. Evaluation

### 6.1 Test Coverage

At this report's May 2026 snapshot, the workspace shipped 218 tests across four test files and per-crate unit tests:

| File | Focus |
|------|-------|
| `tests/smoke_test.rs` | Agent loop, memory, session persistence, tool execution, MCP, swarm, cost tracking |
| `tests/cli_smoke_test.rs` | CLI argument parsing, subcommand dispatch, one-shot mode |
| `tests/sandbox_tests.rs` | Tool sandboxing, shell execution safety, file access boundaries |
| `tests/error_handling_tests.rs` | Provider errors, network failures, malformed responses |

All tests run without API keys using mock providers and in-process fakes. CI executes the full suite on Ubuntu 22.04, macOS 14, and Windows Server 2022. A line coverage gate of at least 60% is enforced on every pull request via `cargo llvm-cov`.

### 6.2 Performance

Release-LTO builds (`cargo build --profile release-lto`) use thin LTO and symbol stripping, producing a single statically-linked binary (on musl targets) of approximately 12–15 MB. Startup time from binary invocation to first TUI frame is under 100 milliseconds on commodity hardware, measured as the time from `execv` to the first `ratatui` render frame. This is substantially lower than Python-based alternatives, which incur interpreter startup and import costs.

Memory footprint during a typical interactive session (no active vector search) is under 30 MB RSS. Peak memory during cosine search over a 1,000-entry memory store is approximately 45 MB, dominated by the embedding vectors loaded into process memory for scoring.

### 6.3 Security Audit

A pre-release P0 security audit was conducted, identifying seven findings across the HTTP server, daemon IPC, sync encryption, and tool execution boundary. All seven findings were resolved before the public beta:

- Bearer token authentication added to all protected HTTP routes
- Unix socket permissions hardened to mode 0600
- Sync passphrase file permissions enforced at write time (mode 0600)
- Shell tool timeout enforced to prevent runaway subprocess loops
- MCP sampling gated behind explicit user approval
- Computer use tool gated behind a hard config flag (`[computer_use] enabled = true`)
- `SpawnAgentTool` sandboxed to base tools only, preventing tool escalation

---

## 7. Limitations and Future Work

**MCP sampling TUI integration:** The `sampling/createMessage` approval flow currently requires a terminal prompt; a dedicated TUI overlay for sampling approvals is planned to provide a richer approval experience with message preview.

**OTLP export coverage:** The OpenTelemetry export path (`otlp_experimental_endpoint`) is tested with mock HTTP responses. Integration tests against a real OTLP collector (e.g., OpenTelemetry Collector in Docker) are not yet part of CI.

**Voice and MLX test depth:** The `harness-voice` and `harness-provider-mlx` crates have unit tests for error paths but lack integration tests that exercise the full audio capture and model inference pipelines, which require hardware (microphone, Apple Silicon GPU).

**Linear memory scan:** Cosine search is a full table scan. For users with tens of thousands of memory entries, approximate nearest-neighbor indexing (e.g., via a SQLite extension or a dedicated vector DB) would improve search latency.

**i18n partial:** A Spanish user manual (`docs/i18n/USER_MANUAL.es.md`) is included but covers only approximately 60% of the full English manual. Additional language coverage depends on community contribution.

**Session title lag:** Auto-naming of sessions is asynchronous; `harness sessions` immediately after a new session may show an untitled entry before the rename completes.

---

## 8. References

1. Anthropic. *Model Context Protocol 2025-03-26 Specification*. https://spec.modelcontextprotocol.io/specification/2025-03-26/ (2025).
2. Matsakis, N. and Klock, F. *The Rust Programming Language*. ACM SIGPLAN Notices, 49(10), 103–104 (2014).
3. Tokio Contributors. *Tokio: An Asynchronous Rust Runtime*. https://tokio.rs (2024).
4. SQLite Authors. *SQLite WAL Mode*. https://www.sqlite.org/wal.html (2024).
5. Filippo Valsorda. *age: A Simple, Modern File Encryption Tool*. https://age-encryption.org/v1 (2021).
6. Johnson, W.B. and Lindenstrauss, J. *Extensions of Lipschitz Mappings into a Hilbert Space*. Contemporary Mathematics, 26, 189–206 (1984). [Foundational result for cosine similarity in high-dimensional spaces.]
7. Myers, E.W. *An O(ND) Difference Algorithm and Its Variations*. Algorithmica, 1(2), 251–266 (1986). [LCS/diff algorithm basis.]
8. Anthropic. *Extended Thinking: Interleaved Thinking Beta (2025-05-14)*. Anthropic API Documentation (2025).
9. OpenTelemetry Authors. *OpenTelemetry Protocol (OTLP) Specification*. https://opentelemetry.io/docs/specs/otlp/ (2024).
10. Tauri Contributors. *Tauri 2.0: Build Desktop Apps with Web Technologies and Rust*. https://tauri.app (2024).
