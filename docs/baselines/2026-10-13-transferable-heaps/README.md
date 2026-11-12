# Allocation-only fragment heaps

Base: main `79e2bd1` (merge of PR #59). Branch:
`mr/transferable-heap-fragments`, local validation on 2026-10-13, macOS ARM64.
Pinned Rust 1.99, Cargo.lock unchanged. See source-hashes.json for exact source
and artifact identities; validation.txt and erlang.txt preserve check outcomes.

## Contract

A crate-private FragmentHeap wraps a private ProcessHeap with a deliberately
restricted allocation/copy API. It has no GC method, Deref/DerefMut, mutable heap
access, borrowed-owner registration or conversion from a general ProcessHeap.
Consequently its old generation stays empty and it cannot accumulate borrowed
external owners that adoption would fail to transfer. Allocation may grow young
chunks; copying retains binary Arcs and materializes external graphs using the
existing copier. There is no new copying algorithm or extra deep-copy pass.

ProcessHeap::adopt_fragment is now crate-private and consumes FragmentHeap.
It transfers the complete young storage and binary-owner set using the existing
moves. Its debug-only old-generation assumption is replaced by construction/API
invariants; no release validation branch is added. Raw roots still require the
unsafe source-validity/graph-ownership contracts at copying and fragment creation.
This type restricts allocation generations; it does not validate arbitrary raw
term graphs. Public callers transfer storage through owned MessageFragments.

Message HeapFragments, remote process-info snapshots, fused complex tuple sends,
EXIT/DOWN signal builders and the legacy non-mio TCP reader use this type. The
wrapper is repr(transparent) and allocating/copying/adoption adapters inline to
avoid runtime abstraction overhead. No additional heap owner, allocation or Arc
clone is introduced by wrapping/adoption. Generated-code ABI is unchanged.

## Verification

- RED: the adoption compile-fail example compiled on main even after its heap
  had promoted objects. See red-adoption.txt. The new internal-only API rejects it.
- New private API compilation test uses the actual builder definition against
  minimal allocator/term types: valid allocation compiles; collection, mutable
  access to the wrapped heap, and conversion from ProcessHeap fail with specific
  compiler error codes. The builder remains private to production clients.
- Adoption tests check pointer identity, sharing and exact allocated-byte
  accounting; source old-generation terms are copied into fresh fragment storage.
  Binary owners survive source drop and target GC, adoption adds no Arc clone,
  and target drop releases ownership. Another case grows multiple young chunks.
- Full workspace: **1,851 passed, zero failed, one ignored**.
- Release runtime: **505 unit tests, two compilation tests and 23 doctests
  passed**; one existing unit test ignored.
- Strict workspace/all-targets Clippy and full release build passed.
- **16 native Erlang scenarios passed**, covering message_fragment_paths,
  process_info, process_info_snapshot_roots, monitor_exit_reason, link_basic,
  link_cascade, link_normal, spawn_monitor_basic, spawn_link_basic, tcp_active_once,
  hibernate_basic, hibernate_gc_args, hibernate_heap_args, hibernate_retained_roots,
  ets_message_passing and self_send. The new snapshot case reads nested binary/list
  graphs after the queried process exits and releases its heap.
- Whitespace validation passed; no unrelated source changes.

Each native executable was linked by a pinned compiler against its adjacent
pinned runtime archive. An isolated target `/tmp/nera-transfer-target` rebuilt
all workspace crates, reusing only unchanged third-party dependency caches.

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test -p nera-runtime --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
/path/to/pinned/nera test message_fragment_paths process_info process_info_snapshot_roots monitor_exit_reason 'link_*' spawn_monitor_basic spawn_link_basic tcp_active_once 'hibernate_*' ets_message_passing self_send -j 1 --timeout 30
```

Execution was macOS ARM64 only. The non-mio TCP fallback is migrated but was not
executed on this mio platform; its allocation helpers are covered by unit tests.
Full Erlang suite, Linux/Windows/32-bit execution, Miri, sanitizers and remote CI
were not run. CI/Release workflows remain suspended by repository policy. Full
native validation and independent review remain required before merging. Implementation validation was recorded before publication. The subsequently
requested PR publishes this validated change; no merge is performed. At
publication, main had advanced to f65b8be (PR #60). This report covers the branch
based on 79e2bd1, not a combined build with those newer changes. The changelog
entry was repositioned to avoid overlapping the new main entry.

## Performance

Identical rustc -O commands link pinned before/after release runtime rlibs from
`cargo build --release --locked -p nera-runtime` in the same worktree path. Main
was built before production edits (only the RED doc example was added); the
changed runtime was built separately. Compilations and tests finished before
measurement. Each series has one warmup and nine alternating before/after pairs.

The small workload transfers one million nested tuples with a shared 256-byte
refc binary. Nesting forces heap fragments rather than InlineTuple/SimpleTuple.
The large workload transfers 100,000 tuples additionally containing a 100-element
list, exercising young overflow chunks. Batches of 1,000 messages use fresh
receiving heaps; each batch verifies retained binary contents. Elapsed µs cover
creation, copying, adoption, target destruction and checks. Whole-process CPU
also covers both workloads and startup; this is not a per-adoption timer.

```sh
rustc --edition=2021 -O probe.rs -L dependency=/tmp/nera-transfer-target/release/deps --extern nera_runtime=/path/to/pinned/libnera_runtime.rlib -o probe
python3 measure.py BEFORE_PROBE AFTER_PROBE > results.json
./probe layout
```

Before/after MessageFragment is **56 bytes** and HeapFragment **272 bytes** on
this build. These are performance observations, not public container ABI promises.

| Series | Metric | Before µs | After µs | Change |
| --- | --- | ---: | ---: | ---: |
| results.json | small | 187,492 | 189,072 | +0.84% |
| results.json | large | 540,142 | 557,620 | +3.24% |
| results.json | process_cpu_us | 728,725 | 750,726 | +3.02% |
| results-repeat.json | small | 188,503 | 188,675 | +0.09% |
| results-repeat.json | large | 538,760 | 535,985 | -0.52% |
| results-repeat.json | process_cpu_us | 727,648 | 726,500 | -0.16% |

Whole-process CPU varied from **+3.02% to -0.16%** across the two series;
large-graph elapsed varied from +3.24% to -0.52%. The second series did not
reproduce the first increase. No repeatable regression was established, but
these samples do not prove zero overhead. The shared machine was loaded
(2.36/4.23/4.65 before measurement and 3.57/4.25/4.61 afterward); small changes
need confirmation under controlled conditions. No allocator instrumentation was
used; absence of additional adoption copies/Arc clones is established by source
inspection and the pointer/ownership regression tests. Remote process-info,
signals and TCP were functionally tested, not benchmarked here.
