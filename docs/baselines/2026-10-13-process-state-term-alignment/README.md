# Process state encapsulation and term-pointer alignment

Isolated development validation and performance comparison: main after PR #69,
`5b24148ce3281db26795adbc07956f86186abdb6`, on
`mr/process-state-term-alignment` at `/tmp/nera-transferable-heap-fragments`.
Publication rebases this branch onto main after PRs #70 and #71,
`45b5443a32236eb8551f565dcacffbb1da8f873b`.
macOS ARM64, pinned Rust 1.99.0, unchanged Cargo.lock.

## Publication integration validation

Resolve the changelog conflict by retaining both main's milestones and this
change's entry. Migrate the newly added Base64 list observer to the runtime
mutable guard and finish(), and the new GC scheduler callback to finish().
The original implementation otherwise retains its API changes after the rebase.

The final combined revision passes **1,904 workspace tests**, **548 release unit
+ 3 compilation + 32 doctests (583 total)**, strict workspace/all-target Clippy
and a full release build. One existing Rust test is ignored. Native validation
passes **87 scenarios**, zero failed, two external onecached scenarios skipped
out of 89 selected; it repeats the original coverage and adds all Base64 cases.
See integrated-*.txt. Dependencies/toolchain remain unchanged.

The final snapshot is in integrated-source-hashes.json. Original source-hashes.json
and the earlier logs below preserve pre-rebase evidence. The four performance
series isolate these changes on the original #69 base; they do **not** measure
the combination with the later Base64/GC work. Do not attribute main's GC-policy
performance changes to this PR or claim a fresh combined throughput measurement.
No merge is performed. CI/Release remain paused, and independent review/full
merge checks are still required before merging.

## Changes

Process lifecycle status and OTP wait cause are private. `status()` and
`wait_reason()` permit observation. Public ProcessGuard retains shared Deref and
safe owned-fragment draining, but no DerefMut: an external caller cannot replace
a locked process, overwrite its OTP state or mutate its scheduling fields.
Full mutable access uses the crate-private transparent RuntimeProcessGuard via
lock_for_runtime/try_lock_for_runtime in scheduler, I/O and FFI scopes. It wraps
the original guard, retains its owning entry and acquires no additional lock.
Rust callbacks use finish() to request normal completion; the worker publishes
that exit after the callback returns, as before. Unused public independent wait
setters are removed; internal complete transitions update status and cause.

The published Release store, wake CAS operations, queue accounting, lock/drop
scopes and reentrant nonblocking access remain unchanged. I/O completion still
updates locked fields without publishing before its wake CAS. A claimed wake
can therefore publish Ready while a public guard observes the still-Waiting
locked state until worker reconciliation; the new state test preserves this
intentional distinction. Generated ProcessContext.status remains the same raw
u32 execution-result ABI, with unchanged context/mailbox layouts.

Public ErlTerm::cons/boxed validate all three alignment bits in release and
debug, panicking on misalignment; try_cons/try_boxed return None instead. They
encode addresses only: aligned null remains encodable, and neither alignment
nor a successful conversion proves liveness, layout, initialization or ownership.
Heap reads retain their separate unsafe contracts and borrowed-owner views.

Allocator, forwarding, copying, ETS and FFI encoders use crate-private unsafe
cons_unchecked/boxed_unchecked with alignment established by the owned word
allocation, recorded forwarding destination or valid runtime graph. The internal
bodies retain the original debug assertions and release bit encoding. No new
release check is placed on these paths. New unsafe blocks at allocation/copy
sites document the alignment proof; existing unsafe graph readers continue to
require valid, aligned runtime objects.

Rust API migration: use status()/wait_reason() for reads and finish() in Rust
callbacks; public locked processes are observational views rather than mutable
whole-process handles. Raw aligned pointer constructor signatures and resulting
tagged values remain compatible. Misaligned inputs now reject in release.

## Tests and readiness

RED compilation examples accepted direct lifecycle/wait writes and replacement
of a locked process (red-state.txt). The release regression accepted an address
at byte offset 1 (red-alignment.txt). No invalid address was dereferenced during
RED. Final tests reject all seven misalignments for both constructors, check
fallible conversions and exact aligned address/tag preservation, compare checked
and trusted encodings, and preserve null's encoding-only behavior.

The new public-guard test verifies observation, exclusion of a second lock,
I/O wake CAS ownership and reconciliation into Running. Existing transition,
callback completion, I/O result, concurrent wake, EXIT, GC, hibernation, copying
and tagged-term tests exercise the migrated interfaces.

Initial isolated validation on the #69 base, before publication rebase:

- **1,893 workspace tests passed**, zero failed, one existing ignored.
- Release runtime: **537 unit tests + 3 compilation tests + 32 doctests passed**
  (572 total), one existing ignored.
- Strict workspace/all-target Clippy and full release build passed.
- Native: **80 passed, zero failed, two skipped out of 82 selected**. Skips are
  ets_onecached_full_startup and ets_onecached_pattern, which require unavailable
  external onecached modules. No success is claimed for those integrations.
- Native coverage includes links/EXIT/monitors, receive, process-info, hibernation,
  GC, TCP, scheduler fairness, bigints, refcounted binaries, ETS and maps.
- Diff checks are clean and temporary benchmark modules are absent from sources.

Logs: workspace.txt, release-runtime.txt, clippy.txt, build.txt and erlang.txt.
Locked dependencies/toolchain are unchanged. No Linux/Windows/cross-target check
or full native-suite result is claimed. CI/Release remain manually paused under
CONTRIBUTING.md. Independent review and full merge checks are still required
before any later merge.

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test -p nera-runtime --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
/tmp/nera-transfer-target/release/nera test 'link_*' 'monitor_*' spawn_link_basic spawn_monitor_basic unlink_basic demonitor_basic 'selective_receive*' 'receive_*' nested_receive 'hibernate_*' 'gc_*' self_send register_self_send process_info process_info_snapshot_roots process_flag_basic 'tcp_*' base64_scheduler_fairness 'bigint_*' 'binary_refc*' 'ets_*' 'map_*' maps_stdlib -j 1 --timeout 30
```

## Performance on the busy machine

The before probe uses main's runtime code plus RED tests/docs and instrumentation;
the after probe uses the validated pre-rebase runtime code. Both use the same locked release-test
profile, toolchain and target directory. The builder temporarily includes the
probe as a child of process.rs to access actual private runtime operations, then
restores the exact original source. Probe sources, artifacts and raw samples are
identified in source-hashes.json. Production retains no benchmark module.

Each series uses one warmup per binary followed by **nine alternating pairs**.
Two initial series were repeated twice more because the second showed noisy
outliers. All four series are retained, rather than selecting only favorable
samples. Initial load averages were **5.79 / 6.96 / 5.70** (load.txt); the host was
busy and unisolated. load-repeat.txt records load after the additional series.
No other validation/build task from this chat ran during measurements.

Workloads: two million lock+transition and lock+empty-drain operations; 50,000
batches of one or 16 immediate messages (including push, lock, drain and removal);
100,000 scans of 128 immediate messages plus one owned non-EXIT boxed tuple;
five million raw boxed encodings per public/trusted path; and one million cons
plus one million tuple allocations in 1,000 fresh heaps. Whole-probe process CPU
includes startup, assertions and every workload, not global runtime CPU.

Median change after relative to before (negative is faster):

| Workload | Series 1 | Series 2 | Series 3 | Series 4 |
| --- | ---: | ---: | ---: | ---: |
| Lock + published transition | -0.14% | +4.35% | +0.45% | +0.26% |
| Lock + empty inbox | +0.54% | +0.93% | -0.52% | -0.40% |
| Single-message inbox | +0.69% | +3.34% | +0.34% | +0.27% |
| 16-message inbox | -0.02% | -0.78% | -0.36% | -0.08% |
| 129-slot EXIT scan | -0.33% | -3.29% | +0.20% | +0.08% |
| Public checked boxed encoding | +2.86% | -0.17% | +2.49% | +2.32% |
| Trusted boxed encoding | +0.97% | -0.89% | +0.00% | -0.06% |
| Cons + tuple heap allocation | +0.39% | -3.67% | -0.22% | -0.10% |
| Whole-probe process CPU | +0.17% | +1.01% | -0.22% | +0.11% |

Series 2's lock samples ranged from 18,276 to 31,397 us before and 18,184 to
26,338 us after. Its +4.35% median lock delta did not persist in the additional
series. The ARM64 lock/transition loop is identical after normalizing code
addresses and external call relocations (50 instructions, including slow paths;
assembly-lock-loop.txt). This checks that specific loop, not every runtime path.

The additional series place internal workload medians within approximately
-0.6% to +0.5%, with no consistent regression observed. The public checked boxed
microbenchmark generally shows about +2.3% to +2.9% (one noisy series -0.17%):
that cost belongs to the new public alignment check. Actual heap allocations and
trusted encodings do not incur it. Very short encoding/single-message workloads,
code placement and host load limit confidence; these measurements do not provide
a global throughput guarantee or statistical confidence bound.

Reproduce on isolated before/after checkouts:

```sh
CARGO_TARGET_DIR=/tmp/nera-transfer-target python3 docs/baselines/2026-10-13-process-state-term-alignment/build_probe.py BEFORE_CHECKOUT before /tmp/nera-state-align-before
CARGO_TARGET_DIR=/tmp/nera-transfer-target python3 docs/baselines/2026-10-13-process-state-term-alignment/build_probe.py AFTER_CHECKOUT after /tmp/nera-state-align-after
python3 docs/baselines/2026-10-13-process-state-term-alignment/measure.py /tmp/nera-state-align-before /tmp/nera-state-align-after
```
