# Existing IR pass extraction — 2026-10-11

## Target

PR #7 was merged after its four CI jobs passed. This extraction started on
`mr/extract-ir-passes` from that merge,
`d0420d8800d78e641d592fe15e6b482881a8ddb9` (local `origin/main`).
The validation target is the uncommitted worktree relative to that base;
HEAD is zero commits ahead/behind the recorded reference. No extraction commit,
push or PR is included. Pre-existing `.claude/` and `.lavish/` files are outside
scope. [Input hashes](inputs.json) identify all 17 changed source/test/document
files, excluding this evidence directory; an uncommitted target has no tree ID.

## Sequential checks

Before extraction, the existing pipeline passed 392 IR unit tests and 20 real
lowering fixtures, with structural verification inserted at each pass boundary.
Five focused scheduling/fusion tests also passed against the original code.

Each pass was then moved independently, in pipeline order. After **each** move,
`cargo test -p nera-ir --locked` passed: 392 unit tests, five pass integration
tests, and 20 real-source verifier fixtures. Full debug IR from every fixture
was byte-identical to its pre-extraction baseline. A separate comparison found
all 11 algorithm/helper bodies unchanged apart from comments and formatting.

After extracting the pipeline entry point and adding helper verification,
the same suite passed with six pass integration tests. All 20 IR snapshots
still matched. The new combined pipeline regression was counter-checked by
moving yield insertion after dead-code elimination: its output assertion failed.
Restoring the original order passed all six pass tests.

[Stage results and fixture digests](stages.json) record these comparisons.
The temporary snapshot hook was removed before final workspace validation.
The fixture test file is unchanged from the base.

## Final checks

Commands used `RUSTC_WRAPPER=` to bypass the local sccache wrapper.

- `cargo test --workspace --locked --no-fail-fast`: **1,750 passed**, zero
  failures or ignored tests, across 17 targets. Run outside the sandbox to allow
  runtime socket tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo doc -p nera-ir --no-deps --locked`: passed.
- `cargo build --release --locked`: passed before native execution.
- `./target/release/nera test -j 1 --timeout 30`: **285 passed, one failed,
  two expected skips / 288 total**. The failure was `tcp_reactor_perf` at
  `test_sustained_throughput_ratio/0`: `{throughput_regression,70,7}` (70 ms
  sustained versus 7 ms warmup, above the 9x threshold). Three separate isolated
  reruns of `nera test tcp_reactor_perf -j 1 --timeout 30` each passed. The user
  reported possible high laptop load. Load sensitivity is plausible, but the
  cause remains unconfirmed; retries do not make the original full run green.
  The test and runtime were not changed. A future recurrence should capture
  system load alongside both timing measurements.
- Rustfmt checks on the new pass modules and integration tests: passed.
- `git diff --check`: passed.

## Boundaries

Production behavior, pass order, generated-helper treatment, public `lower`
paths, runtime and ErlTerm safety boundaries are preserved. Structural checks
run in tests only. No generic pass manager or new optimization was introduced.

Validation is local macOS arm64. Remote CI and cross-target builds have not run
for this uncommitted extraction. PR #7's CI applies only to its earlier verifier
change. Optional third-party integration projects were not rerun.
