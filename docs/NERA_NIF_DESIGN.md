# nera-nif — BEAM NIF Bridge Design Document

## 1. Executive Summary

`nera-nif` is a proposed new crate that provides an ergonomic bridge
between the BEAM virtual machine and the nera runtime. It enables BEAM
applications (such as ejabberd) to offload CPU-intensive work to nera's
actor-based scheduler without blocking BEAM schedulers, using Rust instead of
C to lower the risk of memory corruption. It does not isolate that work from
the BEAM: the bridge, the embedded runtime and the VM share one OS process,
so a native failure can still stop the whole VM (see
[§5.3](#53-panic-handling-and-its-limits)).

Unlike Rustler (which provides safe Rust bindings to the NIF API but no
concurrency model), `nera-nif` bundles the full nera runtime — work-stealing
scheduler and per-process heaps — as an embeddable library inside a BEAM NIF.
The developer writes pure Rust (or Erlang compiled by nera) that runs on
dedicated threads with actor-model concurrency, and results flow back to BEAM
processes via `enif_send`.

This document covers the architecture, API design, term conversion layer,
communication model, performance characteristics, and implementation roadmap.

---

## 2. Goals and Non-Goals

### Goals

- **Non-blocking NIF calls**: the BEAM scheduler thread returns immediately.
  All heavy work runs on nera's own thread pool.
- **Lower memory-safety risk**: processing code is written in safe Rust
  instead of C, which rules out buffer overflows, use-after-free and data
  races in that code. The NIF API boundary, the embedded runtime and some
  dependencies still contain `unsafe` code; a defect there can corrupt BEAM
  memory. Rust reduces this risk; it does not remove it.
- **Panic handling**: an unwinding panic in a work item is caught and
  converted to an error message sent back to the calling BEAM process. This
  is not crash isolation: failures that `catch_unwind` cannot catch still
  terminate the BEAM VM ([§5.3](#53-panic-handling-and-its-limits)).
- **Zero-serialization communication**: data passes between BEAM and nera
  via pointer access and `memcpy` within the same OS process. No
  `term_to_binary` / `binary_to_term` encoding.
- **Actor concurrency inside NIFs**: nera actors can spawn sub-actors for
  parallel work, using the same scheduler and per-process heap model as
  standalone nera programs.
- **Drop-in migration**: existing ejabberd C NIFs (fast_xml, fast_tls,
  stringprep, ezlib) can be replaced one at a time with no changes to
  ejabberd's Erlang architecture.
- **Simple build**: `cargo build` produces a `.so` / `.dylib` that BEAM loads
  as a NIF. No autotools, no `gcc` dependency.

### Non-Goals

- Replacing Rustler for simple synchronous NIFs. If the work takes < 1
  microsecond and doesn't need concurrency, a plain Rustler NIF is simpler
  and faster.
- Running full Erlang programs inside the NIF. The nera compiler pipeline
  (parser, IR, codegen) is not required. Developers write Rust code against
  the nera runtime API.
- BEAM-level integration (hot code loading, distribution awareness, OTP
  supervision of nera actors). The bridge is a unidirectional offload
  mechanism.
- Fault containment. The bridge does not run native code in a separate OS
  process, sandbox or memory domain, and it does not guarantee that the BEAM
  VM survives a native failure. Work that must never be able to stop the VM
  belongs in a port or on a separate node.

---

## 3. Architecture Overview

### 3.1 High-Level Data Flow

```
┌──────────────────────────────────────────────────────────────────┐
│                        BEAM VM Process                            │
│                                                                   │
│  Erlang code:                                                    │
│    Ref = make_ref(),                                             │
│    fast_xml_bridge:parse_async(self(), Ref, Data),               │
│    receive {xml_parsed, Ref, Result} -> ... end                  │
│                                                                   │
│       │                                                           │
│       │  NIF call (~100 ns)                                      │
│       ▼                                                           │
│  ┌────────────────────────────────────────────┐                  │
│  │  nera-nif bridge (Rust, loaded as .so)     │                  │
│  │                                             │                  │
│  │  ┌──────────────────────────────┐           │                  │
│  │  │ 1. Capture BEAM caller PID   │           │                  │
│  │  │ 2. Copy binary data          │           │  Same OS         │
│  │  │ 3. Submit to nera scheduler │──────┐    │  process         │
│  │  │ 4. Return :ok to BEAM        │      │    │                  │
│  │  └──────────────────────────────┘      │    │                  │
│  │                                         │    │                  │
│  │  ┌──────────────────────────────┐      │    │                  │
│  │  │ nera runtime (embedded)      │◄─────┘    │                  │
│  │  │                               │           │                  │
│  │  │  ┌────────┐ ┌────────┐       │           │                  │
│  │  │  │worker 0│ │worker 1│ ...   │           │                  │
│  │  │  └───┬────┘ └────────┘       │           │                  │
│  │  │      │                        │           │                  │
│  │  │      ▼                        │           │                  │
│  │  │  nera actor:                 │           │                  │
│  │  │    parse XML / do TLS / etc   │           │                  │
│  │  │    own heap, own GC           │           │                  │
│  │  │      │                        │           │                  │
│  │  │      │ enif_send() (~200 ns)  │           │                  │
│  │  │      ▼                        │           │                  │
│  │  │  result → BEAM process mailbox│           │                  │
│  │  └──────────────────────────────┘           │                  │
│  └────────────────────────────────────────────┘                  │
│                                                                   │
│  Erlang process picks up result via receive                      │
└──────────────────────────────────────────────────────────────────┘
```

### 3.2 What Lives Where

| Component | Runs on | Memory |
|---|---|---|
| Erlang calling code | BEAM scheduler thread | BEAM process heap |
| NIF entry function | BEAM scheduler thread (briefly) | Stack only |
| nera scheduler + workers | Dedicated OS threads | Own heaps, same address space as BEAM |
| nera actors | nera worker threads | Per-actor bump arena |
| Result construction | nera worker thread | `ErlNifEnv` (BEAM-allocated) |
| `enif_send` | nera worker thread | Deposits into BEAM mailbox |

### 3.3 Why This Is Not a Port

A BEAM port runs in a separate OS process and communicates via pipes or sockets.
This requires serialization (`term_to_binary`), deserialization
(`binary_to_term`), and two context switches per round trip.

The nera-nif bridge shares the BEAM's address space. The NIF `.so` is loaded
via `dlopen` into the BEAM process. nera's worker threads are OS threads within
the same process. Communication uses:

- **Inbound**: pointer access to BEAM binary data (zero-copy for reads) or
  `memcpy` (for owned copies). No serialization.
- **Outbound**: `enif_send()` — a BEAM C API function that deposits a term
  into a process mailbox via a pointer write and a mutex acquisition. No
  pipes, no sockets, no serialization.

Cost comparison:

| Mechanism | Round-trip latency | Serialization | Context switches |
|---|---|---|---|
| Port (pipe) | ~10-50 us | Yes (both directions) | 2 (kernel) |
| Port (TCP) | ~20-100 us | Yes (both directions) | 2 (kernel) |
| Dirty NIF | ~0.1-1 us | No | 0 |
| **nera-nif bridge** | **~0.3-0.5 us** | **No** | **0** |

The bridge has slightly higher latency than a synchronous dirty NIF due to
the work queue submission and `enif_send` return path, but it provides actor
concurrency that dirty NIFs do not. Sharing the BEAM's address space is also
the bridge's limit: like any NIF, a failure that aborts it stops the VM. Only
a port or a separate node keeps native code in another OS process.

---

## 4. Term Conversion Layer

### 4.1 The Problem

BEAM and nera use different tagged pointer schemes. Both encode terms as
64-bit words with low-bit tags, but the tag assignments and heap layouts differ.

BEAM terms cannot be used directly by nera code, and vice versa. A conversion
layer must translate between the two representations at the NIF boundary.

### 4.2 Conversion Direction and Strategy

```
                    Inbound                              Outbound
                    (BEAM → nera)                       (nera → BEAM)

  BEAM binary ──── zero-copy (&[u8]) ────► Rust code    (no conversion)
  BEAM integer ─── decode + re-encode ───► nera term    (rare)
  BEAM atom ────── decode + intern ──────► nera atom    (moderate)
  BEAM tuple ───── decode + allocate ────► nera tuple   (rare)
  BEAM list ────── decode + allocate ────► nera list    (rare)

  nera result ─── enif_make_* calls ───► BEAM term      (always)
```

**Key insight**: for the primary use case (XML parsing, TLS, compression), the
inbound data is a **BEAM binary** (raw bytes). This requires no term conversion
at all — Rust receives a `&[u8]` slice pointing directly into BEAM memory, or
a `Vec<u8>` copy for safety across threads.

The outbound path always requires constructing BEAM terms via `enif_make_*`
calls, regardless of whether the internal representation is nera terms or
plain Rust structs. This is the same cost that current C NIFs pay.

### 4.3 Conversion API

```rust
/// nera-nif term conversion traits

/// Convert a BEAM NIF term to a Rust type.
/// This runs on the BEAM scheduler thread (must be fast).
pub trait FromBeam<'a>: Sized {
    fn from_beam(env: BeamEnv<'a>, term: BeamTerm<'a>) -> Result<Self, ConvertError>;
}

/// Convert a Rust type to a BEAM NIF term.
/// This runs on an nera worker thread via enif_alloc_env + enif_send.
pub trait IntoBeam {
    fn into_beam<'a>(&self, env: BeamEnv<'a>) -> BeamTerm<'a>;
}

// Implementations for common types:

impl<'a> FromBeam<'a> for Vec<u8> {
    fn from_beam(env: BeamEnv<'a>, term: BeamTerm<'a>) -> Result<Self, ConvertError> {
        // ERL_NIF_TERM → enif_inspect_binary → memcpy to owned Vec
        // Cost: ~1 us per 10 KB
    }
}

impl<'a> FromBeam<'a> for &'a [u8] {
    fn from_beam(env: BeamEnv<'a>, term: BeamTerm<'a>) -> Result<Self, ConvertError> {
        // Zero-copy: only valid while env is alive (on BEAM scheduler thread)
        // Use only for synchronous operations, not for async dispatch
    }
}

impl IntoBeam for Vec<XmlElement> {
    fn into_beam<'a>(&self, env: BeamEnv<'a>) -> BeamTerm<'a> {
        // Recursively call enif_make_tuple, enif_make_binary, enif_make_list
        // to build [{xmlel, Name, Attrs, Children}, ...]
    }
}
```

### 4.4 When Full Term Conversion Is Needed

For the NIF bridge use case, full BEAM ↔ nera term conversion is rarely
needed. The typical pattern is:

1. Receive raw bytes from BEAM (binary → `Vec<u8>`, no term conversion)
2. Process in pure Rust (no terms at all, just Rust structs)
3. Construct BEAM terms for the result (Rust structs → `enif_make_*`)

Full term conversion (BEAM atoms ↔ nera atoms, BEAM tuples ↔ nera tuples)
is only needed if:

- The NIF receives structured Erlang data (not just binaries)
- The processing logic uses nera's term representation internally
- An nera-compiled Erlang module runs inside the NIF (future, Phase 4)

This is deferred to Phase 2 of the `nera-nif` roadmap.

---

## 5. nera Runtime Embedding API

### 5.1 Current State

The `nera-runtime` crate currently exposes its modules publicly but has no
designed embedding API. The `Scheduler` struct is tightly coupled to the
standalone binary use case — it starts from `nera_runtime_start_mfa()`,
runs until all processes exit, and then returns.

For NIF embedding, the scheduler must:

- Start when the NIF is loaded (`on_load`)
- Run indefinitely (not exit when work queues are empty)
- Accept work submissions from external threads (BEAM scheduler threads)
- Remain alive across multiple NIF calls

### 5.2 Proposed Embedding API

```rust
// nera-runtime public embedding API

/// Configuration for an embedded scheduler.
pub struct EmbeddedConfig {
    /// Number of worker threads. Default: num_cpus.
    pub workers: usize,
    /// Per-actor heap size threshold for GC (bytes). Default: 256 KiB.
    pub gc_threshold: usize,
}

impl Default for EmbeddedConfig {
    fn default() -> Self {
        EmbeddedConfig {
            workers: num_cpus::get(),
            gc_threshold: 256 * 1024,
        }
    }
}

/// An embedded scheduler handle. Does not take over the process — runs
/// worker threads in the background and accepts work submissions.
pub struct EmbeddedScheduler {
    injector: Arc<Injector<WorkItem>>,
    workers: Vec<thread::JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

impl EmbeddedScheduler {
    /// Start the embedded scheduler with the given configuration.
    /// Worker threads begin running immediately.
    pub fn start(config: EmbeddedConfig) -> Self { ... }

    /// Submit a work item to be executed on an nera worker thread.
    /// Returns immediately. The closure runs on a worker thread with
    /// access to a fresh actor context (own heap, own GC).
    ///
    /// The closure receives an `ActorContext` for heap allocation and
    /// sub-actor spawning.
    pub fn spawn_work<F>(&self, work: F)
    where
        F: FnOnce(&mut ActorContext) + Send + 'static
    { ... }

    /// Shut down the scheduler. Waits for all in-flight work to complete.
    pub fn shutdown(self) { ... }
}

/// Context provided to each work item running on an nera worker thread.
pub struct ActorContext {
    heap: ProcessHeap,
    scheduler_handle: Arc<EmbeddedScheduler>,
}

impl ActorContext {
    /// Allocate terms on this actor's private heap.
    pub fn heap(&mut self) -> &mut ProcessHeap { &mut self.heap }

    /// Spawn a sub-actor on the same scheduler.
    /// The sub-actor gets its own heap and runs concurrently.
    pub fn spawn<F>(&self, work: F)
    where
        F: FnOnce(&mut ActorContext) + Send + 'static
    { ... }
}
```

### 5.3 Panic Handling and Its Limits

Each work item runs inside `std::panic::catch_unwind`:

```rust
// Inside worker loop
let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    let mut ctx = ActorContext::new(heap, scheduler_handle.clone());
    work(&mut ctx);
}));

match result {
    Ok(()) => { /* normal completion */ }
    Err(panic_info) => {
        // Actor panicked and the panic unwound. Log the error.
        // The actor's heap is dropped (all allocations freed).
        // The NIF can send an error message back to the BEAM caller.
    }
}
```

This turns an ordinary, unwinding panic in the Rust processing code (e.g.,
an index out of bounds, an unwrap on None) into an `{error, nif_panic}`
message for one request. It is a recovery path for one kind of bug, not a
containment boundary, and it does not guarantee that the BEAM VM keeps
running. `catch_unwind` cannot catch, and the VM terminates on:

- a panic in a build compiled with `panic = "abort"`;
- a panic in the NIF entry function itself (argument decoding runs there,
  outside `catch_unwind`): the entry function is `extern "C"`, so a panic
  that reaches it aborts the process;
- a second panic raised while the first is unwinding, for example from a
  `Drop` implementation;
- memory allocation failure, which aborts the process by default;
- stack overflow on a worker thread;
- `std::process::abort`;
- undefined behaviour in `unsafe` Rust, the NIF API boundary or a
  dependency's foreign code, which may instead corrupt BEAM memory silently
  and fail later.

A panic while encoding or sending the result, which the generated code in
§6.3 does after its own `catch_unwind`, is not reported to the caller at all:
the caller sees only its own timeout.

After a caught panic, data shared with other work items (through `Arc`, a
`Mutex` or the scheduler) may be left inconsistent: `AssertUnwindSafe`
asserts otherwise without checking it. Because the bridge's worker threads
live in the BEAM's OS process, each failure above stops every BEAM process,
not only the caller. Nothing stops a work item that loops or allocates
without bound either: it keeps consuming worker threads and memory that the
VM shares. The [nera stdlib design](STDLIB_DESIGN.md#what-rust-does-and-does-not-guarantee)
states the same limits for native code inside a nera program.

---

## 6. NIF Macro System

### 6.1 Design Philosophy

The `nera-nif` macro system eliminates NIF boilerplate. The developer writes
a Rust function with typed parameters, and the macro generates:

- NIF function signature (`extern "C" fn(ErlNifEnv, argc, argv)`)
- Argument decoding (BEAM terms → Rust types via `FromBeam`)
- Async dispatch to nera scheduler
- Result encoding (Rust types → BEAM terms via `IntoBeam`)
- `enif_send` call back to the caller PID
- Panic handler

### 6.2 Macro API

```rust
use nera_nif::prelude::*;

/// Async NIF — returns immediately, sends result via message.
///
/// The macro generates:
/// 1. A NIF entry function that decodes args and captures caller PID
/// 2. An async dispatch that submits the body to the nera scheduler
/// 3. A result handler that encodes the return value and calls enif_send
/// 4. A panic handler that sends {error, nif_panic} on failure
#[nera_nif::async_nif]
fn parse_xml(data: Binary) -> NifResult<Vec<XmlElement>> {
    // This body runs on an nera worker thread.
    // `data` is a Vec<u8> copied from the BEAM binary.
    let elements = xml_parser::parse(data.as_bytes())?;
    Ok(elements)
}

/// Sync NIF — runs on BEAM scheduler thread, returns directly.
/// Use only for fast operations (< 1 us).
#[nera_nif::sync_nif]
fn atom_to_string(atom: Atom) -> NifResult<String> {
    Ok(atom.as_str().to_string())
}

/// NIF module declaration.
#[nera_nif::module("fast_xml_bridge")]
struct FastXmlBridge;
```

### 6.3 Generated Code (Conceptual)

For `#[nera_nif::async_nif] fn parse_xml(data: Binary)`, the macro generates:

```rust
// Generated by #[nera_nif::async_nif]
#[no_mangle]
pub extern "C" fn parse_xml_nif(
    env: *mut ErlNifEnv,
    argc: c_int,
    argv: *const ERL_NIF_TERM,
) -> ERL_NIF_TERM {
    // 1. Decode arguments on the BEAM thread
    let beam_env = BeamEnv::from_raw(env);
    let caller_pid = beam_env.self_pid();
    let ref_term = beam_env.make_ref();

    let data: Vec<u8> = match Binary::from_beam(beam_env, argv[0]) {
        Ok(d) => d.to_vec(),   // memcpy — owns the data
        Err(e) => return beam_env.make_badarg(),
    };

    // Save ref for the async response
    let ref_copy = beam_env.copy_term(ref_term);

    // 2. Submit to nera scheduler — returns immediately
    SCHEDULER.get().unwrap().spawn_work(move |_ctx| {
        // 3. Run the user's function on an nera worker thread
        let result = std::panic::catch_unwind(|| {
            parse_xml_impl(Binary(data))
        });

        // 4. Build BEAM terms and send back
        let msg_env = ErlNifEnv::alloc();
        let response = match result {
            Ok(Ok(elements)) => {
                let beam_result = elements.into_beam(msg_env);
                msg_env.make_tuple3(
                    msg_env.make_atom("xml_parsed"),
                    ref_copy.in_env(msg_env),
                    beam_result,
                )
            }
            Ok(Err(e)) => {
                msg_env.make_tuple3(
                    msg_env.make_atom("xml_error"),
                    ref_copy.in_env(msg_env),
                    msg_env.make_string(&e.to_string()),
                )
            }
            Err(_panic) => {
                msg_env.make_tuple3(
                    msg_env.make_atom("xml_error"),
                    ref_copy.in_env(msg_env),
                    msg_env.make_atom("nif_panic"),
                )
            }
        };

        // 5. Send result to BEAM process mailbox
        enif_send(std::ptr::null_mut(), &caller_pid, msg_env, response);
        msg_env.free();
    });

    // Return :ok immediately — BEAM scheduler is free
    beam_env.make_atom("ok")
}
```

### 6.4 Message Protocol

Every `async_nif` follows the same message protocol:

```
Caller → NIF:   fast_xml_bridge:parse_xml(Data)
NIF → Caller:   :ok                              (immediate return)
Worker → Caller: {xml_parsed, Ref, Result}        (async, via enif_send)
                 OR
                 {xml_error, Ref, Reason}          (on failure or panic)
```

The Erlang-side wrapper provides a synchronous convenience API:

```erlang
-module(fast_xml_bridge).
-export([parse/1, parse/2, parse_async/1]).

%% Synchronous with default timeout
parse(Data) ->
    parse(Data, 5000).

%% Synchronous with explicit timeout
parse(Data, Timeout) ->
    Ref = make_ref(),
    ok = parse_nif(self(), Ref, Data),
    receive
        {xml_parsed, Ref, Result} -> {ok, Result};
        {xml_error, Ref, Reason}  -> {error, Reason}
    after Timeout ->
        {error, timeout}
    end.

%% Async — caller handles the response
parse_async(Data) ->
    Ref = make_ref(),
    ok = parse_nif(self(), Ref, Data),
    {ok, Ref}.
```

---

## 7. Performance Analysis

### 7.1 Overhead Budget

For a typical XML stanza parse (10 KB input, 50 elements output):

| Step | Cost | Notes |
|---|---|---|
| NIF call entry | ~100 ns | BEAM function dispatch |
| Argument decoding | ~50 ns | `enif_inspect_binary` — pointer, no copy |
| Binary copy (10 KB) | ~1 us | `memcpy` to owned buffer |
| Queue submission | ~50-200 ns | Lock-free `Injector::push` |
| NIF return to BEAM | ~50 ns | Return atom `ok` |
| **Total BEAM thread time** | **~1.5 us** | BEAM scheduler blocked for 1.5 us |
| XML parsing (10 KB) | ~50-200 us | Actual work, on nera thread |
| Result construction | ~10-20 us | ~200 `enif_make_*` calls |
| `enif_send` | ~200-500 ns | Mutex + mailbox append |
| **Total wall time** | **~60-220 us** | Perceived latency |

### 7.2 Comparison with Current fast_xml C NIF

| Metric | C NIF (today) | nera-nif bridge |
|---|---|---|
| BEAM scheduler blocked | 50-200 us (entire parse) | **~1.5 us** |
| Parse throughput | ~50-200 us | ~50-200 us (same work) |
| Overhead per call | ~0 | ~2 us (copy + dispatch + send) |
| Parallel parsing | No (one scheduler thread) | **Yes (N worker threads)** |
| Crash risk | **VM crash on buffer overflow** | Lower, not zero: VM still stops on abort, OOM, stack overflow or an `unsafe` defect (§5.3) |
| Other processes affected by scheduler blocking | **Yes (scheduler blocked)** | No |

The overhead (~2 us) is negligible compared to the actual parse time
(50-200 us). The critical win is that the BEAM scheduler is blocked for
1.5 us instead of 50-200 us — a **30-100x reduction** in scheduler
contention.

### 7.3 When NOT to Use the Bridge

For operations completing in < 1 us, the async dispatch overhead dominates:

| Operation | Work time | Bridge overhead | Verdict |
|---|---|---|---|
| XML parse (10 KB) | 50-200 us | 2 us (1%) | **Use bridge** |
| TLS handshake | 1-10 ms | 2 us (<0.01%) | **Use bridge** |
| Stringprep normalize | 1-10 us | 2 us (20-200%) | Borderline |
| Atom comparison | <10 ns | 2 us (200,000%) | **Use sync NIF** |

Rule of thumb: use `#[nera_nif::async_nif]` for work > 10 us.
Use `#[nera_nif::sync_nif]` for work < 1 us.

---

## 8. Crate Structure

```
nera/
  crates/
    nera-nif/                      # NEW CRATE
      Cargo.toml
      src/
        lib.rs                      # Public API, re-exports
        env.rs                      # Safe wrapper around ErlNifEnv
        term.rs                     # BeamTerm — safe wrapper around ERL_NIF_TERM
        convert.rs                  # FromBeam / IntoBeam traits + impls
        types.rs                    # Binary, Atom, Pid, Ref — typed wrappers
        scheduler_bridge.rs         # EmbeddedScheduler integration
        send.rs                     # Safe enif_send wrapper
        macros.rs                   # #[async_nif], #[sync_nif], #[module]
        nif_sys.rs                  # Raw erl_nif.h bindings (minimal unsafe)
        error.rs                    # NifResult, ConvertError
      examples/
        fast_xml_bridge/            # Example: XML parsing NIF
          Cargo.toml
          src/lib.rs
          priv/fast_xml_bridge.erl  # Erlang wrapper module
```

### 8.1 Dependencies

```toml
[dependencies]
nera-runtime = { path = "../nera-runtime" }  # Scheduler, heaps, actors

# Minimal — no dependency on Rustler
# Raw NIF bindings generated from erl_nif.h headers
```

### 8.2 Why Not Depend on Rustler

Rustler is a good project, but depending on it creates friction:

- **Redundant scheduler**: Rustler doesn't know about nera's scheduler.
  Combining the two would mean maintaining two threading models.
- **Term representation mismatch**: Rustler wraps BEAM terms directly.
  nera-nif needs to bridge between BEAM terms and nera terms (when
  nera-compiled modules are used inside the NIF in the future).
- **Macro conflicts**: both would want to generate NIF boilerplate. Having
  one coherent macro system is simpler.
- **Build complexity**: Rustler has its own build tooling (Mix integration,
  OTP version detection). nera-nif should be a pure `cargo build`.

The raw NIF C API (`erl_nif.h`) is small and stable. Wrapping it directly
in ~500 lines of `nif_sys.rs` is less effort than integrating Rustler and
working around its assumptions.

---

## 9. Integration with ejabberd

### 9.1 Migration Path

Each C NIF is replaced independently. The Erlang module interface stays the
same (or gains an async variant):

```
Step 1: fast_xml (highest risk, highest value)
  - Replace C parser with Rust parser (e.g., quick-xml crate)
  - Wrap with #[nera_nif::async_nif]
  - Erlang module: fast_xml_bridge (drop-in for fast_xml API)
  - Test: run ejabberd test suite, benchmark with tsung

Step 2: fast_tls (high risk)
  - Replace OpenSSL NIF with rustls-based implementation
  - Wrap with #[nera_nif::async_nif]
  - Erlang module: fast_tls_bridge

Step 3: stringprep (medium risk)
  - Replace C stringprep with Rust unicode-normalization crate
  - May use #[nera_nif::sync_nif] (fast enough for sync)

Step 4: ezlib (low risk, low priority)
  - Replace C zlib NIF with flate2 crate
  - Wrap with #[nera_nif::async_nif] for stream compression
```

### 9.2 Build Integration

The NIF `.so` files would be built by `cargo` and placed in ejabberd's `priv/`
directory:

```
ejabberd/
  priv/
    lib/
      fast_xml_bridge.so    ← cargo build --release (from nera-nif example)
      fast_tls_bridge.so
```

ejabberd's `rebar.config` or `Makefile` would invoke `cargo build` for the
Rust NIFs, similar to how it currently invokes `make` for C NIFs.

### 9.3 Erlang Module Structure

```erlang
-module(fast_xml_bridge).

-on_load(init/0).

-export([parse/1, parse/2, parse_async/1]).

init() ->
    PrivDir = code:priv_dir(ejabberd),
    erlang:load_nif(filename:join([PrivDir, "lib", "fast_xml_bridge"]), 0).

parse(Data) ->
    parse(Data, 5000).

parse(Data, Timeout) ->
    Ref = make_ref(),
    ok = parse_nif(self(), Ref, Data),
    receive
        {xml_parsed, Ref, Result} -> {ok, Result};
        {xml_error, Ref, Reason}  -> {error, Reason}
    after Timeout ->
        {error, timeout}
    end.

parse_async(Data) ->
    Ref = make_ref(),
    ok = parse_nif(self(), Ref, Data),
    {ok, Ref}.

%% NIF stub — replaced by native code on load
parse_nif(_CallerPid, _Ref, _Data) ->
    erlang:nif_error(not_loaded).
```

---

## 10. Comparison with Alternatives

### 10.1 Feature Matrix

| Feature | C NIF | Dirty NIF | Rustler | Port | nera-nif |
|---|---|---|---|---|---|
| Memory-safety risk in native code | High (C) | High (C) | Lower (safe Rust, `unsafe` boundary) | Contained (separate process) | **Lower (safe Rust, `unsafe` boundary)** |
| Blocks BEAM scheduler | Yes | No | Yes | No | **No** |
| VM can stop on a native bug | Yes | Yes | Yes, less often | No | **Yes, less often** |
| Actor concurrency | No | No | No | No | **Yes** |
| Sub-task parallelism | Manual pthreads | Manual pthreads | Manual tokio | N/A | **spawn()** |
| Crash isolation | None | None | Partial (unwinding panics) | Full (OS process) | **Partial (unwinding panics)** |
| GC interference | Risk | Risk | Risk | None | **None** |
| Communication cost | ~0 (sync) | ~0 (sync) | ~0 (sync) | ~10-50 us (pipe) | **~2 us (async)** |
| Build system | autotools + gcc | autotools + gcc | cargo | Any | **cargo** |
| Erlang API change | None | None | None | Different API | **Minimal** (async wrapper) |

### 10.2 When to Use Each

| Approach | Best For |
|---|---|
| C NIF | Legacy code that works and isn't worth rewriting |
| Dirty NIF | Quick fix to stop a C NIF from blocking schedulers |
| Rustler (sync) | Fast operations (< 1 us) that benefit from Rust safety |
| Port | Untrusted external programs, crash-critical isolation |
| **nera-nif (async)** | **CPU-intensive work (> 10 us) that benefits from Rust and concurrency, where a native failure may stop the VM** |

---

## 11. Implementation Roadmap

### Phase 1 — Core Bridge (MVP)

Deliver the minimum viable bridge: async dispatch, result return, panic
handling. No macros — manual NIF wiring.

| Task | Effort | Depends On |
|---|---|---|
| `nif_sys.rs` — raw `erl_nif.h` bindings | Small | None |
| `env.rs` / `term.rs` — safe BEAM term wrappers | Medium | nif_sys |
| `convert.rs` — `FromBeam` / `IntoBeam` for `Binary`, `Atom`, `i64` | Medium | env, term |
| `scheduler_bridge.rs` — `EmbeddedScheduler` API on nera-runtime | Medium | nera-runtime |
| `send.rs` — safe `enif_send` wrapper | Small | env, nif_sys |
| Panic handling (`catch_unwind` per actor) | Small | scheduler_bridge |
| Example: trivial echo NIF (send binary, get it back) | Small | All above |
| Example: XML parsing NIF using `quick-xml` | Medium | All above |

**Deliverable**: a working `.so` that BEAM can load, with async dispatch
to nera workers and message-based result return.

### Phase 2 — Macro System and Ergonomics

| Task | Effort | Depends On |
|---|---|---|
| `#[nera_nif::async_nif]` proc macro | Medium | Phase 1 |
| `#[nera_nif::sync_nif]` proc macro | Small | Phase 1 |
| `#[nera_nif::module]` proc macro (NIF table generation) | Medium | Phase 1 |
| `FromBeam` / `IntoBeam` for tuples, lists, maps | Medium | Phase 1 |
| Error type ergonomics (`NifResult`, `?` operator) | Small | Phase 1 |
| Comprehensive documentation and examples | Medium | All above |

**Deliverable**: developer-friendly macro API. Writing a new NIF requires
< 20 lines of Rust.

### Phase 3 — Full Term Conversion and nera-compiled Modules

| Task | Effort | Depends On |
|---|---|---|
| BEAM term → nera term bidirectional conversion | Large | Phase 1 |
| Atom table synchronization (BEAM atoms ↔ nera atoms) | Medium | Phase 1 |
| Run nera-compiled Erlang modules inside the NIF | Large | nera Phase 3 |
| ejabberd fast_xml replacement (production-ready) | Large | Phase 2 |
| ejabberd fast_tls replacement | Large | Phase 2 |

**Deliverable**: full integration where nera-compiled Erlang code runs
inside a BEAM NIF with bidirectional term conversion.

---

## 12. Open Questions

1. **Thread count coordination**: should nera's worker threads be pinned to
   separate cores from BEAM schedulers, or should they share? Over-subscribing
   cores could cause contention. Under-subscribing wastes capacity.

2. **Backpressure**: if BEAM submits work faster than nera can process it,
   the queue grows unbounded. Should `spawn_work` block when the queue exceeds
   a threshold? Or should it return an error that Erlang code can handle?

3. **Binary reference sharing**: for large binaries (> 64 bytes), copying on
   inbound is wasteful. Can we use BEAM's `enif_make_resource` to share a
   reference-counted binary between BEAM and nera without copying? This
   requires careful lifetime management.

4. **NIF upgrade**: BEAM supports NIF module upgrade (loading a new `.so`
   while the old one is still in use). The nera scheduler must handle
   graceful shutdown of the old instance while the new one starts.

5. **Monitoring**: how does BEAM know if the nera scheduler is healthy?
   Should the bridge expose metrics (queue depth, active actors, GC count)
   via a BIF or sys module?

---

## 13. Summary

`nera-nif` bridges two worlds:

- **BEAM**: battle-tested OTP runtime, massive ecosystem, the right choice
  for application logic, routing, supervision, clustering.
- **nera runtime**: Rust actor system with per-actor heaps, per-work-item
  panic handling, and zero BEAM scheduler interference.

The bridge lets BEAM applications keep their architecture while offloading
the dangerous, CPU-intensive parts to a runtime designed for exactly that
workload. For ejabberd, this means replacing fragile C NIFs with concurrent
Rust actors that are less exposed to memory corruption — one module at a time,
with no changes to the Erlang codebase beyond swapping a module name. Those
actors still run inside the BEAM's OS process: a native failure that
`catch_unwind` cannot catch still stops the VM.
