# Filter exact tuple subtraction candidates

Latest integration checks and measurements: [rebased report](../2026-10-15-binary-list-rebased/README.md). This file preserves historical pre-rebase evidence.

2026-10-15. Local worktree mr/binary-segment-list-contracts, based on main
764bbba (PR #95). This extends the previously validated binary/list changes;
no commit, push, PR or merge is part of this work. The comparison baseline is
the correct structural subtraction implementation, not PR #95's incorrect raw
pointer comparison. Before runtime/source hashes are preserved separately.

## Optimization and safety

Cache a lossy fingerprint for short tuples (one through eight immediate fields).
Preparation is deferred until 64 candidate comparisons have accumulated and at
least eight right candidates remain. Fast successful removals can therefore
finish without preparing any filter. For a left tuple, a different known fingerprint excludes a right candidate
without walking its graph. Equal fingerprints still require exact_equal. Unknown
fingerprints always use exact_equal, preserving maps, nested lists/tuples,
bigints, signed-zero floats and all other existing equality behavior.

FilteredCandidates stores each filter with its term in a single vector entry,
so removal cannot desynchronize two parallel arrays. Immediate operands select
the word-comparison loop once, outside traversal. Structural operands scan a
short prefix, then select either the structural or filtered loop once. There is
no per-element state-machine dispatch. Small operands skip preparation; nested-only
operands and wide tuples stay on the structural loop after one eligibility check. Only
eligible structural operands prepare an additional vector. No locks or global
caches are added. Graphs remain live, immutable and unmoved for the short FFI
call; fingerprint lifetime never crosses a callback or GC. Complexity remains
O(left_length * right_length); collisions affect speed, never correctness.

A regression constructs two distinct tuples with intentionally identical hashes,
verifies that they do not match, then consumes eight duplicates exactly once each.
A property test compares randomly generated tuple lists against a separate Rust
ordered-removal reference, covering empty operands, thresholds and duplicates.
Native tests add collisions and mixed flat/nested/map/float candidates to the
existing operator-domain and equality coverage.

## Benchmark method

Compile identical probe.erl files outside project discovery using before/after
release runtime, entry probe:main, to /tmp/nera-tuple-filter-before and
/tmp/nera-tuple-filter-after. Run measure.py: three series of nine alternating
pairs, 27 observations per version, warm-up subprocesses per series, one scheduler.
No builds/tests intentionally run during measurement. Timers exclude compilation,
startup and printing. Each case has 100 warm-up operations then 10,000 measured
subtractions; every complete result is checked against an independent Erlang
reference. These checks are included in both timings.

The legacy_correct group reuses the earlier length-checking probe against the
correct pre-filter implementation; legacy_original compares the same final binary
to the original raw-pointer baseline. For these disjoint tuple operands the old
code happens to return the correct result. The original baseline still fails
separately allocated equal tuples in general. These legacy groups also check
that the unchanged binary construction cases have not shifted materially.

Cases: immediate values; disjoint tuple tags; same-tag disjoint values; overlap;
duplicates; nested tuple fields; a two-element left/one-element right operand;
and wide tuples. Typical operands have 50 left and ten right values. Original
raw-pointer timing results are not directly comparable to these probes, which
check complete results rather than only lengths and include matching cases.

## Reproduction and limits

Rust 1.99.0, unchanged Cargo.lock, Apple M1/macOS ARM64. Use RUSTC_WRAPPER=,
CARGO_TARGET_DIR=/tmp/nera-transfer-target and RUST_TEST_THREADS=1 for tests:

```sh
cargo test --workspace --locked --no-fail-fast
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
cargo test -p nera-runtime --release --locked
```

Runtime compilation is also checked for Intel macOS and Windows MSVC. Windows
retains its six existing unused-code networking fallback warnings. CI/Release
remain paused. The full native suite, Linux/Windows execution, installed archives
and contention were not checked. The branch was rebased before the initial work;
main has since advanced through PRs #96/#97. Validation is for the explicit local
base/tree, not a combined tree with those newer changes. Rebase and revalidate
before publication/merge; independent review and explicit approval remain required.

The initial eager filter is preserved in performance-eager.json. It reduced
long disjoint tuple scans by 46–51% but slowed the duplicate-rich case by 27.6%.
This motivated deferred preparation, not a change to equality semantics. The
historical eager source fingerprints are separate from the final source hashes.

A second intermediate variant, performance-state-machine.json, deferred preparation
but still dispatched through an enum for each element. It retained 42–48% gains
on long scans but cost 13.2% on duplicates and 8.4% on immediate values in that
probe. The final implementation selects its loop outside traversal instead.
Graph reading returns a typed Result before the short destination-heap borrow.

## Final validation and timings

- Workspace: **1,971 passed**, zero failed, one existing ignored.
- Runtime release: **629 passed**, zero failed, one existing ignored.
- Native selection: **71 passed**, zero failed/skipped.
- Workspace/all-targets Clippy with -D warnings, locked release build, Intel macOS
  and Windows MSVC runtime compilation: passed (six existing Windows warnings).

The final measurements compare the correct pre-filter implementation to the
specialized adaptive implementation. All 27 samples/version/group and per-series
results are preserved; none were discarded.

| Complete-result probe | Before median µs | After median µs | Change | Three-series range |
|---|---:|---:|---:|---:|
| disjoint_tags | 29,843 | 17,059 | -42.84% | -43.85% to -41.91% |
| same_tag | 26,310 | 14,528 | -44.78% | -45.50% to -44.03% |
| overlap | 9,354 | 9,891 | +5.74% | +5.19% to +6.94% |
| duplicates | 5,014 | 5,390 | +7.50% | +4.75% to +8.53% |
| immediate | 9,516 | 9,632 | +1.22% | -0.05% to +1.47% |
| nested | 35,239 | 36,176 | +2.66% | +1.92% to +3.54% |
| small | 580 | 563 | -2.93% | -2.77% to +0.17% |
| wide | 20,773 | 20,767 | -0.03% | -0.83% to +2.61% |

The two long disjoint scans improve consistently by 43–45%. The duplicate-rich
case retains **+7.50%** (roughly **38 ns/operation**), partial overlap **+5.74%**
(roughly **54 ns/operation**), and nested elements **+2.66%**. These are real
tradeoffs in this probe, not a claim of universally zero overhead. Immediate
values are +1.22% in the 50/10 complete-result probe, while the 200/20 legacy
immediate workload is -0.32%; wide and small cases are close or faster.

In the original length-checking benchmark, the correct pre-filter path is
**19,835 → 9,892 µs (-50.13%)** per 10,000 subtractions; all three series improve
by 49.84–50.51%. The separate paired comparison to the raw-pointer original is
**5,538 → 9,882 µs (+78.44%)**, down from the earlier roughly +266% overhead.
The raw-pointer version still fails separately allocated equal tuple cases; it
is not a valid general equality implementation. This disjoint workload happens
to have the same correct result in both versions.

Other unchanged legacy cases show -0.38% dynamic binary sizing, +0.62% two-segment
construction, +0.81% append and -0.32% immediate subtraction. The literal binary
case is +3.59% (about 1.25 ns/construction) even though that code did not change
in this optimization. Do not generalize small timing differences on this host.

Host load averages (1/5/15 min) at measurement start: [8.0068359375, 7.1025390625, 6.708984375];
end: [6.14599609375, 6.728515625, 6.5869140625]. No build/test was intentionally run during timing.
An isolated-host rerun remains useful before claiming broader performance effects.

Final integration inspection: observed main 306f5fe is four commits ahead of
the validated PR #95 base. No readiness of that combined tree is claimed.
