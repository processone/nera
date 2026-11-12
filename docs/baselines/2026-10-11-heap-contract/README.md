# Heap allocation and ownership contracts — 2026-10-11

Base: `fc3e63f` (PR #11 merged). Branch: `mr/heap-ownership-contract`.

## Allocator contract

`CompactHeap::alloc_words` remains safe, with an unconditional capacity check.
It compares the requested word count to remaining capacity before changing the
cursor. This avoids overflow in `offset + n` and leaves the cursor unchanged
on failure. Zero-word allocations work for both empty heaps and full heaps.
The pointer is returned only after validation. The cold failure path panics;
valid calls retain bump-allocation behavior with no allocation or locking added.

The regression tests run in release mode. Before the fix, both the empty-heap
and over-capacity tests failed because invalid allocations were accepted.
After the fix they also cover `usize::MAX`, cursor preservation, exact capacity,
and zero-word allocations at the end of a reservation.

## Owner encapsulation

ProcessHeap's binary-owner list, borrowed-heap owner list, and binary virtual
heap accounting are private. Binary allocation and both message-copy paths use
one inline registration method that retains the Arc and charges the same
virtual heap overhead as before. ETS zero-copy lookup uses an additive owner
retention method. No mutable collection accessor is exposed. Only test builds
expose read-only owner counts.

Existing GC and fragment-adoption logic is unchanged; no new Arc clone/drop,
heap allocation, lock, data-layout change, or dynamic dispatch is introduced by
this encapsulation. The binary-window test retains its own Arc instead of
reaching into ProcessHeap storage. Two compile-fail doctests forbid removing
owners through the public fields. Both failed before encapsulation because the
code compiled, then passed once the fields became private.

## Benchmark method

Use the existing fixtures:

- `docs/baselines/2026-10-11-ets-ownership/bench_ets_updates.erl`
- `benchmarks/bench_binary_send.erl`

Compile each before/after pair with the same compiler executable and its
respective runtime archive, from an isolated directory to avoid project
source discovery. Warm up each executable once and run seven alternating pairs
without validation/build workloads running concurrently. ETS measurements are
internal microseconds; binary send measures internal milliseconds for 100,000
messages containing a 10 KiB binary. The ETS fixture checks multiple insertion,
replacement, update and lookup phases, including concurrent updates.

Local macOS ARM64 measurements are not a cross-platform performance guarantee.


The initial seven pairs had noisy regressions in set immediate updates and
ordered_set lookup. A second 21-pair run was performed to investigate; both
original and repeated samples are retained (`benchmarks.json` and
`benchmarks-repeat.json`). The following medians pool **all 28 pairs**, rather
than selecting only the more favorable run. `benchmark-summary.json` stores
the combined medians. All ETS values below are microseconds; binary send is
milliseconds.

| Workload | Before | After | Change |
| --- | ---: | ---: | ---: |
| set insert | 33545.0 | 33290.0 | -0.76% |
| set overwrite | 48674.0 | 47886.0 | -1.62% |
| set immediate | 22426.5 | 21992.0 | -1.94% |
| set boxed | 35657.0 | 34955.0 | -1.97% |
| set lookup | 43299.5 | 42321.0 | -2.26% |
| set concurrent | 67914.5 | 67752.0 | -0.24% |
| set concurrent_boxed | 27207.0 | 27270.0 | +0.23% |
| ordered_set insert | 47354.0 | 47650.0 | +0.63% |
| ordered_set overwrite | 103189.0 | 103520.5 | +0.32% |
| ordered_set immediate | 76509.5 | 75777.5 | -0.96% |
| ordered_set boxed | 88345.0 | 88132.0 | -0.24% |
| ordered_set lookup | 115216.5 | 115818.5 | +0.52% |
| ordered_set concurrent | 66284.0 | 67796.0 | +2.28% |
| ordered_set concurrent_boxed | 30799.0 | 30803.5 | +0.01% |
| binary_send_ms | 29.0 | 26.0 | -10.34% |

## Validation

- Release allocator regressions: RED before correction, GREEN afterward;
  release filter `compact_heap_` passed 10 tests in total.
- Compile-fail visibility regressions: RED before encapsulation, GREEN afterward;
  runtime doctests passed all 8 tests.
- `cargo test --workspace --locked`: 1,763 passed, including existing ownership,
  GC reclamation, message-copy and zero-copy ETS lifetime regressions.
- Strict workspace/all-targets Clippy: passed.
- Release workspace build: passed.
- Full Erlang suite: 287 passed, zero failures, two skipped for unavailable
  external onecached modules. Used the baseline compiler with the corrected
  runtime archive; the archive matches the final release workspace build.
- `git diff --check`: passed.

Commands used `RUSTC_WRAPPER=` and the shared Cargo target cache. Tests requiring
local sockets ran with that access enabled. Benchmarks ran after validation and
build workloads finished. `validation.json` records source and evidence hashes.
No sanitizer/Miri, cross-platform test or remote CI run. Changes remain local;
no commit, push or PR was created during implementation.
