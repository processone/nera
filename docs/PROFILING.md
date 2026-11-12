# Profiling OneCached with nera

This guide explains how to capture and analyze CPU flamegraphs for
nera-compiled OneCached under realistic memcached load.

## Prerequisites

- **samply** — sampling profiler for macOS/Linux
  ```
  cargo install samply
  ```
- **memtier_benchmark** — Redis/Memcached load generator
  ```
  brew install memtier_benchmark
  ```
- **OneCached source** — default location: `~/devel/onecached/src`

## Quick Start

```bash
./benchmarks/profile_onecached.sh
```

This will:
1. Build nera in release mode with debug symbols
2. Compile OneCached with nera
3. Launch OneCached under the samply profiler
4. Run a warmup (3s) followed by a measured load (15s)
5. Open the Firefox Profiler in your browser with the captured flamegraph

## Options

```
--test-time T       Load duration in seconds (default: 15)
--data-size S       Value size in bytes (default: 256)
--clients N         Concurrent memtier clients (default: 50)
--threads N         memtier threads (default: 4)
--ratio R           set:get ratio (default: 1:1)
--port PORT         OneCached listen port (default: 11211)
--nera-binary PATH  Use a pre-built nera compiler binary
--onecached-src DIR OneCached source directory
--skip-build        Skip building nera (reuse existing binary)
--warmup-time T     Warmup duration in seconds (default: 3)
--save FILE         Save profile to file instead of opening browser
```

## Common Workflows

### Save a profile for later analysis

```bash
./benchmarks/profile_onecached.sh --save /tmp/onecached_profile.json.gz
```

Reopen it any time with:

```bash
samply load /tmp/onecached_profile.json.gz
```

### Longer capture for more accurate data

```bash
./benchmarks/profile_onecached.sh --test-time 30
```

### Higher concurrency

```bash
./benchmarks/profile_onecached.sh --clients 100 --threads 8
```

### Skip rebuild when iterating on analysis

```bash
./benchmarks/profile_onecached.sh --skip-build --nera-binary ./target/release/nera
```

### Read-heavy workload (1 set : 10 gets)

```bash
./benchmarks/profile_onecached.sh --ratio 1:10
```

### Write-heavy workload (10 sets : 1 get)

```bash
./benchmarks/profile_onecached.sh --ratio 10:1
```

## Reading the Flamegraph

The Firefox Profiler UI opens automatically after capture. Key areas to
inspect:

### Call Tree tab

Shows the top-down call hierarchy with cumulative time per function.
Look for the heaviest branches — these are where nera spends most of its
time under load.

### Flame Graph tab

Visual representation of the call stack. Wide bars indicate functions
that consume a large share of CPU time. Focus on:

- **`nera_rt_ets_*`** — ETS table operations (insert, lookup, delete)
- **`nera_rt_send_message`** / **`nera_rt_receive_*`** — message passing
- **`nera_rt_binary_*`** / **`nera_rt_bs_*`** — binary construction and
  pattern matching
- **`deep_copy_term`** / **`deep_copy_to_fragment`** — term copying for
  inter-process messaging
- **`gc_collect`** / **`gc_minor`** — garbage collection
- **`nera_rt_alloc_*`** — heap allocation
- **`worker_loop`** / **`scheduler`** — scheduling overhead
- **`gen_tcp`** / **`reactor`** / **`io_pool`** — TCP and I/O handling
- **FFI wrappers** (`nera_rt_*`) — if many small FFI calls dominate,
  inlining them in Cranelift IR may help

### Filtering

Use the search box at the top to filter by function name. For example,
type `ets` to highlight all ETS-related samples, or `deep_copy` to see
how much time is spent copying terms.

### Comparing profiles

To compare before/after an optimization:

1. Save both profiles with `--save`
2. Open the first with `samply load profile_before.json.gz`
3. Open the second in another tab
4. Compare the flame graphs side by side

## Artifacts

All intermediate files are written to `/tmp/onecached-profile/`:

- `onecached_nera` — the compiled OneCached binary
- `warmup.txt` — warmup run output
- `memtier.txt` — measured run console output
- `memtier.json` — structured memtier results

## Relationship to Benchmark Harness

This profiling script complements `benchmarks/onecached_bench.sh` (the
Phase 0 benchmark harness). Use the benchmark harness for stable
multi-run throughput/latency/memory comparisons across engines. Use this
profiling script to identify where CPU time is spent within nera.

Typical workflow:

1. Run `onecached_bench.sh` to get a throughput baseline
2. Run `profile_onecached.sh` to identify the bottleneck
3. Optimize the hot path
4. Run `onecached_bench.sh` again to measure the improvement
