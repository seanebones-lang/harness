# 043 · obs · OTLP export edges

**Time:** 2026-08-09 swarm-51  

## Work
- Keep existing mock server happy-path
- Payload: us→ns scale (`*1000`), attributes present, trailing `/` on endpoint
- HTTP 500 → `Err` contains `HTTP 500`

## Gate
- tokio tests in same `mod tests`
