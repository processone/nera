# Typed atomic resources and integer domains

Date: 2026-10-14. Integration base: `b772a5501ed9a190f20f6fbba1dc4ae76a9fbb4b`
(merged PR #85). Validation worktree: `mr/typed-atomics-integers`.
The checks below were completed before PR publication.

## Changes and safety contract

Ordinary references, atomics and counters now have distinct metadata in the
reference header. `ResourceRef<T>` checks the complete permitted header before
reading the native payload, then provides a borrowed `&T` through a private
`NonNull<T>`. Its unsafe construction contract requires a live, initialized
runtime graph and descriptors produced by the matching resource constructor or
by a metadata-preserving copy. This is not validation of arbitrary forged memory.
The unsafe `ReferenceResource` mapping fixes the native pointee associated with
each namespace. A numeric ordinary reference ID equal to a native pointer still
fails resource validation. Cross-type operations report `badarg`.

References retain their existing two-word layout and boxed subtype. Headers are
`0x07` (ordinary), `0x17` (signed atomics), `0x57` (unsigned atomics) and `0x27`
(counters). The unsigned bit is immutable and reused after header validation,
removing the extra native signedness load on the hot path. This changes private
reference metadata in the preview ABI; runtime/compiler artifacts must be
rebuilt together. `is_reference` remains true. GC and message copying already
preserve the whole descriptor; both ETS copy paths now do so too. Equality/order
include the reference namespace; ETS set keys serialize header and ID rather
than the movable descriptor address.

Atomic operations continue to access slots directly: no registry lookup, lock,
Arc clone or heap allocation for small results. Boxed input/output conversions
are outlined into cold functions. Slots retain the previous atomic orderings
(Relaxed/AcqRel/Acquire). This task does not establish OTP's global sequential
ordering guarantee. Existing retained Arc ownership is unchanged; unused arrays
are still retained for the host process lifetime, unlike OTP resource reclamation.

## Integer behavior

Signed storage accepts `[-2^63, 2^63-1]`; unsigned storage accepts
`[0, 2^64-1]`. Results preserve that interpretation and use BigInts whenever the
value is outside the runtime's signed 61-bit small-integer range. Arithmetic wraps
modulo 64 bits. Addition deltas accept `[-2^63, 2^64-1]` for either mode;
subtraction accepts `[-2^64+1, 2^63]`, because OTP validates the negated delta
against the addition domain. Exchange and CAS validate storage values before
mutation. Unknown, malformed or improper option lists are rejected before
allocating a resource.

These domains were checked against installed Erlang/OTP 29. The entire
`atomics_integer_domain:test/0` regression passes on BEAM as well as Nera.
[OTP atomics documentation](https://www.erlang.org/docs/27/apps/erts/atomics.html).
The resource-kind regression deliberately checks Nera's existing counter
reference representation; BEAM counters use a different opaque representation.

## Regressions and validation

Before implementation, the ordinary-reference regression crashed with SIGSEGV;
crossing the small-integer boundary with `add_get` aborted in the FFI adapter.
See `red-resource.txt` and `red-native.txt`. The first attempted resource test
used an unsupported `garbage_collect/0` call and failed compilation; it was
replaced with allocation churn before the valid crash reproduction.

New Rust regressions cover wrong-kind validation (including identical numeric
payload bits), signed/unsigned full-GC roots and message copies after dropping
the source heap, and storage/delta boundary conversions. Native regressions
exercise invalid operations without mutation, full signed/unsigned result
bounds, wrapping arithmetic, exchange/CAS, options, ETS keys and copied resource
identity through GC and messages. Final validation and performance results below.

## Final checks and scope

- Workspace Rust: **1,940 passed, 0 failed, 1 existing ignored**.
- Runtime release: **612 passed, 0 failed, 1 existing ignored** (572 unit,
  3 compilation integrations, 37 doctests). The ignored test is the separate
  reactor cancellation scaling measurement, unrelated to this change.
- Full native suite on the final runtime: **344 passed, 0 failed, 2 skipped**
  out of 346. Skips require external onecached modules. Both new atomics
  regressions and existing atomics/counters tests pass.
- Release build and workspace/all-targets Clippy with `-D warnings`: passed.
- Runtime checks for x86_64 Windows MSVC and Intel macOS: passed. Windows retains
  seven pre-existing unused-code warnings in networking code, unchanged here.
- Erlang/OTP 29 domain regression: passed. Its compiler warns that the regression's
  `catch` expression is deprecated (`beam-compile.txt`); execution is successful.
- Final source fingerprints verified and Git whitespace checks clean.

Rust/socket and native networking tests ran with host socket access. An earlier
restricted run had sandbox EPERM/eacces errors; the final complete runs passed
with that restriction removed. No Windows/Linux execution or installed-archive
validation was performed for this local implementation task. CI and Release
workflows remain paused according to CONTRIBUTING.md. An independent review is still required before merge. The validation snapshot
was captured before committing, pushing or opening this task's PR.

`source-hashes.json` binds production sources, tests, fixtures and the unchanged
lock/toolchain files to the final checks; `validation.json` records counts and
runtime/compiler hashes. `intermediate-native-tests.txt` is an earlier run
started before the final cold-path change, not final-state evidence. Final-state
native results are in `native-tests.txt`.

The integration base is the merged PR #85 commit `b772a55`. Before publication,
`origin/main` was fetched and confirmed to match that base; no rebase or source
changes were necessary. The older tracking-ref value and unstaged status in
`validation.json` describe the earlier validation snapshot. The primary checkout's
unrelated changes were left untouched.

## Reproduction

Use Rust 1.99.0 with the unchanged lockfile. In this worktree:

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo test -p nera-runtime --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo build --release --locked
/tmp/nera-transfer-target/release/nera test -j 1 --timeout 30 --no-warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo check -p nera-runtime --target x86_64-pc-windows-msvc --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target cargo check -p nera-runtime --target x86_64-apple-darwin --locked
```

The native benchmark `atomic_probe.erl` measures 320,000 operations per phase
with 1/2/4/8 spawned workers and corresponding `NERA_SCHEDULERS`. Each phase shares
one slot among workers; timings include spawning, execution and completion
messages, and exclude compilation, startup and printing. Read starts at zero,
add/add_get increment by one, and CAS exchanges zero for zero. It does not
measure negative-small-integer output, boxed conversions, resource creation or
ETS-key throughput. Newly supported full-width values cannot be compared against
the old implementation, which rejects or aborts on those values.

Compile each entry point (`main1`, `main2`, `main4`, `main8`) from a directory
without `nera.toml` using the matching runtime/compiler, preserving binaries as
`/tmp/nera-atomics-before-N` and `/tmp/nera-atomics-after-N`. Then run:

```sh
python3 docs/baselines/2026-10-14-typed-atomics/measure.py /tmp/series.json
```

Each independent series warms both versions and takes nine alternating paired
samples per worker count. Three final series provide 27 samples per cell.
Raw samples, binary hashes and load averages are in `series-*.json`.
`initial-series-*.json`, `outlined-series-*.json` and `cached-flags-series-*.json` are intermediate designs,
retained to show the measured iterations; they are not final validation evidence.
The host was not isolated from other activity. No tests or builds were deliberately
run concurrently with the final measurements. `environment.json` records the
Apple M1/macOS and toolchain environment. Small noisy timing differences should
not be treated as proof of throughput changes.

## Final performance observations

The final implementation caches immutable signedness in the validated header,
then outlines the complete boxed-delta operation. ARM disassembly confirms that
`get` no longer loads the native signedness field, nonnegative small outputs use
a shift and zero test, and `add_get` saves a 32-byte frame (matching the baseline,
down from 48 bytes in the intermediate design). The atomic instructions and
orderings are unchanged. See the preserved disassembly excerpts.

Each cell below pools 27 observations. Lower time is better. Paired delta is the
median of the corresponding after/before ratios, while pooled delta compares
separate timing medians; machine variation can make these aggregates differ.
The single-thread series ranged from roughly -12% to +20% depending on operation,
so apparent improvements are not reliable throughput claims.

| Operation | Workers | Before µs | After µs | Pooled delta | Paired delta | Series median delta range |
|---|---:|---:|---:|---:|---:|---:|
| read | 1 | 4567 | 4555 | -0.26% | -1.29% | -11.16% … +19.73% |
| add | 1 | 8787 | 8593 | -2.21% | -3.77% | -12.21% … +12.44% |
| add_get | 1 | 10281 | 10140 | -1.37% | -2.08% | -10.96% … +14.56% |
| cas | 1 | 12976 | 12864 | -0.86% | -2.89% | -9.01% … +7.42% |
| read | 2 | 3982 | 3921 | -1.53% | -1.44% | -17.23% … +12.86% |
| add | 2 | 9605 | 9454 | -1.57% | -0.75% | -4.59% … +13.31% |
| add_get | 2 | 10635 | 11091 | +4.29% | -0.06% | -6.58% … +21.43% |
| cas | 2 | 12388 | 12313 | -0.61% | +0.27% | -1.89% … +1.77% |
| read | 4 | 2663 | 2462 | -7.55% | -4.68% | -21.64% … +0.00% |
| add | 4 | 11008 | 11200 | +1.74% | -3.00% | -11.83% … +0.21% |
| add_get | 4 | 12063 | 12249 | +1.54% | -1.47% | -6.08% … -0.16% |
| cas | 4 | 12301 | 12374 | +0.59% | -0.03% | -3.45% … -1.40% |
| read | 8 | 1446 | 1376 | -4.84% | -6.88% | -10.79% … +7.02% |
| add | 8 | 16945 | 16465 | -2.83% | -1.06% | -5.07% … +1.91% |
| add_get | 8 | 16025 | 16370 | +2.15% | +4.10% | -1.85% … +10.41% |
| cas | 8 | 16045 | 15940 | -0.65% | -0.25% | -1.92% … -0.04% |

An additional 8-worker check takes nine paired samples per series, each summing
ten alternating native executions (3.2 million operations per phase/sample).
Three such series provide 27 observations and 270 runs per version. The measured
times still exclude executable startup. Reproduce with an optional repetition
count and worker selection:

```sh
python3 docs/baselines/2026-10-14-typed-atomics/measure.py /tmp/batched.json 10 8
```

| Operation | Before µs / batch | After µs / batch | Pooled delta | Paired delta | Series median delta range |
|---|---:|---:|---:|---:|---:|
| read | 15250 | 14448 | -5.26% | -2.68% | -7.26% … -2.64% |
| add | 170045 | 165295 | -2.79% | -1.79% | -5.10% … -3.53% |
| add_get | 166064 | 169288 | +1.94% | +4.00% | -2.30% … +4.24% |
| cas | 161604 | 161754 | +0.09% | -0.09% | -0.65% … +0.37% |

No slowdown is established for the tested single-thread small-value paths.
A small `add_get` cost under 8-worker contention remains possible: about +2% by
pooled medians, +4% by paired medians, with series ranging from -2.30% to +4.24%.
Do not describe that case as zero overhead. The host was active throughout
(load averages recorded with every series, reaching 4.48 in the batched runs);
repeat on an isolated host before treating the difference as a stable regression.
The optimization removed avoidable register saves without weakening validation
or changing the atomic synchronization primitives. These measurements do not
establish identical performance across all inputs or platforms.
