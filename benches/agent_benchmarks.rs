use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use harness_memory::MemoryStore;
use tempfile::NamedTempFile;

// ── Benchmark 1: Memory search — cosine similarity over N vectors ─────────────

fn make_random_embedding(dim: usize, seed: u64) -> Vec<f32> {
    // Simple LCG-based pseudo-random for deterministic, dependency-free generation.
    let mut state = seed.wrapping_add(1);
    (0..dim)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            // Map to [-1, 1]
            ((state >> 33) as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect()
}

fn bench_memory_search(c: &mut Criterion) {
    const DIM: usize = 64; // embedding dimension for benchmarks

    let mut group = c.benchmark_group("memory_search");

    for &size in &[100u32, 500u32, 1000u32] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            // Setup: open store, insert `size` embeddings from a different session so
            // they are all visible in `search` (which excludes the query session).
            let tmp = NamedTempFile::new().expect("tempfile");
            let store = MemoryStore::open(tmp.path()).expect("open store");

            for i in 0..size {
                let emb = make_random_embedding(DIM, i as u64);
                store
                    .insert("seed-session", &format!("memory {i}"), &emb)
                    .expect("insert");
            }

            let query = make_random_embedding(DIM, 9999);

            b.iter(|| {
                store
                    .search(&query, "query-session", 5)
                    .expect("search should succeed")
            });
        });
    }

    group.finish();
}

// ── Benchmark 2: JSON serialisation round-trip (low-overhead utility) ─────────
//
// Benchmarks the serde_json round-trip for a JSON-RPC request value, which is
// the hot path inside the MCP client and the provider streaming loop.

fn bench_jsonrpc_roundtrip(c: &mut Criterion) {
    c.bench_function("jsonrpc_roundtrip", |b| {
        b.iter(|| {
            let req = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 42u64,
                "method": "tools/call",
                "params": {
                    "name": "read_file",
                    "arguments": { "path": "/some/path/to/file.rs" }
                }
            });
            let s = serde_json::to_string(&req).expect("serialize");
            let _parsed: serde_json::Value = serde_json::from_str(&s).expect("deserialize");
        });
    });
}

criterion_group!(benches, bench_memory_search, bench_jsonrpc_roundtrip);
criterion_main!(benches);
