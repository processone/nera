# Rust memcached-text baseline

Minimal in-memory memcached-text server used as a native-Rust reference
point in the OneCached performance comparison.

This is **deliberately** a minimal implementation — a tokio multi-threaded
runtime, a `Mutex<HashMap<Vec<u8>, Vec<u8>>>`, and a hand-written parser
for the memcached-text protocol subset that `memtier_benchmark` exercises
(`set` / `get` / `delete` / `version` / `stats` / `flush_all` / `quit`).
No persistence, no LRU, no pluggable storage, no protocol extensions.

Its purpose is to answer one question: **"what is the per-op latency
floor for this workload on this hardware, if we pay none of the Erlang
semantics cost?"** It is not a production memcached server and should not
be used as one.

## Building

It's excluded from the main nera workspace (see `Cargo.toml`'s
`workspace.exclude`), so build it standalone:

```sh
cd benchmarks/rust_baseline
cargo build --release
```

The binary lands in `benchmarks/rust_baseline/target/release/rust_mcached_baseline`.

## Running directly

```sh
./target/release/rust_mcached_baseline           # default port 11212
./target/release/rust_mcached_baseline 11213     # custom port
```

## Running via `onecached_bench.sh`

```sh
./benchmarks/onecached_bench.sh \
    --engines rust \
    --rust-src benchmarks/rust_baseline \
    --runs 5 --ratio 1:1
```

The harness auto-builds the baseline (in release mode) from the path
passed via `--rust-src` and points the generated binary at the
benchmark's Rust port.

## Expected numbers

On a 2026-vintage laptop with 8 worker threads, 50 clients × 4 threads
× 256-byte payloads, 1:1 ratio, `memtier_benchmark`:

| Metric | Rust baseline | BEAM (OTP 28) | Nera |
|--------|--------------:|--------------:|-----:|
| Median ops/sec | ~179 k | ~99 k | ~80 k |
| p50 latency    | 1.05 ms | 1.82 ms | 2.40 ms |
| p99 latency    | 3.1 ms  | 5.4 ms  | 5.6 ms  |

See `docs/ONECACHED_PERFORMANCE_PLAN.md` for the full methodology and
interpretation.
