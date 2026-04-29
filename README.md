# Harness — Rust Coding Agent (April 2026)

Harness is a terminal-based AI coding assistant. It reads files, edits code, runs shell commands, searches your codebase, manages sessions with semantic memory, and can spawn sub-agents for parallel tasks.

Default model: **claude-sonnet-4-6** (Anthropic). Falls back to xAI → OpenAI → local Ollama based on which API keys are set.

---

## Quick Start (3 commands)

```bash
# 1. Build and install
git clone https://github.com/seanebones-lang/harness.git
cd harness
cargo build --profile release-lto
install -m 755 target/release-lto/harness ~/.local/bin/harness
export PATH="$HOME/.local/bin:$PATH"   # add to ~/.zshrc permanently

# 2. Set your API key (any of these work)
export ANTHROPIC_API_KEY="sk-ant-..."   # preferred — unlocks prompt caching + thinking
export XAI_API_KEY="xai-..."            # fallback
export OPENAI_API_KEY="sk-..."          # fallback

# 3. Run from any project
cd /path/to/your/project
harness
```

That's it. Harness auto-detects which API keys are set and picks the best available provider.

---

## April 2026 Model Lineup

| Provider  | Default model                    | Fast model                   | Heavy model          |
|-----------|----------------------------------|------------------------------|----------------------|
| Anthropic | `claude-sonnet-4-6` ($3/$15/M)   | `claude-haiku-4-5` ($1/$5/M) | `claude-opus-4-7` ($5/$25/M, thinking) |
| xAI       | `grok-4.20-0309-reasoning` ($2/$6/M) | `grok-4-1-fast-reasoning` ($0.20/$0.50/M) | same |
| OpenAI    | `gpt-5.5` ($5/$30/M)             | `gpt-5.4-mini` ($0.75/$4.50/M) | same |
| Ollama    | `qwen3-coder:30b` (local)        | same                         | same                 |

Switch models interactively:
```bash
harness models                                 # list all models
harness models --set anthropic:claude-opus-4-7 # switch to Opus for a project
```

---

## Daily workflow

```bash
cd my-project
harness                          # start a new session (TUI)
harness --resume abc12345        # continue a previous session
harness "explain what main.rs does"   # one-shot, no TUI
harness --plan                   # approve-mode: preview changes before they apply
harness --think 10000            # enable extended thinking with 10k token budget
harness status                   # show config, API key, recent sessions
```

---

## TUI keybindings

| Key       | Action                             |
|-----------|------------------------------------|
| `Enter`   | Send message                       |
| `↑ / ↓`   | Scroll chat                        |
| `PgUp/Dn` | Scroll event log (right panel)     |
| `Ctrl+V`  | Hold to voice-record, release to transcribe (Whisper) |
| `Ctrl+E`  | Fork mode — edit a past turn       |
| `Ctrl+C`  | Quit                               |

### Slash commands

| Command              | Effect                                      |
|----------------------|---------------------------------------------|
| `/think [N]`         | Enable extended thinking (N = token budget) |
| `/remember t: fact`  | Store fact under topic t in `.harness/memory/` |
| `/forget t`          | Delete memory topic t                       |
| `/memories`          | List all memory topics                      |
| `/pr [N]`            | List open PRs or load PR #N for review      |
| `/issues`            | List open GitHub issues                     |
| `/ci`                | Show recent CI workflow runs                |
| `/notify test`       | Send a test desktop notification            |
| `/cost`              | Show token usage + cost estimate            |
| `/model X`           | Switch model mid-session                    |
| `/runs`              | List background runs                        |
| `/help`              | Full command list                           |

---

## Per-project setup

```bash
cd my-project
harness init --project           # writes .harness/config.toml
harness                          # agent is now tuned for this repo
```

### Project memory

Store persistent facts about your project — they're injected into every session:

```bash
harness memorize architecture "Monorepo: crates/ + src/. Provider abstraction in harness-provider-core."
harness memorize tests "Run cargo test --workspace. Lints via cargo clippy."
harness memories                 # list all topics
harness forget architecture      # delete a topic
```

---

## Session management

```bash
harness sessions                 # list recent sessions
harness --resume <id> "continue" # resume by id prefix or name
harness export <id>              # print session as Markdown
harness delete <id>
```

---

## GitHub workflow

```bash
harness pr 123                   # load PR #123 diff + comments into agent session
harness pr 123 --comment "LGTM"  # post a review comment
```

Requires `gh` CLI installed and authenticated (`gh auth login`).

---

## Cost tracking

```bash
harness cost today               # today's spend
harness cost week                # last 7 days
harness cost by-model            # breakdown by model
harness cost watch               # live tail (refresh every 5s)
```

Set budget limits in `~/.harness/config.toml`:
```toml
[budget]
daily_usd = 5.00
monthly_usd = 50.00
```

Status bar turns yellow at 80%, red at 100%. Desktop notification fires at each threshold.

---

## Cross-machine sync

Encrypt and sync `~/.harness` state (sessions, memory, cost) to a private git repo:

```bash
harness sync init git@github.com:you/harness-state.git
harness sync push                # encrypt + push
harness sync pull                # pull + decrypt (on another machine)
harness sync status              # show recent syncs
```

Passphrase is stored in macOS Keychain (or `~/.harness/.sync-key` as fallback).

---

## Voice input

```bash
harness voice                    # record one-shot, print transcript
harness voice --send             # record + send to agent immediately
```

Or hold `Ctrl+V` in TUI to record; release to transcribe and insert.

Backends:
- **OpenAI Whisper** (`OPENAI_API_KEY` set) — uses `gpt-4o-transcribe`
- **Local** (`whisper-cli` on `$PATH`) — uses `whisper.cpp`

---

## Computer use (macOS, opt-in)

Enable in `~/.harness/config.toml`:
```toml
[computer_use]
enabled = true   # DANGER: agent can control mouse/keyboard
```

Requires `cliclick` (`brew install cliclick`). Only works with Claude Opus 4.7+.
TUI shows a red `[COMPUTER USE LIVE]` banner when active.

---

## Web UI (optional)

```bash
harness serve --addr 127.0.0.1:8787
# then open http://127.0.0.1:8787
```

---

## MCP tools

Create `.harness/mcp.json` in your project or `~/.harness/mcp.json` globally:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user"],
      "env": {}
    }
  }
}
```

---

## Configuration

Config is loaded from `.harness/config.toml` (project) then `~/.harness/config.toml` (global).
Full annotated reference: [`config/default.toml`](config/default.toml).

Key settings:

```toml
[provider]
api_key = "sk-ant-..."          # or set ANTHROPIC_API_KEY env var
model = "claude-sonnet-4-6"     # default model

[budget]
daily_usd = 5.00
monthly_usd = 50.00

[notifications]
enabled = true

[native_tools]
web_search = true               # provider-native web search

[computer_use]
enabled = false                 # DANGER when true

[memory]
enabled = true

[agent]
system_prompt = "..."           # customize agent persona
```

---

## Architecture

```
src/main.rs              CLI, subcommands, tool wiring
src/agent.rs             core agentic loop + memory injection
src/tui.rs               ratatui two-panel TUI
src/server.rs            axum HTTP/SSE server
src/cost_db.rs           SQLite cost tracking
src/memory_project.rs    .harness/memory/ project facts
src/sync.rs              age-encrypted cross-machine sync
src/notifications.rs     desktop notification hooks
crates/
  harness-provider-anthropic/   Claude Sonnet/Opus/Haiku + prompt caching
  harness-provider-openai/      GPT-5.x
  harness-provider-xai/         Grok 4.x
  harness-provider-ollama/      Local Ollama (Qwen3-Coder)
  harness-provider-router/      Smart multi-provider router with env-key detection
  harness-provider-core/        Shared types (ChatRequest, Delta, Provider trait)
  harness-tools/                Tool trait + shell/gh/computer/file/search/patch/spawn
  harness-memory/               SQLite session store + vector memory
  harness-mcp/                  MCP stdio protocol client
  harness-browser/              Chrome CDP browser tool
  harness-voice/                Whisper audio transcription (OpenAI + local)
```

For a developer deep-dive see [`CLAUDE.md`](CLAUDE.md).

---

## License

MIT
