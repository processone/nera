# Phase 8 — Worker-Local Direct Dispatch: Design

Date: 2026-04-15

**Historical design.** Commits 1–3 and a fourth hybrid-dispatch correction landed
by 2026-04-17 (`c5b89ee`). Performance projections below are not achieved-result
claims. The October restart prioritizes the correctness and packaging blockers
in [PROJECT_STATUS.md](PROJECT_STATUS.md); further dispatch tuning is deferred.

## 1. Goal

Reduce the 200 µs average worker park duration observed in Phase 7
to something close to the minimum OS wake latency (~30-50 µs) by
sending dispatched work to a *specific* worker's local queue and
waking *that* worker directly, instead of pushing to a shared
injector and broadcasting a notify on the global condvar.

Expected 1:1 onecached outcome: **180-240 k ops/sec** (from the
current 133 k post-slab), bringing Nera past the 150 k ambitious
target and into the neighbourhood of the Rust baseline (179 k) on
this workload. See §9 for the arithmetic.

This is a bigger commit than the slab rewrite: ~250-400 lines across
`scheduler.rs` and `reactor.rs`. It's worth writing a design doc
first because it has real race-condition corners that the spin
experiment didn't (and still doesn't).

## 2. Why this, not something else

After two wrong Phase 8 attempts (50 µs spin budget, 15 µs spin
budget — both regressed), the measurement evidence points firmly at
the dispatch cycle being the bottleneck:

- **Phase 7 data**: 60.9% worker wall in `ThreadParker::wait_for`,
  pops/park ~6.5, avg park 200 µs.
- **Per-op math**: at 133 k ops/sec across 8 workers, each worker
  runs a process slice for ~29 µs then waits ~200 µs for the next
  one. Workers are idle 3.4× their actual work time.
- **The spin experiment showed**: pop-gaps shorter than ~50 µs are
  rare on this workload; the median gap is long enough that cheap
  spinning can't amortise it.

The structural alternative: don't make workers *discover* new work
via polling/stealing — **have the reactor push work directly to a
specific worker's queue and wake that worker specifically**.

## 3. Current architecture (what we're changing)

### Dispatch path (today)

```text
                       reactor thread
                             │
                             │ push(process) to injector
                             ▼
                    ┌──────────────────┐
                    │  Global Injector │ (crossbeam-deque)
                    └─────────┬────────┘
                              │
          ┌───────────────────┴───────────────────┐
          │                                       │
          │          notify_one on shared         │
          │             worker_notify cv          │
          │                                       │
   worker-0 ← . . .  worker-7  (all 8 parked on the same cv)
```

When a worker wakes, it tries: local deque → injector → steal from
all 7 peers → spin → park again. The *specific worker that will
actually handle the process* is chosen implicitly by OS wake ordering
plus work-stealing, not by the dispatcher.

### Problems with this

1. **Broadcast wake latency.** `notify_one` on a condvar of 8 parked
   workers takes one OS scheduler tick to decide which worker gets
   the permit. That's the ~200 µs Phase 7 measured.
2. **Work-stealing is a hot path, not a fallback.** At 1:1 steady
   state workers *routinely* end up stealing from peers, which
   means every idle worker's loop touches every other worker's
   deque head pointer. Cache-coherence traffic on 7 other deques
   per spin iteration was a major hidden cost of the spin
   experiment.
3. **No affinity.** A process that ran on worker 3, then became
   runnable again, has a warm cache on worker 3. The current path
   has no way to route it back to worker 3 — any idle worker can
   grab it.

## 4. Proposed architecture

### Overview

```text
                    reactor thread
                          │
                          │ 1. pick target worker via affinity policy
                          │ 2. push(process) to that worker's inbox
                          │ 3. unpark(that worker)
                          ▼
       ┌────────────┐   ┌────────────┐   ┌────────────┐
       │ worker-0   │   │ worker-1   │...│ worker-7   │
       │ inbox      │   │ inbox      │   │ inbox      │ ← MPSC, lock-free
       ├────────────┤   ├────────────┤   ├────────────┤
       │ local deque│   │ local deque│...│ local deque│ ← unchanged, owned by worker
       ├────────────┤   ├────────────┤   ├────────────┤
       │ Parker     │   │ Parker     │...│ Parker     │ ← per-worker, std::thread::Parker
       └────────────┘   └────────────┘   └────────────┘

     worker N's idle loop:
       drain own inbox → local deque
       try pop from local deque
       try pop from global injector (kept for timer/spawn fallback)
       if still nothing: brief spin, then park (per-worker Parker)
       — work stealing is now a last-resort fallback, not the default
```

### New components

#### Per-worker inbox (`SegQueue<Arc<ProcessEntry>>`)

An unbounded MPSC queue, one per worker. The reactor (and other
producers) push into it; only the owning worker drains.
`crossbeam_queue::SegQueue` is already a dependency and is the
natural fit — lock-free, unbounded, MPSC semantics.

Unbounded is important: we never want a `push` to block the
reactor. The queue grows by ~64-element segments; a pathologically
backed-up worker would consume memory but not deadlock.

#### Per-worker `Parker` / `Unparker`

`std::thread::Parker` (available via `std::sync::Condvar` primitives
or the `parking_lot_core` crate — we'll use `std::thread::park` +
`std::thread::Thread::unpark` for simplicity). Each worker owns its
Parker (which is its own Thread handle); the scheduler holds a
`Vec<Thread>` of worker thread handles and calls `unpark()` on the
target when dispatching.

**Why not a shared condvar with target hints**: `notify_one` on a
shared condvar wakes an *arbitrary* waiter, not a specific one. OS
scheduling decides which parked thread gets the permit, and the
reactor has no control. Per-thread unpark gives us the guarantee.

**Why not parking_lot parkers**: `std::thread::Parker` is simpler
(no extra dependency), and its permit-based semantics (unpark before
park → park returns immediately) are exactly what we need for the
race analysis in §6.

#### Target-selection policy: "sticky owner with global fallback"

When the reactor dispatches a ready process, it needs to pick a
target worker. Options considered:

- **Round-robin**: fair but ignores cache locality.
- **Least-loaded**: requires atomic read of every worker's queue
  length per dispatch; racy (the "shortest" worker at read time
  might not be at push time); and the policy itself induces more
  cross-core traffic than the work it dispatches.
- **Owner-pinned at spawn**: each process has a fixed home worker.
  Simple and cache-friendly. But load imbalance is possible if one
  worker's processes are all busy while another is idle.
- **Sticky owner with fallback** (chosen): each `ProcessEntry` has
  a `last_worker: AtomicU8` field. Dispatch picks `last_worker` if
  set, else round-robin among workers. Work stealing remains
  available as a last-resort safety net but is no longer the
  default dispatch mechanism. This gives us cache locality for
  long-lived connections (every SET/GET on the same onecached
  connection goes to the same worker) while keeping a path to
  rebalance via stealing.

`last_worker` is set when a worker *runs* the process, not when it
receives it in its inbox. This way a stolen-to process updates its
home to the new worker naturally.

### Unchanged components

- `Injector` (global FIFO): kept as the second-level queue for
  cases where no specific target is known (timer firings, initial
  `spawn_process` calls, maybe `send_message` if we don't wire it
  through in the first commit).
- `Stealer<Arc<ProcessEntry>>` per worker: kept for the
  last-resort steal fallback path.
- `worker_notify: Arc<(Mutex<()>, Condvar)>`: **kept but demoted**.
  Used only by code paths that don't know a specific target
  (timer/shutdown broadcast, initial spawn). Reactor dispatch stops
  using it entirely.
- Everything else in `scheduler.rs` — termination detection,
  STATUS_* transitions, spin iterations before park, reduction
  counting — unchanged.

## 5. Worker loop, reworked

```rust
fn worker_loop(worker_id, local, stealers, inbox, parker,
               /* ... */) {
    loop {
        if shutdown { return; }

        // 1. Drain own inbox into local deque. Batched push, no
        // per-element work here — this is the new hot path.
        while let Some(proc_entry) = inbox.pop() {
            local.push(proc_entry);
        }

        // 2. Try local deque (normal pop).
        let maybe_proc = local.pop()
            // 3. Global injector (timer / spawn / legacy path).
            .or_else(|| inject_pop_batch(&injector, &local))
            // 4. Steal fallback (last resort).
            .or_else(|| steal_round_robin(&stealers, worker_id,
                                          num_threads));

        match maybe_proc {
            Some(p) => { /* run the process — unchanged */ }
            None => {
                idle_spins += 1;
                if idle_spins <= 3 {
                    stats.spin_iters.fetch_add(1, Relaxed);
                    for _ in 0..64 { std::hint::spin_loop(); }
                    continue;
                }
                // Park on this worker's own Parker.
                stats.parks.fetch_add(1, Relaxed);
                let t0 = Instant::now();
                std::thread::park_timeout(Duration::from_millis(2));
                stats.park_ns_total.fetch_add(
                    t0.elapsed().as_nanos() as u64, Relaxed);
                // Termination detection — unchanged.
                // ...
            }
        }
    }
}
```

### Why drain inbox first, not last

The race analysis in §6 relies on this. The worker MUST drain the
inbox before parking, and specifically must check it AFTER any
observable state change to the inbox. Doing the drain first on each
iteration means:

- If a producer pushed and unparked while we were running a process,
  the permit is pending and `park_timeout` returns immediately.
- On return, we drain and pick up the pushed item.
- Order: push → unpark → (worker finishes current work) → drain inbox →
  park → unpark satisfies permit → park returns → drain again.

## 6. Concurrency invariants

### Invariant A: inbox is MPSC

The inbox's producer side is **multi-producer** (reactor, workers
calling `send_message`, the timer thread, any FFI code). The
consumer side is **single-consumer** — only the owning worker drains
it. `SegQueue::pop` is safe from one consumer; we never call it from
anywhere but `worker_loop`.

### Invariant B: dispatch ordering is push-then-unpark

Every producer must order its operations as:

```rust
worker_inboxes[target].push(entry);
worker_threads[target].unpark();
```

The push must *happen-before* the unpark. `SegQueue::push` has
`Release` semantics internally, and `Thread::unpark` also has
`Release` on its internal permit flag. The target worker's
`park_timeout` has `Acquire` on the permit, and its subsequent
`inbox.pop()` has `Acquire` on the queue head. So the full chain
push→unpark→park-return→pop is correctly synchronised.

### Invariant C: no dispatched item is lost across the drain/park boundary

Proof sketch. Consider any item `I` pushed to `worker[N].inbox` at
time `t_push`, followed by `worker_threads[N].unpark()` at `t_unpark`.
Let `worker[N]` be the owning worker. At some time `t_consume >= t_push`,
one of the following must occur:

1. `worker[N]` is running a process slice or in its retry loop
   between `t_push` and `t_unpark`. On the next iteration of its
   loop, it calls `inbox.drain()` (line 1 in §5). That drain is
   guaranteed to see `I` because `t_push < t_drain`. **✓**
2. `worker[N]` has already passed its drain step at `t_push` and
   is between drain and `park_timeout` at `t_unpark`. The unpark
   makes the permit available before park is called. `park_timeout`
   returns immediately without blocking. Next iteration drains,
   sees `I`. **✓**
3. `worker[N]` is blocked in `park_timeout` at `t_unpark`. The
   unpark wakes it. Next iteration drains, sees `I`. **✓**
4. `worker[N]` is actively pushing `I` into its own inbox at
   `t_push` (it's a worker dispatching to itself). This is a
   cross-worker race only if a *different* worker is the owner;
   a worker pushing to its own inbox is fine — self-drain picks
   it up on the next iteration. The self-unpark is a no-op
   because the worker isn't parked.

No case leaves `I` undrained indefinitely. The only unbounded delay
is the 2 ms park timeout, which caps worst-case drain latency
during shutdown.

### Invariant D: affinity is advisory, not enforced

`ProcessEntry.last_worker` is a hint, not a lock. A worker may run
a process whose `last_worker` is some other index (because it was
stolen, or because the affinity target is busy and stealing
rebalanced). When a worker runs a process, it updates `last_worker`
to its own id — the next dispatch will home there. This naturally
converges under load.

### Invariant E: termination detection is unchanged

The existing `runnable_count + should_terminate` logic continues to
work because it doesn't depend on the wake mechanism — it looks at
atomic counts of runnable/alive processes and the injector/pending
state. A worker's Parker is invisible to that check.

## 7. Sources of wakes — what changes

| Source                    | Before                       | After                                                |
|---------------------------|------------------------------|------------------------------------------------------|
| Reactor dispatch          | push injector + notify_one   | push worker[target].inbox + thread.unpark            |
| send_message cross-worker | push injector + notify_one   | push worker[last_worker].inbox + thread.unpark       |
| Timer fires               | push injector + notify_one   | push worker[0 or RR].inbox + thread.unpark (see note)|
| Initial spawn_process     | push injector + notify_one   | push worker[RR].inbox + thread.unpark                |
| Shutdown                  | broadcast notify_all         | unpark all workers                                   |

Note on timer: timer wakes are infrequent (~timer wheel ticks, not
per-message). Picking a single target (worker 0 or round-robin) is
fine. A spurious extra-parked-worker cost from an unlucky target
choice is bounded and rare.

Note on shutdown: the current `notify_all` broadcast is replaced by
a loop `for t in worker_threads { t.unpark(); }`. Same semantics.

## 8. Commit breakdown

Three commits, each testable in isolation.

### Commit 1 — Inbox infrastructure, wired but unused

**Files**: `scheduler.rs`, `process.rs` (ProcessEntry).

**Changes**:
- Add `inbox: SegQueue<Arc<ProcessEntry>>` to per-worker state in
  `Scheduler` / `worker_loop`.
- Add `worker_threads: Vec<Thread>` captured at worker spawn.
- Add `last_worker: AtomicU8` field to `ProcessEntry`.
- Worker loop drains its inbox first, pushes items into its local
  deque, then proceeds with the existing pop sequence.
- **No producer changes yet**: reactor still pushes to the
  injector and notifies the shared condvar. The inbox path is
  present but dormant.

**Test**: scheduler tests pass, 5-run onecached 1:1 matches the
pre-commit baseline within variance (no regression from the
inbox drain overhead).

**Scope**: ~80 lines. Self-contained and safe.

### Commit 2 — Reactor dispatches via inbox

**Files**: `reactor.rs`, possibly `process.rs` for the ActiveRead
fragment dispatch site, `scheduler.rs` for the SchedulerHandle
API.

**Changes**:
- Add `fn dispatch_to_target(&self, entry: Arc<ProcessEntry>)` to
  `SchedulerHandle`. Picks target via affinity policy (last_worker
  if set, else round-robin), pushes to that worker's inbox,
  unparks that worker's Thread.
- Rewrite reactor's ActiveReadReady fragment dispatch to call this
  instead of pushing to the injector + notify.
- Update `last_worker` on `ProcessEntry` when a worker actually runs
  it (in `worker_loop`, at the `idle_spins = 0` reset point).

**Test**: scheduler tests pass. 20-run onecached 1:1 stability
loop (the 2026-04-11 fragment-stranding regression surface) passes
without hangs. 5-run onecached 1:1 throughput; expected substantial
uplift (see §9). 5-run `bench_tcp_ping_pong` also uplifted. 5-run
`bench_compute` unchanged (regression canary).

**Risk**: this is the race-sensitive commit. The 2026-04-11
teardown races touched the exact flow (worker drain + in_flight
atomic + STATUS_* checks). The new dispatch path must not
re-introduce the stranded-fragment bug. Mitigation: invariant
walkthrough (§6), 20-run stability reproducer, fresh
`NERA_SCHED_STATS` run comparing pops/park/rescues rates.

**Scope**: ~200-300 lines.

### Commit 3 — send_message + spawn_process migrate to inbox

**Files**: `process.rs` (send_message FFI), `scheduler.rs`
(spawn_process).

**Changes**:
- `send_message` already knows the target process's `last_worker`;
  push to that inbox + unpark.
- `spawn_process` picks a target via round-robin; push to that
  inbox + unpark.
- Timer thread is left on the old path (injector + notify) for
  simplicity; can migrate later if measurement says it matters.

**Test**: cargo test, onecached 1:1 and 1:9, spawn_ring
microbenchmark (which stresses `spawn_process` + `send_message`).
Expected: spawn_ring moves in the same direction as onecached; 1:9
matches or exceeds current.

**Scope**: ~100 lines.

## 9. Expected performance — the arithmetic

Current (post-slab + Phase 7 baseline):

- Pops/park: ~6.5
- Avg park: ~200 µs
- Work per pop: ~29 µs
- Per-cycle time: `6.5 × 29 + 200 = 388.5 µs`
- Cycles/sec/worker: `1e6 / 388.5 ≈ 2 573`
- Ops/sec/worker: `2 573 × 6.5 ≈ 16 727`
- Ops/sec aggregate: `16 727 × 8 ≈ 133 800` ✓ (matches 133 k measured)

Worker-local dispatch should move two quantities:

1. **Avg park time down** — from 200 µs to ~30-50 µs (OS minimum
   wake latency for a directly-targeted `unpark`).
2. **Pops/park up** — fewer wasted parks because work goes directly
   to a specific worker's inbox; workers processing a burst of
   dispatches see them all in one drain.

### Optimistic scenario

- Pops/park: 15 (modest improvement from drain-batching)
- Avg park: 40 µs (OS minimum wake)
- Per-cycle: `15 × 29 + 40 = 475 µs`
- Cycles/sec/worker: `~2 105`
- Ops/sec/worker: `~31 580`
- Ops/sec aggregate: **~252 k** (89% improvement over current)

### Realistic scenario

- Pops/park: 10
- Avg park: 60 µs
- Per-cycle: `10 × 29 + 60 = 350 µs`
- Cycles/sec/worker: `~2 857`
- Ops/sec/worker: `~28 570`
- Ops/sec aggregate: **~228 k** (71% improvement over current)

### Pessimistic scenario

- Pops/park: 8 (only small improvement)
- Avg park: 100 µs (affinity churn means park wakes aren't direct)
- Per-cycle: `8 × 29 + 100 = 332 µs`
- Cycles/sec/worker: `~3 012`
- Ops/sec/worker: `~24 096`
- Ops/sec aggregate: **~193 k** (45% improvement over current)

**Range: +45% to +89%, target 193-252 k on 1:1.** Even the
pessimistic case is well past the 150 k ambitious target and close
to the Rust baseline (179 k). The optimistic case brings us **past
the minimal Rust baseline** on this workload — a result I'd love
to see measured but cannot promise.

### Hidden downside (worth watching)

Per-worker affinity can create **cold spots** on workers that
happen to get an unlucky distribution of short-lived processes.
Work stealing used to smooth this implicitly; under this design,
stealing is a last-resort fallback and doesn't run often. If the
benchmark shows per-worker throughput imbalance (via sched-stats
per-worker breakdown) we may need to tune the affinity policy
(lower stickiness threshold, or lookback window).

`bench_compute` is the regression canary for the opposite case: a
single CPU-bound process that happens to home on a worker with
other busy processes would be starved. If `bench_compute` regresses
after commit 2, that's the signal.

## 10. Risks

### Primary: re-introducing the 2026-04-11 fragment-stranding bug

The previous fix (atomic `in_flight` flag + race checks in
`scheduler.rs:773-793`) touched the exact part of the code that
commit 2 rewrites. Mitigation:

- Re-read `memory/project_onecached_perf.md` section "Latent race
  in direct-reregister" before starting commit 2.
- Run the 20-run stability reproducer after commit 2 and confirm
  no teardown hangs (`lsof | grep onecached_nera` drain to zero).
- The invariant walkthrough in §6 should explicitly address: what
  happens when a worker is mid-drain and another worker pushes a
  fragment to that same process?

### Secondary: `SegQueue` memory growth

SegQueue allocates segments of ~64 entries. A worker that falls
behind won't block producers, but its inbox can grow unbounded.
Under sustained backpressure this could increase RSS. Mitigation:
monitor RSS in the bench harness (already measured); if it
regresses, add a soft cap with fallback to injector.

### Tertiary: affinity policy edge cases

`last_worker` of 0 on a freshly-spawned process means "no home yet".
Round-robin dispatch for first dispatch. If RR counter wraps
non-atomically, workers may get bursts of dispatches to the same
target. Use `AtomicU32::fetch_add(1, Relaxed) % num_workers`.

### Quaternary: shutdown races

Shutdown currently uses `notify_all` to wake every parked worker.
New path: `for t in worker_threads { t.unpark(); }`. Workers see
`shutdown=true` on next loop iteration and return. Should be
equivalent but test under load to confirm.

## 11. Validation gates

Every commit in this phase must pass:

1. **Correctness**: `cargo test -p nera-runtime`, full workspace
   build warnings-as-errors.
2. **Stability**: 20 × 5 s 1:1 runs without teardown hangs. Manual
   `lsof | grep onecached_nera | wc -l` after each run to confirm
   fd table drained to expected level.
3. **Throughput**: 5-run median on 1:1 compared against the
   pre-commit baseline. Documented in commit message.
4. **Regression canary**: `bench_tcp_ping_pong` must move in the
   same direction as onecached 1:1 (both faster or both unchanged).
   `bench_compute` must be unchanged.
5. **Profile shape**: re-run `profile_onecached.sh` after commit 2,
   confirm `ThreadParker` wall share drops significantly from 60.9%.
   If it doesn't drop, the change didn't do what it was supposed to
   and we pause before commit 3.
6. **Memory**: RSS load < 80 MB, RSS flush < 40 MB. Current is 46
   / 3 MB so there's headroom, but watch for growth from SegQueue
   segments.

## 12. Rollback plan

Commits 1, 2, 3 are independent and revertible.

- Commit 1 (inbox infrastructure, dormant): safe to keep even if
  2 and 3 revert. No behaviour change.
- Commit 2 (reactor dispatches via inbox): if a race surfaces after
  merge, revert 2 alone. Commit 1's infrastructure stays but goes
  unused until the race is fixed.
- Commit 3 (send_message + spawn via inbox): same, revert alone
  if it causes a spawn_ring regression or cross-process race.

If Phase 8 as a whole turns out to be net-negative (the throughput
gain doesn't materialise after commit 2), revert all three and
treat the `spin_rescues` counter from the Phase 8.0 experiment as
the only durable Phase 8 artifact. The `SegQueue` inboxes and
per-worker affinity would then be left for a future attempt.

## 13. What Phase 8 is NOT

- **Not a replacement for the work-stealing scheduler**. Stealers
  are kept as a last-resort rebalance mechanism. The goal is to
  make stealing the uncommon case, not eliminate it.
- **Not a change to the reactor's I/O model**. The reactor still
  polls kqueue/epoll, still drains a command channel, still owns
  the socket registry. Only the *destination* of dispatched
  fragments changes.
- **Not BEAM's port-driver model**. BEAM's port drivers run inside
  the emulator's scheduler threads; nera's reactor is a separate
  thread. We're making dispatch cheaper, not merging threads.
- **Not a change to how processes are scheduled within a worker**.
  Time-slicing, reduction counting, STATUS_* transitions — all
  unchanged.

## 14. Timeline estimate

- Commit 1: 1 day (inbox + worker_threads plumbing, no producer
  changes, test stability).
- Commit 2: 2-3 days (race audit + implementation + bench runs
  + profile check + potentially one cycle of "reprofile, adjust,
  re-bench").
- Commit 3: 1 day.
- **Total: 4-5 days** of focused work.

## 15. Decision required before starting

Before I write any code:

1. **Affinity policy confirmation**: sticky-owner with round-robin
   fallback, as described. Any preference for least-loaded or
   round-robin-only?
2. **Wake primitive**: `std::thread::Parker` + `Thread::unpark`, or
   `parking_lot_core::park`? Std lib is simpler, no extra dep.
3. **SegQueue vs ArrayQueue**: unbounded vs bounded. SegQueue is
   correct (never blocks producers) but can grow under backpressure.
   ArrayQueue is bounded but can force the producer to fall back.
   Default to SegQueue unless there's a reason to cap.
4. **Timer thread**: leave on the legacy path (injector + notify)
   for commit 2, or migrate in commit 3 with send_message?
