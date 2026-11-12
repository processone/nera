# Typed, bounded socket handles

Date: 2026-10-14. Base: merged main after PR #87, `4559336`.
Local branch/worktree: `mr/socket-handle-domain`, `/tmp/nera-transferable-heap-fragments`.
Rust 1.99.0, unchanged Cargo.lock, macOS ARM64. This report records the
validated worktree before publication; source fingerprints bind the checks to
the code included in the PR.

## Contract and implementation

`SocketHandle` is a transparent private u64 word. Its checked constructor rejects
zero generations, reserved free-list indices, negative/out-of-domain terms and
values above the positive small-integer domain. `MAX_GENERATION = 2^28-1`,
`MAX_INDEX = u32::MAX-1` (u32::MAX is the free-list sentinel). Validated identities
encode into Erlang immediate integers with a shift/OR and no repeated bounds
check. Domain validity does not establish that the socket is currently live.
The generated-code ABI remains u64 Erlang terms; Rust APIs become typed.

The slab still has two states, occupied and vacant. Vacant slots below the final
generation participate in the free list. A slot removed at the final generation
stays vacant permanently, with no resource owner and no free-list link. It is
never reused, so generation arithmetic cannot wrap or recreate an old identity.
Lookups retain the existing Vec index and generation check. Both measured builds
use 16-byte slots; no private Rust container layout is assumed by the code.

Registration now returns io::Result<SocketHandle>. Index exhaustion returns an
I/O error before pushing a new slot, instead of panicking after a narrowing cast.
listen/connect/accept and reactor acceptance propagate that error; consuming the
socket entry on failure drops its owner. Vec allocation still follows Rust's
normal allocator/OOM behavior. No repeated domain validation is added inside
registry lookup; raw Erlang values are validated at the FFI boundary.

Typed identities propagate through registry methods, reactor commands/queues,
accept-waiter indices, I/O results, worker TCP message construction and active-read
notifications. Reactor acceptance reuses try_accept, including packet-option
inheritance and registration errors. close remains idempotent. Malformed inputs
and stale handles return errors rather than becoming registry entries.

## Regression evidence

The test `exhausted_socket_generation_never_reenters_free_list` was added first
and failed on the original implementation because its replacement handle exceeds
the positive small-integer maximum. The test injects a valid final-generation
slot directly into a local test registry; it does not require millions of real
opens. See `red-generation.txt`; the corrected regression passes.

Other tests cover the final generation being usable exactly once, repeated stale
lookup/removal rejection, preservation of other free slots, release of the final
socket owner, handle/term roundtrips, invalid raw and Erlang domains, representation
size/alignment, the index bound without allocating billions of slots, and external
constructor privacy through a compile-fail doctest. Existing FIFO/cancellation
property tests keep their assertions and use explicitly validated synthetic
listener identities. Synthetic identity validity does not imply a live registry
entry.

The native `tcp_handle_domain` test covers malformed integer/noninteger inputs,
positive bounded results, passive transfer, active-once message delivery, stale
handles after close/reuse and idempotent old-handle close without closing the new
listener. The remaining TCP tests cover queued accept, active modes, disconnect,
line packets and process-exit cleanup.

## Final validation

- Workspace: **1,945 passed**, zero failed, one existing ignored.
- Runtime release: **617 passed** (576 unit, 3 compilation integrations,
  38 doctests), zero failed, one existing ignored cancellation-scaling probe.
- Focused socket unit tests: **19 passed**.
- Native `tcp_*`: **10 passed**, zero failed/skipped, on the final release build.
- Workspace/all-targets Clippy with `-D warnings`, final release build: passed.
- Runtime x86_64 Windows MSVC and Intel macOS checks: passed. Windows retains
  the existing unused-code warnings from networking fallback paths.

Socket tests and native probes used host loopback access, since restricted socket
operations return sandbox errors. No full native suite, Linux/Windows execution,
installed-archive check or independent publication review was performed in this
implementation task. These limits must remain explicit before a future merge.
CI and Release workflows remain paused according to CONTRIBUTING.md.
On the non-mio fallback, malformed accept inputs now return `{error, closed}`
(previously `{error, enoent}`); valid-domain stale handles retain the platform
error path. Windows was cross-checked, not executed.

## Performance method and results

`build_probe.py` extracts the exact private Registry/Slot/get_entry source into a
standalone optimized Rust harness. It imports the actual SocketEntry/SocketOpts,
parking_lot and mio dependencies, and selects the dependency identities matching
the runtime library by successful compilation. Saved before/after source copies
are included. There is no timing probe injected into production sources. Both
standalone probes use rustc -O and one codegen unit; they do not reproduce the
whole compiler's LTO profile and are isolated registry measurements.

Probes measure one million read-lock/index/Arc-clone lookups shared over 1/2/4/8
threads, 200,000 remove/reinsert cycles of a pre-existing owner, and 50,000 new
slot inserts. They create one real loopback listener and reuse its owner; they
do not time socket syscalls per registry operation. The native Erlang probe uses
the normal release compiler/runtime and measures 1,000 actual listen/close calls
and 2,000 verified one-byte TCP roundtrips (8,000 send/receive operations), with
one scheduler. Its timers exclude compilation, process startup and printing.

Three separate-process series warm both versions and take nine alternating pairs
per cell, yielding 27 observations per version. Lower elapsed time is better.
These results are preserved, including the large 8-thread discrepancy:

| Probe | Before median µs | After median µs | Pooled change | Range of series median changes |
|---|---:|---:|---:|---:|
| lookup_1 | 11122 | 11100 | -0.20% | -0.41% … +0.02% |
| lookup_2 | 11398 | 11455 | +0.50% | -0.87% … +1.73% |
| lookup_4 | 13342 | 13914 | +4.29% | -1.35% … +3.20% |
| lookup_8 | 34984 | 41104 | +17.49% | +16.16% … +31.50% |
| reuse | 1357 | 1254 | -7.59% | -7.81% … -7.17% |
| fresh | 246 | 243 | -1.22% | -6.20% … +3.43% |
| registration | 37827 | 38809 | +2.60% | +2.33% … +5.90% |
| roundtrip | 61414 | 60431 | -1.60% | -6.32% … +2.28% |

The separate-process 8-thread lookup discrepancy is not evidence of additional
lookup instructions: the two worker functions have **120 identical normalized
ARM instructions**, with identical function placement and registry address in
these builds. Only symbol/target addresses differ. The compared sources contain
the same two-state slot representation, size and lookup primitives.

To investigate the discrepancy, `build_control.py` compiles both exact registry
snapshots into one executable and installs the **same Arc<SocketEntry>** in both.
This controls resource allocation placement and executable differences; both
versions share the same worker function and indirect lookup-call mechanism. It
is a diagnostic control, with additional call overhead, not application throughput.
Three series each take nine alternating pairs per thread count:

| Threads | Before median µs | After median µs | Pooled change |
|---:|---:|---:|---:|
| 1 | 10709 | 10698 | -0.10% |
| 2 | 10941 | 10950 | +0.08% |
| 4 | 11460 | 11427 | -0.29% |
| 8 | 60752 | 58104 | -4.36% |

The 8-thread control varies across series (-10.59%, +4.51%, -0.17%), so it does
not reproduce a stable slowdown attributable to the type change. The cause of
the separate-process discrepancy was not conclusively identified; allocation
placement and scheduling are candidates, not demonstrated explanations. Do not
hide the original numbers or claim identical performance under all contention.

Slot reuse is about 7.6% faster in the isolated probe. Native TCP roundtrip medians
change by -1.6%, with series -6.32% to +2.28%, establishing no stable gain or
slowdown there. Native listen/close is **+2.60%** overall, with series +2.33% to
+5.90%; treat this as a possible creation/close cost rather than claiming zero
overhead. These checks preserve the direct lookup implementation and show no
single-thread lookup regression, but do not establish zero cost for all operations.
The host was active, with load averages around 3.9-4.8 during the paired runs;
recheck on an isolated host before drawing broader throughput conclusions.

## Reproduction

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test -p nera-runtime --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
/tmp/nera-transfer-target/release/nera test 'tcp_*' -j 1 --timeout 30 --no-warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo check -p nera-runtime --target x86_64-pc-windows-msvc --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo check -p nera-runtime --target x86_64-apple-darwin --locked
```

Use build_probe.py ROOT before|after /tmp/nera-socket-before|after against the
corresponding checkout/library, or compile the preserved snapshots with their
matching rlib dependencies. Compile socket_probe:main with each release runtime
from a directory without nera.toml and preserve as /tmp/nera-socket-native-before
and /tmp/nera-socket-native-after. Then measure.py OUTPUT creates a paired series.
After the two registry snapshots exist, build_control.py creates the shared-owner
control executable. Raw samples, normalized assembly, source and binary hashes
are retained. The validation metadata describes the pre-publication worktree;
publication does not change the validated source or test inputs.
