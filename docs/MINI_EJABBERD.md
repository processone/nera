# Mini-ejabberd — A Minimal XMPP Server on nera

**Historical server proposal.** The selected first XMPP demonstration is now the
[external component with a statically checked Erlang core](XMPP_COMPONENT_ROADMAP.md).
Use that roadmap's scope and validation gates before extending to a full server.
The implementation and size targets below remain design goals, not validated
component capabilities.

## 1. Goal

Build a minimal but functional XMPP (Jabber) server in Erlang, compiled with
nera to a standalone native binary. This is the spiritual successor to the
[Mini IRC Server](MINI_IRC_SERVER.md) milestone — a step up in complexity that
exercises more of nera's capabilities and demonstrates a real-world messaging
protocol.

The target is **ejabberd circa November 2002**: the architecture Alexey Shchepin
used *before* adopting OTP behaviors, when `ejabberd_router` and `ejabberd_sm`
were plain `spawn` + `register` + `receive` loops. Only `ejabberd_c2s` used
`gen_fsm` (and even that had a commented-out raw-process version). This
pre-OTP architecture maps directly to what nera supports today.

**Success criteria:** two XMPP clients (e.g., Gajim, Psi, Profanity) connect
to the nera-compiled server, authenticate, exchange messages, and see each
other's presence — all from a single static binary under 5 MB.

---

## 2. Why This Milestone

| What it proves | How |
|---|---|
| Multi-module native compilation | 8-10 Erlang modules compiled and linked together |
| Concurrent networking | One process per client connection + service processes |
| Complex message routing | Process-to-process routing via registered names |
| XML protocol handling | Native Rust XML parsing exposed as BIFs |
| Binary data processing | XMPP streams are binary/UTF-8 data |
| Real protocol compliance | Standard XMPP clients can connect unmodified |

It also validates the [three-tier stdlib architecture](STDLIB_DESIGN.md):
- **Tier 1** primitives: process registry, links/monitors
- **Tier 2** Rust extensions: XML parsing (quick-xml), SHA-1/crypto (ring)
- **Tier 3** Erlang libraries: stanza routing, presence logic, roster management

---

## 3. XMPP Protocol Subset

We implement the minimum subset of XMPP that real clients can use.

### 3.1 Stream Setup (RFC 6120 §4)

Client opens:
```xml
<stream:stream to='mini.example.com'
  xmlns='jabber:client'
  xmlns:stream='http://etherx.jabber.org/streams'>
```

Server responds:
```xml
<stream:stream from='mini.example.com' id='session-id'
  xmlns='jabber:client'
  xmlns:stream='http://etherx.jabber.org/streams'>
```

No `version='1.0'` — this gives us legacy mode with no feature negotiation,
which is simpler and matches the original ejabberd 0.1 behavior.

### 3.2 Authentication (XEP-0078 — Legacy Non-SASL Auth)

This is the simplest authentication mechanism and what ejabberd 0.1 used.

```xml
<!-- Client discovers auth fields -->
<iq type='get' id='auth1'>
  <query xmlns='jabber:iq:auth'><username>alice</username></query>
</iq>

<!-- Server offers plaintext + digest -->
<iq type='result' id='auth1'>
  <query xmlns='jabber:iq:auth'>
    <username/><password/><digest/><resource/>
  </query>
</iq>

<!-- Client authenticates (plaintext or SHA-1 digest) -->
<iq type='set' id='auth2'>
  <query xmlns='jabber:iq:auth'>
    <username>alice</username>
    <password>secret</password>
    <resource>home</resource>
  </query>
</iq>

<!-- Server confirms -->
<iq type='result' id='auth2'/>
```

Digest auth: `SHA1(StreamID ++ Password)` as hex string. This requires a
SHA-1 BIF (see §5.2).

**Phase 2 enhancement:** SASL PLAIN (RFC 6120 §6) for modern clients. Requires
stream features, Base64 BIF, and stream restart after auth.

### 3.3 Message Routing

```xml
<message from='alice@mini/home' to='bob@mini/work' type='chat'>
  <body>Hello Bob</body>
</message>
```

Routing rules:
1. Parse `to` JID → `{User, Domain, Resource}`
2. If domain is local:
   - Full JID (`user@domain/resource`): deliver to specific session
   - Bare JID (`user@domain`): deliver to highest-priority resource
3. If user is offline: return `<service-unavailable/>` error
4. Server stamps `from` attribute with the authenticated JID (prevents spoofing)

### 3.4 Presence (RFC 6121 §4-5)

After authentication, the client sends initial presence:
```xml
<presence/>
```

Server must:
1. Broadcast available presence to contacts with subscription `from` or `both`
2. Send presence probes to contacts with subscription `to` or `both`
3. Deliver any queued offline messages (Phase 2)

On disconnect, server broadcasts:
```xml
<presence from='alice@mini/home' type='unavailable'/>
```

### 3.5 Roster (Contact List)

```xml
<!-- Client requests roster -->
<iq type='get' id='r1'>
  <query xmlns='jabber:iq:roster'/>
</iq>

<!-- Server returns roster items -->
<iq type='result' id='r1'>
  <query xmlns='jabber:iq:roster'>
    <item jid='bob@mini' name='Bob' subscription='both'/>
  </query>
</iq>
```

Subscription management (subscribe/subscribed/unsubscribe/unsubscribed) is
essential for presence to work, as presence is only broadcast to subscribed
contacts.

### 3.6 What We Do NOT Implement

- TLS/STARTTLS (Phase 2 — requires rustls integration)
- SASL authentication (Phase 2 — legacy auth first)
- Server-to-server federation (single-server only)
- Multi-user chat (MUC/XEP-0045)
- Service discovery (XEP-0030)
- Offline message storage (Phase 2)
- Stream compression
- vCards, private storage, server stats

---

## 4. Architecture

### 4.1 Process Structure

Following early ejabberd's pre-OTP architecture: plain processes with
`register/2` for service discovery and `receive` loops for state management.

```
main process (starts everything, then exits)
  │
  ├── auth (registered)
  │     Owns: user → password map
  │     Protocol: {check_password, From, User, Pass}
  │                {check_digest, From, User, Digest, StreamId}
  │                {register_user, From, User, Pass}
  │     Reply:    From ! {auth_result, ok | error}
  │
  ├── router (registered)
  │     Owns: domain → handler_pid map
  │     Protocol: {route, FromJID, ToJID, Stanza}
  │     Routes to: local_handler (for bare domain)
  │                session_mgr (for user JIDs)
  │
  ├── session_mgr (registered)
  │     Owns: {User, Resource} → {Pid, Priority} map
  │     Protocol: {route, From, To, Stanza}
  │                {open_session, User, Resource, Pid}
  │                {close_session, User, Resource}
  │                {get_sessions, From, User}
  │
  ├── local_handler (registered)
  │     Dispatches IQ stanzas by namespace
  │     Owns: namespace → handler function map
  │
  ├── roster_mgr (registered)
  │     Owns: user → [roster_item] map
  │     Protocol: {get_roster, From, User}
  │                {set_item, From, User, Item}
  │                {get_subscriptions, From, User}
  │
  └── listener (accept loop on port 5222)
        │
        ├── c2s_conn (per client — not registered)
        │     State machine: wait_for_stream → wait_for_auth → session
        │     Owns: socket, XML parser handle, JID, presence sets
        │     Receives: {route, From, To, Stanza} from router
        │               {xml_event, ...} from itself (after parsing recv data)
        │
        ├── c2s_conn ...
        └── c2s_conn ...
```

### 4.2 Module Breakdown

```
mini_ejabberd.erl        — Entry point: starts services, starts listener
mini_listener.erl        — TCP accept loop, spawns c2s connections
mini_c2s.erl             — Per-client connection handler (state machine)
mini_router.erl          — Stanza routing by domain
mini_session.erl         — Session manager ({User,Resource} → Pid)
mini_auth.erl            — Authentication (plaintext + digest)
mini_roster.erl          — Roster storage and subscription management
mini_local.erl           — Local domain IQ dispatch
mini_jlib.erl            — JID parsing, IQ helpers, XML construction
mini_xml.erl             — XML utility functions (element_to_string, etc.)
```

10 modules, roughly 1500-2500 lines total.

### 4.3 Data Representations

**JID (Jabber ID):**
```erlang
%% {User, Domain, Resource} — all binaries or atoms
{<<"alice">>, <<"mini.example.com">>, <<"home">>}
```

For simplicity in Phase 1, we use atoms for user/domain/resource since nera's
atom handling is mature. Binaries are Phase 2 for scalability.

**XML Element (matching ejabberd convention):**
```erlang
{xmlel, Name, Attrs, Children}

%% Example:
{xmlel, message, [{to, "bob@mini"}, {type, chat}],
  [{xmlel, body, [], [{xmlcdata, "Hello"}]}]}
```

Where:
- `Name` — atom (tag name)
- `Attrs` — list of `{atom, string}` tuples
- `Children` — list of `{xmlel, ...}` or `{xmlcdata, String}` tuples

**Roster Item:**
```erlang
{roster_item, JID, Name, Subscription, Groups}
%% Subscription: none | to | from | both
```

**Session:**
```erlang
{session, User, Resource, Pid, Priority}
```

### 4.4 Connection Lifecycle

```
Client connects (TCP)
  │
  ▼
wait_for_stream
  │  Client sends: <stream:stream ...>
  │  Server sends: <stream:stream id='SID' ...>
  ▼
wait_for_auth
  │  Client sends: <iq type='get'><query xmlns='jabber:iq:auth'>...
  │  Server sends: <iq type='result'>... (available auth fields)
  │  Client sends: <iq type='set'><query xmlns='jabber:iq:auth'>...
  │  Server verifies credentials via auth process
  │  Server sends: <iq type='result'/> (success)
  │  Server registers session via session_mgr
  ▼
session_established
  │  Client sends: <presence/> (initial presence)
  │  Server loads roster subscriptions
  │  Server broadcasts presence to subscribers
  │  Server probes contacts' presence
  │
  │  ── steady state ──
  │  Client sends messages → routed via router → session_mgr → target c2s
  │  Client receives messages via {route, From, To, Stanza} in mailbox
  │  Presence updates broadcast to subscribers
  │
  ▼
disconnect
  │  Server broadcasts unavailable presence
  │  Server removes session from session_mgr
  │  Socket closed
```

### 4.5 C2S State Machine (mini_c2s.erl)

```erlang
-module(mini_c2s).
-export([start/1]).

start(Socket) ->
    StreamId = mini_jlib:new_stream_id(),
    wait_for_stream(Socket, StreamId).

wait_for_stream(Socket, StreamId) ->
    case gen_tcp:recv(Socket, 0) of
        {ok, Data} ->
            case mini_xml:parse_stream_open(Data) of
                {ok, Domain} ->
                    Header = mini_jlib:stream_header(Domain, StreamId),
                    gen_tcp:send(Socket, Header),
                    wait_for_auth(Socket, StreamId, Domain);
                error ->
                    gen_tcp:send(Socket, <<"<stream:error/>">>),
                    gen_tcp:close(Socket)
            end;
        {error, closed} ->
            ok
    end.

wait_for_auth(Socket, StreamId, Domain) ->
    case recv_stanza(Socket) of
        {iq, get, Id, {auth_query, User}} ->
            %% Advertise auth fields
            Reply = mini_jlib:auth_fields_result(Id),
            gen_tcp:send(Socket, mini_xml:to_string(Reply)),
            wait_for_auth(Socket, StreamId, Domain);
        {iq, set, Id, {auth, User, Pass, Resource}} ->
            %% Check credentials
            case mini_auth:check(User, Pass) of
                ok ->
                    JID = {User, Domain, Resource},
                    gen_tcp:send(Socket, mini_jlib:iq_result(Id)),
                    mini_session:open(User, Resource, self()),
                    session_established(Socket, JID);
                error ->
                    gen_tcp:send(Socket, mini_jlib:auth_error(Id)),
                    wait_for_auth(Socket, StreamId, Domain)
            end;
        closed ->
            ok
    end.

session_established(Socket, JID) ->
    receive
        {route, From, To, Stanza} ->
            %% Incoming stanza from another user
            Xml = mini_xml:to_string(Stanza),
            gen_tcp:send(Socket, Xml),
            session_established(Socket, JID);
        {tcp_data, Data} ->
            %% Outgoing stanza from this client
            handle_client_stanza(Socket, JID, Data),
            session_established(Socket, JID);
        {tcp_closed} ->
            {User, Domain, Resource} = JID,
            mini_session:close(User, Resource),
            mini_roster:broadcast_unavailable(JID)
    end.
```

**Note:** The actual implementation will need a socket reader process (or
non-blocking recv with `after 0` polling) to multiplex between incoming TCP
data and routed messages. Early ejabberd solved this with a separate receiver
process that forwarded parsed XML events to the c2s process.

---

## 5. XML Parsing via quick-xml

### 5.1 Design

Instead of ejabberd's expat C port driver, we embed the `quick-xml` Rust crate
directly in the nera runtime as Tier 2 BIFs. This eliminates the port protocol
overhead and leverages Rust's safety guarantees.

**Two parsing modes:**

**Mode 1 — DOM parse (complete stanzas):**
```erlang
xml:parse(<<"<message to='bob'><body>Hi</body></message>">>) →
  {xmlel, message, [{to, "bob"}],
    [{xmlel, body, [], [{xmlcdata, "Hi"}]}]}
```

Used for parsing individual stanzas after the stream parser has extracted them.

**Mode 2 — Stream parse (incremental, for XMPP streams):**
```erlang
Parser = xml:new_stream_parser(),
{Events1, Parser2} = xml:feed(Parser, <<"<stream:stream to='mini'">>),
{Events2, Parser3} = xml:feed(Parser2, <<" xmlns='jabber:client'>">>),
%% Events: [{stream_start, [{to, "mini"}, {xmlns, "jabber:client"}]}]
{Events3, Parser4} = xml:feed(Parser3, <<"<message><body>Hi</body></message>">>),
%% Events: [{stanza, {xmlel, message, [], [{xmlel, body, ...}]}}]
xml:free_parser(Parser4).
```

The stream parser tracks XML depth: depth-0 open = stream start, depth-1
complete elements = stanzas, depth-0 close = stream end. This is exactly what
ejabberd's `xml_stream.erl` did over the expat port.

### 5.2 Runtime Implementation

**New BIF functions in `ffi.rs`:**

```rust
// --- DOM parsing ---

/// xml:parse(Binary) -> {xmlel, ...} | {error, Reason}
pub extern "C" fn nera_rt_xml_parse(
    ctx: *mut ProcessContext, binary: u64
) -> u64 {
    let bytes = extract_binary(ctx, binary);
    match quick_xml_parse_document(bytes) {
        Ok(element) => element_to_erlang_term(ctx, &element),
        Err(e) => make_error_tuple(ctx, e),
    }
}

// --- Streaming parser ---

/// xml:new_stream_parser() -> ParserRef
pub extern "C" fn nera_rt_xml_new_stream_parser(
    ctx: *mut ProcessContext
) -> u64 {
    // Allocate parser state in a global registry (like socket.rs)
    let parser = XmppStreamParser::new();
    let handle = PARSER_REGISTRY.lock().insert(parser);
    encode_integer(handle as i64)
}

/// xml:feed(ParserRef, Binary) -> {Events, ParserRef}
pub extern "C" fn nera_rt_xml_feed(
    ctx: *mut ProcessContext, parser_ref: u64, data: u64
) -> u64 {
    let handle = decode_integer(parser_ref);
    let bytes = extract_binary(ctx, data);
    let mut registry = PARSER_REGISTRY.lock();
    let parser = registry.get_mut(handle);
    let events = parser.feed(bytes);
    let events_list = events_to_erlang_list(ctx, &events);
    make_tuple2(ctx, events_list, parser_ref)
}

/// xml:free_parser(ParserRef) -> ok
pub extern "C" fn nera_rt_xml_free_parser(
    ctx: *mut ProcessContext, parser_ref: u64
) -> u64 {
    let handle = decode_integer(parser_ref);
    PARSER_REGISTRY.lock().remove(handle);
    ATOM_OK
}
```

**Parser registry** follows the same pattern as `socket.rs` — a global
`Mutex<HashMap<usize, XmppStreamParser>>` with integer handles.

**The `XmppStreamParser` struct:**

```rust
use quick_xml::Reader;

struct XmppStreamParser {
    reader: Reader<Vec<u8>>,  // Buffered reader over accumulated input
    depth: usize,             // Current XML nesting depth
    stanza_buf: Vec<u8>,      // Accumulates bytes for current depth-1 element
}

enum StreamEvent {
    StreamStart(Vec<(String, String)>),  // attrs
    Stanza(XmlElement),                   // complete depth-1 element
    StreamEnd,
    Error(String),
}

impl XmppStreamParser {
    fn feed(&mut self, data: &[u8]) -> Vec<StreamEvent> {
        self.reader.extend(data);
        let mut events = Vec::new();
        // Drive quick-xml reader, tracking depth
        // depth 0 open → StreamStart
        // depth 1 complete → parse stanza_buf into XmlElement → Stanza
        // depth 0 close → StreamEnd
        events
    }
}
```

### 5.3 XML Serialization

The reverse direction — Erlang term to XML string — can be implemented either
as a Rust BIF or as a pure Erlang function. Since serialization is simpler than
parsing and doesn't benefit as much from a Rust crate, we implement it in
Erlang (Tier 3):

```erlang
%% mini_xml.erl
to_string({xmlel, Name, Attrs, []}) ->
    "<" ++ atom_to_list(Name) ++ attrs_to_string(Attrs) ++ "/>";
to_string({xmlel, Name, Attrs, Children}) ->
    "<" ++ atom_to_list(Name) ++ attrs_to_string(Attrs) ++ ">" ++
    children_to_string(Children) ++
    "</" ++ atom_to_list(Name) ++ ">";
to_string({xmlcdata, Text}) ->
    xml_escape(Text).
```

For Phase 1, string (list) operations are sufficient. Phase 2 switches to
iolist/binary construction for performance.

### 5.4 Dependency

Add to `crates/nera-runtime/Cargo.toml`:
```toml
[dependencies]
quick-xml = "0.37"
```

quick-xml is pure Rust, no system dependencies, ~50KB added to binary size.

---

## 6. Additional Runtime BIFs Required

### 6.1 Process Registry (Tier 1 — Core Runtime)

The most critical missing feature. Service processes (router, session_mgr, etc.)
need to be discoverable by name.

```erlang
register(router, self()),         %% Associate atom with pid
RouterPid = whereis(router),      %% Look up pid by atom
unregister(router).               %% Remove association
```

**Implementation:** A `HashMap<AtomId, Pid>` in the scheduler, protected by a
`RwLock`. Three new FFI functions:

```rust
nera_rt_register(ctx, name_atom, pid) -> true | badarg
nera_rt_whereis(ctx, name_atom) -> pid | undefined
nera_rt_unregister(ctx, name_atom) -> true | badarg
```

Sending to a registered name (`router ! Msg`) requires extending
`nera_rt_send_message` to accept atoms as the target, resolving via
`whereis` internally.

### 6.2 Crypto BIFs (Tier 2 — Rust Extension)

For digest authentication (XEP-0078):

```erlang
crypto:sha(Data) -> <<Digest:20/binary>>
mini_jlib:hex(Binary) -> HexString
```

**Implementation:** Use the `ring` or `sha1` Rust crate. Single BIF:

```rust
/// crypto:sha(Binary) -> Binary
pub extern "C" fn nera_rt_crypto_sha(ctx, data) -> u64 {
    let bytes = extract_binary(ctx, data);
    let digest = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, bytes);
    alloc_binary(ctx, digest.as_ref())
}
```

### 6.3 String/List Utilities

The following are needed pervasively and should be implemented as Erlang modules
compiled alongside the server (Tier 3):

```erlang
%% Needed from the lists module:
lists:keyfind(Key, N, TupleList) -> Tuple | false
lists:keydelete(Key, N, TupleList) -> TupleList
lists:foreach(Fun, List) -> ok
lists:map(Fun, List) -> List
lists:foldl(Fun, Acc, List) -> Acc
lists:reverse(List) -> List
lists:member(Elem, List) -> bool
lists:append(L1, L2) -> List   %% (already have ++ operator)

%% Needed from string operations:
string:tokens(String, Separators) -> [Token]
string:to_lower(String) -> String
```

These can all be written in Erlang and compiled by nera. No Rust BIFs needed.

### 6.4 Random String Generation

For stream IDs and other identifiers:

```erlang
mini_jlib:new_stream_id() -> "session-" ++ random_hex(8).
```

**Option A:** Rust BIF using `rand` crate.
**Option B:** Erlang-based PRNG seeded from a Rust-provided entropy BIF:

```rust
/// erlang:unique_integer() -> Integer  (or os:timestamp)
pub extern "C" fn nera_rt_unique_integer(ctx) -> u64
```

### 6.5 Binary-to-List / List-to-Binary

Already partially supported. Needed for converting between socket binaries and
string-based XML processing:

```erlang
binary_to_list(Bin) -> String
list_to_binary(String) -> Bin
```

---

## 7. nera Feature Gap Analysis

### Currently Supported (ready to use)

| Feature | Status |
|---|---|
| Multi-module compilation | Shared atom table, single binary |
| Pattern matching (tuples, lists, atoms) | Full support |
| Spawn processes | `spawn/1` (closures), `spawn/3` (MFA) |
| Message passing (send/receive) | Including `receive...after` |
| TCP sockets | `gen_tcp:listen/accept/recv/send/close` |
| Try/catch/throw | Full exception handling |
| Records | Desugared to tuples at IR level |
| List comprehensions | Including nested generators |
| Closures / fun references | Local and remote |
| apply/3 | Dynamic dispatch |
| Binary construction/matching | Byte-aligned |
| io:format/1,2 | Basic formatting |
| Tail call optimization | True TCO via `return_call` |
| Back-edge yield checks | Cooperative scheduling |

### Must Implement (blocking)

| Feature | Effort | Notes |
|---|---|---|
| **Process registry** (`register/2`, `whereis/1`) | ✅ Done | HashMap in scheduler |
| **Send to registered name** (`name ! Msg`) | ✅ Done | Resolve name in send |
| **XML parsing BIFs** (quick-xml) | Medium | New Tier 2 module |
| **`lists` module** (Erlang) | ✅ Done | Implemented as stdlib |

### Should Implement (important for robustness)

| Feature | Effort | Notes |
|---|---|---|
| Process links | ✅ Done | Detect c2s crash, clean up session |
| `binary_to_list/1`, `list_to_binary/1` | ✅ Done | Socket ↔ string conversion |
| `erlang:system_time/1` | ✅ Done | Stream IDs, timing |
| SHA-1 BIF | Small | Digest authentication |
| Base64 encode/decode | Small | SASL PLAIN (Phase 2) |

### Nice to Have (Phase 2+)

| Feature | Effort | Notes |
|---|---|---|
| Process monitors | ✅ Done | Better crash detection than links |
| ETS | ✅ Done | Full implementation with mmap pools |
| TLS (rustls) | Medium | STARTTLS support |
| Preprocessor / `-define` | ✅ Done | Full preprocessor support |
| Timers (`erlang:send_after`) | ✅ Done | Timer wheel implementation |

---

## 8. Implementation Plan

### Phase 0 — Runtime Prerequisites (1-2 weeks)

Implement the blocking features in the nera runtime before writing any
Erlang code:

1. **Process registry** — `register/2`, `whereis/1`, `unregister/1`
   - Add `HashMap<AtomId, Pid>` to scheduler state
   - Register in `builtins.rs` + `emit_remote_call`
   - Extend `nera_rt_send_message` to resolve atom targets
   - Test: spawn + register + whereis + send-to-name

2. **XML parsing BIFs** — `xml:parse/1`, `xml:new_stream_parser/0`,
   `xml:feed/2`, `xml:free_parser/1`
   - Add `quick-xml` dependency to `nera-runtime`
   - Implement parser registry (like `socket.rs`)
   - DOM parser: binary → `{xmlel, Name, Attrs, Children}` term
   - Stream parser: incremental feed → list of events
   - Test: parse XMPP stanzas, verify term structure

3. **lists module** — Write `mini_lists.erl` with `keyfind`, `foreach`, `map`,
   `foldl`, `reverse`, `member`
   - Pure Erlang, compiled by nera alongside server modules
   - Test: unit tests for each function

### Phase 1 — Minimal Chat Server (2-3 weeks)

Authentication, message routing, two clients can chat.

1. **mini_ejabberd.erl** — Entry point
   - Start auth, router, session_mgr, local_handler processes
   - Register each with its name
   - Start TCP listener on port 5222

2. **mini_listener.erl** — Accept loop
   - `gen_tcp:listen(5222, ...)` + accept loop
   - Spawn `mini_c2s:start(Socket)` per connection

3. **mini_c2s.erl** — Connection handler
   - State machine: `wait_for_stream` → `wait_for_auth` → `session`
   - Socket reader sub-process that parses XML and forwards events
   - In `session` state: multiplex between TCP data and routed messages

4. **mini_auth.erl** — Password store
   - Registered process with `{User, Password}` map
   - Synchronous check via `From ! {auth_result, ...}` pattern
   - Hardcoded users for Phase 1 (config file later)

5. **mini_router.erl** — Stanza routing
   - Receives `{route, From, To, Stanza}`
   - Parses destination JID
   - Forwards to session_mgr for user JIDs

6. **mini_session.erl** — Session registry
   - Maps `{User, Resource}` → `Pid`
   - Handles open/close session
   - Routes messages to target connection process

7. **mini_jlib.erl** + **mini_xml.erl** — Utilities
   - JID parsing (`"alice@mini/home"` → `{alice, mini, home}`)
   - XML element construction helpers
   - XML serialization (`{xmlel, ...}` → string)
   - Stream header/footer generation

**Test:** Two clients connect, authenticate, exchange `<message>` stanzas.

### Phase 2 — Presence and Roster (2-3 weeks)

1. **mini_roster.erl** — Roster management process
   - Store roster items per user
   - Handle get/set/subscribe/subscribed/unsubscribe/unsubscribed
   - Push roster updates to connected resources

2. **Presence in mini_c2s.erl**
   - Track four presence sets: `pres_t`, `pres_f`, `pres_a`, `pres_i`
   - Initial presence broadcast to `pres_f` contacts
   - Presence probes to `pres_t` contacts
   - Unavailable broadcast on disconnect

3. **IQ dispatch in mini_local.erl**
   - Route `jabber:iq:auth` to auth handler
   - Route `jabber:iq:roster` to roster handler
   - Return `service-unavailable` for unknown namespaces

**Test:** Clients see each other's presence, manage contacts.

### Phase 3 — Hardening (ongoing)

1. Process links for crash cleanup (c2s dies → session removed)
2. Stream error handling (malformed XML, auth failures)
3. Proper XML namespace handling
4. SASL PLAIN authentication for modern clients
5. TLS support via rustls Tier 2 BIFs
6. Configuration file (domain name, port, user database)
7. Offline message storage
8. Connection keepalive (whitespace pings)

---

## 9. Testing Strategy

### Unit Tests (Rust)

- XML parser: verify quick-xml BIF produces correct Erlang term structure
- Process registry: register/whereis/unregister lifecycle
- Stream parser: incremental feeding of partial XML

### End-to-End Tests (Erlang, `tests/` directory)

Following the existing nera test convention (`test/0 → ok`):

```
tests/xmpp_xml_parse.erl       — XML BIF parsing
tests/xmpp_jid_parse.erl       — JID string parsing
tests/xmpp_stream_parser.erl   — Incremental stream parsing
tests/xmpp_process_registry.erl — register/whereis/send-to-name
```

### Integration Tests

- **Two-client test script:** Start server binary, connect two XMPP clients
  (via `profanity` or a simple Python `slixmpp` script), exchange messages,
  verify delivery.
- **Presence test:** Connect, send initial presence, verify both clients see
  each other as online.
- **Auth test:** Try wrong password → verify rejection. Try correct password →
  verify success.

### Compatibility Testing

Test with real XMPP clients:
- **Profanity** (TUI) — lightweight, supports legacy auth
- **Gajim** (GUI) — popular desktop client
- **Psi** (GUI) — supports legacy auth mode
- **slixmpp** (Python library) — scriptable for automated tests

---

## 10. Expected Metrics

| Metric | nera mini-ejabberd | ejabberd on BEAM |
|---|---|---|
| Binary size | ~3-5 MB | ~50-100 MB (release) |
| RSS at idle | ~3-5 MB | ~40 MB |
| Startup time | < 10 ms | ~500 ms |
| Per-connection overhead | ~400 B (PCB) + heap | ~2.7 KB (PCB) + heap |
| Max concurrent connections | Limited by OS fd limit | Same |
| Dependencies | None (static binary) | ERTS + OTP + OpenSSL |

---

## 11. Relationship to Other Design Documents

- **[STDLIB_DESIGN.md](STDLIB_DESIGN.md)** — The three-tier architecture. XML
  parsing is Tier 2 (Rust extension), process registry is Tier 1 (core
  runtime), roster/routing logic is Tier 3 (Erlang).

- **[EJABBERD_USE_CASE.md](EJABBERD_USE_CASE.md)** — Covers nera as a
  *complement* to ejabberd (NIF bridge, edge proxy). This document covers
  nera as a *standalone* XMPP server for the simplest use case.

- **[MINI_IRC_SERVER.md](MINI_IRC_SERVER.md)** — The predecessor milestone.
  Mini-ejabberd builds on the same foundation (TCP, spawn, message passing)
  but adds structured protocol handling (XML), authentication, and presence.

- **[RUNTIME_DESIGN.md](RUNTIME_DESIGN.md)** — Process registry and XML parser
  handle additions must follow the runtime's existing patterns (FFI functions,
  handle registries, BIF tables).
