# Preview blocker repairs — 2026-10-10

This follow-up preserves the [original baseline](../2026-10-10/README.md).
It covers three separate, uncommitted fixes on top of
`c5b89ee65d679da1f2fe257aef73f130859440bb` (`main`, matching cached
`origin/main`). No release, tag, push or commit was made.

## Changes

1. **Exception resumption.** Continue unwinding saved callers with exception
   state intact. Generated unprotected call continuations return immediately;
   protected ones reach their existing handler. Function entry now returns
   from the actual stack-overflow branch, instead of mistaking an incoming
   exception for an overflow. The new `tests/resume_exception.erl` requires
   explicit completion and covers catch/try, error/exit/throw, nested callers,
   timeout/message wakeups, cleanup, a handler that suspends again, and normal
   delayed returns. The original compiler exits successfully with no completion
   output; BEAM and the repaired compiler complete. Existing stack-overflow
   coverage also passes.
2. **Formatter grouping.** Preserve weakly binding unary operands and prevent
   nested negation from becoming the `--` token. Preserve right-nested binary
   grouping for left-associative operators. Five deterministic regressions in
   `crates/nera-fmt/tests/operator_precedence.rs` cover these cases and the
   `guard_not`/`fun_parallel_scopes` fixtures. Focused tests failed before each
   repair; formatter unit, integration and property checks now pass.
3. **Distribution.** Detect the host using Rust's canonical triple. Build the
   runtime before the compiler; package the external library at the installed
   lookup path. Cross builds now compile both artifacts for the advertised
   target. MSVC library naming is recognized by embedding and host lookup.
   Archives omit macOS resource-fork files. `scripts/test-dist.py` compiles and
   runs outside the checkout with an empty temporary runtime cache, then removes
   the external library and repeats using the embedded runtime. The release
   workflow invokes this smoke test for native macOS/Linux jobs and configures
   the Linux ARM64 cross linker.

## Final results

| Check | Result |
| --- | --- |
| Final release build | Pass |
| Rust workspace, all crates | **1,677 pass; 5 fail** (existing deprecation tests) |
| Erlang suite, 4 jobs, 30-second timeout | **285 pass; 0 fail; 2 skip / 287**, 62 seconds |
| Formatter suites | 121 unit + 92 integration + 5 new regression + 4 property tests pass |
| BEAM comparisons | Resumption and formatter pass; late-reply mismatch now observable |
| Fresh-source package build | Pass, canonical `aarch64-apple-darwin` target |
| Installed package, external runtime | Pass |
| Installed package, embedded runtime only | Pass |
| Thoas 1.2.1 | Build/run pass; dependency tree identical to baseline |
| Poolboy 1.5.2 | Same five missing ETS/supervisor APIs block compilation |
| Rust formatting | Existing repository-wide differences remain |
| Strict Clippy | Same eight syntax-crate findings stop the check |
| Diff whitespace and distribution script syntax | Pass |
| Independent application of each repair patch | Pass against baseline sources |

The two Erlang skips need external OneCached modules. Both TCP tests passed
this run with no builds active; this does not explain the earlier intermittent
failures or constitute a soak test. The repaired timeout probe now prints
`late_reply` in Nera versus `no_late_reply` on BEAM: process-alias/late-reply
semantics remain separate work. Full release readiness is **not** established.

The archive is also retained at
`dist/nera-0.1.0-repaired-aarch64-apple-darwin.tar.gz`. Its compiler and runtime
both report arm64. This is a local validation artifact, not a published release.

## Reproduction

The toolchain and dependency resolution match the original baseline: macOS
15.7.7 arm64, Rust/Cargo 1.97.1, BEAM OTP 29. The root lock snapshot remains
unchanged. Rust commands use `RUSTC_WRAPPER=` to bypass the local compiler cache
wrapper, plus `--locked --offline` where available. Runtime tests need local
socket access.

```sh
RUSTC_WRAPPER= cargo build --release --locked --offline
RUSTC_WRAPPER= cargo test --workspace --locked --offline --no-fail-fast
./target/release/nera test -j 4 --timeout 30
RUSTC_WRAPPER= CARGO_NET_OFFLINE=true ./scripts/dist.sh \
  --version 0.1.0-repaired --target aarch64-apple-darwin
python3 scripts/test-dist.py dist/nera-0.1.0-repaired-aarch64-apple-darwin.tar.gz
python3 docs/baselines/2026-10-10-repairs/format_semantics.py
python3 docs/baselines/2026-10-10/reproduce.py
```

The last command also tests late-reply semantics, a separate compatibility gap;
it can fail even after the exception and formatter probes pass.

Raw logs and three separate patches are retained locally in
`target/preview-repairs-2026-10-10/`. The source manifest and normalized results
in this directory bind the evidence to the repaired files. The fresh package
build used a disposable source copy with no initial target directory and the
recorded lockfile. No claim of Linux, Windows, cross-target execution, fuzzing,
long-running service reliability, or public-preview readiness follows from
these macOS checks.
