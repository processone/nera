# Unicode scalars and borrowed bit windows

2026-10-15. Base main `211b1fb` (merged PR #101), branch
`mr/unicode-bit-access`. This report preserves the implementation validation before publication;
`source-hashes.json` binds the exact final production sources and regression.
Publication and any later integration checks are recorded separately below.

## Changes and safety

The UTF writers previously narrowed an i64 before checking its scalar range.
4294967361 consequently encoded as U+0041 in UTF-8, UTF-16 and UTF-32. Validate
the complete integer against the Unicode domain and construct a Rust `char`
before encoding. Negative integers, surrogates, oversized small/big integers
and nonintegers raise badarg without changing destination bytes.

UTF writers/readers also previously rounded bit offsets down to bytes.
`<<0:1,65/utf8>>` produced `<<65,0:1>>` rather than `<<32,1:1>>`; decoding the
latter returned 32 instead of 65. The new private BitReader/BitWriter operate
on borrowed slices and preserve neighboring bits at offsets 0..7. The reader
adapter checks offset/total and subtraction before forming a scoped byte
window containing only complete logical bytes plus a carry byte if shifted.
The writer checks the exact logical bit length, not its rounded allocation.
Truncated sequences fail without publishing a codepoint or reading padding.
Whole-window checks precede writes. Safe slice access remains checked; no
get_unchecked or new unchecked pointer reads were introduced in the windows.

Raw pointers remain in small unsafe adapters with explicit ownership,
exclusivity, storage extent and no-GC contracts. Decoding operates on ordinary
references and returns copied char/count values; its higher-ranked callback
prevents a borrowed slice from escaping. Encoding uses stack buffers and adds
no heap allocation on the valid path. The aligned window path is inlined and
copies/loads bytes directly; shifted carry handling is out of line to reduce
the aligned instruction footprint. UTF construction now branches to lexical
try/catch handlers before using an error result as a bit count.

This change covers UTF segment accesses, including explicit big/little endian
encodings; it is not a migration of every binary helper to the new windows.

## Regression and validation

The focused tests first failed on the old implementation: a large scalar
mutated the destination as U+0041, and a one-bit offset overwrote neighboring
bits. Logs preserve these RED assertions. After the runtime fix, the native
regression exposed an uncaught lexical try error; the IR/codegen change makes
that regression pass as well. The final native fixture also passes on OTP 29.

The independent Rust bit-by-bit oracle checks scalar boundaries, all offsets,
all UTF widths and both explicit endiannesses, complete encodings and every
shorter logical length. Additional tests cover malformed/overlong UTF,
surrogates, invalid metadata, unchanged error destinations/results and a
seven-bit destination whose rounded byte must not admit an eight-bit write.
The native fixture tests exact prefix/suffix preservation, matching,
refc-backed tails, invalid domains and local try handlers.

Rust 1.99.0, unchanged Cargo.lock, macOS 15.7.7 ARM64:

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test -p nera-runtime --release --locked
/tmp/nera-transfer-target/release/nera test 'binary*' 'bitstring*' 'try*' 'catch*' 'match*' -j 1 --timeout 30 --no-warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-apple-darwin
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-pc-windows-msvc
erlc -o /tmp/nera-unicode-evidence tests/binary_unicode_bits.erl
erl +S 1 -noshell -pa /tmp/nera-unicode-evidence -eval 'ok=binary_unicode_bits:test(), halt().'
```

- Workspace: **1,985 passed**, zero failed, one existing ignored.
- Runtime release: **639 passed**, zero failed, one existing ignored.
- Native subset: **60 passed**, zero failed/skipped.
- Release build, Clippy and both runtime cross-compilation checks passed.
  Windows retains six pre-existing fallback warnings. OTP emits the existing
  test style's deprecated-catch warning; the regression passes.
- The first sandboxed workspace run could not create local sockets. The full
  suite was rerun with local socket access and passed; both logs are preserved.
  Rust tests run serially to avoid shared socket-registry interference.

This is a focused local validation, not a full native/platform integration run.
Actual Linux/Windows execution, installed archives, upstream Cowlib/PropEr
properties and contention were not tested. CI/Release remain paused per
CONTRIBUTING.md; no workflow settings were changed.

## Performance method

The preserved before compiler/runtime are the PR #101 candidate. Their relevant
production sources match main `211b1fb`: intervening PR #100 changed integration
harnesses/documentation, not compiler/runtime production code. They were copied
before this implementation; the before probe links the preserved archive.
After probes link the final rebuilt release archive. All executable hashes are
preserved in validation/performance JSON.

Both probes check complete expected binaries or decoded scalars/tails on every
operation. They cover UTF-8 ASCII/four-byte values, UTF-16 BMP/surrogate pairs,
UTF-16 little endian and UTF-32. Each case runs 100,000 warm-up operations and
100,000 timed operations. Before/after aligned use offset 0; the corrected
shifted probe uses offset 3. Correctness covers all offsets 0..7, but timings
sample one representative shifted offset. There is no valid old shifted-speed
comparison: its output is wrong.

Three series of nine alternating/rotating triplets yield 27 observations per
executable/case with one scheduler. No builds/tests intentionally run during
measurement. Startup, compilation and printing are outside the timed loop.
The host is active; load averages, every sample, medians, per-series aligned
changes and binary hashes are retained in performance.json. Short microbenchmarks
and code-layout differences limit generalization; small or negative shifted
changes are not evidence that carry processing is intrinsically faster.

To reproduce, copy each aligned/shifted probe to its own temporary directory as
`probe.erl`. Build aligned with the before and after release compilers and
shifted with the after compiler:

```sh
/path/to/nera build probe.erl -e probe:main -o /tmp/nera-unicode-evidence/before-aligned --no-warnings
/path/to/nera build probe.erl -e probe:main -o /tmp/nera-unicode-evidence/after-aligned --no-warnings
/path/to/nera build probe.erl -e probe:main -o /tmp/nera-unicode-evidence/after-shifted --no-warnings
python3 /tmp/nera-unicode-evidence/measure.py
```

Copy measure.py there, and keep the matching libnera_runtime.a beside each
compiler. The after build/linked archive must correspond to source-hashes.json.
The script runs each executable with `+S 1` and verifies every case is present.

## Final observations

Times below are microseconds per 100,000 operations. Aligned change compares
final aligned versus before aligned. Shifted change compares final shifted
versus final aligned; these are different denominators. ns/op is the aligned
absolute change, including the surrounding construction/matching loop.

| Case | Before aligned µs | Final aligned µs | Aligned change | ns/op | Final shifted µs | Shifted vs aligned |
|---|---:|---:|---:|---:|---:|---:|
| u8_ascii_write | 6,207 | 6,231 | +0.39% | +0.24 | 6,765 | +8.57% |
| u8_ascii_read | 2,792 | 2,659 | -4.76% | -1.33 | 2,716 | +2.14% |
| u8_multi_write | 5,273 | 5,317 | +0.83% | +0.44 | 5,631 | +5.91% |
| u8_multi_read | 2,450 | 2,466 | +0.65% | +0.16 | 2,631 | +6.69% |
| u16_bmp_write | 4,246 | 4,433 | +4.40% | +1.87 | 4,787 | +7.99% |
| u16_bmp_read | 2,029 | 2,163 | +6.60% | +1.34 | 2,150 | -0.60% |
| u16_pair_write | 4,055 | 4,214 | +3.92% | +1.59 | 4,637 | +10.04% |
| u16_pair_read | 1,985 | 2,077 | +4.63% | +0.92 | 2,284 | +9.97% |
| u16_le_write | 3,833 | 4,075 | +6.31% | +2.42 | 4,460 | +9.45% |
| u16_le_read | 1,907 | 1,995 | +4.61% | +0.88 | 2,156 | +8.07% |
| u32_write | 4,477 | 4,441 | -0.80% | -0.36 | 4,096 | -7.77% |
| u32_read | 1,830 | 1,886 | +3.06% | +0.56 | 1,969 | +4.40% |

UTF-8 aligned medians are close to the original or faster in these probes;
UTF-32 write is -0.80%, read +3.06%. **UTF-16 aligned retains +3.92% to +6.60%**,
about **0.88–2.42 ns/op**. Those UTF-16 changes are positive in every series and
must not be dismissed as noise. There is no claim of universally unchanged
performance. Correct shifted handling costs up to 10.04% more than the final
aligned version in these probes; some cases are lower due to layout/noise.

The initial generic implementation measured roughly +4% to +12% aligned costs.
Inlining hot window helpers, specializing fixed-width UTF-16 encoding, narrowing
the reader's borrowed window and isolating carry handling reduced several costs.
Intermediate performance JSON files are diagnostic checkpoints, not evidence for
the final source state; final performance.json is bound to the final executables.

Final load averages (1/5/15 min), start: [5.47705078125, 6.08447265625, 5.31494140625]; end: [4.2626953125, 5.78564453125, 5.22119140625].


## Publication integration

Rebased on main `968da9c` (PR #102). Tested production commit `fdbc7d2`;
subsequent publication changes only add this evidence and documentation. Only
CHANGELOG.md conflicted; both milestone entries were retained. All measured
Unicode source and fixture fingerprints still match source-hashes.json.

After rebase: **1,985 workspace tests**, **639 runtime release tests**, and
**61 native tests** passed, zero failures; the Rust runs retain one existing
ignored test. The native selection above additionally includes `math_sqrt` from
main. Locked release build, workspace/all-targets Clippy and both runtime cross
checks pass; Windows retains six existing warnings. **82 Cowlib harness tests**
pass; this does not rerun upstream native properties. Commands use the same
pinned toolchain/target directories as above. Current logs, exact tested commit,
tree and base are in [publication evidence](publication/validation.json).

Performance tables above remain the pre-rebase observations. They were not
rerun after PR #102 integration, whose changes can affect executable layout.
Their Unicode sources are unchanged, but no current-integration timing claim
is made. Full native/platform execution, archives and independent review remain
outside this publication validation; CI/Release stay paused.
