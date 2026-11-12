# ETS counter batch validation — 2026-10-11

Base: `e0adafaf70f9699ae0cb54df1ea13552d1f4273d` (PR #10 merged).
Branch: `mr/ets-counter-atomicity`; local, uncommitted changes.

A counter operation list used to mutate each target before checking the next.
An invalid later position or non-integer field returned an error while keeping
previous increments. The Rust regression observed 15 instead of the original
10; the Erlang regression independently reproduced the same visible mutation
against the previous release runtime.

Validate all target positions and integer types under the existing write lock
before applying a batch. Validation uses checked indexing and introduces no
allocation or tuple copy. Both validation and application are linear in the
number of operations. The single-operation branch remains unchanged. Repeated
positions still observe preceding operations and threshold wrap-around.
The helper accepts a slice rather than requiring a Vec.

Tests exercise set and ordered_set, zero/out-of-range positions, non-integer
fields, repeated positions, and threshold wrapping. The Erlang regression also
checks the badarg exception and the stored tuple after failure.

This is deliberately limited to target-validation errors. Integer overflow and
big-integer support, key-position validation, list parsing semantics, and
insert_new atomicity are separate outstanding work; this change does not claim
complete BEAM counter compatibility.

Contract reference: https://www.erlang.org/doc/apps/stdlib/ets.html#module-concurrency

## Performance fixture

`bench_ets_counters.erl` performs one million single-counter operations and one
million three-operation batches for each table type. It checks the final value.
Build from an isolated directory using the same compiler and separate before/
after runtime archives, warm both executables, then alternate timed runs after
validation workloads have finished. Times exclude compilation and startup.


Seven alternating pairs after one warmup per executable, macOS ARM64. The
same pre-change compiler built both executables; only their runtime archives
differ. Median internal elapsed times (microseconds):

| Workload | Before | After | Change |
| --- | ---: | ---: | ---: |
| set single | 62739 | 64098 | +2.17% |
| set batch | 88521 | 88690 | +0.19% |
| ordered_set single | 49645 | 45814 | -7.72% |
| ordered_set batch | 68397 | 73020 | +6.76% |

The extra validation pass costs 6.8% on this ordered_set batch microbenchmark;
set batches differ by 0.2%. Single-operation source is unchanged (measured
+2.2% for set, -7.7% for ordered_set). These samples are not a general throughput
guarantee. No additional allocations or copies are introduced.

## Validation

- RED: focused Rust regression failed with `Some(15)` instead of `Some(10)`.
- RED: old runtime failed the Erlang regression with `{key,15,not_a_counter}`.
- GREEN: focused Rust regression passed.
- Workspace Rust tests: 1,759 passed. The first sandboxed run failed nine socket
  tests with PermissionDenied; rerunning with local socket access passed.
- Strict workspace/all-targets Clippy passed.
- Release workspace build passed.
- Erlang `ets_*`: 31 passed, zero failures, two skipped (external onecached
  modules absent), using the final compiler/runtime copied out of the build cache.
- `git diff --check` passed.

No full Erlang suite, third-party integration suites, cross-platform tests,
sanitizers/Miri, or remote CI run for this change. No commit, push or PR created.
