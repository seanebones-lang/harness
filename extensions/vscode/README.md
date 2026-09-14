# NextEleven Harness VS Code Extension

Side-panel chat against the Harness daemon. Distributed under the proprietary NextEleven LLC evaluation license; see LICENSE.

## Requirements

- Harness on PATH and a configured provider route (`harness setup`).
- `harness daemon` running (Unix socket on macOS/Linux; loopback TCP and `~/.harness/daemon.port` on Windows).
- VS Code 1.90 or newer.

## Development and packaging

Use Node.js 22 or newer, then run from `extensions/vscode`:

```sh
npm ci
npm audit
npm run compile
npm run package
```

Packaging recompiles the extension and includes only runtime JavaScript, the icon, manifest, README, and proprietary license. A packaged VSIX is a local artifact; marketplace publication and live editor acceptance are separate release gates.
