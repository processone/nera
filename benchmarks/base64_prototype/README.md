# Base64 Rust prototype for Nera

This is an opt-in experiment, not a replacement of the shipped stdlib. `run.py`
copies the current compiler/runtime sources into a temporary workspace, registers
private benchmark BIFs there, and builds into `target/base64-prototype`. It never
edits the original checkout. Since production encoding also uses Rust, the
sandbox restores `base64_reference.erl` (the pre-integration Erlang implementation)
as its `base64` module. Historical rows labelled `base64` therefore still measure
compiled Erlang, not the resumable production encoder.

Production value/error and BEAM regression comparisons can be run separately:

```sh
python3 scripts/test-base64.py --nera /absolute/path/to/nera
```

Production encoding uses 48 KiB slices and adopts the final output allocation.
The prototype intentionally remains synchronous for a reproducible comparison.

Run from the repository root on a non-MSVC host with the pinned Rust toolchain:

```sh
python3 benchmarks/base64_prototype/run.py
```

To preserve a previous run, select a separate result directory:

```sh
python3 benchmarks/base64_prototype/run.py --output benchmarks/base64_prototype/results-corrected
```

For a custom directory, its generated `RESULTS.md` stays inside that directory.
The original measurements remain in `results/` and the original `RESULTS.md`.

The temporary workspace is retained for inspection. Generated executables and
Cargo artifacts stay outside this directory. Results and raw logs are saved in
`results/`. The input manifest binds the experiment to actual source bytes,
including any uncommitted changes, rather than assuming HEAD is the source.

## Recorded runs

- [Original experiment](RESULTS.md): before the match-context fixes.
- [Corrected-source comparison](results-corrected/COMPARISON.md): both fixes on
  the original baseline, with all size/option results in its `RESULTS.md`.
- [Publication-base validation](results-publication/RESULTS.md): the fixes on
  updated main, including the intervening GC and scheduler changes. This is the
  source snapshot used to prepare the two PRs.

Each directory retains its own input manifest, executable hashes, raw rows and
summary. Historical statements about working-tree/publication status describe
when that run was recorded. Comparing separate runs does not isolate the effect
of one individual compiler or runtime change.

Trailing empty lines in captured logs are removed for Git whitespace checks;
log hashes are refreshed after that normalization, without changing output lines.

## Design

- `codec.rs`: safe scalar Rust codec, no external codec dependency and no SIMD.
- `adapter.rs`: small unsafe bridge to Nera terms and heap ownership. It borrows
  input only while GC cannot run. Large output vectors are adopted by
  `Arc<BinaryData>` without another byte-buffer copy; small results are copied
  into an inline binary.
- `base64_rust.erl`: binary fast path. Lists and unsupported types use the
  existing Erlang implementation. Invalid binary encodings return `fallback`
  and are evaluated by the reference, preserving exception class and reason.
- `differential.erl`: compares successful values and exception class/reason to
  the current Nera stdlib compiled by Nera in the same executable. Stack traces
  are intentionally not compared: the additional wrapper changes call frames.

The decoder preserves permissive behavior of this reference: ignored whitespace,
nonzero unused tail bits, optional missing padding and the partial `Zg=` tail
when padding is disabled. Invalid mode/padding options retain the reference's
existing interpretation. This is not a claim of complete OTP Base64 compatibility.

## Measurement

The benchmark warms both implementations, alternates their order across seven
repetitions, and consumes every result with a byte-size checksum. Both paths use
identical dynamic dispatch, input data, options and loop structure. Inputs are
12, 1,026, 16,386 and 65,538 raw bytes, with standard/padded and URL/unpadded modes.
These sizes are multiples of three; tail behavior is covered by correctness
tests, not the throughput table. Each Erlang block processes about 1 MiB of
raw data, with at least four iterations. Rust blocks use 32 times as many calls
to avoid sub-millisecond samples and exercise sustained allocation/GC behavior.
All results are normalized per operation. Timing excludes startup and input
construction, but includes dispatch, wrapper, codec, result allocation and GC.

Both experimental builds use **one scheduler worker**, making thread-local
allocation accounting follow the measured process. This is a controlled
single-process throughput benchmark, not a concurrent-service latency test.

There are two separate release builds with the repository's LTO settings:

1. Timing: allocation measurement functions are no-ops.
2. Allocation traffic: a nonallocating thread-local wrapper counts calls to the
   global Rust allocator (`alloc`, `alloc_zeroed`, `realloc`) and their requested
   sizes. Another counter records calls/bytes requested through
   `ProcessHeap::alloc_words`, including inline-region reservations.

These are cumulative allocation traffic, **not live memory or RSS**. Reallocation
counts the full new requested size. Heap bytes are reserved/charged words, not an
exact logical Erlang-object census: inline allocation reserves chunks, and GC
copying has its own allocator. Global traffic includes measured-loop/runtime/GC
work on the worker; it excludes other threads and allocations that bypass the
Rust global allocator. The two byte counters overlap and must not be added.
Allocation-build durations are never used to calculate speedups.

## Boundaries

The native codec does not yield while processing a binary. Before promotion to
production, it needs a bounded-work or resumable scheduling design and concurrent
latency tests, plus resource-limit/very-large-input testing. Arithmetic capacity
checks prevent size wrapping but do not provide recoverable OOM behavior.
Malformed binaries can incur a partial native decode before Erlang fallback;
list inputs intentionally receive no speedup. This experiment establishes neither
cross-platform performance nor universal error-path performance.
