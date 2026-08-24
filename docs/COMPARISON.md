# Harness vs Other AI Coding Tools

High-level comparison for evaluators. The real test is your own workflow on your stack.

| Feature | Harness | Aider | Claude Code | Cursor |
|---------|---------|-------|-------------|--------|
| **Language** | Rust | Python | — | Electron |
| **Terminal-first** | Yes | Yes | Partial | No |
| **Local / privacy** | Strong (Ollama, MLX) | Strong | Medium | Weak |
| **Multi-provider** | 18 built-in names + custom OpenAI-format endpoints | Good | Limited | Limited |
| **Provider policy** | User chooses exact primary, models, and fallback order | Varies | Vendor-specific | Proprietary |
| **Sub-agents / parallel swarm** | Yes (SQLite registry) | Limited | No | No |
| **Cost tracking** | Built-in SQLite dashboard | Basic | No | No |
| **MCP support** | Yes (2025-03-26: tools, resources, sampling, roots) | No | No | Partial |
| **Browser automation** | Chrome CDP tool | No | No | No |
| **Cross-machine sync** | Age-encrypted git sync | No | No | No |
| **Editor integration** | VS Code extension + daemon (Unix socket / TCP) | No | VS Code plugin | IDE-native |
| **Desktop app** | Tauri 2 (macOS; CI check) | No | No | Yes |
| **Prebuilt binaries** | Yes (GitHub Releases: macOS, Linux, Windows) | No | No | N/A |
| **Open source** | No — proprietary public POC | Apache 2.0 | Proprietary | Proprietary |

## Key differentiators

- **Rust performance and safety** — Low memory footprint; no Python runtime required for the agent itself.
- **Safety-first approvals** — Auto, smart, and plan modes; explicit gates for destructive tools.
- **Local-first + hybrid memory** — Vector memory, project facts (`.harness/memory/`), ambient consolidation with configurable `[ambient]` providers.
- **Built-in observability** — Cost DB, optional OTLP traces, session export to Markdown.
- **Scriptability** — One-shot CLI, HTTP/SSE server, swarm CLI, GitHub PR review, bridges (Obsidian, Notes, Calendar).
- **Power-user tooling** — MCP client, browser CDP, voice (Whisper + Realtime API), diff review in plan mode.

## When Harness fits best

- You live in the terminal and want a fast, hackable agent you can extend in Rust.
- You switch between cloud providers (Claude, Grok, GPT) or run local models (Ollama, MLX on Apple Silicon).
- You care about spend visibility, session export, and encrypted cross-machine sync.
- You want MCP servers, sub-agents, or custom tools without leaving the CLI.

## When to consider alternatives

- **Cursor** — You want a full IDE with inline edits and are fine with a closed, Electron-based product.
- **Claude Code** — You are all-in on Anthropic and want their official VS Code integration.
- **Aider** — You prefer Python, git-centric pair programming, and already know that workflow.

This table is intentionally high-level and may lag vendor feature changes. See [`README.md`](../README.md) and [`CHANGELOG.md`](../CHANGELOG.md) for current Harness capabilities.
