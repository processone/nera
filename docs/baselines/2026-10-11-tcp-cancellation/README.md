# Indexed TCP accept cancellation — 2026-10-11

Optimization working tree: `mr/tcp-cancellation-index`, based on `81cb1fc`.
The correctness fix is published separately in PR #13; this optimization has
not been committed or pushed. PR #13 later merged main to resolve a changelog
conflict; that integration is not part of this working tree's baseline.

## Why the representation changes

The previous FIFO held PIDs in `VecDeque`s. Each process cancellation scanned
all queued requests, making a burst of n cancellations quadratic. The indexed
representation keeps FIFO sequence numbers in one `BTreeMap` per listener and
a reverse lookup from PID to its requests. Sequence numbers, rather than PID
order, determine service order. Duplicate requests and requests on different
listeners are supported.

Cancelling a PID now visits only its k queued requests, with O(k log q) removal
from queues of size q (expected constant-time hash lookup). Enqueue and promotion
become O(log q), rather than amortized constant-time operations. Storage remains
O(n), but has greater allocation and memory overhead from the tree and reverse
index; no memory-usage improvement is claimed. Existing scans of registered
pending operations and active sockets in `cancel_for_pid` remain unchanged;
the whole function is not O(1) in the number of listeners or active sockets.

Promotion, cancellation and listener removal clean both indexes. The reactor
keeps the same single poll registration and head-promotion behavior as PR #13.

## Evidence

The opt-in benchmark exercises the actual cancellation method with the same
sizes, PID permutation, warmup, samples and timing boundaries as the earlier
measurement. Only fixture setup and state assertions adapt to the new storage.
The earlier scan measurements are preserved in
[the initial report](../2026-10-11-tcp-accept/cancellation-scaling.md).

Initial optimized run, median time for cancelling all 32,768 queued requests:

| Listeners | Before | Indexed | Approximate speedup |
| --- | ---: | ---: | ---: |
| 1 | 703.281 ms | 5.993 ms | 117x |
| 64 | 471.109 ms | 7.059 ms | 67x |

These are local macOS ARM64 / Rust 1.99.0 measurements, not latency guarantees.
The runs were not interleaved and background machine load was not controlled.
Setup, allocations during enqueue, process scheduling and socket syscalls are
outside the timed region. The evidence shows cancellation scaling, not an
end-to-end throughput or memory comparison. Raw results are in `indexed.csv`.

## Correctness checks

- Existing real-socket regression: 2,048 waiters, FIFO, head/middle cancellation,
  ready connection after head cancellation and deregistration cleanup.
- New deterministic queue test: duplicate PID requests, multiple listeners,
  repeated cancellation, draining, removal and listener reuse.
- New property test: mixed enqueue/cancel/pop/remove sequences compared with a
  simple FIFO reference; every forward/reverse index entry is checked after
  each operation. This is refactoring coverage of existing behavior; the
  performance evidence is the before/after benchmark, not a claimed new
  behavioral RED/GREEN failure.
- No wall-clock threshold is added to regular CI.

Final validation and source/log hashes are recorded in `results.json`.

## Second measurement and full baseline

The second run, after this task's build and baseline finished, measured **32.101
ms** for the 32,768-request burst on one listener and **10.491 ms** across 64
listeners. Thus the observed median range was 6–32 ms on one listener, versus
703 ms in the prior scan implementation (roughly 22–117x). The variability is
material: its one-listener burst p95 was 122.243 ms. No attempt is made to select
only the fastest run. Raw second-run results are in `indexed-repeat.csv`.

Final workspace Clippy passed with warnings denied; 1,757 Rust tests passed,
with the opt-in measurement ignored in the normal suite. The release workspace
build passed, followed by 287 Erlang passes, zero failures and two expected
external-OneCached skips / 289 total. The stress and throughput checks passed.
Validation is local macOS ARM64; remote CI applies to the separate correctness
PR, not to this uncommitted optimization.

After the full baseline, both `tcp_reactor_stress` and `tcp_accept_concurrent`
passed 100 standalone runs each against the optimized runtime. All exit codes
and captured outputs are retained in local `target/tcp-rearm-audit/index-repeated/results.json`.

## Publication

The evidence above was collected before commit. The optimization PR targets
`mr/tcp-rearm-timeout` to keep its diff separate from correctness PR #13. CI now
also accepts PRs targeting `mr/**`, so stacked PRs run the same checks. Source
hashes were rechecked before publication; the filter-only CI change was checked
with actionlint. Remote results supersede the earlier uncommitted publication
status; they are reported in the PR conversation.
