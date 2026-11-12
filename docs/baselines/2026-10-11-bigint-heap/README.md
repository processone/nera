# Bigint and heap contracts — 2026-10-11

Base: `ab347d5` (main at worktree creation). Working branch:
`mr/bigint-heap-contracts`. macOS ARM64, Rust version pinned by the repository.

## Changes

Raw `term_to_bigint` and `bigint_to_f64` are unsafe: callers must retain live,
initialized storage with the runtime layout, without moving GC or concurrent
mutation. The requirement propagates to numeric BIFs and formatting/comparison
helpers rather than being hidden inside a safe wrapper. Existing FFI adapters
already carry the term lifetime contract. The ETS parser uses a closure under
its existing unsafe contract; it does not retain the callback or terms.

Safe `BorrowedTerm::to_bigint` and `integer_to_f64` use the existing heap borrow.
The returned BigInt owns its digits and may outlive the source heap. No roots,
refcounts or dynamic checks are added. Conversions still use the same algorithm;
removing temporary digit buffers is deliberately outside this change.

Young allocation compares a request against remaining capacity before adding
it to the cursor. Growth checks layout on a cold path. GC uses `Layout::array`
to reject overflowing byte counts. Adaptive thresholds and the configured heap
limit saturate rather than wrap. Valid allocated word counts already fit byte
arithmetic; this change does not add redundant checks to every accounting sum.

## Regression and validation

Before the fix, both new release regressions failed at the intended assertions:
`chunk_rejects_wrapping_request_without_advancing` accepted `usize::MAX`, and
`gc_rejects_wrapping_word_layout` accepted a word count whose byte size wrapped.
Neither test dereferences the invalid reservation or attempts a huge allocation.
The final tests also check that a rejected process reservation preserves existing
storage and accounting, and that a very large configured limit does not wrap.

Compile-fail doctests reject safe raw conversions and use of a borrowed conversion
after moving GC. Runtime tests check owned conversion results after heap drop,
positive/negative/multiword values, small integers, and non-integers.

The first sandboxed runtime run failed only network tests because local sockets
were denied. The run with local socket access passed. Release build logs contain
an Xcode SDK cache/FSEvents environment warning; Rust checks are recorded below.

## Performance methodology

`bench_heap.py` extracts the actual BumpChunk and YoungGen implementations from
the pinned base `ab347d5` and the worktree, compiles both with `rustc -O`, and performs nine alternating
pairs after warmup. Each sample reserves twenty million two-word allocations,
including chunk growth. This isolates allocator overhead; it is not an Erlang
application benchmark and does not measure GC copying or bigint conversion.

An initial implementation using Vec length caused a large regression in this
microbenchmark (raw samples retained). The final version retains capacity-based
allocation and separates growth from the common path. Treat small differences
as measurement noise, especially while other compilation is active. No universal
throughput guarantee is inferred from this microbenchmark. Borrowed wrappers
are inline and contain no runtime state beyond the existing term word.

## Final results

- Workspace: 1,798 passed, zero failed, one ignored; then the final allocator
  implementation was retested across all 477 runtime unit tests in both debug
  and release (one ignored), and 11 documentation tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo build --release --locked`: passed.
- Targeted Erlang suite: 15 passed, zero failed/skipped. Command:
  `nera test 'bigint_*' 'arith_*' inline_arith 'bif_round*' 'ets_counter*' ets_gc_survival gc_receive_compact integer_source_domain io_lib_integer_format base_n_integer -j 4 --timeout 30`.
- `git diff --check`: passed.

Final allocator medians: 20.481 ms before / 21.024 ms after (**+2.65%**);
repeat: 16.651 ms before / 16.328 ms after (**−1.94%**). Each series contains
nine alternating pairs. There is no reproducible slowdown in these two series,
but the system was busy with indexing/synchronization, and compilation overlapped
some measurements. This is not evidence of a controlled idle-machine result.

No ABI layout, locking, conversion allocation count or moving-GC behavior changes.
The Rust API change intentionally requires unsafe callers to establish lifetimes.
The complete Erlang suite, remote CI, Linux/Windows, Miri and sanitizers were not
run at the local validation snapshot. The manifest records that snapshot before
publication.
