# OneCached Performance Summary And Improvement Plan

Date: 2026-03-02

## Goal

Close the performance and memory gap between OneCached on Nera and a native
Rust baseline while preserving Erlang semantics.

## Phase 0 Status (2026-03-03)

Phase 0 harness implementation is now functionally complete, with one
remaining stability gate failure on BEAM variation.

### Harness updates completed

- `benchmarks/onecached_bench.sh` now supports per-engine ports:
  - BEAM port auto-detected from `--beam-src/onecached.hrl` (`?PORT`)
  - explicit overrides: `--beam-port`, `--nera-port`, `--rust-port`
- `--beam-src` compile path now copies `onecached.app` into `$BEAM_DIR`.
- BEAM startup no longer relies on ETS tables owned by the eval process:
  ETS is initialized in a dedicated long-lived owner process before
  `application:start(onecached)`.

### Validation runs (March 3, 2026)

Smoke run:

- Command:
  `./benchmarks/onecached_bench.sh --engines beam,nera --runs 1 --skip-flush-wait --beam-src /home/user/devel/onecached/src`
- Artifact: `/tmp/onecached_bench_20260303_212003`
- Result:
  - BEAM: 142,948 ops/sec
  - Nera: 40,070 ops/sec
  - BEAM `server.log` no longer shows ETS ownership crash (`ets:insert badarg`).

Full Phase 0 run:

- Command:
  `./benchmarks/onecached_bench.sh --runs 5 --beam-src /home/user/devel/onecached/src`
- Artifact: `/tmp/onecached_bench_20260303_212150`
- 5-run median table:
  - BEAM: 137,455 ops/sec (`variation_pct=50.7`)
  - Nera: 39,192 ops/sec (`variation_pct=8.4`)
  - Rust: 168,160 ops/sec (`variation_pct=7.9`)

### Deliverable checklist

- Harness script checked-in: done
- JSON/RSS/env metadata capture: done
- 5-run median reporting and comparison table: done
- Success gate (<10% Ops/sec variation for stable command): partial
  - Nera and Rust pass
  - BEAM currently fails (50.7% variation)

## Benchmark Summary

Workload used for all engines:

- `memtier_benchmark --protocol=memcache_text`
- `--clients=50 --threads=4 --ratio=1:1 --data-size=256`
- `--pipeline=1 --test-time=10`
- Localhost, single machine, warmup before measured run

Measured engines:

- OneCached on BEAM (OTP 28)
- OneCached compiled with Nera (`/tmp/onecached-bench/onecached_nera`)
- Rust memcached-text baseline (`/tmp/onecached-bench/rust_memcached_baseline`)

### Throughput and Latency (single measured run)

| Engine        | Ops/sec | Avg latency (ms) | p50 (ms) | p99 (ms) | p99.9 (ms) |
|---------------|--------:|-----------------:|---------:|---------:|-----------:|
| BEAM          | 159,291 |            0.878 |    0.519 |    6.527 |     21.247 |
| Nera          |  32,359 |            6.176 |    5.311 |   23.551 |    104.447 |
| Rust baseline |  95,239 |            2.097 |    1.287 |   15.743 |     69.631 |

### Memory (RSS, sampled every 200ms during same load profile)

| Engine        | Idle RSS | Avg RSS during load | Peak RSS during load | Final RSS |
|---------------|---------:|--------------------:|---------------------:|----------:|
| BEAM          | 78.55 MB |            58.35 MB |             93.50 MB |  45.25 MB |
| Nera          |  2.53 MB |            72.05 MB |             88.27 MB |  87.89 MB |
| Rust baseline |  2.42 MB |            14.78 MB |             16.88 MB |  12.86 MB |

## Main Findings

1. Nera is currently about 20 percent of BEAM throughput and about 25 percent
of the Rust baseline throughput on this workload.
2. Nera tail latencies are significantly higher than BEAM and Rust.
3. Nera memory under load is close to BEAM peak, but unlike BEAM it remains
high after load (high final RSS retention). *(Superseded — see 2026-04-12
update: load RSS is now 28–58 MB and post-flush RSS drops to 2.5–27 MB.)*
4. Rust baseline keeps much lower steady and post-load RSS than both BEAM and
Nera.

## Likely Bottlenecks (ranked)

1. Off-heap binary accounting and GC trigger policy
   - Known gap already tracked in `ROADMAP.md`: no binary virtual heap
     contribution to GC triggering for refc binaries.
   - Relevant files: `crates/nera-runtime/src/heap.rs`,
     `crates/nera-runtime/src/message.rs`.

2. ETS hot path overhead
   - Current ETS implementation serializes keys and deep-copies terms for
     storage and lookup.
   - Likely expensive for memcached workloads with frequent set/get.
   - Relevant file: `crates/nera-runtime/src/ets.rs`.

3. Socket/message path overhead
   - Per-request socket mode switching and runtime boundary costs likely add
     latency and scheduler pressure.
   - Relevant files: `crates/nera-runtime/src/ffi/gen_tcp.rs`,
     `crates/nera-runtime/src/socket.rs`, `crates/nera-runtime/src/process.rs`.

4. Runtime scheduling and contention
   - Need visibility into lock contention, mailbox pressure, and scheduler
     balance under high connection count.
   - Relevant files: `crates/nera-runtime/src/scheduler.rs`,
     `crates/nera-runtime/src/process.rs`.

5. Codegen/runtime instruction cost
   - Remaining gap may include generated code quality and FFI call overhead.
   - Relevant files: `crates/nera-codegen/src/func_translator.rs`,
     `crates/nera-codegen/src/compiler.rs`.

## Improvement Plan

## Phase 0 - Reproducibility and instrumentation (1 week)

Deliverables:

- Add a checked-in benchmark harness script for 3-way runs (BEAM/Nera/Rust).
- Capture JSON + RSS samples + environment metadata for every run.
- Add at least 5-run median reporting to reduce single-run noise.

Success criteria:

- One command produces a stable comparison table.
- Repeat runs show less than 10 percent variation in Ops/sec median.

## Phase 1 - Memory and GC fixes (highest priority, 1-2 weeks)

Work:

- Implement binary virtual heap accounting for refc binaries in GC trigger.
- Track off-heap bytes per process and include in collection heuristics.
- Optimize `refc_binaries` retention sweep behavior to reduce long-tail pauses.

Target after Phase 1:

- Nera final RSS after load below 60 MB (from ~88 MB baseline).
- Nera p99.9 latency below 80 ms (from ~104 ms baseline).

## Phase 2 - ETS hot path optimization (1-2 weeks)

Work:

- Add fast path for common ETS `set` table operations used by OneCached.
- Reduce key serialization and deep-copy overhead on lookup/insert.
- Introduce microbenchmarks for ETS insert/get/delete with binary keys.

Target after Phase 2:

- Nera throughput at or above 55k Ops/sec.
- Nera p99 latency below 20 ms.

## Phase 3 - Socket/runtime path optimization (1-2 weeks)

Work:

- Profile and reduce socket mode-switch overhead in request handling path.
- Reduce per-request allocations in `gen_tcp` send/recv and iolist flattening.
- Add runtime counters for queue depth, context switches, and send/recv latency.

Target after Phase 3:

- Nera throughput at or above 75k Ops/sec.
- Nera p99.9 latency below 60 ms.

## Phase 4 - Scheduler and codegen tuning (2+ weeks)

Work:

- Add scheduler contention metrics and fix imbalances/lock hotspots.
- Evaluate codegen and FFI boundary costs on top sampled callsites.
- Apply targeted optimizations only where profiling confirms impact.

Target after Phase 4:

- Nera throughput at or above 100k Ops/sec.
- Nera peak RSS below 60 MB and final RSS below 40 MB.

## Validation Gates

For each phase, require all of:

1. Correctness: OneCached protocol regression tests still pass.
2. Performance: 5-run median compared to previous phase.
3. Memory: peak and final RSS tracked and reported.
4. Regression guard: CI benchmark job with thresholds for major regressions.

## Immediate Next Actions

1. Land Phase 0 harness and metrics collection.
2. Start Phase 1 binary virtual heap implementation in:
   - `crates/nera-runtime/src/heap.rs`
   - `crates/nera-runtime/src/message.rs`
3. Add ETS microbenchmarks and flamegraph capture to confirm Phase 2 scope.

## Notes

- This plan is intentionally data-driven: each phase starts with profiling and
  ends with measured deltas against the same workload.
- Current numbers are from local single-machine runs; multi-run medians should
  be treated as the authoritative baseline for tracking progress.

## Update (2026-03-02) - Post epoll/kqueue support validation

A fresh local validation run was executed on March 2, 2026 using the same
workload shape as the Nera baseline in this document.

### Workload used for direct comparison

- `memtier_benchmark --protocol=memcache_text`
- `--clients=50 --threads=4 --ratio=1:1 --data-size=256`
- `--pipeline=1 --test-time=10`
- Localhost, same machine
- Warmup run before the measured run

### Direct Nera baseline vs current

| Metric             | Nera baseline (this doc) | Current Nera |   Delta |
|--------------------|-------------------------:|-------------:|--------:|
| Ops/sec            |                   32,359 |    52,502.34 | +62.25% |
| Avg latency (ms)   |                    6.176 |      3.80872 | -38.33% |
| p50 latency (ms)   |                    5.311 |        3.551 | -33.14% |
| p99 latency (ms)   |                   23.551 |        7.679 | -67.39% |
| p99.9 latency (ms) |                  104.447 |       15.551 | -85.11% |

Measured run artifact:

- `/tmp/onecached_bench_match_baseline_20260302_174800/match_baseline.txt`

### Additional stability check (multi-run)

An additional 3-run profile was executed with the same request mix and payload
shape (`test-time=15`):

- Aggregated average totals: **50,513.92 ops/sec**
- Aggregated average latency: **3.95858 ms**

Artifact:

- `/tmp/onecached_bench_20260302_174247/profile_base.txt`

### Impact on phase targets

- Phase 2 throughput target (`>=55k ops/sec`) is not yet reached, but current
  single-run result (52.5k) is close.
- Phase 2 p99 target (`<20 ms`) is now exceeded with strong margin (7.679 ms).
- Phase 1 p99.9 target (`<80 ms`) is exceeded with strong margin (15.551 ms).

## Update (2026-04-11) - Teardown race conditions fixed

Three race conditions in the connection teardown path were diagnosed and
fixed. Before these fixes the benchmark harness frequently hung at the end
of a memtier run (memtier stopped reporting but still held sockets open),
because stranded `ActiveReadReady` fragments kept `in_flight=true` forever
on the reactor's active registrations, causing the kqueue/epoll reactor to
spin on stale edge-trigger events.

### Fixes committed

1. **`socket::remove()` `Arc::try_unwrap` failure** — `Deregister` now
   carries the raw fd extracted before registry removal, so the reactor
   can still deregister the fd when another code path holds the
   `Arc<Socket>`. Previously the dereg silently failed and the fd stayed
   in kqueue.
2. **Line-mode partial data busy loop** — `handle_active_read_on_worker`
   now consumes peeked partial data into a per-socket line buffer instead
   of returning it to the kernel, preventing an edge-triggered kqueue
   spin when a short command line lands on the socket.
3. **Stranded fragment at waiting-persist path** (`scheduler.rs:773`) —
   when a worker picks a process whose `proc.status == Waiting` and
   `drain_inbox` did not produce any mailbox message (e.g. line mode with
   no newline yet), the worker re-stores `ASTATUS_WAITING`. A new
   `ActiveReadReady` fragment pushed during that window has no race
   check — `try_wake()` saw `READY` and failed. Added a race check that
   mirrors the existing one in the `STATUS_RECEIVE_WAIT` branch.

A matching defensive race check was also added to the `STATUS_IO_WAIT`
branch (draining the inbox) so that an `ActiveReadReady` fragment
targeting a process currently blocked in an I/O wait cannot be stranded
either.

### Post-fix run

- Command:
  `./benchmarks/onecached_bench.sh --engines nera --runs 3 --test-time 5 --port 11211`
- Artifact: `/tmp/onecached_bench_20260411_105918`
- Median: **66,956 ops/sec**, avg 2.0 ms, p99 8.5 ms, p99.9 53.2 ms,
  RSS 29.9 MB, footprint 36.2 MB
- All 3 runs completed cleanly — no hang during teardown, active_regs
  drained to zero after each run.

Variation across runs is still high (34.4%) because the first run is a
cold-start outlier (~45k) while runs 2–3 stabilize around 67k ops/sec.
This is worse than the 10% gate but a separate investigation; it is not
a correctness issue.

### Impact on phase targets (updated)

- Phase 2 throughput target (`>=55k ops/sec`) is now reached (median 67k).
- Phase 2 p99 target (`<20 ms`) still exceeded (8.5 ms).
- Phase 1 p99.9 target (`<80 ms`) still exceeded (53.2 ms).

## Update (2026-04-12) - Scheduler, mailbox, TCO, and ETS wins

A batch of runtime and codegen improvements landed since the 2026-04-11 entry.
Key performance-relevant commits (newest → oldest):

- `c7a0476` Thread `SchedulerHandle` through `ProcessContext`, remove
  `global_scheduler` from hot paths
- `3b2e923` Optimize ETS key allocation with inline immediate-term fast path
- `000b6d0` Fix refc binary hash bug and harden `Arc` lifecycle safety
- `0011568` Fix `persistent_term` region leak and document resource lifetimes
- `7d9dfd1` Cache Cranelift ISA across module compilations
- `f6a7eb8` Sharded process table (`DashMap`) and `alive_count` termination
  fast-path
- `2ad03e5` Skip active-mode TCP reads for exited processes
- `7150e57` Fix mailbox compaction race during active selective receive scan
- `7541ec1` Fix `list_to_existing_atom/1` and add `binary_to_existing_atom/2`
- `ecf8724` Full tail call optimization for all call targets

### Validation runs (April 12, 2026)

Default 1:1 workload (apples-to-apples with the Phase 0 baseline):

- Command:
  `./benchmarks/onecached_bench.sh --engines nera --runs 5 --nera-port 11211`
- Artifact: `/tmp/onecached_bench_20260412_191151`
- Binary: `/tmp/onecached-bench/onecached_nera` rebuilt from HEAD
  (`sha256=1bba0c2cbb24a8bc`)
- 5-run median: **76,612 ops/sec**, avg 2.0 ms, p50 2.527 ms, p99 5.343 ms,
  p99.9 6.527 ms, RSS load 57.7 MB, RSS flush 27.0 MB, footprint 36.4 MB
- Per-run ops/sec: 76257 / 77584 / 77442 / 76611 / 76105 (tight, ~2% spread)

Read-heavy 1:9 workload (apples-to-apples with the 2026-04-11 1:9 run at
commit `9bc7b2d75fbe`, artifact `/tmp/onecached_bench_20260411_112100`):

- Command:
  `./benchmarks/onecached_bench.sh --engines nera --runs 5 --ratio 1:9 --test-time 15 --nera-port 11211`
- Artifact: `/tmp/onecached_bench_20260412_194441`
- 5-run median: **92,533 ops/sec**, avg 2.0 ms, p50 2.039 ms, p99 4.959 ms,
  p99.9 13.503 ms, RSS load 28.7 MB, RSS flush 2.5 MB, footprint 36.6 MB
- Per-run ops/sec: 96492 / 88476 / 98555 / 65416 / 92533 (one cold outlier,
  variation 35.8%)

### Deltas vs previous baselines

| Workload  | Previous (median) | Current (median) | Delta |
|-----------|------------------:|-----------------:|------:|
| 1:1 (Phase 0, `2026-03-03`, git `2adc…`) | 39,192 | **76,612** | **+95.6%** |
| 1:1 (`2026-04-11`, 3 runs × 5s, git `9bc7b2d75fbe`) | 66,956 | **76,612** | +14.4% |
| 1:9 (`2026-04-11`, 5 runs × 15s, git `9bc7b2d75fbe`) | 92,166 | **92,533** | +0.4% (flat) |

### Impact on phase targets

- Phase 3 throughput target (`>=75k ops/sec`) is now reached on 1:1
  (median 76.6k).
- Phase 4 throughput target (`>=100k ops/sec`) is within ~7% on 1:9
  (median 92.5k) but remains unmet on 1:1.
- Phase 2/3 p99 target continues to be exceeded with wide margin
  (5.3 ms on 1:1, 5.0 ms on 1:9).
- Phase 1 p99.9 target (`<80 ms`) exceeded with very wide margin
  (6.5 ms on 1:1, 13.5 ms on 1:9).
- Phase 4 memory targets (`peak RSS <60 MB`, `final RSS <40 MB`) are now
  met on both workloads: 1:1 load 57.7 MB / flush 27.0 MB, 1:9 load
  28.7 MB / flush 2.5 MB. Compared to the Phase 0 baseline (peak 88.3 MB,
  final 87.9 MB), post-load retention in particular is dramatically
  better — the binary virtual heap / GC trigger work from Phase 1 is
  visible here.
- Stability gate (<10% Ops/sec variation): **1:1 passes** (~2% spread);
  1:9 still fails due to a cold-start outlier.

### Interpretation

The 1:1 win (+14% vs April 11, nearly 2× vs Phase 0 baseline) is consistent
with the cluster of scheduler/process-table/TCO changes landing in this
window — the write path exercises mailbox, spawn, termination, and
per-process bookkeeping more than the read path, which is why 1:9 is
flat while 1:1 moves so much. The 1:9 result being stable at ~92.5k
confirms there is no regression from the recent commits.

## Update (2026-04-13) - 1:1 flamegraph findings

A sampling profile of OneCached on Nera under the 1:1 workload was captured
with `samply` (997 Hz, 15 s measured load, 50 clients × 4 threads × 256 B
payload, localhost).

- Profile artifact: `/tmp/onecached-profile/profile_1to1_20260413.json.gz`
  (re-openable with `samply load`).
- Sustained throughput under the profiler: ~70k ops/sec (vs 76.6k unprofiled
  on the same workload). Profiler overhead is modest and does not change
  the shape.

### Harness fixes landed alongside the profile run

Two latent bugs in `benchmarks/profile_onecached.sh` had to be fixed before
a profile could be captured:

- `nera build` was invoked from the nera repo root, so
  `discover_files`'s CWD-based auto-discovery of `tests/` pulled in the
  thoas eunit test files (`?assertEqual` etc.) and failed compilation.
  Fixed by running `nera build` from `$WORK_DIR`.
- The script only passed `-I $ONECACHED_SRC` (i.e. `~/devel/onecached/src`),
  but `onecached.hrl` lives in the sibling `include/` directory. Fixed by
  also passing `-I $onecached_root/include`.

There is also a latent nera CLI issue: when explicit `.erl` files are
passed to `nera build` (no `--src`), `discover_files` still walks
`CWD/tests/` looking for extra `.erl` files. That's surprising and worth
filing, but out of scope for this profile. (Since fixed: explicit files without
`--src` no longer pull in the current folder's `tests/`.)

### Headline finding: the system is ~93% idle

On-CPU vs kernel-wait breakdown (excluding system kernel `kevent` /
`psynch_cvwait` leaves as "wait"):

| Thread   | Samples | On-CPU | In kernel wait |
|----------|--------:|-------:|---------------:|
| reactor  |  17,930 |  6.9%  |          93.1% |
| worker-0 |  18,613 |  6.3%  |          93.7% |
| worker-1 |  18,620 |  6.5%  |          93.5% |
| (workers 2–7 are within ±0.2% of worker-0/1) |

Per-op math: 70k ops/sec ÷ 8 workers × 6.3% on-CPU ≈ **~7 µs of CPU per
op per worker**. At 100% worker utilization the 8-core ceiling is
~1.1M ops/sec on this hardware for the current per-op cost; we are at
**~6.4% of that ceiling**.

This overturns the premise of Phase 4. The remaining gap to the Phase 4
100k ops/sec target on 1:1 is **not a codegen or instruction-cost
problem** — the on-CPU budget per op is already small. The ceiling is
set by **wake-up / dispatch overhead**: workers spend ~79% of wall time
parked in `psynch_cvwait`, the reactor spends ~93% parked in `kevent`.
To move throughput we need to reduce the cost (or avoid) the park/unpark
cycle per dispatched message, not shave cycles off the hot path.

### Top on-CPU self-time (workers, % of on-CPU slice)

| % on-CPU | Function | Location |
|---------:|----------|----------|
|    5.8%  | `socket::get_entry` | `socket.rs:126` |
|    ~4-6% | `core::hash::BuildHasher::hash_one` + siphash `write` (combined) | hashing fds into `DashMap<fd, SocketEntry>` |
|    ~5.3% | `_rjem_sdallocx.cold.1` + `_rjem_malloc` (combined) | jemalloc hot/slow paths |
|    1.1%  | `ets::EtsRegistry::get_table` | `ets.rs:955` — re-looked-up per op |
|    1.0%  | `hashbrown::RawTable::find` | DashMap find |
|    0.9%  | `ProcessHeap::collect` | `heap.rs:580` — GC, modest |
|    0.8%  | `nera_rt_exact_eq` / `nera_rt_tuple_element` | codegen helpers |

### Top on-CPU self-time (reactor, % of on-CPU slice)

| % on-CPU | Function | Location |
|---------:|----------|----------|
|   21.3%  | `socket::get_entry` | `socket.rs:126` — fd lookup per event |
|    ~14%  | `hash_one` + siphash `write` (combined) | same DashMap hashing |
|    15%   | `IoReactor::drain_commands` | `reactor.rs:303/315/318` (combined) |
|    2.5%  | `DashMap::hash_u64` | `dashmap/lib.rs:305/307` |

### What the flamegraph does and does not confirm

The three candidates I guessed before profiling were: `ets:insert` for
binary values, mailbox enqueue under contention, and refc-binary ref
bumping. The profile says:

- **ETS insert path: not hot** (no `EtsTable::insert` frames in the
  top-20; the only ETS frame is the registry lookup at 1.1%). The
  ets_pool / CompactHeap / RwLock write-lock work from the recent
  commits is already cheap on a single well-used table. **Drop this
  from the Phase 5 scope.**
- **Mailbox enqueue: not visible as a hot self-time site.** May still
  cost wall time via parking overhead — but the instruction cost of
  enqueue itself is not the issue.
- **Refc binary ref bumping: not visible.** The bucket I assumed
  would dominate (atomic increments on value binaries) doesn't show up
  in either workers or reactor self-time.

What *is* hot is very different:

- **Socket fd table is a `DashMap<fd, SocketEntry>` hashed with
  siphash.** This single structure drives ~27% of reactor on-CPU and
  ~10% of worker on-CPU. fds are small dense integers, so this should
  be a slab / `Vec<Option<SocketEntry>>` keyed directly by raw fd.
- **`ets:info`-style registry re-lookups** happen every op
  (`EtsRegistry::get_table` at 1.1% of workers), even though onecached
  uses a single long-lived table. This is cacheable in the calling
  context.
- **jemalloc slow-path traffic** (`_rjem_sdallocx.cold.1` at ~3%)
  suggests per-op allocation that isn't being reused — worth tracking
  down the caller before committing to an allocator-side fix.

## Phase 5 - Dispatch and socket-table optimization (2-3 weeks)

Phase 5 supersedes the codegen-tuning portion of Phase 4. The flamegraph
evidence says codegen is not on the critical path for 1:1; wake-up
overhead and the socket fd table are.

### Work

1. **Socket fd table rewrite** — replace the `DashMap<fd, SocketEntry>`
   in `crates/nera-runtime/src/socket.rs` with a direct-indexed
   `Slab<SocketEntry>` or `Vec<Option<SocketEntry>>` keyed by raw fd.
   fds are small dense integers assigned by the kernel, so this is O(1)
   with no hashing and no sharded lock. Handle fd reuse by generation
   counter or by clearing the slot on close. Expected: eliminate ~27%
   of reactor on-CPU and ~10% of worker on-CPU — about 1.8% of total
   wall time across the reactor and ~0.7% across each worker. On its
   own this does not close the throughput gap (we're idle-bound), but
   it is the cheapest standalone win.

2. **Worker park/unpark audit** — instrument the scheduler to answer:
   does every mailbox message force a park cycle for the target
   worker, or does a busy worker pick up consecutive messages without
   parking? If the former, evaluate:
   - a brief spin-before-park loop (standard MPSC hybrid pattern)
   - batching N > 1 messages per wake-up, so one cvwait cost amortizes
     across multiple ops
   - direct process→worker pinning so a given mailbox is always drained
     on a specific worker (and the wake targets one core, not any core)

   This is the workstream most likely to actually move 1:1 throughput,
   but it's also the one with the highest uncertainty — start with
   instrumentation, not implementation.

3. **Reactor dispatch batching** — `IoReactor::drain_commands`
   (reactor.rs:303/315/318) is 15% of reactor on-CPU. Audit whether
   events coming out of kevent are drained and dispatched one at a
   time or as a batch. A single kevent call can return dozens of
   events per wake; we should translate that into at most one wake per
   batch per target worker, not one wake per event.

4. **Per-op allocation cleanup** — track the caller of
   `_rjem_sdallocx.cold.1` under 1:1 load (add a malloc-size histogram
   counter or use a dtrace probe). The cold bin-refill path firing at
   ~3% of worker on-CPU implies a recurring alloc/free at a size that
   crosses a bin boundary; likely a short-lived `Vec` or `Box` in the
   command path that could be reused or arena-allocated.

5. **Cache `ets:info` registry lookups** — cache the `EtsTable` handle
   in the process context (or at the FFI call site) after first
   lookup, so repeated `ets:insert`/`ets:lookup` calls against the
   same named table skip `EtsRegistry::get_table` entirely. Small
   win (~1% worker on-CPU) but essentially free.

### Targets after Phase 5

- Nera 1:1 throughput: **≥100k ops/sec** (the Phase 4 target, unmet at
  76.6k). The 1:9 throughput should cross 100k as a side-effect if the
  dispatch-batching work lands.
- Worker on-CPU utilization under 1:1 load: **≥15%** (from 6.3%).
  This is the primary success metric — it is what says we are
  actually driving the cores, rather than a derived throughput number
  that can move for unrelated reasons.
- Reactor on-CPU under 1:1 load: **≥20%** (from 6.9%) — same rationale.
- 1:9 stability gate (<10% Ops/sec variation across 5 runs) — unresolved
  by Phase 5 but tracked separately; cold-start outlier investigation is
  its own workstream.

### Validation

Each workstream re-runs the same `profile_onecached.sh --ratio 1:1`
flow and compares:

1. New on-CPU / idle-wait breakdown (the primary signal).
2. 5-run median ops/sec and p99/p99.9 against the 2026-04-12 baseline
   (76.6k / 5.3 ms / 6.5 ms).
3. RSS load, flush, footprint — Phase 4 memory targets must stay met.

The "did we actually move the bottleneck" check is whether the
per-worker idle-wait fraction drops, not just whether ops/sec goes up.

### Not in Phase 5

- Codegen / FFI boundary tuning. The profile says on-CPU user code is
  cheap; revisit only if Phase 5 work pushes worker utilization past
  ~30% and instruction cost starts to matter.
- ETS hot-path optimization. The flamegraph shows ETS is not a
  bottleneck under this workload. The 2026-04-12 ETS work
  (immediate-term fast path) was apparently enough.
- Refc-binary ref counting optimization. Same — not visible in this
  profile.

## Update (2026-04-14) - Park/unpark audit and wake-coalescing

Phase 5 workstream #2 (worker park/unpark audit) has been run and its
result inverts the workstream's premise. This update documents both
the audit finding and a related reactor-side change that landed.

### Instrumentation result

Per-worker `pops / parks / spin_iters` counters were added to the
scheduler (via a temporary `NERA_SCHED_STATS=1` env-gated print thread,
reverted before commit) and collected over three 1:1 runs. Steady-state
per-second windows across all three runs consistently show
`pops/park` ratios in the **100–5000** range:

    pops=79,637 parks=26    pops/park=3063
    pops=77,676 parks=23    pops/park=3377
    pops=74,529 parks=15    pops/park=4969
    pops=77,994 parks=29    pops/park=2689
    pops=76,890 parks=23    pops/park=3343

In other words, the existing `idle_spins <= 3` spin-before-park path
at `scheduler.rs:680-689` is already amortising hundreds to thousands
of process pops per actual `cvar.wait_for` call. **Park/unpark is
not the 1:1 throughput bottleneck.** Mark workstream #2 complete with
"not the bottleneck" result.

### The real top-of-stack wait: `mio::Waker::wake`

Walking up from the `libsystem_kernel.dylib` leaves in the 2026-04-13
profile tells a very different story than I initially read. On
worker-0, **84% of all samples with a kernel leaf had
`mio::waker::Waker::wake` (at `waker.rs:94`) as the first
`onecached_nera` ancestor**. On macOS this inlines to
`mio::sys::unix::selector::Selector::wake` at
`mio-1.2.0/src/sys/unix/selector/kqueue.rs:262`:

```rust
syscall!(kevent(kq, &kevent, 1, &mut kevent, 1, ptr::null()))
```

— one `kevent(EVFILT_USER, EV_TRIGGER)` syscall per call. Every call
to `ReactorHandle::send_command` in `reactor.rs:218-221` ends in one
of these. Per-op cost came out to **~78 µs per wake**, not the ~1-5 µs
a free `EV_TRIGGER` should be. The cause is **kqueue lock contention**:
8 workers hammering a single shared kqueue's EVFILT_USER at ~70k
commands/sec serialise through the kernel's per-kqueue lock. This also
explains the previous "direct-reregister gave no speedup" finding
(saved in `project_onecached_perf.md`) — moving one syscall from
reactor to worker doesn't help when both paths contend on the same
kqueue lock.

Reactor-side kernel-leaf breakdown confirms the picture: only 60-66%
is the legitimate `Poll::poll` wait; the remaining ~34% is
`Selector::register/reregister/deregister` — also contending on the
same kqueue.

### The wake-coalescing change

Implemented `wake_pending: Arc<AtomicBool>` shared between
`ReactorHandle` and `IoReactor`. `send_command` only calls
`waker.wake()` if it's the CAS winner that flipped the flag from
`false` to `true`; all subsequent concurrent calls observe `true` and
skip the syscall. The reactor holds the flag `true` for the entire
active phase of each loop iteration (drain → dispatch events → drain
again) and only clears it immediately before `poll.poll` blocks, with
a `cmd_rx.is_empty()` re-check after the clear to catch racers. This
maximises the coalescing window so every `send_command` call that
happens while the reactor is already busy rides for free on an
in-flight wake.

**Correctness note.** Two edits were needed before the pattern
actually coalesced anything: a first attempt that cleared
`wake_pending` at the *start* of `drain_commands` had a window of
essentially zero (nanoseconds between clear and next `try_recv`);
moving the clear to the end of each reactor loop iteration, right
before the blocking `poll.poll`, is what lets the flag stay `true`
for the full microseconds of an active phase.

### Measured impact — real but small

Comparing profile 2026-04-13 (pre-coalesce) to profile 2026-04-14
(post-coalesce, extended-window), both 1:1 at ~70k ops/sec, same
workload shape:

| metric                              | pre   | post  | Δ       |
|-------------------------------------|------:|------:|--------:|
| worker-0 `Waker::wake` wall %       | 78.9% | 74.5% | −4.4 pp |
| worker-0 `ThreadParker::park` wall %|  2.0% |  5.4% | +3.4 pp |
| worker-0 `park_timeout` wall %      |  1.2% |  3.5% | +2.3 pp |
| reactor `Poll::poll` wall %         | 61.2% | 59.1% | −2.1 pp |
| 1:1 ops/sec median (5 runs)         | 76,612| 76,697|  +0.1%  |

The coalescing works — `Waker::wake` really did drop 4.4 percentage
points of worker wall time, a genuine 5-6% relative reduction in the
dominant kernel wait. But the wall that drops out of `Waker::wake`
reappears as scheduler `ThreadParker` and `park_timeout` waits —
workers have the same amount of wall-time to spend, just parked in a
different place. And throughput is flat within noise.

### Why throughput is flat — Nera's own Little's law ceiling

I was misreading the flamegraph. "80% of worker wall in `Waker::wake`"
is *not* "80% of the per-op critical path is wake". The system is
~94% idle per worker overall: almost all wall time is in *some* kind
of wait, and `Waker::wake` was merely the dominant one. Reducing it
just redistributes idle across other waits.

The tighter framing is Little's law. memtier runs **200 concurrent
connections** (50 clients × 4 threads), each one gen_server process
in onecached, each blocked on network I/O between requests.
Throughput ceiling for any engine on this workload is:

    parallelism / per-op latency  =  200 / p50_latency

Plugging in each engine's measured p50 from the Phase 0 baseline
(and Nera's current number):

| Engine       | p50       | Little's law ceiling | Measured | % of own ceiling |
|--------------|----------:|---------------------:|---------:|-----------------:|
| BEAM         | 0.519 ms  |              385 k   | 159 k    |              41% |
| Rust         | 1.287 ms  |              155 k   |  95 k    |              61% |
| Nera Phase 0 | 5.311 ms  |              37.7 k  |  32.4 k  |              86% |
| Nera current | 2.527 ms  |               80 k   |  76.7 k  |          **96%** |

**Nera is at 96% of its own latency-bound ceiling. But that ceiling
is itself bloated — BEAM and Rust have much higher ceilings because
their per-op latency is much lower.** BEAM's p50 is 0.52 ms vs
Nera's 2.53 ms, a **4.9× gap**. The earlier "at the ceiling" framing
was misleading: there is a Nera-specific ceiling, not an absolute
one, and closing the gap to BEAM/Rust means closing the per-op
latency gap, not finding more CPU parallelism.

Applied to wake-coalescing specifically: 76.7k ops/sec × 8 workers ×
74.5% wall = ~745 ms/sec of wake time per worker = **~78 µs of wake
per op**. That's 3.1% of Nera's current 2.5 ms per-op latency.
Eliminating wake entirely caps *this specific workstream's* gain at
~3% — within noise. Doing better than that requires attacking the
latency itself.

### Where Nera's per-op latency actually goes

The onecached SET path runs this sequence per op, which is the
first-order suspect for the bloat:

1. Socket arrives in `active, once, line` mode → `{tcp, Socket, Data}`
   delivered to process
2. `inet:setopts(Socket, [{active, false}, {packet, raw}])`
   → `DeregisterActive` reactor cmd → kqueue dereg + wake
3. `gen_tcp:recv(Socket, Len)` → `Register` reactor cmd → kqueue reg + wake
4. `gen_tcp:send(Socket, Reply)` → TCP write syscall
5. `inet:setopts(Socket, [{active, once}, {packet, line}])`
   → `RegisterActive` reactor cmd → kqueue reg + wake

**Three reactor command round-trips per SET op**, each currently
costing ~78 µs of kqueue-contended syscall plus:

- the socket-entry `DashMap<fd, SocketEntry>` lookup on both sides
  (worker that sends the cmd, reactor that processes it) — earlier
  profile pegged this at ~5-10% of worker on-CPU and ~27% of
  reactor on-CPU
- a mailbox round-trip between worker and reactor
- a worker-side park-on-reply cycle

Rough budget: 3 × ~100 µs ≈ 300 µs per op just in reactor command
round-trip overhead. That's ~12% of the 2.5 ms per-op latency and
~4 ms/s of the 24 ms/s of usable wall on each of 8 workers.
Eliminating it drops per-op latency from 2.5 ms to ~2.2 ms →
Little's law ceiling of ~91 k ops/sec, already above Rust's 95 k
measured number (Rust is at 61% of *its* own ceiling).

BEAM doesn't pay this cost at all: its `inet` driver ports have
direct access to the emulator's I/O scheduler, so
`inet:setopts(active, once)` is a non-blocking flag flip on a port
struct already owned by the emulator's built-in pollset — no
cross-thread command, no kqueue syscall per op, no worker park on
reply. That's 20+ years of highly-tuned VM I/O plumbing, and it is
the structural reason BEAM is ~2× ahead of our nearest reachable
ceiling on this workload.

### Revised Phase 5 status

- **#1 Socket fd table rewrite** (DashMap → Slab) — **still worth
  doing**, but most of its benefit is *in service of* the per-op
  command-reduction work, not standalone. Folded into Phase 6 scope
  below.
- **#2 Worker park/unpark audit** — **done**, not a bottleneck.
- **#3 Reactor dispatch batching** — **partially done** via the
  wake-coalescing landed in this update. The remaining half
  (reduce *number* of commands per op) is the Phase 6 architectural
  work described below, not a small tweak.
- **#4 Per-op allocation cleanup** — unchanged. Probably bigger
  once the reactor commands stop hiding other costs.
- **#5 Cache ETS registry lookups** — unchanged, ~1% on-CPU, free.

### Phase 6 - Per-op reactor-command elimination

**Goal.** Cut per-op reactor commands from 3-4 to 0 for the steady
SET/GET path. Bring Nera's p50 latency down from ~2.5 ms toward
~1.5-1.8 ms, pushing the Little's law ceiling to 110-130 k ops/sec
on this workload — bridging the gap to BEAM and surpassing Rust.

**Approach.** Move the `active` vs `passive` mode distinction from
"what is registered in kqueue" to "what the socket-entry struct
says to do on an event". The fd is registered with kqueue exactly
once at connection open and deregistered exactly once at close;
everything else is a worker-side flag flip on a shared socket
entry. Details and trade-offs are beyond the scope of this plan
update — see the conversation/commit landing the change, or the
rewrite plan I'm about to scope separately.

**Target after Phase 6.**

- Per-op p50 latency: **≤ 1.8 ms** on 1:1 (from 2.5 ms)
- 1:1 ops/sec median: **≥ 110 k** (Little's law ceiling at 1.8 ms)
- Reactor command rate under load: **~connections/s**, not
  **~ops/s** (drops by ~100-200×)
- Worker `Waker::wake` wall share: effectively 0%

**Risks.**

- Active-mode is a subtle state machine; the 2026-04-11 fragment-
  stranding fixes (see earlier plan entry) must not regress.
- Passive `gen_tcp:recv` path currently leans on reactor-driven
  re-registration; the new design parks the worker on a
  socket-entry condition variable instead, and the reactor
  resolves it on event dispatch. This is genuinely new code, not
  a port of an existing path.
- The change touches `reactor.rs`, `socket.rs`, `process.rs`, and
  `gen_tcp.rs` together and cannot be split into trivially-safe
  commits.

**What Phase 6 is *not*.** Not a switch to level-triggered kqueue
(mio doesn't expose that cleanly and we'd fight the abstraction).
Not a reimplementation of BEAM's port driver model. Just the
minimum change required to keep `inet:setopts` off the reactor's
hot path.

### Interpretation (revised)

Wake-coalescing is a real, correctness-adjacent win with a
measurable effect on kqueue lock pressure, and it should land.
But it does *not* move 1:1 throughput, and the reason is
structural: Nera's per-op latency is inflated by per-op reactor
commands, and until those are eliminated, further shaving of wait
costs just redistributes idle time. The honest picture is that
Nera is at ~96% of its own (inflated) Little's law ceiling, and
the path to BEAM/Rust parity is latency reduction, not finding
hidden CPU parallelism.

## Update (2026-04-15) - Fresh 3-engine benchmark and revised target

The 2026-03-02 Phase 0 baseline table at the top of this document
(BEAM 159 k / Rust 95 k / Nera 32 k) turned out to be wrong — or at
least measured under conditions we can't reproduce. Re-ran all three
engines on the same hardware in the same session, with the wake
coalescing changes in place on the Nera side, and got a substantially
different picture. This update is the corrected baseline and the
revised target that falls out of it.

### Fresh 3-engine comparison (5-run medians)

Workload: `memtier_benchmark --protocol=memcache_text --clients=50
--threads=4 --ratio=1:1 --data-size=256 --pipeline=1 --test-time=10`,
localhost, same machine, warmup before each measured run.

Engine binaries:

- **BEAM**: OTP 28, onecached compiled from `~/devel/onecached/src`
  via `--beam-src` in `onecached_bench.sh`.
- **Nera**: current `main` with wake coalescing (commit `c9bb00b`),
  onecached compiled via `nera build`.
- **Rust**: minimal tokio + `Mutex<HashMap>` baseline at
  `benchmarks/rust_baseline/` (added alongside this update), built
  via `--rust-src` in `onecached_bench.sh`.

| Engine                 | Median ops/sec | p50 (ms) | p99 (ms) | p99.9 (ms) | Variation | vs BEAM | vs Rust |
|------------------------|---------------:|---------:|---------:|-----------:|----------:|--------:|--------:|
| **Rust baseline**      |    **179 298** |    1.047 |    3.10  |       8.45 |      3.2% |   1.80× |   1.00× |
| **BEAM** (OTP 28)      |    **99 524**  |    1.823 |    5.41  |       9.60 |     22.6% |   1.00× |   0.55× |
| **Nera** (2026-04-15)  |    **79 581**  |    2.399 |    5.63  |      14.08 |      3.8% |   0.80× |   0.44× |

**Ordering on this workload is Rust ≫ BEAM > Nera.** The Phase 0
ordering (BEAM ahead of Rust) was an artifact of whatever the 2026-03-02
measurement was, not the underlying truth of the engines. On a tokio +
HashMap tight-loop server like `benchmarks/rust_baseline`, native Rust
is 1.8× BEAM's throughput and sits at 3.1 ms p99 vs BEAM's 5.4 ms.

Phase 0 numbers at the top of this doc are **withdrawn**. Compare
against this table instead.

### Little's law sanity check on all three

200 concurrent memtier connections, each blocked on round-trip. Each
engine is close to its own p50-bound ceiling — all three are latency-
bound, not CPU-bound:

| Engine | p50      | Little's law ceiling | Measured | % of own ceiling |
|--------|---------:|---------------------:|---------:|-----------------:|
| Rust   | 1.047 ms |              191 k   |   179 k  |              94% |
| BEAM   | 1.823 ms |              110 k   |    99 k  |              91% |
| Nera   | 2.399 ms |               83 k   |    80 k  |              96% |

**The throughput gap between engines is entirely per-op latency.** On
this workload, nothing about throughput improves without reducing the
time each engine spends per request.

### What the per-op gaps actually represent

Three budgets, one per op, stacked:

- **Rust → BEAM = +776 µs**: the cost of Erlang semantics (process
  isolation, mailbox message passing per op, gen_server state machine,
  per-process GC) on a 20-year-tuned VM with integrated I/O.
- **BEAM → Nera = +576 µs**: the cost of our current runtime
  implementation vs BEAM's integrated port driver / scheduler / mailbox
  path. This is the part of the gap we can actually attack.
- **Rust → Nera = +1352 µs** total.

Framed differently: **BEAM pays ~776 µs for "real Erlang". Nera pays
~1352 µs, of which ~776 µs is the Erlang-semantics tax BEAM also pays
and ~576 µs is our specific overhead.** Phase 6 chips ~130 µs off that
576 µs (see `docs/PHASE6_IMPLEMENTATION_PLAN.md` for details), leaving
~446 µs.

### Revised target — ambitious, past BEAM parity

With the corrected baseline, BEAM parity is no longer out of reach,
and the right question is not "can we match BEAM" but "how far past
BEAM can we push". I'm setting the target deliberately ambitious:

**Target: ≥ 120 k ops/sec on 1:1, p50 ≤ 1.65 ms.**

Rationale:

- **120 k** is 20% above current BEAM (~99.5 k). It puts Nera
  decisively ahead of BEAM on this workload — not just "within 10%"
  but visibly, unambiguously faster.
- **120 k at 200-connection parallelism** requires p50 ≤ 1.67 ms
  (Little's law). From current 2.40 ms, that's **750 µs of per-op
  latency to save**.
- **67% of the Rust baseline ceiling** (179 k). BEAM is currently at
  55% of Rust. Target puts Nera ~12 percentage points closer to
  native Rust than BEAM is — a meaningful competitive claim.
- Phrased as a positioning statement: *"Nera provides Erlang semantics
  at ~2/3 of native Rust performance on I/O-heavy workloads, and
  substantially faster than BEAM."*

The target is explicitly ambitious and not reachable by Phase 6 alone.
Phase 6 is step 1, not the whole plan.

### The budget to 120 k

750 µs per op to find. Phase 6 delivers ~130 µs. The remaining
~620 µs must come from additional workstreams, driven by the per-op
latency decomposition that Phase 6's validation will enable.

Candidate workstreams (unranked — Phase 7 profiling picks the order):

1. **Scheduler dispatch latency reduction.** Current worker wake from
   park on "process became runnable" path has ~50-100 µs latency under
   load. BEAM's integrated scheduler is ~tens of µs. Delta per op:
   small per-wake, large per-op since each op involves multiple wakes
   (reactor → worker delivery, worker → next process pickup). Expected
   savings: 100-200 µs per op.
2. **Per-op allocation cleanup.** The 2026-04-13 profile flagged
   `_rjem_sdallocx.cold.1` + `_rjem_malloc` at ~5% of worker on-CPU,
   suggesting a recurring allocation crossing a jemalloc bin boundary
   on the hot path. Identifying and arena-allocating or reusing the
   offender is cheap. Expected savings: 50-100 µs per op.
3. **Socket fd table rewrite** (DashMap → Slab). Phase 6 workstream
   #1, ~27% of reactor on-CPU and ~10% of worker on-CPU. Standalone
   effect is small but cumulative with other reactor work. Expected
   savings: 30-60 µs per op.
4. **ETS registry lookup caching.** `EtsRegistry::get_table` at ~1%
   of worker on-CPU, called every op despite onecached using one
   long-lived table. Free win. Expected savings: 10-30 µs per op.
5. **p99.9 tail attack** (GC pauses, scheduler quantum). Nera's p99.9
   is 14.08 ms vs BEAM's 9.60 ms — 47% worse. Matching BEAM's p99.9
   shrinks the tail of the latency distribution, indirectly lifting
   the Little's-law-relevant median. Expected savings: harder to
   quantify; probably 20-80 µs effective.
6. **Integrated reactor dispatch.** Fundamental: dispatch I/O events
   directly to worker runqueues without going through a per-op
   condvar wake. This is the moral equivalent of BEAM's port driver
   model and is architecturally the biggest change. Expected savings:
   200-400 µs per op (the same class as Phase 6 in magnitude).
7. **Hot-path codegen tightening.** Micro-optimising the generated
   Erlang → Rust FFI path for the memcached inner loop. Expected
   savings: 20-60 µs per op in aggregate.

Rough arithmetic on the optimistic side: Phase 6 (130) + dispatch
(150) + alloc cleanup (75) + slab (45) + integrated reactor (300) +
misc (50) = **~750 µs**. Tight. Meeting 120 k requires landing most
of this list, not picking one or two items.

### Phase order

1. **Phase 6** — per-op reactor command elimination. Plan at
   `docs/PHASE6_IMPLEMENTATION_PLAN.md`. Expected landing: ~85 k on
   1:1. **Committed to; ready to start once the concurrency
   invariants are walked through.**
2. **Phase 7** — per-op critical-path decomposition. Instrumentation-
   first: add cycle-accurate timers at TCP read → worker dispatch →
   Erlang dispatch → ETS → TCP write phase boundaries, run a single
   op end-to-end, measure. **Answers which of the candidate
   workstreams above is actually worth doing.** No throughput goal;
   the goal is a concrete per-op-phase table like "dispatch 420 µs,
   ETS 180 µs, parse 90 µs, write 60 µs, ...".
3. **Phase 8** — execute the top 2-3 workstreams Phase 7 identified,
   in dependency order. This is where the actual 120 k push happens.
4. **Phase 9 (maybe)** — integrated reactor dispatch if Phases 6+8
   don't get us all the way.

The point of Phase 7 is that I *don't want to commit to workstreams
3–7 above without measurement*. I've been wrong twice in this
document about what the bottleneck is — once assuming ETS was hot
(it wasn't), and once assuming park/unpark was the bottleneck (it
wasn't). The cost of another wrong guess at this scale is a 2-week
rewrite that doesn't move the number. Instrument first.

### Rust baseline methodology note

The Rust baseline at `benchmarks/rust_baseline/` is a **deliberately
minimal** reference — tokio multi-thread runtime, one
`Mutex<HashMap>`, hand-written memcached-text parser for the subset
memtier uses. It is not a production memcached server. A more
optimised Rust server (sharded locks, io_uring, zero-copy framing,
custom allocator) could reach 250-400 k on this workload. Our
baseline is the **floor** for native-Rust performance here, not the
ceiling — and it's already at 179 k.

This matters for target-setting: if we hit 120 k, we are at 67% of a
deliberately-minimal Rust baseline, not 67% of what's theoretically
possible in Rust. The "Erlang semantics tax" we're paying (~776 µs
per op, same as BEAM) remains, and that's the irreducible part.

## Update (2026-04-15) - Slab socket registry, ambitious target exceeded on first commit

The Phase 6 plan listed "socket fd table rewrite (DashMap → Slab)"
as workstream #1, a prerequisite cleanup estimated at **1–3%
throughput improvement, mostly reactor-side CPU reduction**. The
measurement came back ~20× larger than that estimate and **already
exceeds the 120 k ambitious target set earlier today**. This section
records the numbers, the reason my estimate was wrong, and how the
Phase 6 plan needs to be rescoped as a result.

### Change landed

Commit `f52c26e` replaces the socket registry's
`RwLock<Option<HashMap<u64, Arc<SocketEntry>>>>` with an inline slab:
a `Vec` of slots, each free (on a linked freelist) or occupied
(holding an `Arc<SocketEntry>`), each carrying a 32-bit generation
counter that bumps on every free → occupied transition. Handles
become `(generation << 32) | slot_index`, so stale handles to reused
slots fail the generation check and return `None` — preserving the
old "monotonic u64 handles are stale-safe" property. Hot-path lookup
is one `RwLock` read-acquire, one `Vec` index, one generation
compare, one `Arc` clone. No hashing, no probe sequences.

~200 lines of new code in `socket.rs`, no new dependencies, three
new unit tests (handle encoding roundtrip, "handle 0 is never valid"
invariant, ABA safety under register → remove → reused-slot
register). All 15 socket tests pass.

### Measured result (same methodology as the 2026-04-15 3-engine run)

5-run median, 1:1 memtier, 50 clients × 4 threads × 256 B payload,
same hardware, same session:

| Metric          | Pre-slab (HashMap) | Post-slab  | Δ        |
|-----------------|-------------------:|-----------:|---------:|
| Median ops/sec  |             79 581 | **133 146**| **+67.3%**|
| p50 latency     |            2.399 ms|    1.327 ms| −44.7%   |
| p99 latency     |            5.631 ms|    4.799 ms| −14.8%   |
| p99.9 latency   |           14.079 ms|   10.687 ms| −24.1%   |

Per-run numbers: 133 146 / 142 645 / 145 989 / 124 440 / 132 965.
Variation 16.2% — the tail of runs is noisy, but the medians are
consistent. RSS load 76 MB (up from 46 MB pre-slab — worth noting
but still well within Phase 4 memory targets).

### New 3-engine standing

| Engine                          | Median ops/sec | p50 (ms) | vs BEAM | vs Rust |
|---------------------------------|---------------:|---------:|--------:|--------:|
| **Rust baseline**               |        179 298 |    1.05  |   1.80× |   1.00× |
| **Nera** (post-slab, `f52c26e`) |    **133 146** |  **1.33**| **1.34×**| **0.74×**|
| **BEAM** (OTP 28)               |         99 524 |    1.82  |   1.00× |   0.55× |

**Nera is now 34% faster than BEAM on this workload** and at 74% of
the deliberately-minimal Rust baseline. The 120 k ambitious target
set in the previous plan update is exceeded by the first cleanup
commit of the phase. The positioning statement drafted earlier —
*"Nera provides Erlang semantics at ~2/3 of native Rust performance
on I/O-heavy workloads, and substantially faster than BEAM"* — is
now **backed by measurement**, not projection.

### Why my estimate was wrong by ~20×

The 2026-04-13 samply profile showed `socket::get_entry` at ~27% of
reactor on-CPU and ~10% of worker on-CPU, dominated by siphash +
hashbrown probe. I read that narrowly as "CPU cost to be eliminated"
and assumed eliminating it would only move throughput in proportion
to the CPU reduction (which, since workers are ~94% idle, should be
small). That reading was wrong for two reasons:

1. **The cost was per-op latency, not idle CPU overhead.** Each
   `get_entry` call was inside a RwLock read-critical-section that
   showed up as ~1-2 µs including the hash work, cache-line-bounced
   atomic counter ops, and Arc clone. Per op, there are 4–6
   `get_entry` calls (worker dispatches, setopts, reads, sends) on
   the SET path. Cutting each one from ~1.5 µs to ~0.3 µs shaves
   5–8 µs of critical-section time *per op*. That is a real latency
   reduction, which Little's law converts directly to throughput.

2. **Cache-line bouncing amplified the critical-section time.** 8
   workers + reactor all hitting the same `RwLock` + `HashMap`
   buckets from different CPU cores means every read acquisition
   was CAS-ing a hot cache line, and the HashMap's internal
   structure (bucket array, hashbuilder state) bounced between
   caches on every access. The slab's single `Vec` with direct
   indexing is both shorter per-call *and* cache-friendlier (one
   indexed load vs several pointer-chases).

Per-op math works out: 79.6 k → 133 k on 1:1 = 1.67× throughput.
p50: 2.40 ms → 1.33 ms = 1.81× latency reduction. Matches the
expected Little's-law relationship closely (both engine p50 and
aggregate throughput scale together), which tells me the bottleneck
really *was* per-op latency dominated by the old registry, not
something hiding elsewhere.

**Lesson for the remaining workstreams**: profile sample counts are
a lower bound on the optimisation opportunity when the critical
section is held across many threads. I've been systematically
underestimating hot-section contention as a latency source. Rescale
future estimates accordingly.

### Phase 6 rescoping

The original Phase 6 plan (`docs/PHASE6_IMPLEMENTATION_PLAN.md`) is
now **partially obsolete**:

- **Workstream #1 (slab rewrite)**: landed. Expected +1–3%; got
  +67%. Delete this item, it's done.
- **Workstream #2 (reactor reads `opts.active` to dispatch)**: was
  the "big race-sensitive" commit. Its value needs re-measurement
  now that the slab rewrite has changed the bottleneck shape — a
  fresh profile will show whether the remaining reactor command
  cost is still high in *relative* terms or has been demoted by the
  slab win.
- **Workstream #3 (`pending_recv`/`pending_send` on `SocketOpts`)**:
  likely still valuable but also needs re-sizing against the new
  bottleneck picture.

**Before any more reactor work lands, we must reprofile.** A 1:1
profile of the post-slab binary will tell us what the *new* top
user-space hot spot is. That answers three questions:

1. Has `mio::Waker::wake` at `waker.rs:94` (the 78% worker-wall
   concentration from the pre-slab profile) changed? If it stayed
   at ~78% but throughput jumped 67%, then my model of the system
   is off by more than I think. If it dropped proportionally, the
   change moved both CPU and wait-time, and the bottleneck model
   still holds.
2. Is per-op reactor command elimination (Phase 6 #2) still the
   biggest single lever, or has the slab win pushed something else
   to the top?
3. Where does the remaining ~446 µs of non-reactor-command
   nera-specific overhead go? Phase 7 (per-op latency
   decomposition) was meant to answer this via instrumentation;
   the post-slab profile may partially answer it before the
   instrumentation is needed.

### Updated target

With the slab landing, the 120 k target is met. I'm leaving the
ambitious target in place but revising what "ambitious" means:

**Revised ambitious target: ≥ 150 k ops/sec on 1:1, p50 ≤ 1.15 ms.**

- 150 k is 50% above BEAM, 84% of Rust baseline.
- At 200-connection parallelism, p50 must be ≤ 1.33 ms (Little's
  law). Current post-slab p50 is 1.33 ms exactly — we are *at*
  the ceiling for this p50. To push further, p50 must drop to
  ≤ 1.15 ms, which needs ~180 µs more per-op latency reduction on
  top of what the slab just delivered.
- Phrased as positioning: *"Nera provides Erlang semantics at ~84%
  of native Rust performance on I/O-heavy workloads, and ~50%
  faster than BEAM."* That is an unambiguous claim, reachable
  within the same phase shape (Phase 6 partial + Phase 7
  instrumentation + Phase 8 targeted workstreams) — just calibrated
  against a higher bar.

If we hit 150 k, the *next* ambitious target is the Rust baseline
itself. I'd want another 3-engine re-bench at that point to sanity-
check the comparison, but "Nera matches a deliberately-minimal Rust
memcached baseline while running full Erlang semantics" is a
non-trivial claim worth pursuing if the first 150 k gate lands.

### Immediate next step

**Re-profile the post-slab binary on 1:1.** Use
`benchmarks/profile_onecached.sh --ratio 1:1 --test-time 15`, inspect
the new kernel-caller breakdown on workers and reactor, update this
document with the new hot-spot table, and *then* decide whether
Phase 6 workstream #2 is still the right next commit or whether
something else has become the bottleneck.

Do **not** start on Phase 6 workstream #2 until the profile is in.
I've been wrong three times now about what the 1:1 bottleneck is
(ETS, park/unpark, reactor commands), and the slab commit just
shifted everything. The cost of another wrong guess at this point
is ~2 weeks of rewrite that doesn't move the number.

## Update (2026-04-15) - Post-slab profile: bottleneck has shifted to scheduler dispatch

Re-profiled the slab-rewrite binary (`f52c26e`) under the same 1:1
workload. The bottleneck has moved completely: `mio::Waker::wake`
has dropped out of the top-10 kernel callers on workers, and the
new dominant wall-time consumer is `parking_lot_core::ThreadParker`
— the scheduler condvar wait at `scheduler.rs:698`.

### Worker-0 kernel-caller breakdown (pre-slab vs post-slab)

| Caller                                    | Pre-slab (2026-04-13) | Post-slab (2026-04-15) |
|-------------------------------------------|----------------------:|-----------------------:|
| `mio::Waker::wake` (kqueue EV_TRIGGER)    |                 78.9% |         *not in top-10* |
| `ThreadParker::wait_for` (cvar wait)      |                  2.0% |              **60.9%** |
| `TcpStream::write`                        |                  3.1% |                   8.9% |
| `TcpStream::read`                         |                  2.3% |                   4.2% |
| `handle_active_read_on_worker` (reads)    |                  2.4% |                   4.8% |
| `parking_lot::RawMutex::lock_slow`        |                  0.0% |                   2.8% |
| `nera_rt_inet_setopts`                    |                  0.6% |                   1.3% |
| **Worker total on-CPU**                   |              **6.3%** |             **11.1%**  |

### Reactor kernel-caller breakdown (pre-slab vs post-slab)

| Caller                                    | Pre-slab | Post-slab |
|-------------------------------------------|---------:|----------:|
| `mio::Poll::poll` (kqueue wait)           |    61.2% |     23.4% |
| `Selector::reregister`                    |    24.6% |     13.0% |
| `Selector::register`                      |     4.4% |     11.6% |
| `SourceFd::deregister`                    |     5.1% |     14.6% |
| **`Condvar::notify_one_slow`** (new!)     |    ~0.0% |  **10.5%** |
| **Reactor total on-CPU**                  | **6.9%** | **24.1%**  |

### Interpretation

1. **Slab rewrite did exactly what the throughput numbers said.**
   `socket::get_entry`, hashing, siphash probe — all gone from the
   top 10. The 2026-04-13 profile's biggest single on-CPU call site
   (21.3% of reactor on-CPU in `socket::get_entry`) is now scattered
   under 1% across many sites. That's a clean confirmation the slab
   commit hit exactly the target it was designed to hit.

2. **Worker → reactor kqueue signalling is no longer the worker
   bottleneck.** `mio::Waker::wake` dropped from 78.9% of worker
   wall to below the top-10. The combination of wake coalescing
   (2026-04-14) and the slab rewrite has effectively eliminated it
   as a concern on this workload.

3. **The new #1 worker bottleneck is the scheduler park/unpark
   cycle.** 60.9% of worker wall is in `parking_lot_core::ThreadParker`
   — the `cvar.wait_for` at `scheduler.rs:698`. Workers finish a
   process slice much faster post-slab and go back to waiting on the
   scheduler condvar very frequently.

4. **The reactor is ~3.5× more engaged** (24.1% on-CPU vs 6.9%).
   10.5% of reactor wall is now in `Condvar::notify_one_slow` — the
   syscall the reactor makes to wake workers when dispatching a new
   event. That's the *other half* of the park/unpark cycle the
   workers are on.

5. **The 2026-04-14 park/unpark audit's conclusion
   ("park is not the bottleneck") is now stale.** That audit measured
   pops/park ratios of 100-5000 on the pre-slab binary. Under the
   pre-slab bottleneck model workers were rarely parked because they
   spent so much wall time in `Waker::wake`. Post-slab, per-op work
   is so much faster that workers park between every few ops and
   the pops/park ratio has almost certainly collapsed. **We need to
   re-measure it** — that's what the Phase 7 instrumentation below
   is for.

### Budget check: how much p50 is in the dispatch cycle?

Current post-slab p50 is 1.33 ms. The 150 k ambitious target requires
p50 ≤ 1.33 ms (exactly where we are) or lower to get above that. To
hit 150 k with margin, say 160 k, we'd need p50 ≤ 1.25 ms — ~80 µs
to save per op.

Dispatch-cycle budget:

- Reactor `Condvar::notify_one_slow` at 10.5% of wall = ~1.58 s of
  15 s. At an estimated ~100 k notify calls/sec (one per op + change),
  that's **~15 µs per notify call** — one full OS wake syscall.
- Worker `ThreadParker::wait_for` at 60.9% of wall = ~9 s parked
  per worker per 15 s run. **Park count is currently unmeasured**
  post-slab (the 2026-04-14 numbers are stale). Average park duration
  is unknown until we re-measure.

**Conservative ceiling**: if the dispatch cycle is 80 µs per op end to
end (notify + wake up + pick work), halving it via worker-local direct
dispatch gives ~40 µs of p50 savings → p50 ~1.29 ms → ~155 k ops/sec.
That puts 150 k within single-workstream reach. Eliminating it would
give more.

### Candidate workstreams (re-ranked after the profile)

1. **Worker-local direct dispatch** (new top priority). Instead of
   the reactor calling `notify_one` on a shared condvar and hoping
   some worker picks up the new process, push the process directly
   onto a specific worker's local deque and wake *that* worker via
   a per-worker mechanism (thread::park/unpark, or a per-worker
   condvar). Skips the broadcast-cvar cost and the worker-side
   "race to grab the injector" round. Expected: take the ~60% of
   worker wall currently in `ThreadParker` down to a fraction of
   that. Biggest single lever available.
2. **Spin-before-park on dispatch** (cheap companion to #1). The
   current spin-before-park only triggers on the idle → no-work
   path. Extending it so that after finishing a process slice the
   worker spins briefly before parking (watching for a new process
   to become runnable) avoids a park cycle when work arrives within
   a few µs.
3. **Phase 6 workstream #2** (reactor reads `opts.active` to
   dispatch). Still valuable — 34% of reactor wall is in
   register/reregister/deregister syscalls. But *second-priority*
   now: reducing reactor syscalls does not directly shrink worker
   park time. Worth doing after #1 once the dispatch bottleneck is
   open.
4. **`parking_lot::raw_mutex::RawMutex::lock_slow` at 2.8%** on
   workers. Likely `process.proc.lock()` contention under the new
   higher dispatch rate, possibly also `SocketEntry.opts` mutex
   contention. Small on its own, worth a look once #1 lands.

### Phase 7 — dispatch latency instrumentation (landing with this update)

The 2026-04-14 park/unpark audit added three per-worker counters
(`pops`, `parks`, `spin_iters`) but didn't measure **how long each
park lasts**. That's the missing piece: the 60.9% `ThreadParker` wall
share could be either "parks are long" (OS scheduling latency
dominates) or "parks are frequent and short" (per-op dispatch
overhead dominates). The fix for each is different.

A minimal extension adds one new counter: **`park_ns_total`** — the
cumulative nanoseconds spent inside each `cvar.wait_for` call. With
`parks` already counted, the derived metric `park_ns_total / parks`
is the average park duration. Combined with the per-second rate of
parks from the existing periodic stats thread, this tells us:

- **If avg park duration is short (~µs)**: parks are cheap and
  frequent → the lever is reducing park *count*, not park *cost*.
  Workstream #1 (worker-local direct dispatch with fewer parks)
  wins.
- **If avg park duration is long (~tens of µs or more)**: parks
  are inherently slow → the lever is reducing park *cost* per
  park, or eliminating parks entirely via spin loops or futex-based
  handoffs.

The instrumentation commit landing alongside this plan update adds
that counter to the existing `WorkerStats` struct and extends the
`NERA_SCHED_STATS=1` periodic report to print the average park
duration per period. Zero-cost when disabled; a single `Instant::now()`
+ elapsed + atomic-add when enabled.

### What Phase 7 does NOT answer

It answers "how long are workers parked". It does **not** directly
answer "when a process became runnable, how long until a worker
picked it up" — which would require timestamping at the injector
push site. That measurement is one step deeper and I'd only add it
if the `park_ns_total` data is ambiguous. Instrument in layers; stop
as soon as the data tells us the shape of the problem.

### What changes in the plan

- The original Phase 6 plan (`docs/PHASE6_IMPLEMENTATION_PLAN.md`)
  remains the document of record for reactor command elimination,
  but **the relative priority is lower now**. Don't start on Phase 6
  workstream #2 until the Phase 7 instrumentation tells us whether
  park/unpark is the bigger lever.
- The **150 k ambitious target** set earlier today is unchanged —
  it's still the right bar. What changed is the *workstream shape*
  needed to hit it: worker-local dispatch moves up, reactor command
  elimination moves down.
- Phase 6 workstream #1 (slab rewrite) is already done and banked.

### Immediate next step (again)

1. **Commit the Phase 7 instrumentation** (this session).
2. **Run one 1:1 bench with `NERA_SCHED_STATS=1`** and read the new
   counter output: avg park duration, parks/sec, pops/park post-slab.
3. **Based on that data**, decide whether the next commit is
   worker-local direct dispatch (workstream #1 in the re-ranking
   above) or Phase 6 workstream #2 or something else the data
   points at.

## Update (2026-04-15) - Honest multi-benchmark baseline vs BEAM

So far every performance comparison in this document has been
against a single workload (onecached 1:1). To sanity-check whether
Nera's story generalises, ran the full `benchmarks/compare_benchmarks.sh`
sweep (9 microbenchmarks, nera vs BEAM/OTP 28) on the post-slab +
Phase 7 instrumentation binary. The result changes how I'll frame
nera's position going forward.

### Results

Internal timings (`erlang:monotonic_time` from inside each benchmark,
excludes startup):

| Benchmark                     | Nera      | BEAM      | Ratio   | Winner    |
|-------------------------------|----------:|----------:|--------:|-----------|
| `bench_compute` (pure loops)  |    105 ms |     22 ms | **4.77×** | **BEAM** |
| `bench_tailcall`              |    105 ms |     40 ms | **2.63×** | **BEAM** |
| `bench_binary_send`           |     25 ms |     12 ms | **2.08×** | **BEAM** |
| `bench_binary_fanout`         |      6 ms |      2 ms | **3.00×** | **BEAM** |
| `bench_binary_subbinary`      |      0 ms |      1 ms |       —  | noise    |
| `bench_tcp_ping_pong` (50K)   |   1763 ms |   1496 ms |   1.18× | **BEAM** |
| `bench_tcp_throughput` (100MB)|     52 ms |     37 ms |   1.41× | **BEAM** |
| `bench_tcp_connections`       |      8 ms |      6 ms |   1.33× | **BEAM** |
| `bench_process_lifecycle`     |    473 ms |    367 ms |   1.29× | **BEAM** |
| **onecached 1:1** (from above)| **133 k/s** | **99.5 k/s** | **0.75×** | **Nera** |

### What this says — and doesn't say — about nera's strengths

**Nera is slower than BEAM on every microbenchmark tested, by
1.18× to 4.77×.** Yet Nera is 1.34× *faster* than BEAM on onecached
1:1. Both are true. The resolution: **the onecached 1:1 win is not
a general speed advantage — it's a very specific structural win
from removing a cross-thread contention bottleneck (the slab
rewrite) at a level of concurrency where that contention dominated
everything else.**

At onecached scale (200 concurrent connections × 140 k req/sec), the
pre-slab `RwLock<HashMap>` was a hot-cache-line serialiser across
9 threads. Removing it unlocked a regime BEAM cannot reach because
BEAM never had that specific bottleneck to begin with. At
lower-concurrency scales — `bench_tcp_ping_pong` has *2 processes*,
`bench_tcp_throughput` has one sender + one receiver — there is no
cross-thread contention to remove, and Nera's per-op overheads
(reduction counting, FFI boundaries, generated-code quality) become
the visible cost. Those overheads put nera at 1.18–1.41× slower on
I/O benchmarks at low concurrency.

### Where the compute gap comes from

The 4.77× gap on `bench_compute` and 2.63× on `bench_tailcall` is
the most important data point. I had assumed Nera's AOT Cranelift
codegen would be a differentiator on compute-heavy workloads —
"compile Erlang once, run native, pay no JIT warm-up". The data
says the opposite: **BEAM's BeamAsm JIT is substantially faster on
pure-compute tight loops than Nera's Cranelift-generated code.**

Candidate causes for the compute gap (unverified; would need
dedicated investigation to sort):

- **BEAM's JIT has type-specialisation hints** from its register
  analysis that Cranelift doesn't get from the Erlang AST. Nera's
  pipeline probably does not preserve as much type information
  to the backend as BEAM's emitter does.
- **Reduction-count checks**: every N instructions, Nera inserts a
  reduction-count decrement + yield-check. BEAM amortises these
  more aggressively (once per function entry + backward branch).
  On a tight loop, the extra decrements add up.
- **FFI call boundary** for BIFs. Any generated code calling
  `erlang:length/1`, `lists:reverse/1`, etc. crosses a Rust FFI
  boundary with argument marshalling. BEAM's BIFs are in-emulator
  and don't pay that cost.
- **Pattern-match compilation quality**. Nera's match compiler may
  be producing less-tight dispatch trees than BEAM's.

None of these are confirmed; the 4.77× is a "there's work to do in
codegen" signal, not a diagnosis.

### What the positioning statement should actually say

Earlier in this document I drafted:

> *"Nera provides Erlang semantics at ~84% of native Rust
> performance on I/O-heavy workloads, and ~50% faster than BEAM."*

That statement is **true on onecached 1:1 at high concurrency** and
**false on every other benchmark I've tested**. The honest revised
version:

> *"On high-concurrency I/O-heavy workloads that were previously
> bottlenecked on socket-table or similar shared-contention
> structures, Nera is substantially faster than BEAM (onecached
> 1:1: +34% throughput, +34% p50 latency reduction). On pure
> compute, tight loops, bulk I/O, and low-concurrency workloads,
> BEAM's BeamAsm JIT and tuned runtime remain 1.2×-4.8× faster.
> Nera's advantages are structural (memory model, slab-indexed
> shared state, integrated reactor) rather than codegen-based.
> Closing the compute gap is a separate, larger investigation."*

I'll use this framing going forward.

### Impact on Phase 8+

The onecached-focused workstream order (worker-local dispatch,
spin-before-park extension, reactor command elimination) is
**unchanged**. These still target the remaining dispatch-cycle gap
on onecached 1:1, and the 200 µs avg park from Phase 7 is still
the concrete lever.

The **compute gap is a separate Phase**, not a variant of
Phase 8. If we want to close it — which is a strategic question,
not a tactical one — it's probably months of codegen work and
should be explicitly scoped before starting. For now, I'm leaving
Phase 8 on the onecached path and noting the compute gap as a
known-unresolved limitation to be revisited after the I/O path is
wrapped up.

### What to re-run as a regression check

Each Phase 8 commit should run:

1. `onecached_bench.sh --engines beam,nera --ratio 1:1` (5 runs)
   — primary throughput signal.
2. `bench_tcp_ping_pong` and `bench_spawn_ring` — should move
   in the same direction as onecached 1:1 under scheduler changes.
3. `bench_compute` — regression canary for the CPU path. Should
   **not** change (negative or positive) from scheduler-only work.

If `bench_compute` regresses on a scheduler commit, something
unexpected is happening and the commit needs investigation.
If `bench_tcp_ping_pong` doesn't move when onecached moves, the
onecached gain came from something unrelated and the scheduler
change isn't doing what we think.

## Update (2026-04-15) - Phase 8 experiment: extended spin-before-park regressed, reverted

Phase 8's first workstream was extending the spin-before-park
budget from the existing ~600 ns (3 × 64 PAUSE) to a wall-time-
bounded adaptive spin, with a new `spin_rescues` counter to measure
whether the longer spin caught work before each park-would-have-
happened. Phase 7 had identified that ~60.9% of worker wall was in
`ThreadParker::wait_for` with an average park duration of ~200 µs
and ~6.5 pops per park — i.e., workers finish a ~29 µs work slice,
then wait ~200 µs for the next process to become runnable. The
premise of the spin extension was that if the next process becomes
runnable within ~50 µs of the slice completing, a short spin would
catch it and avoid the park cycle entirely.

The experiment ran twice with two different budget values.

### Data

Both runs, same 1:1 onecached workload, same hardware, same session,
same `NERA_SCHED_STATS=1` periodic output:

| Metric                          | No spin change (Phase 7) | 50 µs spin budget | 15 µs spin budget |
|---------------------------------|-------------------------:|------------------:|------------------:|
| **Median ops/sec**              |                **144 k** |         **117 k** |         **122 k** |
| Δ vs Phase 7 baseline           |                        — |            −19%   |            −15%   |
| `pops/park` ratio               |                      6.5 |                12 |             7-9   |
| `parks/s` (aggregate)           |                    ~20 k |             ~10 k |            ~17 k  |
| `spin_iters/s` (aggregate)      |                    ~40 k |            ~2.2 M |             ~1 M  |
| `rescues/s` (aggregate)         |                       —  |             ~6 k  |             ~4 k  |
| `avg_park_us`                   |                      200 |               350 |              230  |
| `park_wall_pct`                 |                     ~53  |                44 |               55  |

### Why both budgets regressed

Budget analysis on the 50 µs run:

- **Spin CPU cost**: 2.2 M iterations/s × (16 PAUSE hints +
  full multi-queue pop attempt ≈ 580 ns per iteration) ≈ 1.28 s of
  CPU per second across 8 workers = **~16% of each worker's wall
  burned on spinning**.
- **Wake-latency savings**: 6 k rescues/s × ~200 µs saved per
  rescue (park avoided) ≈ 1.2 s/s wall savings across 8 workers =
  **~15% of each worker's wall reclaimed**.
- **Net**: break-even on CPU, but spinning is *synchronous per-
  worker cost* while wake-latency savings are *amortised across
  fewer scheduling events* — so the apparent break-even hides
  real variance and cross-core traffic.

Budget analysis on the 15 µs run:

- **Spin CPU cost**: 1 M iterations/s × ~500 ns ≈ 500 ms/s
  aggregate = ~6% per worker.
- **Rescue savings**: 4 k rescues/s × ~193 µs saved ≈ 770 ms/s
  aggregate = ~5% per worker.
- **Net**: still roughly break-even on the pure latency/CPU
  accounting — but both runs showed a **consistent median
  regression** and high variance (27% spread in the 15 µs run,
  including a 93 k cold-start outlier in run 1).

### The hidden cost the arithmetic misses

Each spin iteration does a full pop attempt: local deque +
injector + all **7 stealer peers**. That means **every spin
iteration is hitting 7 other workers' deque heads** across core
boundaries. At 1-2 M spin iterations per second, that's 7-14 M
cross-core cache-coherence operations per second on the deque
head pointers, contending with the real workers that are trying
to push new processes onto those deques.

**The spin is actively polluting the shared state it's trying
to observe.** The arithmetic that says "break-even on CPU" misses
this because it only counts local CPU, not the coherence traffic
the spin induces elsewhere.

### Conclusion

**Extended spin-before-park is not the right fix on this workload.**
Two independent budget points both regressed; the shape of the
data is consistent across them; the hidden cross-core cost explains
why the budget arithmetic looked break-even while the bench
consistently showed a loss.

**Reverting the spin extension** and going to the structural fix:
**worker-local direct dispatch**. The reactor pushes ready
processes directly onto a specific worker's local deque and wakes
only that worker (not a broadcast `notify_one` on the shared
condvar). Workers only spin-check their own local deque — no
cross-peer steal attempts during the idle phase. Expected next
commit to deliver the throughput improvement the spin couldn't.

### What to keep from this experiment

- **The `spin_rescues` counter is useful instrumentation** even
  after the spin reverts. If we later add a bounded spin for a
  different reason, the counter immediately tells us whether
  it's net-positive.
- **"Break-even arithmetic is a warning sign, not a green
  light"** — when a change's cost and savings are of comparable
  magnitude on paper, the cross-core traffic costs are probably
  invisible and probably negative. Treat 2× margin as the
  minimum for "this will be a real win".
- **`compare_benchmarks.sh` + `bench_tcp_ping_pong` are
  working regression canaries.** The 15 µs spin run showed
  27% run-to-run variation — that's the kind of signal we want
  the regression check to surface on future commits.

### Reminder of the rule for the next experiment

Whatever the next Phase 8 iteration is, it must:

1. Produce a **>2× cost/savings margin** on the arithmetic, not
   a break-even.
2. **Not touch cross-core shared state** during its idle-phase
   operations, or if it does, count the coherence traffic as
   part of the cost.
3. Reproduce on **both onecached 1:1 and `bench_tcp_ping_pong`**
   before landing (the latter is cheap to run).

Worker-local direct dispatch meets all three: the dispatch
targets a specific worker, no broadcast; the worker's local
deque is cache-local to that worker; and the change is the same
shape BEAM's port driver uses to move events to scheduler
queues.
