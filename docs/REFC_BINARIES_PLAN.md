# Shared Binaries (Refc Binaries) — Implementation Plan

Large binaries (>64 bytes) should use reference-counted shared backing
storage instead of deep copy, matching BEAM's refc binary design.  Binaries
are immutable in Erlang, making them natural candidates for cross-process
sharing (equivalent to Pony's `val` reference capability).

This document describes the phased plan, starting with prerequisites that
harden the current binary path before introducing shared storage.

---

## Phase 0 — Prerequisites ✅ DONE

### 0.1  Cross-process binary message tests ✅

Tests added in `tests/`.

### 0.2  Unify `binary_data()` accessor ✅

`binary_bytes_raw()` and `refc_binary_bytes()` in `term.rs` handle both
heap and refc binaries.  All FFI functions use the unified accessor.

---

## Phase 1 — Backing storage & refcount system ✅ DONE

**Implementation note:** The plan originally specified `AtomicU64` in a raw
`RefcBinaryBacking` struct.  The actual implementation uses `Arc<BinaryData>`
— Rust's `Arc` provides the same atomic reference counting with less
unsafe code.  On-heap descriptors store `Arc::as_ptr()` as the raw pointer
word; `refc_binaries: Vec<Arc<BinaryData>>` on `ProcessHeap` anchors
ownership so the bump arena's lack of `Drop` doesn't leak backing data.

**2026-10-11 layout update:** `BinaryData` owns the Vec and adds a
`#[repr(C)]` data-pointer prefix. Generated code reads this explicit prefix
instead of assuming a field offset inside Vec. Moving in a buffer does not
copy its bytes; Arc sharing and three-word heap descriptors are unchanged.
The allocator rejects invalid offset/length windows before writing a descriptor.

### 1.1  Backing storage type ✅

`Arc<BinaryData>` allocated outside process heaps.  Raw pointer stored as
`refc_ptr` (word 1) in the 3-word heap descriptor.

### 1.2  Allocation threshold ✅

```rust
pub const REFC_BINARY_THRESHOLD: usize = 64; // bytes — matches BEAM
```

`alloc_binary_auto()` routes to inline heap binary (≤64) or refc (>64).

### 1.3  Construction flow ✅

`alloc_binary_auto()` and `alloc_refc_binary_from_arc()` handle the
promote path.  I/O results and message receive use
`alloc_refc_binary_from_arc()` directly.

---

## Phase 2 — Refcount integration points ✅ DONE

### 2.1  Message passing (`message.rs`) ✅

`deep_copy_term()` clones the `Arc` (atomic refcount increment) and calls
`alloc_refc_binary_from_arc()` on the target heap.  No data bytes copied.

### 2.2  Garbage collection (`heap.rs`) ✅

Implemented via option 1 (tracking list): `refc_binaries: Vec<Arc<BinaryData>>`
on `ProcessHeap`.  After the copy phase, `retain()` drops Arcs whose raw
pointer was not seen in a live descriptor (`live_refc_ptrs: HashSet<u64>`).
Dropped Arcs decrement their strong count; last reference frees the backing.

### 2.3  Process exit cleanup ✅

`ProcessHeap::drop` (via `refc_binaries` Vec drop) decrements all Arcs.
Process destruction at the 4 exit points drops the heap, cleaning up
automatically.

---

## Phase 2.5 — Binary virtual heap for GC triggering ✅ DONE

**Status:** implemented.

**Why:** The current GC trigger only considers on-heap allocation:

```rust
pub fn needs_gc(&self) -> bool {
    self.bytes_allocated >= self.gc_threshold  // on-heap bytes only
}
```

A 3-word refc descriptor is 24 bytes on-heap, but it may reference
megabytes of off-heap data via `Arc<BinaryData>`.  A process receiving 100K
messages each containing a 10 KB binary accumulates ~24 bytes × 100K =
2.4 MB of descriptors (under the 256 KB default threshold after a few GC
cycles that double it) plus ~1 GB of off-heap data.  GC doesn't trigger
often enough to sweep stale Arcs, causing unbounded accumulation.

BEAM solves this with a "binary virtual heap" — a counter that tracks the
total byte-size of off-heap binaries owned by the process and contributes
to the GC trigger threshold.

**Measured impact:** `bench_binary_send` (100K messages × 10 KB binary)
shows ~34 ms overhead and ~9 MB extra RSS from descriptor/Arc accumulation
and slow `retain()` sweeps.

### 2.5.1  New fields on `ProcessHeap`

```rust
pub struct ProcessHeap {
    // ... existing fields ...

    /// Sum of byte-lengths of all refc binaries currently tracked by this
    /// process (in `refc_binaries`).  Approximates off-heap memory pressure.
    binary_vheap_size: usize,

    /// GC triggers when `binary_vheap_size >= binary_vheap_threshold`.
    binary_vheap_threshold: usize,
}
```

Default `binary_vheap_threshold`: match BEAM's initial value of
`BIN_VH_MIN_SIZE` (32K words = 256 KB on 64-bit).  Same ballpark as the
on-heap threshold.

### 2.5.2  Accounting — increment on allocation

In `alloc_refc_binary_from_arc()`, after pushing the Arc:

```rust
self.binary_vheap_size += data.len();  // data: &Arc<BinaryData>
```

This covers all entry points: `alloc_binary_auto()` (construction),
`deep_copy_term()` (message receive), and direct I/O results.

### 2.5.3  Trigger — extend `needs_gc()`

```rust
pub fn needs_gc(&self) -> bool {
    self.bytes_allocated >= self.gc_threshold
        || self.binary_vheap_size >= self.binary_vheap_threshold
}
```

### 2.5.4  Reset — after GC sweep

At the end of `collect()`, after `retain()` sweeps stale Arcs:

```rust
// Recalculate binary_vheap_size from surviving Arcs.
self.binary_vheap_size = self.refc_binaries.iter().map(|a| a.len()).sum();

// Grow threshold: same doubling heuristic as on-heap.
self.binary_vheap_threshold =
    std::cmp::max(self.binary_vheap_size * 2, DEFAULT_BINARY_VHEAP_THRESHOLD);
```

This ensures the threshold adapts: processes that legitimately hold many
large binaries get a higher threshold (fewer unnecessary GCs), while
processes that were just accumulating stale descriptors get collected
promptly.

### 2.5.5  Files changed

| File      | Change                                                                                             |
|-----------|----------------------------------------------------------------------------------------------------|
| `heap.rs` | Add fields, update `alloc_refc_binary_from_arc`, `needs_gc`, `collect`                             |
| —         | No changes needed in `message.rs` — it calls `alloc_refc_binary_from_arc` which handles accounting |

### 2.5.6  Tests

| Test                               | Location  | What it validates                                               |
|------------------------------------|-----------|-----------------------------------------------------------------|
| `binary_vheap_triggers_gc`         | `heap.rs` | Accumulating refc binaries triggers GC before on-heap threshold |
| `binary_vheap_threshold_grows`     | `heap.rs` | After GC with many surviving binaries, threshold doubles        |
| `binary_vheap_shrinks_after_sweep` | `heap.rs` | After GC drops stale binaries, vheap_size decreases             |

End-to-end: re-run `bench_binary_send` and verify RSS reduction.

---

## Phase 3 — Sub-binaries ✅ DONE

A sub-binary is a view into an existing binary without copying data.
`binary_get_binary` (extracting `Rest/binary` from a match) is the primary
producer.

### 3.1  Sub-binary from heap binary

If the source is a heap binary and `Rest` is >64 bytes:
- Promote the source to refc (allocate backing, copy bytes once).
- Return a refc descriptor with `byte_offset` pointing into the backing.

If ≤64 bytes: just copy the bytes into a new heap binary (current behavior,
keep it).

### 3.2  Sub-binary from refc binary

Create a new 3-word descriptor with:
- Same `refc_ptr` as the source.
- `byte_offset = source.offset + extraction_offset`.
- `byte_len` (in header) = sub-range length.
- Increment refcount.

This is O(1) regardless of binary size.

### 3.3  `binary_bytes_raw()` for sub-binaries

```rust
fn binary_bytes_raw(term: ErlTerm) -> Option<(*const u8, usize)> {
    if term.is_heap_binary() {
        // existing inline path
    } else if term.is_refc_binary() {
        let backing = *header_ptr.add(REFC_BINARY_PTR_WORD_INDEX) as *const RefcBinaryBacking;
        let offset  = *header_ptr.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
        let len     = arity; // from header
        Some((backing.data_ptr().add(offset), len))
    }
}
```

All FFI functions that read binary data (get_integer, get_float,
get_binary, binary_size) automatically gain sub-binary support through this
single accessor.

---

## Phase 4 — I/O integration

The async I/O pool (`io_pool.rs`) is the main producer of large binaries
(file reads, TCP receives).  Currently, `wake_process_with_result()`
allocates the result as a heap binary.

**Change:** when the I/O result exceeds 64 bytes, allocate a
`RefcBinaryBacking` directly and write a 3-word descriptor onto the
process heap.  This avoids the promote-after-build copy for I/O results.

For sends/writes, data is already extracted into owned `Vec<u8>` before
submission — no change needed.

---

## Phase 5 — Tests

### Unit tests (Rust)

| Test                                 | Location          | What it validates                          |
|--------------------------------------|-------------------|--------------------------------------------|
| `alloc_refc_binary`                  | `heap.rs`         | >64 byte alloc creates refc descriptor     |
| `refc_binary_bytes_raw`              | `term.rs`         | accessor follows ptr + offset correctly    |
| `refc_binary_refcount_inc`           | `message.rs`      | deep_copy increments refcount              |
| `refc_binary_gc_preserves`           | `heap.rs`         | refc survives GC, refcount unchanged       |
| `refc_binary_gc_decrements`          | `heap.rs`         | unreachable refc decrements on GC          |
| `refc_binary_dealloc_on_zero`        | `heap.rs`         | backing freed when refcount hits 0         |
| `subbinary_from_refc`                | `heap.rs`         | sub-binary shares backing, offset correct  |
| `refc_binary_process_exit_cleanup`   | `scheduler test`  | process exit decrements all refcounts      |

### End-to-end tests (Erlang)

| Test file                   | What it validates                                  |
|-----------------------------|----------------------------------------------------|
| `binary_message.erl`        | Small binary survives cross-process send           |
| `binary_message_large.erl`  | 4 KiB binary survives cross-process send           |
| `binary_message_nested.erl` | Binary inside tuple/list survives send             |
| `binary_message_shared.erl` | `{B, B}` — both copies identical after send        |
| `binary_subbinary.erl`      | `<<_:N/binary, Rest/binary>>` works for large bins |
| `binary_refc_gc.erl`        | Force GC after receiving large binary, data intact |

---

## Ordering & Dependencies

```
Phase 0.1 (tests)               ✅ done
      │
Phase 0.2 (unified accessor)    ✅ done
      │
Phase 1 (backing + alloc)       ✅ done (Arc<BinaryData> instead of AtomicU64)
      │
Phase 2 (refcount in msg/GC/exit) ✅ done
      │
Phase 2.5 (binary virtual heap) ✅ done — GC triggers on off-heap binary pressure
      │
Phase 3 (sub-binaries)          ✅ done (alloc_refc_binary_from_arc with offset)
      │
Phase 4 (I/O integration)       ← optimization, avoids extra copy for I/O results
      │
Phase 5 (full test suite)       ← runs throughout, but final validation here
      │
Phase 6 (idle heap compaction)  ← shrink long-idle process heaps to reclaim RSS
```

Phases 0-3 and 2.5 are implemented.  Phases 4-6 can follow independently.

---

## Phase 6 — Idle heap compaction

**Status:** not started.

**Why:** After a traffic burst, processes that grew large heaps during peak
load keep their oversized arenas indefinitely.  Even with jemalloc's
background page decay (~10 s), the Bump arena itself is never shrunk — it
only grows.  GC replaces the arena with a fresh one sized to live data, but
GC only triggers when the process allocates past its threshold.  An idle
process never allocates, so it never GCs, and its oversized arena persists.

BEAM solves this with `fullsweep_after` — after N minor GCs without a full
sweep, the next GC does a full collection.  For nera's single-generation
collector, the analogous mechanism is: if a process has been idle (no
reductions consumed) for N scheduler ticks, force a `collect()` to replace
the oversized arena with a minimal one.

### 6.1  Idle detection

Add an `idle_ticks: u32` counter to `Process`.  Each scheduler tick (or
each time the worker picks up the process and finds no work), increment
`idle_ticks`.  Reset to 0 whenever the process runs reductions.

### 6.2  Compaction trigger

When `idle_ticks >= IDLE_COMPACT_THRESHOLD` (e.g., 10 ticks) **and**
`heap.bytes_allocated > DEFAULT_GC_THRESHOLD`:
- Run `heap.collect()` with the process's current roots.
- The new arena is sized to live data (typically small for an idle
  process).
- The old oversized arena is dropped, freeing memory back through
  jemalloc.
- Reset `idle_ticks` to 0 and set a flag to avoid re-compacting until
  the process becomes active again.

### 6.3  Safety considerations

- Compaction must only run when the process is not executing (i.e., in
  the `Waiting` or `Ready` state with no active stack frames).
- The root set for an idle process is: mailbox terms, process dictionary,
  registered name, and any terms stored in `ctx.result`.
- Must not interfere with receive timeout deadlines or pending I/O.

### 6.4  Files

| File            | Change                                              |
|-----------------|-----------------------------------------------------|
| `process.rs`    | Add `idle_ticks` counter                            |
| `scheduler.rs`  | Increment/reset counter, trigger compaction         |
| `heap.rs`       | No changes — existing `collect()` suffices          |

---

## Design Decisions Summary

| Decision                  | Choice                           | Rationale                                                   |
|---------------------------|----------------------------------|-------------------------------------------------------------|
| Threshold                 | 64 bytes                         | Matches BEAM, avoids atomic overhead for small bins         |
| Refcount type             | `Arc<BinaryData>` (was `AtomicU64`) | Same atomic refcount; less unsafe code, Rust-idiomatic      |
| Backing allocation        | `Arc::new(BinaryData::new(bytes))`              | Simple, correct; custom allocator can come later            |
| Build buffer              | Promote-after-build              | Simpler than mutable refc during construction               |
| GC tracking               | Per-process `Vec<Arc<BinaryData>>`  | O(refc count) not O(heap size); `retain()` for sweep        |
| Sub-binary representation | Same 3-word descriptor + offset  | Reuses existing reserved layout                             |
| Mutability                | Backing immutable after creation | Erlang binaries are immutable; `put_*` only on build buffer |
| Binary virtual heap       | Counter + threshold (BEAM model) | GC triggers on off-heap pressure, not just on-heap size     |
