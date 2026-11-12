# TCP timeout diagnosis — 2026-10-11

Base: `origin/main`, commit `fa138d8f320c4660b48a4dd17f94ea974841851d`
(PR #1 merged). Changes validated as an uncommitted worktree on
`mr/tcp-timeout-diagnostics`, with Rust 1.99.0 and dependencies unchanged.

## Reproduction and causes

The original TCP performance binary hung on iteration 31 (zero-based) of an
alternating reactor/performance series. It printed a `batch_variance_stall`
exception but did not exit within eight seconds. A macOS process sample showed
parked scheduler workers, the reactor polling, and the main thread waiting for
workers. This was a runtime hang, not a dynamic-loader startup delay.

A reactor readiness notification can wake a process without producing an Erlang
message: for example, after a socket closes, for a partial line, or on WouldBlock.
The scheduler put that process back into waiting state without releasing its
runnable-count slot. The positive counter then prevented termination detection.
The fix releases the slot before the existing inbox race check can reacquire it.
A deterministic regression queues a stale readiness notification: before the fix
it retained a count of one and failed to terminate; after it, the count is zero
and the scheduler terminates.

After this fix alone, five performance assertion failures exited promptly, but
one reactor run still timed out without output (iteration 110). Investigation
also demonstrated a timer-publication race: expiration could remove a due heap
entry before its PID mapping was installed, losing the timer permanently. The
concurrent regression recovered only 19,938 of 20,000 timers before the fix.
Insertion now holds the heap lock until the mapping is published, using the same
lock order as expiration. All 20,000 timers are recovered after the fix.

The timer bug is independently proven; its causal connection to the second TCP
hang is not established. In particular, 600 uninstrumented reactor executions
with scheduler diagnostics enabled passed even before the timer fix. Successful
stress runs are supporting evidence, not proof that every intermittent TCP hang
has been eliminated. No performance thresholds were weakened and failures were
not replaced by retry results.

## Diagnostic behavior

`nera test` retains and labels both stdout and stderr when a test times out or
exits unsuccessfully. A subprocess regression exercises compilation, execution,
termination and reporting; both streams were absent before the fix and are now
preserved. `NERA_SCHED_STATS=1` additionally reports runnable/alive counts,
pending timers, and process wait states using nonblocking process locks.

## Validation

- Focused runner tests: 11 passed, including the observed red-to-green timeout test.
- Focused scheduler tests: 10 passed, including the observed red-to-green stale-readiness test.
- Focused timer tests: 10 passed, including the observed red-to-green concurrent-publication test.
- Final Rust workspace: 1,689 passed, zero failures.
- Final reactor stress with scheduler diagnostics: 400/400 passed, four concurrent executions, eight-second timeout.

- Final Erlang baseline: 286 passed, zero failures, two skipped (288 total).
- Final alternating reactor/performance stress without scheduler diagnostics:
  200/200 passed (100 executions of each test), zero timeouts and zero assertion failures.
- Git whitespace/error check: passed.

Detailed results are recorded in `results.json`.
The source hashes in `source-manifest.json` identify the tested implementation.
Raw local stdout/stderr, process samples, and repeat JSON are retained under
`target/tcp-diagnostic-2026-10-11/`; build and unit-test logs are archived there as
well. Those ignored logs are local evidence, not distributed repository artifacts.

Validation is local macOS ARM64. No remote CI or other-platform execution is
claimed for this uncommitted change.
