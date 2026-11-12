# nera Standard Library Design — Three-Tier Architecture

## 1. Overview

nera needs a standard library that provides Erlang developers with the
modules they expect (`gen_server`, `json`, `lists`, `file`, etc.). The key
design question is: what should be implemented in Erlang (compiled by nera)
versus in Rust (linked into the runtime)?

The answer is **both**, at different layers. This document defines a
three-tier architecture that places each piece of functionality at the
appropriate level based on its nature, performance requirements, and
coupling to the runtime.

---

## 2. The Three Tiers

```
┌──────────────────────────────────────────────────────────┐
│  Tier 3 — Erlang Libraries                               │
│  gen_server.erl, supervisor.erl, proc_lib.erl            │
│  Written in Erlang, compiled by nera alongside user code │
│  Depends on: Tier 1 primitives + Tier 2 libraries        │
├──────────────────────────────────────────────────────────┤
│  Tier 2 — Rust Extension Libraries                       │
│  json, re, crypto, base64, file, unicode, http_client    │
│  Pure functions: ErlTerm → Rust → ErlTerm                │
│  No scheduler/GC coupling. Leverages Rust crate ecosystem │
├──────────────────────────────────────────────────────────┤
│  Tier 1 — Core Runtime Primitives                        │
│  Scheduler, heap/GC, process management, message passing, │
│  links, monitors, EXIT signals, atom table, process       │
│  registry, ETS, timers                                    │
│  Tightly coupled to scheduler internals                   │
└──────────────────────────────────────────────────────────┘
```

### Decision Criteria

| Question                                               | Tier 1 (Rust, runtime) | Tier 2 (Rust, extension) | Tier 3 (Erlang)     |
|--------------------------------------------------------|------------------------|--------------------------|---------------------|
| Does it need scheduler access?                         | Yes                    | No                       | No                  |
| Does it manipulate process state?                      | Yes                    | No                       | Via message passing |
| Is there a mature Rust crate?                          | N/A                    | Yes                      | N/A                 |
| Would a pure Erlang impl be 10x+ slower?               | N/A                    | Yes                      | No                  |
| Is it naturally expressed as message-passing patterns? | No                     | No                       | Yes                 |

### Erlang-First Rule for New Stdlib APIs

For new stdlib APIs, the default is **Tier 3 (Erlang module)**.

Why this is the default:

- Erlang code naturally cooperates with reduction-based scheduling.
- It avoids long native sections that can delay scheduler fairness.
- For many utility functions, Rust BIF vs Erlang performance is often close
  enough that maintainability wins.

Promote to a Rust BIF (Tier 1 or Tier 2) only when at least one condition
holds:

1. The feature needs runtime internals Erlang cannot access (scheduler,
   process internals, term representation, timers, ETS internals).
2. The feature needs semantics that cannot be expressed safely in Erlang
   (for example, specific atomicity guarantees).
3. Benchmarks on representative workloads show a material win that matters in
   practice (guideline: at least ~2x end-to-end for the targeted operation).

If implemented as a BIF, it must still be scheduler-safe: bounded work per
call, no blocking on scheduler threads, and proper reduction accounting (or
trap/yield behavior).

---

## 3. Tier 1 — Core Runtime Primitives

These are implemented in Rust inside `crates/nera-runtime/` because they
require direct access to scheduler internals, process state, or memory
management.

### Currently Implemented

| Primitive                            | Location        | Purpose                     |
|--------------------------------------|-----------------|-----------------------------|
| Scheduler (N:M work-stealing)        | `scheduler.rs`  | Process scheduling          |
| Per-process heap (bumpalo)           | `heap.rs`       | Term allocation             |
| Copying GC                           | `heap.rs`       | Memory reclamation          |
| Tagged terms (64-bit)                | `term.rs`       | Value representation        |
| Atom table (global, lock-free reads) | `atom_table.rs` | Atom interning              |
| Message passing (deep copy)          | `message.rs`    | Inter-process communication |
| Selective receive                    | `process.rs`    | Mailbox scanning            |
| Process spawning                     | `ffi.rs`        | `spawn/3`                   |
| Socket registry                      | `socket.rs`     | gen_tcp handle management   |

### Implemented Primitives

These primitives are all fully operational, enabling OTP behaviours (Tier 3):

| Primitive                                 | Why Rust?                                                |
|-------------------------------------------|----------------------------------------------------------|
| `register/2`, `whereis/1`, `unregister/1` | Global process name table, needs atomic access           |
| `link/1`, `unlink/1`                      | Bidirectional link sets on each process, checked at exit |
| `process_flag(trap_exit, true)`           | Per-process flag, changes exit signal behavior           |
| EXIT signal propagation                   | Scheduler must deliver signals when a process dies       |
| `monitor/2`, `demonitor/1`                | Unidirectional monitoring with `DOWN` message delivery   |
| `process_info/1,2`                        | Introspection into process internals                     |
| `put/2`, `get/1` (process dictionary)     | Per-process mutable state, needs GC root tracking        |
| `erlang:send_after/3`, timers             | Timer wheel integrated with scheduler wake-up            |
| ETS tables                                | Shared mutable storage with concurrent access            |

The dependency chain for OTP support:

```
register/whereis  ─┐
link/trap_exit     ├──► gen_server.erl (Tier 3)
monitor/demonitor ─┤
EXIT signals      ─┘
                        │
                        ▼
                   supervisor.erl (Tier 3)
```

---

## 4. Tier 2 — Rust Extension Libraries

These are implemented in Rust but are **not coupled to the runtime core**.
They are pure (or nearly pure) functions: they take Erlang terms as input,
call a Rust crate, and return Erlang terms. They don't touch the scheduler,
don't manage processes, and don't need GC integration beyond allocating
results on the process heap.

### Why Rust Instead of Erlang?

Two reasons:

1. **Performance**: `serde_json` parses JSON 10-100x faster than any parser
   written in compiled Erlang. `ring` computes SHA-256 hundreds of times
   faster. These are domains where native code with SIMD, cache-friendly
   layouts, and zero-allocation parsing matter.

2. **Ecosystem**: the Rust crate ecosystem has production-grade
   implementations of JSON, regex, crypto, HTTP, compression, and more.
   Reimplementing them in Erlang would be a multi-year effort with inferior
   results.

This matches what BEAM does: `crypto` is backed by OpenSSL via NIFs, `re`
is backed by PCRE via NIFs, `zlib` is backed by zlib via a port driver.
nera uses Rust crates instead of C libraries.

### Proposed Modules

| Module        | Rust Crate              | API Surface                                 | Priority |
|---------------|-------------------------|---------------------------------------------|----------|
| `json`        | `serde_json`            | `encode/1`, `decode/1`                      | High     |
| `base64`      | `base64`                | `encode/1`, `decode/1`                      | High     |
| `re`          | `regex`                 | `run/2,3`, `replace/3,4`, `split/2,3`       | High     |
| `file`        | `std::fs`               | `read_file/1`, `write_file/2`, `list_dir/1` | High     |
| `crypto`      | `ring` or `sha2`+`hmac` | `hash/2`, `mac/3,4`                         | Medium   |
| `unicode`     | `unicode-segmentation`  | `characters_to_binary/1,2`                  | Medium   |
| `filename`    | `std::path`             | `join/1,2`, `basename/1`, `dirname/1`       | Medium   |
| `os`          | `std::env`              | `getenv/1`, `cmd/1`                         | Medium   |
| `zlib`        | `flate2`                | `compress/1`, `uncompress/1`                | Low      |
| `http_client` | `ureq` (sync)           | `get/1`, `post/2,3`                         | Low      |
| `rand`        | `rand`                  | `uniform/0,1`, `seed/1`                     | Low      |
| `calendar`    | `chrono` or `time`      | `local_time/0`, `universal_time/0`          | Low      |

### Implementation Pattern

Every Tier 2 module follows the same four-step pattern, already established
by `gen_tcp` and `socket.rs`:

#### Step 1: Logic module (`crates/nera-runtime/src/<module>.rs`)

Pure Rust logic, no FFI boilerplate. Works with `ProcessHeap` and `ErlTerm`
directly:

```rust
// crates/nera-runtime/src/json.rs
use serde_json::Value;

/// Encode an ErlTerm (map/list/number/binary/atom) to a JSON binary.
pub fn encode(heap: &mut ProcessHeap, term: ErlTerm) -> Result<ErlTerm, ErlTerm> {
    let value = term_to_json(term)?;
    let bytes = serde_json::to_vec(&value).map_err(|e| make_error(heap, e))?;
    Ok(heap.alloc_binary(&bytes))
}

/// Decode a JSON binary into an ErlTerm.
pub fn decode(heap: &mut ProcessHeap, bin: ErlTerm) -> Result<ErlTerm, ErlTerm> {
    let bytes = bin.as_binary_bytes()?;
    let value: Value = serde_json::from_slice(bytes).map_err(|e| make_error(heap, e))?;
    Ok(json_to_term(heap, &value))
}
```

Term-to-JSON mapping:

| Erlang type                                  | JSON type |
|----------------------------------------------|-----------|
| `#{<<"key">> => Val}` (map with binary keys) | object    |
| `[1, 2, 3]` (list)                           | array     |
| Small integer / float                        | number    |
| `<<"text">>` (binary)                        | string    |
| `true` / `false`                             | boolean   |
| `null`                                       | null      |

#### Step 2: FFI wrapper (`crates/nera-runtime/src/ffi.rs`)

Thin C-ABI function, same convention as all existing FFI:

```rust
#[no_mangle]
pub extern "C" fn nera_rt_json_encode(ctx: *mut ProcessContext, term: u64) -> u64 {
    let heap = unsafe { &mut *(*ctx).heap_ptr };
    match json::encode(heap, ErlTerm(term)) {
        Ok(result) => make_ok_tuple(heap, result).0,
        Err(reason) => make_error_tuple(heap, reason).0,
    }
}

#[no_mangle]
pub extern "C" fn nera_rt_json_decode(ctx: *mut ProcessContext, bin: u64) -> u64 {
    let heap = unsafe { &mut *(*ctx).heap_ptr };
    match json::decode(heap, ErlTerm(bin)) {
        Ok(result) => make_ok_tuple(heap, result).0,
        Err(reason) => make_error_tuple(heap, reason).0,
    }
}
```

#### Step 3: BIF registration (`nera-ir/src/bif_table.rs`)

All BIF metadata lives in a unified table (`nera-ir/src/bif_table.rs`). One entry per function — IR lowering and codegen both consume it automatically:

```rust
("json", "encode", 1) => Some(RuntimeFunc {
    symbol: "nera_rt_json_encode",
    arity: 1,
    returns_via_ctx: false,
}),
("json", "decode", 1) => Some(RuntimeFunc {
    symbol: "nera_rt_json_decode",
    arity: 1,
    returns_via_ctx: false,
}),
```

#### Step 4: User writes normal Erlang

```erlang
-module(my_api).
-export([handle_request/1]).

handle_request(Body) ->
    case json:decode(Body) of
        {ok, #{<<"action">> := Action}} ->
            Response = #{status => ok, action => Action},
            {ok, json:encode(Response)};
        {error, Reason} ->
            {error, Reason}
    end.
```

No special syntax. No imports. The module name (`json`) resolves to a Rust
implementation at compile time, exactly like `io:format` or `gen_tcp:listen`
do today.

### What This Is NOT

This is **not a general-purpose FFI/NIF system** for end users. It is the
compiler's own standard library, implemented in Rust for pragmatic reasons.
There is no plugin mechanism, no dynamic loading, no user-extensible
dispatch. The set of Tier 2 modules is fixed at compile time.

A user-facing FFI system (analogous to BEAM NIFs) is a separate concern,
documented in `NERA_NIF_DESIGN.md`.

---

## 5. Tier 3 — Erlang Libraries

These are `.erl` files compiled by nera alongside user code. They express
application-level patterns that are naturally built on message passing,
pattern matching, and process management — the things Erlang excels at.

### Why Erlang Instead of Rust?

OTP behaviours are not magic. `gen_server` is approximately 200 lines of
Erlang: a `receive` loop that dispatches `call`, `cast`, and `info`
messages to callback functions. Hardcoding this in Rust would mean:

- **Combinatorial explosion**: gen_server, gen_statem, gen_event,
  supervisor, application, proc_lib, sys — each with many edge cases.
- **Semantics drift**: subtle incompatibilities with real OTP that confuse
  experienced Erlang developers.
- **Rigidity**: users cannot customize, extend, or inspect the
  implementation. In real Erlang, you can read gen_server source, copy it,
  modify it.

This matches BEAM's own design: the VM provides `link/1`, `monitor/2`,
`register/2` as BIFs (our Tier 1), and all of OTP is built on top in
Erlang (our Tier 3).

### Implemented Modules

These are implemented and shipped in `erlang/stdlib/`:

| Module             | Depends On (Tier 1)                 | Description                                  |
|--------------------|-------------------------------------|----------------------------------------------|
| `proc_lib`         | spawn, links, EXIT signals          | "Safe" process spawning with crash reports ✅ |
| `gen_server`       | register, monitor, links, trap_exit | Generic server behaviour ✅                   |
| `gen_statem`       | same as gen_server                  | Generic state machine                        |
| `supervisor`       | gen_server, links, monitors         | Process supervision trees ✅                  |
| `application`      | supervisor                          | Application start/stop lifecycle ✅           |
| `sys`              | gen_server message protocol         | Debug and introspection for OTP processes    |
| `gen_event`        | gen_server                          | Generic event handler                        |
| `lists`            | none (pure)                         | List utilities (map, foldl, filter, ...) ✅  |
| `proplists`        | none (pure)                         | Property list utilities                      |
| `string`           | none (pure)                         | String manipulation                          |
| `dict` / `orddict` | none (pure)                         | Dictionary data structures                   |
| `queue`            | none (pure)                         | Double-ended queue                           |
| `timer`            | send_after, spawn                   | Timer utilities (apply_after, send_interval) ✅ |
| `io_lib`           | none (pure)                         | I/O formatting utilities                     |

**Note on pure modules**: `lists`, `string`, `proplists`, and similar
modules contain no process interaction. They could theoretically be Tier 2
(Rust), but they are naturally and efficiently expressed in Erlang. A
compiled `lists:map/2` over a 1000-element list runs in microseconds — fast
enough that Rust adds no practical benefit. These modules also serve as a
test of nera's own compilation: if `lists.erl` compiles and runs
correctly, the compiler is sound for real-world code.

### Compilation Model

**Implemented:** Option A (stdlib directory) is the current approach. Stdlib `.erl` files live in `erlang/stdlib/` and are auto-discovered via `find_stdlib_dir()` — which checks installed layout (`<exe>/../lib/nera/erlang/stdlib/`), workspace layout (`<exe>/../../erlang/stdlib/`), and CWD fallback. User modules shadow stdlib by name. The `--no-stdlib` flag disables auto-inclusion.

Tier 3 modules are compiled the same way as user modules. There are two
possible integration strategies:

**Option A: Stdlib directory**

nera ships with a `lib/` directory of `.erl` files. The compiler
automatically includes them when their module name is referenced:

```
nera/
  lib/
    gen_server.erl
    supervisor.erl
    lists.erl
    ...
```

```bash
$ nera my_app.erl -o my_app
# Compiler sees gen_server:start_link → includes lib/gen_server.erl
```

**Option B: Pre-compiled object files**

The stdlib modules are pre-compiled to `.o` files during `cargo build`. The
linker includes them when referenced:

```
nera/
  lib/
    gen_server.o    # Pre-compiled by nera during build
    supervisor.o
    lists.o
```

Option A is simpler and allows easier development/debugging. Option B is
faster for large projects since stdlib modules are compiled once. Both can
coexist — start with A, add B as an optimization.

---

## 6. Comparison with BEAM/OTP

The three-tier split mirrors how BEAM/OTP is structured, with Rust
replacing C:

| Layer                   | BEAM/OTP                                                 | nera                                                              |
|-------------------------|----------------------------------------------------------|-------------------------------------------------------------------|
| Runtime primitives      | C code in erts (link, monitor, ETS, scheduler)           | Rust in nera-runtime (Tier 1)                                     |
| Native library bindings | C NIFs/port drivers (crypto→OpenSSL, re→PCRE, zlib→zlib) | Rust extensions (crypto→ring, re→regex, json→serde_json) (Tier 2) |
| OTP frameworks          | Erlang modules (gen_server.erl, supervisor.erl)          | Erlang modules compiled by nera (Tier 3)                          |

The advantage of nera's approach is **lower risk, not containment**. Tier 2
modules use Rust crates, mostly written in safe Rust, instead of C libraries
accessed through the NIF API. A bug in safe Rust code cannot cause the buffer
overflows, use-after-free or data races that make a C parser or TLS library
dangerous. That Rust code still runs in the same process and address space
as the runtime and every Erlang process, so it is not isolated from them,
and a native failure can still stop the whole program. The next section
states these limits.

### What Rust Does and Does Not Guarantee

These limits apply to all native code in a nera program: Tier 1 primitives,
Tier 2 modules and the crates they use.

**Rust reduces risk.** In code written in safe Rust, the language prevents
memory-safety bugs such as out-of-bounds writes, use-after-free and data
races: an out-of-range index panics instead of corrupting memory. Invalid
input can be reported as an error (`badarg`, `{error, Reason}`), provided the
code checks it before it can panic.

**Rust does not contain native code.** Tier 1 and Tier 2 code is linked into
the same executable and runs on the same scheduler threads as compiled Erlang
code. No sandbox, separate OS process or memory protection separates a Tier 2
module from the runtime or from other Erlang processes. Safe Rust's guarantee
ends at:

- `unsafe` blocks. The runtime uses them for its term representation and
  heaps, and in nearly every C-ABI entry point called by generated code (the
  `nera_rt_json_encode` example above dereferences a raw context pointer).
- Dependencies that contain `unsafe` code or code in other languages, such as
  the C jemalloc allocator linked on non-MSVC targets.
- The calling convention between generated code and the runtime.

A defect in any of these can corrupt memory that belongs to the runtime or to
any Erlang process. Rust also does not limit how much memory or processor time
a native call uses.

**Nothing guarantees the runtime keeps running.** An Erlang exception ends one
process, and links and monitors report it. A native failure is not reported
that way: no `EXIT` or `DOWN` message is delivered. Usually it ends the whole
OS process, and every Erlang process stops with it. Today the runtime does not
catch Rust panics. Its BIF entry points are `extern "C"` functions, so a panic
that reaches one aborts the program. A panic in runtime code that no
`extern "C"` frame encloses, such as garbage collection in a scheduler's own
loop or the timer and I/O threads, instead ends only that thread and can leave
the Erlang processes that depend on it stalled. Catching panics would not
change the following, which also stop the program:

- memory allocation failure (Rust aborts by default);
- stack overflow on a native thread;
- a panic raised while another panic is unwinding;
- an explicit abort;
- undefined behaviour in `unsafe` or foreign code, which may instead corrupt
  memory silently and fail later.

Tier 2 code therefore still has to validate its input, return errors instead
of panicking, and bound its allocations and work. Keep its `unsafe` code small
and reviewed. Rust makes a runtime crash less likely; it neither makes one
impossible nor confines one to the calling Erlang process.

### Where nera Can Exceed OTP

Because Tier 2 modules are part of the compiler's own build, nera can
offer batteries-included functionality that OTP historically left to
third-party packages:

| Capability  | OTP stdlib                    | nera stdlib                                    |
|-------------|-------------------------------|------------------------------------------------|
| JSON        | Added in OTP 27 (pure Erlang) | `serde_json` (10-100x faster)                  |
| HTTP client | `httpc` (dated, limited)      | `ureq` (modern, TLS built-in)                  |
| Regex       | `re` (PCRE via NIF)           | `regex` (linear time, no backtracking DoS)     |
| Crypto      | `crypto` (OpenSSL via NIF)    | `ring` (Rust, C and assembly)                  |
| Compression | `zlib` (C via port driver)    | `flate2` (Rust; backend may use `unsafe` or C) |
| UUID        | not in stdlib                 | `uuid` crate                                   |
| Base64      | `base64` (pure Erlang)        | `base64` crate (SIMD-accelerated)              |

---

## 7. File Organization

```
crates/nera-runtime/src/
  lib.rs
  term.rs              # Tier 1: term representation
  heap.rs              # Tier 1: per-process heap + GC
  scheduler.rs         # Tier 1: work-stealing scheduler
  process.rs           # Tier 1: process state, mailbox
  message.rs           # Tier 1: deep-copy message passing
  atom_table.rs        # Tier 1: global atom table
  ffi.rs               # FFI wrappers for both Tier 1 and Tier 2
  bif.rs               # BIF implementations
  socket.rs            # Tier 2: gen_tcp backend (existing)
  entry.rs             # Tier 1: C-ABI entry point
  # --- Tier 2 extension modules ---
  json.rs              # json:encode/1, json:decode/1
  re_ext.rs            # re:run/2, re:replace/3
  crypto_ext.rs        # crypto:hash/2, crypto:mac/3
  base64_ext.rs        # base64:encode/1, base64:decode/1
  file_ext.rs          # file:read_file/1, file:write_file/2
  os_ext.rs            # os:getenv/1, os:cmd/1

crates/nera-codegen/src/
  builtins.rs          # Maps module:function → FFI symbol (Tier 1 + Tier 2)

lib/                   # Tier 3: Erlang stdlib modules
  lists.erl
  proplists.erl
  gen_server.erl
  supervisor.erl
  proc_lib.erl
  string.erl
  io_lib.erl
  queue.erl
```

---

## 8. Implementation Roadmap

### Phase A — Tier 2 Foundation (no Tier 1 changes needed)

Add high-value Rust extension modules that work today:

1. **`json`** — `serde_json`. High demand, immediate value.
2. **`base64`** — trivial to add, constant use in web/networking.
3. **`file`** — `read_file/1`, `write_file/2`. Basic file I/O.
4. **`re`** — `regex` crate. Needed for string processing.

### Phase B — Tier 1 Primitives for OTP

Add the runtime primitives that OTP behaviours require:

1. `register/2`, `whereis/1` — process registry
2. `link/1`, `process_flag(trap_exit, true)` — bidirectional links
3. EXIT signal propagation — scheduler-level
4. `monitor/2`, `demonitor/1` — unidirectional monitors
5. `put/2`, `get/1` — process dictionary
6. `erlang:send_after/3` — timer support

### Phase C — Tier 3 OTP in Erlang

Build OTP behaviours as Erlang modules:

1. `proc_lib.erl` — safe spawning with crash reports
2. `gen_server.erl` — generic server
3. `supervisor.erl` — process supervision
4. `lists.erl` — standard list operations

### Phase D — Extended Tier 2

Add remaining extension modules as demand arises:

- `crypto`, `unicode`, `zlib`, `http_client`, `rand`, `calendar`

---

## 9. Summary

The guiding principle is: **implement each piece at the lowest tier that
makes sense**.

- Default for new stdlib APIs → **Tier 3** (Erlang modules)
- If it touches scheduler/runtime internals → **Tier 1** (Rust, runtime core)
- If Rust provides a measured material win or unavailable capability →
  **Tier 2** (Rust, extension library)

This gives nera a "batteries included" standard library that is less exposed
to memory-safety bugs (Rust instead of C NIFs, although native code is not
isolated and can still stop the runtime; see
[What Rust Does and Does Not Guarantee](#what-rust-does-and-does-not-guarantee)),
faster (serde_json instead of pure Erlang JSON), and
idiomatically correct (gen_server is real Erlang, not a Rust reimagination
of what gen_server might look like).
