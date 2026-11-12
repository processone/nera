# Rebased binary-size and list-operator validation

2026-10-15. Branch mr/binary-segment-list-contracts rebased on main
306f5fe624718a2eebcadb342133fa8c4e2f0871 (PR #99). Production source commit:
28c40e4. This is the current integration report; the binary-list-contracts and
tuple-subtraction sibling reports retain historical pre-rebase evidence.

## Integration

The rebase preserved main's PropEr/Cowlib additions, including bitstring copies,
binary conversions, parser changes and lists:sublist/2. Only CHANGELOG.md needed
manual conflict resolution; both sets of milestones were retained. No additional
production fix was needed after integration. Subsequent publication changes only
preserve this validation, its measurements and documentation.

Code changes validate individual nonnegative integer segment sizes before scaling
or summing, reject invalid units before narrowing, check capacity/descriptor
rounding before allocation, and propagate exceptions before writes or subsequent
expressions. Literal valid sizes are resolved at compile time; dynamic small
sizes use an inline fast path. ++ validates its left spine while allowing an
arbitrary right tail; -- validates both spines and uses exact structural equality.
Its adaptive filter caches short flat tuple fingerprints only after 64 candidate
comparisons with at least eight right candidates remaining. Hash matches always
receive exact comparison; deliberately colliding hashes and duplicate multiplicity
are covered. Traversal selects its strategy outside the main loop.

## Validation method

Rust 1.99.0, unchanged Cargo.lock, Apple M1/macOS ARM64. With RUSTC_WRAPPER= and
CARGO_TARGET_DIR=/tmp/nera-transfer-target:

```sh
RUST_TEST_THREADS=1 cargo test --workspace --locked --no-fail-fast
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
RUST_TEST_THREADS=1 cargo test -p nera-runtime --release --locked
/tmp/nera-transfer-target/release/nera test 'binary*' 'bitstring*' 'list*' 'bif_*' large_list match_list 'map*' 'compare*' 'term*' 'numeric*' 'bigint*' 'try*' 'catch*' 'receive*' 'gc*' 'os_*' unique_integer process_dict_get_all preproc_conditionals -j 1 --timeout 30 --no-warnings
python3 -m unittest discover -s tests/integration/cowlib -p 'test_*.py'
python3 -m unittest discover -s tests/integration/proper -p 'test_*.py'
```

Serial Rust tests avoid the existing shared socket-registry interference observed
in earlier parallel checks. Runtime cross-compilation checks cover Intel macOS
and Windows MSVC; the latter retains six existing unused-code fallback warnings.
The Python harness tests do not execute upstream native PropEr/Cowlib properties.
The pinned external source checkouts documented by those integrations were absent
at their recorded paths, so those upstream scenarios were not rerun.

## Performance scope and reproduction

Fresh after binaries use the rebased release compiler/runtime. Preserved before
binaries are the pre-filter correct implementation and the original PR #95 raw
pointer implementation, both built on the earlier PR #95 base. They are not
fresh builds of main #99; current comparisons include integration changes and
cannot isolate their cost from the optimization. Historical controlled figures
below remain explicitly labelled as pre-rebase observations.

The probes are identical to the earlier measurements. tuple_probe.erl checks
complete results against a separate reference across disjoint tags, same tags,
partial overlap, duplicates, immediate/nested/wide elements and small operands.
It runs 100 warm-ups and 10,000 timed operations per case. legacy_probe.erl keeps
the original size/length-checking probe and its binary-construction cases.
Compile each outside project discovery as probe:main, with filename probe.erl.
The before/after paths are defined in measure.py; it runs three series of nine
alternating pairs per group (27 observations/version/group), one scheduler and
warm-up subprocesses. Compilation, startup and printing are outside timers.
No builds/tests intentionally run during timing. JSON preserves all samples,
per-series changes, loads and executable hashes.

## Historical controlled figures

Before rebase, the final adaptive filter reduced the initial tuple workload from
19,835 to 9,892 µs per 10,000 operations (-50.13%). Full-result disjoint probes
improved 42.84% and 44.78%. Remaining costs against the correct pre-filter version:
**duplicates +7.50% (~38 ns/op)**, **partial overlap +5.74% (~54 ns/op)**,
nested +2.66%, immediate +1.22%; small -2.93%, wide -0.03%. All three duplicate
and partial-overlap series were positive. These costs must not be presented as
zero overhead. Against the incorrect raw-pointer original, the disjoint legacy
workload still cost +78.44%; the original fails separately allocated equal terms.

Earlier binary-size measurements (54 observations/version before the tuple-filter
follow-up) showed constant -0.26%, dynamic -3.05%, unit scaling -3.28% and two
segments +2.96%. Those observations were made on an active host, with substantial
series variation, and do not establish universal speedups or zero overhead.

The full native suite, actual Linux/Windows execution, installed archives and
contention were not tested. CI/Release remain paused per CONTRIBUTING.md. The PR
is for review; independent review, applicable remaining merge checks and separate
explicit merge approval are still required before merging.

## Rebased results

- Workspace: **1,981 passed**, zero failed, one existing ignored.
- Runtime release: **635 passed**, zero failed, one existing ignored.
- Native: **90 distinct tests passed**, zero failed/skipped. Includes both new
  regressions, the new main bitstring/binary/list cases, BIFs, maps, exceptions,
  receive/GC and process-dictionary/environment/unique-integer cases.
- Locked release build, workspace/all-targets Clippy and both runtime cross checks
  passed. Python harness checks: **73 Cowlib**, **6 PropEr** passed; these are not
  fresh upstream property runs.

| Rebased full-result probe | Before µs | After µs | Change | Series range |
|---|---:|---:|---:|---:|
| disjoint_tags | 30,263 | 17,426 | -42.42% | -42.72% to -41.33% |
| same_tag | 26,538 | 14,805 | -44.21% | -45.53% to -43.09% |
| duplicates | 5,085 | 5,429 | +6.76% | +6.30% to +7.58% |
| overlap | 9,344 | 9,960 | +6.59% | +6.19% to +7.19% |
| immediate | 9,597 | 9,663 | +0.69% | -0.60% to +1.60% |
| nested | 35,009 | 36,089 | +3.08% | +1.72% to +2.74% |
| small | 570 | 561 | -1.58% | -3.30% to +1.40% |
| wide | 20,604 | 20,613 | +0.04% | -0.64% to +0.67% |

The remaining duplicate cost is **+6.76% (~34 ns/op)**; partial overlap is
**+6.59% (~62 ns/op)**. Both stay positive in all three series. Do not call the
optimization universally free. Immediate values are +0.69%, nested +3.08%, wide
+0.04%, small -1.58% in these probes. Long disjoint cases improve 42.42–44.21%.

The legacy correct comparison is **20,068 → 9,906 µs (-50.64%)**, with all series
between -50.78% and -50.25%. Against the original raw-pointer binary the disjoint
legacy case is **5,549 → 9,856 µs (+77.62%)**; correctness still requires value
comparison for separately allocated equal terms. Legacy valid binary cases versus
the original baseline are constant -0.27%, dynamic -2.46%, unit -3.56% and two
segments +1.71%; append -0.12%, immediate subtraction -0.18%. The main integration
confound and series variation above apply; these are observations on an active
host, not general throughput guarantees.

Measurement start load averages (1/5/15 min): [2.2509765625, 3.75390625, 4.96142578125]; end:
[2.17138671875, 3.5595703125, 4.84033203125]. Source and executable fingerprints bind the rebased validation.
