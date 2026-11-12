# Fused tuple send: cold complex-copy helper

Rebased publication validation, 2026-10-14. Base: `main`
`bc8c3b8a229272351791d5341c02b87063cad070` (including URI PR #77).
All measurements and checks below were rerun against this base. The earlier,
unpublished investigation is not used as publication evidence.

## Change and limits

Move the existing temporary tuple root and `deep_copy_term` call into
`copy_complex_tuple`, annotated `#[cold]` and `#[inline(never)]`. The receiver
still owns the complete copied graph, shared children remain shared, and the
source stack root lives through the synchronous copy. The simple-element path,
self-send, return value, and fragment builder are unchanged. No GC, heap,
scheduler, compiler, or allocation-policy changes are included.

The isolation fix had inlined complex-copy setup into the send function even
for immediate tuples. On this arm64 build, extraction restores the send stack
frame from 1,120 to 1,104 bytes and changes register/block placement. It recovers
the measured two-element tuple slowdown. This demonstrates an effect of code
generation; it does not identify a unique instruction or CPU-cache mechanism.

**This is not a uniform performance win:** two-element immediate tuples become
about 1.7–2.2% faster in these paired measurements; four-element immediate tuples
show a small cost of about 0.4–0.5% with one scheduler. Preserve that tradeoff
when reviewing the change. Other processors and workloads may differ.

## Validation

Apple M1, 16 GiB, macOS 15.7.7 arm64, pinned Rust 1.99.0 and locked dependencies.
The 550 relevant tracked source/config files in the isolated build match the
worktree; `validated-sources.json` records their hashes. The source changes are
limited to `ffi/process.rs` and the changelog, plus this evidence directory.

- Release build: passed.
- Clippy workspace/all-targets, `-D warnings`: passed.
- Rust workspace/no-fail-fast: **1,914 passed, 0 failed, 1 ignored**.
- Native suite with one scheduler: **336 passed, 0 failed, 2 skipped / 338**.
  Skips require external OneCached modules.
- Existing ownership and self-send Rust regressions pass. They check independent
  list/binary descriptors, preserved aliases, sender destruction, subsequent
  receiver collection, binary contents, and the send return value.
- `message_tuple_isolation.erl`: complete stdout exactly matches BEAM, with
  one and four Nera schedulers and queued messages inspected after sender exit.
- Standalone compiler-only layout outside the checkout, empty temporary cache:
  the same isolation fixture passes with one and four schedulers using the
  embedded runtime. Its extracted SHA-256 equals the measured corrected archive.
- Cross-object smoke for `x86_64-apple-darwin`: passed. This checks object
  generation/format, not native runtime execution on that architecture.
- Scoped diff/whitespace and source binding: passed.

Linux/Windows and macOS x86_64 native execution were not performed. No workflow
was enabled or triggered: CI/Release remain manually paused. Independent review
and explicit merge approval are still required before merging.

## Benchmark protocol

The freshly rebuilt compiler is identical on both sides. Main and corrected
release runtime archives are built from the same source directory with the same
profile/toolchain; each is placed next to its copy of that compiler. Build
manifests record compiler, archive, Erlang source and executable SHA-256 hashes.
The driver checks the source and executable hashes before measuring. Named
producers are confirmed in IR to use fused `send_direct` tuples of arity 2/3/4.

Each batch sends 20,000 messages to a separate receiver, which verifies count
and checksum. The clock starts before releasing the sender. An independent
process answers RTT probes while the work runs. The final pending probe is
measured and drained, and its stop is acknowledged before the next batch.
There are 100 batches with one scheduler (first 10 excluded), or 30 with four
schedulers (first 5 excluded). Before/after order alternates; case order rotates.
Our own builds and tests finish before measuring. Batch timing excludes printing
but includes the concurrent probe. Peak RSS comes from macOS `/usr/bin/time -l`;
it is not an allocation counter.

The machine is not dedicated or pinned to CPUs/frequencies. Host CPU activity is
recorded per trial. The initial complex and tuple-four confirmation series were
strongly disturbed by external activity; all their results are retained. A
lower-load confirmation was therefore added after that activity subsided.
It still contains outliers; **no trials were discarded**.

## Results on the rebased revision

Changes below are medians of paired batch-time ratios; positive means slower.
Absolute metric medians are computed separately and their ratios need not equal
these paired ratios.

| Message | Initial 11-pair confirmation, 1 scheduler | 7-pair matrix, 1 scheduler | Lower-load 21-pair confirmation, 1 scheduler |
|---|---:|---:|---:|
| Atom control | −0.04% | +0.09% | −0.05% |
| Tuple 2, immediate children | **−2.15%** | **−1.72%** | **−2.17%** |
| Tuple 3, immediate children | — | +0.03% | — |
| Tuple 4, immediate children | — | +0.40% | **+0.52%** |

The lower-load series favors tuple 2 in 20/21 pairs, with one large before-side
outlier (paired change −49.75%) retained. Tuple 4 is slower in 16/21 pairs:
range −5.48 to +6.57%. Its initial matrix had seven positive changes between
+0.03 and +0.57%. The additional intervening eleven-pair tuple-four series had
large control fluctuations and does not negate this small signal.

| Lower-load metric (median of trials) | Tuple 2, main → change | Tuple 4, main → change |
|---|---:|---:|
| Median batch time | 1,291.5 → 1,262 µs | 2,012 → 2,028 µs |
| Sustained throughput | 15.335 → 15.624 M msg/s | 9.842 → 9.773 M msg/s |
| Probe RTT p50 | 137 → 124 µs | 210 → 202 µs |
| Probe RTT p99 | 1,377 → 1,362 µs | 2,111 → 2,162 µs |
| Peak RSS | 6,635,520 → 6,635,520 bytes | 7,241,728 → 7,225,344 bytes |

There are roughly 225 RTT samples per one-scheduler trial: only a few samples
determine p99. These data do not establish a tail-latency bound. Median probe
latency is not the latency of one individual send call.

The lower-load complex probe `{small,[N]}` (11 pairs) gives **−0.34%** median
paired batch time, range −1.22 to +15.86%; changes go in both directions.
Its batch medians are 3,349 → 3,331 µs and RTT p99 3,596 → 3,542 µs. No consistent
extra cost is observed for this narrow complex-message case; it does not cover
all Erlang graph shapes. All checksums pass.

Four-scheduler matrix medians: atom −1.38%, tuple 2 −1.86%, tuple 3 −1.99%,
tuple 4 +1.27%. Their wide ranges (tuple 2: −39.29 to +56.07%) prevent precise
performance or fairness conclusions. Do not advertise these as established gains.

## Evidence and reproduction

Six compressed JSON datasets and `evidence.tar.gz` preserve **370 VM benchmark runs**,
build manifests/logs, validation outputs, IR and disassembly. The lower-load
results are `quiet-confirmation.json.gz` and `quiet-complex.json.gz`; initial results
are `confirmation.json.gz`, `matrix.json.gz`, `complex.json.gz`, and
`tuple4-confirmation.json.gz`. `manifest.json` identifies artifacts and sources. `summary.json` contains readable
medians and every paired percentage; decompress the `.json.gz` files for full
per-batch/per-trial records.

From this directory, using two copies of the same new compiler with their
respective adjacent runtime archives:

```sh
python3 measure.py build --compiler /tmp/nera-tuple-publication/before-bin/nera --output /tmp/nera-tuple-publication/before-native
python3 measure.py build --compiler /tmp/nera-tuple-publication/after-bin/nera --output /tmp/nera-tuple-publication/after-native
python3 measure.py measure --before /tmp/nera-tuple-publication/before-native --after /tmp/nera-tuple-publication/after-native --output /tmp/nera-tuple-publication/confirmation --cases tuple2 atom --schedulers 1 --repeats 11
python3 measure.py measure --before /tmp/nera-tuple-publication/before-native --after /tmp/nera-tuple-publication/after-native --output /tmp/nera-tuple-publication/matrix --repeats 7
python3 measure.py measure --before /tmp/nera-tuple-publication/before-native --after /tmp/nera-tuple-publication/after-native --output /tmp/nera-tuple-publication/quiet-confirmation --cases tuple2 tuple4 atom --schedulers 1 --repeats 21
```

The independent `complex-probe/` driver/source uses the same protocol with a
list child; build both sides with that driver, then measure with
`--cases tuple2 --schedulers 1 --repeats 11`.

Validation commands (sockets permitted for the runtime tests):

```sh
cargo build --release --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
NERA_SCHEDULERS=1 /absolute/path/to/nera test -j 1 --timeout 30
bash scripts/test-cross.sh /absolute/path/to/nera x86_64-apple-darwin
```
