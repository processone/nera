# Typed boxed object subtypes

Base: main after PR #72, `fadb66206453888fd6c1d44764113cd36c055f41`.
Worktree: `/tmp/nera-transferable-heap-fragments`, branch `mr/boxed-subtype`.
macOS ARM64, Rust 1.99.0, unchanged Cargo.lock. Checks below were run before publication.

## Contract and compatibility

`BoxedHeader::new/try_new` and the compiler's `encode_boxed_header` require
`BoxedSubtype` rather than arbitrary integers. Explicit discriminants preserve
all nine ABI values (0 through 8); numeric BOXED_* constants remain compatible.
Use `BoxedSubtype::Tuple`, for example, in Rust header constructors. Convert raw
inputs with `BoxedSubtype::try_from(raw)`; invalid/reserved values return
`InvalidBoxedSubtype` without masking into another object kind.

The enum uses repr(u64), matching the word-oriented header and avoiding a
narrower parameter representation. Only the encoded u64 is stored in objects;
object sizes, alignment and generated-code constants do not change. This
selects a tag, not a proof of initialized payload, ownership or pointer validity.

Raw header reading (`BoxedHeader(word)` / subtype()) retains the original mask.
GC and copy dispatch gain no tag validation or conversion. `with_arity` and
`try_with_arity` preserve the existing raw tag, including reserved tags, while
checking the 60-bit arity limit. Binary resizing uses this operation rather than
validating/decoding the subtype on the hot path. Updating a header still does
not prove the allocation accommodates its size.

## Test approach

The compile-fail examples were first run against the old constructors: all
three compiled, so the tests failed as intended (red-runtime.txt and
red-codegen.txt). Final regressions cover known ABI values, unknown values
including out-of-domain values, arity boundaries, reserved raw-tag preservation,
compiler/runtime header parity and shrinking inline/refcounted binaries without
changing subtype or permitting growth. Existing property tests now generate
known subtypes; raw-header tests cover all sixteen tag bit patterns separately.

## Performance reproduction

`build_probe.py` temporarily includes the checked-in probe in runtime tests and
restores process.rs. It saves independent optimized test executables before and
after the change. Both use the same locked dependencies, target directory and
release settings. Production source contains no probe module.

```
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target python3 docs/baselines/2026-10-13-boxed-subtype/build_probe.py /tmp/nera-transferable-heap-fragments before /tmp/nera-boxed-subtype-before
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target python3 docs/baselines/2026-10-13-boxed-subtype/build_probe.py /tmp/nera-transferable-heap-fragments after /tmp/nera-boxed-subtype-after
python3 docs/baselines/2026-10-13-boxed-subtype/measure.py /tmp/nera-boxed-subtype-before /tmp/nera-boxed-subtype-after
```

The before probe must be built on the base revision (the RED documentation
examples alone do not affect optimized constructors). Each series warms both
executables then alternates their order across nine pairs. Header measurements
perform ten million calls with black_box; heap_boxed performs one million each
of tuple, float and map allocations. Timing is in microseconds; raw samples and
whole-process CPU time are retained. Fallible conversion of external raw values
is not inserted in these measured runtime construction/read paths.

Exploratory u8-series-*.json record the initial repr(u8) candidate: dynamic
header construction rose 22.9–31.7% in these probes. This motivated the final
word-sized representation. These exploratory results are not final-state
validation. The machine was heavily loaded (initial load average 101.40,
42.61, 20.62), so elapsed-time variations must not be treated as precise
throughput estimates or proof of an improvement.

## Final measurements

Median elapsed-time changes (after/before − 1):

| Operation | Series 1 | Series 2 | Series 3 |
| --- | ---: | ---: | ---: |
| Fixed subtype header | −15.76% | +11.19% | +1.40% |
| Varying subtype header | −11.70% | +5.97% | −0.82% |
| Existing header resize | +3.18% | −2.56% | −2.37% |
| Tuple/float/map allocations | −3.78% | −3.06% | −1.14% |
| Whole probe CPU time | −6.87% | +1.75% | −1.86% |

These noisy measurements show no consistent penalty in the final representation;
they do not establish a throughput gain or rule out a small change on an idle
host. Final runs still observed a load average of 15.26 / 31.94 / 29.60.
The before/after ARM64 disassembly excerpts provide additional evidence:
fixed-tag construction has the same eight loop instructions, varying-tag
construction retains sixteen (one bitfield insert becomes a shifted OR), and
resize retains ten with reordered loads. Arity checks remain present. No new
allocation, decode branch or additional loop instruction is introduced in
these three measured construction operations. This is evidence for these
operations and this compiler/target, not a cross-platform guarantee.

## Final validation and limits

- Workspace tests: 1,910 passed; one pre-existing ignored test.
- Runtime release: 551 unit tests, three compilation integration tests and 34
  doctests (588 total); one pre-existing ignored test.
- Strict workspace/all-target Clippy: passed.
- Locked optimized release build: passed.
- Native scenarios: 78 passed, zero failed, two external onecached tests skipped
  out of 80 selected. Covers binary_*, bigint_*, closure_*, fun_closure,
  recursive_closure, map_*, maps_stdlib, atomics_basic, counters_basic, reference,
  ets_*, gc_* and hibernate_* (`-j 1 --timeout 30`).
- Scoped diff/whitespace review: clean. source-hashes.json records the final
  changed Rust files, changelog and build inputs; temporary probe modules have
  been removed. No toolchain or dependency changes.

Commands use `RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target`:
`cargo test --workspace --locked --no-fail-fast`,
`cargo test -p nera-runtime --release --locked`,
`cargo clippy --workspace --all-targets --locked -- -D warnings`,
`cargo build --release --locked`. Logs are retained beside this report.

Validation is macOS ARM64 only. The full native suite, Linux/Windows,
cross-compilation and installed archive checks were not run. GitHub CI/Release
remain paused by repository policy; no historical runs were restarted.
The target is the uncommitted worktree over the base above, with zero committed
ahead/behind divergence against origin/main at validation. The checks below precede the commit, push and
pull request creation; no merge is performed. Independent review and the broader merge
checks are still required before a future merge.
