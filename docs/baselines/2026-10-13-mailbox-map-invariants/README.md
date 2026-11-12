# Mailbox tombstone and map pair contracts

Base: main `53d3778236bcfd774d973236a1dad1645fd6543d` after PR #65.
Branch `mr/mailbox-map-invariants`, local worktree
`/tmp/nera-transferable-heap-fragments`, macOS ARM64, pinned Rust 1.99.
Cargo.lock is unchanged. These checks were recorded before the requested
publication. The PR publishes this validated branch; no merge is performed.
At publication, main had advanced to `bf19444` (PRs #66 and #67). Tests and
measurements cover the branch based on `53d3778`, not a combined revision with
those changes. Complete required combined-revision validation before merging.

## Changes

Mailbox sends reject the internal tombstone sentinel in release before growing,
compacting or writing storage. Removal rejects an already tombstoned slot before
updating any counters, head or scan state. Public valid-operation interfaces and
the ring/generated receive layout remain unchanged. The bounded internal EXIT
scan already proves slot liveness and keeps its existing private primitive.

GC root scattering is now crate-private and unsafe: its caller guarantees
current valid roots, unchanged live slots and no tombstone roots. A debug-only
root check helps catch internal mistakes; there is no extra release pass. The
process GC adapter establishes these invariants from gathered roots and the
collector contract. Safe library clients can no longer bypass mailbox
bookkeeping by scattering arbitrary sentinel values.

Map allocation consumes a `MapPairs` view with private storage. `new` accepts
`&[[ErlTerm; 2]]`, establishing pairing through the array type. `from_flat` returns
None for odd-length interleaved buffers before touching the destination heap.
Both constructors borrow the original terms without allocation, copying or raw
casts. Pairing is checked once at conversion; the allocation body retains its
existing contiguous payload-copy loop. This view does not validate uniqueness
or graph ownership. All production builders (map operations, ETF decoding, ETS copying and info
maps) now produce typed entries directly, so none invokes the flat parity check.
From-list and merge reuse their typed entry buffer instead of allocating another
flat buffer and per-pair temporary Vecs. Map-allocation FFI uses typed entries;
generated-code FFI
signatures and map layouts are unchanged. Rust callers must use the new view.

```rust
use nera_runtime::{heap::{MapPairs, ProcessHeap}, term::ErlTerm};
let entries = [[ErlTerm::atom(1), ErlTerm::small_int(42)]];
let mut heap = ProcessHeap::new();
let map = heap.alloc_map(MapPairs::new(&entries));
```

## Tests

RED release tests accepted a sentinel send and a double removal. Their failing
assertions demonstrated missing guards without invoking undefined behavior.
Compile-fail examples also compiled under the old API: an odd flat map input
was accepted, and a safe library client could scatter roots. See red-*.txt.

New tests verify rejection before any mailbox state mutation, including active
scan state and an empty mailbox; odd/empty/even map views, pointer identity,
exact allocated-word/header/payload layout and typed graph retention through
full GC. Existing tombstone, growth, EXIT, map/ETF/ETS and hibernation tests run
against the migrated APIs.

Final validation: **1,874 workspace tests passed**, zero failed, one ignored.
Release runtime: **526 units, two compilation tests and 25 doctests passed**
(553 total; one existing unit ignored). Strict Clippy and full release build
passed. **24 native Erlang scenarios passed**: maps, counters/atomics/info,
persistent terms, complex ETS copies/GC/messages, selective receive/tombstones,
hibernation, self-send, monitor exit reasons and links. See validation.txt and
erlang.txt. All evidence applies to the final typed production builders.

Validation uses an isolated target and locked dependencies:

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test -p nera-runtime --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
```

Pinned final compiler/runtime native command:

```sh
/path/to/pinned/nera test 'map_*' maps_stdlib match_map counters_basic atomics_basic persistent_term_basic ets_complex_terms ets_gc_survival ets_message_passing 'selective_receive*' 'hibernate_*' self_send monitor_exit_reason 'link_*' -j 1 --timeout 30
```

Full native suite, independent review, Linux/Windows/32-bit execution, Miri,
sanitizers and remote CI are outside this local validation. CI/Release workflows
remain suspended by repository policy. Complete required full local validation
and independent review before merging.

## Performance method

Saved before/after runtime rlibs use the same
`cargo build --release --locked -p nera-runtime` command in the same worktree.
Before includes the initial RED tests/docs but no production edit. Optimized
probes share dependency artifacts and differ only in the API adapter selected
by `--cfg checked` after the change. Each series warms both sides, then alternates
nine before/after pairs; tests/builds finish before measurement.

- `send`: 50,000 batches of 128 opaque valid terms into an already allocated
  ring. Times include only insertion, excluding clearing; counters are checked.
- `remove`: 50,000 batches removing 64 odd logical slots from a 128-message
  ring. Times exclude filling and subsequent head clearing; liveness is checked.
- `map_small` / `map_large`: 1,000,000 two-pair maps / 100,000 32-pair maps,
  using flat buffers; after validates the borrowed view on each allocation.
- `map_typed`: 1,000,000 two-pair maps using typed entries; before safely
  flattens the array slice, after creates MapPairs directly without parity checks.
- Map timings include fresh heaps per 1,000 maps, allocation and destruction.
  Whole-process CPU also includes setup/clearing and checks outside timers.

Timers/black-box overhead remain on both sides. This probe does not measure
end-to-end scheduler throughput, the FFI entry-building loop or GC scattering.
No allocator instrumentation was used; the view introduces no allocations or
copies, as shown by the source and pointer/layout tests. Internal from-list/merge
buffer elimination is a source observation, not an allocator-counter measurement. The machine is
shared and not isolated: report both series, never infer zero cost from noise.

```sh
rustc --edition=2021 -O probe.rs -L dependency=/tmp/nera-transfer-target/release/deps --extern nera_runtime=/path/to/before/libnera_runtime.rlib -o before-probe
rustc --edition=2021 -O --cfg checked probe.rs -L dependency=/tmp/nera-transfer-target/release/deps --extern nera_runtime=/path/to/after/libnera_runtime.rlib -o after-probe
python3 measure.py BEFORE_PROBE AFTER_PROBE > results.json
```

## Final measurements

| Series | Metric | Before µs | After µs | Change |
| --- | --- | ---: | ---: | ---: |
| results.json | send | 13,751 | 13,893 | +1.03% |
| results.json | remove | 6,596 | 6,624 | +0.42% |
| results.json | map_small | 4,947 | 6,480 | +30.99% |
| results.json | map_large | 1,469 | 1,755 | +19.47% |
| results.json | map_typed | 4,938 | 4,913 | -0.51% |
| results.json | process_cpu_us | 77,399 | 79,511 | +2.73% |
| results-repeat.json | send | 13,773 | 13,805 | +0.23% |
| results-repeat.json | remove | 6,599 | 6,572 | -0.41% |
| results-repeat.json | map_small | 4,966 | 6,397 | +28.82% |
| results-repeat.json | map_large | 1,464 | 1,730 | +18.17% |
| results-repeat.json | map_typed | 4,914 | 4,901 | -0.26% |
| results-repeat.json | process_cpu_us | 77,311 | 79,184 | +2.42% |

Mailbox send varies **+1.03% then +0.23%**, removal **+0.42% then -0.41%**,
and typed map allocation **-0.51% then -0.26%**. These small differences on a
shared machine do not prove zero overhead; no substantial repeatable regression
was observed on these paths. The new release mailbox comparisons are real
checks, not assumed free.

Repeated flat-to-view conversion costs **+30.99% / +28.82%** for tiny maps and
**+19.47% / +18.17%** for larger maps in this probe. This is validation on every
allocation, not a slower allocator: MapPairs can be checked once and reused.
Every production builder now uses typed arrays, so it pays no flat parity check.
The checked flat constructor remains available for external Rust buffers and
is exercised by tests. Whole-probe CPU rises **+2.73% / +2.42%**, including those
flat-validation workloads and untimed preparation; it is not production runtime
CPU. No end-to-end throughput claim is made.

Recorded load on the non-isolated machine:

```text
14:36  2 users, load averages: 2.83 4.72 5.04
14:36  2 users, load averages: 2.83 4.72 5.04
```

Source and pinned artifact identities are in source-hashes.json. The generation
of typed entry buffers, removed flattening buffers and GC scattering were
functionally checked; they were not separately timed/instrumented. Only
allocation from the paired view and the selected mailbox operations were timed.
