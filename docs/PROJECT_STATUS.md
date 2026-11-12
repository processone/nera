# Nera public-preview status

Updated **2026-11-12** (baseline text below dated 2026-10-10). This is the active restart and preview-readiness record.
The wider roadmaps are design/history documents, not release checklists.

**Verdict: the three P0 fixes are implemented and verified on macOS arm64;
public-preview readiness is still blocked by the remaining items below.** The
repairs are recorded in separate commits. The original baseline is preserved;
[repair evidence](baselines/2026-10-10-repairs/README.md) records the new results.
The subsequent [toolchain refresh](baselines/2026-10-10-toolchain/README.md)
pins Rust 1.99.0 and upgrades Cranelift to 0.129.2 with no new local regressions.
The [deprecation follow-up](baselines/2026-10-10-deprecation/README.md) now passes
all **1,686 Rust tests** locally, with 285 Erlang passes and two expected skips.
The follow-up is recorded with its tests and baseline; remote CI results below
refer to the earlier PR head.
No release has been published.

## Experimental release plan — 2026-11-12

The first release is an **experimental prerelease** (milestone A), not the
public preview described in the rest of this record. Its gates:

1. Cowlib builds cleanly with `nera check`. Done: Cowlib 2.22.0 checks with
   0 errors and builds natively; see [`examples/cowlib_demo`](../examples/cowlib_demo).
2. Linux x86-64 is verified natively. Open: no native Linux run since CI was
   paused on 2026-10-12; this needs the repository public and the workflows
   re-enabled.
3. Five wrong-result or hang bugs are fixed: atom ordering, a `[]` message read
   as an empty mailbox, garbage-collection crashes on deeply nested lists,
   `ets:new/2` with `named_table`, and `start_link` hanging when `init` crashes.
   All five are fixed.
4. [Known limits](KNOWN_LIMITS.md), this status and the install docs are
   current. Done with this update.

Since the October baseline, main passes the local gate on macOS arm64 with
2,118 Rust workspace tests and 450 Erlang tests (2 skipped: external OneCached
modules); the early-November binary, `lists`, `maps`, `crypto` and `zlib`
fixes are listed in [CHANGELOG.md](../CHANGELOG.md). Tags of the form
`vX.Y.Z-experimental` publish as GitHub prereleases ([RELEASE.md](RELEASE.md)).

The next milestones are Cowboy over plain HTTP (about 4 to 6 weeks after the
experimental release), then Cowboy with TLS. The public announcement waits for
plain-HTTP Cowboy. The October record below is kept as written; where it
conflicts with this section, this section is newer.

## Revision and evidence

- Baseline: `main` at `c5b89ee65d679da1f2fe257aef73f130859440bb` (2026-04-17).
- Source tree: `39856e395ff12a1548741a2cd895975e5e80a856`.
- Local `origin/main` matches that commit; this is not a remote-ref freshness claim.
- Platform tested: macOS 15.7.7, arm64; Rust 1.97.1; BEAM OTP 29.
- [Dated baseline, commands and limits](baselines/2026-10-10/README.md).
- [Machine-readable results](baselines/2026-10-10/results.json),
  [test summary](baselines/2026-10-10/test-summary.txt), and
  [small BEAM comparison probes](baselines/2026-10-10/reproduce.py).

GitHub had no run for the baseline commit when checked. The latest returned
passing run covered `7bf2b0930ca0d32dbdebe3cff721991df953c182`, not this revision:
[CI run 22662245174](https://github.com/processone/nera/actions/runs/22662245174).
The repository was private at baseline review time; no release was published.

## Original baseline (before repairs)

| Check | Result | Meaning |
| --- | --- | --- |
| Release build, existing checkout | Pass | Rebuilt current source and runtime using the recorded lock snapshot |
| Full Rust workspace, no-fail-fast | **1,671 pass; 6 fail** | All crates exercised; 436 runtime tests pass |
| Erlang suite, first run, 4 jobs | **282 pass; 2 fail; 2 skip / 286** | TCP timeout and variance failure while other builds were running |
| Erlang suite, full repeat after builds | **284 pass; 0 fail; 2 skip / 286** | Original intermittent failures remain unexplained |
| TCP failures, individual reruns | 3/3 pass for each | Does not erase the original failures or establish a root cause |
| Thoas 1.2.1, freshly downloaded | Pass | JSON integration builds and runs |
| Poolboy 1.5.2, freshly downloaded | **Compile failure** | Five missing ETS/supervisor APIs; execution not reached |
| Clean-source macOS arm64 archive build | Pass | Packaging succeeds, but archive usability fails below |
| Unpacked archive → Hello World | **Fail** | Runtime library not found; link fails |
| Archive runtime-path control | Pass | Copying the shipped library to the searched path fixes this smoke test |
| Rust formatting check | Fail | Existing differences in 123 files; no bulk reformat performed |
| Clippy, all targets, warnings denied | Fail | Stops on eight syntax-crate lint findings; not a full lint inventory |
| Linux execution, long soak, fuzz campaigns | Not run | Not implied by macOS results or the existence of test infrastructure |

There are 296 tracked Erlang files under `tests/`, including helper modules;
the runner discovers 286 tests. The two skips require external OneCached modules.

## Repair baseline

- Release build: pass with the original dependency lock snapshot.
- Full Rust workspace: **1,677 passed; 5 failed**. All remaining failures are
  the previously recorded deprecation diagnostics; the formatter suites pass.
- Full Erlang suite: **285 passed; 0 failed; 2 skipped / 287**, after builds
  finished. Both TCP tests pass this run; the original intermittent failures
  still need a root cause. The skips still require external OneCached modules.
- Exception regression: fails on the original runtime, passes on BEAM and the
  repaired compiler, including explicit completion. Existing stack-overflow
  coverage passes.
- Formatter: five new deterministic regression tests pass. Original/formatted
  programs have the same BEAM results for operator semantics, `guard_not` and
  `fun_parallel_scopes`; the original guard comparison probe also passes.
- Fresh-source macOS arm64 archive: builds and compiles/runs outside the checkout
  with an empty cache. Removing the separate runtime library still passes via
  the embedded copy. Compiler and static library both identify as arm64.
- Thoas: rebuild/run pass with dependency sources matching the original baseline.
  Poolboy still fails compilation on the same five missing APIs.
- Formatting and strict lint checks retain the existing failures. Linux,
  Windows and cross-target execution remain unverified.

## Concrete blockers and repaired defects

### P0-1 — Exception handlers lost across suspension — REPAIRED

**Original defect, now regression-tested and repaired.** A function waits in `receive`, then raises
`exit(timeout)`. Its caller's `catch` is bypassed; the process exits with status
0 without executing the following statement. BEAM executes the handler and
continues. An immediate exit is caught correctly; a delayed normal return also
continues correctly.

Reproduction: [resume_exception_probe.erl](baselines/2026-10-10/resume_exception_probe.erl).
Expected stdout: `before` then `after`. Nera prints only `before`.

At baseline, the runtime's saved-continuation loop in
`crates/nera-runtime/src/process.rs` (`Process::run_slice`) resumes outer callers
only while status is `STATUS_OK`. A resumed exception therefore never reaches
the outer compiled handler. The scheduler treats exit-class exceptions as quiet
process exits, explaining the lack of a crash report and successful exit status.

**Met locally:** exception-aware unwinding through saved continuations, with
BEAM comparisons covering `catch` and `try`, error/exit/throw, nested callers,
timeout and message wakeups, plus explicit completion assertions. Do not fix this
by merely suppressing the exception or treating every continuation as a handler.

### P0-2 — Formatter changes program behaviour — REPAIRED

**Original defect, now regression-tested and repaired.** Formatting `when not (A < B)` produces
`when not A < B`. BEAM prints `matched` before formatting and `fallback` afterward
for the supplied input. `Expr::UnaryOp` in `crates/nera-fmt/src/format.rs` prints
its operand without preserving the needed binary-expression parentheses.

Reproduction: [formatter_guard_probe.erl](baselines/2026-10-10/formatter_guard_probe.erl).
The existing `idempotent` property test also fails on `tests/guard_not.erl`.

**Met locally:** preserve unary operand grouping, add semantic comparisons
using BEAM and pass the formatter property suite. The repair also preserves
right-nested binary expressions and tests nested negation. This closes the
reproduced defects; it is not proof of full Erlang formatter equivalence.

### P0-3 — Clean release archive cannot compile Hello World — REPAIRED

**Original defect, repaired and verified on macOS arm64.** Running the release workflow's distribution
command in a clean source copy produces a macOS arm64 archive that cannot link
Hello World when unpacked outside the source tree with an empty runtime cache.

Three contributing facts:

1. `scripts/dist.sh` derives `arm64-apple-darwin` from `uname`, but the workflow
   passes `aarch64-apple-darwin`. A native build takes its cross-build branch.
2. That branch builds only the compiler first. On a clean tree its generated
   `EMBEDDED_RUNTIME` is `None`; the runtime is built afterward.
3. The archive puts the library at `lib/nera/libnera_runtime.a`, whereas host
   lookup searches `lib/libnera_runtime.a`. Copying the shipped library to that
   searched path in the disposable installation makes Hello World work.

**Met locally:** canonical Rust host triples, ordered runtime/compiler builds,
consistent installed layout, and unpacked archive tests for both external and
embedded libraries. Cross builds now target both artifacts at the advertised
platform, but those archives still require target-native execution checks.

### P1-1 — TCP stability and trustworthy completion checks

The first full run timed out in `tcp_reactor` after 30 seconds and failed
`tcp_reactor_perf` with `{batch_variance_stall,165,25,[15,21,25,64,165]}`.
Both passed three isolated repeats; an instrumented reactor copy also completed
every subtest and exited. A full repeat after our builds finished passed all 284
enabled tests. The original timeout's location and root cause remain
unproven. Background compiler load is a hypothesis, not an established excuse.

The test runner accepts exit 0 without proof of completion unless `%% EXPECT:`
is supplied. P0-1 demonstrates why this is insufficient. Several reactor tests
also end with `halt(0)`, so their success does not validate graceful shutdown.

**Exit criterion:** retain timeout diagnostics, add explicit completion evidence,
separate correctness gates from noisy performance checks, and demonstrate
repeated TCP/close/shutdown tests without unexplained failures. Include a bounded
service soak before calling the native-service preview reliable.

### P1-2 — Deprecation diagnostic metadata — REPAIRED LOCALLY

The five original failures were caused by the parser discarding `-deprecated`
metadata. The parser now preserves explicit signatures and expands module or
wildcard declarations against exported functions. Existing diagnostics receive
the metadata; the five tests are unchanged and pass. Four additional tests
cover standard attribute forms, arity selection, spans and malformed input.

The local workspace passes **1,686 tests with zero failures**. See the
[follow-up baseline](baselines/2026-10-10-deprecation/README.md). The correction
is covered by the local baseline; remote validation must use its published head.

### P1-3 — Reproducibility and release gates

The Rust refresh pins 1.99.0 in the project and CI, makes the root `Cargo.lock`
eligible for version control, and requires locked builds in CI and packaging.
The local macOS baseline found no new regressions with dependencies unchanged.
These changes are recorded in separate Rust and Cranelift commits.
See the [toolchain refresh](baselines/2026-10-10-toolchain/README.md) for separate
Rust and Cranelift evidence.

CI is PR-only. The packaging repair adds installed-archive smoke tests to the
native macOS/Linux release jobs, but the tag-triggered workflow still lacks a
full source-test gate and cross-target/Windows execution checks. PR #1 native
Linux/macOS jobs fail on the original deprecation tests. The earlier cross jobs
loaded repository fixtures into the Hello World smoke test. The
[CI isolation repair](baselines/2026-10-10-cross-ci/README.md) now runs the smoke
test in a temporary directory and verifies the exact object architecture; local
macOS x86_64 and Linux ARM64 object generation pass. See the follow-up baselines
for the earlier remote failures and the local correction evidence.

**Exit criterion:** version the intended dependency resolution, document/test the
toolchain policy, and require the source checks plus archive execution tests for
the exact release commit on the advertised platforms. Editor plugins and extra
platforms should not become accidental prerequisites for the first core preview.

## Scope decisions, not automatic feature-expansion tasks

- **Poolboy:** blocked by `ets:match/2`, `ets:select/2`,
  `supervisor:start_child/2`, `supervisor:terminate_child/2`, and
  `supervisor:which_children/1`. Either implement these if the reference service
  needs them, or explicitly exclude Poolboy/full OTP compatibility. Its optional
  CI step does not establish support.
- **Late replies / process aliases — now confirmed:** after the exception repair,
  the saved `gen_server:call/3` probe reaches its mailbox check. Nera prints
  `late_reply`; BEAM prints `no_late_reply`. Replies still target caller PIDs.
  Fix this or explicitly bound modern OTP timeout compatibility before promising
  it. This is a separate compatibility gap, not part of the three P0 repairs.
- **OneCached:** reference-workload tooling exists, but its harness defaults to
  externally prepared binaries and the test suite skips external modules. Pin
  the source, dependencies and build/start/stop commands before treating it as a
  reproducible release gate. No throughput claim was revalidated here.
- **Proposed first preview:** native single-node Erlang services on Linux x86-64
  and macOS arm64, demonstrated by a pinned reference service. This remains a
  release-scope proposal, not a tested compatibility promise.
- Defer hot upgrades, distributed Erlang, broad flagship migrations, new language
  features and scheduler-performance work. Missing libraries become blockers
  only when the declared preview workload requires them.

## Next work

1. Review the three separate P0 repairs and their recorded evidence.
2. Verify CI for the published deprecation and cross-compilation isolation fixes.
   Investigate TCP failures with retained diagnostics and completion assertions.
3. Resolve or explicitly bound late-reply compatibility, freeze dependencies,
   and complete source-test and target-native archive gates in CI.
4. Pin and exercise the reference service, publish the supported subset, then
   reassess against a new exact-commit baseline on both proposed platforms.

Rustfmt/Clippy cleanup is recorded separately from runtime correctness. Fuzz
targets and proptests exist, but their existence is not evidence of a successful
fuzz campaign or comprehensive memory-safety verification.
