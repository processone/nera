# Warning audit and Clippy cleanup — 2026-10-11

Base: `origin/main` at `b4008d5b86b449d4a74a767f9b757c58f302e006`
(PR #2 merged). Branch: `mr/warning-cleanup`, in an isolated checkout to preserve
unrelated binary-runtime work in the original checkout.

## Initial compiler audit (before Clippy)

A fresh build found **no Rust compiler warnings or deprecation diagnostics**.
The test build also emitted no warnings. Consequently no source changes,
warning suppressions, dependency updates or toolchain changes were necessary.
Existing targeted lint allowances were not changed. This initial phase covered Rust compiler diagnostics; the Clippy follow-up is
recorded below.

### Initial checks

- Rust 1.99.0, macOS ARM64; Cargo.lock unchanged.
- Empty checkout-local target directory; compiler cache disabled with
  `RUSTC_WRAPPER=`. No RUSTFLAGS or encoded Rust flags were set, and there was no
  project Cargo config suppressing diagnostics.
- `RUSTC_WRAPPER= cargo build --workspace --release --locked`: passed, no warnings
  (fresh release build, 4m 10s).
- `RUSTC_WRAPPER= cargo test --workspace --locked`: 1,689 passed, no failures and
  no warnings, including the existing deprecation-related regression tests.
- Compiled `examples/hello.erl` from an isolated temporary directory, using the
  freshly built compiler and its bundled Erlang standard library: no warnings;
  execution returned `Hello, World!` with exit code zero.
- `./target/release/nera test -j 1 --timeout 30`: 286 passed, zero failures,
  two skipped (288 total). Detailed results are recorded in `results.json`.

The Erlang test runner captures compiler output for successful fixtures, so its
summary alone is not evidence that all fixtures are warning-free. Some fixtures
intentionally test deprecation diagnostics. The warning-free Erlang compilation
claim above applies to Hello World plus the bundled standard library only.

No remote CI, cross-platform execution or CI policy change was performed. Build/test logs and their hashes are recorded in
`results.json`; the logs remain local under `target/warning-audit-2026-10-11/`.

## Clippy follow-up

Ran `RUSTC_WRAPPER= cargo clippy --workspace --all-targets --locked`. Unlike the
compiler-only audit, this failed: `clippy::uninit_vec` found a `Vec<u64>` whose
length was extended over uninitialized storage. Test fixtures also used arbitrary
decimals resembling mathematical constants. Numerous style diagnostics affected
the runtime, compiler, syntax, formatter and LSP.

Corrections include:

- Represent young-generation bump storage as `Vec<MaybeUninit<u64>>`, preserving
  its alignment, allocation strategy and explicit initialization through raw
  pointers without claiming uninitialized words are valid `u64` values.
- Simplify loops while retaining checked slice bounds, pointer offsets and
  native-endian padding for integer writes wider than eight bytes.
- Name callback/result types, use a named ETS insertion error, remove unused
  scheduler notification arguments and recursive-only traversal arguments.
- Apply idiomatic integer rounding, range checks, optional-value handling,
  default implementations, and collection operations.
- Retain float-test coverage with arbitrary non-constant decimal fixtures, and
  move the map helper tests after production items.

No blanket lint suppression was added. There are 21 narrowly scoped `expect`
annotations with reasons: 20 existing long function signatures across lowering,
code generation, runtime/GC and CLI options; one ETS region vector whose boxed
entries must retain stable addresses. The signatures are intentionally deferred
architecture work rather than mechanically bundled arguments. These annotations
expect the specific lint and become diagnostics if that expectation stops being
fulfilled. Other warnings remain enabled.

The strict final command is
`RUSTC_WRAPPER= cargo clippy --workspace --all-targets --locked -- -D warnings`.
Final results and source hashes are recorded in `clippy-results.json` and
`clippy-source-manifest.json`. The initial `results.json` describes the unchanged
base only, not the Clippy cleanup. No dependency or toolchain updates were made.

### Final verification

- Strict Clippy, all workspace targets including tests: passed, zero diagnostics.
- Rust workspace tests: 1,689 passed, zero failures.
- Release workspace build: passed, no compiler warnings.
- Erlang baseline: 286 passed, zero failures, two skipped.
- Cross-object smoke checks: macOS x86_64 and Linux ARM64 passed.
- Git whitespace/error check: passed; source hashes match validation.

The results above describe the original cleanup before integration with PR #3.

### Publication revalidation

Before publication, main had advanced to `6a88d83` (PR #3). The cleanup was
rebased, preserving the new unsafe term-access contracts, BinaryData backing
layout, and updated map tests. Four conflict resolutions were inspected.

Strict Clippy passes on the integrated source, and the Rust workspace now has
1,696 passing tests with no failures. `publication-validation.json` binds these
checks to the code commit and source hashes. The earlier 1,689-test and local
Erlang results remain historical evidence, not claims for the rebased tree.
The PR CI verifies the integrated release/Erlang and cross-compilation paths.
