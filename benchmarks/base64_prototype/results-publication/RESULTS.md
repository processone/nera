# Base64 prototype results — 2026-10-13

Reference source: `45a1d1ff6596bdbdb04c7947252ef5821b23397e`; macOS-15.7.7-arm64-arm-64bit-Mach-O; rustc 1.99.0 (b940084d7 2026-09-28).
The source snapshot includes working-tree changes; source-manifest.json is authoritative, not HEAD alone.
Release builds with LTO, one scheduler worker. Seven alternating-order samples; medians per call.
Rust samples use 32 times the iteration count to exercise sustained allocation and avoid short timing blocks.
The reference is the current Erlang stdlib compiled to native code by Nera, not BEAM.

## Throughput

| Operation | Raw bytes | Options | Erlang µs | Rust µs | Speedup |
|---|---:|---|---:|---:|---:|
| decode | 12 | standard, padding=true | 2.225 | 0.251 | 8.9× |
| decode | 12 | urlsafe, padding=false | 2.160 | 0.230 | 9.4× |
| decode | 1,026 | standard, padding=true | 186.174 | 5.716 | 32.6× |
| decode | 1,026 | urlsafe, padding=false | 181.583 | 4.938 | 36.8× |
| decode | 16,386 | standard, padding=true | 3848.968 | 89.275 | 43.1× |
| decode | 16,386 | urlsafe, padding=false | 3759.444 | 82.612 | 45.5× |
| decode | 65,538 | standard, padding=true | 18637.533 | 356.877 | 52.2× |
| decode | 65,538 | urlsafe, padding=false | 18337.333 | 338.017 | 54.2× |
| encode | 12 | standard, padding=true | 0.917 | 0.194 | 4.7× |
| encode | 12 | urlsafe, padding=false | 0.869 | 0.183 | 4.8× |
| encode | 1,026 | standard, padding=true | 62.075 | 1.022 | 60.8× |
| encode | 1,026 | urlsafe, padding=false | 58.150 | 1.001 | 58.1× |
| encode | 16,386 | standard, padding=true | 1148.492 | 12.963 | 88.6× |
| encode | 16,386 | urlsafe, padding=false | 1084.175 | 12.861 | 84.3× |
| encode | 65,538 | standard, padding=true | 5163.200 | 56.560 | 91.3× |
| encode | 65,538 | urlsafe, padding=false | 4900.600 | 55.438 | 88.4× |

## Allocation traffic at 65,538 raw bytes, standard alphabet with padding

| Operation | Metric per call | Erlang | Rust |
|---|---|---:|---:|
| decode | Global allocation/reallocation requests | 203.8 | 6.0 |
| decode | Global requested bytes | 30,409,890.5 | 65,664.0 |
| decode | Process-heap reservation calls | 219,349.7 | 4.0 |
| decode | Process-heap reserved bytes | 5,290,387.7 | 106.3 |
| encode | Global allocation/reallocation requests | 92.5 | 6.0 |
| encode | Global requested bytes | 5,979,956.7 | 87,507.0 |
| encode | Process-heap reservation calls | 43,880.3 | 4.0 |
| encode | Process-heap reserved bytes | 1,057,267.7 | 105.3 |

These counters measure cumulative traffic, not simultaneously live memory. Global and process-heap bytes overlap; do not add them. See the prototype README methodology for counter coverage and GC/inline-region details.

## Correctness and checks

- Differential suite: no value or exception-class/reason differences across more than 78,000 comparisons, in both builds.
- Covers all prefixes of bytes 0–255, options, lists, wrong types, representative exhaustive short encodings, every byte in every quartet position, whitespace, noncanonical tails, subbinaries, and repeated large operations with GC pressure.
- Existing `tests/base64_options.erl` adapted to the wrapper passes, including dynamic apply and remote fun calls.
- Three focused Rust tests pass, including 16,388 length/option round trips. The mutation checks from the initial experiment are not rerun here.
- Runtime Clippy passes with warnings denied. This rerun does not repeat the initial scoped rustfmt check.
- Every benchmark row validates a consumed-output size checksum; full byte equality is checked separately by the differential suite.

## Interpretation and limits

This prototype removes intermediate Erlang lists/binaries and most runtime allocation work. The very large speedups are relative to this particular Nera implementation and compiler; they are not a general Rust-versus-Erlang result. The counters do not by themselves attribute all cost to individual logical objects or GC phases; see the separate match-context allocation investigation.
Valid binary inputs take the native path. Lists and invalid inputs preserve the Erlang reference through fallback. Stack frames are different. Large binary calls are not preemptible; scheduling, very-large-input/resource limits and concurrent latency still require work before production integration.
No production compiler/runtime files were changed. No commit, push or merge was performed. Full workspace, cross-platform and release gates were not run for this isolated prototype.

Inputs, patched runtime sources, executables and evidence are fingerprinted in `artifacts.json`; the original source snapshot is in `source-manifest.json`. Raw timing/allocation rows are in `rows.json`.
