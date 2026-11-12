# Typed context execution status and bounded process identifiers

Performance/development base: main after PR #82,
`9ebe3ff10fb67273e66b24b281151ffe00ffa0e1`.
Publication base: main after PRs #83/#84,
`a40da33eac57c0e8d5be343c3874848cb629331a`.
Branch/worktree: `mr/context-status-process-id` at
`/tmp/nera-transferable-heap-fragments`. macOS ARM64, Rust 1.99.0,
unchanged Cargo.lock. Development checks ran on the uncommitted worktree.

## Changes and Rust API migration

ProcessContext.status is ExecutionStatus, a repr(u32) enum with the seven
existing discriminants (Ok=0, Yield=1, ReceiveWait=2, Exception=3, IoWait=4,
Hibernate=5, TailCall=6). It remains separate from scheduler lifecycle state.
Runtime STATUS_* aliases are typed; codegen's numeric constants explicitly
cast those variants. Context offsets and the four-byte status slot remain
unchanged. Generated writers use the known constants; unsafe compiled-call
contracts require PID/status writes to preserve their typed domains. Raw
conversion uses TryFrom<u32> with InvalidExecutionStatus. No conversion or
validation is added when reading/writing an ordinary typed status.

ProcessId replaces the u64 alias with a transparent private word. new() checks
in release/debug and panics on invalid values; try_new()/TryFrom<u64> are
fallible. get()/From<ProcessId> expose the numeric value. MAX=(1<<60)-1 keeps
PID bits out of the existing external-term marker at bit 63. Zero remains an
unassigned-context value; scheduler allocation begins at one. Domain validity
does not establish that a process exists or that a manually created ID is
unique. Scheduler, monitor/link, timer, socket and ETS ownership paths retain
typed IDs rather than converting back to unchecked integers.

Constructors such as ProcessContext::new, Process::new and ErlTerm::pid now
accept ProcessId::new(1), for example. as_pid_id returns Option<ProcessId> and
rejects raw PID words with the reserved marker set. Validated PID encoding
remains a shift and OR. AtomicProcessId encapsulates the owner/initial-PID
words: only typed stores and reads are allowed, retaining their original
memory orderings. EtsTable.owner() replaces public access to its raw atomic.
SchedulerHandle's PID allocator is confined to the crate, with next_pid and
try_next_pid methods instead of a mutable public atomic counter.

ProcessIdAllocator uses a bounded relaxed atomic try_update/CAS loop, preserving
uniqueness under concurrency and leaving MAX+1 as the permanent exhausted
state. It cannot wrap to zero/reuse old IDs. next_pid panics on exhaustion;
try_next_pid returns None. All eight FFI spawn routes use the fallible form and
raise system_limit before creating/publishing a child or updating its links,
monitors and runnable/alive accounting. The bound check lives in allocation,
not in each subsequent PID encoding. This does change the isolated allocation
operation from fetch_add to a CAS loop; its measured cost is reported below.

ETF NEW_PID_EXT already has ID and serial words. Preserve the low/high PID
parts across those existing words instead of truncating IDs to 32 bits. Decode
checks the combined domain and reports badarg for reserved bits. Bytes for IDs
below 2^32 are unchanged. This retains the existing simplified local-node
representation; full distributed node/creation identity is not implemented.
Machine context layout and status values remain unchanged. Preview ABI changes
were allowed, but no machine layout change was needed; Rust APIs do change.

## Regression evidence

RED release-domain tests show the old encoder accepts reserved/truncated PID
bits. The raw-status assignment and raw-integer PID constructor compile-fail
tests both compile on the old API and therefore fail as intended. Logs are
red-domain.txt, red-status.txt and red-pid-api.txt.

Final tests cover all status ABI words and invalid conversions, PID boundaries,
reserved raw PID rejection, maximum-ID message copies, concurrent allocation
of the final three IDs exactly once, unchanged saturation after failure,
FFI system_limit without queue/table/accounting changes, and full ETF PID
roundtrip/rejection. Existing scheduler reentry coverage remains exercised.
The native pid_domain_roundtrip scenario covers maximum-ID ETF bytes, self-send,
closure capture/cross-process delivery and badarg for out-of-domain ETF input.
Other migrated tests retain their original assertions and explicitly construct
validated identifiers. Property tests compare decoded typed IDs.

## Performance reproduction

build_probe.py temporarily includes the probe inside process.rs and restores
its original bytes. Before is built in a clean detached checkout of the base;
after is built from the changed worktree. Both use the same release profile,
locked dependencies and Cargo target. Independent executable copies prevent
later builds from changing either measured binary. No probe remains in runtime
sources. Recreate the removed reference checkout with:

```
git worktree add --detach /tmp/nera-context-before-tree 9ebe3ff10fb67273e66b24b281151ffe00ffa0e1
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target python3 docs/baselines/2026-10-14-context-status-process-id/build_probe.py /tmp/nera-context-before-tree before /tmp/nera-context-before
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target python3 docs/baselines/2026-10-14-context-status-process-id/build_probe.py /tmp/nera-transferable-heap-fragments after /tmp/nera-context-after
python3 docs/baselines/2026-10-14-context-status-process-id/measure.py /tmp/nera-context-before /tmp/nera-context-after
```

Each series warms both binaries and alternates nine before/after pairs. Probes:
one million uncontended allocations; four threads allocating 100,000 IDs each;
ten million PID encode/decode pairs; ten million status stores/reads; 10,000
ProcessEntry/Process constructions including PID allocation; and four threads
allocating/publishing 500 processes each into the scheduler table/injector.
The last probe measures actual spawn publication, not execution of the spawned
processes. Objects are freed after timing; PID allocation and status samples are
isolated microbenchmarks, not whole-application performance measurements.

| Median elapsed change | Series 1 | Series 2 | Series 3 |
| --- | ---: | ---: | ---: |
| Uncontended PID allocation | +1.59% | +2.02% | +1.36% |
| Four-thread isolated PID allocation | +58.48% | +60.26% | +57.47% |
| PID encode/decode pair | −6.49% | −4.45% | −1.15% |
| Status stores/reads | −0.04% | +0.12% | −0.02% |
| Process construction | −1.25% | −1.39% | −1.39% |
| Four-thread spawn publication | +1.33% | 0.00% | −0.44% |

The bounded counter has a real contention cost and must not be described as
free. In these samples it does not cause a consistent slowdown in the measured
construction/publication paths. Status typing has no measurable cost here;
encoding/decoding samples do not prove a throughput improvement. ARM64 loop
excerpts are retained: the status loop remains nine instructions; the composed
PID encode/decode probe grows from seven to ten instructions because checked
raw decoding now rejects the external marker. Encoding itself adds no check.
Raw samples and process CPU time are in series-*.json. initial-series-*.json
retain the earlier probes without spawn publication, which prompted the more
representative concurrent measurement; they are not the final probe set.

The machine was shared: observed load averages included 3.71 / 3.63 / 3.65
while checks ran and 2.19 / 3.72 / 4.22 after measurements. No idle-host,
zero-cost-for-all-operations or application-wide speedup claim is made.

## Final validation and limits

With RUSTC_WRAPPER=, CARGO_TARGET_DIR=/tmp/nera-transfer-target and --locked:

- cargo test --workspace --no-fail-fast: **1,929 passed**, one existing ignored
  Rust test.
- cargo test -p nera-runtime --release: **604 passed** (564 unit, three compilation
  integration tests, 37 doctests), one existing ignored Rust test.
- cargo clippy --workspace --all-targets -- -D warnings: passed.
- cargo build --release: passed.
- Native: **74 passed**, zero failed, two external onecached scenarios skipped
  out of 76 selected, using -j 1 --timeout 30. Selection: pid_domain_roundtrip,
  spawn_*, link_*, monitor_*, receive_*, hibernate_*, tcp_*, ets_*, base64*,
  process_info, process_info_snapshot_roots, process_flag_basic,
  selective_receive* and nested_receive.
- Final scoped diff, fingerprints and whitespace checks: passed. Source hashes
  include the changed code/tests/changelog and locked build inputs. Temporary
  probe modules are absent from process.rs.

The baseline checkout was temporary and removed after measurement. The user subsequently requested publication as a PR. Before publication, the
branch was rebased onto main after PRs #83/#84. Only the changelog conflicted;
both entries were retained. Every previously validated production-code and test
fingerprint remained identical. The original benchmark binaries and measurements
remain comparisons against the PR #82 base, not new post-rebase measurements.
source-hashes-before-rebase.json retains the historical fingerprints;
source-hashes.json describes the publication sources. No merge is requested.
BASE_REF/BASE for publication is a40da33eac57c0e8d5be343c3874848cb629331a;
TARGET is the final task diff on that base.

This is macOS ARM64 validation only. Full native-suite, Linux/Windows,
cross-compilation and installed-archive checks were not run. CI/Release remain
paused per CONTRIBUTING.md and were not enabled/rerun. Independent review and
full merge checks remain necessary before a future merge.

## Publication checks after rebase

- Workspace tests: **1,930 passed**, zero failed, one existing ignored. The
  additional test comes from the updated main.
- Clippy with warnings denied: passed.
- Cowlib Python runner/adapters: **54 passed**.
- Optimized build: passed (2m 21s).
- Targeted native scenarios plus the upstream list-generator regression:
  **75 passed**, two external onecached scenarios skipped / 77 selected.
  The first sandboxed run passed all 66 non-TCP scenarios but denied socket
  creation (`eacces`) in all nine TCP tests. Rerunning those nine outside the
  restricted sandbox passed them all. Both logs are retained; the environmental
  failure is not counted as a passing run.

Original runtime release tests remain applicable: runtime sources, dependencies
and their tests did not change during rebase. Post-rebase logs are retained
separately, so development and publication evidence can be distinguished.
