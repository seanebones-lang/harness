# Changelog

All notable changes to NextEleven Harness will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Provider-neutral `harness route show|set|model|add|remove|move|custom` commands with explicit global/project scope
- First-class presets for Cerebras, DeepSeek, Fireworks, Groq, Hugging Face, NVIDIA, OpenRouter, Perplexity, SambaNova, and Together through the OpenAI-compatible transport
- `api_key_env` and `kind = "openai-compatible"` configuration for future bearer-authenticated endpoints without source changes

### Changed
- Setup now asks for one or more exact `provider:model` entries and preserves their order; no provider or model is marked recommended
- Runtime no longer chooses a provider priority, inserts implicit fallbacks, or silently falls back to Ollama
- Browser setup edits the same exact route as the CLI and no longer collects API keys
- `harness models --set` updates only the active config; route commands provide explicit `--global` and `--project` targeting

### Security
- Unknown provider names without an explicit adapter/base URL now fail closed instead of falling through to xAI
- Setup keeps credentials in environment variables instead of writing newly entered secrets to config

### Notes
- Working tree tracks **1.3.0** on `main`. See below for the cut.

## [1.3.0] - 2026-08-09

Public **proof-of-concept** cut. Repository is visible on GitHub for evaluation only.

**License: proprietary NextEleven LLC — NOT MIT, NOT open source.**

### Added
- Gemini + Bedrock providers; Database/Notebook/Docker tools (config-gated)
- `harness bench` offline pack; swarm worker allowlist + wall timeout; model on swarm JSON
- `src/agent/*` and `src/server/*` module splits
- Threat model v2; docs refresh waves
- Swarm-50 / Swarm-51 coverage climbs; CI line gate ≥60% **met** (measured **61.65%**)
- Trust path-inject (`load_from_path` / `save_to_path`) + wiring pure edges
- Connect CLI `--url` clap fix; single-panel Hermes-style TUI
- Public-repo hardening: path scrub, secret hygiene docs, proprietary packaging labels

### Changed
- Workspace version **0.1.2-beta → 1.3.0** (binary, desktop, VS Code, Docker, Homebrew formula)
- Ship branch **main** only (`dev` folded and removed)
- LICENSE: proprietary NextEleven LLC notice (public = POC visibility only)
- Honest gates: bin tests **363**; tools **179**; coverage badge ~62%
- Dockerfile / Homebrew / VS Code / CONTRIBUTING / SUBMISSION\* — proprietary / UNLICENSED (not MIT)
- `deny.toml`: product = `LicenseRef-NextEleven-Proprietary`; MIT only as third-party dep allowance (not a product grant)

### Security
- No API keys in tracked tree; `.env` / `.envrc` gitignored
- Report vulns via SECURITY.md (private advisory)

## [0.1.2-beta] - 2026-05-25

### Added
- `[ambient]` config section and `AmbientProviders` (router fast for summaries, embed for vectors)
- Promotion docs: `docs/PROMOTION_REPORT.md`, `docs/RELEASE_NOTES_v0.1.2-beta.md`
- Refreshed `docs/COMPARISON.md` (Grok 4.x, MCP 2025, daemon, cost DB)

### Fixed
- TUI assistant label drift when router default differed from `[provider].model`
- Grok 4.1 Fast model slug (`grok-4.1-fast`)
- `harness export` and `harness delete` skip first-run setup wizard

### Changed
- `TODO.md` promotion tiers (Tier 0–3)
- `CONTRIBUTING.md` contribution pathways and community section
- README demo video placeholder

## [0.1.1-beta] - 2026-05-24

### Added
- `harness setup` — interactive provider and API key configuration
- `harness update` — prints platform-specific upgrade instructions
- TUI and web UI screenshots in README
- Windows prebuilt in release workflow
- Ollama fallback when no cloud API keys are configured
- MCP inbound request handling for `sampling/createMessage`
- Default MCP command allowlist in config

### Fixed
- Empty `ProviderRouter` panic when no providers configured
- Prebuilt download URL mismatch (`install.sh` ↔ GitHub Releases artifact names)
- XAI API key missing `.unwrap()` panic in `build_arc_provider`
- Stale default Claude model ID (`claude-sonnet-4-6`)
- AppleScript injection in calendar bridge paths
- Constant-time bearer token comparison
- Removed committed merge artifacts (`main.rs.orig`, `main.rs.rej`)

### Changed
- Install scripts warn when `~/.cargo/bin` and `~/.local/bin` both contain harness
- First-run wizard reloads config after saving keys
- `/api/setup/state` no longer exposes filesystem config path

## [0.1.0] - 2026-05-23

### Added
- Initial public release
- Multi-provider support (Anthropic, xAI, OpenAI, Ollama)
- Terminal TUI with ratatui
- Semantic memory + project memory
- Sub-agent swarm support
- Cost tracking and dashboard
- MCP client support
- Browser automation via Chrome DevTools Protocol
- Cross-machine encrypted sync
- GitHub PR review integration

### Notes
- First tagged release
- Prebuilt binaries available for macOS, Linux, and Windows
