# Executable runtime compatibility contracts

This index assigns each compatibility fact to an existing Erlang fixture. The
focused runner executes those assertions unchanged on native Nera (default and
one scheduler) and Erlang/OTP 29. It changes checking, not product behavior or
language support. This is the first guardrail for incremental compiler/runtime
refactoring, not an exception-routing refactor or a claim of full BEAM parity.

## Contract index

| Owning fixture | Semantic responsibility and intent source | Assertions and boundary |
| --- | --- | --- |
| [`try_operation_short_circuit`](../tests/try_operation_short_circuit.erl) | [Immediate failure transfer](https://github.com/processone/nera/commit/4e4dec124e0ecf244bd5c7ed77b3da81d29ab48a): the first raising operation stops later body effects. | Checks handler class/reason, mailbox and process-dictionary effects, guards, nested handlers, cleanup and unprotected operations. Its existing exit-reason projection does **not** establish exit-envelope or stack metadata equality. |
| [`try_after_reraise`](../tests/try_after_reraise.erl) | The fixture's stated regression: `after` runs on all exits and re-raises the held exception; cleanup exceptions replace it. | Checks all three classes, exactly-once cleanup, normal returns, stack preservation within each engine, suspended cleanup and garbage collection. Stack preservation does not mean native and BEAM traces have identical metadata. |
| [`error_2`](../tests/error_2.erl) | The fixture's stated intent: `error/2` raises the supplied reason through direct, tail, apply, fun and computed-module calls. | Checks exact class/reason, non-returning behavior, catch envelope and list-shaped traces. Does not assert retention of `Args`, file metadata or `error_info`. |
| [`float_arith_badarith`](../tests/float_arith_badarith.erl) | The fixture's stated regression: non-finite arithmetic and unrepresentable float operands raise `badarith`; earlier false filters stop evaluation. | Checks zero divisors, overflow, large operands, literal folds, finite results, signed zero/underflow, guard rejection and side effects. Existing uncaught-reason projection does not prove exit-envelope parity or numeric-conversion compatibility. |
| [`bin_comp_badarg`](../tests/bin_comp_badarg.erl) | The fixture's stated regression: each binary-comprehension body must be a bitstring at that iteration, not an iolist. | Checks invalid bodies, exact class/reason, preserved body exceptions, valid controls and ordered filter/body effects. The existing monitor assertion accepts two exit-envelope shapes; this runner does not strengthen or normalize them. |
| [`resume_exception`](../tests/resume_exception.erl) | [Suspended exception repair](PROJECT_STATUS.md#p0-1--exception-handlers-lost-across-suspension--repaired): handlers must survive receive suspension. | Checks exit/throw/error, nested callers, absent later dictionary effects, cleanup, message wakeup and normal return. Does not yet own the later arithmetic-context suspension pilot. |

Each fact stays in its owning fixture. The runner adds no second implementation
of Erlang semantics and performs no textual equality comparison of crash reports,
process IDs, generated helper names, paths or complete printed terms. It adds
no stack normalization and weakens no assertion. The documented inherited
boundaries are not passing assertions for tracked incompatibilities. Future
stack/envelope work should use its owning stricter regressions, not this corpus
as a substitute.

## Run the contracts

Build both artifacts from the same source, runtime first, with the pinned
Rust toolchain and lockfile:

```sh
cargo build --release --locked -p nera-runtime
cargo build --release --locked -p nera
python3 scripts/test-runtime-contracts.py \
  --compiler-source "$(git rev-parse HEAD)" --out /external/results/contracts.json
python3 scripts/test-runtime-contracts-runner.py \
  --out /external/results/contract-rejections.json
```

`--nera` and `--runtime` select other matching artifacts; the runtime defaults
beside the compiler. `--temp-parent` selects disposable storage; all compiled
programs are removed after execution. `--timeout` bounds each program (30 seconds
by default); compilation has a separate 120-second limit. Runs are serial.
`--fixture NAME` is a diagnostic subset, explicitly marked `full_corpus: false`;
it does not replace the complete acceptance run.

The runner copies the compiler and archive together into disposable storage,
compiles outside the checkout, and requires the native linker to report that
exact archive. The supplied source commit is explicitly **declared**, not inferred
from a version string: artifact hashes and observed linkage identify what ran,
but cannot authenticate the caller's build provenance. Retain ordered build logs
with the JSON, particularly for historical artifacts. Fixture source identity,
fixture hashes and fixture-checkout modifications are recorded separately.

The OTP probe reads `releases/29/OTP_VERSION` from the running installation;
a major-only version is insufficient. BEAM compilation uses that same running
OTP installation's `compile:file/2`, not an independently resolved compiler.
The compiler version, exact OTP patch,
platform, scheduler mode, full commands, working directories, elapsed times,
stdout, stderr, exit status, compilation status and completion are machine-readable.
Inherited native/OTP scheduler flags are cleared; `native-single` explicitly sets
`NERA_SCHEDULERS=1`. The other two modes use their default scheduler counts.

Each native and BEAM build uses a small entry module that requires `ok = test()`
and prints a second completion line only afterward. Success requires all of:

1. Compilation and execution actually happened.
2. The real fixture's assertions returned `ok` and the program exited zero.
3. The fixture's own `EXPECT` line and the entry module's return marker each
   appeared exactly once.

Missing commands, unsupported OTP, failed compilation, assertion errors, signals,
timeouts, missing execution and missing completion fail the command. JSON is
written for failed runs too. Outputs are not compared wholesale or normalized.

## Rejection and historical proof

`test-runtime-contracts-runner.py` uses disposable copies of the real `error_2`
fixture. Its unchanged control must pass in all three modes. Removing only the
printed marker must fail despite exit zero and a completed return. Changing its
expected reason to a deliberately incorrect atom must produce an actual assertion
failure in every mode. Exiting zero immediately after the fixture marker must
also fail because the entry module's return marker is absent. Compilation failure is not accepted as negative proof.
No tracked fixture or expectation is edited.

To demonstrate an owning regression, build the source immediately before the
immediate-failure-transfer repair, commit
`a47a763f6257ba1a48a161a6d3eb50eef4fae3fb`, with **its own runtime first**, then
run the current unchanged `try_operation_short_circuit` through the focused runner:

```sh
python3 scripts/test-runtime-contracts.py \
  --nera /external/historical/target/release/nera \
  --runtime /external/historical/target/release/libnera_runtime.a \
  --compiler-source a47a763f6257ba1a48a161a6d3eb50eef4fae3fb \
  --fixture try_operation_short_circuit \
  --out /external/results/historical-contract.json
```

The historical native execution must fail the fixture's assertions, while OTP
passes; a missing tool or compilation failure is not the required regression
proof. Do not substitute today's runtime for the historical archive.

## Gate and coverage limits

Run the complete focused corpus and rejection journey when their coverage changes,
and repeat changed test code ten consecutive times without retries. These checks
do not replace the full current [contributing gate](../CONTRIBUTING.md#running-tests),
strict lint, workspace tests, full serial native suite, stacktrace/allocator probes,
applicable integration/platform journeys, independent review or hosted checks.
The workflow configuration is unchanged. Performance probes for allocator, timer
and shutdown changes follow their own contributing-guide conditions.

A passing result proves only the assertions listed above on the recorded engine,
artifacts and platform. macOS native execution does not prove Linux or Windows
execution; cross-object generation does not prove target-native execution.
No new architecture, performance result or broader language support follows from
these contracts. Refactoring arithmetic exception routing remains a separate
change after these guardrails merge.
