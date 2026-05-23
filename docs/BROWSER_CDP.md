# Browser tool — Chrome DevTools (CDP) setup

The `browser` tool controls Chrome or Chromium over the **Chrome DevTools Protocol**.

## Quick start

1. **Launch Chrome** with remote debugging enabled:

   ```bash
   # macOS
   /Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome \
     --remote-debugging-port=9222 --user-data-dir=/tmp/harness-chrome

   # Linux
   google-chrome --remote-debugging-port=9222 --user-data-dir=/tmp/harness-chrome
   ```

2. **Enable the tool** in config or on the CLI:

   ```toml
   [browser]
   enabled = true
   url = "http://127.0.0.1:9222"
   ```

   ```bash
   harness --browser "open example.com and take a screenshot"
   ```

3. Confirm DevTools is reachable:

   ```bash
   curl -s http://127.0.0.1:9222/json/version | head
   ```

## Common errors

| Symptom | Fix |
|--------|-----|
| `Browser connect failed` / `Chrome DevTools HTTP unreachable` | Chrome is not running with `--remote-debugging-port`, or the port differs from `[browser].url`. |
| Port already in use | Pick another port (e.g. `9223`) in both Chrome flags and config. |
| Connection refused on first tool call | Firewall or wrong host — use `127.0.0.1`, not `localhost`, if IPv6 causes issues. |
| Unknown action | Use one of: `navigate`, `click`, `type`, `focus`, `get_text`, `get_links`, `evaluate`, `screenshot`, `page_info`. |

## Headless vs headed

Headless Chromium works if started with the same `--remote-debugging-port` flag. For interactive debugging, use a visible window and a dedicated `--user-data-dir` so your daily profile is untouched.

## See also

- [`README.md`](../README.md) — install and platform matrix
- [`config/default.toml`](../config/default.toml) — `[browser]` block
- [`crates/harness-browser/`](../crates/harness-browser/) — implementation and unit tests
- [`docs/COOKBOOK.md`](COOKBOOK.md) — example prompts that use the browser tool
