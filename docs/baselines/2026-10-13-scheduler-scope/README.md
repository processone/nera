# Scoped scheduler access

Base: `ff9828a2561214dc0a370381cc2c7f329fccdacb` (merged PR #57).
Branch: `mr/scheduler-borrow-scope`. Validation on macOS ARM64, 2026-10-13,
with the pinned Rust 1.99 toolchain and dependency lock.

## Contract

`with_sched_from_ctx` replaces the fabricated `&'static SchedulerHandle` with
an operation-scoped shared borrow. Its higher-ranked callback cannot return the
reference or stash it outside the callback. The unsafe FFI caller must keep the
scheduler owner alive and shared-accessible until the callback returns, including
unwinding. This expresses the actual guarantee; it does not infer ownership from
a raw pointer or make arbitrary scheduler destruction during reentry safe.

The context's runtime pointer is copied before the callback. No context, process
or heap reference is retained by the helper, so reentrant calls, context mutation
and GC remain possible. Existing caller-specific heap/lock restrictions remain.
Null context/runtime still uses the global singleton. The local path does not
clone an Arc, allocate, lock, or add a validity check. Existing self-send and
self-process-info fast paths still precede scheduler lookup.

Callers use explicitly inlined closures. Initial release inspection showed that
large callbacks could otherwise be outlined despite the helper's own inlining.
The helper resolves its Option once and invokes the callback once, rather than
duplicating the callback at the local/global branches.

## Tests

- Full final Rust workspace: **1,837 passed, zero failed, one ignored**.
  Runtime: 500 unit tests, one private-helper compilation test and 15 doctests
  passed; one existing unit test ignored.
- The compilation test checks the actual private helper source with minimal
  context/scheduler types. An owned result compiles; returning the scheduler as
  static or capturing it outside the callback fails specifically on lifetime
  constraints. The legacy helper accepts the static escape counterexample.
  The helper stays private; no production API is exposed for testing.
- A local, non-static Arc owner test verifies nested access, context mutation,
  pointer identity and an unchanged Arc strong count. Null context and null
  runtime fallback are also covered.
- **33 final native Erlang scenarios passed**, with completion and EXPECT checks:
  spawn_*, register_*, link_*, monitor_*, timer_*, hibernate_*, self_send,
  unlink_basic, demonitor_basic, process_info, bif_process_alive,
  scheduler_yield, nested_dynamic_tail_call, tail_call_closure_wrapper,
  tco_closure_tail, tco_dynamic_apply, tcp_active_once and file_io.
- Final release runtime: 500 unit tests, one compilation test and 15 doctests
  passed, one ignored.
- Final release build, strict workspace/all-targets Clippy and whitespace
  validation are recorded in validation.txt.

Commands (shared target cache, RUSTC_WRAPPER cleared):

```sh
cargo test --workspace --locked --no-fail-fast
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
/path/to/pinned/nera test 'spawn_*' 'register_*' 'link_*' 'monitor_*' 'timer_*' 'hibernate_*' self_send unlink_basic demonitor_basic process_info bif_process_alive scheduler_yield nested_dynamic_tail_call tail_call_closure_wrapper tco_closure_tail tco_dynamic_apply tcp_active_once file_io -j 1 --timeout 30
```

The CLI and runtime archive were copied together before native execution so the
runner linked the tested archive rather than a changing shared build cache.

Full native Erlang suite, Linux/Windows/32-bit execution, Miri and sanitizers
were not run. GitHub workflows remain suspended by repository policy. Evidence
was captured before publication. The full native suite and independent review
remain required before merging during the CI pause.

## Performance method

Before/after release runtime archives were built with
`cargo build --release --locked -p nera-runtime`, in the **same worktree path**.
For the baseline build, the five edited source files were saved, replaced with
exact HEAD contents, and restored in a finally block; no Git reset was used.
Using a different manifest path initially changed crate identity and private
structure layout, so that comparison is not used as final evidence.

`assembly.json` compares instruction encodings of 22 exported FFI functions.
`is_process_alive` has identical encodings (38 instructions). Other functions
have register/control-flow differences; instruction count alone is not a timing
measurement, and the check does not compare relocation targets. There is no
claim that every function's machine code is identical.

`probe.erl` is compiled by one identical pinned CLI, separately linking each
archive. It asserts results for ten million paired whereis/is_process_alive
lookups, three million monitor/demonitor pairs and ten million registered-name
self sends followed immediately by receive. Workers use the runtime's default
CPU count. Each series has a warmup and nine alternating before/after pairs.
Elapsed microseconds cover each loop; whole-process CPU also includes startup,
GC, scheduler activity and the other loops.

```sh
python3 compare_assembly.py BEFORE_ARCHIVE AFTER_ARCHIVE > assembly.json
# From isolated directories containing identical nera CLIs and their respective archives:
./nera build /absolute/path/probe.erl -e probe:test -o probe
python3 measure.py BEFORE_PROBE AFTER_PROBE > results.json
```

The shorter exploratory runs are retained as results-short*.json (one tenth
of the final loop counts). Lookup medians ranged from -1.65% to +13.89%, with
large outliers, which motivated longer measurements. The machine is shared,
not controlled or guaranteed idle; observed load averages before measurements
were 2.30/3.70/4.88. Small differences must not be interpreted as reliable gains.

## Final longer measurements

| Series | Metric | Before µs | After µs | Change |
| --- | --- | ---: | ---: | ---: |
| results.json | lookup | 465,524 | 461,172 | -0.93% |
| results.json | monitor | 531,937 | 500,813 | -5.85% |
| results.json | self_send | 243,952 | 239,457 | -1.84% |
| results.json | process_cpu_us | 1,203,958 | 1,182,141 | -1.81% |
| results-repeat.json | lookup | 416,692 | 414,369 | -0.56% |
| results-repeat.json | monitor | 483,964 | 486,706 | +0.57% |
| results-repeat.json | self_send | 228,079 | 228,068 | -0.00% |
| results-repeat.json | process_cpu_us | 1,143,591 | 1,154,391 | +0.94% |

No material regression was observed in these three workloads. Whole-process CPU
varied from -1.81% to +0.94% across the two series. This is not proof of zero
cost for every scheduler operation or a reliable speedup; after measurement the
machine's load averages were 7.18/4.65/4.70. The timer thread-spawn path and
cross-process message throughput were functionally tested, not benchmarked here.
`source-hashes.json` identifies the exact production/test source snapshot and the
pinned benchmark artifacts. `erlang.txt` records the final native test outcomes.
