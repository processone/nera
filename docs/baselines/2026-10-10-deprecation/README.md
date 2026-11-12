# Deprecation diagnostics repair — 2026-10-10

The local workspace now passes **1,686 Rust tests, zero failures**. This adds
four regressions and fixes the five existing failures without changing those
five tests or any dependency. Base: PR #1 head
`ab63234060f3241270babce992fef0bb24d75298`; the validation below was recorded on the corrected
working tree of `mr/preview-reliability-refresh` before committing it.

## Cause and correction

The parser recognized `-deprecated` but skipped its body and returned an empty
function list. Existing warning analysis and semantic highlighting already
consume that list; they never received the metadata they needed.

The parser now collects shorthand `name/arity`, individual tuples and tuple
lists, including optional string reasons and removal-marker atoms. Module and
wildcard declarations expand against exports after all attributes are read.
Explicit signatures preserve Nera's existing local-call warning policy, even
for private functions. The public AST remains a list of concrete function/arity
pairs. Reasons are accepted but not retained or printed, and this does not add
cross-module warning analysis. Standard forms are described in the
[Erlang Xref documentation](https://www.erlang.org/doc/apps/tools/xref.html).

Three warning regressions cover tuple forms, wildcard export selection,
repeated attributes, quoted names in function references, exact arity matching
and source spans. A parser regression rejects malformed declarations and
out-of-range arities without losing the following function. The five original
failing tests are unchanged. Counter-verification with the original parser
produced eight failing warning tests and one failing malformed-input test;
restoring the fix passes all twelve focused tests.

## Baseline

Rust 1.99.0, Cranelift 0.129.2, macOS ARM64, BEAM OTP 29. Dependency lock SHA-256
remains `2b23030c542224604bc14c0b2b0bceff36dcde82391ab8ebe6a7d7b575802e3e`.

| Check | Result |
| --- | --- |
| Ordered release runtime/compiler build | Pass |
| Rust workspace | **1,686 pass; zero fail** |
| Erlang, four jobs, 30-second timeout | **285 pass; zero fail; two skips / 287** |
| Exception and formatter BEAM probes | Pass |
| Three-source formatter semantics | Pass |
| Thoas build/run | Pass |
| Installed archive, external and embedded runtime | Pass |
| Poolboy | Same five missing APIs |
| Late-reply BEAM comparison | Same known mismatch |
| Rustfmt | Same 123 files with formatting differences |
| Strict Clippy | Same eight syntax-crate findings |
| Diff whitespace | Pass |

Builds finished before running the Erlang suite. Passing TCP tests do not
explain the original intermittent failures or constitute a soak test. The two
skips require external OneCached modules. The archive is retained locally as
`dist/nera-0.1.0-deprecation-aarch64-apple-darwin.tar.gz`.

## PR CI inspection

[Run 38074383034](https://github.com/processone/nera/actions/runs/38074383034)
tests the published head before this correction. Native macOS and Linux jobs
both fail on the five deprecation tests. Their later end-to-end and integration
steps therefore were not reached. Local success with this correction is not a
new remote CI result.

Both cross jobs (macOS x86_64 and Linux ARM64) build the Rust artifacts but their
Hello World checks fail on thirteen unresolved calls from repository fixtures. The CLI's
`discover_files` function adds `tests/` under the current directory even when
an explicit input file is supplied. The job runs from the repository root,
so it checks 307 source files instead of the intended smoke-test sources.
This includes deliberately invalid and external-dependency fixtures.

An isolated temporary-directory control with the corrected local compiler
successfully emits Mach-O x86_64 objects for Hello World and the standard
library. This proves object generation only, not linking or execution on an
x86_64 machine. The cross-job working directory remains a separate CI blocker;
no workflow change is included in this repair. All four jobs finished in failure on the pre-fix head; their final status is
recorded in `ci-status.txt`.

## Evidence and reproduction

`results.json` preserves command exits and timings; `source-manifest.json`
binds the result to the changed source and unchanged toolchain/lockfile.
`log-hashes.json` fingerprints the raw logs retained under
`target/deprecation-repair-2026-10-10/`. CI logs are from the published PR head;
local baseline logs are from the corrected working tree.

```sh
RUSTC_WRAPPER= cargo test -p nera-ir -p nera-syntax deprecated --locked --offline --no-fail-fast
RUSTC_WRAPPER= cargo test --workspace --locked --offline --no-fail-fast
RUSTC_WRAPPER= cargo build --release --locked --offline -p nera-runtime
RUSTC_WRAPPER= cargo build --release --locked --offline
./target/release/nera test -j 4 --timeout 30
```

The recorded runs used `CARGO_TARGET_DIR=target/rust199-baseline` and therefore
`target/rust199-baseline/release/nera`; `target/release/nera` is not the refreshed
binary. Socket tests required local network permission. The earlier compiler
cache workaround `RUSTC_WRAPPER=` remains in use. The original and repaired
baselines are retained without rewriting their historical results.
