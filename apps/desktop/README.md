# NextEleven Harness Desktop (Tauri 2)

Native wrapper around the Harness web UI (`harness serve` on `http://127.0.0.1:8787`).

## Prerequisites

- Rust toolchain
- Node.js 18+ (for `@tauri-apps/cli`)
- **`harness` on `PATH`** (install from repo root: `cargo build --profile release-lto` then copy the binary)
- For development: the app auto-spawns **`harness serve --addr 127.0.0.1:8787`** on launch if `/api/health` is not already reachable

## Icons

Source icon: `src-tauri/app-icon.png` (1024×1024). Regenerate platform icons:

```bash
cd src-tauri
npx --prefix .. tauri icon app-icon.png
```

## Commands

```bash
npm install
npm run dev      # tauri dev
npm run build    # release .app / installers
```

Global shortcut: **Cmd+Shift+H** (Windows/Linux: **Ctrl+Shift+H**) toggles the window. Tray icon click does the same.

## Related docs

- [`README.md`](../../README.md) — install and platform matrix
- [`docs/INSTALL.md`](../../docs/INSTALL.md) — per-OS setup
- [`TODO.md`](../../TODO.md) — severity-ranked backlog; Windows/Linux packaging (REL-03)

This crate is **not** part of the repo-root Cargo workspace; it uses its own `Cargo.lock` under `src-tauri/`.
