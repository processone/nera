# ETS errors and the immediate integer domain — 2026-10-11

Base: `7527140f4af215dcdcb6c2d293dc0b5944406bf5` (PR #15 merged).
Validated source snapshot: worktree on `mr/ets-errors-small-integers` before publication.
The validation manifest records the final source hashes and executable hashes.

## Contracts

`select_delete` now returns `Result<i64, EtsError>`. The FFI adapter translates
errors to `badarg`. Parsing produces an explicit all/none/tuple selection and
finishes before a table write. Invalid tables, malformed clauses, improper
lists and unbound variables cannot accidentally become match-all operations.
Fixed-size clauses and singleton lists are read without temporary vectors.
Variable bindings use one vector, sorted to detect duplicates and resolve guards.
The supported subset is one wildcard/tuple pattern with distinct `$N` variables,
an optional `<`, `>`, `=<` or `>=` guard against a small integer, and a single
boolean result. Tuple arity is checked. Unsupported literals, repeated bindings,
multiple clauses or guards, and complex bodies fail rather than silently
broadening the selection. This is not a full match-spec implementation.

`ErlTerm::small_int` checks the signed 61-bit domain in every build;
`try_small_int` provides fallible encoding. ETS counter updates retain an
in-place path for small values. The existing batch result allocation doubles
as a rollback log, including repeated positions. On overflow, arbitrary
precision operations are staged before publishing a replacement owner under
the same write lock. Ordered tables remove and reinsert the paired comparison
key when replacing its backing heap. Results own their big integers and cannot
outlive a borrowed table pointer. List results remain lists even for one op;
improper lists and updates to the key position are rejected.

The producer audit also fixed wall-clock nanoseconds, atomic counter reads and
signed/unsigned binary extraction up to 64 bits. They promote wide values
rather than triggering the new constructor check. Generated unsigned 64-bit
loads use an unsigned range comparison. Small values still avoid allocation.
The cost of newly supported wide values is not compared with the old,
incorrect truncation behavior.

## Regression evidence

- Before correction, the release constructor test accepted out-of-domain values.
- The ETS counter regression observed `-1152921504606846976` instead of
  `1152921504606846976` after incrementing the largest small integer.
- The selection regression observed table mutation instead of `badarg`.
- The binary-source regression observed `-1` for the unsigned 64-bit maximum.
- Rust regressions cover selection failures without deletion, promotion with
  rollback, retained lookup snapshots, ordered boxed keys, and both binary
  extraction interfaces with signed/unsigned and big/little-endian values.
- Erlang regressions check explicit completion, all four ETS table types for
  selection, set/ordered_set for counters, promotion/demotion, thresholds,
  repeated positions, malformed lists, wide reads and timestamps.
- The two integer regression scenarios also pass on BEAM OTP 29. The selection
  test deliberately checks some valid but unsupported match specs as `badarg`,
  so full BEAM parity is not claimed for that API.

## Performance method

Use the same pinned compiler and separate runtime archives for before/after.
Build from outside the repository to avoid discovering integration fixtures.
Measure internal elapsed microseconds after a warmup, with nine pairs whose
before/after order alternates. Fixtures:

- `../2026-10-11-ets-counters/bench_ets_counters.erl`: one million scalar and
  one million three-operation updates per table type.
- `../2026-10-11-ets-ownership/bench_ets_updates.erl`: existing insertion,
  replacement, lookup and concurrent update workloads.
- `bench_ets_selection.erl`: repeated valid selections over one and 50,000
  entries, exercising parse overhead and scan throughput without deletion.

Raw samples and medians are stored in `benchmarks.json`. These local
microbenchmarks are not a general throughput guarantee.

## Separate known issue

The integer-source probe exposed an existing constant-folding defect:
`1 bsl 63` folds to the signed machine minimum, so `-(1 bsl 63)` produces a
positive value. `nera-ir/src/passes/constant_folding.rs` shifts an `i64` without
checking value overflow. The binary extraction regression uses the equivalent
`-9223372036854775807 - 1` as its oracle, independently verified on BEAM.
This compiler shift issue is outside the ETS/constructor change. Source review
also suggests the generated left-shift fast path needs a lost-bit check; that
additional case was not executed in this task.

## Final validation

- Rust workspace: **1,768 passed**, zero failed, one ignored; followed by a
  runtime rerun after the final parser optimization (**463 passed**).
- Release runtime: **463 passed**, zero failed, one ignored. This includes the
  constructor bounds regression under optimizations.
- Strict workspace/all-targets Clippy and the release builds: passed.
- Full Erlang suite: **291 passed**, zero failed, two skipped out of 293.
  After the final parser tuning, all affected Erlang tests were rerun:
  **34 passed**, zero failed, two skipped. The skips require external OneCached
  modules.
- Both integer-source/counter regressions pass on BEAM OTP 29; its compiler
  reports catch-syntax deprecation warnings in the counter test.
- `git diff --check`: passed.

The first sandboxed Rust run failed socket tests with `PermissionDenied`;
subsequent complete runs with local socket access passed. An earlier full Erlang
pass had 290 passes and two skips before the integer-source fixture was added;
the 291-pass result above includes that fixture, followed by the targeted rerun.

At validation time, local `origin/main` had advanced to
`7615c42` (seven commits ahead of this branch's base, TCP-related changes).
Those changes were not integrated here. This evidence covers this worktree,
not the newer `main`. At that validation snapshot, no commit, push, PR or remote CI run had been made. Linux,
Windows, Miri, sanitizers and prolonged stress testing were not run.

## Measured performance

These measurements were collected on a shared machine that was overloaded during
the benchmark session. They are not controlled idle-machine measurements; the
size of the overhead must be confirmed on an idle machine.

Nine alternating pairs after warmup, macOS ARM64. The first two series were
interfered with by concurrent builds and are retained as `benchmarks-initial-busy.json`
and `benchmarks-tuned-busy*.json`. The final series began after no concurrent
compiler was observed, but elapsed measurements still have substantial spread.
No claim of unchanged wall-clock performance is supported by these samples.

CPU time covers each entire fixture, including setup and scheduler threads;
it is less sensitive to preemption, but is not a latency measurement:

| Fixture | Before CPU median (µs) | After CPU median (µs) | Change |
| --- | ---: | ---: | ---: |
| counters | 377,980 | 398,618 | +5.46% |
| updates | 1,638,266 | 1,670,892 | +1.99% |
| selection | 106,621 | 122,407 | +14.81% |

The checks retain the original small-counter allocation count and in-place
updates, but do add validation instructions. The observed CPU medians suggest a cost,
but the overloaded-machine context prevents treating these percentages as
reliable regression estimates. They are not proof of zero regression. Selection overhead includes verifying clause
shape, bindings, tuple arity and result bodies that the old code ignored.

Internal elapsed medians, with raw samples retained for assessing variability:

| Fixture / workload | Before (µs) | After (µs) | Change |
| --- | ---: | ---: | ---: |
| counters / set single | 117,098 | 113,442 | -3.12% |
| counters / set batch | 155,135 | 209,424 | +34.99% |
| counters / ordered_set single | 71,375 | 102,843 | +44.09% |
| counters / ordered_set batch | 122,765 | 170,387 | +38.79% |
| updates / set insert | 71,270 | 67,493 | -5.30% |
| updates / set overwrite | 119,125 | 102,033 | -14.35% |
| updates / set immediate | 44,323 | 54,302 | +22.51% |
| updates / set boxed | 61,021 | 73,845 | +21.02% |
| updates / set lookup | 89,473 | 107,836 | +20.52% |
| updates / set concurrent | 87,132 | 86,657 | -0.55% |
| updates / set concurrent_boxed | 45,389 | 47,089 | +3.75% |
| updates / ordered_set insert | 90,305 | 80,048 | -11.36% |
| updates / ordered_set overwrite | 179,523 | 196,455 | +9.43% |
| updates / ordered_set immediate | 132,299 | 139,462 | +5.41% |
| updates / ordered_set boxed | 154,112 | 147,685 | -4.17% |
| updates / ordered_set lookup | 230,768 | 292,037 | +26.55% |
| updates / ordered_set concurrent | 103,016 | 105,417 | +2.33% |
| updates / ordered_set concurrent_boxed | 53,881 | 47,377 | -12.07% |
| selection / set 1 | 19,771 | 20,987 | +6.15% |
| selection / ordered_set 1 | 24,416 | 46,296 | +89.61% |
| selection / set 50000 | 8,188 | 10,360 | +26.53% |
| selection / ordered_set 50000 | 36,558 | 65,296 | +78.61% |
