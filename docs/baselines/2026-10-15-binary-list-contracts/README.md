# Binary segment sizes and list operator contracts

Latest integration checks and measurements: [rebased report](../2026-10-15-binary-list-rebased/README.md). This file preserves historical pre-rebase evidence.

This report records the pre-filter checkpoint. The [tuple subtraction follow-up](../2026-10-15-tuple-subtraction/README.md) records the subsequent optimization and current validation.

2026-10-15. Base: main `764bbbad82148a0e61ab5429bde5b9db5bb9f50d` (PR #95).
Target: local worktree `mr/binary-segment-list-contracts`,
`/tmp/nera-transferable-heap-fragments`. Rust 1.99.0, unchanged Cargo.lock,
Apple M1 / macOS ARM64. No commit or publication is part of this validation.

## Contracts and implementation

Each explicit binary segment size must be a nonnegative integer before unit
multiplication or summing. SegmentBits checks the multiplication, usize conversion
and tagged bit-count domain. Invalid types and negative values raise badarg;
capacities outside the construction ABI raise system_limit. The allocator also
checks the rounded whole-byte descriptor domain before allocating, including the
seven highest bit counts that would overflow that intermediate representation.
Binary integer writes check offset-plus-length before either raw write path.

Valid literal sizes are checked/scaled while lowering. Valid dynamic small sizes
use inline tag/range checks and scaling, with no FFI call or status load on that
validation fast path. Total-size accumulation and the first segment offset reuse
the validated size instead of repeating checked additions to zero. Error
classification uses the runtime fallback. The final
allocation has an exception check so it cannot fall through into segment writes
or later expressions. Lexical try/catch receives an immediate error edge.

Segment units are checked in 1..=256 before narrowing in the parser, with a
matching lowering invariant for ASTs built without parsing. In particular unit:0
cannot reach the inline division, and large values cannot truncate to a valid unit.

For ++, the left spine must terminate in nil; any right tail is allowed. For --,
both spines must terminate in nil. These checks reuse the existing traversals.
Subtraction compares typed ErlTerm values using exact structural equality, so
separately allocated tuples, lists, binaries, maps and bigints can match without
conflating integers and floats. An immediate-only right operand retains the tight
word-comparison scan; structural equality has an inline flat-tuple path and
starts its general traversal without allocating a work vector. Nested graphs
retain the iterative traversal. All input graphs stay
live/unmoved during these short non-reentrant reads; no GC or Erlang callback is
introduced. Operator errors propagate before later expressions, including in try.

This change does not attempt to implement every remaining bit-syntax validation
rule (such as validating segment values/source kinds or float widths), change
binary matching semantics, or change the allocation-failure policy for otherwise
representable but physically unavailable memory.

## Regression evidence and checks

Both native regressions fail on the baseline: a floating segment size becomes an
empty binary, and improper-left ++ silently discards the tail (red.txt). A separate
unit-domain regression accepts unit:0 before the parser fix (red-unit.txt) and
passes afterward (green-unit.txt). Baseline exploratory reproduction also showed
-- failing to remove separately allocated equal tuples. Final native coverage
includes exact equality across compound types, duplicate removal, numeric type
distinctions, both malformed spines, permitted improper ++ right tails, finite
valid sizes, negative/float/noninteger/oversized sizes, unit scaling, total-size
overflow, descriptor rounding and immediate propagation inside/outside try/catch.
The runtime unit test checks boundaries without requesting enormous allocations.

Workspace tests are run with RUST_TEST_THREADS=1 because an initial parallel run
hit a shared global socket-registry interference in stale_handle_after_reuse.
The serial final run passes. A sandboxed preliminary run could not open sockets;
network-dependent checks were rerun with local socket access.

## Reproduction

Use RUSTC_WRAPPER= and CARGO_TARGET_DIR=/tmp/nera-transfer-target:

```sh
RUST_TEST_THREADS=1 cargo test --workspace --locked --no-fail-fast
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
RUST_TEST_THREADS=1 cargo test -p nera-runtime --release --locked
```

Cross checks compile nera-runtime for x86_64-pc-windows-msvc and
x86_64-apple-darwin; Windows retains its existing six unused-code fallback warnings.

## Performance method

Compile the identical probe.erl outside project discovery using the baseline
and final release compiler/runtime, entry probe:main. Preserve the executables as
/tmp/nera-binary-list-before and /tmp/nera-binary-list-after, then run measure.py twice, preserving its outputs as
performance-final-first.json and performance-final-repeat.json; combine.py pools
all six series without dropping observations.
Two final runs of three series of nine alternating pairs provide 54 observations
per version, with
warm-up subprocesses per series and one scheduler. Native timers exclude startup,
compilation and printing. No builds/tests run intentionally during measurement.

Each binary case constructs and checks 100,000 two-byte binaries after 100 warm-up
iterations: literal sizes, a dynamic bit size, a dynamic unit-8 size, and two
dynamic segments. Each list case runs 10,000 operations after ten warm-ups,
checking every result length. Append uses 200+20 immediate values; subtraction
removes 20 values from 200. The structural case subtracts ten disjoint flat tuples
from 50 tuples. Both versions return the same correct result for that disjoint
case, but only the final version implements general structural equality. Its
cost must not be hidden by claiming the baseline performed equivalent comparisons.

The full native suite, Linux/Windows execution, installed archives and contention
measurements are outside this focused validation. CI/Release remain paused per
CONTRIBUTING.md. Independent review and explicit approval are required before
merging. Code, fixtures and probe fingerprints bind the checks to this worktree.

Initial timings before the final fast-path optimizations are preserved in
performance-initial.json, with source-hashes-initial.json and executable hashes.
They motivated removing redundant zero additions and inlining flat-tuple equality.

## Final results

- Workspace: **1,969 passed**, zero failed, one existing ignored.
- Runtime release: **627 passed**, zero failed, one existing ignored.
- Native: **71 distinct tests passed**, zero failed/skipped, including the two
  regressions and binary/bitstring/list/map/comparison/exception/receive/GC cases.
- Locked release build, workspace/all-targets Clippy with -D warnings, and both
  runtime cross-compilation checks passed.

| Case | Before median µs | After median µs | Pooled change | Six-series range |
|---|---:|---:|---:|---:|
| constant | 1,913.0 | 1,908.0 | -0.26% | -7.82% to +10.65% |
| dynamic | 2,325.0 | 2,254.0 | -3.05% | -9.70% to +12.70% |
| unit | 2,818.5 | 2,726.0 | -3.28% | -4.69% to +5.74% |
| multi | 3,711.5 | 3,821.5 | +2.96% | -0.32% to +11.40% |
| append | 12,737.5 | 12,869.0 | +1.03% | -2.75% to +18.91% |
| subtract | 20,058.0 | 20,162.0 | +0.52% | -2.42% to +3.72% |
| structural | 5,549.5 | 20,314.5 | +266.06% | +240.30% to +281.77% |

The single dynamic-size pooled result was initially +5.96%; after removing
redundant zero additions the final experiments show -3.05%. These separate runs
do not isolate the optimization from host variation. Dynamic unit scaling is -3.28%. The literal-size
case is -0.26%. The two-segment case retains a possible +2.96% cost. Append and
immediate subtraction are +1.03% and +0.52%. Series vary substantially, including
sign changes; these are observations, not a promise of zero overhead or stable
speedups. Short 2–4 ms binary cases are particularly sensitive to host activity.

Structural subtraction is consistently slower: **+266.06%, or 3.66x**, after the
flat-tuple fast path reduced the initial +474.06% cost. The measured operation
contains 50 left and ten disjoint right tuples, hence 500 candidate comparisons.
Per subtraction that is approximately **0.555 µs before vs 2.031 µs after**.
The old code compared only addresses, and failed on separately allocated equal
terms. The final code performs the required value comparisons; this material cost
must remain explicit rather than describing the change as performance-neutral.
No extra locks, GC or comparison-buffer allocations occur in this flat-tuple case.
Nested graph comparisons can still allocate the existing iterative work buffer.

The machine was active. Final measurement start load averages (1/5/15 min):
[10.12353515625, 12.90234375, 10.0087890625]; end: [6.658203125, 11.2060546875, 9.58935546875]. No build or test was intentionally
run during either experiment. Recheck on an isolated host before generalizing.

## Integration state at final inspection

The branch was rebased before implementation. While checks/measurements ran,
main advanced to 4e14b65 through PRs #96 and #97. The validated worktree remains
based on PR #95 (764bbba), two commits behind that observed tip. Those newer
changes include compiler/runtime surfaces; the checks above do not establish
validation of a combined tree. Rebase and revalidate before publication/merge.
No unrelated work or shared main refs were changed by this task.
