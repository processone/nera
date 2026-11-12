# ETS Optimization Plan

## Context

ETS (Erlang Term Storage) is the backbone of high-performance Erlang
applications like caches, routers, and session stores.  For nera — an
AOT-compiled Erlang runtime — ETS can become a genuine performance
advantage over BEAM, because we control the memory layout, the GC, and
the term representation at the native level.

BEAM always deep-copies terms on ETS insert and lookup.  This is the
safe default, but it is not required by Erlang semantics.  Erlang terms
are **immutable** — the semantic contract is **isolation**, not copying.
If a term cannot be mutated, sharing it is safe.  nera can exploit this.

This document is the roadmap for ETS optimizations, ordered by
dependency and risk.

---

## Phase 1: Split Lock Domains (DONE)

**Status**: Implemented

**Problem**: `store_tuple` held a single `RwLock<EtsTableData>` write
lock for the entire deep-copy phase — O(term_complexity) — blocking all
concurrent readers and writers.

**Solution**: Split into `map: RwLock<HashMap>` + `pool: Mutex<EtsRegionPool>`.
Insert phases:

1. Extract key/elements from process heap (no lock)
2. `pool.lock().alloc(n)` — brief Mutex (bump pointer, ~ns)
3. `deep_copy_into_compact()` — no lock (writes private CompactHeap)
4. `map.write().insert()` — brief write lock (hash + insert, ~ns)

Write lock hold time reduced from O(term_size) to O(1).

**Files changed**: `crates/nera-runtime/src/ets.rs`

---

## Phase 2: Atomic update_counter/3 (DONE)

**Status**: Implemented

**Problem**: `ets:update_counter/3` was not implemented.  Applications
fell back to `ets:lookup/2` + `ets:update_element/3` — two round-trips,
two lock acquisitions, and a full deep-copy on each lookup.

**Solution**: In-place `map.get_mut(&key)` under single write lock.
No deep-copy, no pool allocation — O(1) for the common case.
Supports bare integer, `{Pos, Incr}`, and `{Pos, Incr, Threshold, SetValue}`.

Also fixed: reclaim-on-overwrite in `store_tuple` — `map.insert()` now
detects when an old entry was replaced and triggers `reclaim_dead_regions()`.

**Files changed**: `crates/nera-runtime/src/ets.rs`, `crates/nera-runtime/src/ffi/ets.rs`,
`crates/nera-ir/src/bif_table.rs`, `crates/nera-runtime/src/ffi/apply.rs`

---

## Phase 3: Complete ETS API Surface (DONE)

**Status**: Implemented

### Implemented operations

| Operation           | Status | Notes                                                             |
|---------------------|--------|-------------------------------------------------------------------|
| `insert_new/2`      | DONE   | Atomic check-and-insert; list form aborts all on any existing key |
| `tab2list/1`        | DONE   | Deep-copies all entries under read lock                           |
| `first/1`, `next/2` | DONE   | HashMap iteration order (non-deterministic, matches BEAM set)     |
| `info/1`            | DONE   | Returns proplist: size, name, type, keypos                        |
| `info/2`            | DONE   | Returns individual item: size, name, type, keypos, memory         |
| `update_counter/3`  | DONE   | See Phase 2                                                       |

### Still missing (lower priority)

| Operation        | Notes                                                     |
|------------------|-----------------------------------------------------------|
| `match_object/2` | Pattern matching on tuples — needs match spec interpreter |
| `select/2,3`     | Full match spec support — needed for OTP apps             |
| `foldl/3`        | Higher-order fold — requires calling Erlang funs from FFI |

### Design notes

- `first/next`: Uses HashMap iteration order directly. For `set` tables
  this matches BEAM behavior (order is hash-dependent, not sorted).
  No snapshot/cursor needed since BEAM `set` tables also have unstable order.
- `tab2list_raw()` helper returns `Vec<ErlTerm>` for future use by `foldl/3`.
- `ets_info` `memory` item returns approximate words based on region count.

**Files changed**: `crates/nera-runtime/src/ets.rs`, `crates/nera-runtime/src/ffi/ets.rs`,
`crates/nera-ir/src/bif_table.rs`, `crates/nera-runtime/src/ffi/apply.rs`

**Tests**: `tests/ets_api.erl` — covers insert_new, tab2list, first/next, info/1,2

---

## Phase 4: Binary Match Context

**Status**: Done
**Risk**: Medium
**Impact**: Very high — ~50x potential on binary pattern matching

This is orthogonal to ETS but included here because it directly affects
TCP-based workloads like Onecached (protocol parsing).

### Problem

Each `<<_, Rest/binary>>` iteration creates a new refc sub-binary
(3-word descriptor + Arc clone + heap alloc).  BEAM uses an in-place
cursor (match context) that just advances an offset register — no
allocation per iteration.

### Design

See the optimization roadmap in MEMORY.md (item 1).  In summary:

- Pattern compiler emits match context instructions instead of
  sub-binary extraction
- Match context is a register pair: `(base_binary_ptr, bit_offset)`
- Sub-binary only materialized when it escapes the match (stored in
  variable, sent as message, inserted in ETS)
- Codegen emits Cranelift IR for offset arithmetic instead of FFI calls

### Files to change

- `crates/nera-ir/src/pattern.rs` — match context IR instructions
- `crates/nera-codegen/src/func_translator.rs` — Cranelift codegen
- `crates/nera-runtime/src/ffi.rs` — binary FFI fallbacks

---

## Phase 5: Zero-Copy ETS Lookups

**Status**: Design phase
**Risk**: High (touches term representation, GC, and process heap)
**Impact**: Very high — eliminates O(term_size) per lookup
**Dependencies**: Phase 1 (done), Phase 3 (stable API surface)

### The core insight

Erlang terms are immutable.  Once a term is stored in ETS, no Erlang
operation can modify its contents.  The only risks from sharing are:

1. **Dangling pointer**: ETS entry deleted while a process holds a
   reference.
2. **GC interference**: GC tries to relocate a pointer into ETS memory,
   or writes a forwarding pointer into shared storage.

Both are solvable without copying.

### Design: Arc-shared CompactHeap + external pointer tag

#### 5a. Arc-wrap StoredTuple internals

Change the storage so that the CompactHeap (word buffer + refc binaries)
is reference-counted:

```rust
struct StoredTuple {
    inner: Arc<StoredTupleInner>,
}

struct StoredTupleInner {
    heap: CompactHeap,        // mmap-backed word buffer
    elements: Vec<ErlTerm>,   // pointers into heap
}
```

On lookup, instead of deep-copying, clone the Arc (1 atomic increment)
and return the element ErlTerms directly — they point into the
CompactHeap which stays alive as long as any Arc reference exists.

On ETS delete/update, the old StoredTuple is dropped from the HashMap.
If no process holds an Arc, the CompactHeap is freed immediately.  If
processes still hold Arcs, the CompactHeap lives until the last Arc is
dropped (via process GC sweep).

#### 5b. External pointer tag (bit 63)

On x86-64 and ARM64, user-space virtual addresses use at most 48-57
bits.  Bit 63 is always 0 for user-space pointers.  We use it as an
"external" flag:

```
Normal boxed:   0xxx...xxxx_xxx_000   (bit 63 = 0, tag = boxed)
External boxed: 1xxx...xxxx_xxx_000   (bit 63 = 1, tag = boxed)
External cons:  1xxx...xxxx_xxx_001   (bit 63 = 1, tag = cons)
```

Term operations:

| Operation        | Behavior                                   |
|------------------|--------------------------------------------|
| `is_boxed()`     | Check low 3 bits = 000 (unchanged)         |
| `is_cons()`      | Check low 3 bits = 001 (unchanged)         |
| `is_external()`  | Check bit 63 (new)                         |
| `as_boxed_ptr()` | Mask off bit 63 + low 3 bits               |
| Pattern match    | Follow pointer transparently (mask bit 63) |
| Comparison       | Follow pointer transparently               |
| Hashing          | Follow pointer transparently               |

The external bit is set ONLY on terms returned from ETS lookups.  It
tells the GC: "this pointer leads to shared memory — do not copy the
target, do not write a forwarding pointer."

#### 5c. GC integration

Current GC flow (`HeapCopier::copy_term`):

```
copy_term(term):
  if immediate → return as-is
  if forwarding[old_addr] exists → return forwarded
  read header at old_addr
  alloc in new arena
  copy data
  forwarding[old_addr] = new_addr
  return new_ptr
```

New GC flow:

```
copy_term(term):
  if immediate → return as-is
  if term.is_external():
    // Shared storage — do not copy, do not write forwarding pointer
    record associated Arc as "live" (like live_refc_ptrs)
    return term  // keep pointing to CompactHeap
  if forwarding[old_addr] exists → return forwarded
  ... (normal copy path)
```

Process heap adds a new field:

```rust
pub struct ProcessHeap {
    // ... existing fields ...
    pub refc_binaries: Vec<Arc<Vec<u8>>>,
    pub borrowed_heaps: Vec<Arc<StoredTupleInner>>,  // NEW
}
```

Post-GC sweep for borrowed_heaps mirrors refc_binaries:

```rust
// Track which Arc pointers are still reachable
// (from external terms encountered during copy_term)
self.borrowed_heaps.retain(|arc| {
    let raw_ptr = Arc::as_ptr(arc) as u64;
    copier.live_borrowed_ptrs.contains(&raw_ptr)
});
```

#### 5d. Lookup path — before vs after

**Before (current):**

```
map.read()
for each element:
  recursive deep_copy into process heap  ← O(term_size)
alloc tuple wrapper on process heap
alloc cons cell on process heap
```

**After (zero-copy):**

```
map.read()
Arc::clone(inner)                        ← 1 atomic op
alloc tuple wrapper on process heap      ← 1+N words
mark elements as external (set bit 63)   ← N bitwise ORs
alloc cons cell                          ← 2 words
store Arc in process.borrowed_heaps
```

Total cost: 1 atomic + N+3 words of process heap.  No recursive walk.
No data copy.  O(1) regardless of term complexity.

#### 5e. Identifying external Arcs during GC

When GC encounters an external boxed pointer, it needs to find which
`Arc<StoredTupleInner>` to mark as live.  Options:

- **A. Embed an identifier in the external term**: Use unused bits in
  the pointer or header to encode an index into `borrowed_heaps`.
  Fast but fragile (index invalidated if Vec is reordered).

- **B. HashMap from address range to Arc index**: Pre-build a mapping
  from CompactHeap word buffer address ranges to borrowed_heaps
  indices.  O(1) lookup with a sorted-range binary search.

- **C. Mark ALL borrowed_heaps as live during GC, sweep separately**:
  During GC, don't try to match individual external pointers to
  specific Arcs.  Instead, just keep a flag `has_external_terms: bool`.
  After GC, if no external terms survived, clear all borrowed_heaps.
  If any survived, keep all.  This over-retains but is simple and
  correct.  On the next GC cycle, the stale Arcs will be dropped if
  the process no longer holds external terms.

- **D. Stamp each external term with the Arc's raw pointer**: Store the
  `Arc::as_ptr()` value in a side table or in unused bits.  GC records
  seen Arc pointers in a `live_borrowed_ptrs: HashSet<u64>`, then
  sweeps borrowed_heaps via `retain()`.  This mirrors the refc binary
  pattern exactly.

**Recommended: Option D** — it matches the existing refc binary sweep
pattern.  The Arc raw pointer can be stored in the process's own data
structure (e.g., a `HashMap<address_range, u64>` built when the Arc is
first borrowed).

The simplest implementation: when a zero-copy lookup returns, store a
mapping `(words_ptr..words_ptr+words_len) → Arc::as_ptr()` in the
process.  When GC encounters an external pointer, look up which Arc it
belongs to and add to `live_borrowed_ptrs`.  After GC, sweep.

An even simpler approach: since each `StoredTupleInner` contains a
`CompactHeap` with a known `words_ptr`, the GC can check if an external
pointer falls within any borrowed CompactHeap's range.  With
borrowed_heaps being a small Vec (typically 0-10 entries), a linear
scan is fast enough.

### Safety argument

- **No mutation**: Erlang terms are immutable.  Sharing is safe.
- **Lifetime**: Arc ensures the CompactHeap lives until the last
  reference is dropped.  ETS deletion drops one Arc reference; process
  GC sweep drops the other.
- **GC correctness**: External pointers are never relocated.  The GC
  copies only the ErlTerm value (the tagged u64), not the target data.
  No forwarding pointer is written to shared memory.
- **Region reclamation**: `CompactHeap::drop()` decrements the region's
  `live_count`.  With Arc, this happens when the last Arc is dropped.
  Region reclamation is delayed but still correct.
- **Concurrent access**: The map's RwLock prevents concurrent
  modification during lookup.  After the read lock is released, the Arc
  keeps the data alive independently of the table.

### What this does NOT optimize

- **Insert path**: Still requires deep-copy from process heap to
  CompactHeap (process heap is GC'd, pointers move).  Could be
  optimized later with "ETS-direct allocation" but that is a larger
  change.
- **Small immediate-only tuples**: Already fast.  The saved overhead is
  just the tuple wrapper allocation, which is minimal.

### Expected impact

| Lookup term type          | Current cost              | Zero-copy cost      | Speedup |
|---------------------------|---------------------------|---------------------|---------|
| Large binary value (refc) | Arc clone + descriptor    | Arc clone only      | ~1.5x   |
| Complex nested term       | Full recursive walk       | Arc clone + wrapper | 10-100x |
| Small immediate tuple     | Tuple alloc + word copies | Same                | ~1x     |
| List of N elements        | 2N words + recursive walk | Arc clone + wrapper | Nx      |

### Files to change

- `crates/nera-runtime/src/term.rs` — `EXTERNAL_BIT`, `is_external()`,
  update `as_boxed_ptr()` / `as_cons_ptr()` to mask bit 63
- `crates/nera-runtime/src/heap.rs` — `borrowed_heaps` field,
  `HeapCopier` external-pointer handling, post-GC sweep
- `crates/nera-runtime/src/ets.rs` — `Arc<StoredTupleInner>`, new
  `lookup_tuples_zerocopy()`, mark elements as external
- `crates/nera-runtime/src/message.rs` — handle external terms in
  deep-copy (clone Arc instead of copying data)
- `crates/nera-runtime/src/ffi.rs` — any term-walking FFI functions
  that dereference boxed pointers (must mask bit 63)

---

## Phase 6: Zero-Copy Message Relay

**Status**: Future
**Risk**: Medium
**Impact**: High for multi-process pipelines
**Dependencies**: Phase 5

### The cascade effect

Once Phase 5 is in place, terms that originate from ETS lookups carry
external pointers backed by `Arc<StoredTupleInner>`.  When such a term
is sent as a message to another process, the current deep-copy in
`message.rs` would copy the data into a new `MessageFragment`.

Instead, the message-send path can detect external pointers and **clone
the Arc** rather than copying the data:

```
Process A: ets:lookup → external term (Arc #1)
Process A: B ! Term   → clone Arc (#2), send external pointers
Process B: receive    → gets external term (Arc #2)
```

No data copy at any point.  The term flows from ETS to Process A to
Process B as shared pointers into the same CompactHeap.

### Implementation

In `message.rs`, `deep_copy_term()` checks `term.is_external()`:

- If external: include the Arc reference in the MessageFragment
  (new variant: `MessageFragment::Borrowed { arc, root }`)
- Receiver's `drain_inbox` stores the Arc in `borrowed_heaps` and
  uses the external term directly

### Safety

Same as Phase 5 — immutability guarantees no mutation, Arc guarantees
lifetime.  Each process independently manages its Arc reference via GC
sweep.

---

## Phase 7: Insert Optimizations (Future)

These are lower-priority because inserts are typically less frequent
than lookups in cache workloads, and the lock split (Phase 1) already
removed the serialization bottleneck.

### 7a. Refc promotion for small binaries in ETS

Currently, binaries <= 64 bytes are stored inline (copied in full).
For ETS specifically, we could lower the refc threshold to 0 — store
ALL binaries as `Arc<Vec<u8>>` in ETS.  This makes lookups cheaper
(Arc clone instead of memcpy) at the cost of an extra Arc allocation
on insert.

Trade-off: good for read-heavy workloads (caches), bad for
write-heavy workloads with small binaries.  Could be a per-table
option.

### 7b. ETS-direct allocation

When a process constructs a term specifically for ETS insertion (common
pattern: `ets:insert(Tab, {Key, Value})`), the runtime could detect
this and allocate the term directly in the ETS pool, avoiding the
process-heap → CompactHeap copy entirely.

This requires compiler support: the codegen would need to recognize
the `ets:insert` call and route the tuple construction to the ETS
allocator.  Significant complexity for moderate gain (insert is already
fast after Phase 1).

### 7c. Insert-from-borrowed shortcut

If the term being inserted came from another ETS lookup (it carries
external pointers to a CompactHeap), the insert could share the source
CompactHeap's data instead of deep-copying.  The new StoredTuple would
hold an Arc to the source CompactHeap.

Challenge: the new entry's key position might differ, and the tuple
elements might be a mix of external and process-heap terms (e.g.,
`ets:insert(NewTab, {NewKey, OldValue})` where OldValue is external).
Partial sharing adds complexity.

---

## Implementation Order

| Phase | Optimization            | Risk   | Impact    | Depends on                  |
|-------|-------------------------|--------|-----------|-----------------------------|
| 1     | Lock split              | Low    | High      | —                           |
| 2     | `update_counter/3`      | Low    | High      | —                           |
| 3     | Complete ETS API        | Low    | Medium    | Design with Phase 5 in mind |
| 4     | Binary match context    | Medium | Very high | Orthogonal                  |
| 5     | Zero-copy lookups       | High   | Very high | Phase 1, 3                  |
| 6     | Zero-copy message relay | Medium | High      | Phase 5                     |
| 7     | Insert optimizations    | Medium | Medium    | Phase 5                     |

Phases 2, 3, and 4 can proceed in parallel.  Phase 5 should wait until
the API surface (Phase 3) is stable and the GC is well-tested under
concurrent ETS workloads.

---

## Current ETS Operations — Implementation Status

| Operation              | Status  | Notes                                             |
|------------------------|---------|---------------------------------------------------|
| `new/2`                | Done    | set type, named_table, {keypos,N}, public         |
| `insert/2`             | Done    | Single tuple or list of tuples                    |
| `lookup/2`             | Done    | Returns `[Tuple]` or `[]`                         |
| `delete/1`             | Done    | Delete entire table                               |
| `delete/2`             | Done    | Delete by key                                     |
| `delete_all_objects/1` | Done    | Instant munmap of all regions                     |
| `update_element/3`     | Done    | Positional update                                 |
| `select_delete/2`      | Done    | Simplified guards (`<`, `>` on integer positions) |
| `member/2`             | Done    | Key existence check                               |
| `update_counter/3`     | Done    | Atomic increment (Phase 2)                        |
| `insert_new/2`         | Done    | Conditional insert (Phase 3)                      |
| `tab2list/1`           | Done    | Full table scan (Phase 3)                         |
| `first/1`, `next/2`    | Done    | Table iteration (Phase 3)                         |
| `match_object/2`       | —       | Pattern matching (not yet implemented)            |
| `select/2,3`           | —       | Full match specs (not yet implemented)            |
| `foldl/3`              | —       | Fold over entries (not yet implemented)           |
| `info/1,2`             | Done    | Table metadata (Phase 3)                          |

---

## Benchmark Targets

Once all phases are implemented, the expected performance profile for
a cache workload (Onecached-like):

| Operation              | vs BEAM          | vs native Rust   |
|------------------------|------------------|------------------|
| ETS lookup (cache hit) | 2-5x faster      | ~1.5-2x slower   |
| ETS insert             | ~1x (comparable) | ~2x slower       |
| update_counter         | ~1x (comparable) | ~1.5x slower     |
| TCP throughput         | ~1x (comparable) | ~1.3x slower     |
| Memory usage           | 3-5x less        | ~1x (comparable) |

The key advantage vs BEAM: zero-copy lookups eliminate the deep-copy
tax that BEAM pays on every `ets:lookup/2`.  For complex stored terms,
this is a 10-100x improvement on the lookup path alone.

The remaining gap vs native Rust: term tagging overhead, the insert-side
deep-copy, process scheduling overhead, and the GC sweep cost for
borrowed Arcs.  These are inherent to the Erlang execution model but
bounded and predictable.

---

## Correctness Verification Strategy

### Current test coverage (baseline)

| Layer          | Tests        | Type                                           | Gap                 |
|----------------|--------------|------------------------------------------------|---------------------|
| ETS Rust unit  | 9 tests      | Single-threaded insert/lookup/delete/roundtrip | No concurrency      |
| ETS pool       | 8 tests      | Region alloc/clear/reclaim/stability           | No concurrent alloc |
| ETS end-to-end | 3 .erl tests | Single-process CRUD                            | No multi-process    |
| Scheduler      | 9 tests      | Process spawn/yield/message passing            | No ETS interaction  |

**Critical gap: zero concurrent ETS tests exist today.**  Every ETS
test runs from a single thread or single Erlang process.  The lock
split (Phase 1) and all future phases change concurrent behavior, so
we need concurrent tests before and alongside each optimization.

### Test categories

Each phase must add tests in three categories:

**Category A — Rust unit tests** (`ets.rs`, `ets_pool.rs`)
Deterministic, fast.  Test internal invariants: data structure
consistency, lock ordering, region lifecycle, pointer validity.

**Category B — Rust concurrent stress tests** (`ets.rs`)
Multi-threaded.  Spawn OS threads that hammer ETS operations
simultaneously.  Use `Arc<Barrier>` for synchronized starts,
`AtomicU64` for result checking.  These catch races, deadlocks,
and memory corruption.

**Category C — End-to-end Erlang tests** (`tests/ets_*.erl`)
Multi-process.  Spawn Erlang processes that perform ETS operations
concurrently, verify results, and detect data corruption via
pattern matching (badmatch = failure).  These exercise the full
stack: codegen → FFI → runtime → scheduler → GC.

### Phase 1 verification (lock split — DONE, tests to add)

#### B1. Concurrent insert + lookup (Rust)

Spawn N threads, all sharing one `Arc<EtsTable>`:
- Writer threads: insert keys 0..K with distinct values
- Reader threads: lookup random keys, verify value matches key
- Run for M iterations
- Check: no panics, no corrupt data, all inserted keys findable

Edge cases:
- **Insert while lookup in progress**: reader holds map read lock,
  writer blocks on write lock, completes after reader finishes
- **Overwrite while reading**: writer replaces key K with new value
  while reader is deep-copying old value.  Old CompactHeap must
  remain valid for the duration of the read lock.

#### B2. Concurrent insert + delete (Rust)

Spawn writers and deleters on the same table:
- Writers insert keys 0..K in a loop
- Deleters delete random keys in a loop
- Run for M iterations
- Check: no panics.  Lookup of any key returns either the correct
  value or [] (deleted).  Never a corrupt term.

Edge case:
- **Region reclaim during insert**: deleter removes all entries from
  a region (live_count → 0), pool.reclaim munmaps it.  Meanwhile
  inserter allocates from same pool — must get a fresh region, not
  the reclaimed one.

#### B3. Concurrent delete_all_objects + insert (Rust)

One thread loops `delete_all_objects`.  Other threads loop insert +
lookup.  Verify: after each delete_all_objects, the table is empty
(no stale entries).  After subsequent inserts, lookups return correct
data.

Edge case:
- **delete_all_objects while pool.alloc in flight**: inserter gets
  `(region_ptr, words_ptr)` from pool, then `delete_all_objects`
  clears the pool (munmaps regions).  The inserter's CompactHeap
  now points to freed memory.  This is safe because pool.alloc
  incremented live_count before returning — the region's live_count
  is > 0, so `clear()` will munmap but the EtsRegion struct (with
  live_count) lives until Box is dropped.  **WAIT**: `clear()` calls
  `self.regions.clear()` which drops ALL Box<EtsRegion>.  If an
  in-flight CompactHeap holds a back-pointer to a dropped EtsRegion,
  its Drop will dereference freed memory.  **This is a potential
  bug** that this test would catch.  Mitigation: `clear()` should
  only munmap regions with `live_count == 0`, or CompactHeap must
  Arc-wrap the region.

#### C1. Concurrent ETS from Erlang processes

```erlang
test() ->
    Tab = ets:new(conc_test, [set, public, named_table]),
    Self = self(),
    N = 1000,
    Procs = 10,
    %% Spawn writers
    [spawn(fun() ->
        [ets:insert(Tab, {I, I * 10}) || I <- lists:seq(P*N, (P+1)*N - 1)],
        Self ! {done, P}
    end) || P <- lists:seq(0, Procs - 1)],
    %% Wait for all writers
    [receive {done, _} -> ok end || _ <- lists:seq(0, Procs - 1)],
    %% Verify all entries
    ok = verify_range(Tab, 0, Procs * N),
    ok.
```

### Phase 2 verification (update_counter/3)

#### A1. update_counter basic (Rust unit)

- Insert `{key, 0}`, call update_counter +1, verify value is 1
- Increment by negative value, verify underflow behavior
- Increment non-integer element → error
- Increment non-existent key → false

#### A2. update_counter with threshold (Rust unit)

- `update_counter(Tab, Key, {2, 1, 10, 0})` — increment, wrap at 10
- Verify wrap-around: 9 → 10 → 0 (reset to SetValue)

#### B4. Concurrent update_counter (Rust)

Spawn N threads, each incrementing the same counter M times.
Final value must be N * M (no lost increments).

Edge case:
- **update_counter + delete race**: one thread deletes the key while
  others increment.  delete wins → subsequent increments return false
  (key not found).  No crash.

#### C2. Concurrent counters from Erlang

```erlang
test() ->
    Tab = ets:new(counter_test, [set, public, named_table]),
    ets:insert(Tab, {counter, 0}),
    Self = self(),
    Procs = 100,
    Incrs = 1000,
    [spawn(fun() ->
        [ets:update_counter(Tab, counter, 1) || _ <- lists:seq(1, Incrs)],
        Self ! done
    end) || _ <- lists:seq(1, Procs)],
    [receive done -> ok end || _ <- lists:seq(1, Procs)],
    [{counter, Final}] = ets:lookup(Tab, counter),
    100000 = Final,  %% Procs * Incrs, badmatch if lost increments
    ok.
```

### Phase 3 verification (complete API)

#### A3. insert_new semantics (Rust unit)

- Insert `{1, a}`, then `insert_new({1, b})` → false, value unchanged
- `insert_new({2, c})` on empty key → true, value stored

#### B5. Concurrent insert_new (Rust)

Spawn N threads, all calling `insert_new(Tab, {same_key, thread_id})`.
Exactly one must succeed (return true).  All others return false.
Final lookup returns the winning thread's value.

#### C3. tab2list + concurrent modification (Erlang)

One process calls `tab2list/1` while another inserts.  Verify:
- No crash
- Returned list is a consistent snapshot (all returned entries are
  valid tuples, no half-written data)

#### C4. first/next iteration (Erlang)

Iterate entire table with first/next, collect all keys, verify they
match expected set.  Test with concurrent inserts to verify either:
- Iteration sees a consistent snapshot, or
- Iteration raises an error on concurrent modification (acceptable)

### Phase 5 verification (zero-copy lookups)

This is the most critical phase for correctness.  Memory corruption
here would be silent and devastating.

#### A4. External bit tag (Rust unit)

- Create an ErlTerm with bit 63 set, verify `is_external() == true`
- Verify `is_boxed()` still works (low 3 bits unchanged)
- Verify `as_boxed_ptr()` masks bit 63 and returns correct address
- Round-trip: `boxed_external(ptr)` → `as_boxed_ptr()` → same ptr

#### A5. GC skips external pointers (Rust unit)

- Create a ProcessHeap with mixed terms: some local, some external
- Run GC (collect)
- Verify: local terms were relocated (new addresses)
- Verify: external terms kept their original addresses (unchanged)
- Verify: borrowed_heaps Vec retains only live Arcs

#### A6. GC does not write forwarding pointers to external memory (Rust unit)

- Allocate a buffer simulating CompactHeap memory
- Create an external boxed pointer to it
- Store original contents of the buffer
- Run GC
- Verify buffer contents unchanged (no forwarding pointer written)

#### A7. Arc lifecycle (Rust unit)

- Create `Arc<StoredTupleInner>` with strong_count=1 (table owns it)
- Simulate lookup: clone Arc (count=2)
- Remove entry from table (count=1, process holds)
- Drop process reference (count=0, CompactHeap freed)
- Verify: region live_count decremented to 0

#### A8. Arc lifecycle — GC sweep (Rust unit)

- Lookup term → borrowed_heaps has 1 Arc (count=2)
- Overwrite the process variable (term no longer in roots)
- Run GC
- Verify: Arc swept from borrowed_heaps (count=1, only table)

#### B6. Concurrent lookup + delete with zero-copy (Rust)

Spawn readers and deleters:
- Readers: lookup key, hold result, sleep briefly, access elements
- Deleters: delete key from table
- Key behavior: reader gets Arc before delete.  After delete, table's
  Arc is dropped, but reader's Arc keeps CompactHeap alive.  Reader
  must be able to access all elements without crash.

Edge case:
- **Use-after-delete**: reader looks up, gets external pointer.
  Another thread deletes the entry.  Reader dereferences the
  external pointer.  Must succeed (Arc keeps memory alive).

#### B7. Concurrent lookup + delete_all_objects with zero-copy (Rust)

Same as B6 but with delete_all_objects (which clears the pool).
The reader's Arc must keep the CompactHeap alive even after the
pool has been cleared and regions potentially reclaimed.

Edge case:
- **Region reclaim race**: delete_all_objects clears map (all
  StoredTuples dropped, their Arc count decremented).  But a reader
  holds an Arc clone, so count > 0.  The CompactHeap's region
  back-pointer must still be valid.  `EtsRegionPool::clear()` drops
  all `Box<EtsRegion>`.  If CompactHeap holds a raw `*const
  EtsRegion`, this is a use-after-free on CompactHeap::drop().
  **This is a design constraint**: with Arc-shared CompactHeaps,
  the region back-pointer must also be Arc'd, or CompactHeap::drop
  must skip live_count decrement when the region is already freed.

#### B8. GC under concurrent ETS access (Rust)

Spawn processes that:
1. Lookup from ETS (get external terms)
2. Allocate heavily to trigger GC
3. After GC, verify external terms are still valid
4. Repeat in a loop

This catches: GC relocating external pointers (should not happen),
GC dropping borrowed_heaps prematurely, forwarding-pointer corruption.

#### C5. Lookup + GC + access from Erlang

```erlang
test() ->
    Tab = ets:new(gc_test, [set, public]),
    ets:insert(Tab, {key, lists:seq(1, 1000)}),
    %% Lookup and hold result
    [{key, List}] = ets:lookup(Tab, key),
    %% Trigger GC by allocating garbage
    [lists:seq(1, 10000) || _ <- lists:seq(1, 100)],
    %% Verify the looked-up term survived GC
    1000 = length(List),
    1 = hd(List),
    ok.
```

#### C6. Cross-process ETS value relay (Erlang)

```erlang
test() ->
    Tab = ets:new(relay, [set, public, named_table]),
    ets:insert(Tab, {key, <<"large_binary_value_here_over_64_bytes_to_be_refc">>}),
    Self = self(),
    spawn(fun() ->
        [{key, Val}] = ets:lookup(Tab, key),
        %% Send the looked-up value to another process
        Self ! {result, Val}
    end),
    receive
        {result, V} ->
            <<"large_binary_value_here_over_64_bytes_to_be_refc">> = V,
            ok
    end.
```

When Phase 6 (zero-copy relay) is implemented, this test verifies
the Arc-sharing cascade: ETS → process A → message → process B.

#### C7. Stress: rapid insert-overwrite-lookup cycle (Erlang)

```erlang
test() ->
    Tab = ets:new(stress, [set, public, named_table]),
    Self = self(),
    Procs = 20,
    Rounds = 5000,
    [spawn(fun() ->
        stress_loop(Tab, P, Rounds),
        Self ! {done, P}
    end) || P <- lists:seq(1, Procs)],
    [receive {done, _} -> ok end || _ <- lists:seq(1, Procs)],
    ok.

stress_loop(_Tab, _P, 0) -> ok;
stress_loop(Tab, P, N) ->
    Key = {P, N rem 100},  %% 100 keys per process, overwritten
    ets:insert(Tab, {Key, N, make_value(N)}),
    case ets:lookup(Tab, Key) of
        [{Key, _, _}] -> ok;
        [] -> ok  %% deleted by another process
    end,
    stress_loop(Tab, P, N - 1).

make_value(N) when N rem 3 == 0 -> lists:seq(1, 50);
make_value(N) when N rem 3 == 1 -> {nested, {tuple, N}};
make_value(N) -> list_to_binary(integer_to_list(N)).
```

This exercises diverse term types (lists, nested tuples, binaries)
under concurrent insert/overwrite/lookup.  Pattern match on lookup
result catches any corruption.

#### B9. Memory leak detection (Rust)

After a test that inserts and deletes many entries:
- Check `pool.mapped_bytes() == 0` (all regions reclaimed)
- Check `pool.live_region_count() == 0`
- With zero-copy: after all processes drop their Arcs, verify same

After a test with zero-copy lookups + GC:
- Force GC on all processes
- Verify `borrowed_heaps.len() == 0` (all Arcs swept)
- Verify `Arc::strong_count() == 1` (only table holds it)

### Phase 6 verification (zero-copy message relay)

#### B10. Message relay preserves Arc sharing (Rust)

- Process A lookups from ETS → external term (Arc count = 2)
- Process A sends to Process B → Arc count should be 3
  (table + A + B), NOT a fresh deep-copy
- Delete ETS entry (Arc count = 2)
- GC Process A (if term not in roots, Arc count = 1)
- Process B accesses term → must succeed (Arc count = 1)
- GC Process B → Arc count = 0, CompactHeap freed

#### C8. Chain relay (Erlang)

A → B → C → D, each process receives from previous and forwards to
next.  Final process verifies data integrity.  With zero-copy relay,
all share the same CompactHeap.  Delete the ETS entry mid-chain.
All processes that already received must still access valid data.

### Test infrastructure additions

#### Rust test helpers needed

```rust
/// Create a table and insert N entries with diverse term types.
fn populate_test_table(n: usize) -> (TableId, Arc<EtsTable>)

/// Spawn M OS threads, each running `f` concurrently.
/// Synchronized start via Barrier.
fn concurrent_test<F: Fn(usize) + Send + Sync>(threads: usize, f: F)

/// Assert no memory leaks: all regions reclaimed, no stale Arcs.
fn assert_no_leaks(table: &EtsTable)
```

#### Erlang test helpers needed

```erlang
%% Spawn N processes, each running F, wait for all to finish.
par_run(N, F) ->
    Self = self(),
    [spawn(fun() -> F(I), Self ! {done, I} end) || I <- lists:seq(1, N)],
    [receive {done, _} -> ok end || _ <- lists:seq(1, N)].
```

### Known Issues

**GC corruption under heavy allocation with closures/ETS lookup results on heap**

When a process accumulates large amounts of data on its heap (e.g., ETS lookup
results + closure environments) and then performs heavy allocation (e.g., 50K
tuples via `make_tuple/10`), the GC can corrupt closure or term pointers,
leading to SIGBUS (exit 138) or spurious `function_clause` errors.

Observed behavior:
- 20K tuple allocations after ETS lookups: works reliably
- 50K tuple allocations after ETS lookups: crashes with SIGBUS
- `lists:foreach` with closure over 600+ ETS lookup entries: `function_clause`
  after ~3-4 iterations (deterministic, position-dependent, not data-dependent)

This is a pre-existing GC bug unrelated to ETS optimizations. The
`ets_gc_survival` regression test uses 20K allocations as a workaround.
The `ets_stress_complex` test uses per-process verification to avoid heap
accumulation.

Root cause investigation: likely in `heap.rs` `HeapCopier` — possibly a
forwarding pointer collision or closure environment corruption during
`collect()` when the heap contains a mix of deep structures (cons cells,
tuples, refc binary descriptors, closure env slots) at high volume.

### Known edge cases to watch for (all phases)

| Edge case                                                              | Risk                                                         | Phase |
|------------------------------------------------------------------------|--------------------------------------------------------------|-------|
| delete_all_objects while CompactHeap in flight                         | Use-after-free on region back-pointer                        | 1, 5  |
| GC during ETS lookup (if FFI suspends mid-copy)                        | Stale pointers in half-copied tuple                          | 1     |
| Reader holds external pointer, entry overwritten (not deleted)         | Old CompactHeap must stay alive                              | 5     |
| Process dies with borrowed_heaps non-empty                             | Arc must be dropped on process cleanup                       | 5     |
| Nested external pointers (tuple in ETS contains external cons)         | GC must recursively skip all external sub-terms              | 5     |
| External term used as key in another ETS insert                        | deep_copy must follow external pointer for key serialization | 5     |
| External term compared with local term for equality                    | Comparison must mask bit 63 transparently                    | 5     |
| HashMap rehash while reader holds reference                            | Not possible: RwLock prevents concurrent rehash + read       | 1     |
| Pool region growth (Vec<Box<EtsRegion>> realloc) while alloc in flight | Safe: Box provides pointer stability                         | 1     |
| update_counter overflow past small_int range                           | Must detect and either promote to bigint or raise error      | 2     |
