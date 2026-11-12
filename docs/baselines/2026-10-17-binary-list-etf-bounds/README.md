# Binary list domain and ETF allocation bounds

2026-10-17, macOS ARM64, pinned Rust 1.99.0 and locked dependencies.
Base: main `4eb9f09` (merged #110), rebased before this work.
Target: uncommitted worktree `mr/binary-list-etf-bounds`.

## Contracts

`binary_to_list/1` reads the descriptor once with `binary_data_bits` and rejects
lengths not divisible by eight before exposing bytes or allocating the output
list. Byte-aligned heap/refcounted binaries, empty binaries and backing offsets
retain their existing behavior. `bitstring_to_list/1` remains the API for partial
bitstrings. The existing input-graph liveness/immutability and non-reentrant heap
allocation contracts remain in force; this change adds no copy or traversal.

ETF list, small/large tuple and map decoders check the advertised count before
`Vec::with_capacity`. Every term needs at least one tag byte, lists additionally
need a tail term, and map entries need two terms. The checks use subtraction and
division over the remaining input, avoiding multiplication overflow. They allow
one-byte NIL_EXT elements, empty containers, improper tails, nested values and
existing trailing-input behavior. Recursive decoding still validates each term.

These bounds prevent reservations for counts impossible for the remaining
input. They are not a general allocation budget or recursion-depth limit, and
do not promise zero allocation for every malformed ETF stream: an enclosing
plausible container may already have allocated before a nested error is found.
The successful decoding path still uses its existing capacities and allocations.

## Regression evidence

Before the changes, the binary-domain unit test accepted a one-bit bitstring.
The allocation-counter regression reserved 2,048 bytes for a truncated small
tuple with count 255 before returning an error. Both tests now pass.

The counter test uses bounded counts on the old decoder (at most 64 KiB), then
asserts zero allocated bytes for impossible small/large tuple, list and map
counts. It uses the current thread's jemalloc counter, with setup outside the
measurement, and is excluded on MSVC. Separate tests reject u32::MAX counts and
missing list tails; those extreme cases were not run against the old decoder.

Native regressions cover all seven partial-byte lengths, the heap/refcounted
threshold, byte-aligned slices, invalid argument types and the distinct
bitstring conversion behavior. ETF fixtures cover maximum counts, nested errors,
minimal one-byte elements, empty containers, improper tails and roundtrips.
Both new native fixtures also pass OTP 29 / ERTS 17.0.5.

## Validation

Workspace targets ran with `--no-fail-fast`. The initial sandboxed invocation
exited 101 because 14 runtime socket/reactor tests could not open local ports
(`Operation not permitted`); subprocess diagnostics repeat two failure summaries.
The exact compiled runtime test binary was rerun with local-network permission:
**628 passed, zero failed, one existing ignored measurement**. All other
workspace targets passed in the initial invocation. Combining the successful
runtime rerun with those targets gives **2,021 passing workspace tests**; the
original whole-workspace command is not represented as a zero-exit run.

The runtime release suite passed **672 tests**, with the same existing ignored
test. The targeted native suite passed **105/105**, with zero skipped tests. Release build and workspace/all-target Clippy with warnings denied passed.
Intel macOS and Windows MSVC runtime cross-checks passed; Windows retains six
existing platform-fallback warnings. Cross-checks do not establish execution on
those targets. Raw commands/output are retained below this report.

The full native suite, external integration suites, installed archives, actual
Linux/Windows execution and contention are outside this validation. CI/Release
remain paused under CONTRIBUTING.md. No commit, push or PR is requested in this
implementation; full local merge gates and independent review still apply before
merging.

## Reproduction

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target RUST_TEST_THREADS=1 cargo test -p nera-runtime --release --locked
/tmp/nera-transfer-target/release/nera test 'binary*' 'bitstring*' '*etf*' 'bif*' 'gc*' 'try*' 'catch*' 'match*' 'unicode*' 'closure*' 'fun*' -j 1 --timeout 30 --no-warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-apple-darwin
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-tcp-cross cargo check -p nera-runtime --locked --target x86_64-pc-windows-msvc
erlc -o /tmp/nera-binary-list-etf-evidence tests/binary_to_list_domain.erl tests/etf_container_bounds.erl
erl +S 1 -noshell -pa /tmp/nera-binary-list-etf-evidence -eval 'ok=binary_to_list_domain:test(),ok=etf_container_bounds:test(),halt().'
```

## Performance method

The preserved before compiler/archive hashes match the validated #110 artifacts.
The after compiler links the changed runtime. Build the included `probe.erl`
from an isolated directory with each compiler, retaining its adjacent archive:

```sh
/path/to/compiler build probe.erl -e probe:main -o /evidence/before --no-warnings
# Repeat as /evidence/after, then put measure.py beside both executables:
python3 measure.py
```

Three series of five groups, 15 observations per version/case, alternating order,
equal warm-up, one scheduler. No own build or test runs overlap timing. Each
iteration checks full output equality. The binary workloads include conversion,
list construction and comparison; ETF workloads include decoding, result
construction and comparison. They do not isolate the cost of a single check.
Probe setup occurs once per sample, not once per loop iteration.

## Performance results

The final campaign includes two controls: scalar ETF decoding and whole-byte
`bitstring_to_list/1`, whose valid paths were not changed. Earlier campaigns
without those controls are retained, including the initial heavily loaded run;
they were not discarded or silently pooled. Every campaign has 15 observations
per variant/case. The final probes include controls and therefore have different
executable hashes from the first two campaigns.

Elapsed medians (microseconds; positive changes mean more elapsed time):

| Case | Operations | Before | After | Change |
|---|---:|---:|---:|---:|
| binary0 | 1,000,000 | 11,434 | 11,256 | -1.56% |
| binary1 | 1,000,000 | 38,333 | 37,894 | -1.15% |
| binary16 | 300,000 | 37,183 | 38,326 | +3.07% |
| binary256 | 20,000 | 30,478 | 30,503 | +0.08% |
| binary4096 | 2,000 | 63,381 | 64,008 | +0.99% |
| tuple0 | 2,000,000 | 36,458 | 37,987 | +4.19% |
| tuple16 | 300,000 | 25,656 | 25,305 | -1.37% |
| tuple256 | 30,000 | 29,787 | 29,580 | -0.69% |
| list16 | 300,000 | 53,538 | 54,681 | +2.13% |
| map16 | 100,000 | 52,324 | 52,084 | -0.46% |
| control_scalar | 2,000,000 | 24,866 | 24,243 | -2.51% |
| control_bitstring16 | 300,000 | 40,815 | 40,853 | +0.09% |

Changes across all three campaigns:

| Case | Initial | Repeat | With controls |
|---|---:|---:|---:|
| binary0 | +8.84% | -5.66% | -1.56% |
| binary1 | +1.13% | -4.08% | -1.15% |
| binary16 | +1.47% | -1.09% | +3.07% |
| binary256 | +8.29% | -4.48% | +0.08% |
| binary4096 | -3.72% | +0.71% | +0.99% |
| tuple0 | +9.75% | +3.57% | +4.19% |
| tuple16 | +6.57% | -1.09% | -1.37% |
| tuple256 | +3.19% | +3.28% | -0.69% |
| list16 | +5.95% | +4.03% | +2.13% |
| map16 | +9.49% | +4.16% | -0.46% |

Load averages (1/5/15 minutes):

- Initial: (41.33, 20.46, 12.97) at start; (27.11, 19.01, 12.7) at end.
- Repeat: (13.93, 16.9, 12.35) at start; (16.16, 17.12, 12.59) at end.
- With controls: (12.48, 15.61, 12.4) at start; (9.48, 14.57, 12.14) at end.

The final campaign ranges from -1.56% to +3.07% for binary conversions and
-1.37% to +4.19% for ETF containers. The empty tuple retains a measured +4.19%
cost (+3.57% in the repeat); it is not claimed to have zero overhead. Controls
vary -2.51% and +0.09%. Earlier larger costs and inter-series variation are
visible in the retained raw observations. These measurements do not establish
statistical significance or prove that all positive differences are noise.

ARM64 disassembly confirms one descriptor-header read for binary conversion;
the old byte-rounding conditional increment becomes alignment validation and
an exact division. ETF checks remain constant per container, with no extra
traversal, allocation or copying on valid input. No ABI changed.

The machine was shared and loaded. These end-to-end local medians are not an
intrinsic per-check cost, a throughput guarantee, evidence for another CPU or
results under contention. The residual costs are reported explicitly; the
correctness improvements do not come with a claim of performance neutrality.

Recorded text logs are normalized only for trailing whitespace and blank lines
at EOF; original command output remains in the external evidence directory.
