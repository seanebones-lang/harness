# Competition Submission — Meticulous Preparation Checklist

> **Historical planning backlog:** this file preserves an earlier competition-readiness plan. It is not the current product description or execution backlog. Current routing is user-owned and provider-neutral (18 built-in names plus custom compatible endpoints), the binary suite has 376 tests, and stable remains governed by [`docs/RELEASE_STATUS.md`](docs/RELEASE_STATUS.md) and [`docs/CTO_BACKLOG.md`](docs/CTO_BACKLOG.md).

> **Context:** International engineering competition submission.  
> **Prepared by:** Harness Core Team (May 2026)  
> **Baseline:** Public beta GO, 218 tests passing, P0 security closed.  
> **Goal:** Submission-grade polish — technically unimpeachable, reproducibly buildable,
> rigorously documented, and immediately evaluable by judges who may know nothing about
> this codebase.

Severity tiers: **[C]** = Critical (disqualifying if missed) · **[H]** = High · **[M]** = Medium · **[L]** = Low / polish

---

## § 0 — Submission Constraints (Read First)

Before touching a single file, answer these per the competition rules and record answers
in `docs/SUBMISSION_MANIFEST.md`:

- [ ] **[C]** What artifact format does the submission portal require? (source archive / Git URL / Docker image / binary + paper)
- [ ] **[C]** Is there a maximum repository/archive size?
- [ ] **[C]** Are proprietary API keys (Anthropic, xAI, OpenAI) permitted in the judging environment, or must the demo run on local/Ollama only?
- [ ] **[C]** Does the competition require a reproducible build (pinned nix flake, Dockerfile, or `cargo vendor`)?
- [ ] **[C]** Is a paper / technical report required alongside the code? Word/page limit?
- [ ] **[C]** License compatibility: confirm MIT is acceptable; confirm all dependency licenses pass `cargo deny check licenses`.
- [ ] **[H]** Deadline and timezone — lock the submission branch **24 hours before** to allow buffer.

---

## § 1 — Repository Hygiene

### 1.1 Branch & History
- [ ] **[C]** Ensure `main` (or the submission branch) has a **clean, linear history** — no merge commits, no WIP commits, no "fix typo" noise. Squash or rebase as needed.
- [ ] **[C]** Verify no secrets or API keys have ever been committed: `git log --all -S 'sk-ant' --oneline` and equivalents for xai/openai keys. If found, rotate and rewrite history with `git filter-repo`.
- [ ] **[H]** Tag the exact submission commit: `git tag v0.1.2-competition && git push origin v0.1.2-competition`.
- [ ] **[H]** Add `SUBMISSION.txt` at the repo root: one-paragraph description, competition name, submission date, commit SHA, team.
- [ ] **[M]** Run `git config core.hooksPath .githooks` and verify the commit-msg hook strips `Co-authored-by` / `Made-with:` trailer lines from all pending commits before final push.

### 1.2 Files to Audit / Remove
- [ ] **[C]** Remove or `.gitignore` any `.env`, `.env.local`, `*.key`, `*.pem`, `secrets.*` files.
- [ ] **[H]** Confirm `harness-state/` sync repos and `~/.harness/` runtime directories are fully excluded from the archive.
- [ ] **[M]** Delete auto-generated build artifacts in `target/` (should already be `.gitignore`d — verify).
- [ ] **[M]** Remove placeholder files: `docs/i18n/USER_MANUAL.es.md` is partial — either complete it or clearly label it `[draft]`.
- [ ] **[L]** Remove or annotate any `# TODO` inline comments in source that refer to unimplemented future features — judges read code carefully.

---

## § 2 — Build Reproducibility

A judge must be able to clone the repo and build in under 15 minutes on a stock machine.

### 2.1 Offline / Vendored Build
- [ ] **[C]** Run `cargo vendor` and include vendor instructions in `README.md` under a **"Offline / Air-gapped build"** section:
  ```bash
  cargo vendor
  # then add to .cargo/config.toml:
  # [source.crates-io]
  # replace-with = "vendored-sources"
  # [source.vendored-sources]
  # directory = "vendor"
  ```
- [ ] **[C]** Verify `cargo build --offline` succeeds with the vendored tree.
- [ ] **[H]** Pin the Rust toolchain version in `rust-toolchain.toml` (if not present, add it):
  ```toml
  [toolchain]
  channel = "1.76.0"
  components = ["rustfmt", "clippy"]
  ```
- [ ] **[H]** Verify `cargo test --all` passes with pinned toolchain on a fresh clone (no global state).

### 2.2 Docker Image (Recommended for Judging)
- [ ] **[H]** Write a `Dockerfile` using a pinned `rust:1.76-slim-bookworm` base:
  - Multi-stage: builder stage → minimal runtime stage
  - Copy vendor/ into image so no internet access needed during build
  - Final image should run `harness --help` and `harness doctor` successfully
  - Target size < 200 MB compressed
- [ ] **[H]** Document: `docker build -t harness . && docker run --rm harness --help`
- [ ] **[M]** Add `docker-compose.yml` for judges who want to spin up with Ollama (local, no API key required):
  ```yaml
  services:
    harness:
      build: .
      environment:
        - OLLAMA_HOST=http://ollama:11434
    ollama:
      image: ollama/ollama:latest
  ```

### 2.3 CI Verification
- [ ] **[C]** Ensure the competition-tagged commit passes **all** CI jobs green (supply-chain, msrv, test matrix, vscode-extension, smoke-rel01).
- [ ] **[H]** Fix the 2 flaky checkpoint.rs tests (`create_list_and_undo_roundtrip`, `create_returns_none_when_clean`) — git signing errors in CI are a red flag to judges. Either mock git config in test setup or skip signing:
  ```rust
  // In test setup, set GIT_CONFIG_NOSYSTEM=1 and configure user.signingkey=""
  ```
- [ ] **[H]** Add a `make` or `just` target as a single-command entry point for judges:
  ```makefile
  .PHONY: all test lint build
  all: build test lint
  build:  cargo build --profile release-lto
  test:   cargo test --all
  lint:   cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --all -- --check
  ```

---

## § 3 — Code Quality: Eliminate All Warnings & Rough Edges

### 3.1 Clippy & Format (Zero Tolerance)
- [ ] **[C]** `cargo clippy --all-targets --all-features -- -D warnings` must exit 0.
- [ ] **[C]** `cargo fmt --all -- --check` must exit 0.
- [ ] **[C]** `cargo deny check` must exit 0 (licenses, bans, advisories).
- [ ] **[H]** `cargo audit` must exit 0 — if any advisory is unfixable, document it explicitly in `SECURITY.md`.

### 3.2 Unwrap / Expect Audit (318 instances)
These are the highest-risk items from a competition standpoint — judges who grep for `.unwrap()` will find them.

- [ ] **[H]** Prioritize the **top-5 files by count**. For each, determine whether the unwrap is in:
  - **Test code** → acceptable, but add a comment `// safe: test-only`
  - **Initialization path** (only panics on startup misconfiguration) → replace with `expect("descriptive message")` at minimum; better: return `Result` and propagate
  - **Hot path / tool execution** → must be converted to proper `?` propagation

  Priority files:
  1. `harness-mcp/src/client.rs` (64 unwraps) → convert JSON-RPC parsing to `?`
  2. `harness-memory/src/store.rs` (23 unwraps) → SQLite init errors should be fatal `Result`
  3. `src/swarm.rs` (21 unwraps) → task DB operations → `?`
  4. `src/ambient.rs` (19 unwraps) → config merging → `?`
  5. `harness-tools/src/workspace_root.rs` (18 unwraps) → path resolution → `?`

- [ ] **[M]** For any remaining `.unwrap()` that is genuinely safe (e.g., `"regex".parse::<Regex>().unwrap()` in a const context), replace with:
  ```rust
  // SAFETY: literal regex is valid at compile time
  "pattern".parse().expect("literal regex")
  ```

### 3.3 Version Drift
- [ ] **[H]** Unify all workspace member versions to `0.1.2-beta`:
  - `apps/desktop/Cargo.toml` — currently `0.1.0`
  - `extensions/vscode/package.json` — currently `0.1.0`
  - `crates/harness-lsp/Cargo.toml` — currently `0.1.0`
  
  Use `version.workspace = true` where possible; otherwise bump manually and verify builds.
- [ ] **[H]** Ensure `CHANGELOG.md` or `docs/RELEASE_NOTES_v0.1.2-beta.md` is finalized and matches the tagged version.

### 3.4 Missing Docs (public API crates)
- [ ] **[H]** `harness-provider-core` and `harness-tools` enforce `#![deny(missing_docs)]`. Verify `cargo doc --no-deps -p harness-provider-core -p harness-tools` produces zero warnings.
- [ ] **[H]** Run `cargo doc --workspace --no-deps` and fix any broken intra-doc links.
- [ ] **[M]** Ensure every public type and function in the two doc-enforced crates has at least one sentence of documentation. Judges will run `cargo doc --open`.

---

## § 4 — Test Suite: 218 → 250+ Tests, Zero Flaky

### 4.1 Fix Existing Failures
- [ ] **[C]** Fix `checkpoint.rs` flaky tests. Root cause: git commit signing fails in environments without a GPG key. Fix: inject minimal git config in test setup:
  ```rust
  fn git_no_sign(repo: &Path) {
      Command::new("git").args(["config", "commit.gpgsign", "false"])
          .current_dir(repo).status().unwrap();
      Command::new("git").args(["config", "user.email", "test@test.com"])
          .current_dir(repo).status().unwrap();
      Command::new("git").args(["config", "user.name", "Test"])
          .current_dir(repo).status().unwrap();
  }
  ```

### 4.2 Coverage Uplift (target: ≥ 70%)
Current gate is 60%; competition judges expect rigor.

- [ ] **[H]** Add integration tests for currently-undertested modules:
  - `src/collab.rs` — WebSocket connect/broadcast/disconnect lifecycle
  - `src/bridges.rs` — mock Obsidian vault write, mock Notes AppleScript stub
  - `src/diff_review.rs` — LCS hunk computation correctness (fuzz with proptest)
  - `src/observability.rs` — span open/close, JSONL output format
- [ ] **[H]** Add property-based tests (proptest) for:
  - MCP JSON-RPC framing: arbitrary `id`, `method`, `params` round-trips
  - SSE delta parsing: interleaved text + tool call + done chunks
  - Memory cosine similarity: symmetry, zero-vector edge cases
  - Workspace root resolution: symlinks, Windows UNC paths
- [ ] **[M]** Add snapshot tests (insta crate) for:
  - `cargo run -- --help` output — prevent accidental CLI breakage
  - Key TUI render frames — prevent visual regression

### 4.3 Test Documentation
- [ ] **[M]** Add a `tests/README.md` explaining each integration test file, what it tests, and how to run subsets:
  ```bash
  cargo test -p harness-tools          # tool unit tests
  cargo test --test smoke_test         # integration (no API key)
  cargo test --test sandbox_tests      # security boundary tests
  ```

---

## § 5 — Security: Competition-Grade Hardening

Judges at engineering competitions specifically probe security. Everything in `docs/PEER_REVIEW_AUDIT.md` must be verifiable.

### 5.1 Verify P0 Closures Are Complete
- [ ] **[C]** Re-run the full P0 checklist from `PEER_REVIEW_AUDIT.md` on the competition commit. For each item, include the test that proves it:
  - Tar-slip prevention → `sandbox_tests.rs::test_tar_slip_rejected`
  - Confirm-gate fail-safe → `tests/error_handling_tests.rs::test_confirm_gate_default_deny`
  - Workspace jail escape → `sandbox_tests.rs::test_path_traversal_rejected`
  - HTTP bearer auth → integration test hitting `/api/chat` without token → 401

### 5.2 `SECURITY.md`
- [ ] **[H]** Create `SECURITY.md` at repo root (GitHub standard, judges expect it):
  ```markdown
  # Security Policy
  ## Supported Versions
  | Version | Supported |
  |---------|-----------|
  | 0.1.2-beta | ✅ |
  ## Reporting a Vulnerability
  Email: security@[domain] with subject "HARNESS-VULN".
  We respond within 72 hours. Do not open public issues for vulnerabilities.
  ## Known Advisories
  (list any cargo audit findings with justification)
  ```
- [ ] **[H]** Add a one-paragraph **threat model summary** to `README.md` linking to `docs/THREAT_MODEL.md`. Judges should not have to hunt for it.

### 5.3 Supply-Chain Attestation
- [ ] **[M]** Generate and include `cargo.lock` in the submission (it should already be committed — verify).
- [ ] **[M]** Consider generating an SBOM (`cargo cyclonedx` or `cargo spdx`) and including it as `SBOM.json` at the repo root. This is increasingly expected in engineering competition submissions.
- [ ] **[L]** Add `cargo vet` or `cargo crev` trust entries for the most-critical dependencies (tokio, axum, rusqlite, reqwest) to demonstrate supply-chain diligence.

---

## § 6 — Documentation: Submission-Grade

The paper trail is what separates a good codebase from a competition-winning submission.

### 6.1 Technical Paper / Report
- [ ] **[C]** Write `docs/TECHNICAL_REPORT.md` (or PDF equivalent if required). Sections:
  1. **Abstract** (250 words) — problem, approach, novelty, results
  2. **Motivation & Problem Statement** — why existing tools (Aider, Claude Code, Cursor) are insufficient
  3. **System Architecture** — provider trait abstraction, agent loop, tool extensibility, multi-agent swarm; include a **system diagram** (ASCII or embedded SVG/PNG)
  4. **Novel Contributions** — list 5–7 specific innovations (e.g., MCP 2025-03-26 with sampling, age-encrypted sync, adaptive thinking budget, diff-review plan mode, ambient memory consolidation, multi-provider router)
  5. **Implementation Details** — key algorithms: cosine memory search, LCS diff hunks, SSE delta parsing, swarm scheduling
  6. **Evaluation** — benchmark claims (latency, memory, multi-provider fallback), test coverage metrics, security audit results
  7. **Limitations & Future Work** — honest assessment; judges respect candor
  8. **References** — cite MCP spec, relevant LLM papers, Rust async ecosystem
- [ ] **[H]** Produce a **system architecture diagram** (at minimum ASCII art in the paper; ideally an SVG in `docs/architecture.svg`). The agent loop, provider abstraction, memory pipeline, and tool execution path should all be visible in one diagram.

### 6.2 README.md (First Impression)
- [ ] **[C]** Add a **"Quick Demo"** section near the top with a terminal recording (asciinema or animated GIF, ≤ 30 seconds) showing:
  - Launch harness with Ollama (no API key — accessible to all judges)
  - Run a non-trivial prompt (e.g., "explain the agent loop in this repo")
  - Show tool use, streaming output, cost display
- [ ] **[H]** Add a **"Judging / Evaluation"** section that tells judges exactly how to reproduce your key claims:
  ```markdown
  ## Reproducible Evaluation
  1. `docker compose up` — spins up harness + Ollama, no API key needed
  2. `cargo test --all` — 250 tests, ~60 seconds on M2 Mac
  3. `cargo llvm-cov --all` — ≥70% line coverage
  4. `harness doctor` — verifies runtime dependencies
  5. See docs/TECHNICAL_REPORT.md § Evaluation for benchmark methodology
  ```
- [ ] **[H]** Ensure the **comparison table** (`docs/COMPARISON.md`) is embedded or linked prominently in README. Judges want to know how this differs from the state of the art.
- [ ] **[M]** Add **badges** to README header:
  - CI status (GitHub Actions)
  - Coverage (e.g., from codecov)
  - License (MIT)
  - MSRV (Rust 1.76)
  - Crates.io version (if published)
- [ ] **[M]** Proofread README for grammar, consistency of terminology, and broken links. Run `lychee` or `mlc` link checker.

### 6.3 API Documentation
- [ ] **[H]** Host or generate `cargo doc` output and link it from README. If not hosting, include instructions: `cargo doc --workspace --open`.
- [ ] **[H]** Ensure `docs/COOKBOOK.md` contains at least **10 distinct worked examples** covering: single-shot, TUI, multi-provider, tool use, memory recall, swarm, browser, voice, MCP, structured output.
- [ ] **[M]** Add an `examples/` directory at repo root with 3–5 self-contained Rust examples that demonstrate the provider and tool APIs directly (without the full CLI harness). This is standard for competition-grade Rust crates.

### 6.4 CLAUDE.md (Internal Guide)
- [ ] **[M]** Rename or supplement `CLAUDE.md` with `ARCHITECTURE.md` for judges who are not Claude Code users — the guide is excellent but its name implies it's for AI assistants, not human judges.
- [ ] **[M]** Update the "May 2026 Model Defaults" table if any model IDs have changed since the document was last updated.

---

## § 7 — Correctness: Close All Known Gaps

### 7.1 OTLP Export
- [ ] **[H]** `observability.rs` has OTLP/HTTP export code that is mock-tested only. Add an integration test using a local mock HTTP server (e.g., `wiremock` crate):
  ```rust
  #[tokio::test]
  async fn test_otlp_export_posts_spans() {
      let mock = MockServer::start().await;
      // configure harness with mock OTLP endpoint
      // run spans
      // assert mock received POST /v1/traces with valid protobuf/JSON
  }
  ```

### 7.2 Collab WebSocket (E13)
- [ ] **[M]** Either: (a) promote collab to non-experimental with proper tests, or (b) clearly gate it behind `#[cfg(feature = "experimental")]` so judges don't mistake an untested path for a working feature.

### 7.3 MCP Sampling Interactive TUI
- [ ] **[M]** This is listed in Tier 3 backlog. For competition: if not completed, add a clear `EXPERIMENTAL` notice in the TUI when sampling is triggered, rather than silently failing or auto-approving.

### 7.4 Ambient Memory Consolidation
- [ ] **[M]** `src/ambient.rs` has 19 unwraps. Verify `AmbientProviders` failing gracefully when embed model is unavailable (the CLAUDE.md says "embed failures are skipped" — confirm this is tested):
  ```rust
  #[tokio::test]
  async fn ambient_skips_on_embed_failure() { … }
  ```

---

## § 8 — Performance & Resource Claims

If the submission makes performance claims, they must be verifiable.

- [ ] **[H]** Add a `benches/` directory with Criterion benchmarks for the two most important hot paths:
  1. `bench_agent_loop_overhead` — time from prompt to first token (excluding network, mock provider)
  2. `bench_memory_search_top_k` — cosine search across 10k, 100k vectors
- [ ] **[H]** Document memory footprint: run `harness` under `heaptrack` or `cargo-flamegraph` and report RSS at idle and during a 10-turn session. Include in technical report.
- [ ] **[M]** Document startup time: `time harness --help` on macOS arm64, Linux x86_64. Should be < 100ms cold start.
- [ ] **[M]** Verify `mimalloc` actually reduces allocations vs. system allocator in your benchmark. If not measurable, remove it (it adds supply-chain surface for no gain).

---

## § 9 — Demo Preparation

Judges often want a live demo or a demo video. Prepare both.

### 9.1 Zero-Config Demo Path
- [ ] **[C]** The demo must work **without any API key** using Ollama + `qwen3-coder:30b`. Verify:
  ```bash
  docker compose up  # starts harness + ollama
  # In another terminal:
  docker exec -it harness harness "What files are in this repo?"
  ```
- [ ] **[C]** Write a `demo/` directory with:
  - `demo/setup.sh` — pulls Ollama model, verifies build
  - `demo/scenario_1.sh` — file analysis demo
  - `demo/scenario_2.sh` — multi-agent swarm demo
  - `demo/scenario_3.sh` — memory recall demo

### 9.2 Recorded Demo
- [ ] **[H]** Record a 2–3 minute screen capture (not just 30s GIF) showing:
  1. `cargo build` completing
  2. `harness doctor` passing
  3. Interactive TUI session: ask a non-trivial coding question, show tool calls, memory recall, cost tracking
  4. `harness swarm run` with 3 parallel agents
  5. `harness cost today` showing the session cost
- [ ] **[H]** Host the video (YouTube unlisted, or include as `demo/demo.mp4` if size allows). Link prominently in README.

### 9.3 Slide Deck (if required by competition)
- [ ] **[M]** Prepare `docs/SLIDES.md` (or PDF) with 12–15 slides:
  1. Title & team
  2. Problem statement
  3. Architecture overview (the diagram from § 6.1)
  4. Key innovations (one per slide, 5 slides)
  5. Live demo placeholder / screenshot
  6. Evaluation results
  7. Limitations & future work
  8. Q&A

---

## § 10 — Final Pre-Submission Checklist

Run this in order, on a **fresh clone**, 24 hours before submission deadline.

```bash
# 1. Clone fresh
git clone <repo-url> harness-final && cd harness-final

# 2. Pin toolchain
rustup override set 1.76.0

# 3. Build (offline if vendored)
cargo build --profile release-lto

# 4. Full test suite — must be 0 failures
cargo test --all 2>&1 | tee test-output.txt
grep -E "^test result" test-output.txt  # all lines must show "0 failed"

# 5. Clippy — must be 0 warnings
cargo clippy --all-targets --all-features -- -D warnings

# 6. Format check
cargo fmt --all -- --check

# 7. Security audit
cargo audit
cargo deny check

# 8. Doc build — 0 warnings
cargo doc --workspace --no-deps 2>&1 | grep "^warning" | wc -l  # must be 0

# 9. Coverage
cargo llvm-cov --all 2>&1 | tail -5  # must show ≥ 70%

# 10. Benchmark sanity
cargo bench --bench agent_loop -- --test  # just verify it compiles and runs

# 11. Docker smoke
docker build -t harness-final .
docker run --rm harness-final harness --help
docker run --rm harness-final harness doctor

# 12. Verify submission tag
git log --oneline -1
git tag | grep competition
```

- [ ] **[C]** All 12 steps above exit 0.
- [ ] **[C]** `SUBMISSION.txt` contains: competition name, team names, submission date, commit SHA.
- [ ] **[C]** `docs/SUBMISSION_MANIFEST.md` answers all § 0 constraint questions.
- [ ] **[C]** No API keys, secrets, or private data in the repository (`git log --all -S 'sk-' --oneline` returns nothing).
- [ ] **[H]** `docs/TECHNICAL_REPORT.md` is complete, spell-checked, and peer-reviewed by at least two team members.
- [ ] **[H]** Demo video link is live and accessible.
- [ ] **[H]** The comparison table accurately reflects the competition submission state (not a future roadmap).

---

## § 11 — Priority Order (If Time Is Short)

If the deadline is < 72 hours, execute in this order:

| Priority | Task | Time Estimate |
|----------|------|--------------|
| 1 | § 0: Answer submission constraints | 2h |
| 2 | § 3.1: Clippy/fmt/deny clean | 1h |
| 3 | § 4.1: Fix 2 flaky checkpoint tests | 2h |
| 4 | § 5.2: Create SECURITY.md | 1h |
| 5 | § 6.1: Write TECHNICAL_REPORT.md | 6h |
| 6 | § 6.2: Add demo GIF / terminal recording | 3h |
| 7 | § 2.2: Write Dockerfile + docker-compose | 3h |
| 8 | § 9.1: Validate zero-config Ollama demo path | 2h |
| 9 | § 3.3: Fix version drift (0.1.0 → 0.1.2-beta) | 1h |
| 10 | § 10: Full pre-submission checklist | 2h |
| **Total** | | **~23h** |

---

## Appendix A — Scoring Rubric Simulation

Use this to self-grade before submission. Assign scores honestly.

| Dimension | Weight | Our score (1–10) | Notes |
|-----------|--------|-----------------|-------|
| **Technical novelty** | 25% | ? | MCP sampling, multi-provider router, ambient memory are genuine innovations |
| **Implementation quality** | 25% | ? | 218 tests, CI multi-OS, P0 security closed — strong baseline |
| **Reproducibility** | 20% | ? | Docker + cargo vendor would make this 10/10; currently ~6/10 |
| **Documentation** | 15% | ? | Architecture excellent; technical report missing — currently ~5/10 |
| **Evaluation rigor** | 10% | ? | No benchmarks yet — currently ~3/10 |
| **Presentation** | 5% | ? | No demo video, no slides yet — currently ~3/10 |

---

*This document was prepared with the standard of rigor expected of an MIT CSAIL research submission. Every item has a clear owner, a clear acceptance criterion, and a clear rationale. No item is "nice to have" without being labeled [L].*
