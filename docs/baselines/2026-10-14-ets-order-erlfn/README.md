# Exact ETS key ordering and unsafe compiled function calls

Base: main after PR #80, `514b35b030dc5415e39a48aedf8f44b12183404b`.
Worktree: `/tmp/nera-transferable-heap-fragments`, `mr/ets-order-erlfn-contract`.
macOS ARM64, pinned Rust 1.99.0, unchanged Cargo.lock. Development checks below ran in the uncommitted worktree before publication.

## Contracts

OrdTermKey's PartialEq now uses the same comparison as Ord. Ordered numeric keys
such as integer 1 and float 1.0 therefore agree about equality, including within
structural keys. The exact serialized eq_key remains for replacement/update
bookkeeping. BTreeMap lookup still uses its existing comparison path; no new
serialization or comparison pass is introduced there.

compare_numeric returns Ordering, with a compatibility cast at compare_terms'
existing integer-valued internal interface. Mixed numeric comparisons never
round an integer to f64. Small integers compare directly, without BigInt
allocation. Small integer/float comparisons bound the float before truncation
and distinguish any remaining fraction. Big integer/float comparisons use
sign and bit length first; when magnitudes overlap, the IEEE mantissa's virtual
words compare directly against the integer digits. No second BigInt is created
for the float. Decoding a boxed integer to its existing owned BigInt still
allocates as before. Signed zeros remain numerically equal; raw Rust NaNs have
a deterministic order consistent between mixed and float-only comparisons.
No new NaN-producing Erlang API is introduced.

ErlFn is unsafe extern "C" fn, and nera_runtime_start uses that shared alias.
Its contract covers a live exclusive context, readable valid arguments with
matching function arity/captured environment, rooting before GC, reentrancy and
code lifetime. Entry, continuation, apply and closure calls use explicit unsafe
blocks; continuation tests are adapted to the same contract. The qualifier
adds no runtime validation or dispatch machinery and does not change pointer
size or the machine calling convention. ABI compatibility was not a constraint
for this preview change, but no machine ABI change was necessary.

## Regression evidence

RED numeric_order tests fail on the original comparator: 2^53 + 1 compares
Equal to float 2^53, and Eq disagrees with Ord for integer 1 versus float 1.0.
The compile-fail ErlFn example compiles on the old safe function type, so that
test also fails as intended. Final focused tests cover precision, negative and
fractional cases, signed zero, i64 boundaries, huge integers versus infinities,
subnormal floats, ordering laws and large mantissas across limb boundaries up
to the largest finite exponent. The mantissa tests use num-bigint's independent
float-to-integer conversion as an oracle for exact values and their neighbours.

ets_numeric_order.erl checks actual compiled comparisons, equal numeric key
replacement, distinct neighbouring keys, traversal, lookup and deletion.

## Performance

Before is compiled in a clean detached checkout of the base above; after is
compiled from the corrected worktree. build_probe.py temporarily includes the
appropriate probe inside process.rs and restores its original bytes. Saved
executables use the same Cargo target, locked dependencies and release profile.
No probe module remains in runtime sources.

```
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target python3 docs/baselines/2026-10-14-ets-order-erlfn/build_probe.py /tmp/nera-order-before-tree before /tmp/nera-order-before
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-transfer-target python3 docs/baselines/2026-10-14-ets-order-erlfn/build_probe.py /tmp/nera-transferable-heap-fragments after /tmp/nera-order-after
python3 docs/baselines/2026-10-14-ets-order-erlfn/measure.py /tmp/nera-order-before /tmp/nera-order-after
```

Each of three series warms both binaries and alternates nine before/after
pairs. Comparison probes perform 100,000 black_box calls each. ordered_set
creates 100 tables and inserts/queries 128 small-integer keys per table, then
deletes the table. These are targeted microbenchmarks, not application-wide
throughput measurements. Raw elapsed microseconds and process CPU time are
retained in series-*.json.

| Median elapsed change | Series 1 | Series 2 | Series 3 |
| --- | ---: | ---: | ---: |
| Two small integers | −97.76% | −97.91% | −97.56% |
| Small integer / float | −92.46% | −92.62% | −91.80% |
| Big integer / distant float | −8.94% | −3.05% | −6.54% |
| Big integer / equal large float | −2.54% | +11.51% | +1.47% |
| Small-key ordered_set scenario | −86.25% | −87.03% | −86.35% |
| Whole probe CPU time | −67.50% | −65.28% | −67.40% |

The improvement on common integer paths is consistent with removing repeated
BigInt allocation/conversion. Close big integer/float timing is inconclusive:
results vary from −2.54% to +11.51%, so a small cost cannot be excluded for this
exactness path. It adds no float-integer allocation. Host load averages ranged
from 3.72 / 3.32 / 4.07 to 7.89 / 5.32 / 4.67 during work; tests and builds also
ran on the machine. Do not claim an idle-host measurement, zero cost for every
comparison, or application-wide speedups from these numbers.

## Final validation

All checks below use RUSTC_WRAPPER= and
CARGO_TARGET_DIR=/tmp/nera-transfer-target, with --locked:

- Workspace tests (--workspace --no-fail-fast): **1,922 passed**, one existing
  ignored Rust test.
- Runtime release tests: **597 passed** (559 unit, three compilation integration
  tests, 35 doctests), one existing ignored Rust test.
- Workspace/all-target Clippy with -D warnings: passed.
- Optimized release build: passed.
- Native validation: **74 passed**, zero failed, two external onecached scenarios
  skipped out of 76 selected. Includes the new ets_numeric_order test, all ets_*,
  bigint_*, arith_*, guard_*, closure_*, fun_closure, recursive_closure,
  tail_call_closure_wrapper, receive_*, nested_receive, base64*, hibernate_* and
  spawn_monitor_basic. Uses -j 1 --timeout 30.
- Final diff/whitespace check and source fingerprint verification: passed.
  source-hashes.json records the changed production/tests/changelog and locked
  build inputs. No temporary probe module remains in process.rs.

The temporary clean before checkout was removed after measurement. Recreate it
with git worktree add --detach /tmp/nera-order-before-tree
514b35b030dc5415e39a48aedf8f44b12183404b before running the commands above.

Validation covers macOS ARM64 only. The full native suite, Linux/Windows,
cross-compilation and installed archives were not checked. CI/Release remain
paused per CONTRIBUTING.md; no workflows were enabled or rerun. Independent
review and full merge checks remain required before a future merge.

BASE_REF is the fetched main revision above. HEAD has zero committed divergence
from that base; TARGET is the corrected uncommitted worktree. The unchanged
primary checkout is outside the scope of this work. These development checks precede commit, push and PR creation. No merge has
been performed.


## Publication integration

Before publication, rebase onto main after PR #81 (`09d7a37`). The upstream
change corrects the scoped Cowlib integration-test comparator and its evidence;
it changes no Rust code or Cargo/toolchain inputs. Every previously validated
Rust/test/build-input fingerprint still matches source-hashes.json; only the
changelog also gains main's milestone. publication-source-hashes.json records
the combined source state. Cowlib runner unit checks are rerun after integration
(publication-cowlib-tests.txt). The Rust checks and all three performance series
above remain evidence for the unchanged Rust implementation on the #80 base;
no fresh combined performance series or full Cowlib integration run is claimed.
