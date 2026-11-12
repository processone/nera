# Mini IRC Server — First Achievement Milestone

## 1. Goal

Build a minimal IRC server in Erlang, compiled with nera to a standalone native
binary. This is the first "real-world" demonstration of nera: a concurrent,
networked service that handles multiple clients, routes messages between them,
and does so with a fraction of BEAM's memory footprint and startup time.

The server implements a subset of RFC 1459 — enough for standard IRC clients
(irssi, WeeChat, HexChat) to connect, join channels, and chat.

---

## 2. Why an IRC Server

IRC is the ideal first milestone because it exercises exactly the features nera
already has, while being a recognizable, testable, real networked application:

| Requirement             | nera Feature Used                   |
|-------------------------|--------------------------------------|
| Accept TCP connections  | `gen_tcp:listen/2`, `gen_tcp:accept/1` |
| One process per client  | `spawn/3`                            |
| Parse text commands     | Pattern matching, binary/list ops    |
| Route messages          | `!` (send operator)                  |
| Maintain channel state  | Recursive receive loops              |
| Handle disconnections   | `{error, closed}` from `gen_tcp:recv/2` |

It also naturally reveals what to prioritize next (registered processes, monitors,
string BIFs) without requiring any of them for a working first version.

---

## 3. Expected Advantages Over BEAM

### Memory Footprint

**Global overhead — where nera wins decisively:**

| Metric                      | BEAM/OTP         | nera            |
|-----------------------------|------------------|------------------|
| Binary size                 | ~50 MB (release) | ~2–5 MB          |
| RSS at idle (0 clients)     | 30–50 MB         | 3–5 MB           |
| RSS at 100 clients          | 50–70 MB         | 8–15 MB          |

BEAM loads the entire OTP application framework, code server, distribution
layer, ETS infrastructure, and standard library at startup. nera links only
the runtime functions actually called, producing a binary orders of magnitude
smaller. The dominant memory advantage is this **global baseline**, not
per-process savings.

**Per-process overhead — honest comparison:**

| Component            | BEAM               | nera              |
|----------------------|--------------------|--------------------|
| Process struct (PCB) | 338 words = 2,704 B | 328 B             |
| Initial heap         | 233 words (included) | 0 (lazy, then ~512 B chunk) |
| Initial stack        | 80 words (included)  | N/A (native stack) |
| Mailbox              | Included           | 0 (lazy, then ~64 B) |
| **At spawn**         | **~2,704 B**       | **~328 B**         |
| **After first use**  | **~2,704 B**       | **~904 B**         |

Bumpalo's `Bump::new()` allocates nothing — it starts with an empty sentinel.
The first allocation triggers a chunk of only ~512 bytes
(`FIRST_ALLOCATION_GOAL = 1 << 9`), not 4 KiB as one might assume. Chunks then
double on demand: ~512 B → ~1 KiB → ~2 KiB → ~4 KiB. This means a typical
I/O-bound process (IRC client handler doing recv/send with small messages) stays
in the first one or two chunks.

nera wins per-process at every stage: **8× smaller at spawn**, **~3× smaller
after first use**. Combined with the ~30–45 MB global baseline advantage, this
means nera uses less memory both in total and per connection.

For even more control, `ProcessHeap::new()` could use `Bump::with_capacity(n)`
to pre-allocate exactly the right initial size for a given workload, avoiding
the lazy-allocation overhead on the first `alloc_words` call. This is a
potential micro-optimization, not a priority for the milestone.

### Scaling Projections (with Async I/O)

With async I/O (Phase 4), the memory advantage translates directly into higher
connection ceilings on the same hardware:

| Concurrent connections | BEAM total RSS      | nera total RSS     | nera advantage |
|------------------------|---------------------|---------------------|-----------------|
| 100K                   | ~310 MB             | ~94 MB              | 3.3×            |
| 500K                   | ~1.39 GB            | ~454 MB             | 3.1×            |
| 1M                     | ~2.74 GB            | ~904 MB             | 3.0×            |

*(Calculated as: global baseline + N × per-process cost. BEAM: 40 MB + N × 2.7 KB.
nera: 4 MB + N × 0.9 KB.)*

On an 8 GB machine, nera could theoretically handle **~2.5M connections** before
exhausting memory, vs **~800K for BEAM** — with the OS file descriptor limit
(`ulimit -n`) being the real bottleneck long before either runtime runs out of
RAM.

### Speed

| Metric           | BEAM/OTP      | nera (expected)   |
|------------------|---------------|--------------------|
| Startup time     | ~200 ms       | < 1 ms             |
| Message latency  | ~1–5 μs       | ~0.5–2 μs (native) |
| Pattern matching | Interpreted   | Native machine code |

The initial version uses blocking I/O (one OS thread blocked per `accept`/`recv`
call), limiting concurrency to ~N cores. For a first milestone targeting 100–500
clients, this is acceptable.

With async I/O (Phase 4), the blocking limitation disappears. Both BEAM and nera
would share the same fundamental architecture: `kqueue`/`epoll` event loop +
lightweight processes that suspend on I/O. At that point nera should match or
exceed BEAM's connection ceiling, because:

- **Much lower global baseline** — no code server, no ETS, no distribution layer,
  no OTP application framework. At 10K connections, BEAM's ~30–45 MB baseline is
  a significant fraction of total RSS. nera starts at ~3–5 MB.
- **Native code** — scheduler threads run compiled code, not decode-dispatch.
  More CPU available for the event loop and message routing.
- **Less global lock contention** — fewer shared runtime structures means less
  contention between scheduler threads under load.

Per-process overhead also favors nera (~900 B vs ~2.7 KB after first use — see
section 3). Combined with the global baseline advantage, nera dedicates almost
all of its memory to actual connection state rather than runtime infrastructure.

The real limit at scale becomes the OS (file descriptor ceiling, kernel socket
buffers), not the runtime — same as BEAM. But nera would get there using
significantly less total memory thanks to the leaner baseline.

### Where BEAM Still Wins

- **Fault tolerance** — Process links, monitors, and supervisors are now
  implemented, but the OTP ecosystem maturity gap remains significant.
- **Hot code loading** — nera requires restart to update.
- **Standard library** — BEAM has decades of battle-tested stdlib. nera has
  `lists`, `maps`, `gen_server`, `supervisor`, `proc_lib`, `timer`, and
  `application`, but coverage is still narrower.
- **Maturity** — 30 years of production hardening, edge cases handled.

---

## 4. Supported IRC Commands

Minimal subset for a working chat server:

| Command                          | Description                    |
|----------------------------------|--------------------------------|
| `NICK <nickname>`               | Set or change nickname         |
| `USER <user> 0 * :<realname>`   | Complete registration          |
| `JOIN #<channel>`               | Join a channel                 |
| `PART #<channel>`               | Leave a channel                |
| `PRIVMSG #<channel> :<message>` | Send message to channel        |
| `PRIVMSG <nick> :<message>`     | Send private message to user   |
| `QUIT [:<message>]`             | Disconnect from server         |
| `PING <token>`                  | Keep-alive (reply with PONG)   |
| `PONG <token>`                  | Reply to server PING           |

The server also sends standard numeric replies for registration:

| Reply | Meaning                          |
|-------|----------------------------------|
| 001   | RPL_WELCOME — registration complete |
| 353   | RPL_NAMREPLY — channel member list  |
| 366   | RPL_ENDOFNAMES — end of names list  |
| 432   | ERR_ERRONEUSNICKNAME               |
| 433   | ERR_NICKNAMEINUSE                  |
| 461   | ERR_NEEDMOREPARAMS                 |

---

## 5. Architecture

### Process Model

```
                        ┌──────────────────┐
                        │   Acceptor Loop  │  One process
                        │ (listen + accept)│
                        └────────┬─────────┘
                                 │ spawn per connection
                    ┌────────────┼────────────┐
                    ▼            ▼             ▼
              ┌──────────┐ ┌──────────┐ ┌──────────┐
              │ Client 1 │ │ Client 2 │ │ Client N │  One process per client
              │ Handler  │ │ Handler  │ │ Handler  │
              └─────┬────┘ └────┬─────┘ └────┬─────┘
                    │           │             │
                    └───────────┼─────────────┘
                                │ messages (!, receive)
                         ┌──────▼──────┐
                         │   Router    │  One process
                         │  (central)  │  Holds all state:
                         │             │  - nick → pid map
                         │             │  - channel → [pid] map
                         └─────────────┘
```

**Three process types:**

1. **Acceptor** — Calls `gen_tcp:accept/1` in a loop, spawns a client handler
   for each new connection, and sends the router PID to the new handler.

2. **Client handler** — One per TCP connection. Reads lines from the socket,
   parses IRC commands, and forwards them to the router. Also receives messages
   from the router and writes them to the socket.

3. **Router** — Single central process that holds all server state:
   - Nickname → client PID mapping
   - Channel name → list of member PIDs
   - Processes JOIN/PART/NICK/PRIVMSG/QUIT by updating its state and
     forwarding messages to the appropriate client handlers.

### Why a Central Router

Although nera now supports `register/2` / `whereis/1`, the router PID is
passed explicitly in this design:
- The main `start/0` spawns the router, gets its PID.
- The acceptor receives the router PID as an argument.
- Each client handler receives the router PID at spawn time.

This is idiomatic Erlang — passing PIDs explicitly is cleaner than global
registration for a single-server design. Alternatively, `register(router, RouterPid)`
could be used for a cleaner architecture.

### Message Protocol (Internal)

Messages between client handlers and the router:

```erlang
%% Client → Router
{nick, ClientPid, Nickname}
{join, ClientPid, Channel}
{part, ClientPid, Channel}
{privmsg, ClientPid, Target, Message}
{quit, ClientPid, Reason}

%% Router → Client
{send, Line}              %% Raw IRC line to write to socket
{nick_ok, Nickname}       %% Nickname accepted
{nick_error, Reason}      %% Nickname rejected (in_use, invalid)
```

---

## 6. Module Structure

A single Erlang source file to keep it simple (multi-module linking is partial):

```
examples/mini_irc.erl       — The complete IRC server (~250–350 lines)
```

### Function Breakdown

```erlang
-module(mini_irc).
-export([start/0, acceptor/2, client_loop/3, router/2]).

%% --- Entry point ---
start/0
    → Starts router, opens listener on port 6667, enters accept loop.

%% --- Acceptor ---
acceptor/2  (ListenSock, RouterPid)
    → accept loop, spawns client_init for each connection.

client_init/2  (Sock, RouterPid)
    → Initial state before NICK/USER, enters client_loop.

%% --- Client handler ---
client_loop/3  (Sock, RouterPid, State)
    → Reads a line from socket, parses, dispatches to router.
    → Also receives messages from router via selective receive.
    → State = {Nick, User, Registered}

%% --- Line parsing ---
parse_line/1  (RawLine)
    → Splits on spaces, returns {Command, Params}.

%% --- Router ---
router/2  (Nicks, Channels)
    → Nicks = [{Nick, Pid}, ...]  (association list)
    → Channels = [{ChanName, [Pid, ...]}, ...]
    → receive loop handling all command messages.

%% --- Helpers ---
send_line/2  (Pid, Line)
    → Sends {send, Line} to a client handler.

broadcast/3  (Pids, Line, ExcludePid)
    → Sends Line to all Pids except ExcludePid.

lookup_nick/2  (Nick, Nicks)
    → Returns {ok, Pid} | error.

channel_members/2  (Channel, Channels)
    → Returns list of PIDs in channel.

add_to_channel/3  (Channel, Pid, Channels)
    → Returns updated Channels list.

remove_from_channel/3  (Channel, Pid, Channels)
    → Returns updated Channels list.

remove_from_all_channels/2  (Pid, Channels)
    → Removes Pid from every channel, returns updated Channels.
```

---

## 7. Implementation Plan

### Phase 1 — Runtime Prerequisites

Before writing the IRC server, add the small number of missing BIFs needed for
string manipulation. These are generally useful and not IRC-specific.

| BIF / Feature              | Purpose in IRC Server             | Effort |
|----------------------------|-----------------------------------|--------|
| `erlang:list_to_binary/1`  | Convert parsed list to binary     | Small  |
| `erlang:binary_to_list/1`  | Convert recv'd binary to list     | Small  |
| `erlang:integer_to_list/1` | Format numeric replies (001, 433) | Small  |
| `erlang:atom_to_list/1`    | Error formatting                  | Small  |
| `erlang:list_to_integer/1` | Parse port numbers                | Small  |
| Split binary on `\r\n`     | IRC line framing                  | Medium |

Two approaches for line splitting:
- **A) Implement in Erlang** — recursive function scanning the list for `\r\n`.
  Works today, no runtime changes needed.
- **B) Add a BIF** — `binary:split/2` or similar. Cleaner but requires runtime work.

Recommendation: **Use approach A** for the first version (pure Erlang), add BIFs
later as optimization.

### Phase 2 — Core IRC Server

Implement the server in `examples/mini_irc.erl`:

1. **Router process** — Association-list state, receive loop for all commands.
2. **Acceptor** — `gen_tcp:listen` on port 6667, spawn client handlers.
3. **Client handler** — Read loop with `gen_tcp:recv`, line parsing, dispatch.
4. **NICK/USER registration** — With duplicate-nick detection.
5. **JOIN/PART** — Channel membership management.
6. **PRIVMSG** — Channel broadcast and private messages.
7. **QUIT** — Cleanup client from all channels and nick table.
8. **PING/PONG** — Keep-alive support.

### Phase 3 — Testing & Benchmarking

1. **Manual testing** — Connect with `nc` (netcat) and verify protocol.
2. **IRC client testing** — Connect with irssi or WeeChat.
3. **Benchmark** — Compare memory and startup with equivalent BEAM version:
   - Write the same server in standard Erlang, run on BEAM.
   - Measure RSS, binary size, startup time, message latency.
   - Document results.

### Phase 4 — Async I/O

Replace the blocking `gen_tcp` implementation with an event-driven model. This
is the single most impactful runtime improvement — it removes the OS-thread-per-
connection bottleneck and makes nera competitive with BEAM at any scale.

**The Erlang code does not change.** `gen_tcp:recv` still looks blocking to the
programmer. The async machinery is entirely inside the Rust runtime.

#### Design

```
Scheduler threads (N)              I/O thread (1)
┌──────────────┐                  ┌──────────────────┐
│ Process calls│  io_request msg  │                  │
│ gen_tcp:recv ├─────────────────►│ mio/polling loop │
│              │                  │ (kqueue / epoll) │
│ Process is   │                  │                  │
│ SUSPENDED    │  io_ready msg    │ fd becomes ready │
│ (off run q)  │◄─────────────────┤ read data        │
│              │                  │                  │
│ Process      │                  └──────────────────┘
│ RESUMED with │
│ {ok, Data}   │
└──────────────┘
```

1. **I/O thread** — Single dedicated thread running `mio` (Rust crate wrapping
   `kqueue`/`epoll`). Receives registration requests, polls for readiness,
   performs the actual read/write, sends results back.

2. **Process suspend on I/O** — Reuses the existing receive-wait mechanism.
   When a process calls `gen_tcp:recv`, instead of blocking the OS thread,
   the runtime suspends the process (removes from run queue) and registers
   the socket fd with the I/O thread.

3. **Process resume on ready** — When data arrives, the I/O thread reads it
   and pushes the process back onto the scheduler's run queue with the result
   in its context. The process continues as if `recv` returned normally.

#### Why nera Can Match or Exceed BEAM at Scale

With async I/O, both runtimes share the same architecture: event loop +
lightweight processes + N scheduler threads. nera has advantages at every level:

- **~3× smaller per-process** — ~900 B after first use vs BEAM's ~2.7 KB (see
  section 3). Bumpalo's first chunk is only ~512 B, and I/O-bound processes
  often stay within the first one or two chunks.
- **~30–45 MB less global baseline** — No code server, ETS, distribution layer,
  or OTP application framework. This memory goes to connection state instead.
- **Native code** — No decode-dispatch overhead on scheduler threads. More CPU
  budget for the event loop and message routing.
- **Less global lock contention** — Fewer shared runtime data structures means
  scheduler threads spend less time waiting.

The ceiling becomes the OS (fd limits, kernel buffers), not the runtime.
nera reaches that ceiling using less memory at both the per-process and
global levels.

#### Dependencies

- `mio` crate (pure Rust, wraps platform poller)
- Channel between scheduler threads and I/O thread (`crossbeam-channel`,
  already a dependency)

### Phase 5 — Hardening

Additional features that improve the server after the core milestone:

| Feature                   | Benefit                                     | Status  |
|---------------------------|---------------------------------------------|---------|
| `register/2`, `whereis/1` | Named router process, cleaner architecture  | Done |
| Process monitors          | Detect client crashes, auto-cleanup         | Done |
| `try/catch`               | Graceful error handling on malformed input  | Done |
| `lists:member/2` etc.     | Cleaner list operations                     | Done |

---

## 8. Potential Issues & Mitigations

### Blocking I/O and Client Handler Receive

The client handler needs to both read from the TCP socket (`gen_tcp:recv`) and
receive messages from the router (Erlang `receive`). Since `gen_tcp:recv` blocks
the process, the handler cannot simultaneously wait for router messages.

**Mitigation:** Use `receive...after 0` to drain the router mailbox between
socket reads. Structure the loop as:

```erlang
client_loop(Sock, RouterPid, State) ->
    %% First, drain any pending messages from the router
    drain_mailbox(Sock),
    %% Then do a blocking recv (with implicit timeout from TCP)
    case gen_tcp:recv(Sock, 0) of
        {ok, Data} ->
            handle_data(Data, Sock, RouterPid, State);
        {error, closed} ->
            RouterPid ! {quit, self(), "Connection closed"},
            gen_tcp:close(Sock)
    end.

drain_mailbox(Sock) ->
    receive
        {send, Line} ->
            gen_tcp:send(Sock, Line),
            drain_mailbox(Sock)
    after 0 ->
        ok
    end.
```

This works because `after 0` is already supported. Messages queue in the mailbox
while `recv` blocks, and are flushed on the next iteration.

### Line Buffering

TCP doesn't guarantee message boundaries. A single `recv` may return a partial
line or multiple lines. The client handler must buffer partial data and split on
`\r\n`.

**Mitigation:** Maintain a buffer in the client handler state. After each recv,
append to buffer, split on `\r\n`, process complete lines, keep the remainder.

### Association List Performance

Using `[{Key, Value}, ...]` for nick lookup and channel membership is O(n).
For a mini server targeting < 500 users, this is perfectly fine. A map-based
version can be swapped in later (maps are already supported).

### No Error Handling

Without `try/catch`, a malformed IRC command that causes a pattern match failure
will crash the client handler process. This only affects the misbehaving client.

**Mitigation:** Defensive parsing — use a catch-all clause in every `case` that
handles the unknown/malformed case gracefully.

---

## 9. Example Session

```
$ nera examples/mini_irc.erl -o mini_irc
$ ./mini_irc
Mini IRC server listening on port 6667

# Terminal 2:
$ nc localhost 6667
NICK alice
USER alice 0 * :Alice
:mini_irc 001 alice :Welcome to the nera IRC server
JOIN #test
:alice!alice@localhost JOIN #test
:mini_irc 353 alice = #test :alice
:mini_irc 366 alice #test :End of /NAMES list
PRIVMSG #test :Hello from nera!

# Terminal 3:
$ nc localhost 6667
NICK bob
USER bob 0 * :Bob
:mini_irc 001 bob :Welcome to the nera IRC server
JOIN #test
:bob!bob@localhost JOIN #test
:mini_irc 353 bob = #test :alice bob
:mini_irc 366 bob #test :End of /NAMES list
# bob sees: :alice!alice@localhost PRIVMSG #test :Hello from nera!
```

---

## 10. Success Criteria

The milestone is achieved when:

- [ ] `nera examples/mini_irc.erl -o mini_irc` compiles without errors
- [ ] Server accepts multiple concurrent TCP connections
- [ ] NICK/USER registration works (with duplicate detection)
- [ ] JOIN/PART channel management works
- [ ] PRIVMSG routes messages to channel members
- [ ] PRIVMSG routes private messages to individual users
- [ ] QUIT cleans up client state
- [ ] PING/PONG keep-alive works
- [ ] At least one standard IRC client (irssi or WeeChat) can connect and chat
- [ ] Binary size < 10 MB
- [ ] RSS at idle < 10 MB
- [ ] Startup time < 10 ms
