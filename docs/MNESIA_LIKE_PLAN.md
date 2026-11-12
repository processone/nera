# Mnesia-Like Persistent Storage for nera

## Motivation

Mnesia is Erlang/OTP's built-in distributed database — the standard way to
store application state with ACID transactions, replication, and schema
management.  Porting the real Mnesia (30K+ lines of Erlang) would require
gen_server, supervisors, application behaviour, disk_log, DETS, and Erlang
distribution — essentially half of OTP.

Instead, nera can build a **Mnesia-compatible storage layer** in Rust,
directly behind the ETS infrastructure.  The design philosophy:

- **ETS is the engine** — all reads stay in-memory, zero disk I/O on the hot path
- **WAL + snapshots for durability** — append-only log, periodic checkpoint
- **Rust-native transactions** — read/write locks with rollback, no gen_server needed
- **API compatibility layer** — an Erlang `mnesia` module that maps to FFI calls
- **Distribution deferred** — single-node first, multi-node as a future phase

This gives nera applications `mnesia:transaction/1` semantics without
requiring the OTP application framework.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│  Erlang code                                            │
│  mnesia:transaction(fun() -> mnesia:write(R) end)       │
└────────────────────────┬────────────────────────────────┘
                         │ FFI calls
┌────────────────────────▼────────────────────────────────┐
│  API Compatibility Layer  (erlang/stdlib/mnesia.erl)    │
│  + Rust FFI (ffi/mnesia.rs)                             │
│                                                         │
│  mnesia:create_table/2  mnesia:write/1  mnesia:read/2   │
│  mnesia:delete/1        mnesia:transaction/1            │
│  mnesia:dirty_write/1   mnesia:dirty_read/2             │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│  Transaction Manager  (transaction.rs)                  │
│                                                         │
│  Per-process transaction context (write set, read set)  │
│  Table-level read/write locks with deadlock detection   │
│  Commit: apply write set → ETS + WAL                    │
│  Abort:  discard write set, release locks               │
└─────────┬──────────────────────────────┬────────────────┘
          │                              │
┌─────────▼──────────┐    ┌─────────────▼────────────────┐
│  ETS Engine        │    │  WAL + Snapshot Engine        │
│  (ets.rs)          │    │  (wal.rs, snapshot.rs)        │
│                    │    │                               │
│  In-memory         │    │  Append-only log file         │
│  HashMap + mmap    │    │  Periodic full checkpoint    │
│  pool (existing)   │    │  Recovery: snapshot + replay │
└────────────────────┘    └──────────────────────────────┘
```

---

## Phase 1: Term Serialization (External Term Format)

**Prerequisite for everything else.**

### What

Implement `term_to_binary/1` and `binary_to_term/1` — Erlang's external
term format (ETF, version tag 131).  This is the canonical wire format for
persisting and transmitting Erlang terms.

### Why

- WAL entries need a stable binary representation of terms
- Snapshot files need to serialize entire tables
- Future distribution protocol needs ETF
- `TermKey` in ets.rs already does a minimal serialization — ETF generalizes it

### Scope

Subset sufficient for nera's term types:

| Tag  | Type           | Format                              |
|------|----------------|-------------------------------------|
| 97   | SMALL_INTEGER  | 1 byte unsigned                     |
| 98   | INTEGER        | 4 bytes signed big-endian           |
| 70   | NEW_FLOAT      | 8 bytes IEEE 754                    |
| 100  | ATOM (legacy)  | 2-byte len + UTF-8 name             |
| 118  | ATOM_UTF8      | 2-byte len + UTF-8 name             |
| 104  | SMALL_TUPLE    | arity (1 byte) + elements           |
| 105  | LARGE_TUPLE    | arity (4 bytes) + elements          |
| 106  | NIL            | empty list                          |
| 107  | STRING         | 2-byte len + bytes (Latin-1 list)   |
| 108  | LIST           | 4-byte len + elements + tail        |
| 109  | BINARY         | 4-byte len + bytes                  |
| 116  | MAP            | 4-byte arity + key/value pairs      |
| 88   | PID (NEW_PID)  | node + id + serial + creation       |
| 90   | REF (NEWER_REF)| node + creation + id words          |
| 110  | SMALL_BIG      | n (1 byte) + sign + digits          |
| 111  | LARGE_BIG      | n (4 bytes) + sign + digits         |

### Files

- `crates/nera-runtime/src/etf.rs` — new module
- `crates/nera-runtime/src/ffi/misc.rs` — FFI entry points
- `crates/nera-ir/src/bif_table.rs` — register as BIFs

### Estimated effort: ~500 lines of Rust

---

## Phase 2: Write-Ahead Log (WAL)

### What

A sequential append-only log that records every mutation to persistent
tables.  On crash, the WAL is replayed to recover uncommitted state.

### Design

```
WAL file format (binary, little-endian):

┌──────────┬────────┬──────────┬────────────────────┬──────────┐
│ len: u32 │ op: u8 │ tab: u64 │ term_bytes: [u8]   │ crc: u32 │
└──────────┴────────┴──────────┴────────────────────┴──────────┘

Operations:
  0x01  INSERT    — term_bytes = ETF-encoded tuple
  0x02  DELETE    — term_bytes = ETF-encoded key
  0x03  CLEAR     — term_bytes empty (delete_all_objects)
  0x04  TX_BEGIN  — term_bytes = tx_id (u64)
  0x05  TX_COMMIT — term_bytes = tx_id (u64)
  0x06  TX_ABORT  — term_bytes = tx_id (u64)
```

### Write path

```
ets:insert(Tab, Record)   [persistent table]
  │
  ├── 1. Deep-copy into ETS (existing path)
  ├── 2. Serialize tuple → ETF bytes
  ├── 3. Append WAL entry (BufWriter, no fsync per op)
  └── 4. Return to caller

Durability guarantee:
  - "dirty" ops: fsync on buffer flush (configurable interval)
  - transaction commit: fsync on TX_COMMIT entry
```

### Read path

**No change.**  Reads always hit the in-memory ETS HashMap.
The WAL is write-only during normal operation.

### Configuration

```erlang
mnesia:create_table(users, [
    {type, set},
    {disc_copies, [node()]},     %% enables WAL + snapshot
    {attributes, [id, name, email]},
    {wal_dir, "/var/data/mnesia"},
    {sync_interval_ms, 100}      %% fsync interval for dirty ops
]).
```

Under the hood, this creates a regular ETS table plus a `WalWriter`
attached to the table metadata.

### Recovery

```
Startup:
  1. Find latest snapshot file (if any)
  2. Deserialize snapshot → populate ETS table
  3. Find WAL file(s) newer than snapshot
  4. Replay WAL entries sequentially:
     - INSERT → ets:insert
     - DELETE → ets:delete
     - TX_BEGIN/COMMIT/ABORT → skip aborted transactions
  5. Truncate replayed WAL
  6. Table ready
```

### Files

- `crates/nera-runtime/src/wal.rs` — WAL writer/reader
- `crates/nera-runtime/src/ets.rs` — `wal: Option<WalWriter>` on EtsTable

### Estimated effort: ~600-800 lines of Rust

---

## Phase 3: Snapshots

### What

Periodic full serialization of a table to a file.  Allows WAL truncation
and bounds recovery time.

### Design

```
Snapshot file format:

┌────────────────────┬───────────┬───────────────────────────┐
│ magic: [u8; 4]     │ version:  │ table_meta:               │
│ "NSNP"             │ u32       │ (name, type, keypos, ...) │
├────────────────────┴───────────┴───────────────────────────┤
│ entry_count: u64                                           │
├────────────────────────────────────────────────────────────┤
│ entries: [ETF-encoded tuple] × entry_count                 │
├────────────────────────────────────────────────────────────┤
│ checksum: u64 (xxhash of all entries)                      │
└────────────────────────────────────────────────────────────┘
```

### Trigger policy

- **Count-based**: every N WAL entries (default: 100,000)
- **Time-based**: every M seconds (default: 300)
- **Manual**: `mnesia:checkpoint/0` or table-specific

### Snapshot process

1. Acquire ETS `map.read()` lock (readers not blocked)
2. Iterate all entries, serialize via ETF
3. Write to temp file, fsync, rename to final path (atomic)
4. Truncate WAL entries older than snapshot LSN
5. Release read lock

Read lock duration is proportional to table size.  For very large tables
(millions of rows), a **copy-on-write snapshot** could be used: fork the
mmap region, serialize from the copy.  This is a future optimization.

### Files

- `crates/nera-runtime/src/snapshot.rs` — snapshot writer/reader
- `crates/nera-runtime/src/wal.rs` — WAL truncation after snapshot

### Estimated effort: ~400 lines of Rust

---

## Phase 4: Transaction Manager

### What

ACID transactions over one or more persistent tables.  The transaction
manager provides:

- **Atomicity**: all writes commit or none do
- **Consistency**: type/key constraints checked before commit
- **Isolation**: read-committed or serializable (configurable)
- **Durability**: WAL fsync on commit

### Design

```rust
// Per-process, per-transaction context
struct TxContext {
    tx_id: u64,
    write_set: Vec<TxOp>,        // buffered mutations
    read_set: Vec<(TableId, TermKey)>,  // for conflict detection
    locks_held: Vec<LockEntry>,
}

enum TxOp {
    Insert(TableId, Vec<u8>),    // ETF-encoded tuple
    Delete(TableId, Vec<u8>),    // ETF-encoded key
}

// Global lock manager
struct LockManager {
    // (TableId, TermKey) → (LockMode, Vec<ProcessId>)
    locks: RwLock<HashMap<(TableId, TermKey), LockState>>,
}

enum LockMode { Read, Write }
```

### Transaction flow

```
mnesia:transaction(Fun)
  │
  ├── 1. Allocate TxContext, write TX_BEGIN to WAL
  │
  ├── 2. Execute Fun:
  │      mnesia:write(Record) → acquire write lock → buffer in write_set
  │      mnesia:read(Tab, Key) → acquire read lock → read from ETS
  │      mnesia:delete({Tab, Key}) → acquire write lock → buffer delete
  │
  ├── 3. On success (Fun returns):
  │      ├── Apply write_set to ETS tables
  │      ├── Write all ops + TX_COMMIT to WAL, fsync
  │      └── Release all locks
  │      → return {atomic, Result}
  │
  └── 4. On failure (exception in Fun):
         ├── Discard write_set
         ├── Write TX_ABORT to WAL
         └── Release all locks
         → return {aborted, Reason}
```

### Deadlock detection

Simple timeout-based for Phase 4: if a lock cannot be acquired within
N milliseconds, abort and retry (up to max_retries).  BEAM Mnesia uses
a similar approach — transactions are retried on deadlock.

A future enhancement could add wait-for graph analysis, but timeout-based
detection is simpler and sufficient for most workloads.

### Dirty operations

```erlang
mnesia:dirty_write(Record)   %% No transaction, no lock, immediate ETS + WAL
mnesia:dirty_read(Tab, Key)  %% No lock, direct ETS lookup
mnesia:dirty_delete(Tab, Key)
```

These bypass the transaction manager entirely.  They write to the WAL
for durability but offer no atomicity across multiple operations.
This matches BEAM Mnesia's dirty semantics exactly.

### Files

- `crates/nera-runtime/src/transaction.rs` — TxContext, commit/abort
- `crates/nera-runtime/src/lock_manager.rs` — table/key-level locking
- `crates/nera-runtime/src/ffi/mnesia.rs` — FFI entry points

### Estimated effort: ~1000-1200 lines of Rust

---

## Phase 5: Erlang API Compatibility Layer

### What

An Erlang module (`mnesia.erl`) in stdlib that provides the standard
Mnesia API, delegating to Rust FFI for the heavy lifting.

### API surface (priority order)

#### Tier 1 — Core (Phase 5a)

```erlang
%% Schema
mnesia:create_table(Name, Opts) -> {atomic, ok} | {aborted, Reason}
mnesia:delete_table(Name)       -> {atomic, ok} | {aborted, Reason}
mnesia:table_info(Tab, Item)    -> Value

%% Transactions
mnesia:transaction(Fun)         -> {atomic, Result} | {aborted, Reason}
mnesia:transaction(Fun, Retries)

%% Read/Write (inside transactions)
mnesia:write(Record)            -> ok
mnesia:read(Tab, Key)           -> [Record] | []
mnesia:delete({Tab, Key})       -> ok
mnesia:match_object(Pattern)    -> [Record]

%% Dirty operations (outside transactions)
mnesia:dirty_write(Record)      -> ok
mnesia:dirty_read(Tab, Key)     -> [Record] | []
mnesia:dirty_delete({Tab, Key}) -> ok
```

#### Tier 2 — Table management (Phase 5b)

```erlang
mnesia:all_keys(Tab)            -> [Key]
mnesia:first(Tab)               -> Key | '$end_of_table'
mnesia:next(Tab, Key)           -> Key | '$end_of_table'
mnesia:foldl(Fun, Acc, Tab)     -> Acc
mnesia:select(Tab, MatchSpec)   -> [Result]
mnesia:index_read(Tab, Val, Pos)-> [Record]
mnesia:add_table_index(Tab, Pos)
mnesia:clear_table(Tab)         -> {atomic, ok}
mnesia:table_info(Tab, size)
mnesia:table_info(Tab, memory)
```

#### Tier 3 — System (Phase 5c)

```erlang
mnesia:start()                  -> ok
mnesia:stop()                   -> ok
mnesia:info()                   -> ok  %% prints to stdout
mnesia:system_info(Item)        -> Value
mnesia:create_schema(Nodes)     -> ok  %% single-node only initially
mnesia:wait_for_tables(Tabs, Timeout) -> ok | {timeout, BadTabs}
```

### Record convention

Mnesia tables are keyed by the first element of the record tuple
(the record name tag).  For example:

```erlang
-record(user, {id, name, email}).
mnesia:write(#user{id=1, name="Alice", email="a@b.com"}).
%% Stores: {user, 1, "Alice", "a@b.com"}
%% Key is element 2 (the id field), keypos=2
```

The compatibility layer maps this to ETS with `keypos=2` (element after
the record tag).  `mnesia:create_table(user, [{attributes, [id, name, email]}])`
sets up the mapping.

### Implementation split

```
mnesia.erl (Erlang)              mnesia FFI (Rust)
─────────────────────            ──────────────────
mnesia:transaction/1             nera_rt_mnesia_tx_begin
  wraps Fun in try/catch         nera_rt_mnesia_tx_commit
  handles retries                nera_rt_mnesia_tx_abort

mnesia:write/1                   nera_rt_mnesia_write
  extracts tab from record       (acquire lock + buffer)
  calls FFI

mnesia:read/2                    nera_rt_mnesia_read
  calls FFI                      (acquire lock + ETS lookup)
  returns result

mnesia:dirty_write/1             nera_rt_ets_insert + WAL append
  thin wrapper

mnesia:create_table/2            nera_rt_mnesia_create_table
  parses options                 (ETS + WAL + schema entry)
```

### Files

- `erlang/stdlib/mnesia.erl` — Erlang API module
- `crates/nera-runtime/src/ffi/mnesia.rs` — FFI functions
- `crates/nera-ir/src/bif_table.rs` — BIF registrations

### Estimated effort: ~800 lines Erlang + ~600 lines Rust

---

## Phase 6: Schema Persistence

### What

The set of table definitions (name, type, keypos, attributes, storage
mode) must itself survive restarts.  BEAM Mnesia stores schema in a
special `schema` table on disk.

### Design

A `schema.nera` file in the data directory, written on every
`create_table`/`delete_table`:

```
{
  "version": 1,
  "tables": {
    "user": {
      "type": "set",
      "keypos": 2,
      "attributes": ["id", "name", "email"],
      "storage": "disc_copies",
      "wal_dir": "/var/data/mnesia"
    }
  }
}
```

JSON is fine for schema (small, human-readable, easy to debug).
On startup, `mnesia:start()` reads the schema file and recreates all
tables with their WAL/snapshot recovery.

### Files

- `crates/nera-runtime/src/schema.rs` — schema read/write
- Dependency: `serde_json` (already used elsewhere, or minimal hand-rolled)

### Estimated effort: ~200 lines of Rust

---

## Phase 7: Secondary Indexes (Future)

### What

`mnesia:add_table_index(Tab, Pos)` creates a secondary HashMap
`{FieldValue → Vec<PrimaryKey>}` for fast lookups on non-key fields.

### Design

```rust
struct EtsTable {
    // ... existing fields ...
    indexes: RwLock<HashMap<usize, HashMap<TermKey, Vec<TermKey>>>>,
    //                       ^pos          ^field_val  ^primary_keys
}
```

Insert/delete maintain index entries.  `mnesia:index_read(Tab, Val, Pos)`
looks up the secondary index, then fetches full records from the primary.

### Estimated effort: ~400 lines of Rust

---

## Phase 8: Distribution (Long-term)

### What

Multi-node Mnesia with table replication.  This is the most complex phase
and depends on Erlang distribution support in nera.

### Prerequisites

- Erlang distribution protocol (epmd, handshake, DFLAG negotiation)
- `net_kernel`, `global` modules
- External term format over TCP (Phase 1 helps here)
- Node monitoring (`monitor_node/2`)

### Replication strategy (simplified vs BEAM)

BEAM Mnesia uses a complex multi-phase commit protocol with coordinator
election.  A simpler approach for nera v1:

1. **Primary-replica model**: each table has one primary node, N replicas
2. **Synchronous write**: primary writes locally, streams WAL entry to replicas
3. **Async read**: replicas serve reads from local ETS copy
4. **Failover**: if primary dies, a replica promotes (manual or automatic)

This is simpler than BEAM's symmetric replication but covers 80% of use
cases (session stores, caches, config tables).

### Estimated effort: Several thousand lines (depends on distribution support)

---

## Implementation Order & Dependencies

```
Phase 1: ETF serialization ◄── no dependencies
   │
   ▼
Phase 2: WAL ◄── depends on Phase 1
   │
   ▼
Phase 3: Snapshots ◄── depends on Phase 1, 2
   │
   ▼
Phase 4: Transaction manager ◄── depends on Phase 2
   │
   ▼
Phase 5: API compatibility ◄── depends on Phase 4
   │
   ▼
Phase 6: Schema persistence ◄── depends on Phase 5
   │
   ▼
Phase 7: Secondary indexes ◄── independent (can be done earlier)
   │
   ▼
Phase 8: Distribution ◄── depends on erlang dist protocol
```

### Minimum viable product

**Phases 1-5a** give a usable Mnesia-like system:
- Persistent tables that survive restarts
- ACID transactions with `mnesia:transaction/1`
- Dirty operations for high-throughput paths
- Standard API that existing Erlang code can target

Estimated total: **~3000-3500 lines of Rust + ~800 lines of Erlang**

---

## Key Design Decisions

### 1. No DETS

BEAM Mnesia uses DETS (Disk Erlang Term Storage) as a separate hash-based
file format for `disc_only_copies`.  DETS is complex (bucket management,
crash recovery, file format versioning) and limited (2GB file size cap).

nera replaces DETS with WAL + snapshots:
- `disc_copies` = ETS (RAM) + WAL + snapshots (same as BEAM)
- `disc_only_copies` = not supported initially (could map to a
  RocksDB-backed table in the future if needed)

This eliminates ~5000 lines of DETS complexity.

### 2. Rust-native vs pure Erlang transactions

BEAM Mnesia's transaction manager is pure Erlang (gen_server holding
lock state).  nera implements it in Rust because:

- No gen_server dependency
- Lock operations are fast Rust `RwLock`/`Mutex` (no message passing)
- Transaction context stored as a process-local pointer (no ETS lookup)
- WAL write is a direct Rust `BufWriter::write_all` (no port driver)

The trade-off is less flexibility — custom lock modes or conflict
resolution strategies would require Rust changes, not Erlang code.

### 3. ETS table options for persistence

Rather than a separate `mnesia:create_table` that creates a different
kind of table, persistent tables are ETS tables with extra metadata.
This means:

- `ets:lookup/2` works on persistent tables (zero overhead, no WAL read)
- `ets:insert/2` on persistent tables also appends to WAL
- `mnesia:write/1` is syntactic sugar for lock + buffer + ets:insert + WAL
- A persistent table can be used with both mnesia and raw ets APIs

### 4. fsync policy

- **Dirty ops**: buffered WAL writes, fsync every N ms (configurable)
- **Transaction commit**: immediate fsync (guaranteed durability)
- **Snapshots**: fsync + atomic rename

This matches PostgreSQL's approach: transactions get `fsync`, background
writes are batched.  The `sync_interval_ms` option lets applications
trade durability for throughput on dirty operations.

---

## Comparison with BEAM Mnesia

| Feature                  | BEAM Mnesia            | nera Mnesia-like          |
|--------------------------|------------------------|---------------------------|
| In-memory storage        | ETS                    | ETS (same)                |
| Disk storage             | DETS + disk_log        | WAL + snapshots           |
| Transactions             | Pure Erlang gen_server | Rust lock manager         |
| Distribution             | Built-in               | Future (Phase 8)          |
| disc_only_copies         | DETS                   | Not supported (initially) |
| Match specifications     | Full support           | Simplified (Phase 5b)     |
| Checkpoints              | Complex coordinator    | Simple snapshot           |
| Schema                   | Replicated table       | JSON file                 |
| Secondary indexes        | Per-table              | Per-table (Phase 7)       |
| Fragmented tables        | Yes                    | Not planned               |
| Table events/subscribers | mnesia:subscribe/1     | Future                    |
| Backup/restore           | mnesia:backup/1        | Snapshot files            |
| Max table size           | RAM-limited            | RAM-limited               |
| Recovery time            | Snapshot + log replay  | Snapshot + WAL replay     |
| Dependencies             | Half of OTP            | None (self-contained)     |

---

## API Modernization

BEAM Mnesia's API carries 30 years of baggage.  Since nera is building
from scratch, we can offer a cleaner interface alongside the compatibility
layer.  The Erlang module can expose both — legacy functions for porting
existing code, modern ones for new code.

### Pain points in BEAM Mnesia

1. **Record-based API is awkward**: `mnesia:write(#user{id=1, name="Alice"})`
   requires the record to carry its table name as element 1.  This couples
   data shape to storage identity.

2. **read returns a list**: `mnesia:read(user, 1)` returns `[#user{}]` or `[]`
   even for set tables where at most one record exists.  Forces pattern
   matching on every read.

3. **create_table options are verbose**: property lists with atoms like
   `{disc_copies, [node()]}` — easy to misspell, no compile-time checking.

4. **Match specs are unreadable**: `ets:fun2ms/1` parse transform helps,
   but the raw format (`[{{'$1','$2','_'}, [{'>', '$2', 100}], ['$1']}]`)
   is hostile.

5. **Error tuples are inconsistent**: `{atomic, ok}`, `{aborted, Reason}`,
   bare `ok`, exceptions — different functions use different conventions.

### Proposed modern API (alongside legacy)

```erlang
%% --- Table lifecycle ---

%% Legacy:
mnesia:create_table(user, [{attributes, [id, name, email]},
                           {disc_copies, [node()]}]).
%% Modern:
nera_db:create(user, #{
    fields => [id, name, email],
    storage => persistent,          %% ram | persistent
    key => id                       %% explicit key field name
}).

%% --- Reads ---

%% Legacy (returns list):
[User] = mnesia:read(user, 1).

%% Modern (returns value or error):
{ok, User} = nera_db:get(user, 1).
error       = nera_db:get(user, 999).

%% --- Writes ---

%% Legacy (record-tagged tuple):
mnesia:write(#user{id=1, name="Alice"}).

%% Modern (table explicit, map-friendly):
nera_db:put(user, #{id => 1, name => "Alice"}).
nera_db:put(user, {1, "Alice", "a@b.com"}).  %% tuple also accepted

%% --- Deletes ---

%% Legacy:
mnesia:delete({user, 1}).

%% Modern:
nera_db:delete(user, 1).

%% --- Transactions ---

%% Legacy:
{atomic, Result} = mnesia:transaction(fun() ->
    [U] = mnesia:read(user, 1),
    mnesia:write(U#user{name = "Bob"}),
    ok
end).

%% Modern (same semantics, cleaner return):
{ok, Result} = nera_db:transaction(fun() ->
    {ok, U} = nera_db:get(user, 1),
    nera_db:put(user, U#{name := "Bob"})
end).
%% On abort: {error, Reason}

%% --- Queries ---

%% Legacy (match spec):
mnesia:select(user, [{#user{name='$1', _='_'},
                       [{'==', '$1', "Alice"}],
                       ['$_']}]).

%% Modern (predicate function):
nera_db:filter(user, fun(#{name := N}) -> N == "Alice" end).

%% Modern (field match — most common case):
nera_db:match(user, #{name => "Alice"}).

%% --- Iteration ---

%% Legacy:
mnesia:foldl(fun(Rec, Acc) -> ... end, [], user).

%% Modern (same, but also):
nera_db:foreach(user, fun(Rec) -> io:format("~p~n", [Rec]) end).
nera_db:all(user).   %% returns full list (like tab2list)
nera_db:keys(user).  %% returns all keys
```

### Design principle

- **`mnesia` module**: API-compatible with BEAM for porting existing code
- **`nera_db` module**: modern API, maps-friendly, cleaner error handling
- **Same storage engine underneath**: both modules operate on the same tables
- A table created via `mnesia:create_table` is accessible via `nera_db:get`,
  and vice versa

This dual-module approach means users can migrate incrementally — port
with `mnesia`, then refactor to `nera_db` at their own pace.

---

## Open Questions

1. **Ordered set tables**: Mnesia's `ordered_set` uses a tree-based
   structure.  nera ETS currently only supports `set` (HashMap).  Should
   ordered_set be a BTreeMap-backed table type before or after Mnesia?

2. **Bag tables**: Mnesia supports `bag` (multiple records per key).
   This requires `HashMap<TermKey, Vec<StoredTuple>>`.  Priority?

3. **QLC integration**: BEAM Mnesia integrates with QLC (Query List
   Comprehensions) for SQL-like queries.  This is complex and may not
   be worth porting.  Simple `select/2` with match specs covers most
   use cases.

4. **Activity contexts**: BEAM Mnesia supports `mnesia:activity/2` with
   pluggable access modules (transaction, async_dirty, sync_dirty,
   ets).  Is this needed, or is `transaction/1` + `dirty_*/1` sufficient?

5. **RocksDB for disc_only_copies**: If a user needs tables larger than
   RAM, a RocksDB-backed table type could provide `disc_only_copies`
   semantics without implementing DETS.  Worth the dependency?
