# Binary-match publication validation — 2026-10-13

The two corrections are applied to updated `origin/main` at
`79e2bd1eddf5e18589c601b657702ac01f7174e4`, including the intervening GC,
message-binary and scheduler-borrow fixes. No production code adjustment was
needed when applying the changes to that base.

## Checks on the publication source snapshot

- Release build: pass.
- Workspace Clippy, all targets with warnings denied: pass.
- Full Rust workspace and doctests: **1,850 passed**, one existing ignored
  release-only accept-cancellation scaling benchmark.
- Full native Erlang suite: **322 passed**, two existing external-OneCached
  skips; socket tests ran with local-network access.
- BEAM/native regression: identical successful completion across the original
  four-bit counterexample, 2,048 offset/value combinations, terminal readers,
  signed/little-endian extraction and a large binary.
- Whitespace check and syntax parsing of the four Python harnesses: pass.

Commands: `cargo build --release --locked`,
`cargo clippy --workspace --all-targets --locked -- -D warnings`,
`cargo test --workspace --locked --no-fail-fast`, and
`nera test -j 1 --timeout 30`. Builds use `RUSTC_WRAPPER=` and a separate
`target/binary-match-publication` directory. The explicit BEAM comparison uses
`docs/baselines/2026-10-13-bit-offset/compare.py --nera <built-compiler>`.

`validation.json` binds source, runtime/compiler and log hashes to this base.
Linux/Windows, cross-target, installed-archive and independent merge-review
checks are not claimed. GitHub CI remains paused under repository policy.
The PR is prepared as a draft, not merged.

## Historical evidence

The adjacent `2026-10-13-match-context-allocation` and
`2026-10-13-bit-offset` directories are preserved historical snapshots. Their
base IDs, validation scope, publication status and known-bug statements describe
when they were recorded, not the final combined correction. In particular, the
bit-offset failure preserved in the allocation investigation is now corrected.
The allocation-attribution script depends on the Base64 prototype delivered in
the companion PR; the correction regressions and BEAM comparison are standalone.

For Git whitespace checks, trailing empty lines in captured logs were removed
and their evidence hashes refreshed. Original raw logs remain in the source
checkout. No test output lines or source snapshots were changed.
