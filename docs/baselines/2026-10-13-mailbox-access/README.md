# Checked mailbox access and EXIT scan indices

Base: main `8f027bad46ea4e034b61aa11bfc1836419dd4f52` (including PR #62 and
PR #63). Local branch `mr/mailbox-access-exit-scan`, worktree
`/tmp/nera-transferable-heap-fragments`, macOS ARM64, pinned Rust 1.99.
Cargo.lock is unchanged. These checks were recorded before the requested
publication. The PR publishes this validated branch; no merge is performed.
At publication, main had advanced to `eb08091` (PR #64). Validation and measurements
cover the branch based on `8f027ba`, not a combined revision with those newer
scheduler changes. Required combined-revision checks remain before merging.

## Contract and fix

`Mailbox::get` preserves its slot-based interface, including tombstones, but
checks `i < len` before narrowing the index or accessing raw storage in release
as well as debug. Invalid indices panic without mutation. The existing
`#[inline]` lets callers with established bounds remove the redundant check;
no unchecked safe entry point remains. The ring layout and generated receive
operations are unchanged.

The EXIT scan snapshots the original occupied physical slots and visits them in
FIFO order. Tombstoning/removal may move the logical head but cannot move or
reallocate the buffer during this function. Every scanned slot therefore remains
initialized in the original allocation; a skipped retired tombstone is still
safe to read. No stale logical index is used for reading or removal.

A private unsafe `remove_scanned_slot` centralizes tombstoning and head trimming.
Its callers establish the physical slot bounds and live-message invariant before
mutation. Ordinary `remove_at` is unchanged; the new physical-slot primitive is
restricted to the signal scan. This avoids redundant bounds checks and coordinate conversion
inside the signal loop. There is no new vector, allocation or term copy. Kill
remains untrappable; trapping processes retain normal EXIT messages. Regular
message ordering and selective receive bookkeeping are preserved.

## Regression evidence

Two tests were run against the old implementation before editing production
code. Release `Mailbox::get(16)` accepted an invalid index and the EXIT scan
returned None instead of killed after a leading normal EXIT followed by kill.
The whole ring was initialized for both RED cases so the old unchecked read
stays within initialized allocated memory; the tests demonstrate failure without
invoking undefined behavior. See red-get.txt and red-exit.txt.

Final tests cover empty, oversized and truncated indices, wrapped storage,
interior tombstones, consecutive normal signals, head trimming, later kill,
trapping mode and all-normal queues. The original regressions pass in both
release and debug.

## Validation commands

All checks use the isolated `/tmp/nera-transfer-target`, not another checkout's
workspace artifacts. Tests that open local sockets were rerun with the required
network permission after sandbox PermissionDenied failures; those initial runs
are not counted as passing. Final outcomes: **1,862 workspace tests passed**, zero failed, one ignored;
**516 release runtime units, two compilation tests and 23 doctests passed**
(541 total, one existing unit ignored). Strict workspace/all-targets Clippy and
full release build passed. **13 native Erlang scenarios passed**, zero failed or
skipped. See validation.txt and erlang.txt for preserved outcomes.

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test -p nera-runtime --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
```

The pinned final compiler/runtime pair executes these native regressions:

```sh
/path/to/pinned/nera test 'link_*' 'selective_receive*' receive_after receive_bound_var nested_receive monitor_exit_reason spawn_monitor_basic spawn_link_basic gc_receive_compact -j 1 --timeout 30
```

Full native suite, independent review, Linux/Windows/32-bit execution, Miri,
sanitizers and remote CI were not run. CI/Release workflows remain suspended
by repository policy. Required full local validation and independent review
must be completed before merging.

## Performance method

Same-path before/after release runtime builds use
`cargo build --release --locked -p nera-runtime`. Identical `rustc -O` probes
link saved rlibs and the same dependency cache. Before contains the two RED
unit tests but no production edit. Each measurement series warms both versions,
then alternates nine before/after pairs. No tests or compilations run during
measurement.

`get` times 160 million valid slot reads. Indices are opaque to the optimizer,
so this intentionally exercises the retained check when callers cannot prove
bounds. `exit` sums scan times over 500,000 queues, each containing 16 immediate
messages followed by 16 normal EXIT signals; setup and queue destruction are
excluded. An immediate head ensures the old and new scans do equivalent valid
work. The buggy head-removal case is tested for correctness, not used as a
before/after performance comparison. Timer and black-box overhead remain in
both versions. Whole-process CPU includes setup/destruction for both workloads.

This probe does not measure generated receive instructions or claim system-wide
throughput equivalence. No allocator instrumentation was used; unchanged
allocation behavior follows from the production diff.

## Final measurements

| Series | Metric | Before µs | After µs | Change |
| --- | --- | ---: | ---: | ---: |
| results.json | get | 88,671 | 112,119 | +26.44% |
| results.json | exit | 32,745 | 34,223 | +4.51% |
| results.json | process_cpu_us | 173,345 | 199,183 | +14.91% |
| results-repeat.json | get | 99,602 | 139,318 | +39.87% |
| results-repeat.json | exit | 39,104 | 38,555 | -1.40% |
| results-repeat.json | process_cpu_us | 196,375 | 219,423 | +11.74% |

The final EXIT scan varies **+4.51% then -1.40%**. The repeat does not reproduce
the first increase; these observations do not establish a stable regression or
prove zero cost. The checked public API has a real cost on the opaque-index
microbenchmark: **+26.44% then +39.87%** (about 0.15–0.25 ns per read averaged
across this optimized probe, including its loop/black-box overhead). This is
not a latency guarantee. Whole-process CPU rises **+14.91% then +11.74%** in the
probe dominated by those public reads; it is not scheduler CPU measurement.

Production Rust call sites no longer call `Mailbox::get`: the EXIT scan uses
bounded physical slots. Existing generated receive instructions are unchanged.
Thus the checked API cost does not get added to either current production path.
No additional allocation/copy was introduced. End-to-end runtime throughput was
not benchmarked; only the selected scan and API workload were measured.

The shared machine was not isolated; load varied during measurement. Small scan
differences need confirmation under controlled conditions. Recorded load:

```text
13:59  2 users, load averages: 5.52 5.47 4.94
13:59  2 users, load averages: 7.22 5.82 5.07
```

Reproduction:

```sh
rustc --edition=2021 -O probe.rs -L dependency=/tmp/nera-transfer-target/release/deps --extern nera_runtime=/path/to/pinned/libnera_runtime.rlib -o probe
python3 measure.py BEFORE_PROBE AFTER_PROBE > results.json
```
