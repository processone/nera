# Initial process keepalive regression

Base: `d0420d8800d78e641d592fe15e6b482881a8ddb9` (`origin/main`).
Branch: `mr/initial-process-keepalive`, in an isolated checkout. This test update changes no runtime
behavior, dependencies or toolchain version.

## Contract and synchronization

The old test waited for the child to finish and deleted the ETS table before
`test/0` returned. It therefore did not test the initial-process keepalive path.

The strengthened test sends its last signal to the linked child and returns
without any further blocking operations. The child waits until the parent is
reported as `waiting`; because the parent cannot wait inside `test/0` after
sending the signal, this establishes that the entry function has returned.
Polling handles both BEAM's `running` and `runnable` states and is bounded.
There is no fixed sleep used as proof of return.

Only then does the child verify the ETS owner, its link to the parent, increment
the counter and read it back. It prints `keepalive_after_entry_return_ok` and
calls `halt(0)` on success, or prints the exception and calls `halt(1)` on failure.
The runner's `EXPECT` directive also rejects a premature successful VM exit
without the success marker.

## BEAM comparison and launch semantics

Nera preserves its initial process when its entry function returns with links
still present. Plain `erl -noshell -s initial_process_keepalive test` does **not**
have that policy: BEAM's startup process terminates normally, deleting its ETS
table. The VM itself can remain alive. A separate monitored-owner probe observed
`owner_exit=normal table=undefined` on the installed OTP 29.

The Erlang [init documentation](https://www.erlang.org/doc/apps/erts/init.html)
explicitly describes termination of the initialization process after `-s`
functions return. The scheduler comment previously calling Nera's keepalive
policy “BEAM semantics” has been corrected.

To compare ETS access under equivalent owner lifetimes, keep the BEAM caller
alive explicitly after the same `test/0` call. This is a launcher adaptation,
not evidence that the two runtimes have identical default entry-return behavior.
Both executions let the child stop the VM after its assertions; neither appends
an immediate halt after the entry call.

From the checkout root:

```sh
mkdir -p target/keepalive-audit/beam
erlc -o target/keepalive-audit/beam tests/initial_process_keepalive.erl

# Equivalent owner lifetime: expected success on BEAM.
erl -noshell -pa target/keepalive-audit/beam -eval \
  'initial_process_keepalive:test(), receive after infinity -> ok end.'

# Native Nera lifecycle: expected success, checked by the standard runner.
./target/release/nera test initial_process_keepalive -j 1 --timeout 30

# Deliberately different owner lifetime: expected failure on BEAM.
erl -noshell -pa target/keepalive-audit/beam -s initial_process_keepalive test
```

## Regression sensitivity

In this checkout only, the scheduler's initial-process keepalive condition was
made false using `std::hint::black_box(false)`. Only the runtime was rebuilt;
the compiler selected the adjacent runtime archive when linking each fixture.

- The original `HEAD` fixture still exited successfully: the old test missed
  the regression.
- The strengthened fixture failed with exit 1 and
  `keepalive_after_entry_return_failed: error:{badmatch,[]}`. The parent
  no longer remains alive after return.
- Restoring the exact scheduler source and rebuilding the runtime made the
  strengthened fixture pass again, with its required success marker.

The temporary mutation is absent from the final diff. Detailed local logs are
under `target/keepalive-audit/`, including `old-test-mutant.log`,
`new-test-mutant.log`, `restored-runtime-build.log` and `new-test-green.log`.

## Focused verification

On macOS ARM64 with Rust 1.99.0 and Erlang/OTP 29:

- Fresh release workspace build: passed (3m 49s).
- Final release workspace build after restoring the mutation: passed (2m 30s).
- Final fixture compiled with `erlc -Werror`: passed.
- Nera: 100/100 runs passed with the required success marker and exit 0.
- BEAM with an explicit post-entry caller loop: 30/30 passed (10 default,
  10 with `+S 1:1`, 10 with `+S 2:2`).
- BEAM with plain `-s`: 10/10 expected failures, exit 1 with
  `{unexpected_parent_status,undefined}`. These are comparison controls, not
  failures of the Nera baseline.

Raw repeated-run results are in local `target/keepalive-audit/beam-results.json`
and `nera-repeat-results.json`. The final source hashes and check results are
recorded in `results.json` beside this report.

## Final surrounding checks

- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo test --workspace --locked`: 1,744 passed, zero failures.
- `./target/release/nera test -j 1 --timeout 30`: 285 passed,
  1 failure, 2 skipped / 288 total. The skipped tests require external
  onecached modules, as in the previous baseline.
- `git diff --check`: passed. The scheduler diff contains comments only;
  runtime logic is byte-for-byte unchanged after excluding comment lines.

Validation covers this uncommitted worktree based on the commit above. No CI
run, cross-platform validation, commit, push or pull request was performed.

## Remaining baseline failure: TCP rearm timeout

The complete baseline failed in `tcp_reactor_stress`, despite the keepalive
fixture passing. Before the runner's 30-second timeout, stderr contained
`error:{timeout_mp_rearm,19}` and `error:{timeout_mp_rearm,20}` in
`tcp_reactor_stress:mp_rearm_loop/3` (line 180). The initial baseline output is
retained in `target/keepalive-audit/erlang-baseline.log`.

One immediate focused rerun passed. A subsequent series of ten independent
compile/run attempts produced eight passes and two failures (attempts 2 and 7),
with the same timeout class. Their outputs are retained in `tcp-repeat-*.log`
and `tcp-repeat-results.json`. Passing retries do not clear the baseline failure.

The TCP fixture is unchanged, and all runtime executable logic matches the
base commit: only the scheduler comment was edited. The underlying TCP failure
has not been diagnosed or fixed by this scoped test update. The global baseline
must not be reported as green, and this remains a separate reliability blocker.
