# Borrowed term copying and direct bigint storage — 2026-10-12

Base: `4f5a266e291d5b4c3dc56cfc060663000bb59d9c`.
Branch: `mr/term-copy-bigint-storage`. Local, uncommitted validation snapshot.

## Changes

`deep_copy_term` now requires an explicit unsafe source-validity contract.
`BorrowedTerm::copy_to_heap` exposes the same copier through a heap-bound view;
its source cannot be dropped, mutably borrowed or collected while the view
is still used. There is no new runtime check, root registration or copying
algorithm on this path. Existing list, tuple, shared-substructure, float,
binary, bigint, map and reference tests use the borrowed API.

The caller audit found two invalid source-lifetime assumptions:

- Hibernate retained raw arguments after discarding their heap. Arguments now
  become owned message fragments before reset and are consumed on wake-up.
  Immediate fragments do not allocate a backing heap.
- Remote process_info snapshots released the process lock before copying raw
  message/dictionary terms. Requested term graphs are now copied under the
  source lock into an owned fragment heap, then adopted by the caller without
  a second copy. Scalar queries skip those graphs. The heap owner is boxed to
  keep scalar snapshots compact and is allocated only for non-immediate terms.

`alloc_bigint` writes each zero-padded eight-byte chunk directly into the final
heap allocation using `u64::from_le_bytes`. This removes the intermediate
Vec<u64>; the byte buffer, header/sign format and explicit endianness remain.
Tests exercise both signs across byte lengths 1 through 33 and boxed zero.

## Validation

- RED: the new compile-fail doctest compiled successfully against the old safe
  raw-copy API; see red-api.txt. GREEN: raw calls require unsafe, and a moving
  GC while a source view is still used is rejected at compile time.
- Workspace suite: 1,818 passed, one ignored, before the final process_info
  performance adjustments. All affected runtime tests were rerun afterward:
  **489 unit tests + 13 doctests passed**, one ignored.
- Final workspace/all-targets Clippy with warnings denied passed.
- Release compiler baseline and final release runtime builds passed.
- 15 native Erlang scenarios passed: bigint_arith, bigint_bitwise,
  bigint_factorial, bigint_format, ets_counter_integer_domain,
  ets_counter_atomicity, ets_update_counter, hibernate_basic,
  hibernate_heap_args, process_info, self_send, monitor_basic,
  monitor_exit_reason, link_cascade and spawn_monitor_basic. process_info was
  rebuilt and rerun again after the final snapshot representation adjustment.
- Rust tests explicitly collect/drop the snapshot's source and then drop the
  snapshot after adoption before reading retained message/dictionary values.
  Hibernate storage is tested across heap destruction. The new Erlang hibernate
  fixture also passed on the baseline: allocator reuse makes use-after-free
  nondeterministic, so it is not claimed as RED evidence.
- Whitespace checks passed. Cargo reordered unchanged lock entries during the
  build; the original lockfile was restored, with no dependency changes.

Execution was on macOS ARM64. No full Erlang suite, cross-platform execution,
Miri, sanitizers or remote CI was run. No commit, push or PR was created.

## Performance

The identical pinned release compiler builds before/after binaries from separate
working directories; only the release runtime archive differs. Each series uses
one warmup and nine alternating pairs. Counters perform 200,000 increments per
table type, starting at 2^62, and assert final stored values. The scalar fixture
performs 30,000 remote message_queue_len and 30,000 status queries while a boxed
message remains queued. Fixture assertions are checked on every execution.

```sh
/path/to/nera build /path/to/bench_promoted_counters.erl -e bench_promoted_counters:start -o /path/to/counters
/path/to/nera build /path/to/bench_copy.erl -e bench_copy:start -o /path/to/snapshots
python3 measure.py /path/to/before/binary /path/to/after/binary > results.json
```

Elapsed values below are internal loop microseconds. CPU covers the entire
process, including startup/runtime threads. `counters-final.json` and the two
final snapshot series use the final runtime; earlier counter series precede
only process_info adjustments. `snapshot-initial.json`, `copy.json`, and
`snapshot-inline-owner.json` retain intermediate tuning results, not final
performance claims.

| Series | Metric | Before µs | After µs | Change |
| --- | --- | ---: | ---: | ---: |
| counters.json | set | 78,820 | 69,244 | -12.15% |
| counters.json | ordered_set | 79,324 | 68,791 | -13.28% |
| counters.json | process_cpu_us | 163,429 | 142,681 | -12.70% |
| counters-repeat.json | set | 79,111 | 68,919 | -12.88% |
| counters-repeat.json | ordered_set | 80,201 | 68,641 | -14.41% |
| counters-repeat.json | process_cpu_us | 164,591 | 143,205 | -12.99% |
| counters-final.json | set | 79,809 | 69,231 | -13.25% |
| counters-final.json | ordered_set | 79,893 | 68,708 | -14.00% |
| counters-final.json | process_cpu_us | 165,319 | 143,640 | -13.11% |
| snapshot-final.json | scalar_snapshot | 3,818 | 3,366 | -11.84% |
| snapshot-final.json | status_snapshot | 4,215 | 4,020 | -4.63% |
| snapshot-final.json | process_cpu_us | 12,342 | 11,733 | -4.93% |
| snapshot-repeat.json | scalar_snapshot | 3,820 | 3,377 | -11.60% |
| snapshot-repeat.json | status_snapshot | 4,155 | 4,054 | -2.43% |
| snapshot-repeat.json | process_cpu_us | 12,324 | 11,627 | -5.66% |

The observed improvements are workload-specific. The machine was shared rather
than controlled/idle (observed load averages 2.96/3.60/3.31). Task compilation
and tests were not running during the final measurement pairs. Precise gains
should be confirmed on an idle machine. No general application throughput or
zero-overhead guarantee is implied; allocation removal is source evidence,
not allocator instrumentation.

Exploratory self-send of a complex tuple timed out on the baseline. A repeated
process_info(messages) probe also failed there with badarg. These unsuccessful
probes were excluded from performance comparisons; their causes were not
investigated in this change. Bulk message-copy throughput and the cost of
longer lock holds for remote boxed snapshots therefore remain unmeasured.
