# Rust and Cranelift refresh — 2026-10-10

This report preserves the validation-time state before publication. The machine
default was subsequently changed to Rust 1.99.0 at the user's request. The
changes are now recorded in separate commits on `mr/preview-reliability-refresh`;
see [publication evidence](publication.json) for the source comparison.

Two sequential upgrades on top of the [three repaired blockers](../2026-10-10-repairs/README.md).
No new regressions were found in either local macOS ARM64 baseline. Existing
failures remain; this is not a green CI or public-preview readiness verdict.

## Changes and order

1. Installed Rust 1.99.0 alongside the existing Rust 1.97.1 default. First ran
   the full baseline with the original dependency lockfile byte-for-byte intact
   (SHA-256 `ad42464c59618c1e56a7102589e51a26ffac0f55401b8e4d7e24c039f110bb1d`).
   After that comparison passed, pinned 1.99.0 in `rust-toolchain.toml`, CI and
   release workflows, added the root lockfile as a version-control input,
   required `--locked` for CI/package builds, and updated contributor guidance.
   At validation time, the machine default remained 1.97.1 outside this project. Rustup also updated
   itself from 1.29.0 to 1.29.1 during installation.
2. Updated the five direct Cranelift requirements from `0.129` to `0.129.2`.
   The lockfile moves all 15 Cranelift crates from 0.129.1 to 0.129.2 and
   `wasmtime-internal-core` from 42.0.1 to 42.0.2. Every unrelated package,
   checksum and dependency edge is unchanged. No compiler-source changes were
   needed. The final lock SHA-256 is
   `2b23030c542224604bc14c0b2b0bceff36dcde82391ab8ebe6a7d7b575802e3e`.

The limited Cranelift patch addresses the upstream ARM64 shifted-index load
lowering bug. Its applicability as an exploitable issue in Nera was not
established. This does not adopt the newer Cranelift release series or claim
measured performance gains. See the [upstream advisory](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-jhxm-h53p-jm7w).

## Comparison

| Check | Rust 1.97.1 + Cranelift 0.129.1 (repaired) | Rust 1.99.0 + Cranelift 0.129.1 | Rust 1.99.0 + Cranelift 0.129.2 |
| --- | --- | --- | --- |
| Release build | Pass | Pass | Pass |
| Rust workspace | 1,677 pass; 5 fail | Same | Same |
| Erlang suite | 285 pass; 0 fail; 2 skip | Same | Same |
| Exception resumption / formatter BEAM probes | Pass | Pass | Pass |
| Formatter semantics, three sources | Pass | Pass | Pass |
| Late-reply BEAM probe | Known mismatch | Same | Same |
| Thoas 1.2.1 build and run | Pass | Pass | Pass |
| Poolboy 1.5.2 | Five missing APIs | Same | Same |
| Installed external and embedded runtime | Pass | Pass | Pass |
| Rust formatting | 123 files differ | Same file set | Same file set |
| Strict Clippy | Eight syntax findings | Same | Same |

The five Rust failures are the existing deprecation-diagnostic tests in
`nera-ir`: `deprecated_attribute_parsed`, `deprecated_multiple_functions`,
`deprecated_call_warns`, `deprecated_call_has_correct_code`, and
`deprecated_fun_ref_warns`. Poolboy still needs `ets:match/2`, `ets:select/2`,
`supervisor:start_child/2`, `supervisor:terminate_child/2`, and
`supervisor:which_children/1`. The two Erlang skips require external OneCached
modules. TCP tests were run without builds competing for CPU; a passing run
does not resolve the earlier intermittent TCP failures or replace a soak test.

Workflow structure validation passes with `actionlint -shellcheck=`. Full
`actionlint` reports the same pre-existing SC2012 warning in the JetBrains
archive rename step before and after this change. Shell syntax and Git diff
whitespace checks pass. CI configuration was inspected locally; GitHub CI,
Linux, Windows, and cross-target execution were not run.

## Reproduction and artifacts

The main test commands are:

```sh
RUSTC_WRAPPER= cargo build --release --locked -p nera-runtime
RUSTC_WRAPPER= cargo build --release --locked
RUSTC_WRAPPER= cargo test --workspace --locked --no-fail-fast
./target/release/nera test -j 4 --timeout 30
RUSTC_WRAPPER= ./scripts/dist.sh --version 0.1.0-cranelift --target aarch64-apple-darwin
python3 scripts/test-dist.py dist/nera-0.1.0-cranelift-aarch64-apple-darwin.tar.gz
```

Actual validation used `CARGO_TARGET_DIR=target/rust199-baseline`, initially
with explicit `+1.99.0` and subsequently the project pin. Build/test commands
used `--offline` after downloading the locked dependencies. The directory was
reused for the Cranelift run; its current compiler is Rust 1.99.0 + Cranelift
0.129.2. The older `target/release` binaries were deliberately preserved and
are not the refreshed result. The `RUSTC_WRAPPER=` setting bypasses this
machine's compiler-cache wrapper. Runtime tests require local socket access.

Retained local archives:

- `dist/nera-0.1.0-rust199-pinned-aarch64-apple-darwin.tar.gz`
- `dist/nera-0.1.0-cranelift-aarch64-apple-darwin.tar.gz`

Each installed-archive smoke test runs outside the checkout with an empty
runtime cache, first with the external library, then using only the embedded
runtime. Separate patches are saved here as `01-rust199-pin.patch` and
`02-cranelift1292.patch`; apply them in that order to the pre-refresh repaired
worktree. The first introduces the unchanged original lockfile; the second
updates it. Ordered application was verified to reproduce the final scoped
files. The earlier implementation fixes and regression tests are unchanged.

`results.json` records commands, exits, timings and probe results;
`dependency-changes.json` lists dependency moves; `source-manifest.json` binds
the final evidence to file hashes; `rust199-pin-hashes.json` records the
intermediate pin state; `log-hashes.json` fingerprints raw logs retained in
`target/toolchain-refresh-2026-10-10/`. No new test is needed for the mechanical
pin/version changes; both stages reran the existing regression suites.

Git scope: `BASE_REF=origin/main`, `BASE=c5b89ee65d679da1f2fe257aef73f130859440bb`,
base tree `39856e395ff12a1548741a2cd895975e5e80a856`, cached divergence 0/0.
`TARGET` is the uncommitted working tree identified by the source manifest,
including the earlier repairs. This refresh changes the two workflows,
`.gitignore`, root lockfile, toolchain file, codegen dependency manifest,
distribution script, README, CONTRIBUTING and project-status documentation.
At validation time, all changes were unstaged; no commit, push, tag or release
had been created. The publication follow-up above records the later Git state.
