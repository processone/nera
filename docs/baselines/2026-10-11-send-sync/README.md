# Send/Sync ownership audit — 2026-10-11

Base: `origin/main`, `309548d32e7dfde1de98a3e06f431f6e676ee5c3` (PR #6).
Target: uncommitted worktree on `mr/send-sync-invariants`, isolated from the
concurrent IR verifier work. `validation.json` identifies the exact source
contents by SHA-256; no commits, push, PR or remote CI were requested.

## Scope

Manual Send/Sync declarations decrease from 17 to 8. Rust derives the traits for
message/storage wrappers; ProcessContext retains Send but loses Sync. Remaining
unsafe declarations document the raw owner's invariants. See `docs/SEND_SYNC.md`.

An audit also found that a zero-copy ETS result retained its CompactHeap but not
the mapping owner: deleting the table could unmap live data and leave a dangling
region pointer used by CompactHeap's destructor. Each allocation now retains an
Arc mapping owner, with the mutable cursor kept separately in the pool. The Arc
count replaces the old atomic live-entry counter, retaining bump allocation and
zero-copy reads without extra locks or per-entry data copies.

## Verification

- Test-first RED: ProcessContext's negative Sync doctest compiled unexpectedly
  on the base; the allocation-owner regression failed for the original raw
  region pointer. Both pass after the change.
- `RUSTC_WRAPPER= cargo test --workspace --locked`: 1,708 passed.
- Final runtime rerun: 451 unit tests and 6 doctests passed, including pool
  lifetime/reclamation and nested ETS lookup retention across table deletion,
  thread transfer and full GC.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo build --release --workspace --locked`: passed.
- `nera test -j 1 --timeout 30`: 286 passed, 0 failed, 2 skipped because external
  onecached modules are absent.
- Thoas v1.2.1 integration: all 11 groups passed. Compilation reports four
  non-tail-recursion warnings in the dependency, unrelated to this change.
- Git whitespace/error check: passed.

Rust checks use `RUSTC_WRAPPER=` and the shared Cargo target cache. Evidence is
from this worktree's sources, not the unrelated shared checkout's sources.
Validation ran on macOS ARM64 / Rust 1.99.0. No remote CI, other-platform execution,
Miri or sanitizer run is claimed. Raw term accesses still rely on their existing
GC/aliasing contracts; this is not a proof of all runtime memory safety.

## Performance protocol

Compare base and candidate release runtime archives using the **same compiler**
to build both executables. The compiler locates `libnera_runtime.a` beside itself;
archive hashes are in `validation.json`. Build from an isolated directory to
avoid project dependency discovery, for example:

```sh
/path/to/compiler/nera build /path/to/bench_ets_boxed.erl \
  -o /tmp/bench_ets_boxed -e bench_ets_boxed:start
```

`bench_ets_boxed.erl` is the existing ETS benchmark with nested tuple/list values
and 200,000 entries, ensuring actual CompactHeap allocation. Other workloads are
`benchmarks/bench_persistent_term.erl` with N increased from 50,000 to 200,000, and
`benchmarks/bench_binary_send.erl` with N increased from 100,000 to 2,000,000.
No production benchmark files were changed.

After tests/builds finished, warm each executable once, then run nine pairs,
alternating which version runs first. Record internal millisecond timings,
excluding startup. `benchmarks.json` contains all output, samples and medians.
The ETS and persistent-term total is the sum of reported operation phases.

| Workload | Base median | Candidate median |
| --- | ---: | ---: |
| ETS boxed values | 359 ms | 360 ms |
| persistent_term | 383 ms | 350 ms |
| binary send | 1,095 ms | 1,247 ms |

ETS is stable in this sample; persistent_term shows no observed regression.
Binary-send samples have substantial variance (base 625–2,341 ms; candidate
672–2,220 ms), so the slower median alone does not establish a regression.

A follow-up uses three interleaved arms: base, the same base executable again as
an A/A control, and candidate. Fifteen samples per arm, rotating order and
reversing it every other iteration. `binary-control.json` records all samples.
Medians are 798 ms (base), 854 ms (identical base control), and 877 ms (candidate).
The A/A difference itself is 7%; these measurements do not support a firm binary
throughput conclusion. Keep this limitation when reviewing the change rather
than claiming an across-the-board speedup or proven performance equivalence.
No new runtime operation was introduced in binary/message transfer: those
changes remove redundant trait declarations only. A more controlled performance
environment would be needed to resolve the remaining throughput uncertainty.
