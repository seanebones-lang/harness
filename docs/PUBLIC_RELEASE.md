# Public release checklist

Use this before tagging a release or declaring the repo “ready for anyone to build.”

## 1) Legal and docs

- [ ] `LICENSE` is the intended public license (MIT).
- [ ] `README.md` **License** section matches `LICENSE`.
- [ ] No leftover proprietary wording in docs: search for `proprietary`, `All Rights Reserved`, `no license granted`.
- [ ] Root [`Cargo.toml`](../Cargo.toml) `workspace.package.license` is `MIT`.

## 2) Automated gates (local)

Run from the repo root:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --profile release-lto
```

Confirm `harness --version` after install (or `target/release-lto/harness --version`).

## 3) Manual smoke (needs keys or local stack)

Check at least one provider end-to-end:

- [ ] **One-shot:** `XAI_API_KEY=… harness "list files in ."` (or another configured key)
- [ ] **TUI:** `harness` — send a prompt; confirm token/cost line updates where applicable
- [ ] **Web:** `harness serve --addr 127.0.0.1:8787` — chat round-trip in the browser
- [ ] **Sessions:** `harness export <id>` produces readable Markdown; `harness sessions` lists rows

**GitHub (optional):** with `gh auth login`:

- [ ] `harness pr` or `/pr` lists PRs; `/issues`, `/ci` work in the TUI for your repo

## 4) Fresh-machine / quickstart rehearsal

On a clean machine (or clean clone + new shell):

- [ ] Follow **Quick Start** in [`README.md`](../README.md) only (no extra tribal knowledge).
- [ ] `harness doctor` reports sensible defaults (keys may be missing until export).

## 5) Go / no-go

**Ship** when (1)–(2) are green, (3)–(4) are acceptable for your audience, and README expectations match reality.

**No-go** if automated gates fail, license is inconsistent, or quickstart cannot be completed from docs alone—fix or document blockers first.

---

## Maintainer verification log (example)

Record here when you run this checklist (update the table per release).

| Check | Notes |
| ----- | ----- |
| Automated gates §2 | `cargo fmt`, `clippy -D warnings`, `test`, `release-lto` |
| `harness --version` | After install or `target/release-lto/harness` |
| `harness doctor` | Keys/tooling summary |
| One-shot prompt | e.g. `harness "Reply with exactly: OK"` |
| `harness serve` + `/api/health` | JSON `{"status":"ok",...}` |
| `harness sessions` / `harness export <id>` | Markdown output |
| TUI `harness` | Manual — send a message, confirm streaming |
| `gh auth` + `/pr`, `/issues` | Manual — requires GitHub CLI |

### Snapshot for current tree (fill in when cutting a release)

- **Date:** 2026-05-03 (workspace verification pass)
- **Recorded revision:** run `git log -1 --oneline` when you tag — should include this checklist and MIT license change.
- **Go / no-go:** **GO** for public Beta under MIT — complete TUI + `gh` checks on a full dev machine before calling it stable.

---

_Last automated run: record `git rev-parse --short HEAD` and date here when you cut a release._
