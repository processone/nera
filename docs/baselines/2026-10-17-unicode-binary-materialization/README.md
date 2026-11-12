# Unicode scalar checks and direct binary materialization

2026-10-17; macOS ARM64, pinned Rust 1.99.0 and locked dependencies.
Base: main `67b2ccf` (merged #109), rebase completed before implementation.
Target: uncommitted worktree `mr/unicode-binary-materialization`.
Source and artifact hashes bind the recorded evidence to the changed sources.

## Changes and contracts

`term_to_string` now uses checked integer narrowing followed by `char::from_u32`.
Negative, surrogate, out-of-range and noninteger elements are rejected before
UTF-8 encoding or atom interning. A large integer such as `0x100000041` no longer
aliases `A`. The existing raw graph access/lifetime contract remains explicit;
valid scalar encodings and proper-list checks are unchanged.

For shifted/partial matching windows, `bs_materialize` fills the final destination
with the existing overlap-tested bytewise bit-copy helper. Small results are
written directly to zero-initialized process-heap storage. Large results fill one
Vec which moves into its final BinaryData/Arc owner; no second byte allocation or
copy follows. BinarySlice validates its exact bit length. Unused bits remain zero
and the header retains the significant bit count. Existing aligned behavior,
including refcounted sharing, is retained. Logical range checks precede allocation.

No Rust source-byte slice spans heap mutation. Source storage stays live, immutable
and unmoved under the FFI contract; arena bump/chunk allocation cannot collect or
reenter. Byte writes finish before large output is shared through Arc. Result
owners remain traced by the destination heap. ABI and small/refcounted threshold
are unchanged.

## Correctness and allocation evidence

RED: the Unicode test accepted `0x100000041` as `A`; the old native fixture failed
with `expected_badarg`. Both now pass. Tests cover scalar boundaries, surrogates,
negative/oversized values, nonintegers and improper lists. The common native
fixture passes on OTP 29 as well.

RED: 100 small 13-bit materializations at offset 3 allocated **800 bytes** outside
the preallocated destination arena. The new test asserts **zero**. RED: a large
8,191-bit materialization allocated **2,096 bytes**; the new allocation budget is
at most **1,280 bytes**, allowing the 1 KiB final buffer and owner metadata but
excluding another 1 KiB temporary. These use current-thread jemalloc allocated
byte counters, with arena/owner-vector setup outside the measurement; they are
excluded on MSVC. No zero-allocation claim is made for final large storage.

Independent bit-by-bit window checks cover all bit offsets, empty/partial outputs,
small/refcounted sources and outputs. New source-drop/moving-GC tests verify exact
logical length, content and zero padding after source ownership is released.
Existing shortened-range rejection, invalid-input no-allocation, bitwise overlap,
closure/process/ETS transfer and Unicode segment regressions remain passing.

### Deliberate malformed-numeric-input difference

The shared string helper also serves numeric-list conversion. OTP 29 rejects
`list_to_atom([4294967361])` but returns `1` for
`list_to_integer([4294967345])`, apparently narrowing the malformed element into a
digit. Nera deliberately rejects both values before narrowing. The separate
`unicode_integer_scalar_domain` fixture checks this stricter behavior and is not
claimed to pass OTP. Ordinary numeric strings are unchanged. Raw OTP observations
are retained and the difference is noted in COMPATIBILITY.md.

## Validation

Passed on the recorded sources: **2,017 workspace tests**, **668 runtime release
tests**, release build, workspace/all-target Clippy with warnings denied,
**117/117 targeted native tests plus the separate strict-numeric regression**,
OTP 29 on the shared Unicode fixture, and Intel macOS/Windows MSVC runtime cross
checks. Each Rust suite retains one existing ignored test; Windows retains six
existing fallback warnings. Compilation is not target-platform execution.

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test -p nera-runtime --release --locked
/tmp/nera-transfer-target/release/nera test 'unicode*' 'binary*' 'bitstring*' 'closure*' 'fun*' 'gc*' 'io*' 'file*' 'try*' 'catch*' 'match*' 'bif*' '*atom*' jaerlang2_convert -j 1 --timeout 30 --no-warnings
/tmp/nera-unicode-materialize-evidence/after-runtime/nera test unicode_integer_scalar_domain -j 1 --timeout 30 --no-warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-apple-darwin
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-pc-windows-msvc
erlc -o /tmp/nera-unicode-materialize-evidence tests/unicode_list_scalar_domain.erl
erl +S 1 -noshell -pa /tmp/nera-unicode-materialize-evidence -eval 'ok=unicode_list_scalar_domain:test(),halt().'
```

The strict-numeric fixture was added after the broad native inventory was
collected and was run separately; combined distinct coverage is 118 tests. The
full native suite, external integration suites, installed archives, actual
Linux/Windows execution and contention were not validated. CI/Release remain
paused per CONTRIBUTING.md. No commit/push/PR is part of this implementation;
full local merge gates and independent review remain required before a merge.

## Performance

Preserved before/after compilers link their respective adjacent runtime archives.
Generate the included sources with `python3 make_probes.py`. From each isolated
probe directory (to avoid discovering project sources), build the same source
with each compiler:

```sh
/path/to/compiler build probe.erl -e probe:main -o /evidence/before-unicode --no-warnings
# Repeat as after-unicode, before-0, after-0, before-3 and after-3.
# Put the executables beside measure.py and run:
python3 measure.py
```

Three series of five groups, 15 observations per variant/case; alternating and
rotated order, equal warm-up, one scheduler (`+S 1`), no concurrent Nera build or
test runs during measurement. Each iteration checks the atom or complete result
bitstring. Unicode rows include list conversion and atom lookup. Materialization
rows include matching, capture/materialization, closure creation/call and equality;
these are not isolated memcpy timings. Inputs contain a nonzero patterned payload.

Unicode/list-to-atom elapsed medians (microseconds, lower is better):

| Case | Operations | Before | After | Change |
|---|---:|---:|---:|---:|
| ASCII, 12 chars | 300,000 | 27,785 | 27,829 | +0.16% |
| ASCII, 128 chars | 100,000 | 42,469 | 43,128 | +1.55% |
| U+00E9, 32 chars | 300,000 | 56,456 | 56,814 | +0.63% |
| U+10FFFF, 128 chars | 100,000 | 65,720 | 66,051 | +0.50% |

Materialization elapsed medians:

| Window | Operations | Before aligned | After aligned | Change | Before +3 bits | After +3 bits | Change |
|---|---:|---:|---:|---:|---:|---:|---:|
| 13 bits | 300,000 | 25,215 | 21,220 | -15.84% | 25,378 | 20,918 | -17.57% |
| 16 bytes | 300,000 | 18,310 | 18,686 | +2.05% | 36,104 | 30,023 | -16.84% |
| 64 bytes | 100,000 | 7,629 | 7,744 | +1.51% | 31,199 | 27,069 | -13.24% |
| 65 bytes | 100,000 | 76,942 | 76,787 | -0.20% | 36,242 | 31,047 | -14.33% |
| 256 bytes | 100,000 | 177,873 | 176,992 | -0.50% | 114,502 | 99,980 | -12.68% |

The 13-bit window is partial even with an aligned start and uses the changed
path; whole-byte aligned rows retain the existing delegation/sharing behavior.
Shifted windows take **12.68–17.57% less elapsed time**. The partial window at
zero offset takes 15.84% less time. Unicode checking retains a measured
**+0.16..1.55%** cost; aligned whole-byte controls vary **-0.50..+2.05%**.
These costs are recorded rather than claimed to be zero. No extra runtime check
was removed to obtain the copy gains. There is no claim that every binary-copy
path elsewhere in the runtime is now allocation-free.

The host was loaded and shared. Load averages (1/5/15 minutes) were
(7.68, 15.45, 13.02) at start and (4.56, 13.26, 12.36) at end.
The small differences on unchanged aligned controls demonstrate why these local
medians cannot establish a statistical guarantee, an intrinsic per-instruction
cost, another CPU's performance or behavior under contention. Raw observations,
per-series medians, build inputs and executable hashes are retained.
