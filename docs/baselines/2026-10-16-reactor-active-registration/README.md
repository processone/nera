# Active TCP registration liveness — 2026-10-16

Base: merged main `663aa0b` (PR #106), including PR #105. Local platform: macOS arm64, pinned
Rust 1.99.0 and locked dependencies. This change affects the mio reactor path.

## Failure and correction

The scheduler consults `ReactorHandle::has_pending()` before declaring all
waiting processes deadlocked. Previously this counted only live reactor
registrations. A `RegisterActive` command could be queued, or removed from the
channel for processing, without appearing in that count. A controlling process
waiting for TCP data could therefore be declared deadlocked before its future
readiness source was installed.

Registration commands (`RegisterActive` and `Register`) now own a guard before
enqueue. The command envelope retains the guard through dequeue and handler
completion. After success, the existing live-registration count carries the
work; failure, command discard, disconnected receivers and receiver shutdown
release the guard. Cloned handles share the pending-registration counter.
`has_pending()` reads the handoff counter with Acquire before reading the live
count, paired with guard release after publication. Wake coalescing is unchanged.
Rearm/cleanup commands do not allocate a registration guard or increment that
counter. Diagnostic `pending` still reports live registrations only.

## Deterministic regressions

Three Rust tests in `reactor.rs` execute in isolated subprocesses to prevent
other tests' socket-registry shutdown or global diagnostic counters masking the
result. The reactor loop/thread is never started: each handoff is advanced
explicitly, without sleeps, deadlines or dependence on reactor scheduling.
One loopback connection supplies a real fd for successful poll registration;
its peer sends no data, so a readiness event cannot hide the registration gap.

- `active_registration_is_pending_until_published`: queued command, empty
  channel after dequeue, then successful live registration and release; repeated
  registration deduplicates the socket and final deregistration leaves no work.
- `active_registration_failures_release_pending_work`: disappeared socket,
  ordinary registration abandonment, cleanup without a wake source, command
  discard and 100 sends after receiver disconnect leave no ghost work.
- `active_registration_clones_keep_each_command_alive`: two clones/commands
  retain separate ownership; dropping one cannot release the other. Shutdown
  before a queued registration is processed releases it when the receiver drops.

All three tests fail at the intended queued/in-flight liveness assertions on
the original main implementation, then pass with the guard. The success test
observes the actual drain loop after dequeue and before handler publication,
with the channel and live-registration count both empty. A counter-verification
that deliberately drops the guard at that point fails specifically at the
in-flight assertion; restoring the lifetime makes it pass. See `validation.json`
for logs/hashes and final verification outcomes.

```sh
cargo test -p nera-runtime --lib --locked reactor::tests::active_registration -- --test-threads=1
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
cargo build --release --locked
./target/release/nera test 'tcp_*' '*receive*' 'timer_*' send_after_keeps_alive start_timer_keeps_alive -j 1 --timeout 30
./target/release/nera test -j 1 --timeout 30
```

## Scope

The focused native run exercises existing active-once, framing, close/kill,
concurrent accept, stress and relative performance regressions, plus receive
and timer scenarios. The new deterministic proof is the registration handoff,
not a reproduction relying on the timing of a live scheduler shutdown.
The evidence records this candidate's source and binary fingerprints. No new
absolute performance or cross-platform compatibility claim is made; the
non-mio active-reader fallback is unchanged. Final publication checks after
integration of main pass: release build, Clippy with warnings denied,
2,000 Rust tests (one existing ignored measurement), and the full native Erlang
suite (380 passed, zero failures, two existing Onecached skips).
The earlier focused run passed all 26 TCP/receive/timer scenarios; these are
also included in the final full native run. Cowlib/EUnit suites were not rerun.
CI remains manually paused; an independent review and explicit merge approval
remain required by CONTRIBUTING.md. No third-party source is introduced.
