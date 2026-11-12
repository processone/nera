# Checked TCP arguments and receive allocation

2026-10-14; base main `8367d36cdb0aebb014ae172d043a998d2b0c75e5` (PR #89).
Validated local worktree: `mr/tcp-argument-validation` in
`/tmp/nera-transferable-heap-fragments`. Rust 1.99.0, unchanged Cargo.lock,
Apple M1, macOS ARM64. Source fingerprints accompany the final results.

## Contract and implementation

Ports must be small integer terms in 0..=65535. Invalid values raise badarg
before binding or connecting; port zero remains valid. The old narrowing cast
wrapped 65536 to zero and treated noninteger terms as zero.

Receive lengths must be nonnegative small integer terms representable as usize
and no greater than isize::MAX. Invalid terms raise badarg without consuming
queued data. Zero retains read-any semantics. recv/2 delegates to the validated
recv/3 implementation: it previously ignored its length altogether. Positive
lengths now consistently request an exact read, including partial reactor reads.
The existing timeout parameter remains unimplemented; this change does not alter it.

On reactor platforms the initial reservation is min(length, 4096), followed by
normal geometric Vec growth as data arrives. A WouldBlock/closed peer cannot
force reservation of the entire requested length. The blocking fallback uses a
bounded Read::take followed by read_to_end, verifies the complete length, and
returns UnexpectedEof for short input. It does not zero/reserve the requested
length up front. This removes speculative length-proportional allocation, not
ordinary allocation failure when actually accumulating large amounts of data.

## Tests

- Workspace: **1,961 passed**, 0 failed, 1 existing ignored.
- Runtime release: **620 passed**, 0 failed, 1 existing ignored.
- Native TCP suite: **11 passed**, 0 failed/skipped.
- Workspace/all-targets Clippy with -D warnings and locked release build: passed.
- Runtime x86_64 Windows MSVC and Intel macOS cross-checks: passed. The Windows
  fallback retains the existing networking unused-code warnings.

The native regression rejects negative, oversized and noninteger ports; invalid
lengths through both arities; verifies queued data survives those errors; tests
zero-length read-any, exact one-MiB receipt, and a maximum-small-integer receive
on a closed peer. Inline EOF reports closed, while the existing reactor EOF path
maps UnexpectedEof to einval; the regression explicitly accepts those two existing
outcomes. EOF normalization is outside this change.

The initial native regression failed at the expected badarg assertion, receiving
a socket instead (`red.txt`). A separate baseline-linked counter-check proves
that recv/2 with -1 consumed the queued byte and returned `{ok, <<42>>}` instead
of badarg (`red_recv.erl`, `red-recv.txt`). Unit tests cover port endpoints, length
conversion, exact-reader boundary/no overread, zero-length reads, short input,
and an enormous request on an empty reader.

## Performance

Actual native release TCP measurements, same benchmark source before/after.
Use recv/3 for both versions: the old recv/2 ignored length, so it cannot provide
an equivalent large exact-read comparison. Payload construction, socket setup,
compilation and startup are excluded. A sender process transmits the full payload
and waits for a one-byte acknowledgement before the next message. The receiver
checks complete byte-for-byte equality. One scheduler, eight warm-up transfers
per size, then the measured counts below. Three series of nine alternating pairs
produce 27 observations per version; lower elapsed time is better.

| Payload | Transfers/run | Before median µs | After median µs | Pooled change | Series changes |
|---|---:|---:|---:|---:|---:|
| 1 byte | 2000 | 63,741 | 64,524 | +1.23% | +0.36% to +1.63% |
| 64 bytes | 2000 | 59,710 | 59,598 | -0.19% | -0.71% to -0.01% |
| 4 KiB | 1000 | 32,401 | 32,463 | +0.19% | -0.41% to +0.41% |
| 1 MiB | 64 | 44,898 | 44,624 | -0.61% | -1.40% to -0.38% |

The 1-byte result is slightly slower in all three series (+0.36% to +1.63%), so
retain it as a possible small-message cost. The 64-byte and 4-KiB results are
close; the 1-MiB measurements show no slowdown in these runs. The differences do
not establish universal zero overhead or a throughput improvement. Host load
averages were about 2.7–3.5; no builds/tests were intentionally run during timing.
Recheck on an isolated machine before generalizing. Windows blocking-path
performance was not measured; the native comparison covers the macOS reactor.
Raw samples, binary hashes and load observations are in performance.json.

## Reproduction and coverage

Set RUSTC_WRAPPER= and CARGO_TARGET_DIR=/tmp/nera-transfer-target, then run:

```sh
cargo test --workspace --locked --no-fail-fast
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
cargo test -p nera-runtime --release --locked
/tmp/nera-transfer-target/release/nera test 'tcp_*' -j 1 --timeout 30 --no-warnings
```

Cross-check with cargo check -p nera-runtime --locked --target
x86_64-pc-windows-msvc and x86_64-apple-darwin. The baseline runtime was built
from git archive of the recorded base into /tmp/nera-tcp-baseline, using a separate
/tmp/nera-tcp-baseline-target. The same compiler executable was copied alongside
that runtime archive to link the baseline probe. Compile recv_probe.erl with
entry recv_probe:main from /tmp (outside project source discovery), preserving
executables /tmp/nera-recv-before and /tmp/nera-recv-after, then run measure.py.

Full native suite, Linux/Windows execution and installed-archive checks were not
run. CI/Release remain paused per CONTRIBUTING.md. Independent review and explicit
merge approval remain required before any merge. This report records the
validated worktree before publication; publication preserves the validated
source and test inputs.
