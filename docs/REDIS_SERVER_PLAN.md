# Redis-like TCP Server: Gap Analysis & Implementation Plan

First minimal real-world production project for nera — a Redis/memcached-like
in-memory key-value store speaking the RESP (Redis Serialization Protocol).

## Why This Project

- **Single native binary** — no VM, no runtime install, instant startup.
- **Small footprint** — ideal for containers, edge, embedded.
- **Leverages Erlang strengths** — process-per-connection, binary protocol
  parsing, fault isolation.
- **Benchmarkable** — easy to compare latency/throughput vs BEAM equivalent.
- **Exercises most of the compiler** — TCP, processes, maps, binaries,
  pattern matching, message passing, closures.

---

## Current Capabilities (What Already Works)

| Capability                                | Notes                                     |
|-------------------------------------------|-------------------------------------------|
| TCP listen / accept / recv / send / close | Async via I/O pool                        |
| Process-per-connection                    | `spawn/1`, `spawn_link/1`                 |
| Binary pattern matching                   | Endianness, size specs, `/binary` rest    |
| Binary construction                       | `<<A, B:16/big, Rest/binary>>`            |
| Maps (KV backend)                         | 16 FFI + 13 Erlang stdlib functions       |
| Process registration                      | `register/2`, `whereis/1` for named store |
| Message passing                           | Synchronous call pattern possible         |
| try / catch / after                       | Full exception handling                   |
| monitor / link                            | Hand-rolled supervision                   |
| io:format/2                               | `~w`, `~s`, `~n` — debug logging          |
| Process dictionary                        | `put/2`, `get/1` for per-process state    |
| Receive with timeout                      | Timer wheel, `receive...after N ->`       |
| `++` list append                          | Building iolists                          |
| integer\_to\_list/1                       | Formatting numbers in protocol responses  |
| list\_to\_integer/1                       | Parsing numeric commands                  |
| byte\_size/1                              | Protocol length headers                   |

---

## Gaps — Organized by Priority

### Phase 1: Minimum Viable Server (unblocks a working prototype)

#### 1.1 `list_to_binary/1` + `binary_to_list/1` — ✅ IMPLEMENTED

Cannot convert between lists and binaries. Essential for building protocol
responses dynamically.

```erlang
%% Currently impossible:
Response = list_to_binary("+OK\r\n"),
gen_tcp:send(Socket, Response).
```

**Workaround**: Build everything with `<<...>>` syntax — very cumbersome for
dynamic strings.

**Fix**: Add two FFI functions in `ffi.rs` + bif\_table entries.
- `nera_rt_list_to_binary`: walk list, collect bytes, allocate binary on heap.
- `nera_rt_binary_to_list`: iterate binary bytes, build cons list.

**Effort**: Medium.

#### 1.2 `recv/2` ignores Length parameter + fixed 4096-byte buffer (Blocker)

Cannot read exact N bytes. Redis RESP needs exact-length reads
(`$5\r\nhello\r\n` — read exactly 5 bytes of payload).
Messages > 4096 bytes are silently truncated.

**Fix**: Respect Length param in `nera_rt_gen_tcp_recv`. Use dynamic buffer
(`Vec::with_capacity(length)` when length > 0, fallback to 4096 when 0).

**Effort**: Small.

#### 1.3 `{reuseaddr, true}` socket option — ✅ IMPLEMENTED

After server restart, `listen` fails with `eaddrinuse` for ~60 seconds
(TCP TIME\_WAIT). Blocks fast development iteration.

**Fix**: Parse options list in `nera_rt_gen_tcp_listen`, apply
`SO_REUSEADDR` via `setsockopt` before bind.

**Effort**: Tiny.

#### 1.4 Verify `<<"string">>` binary syntax

Confirm whether `<<"OK">>` (binary from string literal) is supported in
the parser and codegen. If not, it needs parser + codegen work to convert
string literals inside `<< >>` to binary bytes.

**Effort**: Small (verification) to Medium (if not supported).

---

### Phase 2: Usable RESP Protocol (parse Redis commands properly)

#### 2.1 `binary:split/2,3` — Not yet implemented

Cannot split binary on delimiter. Essential for line-delimited protocol parsing.

```erlang
%% Currently impossible:
[Command | Args] = binary:split(Line, <<" ">>, [global]).
```

**Workaround**: Manual byte-by-byte scanning via binary pattern matching.

**Fix options**:
- (a) Implement as FFI function in Rust (fastest).
- (b) Implement as Erlang in `erlang/stdlib/binary.erl` using pattern matching.

**Effort**: Medium.

#### 2.2 `erlang:system_time/0,1` — ✅ IMPLEMENTED

Cannot implement TTL/expiry (core Redis feature), cannot log timestamps.

```erlang
%% Currently impossible:
Expiry = erlang:system_time(millisecond) + TTL,
```

**Fix**: One FFI function calling `SystemTime::now()` or
`Instant::now()`. Return milliseconds as integer.

**Effort**: Small.

#### 2.3 `integer_to_binary/1` + `atom_to_binary/1` — ✅ IMPLEMENTED

Only `integer_to_list/1` exists. Protocol formatting requires
integer → binary without going through list → binary.

**Fix**: Thin FFI wrappers (format to string, allocate binary).

**Effort**: Small.

#### 2.4 `gen_tcp:connect/3` — ✅ IMPLEMENTED

Cannot write Erlang-side integration tests (client connections).
Cannot build replication or cluster features later.

**Fix**: Add `nera_rt_gen_tcp_connect` FFI function — resolve host,
connect socket, register in socket table, return handle.

**Effort**: Medium.

#### 2.5 `iolist_to_binary/1` — MISSING

Cannot flatten nested iolists to binary. Important for efficient response
building:

```erlang
%% Currently impossible:
Response = iolist_to_binary(["$", integer_to_list(Size), "\r\n", Data, "\r\n"]).
```

**Workaround**: `gen_tcp:send` already accepts iolists for sending directly.
This matters mainly for internal binary manipulation.

**Effort**: Medium.

---

### Phase 3: Production Polish

#### 3.1 `inet:peername/1` + `inet:sockname/1` — MISSING

Cannot log client IP addresses or local bind address.

**Fix**: Two FFI functions reading `getpeername`/`getsockname` from socket
handle, returning `{ok, {Address, Port}}`.

**Effort**: Small.

#### 3.2 `{packet, line}` socket option

Redis RESP is line-delimited (`\r\n`). Without `{packet, line}`, must
manually scan for `\r\n` in received data, handle partial reads, and
buffer across recv calls.

**Fix**: Parse `{packet, line}` option, apply at socket level (read until
`\n`, return complete line).

**Effort**: Medium.

#### 3.3 More `io:format` directives (`~p`, `~B`, `~.Nf`)

Debug logging is limited with only `~w`, `~s`, `~n`.

**Fix**: Extend format parser in `bif.rs` to handle `~p` (pretty-print with
indentation) and `~B` (integer with base).

**Effort**: Small–Medium.

#### 3.4 `gen_tcp:controlling_process/2`

Accept loop cannot hand off socket to handler process cleanly.

**Fix**: Add ownership metadata to socket registry, FFI function to
transfer.

**Effort**: Small.

#### 3.5 `{active, once}` socket mode — ✅ IMPLEMENTED

Active mode sockets are fully implemented via mio reactor.

Enables event-driven receive patterns instead of blocking `recv` loops.

**Fix**: Requires socket-to-process message delivery pipeline — socket
registry must track owning PID, I/O thread sends `{tcp, Socket, Data}`
messages.

**Effort**: Large.

#### 3.6 ETS tables — ✅ IMPLEMENTED

ETS is fully implemented with all core operations.

KV store state must currently live in a single process. No concurrent read
access. Limits throughput on multi-core.

**Workaround**: Single "store" process + message passing. Fine for demo.

**Fix**: Full ETS implementation — concurrent hash tables with process
ownership, match specs.

**Effort**: Large.

#### 3.7 Maps performance — ✅ IMPLEMENTED

Maps now use proper hash map semantics.

Maps use linear scan. KV store with >100 keys will be slow.

**Workaround**: Acceptable for demo / small datasets.

**Fix**: Replace internal representation with hash map or HAMT.

**Effort**: Large.

---

## Suggested Implementation Order

### Phase 1 — Minimum viable server

| # | Item                                      | Effort | Unblocks                    |
|---|-------------------------------------------|--------|-----------------------------|
| 1 | `list_to_binary/1` + `binary_to_list/1`   | Medium | Protocol response building  |
| 2 | `{reuseaddr, true}` socket option         | Tiny   | Fast dev iteration          |
| 3 | Fix `recv/2` Length param + buffer        | Small  | Correct protocol parsing    |
| 4 | Verify / fix `<<"string">>` binary syntax | Small  | Ergonomic response literals |

### Phase 2 — Usable RESP protocol

| # | Item                                | Effort | Unblocks                    |
|---|-------------------------------------|--------|-----------------------------|
| 5 | `binary:split/2` (or Erlang stdlib) | Medium | Line/token parsing          |
| 6 | `erlang:system_time/1`              | Small  | TTL, timestamps, logging    |
| 7 | `integer_to_binary/1`               | Small  | Protocol formatting         |
| 8 | `gen_tcp:connect/3`                 | Medium | Erlang-side test client     |
| 9 | `iolist_to_binary/1`                | Medium | Efficient response assembly |

### Phase 3 — Production polish

| # | Item | Effort | Unblocks |
|---|---|---|---|
| 10 | `inet:peername/1` | Small | Client IP logging |
| 11 | `{packet, line}` option | Medium | Simpler protocol parsing |
| 12 | More `io:format` specs | Small | Better debugging |
| 13 | `controlling_process/2` | Small | Clean socket handoff |

---

## What You Can Build Today (No Changes)

A working echo / PING server:

```erlang
-module(main).
-export([start/0]).

start() ->
    {ok, L} = gen_tcp:listen(6379, []),
    io:format("Listening on port 6379~n", []),
    accept_loop(L).

accept_loop(L) ->
    {ok, S} = gen_tcp:accept(L),
    spawn(fun() -> handle(S) end),
    accept_loop(L).

handle(S) ->
    case gen_tcp:recv(S, 0) of
        {ok, Data} ->
            gen_tcp:send(S, Data),
            handle(S);
        {error, closed} -> ok
    end.
```

## Target Architecture (After Phase 1+2)

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Client 1   │     │  Client 2   │     │  Client N   │
└──────┬──────┘     └──────┬──────┘     └──────┬──────┘
       │ TCP               │ TCP               │ TCP
       ▼                   ▼                   ▼
┌──────────────────────────────────────────────────────┐
│                   Accept Loop                        │
│              (single process, spawns)                │
└──────┬───────────────────┬───────────────────┬───────┘
       │ spawn             │ spawn             │ spawn
       ▼                   ▼                   ▼
┌──────────────┐   ┌──────────────┐   ┌──────────────┐
│  Handler 1   │   │  Handler 2   │   │  Handler N   │
│ (parse RESP, │   │ (parse RESP, │   │ (parse RESP, │
│  send cmds)  │   │  send cmds)  │   │  send cmds)  │
└──────┬───────┘   └──────┬───────┘   └──────┬───────┘
       │ {cmd, From, ...} │                   │
       └──────────┬───────┘───────────────────┘
                  ▼
       ┌────────────────────┐
       │    Store Process   │
       │  (registered name) │
       │                    │
       │  State = #{        │
       │    key => value,   │
       │    ...             │
       │  }                 │
       └────────────────────┘
```

- One accept loop spawns a handler per connection.
- Handlers parse RESP, send `{Command, From, Args}` to the store.
- Store process holds a single `maps` state, replies to callers.
- Single-writer avoids concurrency issues without ETS or locks.
- Handler processes isolate client failures from the store.

---

## Other Server Project Ideas

Alternative first projects for nera, in order of feasibility:

| Project                        | Why                                        | Risk                     |
|--------------------------------|--------------------------------------------|--------------------------|
| **TCP relay / proxy**          | Simple protocol, showcases concurrency     | Minimal                  |
| **Syslog collector**           | UDP/TCP listener → file, real ops use case | Needs UDP support        |
| **Health checker**             | Spawn processes per target, report status  | Needs HTTP client (hard) |
| **CLI tool** (e.g. log parser) | Shows fast startup vs `escript`            | Less "Erlang-like"       |
| **Chat server**                | Classic Erlang demo, registered processes  | Needs a client too       |
