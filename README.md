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
harness doctor                   # health checks (keys, daemon, MCP paths, …)
harness completions zsh          # print completions → save to your shell's completion dir
harness swarm list               # parallel sub-agent tasks
harness swarm status <task-id>   # one task
harness swarm result <task-id>   # output when done
harness trace                    # list spans from last local trace (needs observability)
```

---

## TUI keybindings

Full cheat sheet: [`docs/SHORTCUTS.md`](docs/SHORTCUTS.md). Phase E highlights:

| Key       | Action                             |
|-----------|------------------------------------|
| `Enter`   | Send message                       |
| `Shift+Enter` / `Alt+Enter` | Newline in input           |
| `↑ / ↓`   | Scroll chat / history navigation   |
| `PgUp/Dn` | Scroll event log (right panel)     |
| `Ctrl+S`  | Voice record (Whisper) — **Phase E** (was Ctrl+V in Phase D) |
| `Ctrl+Y`  | Copy last assistant reply          |
| `Ctrl+F`  | Search chat                        |
| `Ctrl+L`  | Jump chat to bottom                |
| `Ctrl+]` / `Ctrl+[` | Resize right panel           |
| `Ctrl+E`  | Fork mode — branch from a past turn |
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
| `/focus [N]`         | Pomodoro: silence notifications N min (default 25) |
| `/schema …`          | Strict JSON output schema (see docs)        |
| `/obsidian save`     | Save reply to Obsidian (when configured)    |
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

### Project lifecycle commands

Manage linked repos from one command surface:

```bash
harness proj ls                              # alias: harness project list
harness project dashboard                    # all projects: branch, ahead/behind, dirty state
harness project status my-project            # one project health view
harness project sync my-project              # fetch + ff-only pull
harness project sync --all                   # bulk sync every linked project
harness project push my-project              # push current branch (safe defaults)
harness project exec my-project -- cargo test
```

Create/publish flows:

```bash
harness project init my-new-app              # mkdir + git init + auto-link
harness project publish my-new-app --private # gh repo create + remote wiring
harness project clone git@github.com:you/repo.git
harness project import --root ~/code --recursive
harness project prune                         # drop missing paths from registry
```

Helpful aliases:
- `harness proj ...` = `harness project ...`
- `ls`, `dash`, `st`, `up`, `pub`, `run`, `ship`, `rm`, `scan`, `clean`, `new`, `link`, `cl`

### Quickstart recipes

1) **Start a brand-new app**

```bash
harness project init my-new-app
harness project exec my-new-app -- git add .
harness project exec my-new-app -- git commit -m "chore: initial scaffold"
harness project publish my-new-app --private --push
```

2) **Onboard existing local repos**

```bash
harness project import --root ~/code --recursive
harness project dashboard
harness project status some-repo
```

3) **Daily update + push loop**

```bash
harness proj up --all              # pull latest safely for every linked repo
harness project dashboard          # check what needs attention
harness project exec my-app -- npm test
harness project pub my-app         # alias for `project push`
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
harness voice --realtime         # OpenAI Realtime API duplex (requires OPENAI_API_KEY)
```

In the TUI use **`Ctrl+S`** for push-to-talk style recording (see [`docs/MIGRATION.md`](docs/MIGRATION.md) if you still use Ctrl+V).

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

## Desktop app (Tauri 2, optional)

Wraps the same UI in a native window with tray icon and **Cmd+Shift+H** (Ctrl+Shift+H on Linux/Windows) to show/hide.

```bash
cd apps/desktop
npm install
npm run dev      # development
npm run build    # release bundle
```

Requires `harness` on `PATH` for auto-spawn of `harness daemon`. See [`apps/desktop/README.md`](apps/desktop/README.md).

---

## VS Code extension (optional)

`extensions/vscode/` — side-panel chat and inline edit against the harness daemon (Unix socket). Install dependencies with `npm install`, then **Run Extension** from VS Code or package with `vsce`.

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

# Optional Phase E blocks — see `config/default.toml` (**commented** templates; do not
# paste raw `[observability]` tables until those structs exist in harness `Config`).
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
  harness-voice/                Whisper audio transcription + Realtime API duplex
  harness-term-graphics/        Inline terminal images (Kitty / iTerm2 / Sixel)
extensions/vscode/             VS Code integration (MVP)
apps/desktop/                  Tauri 2 native shell
```

For a developer deep-dive see [`CLAUDE.md`](CLAUDE.md). User-facing migration notes: [`docs/MIGRATION.md`](docs/MIGRATION.md).

---

## License

MIT
