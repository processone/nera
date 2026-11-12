# Accept cancellation scaling

Run explicitly in release mode:

```sh
RUSTC_WRAPPER= cargo test -p nera-runtime --release --locked accept_cancellation_scaling -- --ignored --nocapture
```

The opt-in test calls the actual `IoReactor::cancel_for_pid` against synthetic
pending state, with 128, 2,048, 8,192 and 32,768 queued PIDs across one or 64
listeners. It measures an absent PID, one middle removal and a burst cancelling
all queued PIDs in a deterministic permutation. Setup and correctness assertions
are outside the timed region. Heads stay registered, so this measures queue
scans rather than socket syscalls, scheduler latency or real parallel processes.
The existing real-socket regression covers head promotion separately.

One warmup precedes 31 samples per single-cancellation case and seven per burst.
CSV values are microseconds for the entire operation or burst, with median and
nearest-rank p95 (the maximum of seven for bursts). There are no timing assertions
in normal CI. Assertions still check remaining counts, target removal and intact
registered heads. This is a measurement, not a new RED/GREEN behavioral test.

Two runs passed on local macOS ARM64 / Rust 1.99.0; the second followed completion
of this task's compilations. Other machine load was not controlled. Results vary,
so use them as scaling evidence rather than service latency guarantees.

Second-run medians with one listener:

| Queued PIDs | Middle removal (µs) | Whole cancellation burst (ms) |
| --- | ---: | ---: |
| 128 | 0.166 | 0.011 |
| 2048 | 2.250 | 2.467 |
| 8192 | 8.625 | 38.733 |
| 32768 | 35.166 | 703.281 |

The scan is linear per cancellation, hence quadratic across a full cancellation
burst. The observed hundreds of milliseconds at 32,768 waiters justify a separate
optimization if mass cancellation is a target workload. An index to the listener
alone would not eliminate scans within that listener's queue; the removal
strategy must also change. Production behavior is not modified by this benchmark.

Post-addition checks: runtime tests passed (452 unit tests and six doctests; the
new measurement is explicitly ignored by default), runtime all-target strict
Clippy passed, and both release benchmark runs passed. The earlier complete
workspace/Erlang baseline predates only this test addition and documentation;
it was not repeated. Its source manifest is intentionally retained. The current
reactor hash and benchmark logs are bound in `cancellation-validation.json`.
