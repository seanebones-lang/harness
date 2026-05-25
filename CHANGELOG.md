# Changelog

All notable changes to Harness will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2-beta] - 2026-05-25

### Added
- `[ambient]` config section and `AmbientProviders` (router fast for summaries, embed for vectors)
- Promotion docs: `docs/PROMOTION_REPORT.md`, `docs/RELEASE_NOTES_v0.1.2-beta.md`
- Refreshed `docs/COMPARISON.md` (Grok 4.x, MCP 2025, daemon, cost DB)

### Fixed
- TUI assistant label drift when router default differed from `[provider].model`
- Grok 4.1 Fast model slug (`grok-4.1-fast`)
- `harness doctor` and other diagnostic commands no longer block on first-run setup wizard

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
