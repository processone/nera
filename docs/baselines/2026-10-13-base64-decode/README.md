# Incremental native Base64 decoding

Builds on the generic native-work PR #66, merged into main as `f698a19` while
this PR was being validated. Its tree is identical to the validation parent
`e9fbf57`; the decoder branch is rebased onto that main merge before publication.
The PR targets main and contains only the decoder step.

Binary `base64:decode/2` now calls the internal `decode_binary/3` Rust BIF.
Options retain the existing Erlang wrapper. The decoder carries a quartet,
position, padding state and output buffer across resumptions. It scans at most
32 KiB per step and charges one reduction per 16 inspected input bytes, including
whitespace and bytes that lead to an error. Remaining reductions can shorten a
step further. These are logical work estimates, not elapsed-time guarantees.

Malformed binary input never falls back to Erlang or restarts a full scan.
Trailing symbols after a padded group preserve their symbolic `case_clause`
value; padded tails preserve the existing badarith/badarg precedence. Incomplete
input preserves missing_padding/function_clause behavior. Noncanonical trailing
bits, standard/URL alphabets and padding=false remain accepted as before.

Native errors contain no GC-managed terms. The common adapter charges the last
step, releases the input borrow, materializes the error and drops pending state.
Resume must retain the exception reason set in ProcessContext instead of
replacing it with the nil return sentinel. The regression checks this directly.
Input roots survive full GC between decoder steps; completed output adopts the
original allocation. The process owns suspended state and frees it on exit.

## Compatibility and scope

- `red.log`: before implementation, the internal decode BIF test fails with
  `{badmatch,{error,undef}}`. `green.log` passes on the production implementation.
- `focused.log`: arbitrary quartet boundaries, whitespace, padding/error
  precedence, full GC, buffer adoption and reduction charging on resumed errors.
- `base64.log`: common vectors/large cases run unchanged on BEAM and Nera;
  production passes the complete frozen-Erlang value/error corpus. Independently,
  BEAM passes that corpus's binary-decoding comparisons against the same oracle,
  including exhaustive short strings over representative symbol classes and
  every byte value in each quartet position. Compare values and exception
  class/reason; stack-trace shape is not asserted.
- `scheduler.log`: BEAM +S 1:1 and five Nera NERA_SCHEDULERS=1 runs verify observer
  progress during large valid data, whitespace-only input, leading whitespace,
  late malformed quartets and trailing data after padding.

List decoding remains the existing Erlang path. It is covered against the frozen
Nera oracle but is not claimed fully BEAM-compatible for malformed non-byte
lists: for example `[256]` with padding=false already raises function_clause in
Nera and badarg in OTP 29. This PR does not change that existing behavior.

Output capacity is checked and reserved once, then transferred without a final
full copy. Reservation, allocator/OS calls and GC remain synchronous; very large
whitespace inputs may reserve capacity they never use. No hard latency bound is
claimed. Native execution is validated on macOS ARM64 only; cross checks generate
objects (including a decode call), not linked foreign runtime execution.

## Reproduction

Use pinned Rust and Cargo.lock. Build runtime before compiler to embed the
current archive. Actual runs used RUSTC_WRAPPER='' and
CARGO_TARGET_DIR=/home/user/AIProjects/nera/target/binary-match-publication.

```
cargo build --release --locked -p nera-runtime
cargo build --release --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
target/release/nera test -j 1 --timeout 30
NERA_SCHEDULERS=1 python3 scripts/test-base64.py --nera "$PWD/target/release/nera"
python3 scripts/test-base64-scheduler.py --nera "$PWD/target/release/nera"
bash scripts/dist.sh --version base64-decode
NERA_SCHEDULERS=1 python3 scripts/test-dist.py dist/nera-base64-decode-aarch64-apple-darwin.tar.gz
bash scripts/test-cross.sh "$PWD/target/release/nera" x86_64-apple-darwin
bash scripts/test-cross.sh "$PWD/target/release/nera" aarch64-unknown-linux-gnu
python3 benchmarks/native_latency/run.py --nera "$PWD/target/release/nera" --output /tmp/decode-latency.json
```

Latency runs follow all builds and checks, without concurrent validation work.
See `benchmarks/native_latency/README.md` for sampling details. CI remains
intentionally paused; independent review and explicit merge approval are still
required.

## Final validation and latency

Release build, workspace Clippy and all 1,874 Rust tests pass (one existing
ignored measurement). The Erlang suite passes 327 tests with two existing
unavailable-module skips. BEAM/frozen-oracle comparisons, five single-scheduler
progress runs, installed external/embedded runtimes and both cross-object
checks pass. Log hashes and source provenance are in validation.json.

Each row summarizes five runs after one warmup, on one scheduler. Latencies are
ping/pong round trips of competing runnable processes, in microseconds.

| Workload | p95 range (µs) | p99 range (µs) | Maximum (µs) | Samples/run |
|---|---:|---:|---:|---:|
| Idle | 2–2 | 3–3 | 50 | 2000 |
| 2,048 × 49,149-byte encodes | 367–402 | 453–459 | 500 | 402 |
| 16 × 8 MiB encodes | 121–125 | 137–141 | 2472 | 1369 |
| 16 × 8 MiB valid decodes | 136–154 | 151–177 | 1554 | 2053 |
| 16 × 8 MiB spaces | 42–45 | 48–50 | 184 | 2053 |
| 16 × 8 MiB spaces + late error | 40–43 | 46–48 | 169 | 2057 |

Valid-decode total elapsed times are 261.2–265.8 ms for 128 MiB of input plus
probe traffic. Space/error cases take 70.6–72.1 ms. These are workload timings,
not isolated codec throughput benchmarks. The expanded harness and host noise
make the earlier PR's run unsuitable as an isolated before/after comparison.
The observations do not establish a latency SLO or identify the causes of the
largest samples; allocation, GC and OS scheduling remain possible contributors.
