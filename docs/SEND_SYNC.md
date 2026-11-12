# Runtime Send/Sync invariants

Rust derives the traits for message fragments, stored ETS tuples and keys, and
persistent values from their fields. Do not add blanket unsafe implementations
to those wrappers when introducing a field: let the compiler check its traits.

The remaining explicit implementations live at the raw-memory owners:

| Type | Traits | Required invariant |
| --- | --- | --- |
| `BinaryData` | Send + Sync | Owns a stable Vec allocation; safe access is read-only. Raw construction writes finish before publication. |
| `EtsRegion` | Send + Sync | Owns a mapping until its final Arc is dropped. Pool cursors reserve disjoint ranges; callers initialize each range before publishing it. |
| `CompactHeap` | Send + Sync | Retains its mapping and binary owners. Construction has exclusive access; published words are immutable. |
| `Mailbox` | Send | Owns its ring allocation. Mutation requires exclusive process access; stored terms follow the process heap lifetime. |
| `ProcessContext` | Send | Moves with its process under the process mutex. `run_slice` refreshes process-local pointers before executing compiled code. It is deliberately not Sync. |

An Arc keeps an allocation alive; it does not synchronize writes to that
allocation. Raw access still requires the documented initialization, aliasing,
and process/GC invariants.

## ETS ownership

The pool owns allocation cursors separately from shared mapping owners.
Each nonempty `CompactHeap` holds one `Arc<EtsRegion>`. Zero-copy lookup heaps
retain their `StoredTupleInner`, which retains the mapping even after the table
is deleted. GC preserves this ownership while external terms remain reachable.
Pool reclamation releases mappings with no allocation owners; clearing or
dropping the pool never invalidates outstanding allocations.

The Arc strong count replaces the old live-entry atomic counter: allocation
and release still require one increment and one decrement per compact heap.
The compact heap keeps a pointer-sized optional owner, and there are no extra
per-entry locks, data copies, or changes to generated-code layouts.

## Regression coverage

- A compile-fail doctest prevents restoring Sync on `ProcessContext`.
- Positive trait checks preserve required worker transfers and shared storage.
- Pool tests retain an allocation across pool clear/drop and a thread transfer,
  check final owner release, and verify reclamation after allocation drops.
- An ETS regression retains nested lookup data after table deletion and source
  heap destruction, moves the receiving heap to another thread, and runs full GC.

## Ordered keys and element updates

A boxed ordered key borrows the heap of its paired stored value. `BTreeMap::insert`
retains an existing equal key, so overwrites of boxed keys explicitly remove the
complete old pair under the write lock before inserting the replacement pair. This avoids
retaining obsolete values solely to keep comparison keys alive. Transient lookup
keys borrow the caller's live input for the duration of the operation. New keys
use the vacant Entry path, and immediate keys can safely retain their existing
key while replacing only the value, with no extra tree traversal.

`ets_update_element` holds the map write lock across read/modify/replace, including
any deep copy. No borrowed element pointer escapes that lock. Updates serialize
with insert, delete, clear and counter operations on the same table. The lock
order is storage then pool; insertion releases its pool lock before taking the
storage lock. Reclamation happens after releasing the storage lock.

When both old and new updated elements are immediate, only the private element
slots change: no heap allocation or published word mutation is needed. Replacing
a boxed element still rebuilds the compact heap so obsolete binary owners can be
released. Lookup results keep their own outer tuples and retain their original
immutable backing heaps.

## Allocation bounds and owner encapsulation

`CompactHeap::alloc_words` checks its remaining capacity in every build before
advancing the cursor or returning a pointer. The comparison uses remaining
capacity rather than `offset + n`, so an oversized request cannot wrap the
addition. Zero-sized heaps permit zero-word allocations. Failed reservations
leave the cursor unchanged.

`ProcessHeap` keeps binary and borrowed-heap owners private. Binary registration
and virtual heap charging share one inline method used by allocation and message
copy paths. ETS lookups retain storage through an additive method; no mutable
owner collection is exposed. GC and fragment adoption remain inside the heap
module, preserving their existing ownership and accounting behavior. Tests can
inspect counts without gaining mutable access to owners.
