# Floating segment domains and bit windows

2026-10-15. Base main `6047d3f` (merged PR #103), branch
`mr/float-segment-bit-access`. Validation snapshot recorded before publication.
source-hashes.json binds production sources, toolchain,
lockfile and the native regression to the validation below.

## Changes

FloatWidth admits IEEE binary16, binary32 and binary64. Invalid numeric inputs
and unsupported widths no longer silently encode zero. Dynamic widths are
validated before allocation and unit scaling cannot wrap into a supported
width; large capacities retain the runtime's system_limit classification.
Valid constant widths are resolved at compile time. Valid dynamic small widths
use inline tag/range/product checks; only errors call the FFI width helper.
Construction errors reach lexical try/catch or stop an uncaught process before
subsequent writes/expressions in the construction continuation.

Float reads and writes now use the same borrowed bit windows as UTF segments.
They preserve neighbors, check logical lengths instead of rounded byte capacity,
and honor a match context's logical end even if its backing binary is larger.
The raw pointer/bit-length descriptor accessor centralizes metadata extraction;
existing byte accessors derive their rounded lengths from it. Reads complete
before borrowing the destination process heap. No input byte borrow survives
allocation or a reentrant call. The unsafe adapter documents graph liveness,
immutability, exclusive construction writes and the no-GC scope; the window
methods use ordinary checked slices. No allocation is added to valid 32/64-bit
encoding; successful decoding allocates one boxed float as before, while failed
decoding returns nil without allocating a fabricated 0.0.

The internal float-write adapter returns a tagged bit count or nil on error;
the compiler checks that return directly instead of reloading process status.
For reads, a last-instruction predicate is consumed by the immediately following
branch, avoiding an intermediate Erlang boolean. It is cleared on intervening
instructions and all terminators and reset for each function, so a raw
Cranelift predicate never escapes its defining block.
The compiler tests the nil failure sentinel directly. Float wildcard patterns
also decode/validate rather than accepting NaN/infinity encodings. Input floats
must be finite, but narrowing a finite value may encode infinity, matching OTP
(e.g. binary16 65520.0); matching that encoding fails. Binary16 rounds directly
from f64 with ties to even, avoiding double rounding through f32. Signed zero,
subnormals, overflow and explicit/native endianness are covered. Existing
32/64-bit conversions still use native Rust floating operations.

## Regression evidence and checks

Before the correction, an atom encoded eight zero bytes, a dynamic 24-bit width
encoded three zero bytes, and shifted floats overwrote prefixes/decoded the
wrong number. RED Rust/native logs retain those failures. Tests then passed for
invalid domains, every offset 0..7, neighboring bits, both heap/refc-backed
windows, malformed/truncated/nonfinite inputs, local handlers and uncaught exits.

The native oracle independently computes the exact value of every finite
binary16 bit pattern using integer/power-of-two arithmetic and checks both
decoding and re-encoding. All 65,536 patterns are covered, including rejection
of every nonfinite encoding. The fixture also passes on OTP 29. Rust tests use
an independent bit-by-bit splice oracle, exhaust every finite half roundtrip,
and cover direct-f64 ties, midpoint neighbors, subnormals and signed overflow.

On pinned Rust 1.99.0 / unchanged Cargo.lock, macOS ARM64:

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test -p nera-runtime --release --locked
/tmp/nera-transfer-target/release/nera test 'binary*' 'bitstring*' 'try*' 'catch*' 'match*' math_sqrt -j 1 --timeout 30 --no-warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-apple-darwin
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-pc-windows-msvc
erlc -o /tmp/nera-float-evidence tests/binary_float_bits.erl
erl +S 1 -noshell -pa /tmp/nera-float-evidence -eval 'ok=binary_float_bits:test(),halt().'
```

- **1,991 workspace tests**, **645 runtime release tests**, **62 native tests**
  passed; no failures. Each Rust suite retains one existing ignored test.
- Locked release build and workspace/all-targets Clippy with warnings denied pass.
- Intel macOS and Windows MSVC runtime checks pass; Windows retains six existing
  unused-code fallback warnings. Cross compilation is not actual target execution.
- OTP 29 passes the native fixture. OTP warns that two error-testing binary
  expressions have unused results; these intentionally test exceptions.
- Rust tests run serially with local socket access to avoid existing shared
  socket registry interference and sandbox restrictions.

The full native suite, actual Linux/Windows execution, installed archives,
upstream Cowlib/PropEr properties and contention were not run. CI/Release stay
paused per CONTRIBUTING.md. This evidence is not a merge approval or independent
review.

## Measurement method

The before compiler/runtime were preserved before mutation, with production
sources matching main #103. The final after probes are rebuilt from the verified
source snapshot. They compare complete expected binaries or decoded values and
tails on every operation, not just lengths. Common cases include binary32/64,
big/little endian, small integers, big integers and a dynamic width. Binary16
has separate after-only aligned/shifted cases: the old implementation is wrong
for that format. Similarly there is no valid old shifted performance baseline.

Each case executes 500,000 warm-up operations and 500,000 timed operations, one
scheduler. Three series of nine alternating/rotating groups yield 27 samples per
case/executable. Aligned uses offset 0, shifted uses offset 3; correctness covers
all offsets 0..7, but timings sample one representative shifted offset. Startup,
printing and compilation are outside timers. No builds/tests from this task
intentionally run during timing; the host remains active with other workspace
activity. Load averages, executable hashes, samples and per-series aligned
changes are in performance.json. Short durations, scheduling/thermal variation
and executable layout limit generalization; do not infer universally free
validation or intrinsically faster shifted processing from small/negative deltas.

To reproduce, place the four probe sources in separate temporary directories,
each named probe.erl. Compile aligned with the preserved before and final after
compilers, and the other probes with the after compiler. Keep each compiler's
matching libnera_runtime.a beside it. Example (from each probe directory):

```sh
/path/to/nera build probe.erl -e probe:main -o /tmp/nera-float-evidence/before-aligned --no-warnings
/path/to/nera build probe.erl -e probe:main -o /tmp/nera-float-evidence/after-aligned --no-warnings
/path/to/nera build probe.erl -e probe:main -o /tmp/nera-float-evidence/after-shifted --no-warnings
/path/to/nera build probe.erl -e probe:main -o /tmp/nera-float-evidence/after-half-aligned --no-warnings
/path/to/nera build probe.erl -e probe:main -o /tmp/nera-float-evidence/after-half-shifted --no-warnings
python3 /tmp/nera-float-evidence/measure.py
```

Copy measure.py there. Every process runs with +S 1 and fails on a wrong result.
The JSON hashes bind samples to the actual linked executables. Intermediate
performance JSON files are diagnostic checkpoints, not final-source evidence.
The earlier 100,000-operation checkpoints are not directly comparable to the
final 500,000-operation series; longer warm-ups/timers reduce frequency and
startup sensitivity without changing the per-operation result checks.

## Final results

Times are microseconds per 500,000 operations. Aligned changes compare final
aligned against the correct legacy aligned implementation. Shifted changes
compare final shifted against final aligned (a different denominator). Absolute
ns/op includes the surrounding allocation/matching loop and correctness check.

| Case | Before aligned µs | After aligned µs | Aligned change | ns/op | After shifted µs | Shifted vs aligned |
|---|---:|---:|---:|---:|---:|---:|
| f32_write | 22,038 | 23,049 | +4.59% | +2.02 | 24,695 | +7.14% |
| f32_read | 9,583 | 10,732 | +11.99% | +2.30 | 12,263 | +14.27% |
| f64_write | 18,963 | 19,606 | +3.39% | +1.29 | 22,717 | +15.87% |
| f64_read | 9,235 | 10,415 | +12.78% | +2.36 | 11,901 | +14.27% |
| f32le_write | 18,199 | 18,993 | +4.36% | +1.59 | 21,285 | +12.07% |
| f32le_read | 9,166 | 10,495 | +14.50% | +2.66 | 12,621 | +20.26% |
| f64le_write | 19,108 | 19,915 | +4.22% | +1.61 | 22,746 | +14.22% |
| f64le_read | 9,236 | 10,448 | +13.12% | +2.42 | 13,470 | +28.92% |
| f32int_write | 18,213 | 18,858 | +3.54% | +1.29 | 21,061 | +11.68% |
| f32int_read | 9,243 | 10,565 | +14.30% | +2.64 | 12,327 | +16.68% |
| f64big_write | 33,881 | 34,366 | +1.43% | +0.97 | 37,541 | +9.24% |
| f64big_read | 9,262 | 10,470 | +13.04% | +2.42 | 11,949 | +14.13% |
| f64dynamic_write | 21,511 | 20,475 | -4.82% | -2.07 | 23,427 | +14.42% |
| f64dynamic_read | 10,667 | 11,628 | +9.01% | +1.92 | 12,434 | +6.93% |

Binary16 after-only results (no correct legacy baseline):

| Case | Aligned µs | Shifted µs | Shifted vs aligned |
|---|---:|---:|---:|
| f16_write | 23,226 | 24,558 | +5.73% |
| f16_read | 10,836 | 11,149 | +2.89% |
| f16le_write | 19,363 | 21,118 | +9.06% |
| f16le_read | 10,526 | 12,142 | +15.35% |

Final host load averages (1/5/15 min): start [3.00048828125, 5.6357421875, 6.71826171875]; end [2.64501953125, 5.01318359375, 6.40234375].

The retained long series show aligned writes **+1.43% to +4.59%**, except the
valid dynamic-width write case **-4.82%**. The positive write deltas are about
**0.97–2.02 ns/op**. Aligned reads retain **+9.01% to +14.50%**, approximately
**1.92–2.66 ns/op**, positive in all three series. Those recurring read costs
must not be dismissed as host noise or described as zero overhead. The request's
complete performance-neutrality objective is not fully achieved by these probes.

Shifted 32/64-bit writes cost **+7.14% to +15.87%** relative to corrected aligned
writes; shifted reads cost **+6.93% to +28.92%** relative to corrected aligned
reads. Binary16 shifts cost **+5.73% to +9.06%** for writes and **+2.89% to
+15.35%** for reads. These shifted comparisons describe the cost of newly correct
behavior; the old shifted implementation is not a correct baseline.

Earlier diagnostic measurements were noisier (including host load above 13)
and used different intermediate code; some also used shorter 100,000-operation
loops. They are retained for transparency, not for attributing isolated gains
to individual optimizations. Final correctness checks and final performance
hashes are the evidence for the retained implementation. No universal throughput
or full application impact claim is made.
