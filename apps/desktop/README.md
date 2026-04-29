# Harness Desktop (Tauri 2)

Native wrapper around the Harness web UI (`harness serve` on `http://127.0.0.1:8787`).

## Prerequisites

- Rust toolchain
- Node.js 18+ (for `@tauri-apps/cli`)
- **`harness` on `PATH`** (install from repo root: `cargo build --profile release-lto` then copy the binary)
- For development: run `harness serve` **or** rely on auto-spawn of `harness daemon` on app launch

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

This crate is **not** part of the repo-root Cargo workspace; it uses its own `Cargo.lock` under `src-tauri/`.
