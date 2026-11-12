# Matching context invariants and shifted binary copies

2026-10-16, macOS ARM64, pinned Rust 1.99.0, locked dependencies.
Base and merge base: `663aa0b41c789f1a3b90cedb74d75bfb71434e29` (merged #106).
Target: uncommitted worktree on `mr/match-context-bit-copy`, initially 0 ahead /
0 behind origin/main. No commit, push or PR is part of this implementation.
`source-hashes.json` identifies the validated sources and fixtures.

## Contracts and behavior

`MatchContext` is a private-field `repr(C)` descriptor containing a header,
traced source binary, current bit offset and logical end. Its unsafe constructor
checks `offset <= end <= source_bits` before heap allocation. Its safe range
operations cannot wrap or advance past the end. It is a copied metadata snapshot,
not ownership of the source; unsafe graph access still requires live, unmoved
storage. GC rebinds only the traced source, preserving the logical range. No byte
borrow survives GC, allocation or reentry. The generated-code ABI remains four
64-bit words, with all field offsets and allocation size derived from the type.

Runtime helpers resolve reads/materialization against the logical matching window,
including shortened windows whose backing storage is longer. Generated pattern
bounds checks continue to dominate inline reads and cursor advances. The compiler
only transfers a cursor to an exclusive tail-call argument accepted through binary
patterns in every callee clause. Multiple arguments referencing the same deferred
cursor, including references nested in another expression, now materialize before
forwarding. The ordinary exclusive recursive scan retains inline cursor reuse.
This extra alias analysis runs at compile time.

Cross-heap context copies now materialize the exact bit window rather than copying
whole bytes from a rounded source offset. Source drop and moving collection are
covered. The native regression also exposed a distinct rounding bug in optimized
refcounted message sends: a 519-bit value became 520 bits. `BinarySlice` now retains
an exact bit length in place of its byte length, without another field or allocation.
Direct sends, inline/simple tuple fragments and fused tuple sends preserve that
length. Whole-byte callers keep the existing checked byte-length constructor.

Shifted binary construction uses raw, overlap-aware bitwise memmove without an
intermediate `Vec`. It reads each source chunk before a masked destination-byte
write, preserving adjacent bits. Physical byte addresses and bit remainders select
forward/backward copying without overflowing address arithmetic. The aligned
bulk-copy path remains `ptr::copy`. Arbitrary-window `bs_materialize` still has its
existing packing buffer; this change removes the construction copy's snapshot.

## Regression evidence

- RED: materialization beyond a shortened context end succeeded on the old code,
  although the request remained inside the backing allocation; now fails before
  allocation.
- RED: context copies lost partial/shifted bits; now survive source heap drop and
  destination collection with exact bits and zero-padded output.
- RED: repeated deferred arguments produced an undeclared IR variable before
  passes. Shared/nested argument uses now materialize; an ordinary recursive scan
  remains eligible for cursor reuse.
- RED: optimized messages rounded 519 bits to 520; direct and fragment tests cover
  bare binaries and tuples of arity 2/4, with the native test covering fused sends.
- An independent snapshot oracle checks 208,000 overlapping copies across source
  and destination byte/bit offsets, lengths 0..129 and both copy directions.
- A thread-local jemalloc allocation counter records zero additional allocated
  bytes around 400 valid shifted/overlapping FFI copies after setup (macOS; the
  jemalloc-specific test is excluded on MSVC).
- Native/OTP regression checks offsets 0..7, partial and empty windows, heap/refc
  boundaries, process transfer and repeated cursor arguments with explicit equality
  and completion checks.

## Verification commands

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test -p nera-runtime --release --locked
/tmp/nera-transfer-target/release/nera test 'binary*' 'bitstring*' 'gc*' 'try*' 'catch*' 'match*' math_sqrt '*timer*' 'send*' 'receive*' 'message*' 'spawn*' -j 1 --timeout 30 --no-warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-apple-darwin
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-pc-windows-msvc
erlc -o /tmp/nera-match-copy-evidence tests/binary_match_windows.erl
erl +S 1 -noshell -pa /tmp/nera-match-copy-evidence -eval 'ok=binary_match_windows:test(),halt().'
```

Actual Linux/Windows execution, installed archives, full native suite, upstream
Cowlib and contention are not covered here. Cross compilation is not execution.
Windows retains six existing fallback warnings. CI/Release remain paused per
CONTRIBUTING.md. Independent review and full local merge gates remain required
before a subsequent merge.

Final checks: **2,005 workspace tests**, **658 runtime release tests**, **82 native
regressions**, locked release build, workspace/all-target Clippy with warnings
as errors, both cross checks and OTP 29 pass. Each Rust suite retains one existing
ignored test. The initial native failure is retained as RED evidence and fixed
in the final suite; no expected failure was waived.

## Performance

Pre-change compiler/archive were preserved before editing; final compiler/archive
and probe executables have SHA-256 hashes in the accompanying records. Both
versions compile the same probe from an isolated directory (to avoid project
source discovery) and link their own adjacent runtime archive. To reproduce,
place each compiler next to its matching `libnera_runtime.a`, run
`python3 make_probes.py`, then build `probe-0/probe.erl` and `probe-3/probe.erl`
from their respective directories with:

```sh
/path/to/compiler build probe.erl -e probe:main -o /path/to/before-0 --no-warnings
# Repeat for after-0, before-3 and after-3 with their matching compiler/archive.
# Place all four executables next to measure.py, then:
python3 measure.py
```

Three series of five groups; 15 observations per executable/case. Alternating and
rotated execution order, one scheduler (`+S 1`), equal warm-up before every timed
sample, no Nera builds/tests running during measurement. Every iteration compares
the complete constructed value or checksum; message ping-pong checks sender and
complete payload, waits for normal process termination. No incorrect pre-change
partial-message path is used as a performance baseline.

Median elapsed microseconds (lower is better); copy rows include construction and
complete-result equality, not only the low-level copy:

| Case | Operations | Before aligned | After aligned | Change | Before +3 bits | After +3 bits | Change |
|---|---:|---:|---:|---:|---:|---:|---:|
| Copy 1 byte | 500,000 | 24,385 | 24,400 | +0.06% | 32,840 | 26,897 | -18.10% |
| Copy 13 bits | 500,000 | 33,134 | 21,160 | -36.14% | 35,244 | 23,120 | -34.40% |
| Copy 16 bytes | 500,000 | 24,860 | 25,403 | +2.18% | 175,450 | 44,379 | -74.71% |
| Copy 256 bytes | 50,000 | 9,830 | 9,760 | -0.71% | 251,690 | 49,336 | -80.40% |
| Scan 256 bytes | 10,000 | 46,081 | 46,751 | +1.45% | 76,509 | 77,356 | +1.11% |
| Message 256 bytes | 10,000 | 96,944 | 97,063 | +0.12% | 96,987 | 96,974 | -0.01% |

The 13-bit case is partial even at offset zero, so it uses the new bitwise path.
Each scan traverses 256 bytes with exclusive tail-recursive cursor reuse and
checks sum 32,640. Message rows are the same whole-byte control in both offset
cohorts (10,000 round trips), not shifted-message timing.

Shifted 16/256-byte construction is **3.95x / 5.10x faster**, with no snapshot
allocation. Remaining measured increases: **+2.18%** for aligned 16-byte copying
(about 1.09 ns per construction), **+1.45% / +1.11%** for aligned/shifted scanning,
and **+0.12%** for the first message cohort. Aligned 256-byte copying is -0.71%.
The scan increases appear across all three series (aligned +1.23..2.40%, shifted
+1.09..1.65%); do not dismiss them as proven measurement noise or promise zero
runtime cost. No extra check is introduced per generated cursor advance; the
measurements include code placement, runtime helper and GC effects, which this
experiment does not isolate.

The host was shared rather than isolated; load averages (1/5/15 minutes) were
(2.84, 3.45, 3.31) at start and (2.73, 3.34, 3.28) at end.
These are local comparisons, not a statistical guarantee for other CPUs or
production workloads. There is no contention claim. The raw observations and
per-series medians are retained in `performance.json`.

## Publication integration on main #107

PR #108 was rebased onto `a3c1e841e3948cfe0165ac50e8fab0f1f1c84bfc`
(main #107) before publication completed. The only rebase conflict was the
changelog; both entries are retained. The matching/copy sources and regression
fixture are byte-for-byte unchanged. Main adds the independent reactor pending
registration liveness correction and three Rust tests.

Fresh checks on this integration pass: **2,008 workspace tests**, **661 runtime
release tests**, release build, workspace/all-target Clippy with warnings denied,
Intel macOS and Windows MSVC runtime cross checks (six existing Windows fallback
warnings). Each Rust suite retains its one existing ignored test. The native
command above was extended with `'tcp*'`: **94/94** tests pass, including the
12 TCP scenarios and all 82 matching/copy/GC/message/exception regressions.
OTP evidence remains applicable to the unchanged Erlang fixture.

`publication-integration/` contains fresh logs, a source manifest including the
new main reactor and structured results. The performance table and artifacts
above intentionally describe the original comparison on main #106; timings were
not rerun after integrating the unrelated main reactor change. They must not be
read as fresh measurements of the rebased executable. Full native/installed
archive/platform execution and independent review remain outside this PR-opening
validation; the merge requirements in CONTRIBUTING.md still apply.
