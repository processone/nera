# Cooperative native binary work

Base: `origin/main` at `53d3778` (includes merged PRs #64 and #65).

`BinaryWork` is process-owned, `Send`, and contains no Erlang terms or borrowed
pointers. The shared FFI adapter roots the single binary input in the existing
continuation, reacquires a temporary byte slice on every invocation, accounts
for inspected bytes, and adopts completed output without a full-size copy.
Small completed calls do not allocate a boxed continuation. Process destruction
also destroys pending native state.

Each operation specifies a byte cost, maximum step and input quantum. The
adapter bounds work by both remaining reductions and the maximum step, rounding
down to the quantum. Exhausted budgets suspend before scanning bytes. Base64
encoding uses 32 bytes/reduction, a three-byte quantum and a 48 KiB maximum.
This prevents padding in the middle of a quartet when a budget ends early.

The regression in `red.log` proves the previous encoder consumed a whole 48 KiB
binary with one remaining reduction (returned STATUS_OK instead of yielding).
The final focused test processes at most 32 bytes and suspends. Existing tests
cover GC while suspended, cancellation cleanup, buffer adoption, accounting of
empty/small/final chunks, and progress of a competing process on one scheduler.
The direct-resume test harness now renews the budget as the real scheduler does.

This deliberately covers byte-oriented, single-input binary work rather than
arbitrary Rust calls. Implementations must bound their loops and avoid calls
into Erlang or GC while borrowing input. It cannot interrupt allocator/OS calls
or GC. Initial output reservation and list-to-binary conversion remain
synchronous. Reduction costs estimate work and do not promise elapsed time.

## Reproduction

Use the pinned Rust toolchain and lockfile, with `RUSTC_WRAPPER=''` if a local
sccache setup is unavailable. Build the release runtime before the compiler so
its embedded archive is current.

```
cargo build --release --locked -p nera-runtime
cargo build --release --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
target/release/nera test -j 1 --timeout 30
python3 scripts/test-base64-scheduler.py --nera "$PWD/target/release/nera"
NERA_SCHEDULERS=1 python3 scripts/test-base64.py --nera "$PWD/target/release/nera"
bash scripts/dist.sh --version native-work
NERA_SCHEDULERS=1 python3 scripts/test-dist.py dist/nera-native-work-aarch64-apple-darwin.tar.gz
bash scripts/test-cross.sh "$PWD/target/release/nera" x86_64-apple-darwin
bash scripts/test-cross.sh "$PWD/target/release/nera" aarch64-unknown-linux-gnu
python3 benchmarks/native_latency/run.py --nera "$PWD/target/release/nera" --output /tmp/latency.json
```

Actual execution used
`CARGO_TARGET_DIR=/home/user/AIProjects/nera/target/binary-match-publication`
and the compiler from its `release` subdirectory. Latency runs follow validation,
without concurrent builds/tests. The harness README describes methodology and
limitations. This measures runnable-process responsiveness, not a hard bound.

Native validation is macOS ARM64 only. Cross checks generate objects, not linked
foreign runtimes. Linux/Windows native execution is not covered. CI remains
intentionally paused; independent review and explicit merge approval are still
required before merging.

## Observed results

Release build, workspace Clippy, 1,870 Rust tests (one existing ignored test),
325 Erlang tests (two existing unavailable-module skips), BEAM/progress checks,
frozen-Erlang differential checks, both installed runtime paths and both
cross-object checks pass. See logs for the exact commands' output.

Five runs on macOS ARM64, one scheduler, after one discarded warmup per case:

| Workload | p95 range (µs) | p99 range (µs) | Maximum across runs (µs) | Round trips per run |
|---|---:|---:|---:|---:|
| Idle | 1 | 1 | 43 | 2,000 |
| 2,048 × 49,149-byte encodes | 243–251 | 256–298 | 345 | 402 |
| 16 × 8 MiB encodes | 106–108 | 137–149 | 6,164 | 1,369 |

Large-workload total elapsed times are 133.6–137.9 ms; small-workload times are
89.5–90.2 ms. The long-tail maximum is not attributed to a specific cause by
this experiment: allocation, GC and OS descheduling remain possible sources.
These are measurements of the final implementation, not a measured before/after
comparison or a latency SLO. Microsecond clock resolution explains idle zeros.
