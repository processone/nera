# Reclaim accept queue memory after peaks — 2026-10-11

Follow-up on `mr/tcp-cancellation-index`, not yet committed or pushed. Source
and log hashes, base revision and checks are in `results.json`.

## Policy

After cancellation, promotion or listener removal, an index with capacity over
256 entries shrinks only when its occupancy falls to one eighth or below. Its
requested new capacity is twice the live size; Rust may round that upward.
The same rule applies to a PID's nested request set when it still has surviving
requests. Empty nested sets are dropped as before.

This gives a wide gap between shrinking and growing again. Small tables retain
a bounded cache; large tables shrink in geometric steps instead of being rebuilt
on every removal. A shrink itself can still take linear time in the old table
capacity; this is not a constant worst-case latency guarantee. Peak memory use
is unchanged. Memory is returned to the allocator, not necessarily immediately
to the OS.

## Regression

The new regression first failed against the original implementation with
`cancel: PID index retained peak capacity` (`red.txt`). It now passes. For
cancellation, normal promotion and listener removal, it builds 32,768 requests,
reduces to 64 survivors, checks bounded capacities and surviving requests, then
drains and reuses the queues. A separate case checks a single PID with 32,768
requests so the nested reverse-index set is also covered. No timing assertions
are used. The existing randomized FIFO/index-invariant test also passes.

## Memory

The reproducible `memory.rs` driver imports the actual queue implementation,
uses a counting System allocator, and measures live requested heap bytes. It
excludes allocator metadata, rounding, caches, process/socket storage and RSS.
The old VecDeque comparator is retained in the CSV, while `new_*` denotes the
indexed implementation before/after this follow-up in the respective files.
Dropping the measured structure is checked to release all counted allocations.

```sh
rustc --edition 2021 -O docs/baselines/2026-10-11-tcp-reclaim/memory.rs -o /tmp/nera-memory
/tmp/nera-memory
```

One listener, 32,768 queued requests:

| Measure | Before reclamation | After reclamation |
| --- | ---: | ---: |
| Heap at peak | 7,349,172 bytes | 7,349,172 bytes |
| Heap retained after all cancellations | 3,735,700 bytes | 14,740 bytes |

Retained memory decreases by about **99.6%**, from 3.56 MiB to 14.4 KiB.
For 16 listeners it decreases from 3,736,624 to 15,664 bytes. At 64 queued
requests the retained cache is unchanged. The portable driver reproduced the
captured after-results exactly. Raw data: `memory-before.csv`, `memory-after.csv`.

## Cancellation performance

The existing release benchmark is unchanged. Two runs measured median whole
bursts of 32,768 cancellations:

| Listeners | First run | Second run |
| --- | ---: | ---: |
| 1 | 8.595 ms | 5.170 ms |
| 64 | 7.429 ms | 5.862 ms |

The indexed implementation before reclamation measured 6–32 ms for one listener;
the original unindexed scan measured 703 ms. These non-interleaved local runs
support preservation of the cancellation speedup, not a claim that shrinking
itself makes cancellation faster. Background load was uncontrolled. The second
run followed completion of this task's other checks. CSVs include p95 and the
other sizes/scenarios, with the same sampling method as the previous report.

## Verification

- Regression RED then GREEN; property and real-socket queue tests pass.
- Runtime suite: 460 unit tests and six doctests pass; one opt-in benchmark skipped.
- Strict runtime Clippy, all targets: pass.
- Release runtime rebuild: pass; the adjacent runtime archive is used by the
  existing compiler for the TCP fixtures.
- All eight TCP tests: pass, including stress, throughput and concurrent accepts.
- Both explicit release benchmark runs: pass.
- Whitespace check: pass.

The full workspace and full Erlang baseline were not rerun for this scoped
queue-capacity change. Remote CI does not cover these uncommitted changes.
