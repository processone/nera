# Base64 reductions and scheduler fairness

Integration base: `8f027bad46ea4e034b61aa11bfc1836419dd4f52` (`main`, after #63).

## Reproduced failures

Three regressions using the actual `Scheduler::new(1)` failed before the fix:

| Scenario | Before | After |
| --- | --- | --- |
| Local process yields repeatedly while an observer enters the global queue | Observer waits all 128 slices, until the local process exits | Observer runs after 16 slices |
| Repeated 49,149-byte encodings | All 64 calls fit in the first slice | Three calls in the first slice; global observer runs after 48 calls, before completion |
| Native encoding of 3 MiB | Observer never sees an unfinished encoding | Observer runs after 786,432 input bytes, while output is still private |

These assertions count work and check progress order rather than relying on
machine-speed-dependent latency thresholds. The finite workloads terminate on
the broken implementation too. `red.log` records the original assertion
failures; `green.log` records the passing measurements.

The inbox regression first extracted the existing unbounded drain unchanged,
then demonstrated that it drained all 1,024 queued entries before execution
could proceed. The fixed drain takes at most 64 entries per dispatch attempt,
and the test verifies that repeated drains preserve all entries.

## Changes

- The native encoder charges `ceil(input_bytes / 32)` reductions, minimum one,
  including empty inputs and the final resumed chunk. The normal loop budget
  is 4,000 reductions; work below the 48 KiB native-yield threshold now consumes
  that budget. The 32-byte conversion is a work estimate, not a time guarantee.
- The worker attempts a global-queue steal after 16 dispatches even if its local
  FIFO is nonempty. Intervening local dispatches retain locality and prevent a
  busy global injector from taking all dispatches. Under contention a steal may
  retry; 16 is a polling bound, not a per-process wall-clock service guarantee.
- Incoming worker queues drain in bounded batches, allowing execution to proceed
  even when producers continue adding work.
- Base64 uses the existing `ATOM_TRUE` constant instead of taking the atom-table
  read lock on every call.
- `NERA_SCHEDULERS=1..255` configures worker count at runtime, with invalid values
  rejected before entry execution. The default is CPU count, capped at the
  worker-affinity representation's 255-worker limit. Both runtime entry paths
  use the same parser. The prototype harness now uses this setting instead of
  rewriting runtime source to force one worker.

The compiled regression handshakes an observer into a waiting state, wakes it
from the encoder, and requires its response to arrive before encoding completes.
It covers repeated encodings just below and exactly at 48 KiB, plus one 3 MiB
encoding. `scripts/test-base64-scheduler.py` runs this with Nera's single-worker
setting five times and with BEAM `+S 1:1`, and checks invalid configuration.
The general Erlang suite verifies completion/results without assuming ordering
between OS threads on a multi-worker run.

## Scope and remaining boundaries

This establishes cooperative progress for the tested binary-encoding workloads
and removes the reproduced queue starvation. It does **not** establish a hard
latency bound or make the whole runtime nonblocking: the initial output
reservation, allocator/OS operations, garbage collection, and the existing
list-to-binary conversion remain synchronous. Flat-list Base64 inputs still
perform that conversion before entering the native binary encoder. Changing
those generic allocation/conversion mechanisms is not part of this correction.

## Reproduction

```sh
RUSTC_WRAPPER='' cargo test --locked -p nera-runtime single_worker_ -- --nocapture
RUSTC_WRAPPER='' cargo test --locked -p nera-runtime worker_inbox_drain -- --nocapture
RUSTC_WRAPPER='' cargo build --release --locked -p nera-runtime
RUSTC_WRAPPER='' cargo build --release --locked
RUSTC_WRAPPER='' cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER='' cargo test --workspace --locked --no-fail-fast
./target/release/nera test -j 1 --timeout 30
python3 scripts/test-base64-scheduler.py --nera "$PWD/target/release/nera"
NERA_SCHEDULERS=1 python3 scripts/test-base64.py --nera "$PWD/target/release/nera"
RUSTC_WRAPPER='' ./scripts/dist.sh --version base64-fairness
NERA_SCHEDULERS=1 python3 scripts/test-dist.py dist/nera-base64-fairness-aarch64-apple-darwin.tar.gz
bash scripts/test-cross.sh "$PWD/target/release/nera" x86_64-apple-darwin
bash scripts/test-cross.sh "$PWD/target/release/nera" aarch64-unknown-linux-gnu
```

Validation platform: macOS ARM64, Rust 1.99.0, OTP 29. Source, executable and log
hashes are recorded in `validation.json`. Logs distinguish failures that prove
the regression from final passing checks. Native Linux/Windows execution and
cross-linked runtime archives are not covered. GitHub CI remains intentionally
paused; independent review and explicit merge approval remain required.

Final results: release build and workspace Clippy pass; 1,864 Rust tests pass
with one existing ignored cancellation-scaling measurement; 325 Erlang tests
pass with two existing OneCached skips requiring unavailable external modules.
Single-scheduler compiled progress, BEAM/value-error differential comparisons,
installed external/embedded runtime paths and both cross-object checks pass.
The prototype harness also completes timing/allocation and differential runs
using the environment override. Those timings ran alongside validation work
and are not presented as a new performance baseline.
