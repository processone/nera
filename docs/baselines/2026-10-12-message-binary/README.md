# Message fragments and binary size contracts

Work started 2026-10-12; final validation and measurements 2026-10-13.
Base: `fd364b50f881d273594a5e2e51c7e158aa0ef91b` (main at worktree creation).
Branch: `mr/message-binary-contracts`; local, uncommitted validation snapshot.

## Construction contracts

HeapFragment's heap and root fields are private. Internal ownership-transfer
construction explicitly requires an unsafe guarantee that the complete root
graph belongs to the transferred heap. Public MessageFragment variants use
validated ImmediateTerm, InlineArity and BinarySlice components, so ordinary
safe construction cannot pair an invalid heap root, embed a heap pointer as an
immediate, exceed the inline array capacity or forge a binary byte window.
ActiveReadReady retains its existing control-message semantics.

BinarySlice owns its existing Arc and validates offset/length using subtraction,
plus the boxed-header bit-length domain. It allocates no additional storage.
A crate-private unsafe constructor reuses the validity guarantees of existing
runtime descriptors, avoiding bounds revalidation during every message transfer.
Its contract includes valid byte windows and representable bit length. Dereferencing
raw term graphs still requires live initialized storage and absence of concurrent
mutation. BorrowedTerm::to_message_fragment offers a safe conversion once such a
view exists; the fragment independently owns its storage afterward.

The receiver consumes validated windows without another range check or Arc clone.
Small tuples retain their fixed array and stack allocation. On macOS ARM64,
MessageFragment remains 56 bytes and FragmentElem remains 24 bytes. These are
observations, not stable layout contracts. BinaryData's repr(C) prefix and runtime
binary descriptor layout remain unchanged.

## Binary size planning

BoxedHeader::new rejects arities above its 60-bit domain in every build;
try_new offers a fallible encoding. Subtype masking retains its prior behavior.
Binary allocation validates byte length against both header bit capacity and the
aligned isize allocation bound before touching the heap. Reserving one word for
the header proves subsequent ceiling division, addition and padding multiplication
cannot overflow, including on 32-bit targets. The two construction helpers inline
to keep the existing frequent paths efficient.

## Validation

- RED: oversized header arity was accepted by the baseline in release;
  red-header-release.txt preserves the failure. The compile-fail fixture for
  public HeapFragment field construction also compiled on the baseline;
  see red-fragment-api.txt. Both regressions pass on the final code.
- Final full Rust workspace: **1,829 passed**, zero failed, one ignored.
- Final release runtime: **496 unit tests + 15 doctests passed**, one ignored.
- Final workspace/all-targets Clippy with warnings denied passed.
- Final release runtime build passed.
- **26 native Erlang scenarios passed**: all binary_* fixtures plus
  message_fragment_paths, tcp_active_once, monitor_exit_reason,
  hibernate_heap_args and self_send. Expected output follows the runner's
  first EXPECT marker; the advanced binary fixture's explanatory comment is
  not treated as a second output requirement.
- Tests cover invalid/overflowing byte windows, empty end windows, backing
  lifetime after dropping the original owner, invalid immediate/inline tuple
  components, source-heap destruction after fragment creation, and oversized
  allocation rejection with retained heap content/accounting. The maximum
  accepted size is planned and checked against Layout without allocating it;
  the next size is rejected before any heap mutation.
- Diff whitespace check passed.

Execution was on macOS ARM64. Full Erlang suite, Linux/Windows/32-bit execution,
Miri, sanitizers and remote CI were not run. No commit, push or PR was created.

## Performance

The same rustc and -O compile the identical Rust probe against pinned before/
after release runtime rlibs. It performs one million 63-byte zeroed binary
constructions and one million owned fragment transfers of a two-element tuple
with an immediate and 256-byte refc binary. Work is grouped in fresh heaps of
1,000 operations. Each message batch verifies the copied integer and bytes.
The compiler/runtime build work and tests had finished before the final samples.

```sh
cargo build --release --locked -p nera-runtime
rustc --edition=2021 -O probe.rs -L dependency=/path/to/target/release/deps --extern nera_runtime=/path/to/libnera_runtime.rlib -o probe
python3 measure.py /path/to/before/probe /path/to/after/probe > results.json
```

Each series has one warmup and nine alternating before/after pairs. Elapsed
microseconds measure each loop; whole-process CPU includes both loops and
startup. results-short*.json retain earlier 200,000-operation samples and are
not the final probe configuration.

| Series | Metric | Before µs | After µs | Change |
| --- | --- | ---: | ---: | ---: |
| results.json | binary | 3,752 | 3,783 | +0.83% |
| results.json | message | 18,139 | 18,278 | +0.77% |
| results.json | process_cpu_us | 23,697 | 23,978 | +1.19% |
| results-repeat.json | binary | 3,742 | 3,763 | +0.56% |
| results-repeat.json | message | 18,013 | 18,256 | +1.35% |
| results-repeat.json | process_cpu_us | 23,570 | 23,793 | +0.95% |

The final samples show approximately 1% CPU overhead, with binary construction
elapsed up 0.6–0.8% and message transfer elapsed up 0.8–1.3%. This does not prove
zero overhead. The shared machine was not controlled/idle (observed load averages
4.38/10.79/10.79), so precise small differences should be confirmed on an idle
machine. This is a focused Rust runtime probe, not an application message-
throughput guarantee. No allocator instrumentation was used; absence of extra
fragment allocations/refcounts is established from the source change.
