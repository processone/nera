# Deferred binary closure captures and raw BIF safety contracts

2026-10-17; macOS ARM64, Rust 1.99.0, locked dependencies.
Base: main `699180ea51c106eca29750d5d0d0b5a5338cf5d4` (merged #108).
Target: uncommitted worktree on `mr/closure-captures-bif-contracts`.
`source-hashes.json` binds changed sources and fixture to the validation here.

## Changes

Before a closure captures a deferred binary rest, the compiler now uses the same
materialization/rebinding operation as an ordinary variable read. Repeated uses
capture and return the resulting ordinary bitstring rather than an undefined
register or a mutable matching cursor. Nested and named closures are covered.
Matching metadata is also saved, moved out, and restored with each nested
function's builder state: nested VarIds start from zero and must not inherit or
leave matching windows from a different function. Both changes are in compiler
lowering; nonescaping tail-recursive matching retains cursor reuse.

Twenty-four previously safe public Rust functions in `bif.rs` now require unsafe
calls when they read or traverse raw term graphs, directly or through another
helper. This covers list/tuple/map access, numeric comparisons, hashing,
conversions, formatting and ETF. Six private raw-reading helpers carry the same
contract. Safe operations restricted to immediate bits, borrowed byte slices or
new allocations remain safe. The contract requires initialized runtime layouts,
live owners (including external storage), immutable/unmoved inputs and no moving
GC or reentrant mutation during access. No new owner borrow is held across GC or
an Erlang callback.

Existing unsafe FFI adapters already establish raw-context/input guarantees. The
scheduler's crash formatter now has a locally documented unsafe call under the
process guard, and tests explicitly acknowledge their live fixture heaps. This
is a source-level Rust API change: embedders must establish the contract before
calling these functions. The C ABI and runtime BIF operations do not change.
A normalized production-token audit confirms `bif.rs` differs only in comments,
whitespace and unsafe markers (`bif-operation-audit.txt`). The benchmark's complete
common-0 IR is also byte-identical before/after, including stdlib (`ir-audit.txt`).

## Tests

RED/GREEN evidence retained:

- Capturing a deferred rest failed IR structural verification with an undeclared
  variable. The regression now verifies one materialization for simple, repeated,
  nested/named captures and a subsequent parent use.
- Nested-function metadata isolation failed when restoring the old shared-map
  behavior; it now rejects inheritance and restores the parent's own cursor.
- Three compile-fail examples for list traversal, formatting and serialization
  compiled successfully before the signature change, failing their tests. They
  now require an explicit unsafe call and pass.
- The native fixture checks 224 length/offset combinations, explicit equality,
  process sender/completion, named/nested closures, subsequent variable use,
  allocation pressure and ETS/process transfer. It passes on Nera and OTP 29.
  A completion marker is required. An initial test-only use of unsupported
  `erlang:garbage_collect/0` was replaced by observable allocation/hash work,
  following the existing closure GC regression strategy. This is allocation
  pressure evidence, not a measured count of GC cycles.

Passed: **2,013 workspace tests**, **664 runtime release tests**, release build,
workspace/all-target Clippy with warnings denied, Intel macOS and Windows MSVC
runtime cross checks, and OTP 29. Each Rust suite retains one existing ignored
test. Windows retains six existing fallback warnings. Cross checks are compilation
rather than execution evidence.

The broad native run passed 156 tests, skipped two external Onecached fixtures,
and failed the new fixture at compile time because of the unsupported GC BIF.
After correcting only that fixture, its focused revalidation passes with the
completion marker: **157 distinct native tests validated**, two external tests
not run. Both logs are retained; no failure was waived and the broad command was
not rerun in full after the fixture-only correction.

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test -p nera-runtime --release --locked
/tmp/nera-transfer-target/release/nera test 'closure*' 'fun*' 'binary*' 'bitstring*' 'list*' 'tuple*' 'bif*' 'type*' 'io*' 'format*' 'gc*' 'ets*' 'try*' 'catch*' 'match*' 'term*' phash2_refc_binary jaerlang2_convert 'tco_closure*' 'tail_call_closure*' 'guard_type*' -j 1 --timeout 30 --no-warnings
/tmp/nera-closure-bif-evidence/after-runtime/nera test closure_binary_captures -j 1 --timeout 30 --no-warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-apple-darwin
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-pc-windows-msvc
erlc -o /tmp/nera-closure-bif-evidence tests/closure_binary_captures.erl
erl +S 1 -noshell -pa /tmp/nera-closure-bif-evidence -eval 'ok=closure_binary_captures:test(),halt().'
```

The full native suite, external Onecached modules, installed archives, upstream
integration suites, actual Linux/Windows execution and contention were not
validated. CI/Release remain paused per CONTRIBUTING.md. No commit, push or PR
is part of this implementation; merge-time full local gates and independent
review remain required before a later merge.

## Performance

Before/after compiler and runtime archives were preserved separately; probes are
compiled in isolated directories with their matching adjacent runtime archive.
The manifests retain executable/archive hashes. Generate sources with
`python3 make_probes.py`, build each common probe with the before/after compiler
and each capture probe with the after compiler, then run `python3 measure.py`.
Example (run from the probe directory to avoid project source discovery):

```sh
/path/to/compiler build probe.erl -e probe:main -o /evidence/before-0 --no-warnings
# Common variants: before-0, after-0, before-3, after-3.
# Capture variants: after-capture-0, after-capture-3 (directories capture-0/3).
python3 measure.py
# Build scan-0/3 similarly as before-scan-0/3 and after-scan-0/3, then:
python3 measure_scan.py
```

Each campaign uses three series of five groups (15 observations per variant),
alternating/rotated order, equal warm-up, `+S 1`, explicit complete-result/checksum
validation, and no concurrent Nera builds or tests during timing. The host was
shared and loaded, not isolated. The ordinary closure and BIF cases use identical
workloads in both offset cohorts; only the scan's prefix differs by 0/3 bits.
Times include the tested call, comparison and loop work.

Initial medians in microseconds (lower is better):

| Case | Operations | Before cohort 0 | After cohort 0 | Change | Before cohort 3 | After cohort 3 | Change |
|---|---:|---:|---:|---:|---:|---:|---:|
| Ordinary capture/call, 16 bytes | 300,000 | 8,743 | 8,754 | +0.13% | 8,738 | 8,747 | +0.10% |
| Ordinary capture/call, 256 bytes | 100,000 | 2,919 | 2,923 | +0.14% | 2,920 | 2,929 | +0.31% |
| length/1, one element | 1,000,000 | 8,575 | 8,585 | +0.12% | 8,585 | 8,578 | -0.08% |
| length/1, 32 elements | 300,000 | 8,336 | 8,371 | +0.42% | 8,340 | 8,347 | +0.08% |
| tuple_size/1 + element/2 | 1,000,000 | 13,970 | 13,942 | -0.20% | 13,943 | 13,941 | -0.01% |
| list_to_atom/1 | 300,000 | 27,044 | 26,994 | -0.18% | 27,639 | 27,071 | -2.06% |
| Scan 256 bytes | 10,000 | 55,893 | 61,726 | +10.44% | 92,755 | 91,639 | -1.20% |

Ordinary closure/BIF differences range from -2.06% to +0.42%; this does not
establish a runtime gain from unsafe markers. The initial scan row was unstable:
aligned changes by series were +5.68%, +14.38%, -6.87%, and shifted changes were
+6.28%, +26.43%, +1.66%. Its +10.44% aggregate aligned result triggered a focused
follow-up with ten times the work per observation (100,000 scans of 256 bytes,
25.6 million byte-matching steps, plus equal warm-up). Existing initial observations
are retained rather than overwritten.

Long-scan medians (15 observations per variant):

| Prefix | Before, µs | After, µs | Change |
|---|---:|---:|---:|
| 0 bits | 497,237 | 496,248 | -0.20% |
| 3 bits | 899,124 | 860,126 | -4.34% |

This follow-up does not reproduce the initial aligned regression. Whole-process
user CPU medians (including warm-up and startup, so not directly comparable to
the timed-only wall samples) are approximately 0.9912→0.9877 s aligned and
1.6775→1.6716 s shifted. The -4.34% wall-time shifted result is not claimed as a
code optimization: before/after common benchmark IR is identical and the BIF
contract patch introduces no runtime operation. Host scheduling, code placement
and GC contributions are not isolated by this experiment.

Corrected direct captures have no valid legacy timing: they previously referenced
an undefined register. Their after-only medians below include matching,
materialization, closure creation/call and complete bitstring equality:

| Rest length | Operations | Aligned, µs | Shifted +3 bits, µs | Aligned ns/op | Shifted ns/op |
|---|---:|---:|---:|---:|---:|
| 16 bytes | 300,000 | 19,369 | 38,180 | 64.56 | 127.27 |
| 256 bytes | 100,000 | 91,483 | 137,430 | 914.83 | 1374.30 |

Materialization is only added when a deferred window escapes into a closure and
is performed once per binding. Existing materialization behavior is retained,
including its aligned refcounted-binary sharing path; the shifted packing buffer
is not removed here. These measurements must not be presented as a before/after
speedup of the previously broken capture case.

Load averages (1/5/15 minutes): initial campaign start/end
(6.68, 7.47, 6.12) / (9.14, 8.1, 6.41);
long scan start/end (11.04, 8.92, 6.92) / (13.06, 10.08, 7.57).
No statistically established performance guarantee, other-CPU result or contention
result is claimed. Both raw campaigns, scripts, probe sources and per-series
medians are retained.
