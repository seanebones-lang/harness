# NextEleven Harness VS Code Extension

Side-panel chat against the harness daemon.

## Requirements

- `harness` on PATH
- `harness daemon` running (Unix socket on macOS/Linux; loopback TCP + `~/.harness/daemon.port` on Windows)

## Development

```bash
cd extensions/vscode
npm install
npm run compile
```

Then **Run Extension** from VS Code.

## Packaging

Add `media/icon.png` before marketplace publish (`package.json` references it).
