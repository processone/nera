# Owned ETS allocations — 2026-10-12

Base: `4b612a7` (main at worktree creation).
Branch: `mr/ets-owned-allocation`; local, uncommitted validation snapshot.

## Contract

EtsRegionPool now returns EtsAllocation, a non-cloneable handle with private
mapping owner, NonNull word pointer and reserved word length. Raw-pointer
accessors are inlined; dereferencing still requires the caller to establish
initialization, range and aliasing guarantees. Mutable pointer access requires
an exclusive handle borrow. The allocation is Send for ownership transfer,
but does not implement Sync. There are no new Rust references to raw words.

CompactHeap consumes an Option<EtsAllocation> through a safe constructor,
rather than accepting independent owner, pointer and capacity arguments under
an unsafe pairing contract. Its existing cursor bounds checks remain in place.
Persistent terms use the same reservation type. None represents empty compact
heaps and keeps the immediate-only path free of region allocation.

The same region mappings and Arc increment/decrement remain: no extra mapping,
heap buffer, owner allocation or reference-count operation is introduced.
This does not make raw dereferences safe or prevent a caller from retaining a
raw pointer beyond the owner; those accesses remain unsafe.

## Validation

- RED: the compile-fail doctest for tuple destructuring compiled on the old
  API; see red-api.txt. GREEN: reservations can no longer be split via tuple
  destructuring. Their fields cannot be constructed or altered externally.
- Full Rust workspace: **1,815 passed**, zero failed, one ignored (including
  487 runtime unit tests and 12 runtime doctests).
- Workspace/all-targets Clippy with warnings denied passed.
- Release runtime build passed.
- Ten native Erlang scenarios passed: ets_delete_reader, ets_delete_table,
  ets_ordered_set, ets_update_counter, ets_counter_integer_domain,
  ets_counter_atomicity, ets_insert_hardening, ets_otp_insert,
  ets_message_passing and persistent_term_basic.
- Pool tests retain allocations after pool clear/drop, read a non-base
  reservation on another thread, and verify final owner release. Bounds,
  adjacent nonoverlapping reservations, zero-length reservations, region
  exhaustion/growth and reclamation are covered. CompactHeap rejection still
  preserves the cursor; empty heaps retain their zero-word behavior.
- Diff whitespace check passed.

Execution was on macOS ARM64. No full Erlang suite, Windows/Linux execution,
Miri, sanitizers or remote CI was run. No commit, push or PR was created.

## Performance

The identical pinned release compiler builds before/after binaries from
separate working directories, using different release runtime archives. The
fixture retains lookup snapshots and exercises set/ordered_set insertion,
overwrite, immediate/boxed update, lookup and concurrent updates. Each series
uses a warmup per executable followed by nine alternating pairs. Tasks builds
and tests had finished before measurement; every fixture run asserts its
expected lookup results and retained snapshot length.

```sh
/path/to/nera build /path/to/bench_ets_updates.erl -e bench_ets_updates:start -o /path/to/bench
python3 measure.py /path/to/before/bench /path/to/after/bench > results.json
```

Elapsed medians are internal workload times. Process CPU covers the entire
fixture, including setup and scheduler threads. Raw samples are retained.

| Series | Workload | Before µs | After µs | Change |
| --- | --- | ---: | ---: | ---: |
| results.json | set/insert | 29,213 | 29,689 | +1.63% |
| results.json | set/overwrite | 43,635 | 43,652 | +0.04% |
| results.json | set/immediate | 18,658 | 18,667 | +0.05% |
| results.json | set/boxed | 30,870 | 31,325 | +1.47% |
| results.json | set/lookup | 38,348 | 38,365 | +0.04% |
| results.json | set/concurrent | 59,082 | 59,655 | +0.97% |
| results.json | set/concurrent_boxed | 26,248 | 26,311 | +0.24% |
| results.json | ordered_set/insert | 45,600 | 45,588 | -0.03% |
| results.json | ordered_set/overwrite | 97,718 | 97,755 | +0.04% |
| results.json | ordered_set/immediate | 72,984 | 72,826 | -0.22% |
| results.json | ordered_set/boxed | 85,277 | 85,077 | -0.23% |
| results.json | ordered_set/lookup | 108,932 | 108,866 | -0.06% |
| results.json | ordered_set/concurrent | 63,249 | 62,569 | -1.08% |
| results.json | ordered_set/concurrent_boxed | 30,413 | 30,304 | -0.36% |
| results.json | process_cpu_us | 1,285,831 | 1,285,045 | -0.06% |
| results-repeat.json | set/insert | 29,024 | 29,618 | +2.05% |
| results-repeat.json | set/overwrite | 43,829 | 44,552 | +1.65% |
| results-repeat.json | set/immediate | 19,319 | 18,926 | -2.03% |
| results-repeat.json | set/boxed | 31,442 | 31,244 | -0.63% |
| results-repeat.json | set/lookup | 38,691 | 38,638 | -0.14% |
| results-repeat.json | set/concurrent | 59,420 | 59,516 | +0.16% |
| results-repeat.json | set/concurrent_boxed | 26,457 | 26,081 | -1.42% |
| results-repeat.json | ordered_set/insert | 45,520 | 45,614 | +0.21% |
| results-repeat.json | ordered_set/overwrite | 97,779 | 98,333 | +0.57% |
| results-repeat.json | ordered_set/immediate | 73,048 | 72,923 | -0.17% |
| results-repeat.json | ordered_set/boxed | 85,334 | 84,831 | -0.59% |
| results-repeat.json | ordered_set/lookup | 108,960 | 109,110 | +0.14% |
| results-repeat.json | ordered_set/concurrent | 63,340 | 62,783 | -0.88% |
| results-repeat.json | ordered_set/concurrent_boxed | 30,433 | 30,614 | +0.59% |
| results-repeat.json | process_cpu_us | 1,294,500 | 1,295,819 | +0.10% |

These samples are local microbenchmarks, not a universal zero-overhead or
application-throughput guarantee. The machine was shared rather than
controlled/idle (observed load averages 5.50/4.01/3.66); confirm small differences
on an idle machine. No allocator instrumentation was used; absence of extra
allocations/refcounts is established from the source change.
