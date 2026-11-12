# nera and ejabberd — Use Cases and Integration Strategy

**Demonstrator priority (2026-10-14):** start with the
[standalone XMPP component roadmap](XMPP_COMPONENT_ROADMAP.md), which combines
source reuse, a statically checked Erlang core and native deployment. The broader
integration proposals below are background designs; their estimates do not
establish current compatibility or measured deployment properties.

## 1. Executive Summary

**ejabberd** is one of the most complex Erlang/OTP applications in production,
serving millions of concurrent XMPP, MQTT, and SIP connections worldwide. It
relies deeply on OTP behaviors, hot code loading, distribution, Mnesia, ETS, and
a large ecosystem of C-based NIFs for performance-critical paths.

**nera cannot replace BEAM for ejabberd.** Full OTP compatibility is a non-goal,
and the gap between nera's current feature set and what ejabberd requires is
measured in years of engineering effort.

However, nera can **complement** ejabberd in two concrete ways:

1. **A non-blocking NIF replacement with lower memory-safety risk** — nera's
   runtime (scheduler + per-process heaps + actor model) can be embedded as a
   Rust library inside BEAM NIFs, offloading CPU-intensive work to dedicated
   threads without ever blocking BEAM schedulers. Rust makes a VM crash less
   likely than C does, but the embedded runtime shares the VM's OS process, so
   a native failure can still stop the VM.

2. **Lightweight edge proxies and IoT gateways** — nera can compile
   stripped-down XMPP/MQTT proxies into single static binaries (2-5 MB) for
   deployment on constrained devices that cannot run a full BEAM installation.

This document describes both use cases in detail, covering architecture,
implementation strategy, and the nera features required to enable them.

---

## 2. Why ejabberd Cannot Run on nera

ejabberd depends on BEAM features that are fundamental to its architecture and
are explicit non-goals for nera:

| BEAM Feature                                       | ejabberd Usage                                | nera Status               |
|----------------------------------------------------|-----------------------------------------------|---------------------------|
| OTP behaviors (gen_server, supervisor, gen_statem) | Every module                                  | gen_server, supervisor ✅  |
| Hot code loading                                   | Live upgrades in production                   | Not planned               |
| Distribution protocol                              | Multi-node clustering, Mnesia replication     | Not planned               |
| Mnesia / ETS                                       | Session tables, routing tables, configuration | ETS ✅, Mnesia not planned |
| NIFs                                               | fast_xml, fast_tls, stringprep, ezlib         | N/A (different approach)  |
| Ports                                              | External program integration                  | Not implemented           |
| Elixir support                                     | Mix configuration, Elixir modules             | Not planned               |
| Exception handling                                 | try/catch/throw pervasive throughout          | ✅ Implemented             |
| Binary/bit syntax                                  | XMPP stream parsing, protocol framing         | ✅ Implemented (bit-level) |

The codebase is ~300K+ lines of Erlang/Elixir. Reaching feature parity with BEAM
for this workload is not a realistic objective.

---

## 3. Use Case 1 — nera Runtime as a Better NIF

### 3.1 The Problem with C NIFs Today

ejabberd uses several C-based NIFs for performance-critical operations:

- **fast_xml** — XML stream parsing (the most CPU-intensive path in XMPP)
- **fast_tls** — TLS handshake and record processing
- **stringprep / SCRAM** — Unicode normalization and authentication
- **ezlib** — zlib stream compression

These NIFs share three fundamental problems:

**Memory unsafety.** C code runs in the BEAM's address space with no protection.
A buffer overflow in fast_xml can corrupt the BEAM heap, crashing the entire VM
and dropping all connected users.

**Scheduler blocking.** NIFs run on BEAM scheduler threads. A NIF that takes
longer than 1 ms to return degrades all Erlang processes sharing that scheduler.
XML parsing of large stanzas and TLS handshakes routinely exceed this threshold.

**Build complexity.** C NIFs require autotools, gcc/clang, platform-specific
flags, and careful cross-compilation setup. This creates friction for
contributors and CI pipelines.

### 3.2 Existing Alternatives and Their Limitations

**Dirty schedulers (OTP 20+).** NIFs can be marked as `dirty_cpu` or `dirty_io`
to run on dedicated threads. This solves scheduler blocking but not memory
safety — a crash still takes down the VM.

**Rustler (Rust NIF bindings).** Lets NIFs be written mostly in safe Rust, which
lowers the memory-safety risk and simplifies builds. The NIF still shares the
VM's OS process, so an abort in it stops the VM. It also still runs on a BEAM
scheduler thread (or dirty scheduler). There is no built-in concurrency model
inside the NIF — for parallel work, you must manually integrate tokio or
rayon, which do not provide Erlang-style process isolation or fault tolerance.

**Ports (external processes).** Fully isolated but require serialization over
pipes. The latency overhead is too high for hot-path XML parsing (microsecond
budget).

### 3.3 The nera Bridge Architecture

nera's runtime provides exactly what is missing: **a Rust-native actor system
with its own scheduler and per-process heaps** — embeddable as a library inside
a BEAM NIF.

```
┌──────────────────────────────────────────────────────────────┐
│                     BEAM VM (ejabberd)                        │
│                                                               │
│  Erlang process (c2s connection handler)                     │
│    │                                                          │
│    ├── fast_xml_bridge:parse_async(self(), Ref, Data)        │
│    │     │                                                    │
│    │     ▼                                                    │
│    │   ┌────────────────────────────────────────┐            │
│    │   │  Rust NIF — thin bridge layer          │            │
│    │   │                                         │            │
│    │   │  1. Convert BEAM binary → Rust &[u8]   │            │
│    │   │  2. Submit work to nera scheduler ─────────┐       │
│    │   │  3. Return immediately to BEAM          │   │       │
│    │   └────────────────────────────────────────┘   │       │
│    │                                                  │       │
│    ├── receive                                        │       │
│    │     {xml_parsed, Ref, Elements} ->              │       │
│    │       process_stanzas(Elements)                 │       │
│    │   end                                            │       │
│                                                       │       │
└───────────────────────────────────────────────────────│──────┘
                                                        │
          ┌─────────────────────────────────────────────▼──────┐
          │    nera runtime (linked into the BEAM process)      │
          │                                                     │
          │  ┌──────────┐ ┌──────────┐ ┌──────────┐           │
          │  │ worker 0  │ │ worker 1  │ │ worker 2  │  ...     │
          │  └─────┬─────┘ └──────────┘ └──────────┘           │
          │        │                                            │
          │        ▼                                            │
          │  nera actor:                                       │
          │    - Parses XML on its own thread                   │
          │    - Own heap, own GC                               │
          │    - Can spawn sub-actors for parallel chunks       │
          │    - On completion: enif_send() result to BEAM PID │
          │                                                     │
          │  Never blocks BEAM schedulers                       │
          │  Unwinding panic: caught, error sent to caller      │
          │  Abort, OOM, stack overflow: the whole VM stops     │
          │  nera GC does not interact with BEAM GC            │
          └─────────────────────────────────────────────────────┘
```

The key properties:

- **Non-blocking.** The NIF returns immediately. BEAM scheduler is free in
  microseconds, regardless of how long the XML parse takes.
- **Lower memory-safety risk.** Processing code is safe Rust, which rules out
  buffer overflows and use-after-free there. The NIF boundary, the embedded
  runtime and some dependencies still contain `unsafe` code, so BEAM heap
  corruption remains possible, though less likely than with C.
- **Panic handling, not crash isolation.** An unwinding panic in a work item
  is caught and reported as an error message to the calling BEAM process.
  The bridge shares the VM's OS process: an abort, allocation failure, stack
  overflow or `unsafe` defect still stops the whole VM. Work that must never
  stop the VM belongs in a port. See
  [NERA_NIF_DESIGN.md §5.3](NERA_NIF_DESIGN.md#53-panic-handling-and-its-limits).
- **Concurrent.** nera actors can spawn sub-actors for parallel work (e.g.,
  parsing multiple stanzas concurrently) using the same scheduler.
- **No BEAM GC interference.** nera actors have their own per-process heaps.
  GC pressure in the native layer never causes BEAM GC pauses.

### 3.4 Erlang-Side API

The ejabberd-side code uses standard async messaging patterns:

```erlang
-module(fast_xml_bridge).
-export([parse_async/3, parse_sync/2]).

%% Async — returns immediately, result delivered as a message
parse_async(CallerPid, Ref, XmlData) ->
    %% NIF submits work to nera scheduler and returns :ok
    nif_parse_async(CallerPid, Ref, XmlData).

%% Sync convenience wrapper with timeout
parse_sync(XmlData, Timeout) ->
    Ref = make_ref(),
    nif_parse_async(self(), Ref, XmlData),
    receive
        {xml_parsed, Ref, Result} -> {ok, Result};
        {xml_error, Ref, Reason}  -> {error, Reason}
    after Timeout ->
        {error, timeout}
    end.
```

Usage in ejabberd's c2s connection handler:

```erlang
handle_xml_stream(Data, State) ->
    case fast_xml_bridge:parse_sync(Data, 5000) of
        {ok, Elements} ->
            lists:foldl(fun process_stanza/2, State, Elements);
        {error, Reason} ->
            close_stream(State, Reason)
    end.
```

No changes to ejabberd's architecture. The bridge is a drop-in replacement for
the existing NIF call, wrapped in async messaging.

### 3.5 Rust-Side Implementation

The NIF bridge consists of two layers:

**Layer 1 — NIF entry point (thin glue):**

```rust
// fast_xml_bridge_nif.rs
use rustler::{Env, Binary, Pid, Term};
use nera_runtime::Scheduler;
use std::sync::OnceLock;

static SCHEDULER: OnceLock<Scheduler> = OnceLock::new();

#[rustler::nif]
fn nif_parse_async<'a>(
    env: Env<'a>,
    caller_pid: Pid,
    ref_term: Term<'a>,
    data: Binary<'a>,
) -> rustler::Atom {
    let scheduler = SCHEDULER.get().unwrap();
    let data_vec = data.as_slice().to_vec();
    let ref_copy = ref_term.deep_copy();

    // Submit to nera scheduler — returns immediately
    scheduler.spawn_work(move |_ctx| {
        match xml_parser::parse(&data_vec) {
            Ok(elements) => {
                // Convert result to BEAM terms and send back
                send_to_beam(&caller_pid, ("xml_parsed", &ref_copy, elements));
            }
            Err(e) => {
                send_to_beam(&caller_pid, ("xml_error", &ref_copy, e.to_string()));
            }
        }
    });

    rustler::atoms::ok()
}

#[rustler::init("fast_xml_bridge")]
fn load(env: Env, _info: Term) -> bool {
    // Start nera scheduler with one worker per core
    SCHEDULER.get_or_init(|| Scheduler::new_embedded());
    true
}
```

**Layer 2 — Actual parser (pure Rust, no NIF dependency):**

```rust
// xml_parser.rs — runs on nera worker threads
pub fn parse(data: &[u8]) -> Result<Vec<XmlElement>, ParseError> {
    // Pure Rust XML parser (e.g., quick-xml crate)
    // No BEAM dependency, no NIF API, no unsafe code
    // Can be tested independently with cargo test
}
```

### 3.6 Comparison with Current Approaches

| Aspect | C NIF (today) | Dirty NIF | Rustler | nera bridge |
|---|---|---|---|---|
| Memory-safety risk in NIF code | High (C) | High (C) | Lower (Rust) | **Lower (Rust)** |
| Blocks BEAM scheduler | Yes | No | Yes | **No** |
| VM can stop on a native bug | Yes | Yes | Yes, less often | **Yes, less often** |
| Own concurrency model | No | No | Manual | **Actor model** |
| Crash isolation | None | None | Partial (panics) | **Partial (panics)** |
| GC interference | Risk | Risk | Risk | **None** |
| Build system | autotools + gcc | autotools + gcc | cargo | **cargo** |
| Parallel sub-tasks | pthreads | pthreads | tokio/rayon | **spawn/1** |
| Incremental migration | N/A | Easy | Moderate | **Moderate** |

### 3.7 Migration Targets in ejabberd

Ordered by impact and feasibility:

| NIF            | Risk Level                     | CPU Intensity | Migration Value |
|----------------|--------------------------------|---------------|-----------------|
| **fast_xml**   | High (complex C, large inputs) | High          | **Highest**     |
| **fast_tls**   | High (OpenSSL complexity)      | Medium-High   | High            |
| **stringprep** | Medium                         | Low-Medium    | Medium          |
| **ezlib**      | Low (stable, small)            | Low           | Low             |

Each NIF can be migrated independently. The recommended order is fast_xml first
(highest risk, highest CPU usage), followed by fast_tls. stringprep and ezlib
can remain as C NIFs until the bridge is proven in production.

### 3.8 Required nera Features

This use case does **not** require the full nera compiler (parser, IR, codegen).
It only requires the runtime crate with a public embedding API:

| Feature                                                | Status                            | Effort |
|--------------------------------------------------------|-----------------------------------|--------|
| `nera-runtime` as embeddable Rust library              | Exists as crate, needs public API | Small  |
| `Scheduler::new_embedded()` — start without `main()`   | Not yet exposed                   | Small  |
| `scheduler.spawn_work(closure)` — submit external work | Not yet exposed                   | Small  |
| BEAM term ↔ nera term conversion layer                 | Not implemented                   | Medium |
| `enif_send` integration for results                    | Not implemented                   | Medium |
| Panic handling per actor (not crash isolation)         | Not implemented                   | Small  |

Total estimated scope: the runtime embedding API is a focused effort that does
not require advancing the compiler pipeline.

---

## 4. Use Case 2 — Edge XMPP/MQTT Proxy

### 4.1 The Problem

ejabberd clusters serve millions of connections but the BEAM VM has constraints
at the edge:

- **IoT gateways** (Raspberry Pi, OpenWrt routers, ARM SBCs) have 256 MB–1 GB
  RAM. A BEAM + OTP release is 50 MB+ before any application code.
- **Container cold starts** for BEAM releases take hundreds of milliseconds due
  to VM boot, code loading, and application startup.
- **Deployment complexity** — an Erlang release requires rebar3/mix packaging,
  ERTS bundling, and platform-specific builds.

Many edge deployment scenarios need only a thin proxy: accept client connections,
terminate TLS, handle XMPP/MQTT framing, and forward traffic to the ejabberd
cluster. This does not require full OTP.

### 4.2 Architecture

```
  IoT devices / mobile clients
        │
        ▼
┌─────────────────────────┐
│  nera-compiled proxy    │     Single static binary, 2-5 MB
│                          │     One actor per connection
│  ┌────────────────────┐ │
│  │ listener actor      │ │     Accept TCP/TLS connections
│  │   │                 │ │
│  │   ├─► conn actor 1  │ │     Parse XMPP stream
│  │   ├─► conn actor 2  │ │     Authenticate (SASL PLAIN/SCRAM)
│  │   ├─► conn actor 3  │ │     Forward stanzas to cluster
│  │   └─► ...           │ │
│  └────────────────────┘ │
│                          │
│  Rust libraries:         │
│  - rustls (TLS)          │
│  - quick-xml (XML)       │
│  - tokio (async I/O)     │
└───────────┬──────────────┘
            │
            │  S2S or component protocol
            ▼
  ┌──────────────────────┐
  │   ejabberd cluster    │
  │   (full BEAM/OTP)     │
  └──────────────────────┘
```

### 4.3 What the Proxy Handles

- TCP/TLS listener with connection acceptance
- XMPP stream initialization (XML stream header, feature negotiation)
- SASL authentication (PLAIN, SCRAM-SHA-1/256 — delegated to ejabberd)
- Stanza routing: forward `<message>`, `<presence>`, `<iq>` to cluster
- Connection keepalive (whitespace pings, XEP-0199 XMPP Ping)
- Stream compression (optional, via zlib)
- MQTT 3.1.1/5.0 framing (for IoT use cases)

### 4.4 What the Proxy Does Not Handle

- Routing logic (handled by ejabberd)
- Roster management, MUC, PubSub (handled by ejabberd)
- Persistent storage (handled by ejabberd)
- Clustering (the proxy is a single-node forwarder)

### 4.5 Example Erlang Source

```erlang
-module(xmpp_edge).
-export([start/0, accept_loop/1, handle_client/1]).

start() ->
    {ok, ListenSock} = tcp:listen(5222, [binary, {active, false}]),
    accept_loop(ListenSock).

accept_loop(ListenSock) ->
    {ok, ClientSock} = tcp:accept(ListenSock),
    spawn(fun() -> handle_client(ClientSock) end),
    accept_loop(ListenSock).

handle_client(Sock) ->
    {ok, TlsSock} = tls:handshake(Sock),
    {ok, Stream} = xmpp_stream:init(TlsSock),
    proxy_loop(Stream).

proxy_loop(Stream) ->
    case xmpp_stream:recv(Stream) of
        {stanza, Stanza} ->
            forward_to_cluster(Stanza),
            proxy_loop(Stream);
        {error, closed} ->
            ok
    end.
```

Compiled with nera:

```
$ nera xmpp_edge.erl -o xmpp_edge
$ file xmpp_edge
xmpp_edge: Mach-O 64-bit executable arm64
$ ls -lh xmpp_edge
-rwxr-xr-x  1 user  staff  3.2M  xmpp_edge
$ scp xmpp_edge iot-gateway:/usr/local/bin/
$ ssh iot-gateway "xmpp_edge &"
```

### 4.6 Deployment Advantages

| Aspect | ejabberd (full BEAM) | nera proxy |
|---|---|---|
| Binary size | 50-100 MB (release) | **2-5 MB** |
| RAM at idle | ~40 MB | **~2 MB** |
| Startup time | ~500 ms | **< 10 ms** |
| Dependencies | ERTS, OTP libs, OpenSSL, system packages | **None** |
| Cross-compile | Complex (rebar3 + cross toolchain) | **cargo + target triple** |
| Deployment | Release tarball + scripts | **scp one file** |
| Update | Hot code loading or rolling restart | **Replace binary, restart** |

### 4.7 Required nera Features

This use case requires more of the compiler pipeline than Use Case 1:

| Feature                        | Status                           | Effort  |
|--------------------------------|----------------------------------|---------|
| Binary/bitstring support       | ✅ Implemented (bit-level)        | —       |
| TCP/networking BIFs            | ✅ Implemented (gen_tcp + mio)    | —       |
| TLS integration (rustls)       | Not implemented                  | Medium  |
| receive statement codegen      | ✅ Implemented (selective)        | —       |
| spawn codegen                  | ✅ Implemented (spawn/1, spawn/3) | —       |
| Process links/monitors         | ✅ Implemented                    | —       |
| Exception handling (try/catch) | ✅ Implemented                    | —       |
| More BIFs (lists:*, binary:*)  | ✅ Substantially implemented      | Ongoing |

This is a longer-term goal. It becomes realistic after nera reaches Phase 3
(Language Completeness) on the existing roadmap.

---

## 5. Use Case 3 — Standalone MQTT Broker for IoT

### 5.1 Concept

ProcessOne has invested in MQTT support within ejabberd. A stripped-down MQTT
broker compiled to a single binary targets a distinct market segment: embedded
devices, factory floors, and edge gateways that need local pub/sub without a
full messaging server.

```
  Sensors / actuators
      │  MQTT
      ▼
┌──────────────────────┐
│  nera MQTT broker    │     Single binary, runs on ARM gateway
│                       │
│  Actor per connection │
│  Topic routing        │
│  QoS 0/1 support      │
│  Bridge to ejabberd   │     Optional upstream sync
│                       │
└──────────────────────┘
```

### 5.2 Value Proposition

- Local MQTT broker with sub-millisecond latency on the gateway itself
- No dependency on cloud connectivity for local device-to-device messaging
- Optional bridge to ejabberd cluster for cloud sync when connectivity is
  available
- Single binary deployment to constrained ARM/MIPS devices

### 5.3 Required nera Features

Same as Use Case 2 (edge proxy), with additional needs for:

- MQTT protocol parsing (implementable as Rust library called via FFI)
- Topic trie data structure (Rust library)
- Persistent QoS 1 message queue (could use sled or sqlite via Rust FFI)

---

## 6. Use Case 4 — Prototyping and Testing Tool

### 6.1 Concept

nera's instant startup enables a fast feedback loop for developing and testing
XMPP/MQTT protocol logic in isolation:

```
$ nera compile test_stanza_routing.erl && ./test_stanza_routing
Route: user@example.com -> room@conference.example.com
PASS: MUC routing correct

$ time ./test_stanza_routing
real    0m0.003s
```

Compare with the BEAM workflow:

```
$ rebar3 compile && erl -pa _build/default/lib/*/ebin -noshell \
    -eval 'test_stanza_routing:run(), halt().'
real    0m0.850s
```

### 6.2 Applications

- Unit-testing stanza processing logic outside the BEAM
- Rapid iteration on protocol parsers
- CI pipeline optimization (hundreds of small test binaries run in parallel)
- Teaching XMPP/MQTT protocol handling to new contributors

---

## 7. Strategic Summary

```
                        ┌──────────────────────┐
                        │    Full ejabberd      │
                        │    on BEAM/OTP        │
                        │                       │
                        │  Millions of users    │
                        │  Clustering, HA       │
                        │  Hot code loading     │
                        │  Full OTP stack       │
                        └───────────┬──────────┘
                                    │
                    ┌───────────────┼───────────────┐
                    │               │               │
                    ▼               ▼               ▼
          ┌─────────────┐ ┌──────────────┐ ┌──────────────┐
          │ Better NIFs  │ │ Edge proxy    │ │ MQTT broker   │
          │              │ │              │ │              │
          │ nera runtime│ │ nera-compiled│ │ nera-compiled│
          │ as library   │ │ XMPP proxy   │ │ standalone    │
          │ inside BEAM  │ │              │ │              │
          │              │ │ Single binary │ │ Single binary │
          │ Near-term    │ │ Medium-term   │ │ Medium-term   │
          └─────────────┘ └──────────────┘ └──────────────┘
```

### Priority and Feasibility

| Use Case            | Impact | Feasibility (today) | Requires                   |
|---------------------|--------|---------------------|----------------------------|
| 1. NIF bridge       | High   | **Near-term**       | Runtime embedding API only |
| 2. Edge XMPP proxy  | High   | Medium-term         | Phase 3 compiler features  |
| 3. MQTT broker      | Medium | Medium-term         | Phase 3 + MQTT library     |
| 4. Prototyping tool | Low    | **Near-term**       | Current features + polish  |

### The Positioning

nera does not compete with BEAM for ejabberd's core workload. Instead, it
extends ejabberd's reach:

- **Inward** — faster native code with lower memory-safety risk running
  inside the BEAM via the NIF bridge, replacing fragile C NIFs with Rust
  actors. They share the VM's OS process, so they are not crash-isolated.

- **Outward** — lightweight native binaries at the edge, bringing Erlang's
  actor model to devices and environments where the BEAM cannot go.

The same concurrency model that makes ejabberd scale to millions of users,
compiled to a static binary for the devices connecting to it.
