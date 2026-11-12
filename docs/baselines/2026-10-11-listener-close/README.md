# Consolidated main baseline and pending accept closure

Base: `7615c42` (main after PRs #12, #13, #14 via #16, #15 and #16).
Working branch: `mr/tcp-listener-close`. macOS ARM64, Rust 1.99.0, BEAM OTP 29,
unchanged dependencies. No changes have been committed or published.

## Main baseline, before any source change

- `RUSTC_WRAPPER= cargo build --workspace --release --locked`: pass (5m 06s).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `cargo test --workspace --locked`: **1,767 passed**, zero failed, one
  intentionally ignored cancellation benchmark.
- `nera test -j 1 --timeout 30`: **287 passed, one failed, two expected external
  OneCached skips / 290 total**. This baseline is not fully green.

The failure was `tcp_reactor_perf`, with
`{batch_variance_stall,77,14,[8,10,14,14,77]}`. See `main-failure.txt`.
A single isolated rerun of the unchanged test passed. This does not erase the
original failure. System load averages were 47.16/33.05/22.52 during the suite
and 41.77/45.86/34.51 before the isolated control. Load sensitivity is plausible,
but this measurement does not establish the cause of that variance. Test
thresholds and timeouts were not changed. Original logs are preserved in local
`target/listener-close-audit/main-*.log`.

## Confirmed listener-close failure

The same external fixture passed on BEAM and failed on freshly built main:
`accept_close_failed: {accept_close_timeout,<0.2.0>}` after five seconds.
Captured results are in `regression-red.json` and `beam.txt`. The first scenario
has only one acceptor, so this is not restricted to very large queues.

`socket::remove` deletes the registry entry and sends `Deregister` to the
reactor. That handler removed pending accept operations and the additional
waiter queue without calling `wake_process_with_result`. Their processes
therefore remained in `IO_WAIT` indefinitely. `DeregisterActive` is a separate
command used for mode changes; the full `Deregister` command is sent by removal.

## Correction

Closing the listener now wakes the registered acceptor and every queued
acceptor. An explicit `IoResult::Closed` converts to `{error, closed}` without
changing generic POSIX error mapping for files or other operations.

Closure can win before the worker attempts accept, between WouldBlock and
reactor registration, during poll registration, or before readiness dispatch.
The accept-specific paths handle the missing listener consistently as closed.
Other pending operation types retain their existing deregistration behavior.

The permanent `tcp_accept_close` regression covers one suspended acceptor,
16 suspended acceptors, and 20 rounds of four acceptors racing with close.
It checks the exact result from each process and prints a required success
marker before halting; a premature successful VM exit cannot pass the fixture.
The same fixture passed after rebuilding the corrected runtime.

## Final verification

See `results.json` for final checks, source hashes and log hashes. All Cargo
commands bypass the local compiler-cache wrapper with `RUSTC_WRAPPER=`.
The main baseline above remains a separate pre-fix observation. Verification
of this fix focuses on the workspace Rust checks, release build and TCP suite;
the entire Erlang suite is not rerun for this narrow change. Native reactor
behavior is tested on macOS; Linux and cross-target CI were not run locally.

Final checks passed: strict workspace Clippy, 1,767 Rust tests (one opt-in
benchmark ignored), release workspace build and all **nine TCP tests**. The final
release build took 10 minutes under very high machine load (111.45/89.39/59.15
observed near its end). The passing post-fix performance test is not evidence
that the listener fix explains the original batch-variance failure: the
unchanged isolated control already passed before the fix.

The final accept-close regression also passed **100/100 consecutive executions**
with the final release compiler and runtime. Per-run output and timings are
retained locally in `target/listener-close-audit/repeat/results.json`.
