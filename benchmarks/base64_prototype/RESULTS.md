# Base64 prototype results — 2026-10-13

Reference source: `9e2ddcebaf725e6ea649ffcaafa98ccc682986cc`; macOS-15.7.7-arm64-arm-64bit-Mach-O; rustc 1.99.0 (b940084d7 2026-09-28).
Release builds with LTO, one scheduler worker. Seven alternating-order samples; medians per call.
Rust samples use 32 times the iteration count to exercise sustained allocation and avoid short timing blocks.
The reference is the current Erlang stdlib compiled to native code by Nera, not BEAM.

## Throughput

| Operation | Raw bytes | Options | Erlang µs | Rust µs | Speedup |
|---|---:|---|---:|---:|---:|
| decode | 12 | standard, padding=true | 2.278 | 0.259 | 8.8× |
| decode | 12 | urlsafe, padding=false | 2.217 | 0.244 | 9.1× |
| decode | 1,026 | standard, padding=true | 191.495 | 5.756 | 33.3× |
| decode | 1,026 | urlsafe, padding=false | 187.936 | 4.983 | 37.7× |
| decode | 16,386 | standard, padding=true | 3936.651 | 89.337 | 44.1× |
| decode | 16,386 | urlsafe, padding=false | 3835.571 | 85.441 | 44.9× |
| decode | 65,538 | standard, padding=true | 19081.067 | 355.804 | 53.6× |
| decode | 65,538 | urlsafe, padding=false | 18730.933 | 337.304 | 55.5× |
| encode | 12 | standard, padding=true | 1.156 | 0.195 | 5.9× |
| encode | 12 | urlsafe, padding=false | 1.124 | 0.188 | 6.0× |
| encode | 1,026 | standard, padding=true | 162.068 | 1.026 | 158.0× |
| encode | 1,026 | urlsafe, padding=false | 156.867 | 1.016 | 154.4× |
| encode | 16,386 | standard, padding=true | 4113.492 | 14.458 | 284.5× |
| encode | 16,386 | urlsafe, padding=false | 3573.952 | 13.362 | 267.5× |
| encode | 65,538 | standard, padding=true | 15827.067 | 56.667 | 279.3× |
| encode | 65,538 | urlsafe, padding=false | 15556.933 | 56.627 | 274.7× |

## Allocation traffic at 65,538 raw bytes, standard alphabet with padding

| Operation | Metric per call | Erlang | Rust |
|---|---|---:|---:|
| decode | Global allocation/reallocation requests | 203.8 | 6.0 |
| decode | Global requested bytes | 30,409,890.5 | 65,664.0 |
| decode | Process-heap reservation calls | 219,349.7 | 4.0 |
| decode | Process-heap reserved bytes | 5,290,387.7 | 106.3 |
| encode | Global allocation/reallocation requests | 172.8 | 6.0 |
| encode | Global requested bytes | 264,158,669.0 | 87,507.0 |
| encode | Process-heap reservation calls | 65,541.0 | 4.0 |
| encode | Process-heap reserved bytes | 90,128,682.1 | 97.7 |

These counters measure cumulative traffic, not simultaneously live memory. Global and process-heap bytes overlap; do not add them. See [methodology](README.md) for counter coverage and GC/inline-region details.

## Correctness and checks

- Differential suite: no value or exception-class/reason differences across more than 78,000 comparisons, in both builds.
- Covers all prefixes of bytes 0–255, options, lists, wrong types, representative exhaustive short encodings, every byte in every quartet position, whitespace, noncanonical tails, subbinaries, and repeated large operations with GC pressure.
- Existing `tests/base64_options.erl` adapted to the wrapper passes, including dynamic apply and remote fun calls.
- Three focused Rust tests pass, including 16,388 length/option round trips. A deliberately corrupted encoder is rejected by two of those tests; the initial empty-codec stub also failed two tests.
- Runtime Clippy passes with warnings denied. The three new Rust files pass the scoped rustfmt check.
- Every benchmark row validates a consumed-output size checksum; full byte equality is checked separately by the differential suite.

## Interpretation and limits

This prototype removes intermediate Erlang lists/binaries and most runtime allocation work. The very large speedups are relative to this particular Nera implementation and compiler; they are not a general Rust-versus-Erlang result. The allocation counters reveal especially large process-heap reservations in the reference encoder, but do not by themselves attribute all cost to individual logical objects or GC phases.
Valid binary inputs take the native path. Lists and invalid inputs preserve the Erlang reference through fallback. Stack frames are different. Large binary calls are not preemptible; scheduling, very-large-input/resource limits and concurrent latency still require work before production integration.
No production compiler/runtime files were changed. No commit, push or merge was performed. Full workspace, cross-platform and release gates were not run for this isolated prototype.

Inputs, patched runtime sources, executables and evidence are fingerprinted in `results/artifacts.json`; the original source snapshot is in `results/source-manifest.json`. Raw timing/allocation rows are in `results/rows.json`.
