# Phase 6 — Per-op Reactor Command Elimination: Implementation Plan

Date: 2026-04-14

## 1. Goal

Eliminate per-op reactor command round-trips on the steady-state
OneCached SET/GET path. Bring Nera's 1:1 p50 latency down from 2.527 ms
toward ~2.3 ms, pushing the Little's-law throughput ceiling at
200-connection parallelism from ~80k ops/sec to ~85-88k ops/sec.

This is the implementation plan for the Phase 6 workstream scoped in
`docs/ONECACHED_PERFORMANCE_PLAN.md`. It is self-contained and should
be reviewed before any code lands.

## 2. What the current code actually does

A careful re-read of the existing FFI hot path shows the fast path is
already more optimised than I thought when writing the 2026-04-14 plan
update. Specifically:

- **`gen_tcp:recv`** (`ffi/gen_tcp.rs:240`) calls `try_recv` first — a
  direct non-blocking syscall. Only on `WouldBlock` does it fall
  through to `submit_to_reactor_and_wait`. Data buffered in the kernel
  is delivered without touching the reactor.
- **`gen_tcp:send`** (`ffi/gen_tcp.rs:294`) calls `try_send` first.
  Only on partial write or EAGAIN does it submit to the reactor.
- **`inet:setopts(active, once)`** (`ffi/gen_tcp.rs:856`) has an
  inline fast path: it peeks the kernel buffer, finds a complete line,
  consumes it, delivers `{tcp, Socket, Data}` directly to the caller's
  mailbox, and only falls through to `RegisterActive` when the kernel
  buffer is empty (EAGAIN on peek).
- **`inet:setopts(active, false)`** sends `DeregisterActive` only if
  `opts.reactor_registered` is true (previously registered with the
  reactor). A pure-fastpath cycle never sets this flag, so it never
  dereg's.
- **`handle_active_read_on_worker`** (`process.rs:197`) drains the
  socket buffer per event (line mode loops until EAGAIN, carves
  newlines, delivers each `{tcp, Data}` inline on the worker thread).

So the current code *already* avoids the reactor on most of the fast
path. The remaining cost is:

1. Each fastpath `setopts(active, once)` call falls through to
   `RegisterActive` **when the kernel buffer is empty** — which is
   almost always the case in the steady SET path, because by the time
   onecached gets back to re-arming, memtier hasn't yet sent the next
   request.
2. Each `setopts(active, false)` call sends `DeregisterActive` **if
   reactor_registered is true** — which it becomes after every
   `RegisterActive`.
3. Each active-read event arrives via an `ActiveReadReady` fragment
   pushed by the reactor to the worker's mailbox (one kqueue event +
   one fragment delivery + one worker wake per inbound).

Per SET op with steady memtier pipelining: **1 RegisterActive + 1
DeregisterActive reactor command**, plus 1 inbound event. Call it
**~2 worker→reactor waker wakes per op** on the 1:1 workload.

## 3. Measured per-op cost of the remaining commands

From the 2026-04-13 profile:

- Worker-0 wall in `mio::Waker::wake` / kernel leaf: **78.9%**
  of 18.67 s = 14.73 s
- Estimated waker call rate per worker: ~2 ops/call × 8750 ops/s =
  **~17,500 calls/s per worker**
- Wall time per call: 14.73 s / (17.5 k × 18.67 s) = **~45 µs per wake**
- 45 µs × 2 wakes per op = **~90 µs/op attributable to waker cost**

The 90 µs is **on the per-op critical path** because the worker must
complete the `setopts` call (including the waker syscall) before
Erlang-level execution continues. It is not amortised across ops.

In addition, each of those reactor commands triggers:

- 1 reactor-side `kevent(EVFILT_EV_SET, register)` syscall (~10 µs
  uncontended, more under contention — the reactor also sits in kqueue
  most of its wall time, with 24% in `Selector::reregister` alone per
  the 2026-04-13 profile)
- 1 lookup in the `socket::REGISTRY` `RwLock<HashMap<u64,
  Arc<SocketEntry>>>` on the worker side (fast on read-contended path,
  ~1-2 µs but measurable in the aggregate)
- 1 socket-entry `Mutex<SocketOpts>` acquisition to toggle state

Rough additional savings from eliminating these bookkeeping costs:
~20–50 µs per op. Combined with the waker cost:

**Optimistic per-op saving: ~140 µs. Realistic: ~110 µs. Pessimistic:
~60 µs** (if contention measurements are overestimated).

## 4. Expected throughput gain (the honest numbers)

Plugging the saved latency into Little's law with parallelism = 200:

| Scenario     | New p50     | Little's law ceiling | Expected measured |
|--------------|------------:|---------------------:|------------------:|
| Baseline     |   2.527 ms  |               80 k   |            76.7 k |
| Pessimistic  |  ~2.467 ms  |               81 k   |         ~80-81 k  |
| Realistic    |  ~2.417 ms  |               83 k   |         ~82-83 k  |
| Optimistic   |  ~2.387 ms  |               84 k   |         ~84-85 k  |

**Expected gain: +5% to +11% on 1:1 throughput. p50 drops by
~60-140 µs.**

This is substantially less than my earlier "2.5 → 1.8 ms" framing and
I should say so plainly: **Phase 6 will not close the gap to BEAM
(159k).** BEAM's 4.9× latency advantage comes from architectural
properties Phase 6 does not touch: integrated I/O in the emulator,
native mailbox-push semantics, zero cross-thread command machinery,
and decades of tuned scheduler/GC path. Phase 6 is the biggest single
latency lever we can pull *within Nera's current runtime shape*, but
it is not the path to BEAM parity.

Phase 6 *does* get Nera close to Rust's 95k baseline. Rust at 1.287 ms
p50 is at 61% of its own Little's law ceiling — so Rust is leaving
even more headroom on the table than we are, meaning Phase 6 may
meaningfully narrow the gap to Rust's *measured* number without
matching its per-op latency.

**If Phase 6 lands and measures at +10% or below, the next lever is
architectural — scheduler integration, not shaving µs from I/O paths.**

## 5. Design

### 5.1. Core move

Register each TCP fd with kqueue **once** at connection open, under
a stable `Token` that never changes. Deregister **once** at close.
Everything else — `active` vs `passive` mode, controlling pid changes,
pending `recv`/`send` ops — becomes a flag flip on the socket's
existing `SocketEntry.opts` mutex, with **no reactor command and no
kqueue syscall** on the state change itself.

The reactor's event handler decides what to do with an incoming event
by reading the `SocketEntry.opts` state, not by looking up which
`Token` fired in a `pending: HashMap<Token, PendingOp>` table.

### 5.2. New `SocketOpts` fields

Extend the existing `SocketOpts` struct (`socket.rs:54`) rather than
replace it:

```rust
pub struct SocketOpts {
    // existing
    pub active: u8,              // 0/1/2
    pub packet: u8,              // 0=raw, 1=line
    pub controlling_pid: u64,
    pub line_buffer: Vec<u8>,

    // REMOVED: reactor_registered (no longer needed — registration is
    // per-connection-lifetime, not per-mode)

    // NEW — passive-mode pending op state, None when no recv is waiting
    pub pending_recv: Option<PendingRecvState>,
    // NEW — passive-mode pending send state, None when no partial send
    pub pending_send: Option<PendingSendState>,
}

struct PendingRecvState {
    waiter: ProcessId,
    // None = recv any available; Some(n) = recv exactly n bytes
    want_bytes: Option<usize>,
}

struct PendingSendState {
    waiter: ProcessId,
    buf: Vec<u8>,
    offset: usize,
}
```

The socket fd is *always* registered with `Interest::READABLE |
Interest::WRITABLE` from the moment it becomes known to nera. Edge-
triggered kqueue delivers a new event on each transition, which is
all the reactor needs under this design.

### 5.3. Reactor event handler (rewritten)

Current `IoReactor::handle_event` (`reactor.rs`, ~line 580) looks up
`pending[token]` and `active_regs[token]`. New shape:

```rust
fn handle_event(&mut self, token: Token, readable: bool, writable: bool) {
    let Some(entry) = self.token_to_entry.get(&token).cloned() else {
        return;
    };
    let mut opts = entry.opts.lock();

    if readable {
        match opts.active {
            0 => self.handle_passive_readable(&entry, &mut opts),
            1 | 2 => self.handle_active_readable(&entry, &mut opts),
            _ => {}
        }
    }
    if writable {
        self.handle_writable(&entry, &mut opts);
    }
}

fn handle_active_readable(&mut self, entry: &Arc<SocketEntry>, opts: &mut SocketOpts) {
    // Move the active-read logic (currently in
    // process::handle_active_read_on_worker) here, running on the
    // reactor thread. Deliver {tcp, ...} fragments via the existing
    // process mailbox push + wake path.
    //
    // For {active, once}: set opts.active = 0 after delivery.
}

fn handle_passive_readable(&mut self, entry: &Arc<SocketEntry>, opts: &mut SocketOpts) {
    let Some(pending) = opts.pending_recv.take() else {
        // Passive with no waiter: do nothing. Data stays in kernel buf.
        // Next recv call will get it via try_recv (direct syscall).
        return;
    };
    // Try to fulfil the pending recv from the kernel buffer.
    match try_fulfil_recv(entry, &pending) {
        Ok(bytes) => {
            deliver_recv_result(pending.waiter, Ok(bytes));
        }
        Err(e) if e.kind() == ErrorKind::WouldBlock => {
            // Put the pending op back — not enough data yet.
            opts.pending_recv = Some(pending);
        }
        Err(e) => {
            deliver_recv_result(pending.waiter, Err(e));
        }
    }
}
```

### 5.4. `inet:setopts(active, false)` (rewritten)

```rust
// No reactor command. No kqueue syscall. Just a flag flip.
with_entry_opts(handle, |opts| {
    opts.active = 0;
    opts.packet = new_packet.unwrap_or(opts.packet);
});
```

### 5.5. `inet:setopts(active, once)` (rewritten)

```rust
with_entry_opts(handle, |opts| {
    opts.active = 1;   // or 2 for {active, true}
    opts.packet = new_packet.unwrap_or(opts.packet);
    opts.controlling_pid = self_pid;
});
// Optional: inline-drain the kernel buffer on the worker here, the
// same way the current fast path does it. This is an *optimisation*,
// not correctness — the reactor will deliver an event for any pending
// readable data on its next poll iteration anyway.
```

No `RegisterActive` command. No `reactor_registered` tracking.

### 5.6. `gen_tcp:recv` (rewritten)

```rust
// Fast path unchanged: try_recv first, return inline if data available.
match try_recv(handle) { Ok(bytes) => return bytes, ... }

// Slow path: park the process with pending_recv set.
with_entry_opts(handle, |opts| {
    opts.pending_recv = Some(PendingRecvState {
        waiter: self_pid,
        want_bytes: None,
    });
});
// Park the caller; reactor will wake it on data arrival.
park_caller_on_socket(self_pid, handle);
```

No `PendingOp::Recv` submitted through the command channel.

### 5.7. Concurrency / correctness invariants

**Invariant A — single registration per fd**: each fd is registered
with kqueue exactly once, using a token allocated at registration
time. The `token_to_entry: HashMap<Token, Arc<SocketEntry>>` map on
the reactor is keyed by this stable token.

**Invariant B — `opts` mutex is the single source of truth**: the
reactor reads `opts.active` to decide dispatch mode; workers write
`opts.active` to request a mode change. Both hold the per-socket
`opts` mutex during the operation. No atomic flags on the side.

**Invariant C — event delivery is race-free with mode changes**: if a
worker flips `opts.active = 0` while the reactor is mid-dispatch of a
readable event, the reactor takes the mutex, sees `active == 0`, and
routes the event through `handle_passive_readable` instead. No lost
events, no double-delivery.

**Invariant D — kernel buffer is the source of truth for data**: data
in the kernel buffer survives any mode change. Workers never lose
access to received data because they can always `try_recv` directly.

**The 2026-04-11 fragment-stranding fixes (see earlier plan entry)
must be re-validated under this design.** The current fix lives in a
`in_flight: Arc<AtomicBool>` flag on `ActiveReg` with race checks in
`scheduler.rs` around `STATUS_RECEIVE_WAIT` / `STATUS_IO_WAIT` /
`ASTATUS_WAITING`. In the new design, the equivalent guarantee is
"mailbox message push + `try_wake` under the opts mutex happens
atomically with the state change", which I believe eliminates the
stranding pattern entirely — but this needs a deliberate walkthrough
before the rewrite commit lands.

## 6. Commit breakdown

Three commits, each independently testable and rollback-safe.

### Commit 1 — Slab-backed socket registry (prerequisite)

**Files:** `socket.rs`

**Change:** Replace
`static REGISTRY: RwLock<Option<HashMap<u64, Arc<SocketEntry>>>>` with
a `Slab<Arc<SocketEntry>>` or a `Vec<Option<Arc<SocketEntry>>>` +
generation-counter scheme. `get_entry(handle)` becomes an
index-lookup with no lock for reads (if slab is RCU-style) or a brief
read-lock on an `Arc<RwLock<Slab>>`.

This was Phase 5 workstream #1 and is a prerequisite for Phase 6
because the reactor will hit this table on every event dispatch under
the new design. Ship it separately so the benchmark delta is
attributable.

**Scope:** ~200 lines, self-contained.

**Validation:** `cargo test -p nera-runtime`, full onecached bench
5-run 1:1, profile to confirm the DashMap hot spots in
`socket::get_entry` drop out.

**Expected gain in isolation:** 1-3% throughput on 1:1, mostly
reactor-side CPU reduction. Not a latency lever on its own.

### Commit 2 — Reactor event dispatch reads `SocketEntry.opts`

**Files:** `reactor.rs`, `socket.rs`, `process.rs`

**Change:** Add a stable `token: Token` field to `SocketEntry` set at
registration. Register each TCP stream once at `socket::register`
time with `Interest::READABLE`. The reactor's event handler looks up
the entry by token and dispatches based on `opts.active`. Move the
active-read logic from `process::handle_active_read_on_worker` into
the reactor thread.

Remove `ActiveReg`, `active_regs`, `active_handle_to_token`,
`RegisterActive`, `DeregisterActive`, `RearmActive` — the whole
parallel active-mode machinery.

Keep `PendingOp::Recv` / `PendingOp::Send` for now (used by the
slow-path `submit_to_reactor_and_wait` callers). Commit 3 replaces
those.

**Scope:** ~500 lines of churn across 3 files. The single largest
commit in the phase.

**Risk:** This is the race-prone commit. The 2026-04-11 stranding-fix
walkthrough goes here. I'll want to run the reproducer
(`./benchmarks/onecached_bench.sh --engines nera --runs 20 --test-time
5 --ratio 1:1` — historically the shape that surfaced the hang) as
part of validation.

**Validation:** cargo test, cargo test -p nera-runtime with `--
--nocapture`, full bench 5-run 1:1 AND 1:9 AND `memtier --ratio=9:1`,
20-run stability loop to check for teardown races, re-profile to
confirm `Waker::wake` wall share drops from ~75% to near 0%.

**Expected gain in isolation:** +5% to +11% on 1:1 throughput (the
core Phase 6 gain). +2-5% on 1:9 (since 1:9 triggers fewer setopts
cycles).

### Commit 3 — Pending recv/send state on `SocketEntry.opts`

**Files:** `reactor.rs`, `socket.rs`, `ffi/gen_tcp.rs`

**Change:** Add `pending_recv` / `pending_send` fields to
`SocketOpts`. Rewrite `gen_tcp:recv` / `gen_tcp:send` slow paths to
set these fields and park the caller, instead of submitting
`PendingOp::Recv` / `PendingOp::Send` through the command channel.
Reactor's event handler consults `pending_recv` / `pending_send` on
readable/writable events and fulfils them in-place.

Delete `PendingOp::Recv`, `PendingOp::Send`, and
`submit_to_reactor_and_wait`'s cmd-channel path.

**Scope:** ~300 lines.

**Risk:** Low — the same `try_recv` / `try_send` fast paths remain;
only the slow path changes. Slow path only fires when EAGAIN hits the
worker, which is rare on localhost.

**Validation:** cargo test, full bench, confirm 1:1 and 1:9 both
stable. Re-run a 200-connection TCP-ping-pong benchmark where EAGAIN
is more likely (`bench_tcp_throughput.erl`) to exercise the new
slow path.

**Expected gain in isolation:** negligible on 1:1 (slow path is
cold), but closes the symmetry gap — no reactor-command code remains.
This commit is about code cleanliness, not throughput.

## 7. Validation gates

Each commit must pass all of:

1. **Correctness.** `cargo test` across all nera crates. Poolboy /
   thoas integration tests must not regress.
2. **Stability.** 20 × 5 s 1:1 runs without teardown hangs. Manual
   sanity check with `lsof | grep onecached_nera` after each run to
   confirm fd table drains to expected level.
3. **Throughput.** 5-run median on 1:1 and 1:9 reported against the
   baselines captured in `ONECACHED_PERFORMANCE_PLAN.md`.
4. **Profile shape.** After commit 2, re-run `profile_onecached.sh`
   and confirm `Waker::wake` wall share on workers has dropped from
   ~75% to near zero. If it hasn't, the change did not do what it
   was supposed to and we pause before commit 3.
5. **Memory.** RSS load / flush / footprint must not regress beyond
   the Phase 4 targets (load < 60 MB on 1:1, flush < 40 MB).

## 8. Rollback plan

Each commit is independent and can be reverted via `git revert`
without touching the others. If commit 2 turns out to be race-prone
(teardown hang returns), revert commit 2 + commit 3 and keep commit 1
(the slab rewrite) as a standalone improvement.

## 9. What Phase 6 is NOT

- **Not a switch to level-triggered kqueue.** mio's edge-triggered
  semantics are correct for this design; level-triggered would
  require bypassing mio and gains us nothing.
- **Not a reimplementation of BEAM's port driver.** BEAM's advantage
  is structural and out of scope.
- **Not a scheduler rewrite.** Worker park/unpark is not the
  bottleneck (confirmed 2026-04-14 audit).
- **Not a throughput miracle.** Expected +5-11% on 1:1. If the
  measurement comes in at +3% or less, that tells us the per-op cost
  I estimated above was dominated by something else, and the next
  investigation is cycle-accurate per-op tracing — not more
  Phase-6-style rewrites.
