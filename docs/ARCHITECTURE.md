# nera — Architecture

## Vision

**nera** is an Erlang-to-native compiler written entirely in Rust. Both the compiler itself and the programs it produces are **standalone single binaries**:

- **The compiler** (`nera`) — a single Rust binary with no dependency on LLVM, GCC toolchain, or the Erlang VM. Built with `cargo build`, it only requires the system linker (`cc`) at compile time.
- **Compiled programs** — each `.erl` file becomes a self-contained native executable with the runtime statically linked in. No Erlang VM, no BEAM files, no shared libraries needed at run time.

It works like Go: one source file in, one native executable out.

The compiler embeds a lightweight Erlang runtime written in Rust, providing:
- Erlang-style lightweight processes with per-process heap isolation
- Work-stealing scheduler with reduction counting and back-edge yield checks
- Per-process garbage collection (no global stop-the-world pauses)
- Lock-free deep-copy message passing between process heaps
- Event-driven I/O via mio reactor (epoll/kqueue)
- ETS tables with mmap-backed region pools
- Timer wheel for `receive...after` and `timer:send_after`

---

## Current Status

**Working compiler** — compiles multi-module Erlang programs to native binaries with full concurrency support.

```
$ nera examples/ping_pong.erl -o ping_pong
$ ./ping_pong
```

- **600 unit tests** across 5 crates + **220 end-to-end tests** in `tests/`
- Benchmarked against BEAM OTP 28 (see Benchmarks section)
- Tested on macOS arm64 (Apple Silicon) and Linux x86_64
- Standard library: 7 Erlang modules auto-compiled with user code

---

## Architecture Overview

```
                    ┌─────────────┐
                    │  .erl files │
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │   Parser    │  nera-syntax
                    │ (logos +    │  Lexer → AST with atom interning
                    │  Pratt RD)  │  Preprocessor (-define, -ifdef, -include)
                    └──────┬──────┘
                           │ AST
                    ┌──────▼──────┐
                    │  IR Lowering│  nera-ir
                    │  + Pattern  │  AST → mutable-register IR
                    │  Compiler   │  Pattern match decision trees (Maranget)
                    │  + Warnings │  Static analysis passes
                    └──────┬──────┘
                           │ IR (BasicBlocks + Terminators)
                    ┌──────▼──────┐
                    │  Cranelift  │  nera-codegen
                    │  Code Gen   │  IR → native object files
                    │  + Linker   │  Static link with runtime
                    └──────┬──────┘
                           │
              ┌────────────▼────────────┐
              │   Standalone Binary     │
              │  ┌────────────────────┐ │
              │  │  Compiled Erlang   │ │
              │  │  (native code)     │ │
              │  ├────────────────────┤ │
              │  │  nera-runtime      │ │  nera-runtime
              │  │  (statically linked│ │  Scheduler, processes,
              │  │   Rust runtime)    │ │  GC, message passing,
              │  │                    │ │  I/O reactor, ETS
              │  └────────────────────┘ │
              └─────────────────────────┘
```

The precise current representation and its verification limits are documented in
[the IR contract](IR_CONTRACT.md). Nera IR is not strict SSA; the Cranelift
frontend constructs SSA during native translation.

### Compilation Pipeline

```
nera file1.erl [file2.erl ...] -o binary [-e module:function] [--emit-ir] [--no-stdlib]

1. Discover   → Find stdlib .erl files, prepend to compilation list
2. Preprocess → Expand macros, -define, -ifdef, -include
3. Parse      → Source to AST with atom interning
4. Lower      → AST to mutable-register IR with pattern match compilation
5. Warnings   → Static analysis (unused vars/funs, shadowed vars, unreachable clauses)
6. Codegen    → IR to Cranelift IR → native object files (.o)
7. Link       → Static link objects + libnera_runtime.a → single binary
```

Entry point defaults to `main:start` and can be overridden with `-e module:function`.

---

## Crate Structure

```
nera/
├── Cargo.toml                         # Workspace root
├── crates/
│   ├── nera-syntax/                  # Lexer + Parser + AST + Preprocessor
│   ├── nera-ir/                      # AST → IR lowering + pattern compiler + warnings
│   ├── nera-codegen/                 # IR → Cranelift → object files + linker
│   ├── nera-runtime/                 # Embedded runtime: terms, scheduler, GC, I/O, ETS
│   └── nera/                         # CLI driver binary
├── erlang/stdlib/                     # Standard library (Erlang source, auto-compiled)
├── examples/                          # Example .erl programs
├── tests/                             # End-to-end integration tests
├── benchmarks/                        # Performance benchmarks (nera vs BEAM)
├── scripts/                           # Distribution and install scripts
└── docs/                              # Documentation
```

| Crate          | Purpose                                                                                           | Key Dependencies                                                                           |
|----------------|---------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------|
| `nera-syntax`  | Lexer + recursive descent parser with Pratt operator precedence + preprocessor                    | `logos`, `string-interner`                                                                 |
| `nera-ir`      | AST-to-IR lowering, pattern match compilation (Maranget decision trees), static analysis warnings | `nera-syntax`                                                                              |
| `nera-codegen` | Cranelift code generation (x86_64 + arm64), object emission, static linking                       | `cranelift-*` (v0.116), `nera-ir`, `nera-runtime`                                          |
| `nera-runtime` | Lightweight Erlang runtime: tagged terms, scheduler, GC, BIFs, I/O reactor, ETS                   | `bumpalo`, `crossbeam-deque`, `crossbeam-queue`, `mio`, `parking_lot`, `tikv-jemallocator` |
| `nera`         | CLI driver: discover stdlib → parse → lower → codegen → link                                      | `clap`, `toml`, `serde`                                                                    |

### Crate Details

#### nera-syntax

```
src/
├── lib.rs
├── token.rs          # Token definitions
├── lexer.rs          # logos-based lexer
├── ast.rs            # AST node types (Expr, Pattern, Module, etc.)
├── parser.rs         # Recursive-descent parser with Pratt precedence
├── preproc.rs        # Preprocessor (-define, -ifdef, -include, macros)
├── span.rs           # Source span types
├── error.rs          # Parse/lex error types
└── source_map.rs     # Source file tracking
```

#### nera-ir

```
src/
├── lib.rs
├── ir.rs             # IR instruction set (IrModule, IrFunction, BasicBlock, Terminator)
├── pattern.rs        # Pattern compilation data structures (decision trees)
├── bif_table.rs      # Native BIF capability, FFI symbols and editor metadata
├── otp_auto_imports.rs # Versioned OTP language-name catalogue
├── resolution.rs     # Per-module import, local and suppression policy
├── display.rs        # IR pretty-printer
├── validate.rs       # Cross-module call resolution
├── verify/           # Read-only structural verifier
├── verify.rs         # Verifier API
├── passes/           # Ordered IR transformations (see IR_PASSES.md)
├── lower/
│   ├── mod.rs            # IrBuilder, lower_module, source-function pipeline handoff
│   ├── expr.rs           # Expression lowering
│   ├── closure.rs        # Closure capture/lowering
│   ├── exception.rs      # try/catch/raise lowering
│   ├── list_comp.rs      # List comprehension lowering (tail-recursive accumulator)
│   ├── pattern_lower.rs  # Pattern compilation to IR instructions
│   ├── record.rs         # Record desugaring to tuple operations
│   └── tests.rs
└── warnings/
    ├── mod.rs
    ├── ast_walkers.rs    # Generic AST traversal helpers
    ├── shadowed.rs       # Shadowed variable warnings
    ├── unreachable.rs    # Unreachable clause detection
    ├── unused_funcs.rs   # Unused function warnings
    ├── unused_vars.rs    # Unused variable warnings
    └── tests.rs
```

#### nera-codegen

```
src/
├── lib.rs
├── compiler.rs           # Module compiler (atom/function batch init, data sections)
├── builtins.rs           # BIF lowering dispatch
├── linker.rs             # Object file linking → native binary
├── term_layout.rs        # ErlTerm bit layout constants
└── func_translator/
    ├── mod.rs            # FuncTranslator entry point
    ├── atoms.rs          # Atom value emission
    ├── calls.rs          # call/tail-call/return_call emission
    ├── ffi_emit.rs       # FFI call emission helpers
    ├── instructions.rs   # Instruction-by-instruction codegen
    ├── liveness.rs       # Variable liveness analysis
    ├── resume.rs         # Yield/resume point codegen (YieldCheck, receive suspend)
    ├── signatures.rs     # Cranelift signature construction (Tail vs SystemV)
    ├── terminators.rs    # Block terminator codegen
    └── tests.rs
```

#### nera-runtime

Built as both a Rust library and a C static library (`staticlib`) for linking into compiled binaries. Uses jemalloc as the global allocator (non-Windows).

```
src/
├── lib.rs
├── term.rs           # 64-bit tagged pointer ErlTerm, BOXED_* tags
├── atom_table.rs     # Global interned atom table
├── heap.rs           # Per-process bumpalo arena, copying GC, binary vheap tracking
├── process.rs        # Process struct, ProcessEntry (lock-free inbox), ASTATUS_* constants
├── message.rs        # Deep-copy message passing (MessageFragment enum)
├── scheduler.rs      # Work-stealing N:M scheduler, timer wake, exit propagation
├── bif.rs            # Public BIF dispatch entry points
├── bigint.rs         # Arbitrary-precision integer support (num-bigint)
├── timer.rs          # Receive and message timers, nera-timer thread
├── entry.rs          # C-ABI entry point, function table, process setup
├── socket.rs         # Global socket registry (Arc<TcpStream/TcpListener>)
├── io_pool.rs        # I/O thread pool (file I/O; lazy-started)
├── reactor.rs        # mio event-driven reactor (epoll/kqueue)
├── ets.rs            # ETS table implementation (HashMap-backed)
├── ets_pool.rs       # mmap-backed bump allocator for ETS word buffers
└── ffi/
    ├── mod.rs
    ├── alloc.rs          # Term allocation FFI
    ├── apply.rs          # nera_rt_apply (dynamic dispatch + BIF fallback)
    ├── arith.rs          # Arithmetic operations
    ├── binary_ops.rs     # Binary construction/matching/iteration, match context
    ├── closure.rs        # Closure allocation, call_closure, spawn_closure
    ├── compare.rs        # Term comparison
    ├── convert.rs        # Term conversion (integer_to_list, etc.)
    ├── ets.rs            # ETS FFI (insert, lookup, delete, select, etc.)
    ├── exception.rs      # nera_rt_raise, halt/0,1
    ├── file_io.rs        # File read/write FFI
    ├── gen_tcp.rs        # TCP socket FFI (connect, accept, send, recv, active mode)
    ├── helpers.rs        # Shared FFI helpers
    ├── io_format.rs      # io:format, io_lib:format
    ├── list_ops.rs       # List BIFs (reverse, member, keyfind, nth, etc.)
    ├── map_ops.rs        # Map BIFs (new, get, put, remove, keys, values, etc.)
    ├── misc.rs           # Miscellaneous BIFs (memory_stats, atom_to_list, etc.)
    ├── process.rs        # Process BIFs (spawn, send, link, monitor, register)
    ├── receive.rs        # Selective receive protocol (peek, next, accept)
    ├── timer.rs          # Timer BIFs (send_after, cancel_timer)
    ├── tuple_ops.rs      # Tuple BIFs (make_tuple, tuple_to_list, etc.)
    └── type_check.rs     # Type-test BIFs
```

---

## Runtime Design

### Term Representation (64-bit Tagged Pointers)

Every Erlang value is a 64-bit word. The low 3 bits encode the type:

```
  Bits [63:3]          [2:0]
  ┌──────────────────┬───────┐
  │   payload        │  tag  │
  └──────────────────┴───────┘

  000  Boxed pointer  → header word at address (tuple, float, bigint, binary, closure, map, match ctx)
  001  Cons cell      → [head | tail] at address
  010  Small integer  → 61-bit signed integer (arithmetic shift)
  011  Atom           → index into global atom table
  100  PID            → process identifier
  101  Nil            → empty list (immediate)
```

Boxed subtypes (distinguished by header word): `BOXED_TUPLE`, `BOXED_FLOAT`, `BOXED_BIGINT`, `BOXED_BINARY`, `BOXED_REFC_BINARY`, `BOXED_CLOSURE`, `BOXED_MAP`, `BOXED_REF`, `BOXED_MATCH_CTX`.

### Per-Process Heap & Garbage Collection

Each process owns a **bumpalo arena** — a contiguous memory region where allocation is a simple pointer bump (O(1)).

- **Allocation**: `alloc_cons()`, `alloc_tuple()`, `alloc_float()`, `alloc_binary()` — just bump a pointer
- **GC trigger**: When arena exceeds threshold OR binary virtual heap exceeds threshold
- **GC algorithm**: Copying collector — allocate fresh arena, trace roots (stack + mailbox), copy live data with forwarding pointers, drop old arena
- **Binary vheap**: Tracks large binary allocations separately; threshold adapts (grows to 2x live binary size after GC, shrinks back to default 256 KiB when swept)
- **No global pause**: Only the process being GC'd is affected
- **Process exit**: `drop(process)` frees the entire arena instantly
- **Large binaries**: Binaries > 64 bytes auto-promote to reference-counted (`BOXED_REFC_BINARY`), matching BEAM semantics

### Work-Stealing Scheduler

```
Thread 0          Thread 1          Thread 2          Thread N
┌──────────┐      ┌──────────┐      ┌──────────┐      ┌──────────┐
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

- **N threads** (one per CPU core), each with a local `crossbeam_deque::Worker`
- **Strategy**: pop local → steal from global injector → steal from random peer → park (condvar with 10ms timeout)
- **Reduction counting**: Each function entry decrements reductions; yield after 4,000
- **Back-edge yield checks**: CFG back-edges get `YieldCheck` instructions — full suspend/resume points that prevent infinite loops from starving the scheduler
- **Process states**: Ready → Running → Waiting (receive/I/O) → Exited
- **Atomic status** (`ASTATUS_*`): `READY`, `RUNNING`, `WAITING` (receive), `IO_WAIT` (I/O), separate CAS transitions to prevent double-wake races

### True Tail Call Optimization

Erlang functions use Cranelift's `CallConv::Tail` calling convention:

- **Self-tail-calls**: Lowered to loops (jump back to function entry) — zero overhead
- **Non-self tail calls**: Emit `return_call` (Cranelift instruction) which reuses the caller's stack frame
- **FFI/wrapper functions**: Use `SystemV` convention; cross-convention calls are handled by Cranelift

### Lock-Free Message Passing

Messages are deep-copied between process heaps, maintaining complete process isolation (like BEAM).

```
Sender Process                          Receiver ProcessEntry
┌─────────────┐                         ┌──────────────────┐
│ deep_copy   │  MessageFragment        │ inbox: SegQueue  │
│ to fragment │ ─────────────────────►  │ (lock-free MPSC) │
│ heap        │                         │                  │
└─────────────┘                         │ atomic_status    │
                                        │ (CAS wake)       │
                                        │                  │
                                        │ proc: Mutex      │
                                        │ (worker access)  │
                                        └──────────────────┘
```

- `ProcessEntry` wraps `inbox: SegQueue<MessageFragment>`, `atomic_status: AtomicU32`, `proc: Mutex<Process>`
- **Direct-to-heap send** (fast path): `try_lock()` the target when WAITING/READY, copy directly to receiver's heap using `direct_copy_message` (skips HashMap forwarding for simple tuples). Falls back to SegQueue on contention
- **Lock-free send** (fallback): deep-copy to fragment → `inbox.push()` → CAS `WAITING→READY` → inject into scheduler + notify workers
- **Fragment types**: `Immediate(ErlTerm)` for atoms/ints/PIDs/nil, `InlineTuple` (arity ≤ 2, zero alloc), `SimpleTuple` (Vec-backed), `Heap { heap, root }` for complex terms
- **SendDirect fusion**: IR pass `fuse_make_tuple_send` detects `MakeTuple(msg, elems) + Send(result, pid, msg)` where `msg` is single-use, fuses into `SendDirect(result, pid, elems)`. Codegen emits `nera_rt_send_tuple` which constructs the tuple directly on the receiver's heap without sender heap allocation
- **Worker drain**: Before running a process, the worker drains the lock-free inbox into the process mailbox
- **Self-send safety**: Detects `self_pid == target_pid` and pushes directly via `ctx.mailbox_ptr`, avoiding deadlock
- **Ordering guarantee**: All message delivery (regular, EXIT, DOWN) goes through the inbox to preserve FIFO

### Selective Receive

Three-BIF protocol with O(1) tombstone-based removal:

1. `receive_peek` — reset scan cursor, get first live candidate (skipping tombstones); this is the suspend point
2. `receive_next` — advance scan cursor past non-matching message (skipping tombstones)
3. `receive_accept` — write a tombstone sentinel at the matched slot (O(1), no shifting)

The IR lowering emits a scan loop: fail_block calls `receive_next`, updates the message variable via `Move`, and branches back to the pattern match entry. Both `receive_peek` and `receive_next` have Cranelift-inlined fast paths that include tombstone-skip loops. Leading tombstones are trimmed immediately (advancing `head`); interior tombstones are compacted lazily when the ratio exceeds 50%.

### Timer Wheel

- Min-heap (`BinaryHeap`) of `TimerEntry` with lazy cancellation via `pid_to_timer` map
- Dedicated `nera-timer` thread sleeps until the next deadline, pops expired entries, wakes processes
- Its wait keeps wakes pending, so a timer inserted while it computes its sleep is not missed. On macOS it is a kqueue whose receive-deadline timer is marked `NOTE_CRITICAL`, so it fires on time even when macOS coalesces the timers of a utility- or background-priority process; elsewhere it is a condvar
- `send_message` and `propagate_exit_signals` cancel timers when waking processes before their timeout fires
- Scheduler termination checks `timer_wheel.has_pending_timers()`

### I/O Architecture

**mio reactor** (`reactor.rs`): Event-driven I/O using mio (epoll/kqueue). Gated behind `#[cfg(mio_supported)]` (Linux/macOS/BSD; Windows falls back to I/O pool).

- Single `nera-reactor` thread runs `mio::Poll`, dispatches readiness events
- Reactor does NOT own sockets — they live in a global socket registry as `Arc<mio::net::TcpStream/TcpListener>`
- FFI functions try non-blocking I/O first (fast path), fall back to reactor on `WouldBlock`
- Active mode (`{active, once|true}`) uses reactor-driven reads
- Commands sent via `crossbeam-channel` + `mio::Waker`
- `ASTATUS_IO_WAIT (=4)`: Distinct from receive-waiting; `try_wake_io()` CAS `IO_WAIT→READY`

**I/O pool** (`io_pool.rs`): 2 threads on mio platforms (file I/O only), full pool on non-mio platforms. Lazy-started with double-checked locking — threads only spawned on first I/O submission.

### ETS

- HashMap-backed ETS tables with per-table `RwLock`
- **mmap region pool** (`ets_pool.rs`): Each `EtsTable` owns an `EtsRegionPool` (list of 2MB mmap'd regions). Allocation cursors remain private to the pool; each `CompactHeap` retains an `Arc<EtsRegion>` owning its mapping. Clearing or deleting a table releases its entries, while zero-copy lookup results keep their mappings alive across worker migration and GC. Reclamation releases regions owned only by the pool; the final owner unmaps the memory directly, bypassing jemalloc fragmentation. The Arc count replaces the former separate live-entry counter.
- `CompactHeap` stores the word buffer pointer and an owning region handle for each entry

### Process Features

- **spawn/1,3**: `spawn_closure` extracts fn_ptr and env from the closure, deep-copies env to new process heap
- **Links**: Bidirectional, with `trap_exit` flag for converting exit signals to messages
- **Monitors**: Unidirectional, delivering `{'DOWN', Ref, process, Pid, Reason}` on exit
- **Registered processes**: `registered_names: Arc<RwLock<HashMap<u32, ProcessId>>>` — `register/2`, `whereis/1`, `unregister/1`, send-by-name via atom resolution
- **Exit code**: Only the initial process (PID 1) affects the program exit code; spawned process crashes print stderr reports but don't change exit code

---

## Erlang Feature Support

### Types

| Type       | Status | Notes                                                        |
|------------|--------|--------------------------------------------------------------|
| Atoms      | Done   | Global atom table, batch interning at startup                |
| Integers   | Done   | 61-bit small ints + arbitrary precision bigints (num-bigint) |
| Floats     | Done   | 64-bit IEEE 754                                              |
| Lists      | Done   | Cons cells, proper/improper lists, strings as lists          |
| Tuples     | Done   | Heap-allocated, element access, make_tuple                   |
| Binaries   | Done   | Byte-aligned + bit-level segments, refc for >64 bytes        |
| Maps       | Done   | Heap-allocated, pattern matching, create/update              |
| PIDs       | Done   | Tagged immediate                                             |
| References | Done   | Unique refs for monitors                                     |
| Closures   | Done   | Captured environment, inter-process deep copy                |

### Language Features

| Feature                 | Status | Notes                                                                                          |
|-------------------------|--------|------------------------------------------------------------------------------------------------|
| Pattern matching        | Done   | Wildcards, variables, literals, tuples, cons, binaries, maps, records, aliases, nested, guards |
| Guards                  | Done   | Comparison ops, type tests, arithmetic, conjunction `,`, disjunction `;`                       |
| Case/if expressions     | Done   | Full pattern matching with guards                                                              |
| Receive...after         | Done   | Selective receive with timeout, timer wheel integration                                        |
| Try/catch/throw         | Done   | Exception propagation, `try...of...catch...after...end`                                        |
| List comprehensions     | Done   | Nested generators, filters, tail-recursive accumulator (O(1) stack)                            |
| Binary comprehensions   | Done   | `<< <<X>> \|\| X <- List >>`                                                                   |
| Anonymous functions     | Done   | `fun(X) -> X end`, closures with captured env                                                  |
| Fun references          | Done   | `fun F/A`, `fun M:F/A` (local and remote)                                                      |
| Records                 | Done   | Desugared to tuples at IR level                                                                |
| Map create/update       | Done   | `#{k => v}`, `M#{k := v}`, `M#{k => v}`                                                        |
| Preprocessor            | Done   | `-define`, `-ifdef`/`-ifndef`/`-else`/`-endif`, `-include`, macros with args                   |
| Send `!`                | Done   | With registered name resolution                                                                |
| Spawn                   | Done   | `spawn/1` (closures), `spawn/3` (M:F:A)                                                        |
| Links & monitors        | Done   | `link/1`, `monitor/2`, `process_flag(trap_exit, true)`                                         |
| Process hardening       | Done   | `process_flag(fullsweep_after/max_heap_size)`, `hibernate/3`, `spawn_opt/2,4`                  |
| Multiple modules        | Done   | Cross-module calls, stdlib auto-discovery                                                      |
| Tail call optimization  | Done   | Self-recursion to loop + `return_call` for non-self tail calls                                 |
| Binary pattern matching | Done   | Bit-level segments, match contexts, integer/binary/bitstring/utf8 types                        |
| ETS                     | Done   | `new`, `insert`, `lookup`, `delete`, `select`, `match`, `update_counter`, etc.                 |
| Timers                  | Done   | `erlang:send_after/3`, `timer:send_after/2,3`, `timer:cancel/1`                                |
| TCP sockets             | Done   | `gen_tcp:listen/connect/accept/send/recv/close`, active mode                                   |
| File I/O                | Done   | `file:read_file/1`, `file:write_file/2`                                                        |
| halt/0,1                | Done   | Immediate process exit                                                                         |

### BIFs

Native BIF capability, FFI symbols and editor metadata live in `nera-ir/src/bif_table.rs`. Adding an implementation there does not auto-import its name. The separate, reproducibly derived OTP 29.0.4 catalogue (`otp_auto_imports.rs`) defines language names; `resolution.rs` applies module declarations and suppression consistently during diagnostics and lowering. Codegen continues to use `lookup_ffi_symbol` independently.

**Inlined BIFs** (Cranelift IR, no FFI call overhead): `hd/1`, `tl/1`, `is_atom/1`, `is_list/1`, `is_pid/1`.

### Standard Library

Seven Erlang modules in `erlang/stdlib/`, auto-compiled with user code (disable with `--no-stdlib`):

| Module        | Description                                                                                                                |
|---------------|----------------------------------------------------------------------------------------------------------------------------|
| `lists`       | Higher-order functions (map, filter, foldl, sort, etc.) — hot-path functions (reverse, member, keyfind, nth) are Rust BIFs |
| `maps`        | Higher-order functions (map, filter, fold, etc.) — hot-path functions (get, put, remove, keys, values) are Rust BIFs       |
| `gen_server`  | OTP gen_server behaviour                                                                                                   |
| `supervisor`  | OTP supervisor behaviour                                                                                                   |
| `application` | OTP application behaviour stub                                                                                             |
| `proc_lib`    | Process library helpers                                                                                                    |
| `timer`       | Timer module (send_after, cancel, etc.)                                                                                    |

Stdlib discovery: checks installed layout (`<exe>/../lib/nera/erlang/stdlib/`), workspace layout (`<exe>/../../erlang/stdlib/`), and CWD fallback. User modules shadow stdlib by name.

### Optimizations Implemented

| Optimization                       | Impact                                                                    |
|------------------------------------|---------------------------------------------------------------------------|
| Batch atom interning               | Single FFI call for all atoms at startup                                  |
| Batch function registration        | Single FFI call for all functions at startup                              |
| Lazy I/O pool                      | Threads only spawned on first I/O submission                              |
| Binary match contexts              | Reuse match state across pattern segments (avoids sub-binary allocation)  |
| Binary virtual heap                | GC triggers account for large binary allocations                          |
| Lock-free MPSC mailbox             | No target Mutex acquisition on send                                       |
| Back-edge yield checks             | Full suspend/resume at CFG back-edges                                     |
| Tail-recursive list comprehensions | O(1) stack instead of O(N)                                                |
| Inline BIF calls                   | `hd/1`, `tl/1`, type checks avoid FFI overhead                            |
| True TCO via `return_call`         | Cranelift `CallConv::Tail` for non-self tail calls                        |
| Refc binaries                      | Large binaries (>64B) shared via reference counting                       |
| ETS mmap pools                     | Instant OS memory return on table clear, bypasses allocator fragmentation |

---

## Benchmarks

6 benchmarks comparing nera vs BEAM OTP 28 (`benchmarks/compare_benchmarks.sh`):

| Benchmark                       | nera   | BEAM   | nera RSS | BEAM RSS |
|---------------------------------|--------|--------|----------|----------|
| binary_send (100K x 10KB)       | 31ms   | 13ms   | 41MB     | 56MB     |
| binary_fanout (1000 x 100KB)    | 1ms    | 1ms    | 11MB     | 59MB     |
| binary_subbinary (100K x 100KB) | 20ms   | 0ms    | 5MB      | 55MB     |
| tcp_ping_pong (50K x 1KB)       | 1546ms | 1432ms | 8MB      | 56MB     |
| tcp_throughput (100MB bulk)     | 54ms   | 37ms   | 11MB     | 55MB     |
| tcp_connections (100 conns)     | 8ms    | 6ms    | 6MB      | 57MB     |

nera RSS: 5-41MB vs BEAM 55-59MB (5-8x less memory).

---

## Design Decisions & Trade-offs

### Why Cranelift instead of LLVM?

- **Pure Rust** — no C++ dependency, simpler build
- **Fast compilation** — Cranelift compiles much faster than LLVM
- **JIT-ready** — same backend can be used for JIT compilation later
- **Trade-off**: Generated code is ~20-30% slower than LLVM at -O2

### Why deep-copy messages instead of shared-nothing?

This matches BEAM semantics exactly. Each process can GC independently without coordinating with others. The cost of copying is offset by:
- No synchronization overhead on reads
- Instant process cleanup (drop entire arena)
- Reference-counted binaries for large data (>64 bytes)

### Why reduction counting instead of preemptive interrupts?

- **Simpler implementation** — no signal handlers or stack switching
- **Deterministic** — same program yields at same points
- **Portable** — works on all platforms without OS-specific tricks
- **Back-edge yield checks** prevent infinite loops from starving the scheduler

### Why Erlang-first for new stdlib functions?

- **Scheduler fairness by default** — Erlang code is reduction-accounted naturally
- **Maintainability** — standard library behavior stays visible in Erlang modules
- **BIFs are exceptions** — use BIFs only for runtime-internal access, semantics Erlang cannot express, or measured material wins

### Why jemalloc?

- Better multi-threaded allocation performance than system malloc
- `tikv-jemalloc-ctl` provides memory statistics (`nera:memory_stats/0`)
- Non-Windows defaults use `tcache_max:4096,narenas:2` through the prefixed
  `_rjem_malloc_conf` C symbol; macOS additionally uses `retain:false`
- Linux keeps jemalloc's retain default: the [5.4 manual](https://github.com/jemalloc/jemalloc/blob/7a34f18502e7b222724097cdcd499b437d189acc/doc/jemalloc.xml.in#L1032-L1058)
  explains that discarding mappings on 64-bit Linux can leave semi-permanent
  holes in the virtual memory map and harm performance
- `_RJEM_MALLOC_CONF` can override startup options; Windows owns its jemalloc
  configuration symbol, so Nera does not export a competing strong definition
- Dirty/muzzy decay stays at 10000/0 ms; background threads are not enabled
- Two automatic arenas reduce memory overhead but may increase contention

### Why static linking?

- **Single binary deployment** — like Go
- **No runtime dependencies** — ships everything needed
- **Trade-off**: Larger binaries (~2-5MB baseline), acceptable for server/CLI tools

---

## Testing

### Unit Tests

548 tests across 5 crates, run with:
```bash
cargo test --workspace
```

### End-to-End Tests

182 tests in `tests/`, run with:
```bash
./tests/run_tests.sh
```

Convention: each `.erl` file exports `test/0 -> ok`; badmatch = failure. `id(I) -> I` prevents constant folding. `%% SKIP: reason` on first line to skip tests.

Test categories: arithmetic, binaries, control flow, closures, processes (spawn/link/monitor), ETS, TCP, maps, records, preprocessor, gen_server, supervisor, and more.

### Benchmarks

```bash
./benchmarks/compare_benchmarks.sh    # nera vs BEAM side-by-side
./benchmarks/run_benchmarks.sh        # nera-only
```

---

## Distribution

```bash
./scripts/dist.sh [--version X.Y.Z]
```

Builds a tarball containing `bin/nera` + `lib/` + `lib/nera/erlang/stdlib/`.

---

## Contributing

- **Adding a BIF**: Add one entry to `nera-ir/src/bif_table.rs` — both IR lowering and codegen consume it automatically. Implement the FFI function in the appropriate `nera-runtime/src/ffi/*.rs` file.
- **Adding syntax**: Extend `token.rs` + `parser.rs` + `ast.rs` in `nera-syntax`, add preprocessor support in `preproc.rs` if needed.
- **Adding an IR instruction**: Extend `ir.rs` definitions, handle in `lower/*.rs`, translate in `func_translator/instructions.rs`.
- **Adding a runtime feature**: Implement in appropriate runtime module, expose via `ffi/` C-ABI.
- **Adding a stdlib module**: Create `.erl` file in `erlang/stdlib/`; it will be auto-discovered and compiled.
- **Adding a warning pass**: Create new file in `nera-ir/src/warnings/`, register in `warnings/mod.rs`.
