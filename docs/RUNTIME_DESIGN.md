# nera Runtime Design Document

## 1. Executive Summary

**nera** is an Erlang-to-native compiler written in Rust. It produces standalone
single binaries — one source file in, one native executable out — with the
runtime statically linked. This document describes the design of the embedded
runtime (`nera-runtime`), covering concurrency, memory management, scheduling,
inter-process communication, and all supporting subsystems.

The runtime aims to faithfully reproduce the BEAM virtual machine's concurrency
semantics while leveraging Rust's type system, ownership model, and ecosystem
to eliminate entire classes of bugs and to replace, where possible, runtime
garbage collection with static and arena-based memory management.

---

## 2. Goals and Non-Goals

### Goals

- **BEAM-compatible concurrency semantics**: lightweight processes, message
  passing, selective receive, process isolation, per-process failure.
- **Single-binary deployment**: the runtime is statically linked into every
  compiled program. No external VM, no shared libraries at run time.
- **Rust-native implementation**: the runtime is 100% Rust (plus a few lines
  of platform-specific assembly for context switching). No C or C++ dependencies
  beyond the system linker.
- **Multi-core scalability**: one scheduler thread per CPU core with work
  stealing, matching BEAM's parallelism model.
- **Predictable latency**: per-process garbage collection (no global
  stop-the-world pauses) and bounded scheduling quanta via reduction counting.
- **Rust interoperability**: long-term, Rust code should be able to spawn
  Erlang processes and vice versa, sharing the same scheduler.

### Non-Goals (out of initial scope)

- Full OTP compatibility (e.g. hot code loading, distribution protocol).
- BEAM bytecode execution — nera compiles to native code via Cranelift.
- Drop-in replacement for the BEAM VM.

---

## 3. Architecture Overview

```
┌──────────────────────────────────────────────────────────┐
│                  Compiled Erlang Program                  │
│  ┌────────────────────────────────────────────────────┐  │
│  │             Native Code (Cranelift)                │  │
│  │   • Compiled Erlang functions                      │  │
│  │   • Reduction checks at function entry             │  │
│  │   • Calls to runtime via C-ABI FFI                 │  │
│  └───────────────────────┬────────────────────────────┘  │
│                          │ C-ABI                         │
│  ┌───────────────────────▼────────────────────────────┐  │
│  │              nera-runtime (Rust)                   │  │
│  │                                                    │  │
│  │  ┌──────────┐ ┌──────────┐ ┌─────────────────┐   │  │
│  │  │ Scheduler│ │  Process  │ │   Atom Table    │   │  │
│  │  │ (N:M)    │ │  Table    │ │   (global,      │   │  │
│  │  │ work-    │ │  (RwLock) │ │    lock-free    │   │  │
│  │  │ stealing │ │           │ │    reads)       │   │  │
│  │  └────┬─────┘ └────┬─────┘ └─────────────────┘   │  │
│  │       │             │                              │  │
│  │  ┌────▼─────────────▼─────────────────────────┐   │  │
│  │  │              Per-Process State              │   │  │
│  │  │  ┌───────┐ ┌─────────┐ ┌──────────────┐   │   │  │
│  │  │  │ Heap  │ │ Mailbox │ │  Reductions   │   │   │  │
│  │  │  │(bump) │ │ (VecDq) │ │  counter      │   │   │  │
│  │  │  └───────┘ └─────────┘ └──────────────┘   │   │  │
│  │  └────────────────────────────────────────────┘   │  │
│  │                                                    │  │
│  │  ┌──────────────────┐  ┌──────────────────────┐   │  │
│  │  │  Message Passing │  │  Function Table      │   │  │
│  │  │  (deep copy)     │  │  (MFA → fn pointer)  │   │  │
│  │  └──────────────────┘  └──────────────────────┘   │  │
│  │                                                    │  │
│  │  ┌──────────────────┐  ┌──────────────────────┐   │  │
│  │  │  BIFs            │  │  GC                  │   │  │
│  │  │  (io, lists,     │  │  (copying collector) │   │  │
│  │  │   type checks)   │  │                      │   │  │
│  │  └──────────────────┘  └──────────────────────┘   │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

---

## 4. Term Representation

`ErlTerm` is an unrooted, transparent 64-bit value. `from_raw()` and
`to_raw()` move ABI bits without validating or dereferencing them. Tag tests
and `Debug` remain safe; reading a heap header or binary requires a live,
current term and is explicitly `unsafe`.

Rust callers can establish that contract once with
`unsafe { term.borrow(&heap) }`, then use the safe accessors on
`BorrowedTerm<'_>`. Its lifetime prevents collecting or dropping the heap
while the view or a returned byte slice is in use. It is one word and adds
no allocations or reference counting. The initial binding remains unsafe:
the caller must establish that the term is valid and owned by that heap.

### 4.1 Tagged Pointer Scheme

Every Erlang value is a 64-bit word (`ErlTerm`). The low 3 bits encode the
primary type tag:

```
  Bits [63:3]          [2:0]
  ┌──────────────────┬───────┐
  │   payload        │  tag  │
  └──────────────────┴───────┘

  000  Boxed pointer  → header word at address (tuple, float, bigint, binary, closure)
  001  Cons cell      → [head | tail] at address
  010  Small integer  → 61-bit signed integer (arithmetic shift)
  011  Atom           → index into global atom table
  100  PID            → process identifier
  101  Nil            → empty list (immediate)
```

**Immediates** (small integers, atoms, PIDs, nil) carry their entire value
inside the 64-bit word. No heap allocation, no pointer indirection. Testing
two atoms for equality is a single integer comparison. Ordering two atoms
compares their names, as on BEAM, not their indices (see §5.4).

Small integers cover the range ±2^60 — sufficient for the vast majority of
Erlang integer arithmetic without falling back to heap-allocated bignums.

### 4.2 Boxed Object Layout

Heap-allocated objects (tuples, floats, bignums, binaries, closures) begin
with a **header word**. The low 4 bits of the header encode the boxed subtype:

```
  Header word: [ arity/size (60 bits) | subtype (4 bits) ]

  Subtypes:
  0x0  Tuple    → header + N element words
  0x1  Float    → header + 1 word (IEEE 754 double bits)
  0x2  BigInt   → header + N limb words
  0x3  Binary   → header + byte data (padded to word alignment)
  0x4  Closure  → header + function pointer + arity + captured variables
  0x5  Map      → header + size + key/value pairs
  0x6  Ref      → header + unique reference id
  0x7  BigInt   → header + sign + limb data
  0x8  RefcBin  → header + Arc pointer + offset + size
  0x9  MatchCtx → header + base pointer + bit offset + bit size
```

The tagged pointer `ErlTerm` for a boxed object stores the aligned address of
the header word with the low 3 bits set to `000` (boxed tag). Since all heap
allocations are 8-byte aligned, the tag bits are always available.

### 4.3 Cons Cells

Lists are chains of cons cells, each consisting of two contiguous words
(`head`, `tail`). The `ErlTerm` for a cons cell stores the address with the
low 3 bits set to `001`.

```rust
#[repr(C)]
pub struct ConsCell {
    pub head: ErlTerm,
    pub tail: ErlTerm,
}
```

### 4.4 Design Rationale

This scheme matches BEAM's general approach but is simplified for 64-bit
systems. The key property is that the common case — comparing integers,
matching atoms, traversing lists — involves **zero heap allocation** and
**zero pointer indirection** for immediates. Pattern matching on atoms
compiles down to integer comparisons in the generated native code.

---

## 5. Atom Table

### 5.1 Structure

The global atom table is a singleton (`OnceLock<AtomTable>`) providing
bidirectional mapping between atom strings and compact `u32` indices:

```
  "ok"              ↔  0
  "error"           ↔  1
  "true"            ↔  2
  "false"           ↔  3
  "undefined"       ↔  4
  ...
```

### 5.2 Concurrency Strategy

- **Name to index** (interning, `binary_to_existing_atom`): acquire a
  `parking_lot::RwLock` in read mode on the name map.
- **Index to name** (`atom_to_list`, ordering): no lock. Names are stored by
  index in blocks of 1,024 that are allocated on first use. Each block and
  each name is written once, before its index is returned, so any holder of
  an atom can read its name.
- **Writes** (rare, only when a new atom is first encountered): acquire the
  name map's write lock, double-check for races, store the name, insert.

### 5.3 Compiler Integration

Well-known atoms (`ok`, `error`, `true`, `false`, `badarg`, etc.) are
pre-registered at fixed indices during runtime initialization. The compiler
emits these indices as immediate constants, avoiding any table lookup at
run time for built-in atoms.

At program startup, the generated `main()` function calls
`nera_rt_intern_atom()` for each atom used in the source code, populating
the table before any process starts executing.

### 5.4 Ordering

Indices follow creation order, which depends on the program, so they cannot
order atoms. `<`, `lists:sort/1`, map key order, `ordered_set` tables and
`erlang:max/2` compare atoms as BEAM does: by their UTF-8 names, byte by
byte, a name before every longer name that starts with it. For `zebra` and
`apple`, `zebra > apple` whichever was created first.

Each stored name carries an order key: its first eight bytes, big-endian and
padded with zeros. Keys that differ order as the names do, so most
comparisons are two lock-free reads and one integer comparison; equal keys
fall back to comparing the full names.

### 5.5 Future: Atom Limit

BEAM limits atoms to ~1,048,576 because they are never garbage-collected.
The current implementation has no hard limit; one should be added to prevent
pathological programs from exhausting memory.

---

## 6. Concurrency Model

### 6.1 Overview

nera reproduces BEAM's concurrency model:

- **Lightweight processes**: thousands to millions of isolated execution
  contexts, each with its own heap and mailbox.
- **No shared memory**: processes communicate exclusively via message passing.
  Messages are deep-copied between heaps.
- **Preemptive scheduling**: reduction counting ensures fair CPU sharing.
  No process can starve others.

### 6.2 Comparison with BEAM and Go

| Aspect              | BEAM                            | Go (GMP)                        | nera                           |
|---------------------|---------------------------------|---------------------------------|---------------------------------|
| Unit of concurrency | Process (isolated heap)         | Goroutine (shared memory)       | Process (isolated heap)         |
| Preemption          | Reduction counting              | Cooperative + async preemption  | Reduction counting              |
| Scheduling          | Per-scheduler run queues + steal| P-local run queues + steal      | Per-worker deque + global inject|
| Memory isolation    | Full (no shared mutable state)  | Shared address space            | Full (no shared mutable state)  |
| GC                  | Per-process (no global pause)   | Global stop-the-world           | Per-process (no global pause)   |
| Message passing     | Deep copy                       | Channel (reference passing)     | Deep copy                       |

nera follows the BEAM model, not Go's model. This is a deliberate choice:
process isolation enables independent per-process GC, fault isolation, and
reasoning about concurrency without data races.

---

## 7. Scheduler

### 7.1 Architecture

The scheduler uses N OS threads (one per CPU core), each running a **worker
loop**. Work is distributed via three levels:

```
Thread 0          Thread 1          Thread 2          Thread N
┌─────────┐      ┌─────────┐      ┌─────────┐      ┌─────────┐
│ Worker   │      │ Worker   │      │ Worker   │      │ Worker   │
│ deque    │ ◄──► │ deque    │ ◄──► │ deque    │ ◄──► │ deque    │
└────┬─────┘      └────┬─────┘      └────┬─────┘      └────┬─────┘
     │                 │                 │                 │
     └────────────┬────┘────────────┬────┘────────────┬────┘
                  │                                   │
            ┌─────▼───────────────────────────────────▼─────┐
            │              Global Injector Queue             │
            └───────────────────────────────────────────────┘
```

1. **Local deque** (`crossbeam_deque::Worker`): FIFO deque owned by each
   worker thread. Processes that yield go back here for locality.
2. **Global injector** (`crossbeam_deque::Injector`): newly spawned processes
   are pushed here. Workers steal batches from the injector when their local
   deque is empty.
3. **Peer stealing**: if both local and global queues are empty, a worker
   steals from a random peer's deque.

### 7.2 Worker Loop

```
loop:
    1. Pop from local deque
    2. If empty → steal batch from global injector
    3. If empty → steal from random peer
    4. If empty → check termination condition → park briefly

    If process obtained:
        5. Set process status = Running, reset reductions to 4000
        6. Execute process for one time-slice (run_slice)
        7. Inspect result:
           - STATUS_OK + no more work → process exited, remove from table
           - STATUS_OK + continuation  → push back to local deque
           - STATUS_YIELD              → push back to local deque
           - STATUS_RECEIVE_WAIT       → mark Waiting, push to injector
           - STATUS_EXCEPTION          → process exited, remove from table
```

### 7.3 Reduction Counting

Each process is granted a budget of **4,000 reductions** per scheduling
quantum. The compiled code decrements the reduction counter at every function
entry point:

```
compiled_function:
    ctx.reductions -= 1
    if ctx.reductions <= 0:
        ctx.status = STATUS_YIELD
        return
    ... function body ...
```

When the budget is exhausted, the process yields control back to the
scheduler, which re-queues it and picks the next runnable process. This
ensures fair scheduling without OS-level preemption or signal injection.

**Trade-off**: a single long-running BIF or NIF can delay yielding. This is
addressed in Phase 5 by introducing dirty schedulers (see Section 15).

### 7.4 Termination Detection

The scheduler terminates when **all processes are either Exited or Waiting**
(deadlock) and no ready processes remain in any queue. Workers check this
condition when idle and propagate a shutdown signal via `AtomicBool`.

---

## 8. Process Representation

### 8.1 Process Structure

```rust
pub struct Process {
    pub id: ProcessId,              // Unique u64 identifier
    pub status: ProcessStatus,      // Ready | Running | Waiting | Exited
    pub heap: ProcessHeap,          // Per-process bump-allocated arena
    pub mailbox: Mailbox,           // FIFO message queue with selective scan
    pub reductions: i64,            // Remaining reductions this quantum
    pub context: ProcessContext,    // C-ABI context passed to compiled code
    pub continuation: Option<Continuation>,  // Resume point after yield
    pub initial_fn: Option<ErlFn>,  // Entry function pointer
    pub initial_args: Vec<ErlTerm>, // Entry function arguments
}
```

### 8.2 ProcessContext (C-ABI Bridge)

The `ProcessContext` is a `repr(C)` struct passed as a pointer to every
compiled function. It bridges the native code and the Rust runtime:

```rust
#[repr(C)]
pub struct ProcessContext {
    pub pid: u64,           // Current process PID
    pub reductions: i64,    // Reduction counter (decremented by compiled code)
    pub status: u32,        // Set by compiled code: OK/YIELD/RECEIVE_WAIT/EXCEPTION
    _padding: u32,
    pub result: u64,        // Return value or exception term
    pub heap_ptr: *mut ProcessHeap,     // Direct access to process heap
    pub mailbox_ptr: *mut Mailbox,      // Direct access to process mailbox
}
```

The scheduler sets `heap_ptr` and `mailbox_ptr` before each `run_slice()`,
giving compiled code direct access to the process's heap and mailbox through
FFI functions without needing to look up the process in the global table.

### 8.3 Process Lifecycle

```
     spawn()
        │
        ▼
    ┌───────┐     run_slice()     ┌─────────┐
    │ Ready │ ───────────────────►│ Running │
    └───────┘                     └────┬────┘
        ▲                              │
        │  message arrives             │
        │  or timeout                  │
    ┌───────┐     receive with    ┌────▼────┐
    │Waiting│ ◄───no match───────│ Running │
    └───────┘                     └────┬────┘
                                       │
                                  reductions=0 or
                                  function returns
                                       │
                                  ┌────▼────┐
                                  │ Exited  │
                                  └─────────┘
```

---

## 9. Memory Management Strategy

### 9.1 The Fundamental Challenge

Erlang is dynamically typed and immutable. The lifetime of any term depends on
runtime control flow:

```erlang
case X of
    a -> Foo;    % Foo escapes
    b -> ok      % Foo is garbage
end
```

A pure Rust ownership model (compile-time lifetimes) cannot fully determine
when Erlang terms become unreachable. However, Erlang's immutability guarantees
**no cyclic data structures**, which opens the door to strategies that
outperform a traditional tracing GC for most workloads.

### 9.2 Hybrid Memory Architecture

nera uses a **four-layer memory strategy** that combines Rust's strengths
with a minimal GC for the cases where static analysis is insufficient:

```
┌──────────────────────────────────────────────────────────────┐
│                     MEMORY LAYERS                            │
│                                                              │
│  Layer 1: Stack Allocation (Compiler Escape Analysis)        │
│  ├── Temporaries proven local to a function → stack          │
│  ├── Eliminates ~30-50% of allocations entirely              │
│  └── Zero cost: no heap, no GC, no refcount                 │
│                                                              │
│  Layer 2: Per-Process Bump Arena (bumpalo)                   │
│  ├── All escaping terms bump-allocated here                  │
│  ├── O(1) allocation: just increment a pointer               │
│  ├── Same performance as BEAM's nursery heap                 │
│  └── Freed instantly on process death (drop the arena)       │
│                                                              │
│  Layer 3: Copying GC (only when arena exceeds threshold)     │
│  ├── Triggered when arena passes 256 KiB                     │
│  ├── Cheney's copying collector: trace roots, copy live data │
│  ├── Per-process: no global pause, no coordination           │
│  ├── Short-lived processes: NEVER triggers GC                │
│  └── Long-lived servers: occasional minor collections        │
│                                                              │
│  Layer 4: Reference-Counted Shared Objects (implemented)     │
│  ├── Large binaries (>64 bytes): Arc<BinaryData>                │
│  ├── No cycles possible (Erlang is immutable) → safe         │
│  └── Shared across processes without deep copy               │
└──────────────────────────────────────────────────────────────┘
```

### 9.3 Why Not Pure Reference Counting?

Reference counting (`Arc<T>`) for all terms would eliminate GC entirely, and
Erlang's immutability guarantees no cycles, making it theoretically sound.
However, the performance cost is prohibitive:

| Operation           | Bump Allocator     | Arc\<T\>                      |
|---------------------|--------------------|-------------------------------|
| Allocate a tuple    | pointer bump: ~1ns | malloc: ~25-50ns              |
| Create a cons cell  | pointer bump: ~1ns | malloc: ~25-50ns              |
| Clone a value       | word copy: <1ns    | atomic increment: ~5-15ns     |
| Drop a value        | nothing (GC later) | atomic decrement + maybe free |
| Build list of 1000  | 1000 bumps: ~1us   | 1000 mallocs: ~30us           |
| Message send (copy) | memcpy region      | 1000 Arc::clone: ~10us        |

Functional languages create large volumes of short-lived garbage. Bump
allocation handles this pattern optimally: allocate by incrementing a pointer,
and free everything at once when the process dies or a GC cycle collects the
arena.

### 9.4 Per-Process Heap

Each process owns a `ProcessHeap` backed by a `bumpalo::Bump` arena:

- **Allocation**: `alloc_cons()`, `alloc_tuple()`, `alloc_float()` — each is
  a single pointer bump, O(1).
- **GC trigger**: when `bytes_allocated >= gc_threshold` (default 256 KiB).
- **Process exit**: `drop(process)` drops the entire arena instantly. No
  per-object cleanup, no finalizers.

### 9.5 Copying Garbage Collector

When the heap threshold is exceeded, a **Cheney's copying collector** runs:

1. Allocate a fresh `bumpalo::Bump` arena (to-space).
2. Identify roots: stack frames, mailbox messages, process dictionary.
3. Traverse each root. For every heap-allocated term:
   - If already forwarded (seen before), return the forwarded address.
   - Otherwise, allocate space in to-space, copy the object, record a
     forwarding entry in a `HashMap<old_addr, new_addr>`.
   - Recursively copy children (tuple elements, cons head/tail).
4. Swap to-space into `self.arena`, drop old arena (all garbage freed at once).

**Key properties**:
- Only the process being collected is paused. No other process is affected.
- Cost is proportional to **live data**, not total heap size.
- After GC, the heap is compacted with no fragmentation.

**Binary virtual heap**: The GC also tracks large binary allocations separately
via `binary_vheap_size` / `binary_vheap_threshold`. The threshold adapts: grows
to 2× live binary size after GC, shrinks to the default (256 KiB) when swept.
This prevents large binary allocations from deferring GC indefinitely.

### 9.6 When Each Layer Applies

| Erlang Pattern                        | Memory Strategy                     |
|---------------------------------------|-------------------------------------|
| Temporary tuple in local computation  | Layer 1: stack (escape analysis)    |
| List built and returned from function | Layer 2: arena bump allocation      |
| Message sent to another process       | Deep copy into target's arena       |
| gen_server state accumulating data    | Layer 2 → Layer 3 GC at threshold   |
| Short-lived spawned worker            | Layer 2, freed on exit (no GC)      |
| Large binary (file contents)          | Layer 4: Arc<BinaryData> (implemented) |

### 9.7 Process Lifetime Distribution

For a typical Erlang application:

- **~70% of processes**: spawn, do work, die. Arena freed on death. **Zero GC
  cost.**
- **~25% of processes**: gen_server with moderate state. Arena + occasional
  minor GC when threshold exceeded.
- **~5% of processes**: long-lived, large state. Full GC management (same as
  BEAM).

This distribution means the GC is nearly invisible for the majority of
processes, and when it does run, it is scoped to a single process with no
global coordination.

---

## 10. Message Passing

### 10.1 Design Principle: Complete Process Isolation

Processes share no heap memory. When process A sends a term to process B,
the term is **deep-copied** from A's heap into B's heap. This guarantees:

- No data races: B owns its copy and can GC it independently.
- No coordination: A's GC never needs to ask B about liveness.
- No use-after-free: dropping A's heap cannot invalidate B's data.

### 10.2 Deep Copy Algorithm

The `deep_copy_term()` function walks the term recursively:

- **Immediates** (small integers, atoms, PIDs, nil): returned as-is. No
  allocation needed — the value is entirely in the 64-bit word.
- **Cons cells**: allocate a new cell on the target heap, recursively copy
  head and tail. A forwarding table (`HashMap<usize, ErlTerm>`) ensures
  shared sub-structure is copied only once.
- **Boxed objects** (tuples, floats, etc.): allocate header + payload words
  on the target heap, recursively copy elements for tuples.

For the common case of simple messages (tuples of immediates + refc binaries),
`direct_copy_message()` provides a fast path that skips the HashMap forwarding
table entirely, using batch allocation for tuple elements.

### 10.3 Send/Receive Mechanics

**Sending** (`Pid ! Msg`):

1. Compiled code calls `nera_rt_send_message(ctx, pid_term, msg)`, or for
   fused sends, `nera_rt_send_tuple(ctx, pid, arity, e0, e1, e2, e3)`.
2. The runtime resolves the target PID (or registered name) via the process
   table (with thread-local cache for repeated sends to the same PID).
3. **Direct-to-heap** (fast path): If the target is WAITING or READY and
   `try_lock()` succeeds, the message is copied directly to the receiver's
   heap using `direct_copy_message()`. No fragment allocation needed.
4. **Lock-free fragment** (fallback): On contention, a `MessageFragment` is
   created and pushed to the receiver's `SegQueue` inbox.
5. If the target was in `Waiting` state, CAS transitions it to `Ready` and
   injects it into the scheduler's work queue.
6. The send expression evaluates to the sent message (Erlang convention).

**SendDirect fusion** (IR optimization):

The `fuse_make_tuple_send` pass detects single-use tuples immediately sent to
another process (`MakeTuple` + `Send` in the same basic block) and fuses them
into a `SendDirect` instruction. This avoids allocating the tuple on the sender's
heap entirely — `nera_rt_send_tuple` constructs the tuple directly on the
receiver's heap (or builds an `InlineTuple`/`SimpleTuple` fragment for the
lock-free path). Supports arity ≤ 4.

**Receiving** (`receive ... end`):

1. Compiled code calls `nera_rt_receive_msg(ctx)` to attempt matching.
2. The runtime scans the mailbox from the current scan position.
3. If a message is found, it is removed from the mailbox and returned.
4. If no message matches, `nera_rt_receive_wait(ctx)` sets
   `STATUS_RECEIVE_WAIT` and the process returns to the scheduler.
5. The scheduler marks the process as `Waiting` and re-injects it into the
   global queue.
6. When a new message arrives (step 5 of sending), the process is woken.

### 10.4 Selective Receive

Erlang's `receive` is not a simple FIFO dequeue — it scans the mailbox for
the **first message matching any clause pattern**. The mailbox maintains a
`scan_pos` index:

- On a new `receive`, scan starts from position 0.
- Each `try_match_next()` advances the scan position (skipping tombstones).
- When a match is found, `remove_at(index)` writes a **tombstone sentinel**
  (`0b110`, an unused tag) instead of shifting all later elements — O(1)
  removal instead of O(n).
- Leading tombstones are trimmed immediately (head advances), so the common
  case (accept-first-message) stays zero-overhead.
- Lazy compaction reclaims tombstoned slots when the ratio exceeds 50%,
  triggered at `reset_scan()` or when `send()` needs to grow the buffer.
- Unmatched messages remain in the mailbox for future receives.
- The JIT-inlined fast paths for `receive_peek` and `receive_next` skip
  tombstones via a compact loop in the generated Cranelift IR.

### 10.5 Optimization: Reference-Counted Large Binaries (Implemented)

For large binaries (>64 bytes), deep copying is wasteful. The runtime uses
`Arc<BinaryData>` — an atomically reference-counted
shared buffer. Since Erlang binaries are immutable, sharing is safe. The
tagged term would carry a pointer to a `RefcBinaryRef` on the process heap,
which in turn points to the shared `Arc`.

`BinaryData` owns a private `Vec<u8>` and exposes a stable `#[repr(C)]`
data-pointer prefix. Generated code uses `offset_of!` for that prefix and
never reads the Vec's private layout. The buffer is moved in without copying
or reallocating; writes are restricted to exclusive construction before
publication or borrowing. The owner adds one pointer-sized field, while
per-process descriptors remain three words: header (bit length), owner
pointer, and byte offset. Allocation checks that the selected window fits
in the backing buffer.


```
Process A heap:                    Shared:
┌─────────────────┐               ┌─────────────────┐
│ RefcBinaryRef   │──────────────►│ Arc<BinaryData>    │
│  header         │               │  refcount = 2   │
│  *Arc ptr       │               │  data: [...]    │
│  offset, size   │               └─────────────────┘
└─────────────────┘                        ▲
                                           │
Process B heap:                            │
┌─────────────────┐                        │
│ RefcBinaryRef   │────────────────────────┘
│  header         │
│  *Arc ptr       │
│  offset, size   │
└─────────────────┘
```

No cycles are possible (Erlang data is immutable), so reference counting is
always sound and a cycle collector is never needed.

---

## 11. Function Dispatch and FFI

### 11.1 Function Table

Compiled functions are registered at startup in a global `FunctionTable`:

```
  Key: (module_atom_id, function_atom_id, arity) → Value: extern "C" fn pointer
```

The generated `main()` calls `nera_rt_register_function()` for each exported
function before starting the scheduler. `spawn/3` uses this table to look up
the entry function for a new process.

### 11.2 C-ABI Bridge

All interaction between compiled code and the runtime uses `extern "C"`
functions. The compiled code receives a `*mut ProcessContext` as its first
argument, through which it accesses:

- The reduction counter (to decrement and check for yield).
- The heap pointer (to allocate terms via FFI functions).
- The mailbox pointer (to send/receive messages).
- The process PID (for `self()`).

### 11.3 FFI Function Catalog

| FFI Function                   | Purpose                                    |
|--------------------------------|--------------------------------------------|
| `nera_rt_alloc_cons`           | Allocate cons cell on process heap         |
| `nera_rt_alloc_tuple`          | Allocate tuple on process heap             |
| `nera_rt_alloc_float`          | Allocate boxed float on process heap       |
| `nera_rt_list_head`/`_tail`    | Extract head/tail of cons cell             |
| `nera_rt_tuple_element`        | Extract tuple element (0-indexed)          |
| `nera_rt_tuple_size`           | Get tuple arity                            |
| `nera_rt_io_format`/`_1`       | Formatted output (io:format/1,2)           |
| `nera_rt_get_self`             | Get current process PID (self/0)           |
| `nera_rt_length`               | List length (length/1)                     |
| `nera_rt_hd`/`_tl`             | List head/tail (hd/1, tl/1)                |
| `nera_rt_element`              | Tuple element (1-indexed, element/2)       |
| `nera_rt_is_integer`/`_atom`/… | Type check BIFs                            |
| `nera_rt_intern_atom`          | Intern atom string, return atom term       |
| `nera_rt_send_message`         | Send message to PID (deep copy)            |
| `nera_rt_send_tuple`           | Fused send: build tuple on receiver heap   |
| `nera_rt_receive_msg`          | Try to receive next mailbox message        |
| `nera_rt_receive_wait`         | Signal receive-wait (no matching message)  |
| `nera_rt_spawn`                | Spawn new process by MFA                   |
| `nera_rt_register_function`    | Register compiled function in global table |
| `nera_runtime_init`            | Initialize runtime (atom table)            |
| `nera_runtime_start_mfa`       | Start scheduler with given entry MFA       |

---

## 12. Reduction Injection in Compiled Code

### 12.1 The Challenge

In an interpreter (BEAM), the eval loop naturally checks reductions between
bytecode instructions. In a native compiler, reduction checks must be
**injected by the compiler** into the generated code at appropriate yield
points.

### 12.2 Current Strategy: Function-Entry Yield Points

The compiler inserts a reduction check at the entry of every compiled
function:

```
compiled_function(ctx, args, nargs):
    ctx->reductions -= 1
    if ctx->reductions <= 0:
        ctx->status = STATUS_YIELD
        return
    ... function body ...
```

This means a process can execute at most 4,000 function calls before yielding.
For typical Erlang code (many small functions, recursive loops), this provides
fine-grained scheduling.

### 12.3 Current Limitation

If a single function body performs a long computation without calling other
functions (e.g., a tight arithmetic loop), it will not yield until it
returns. This is acceptable for the current prototype because:

- Erlang idioms favour many small functions and recursive loops.
- BIFs (implemented in Rust) can manually check reductions.

### 12.4 Future: Mid-Function Yield via CPS or Stack Switching

Two strategies are planned for true preemption within a function body:

**Strategy A: Loop back-edge instrumentation**

Insert reduction checks at loop back-edges (the `jump` instruction that loops
back to a block header). This catches the most common case: tail-recursive
server loops and `lists:map`-style iteration.

**Strategy B: Stackful coroutines**

Give each process its own stack (4-8 KiB, growable). On yield, save/restore
register state via a small assembly context-switch routine (~20 lines per
platform). This is what Go does and provides the most transparent model:

```
yield_point(ctx):
    ctx.reductions -= 1
    if ctx.reductions <= 0:
        context_switch(process_stack, scheduler_stack)
        // Execution resumes here when rescheduled
```

This approach is recommended for Phase 5 because it simplifies the compiler
(functions look normal, no CPS transform) and matches the proven Go model.

---

## 13. Tail Call Optimization

### 13.1 Importance

Erlang programs rely heavily on tail recursion for loops. A gen_server's main
loop is an infinite tail-recursive call. Without TCO, the stack would overflow
within seconds.

### 13.2 Current State

The IR lowering detects tail-position calls and emits `TailCall` terminators.
Two optimizations are implemented:

1. **Self-recursion to loop**: Direct self-recursive tail calls are lowered to
   `jump` back to the function entry. Zero overhead. Covers gen_server loops
   and most Erlang patterns.

2. **Cranelift `return_call`**: Erlang functions use `CallConv::Tail`. Non-self
   tail calls emit `return_call` (Cranelift instruction) which reuses the
   caller's stack frame. Handles mutual tail recursion.

FFI/wrapper functions use `SystemV` convention. Cross-convention calls
(wrapper→Erlang, Erlang→FFI) are handled by Cranelift.

---

## 14. Future Subsystems

### 14.1 Process Links and Monitors (Implemented)

Both process links and monitors are fully implemented.

**Links** (bidirectional):

- When process A links to B, an abnormal exit in either propagates to the
  other.
- If the receiving process has `trap_exit` set, the exit signal is converted
  to a `{'EXIT', Pid, Reason}` message.
- `kill` signals are untrappable.

**Monitors** (unidirectional):

- When process A monitors B, A receives `{'DOWN', Ref, process, B, Reason}`
  when B exits.
- No propagation of failure — monitors are purely observational.

These are the foundation for OTP supervision trees.

### 14.2 ETS (Erlang Term Storage) (Implemented)

ETS is fully implemented with HashMap-backed tables, mmap-backed region pools (`EtsRegionPool`), and comprehensive API support including `update_counter/3`, `insert_new/2`, `tab2list/1`, `first/1`/`next/2`, and `info/1,2`.

ETS tables provide **mutable shared-memory storage** accessible by any
process — the single exception to Erlang's no-shared-memory rule.

Design considerations for the Rust implementation:

- **Table types**: `set` (hash table, unique keys), `ordered_set` (B-tree),
  `bag`, `duplicate_bag`.
- **Concurrency**: striped `RwLock` per hash bucket (same strategy as BEAM)
  for `set`/`bag`. Single `RwLock` for `ordered_set`.
- **Term ownership**: ETS entries are deep-copied in and out, maintaining
  process isolation. `insert` copies from process heap to ETS storage.
  `lookup` copies from ETS storage to process heap.
- **Access modes**: `public`, `protected` (read-any, write-owner-only),
  `private` (owner-only).

### 14.3 Timer Wheel (Implemented)

Implemented as a min-heap (`BinaryHeap`) of `TimerEntry` with lazy cancellation via `pid_to_timer` map. A dedicated `nera-timer` thread sleeps until the next deadline, pops expired entries, and wakes processes. Its wait keeps wakes pending, so a timer inserted while it computes its sleep is not missed. On macOS the wait is a kqueue whose receive-deadline timer is marked `NOTE_CRITICAL`, so it fires on time even when macOS coalesces the timers of a utility- or background-priority process; other platforms use a condvar. `send_message` and `propagate_exit_signals` cancel timers when waking processes before their timeout fires.

### 14.4 Dirty Schedulers

For long-running operations that would block a normal scheduler thread:

- **Dirty CPU schedulers**: N threads (equal to core count) for CPU-bound
  NIFs.
- **Dirty I/O schedulers**: 10 threads (BEAM default) for blocking I/O.
- Normal schedulers never block — if a BIF would block, it is dispatched to
  a dirty scheduler.

### 14.5 Binary Pattern Matching (Implemented)

Fully implemented including bit-level segments (`<<X:4, Y:4>>`), match contexts (`BOXED_MATCH_CTX`), variable-size segments, endianness/sign specifiers, and UTF-8/UTF-16/UTF-32 types.

Erlang's bit syntax is one of its most distinctive features:

```erlang
<<Type:8, Length:16/big, Payload:Length/binary, Rest/binary>> = Data
```

Implementation in compiled code:

- A `MatchContext` struct tracks a pointer + bit offset into binary data.
- The compiler emits sequential read operations: `read_uint(8)`,
  `read_uint_big(16)`, `read_binary(Length * 8)`, etc.
- Sub-binaries share the underlying data (offset + length into the original)
  without copying.

### 14.6 Distribution Protocol

For multi-node Erlang systems:

- **External Term Format**: serialize/deserialize terms to bytes for network
  transmission.
- **Connection management**: TCP connections between nodes with the Erlang
  distribution handshake.
- **Transparent messaging**: `Pid ! Msg` works the same whether Pid is local
  or remote; the runtime routes to the appropriate node.

---

## 15. Compilation Pipeline Integration

### 15.1 How the Runtime Connects to Generated Code

```
  1. nera parses .erl source → AST          (nera-syntax)
  2. AST is lowered to SSA-like IR            (nera-ir)
     - Pattern matches compiled to decision trees (Maranget algorithm)
     - Tail calls detected and marked
  3. IR is translated to Cranelift IR         (nera-codegen)
     - Each Erlang function → Cranelift function
     - Reduction check injected at entry
     - BIF calls → extern "C" calls to nera_rt_* functions
     - Term operations → calls through ProcessContext
  4. Cranelift emits native object files (.o)
  5. Object files are statically linked with libnera_runtime.a
  6. Generated main() function:
     a. Calls nera_runtime_init() to initialize atom table
     b. Calls nera_rt_intern_atoms_batch() for all atoms (single FFI call)
     c. Calls nera_rt_register_functions_batch() for all functions (single FFI call)
     d. Calls nera_runtime_start_mfa() with the entry module:function
     e. The scheduler starts, spawns the initial process, and runs
        until all processes exit or deadlock
```

### 15.2 Generated Code Example

For an Erlang function like:

```erlang
factorial(0) -> 1;
factorial(N) -> N * factorial(N - 1).
```

The compiler generates (conceptually):

```
factorial(ctx, args, nargs):
    // Reduction check
    ctx->reductions -= 1
    if ctx->reductions <= 0:
        ctx->status = YIELD
        return

    N = args[0]

    // Clause 1: factorial(0) -> 1
    if N == SmallInt(0):
        ctx->result = SmallInt(1)
        ctx->status = OK
        return

    // Clause 2: factorial(N) -> N * factorial(N - 1)
    sub_args = [N - 1]
    call factorial(ctx, sub_args, 1)
    if ctx->status != OK: return  // propagate yield/exception
    result = ctx->result
    ctx->result = N * result
    ctx->status = OK
    return
```

---

## 16. Build and Deployment

### 16.1 Crate Structure

```
nera/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── nera-syntax/             # Lexer (logos) + Parser + AST
│   ├── nera-ir/                 # AST → IR + pattern match compiler
│   ├── nera-codegen/            # IR → Cranelift → .o + linker
│   ├── nera-runtime/            # Runtime: terms, scheduler, GC, BIFs
│   └── nera/                    # CLI driver
├── examples/                     # Test programs
├── tests/                        # Integration tests
└── docs/                         # Documentation
```

### 16.2 Runtime Crate Dependencies

| Dependency         | Purpose                                              |
|--------------------|------------------------------------------------------|
| `bumpalo`          | Bump-allocated arenas for per-process heaps          |
| `crossbeam-deque`  | Lock-free work-stealing deques for the scheduler     |
| `crossbeam-utils`  | Concurrent utilities                                 |
| `parking_lot`      | Fast RwLock/Mutex for atom table, process table      |
| `num_cpus`         | Detect CPU count for scheduler thread configuration  |
| `crossbeam-queue`  | Lock-free MPSC mailbox (SegQueue) for message passing |
| `crossbeam-channel`| Communication between scheduler, I/O pool, and reactor|
| `mio`              | Event-driven I/O reactor (epoll/kqueue)               |
| `tikv-jemallocator`| jemalloc global allocator (non-Windows)               |
| `tikv-jemalloc-ctl`| jemalloc statistics for memory_stats/0                |
| `libc`             | mmap/munmap for ETS region pools                      |

### 16.3 Static Linking

The runtime crate builds as both a Rust library (`lib`) and a C static
library (`staticlib`). The codegen crate links the generated object files
against `libnera_runtime.a` using the system linker (`cc`), producing a
single self-contained executable.

---

## 17. Design Decisions Summary

| Decision                        | Choice                     | Rationale                                                |
|---------------------------------|----------------------------|----------------------------------------------------------|
| Code generator backend          | Cranelift                  | Pure Rust, fast compilation, JIT-ready. ~20-30% slower code than LLVM, acceptable for prototype. |
| Term representation             | 64-bit tagged pointers     | Matches BEAM. Immediates require no allocation. Comparisons are fast integer ops. |
| Heap allocation strategy        | Per-process bump arena     | O(1) allocation. No contention between processes. Instant free on process death. |
| Garbage collector               | Cheney's copying per-process| No global pauses. Cost proportional to live data. Simple implementation. |
| Message passing                 | Deep copy between heaps    | Matches BEAM. Enables independent per-process GC. No shared-memory hazards. |
| Scheduler                       | N:M work-stealing (crossbeam)| One OS thread per core. Lock-free local queues. Proven design. |
| Preemption                      | Reduction counting         | Simple, deterministic, portable. No signal handlers or stack switching. |
| Concurrency model               | BEAM-style (not Go-style)  | Process isolation enables per-process GC, fault isolation, and reasoning about correctness. |
| Linking                         | Static                     | Single binary deployment, no runtime dependencies. Like Go. |
| Atom table                      | Global RwLock + interning  | Read-heavy workload. parking_lot RwLock is fast for concurrent reads. |
| Function dispatch               | Global HashMap of MFA→fn   | Simple, fast lookup. Sufficient until hot code loading is needed. |

---

## 18. Roadmap

### Phase 1 — Core Language ✅

Lexer, parser, IR, Cranelift codegen, basic runtime, static linking.

### Phase 2 — Concurrency ✅

Receive with selective mailbox scanning, spawn/1 and spawn/3,
per-process heap, true tail call optimization (self-recursion to loop +
`return_call`), process links, monitors, registered processes, timer wheel.

### Phase 3 — Language Completeness ✅

Anonymous functions (closures), try/catch/throw, list comprehensions,
records (desugared to tuples at IR level), binaries/bit syntax (including bit-level),
maps (create/update/pattern matching), binary comprehensions.

### Phase 4 — Runtime Hardening ✅

ETS with mmap-backed region pools, lock-free MPSC mailbox, mio reactor
for event-driven I/O, TCP active mode, binary match contexts, refc binaries,
back-edge yield checks, inline BIF calls (hd/tl/type checks).

### Phase 5 — Standard Library ✅

Stdlib: lists, maps, gen_server, supervisor, application, proc_lib, timer.
BIF/Erlang split for hot-path vs higher-order functions.

### Phase 6 — Production & Distribution (Current)

Optimization passes, Rust interoperability, cross-compilation, extended
stdlib, benchmarking against BEAM.

### Phase 7 — Distribution and OTP (Long-term)

Multi-node distribution, hot code loading, additional OTP behaviours
(gen_statem, gen_event), ports and NIFs.

---

## Appendix A: Comparison of GC-Free Approaches

| Strategy                      | Pros                                     | Cons                                                 | Verdict         |
|-------------------------------|------------------------------------------|------------------------------------------------------|-----------------|
| Arc\<T\> everywhere           | No GC, no cycles (Erlang immutable)      | 10-30x slower allocation, atomic ops on every clone  | Too slow        |
| Arena per process, free on death | Zero GC for short-lived processes     | Long-lived processes leak forever                    | Partial solution|
| Arena + epoch rotation        | Better than pure arena                   | Cannot free old arenas without knowing what's alive  | Re-invents GC   |
| Arena + compiler escape analysis | Stack-allocate temporaries            | Only helps with local values; escaping terms still need heap | Partial solution |
| **Hybrid (nera approach)**   | Best of all worlds: stack + arena + lazy GC + refcount | Complexity of multiple layers | **Chosen**      |

## Appendix B: BEAM vs nera Feature Parity

| BEAM Feature                  | nera Status     | Notes                                              |
|-------------------------------|------------------|----------------------------------------------------|
| Lightweight processes         | ✅ Working       | Per-process heap, reduction counting               |
| Message passing               | ✅ Working       | Lock-free MPSC, deep copy, selective receive       |
| Work-stealing scheduler       | ✅ Working       | N threads, crossbeam deques, back-edge yield       |
| Pattern matching              | ✅ Working       | Maranget decision trees                            |
| Per-process GC                | ✅ Working       | Generational copying GC + binary vheap             |
| Process hardening             | ✅ Working       | fullsweep_after, max_heap_size, hibernate, spawn_opt |
| Tail call optimization        | ✅ Working       | Self-recursion to loop + `return_call`             |
| Anonymous functions           | ✅ Working       | Closures with captured env, inter-process deep copy|
| Try/catch                     | ✅ Working       | Full try...of...catch...after...end                |
| Binaries / bit syntax         | ✅ Working       | Bit-level segments, match contexts, refc binaries  |
| Maps                          | ✅ Working       | Create, update, pattern matching                   |
| Records                       | ✅ Working       | Desugared to tuples at IR level                    |
| ETS                           | ✅ Working       | HashMap-backed, mmap region pools                  |
| Process links/monitors        | ✅ Working       | Bidirectional links, unidirectional monitors       |
| Process registry              | ✅ Working       | register/2, whereis/1, send-by-name                |
| Timers                        | ✅ Working       | Timer wheel, send_after, receive...after           |
| TCP sockets                   | ✅ Working       | gen_tcp with mio reactor, active mode              |
| gen_server / supervisor       | ✅ Working       | Erlang stdlib modules                              |
| Distribution                  | 🔮 Long-term    | External term format + network layer               |
| Hot code loading              | 🔮 Long-term    | Requires function dispatch indirection             |
