# 041 · obs · config + SpanBuilder pure tests

**Time:** 2026-08-09 swarm-51  
**Files:** `src/observability.rs` only

## Work
- Serde: empty toml → enabled/local_traces true; `otlp_endpoint` alias
- Derive `Default` vs serde defaults asserted explicitly
- `SpanBuilder` finish/finish_err + attrs/events (tracer `enabled=false`)
- `child_tracer` shares `trace_id`; `new_id` / `now_us` smoke
- Span serde roundtrip incl. `SpanStatus::Error`

## Gate
- unit tests in existing `mod tests`
