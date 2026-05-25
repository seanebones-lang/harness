# Release notes — v0.1.2-beta (draft)

Use this body when creating the GitHub Release after tagging. Update the date and test counts if they change at cut time.

---

## Harness v0.1.2-beta — Public beta polish

**Harness** is a fast, terminal-first Rust coding agent with multi-provider support, semantic memory, MCP, cost tracking, and safety-first approvals.

### Highlights

- **Router-aware ambient memory** — `[ambient]` config; fast model for summaries, embed model for vectors (`AmbientProviders`)
- **TUI model label fix** — Chat labels and status bar stay in sync with the live provider; in-session `/model` switches rejected with clear guidance
- **MIT Round 2 hardening** — GitHub Projects bridge stdin safety, AppleScript escaping, health endpoint loopback gate, mutex fail-closed paths, 218 tests
- **CI & install** — `smoke-rel01` job, Windows prebuilt in `install.ps1`, release checksum hardening

### Install

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.sh | bash
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.ps1 | iex
```

Or download prebuilt binaries from [GitHub Releases](https://github.com/seanebones-lang/harness/releases).

### Configuration

Set one API key (router priority: Anthropic → xAI → OpenAI → Ollama → MLX):

```bash
export ANTHROPIC_API_KEY=sk-ant-...
# or XAI_API_KEY, OPENAI_API_KEY
harness
```

First-run wizard: `harness setup` or just `harness`.

### What's new since v0.1.1-beta

#### Added
- `[ambient]` config section and `AmbientProviders` (summary vs embed split)
- `build_router()` / `build_ambient_providers()` in provider wiring
- Ambient consolidation unit tests (split-provider + config mapping)
- Promotion docs: `docs/PROMOTION_REPORT.md`, refreshed `COMPARISON.md`

#### Fixed
- TUI assistant label drift when router default differed from `[provider].model`
- Grok 4.1 Fast model slug (`grok-4.1-fast`)
- Round 2 security and robustness items (see CHANGELOG Unreleased)

#### Changed
- CONTRIBUTING: contribution pathways, good-first-issue targets, community section
- README: demo video placeholder link

### Docs

- [Install guide](https://github.com/seanebones-lang/harness/blob/main/docs/INSTALL.md)
- [Comparison vs Aider / Claude Code / Cursor](https://github.com/seanebones-lang/harness/blob/main/docs/COMPARISON.md)
- [Promotion report](https://github.com/seanebones-lang/harness/blob/main/docs/PROMOTION_REPORT.md)

### Known limitations (beta)

- **Stable** release blocked until manual smoke §3 on macOS, Linux, and Windows (REL-01)
- Homebrew formula SHA update pending post-tag (`scripts/update-homebrew-sha.sh`)
- MCP inbound sampling requires interactive TUI approval (planned)

### Full changelog

See [CHANGELOG.md](https://github.com/seanebones-lang/harness/blob/main/CHANGELOG.md).

---

**Contributors welcome** — see [CONTRIBUTING.md](https://github.com/seanebones-lang/harness/blob/main/CONTRIBUTING.md).
