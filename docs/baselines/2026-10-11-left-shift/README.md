# Left-shift overflow — 2026-10-11

Base: `bb20f3b` (main at worktree creation). Branch `mr/left-shift-overflow`.

## Correctness

The constant folder previously evaluated an i64 shift before checking for lost
bits: `1 bsl 63` became negative. Folding now computes in i128 and converts back
only when the result fits an i64 literal. Otherwise it retains runtime evaluation
through the existing bigint path. Valid representable constants still fold.

For dynamic small integers, checking the truncated machine result against the
61-bit range missed cases such as `16 bsl 60`, which became zero. Generated code
now shifts by `amount + TAG_BITS`, reverses that shift arithmetically, and compares
with the original value. The existing amount guard guarantees a total shift in
3..=63. An equal roundtrip proves both that no significant bits were lost and
that the mathematical result fits the signed 61-bit payload. Failure uses the
existing arbitrary-precision implementation. Right shifts are unchanged.

## Regression evidence

- The original constant-folding regression failed against the baseline.
- Both native fixtures failed at runtime before the correction: the constant
  fixture observed -9223372036854775808; the dynamic fixture observed zero.
- The initial fixtures used decimal literals beyond the parser's i64 range and
  failed lexing. Those setup failures are not counted as regression evidence;
  final fixtures parse decimal strings instead.
- Final IR cases use explicit expected values at signed i64 boundaries and
  unsupported shift counts. Native tests cover constant and dynamic cases,
  negative/zero operands, negative counts, and 715 combinations over shifts
  0..64. The sweep derives expected values by repeated addition, independently
  of shift implementation. Both native fixtures also pass on BEAM.

## Performance reproduction

Build `bench_bsl.erl` with the baseline and modified compilers, linking the exact
same release runtime archive. Use separate build directories because compilation
writes temporary object files in the current directory:

```
/path/to/compiler/nera build /path/to/bench_bsl.erl -e bench_bsl:main -o /path/to/bench
python3 measure.py /path/to/before/bench /path/to/after/bench > results.json
```

The compiler executables used for the comparison are debug builds; both generate
native code with Cranelift's normal `opt_level=speed`. Compiler execution time is
not measured. Each workload executes ten million dynamic shifts with small,
non-overflowing inputs, and verifies a checksum. The script warms both binaries
and alternates before/after order over nine pairs. Internal elapsed time covers
the loop; process CPU includes startup and scheduler threads. This is a focused
fast-path workload, not a general application throughput guarantee. Bigint cases
cannot fairly be compared against the old, incorrect truncated result.

## Separate defect observed

The first sweep used multiplication as its oracle and exposed an independent
bug: multiplying -1152921504606846976 by 16 returned zero, while the corrected
shift returned -18446744073709551616. A diagnostic run recorded both values.
This multiplication overflow is outside the current change; the final oracle
uses repeated addition and is independently checked on BEAM.

## Final validation and measurements

- Workspace Rust: **1,793 passed, 0 failed, 1 ignored**. The final explicit IR
  boundary table replaced the initial wide-arithmetic oracle afterward and its
  focused test passed again.
- Strict workspace/all-targets Clippy and release build passed.
- Native targeted suite: **10 passed, 0 failed** (shift fixtures, bigint arithmetic,
  formatting, factorial, float/basic arithmetic, inline arithmetic and integer
  sources). Both shift fixtures passed again with the release-built compiler.
- BEAM OTP fixtures: both passed, including the complete dynamic sweep.
- Whitespace/diff check passed. No Linux/Windows, Miri, sanitizers or complete
  Erlang suite were run at this pre-publication validation snapshot.

| Series | Before elapsed | After elapsed | Change | Before process CPU | After process CPU | Change |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| First | 68,995 µs | 68,832 µs | −0.24% | 73,721 µs | 74,751 µs | +1.40% |
| Repeat | 72,470 µs | 69,371 µs | −4.28% | 78,939 µs | 72,549 µs | −8.09% |

There is no reproducible slowdown in these samples. The machine was heavily
loaded: the observed 1/5/15-minute load averages were 28.45/33.19/36.29, with
FileProvider and Spotlight work active. Our compilation and targeted tests had
finished before the final measurement series, but these are still not controlled
idle-machine results. Neither a universal performance gain nor a zero-cost claim
is established. The runtime archive is identical before and after.
