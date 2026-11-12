# Typed socket modes and encapsulated atomic arrays

Date: 2026-10-15. Base: `9cd08b8a58ff2138eb99f23be8dd5bfd20bca34f`
(main after PR #90). Target: local worktree `mr/socket-modes-atomic-arrays`,
`/tmp/nera-transferable-heap-fragments`. Rust 1.99.0, unchanged Cargo.lock,
Apple M1 / macOS ARM64. Code and tests are bound by source-hashes.json.

## Implementation and invariants

- ActiveMode::{Passive, Once, Active} and PacketMode::{Raw, Line} replace u8
  values throughout socket fields/setters, Erlang option parsing, worker reads,
  blocking fallback and reactor dispatch. Each enum uses repr(u8); default modes
  remain passive/raw. Accepted Erlang options and their handling are unchanged.
- SocketOpts remains 40 bytes in both measured builds (layout-before/after.txt).
  This is a measured property, not an ABI guarantee for this Rust struct. The
  generated Erlang term ABI is unchanged; Rust socket APIs now require enums.
- AtomicArray and CounterArray storage is private. The redundant size fields
  are removed; len() and memory_size() derive metadata from the actual Vec.
  The atomics signedness field is private; resource descriptor flags retain
  their existing immutable meaning. Resource ownership/lifetime is unchanged.
- slot(index) validates a one-based index against the actual storage and returns
  Option<&AtomicI64>. It handles zero and usize::MAX without underflow or panic.
  FFI operations keep their existing error order and atomic memory orderings.
  There are no additional locks, registry lookups, Arc clones or allocations
  on an operation's hot path. Storage is not exposed for external resizing.

## Validation

- Workspace: **1,964 passed**, 0 failed, 1 existing ignored.
- Runtime release: **623 passed**, 0 failed, 1 existing ignored.
- Native tcp_*: **11 passed**; atomic*: **4 passed**; counters_basic: **1 passed**.
  Zero failed/skipped across those 16 relevant native tests.
- Workspace/all-targets Clippy with -D warnings and locked release build: passed.
- Runtime x86_64 Windows MSVC and Intel macOS checks: passed. Existing Windows
  networking fallback unused-code warnings remain.

This is a refactor with preserved behavior, covered by existing tests; no
behavioral RED failure is claimed. New tests cover one-based endpoints, zero,
out-of-range/extreme indices, metadata consistency and unchanged neighboring
cells, plus enum byte size/defaults. Existing tests cover signed/unsigned integer
boundaries, resource type rejection, GC/message copying, active-once/continuous
TCP, packet lines, passive reads, cancellation and socket close/cleanup.
The modes benchmark additionally asserts every received payload and exercises
listener packet inheritance, rearming once and continuous line delivery.

## Generated code

The actual native ARM64 FFI functions were disassembled with llvm-objdump.
The whole function instruction counts (including cold/error blocks) are:

| Function | Before | After |
|---|---:|---:|
| atomics_get | 68 | 61 |
| atomics_add | 66 | 59 |
| counters_get | 56 | 49 |
| counters_add | 63 | 56 |

The redundant bounds-panic path is absent from those final functions. This
supports removal of duplicate validation; instruction count alone does not
establish a throughput improvement. See ffi-before/after-arm64.txt and
ffi-summary.json. No unchecked indexing was introduced.

## Native performance method

All timed binaries use the real release runtime and normal Erlang FFI calls.
Baseline runtime/compiler artifacts were verified against PR #90's recorded
hashes before changes; the relevant TCP source fingerprints also matched. The
same probe sources were compiled against the baseline and final runtime.

For each case, three series take nine alternating before/after pairs, yielding
27 observations per version. Each series also warms both executables. Timing
excludes compilation, startup, output and resource/payload construction; the
atomic timer includes process spawning and completion, identically before/after.
Lower elapsed time is better. Pooled changes compare medians of all observations;
series ranges compare each series' medians. Nonstationary timing distributions
can make a pooled ratio differ materially from the individual series ratios.

TCP modes: one scheduler, 16 warm-up cycles then 2,000 verified transfers each
for raw active-once, line active-once, and continuous line delivery. Line payloads
are checked after the runtime's existing newline stripping. Receives: one
scheduler, eight warm-up transfers, then 2,000 at 1/64 bytes, 1,000 at 4 KiB and
64 at 1 MiB. Exact recv/3, full byte equality and a one-byte acknowledgement
between payloads preserve the previous comparison's method.

| TCP case | Before median µs | After median µs | Pooled change | Series changes |
|---|---:|---:|---:|---:|
| line_active | 69,370 | 66,113 | -4.70% | -8.46% to +2.68% |
| line_once | 65,343 | 64,392 | -1.46% | -5.45% to +1.98% |
| raw_once | 46,471 | 45,494 | -2.10% | -4.84% to +3.40% |
| recv_1 | 63,106 | 61,578 | -2.42% | -5.24% to +4.84% |
| recv_1048576 | 45,616 | 46,408 | +1.74% | -0.13% to +2.48% |
| recv_4096 | 33,215 | 33,104 | -0.33% | -1.68% to +0.65% |
| recv_64 | 61,034 | 60,273 | -1.25% | -1.91% to +0.66% |

Arrays: 320,000 total operations per phase distributed over 1, 2, 4 or 8 Erlang
workers with the same number of schedulers. All workers share cell 1. atomics
phases are read, add, add_get and successful compare_exchange(0,0); counters
phases are read and add. Final values are verified after every phase. These are
end-to-end contention probes, including loop/FFI/scheduler costs, not isolated
atomic instruction latencies.

| Threads | Operation | Before median µs | After median µs | Pooled change | Series changes |
|---:|---|---:|---:|---:|---:|
| 1 | read | 7,652 | 7,947 | +3.86% | -23.50% to +10.61% |
| 1 | add | 9,348 | 9,565 | +2.32% | -15.68% to +5.17% |
| 1 | add_get | 8,585 | 8,801 | +2.52% | -10.36% to +4.68% |
| 1 | cas | 10,433 | 10,792 | +3.44% | -8.88% to +3.44% |
| 1 | counter_read | 3,370 | 3,474 | +3.09% | -6.78% to +3.44% |
| 1 | counter_add | 4,984 | 5,027 | +0.86% | -6.58% to +3.83% |
| 2 | read | 6,714 | 6,700 | -0.21% | -2.13% to +10.24% |
| 2 | add | 10,706 | 10,906 | +1.87% | -2.89% to +2.00% |
| 2 | add_get | 13,094 | 13,165 | +0.54% | -2.88% to +0.82% |
| 2 | cas | 12,481 | 11,777 | -5.64% | -8.35% to +1.35% |
| 2 | counter_read | 2,766 | 2,779 | +0.47% | -4.61% to +2.31% |
| 2 | counter_add | 5,868 | 5,955 | +1.48% | +0.29% to +1.41% |
| 4 | read | 4,684 | 4,619 | -1.39% | -2.12% to +2.44% |
| 4 | add | 13,055 | 13,185 | +1.00% | -6.06% to +6.95% |
| 4 | add_get | 13,938 | 13,888 | -0.36% | -2.97% to +2.53% |
| 4 | cas | 12,526 | 12,676 | +1.20% | +0.61% to +2.70% |
| 4 | counter_read | 2,421 | 2,238 | -7.56% | -7.77% to -1.54% |
| 4 | counter_add | 6,821 | 6,977 | +2.29% | -2.93% to +3.72% |
| 8 | read | 2,575 | 2,543 | -1.24% | -17.14% to +78.32% |
| 8 | add | 18,579 | 18,863 | +1.53% | -0.65% to +23.64% |
| 8 | add_get | 17,837 | 17,459 | -2.12% | -5.15% to +21.56% |
| 8 | cas | 15,890 | 15,733 | -0.99% | -1.55% to +0.06% |
| 8 | counter_read | 1,950 | 1,748 | -10.36% | -13.23% to -6.12% |
| 8 | counter_add | 11,149 | 11,085 | -0.57% | -5.87% to +1.69% |

## Interpretation and limits

Socket-mode pooled times are -1.46% to -4.70%, but series change signs: do not
claim stable gains. Receive results range from -2.42% to +1.74% overall; retain
the possible 1-MiB cost (+1.74%, series -0.13% to +2.48%). At one thread, pooled
array results are +0.86% to +3.86%, while series often change signs, so small
costs cannot be ruled out. At eight threads the pooled atomics add result is
+1.53%; the other pooled array results are negative, including counters reads
at -10.36%. This does not establish a universal gain or zero overhead.

The machine was active, with 1/5/15-minute load averages about 2.8/4.6/5.3 at
start and 3.3/4.3/5.1 at end. No builds/tests were intentionally run during
measurement. Some contention series vary strongly: the first 8-thread atomics
read series is +78.32%, while the next two are -17.14% and -5.86%. All samples
and positive results are retained. Their cause was not isolated; run on an idle
host before attributing such swings to the refactor. Shorter FFI code and the
absence of extra synchronization support the design, not a claim that these
measurements prove no regression under every workload.

Windows blocking-path execution/performance, Linux execution, the full native
suite and installed archives were not tested. CI and Release remain paused per
CONTRIBUTING.md. Independent review and explicit merge approval remain required
before merging. This report records validation of the worktree before
publication; the published source and test inputs match those fingerprints.

## Reproduction

With RUSTC_WRAPPER= and CARGO_TARGET_DIR=/tmp/nera-transfer-target:

```sh
cargo test --workspace --locked --no-fail-fast
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
cargo test -p nera-runtime --release --locked
/tmp/nera-transfer-target/release/nera test 'tcp_*' -j 1 --timeout 30 --no-warnings
/tmp/nera-transfer-target/release/nera test 'atomic*' -j 1 --timeout 30 --no-warnings
/tmp/nera-transfer-target/release/nera test counters_basic -j 1 --timeout 30 --no-warnings
```

Cross-check nera-runtime with cargo check --locked --target
x86_64-pc-windows-msvc and x86_64-apple-darwin. Compile probe sources from /tmp,
outside project discovery, with nera build FILE -e MODULE:ENTRY -o OUTPUT.
Use modes_probe:main, recv_probe:main and atomic_probe:main1/main2/main4/main8.
Preserve binaries as /tmp/nera-modes-before|after, /tmp/nera-modes-recv-before|after
and /tmp/nera-arrays-N-before|after, then run measure.py. The script records
binary hashes, raw samples and load observations in performance.json.
