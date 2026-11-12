# Multiplication overflow — 2026-10-11

Base: `9b64f62` (main at worktree creation). Branch `mr/multiplication-overflow`.
The separate bigint/heap and left-shift PRs are not prerequisites for this fix.

## Contract and implementation

The generated arithmetic helper previously multiplied two decoded operands with
`imul`, then tested the truncated low word against the signed 61-bit domain.
A product such as -1152921504606846976 * 16 became zero. Both runtime fallback
and constant folding already use checked multiplication; the defect was in the
native immediate path.

The new path clears the second operand's tag, yielding b * 8, which is exactly
representable as signed i64 for every signed 61-bit b. It computes a * (b * 8)
with Cranelift `smul_overflow`. Success is equivalent to a * b fitting the signed
61-bit domain, and already yields the shifted payload ready for its tag. On
overflow the existing bigint fallback receives the original operands. This
combines the machine overflow and immediate-domain checks. Zero, negative
operands and the asymmetric signed lower bound are included in the regression.

An `InlineArithmetic` enum selects the arithmetic operation and matching runtime
symbol together, replacing the separate computation closure and symbol string.
Addition and subtraction retain their prior range check; two 61-bit operands
cannot overflow their intermediate i64 sum/difference. Floating-point and badarith
handling still use the existing runtime path.

## Regression evidence

The baseline failed the focused native case with `badmatch,0`, while the expected
value was -18446744073709551616. The final fixture adds 289 explicit integer
products (17 boundary values in both operand positions), checked both in ordinary
expressions and guards. Reference strings were generated with Python integer
arithmetic and independently verified by BEAM. It also covers constant operands,
mixed integer/float multiplication, and badarith in expressions versus guard
failure. Cases are grouped into small functions so the test does not require a
single large literal list.

## Benchmark reproduction

Build `bench_mul.erl` with the pinned before/after compilers using separate build
directories, and the exact same release runtime archive next to each compiler:

```
/path/to/nera build /path/to/bench_mul.erl -e bench_mul:main -o /path/to/bench
python3 measure.py /path/to/before/bench /path/to/after/bench > results.json
```

Each sample executes ten million dynamic, non-overflowing signed multiplications
and checks the result. Nine alternating pairs follow one warmup per binary.
Internal elapsed time measures the loop; process CPU includes startup and runtime
threads. Compiler executables are debug builds, but both generate native code with
Cranelift's normal `opt_level=speed`; compiler time is excluded. This is a focused
common-path measurement, not an application throughput guarantee. We do not
compare correct bigint arithmetic to the old incorrect truncation as a performance
regression.

## Final validation

- Rust workspace: **1,794 passed, 0 failed, 1 ignored**.
- Strict workspace/all-targets Clippy and release compiler build passed.
- Targeted native suite: **8 passed, 0 failed**, including multiplication,
  bigint arithmetic/bitwise/factorial/formatting, basic/float and inline arithmetic.
- The final regression, with an additional constant-operand assertion, passed
  again with the release compiler and independently on BEAM OTP.
- x86_64-unknown-linux-gnu object generation for the dynamic multiplication
  benchmark passed; this checks backend lowering, not execution on x86-64.
- Diff whitespace check passed. No full Erlang suite, remote CI, Windows, Miri
  or sanitizers were run at this local validation snapshot.

## Performance results

| Series | Before elapsed | After elapsed | Change | Before CPU | After CPU | Change |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| First | 92,807 µs | 90,971 µs | −1.98% | 96,947 µs | 96,896 µs | −0.05% |
| Repeat | 90,487 µs | 91,575 µs | +1.20% | 96,396 µs | 96,581 µs | +0.19% |

No reproducible slowdown is observed in these samples. The CPU medians are
nearly unchanged, but this is not a universal zero-overhead claim. Our compilation
and regression runs had finished before measurement; the machine remained busy
with FileProvider/Spotlight and other background work. The pre-measurement
1/5/15-minute load averages were 32.09/32.65/33.72. Confirm small differences on
an idle machine before treating them as a regression or improvement.
