# Direct bigint byte buffer — 2026-10-11

Base: `9b64f629dbc854014ad351a451145cd55df6a50c`.
Branch: `mr/bigint-byte-buffer`; local validation snapshot, not published.

`term_to_bigint` now fills its byte buffer directly from the heap's u64 limbs.
This removes the temporary limb vector and its traversal. Explicit
`to_le_bytes` conversion, sign handling, zero trimming and the small-integer
path are unchanged. No new unsafe operation or representation assumption is
introduced. Existing focused conversion tests cover positive/negative values,
zero, small integers, rejection of non-integers and multi-limb factorials.

## Validation

- Runtime Rust tests: 474 passed, one ignored; 8 doctests passed.
- Runtime/all-targets Clippy with warnings denied passed.
- Release runtime build passed.
- Seven native Erlang scenarios passed: bigint_arith, bigint_bitwise,
  bigint_factorial, bigint_format, ets_counter_integer_domain,
  ets_counter_atomicity and ets_update_counter.
- Diff whitespace check passed.

Execution was on macOS ARM64. No full workspace or full Erlang suite,
cross-platform execution, Miri, sanitizers or remote CI was run. This is a
mechanical optimization covered by existing tests, not a behavior fix with
a newly failing regression.

## Promoted ETS counter benchmark

The fixture exercises the bigint counter path introduced by PR #19, starting
at 2^62 and performing 200,000 increments per table type (set and ordered_set).
It asserts the final stored value. The counter remains boxed throughout.
This measures complete counter updates, not just bigint conversion, and does
not measure small counters or very large multi-limb counters.

The identical pinned release compiler from the multiplication correction
compiled both binaries; only the release runtime archive differs. Compiler
execution is excluded from timing. Use separate working directories with each
runtime archive next to its compiler:

```sh
/path/to/nera build /path/to/bench_promoted_counters.erl -e bench_promoted_counters:start -o /path/to/bench
python3 measure.py /path/to/before/bench /path/to/after/bench > results.json
```

Two series each use a warmup and nine alternating before/after pairs. Elapsed
microseconds are measured inside each loop; process CPU includes startup and
runtime threads. Raw samples are retained in results*.json.

| Series | Workload | Before median µs | After median µs | Change |
| --- | --- | ---: | ---: | ---: |
| First | set elapsed | 81,464 | 78,332 | -3.84% |
| First | ordered_set elapsed | 81,959 | 78,616 | -4.08% |
| First | process CPU | 168,368 | 161,992 | -3.79% |
| Repeat | set elapsed | 81,297 | 78,214 | -3.79% |
| Repeat | ordered_set elapsed | 81,917 | 78,218 | -4.52% |
| Repeat | process CPU | 168,107 | 161,296 | -4.05% |

These samples indicate an improvement on this workload, not a universal
throughput guarantee. The shared machine was not controlled or idle; observed
1/5/15-minute load averages were 5.59/12.13/21.79. The second series ran after
all task builds and tests had completed. Confirm precise gains on an idle
machine. Allocation removal is established by the source diff, not an
allocator-instrumented measurement.
