# Validated iodata and checked process options

Date: 2026-10-15. Base main after PR #92:
`3a671593017e485e1fbc17ad47291531a4f3be33`. Target: local worktree
`mr/iodata-process-options`, `/tmp/nera-transferable-heap-fragments`.
Rust 1.99.0, unchanged Cargo.lock, Apple M1 / macOS ARM64.
Source fingerprints bind final validation to the code and tests.

## Changes and contracts

File writes and TCP sends share iodata_to_bytes. A binary root retains its
existing direct copy into an owned Vec for asynchronous I/O. List roots accept
nested byte lists, byte-aligned binaries, empty lists, and binary tails. Bytes
must fit 0..=255. Standalone byte roots, invalid elements, integer/nonbinary
improper tails and partial-byte bitstrings are rejected before submitting I/O.
No partial converted prefix is sent or written. File I/O raises its existing
badarg exception; TCP returns its existing {error,badarg} error convention.

Previously, file writes narrowed integers unchecked, rejected valid nested
lists, and accepted byte overflow. TCP narrowed integers unchecked and silently
ignored some invalid terms/tails. Both duplicate converters are removed.
The existing flatten_iolist helper now handles byte heads in a direct checked
loop, recursing only for nested data. Its other binary-conversion consumers
retain their existing leaf semantics. No additional temporary Vec or per-byte
recursive call is introduced. Nested traversal remains recursive, as before;
this change does not introduce a depth policy or redesign that traversal.

Spawn option parsing returns Result<SpawnOpts, InvalidSpawnOptions>, rather
than a partially filled struct with is_err. Non-list roots and improper tails
are rejected. fullsweep_after uses a shared checked u32 conversion in both
spawn options and process_flag; max/min heap sizes use checked usize conversions.
Invalid options return before PID reservation/process allocation. The parser
returns only owned scalar values and documents the lifetime/GC requirements
for traversing the term graph. Existing accepted options, duplicate-option
handling, links and monitors retain their behavior. This does not change heap
allocation-failure policy or add new spawn options.

## RED/GREEN evidence and final validation

Before implementation, file_iodata_domain and tcp_iodata_domain failed because
invalid data returned ok. spawn_options_domain failed because a non-list option
term produced a PID. process_fullsweep_domain independently failed because
2^32 was accepted and returned the previous flag instead of raising badarg.
See red-native.txt and red-fullsweep.txt. All four pass on the final runtime
(green-native.txt).

- Workspace: **1,966 passed**, zero failed, one existing ignored.
- Runtime release: **625 passed**, zero failed, one existing ignored.
- Native new regressions: **4 passed**; surrounding native set: **21 passed**.
  Two tests overlap, giving **23 distinct native tests**, zero failed/skipped.
- Workspace/all-targets Clippy with -D warnings and locked release build: passed.
- Runtime x86_64 Windows MSVC and Intel macOS checks: passed. Existing Windows
  networking fallback unused-code warnings remain.

Tests cover invalid bytes/terms/tails, nested lists and binary tails, partial
bitstrings, empty data, no file mutation or TCP prefix output on invalid data,
spawn closure/MFA entry points, improper option tails, invalid numeric values,
fullsweep endpoints and preservation of an existing flag after rejection.
Unit tests also cover extreme bytes and checked parsing against live heap terms.
Surrounding tests exercise TCP modes/cleanup, file I/O, iolist size, binary
conversion, spawn links/monitors/options and process flags.

## Native performance

The same probe sources are compiled against the initial and final real release
runtime. Three series of nine alternating before/after pairs yield 27 samples
per version, with warm-up runs for both. One scheduler. Timing excludes binary
compilation, startup, socket/data construction and printing.

I/O payloads are 1,024 bytes. TCP measures 2,000 send/exact-receive cycles per
representation after 16 warm-up cycles and checks every received payload.
Representations are a binary, 1,024 byte-list elements, and a nested list mixing
512 byte elements and a 512-byte binary. File probes measure 200 complete
write_file calls after eight warm-up writes, then verify file contents. File
nested data is not timed against the baseline, which did not accept that input.
These are end-to-end I/O timings, including scheduler/syscall costs, not isolated
conversion latencies.

Spawn probes measure 2,000 spawn-and-acknowledgement cycles after 16 warm-up
cycles: closure with empty options, closure with fullsweep/min/max-heap options,
and exported MFA with those heap options. Child acknowledgement validates body
execution; the timer includes creation, scheduling and message delivery, and
is not an isolated parser microbenchmark or a per-child DOWN/teardown barrier.

Positive changes mean longer elapsed time. Pooled ratios compare medians of all
samples; ranges compare the three individual series' medians.

| Case | Before median µs | After median µs | Pooled change | Series changes |
|---|---:|---:|---:|---:|
| file_binary | 9,715 | 9,683 | -0.33% | -1.21% to +0.73% |
| file_flat | 9,831 | 9,965 | +1.36% | -0.67% to +10.79% |
| tcp_binary | 30,097 | 30,175 | +0.26% | +0.06% to +0.72% |
| tcp_flat | 31,966 | 32,292 | +1.02% | +0.71% to +1.62% |
| tcp_nested | 25,593 | 25,987 | +1.54% | +1.11% to +3.03% |
| closure_empty | 7,285 | 7,164 | -1.66% | -4.95% to +0.16% |
| closure_heap | 6,746 | 6,747 | +0.01% | -3.66% to +1.20% |
| mfa_heap | 6,982 | 6,955 | -0.39% | -2.63% to -0.17% |

The binary TCP path is close (+0.26%). TCP byte lists are +1.02% and nested data
+1.54%; all three series have positive changes, so retain these as possible
validation costs rather than claiming zero overhead. File binary writes are
-0.33%; flat lists are +1.36%, with a large first-series +10.79% versus -0.67%
and +1.60% later. Spawn pooled results are -1.66% to +0.01%, with changing signs
in the closure series; they do not establish a stable throughput gain.

The host was active (1/5/15-minute load about 3.7/4.6/4.3 at start and
2.9/4.3/4.2 at end). No builds/tests were intentionally run during timing.
Raw samples and executable hashes are retained in performance.json. Recheck on
an isolated host before generalizing. No claim is made about Windows runtime
performance or contention: these probes use the macOS reactor and one scheduler.

## Reproduction and remaining coverage

With RUSTC_WRAPPER= and CARGO_TARGET_DIR=/tmp/nera-transfer-target:

```sh
cargo test --workspace --locked --no-fail-fast
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
cargo test -p nera-runtime --release --locked
/tmp/nera-transfer-target/release/nera test file_iodata_domain tcp_iodata_domain spawn_options_domain process_fullsweep_domain -j 1 --timeout 30 --no-warnings
/tmp/nera-transfer-target/release/nera test 'tcp_*' 'spawn_*' 'process_flag_*' file_io iolist_size bif_binary_conv -j 1 --timeout 30 --no-warnings
```

Cross-check nera-runtime with cargo check --locked --target
x86_64-pc-windows-msvc and x86_64-apple-darwin. Compile io_probe.erl and
spawn_probe.erl from /tmp, outside project source discovery, with entry points
io_probe:main and spawn_probe:main. Preserve /tmp/nera-iodata-before|after and
/tmp/nera-spawn-options-before|after, then run measure.py.

The full native suite, Linux/Windows execution and installed-archive checks were
not run. CI/Release remain paused per CONTRIBUTING.md. Independent review and
explicit merge approval remain required before a future merge. This report
records the validated worktree before publication; source and test inputs
remain bound by the recorded fingerprints.
