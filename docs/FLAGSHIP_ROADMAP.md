# nera Flagship Roadmap: Compiling Erlang's Crown Jewels

**First demonstrator (2026-10-14):** the
[XMPP component roadmap](XMPP_COMPONENT_ROADMAP.md) defines the selected initial
scope and validation gates: a standalone external component with reused Erlang
code and an enforced static-checking boundary. The full-system targets and
timelines below remain broader proposals, not evidence of compatibility.

> *"We've run one of the world's largest Erlang systems for 20+ years. We love
> the language and the concurrency model. But we've hit the ceiling of what
> the BEAM deployment model allows. So we built a native compiler — and we're
> proving it with our own production systems."*

## The Convergence

Three things are converging that make this moment unique:

**1. Domain expertise.** ProcessOne built and maintains ejabberd — one of the
largest, longest-running Erlang applications in production. We know which OTP
features matter for real systems and which are ceremony. We've lived with the
BEAM's strengths and limitations for two decades.

**2. A working compiler.** nera already compiles Erlang to native code with
processes, message passing, closures, pattern matching, GC, and preemptive
scheduling. The hard part — the pipeline from source to native binary — is
done.

**3. The right targets.** The three crown jewels of Erlang infrastructure are
all messaging systems: ejabberd (XMPP), RabbitMQ (AMQP), and EMQX (MQTT).
They share the same core patterns. Build the foundation for one, and the path
to all three becomes clear.

---

## Why These Three Projects

| Project      | Protocol                | Scale                         | What it proves                 |
|--------------|-------------------------|-------------------------------|--------------------------------|
| **ejabberd** | XMPP, MQTT, SIP         | Millions of connections       | The original. Our own system.  |
| **RabbitMQ** | AMQP 0-9-1, MQTT, STOMP | Enterprise messaging standard | OTP discipline at its peak     |
| **EMQX**     | MQTT 3.1.1/5.0          | 100M+ connections claimed     | Modern Erlang at extreme scale |

All three are:
- Open-source, with code we can study and compile against
- Heavy users of the same OTP patterns (gen_server, supervisor, ETS)
- Binary protocol parsers (XMPP/XML, AMQP frames, MQTT packets)
- Process-per-connection architectures
- Deployed in environments that would benefit from native binaries

If nera can compile the core modules of these three, it proves that native
Erlang compilation works for real infrastructure — not just toy programs.

---

## Common OTP Patterns Across All Three

Before diving into per-project specifics, these are the shared building blocks.
Implementing them once unlocks progress across all three targets.

### gen_server (Critical — used everywhere)

All three projects use gen_server (or ProcessOne's `p1_server` variant) as
their fundamental building block:

- **ejabberd**: `ejabberd_router`, `ejabberd_sm`, `ejabberd_hooks`, every `mod_*`
- **RabbitMQ**: `rabbit_channel`, `rabbit_reader`, `rabbit_amqqueue_process`
- **EMQX**: `emqx_connection`, `emqx_session`, `emqx_broker`

**What gen_server needs from the compiler:**
- `receive` with selective pattern matching (match by reference)
- Process dictionary (used for `$callers`, `$ancestors`)
- `monitor/2` + `'DOWN'` message handling (for call timeouts)
- `sys` module stubs (debug hooks — can be no-ops initially)
- `proc_lib:init_ack/1` (parent notification on startup)

**Implementation strategy:** gen_server is Erlang code — it can compile on
nera as a stdlib module once selective receive and monitors work correctly.
No Rust FFI needed. Start with a minimal gen_server that supports
`start_link`, `call`, `cast`, and `handle_info`. Add `code_change` as a
no-op stub.

### supervisor (Critical — fault tolerance)

- **ejabberd**: `ejabberd_sup` (30+ children), `ejabberd_tmp_sup` (dynamic)
- **RabbitMQ**: `rabbit_sup`, per-vhost supervisors, queue supervisors
- **EMQX**: `emqx_sup`, listener supervisors, connection supervisors

**What supervisor needs:**
- Working gen_server (supervisor is built on gen_server)
- `spawn_link/1,3` with EXIT signal propagation (**already implemented**)
- `process_flag(trap_exit, true)` (**already implemented**)
- Timer support for restart intensity tracking
- Child spec parsing (tuples and maps)

**Implementation strategy:** Like gen_server, supervisor is Erlang code.
Implement `one_for_one` first (simplest, most common). Add
`simple_one_for_one` second (used for dynamic children like connections).
`one_for_all` and `rest_for_one` come later.

### ETS (Critical — shared concurrent state)

- **ejabberd**: Hook registry, session cache, config store, shaper rules
- **RabbitMQ**: Exchange/queue/binding resolution, routing tables, metadata
- **EMQX**: Topic routing trie, session registry, shared subscriptions

All three use ETS as a concurrent read-heavy cache with occasional writes.
The pattern is universal: `ets:new(name, [set, public, {read_concurrency, true}])`.

**Implementation strategy:** Rust `DashMap` for `set` tables, `BTreeMap` under
`RwLock` for `ordered_set`. Start with:
- `ets:new/2` (named tables, set/ordered_set, public/protected/private)
- `ets:insert/2`, `ets:lookup/2`, `ets:delete/2`
- `ets:match_object/2`, `ets:select/2` (needed for supervisor child lookup)
- `ets:tab2list/1`, `ets:foldl/3`
- `{read_concurrency, true}` uses `DashMap`'s sharded design naturally

### Binary protocol parsing (Critical — the hot path)

- **ejabberd**: XML stream parsing, JID parsing, stanza codec
- **RabbitMQ**: AMQP 0-9-1 frame parsing, content header decoding
- **EMQX**: MQTT packet parsing (variable-length encoding, topic matching)

All three depend on bit-level binary pattern matching:

```erlang
%% MQTT packet header
parse(<<Type:4, Dup:1, QoS:2, Retain:1, Rest/binary>>) -> ...

%% AMQP frame
parse(<<Type:8, Channel:16, Size:32, Payload:Size/binary, 16#CE>>) -> ...

%% XMPP (via fast_xml NIF, but also pure Erlang fallbacks)
parse_utf8(<<C/utf8, Rest/binary>>) -> ...
```

**Implementation strategy:** Bit-level binary matching is already on the
roadmap (MOONSHOT Phase 2.1). Prioritize:
1. Bit-level extraction (`<<X:4>>`)
2. Size-variable segments (`<<Payload:Size/binary>>`)
3. UTF-8/UTF-16/UTF-32 segment types
4. Binary comprehensions

### Networking (Critical — every connection is a process)

- **ejabberd**: `gen_tcp` accept loop, TLS via fast_tls NIF
- **RabbitMQ**: `gen_tcp` + `ssl` module, ranch for connection pooling
- **EMQX**: Custom acceptor pools, `gen_tcp` + `ssl`

**What exists:** `gen_tcp:listen/2`, `accept/1`, `recv/2`, `send/2`, `close/1`
are already implemented as blocking FFI calls.

**What's needed:**
- `gen_tcp:connect/3` (client-side connections)
- `gen_tcp` options: `{active, once}`, `{active, true}`, `{packet, N}`
- `inet:setopts/2`
- `ssl` module (TLS via rustls)
- Non-blocking I/O pool (blocking accept/recv currently stalls scheduler threads)

---

## Project-Specific Analysis

### ejabberd

ejabberd is our own project. We control the codebase, understand every module,
and can adapt code to compile on nera. This makes it the ideal first target.

**Architecture highlights:**
- `xmpp_stream_in` / `xmpp_stream_out`: Custom behaviors (gen_server-based FSMs)
  for XMPP connection lifecycle
- `ejabberd_hooks`: Central event dispatch via ETS-stored callback chains
- `gen_mod`: Plugin system with `start/2`, `stop/1`, `reload/3` callbacks
- `p1_server`: ProcessOne's gen_server with mailbox size limits
- `ets_cache`: Two-tier caching (ETS + DB fallback) used pervasively
- Mnesia for default storage, SQL/Redis as production backends
- C NIFs: fast_xml, fast_tls, stringprep, ezlib, fast_yaml

**ejabberd-specific needs beyond common patterns:**
- `p1_server` (gen_server variant with queue limits) — can adapt to use
  standard gen_server or port p1_server as stdlib module
- `yconf` YAML configuration parsing — replace with Rust YAML parser via FFI
- Hook system — pure Erlang, compiles once gen_server + ETS work
- `ets_cache` — pure Erlang over ETS, compiles once ETS works

**Adaptation strategy:** ejabberd's modular architecture is an advantage.
The core routing engine (`ejabberd_router`, `ejabberd_sm`, `ejabberd_hooks`,
`ejabberd_c2s`) can be extracted and adapted for nera without dragging in
the entire module ecosystem. ProcessOne controls the codebase and can create
an `ejabberd-core` subset.

### RabbitMQ

RabbitMQ represents OTP discipline at its finest — deep supervision trees,
gen_statem for connection state machines, sophisticated use of Mnesia for
metadata, and the Ra Raft library for quorum queues.

**Architecture highlights:**
- `rabbit_reader`: AMQP connection process (parses binary frames)
- `rabbit_channel`: Channel-level process (one per AMQP channel)
- `rabbit_amqqueue_process`: Queue process (classic queues)
- `ra`: Raft consensus library (quorum queues) — gen_statem-based
- `rabbit_exchange_type_*`: Exchange routing (direct, topic, fanout, headers)
- Mnesia for exchange/queue/binding metadata
- ETS for fast exchange resolution and routing
- Plugin system via OTP application behavior

**RabbitMQ-specific needs beyond common patterns:**
- `gen_statem` (or gen_fsm) — Ra library and connection handlers use state machines
- Mnesia transactions — metadata operations are transactional
- OTP application behavior — the plugin system relies on it
- `code:ensure_loaded/1`, `erlang:function_exported/3` — dynamic dispatch
- `digraph` module — dependency resolution
- `timer` module — `send_after`, `apply_after`
- Credit-based flow control — internal back-pressure mechanism

**Compilation target:** The AMQP frame parser and exchange routing engine are
the most interesting targets. They're CPU-intensive, binary-heavy, and
relatively self-contained. A native AMQP frame parser that's compatible with
RabbitMQ's wire format would be immediately useful.

### EMQX

EMQX pushes Erlang to its absolute scaling limits — 100M+ connections per
cluster. It represents modern Erlang engineering with sophisticated use of
ETS, custom clustering (not Erlang distribution), and extreme process count.

**Architecture highlights:**
- `emqx_connection`: One process per MQTT connection
- `emqx_session`: Session state management (persistent sessions, QoS)
- `emqx_broker`: Central pub/sub dispatch (ETS-based topic trie)
- `emqx_router`: Cluster-wide topic routing
- `emqx_channel`: MQTT protocol state machine
- Custom clustering via `ekka` (not Erlang distribution)
- Rule engine for message transformation/routing
- ETS-heavy design (topic trie, shared subscriptions, metrics)

**EMQX-specific needs beyond common patterns:**
- MQTT packet parsing — bit-level binary matching (packet types, QoS flags,
  variable-length encoding)
- Topic trie data structure — typically in ETS with custom matching
- Persistent sessions — QoS 1/2 message queuing
- Shared subscriptions — load-balanced consumer groups
- Metrics collection — atomic counters, ETS-based aggregation
- `ets:select/2` with match specifications — used for topic matching

**Compilation target:** The MQTT codec (`emqx_frame`) and topic matching
engine are ideal. MQTT's binary format is compact and well-specified,
making it a perfect showcase for nera's binary pattern matching.

---

## The Roadmap

### Milestone 0: Selective Receive (Foundation)

**Status: ✅ Done**
**Unlocks: gen_server, supervisor, and everything built on them — all now working**

Selective receive is fully implemented using a three-BIF scan protocol: `receive_peek` (reset scan, first candidate), `receive_next` (advance past non-matching), `receive_accept` (remove matched message). Real gen_server `call` depends on matching a specific
reference in the mailbox while ignoring other messages.

```erlang
%% gen_server:call/2 requires this to work:
call(Pid, Request) ->
    Ref = make_ref(),
    Pid ! {'$gen_call', {self(), Ref}, Request},
    receive
        {Ref, Reply} -> Reply   %% Must skip non-matching messages
    after 5000 ->
        exit(timeout)
    end.
```

**Implementation:**
- Mailbox scan: iterate all messages, try pattern match, skip non-matching
- Save queue: non-matching messages stay in mailbox for future receives
- Optimize: after a new message arrives, only try matching the new message
  (not the full rescan — BEAM's optimization)

**Effort:** Medium. Core scheduler change.
**Tests:** gen_server call/reply patterns, interleaved messages from
multiple senders.

### Milestone 1: gen_server & supervisor (OTP Core)

**Status: ✅ Done**
gen_server, supervisor, proc_lib, application, and timer are implemented as Erlang stdlib modules in `erlang/stdlib/`.
**Unlocks: The foundation for all three projects**

Implement gen_server and supervisor as Erlang stdlib modules compiled by
nera. Not Rust FFI — actual Erlang code, proving the compiler can handle
OTP patterns.

**gen_server (Phase 1a):**
- `gen_server:start_link/3,4` — spawn + register + init callback
- `gen_server:call/2,3` — synchronous request/reply with timeout
- `gen_server:cast/2` — asynchronous fire-and-forget
- `handle_call/3`, `handle_cast/2`, `handle_info/2` callbacks
- `terminate/2` callback on process exit
- `init/1` returning `{ok, State}` or `{stop, Reason}`
- `code_change/3` as no-op stub

**supervisor (Phase 1b):**
- `supervisor:start_link/2,3`
- Child specs as tuples and maps
- `one_for_one` restart strategy
- `simple_one_for_one` for dynamic children
- Restart intensity tracking (max restarts in time window)
- `supervisor:start_child/2`, `supervisor:terminate_child/2`

**Supporting stdlib:**
- `proc_lib:spawn_link/1,3` — proper OTP process init
- `proc_lib:init_ack/1,2` — parent notification
- `sys` module stubs — enough that `sys:get_state/1` doesn't crash
- `timer:send_after/2,3`, `timer:apply_after/4`

**Effort:** Large. ~2000 lines of Erlang stdlib + selective receive.
**Validation:** A supervision tree with workers that crash and restart.
gen_server call/cast round-trips under load.

### Milestone 2: ETS (Shared State)

**Status: ✅ Done**
ETS is fully implemented with HashMap-backed tables, mmap-backed region pools, and comprehensive API: new, insert, lookup, delete, delete_all_objects, update_counter, insert_new, tab2list, first/next, info/1,2, member, select_delete, update_element.
**Unlocks: Hook systems, routing tables, caches — the glue of all three**

**Phase 2a — Core ETS:**
- `ets:new/2` with `set`, `ordered_set`, `named_table`, access modes
- `ets:insert/2`, `ets:lookup/2`, `ets:delete/1,2`
- `ets:member/2`, `ets:info/1,2`
- `ets:tab2list/1`
- Owner process semantics (table dies with owner)
- `{heir, Pid, Data}` for table transfer on owner death

**Phase 2b — Query operations:**
- `ets:match/2`, `ets:match_object/2`
- `ets:select/2` with match specifications
- `ets:foldl/3`, `ets:foldr/3`
- `ets:first/1`, `ets:next/2` (for ordered_set iteration)
- `ets:update_counter/3` (atomic increment)
- `ets:insert_new/2` (conditional insert)

**Phase 2c — Performance:**
- `{read_concurrency, true}` — sharded reads (DashMap natural fit)
- `{write_concurrency, true}` — sharded writes
- `{decentralized_counters, true}` — for update_counter at scale

**Rust implementation:** Each ETS table is a Rust struct behind an Arc.
`set` tables use `DashMap<ErlTerm, Vec<ErlTerm>>`. `ordered_set` tables
use `BTreeMap` behind `RwLock`. Table registry maps names/IDs to table
handles.

**Effort:** Large. Core runtime feature with significant API surface.
**Validation:** ejabberd_hooks can store/retrieve hook chains. RabbitMQ
exchange routing via ETS lookup. EMQX topic trie operations.

### Milestone 3: Bit-Level Binaries (Protocol Parsing)

**Status: ✅ Done**
Bit-level binary pattern matching is fully implemented including sub-byte segments, variable-size segments, all endianness/sign specifiers, UTF-8/UTF-16/UTF-32, binary comprehensions, and match contexts for efficient iteration.
**Unlocks: AMQP, MQTT, and advanced XMPP parsing**

**Phase 3a — Bit-level extraction:**
- `<<X:4, Y:4>>` — sub-byte segments in patterns and construction
- `<<X:N>>` where N is a variable — size from runtime value
- `<<Payload:Size/binary>>` — size-delimited binary segments

**Phase 3b — Segment types:**
- `/integer`, `/float`, `/binary`, `/bits`
- `/big`, `/little`, `/native` endianness
- `/signed`, `/unsigned`
- `/utf8`, `/utf16`, `/utf32`

**Phase 3c — Binary comprehensions:**
- `<< <<X:8>> || <<X:4>> <= Bin >>`
- Generator from binary, body producing binary

**Effort:** Large. Touches parser, IR, and codegen.
**Validation:** Parse MQTT packets (4-bit type, QoS flags, variable-length
integers). Parse AMQP frames (type, channel, size-delimited payload).

### Milestone 4: Networking & I/O Hardening

**Status: ✅ Done**
gen_tcp is fully functional with `connect/3`, `{active, once}`, `{active, true}`, mio reactor for event-driven I/O, and 2-thread I/O pool for file operations. TLS via rustls remains future work.
**Unlocks: Real server applications**

**Phase 4a — gen_tcp completeness:**
- `gen_tcp:connect/3,4` — client connections
- `{active, once}` and `{active, true}` modes — socket data as messages
- `{active, N}` — flow-controlled active mode
- `{packet, 0|1|2|4|raw|line}` — framing
- `inet:setopts/2`, `inet:peername/1`, `inet:sockname/1`
- `gen_tcp:controlling_process/2`

**Phase 4b — Non-blocking I/O architecture:**
- Dedicated I/O thread pool (like BEAM's dirty I/O schedulers)
- `epoll`/`kqueue` event loop for socket readiness
- Socket data delivered as messages to owning process
- Accept loop doesn't block scheduler threads

**Phase 4c — TLS:**
- `ssl:connect/2,3`, `ssl:listen/2`, `ssl:handshake/1,2`
- Backed by rustls (Rust TLS library)
- Certificate loading from files
- ALPN negotiation (needed for HTTP/2, XMPP)

**Effort:** Large. Fundamental runtime architecture change for I/O.
**Validation:** Accept 10K concurrent TLS connections. Active-mode socket
receiving messages. Connection handler processes using gen_server.

### Milestone 5: Application Framework (Lightweight)

**Status: Not started**
**Unlocks: Module loading, plugin systems, proper startup/shutdown**

All three projects use OTP applications for lifecycle management. nera
doesn't need the full application controller, but it needs enough to
start a supervision tree from a configuration.

**What to implement:**
- `application:start/1,2` — start a module's supervision tree
- `application:stop/1` — orderly shutdown
- `application:get_env/2,3` — configuration lookup
- `application:set_env/3,4` — runtime configuration
- `-behaviour(application)` with `start/2`, `stop/1` callbacks

**What to skip:**
- Application resource files (`.app` / `.app.src`)
- Distributed applications
- Application dependencies / boot order
- Release handling

**Implementation:** A simple gen_server that tracks started applications
and their root supervisors. `get_env`/`set_env` backed by ETS or a
dedicated process.

**Effort:** Medium.
**Validation:** `application:start(my_app)` starts a supervisor tree.
Configuration available to child processes.

### Milestone 6: First Compilation Targets

With Milestones 0-5 complete, attempt to compile real modules from each
project. These are not full applications — they're self-contained modules
that exercise the compiler against production Erlang code.

**ejabberd targets:**
| Module | LOC | What it tests |
|--------|-----|---------------|
| `ejabberd_hooks` | ~300 | gen_server + ETS + callback dispatch |
| `ejabberd_router` | ~400 | gen_server + ETS + process registry |
| `ejabberd_config` | ~500 | ETS + YAML parsing + validation |
| `ejabberd_c2s` (subset) | ~800 | gen_server FSM + binary/XML + networking |

**RabbitMQ targets:**
| Module | LOC | What it tests |
|--------|-----|---------------|
| `rabbit_binary_parser` | ~200 | AMQP frame decoding, binary patterns |
| `rabbit_binary_generator` | ~200 | AMQP frame encoding |
| `rabbit_exchange_type_direct` | ~100 | ETS lookup, routing logic |
| `rabbit_queue_index` | ~400 | File I/O, binary serialization |

**EMQX targets:**
| Module | LOC | What it tests |
|--------|-----|---------------|
| `emqx_frame` | ~500 | MQTT packet codec, bit-level binaries |
| `emqx_topic` | ~200 | Topic parsing and matching |
| `emqx_message` | ~300 | Message record handling |
| `emqx_broker` | ~500 | gen_server + ETS pub/sub dispatch |

**Strategy:** Start with the codec/parser modules (no OTP dependencies).
Then attempt the gen_server-based modules. Track compilation failures as
a backlog of missing features.

### Milestone 7: gen_statem (State Machines)

**Status: Not started**
**Unlocks: RabbitMQ connection handling, Ra/Raft, EMQX channels**

RabbitMQ uses gen_statem extensively (via the Ra Raft library). EMQX uses
it for MQTT connection state machines. ejabberd uses p1_fsm (a gen_fsm
variant).

**What to implement:**
- `gen_statem:start_link/3,4`
- State function mode (`handle_event_function` and `state_functions`)
- `{next_state, NewState, NewData}` transitions
- `{keep_state, NewData}`, `{keep_state_and_data}`
- State enter calls
- Event types: `call`, `cast`, `info`, `timeout`, `state_timeout`
- Generic timeouts and event timeouts

**Implementation:** gen_statem is complex Erlang code (~2500 lines in OTP).
Rather than port the full module, implement the subset used by the target
projects. Ra uses `handle_event_function` mode; RabbitMQ readers use
`state_functions` mode.

**Effort:** Large.
**Validation:** A state machine that transitions through connection states
(connecting → handshaking → established → closing).

### Milestone 8: Extended Stdlib

Fill in the stdlib modules that the three projects depend on.

**High priority (used by all three):**
| Module | Key functions | Backend |
|--------|--------------|---------|
| `string` | `tokens/2`, `join/2`, `trim/1,2`, `lowercase/1` | Rust `unicode` crates |
| `binary` | `split/2,3`, `match/2,3`, `replace/3,4`, `part/2,3` | Rust FFI |
| `re` | `run/2,3`, `replace/3,4`, `split/2,3` | Rust `regex` crate |
| `timer` | `send_after/2,3`, `apply_after/4`, `cancel/1` | Erlang over existing timer wheel |
| `proplists` | `get_value/2,3`, `get_bool/2`, `delete/2` | Pure Erlang |
| `orddict` | `new/0`, `store/3`, `find/2`, `fold/3` | Pure Erlang |
| `dict` / `gb_trees` | Standard dictionary/tree operations | Pure Erlang |
| `queue` | `new/0`, `in/2`, `out/1`, `len/1` | Pure Erlang |
| `sets` / `gb_sets` | `new/0`, `add_element/2`, `is_element/2` | Pure Erlang |
| `unicode` | `characters_to_binary/1,2`, `characters_to_list/1` | Rust FFI |

**Medium priority (project-specific):**
| Module | Used by | Key functions |
|--------|---------|---------------|
| `crypto` | All three (TLS, auth) | `hash/2`, `mac/3,4`, `strong_rand_bytes/1` |
| `json` | EMQX, RabbitMQ (management) | `encode/1`, `decode/1` |
| `filelib` | All three (config loading) | `is_file/1`, `is_dir/1`, `wildcard/1` |
| `filename` | All three | `join/1,2`, `basename/1`, `dirname/1` |
| `io_lib` | All three (formatting) | `format/2`, `fwrite/2` |
| `erlang` (extras) | All three | `system_time/0,1`, `monotonic_time/0,1`, `unique_integer/0,1` |

**Effort:** Ongoing. Many of these are pure Erlang and compile once the
language features work. Rust-backed modules (re, crypto, unicode) need FFI
bindings.

### Milestone 9: Mnesia Subset or Alternative

**ejabberd** uses Mnesia as its default backend (ram_copies for sessions,
disc_copies for rosters). **RabbitMQ** uses Mnesia for exchange/queue/binding
metadata. **EMQX** uses Mnesia (via Mria/Rlog) for cluster routing state.

Full Mnesia reimplementation is not realistic. Two alternatives:

**Option A: ETS + persistence layer**
- `ram_copies` tables are just ETS tables — already covered by Milestone 2
- `disc_copies` = ETS table + write-ahead log to disk
- `dirty_read/write` = direct ETS operations (no transactions)
- `transaction` = per-table write locks + rollback on exception
- Skip: distributed Mnesia replication (use external sync)

**Option B: Mnesia API compatibility shim over SQLite**
- `mnesia:create_table/2` → SQLite table creation
- `mnesia:dirty_write/1` → SQLite INSERT/REPLACE
- `mnesia:dirty_read/2` → SQLite SELECT by key
- `mnesia:dirty_index_read/3` → SQLite indexed SELECT
- `mnesia:transaction/1` → SQLite transaction
- Backed by Rust `rusqlite` crate via FFI

**Recommended:** Option A for ram_copies (most common in all three projects),
Option B as optional persistence backend. This matches how ejabberd already
works in production — SQL backend with ETS caches.

**Effort:** Large for Option A, very large for Option B.

### Milestone 10: Core Erlang Frontend (The Shortcut)

A parallel track that provides a faster path to compiling complex OTP code:

```
.erl → erlc +to_core → .core → nera Core Erlang parser → IR → native
```

The standard Erlang compiler (`erlc`) handles all preprocessing, macros,
parse transforms, `-include_lib`, and OTP-specific compilation. The output
is Core Erlang — a small, stable intermediate language with ~15 node types.

**Why this matters for the flagship projects:**
- ejabberd uses `-include_lib("xmpp/include/xmpp.hrl")` — requires OTP
  include path resolution
- RabbitMQ uses parse transforms and complex macros
- EMQX uses extensive macro definitions and conditional compilation

Rather than reimplementing all of OTP's compilation infrastructure, let
`erlc` do what it does best and take over at the Core Erlang stage.

**Implementation:**
- Core Erlang text parser (~500 lines — the grammar is tiny)
- Core Erlang → nera IR lowering (~1000 lines)
- CLI: `nera file.core -o output` and `nera --from-core file.erl`

**Effort:** Medium. High leverage.
**Validation:** Compile an OTP stdlib module (e.g., `lists.erl`) through
`erlc +to_core` then through nera to native code.

---

## Timeline

```
         Now            +6 months          +12 months          +18 months
          │                 │                   │                   │
          ▼                 ▼                   ▼                   ▼

    ┌─────────────────────────────┐
    │ M0: Selective Receive       │
    └──────────────┬──────────────┘
                   │
    ┌──────────────▼──────────────┐
    │ M1: gen_server + supervisor │
    └──────────────┬──────────────┘
                   │
          ┌────────┴────────┐
          ▼                 ▼
    ┌───────────┐   ┌─────────────┐
    │ M2: ETS   │   │ M3: Bit-    │    (parallel tracks)
    │           │   │ level bins  │
    └─────┬─────┘   └──────┬──────┘
          │                │
          └───────┬────────┘
                  ▼
    ┌─────────────────────────────┐
    │ M6: First compilation       │    <-- Attempt real modules
    │     targets                 │
    └──────────────┬──────────────┘
                   │
          ┌────────┼────────┐
          ▼        ▼        ▼
    ┌─────────┐┌────────┐┌──────────┐
    │ M4: Net ││ M7:    ││ M8:      │   (parallel tracks)
    │ & I/O   ││ statem ││ Stdlib   │
    └────┬────┘└───┬────┘└────┬─────┘
         │         │          │
         └─────────┼──────────┘
                   ▼
    ┌─────────────────────────────┐
    │ M5: Application framework   │
    │ M9: Mnesia alternative      │
    └──────────────┬──────────────┘
                   ▼
    ┌─────────────────────────────┐
    │ Full module compilation     │    <-- ejabberd_c2s, rabbit_reader,
    │ from all three projects     │        emqx_connection
    └─────────────────────────────┘

    ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
    M10: Core Erlang frontend         (independent parallel track,
         can start anytime)            high leverage shortcut)
```

### Parallel track: Core Erlang frontend

Milestone 10 is independent and can proceed at any time. It provides an
alternative path: instead of extending nera's own parser to handle every
OTP idiom, use `erlc` as a frontend and compile Core Erlang to native code.
This is especially valuable for RabbitMQ and EMQX, whose codebases use
OTP features (parse transforms, complex macros) that nera's parser may
never fully replicate.

---

## Success Criteria

### 6-month checkpoint
- [x] Selective receive works correctly (gen_server call/reply pattern)
- [x] gen_server and supervisor compile as Erlang stdlib modules
- [x] ETS core operations (new, insert, lookup, delete, tab2list)
- [x] Bit-level binary matching (`<<X:4, Y:4>>`)
- [ ] First real module from each project compiles and passes tests:
  - ejabberd_hooks (or simplified version)
  - rabbit_binary_parser (AMQP frame decoding)
  - emqx_frame (MQTT packet codec)

### 12-month checkpoint
- [ ] Full gen_server/supervisor/gen_statem working
- [ ] ETS with match specs and concurrent access
- [ ] Non-blocking I/O with active socket mode
- [ ] TLS support via rustls
- [ ] 10+ modules from each project compile
- [ ] Core Erlang frontend can compile OTP stdlib modules
- [ ] A functioning mini-server for each protocol:
  - Minimal XMPP server accepting connections
  - AMQP frame parser compatible with RabbitMQ wire format
  - MQTT broker handling connect/subscribe/publish

### 18-month checkpoint
- [ ] ejabberd-core (router, session manager, hooks, c2s) compiles
- [ ] RabbitMQ core (reader, channel, exchange routing) compiles
- [ ] EMQX core (connection, session, broker, frame) compiles
- [ ] Lightweight application framework
- [ ] ETS-based Mnesia alternative for ram_copies tables
- [ ] Single-binary servers for each protocol pass basic conformance tests

---

## The Narrative

The story is not "we're replacing the BEAM." The story is:

**Phase 1 — Prove the compiler.** Compile individual modules from the three
largest Erlang infrastructure projects. Show that native compilation produces
correct results for real production code.

**Phase 2 — Unlock new deployments.** Single-binary protocol servers for
edge, IoT, serverless, and embedded — environments where the BEAM can't go.
An MQTT broker on a Raspberry Pi. An XMPP proxy as a 3 MB binary. An AMQP
frame parser running in WASM.

**Phase 3 — The ecosystem effect.** When the team behind ejabberd, compiling
modules from RabbitMQ and EMQX, says "native Erlang compilation works for
production infrastructure" — that changes the conversation for the entire
Erlang community. It's not a research project. It's practitioners solving
real problems.

ProcessOne is uniquely positioned to tell this story. Nobody else has 20+
years of production Erlang experience, an active flagship project, AND a
working native compiler. The convergence is real, and the time is now.

---

## Relationship to Existing Documents

This roadmap builds on and connects to:

- **[MOONSHOT.md](MOONSHOT.md)** — The broad vision. This document narrows
  the focus to three concrete flagship targets.
- **[EJABBERD_USE_CASE.md](EJABBERD_USE_CASE.md)** — The original ejabberd
  analysis. This document expands the scope to include RabbitMQ and EMQX,
  and shifts from "nera cannot replace BEAM" to "here's the path to
  compiling the core."
- **[ARCHITECTURE.md](ARCHITECTURE.md)** — Compiler internals. Milestones
  here map to specific changes in the pipeline.
- **[STDLIB_DESIGN.md](STDLIB_DESIGN.md)** — Stdlib tiers. Milestone 8
  extends the stdlib toward what the flagship projects need.
