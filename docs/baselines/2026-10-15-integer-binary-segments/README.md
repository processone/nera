# Integer/binary segment contracts and integer decoding

2026-10-15. Base main `902702e` (merged PR #104), local branch
`mr/integer-binary-segments`. Validation snapshot recorded before publication.
`source-hashes.json` binds the changed Rust sources, native fixture, toolchain
and dependency lock to the evidence retained here.
Publication was subsequently rebased onto main #105; the final section records
fresh integration checks and distinguishes them from the original timings.

## Behavior and safety

Integer construction rejects nonintegers rather than substituting zero, even
for a zero-width segment. Binary construction rejects non-bitstring sources and
source ranges shorter than the requested segment. All-source construction also
checks divisibility by its declared unit: `/binary` requires a whole number of
bytes, whereas `/bitstring` admits partial bytes. Explicit zero-byte binary
segments may use a bitstring source, matching OTP.

Both internal write adapters now return a tagged bit count or nil with `badarg`.
Generated code checks the result directly outside lexical handlers; inside a
handler, explicit exception edges stop construction. Integer writes validate
against exact logical bits before any byte mutation. Values are checked at write
time, after destination allocation; this does not promise pre-allocation value
validation. No allocation is added to the valid immediate-integer write path.

Integer decoding supports zero bits and widths above 64, promoting to BigInt
where needed. Partial little-endian groups and native order are handled explicitly.
Wide decoding establishes a bounded input slice at the unsafe boundary, then uses
ordinary checked indexing in a Rust function to create an owned integer. Input
byte borrows end before destination heap allocation. The term-taking adapter
checks exact logical bounds, including a match context's logical end. The raw
adapter has an explicit unsafe backing-range contract; generated loads depend
on dominating pattern bounds checks.

Dynamic pattern sizes validate type, sign and scaled range before any raw access,
including wildcards. Invalid sizes produce pattern failure without raising an
exception. Valid size checks stay inline; the matching case selects its nil
sentinel without a runtime call. Fixed segments following dynamic offsets check
the remaining range as well. When multiple fixed fields follow, the dynamic
check reserves their combined tail once instead of repeating every guard. A
single later guard stays local; UTF decoding invalidates a prior reservation
because it consumes a variable number of bits. Compile-time size multiplication, minimum-size sums
and constant offset folding use checked arithmetic.

Direct aligned 8/16/32/64-bit loads remain in generated code. Common full-source
validation is inline. This change does not remove the temporary buffer or per-bit
copy loop for shifted binary copies; overlap handling remains in place.

## Verification

The new native regression first failed with `expected_badarg` on a noninteger
input. Before the correction, separate probes also showed 80-bit decoding of
`<<1:80>>` returning 0 instead of 1, and a 12-bit little-endian construction/read
returning 11 instead of 2748. The new fixture compares complete encodings against
an independent bit-by-bit oracle, checks widths through 1,025 bits, offsets 0..7,
all byte orders, unsigned/signed decoding, zero widths, caught/uncaught errors,
invalid dynamic sizes, scaled-size overflow, enormous constant patterns and
insufficient room for a fixed field after a dynamic field. It also passes on
OTP 29. Rust decoding tests independently check every width 0..160, offset 0..7,
byte order and signedness (7,728 combinations), alongside no-mutation/no-allocation
checks for invalid logical ranges.

Commands (pinned Rust 1.99.0, unchanged Cargo.lock, macOS ARM64):

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test -p nera-runtime --release --locked
/tmp/nera-transfer-target/release/nera test 'binary*' 'bitstring*' 'try*' 'catch*' 'match*' math_sqrt -j 1 --timeout 30 --no-warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-apple-darwin
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-pc-windows-msvc
erlc -o /tmp/nera-integer-segment-evidence tests/binary_integer_contracts.erl
erl +S 1 -noshell -pa /tmp/nera-integer-segment-evidence -eval 'ok=binary_integer_contracts:test(),halt().'
```

Final checks pass: **1,994 workspace tests**, **648 runtime release tests**, **63 native tests**, locked release build and workspace/all-targets Clippy with warnings denied. Each Rust suite retains one existing ignored test. Intel macOS and Windows MSVC runtime cross checks pass; OTP 29 passes the native fixture. An initial sandboxed
workspace run failed local socket tests on OS permissions; the subsequent full
run uses local socket access and serial Rust tests. Two OTP warnings concern
intentionally unused binary constructions that exercise exception handling.
Windows cross checking retains six existing fallback warnings.

The full native suite, actual Linux/Windows execution, installed archives,
upstream Cowlib/PropEr runs and contention are not covered. Cross checking is
compilation evidence, not execution. CI/Release remain paused per CONTRIBUTING.md;
this is not merge approval or independent review.

## Performance method

The before compiler and matching runtime archive were preserved from main #104
before edits. Probes assert complete binary output or decoded value and suffix
on every iteration. Expected encodings are independent byte literals generated
by the accompanying script, including bits at the end; the old integer writer
cannot reliably construct a wide expected value in one segment.

Offsets 0 and 3 have separate executables. Most cases use fixed prefix sizes to
exercise the existing direct aligned loads. `dynamicprefix32` passes the prefix
size as a runtime argument; `dynamic32` passes the integer segment size as a
runtime argument. Correctness covers every offset 0..7, while timing samples one
shifted offset. Common cases have correct before and after output. Partial
little-endian and widths above 64 have after-only cases: there is no correct old
baseline for their full write/read workloads.

Each case first warms up with the same iteration count as its timer: 500,000 for
common small cases, 50,000 for 256-byte binary cases and 100,000 for after-only
cases. Counts differ to keep the wide/copy loops bounded in duration; compare
only matching cases/counts, or normalize by operations. Three series of five
alternating/rotating groups give 15 observations per case/executable, with one
scheduler (+S 1). Startup, printing and compilation are outside the timers.

No builds/tests from this task intentionally overlap final timing. The host
remains active with other workspace activity; load averages and samples are
retained. Short microbenchmarks, scheduling/thermal variation and executable
layout limit generalization. Timings include construction/matching and complete
result comparisons, not just isolated memory accesses. Earlier measurements are diagnostic and superseded, with different compiler
variants and sometimes different loop counts. They guided the choice to reserve
multiple fixed fields together while keeping a single later guard local; they
must not be compared as final-source results or used to attribute isolated gains
to one optimization.


## Final performance results

Medians are microseconds for the indicated number of operations. Aligned and
shifted before/after deltas each use the correct legacy implementation at the
same offset. The last column compares final shifted against final aligned,
which is a separate denominator and includes the existing cost of shifted copy
loops. No incorrect legacy decoder is used as a speed baseline.

| Case | Ops | Before aligned µs | After aligned µs | Aligned change | Before shifted µs | After shifted µs | Shifted change | Shifted vs final aligned |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| i8_write | 500,000 | 18,813 | 18,794 | -0.10% | 20,775 | 21,341 | +2.72% | +13.55% |
| i8_read | 500,000 | 8,670 | 8,955 | +3.29% | 11,543 | 12,472 | +8.05% | +39.27% |
| i32_write | 500,000 | 20,317 | 20,162 | -0.76% | 22,796 | 23,740 | +4.14% | +17.75% |
| i32_read | 500,000 | 8,824 | 9,283 | +5.20% | 12,393 | 13,343 | +7.67% | +43.74% |
| i64_write | 500,000 | 22,031 | 21,950 | -0.37% | 25,660 | 26,297 | +2.48% | +19.80% |
| i64_read | 500,000 | 8,944 | 9,277 | +3.72% | 14,052 | 14,876 | +5.86% | +60.35% |
| i64le_write | 500,000 | 21,631 | 21,717 | +0.40% | 26,162 | 27,052 | +3.40% | +24.57% |
| i64le_read | 500,000 | 8,882 | 9,265 | +4.31% | 16,438 | 17,376 | +5.71% | +87.54% |
| i13_write | 500,000 | 19,886 | 20,090 | +1.03% | 21,007 | 21,530 | +2.49% | +7.17% |
| i13_read | 500,000 | 11,550 | 12,641 | +9.45% | 11,558 | 12,806 | +10.80% | +1.31% |
| dynamic32_write | 500,000 | 21,728 | 21,985 | +1.18% | 24,656 | 25,982 | +5.38% | +18.18% |
| dynamic32_read | 500,000 | 10,241 | 12,081 | +17.97% | 13,866 | 16,075 | +15.93% | +33.06% |
| dynamicprefix32_write | 500,000 | 21,718 | 21,666 | -0.24% | 24,264 | 25,379 | +4.60% | +17.14% |
| dynamicprefix32_read | 500,000 | 10,653 | 11,981 | +12.47% | 14,150 | 15,956 | +12.76% | +33.18% |
| binary16_write | 500,000 | 22,129 | 24,118 | +8.99% | 178,358 | 179,170 | +0.46% | +642.89% |
| binary16_read | 500,000 | 21,729 | 22,232 | +2.31% | 52,550 | 52,851 | +0.57% | +137.72% |
| binary256_write | 50,000 | 10,406 | 10,189 | -2.09% | 260,634 | 260,948 | +0.12% | +2461.08% |
| binary256_read | 50,000 | 25,781 | 25,820 | +0.15% | 57,099 | 57,220 | +0.21% | +121.61% |

After-only cases, **100,000 operations** each (no correct legacy baseline):

| Case | Aligned µs | Shifted µs | Shifted vs aligned |
|---|---:|---:|---:|
| i13le_write | 4,078 | 4,393 | +7.72% |
| i13le_read | 2,544 | 2,551 | +0.28% |
| i80_write | 29,517 | 30,076 | +1.89% |
| i80_read | 17,645 | 17,508 | -0.78% |
| i128le_write | 42,233 | 43,108 | +2.07% |
| i128le_read | 16,776 | 17,520 | +4.43% |

Final host load averages (1/5/15 minutes): start [5.97265625, 5.2900390625, 5.3134765625]; end [3.701171875, 4.71875, 5.0888671875].

To reproduce, copy the four probe directories and measure.py into a temporary
root. Compile the common probes with each matching compiler/runtime pair into
before-0, after-0, before-3 and after-3; compile extra probes with the final pair
into after-extra-0 and after-extra-3. Update measure.py's root if needed, then
run it. Keep each compiler's libnera_runtime.a beside it. Example:

```sh
cd /tmp/nera-integer-segment-evidence/common-0
/path/to/nera build probe.erl -e probe:main -o /tmp/nera-integer-segment-evidence/after-0 --no-warnings
python3 /tmp/nera-integer-segment-evidence/measure.py
```

The internal integer/binary write return ABI changed; use a compiler and runtime
from the same revision. No installed-archive compatibility claim is made.


The retained measurements are not completely performance neutral. Common aligned
integer writes range from **-0.76% to +1.18%**; shifted writes range from **+2.48%
to +5.38%** against the valid legacy implementation at the same offset. Fixed-size
integer reads retain **+3.29% to +9.45% aligned** (about **0.57–2.18 ns/op**) and
**+5.71% to +10.80% shifted** (about **1.65–2.50 ns/op**).

Dynamic-width reads retain **+17.97% aligned / +15.93% shifted** (about
**3.68 / 4.42 ns/op**). Dynamic-prefix reads retain **+12.47% / +12.76%**
(**2.66 / 3.61 ns/op**). The 16-byte aligned binary write retains **+8.99%**
(**3.98 ns/op**); the 256-byte aligned write changes **-2.09%**. Shifted binary
writes change **+0.12% to +0.46%**, with the existing buffer/per-bit loop still
dominating their cost. Binary reads change **+0.15% to +2.31%** in these probes.
The remaining positive read costs recur across the three series and must not be
dismissed as host noise. Small deltas and isolated apparent gains do not establish
universal improvements; these are whole-loop workloads on an active machine.

Per-series aligned changes, retained to distinguish recurring costs from a
single aggregate sample:

| Case | Series 1 | Series 2 | Series 3 |
|---|---:|---:|---:|
| i8_read | +3.68% | +3.97% | +0.94% |
| i32_read | +4.12% | +5.53% | +4.43% |
| i64_read | +4.94% | +3.90% | +3.48% |
| i64le_read | +4.11% | +4.83% | +4.21% |
| i13_read | +8.72% | +8.38% | +11.10% |
| dynamic32_read | +18.69% | +17.95% | +17.97% |
| dynamicprefix32_read | +12.70% | +12.42% | +12.61% |
| binary16_write | +9.28% | +7.07% | +9.48% |


## Publication integration — 2026-10-16

Main advanced to `99a16f0` (PR #105) during PR creation. Rebase
conflicted only in CHANGELOG.md; both milestone entries were preserved. The
integer/binary implementation is unchanged. PR #105 changes timer/scheduler
liveness, so validation was rerun on the integrated code commit `9ee81ce`.
The following commit adds only this validation evidence.

- Workspace: **1997 passed**, zero failed, one existing ignored.
- Runtime release: **651 passed**, zero failed, one existing ignored.
- Expanded native binary/bitstring/try/catch/match/math/timer/send-after/receive
  selection: **73 passed**, zero failed/skipped.
- Locked release build and workspace/all-targets Clippy with warnings denied pass.
- Intel macOS and Windows MSVC runtime cross-checks pass; the same six existing
  Windows fallback warnings remain. Actual target execution is not covered.

The performance tables above deliberately retain the original measurements at
main #104 (`902702e`), isolating this change before the #105 integration. They
are not timings of the rebased scheduler/runtime; performance was not rerun on
that integration. Original hashes/logs remain unchanged. See publication-integration/
for fresh logs, source hashes and the tested code/tree IDs. Missing full native,
platform execution, archives, upstream properties and independent review remain
as stated above; CI stays paused.
