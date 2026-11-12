# ETS ownership and atomic element updates — 2026-10-11

Base: `a53877f` (PR #8 merged). Worktree: `mr/ets-ownership-atomicity`.
This change covers ordered key/value replacement and `ets_update_element`;
it does not claim to audit the atomicity of every other ETS operation.

## Correctness

`BTreeMap::insert` replaces a value but retains the existing equal key. A boxed
comparison key could therefore point into a dropped stored tuple. Ordered
insertion now uses Entry: vacant entries insert directly, occupied immediate
keys can keep their key, and occupied boxed keys are removed with their values
before the replacement pair is inserted. No separate key Arc or obsolete tuple
is retained solely to keep a comparison key alive.

Element updates hold the storage write lock through read/modify/copy/replace.
The original tuple remains owned throughout copying, concurrent updates cannot
overwrite untouched fields with stale values, and a concurrent delete cannot be
followed by reinsertion of a stale snapshot. Boxed ordered keys are replaced as
complete pairs. The lock order is storage then pool; ordinary insertion releases
its pool lock before taking the storage lock.

When old and new updated fields are immediate, the update changes only the
stored tuple's private element slots. No backing heap allocation, copy or pool
reclamation scan is needed. Boxed-to-immediate changes rebuild the heap so old
binary ownership can be released. Published lookup tuples and backing words are
never modified.

## Regression evidence

Before implementation:

- `ordered_replacement_keeps_comparison_key_alive` failed its owner-lifetime
  assertion. It checks a weak owner before any potentially dangling key access.
- `concurrent_element_updates_preserve_other_fields` detected 204 lost field
  updates in one run. This counterexample uses immediate terms only, avoiding
  invalid memory access on the old implementation.

The final tests additionally check that an update cannot resurrect a concurrently
deleted entry, and that nested lookup snapshots survive immediate and boxed
updates followed by table clear/delete and full GC. Both set and ordered_set
paths are exercised.

## Performance

macOS ARM64, Rust 1.99.0, release runtime archives. The same compiler executable
builds both benchmark executables, each linked against its respective runtime.
The fixture is `bench_ets_updates.erl`. Run the compiler from an isolated directory
to avoid project discovery, with its runtime archive beside it:

```sh
/path/to/compiler/nera build /path/to/bench_ets_updates.erl \
  -o /tmp/bench_ets_updates -e bench_ets_updates:start
```

Each sequential phase uses 100,000 entries with tuple keys and nested values.
Concurrent phases use four different immediate keys, so the baseline does not
race two updates on the same entry: 400,000 total immediate updates, or 100,000
total boxed updates. `lookup` snapshots retain all original entry heaps through
the sequential phases so the old ordered-key bug does not invalidate baseline
measurements. `tab2list` is insufficient because it copies data independently.
An exploratory benchmark using tab2list crashed the old runtime; it is not used
for the timing comparison.

After builds finished, warm both executables once, then run seven alternating
pairs with no validation workload running concurrently. Internal microsecond
measurements exclude startup. `benchmarks.json` contains every sample and stdout;
the table shows medians in milliseconds. These are local samples, not a universal
performance guarantee.

| Phase | Before (ms) | After (ms) | Change |
| --- | ---: | ---: | ---: |
| set insert | 29.768 | 29.650 | -0.4% |
| set overwrite | 46.669 | 45.222 | -3.1% |
| set immediate | 35.824 | 20.141 | -43.8% |
| set boxed | 35.966 | 32.374 | -10.0% |
| set lookup | 39.641 | 39.541 | -0.3% |
| set concurrent | 115.000 | 58.968 | -48.7% |
| set concurrent_boxed | 32.617 | 26.912 | -17.5% |
| ordered_set insert | 46.130 | 46.044 | -0.2% |
| ordered_set overwrite | 118.562 | 98.353 | -17.0% |
| ordered_set immediate | 191.144 | 72.873 | -61.9% |
| ordered_set boxed | 191.797 | 86.009 | -55.2% |
| ordered_set lookup | 112.452 | 113.588 | +1.0% |
| ordered_set concurrent | 140.177 | 63.939 | -54.4% |
| ordered_set concurrent_boxed | 38.312 | 30.766 | -19.7% |

## Final validation

- `RUSTC_WRAPPER= cargo test --workspace --locked`: 1,752 passed.
- Final runtime-specific run: 455 unit tests and 6 doctests passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo build --release --workspace --locked`: passed.
- `nera test -j 1 --timeout 30`: 286 passed, zero failures, two skipped for
  absent external onecached modules.
- `git diff --check`: passed.

Builds used the shared Cargo target cache with sccache disabled. A private copy
of the final compiler/runtime was used for the end-to-end suite. Benchmark and
end-to-end runtime archive hashes match. `validation.json` binds evidence to
source contents and records the base, branch and local log hashes. No commit,
push, PR, remote CI, cross-platform run or sanitizer/Miri run was performed.
