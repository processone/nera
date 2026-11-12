# Lessons from Pony for nera

Pony is an actor-model language compiled to native code (via LLVM) that solves many
problems relevant to nera. This document captures the key takeaways and how they
inform our design decisions.

---

## Background: Pony vs Erlang Architecture

Pony and Erlang share the actor model but make opposite tradeoffs at the memory level:

|                          | Pony                                      | Erlang / nera                  |
|--------------------------|-------------------------------------------|---------------------------------|
| **Heap model**           | Shared heap with cross-actor aliasing     | Isolated per-process heaps      |
| **Data-race prevention** | Type system (reference capabilities)      | Architecture (no shared memory) |
| **Message passing**      | Zero-copy (pointer transfer)              | Deep copy                       |
| **GC**                   | Orca (cross-actor reference tracking)     | Per-process (independent)       |
| **Blocking receive**     | No (behaviours are async)                 | Yes (`receive` blocks)          |
| **Type system**          | Static, with 6 reference capabilities     | Dynamic                         |
| **Learning curve**       | Steep (capabilities are the #1 complaint) | Gentle                          |

---

## Key Insights

### 1. Erlang Is Already Data-Race Free

Pony needs reference capabilities (`iso`, `val`, `trn`, `ref`, `box`, `tag`) to prove
data-race freedom because it allows cross-actor aliasing and must restrict what each
alias can do.

Erlang took the more radical approach: no shared memory at all. Every `!` goes through
`deep_copy_term()`. Every process has its own heap. Data-race freedom is an
architectural invariant, not a type-system proof.

**Implication**: We do not need a capability type system. We already have the safety
guarantee. What we lack are the *performance* benefits Pony derives from aliasing.

### 2. Per-Actor GC Scales (Orca)

Pony's Orca GC collects each actor's heap independently with no stop-the-world pauses
and no cross-actor coordination. Each actor has ~240 bytes of overhead, enabling tens
of millions of actors.

nera implements a similar per-process generational GC with no stop-the-world pauses.
Each process owns an isolated two-generation heap: a young generation (multi-chunk bump
allocator) and an old generation (arena allocator). Minor GC copies live young-gen data
to old gen; major GC copies all live data into a fresh arena. The scheduler triggers
collection during the process's time slice based on adaptive thresholds that grow or
shrink with the workload. No write barriers are needed because Erlang data is immutable.
Large binaries live off-heap as `Arc<Vec<u8>>` with virtual heap tracking to trigger GC
when binary pressure accumulates.

Remaining future work: GC pause time profiling, and potentially incremental or
concurrent collection for very large heaps.

### 3. Zero-Copy Messaging Without Shared Memory

Pony achieves zero-copy by proving at compile time that sent data is either isolated
(single owner, transferable) or immutable (safely shareable). We approximate this
with targeted optimizations that don't require type annotations:

**MakeTuple + Send fusion (implemented)**: The IR lowering pass detects single-use
tuples (arity ≤ 4) immediately followed by a send, and fuses them into a `SendDirect`
instruction. This constructs the tuple directly on the receiver's heap, skipping
sender-side allocation entirely. This covers the common `Pid ! {tag, Data}` pattern.

**Direct-to-heap send (implemented)**: When the receiver is in a waiting or ready
state, the sender tries to lock the receiver and copy directly to its heap, bypassing
the lock-free `SegQueue` intermediate allocation.

**Refcounted immutable binaries (implemented)**: Large binaries are immutable in
Erlang. They live off-heap as `Arc<Vec<u8>>`, shared across processes with only an
atomic refcount increment on send — no data copy. This matches Pony's `val` semantics
and is exactly what BEAM does for refc binaries.

**Compile-time constant sharing**: Terms built entirely from literals can live in a
read-only `.rodata` segment, shared by all processes with no refcount.

**Full move semantics (future work)**: General dead-after-send detection via liveness
analysis — when a variable is never used after `!`, the compiler could transfer its
entire heap subgraph instead of copying. This is equivalent to Pony's `iso` capability.
The current `SendDirect` fusion handles the most common case; general move semantics
would extend this to arbitrary term shapes and sizes.

### 4. Work-Stealing Scheduler with Backpressure

Pony's scheduler uses work-stealing (idle threads steal from busy ones) and actor-level
backpressure (sending to an overloaded actor "mutes" the sender, preventing mailbox
explosion).

Our scheduler currently round-robins across ready processes. Work-stealing would
improve CPU utilization for uneven workloads. Backpressure would prevent the classic
"fast producer, slow consumer" OOM.

### 5. Deadlock Prevention vs Detection

Pony prevents deadlocks by not having blocking receive — all behaviours are async.
Erlang's `receive` blocks, which means deadlocks are possible.

We can't change Erlang's semantics, but we can add compiler warnings for detectable
deadlock patterns:
- `receive` without `after` timeout and no visible sender matching the patterns
- Self-deadlock: `self() ! Msg` followed by blocking `receive`
- Circular wait between two functions that both send-and-receive

### 6. Compile-Time Warnings (Safety Without Restrictions)

Pony's type checker rejects programs at compile time. This is powerful but comes at a
steep learning cost (reference capabilities are Pony's biggest adoption barrier).

A better approach for Erlang is Rust-style warnings: the compiler notices suspicious
patterns and warns, but never blocks compilation. This covers:
- Unreachable clauses (dead branches after catch-all patterns)
- Receive without timeout
- ETS read-modify-write race patterns (when ETS is implemented)
- Unused variables and dead code

---

## Architectural Decision: No Shared Aliasing

After studying Pony's design, we decided **not** to adopt cross-process memory aliasing.

### Rationale

**GC complexity**: Pony's Orca GC is the hardest part of their runtime — years of
development for correct cross-actor reference tracking. Our per-process GC is trivially
simple by comparison.

**Fault isolation**: Erlang's "let it crash" works because a crashing process takes
nothing with it except its own heap. Shared aliasing weakens this — if process A
crashes, shared objects referenced by B and C need careful cleanup.

**Distribution**: Erlang's distribution works because messages are self-contained.
Shared aliasing would require two code paths (local: share pointer, remote: serialize),
breaking location transparency.

**Complexity budget**: Full aliasing would cost 6+ months of development for marginal
gains over the targeted optimizations described above, which cover 80-90% of the
performance benefit.

### What We Do Instead

Targeted zero-copy optimizations that preserve heap isolation:

1. **MakeTuple + Send fusion** for single-use tuples sent immediately (done)
2. **Direct-to-heap send** bypassing SegQueue when receiver is idle (done)
3. **Refcounted binaries** for large immutable data — Arc clone only (done)
4. **Constant sharing** for compile-time literals (read-only segment)
5. **Skip copying immediates** in compound terms made entirely of atoms/small ints
6. **Full move semantics** for variables dead after send (future — liveness-based)

This gets most of Pony's performance benefits with none of the architectural cost.

---

## Summary Table

| Pony Feature             | nera Approach              | Mechanism                |
|--------------------------|-----------------------------|--------------------------|
| `iso` (unique mutable)   | SendDirect fusion + future move | Tuple fusion (done), general liveness (future) |
| `val` (shared immutable) | Refcounted binaries         | `Arc<Vec<u8>>` shared heap (done) |
| `tag` (opaque identity)  | PIDs                        | Already done             |
| `ref` (local mutable)    | All heap data               | Already done (isolation) |
| Orca GC                  | Per-process generational GC | Copying GC, isolated heaps (done) |
| Work-stealing scheduler  | Work-stealing scheduler     | Same technique           |
| Backpressure             | Mailbox depth tracking      | Reduction throttling     |
| No blocking receive      | Deadlock warnings           | IR analysis pass         |

---

## References

- Orca GC paper: "Ownership and Reference Counting Based Garbage Collection in the Actor World" (Clebsch & Drossopoulou, 2016)
- Pony reference capabilities: https://tutorial.ponylang.io/reference-capabilities/
- Pony runtime architecture: https://www.ponylang.io/faq/runtime/
- Wallaroo production experience: https://blog.wallaroolabs.com/2017/10/why-we-used-pony-to-write-wallaroo/
