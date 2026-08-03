# OTLP / local traces smoke (W4.5)

Harness writes **local JSONL traces** under `~/.harness/traces/` when observability is on. Optional **experimental** OTLP/HTTP JSON export posts simplified span payloads to a collector (not a full OTLP protobuf pipeline).

## Config

```toml
[observability]
enabled = true
local_traces = true
# Optional experimental exporter (alias: otlp_endpoint)
otlp_experimental_endpoint = "http://127.0.0.1:4318"
```

`otlp_experimental_endpoint` is a base URL. Export posts to `{base}/v1/traces` when the path does not already end in `/v1/traces`.

## Local JSONL (primary path)

1. Enable config above (or defaults: `enabled` + `local_traces` default true when the section exists — see `ObservabilityConfig`).
2. Run any agent turn that creates spans (TUI chat, one-shot prompt).
3. Inspect:

```bash
ls ~/.harness/traces/
./target/debug/harness trace          # last trace summary
./target/debug/harness trace <id>     # export/print one file
```

TUI: `/trace`, `/trace last`, `/trace list` — same filesystem path; honest empty message if the directory is empty.

## OTLP experimental smoke

Unit coverage: `observability::tests::otlp_export_posts_to_v1_traces` (in-process mock HTTP).

Manual collector (optional):

```bash
# Example: Jaeger all-in-one OTLP HTTP often on 4318
docker run --rm -p 4318:4318 -p 16686:16686 jaegertracing/all-in-one:latest

# Point harness at it
# otlp_experimental_endpoint = "http://127.0.0.1:4318"

# Run one agent turn, then open http://localhost:16686
```

**Caveats**

- Exporter uses simplified JSON (trace/span IDs and timestamps are harness-local shapes converted best-effort to ns). Some collectors may accept and others may drop fields.
- Failures are logged (`OTLP experimental export failed`) and never abort the agent loop.
- Prefer local JSONL + `harness trace` for day-to-day debugging.

## Doctor

`harness doctor` reports observability enabled flag, traces directory presence, and whether `otlp_experimental_endpoint` is set.

## See also

- `src/observability.rs` — Tracer + export
- `docs/SHORTCUTS.md` — `/trace`
- `docs/NOTIFICATIONS_AUDIT.md` — unrelated desktop notify path
