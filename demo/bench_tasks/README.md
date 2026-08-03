# Offline bench pack (W7.3)

Run without API keys:

```bash
cargo build --bin harness
./target/debug/harness bench
./target/debug/harness bench --json
./target/debug/harness bench --pack demo/bench_tasks
```

`pack.json` lists built-in case ids to keep. Omit or empty `cases` to run all built-ins.

Live provider latency/cost benches stay gated on `HARNESS_BENCH_LIVE=1` (not implemented in this pack).
