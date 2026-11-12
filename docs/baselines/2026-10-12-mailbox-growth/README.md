# Mailbox release growth contract — 2026-10-12

Base: `4b612a7` (main at worktree creation).
Branch: `mr/mailbox-capacity-contract`; local, uncommitted validation snapshot.

## Contract

Mailbox growth now uses checked u32 power-of-two rounding, verifies its word
allocation layout, and checks the message-count increment on the resize path.
An impossible capacity panics before replacement allocation or mutation of the
mailbox, consistent with the existing infallible allocation API. The buffer's
repr(C) layout and generated receive offsets are unchanged. The send path with
available capacity has no new check. The existing allocation-failure behavior
is preserved; this is not a recoverable OOM API.

## Regression and validation

- RED: a release regression observed that a request above 2^31 was accepted;
  see red-release.txt. The old rounding wrapped to zero and selected the
  minimum capacity. The fixture uses only eight slots, so the baseline
  reproduction itself does not perform an out-of-bounds copy.
- GREEN: requests 2^31+1 and u32::MAX panic in release, preserving the buffer
  pointer, capacity, head, occupied length, tombstone count, scan position and
  live message order. Both inactive and active scans are tested. Sending a
  message after rejection still works. No giant allocation is attempted.
- Full release runtime: **487 unit tests + 11 doctests passed**, one ignored.
- Workspace/all-targets Clippy with warnings denied passed.
- Baseline release compiler and final release runtime builds passed.
- Nine native Erlang scenarios passed: selective_receive,
  selective_receive_stress, selective_receive_tombstone_stress,
  gc_receive_compact, receive_after, receive_bound_var, nested_receive,
  self_send and message_fragment_paths.
- Diff whitespace check passed.

Execution was on macOS ARM64. Full Rust workspace/full Erlang suites,
32-bit execution, cross-platform execution, Miri, sanitizers and remote CI were
not run. No commit, push or PR was created.

## Performance

The Rust probe is compiled with the same rustc and optimization setting,
linked against each corresponding release runtime rlib. `growth` constructs
20,000 mailboxes and sends 256 immediate messages to each, forcing repeated
resizes. `hot` runs one million send/scan/remove/reset cycles on an already
allocated mailbox. Both verify their final state.

```sh
cargo build --release --locked -p nera-runtime
rustc --edition=2021 -O probe.rs -L dependency=/path/to/target/release/deps --extern nera_runtime=/path/to/target/release/libnera_runtime.rlib -o probe
python3 measure.py /path/to/before/probe /path/to/after/probe > results.json
```

Two final series each use one warmup per executable and nine alternating
before/after pairs. Task builds and tests had finished before these measurements.
Earlier samples collected alongside validation are retained separately as
results-during-validation*.json, not used for the conclusion below.

| Series | Metric | Before µs | After µs | Change |
| --- | --- | ---: | ---: | ---: |
| results.json | growth | 16,498 | 17,411 | +5.53% |
| results.json | hot | 5,425 | 5,430 | +0.09% |
| results.json | process_cpu_us | 23,815 | 24,666 | +3.57% |
| results-repeat.json | growth | 16,556 | 17,444 | +5.36% |
| results-repeat.json | hot | 5,392 | 5,418 | +0.48% |
| results-repeat.json | process_cpu_us | 23,869 | 24,810 | +3.94% |

The repeated-growth microbenchmark shows roughly 5.4% overhead; its complete
process CPU rises roughly 3.6–3.9%. The common path varies by 0.1–0.5% in these
samples and gains no source-level check. This does not establish universal
zero regression. The shared machine was not controlled/idle (observed load
averages 2.80/3.05/3.37); confirm precise differences on an idle machine.
Elapsed timings measure the loops, whereas CPU includes both workloads and
process startup. This is not a full Erlang message-throughput benchmark.
