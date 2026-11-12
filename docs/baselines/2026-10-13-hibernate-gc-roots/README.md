# Hibernate retention and GC root contracts

Base: `676b99b35de387ae3a125d52e1249daa6cb65fc0` (merged PR #58).
Branch: `mr/hibernate-gc-roots`. Local validation snapshot, 2026-10-13,
macOS ARM64, pinned Rust 1.99 and dependency lock. Evidence captured before publication.

## Retained storage

Hibernation drops execution roots before collecting the state that survives:
mailbox messages and dictionary keys/values. Heap graphs are compacted through
one full collection and their root locations are updated. The collector preserves
sharing between retained roots, shared binary owners and external heap owners
using its existing ownership bookkeeping. Borrowed-owner sweeping remains
conservative, as in ordinary GC. The default direct heap reset remains for empty
or immediate-only state; determining whether copying is needed reads term tags,
not pointed-to objects. Heap limits and fullsweep_after are preserved.

Pending hibernate MFA arguments are still raw thread-local terms until copied to
owned MessageFragments. The scheduler skips its normal post-slice collection for
STATUS_HIBERNATE, so those terms remain current until ownership is established.
Hibernation then handles reclamation itself, avoiding a redundant collection.
An already queued mailbox message also triggers the existing wake CAS after the
waiting transition; runnable accounting and inbox race checks remain in place.

## Unsafe boundary

ProcessHeap::collect/collect_full require current, initialized, complete roots,
owned storage and no competing access or live object borrows. Process fields
still expose raw terms, so a Process reference alone does not establish validity.
Process::collect_garbage/collect_garbage_full/hibernate/run_slice and Scheduler::run
therefore also require explicit unsafe contracts. Runtime start FFI adapters
propagate those guarantees from generated code; symbol names and C ABI do not
change. Rust callers must update their calls to acknowledge these preconditions.

This is a contract change, not dynamic pointer validation. There is no per-object
validity check in the collector and its copying algorithms are unchanged. The
scheduler's extra hibernate-status guard is evaluated only when GC is needed.
Existing BorrowedTerm compile-fail examples still test borrow conflicts rather
than merely failing on the newly unsafe collection call.

## Validation

- RED unit test: baseline hibernation dropped a binary owner still referenced by
  mailbox/dictionary roots. Ownership is checked through Weak before any read of
  freed term memory. See red-ownership.txt.
- RED compile-fail doctest: baseline collect compiled without unsafe, causing the
  new E0133 contract test to fail. See red-contract.txt.
- RED native: the main runtime rebuilt in the isolated target failed to resume a
  process with a queued retained message. See red-native.txt.
- Final isolated workspace: **1,847 passed, zero failed, one ignored**.
- Final release runtime: **503 unit tests + one compilation test + 22 doctests
  passed**, one existing ignored test.
- Final strict workspace/all-targets Clippy and full release build passed.
- **16 native Erlang scenarios passed**: hibernate_basic, hibernate_heap_args,
  hibernate_gc_args, hibernate_retained_roots, gc_receive_compact, process_dict,
  process_info, ets_gc_survival, ets_complex_terms, ets_message_passing,
  ets_delete_reader, ets_overwrite_reader, self_send, scheduler_yield,
  nested_dynamic_tail_call and tcp_active_once. Completion and EXPECT markers
  were checked by the repository runner. See erlang.txt.
- Tests cover young/old heap graphs, shared roots, refc and external owners,
  discarded execution data, immediate-only state, preserved heap settings,
  existing arguments and a large allocation immediately before hibernate.
- Whitespace checks passed; Cargo.lock unchanged.

The shared workspace target initially reused artifacts from another checkout,
including an unrelated IR instruction absent from this branch, and native
execution used an incorrect runtime. Those outcomes are not final validation.
All workspace crates were rebuilt in `/tmp/nera-hibernate-target`, isolated from
other producers; only unchanged third-party dependency caches were reused.
Pinned copies of compiler, archive and probe prevent later cache changes from
altering native tests or measurements. Source/artifact hashes identify the final
snapshot. The baseline archive/rlib was rebuilt in the same worktree path by
saving all edited Rust files, restoring exact HEAD contents, then restoring the
saved files in a finally block. No Git reset or unrelated checkout edit was used.

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-hibernate-target cargo test --workspace --locked --no-fail-fast
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-hibernate-target cargo test -p nera-runtime --release --locked
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-hibernate-target cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/tmp/nera-hibernate-target cargo build --release --locked
/path/to/pinned/nera test 'hibernate_*' gc_receive_compact process_dict process_info ets_gc_survival ets_complex_terms ets_message_passing ets_delete_reader ets_overwrite_reader self_send scheduler_yield nested_dynamic_tail_call tcp_active_once -j 1 --timeout 30
```

The full Erlang suite, Linux/Windows/32-bit execution, Miri, sanitizers and remote
CI were not run. Repository CI/Release workflows remain suspended. The full
native suite and independent review remain required before merging during
the pause.

## Performance

`probe.rs` uses one million operations per workload, with identical -O rustc
commands and isolated dependency search paths, linking pinned before/after
release runtime rlibs. The comparable workloads are empty-state hibernation,
immediate-only mailbox/dictionary hibernation and ordinary full GC. Each series
has one warmup and nine alternating before/after pairs. Elapsed microseconds
cover each workload and its assertions; process CPU includes all three workloads
and startup. Compiler builds and tests had finished before measurement.

```sh
rustc --edition=2021 -O probe.rs -L dependency=/tmp/nera-hibernate-target/release/deps --extern nera_runtime=/path/to/pinned/libnera_runtime.rlib -o probe
python3 measure.py BEFORE_PROBE AFTER_PROBE > results.json
./AFTER_PROBE retained
```

For retained heap graphs, the old implementation drops live storage, so it is
not a valid equivalent baseline. The after-only `retained` workload repeatedly
hibernates one shared two-element tuple containing a 256-byte refc binary, rooted
in both mailbox and dictionary. It checks sharing and binary contents afterward.
One warmup and nine samples give the cost of the corrected operation, rather
than claiming a speed comparison with the invalid baseline.

| Series | Metric | Before µs | After µs | Change |
| --- | --- | ---: | ---: | ---: |
| results.json | empty | 29,707 | 28,634 | -3.61% |
| results.json | immediate | 29,859 | 29,585 | -0.92% |
| results.json | gc | 94,319 | 93,233 | -1.15% |
| results.json | process_cpu_us | 156,072 | 153,409 | -1.71% |
| results-repeat.json | empty | 31,208 | 29,712 | -4.79% |
| results-repeat.json | immediate | 31,028 | 30,755 | -0.88% |
| results-repeat.json | gc | 97,975 | 96,967 | -1.03% |
| results-repeat.json | process_cpu_us | 162,469 | 159,771 | -1.66% |

Corrected retained-graph hibernation: **0.197 µs per call**
for this small graph (197,131 µs per million calls). Cost scales with
retained graph size; this is not a general application throughput guarantee.

No material regression was observed on the comparable workloads. The machine
was shared and loaded (3.86/6.46/7.60 before measurements, 1.98/5.02/6.90 afterward).
The observed improvements are not reliable gains or proof of zero overhead.
