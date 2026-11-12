# Concurrent TCP accept repair — 2026-10-11

Target: the uncommitted working tree of `mr/tcp-rearm-timeout`, based on
`origin/main` at `fa0d77e3f150d47f010d1f905c772ccfb54289d5`. The keepalive
fixture changes are in a separate worktree and are not included here.

## Reproduction and cause

A fresh release build of main reproduced the original `tcp_reactor_stress`
timeout on the first focused run. Its captured stderr reported
`timeout_mp_rearm` at rounds 20 and 19. [Original output](main-repro.txt).

The shared listener had multiple processes suspended in `gen_tcp:accept/1`.
`register_pending` replaced the first pending accept with the next one for the
same handle. The displaced process remained in `IO_WAIT` without an operation
that could wake it. It could therefore never send the data another stress-test
worker expected. The timeout surfaced during active-once receive, but its cause
was the earlier lost accept.

A temporary diagnostic build captured this replacement:

```text
TRACE register accept token=Token(22) pid=3 listener=55834574848 old=None
TRACE register accept token=Token(23) pid=4 listener=55834574848 old=Some(Token(22))
DIAG pid=3 status=Waiting atomic=4 inbox=0 mailbox=0 wait=Some(IoWait)
```

The [captured failure](diagnostic-failure.json) retains stdout and stderr.
The diagnostic source modifications have been removed. This failure appeared
on iteration 35 of that instrumented probe; synchronous tracing changed the
schedule, so its reproduction rate is not comparable to the uninstrumented run.
Multiple acceptors sharing a listener are also used in the official
[Erlang TCP examples](https://www.erlang.org/doc/apps/kernel/gen_tcp.html).

## Change and regression coverage

The reactor keeps one poll registration per listener and queues additional
acceptor PIDs in a `VecDeque`. It promotes the next waiter after each completion,
drains available connections, and leaves the next waiter registered on
`WouldBlock`. Cancelling a process removes its queued requests; cancelling the
head immediately tries the next waiter so an already-ready connection does not
need another readiness edge.

Enqueue and dequeue at the head are amortized O(1), with O(n) PID storage.
Cancellation scans the queues and is O(total queued waiters). The Rust regression
covers 2,048 waiters, FIFO service, cancellation in the middle and at the head,
an already-ready connection during head cancellation, and queue cleanup when
the listener is deregistered. It runs in a child process to isolate the socket
registry from other scheduler tests' global cleanup.

The deterministic Rust test failed before the fix with “a second accept must not
discard the first suspended process” ([RED](rust-red.txt)), then passed
([GREEN](large-queue-green.txt)). The new Erlang fixture also failed against the
old runtime with `acceptor_lost` ([RED](erlang-red.txt)). It coordinates three
waiting acceptors before connecting clients and repeats that scenario 20 times.
It passes under Nera and under BEAM OTP 29 ([BEAM output](beam-regression.txt)).
The original stress fixture is unchanged.

## Validation

See [results](results.json) and the [source manifest](source-manifest.json) for
counts, log hashes, and the exact tested source. Commands use `RUSTC_WRAPPER=`
for Cargo to bypass the local compiler cache wrapper.

- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo test --workspace --locked`: 1,755 passed, zero failed.
- `cargo build --workspace --release --locked`: passed.
- `nera test 'tcp*' -j 1 --timeout 30`: eight passed.
- 100 standalone executions each of `tcp_reactor_stress` and
  `tcp_accept_concurrent`: all 200 passed. The binaries were linked to the fixed
  release runtime; every exit status and captured stdout/stderr is retained.
- `nera test -j 1 --timeout 30`: 287 passed, zero failed, two expected
  OneCached external-module skips / 289 total.
- `git diff --check`: passed.

## Boundaries

Validation is local macOS ARM64 with Rust 1.99.0 and unchanged dependencies.
Linux, cross-target builds, remote CI, and optional external integration
projects were not run for this change. Repetition provides additional evidence,
not a guarantee against every possible scheduling race.

Deregistering a listener clears the new queue, preserving the existing close
policy. This change does not implement delivery of `{error, closed}` to live
acceptors when another process closes their listener; the cleanup assertion
checks reactor bookkeeping only. The one-pending-operation policy for other
operation types is unchanged. No keepalive, scheduler, or test-runner behavior
is changed. Validation was recorded before commit; remote publication was not requested.

## Follow-up measurement

The added [cancellation benchmark](cancellation-scaling.md) measures the cost of
large queues and mass cancellation, with raw CSV results and scoped validation.
