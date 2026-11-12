# Avoid redundant process garbage collections — 2026-10-13

Base: main `fa38ee5b9f82b49b26e25ba71a240d3a327c98c4` (merge of #70). Source and artifact hashes
are recorded in `source-manifest.json`; the after build identifies a dirty
worktree based on this main revision, not an already committed revision.

## Cause and correction

A large minor-GC promotion could cross the unchanged old-generation threshold
and request a major GC on the next slice. Separately, every receive-wait with
more than 4 MiB in old generation requested a full sweep, including a graph
that had just been completely traced and remained entirely reachable. A
single receive boundary could also collect once for pressure and again to
shrink the heap.

The old-generation threshold now gives newly promoted live bytes 2x headroom.
It does not double the cumulative old-generation size after each minor GC:
small promotions can still accumulate enough old data to trigger a major GC.
The existing low-promotion shrink heuristic remains, except when the last
complete trace proves that the same graph is still rooted.

For large heaps, a complete trace remembers the sorted, deduplicated identities
of pointer roots. Immediate values, order and duplicate roots cannot change
reachability. A minor collection into an empty old generation also traces the
whole heap. An unchanged set can skip idle compaction only without pressure;
new promotions into an existing old generation invalidate the snapshot. A
changed root set conservatively retries, including when roots are dropped
without allocating anything. Stored words are comparison-only, never
traversed or retained as roots. The cache relies on immutable graph edges;
future destructive updates of object references must invalidate it. Mutable
binary contents and match offsets do not alter those edges.

The scheduler chooses at most one collection at the receive boundary before
checking max_heap_size. Young/old/binary pressure and heap limits override the
idle optimization. Explicit full collection and hibernation are unchanged.
Different pointer roots may still cause an expensive collection even when
most of the graph is shared. This is not an incremental collector or a bound
on pause duration.

## Regression evidence

- `red-promotion.log`: first large promotion requests another GC on main.
- `red-idle.log`: identical pointer roots still request idle full collection
  on main. The test-only extraction of the idle decision retains main's
  unconditional size-based policy for this RED run.
- `red-existing-old.log`: verifies the additional case of a large promotion
  into an already nonempty old generation before generalizing headroom.
- Seven added Rust tests cover both promotion cases, unchanged/reordered/
  duplicated roots and immediate counters, root drops without allocations,
  binary owner release, allocation/binary/heap-limit pressure, explicit full
  collection, fullsweep_after, new-promotion invalidation, process dictionary
  roots and actual single-scheduler receive transitions. They assert state
  and pointer movement, not a machine-dependent latency threshold.
- `gc_idle_live_roots.erl` keeps a large live list across repeated receives
  and decodes it afterward. Its expected output is also checked on BEAM +S 1.

## Single-scheduler latency

Apple M1, 16 GiB, macOS 15.7.7 (24G720), Rust 1.99.0, OTP 29 / ERTS 17.0.5.
Production release builds, uninstrumented GC and allocator; exactly one Nera
scheduler. No builds or test suites ran concurrently with these measurements.

One discarded fresh-process warmup per version/case, then five fresh-process
trials in alternating order. The worker runs for at least one second, checking
the deadline after a complete operation. Input construction and spawn copying
precede the timer; worker collections are not warmed up. The observer measures
ping/pong RTT, including any scheduler/GC pauses that overlap that exchange.
The parent allocates RTT samples itself; its GC and OS scheduling contribute
to outliers. All per-trial sample counts, elapsed times, operation counts,
percentiles and maxima are in `latency/results.json`; compressed text files
retain the complete measured samples. `benchmarks/gc_latency/run.py` reproduces
the protocol and checks executable/workload hashes.

| Workload | Median trial p99, before → after | Median trial maximum, before → after | Worst of five maxima, before → after |
|---|---:|---:|---:|
| Decode 1,048,576 list cells (16 MiB cons storage) | 112 → 115 µs | 62.233 → 57.507 ms | 73.956 → 92.834 ms |
| Decode 8,388,608 list cells (128 MiB cons storage) | 95 → 79 µs | 1,721.187 → 846.628 ms | 1,915.755 → 943.674 ms |
| Repeated waits with 1,048,576 live cells | 59,658 → 1 µs | 117.553 → 58.611 ms | 265.802 → 66.567 ms |
| Idle probe baseline | 1 → 1 µs | 13.950 → 13.928 ms | 15.267 → 24.971 ms |

For the huge decode, median maxima fall about 51%; a single cold collection
still produces a pause approaching one second. The p99 alone hides that pause
because it affects few samples. Repeated wait iterations rise from a median
16 to 131 per nominal one-second window as redundant full sweeps disappear.
The 1M-cell decode and idle baseline remain noisy: their worst maxima increase,
so these measurements do not establish an improvement for every workload.

## Validation

All Cargo commands used `RUSTC_WRAPPER=''` and the dedicated publication
`CARGO_TARGET_DIR`, with the pinned toolchain and lockfile.

| Check | Result / evidence |
|---|---|
| `cargo build --release --locked` on main | Baseline build, `build-before.log` |
| `cargo test --workspace --locked --no-fail-fast` | 1,898 passed, 1 existing ignored, `workspace-tests.log` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed, `clippy.log` |
| `scripts/dist.sh --version 0.1.0-gc-redundant-validation` | Runtime followed by compiler release build, `dist-build-final.log` |
| `nera test -j 1 --timeout 30` | 330 passed, 2 existing skips / 332, `erlang-tests.log` |
| `erl +S 1:1 ... gc_idle_live_roots:test()` | Expected completion output, `beam-idle.log` |
| `python3 scripts/test-base64.py --nera <compiler>` | 67,529 BEAM list values/errors and existing encode/binary checks, `beam.log` |
| `python3 scripts/test-base64-scheduler.py --nera <compiler>` | BEAM +S 1 and five Nera single-scheduler progress runs, `scheduler.log` |
| `python3 scripts/test-dist.py <archive>` | External and embedded runtime smoke tests, `dist-test.log` |
| `scripts/test-cross.sh <compiler> x86_64-apple-darwin` | Object generation only, `cross-macos.log` |
| `scripts/test-cross.sh <compiler> aarch64-unknown-linux-gnu` | Object generation only, `cross-linux.log` |

The two Erlang skips require external `onecached` / `onecached_storage` modules.

GitHub CI is intentionally paused by repository policy and was not enabled or
rerun. Native Linux/Windows execution and their installed archives were not
tested. Independent review and separate user approval remain required before
merge. Unrelated primary-checkout work and pre-existing Base64 benchmark
artifacts remain outside this PR.
