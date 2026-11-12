# Process-ID block reservations

Production baseline: PR #85 commit `5fb648d55ccc2ac3818062537828f0805978e01f`,
based on main `a40da33eac57c0e8d5be343c3874848cb629331a`.
macOS ARM64, Rust 1.99.0, locked dependencies, release profile unchanged.

## Contract

For schedulers with at least two workers, each OS thread caches one range of
up to 64 identifiers. Single-worker schedulers retain direct bounded atomic
increments, avoiding TLS overhead. Their external callers remain concurrency-safe,
although this mode does not amortize contention between external threads. The
policy is immutable for the lifetime of each allocator. Scheduler and
SchedulerHandle share the same Arc allocator and therefore the same identity.
External caller threads use the same path as scheduler workers. The hot path
advances only the private thread-local cursor; global reservation retains the
bounded relaxed CAS update. Final reservation is truncated at ProcessId::MAX,
and the global counter remains permanently saturated at MAX+1.

A Weak in the cache retains the allocator's allocation identity, without
retaining the owner. Comparing its address with a live Arc is safe even if a
previous allocator has been destroyed: the Weak prevents allocation-address
reuse until replaced/dropped. No pointer is dereferenced through that address.
No callback, FFI or GC call occurs while the local RefCell is borrowed.
Allocation does not add locks or allocate an extra range container on the hot
path. Weak reference counts change only when the cached allocator changes. Refilling
a range for the same allocator keeps the existing Weak, avoiding unnecessary
shared atomic updates.

The cache is bounded to one entry per OS thread. Replacing a cached block allocator or
exiting a thread abandons unused IDs; ranges are never reclaimed or reassigned. IDs may
have gaps and are not globally ordered by time of issuance. After global ranges
are exhausted, threads with existing reserves can still issue their remaining
IDs, whereas a thread without a reserve reports exhaustion. This deliberately
conservative policy can reach exhaustion before all reserved IDs are consumed.
The public 60-bit ProcessId domain, FFI system_limit paths, uniqueness and absence
of counter wrapping/reuse are preserved.

## Regression checks

The RED test expected a single allocation to reserve 64 IDs; the previous
allocator advanced its counter to 2 rather than 65. Six focused tests now
cover cross-thread range ownership, 8,200 unique IDs across eight threads and
multiple refills, alternating allocators, owner destruction, final partial
ranges usable after global exhaustion, concurrent final-ID saturation, and the direct path under external contention.
The existing FFI exhaustion/reentrancy and typed PID/status tests remain covered.

## Performance method

Before and after optimized test executables are copied before later builds can
replace them. build_probe.py temporarily adds the archived probe to process.rs
and restores its original bytes, even on build failure. The baseline includes
the initial RED regression but has unchanged production allocator code.

The allocator probe measures a fixed total of 800,000 allocations at each of
1, 2, 4 and 8 threads, with a barrier and fresh caller threads. A prior untimed
pass warms the code; timing includes thread creation/join and barrier overhead.

spawn_probe.erl is separately compiled into a native executable for each runtime.
Four Erlang producer processes create 10,000 children each. Every child executes
and sends an acknowledgement to its producer; producers acknowledge the main
process only after receiving all 10,000 messages. The timed region covers real
spawn, execution and message delivery, excluding program/compiler startup and
printing. All children/producers terminate normally before executable exit.
The same 40,004 spawned-process workload runs at NERA_SCHEDULERS=1,2,4,8.

measure.py warms both versions, then alternates nine before/after pairs per
series. Three independent series retain every elapsed sample in microseconds.
All builds complete before measurement; no intentional idle-machine claim is
made. Benchmarks establish performance only on this machine and these workloads.

The initial-series-*.json samples retain the first implementation, which replaced
its Weak even when refilling for the same allocator. The final implementation
keeps that Weak; this removes shared reference-count updates from refills.
Initial measurement grouped counter and spawn probes in one pass. Final measure.py
separates their paired series so the counter load does not directly precede each
spawn measurement. initial-measure.py retains the original method.

## Reproduction

Recreate the reference checkout at the production baseline (no previous worktree
or uncommitted sources are required):

```sh
git worktree add --detach /tmp/nera-pid-block-reference 5fb648d55ccc2ac3818062537828f0805978e01f
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target python3 docs/baselines/2026-10-14-pid-blocks/build_probe.py /tmp/nera-pid-block-reference before /tmp/nera-pid-block-before
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target python3 docs/baselines/2026-10-14-pid-blocks/build_probe.py /tmp/nera-transferable-heap-fragments after /tmp/nera-pid-block-after
```

For each version, build its optimized compiler/runtime, then compile the same
spawn_probe.erl from a directory without nera.toml (to avoid project discovery):

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
cd /tmp
/tmp/nera-transfer-target/release/nera build /tmp/nera-transferable-heap-fragments/docs/baselines/2026-10-14-pid-blocks/spawn_probe.erl -e spawn_probe:main -o /tmp/nera-pid-block-spawn-after --no-warnings
```

Save the baseline executable as /tmp/nera-pid-block-spawn-before before building
the modified runtime. Once all compilation has completed, run measure.py with
both saved probe binaries, both spawn binaries and the JSON output path:

```sh
python3 docs/baselines/2026-10-14-pid-blocks/measure.py /tmp/nera-pid-block-before /tmp/nera-pid-block-after /tmp/nera-pid-block-spawn-before /tmp/nera-pid-block-spawn-after docs/baselines/2026-10-14-pid-blocks/series-1.json
```

all-blocks-series-*.json retain the next implementation, with Weak reuse but
block caching also enabled for one-worker schedulers. Its one-thread allocator
medians were 3–13% slower, motivating the direct path in the final implementation.
It must not be confused with the final series-*.json results.

## Final measurements

Median of all 27 samples per version (three series of nine paired runs), times
in milliseconds. Percentage is after/before minus one; negative means less time.

| Workers/caller threads | Counter before | Counter after | Counter change | Real spawn before | Real spawn after | Real spawn change |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1.933 | 2.057 | +6.41% | 100.209 | 99.833 | -0.38% |
| 2 | 4.848 | 1.246 | -74.30% | 57.482 | 59.647 | +3.77% |
| 4 | 12.911 | 1.793 | -86.11% | 46.677 | 44.922 | -3.76% |
| 8 | 28.557 | 2.139 | -92.51% | 43.894 | 44.050 | +0.36% |

The contention benefit is consistent across the three final series: counter
changes span -31.35 to -74.70% at two threads, -75.73 to -85.52% at four, and
-89.81 to -93.31% at eight. The single-worker direct mode still measures +3.77
to +8.49% in the isolated counter, a pooled 0.124 ms increase for 800,000 IDs;
its immutable mode check is not claimed free. Configured worker count equals
caller-thread count in this probe. A multi-worker scheduler used by only one
caller still uses block caching; that underutilized configuration is not measured.

Real spawn changes are mixed: series span -1.78 to +1.70% at one worker,
-2.81 to +6.16% at two, -9.44 to +4.96% at four and -1.99 to +2.11% at eight.
The pooled two-worker slowdown of +3.77% is retained, not normalized away.
These samples demonstrate less contention in allocation; they do not establish
an application-wide spawn speedup or absence of regressions for all workloads.

The host remained shared and loaded. Recorded load averages were
6.53 / 8.14 / 7.21 before final measurements and 4.93 / 7.18 / 6.95 afterward;
a one-minute load around 16 was observed during an earlier compilation. The
native mixed results need a quieter-machine comparison before drawing a small
performance-regression conclusion. Raw samples, timestamps and saved executable
hashes are retained in series-*.json, summary.json and environment.json.

## Final checks and scope

With RUSTC_WRAPPER=, CARGO_TARGET_DIR=/tmp/nera-transfer-target and locked inputs:

- Six focused allocator tests: passed (including the valid initial RED failure).
- cargo test --workspace --locked --no-fail-fast: **1,935 passed**, zero failed,
  one existing ignored.
- cargo test -p nera-runtime --release --locked: **609 passed** (569 unit, three
  compilation integration, 37 doctests), one existing ignored.
- cargo clippy --workspace --all-targets --locked -- -D warnings: passed.
- cargo build --release --locked: passed (1m 50s).
- Native scenarios with NERA_SCHEDULERS=4: **74 passed**, zero failed, two
  external onecached scenarios skipped / 76 selected. Selection: pid_domain_roundtrip,
  spawn_*, link_*, monitor_*, receive_*, hibernate_*, tcp_*, ets_*, base64*,
  process_info, process_info_snapshot_roots, process_flag_basic, selective_receive*
  and nested_receive; -j 1 --timeout 30, with local sockets allowed.
- Both compiled Erlang spawn executables completed every warm-up and timed run
  with 1, 2, 4 and 8 workers; every producer waited for all child acknowledgements.

The first sandboxed workspace run failed on ten socket/reactor permission checks.
The complete workspace and release runs above passed after allowing local sockets;
the failure log is retained as workspace-sandbox-tests.txt. No failures were waived.

source-hashes.json binds the production files, benchmark sources and locked build
inputs. The final production diff contains only process.rs, scheduler.rs and the
changelog; the report directory is also local. Temporary performance modules
were removed after each build. The implementation was initially left local. The user subsequently requested
updating and merging PR #85. Publication incorporates the remote branch
c27de6d35162f5bfa37c609f9f8bf594d9128a40, including main after PR #86
(f9a05490dd8b5c39a588b5157011df21e4b27c39). The task runtime and benchmark
source fingerprints remained identical; historical performance comparisons
retain their original baseline. source-hashes-before-publication.json preserves
the pre-integration changelog fingerprint.

This is macOS ARM64 validation only; Linux/Windows, cross-target, installed-archive
checks were not run in the initial implementation validation. CI/Release remain paused by repository
policy and were not enabled or rerun.

## Publication validation on main after PR #86

The refreshed workspace has **1,937 passing tests**, one existing ignored;
Clippy with warnings denied and the optimized build pass. Cowlib's runner/adapters
have **55 passing tests**. No runtime production code changed during integration.
A read-only independent Codex review of the complete PR against main f9a0549
reported no actionable correctness regressions in PID allocation, typed execution
status or ETF. Its scope included the current direct/block allocator code.
The full native suite, installed archive and cross checks are executed as the
local merge gate; their final results are recorded in the PR description.

## Additional cross-platform findings before merge

The Windows MSVC compile check exposed three missing ProcessId migrations in the
non-mio TCP reader, fixed by typing its destination parameter. Eight additional
Unix/reactor errors were reproduced on main f9a0549. Mio-only worker-read code
and its message helpers are now gated to mio-supported platforms; the non-mio
worker handler releases synthetic readiness notifications, while normal fallback
reader threads continue delivering owned message fragments. No new checks or
branches are added to the macOS/Linux mio worker read loop.

The Windows runtime compile check now passes, retaining seven existing
platform-specific unused-code warnings. This is compilation coverage, not Windows
execution or linking coverage. Native macOS x86_64 runtime checking, emitted
x86_64 Mach-O/aarch64 Linux ELF object smoke tests and both installed/extracted
and embedded runtime archive smoke tests passed before this final platform repair.
They are repeated as needed for the final revision and recorded in the PR.

source-hashes-measured.json retains the runtime sources used for the performance
measurements (available at ff1237bbade1e5432be7084e397b41c19ea1e90f);
source-hashes.json includes the final Windows repair. Measurements were not
rerun after the platform guards and fallback destination-type correction.
