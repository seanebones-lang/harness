# NextEleven Team Update — harness (2026-07-30)

**Audience:** NextEleven HQ / engineering  
**Project:** [seanebones-lang/harness](https://github.com/seanebones-lang/harness) — Rust multi-provider coding agent  
**Branch:** `dev` @ `0c7d59e` (pushed to `origin/dev`)  
**Author:** Engineering session (Sean + Hermes)  
**Related:** [`ROADMAP.md`](ROADMAP.md) · [`RELEASE_STATUS.md`](RELEASE_STATUS.md) · root [`TODO.md`](../TODO.md)

---

## 1. Executive summary

Today we stood up focused **development on a `dev` branch**, hardened and expanded the **parallel swarm subsystem** (CLI + TUI + orphan cleanup), cleaned **local hygiene** (Obsidian vault ignore), and **shipped the work to GitHub**. The product remains **public-beta GO**; **stable** is still blocked on cross-OS smoke and full prebuilt matrix.

| Area | Outcome |
|------|---------|
| Branch | `dev` created and tracking `origin/dev` |
| Feature | Swarm observability + garbage collection |
| UX | TUI swarm panel (F2 / `/swarm`) |
| Quality | 84 binary tests pass; clippy clean on harness bin; 11 swarm unit tests |
| Docs | This brief + roadmap; README / shortcuts / release log updated |
| Ops | Stale local swarm task from May reaped successfully |

**One-line for Slack/Telegram:**  
> harness `dev` now has a live swarm panel, `swarm gc` for orphan tasks, and richer swarm CLI — pushed to GitHub; roadmap for max capability is in `docs/ROADMAP.md`.

---

## 2. Why we did this

| Driver | Detail |
|--------|--------|
| **Multi-agent is a differentiator** | Swarm is a core pitch vs single-agent CLIs. If the registry lies (stuck `running`) or is hard to inspect, trust collapses. |
| **Production hygiene** | Orphan tasks after crashes/restarts polluted `~/.harness/swarm.db` (we had a real May 2026 orphan still marked running). |
| **Daily-driver UX** | Operators need in-TUI visibility without leaving the session (`list`/`status` alone is not enough during long parallel work). |
| **Demo honesty** | `demo/scenario_2_swarm.sh` called `--agents` which did not exist — demos and docs must match the binary. |
| **Team velocity** | Local notes vault should not pollute git; product docs should live under `docs/` for the whole team. |

---

## 3. What shipped today (concrete)

### 3.1 Branch & repo hygiene
- Active work on **`dev`** (not only `main`).
- `.gitignore`: `.obsidian/`, `Vault/`, `IMPROVEMENTS_TODO.md*` (local engineering vault stays local).
- Commit: **`feat(swarm): TUI panel, orphan GC, richer status CLI`** → `0c7d59e`.

### 3.2 Swarm CLI
| Command | Behavior |
|---------|----------|
| `harness swarm list` | Counts (p/r/d/f/c) + table with status, created UTC, prompt |
| `harness swarm status \| wait` | Multi-line detail (id, status, prompt, times, result preview) |
| `harness swarm run` | `--count` / **`--agents`** / **`-n`** aliases |
| `harness swarm gc` | Reap orphans; optional `--keep`, `--older-than-secs`, `--dry-run` |

**GC semantics**
- Orphan = pending/running **with no live cancel handle** in this process, older than `--stale-secs` (default 3600).
- Marked `failed(stale: orphaned after …s with no live worker)`.
- Purge of terminal rows is opt-in (keep newest N and/or age).
- After reap, purge re-lists so newly failed rows participate in keep/age rules.

### 3.3 TUI
- **F2** or **`/swarm`** — toggle right panel Events ↔ Swarm.
- Live refresh (~800 ms open; ~5 s status chip when closed).
- Status bar: `[SWARM]` / `[SWARM N]`.
- Legend: `*` live worker, `!` non-live non-terminal (orphan candidate).
- Slash: `/swarm refresh`, `/swarm gc [secs|stale=N|keep=N]`.
- PgUp/PgDn scroll the swarm list when the panel is active.

### 3.4 Fixes & verification
- Demo script uses `--count 3`.
- UTF-8-safe truncation for labels/previews.
- Live cleanup: reaped `swa54891d3` orphan from local DB.
- **Tests:** 84 pass (`cargo test --bin harness`); swarm filter 11/11.
- **Clippy:** `cargo clippy --bin harness -- -D warnings` clean.

### 3.5 Docs touched with the code
- `README.md` — swarm gc + flag aliases  
- `docs/SHORTCUTS.md` — F2 / swarm panel (this session)  
- `docs/ROADMAP.md` — forward plan (this session)  
- `docs/RELEASE_STATUS.md` — verification log entry (this session)

---

## 4. What to expect next (near term)

### For the team (using harness)
1. Prefer **`dev`** for swarm/TUI experiments until merged to `main`.
2. After crashes or long idle parallel jobs:  
   `harness swarm gc --dry-run` then `harness swarm gc`.
3. In TUI: **F2** to watch parallel work; `/swarm gc` if the panel shows `!` orphans.
4. Demos: use `--count` or `--agents` interchangeably.

### For release posture
| Track | Expectation |
|-------|-------------|
| Public beta | Still **GO** (source + existing macOS arm64 story). |
| Stable | Still **NO-GO** until REL-01 (manual smoke macOS/Linux/Windows) + full prebuilts/Homebrew (P2-10). |
| CI billing | Historical blocker for multi-platform Release workflow — still a maintainer dependency. |

### For process
- Canonical backlog remains **`TODO.md`** + **`COMPETITION_TODO.md`**.
- Forward strategy lives in **`docs/ROADMAP.md`**.
- Local Obsidian vault is optional personal/team knowledge; not required for CI.

---

## 5. Current system snapshot (honest)

| Dimension | State |
|-----------|--------|
| Version | `0.1.2-beta` |
| Architecture | Workspace: root binary + provider/tool crates; swarm in `src/swarm.rs` + SQLite `~/.harness/swarm.db` |
| Providers | Anthropic, xAI, OpenAI, Ollama, MLX, smart router |
| Multi-agent | Swarm registry + `spawn_swarm` tool + TUI panel + GC |
| Tests (this workspace) | 84 on main binary path today; historical workspace totals higher across crates |
| Coverage gate | CI target ≥60% line; still a stretch goal (~39% called out earlier) |
| Biggest risks | Incomplete REL-01 matrix; coverage; large file splits; remaining unwraps; MCP sampling UX incomplete |

---

## 6. Roadmap pointer (maximum capability & compatibility)

Full phased plan: **[`docs/ROADMAP.md`](ROADMAP.md)**.

**North star:** harness as the default daily-driver coding agent — multi-provider, multi-agent, multi-OS, editor + desktop + headless, interoperable (MCP/LSP/browser), safe by default, measurable quality.

| Horizon | Theme |
|---------|--------|
| **H0 (this week)** | Merge `dev` → `main` when green; finish REL-01 smoke on at least macOS + one Linux; document swarm GC in COOKBOOK |
| **H1 (2–4 weeks)** | Coverage ≥60%; TUI polish (scrollbar, session names); MCP sampling approval UX; browser error clarity |
| **H2 (1–2 months)** | Providers (Mistral/Gemini/Bedrock); VS Code + Tauri packaging; Docker/DB/notebook tools |
| **H3 (quarter)** | Cross-machine swarm federation; collab polish; competitive SWE-bench-style tracking; community channels |

---

## 7. How to try today’s work

```bash
git fetch origin
git checkout dev
git pull origin dev
cargo build
cargo test --bin harness swarm

# CLI
./target/debug/harness swarm list
./target/debug/harness swarm gc --dry-run
./target/debug/harness swarm --help

# TUI (needs a provider key as usual)
./target/debug/harness
# then press F2 or type /swarm
```

PR (if desired): https://github.com/seanebones-lang/harness/pull/new/dev

---

## 8. Asks / decisions for the team

1. **Merge policy:** PR `dev` → `main` now vs after REL-01 macOS full pass?  
2. **Owner for REL-01 Linux/Windows** smoke checklist (`docs/PUBLIC_RELEASE.md` §3).  
3. **CI billing** — unblock multi-platform release artifacts.  
4. **Priority pick for H1:** coverage vs MCP sampling UX vs TUI scrollbar — vote once so we don’t thrash.

---

## 9. Document map (recomposed)

| Doc | Role |
|------|------|
| **This file** | Team narrative: today / why / expect |
| [`ROADMAP.md`](ROADMAP.md) | Capability + compatibility roadmap |
| [`RELEASE_STATUS.md`](RELEASE_STATUS.md) | Go/no-go log |
| [`SHORTCUTS.md`](SHORTCUTS.md) | TUI keys (incl. F2 swarm) |
| [`../TODO.md`](../TODO.md) | Canonical open tasks |
| [`../COMPETITION_TODO.md`](../COMPETITION_TODO.md) | Submission / competition hygiene |
| [`../CLAUDE.md`](../CLAUDE.md) | Codebase guide for agents/contributors |
| [`../README.md`](../README.md) | User-facing entry |

---

*End of brief — 2026-07-30.*
