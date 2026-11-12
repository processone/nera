# Incremental Base64 list decoding — 2026-10-13

Base: main `5b24148ce3281db26795adbc07956f86186abdb6` (after #69).
The branch was initially based on #68, then rebased onto #69 before publication;
the observer regression now uses the new process-entry guard. All checks below
were rerun on that final base. Source/artifact/evidence SHA-256 values are in
`source-manifest.json`; the final commit adds only validation documentation to
those validated source contents.

## Scope and behavior

- `base64:decode/2` list inputs use the native `base64:decode_list/3` BIF.
  The Erlang symbol/group decoder is removed from production; the frozen
  historical benchmark module is retained.
- At most 8,192 inspected cells per invocation, bounded by the remaining
  reductions at eight cells per reduction. Empty/final/error work is charged;
  whitespace counts. The continuation roots the remaining list, while native
  state contains no Erlang terms or borrowed pointers.
- The completed output is adopted without a full-size final copy. Buffer
  growth is fallible and checked against the binary header's size domain.
- The public list path matches OTP 29.0.5 values and exception class/reason,
  including formerly divergent malformed-list and out-of-byte-range behavior.
  This is intentionally a compatibility change from Nera's old list decoder.
- No changes to GC triggers, collection traversal, or scheduler policy. GC and
  allocation/reallocation remain synchronous; this is not a latency bound.
  GC corrections are explicitly deferred to a separate PR.

## RED / GREEN

`base64_list_decode` passes on BEAM but fails on pre-change main with
`{badmatch,{error,function_clause}}` where `[512]` should raise `badarg`.
See `red-beam.log` and `red-nera.log`. RED was recorded on #68; its Base64
stdlib blob is byte-identical to #69's pre-change version.

The new implementation passes the same regression and the direct BEAM corpus:
67,529 values/errors covering short symbol combinations, all byte values,
large nonuniform inputs, whitespace, partial quartets at suspension boundaries,
invalid integers/floats/atoms, improper tails, option defaults/errors and error
precedence. The corpus compares complete output values, not just sizes. The
frozen reference remains the oracle for encode and binary decode; BEAM is now
the list oracle.

## Checks on macOS arm64

Environment: Apple M1 / 16 GiB / macOS 15.7.7, Rust 1.99.0, OTP 29.0.5.
`RUSTC_WRAPPER=''` and a dedicated `CARGO_TARGET_DIR` were used.

| Check | Result / evidence |
|---|---|
| Release runtime, compiler and distribution | Passed, `dist-build.log` |
| `cargo test --workspace --locked --no-fail-fast` | 1,891 passed, 1 existing ignored, `workspace-tests.log` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed, `clippy.log` |
| `nera test -j 1 --timeout 30` | 329 passed, 2 existing skipped / 331, `erlang-tests.log` |
| `python3 scripts/test-base64.py --nera <compiler>` | BEAM public regressions + 67,529 list results + existing encode/binary corpus, `beam.log` |
| `python3 scripts/test-base64-scheduler.py --nera <compiler>` | BEAM +S 1 and Nera one-scheduler progress, five native repetitions, `scheduler.log` |
| `python3 scripts/test-dist.py <archive>` | List success and late error through external and embedded runtimes, `dist-test.log` |
| `scripts/test-cross.sh <compiler> x86_64-apple-darwin` | Object generation only, `cross-macos.log` |
| `scripts/test-cross.sh <compiler> aarch64-unknown-linux-gnu` | Object generation only, `cross-linux.log` |

The two Erlang skips require external `onecached` / `onecached_storage` modules.
The new runtime tests cover partial-quartet state across full moving GC,
remaining-input ownership and process destruction, zero/one/normal budgets,
final and whitespace reduction charging, resumed errors, and an observer running
before a large native list decode completes on one scheduler.

Linux/Windows runtime execution and their native archives were not tested.
GitHub CI is intentionally paused by repository policy and was not enabled or
rerun. Independent review and the user's separate merge approval remain required;
this task creates a PR, not a merge.

## Local custody

Only the scoped production changes, tests and this evidence are published.
Pre-existing work in the primary checkout and the untracked production benchmark
results in the attached worktree remain untouched and outside this PR.
