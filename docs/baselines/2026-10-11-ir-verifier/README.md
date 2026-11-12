# IR contract and structural verifier validation — 2026-10-11

## Target and scope

Validated the uncommitted worktree changes listed below on branch
`mr/process-state-transitions`, with final HEAD
`d1f3bfca752a6223f57e5ce8c3b777c36b9cc6ba`. No staging, commits or publication
were performed for this task. Unrelated runtime edits present at the start
were left untouched; they were committed separately while this task ran.

Integration reference: local `main`; merge base with final HEAD:
`c1b42dcc2c31634def8de8812289c298cef1ddac`. Final HEAD is 25 commits ahead,
0 behind that local reference. This report assesses only the IR worktree
changes relative to final HEAD, not those 25 commits. There is no Git tree ID
for the uncommitted target; the SHA-256 manifest below identifies its scoped
file contents. Pre-existing `.claude/` and `.lavish/` files are outside scope.

## Sequential evidence

1. Existing IR baseline: **372 unit tests passed**. Documentation-only edits
   corrected the SSA description and added the contract; `cargo doc -p nera-ir
   --no-deps --locked` passed before verifier implementation.
2. RED: the new API initially returned no violations. **12 of 14 focused tests
   failed at the intended missing-diagnostic assertions**, and two legal-IR
   cases passed. This was an assertion failure, not a compilation/setup failure.
3. GREEN: structural and definite-assignment implementation passed all 14
   focused tests. Verification was then added to the existing source-lowering
   test helper: **386 unit tests passed**.
4. Additional edge cases and real-source coverage: **392 IR unit tests and
   20 integration fixtures passed**. Fixtures include shared branch results,
   loops, closures, binary matching, exceptions after receive suspension, record
   includes, `lists` and `gen_server`.
5. Workspace: `cargo test --workspace --locked --no-fail-fast` passed all targets
   except the runtime unit target. Its nine socket failures all reported OS
   permission errors under the sandbox. Rerunning the entire runtime unit target
   outside the sandbox passed **448/448**. Across these runs, all workspace test
   targets passed (**1,744 tests**, including integration and documentation tests).
6. Final documentation build, `git diff --check`, and rustfmt checks for the
   new verifier and integration-test files passed.

Commands used `RUSTC_WRAPPER=` to bypass the local sccache wrapper, which
could not start under the sandbox. Rust/toolchain/dependency versions were not
changed.

## Lint boundary

`cargo clippy -p nera-ir --all-targets --locked` fails on the existing
`clippy::approx_constant` error at `src/pattern.rs:1381` (`3.14` in a pattern
construction test). The same line exists in committed HEAD and was not modified.
Rerunning with `-- -A clippy::approx_constant` completes; existing warnings remain
in older code, with no findings in the new verifier or integration test.
This is not a claim that the repository is lint-clean.

## Behavior and limits

The verifier is a public read-only API (`verify_function`, `verify_module`),
invoked by tests only. No production call site, IR transformation, runtime API,
ErlTerm boundary, dependency, or native-code emission was changed. Rust workspace
regressions were run; the separate native Erlang end-to-end runner was not rerun
for this additive API/documentation milestone. This is structural verification
of explicit IR, not proof of GC, exception, suspension or Erlang semantics.

## Scoped file manifest

SHA-256 of the final source/documentation inputs (this report excluded):

- `ae147c67d18d1dfafdda770314b64cb51ac90c9591dc665992ac27d462288df3` — `docs/IR_CONTRACT.md`
- `3da3c88f649a7e99a6deb5ee62cb0602d6e1293d809f8c5506ec2c445b08312f` — `docs/ARCHITECTURE.md`
- `e376dd0507cb8062dc88c20182db1099232211bb0bccf3fdb3f511edaa7df173` — `crates/nera-ir/src/ir.rs`
- `d75ac562a83c482505f9e6dfafd72c4c375dcc588daf3f5a3871b01572c2dcc3` — `crates/nera-ir/src/lib.rs`
- `537c63f01e83a1fc8c2256ac2dfdeb81df92ed015de25a78c3449c4a534329c9` — `crates/nera-ir/src/lower/mod.rs`
- `c0946136f5d61871074bb45361d4c91560fa77dc7aeed7b0d14d72d2ec7e5c59` — `crates/nera-ir/src/lower/tests.rs`
- `fe0c1ea9622e2cb450b5c474a44874ecca1765f6f275348edc4c3be5a3b7511a` — `crates/nera-ir/src/verify.rs`
- `77df0a50f2b62403225f7ce4fa74b851e13a2cfe25782447c5034ec8b26b1bb6` — `crates/nera-ir/src/verify/operands.rs`
- `28b8b5cbd6a6520d0f69bf6b2fd22cad9fa620b557659471c801b927ff7e3168` — `crates/nera-ir/src/verify/tests.rs`
- `6671b4b1d3511b4f9ade9a5c0a64bb0d906a4ba81603ba49c59c7be1107441a7` — `crates/nera-ir/tests/verify_lowered.rs`
