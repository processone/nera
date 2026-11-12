# Process-entry guards and EXIT memory contract

Base/integration reference: main after PR #68, `a5dafc332cf853f7539a7ffd0887a4c917aef1d7`.
Validation target: worktree snapshot on `mr/process-entry-guard`,
`/tmp/nera-transferable-heap-fragments`. At validation, HEAD was exactly the
base (0 commits ahead/behind it). Publication packages these validated sources;
main still points to the same base at publication. No merge is performed.
macOS ARM64; pinned Rust 1.99.0; Cargo.lock unchanged. Source/artifact SHA-256
identities are recorded in source-hashes.json for the final worktree.

## Changes and boundaries

The process-entry mutex is private. `ProcessEntry::lock` and `try_lock` produce
`ProcessGuard`, which retains both its owning entry and the original mutex guard
with private fields and a lifetime tied to that entry. Inbox draining and
worker-owned published transitions are guard methods, without a separately
supplied process or entry. `Deref`/`DerefMut` keep ordinary process access usable;
they do not validate raw term graphs. The transition method remains crate-private.

The single mutex, release points, Release publication store, wake CAS operations
and scheduler accounting/race checks are unchanged. FFI/reentrant nonblocking
access still uses try_lock. The small draining adapter is inline and passes the
original two references to a private core; neither it nor the transition adapter
adds allocation, validation, locking or atomic operations. The existing 0/1
message fast path and multiple-message batch buffer are preserved. Core drain
and EXIT scan bodies match the base after comment/indentation and receiver-name
normalization. Generated-code ABI and context/mailbox layouts are unchanged.

Raw `process_exit_signals` is now crate-private and unsafe. Its caller must
provide current initialized terms, complete boxed layouts and live heap/retained
owners, excluding mutation, collection and reentrancy during the scan. A returned
heap reason must be rooted before storage can move or disappear. The scheduler
calls it under the exclusive guard after inbox delivery, stores the reason and
exits without collecting before exit propagation. No dynamic term-validation
pass or extra scan is introduced.

Safe Rust callers must migrate `entry.proc.lock()` to `entry.lock()` and
`entry.drain_inbox(&mut process)` to `guard.drain_inbox()`. The raw EXIT operation
is no longer a public safe API.

## Regression evidence

The three new compile-fail examples initially compiled successfully under the
old APIs (red-api.txt). Counter-verification of the private adapter with only
its unsafe qualifier removed also accepted a safe call (red-unsafe.txt). The
final APIs reject those calls; the internal compilation fixture verifies E0133
rather than relying solely on external visibility.

New runtime tests check that one entry's guard drains/publishes only that entry,
try_lock fails without blocking while held, drop releases the original mutex,
and the other entry's inbox/state remains unchanged. The EXIT ownership test
collects a queued tuple containing a refcounted binary, scans its updated roots,
then roots the returned reason in context.result and collects again. Existing
concurrent wake ownership, I/O completion, wrapped/tombstoned EXIT scans and
active-read cleanup tests use the new guard.

Final checks:

- Workspace: **1,887 passed, zero failed, one existing ignored** (workspace.txt).
- Release runtime: **534 unit tests, three compilation tests and 29 doctests
  passed**; one existing ignored (release-runtime.txt), 566 passed total.
- Strict workspace/all-target Clippy and full release build passed
  (clippy.txt, build.txt).
- **36 targeted native Erlang scenarios passed**, zero failed/skipped
  (erlang.txt): EXIT/link cascades, monitors, self-send/registration, process-info
  snapshots, selective receive, hibernation/GC, TCP and Base64 scheduler fairness.
- Diff whitespace checks are clean. Cargo.lock and the pinned toolchain are
  unchanged. The temporary benchmark module is absent from production sources.

The first sandboxed workspace attempt failed ten socket/reactor tests because
loopback socket creation returned PermissionDenied (workspace-sandbox.txt).
The final complete suite, release runtime and native scenarios passed with local
socket access. These failures were not waived. No Linux/Windows/cross-target
coverage or full native-suite result is claimed. CI/Release remain paused under
CONTRIBUTING.md; independent review and the full merge checks remain required
before a future merge.

Commands (from the worktree, final sources):

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test -p nera-runtime --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
/tmp/nera-transfer-target/release/nera test 'link_*' 'monitor_*' spawn_link_basic spawn_monitor_basic unlink_basic demonitor_basic 'selective_receive*' 'receive_*' nested_receive 'hibernate_*' gc_receive_compact self_send register_self_send process_info process_info_snapshot_roots process_flag_basic 'tcp_*' base64_scheduler_fairness -j 1 --timeout 30
```

## Performance

Both probes build with the same locked release-test profile and target directory.
The before probe uses main's runtime code plus compile-fail documentation and
benchmark instrumentation; the after probe uses the final runtime code. The
probe module is temporarily included inside process.rs so it measures the actual
private transition and EXIT APIs. The builder restores the source afterwards.
Probe sources and binary hashes are preserved; probes are local artifacts,
not included in normal test runs.

After final validation, run one warmup per binary then nine alternating pairs,
and repeat the entire series. Results retain every sample in results.json and
results-repeat.json. Each sample measures two million lock/transition and
lock/empty-drain operations; 50,000 batches of one or 16 immediate messages,
including push, lock, drain and removal; and 100,000 scans of 128 immediate
messages plus one owned, non-EXIT boxed tuple. Whole-probe process CPU includes
startup, assertions and all probe workloads, not whole-runtime CPU.

The machine was busy and unisolated: load averages at completion were
3.00 / 4.14 / 4.53 (load.txt). These microbenchmarks do not establish a global
scheduler throughput guarantee, allocation-counter result or statistical
confidence bound. In particular, the single-message workload is short.

Median change, after relative to before (negative is faster):

| Workload | Series 1 | Series 2 |
| --- | ---: | ---: |
| Lock + published transition | -0.07% | +0.00% |
| Lock + empty inbox | -0.60% | -0.85% |
| Single-message inbox | +0.00% | +0.28% |
| 16-message inbox | +0.13% | +0.22% |
| 129-slot EXIT scan | -0.87% | -0.60% |
| Whole-probe process CPU | -0.32% | -0.31% |

No performance regression is observable beyond the variation in these samples.
The final core paths add no branch, allocation, lock or atomic operation. Initial
measurements of the larger guard-based draining body showed roughly +1% on
16-message batches (preliminary-results*.json); the final inline adapter keeps
reference passing outside the core loop and was remeasured as shown above.

Reproduce using isolated checkouts of the base and final source:

```sh
CARGO_TARGET_DIR=/tmp/nera-transfer-target python3 docs/baselines/2026-10-13-process-entry-guard/build_probe.py BEFORE_CHECKOUT before /tmp/nera-entry-before
CARGO_TARGET_DIR=/tmp/nera-transfer-target python3 docs/baselines/2026-10-13-process-entry-guard/build_probe.py AFTER_CHECKOUT after /tmp/nera-entry-after-final
python3 docs/baselines/2026-10-13-process-entry-guard/measure.py /tmp/nera-entry-before /tmp/nera-entry-after-final
```
