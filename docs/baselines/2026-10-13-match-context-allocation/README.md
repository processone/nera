# Binary match-context allocation — 2026-10-13

Base: `9e2ddcebaf725e6ea649ffcaafa98ccc682986cc` on `main`. The change is in the working tree; no commit or publication has been performed.

## Confirmed cause

Generated `BsMatchStart2` uses a fast path for fresh binaries with inline capacity, and one shared slow path for three different cases: capacity exhaustion, an existing match context, or non-binary input. `nera_rt_bs_start_match2` already reuses an existing context without allocating. Its wrapper `nera_rt_bs_start_match2_slow` nevertheless unconditionally reserved a new inline region and replaced `heap_cursor`/`heap_limit`.

Those regions grow from 512 bytes to 4 KiB. Every recursive reuse therefore abandoned the remaining capacity, charged it to the young heap, and increased arena growth and GC work. Before implementation, a focused test measured 4,189,864 heap bytes after 1,024 reuses versus 680 bytes before: no new match context or output object was needed. Invalid-input rejection also wasted capacity (512 → 3,584 bytes in the test). Both assertions failed on the original implementation and pass after the fix.

The fix replenishes inline capacity only when the underlying operation returned a distinct newly allocated match context. Reuse returns the original term; rejection returns zero. No GC runs during the helper, making raw term identity stable. The exhausted-region path for fresh binaries/bitstrings keeps its previous behavior. No compiler IR, stdlib algorithm or representation changes are needed.

## Controlled before/after measurements

Measured on macOS arm64 with Rust 1.99.0, release/LTO, one scheduler worker. Both versions run the same compiled Erlang code. Timing builds have no counters; separate instrumented builds count allocation traffic. Two complete runs per version in before/after/after/before order, seven samples each: medians over 14 per-call samples. Each block processes about 256 KiB, at least four iterations. Inputs and result checksums are identical.

At 65,538 raw bytes, standard Base64 with padding:

| Operation | Before | After | Speedup | Global allocation traffic before → after | Process-heap reservations before → after |
|---|---:|---:|---:|---:|---:|
| encode | 19.371 ms | 5.503 ms | 3.52× | 265,346,312 → 5,333,320 B | 90,128,352 → 1,056,680 B |
| decode | 23.041 ms | 21.165 ms | 1.09× | 30,419,670 → 30,419,670 B | 5,289,868 → 5,289,868 B |
| scan | 3.033 ms | 1.082 ms | 2.80× | 567,288,436 → 13,572 B | 268,295,936 → 3,096 B |

The decoder has **identical allocation counts/bytes** before and after; its timing variation is not evidence of an allocation improvement. The scan sums every input byte and produces only an integer: its improvement demonstrates that the fix benefits binary consumers independently of Base64.

## Where the bytes go

- Encoder before: 21,845 region refills per call reserve 89,429,248 bytes, explaining **99.2%** of the 90,128,352 charged heap bytes. After: about 92 refills and 356,096 reserved region bytes per call; these now serve actual inline list allocations.
- The global allocator counts include backing-arena capacity growth and GC allocations. Allocations observed inside minor/major GC decrease from about 100.7 MB to 3.6 MB per encoding call. GC uses a forwarding hash table sized from charged heap bytes and allocates copying arenas/new young-generation storage, so unused reservations amplify the cost beyond their own byte count.
- The remaining encoder heap traffic is consistent with its algorithm: 21,846 four-byte output fragments (16-byte heap binaries), plus the accumulator list and its reverse (two 16-byte cons cells per fragment). That is about 1,048,608 bytes before small region/dispatch overhead, close to the measured 1,056,680 bytes.
- Decoder: `binary_to_list` plus the symbol accumulator and its reverse construct roughly three list cells per encoded byte. Output fragments and their two lists add further cells. At this size, `48 × 87,384 + 48 × 21,846 = 5,243,040` logical bytes, close to the 5,289,868 measured heap reservations. About 20.5 MB of its 30.4 MB global traffic occurs inside GC. That remaining cost is not the redundant match-context refill defect.

All figures are **cumulative requested/reserved bytes, not peak or live memory**. Global allocator and heap counters overlap and must not be added. Global reallocations count the new requested size. Heap counters include inline-region reservations but not GC-copier object allocations; GC counters are a subset of global allocator traffic. The instrumented runtime is confined to a disposable workspace.

## Compatibility and known boundary

- The Base64 differential corpus (more than 78,000 comparisons) and the existing Base64 option/error tests pass before and after, in both timing and instrumented builds.
- A new compiled regression interleaves recursive binary matching, tuple/cons allocation, GC/yield pressure, retained input, process spawning and messaging. It passes on both versions.
- Extended probing discovered an independent pre-existing sub-byte cursor defect: `bit_offset_probe.erl` sums `<<1:1,0:1,1:1,1:1>>` to 3 on BEAM but 4 on both original and fixed Nera. The original failure, focused probe and outputs are preserved as `known-bit-offset-*.log`. It is not part of the allocation fix and has not been silently treated as a pass.

## Reproduction and evidence

Run the prototype harness once if its isolated compiler/workspace is unavailable, then:

```sh
python3 docs/baselines/2026-10-13-match-context-allocation/measure.py
```

`measure.py` copies the preserved baseline workspace, uses the baseline allocation adapter from Git and the working-tree adapter for the fixed variant, builds independent executables, validates all result checksums, and records source/runtime/executable hashes. It does not modify the original prototype workspace, stdlib or compiler sources. It reuses `target/base64-prototype` for Cargo artifacts.

Raw measurements, all nine size/operation comparisons and binary hashes are under `measurements/`. `red.log` and `green.log` record the focused regression. `validation.json` records the full local checks and exit codes; see `preflight.json` for the final scoped files, hashes, omissions and known boundary.

## Final local validation

- Release build and workspace Clippy with all targets/warnings denied: pass.
- Full Rust workspace and doctests: 1,831 passed, one ignored release-only scaling benchmark.
  The first restricted run failed 10 socket/reactor tests with `EPERM`; the
  full rerun with local-network access passed.
- Erlang suite: 310 non-network passes; all nine TCP tests initially failed
  with socket permission errors and pass on the explicit network-enabled rerun.
  Combined coverage is 319 passed and two pre-existing external-OneCached skips.
- The new compiled regression passes on BEAM as well as baseline/fixed Nera.
- The separate sub-byte semantic gap described above remains reproducible.
- Linux, Windows, archive installation and long-running concurrent-service
  performance were not tested. No remote CI or publication was requested.
