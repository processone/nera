//! Work-stealing scheduler for Erlang processes.
//!
//! The scheduler maintains a global injector queue for newly spawned
//! processes and per-worker local deques. Workers first try their own
//! deque, then steal from the global injector, and finally steal from
//! random peers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use crossbeam_queue::SegQueue;
use dashmap::DashMap;
use parking_lot::{Condvar, Mutex, RwLock};

use std::collections::HashSet;

use crate::io_pool::IoPool;
use crate::process::{
    MonitorRef, Process, ProcessEntry, ProcessId, ProcessState, ProcessStatus, RuntimeProcessGuard,
    WaitReason, ASTATUS_EXITED, ASTATUS_IO_WAIT, ASTATUS_READY, ASTATUS_RUNNING, ASTATUS_WAITING,
    REDUCTION_LIMIT, STATUS_EXCEPTION, STATUS_HIBERNATE, STATUS_IO_WAIT, STATUS_OK,
    STATUS_RECEIVE_WAIT, STATUS_YIELD,
};
#[cfg(mio_supported)]
use crate::reactor::ReactorHandle;
use crate::timer::TimerWheel;

// ---------------------------------------------------------------------------
// Global scheduler reference for FFI access
// ---------------------------------------------------------------------------

static GLOBAL_SCHEDULER: OnceLock<Arc<SchedulerHandle>> = OnceLock::new();

thread_local! {
    /// The worker_id of the currently running worker thread, or `u8::MAX`
    /// on non-worker threads (reactor, timer, I/O pool, FFI at startup).
    /// Set by `worker_loop` on entry; read by `dispatch_to_worker` to
    /// choose between same-worker inbox push (cache-hot, no unpark) and
    /// cross-worker injector push (work-stealing keeps serial chains
    /// running on one worker — avoids the per-hop park/unpark penalty
    /// that regressed spawn_ring / hibernate in the pure commit-3 path).
    pub(crate) static CURRENT_WORKER: std::cell::Cell<u8> = const { std::cell::Cell::new(u8::MAX) };
}

/// A handle to the scheduler internals that FFI functions can use.
/// This is a separate struct that shares the same underlying data.
pub struct SchedulerHandle {
    pub injector: Arc<Injector<Arc<ProcessEntry>>>,
    pub process_table: Arc<DashMap<ProcessId, Arc<ProcessEntry>>>,
    pub(crate) next_pid: Arc<crate::process::ProcessIdAllocator>,
    /// Count of runnable (Ready/Running) processes. Atomically updated
    /// whenever a process transitions to/from Ready/Running state.
    /// When this reaches 0 and stays 0, the scheduler can terminate.
    pub runnable_count: Arc<AtomicI64>,
    /// Count of alive (non-exited) processes. Used by `should_terminate()`
    /// to short-circuit the full table scan when all processes have exited.
    pub alive_count: Arc<AtomicI64>,
    /// Registered process names: atom_id -> pid.
    pub registered_names: Arc<RwLock<HashMap<u32, ProcessId>>>,
    /// Timer wheel for efficient receive...after timeout management.
    pub timer_wheel: Arc<TimerWheel>,
    /// Condvar to wake idle worker threads when new work is available.
    /// Kept for the legacy path (timer wakes, shutdown broadcast).
    pub worker_notify: Arc<(Mutex<()>, Condvar)>,
    /// I/O thread pool for async blocking I/O operations.
    pub io_pool: Arc<IoPool>,
    /// Event-driven I/O reactor (epoll/kqueue/IOCP) for TCP operations.
    #[cfg(mio_supported)]
    pub reactor: ReactorHandle,
    /// Phase 8: per-worker lock-free inboxes for direct dispatch. Index
    /// matches worker_id. Producers push ready `ProcessEntry`s here;
    /// the owning worker drains into its local deque.
    pub worker_inboxes: Arc<Vec<Arc<SegQueue<Arc<ProcessEntry>>>>>,
    /// Number of workers — used by dispatchers for round-robin.
    pub num_workers: usize,
    /// Phase 8: per-worker Thread handles for targeted `unpark()`.
    /// Pre-allocated with `num_workers` OnceLock entries; each is filled
    /// after the corresponding worker thread is spawned (OnceLock is
    /// interior-mutable). Dispatchers read from here to unpark a
    /// specific worker without a shared condvar.
    pub worker_threads: Arc<Vec<OnceLock<thread::Thread>>>,
    /// Phase 8: round-robin counter for dispatching unhomed processes.
    /// Wraps at `num_workers`. Relaxed ordering is fine — slight
    /// over-dispatch to one worker is harmless; work stealing rebalances.
    pub dispatch_rr: AtomicU32,
}

impl SchedulerHandle {
    /// Allocate a unique encodable PID, failing closed on domain exhaustion.
    #[inline]
    pub fn next_pid(&self) -> ProcessId { self.next_pid.next() }

    #[inline]
    pub fn try_next_pid(&self) -> Option<ProcessId> { self.next_pid.try_next() }


    /// Dispatch a ready process to a worker. Hybrid policy:
    ///
    /// 1. **Non-worker caller** (reactor, timer, I/O pool, FFI at
    ///    startup): push to target's inbox + targeted unpark — fast
    ///    wake of a parked worker for async events.
    /// 2. **Worker caller, same-worker affinity** (target.last_worker
    ///    == current): push to the local inbox, no unpark (this
    ///    thread IS that worker and is running). Drains on next slice.
    /// 3. **Worker caller, cross-worker, sender's inbox non-empty**
    ///    (parallel load): inbox + targeted unpark of the target
    ///    worker — matches commit-3's fast-wake path for onecached
    ///    workloads with many concurrent ready procs.
    /// 4. **Worker caller, cross-worker, sender's inbox empty** (serial
    ///    chain / token ring / ping-pong): push to the global injector
    ///    so the sender's worker work-steals it at slice end — keeps
    ///    the chain cache-hot on a single worker and avoids the
    ///    per-hop `thread::park` / `unpark` penalty that regressed
    ///    spawn_ring and hibernate in the pure commit-3 path.
    pub fn dispatch_to_worker(&self, entry: Arc<ProcessEntry>) {
        // Guard: no workers or no inboxes (stale handle from tests).
        if self.num_workers == 0 || self.worker_inboxes.is_empty() {
            self.injector.push(entry);
            let (ref _lock, ref cvar) = *self.worker_notify;
            cvar.notify_one();
            return;
        }

        let current = CURRENT_WORKER.with(|c| c.get());
        let home = entry.last_worker.load(Ordering::Relaxed);
        let homed = home < self.num_workers as u8;

        if current != u8::MAX {
            // Called from a running worker.
            if homed && home == current {
                // Same-worker affinity — push to the local inbox. The
                // worker is running; it will drain the inbox on the
                // next scheduler pickup. No unpark needed.
                self.worker_inboxes[current as usize].push(entry);
                return;
            }
            // Cross-worker (or unhomed). Two distinct regimes, chosen
            // by local queue pressure on the sender's worker:
            //
            // - **Parallel load** (sender's inbox has pending items):
            //   the reactor / other producers are actively feeding the
            //   sender's worker; workers won't go idle soon and won't
            //   work-steal from the injector in time. Dispatch to the
            //   target's inbox + targeted unpark — matches the
            //   commit-3 fast-wake path for onecached-style workloads.
            //
            // - **Serial chain** (sender's inbox empty): nothing else
            //   queued on this worker; the sender is about to finish
            //   its slice and look for work. Push to the injector so
            //   the sender's worker work-steals `entry` in-place and
            //   keeps the chain cache-hot on a single worker. Fixes
            //   spawn_ring and hibernate ping-pong.
            //
            // This check is a fast atomic read on the SegQueue head —
            // cheap enough for the hot send-message path.
            let sender_busy = !self.worker_inboxes[current as usize].is_empty();
            if sender_busy {
                let target = if homed {
                    home as usize
                } else {
                    self.dispatch_rr
                        .fetch_add(1, Ordering::Relaxed) as usize
                        % self.num_workers
                };
                self.worker_inboxes[target].push(entry);
                if let Some(t) = self.worker_threads.get(target).and_then(|o| o.get()) {
                    t.unpark();
                }
            } else {
                self.injector.push(entry);
            }
            return;
        }

        // Non-worker caller: reactor / timer / FFI startup. Target the
        // process's home worker directly and unpark it; this is the
        // Phase 8 commit 2/3 fast-wake path for async dispatch.
        let target = if homed {
            home as usize
        } else {
            self.dispatch_rr
                .fetch_add(1, Ordering::Relaxed) as usize
                % self.num_workers
        };
        self.worker_inboxes[target].push(entry);
        if let Some(t) = self.worker_threads.get(target).and_then(|o| o.get()) {
            t.unpark();
        } else {
            let (ref _lock, ref cvar) = *self.worker_notify;
            cvar.notify_one();
        }
    }
}

/// Set the global scheduler handle. Called once during runtime init.
pub fn set_global_scheduler(handle: Arc<SchedulerHandle>) {
    let _ = GLOBAL_SCHEDULER.set(handle);
}

/// Get the global scheduler handle. Returns None if not yet initialized.
pub fn global_scheduler() -> Option<&'static Arc<SchedulerHandle>> {
    GLOBAL_SCHEDULER.get()
}

/// Shared process table type used by the scheduler and worker threads.
type ProcessTable = Arc<DashMap<ProcessId, Arc<ProcessEntry>>>;

// ---------------------------------------------------------------------------
// Worker instrumentation
// ---------------------------------------------------------------------------

/// Per-worker counters used for dispatch-overhead instrumentation.
///
/// Printed on shutdown when `NERA_SCHED_STATS=1`. The key ratio is
/// `pops / parks`: work items processed per actual condvar wait. A high
/// ratio (≫1) means parking amortizes across many dispatches; a low
/// ratio (~1) means every op pays the park/unpark cost.
///
/// Phase 7 addition (2026-04-15): `park_ns_total` measures the
/// cumulative wall time spent inside the `cvar.wait_for` call.
/// Combined with `parks`, this yields the **average park duration**
/// (`park_ns_total / parks`), which distinguishes two very different
/// bottleneck shapes that both show up as "60% of worker wall in
/// `ThreadParker`" in a samply profile:
///
/// - **Many short parks** → per-op dispatch overhead dominates.
///   Fix: reduce park *count* by amortising dispatch (worker-local
///   direct dispatch, spin-before-park on the post-slice path).
/// - **Few long parks** → OS cvar/schedule latency dominates.
///   Fix: reduce park *cost* via direct-handoff mechanisms (futex,
///   per-worker parker, etc.) or eliminate parks entirely.
///
/// Phase 8 addition (2026-04-15): `spin_rescues` counts pops that
/// succeeded after at least one failed pop — i.e., work became
/// visible to the worker during the spin-before-park retry loop
/// instead of requiring a full park. Compare against `parks` to
/// judge whether the current spin budget is catching enough work
/// to be net-positive. Kept after the Phase 8 extended-spin
/// experiment reverted (see plan doc) as general-purpose
/// instrumentation for future bounded-spin work.
#[repr(align(64))] // cache-line align to avoid false sharing between workers
pub struct WorkerStats {
    /// Successful process pops (local deque / injector / stealing).
    pub pops: AtomicU64,
    /// Condvar waits actually entered (idle_spins > 3 path).
    pub parks: AtomicU64,
    /// Idle iterations that used the spin path (idle_spins <= 3).
    pub spin_iters: AtomicU64,
    /// Cumulative nanoseconds spent inside `cvar.wait_for` across all
    /// park calls on this worker. Avg park duration = this / parks.
    pub park_ns_total: AtomicU64,
    /// Pops that came after one or more failed-pop iterations — i.e.,
    /// work arrived during the spin-before-park phase and the park
    /// was avoided. See Phase 8 note above.
    pub spin_rescues: AtomicU64,
}

impl WorkerStats {
    fn new() -> Self {
        Self {
            pops: AtomicU64::new(0),
            parks: AtomicU64::new(0),
            spin_iters: AtomicU64::new(0),
            park_ns_total: AtomicU64::new(0),
            spin_rescues: AtomicU64::new(0),
        }
    }
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

pub struct Scheduler {
    num_threads: usize,
    injector: Arc<Injector<Arc<ProcessEntry>>>,
    process_table: ProcessTable,
    next_pid: Arc<crate::process::ProcessIdAllocator>,
    shutdown: Arc<AtomicBool>,
    runnable_count: Arc<AtomicI64>,
    /// Count of alive (non-exited) processes for fast termination check.
    alive_count: Arc<AtomicI64>,
    /// Set to true if the initial (main) process exited with an uncaught exception.
    any_crash: Arc<AtomicBool>,
    /// PID of the initial (main) process. Set on first `spawn_process` call.
    initial_pid: Arc<crate::process::AtomicProcessId>,
    /// Registered process names: atom_id -> pid.
    registered_names: Arc<RwLock<HashMap<u32, ProcessId>>>,
    /// Timer wheel for efficient receive...after timeout management.
    timer_wheel: Arc<TimerWheel>,
    /// Condvar to wake idle worker threads when new work is available.
    worker_notify: Arc<(Mutex<()>, Condvar)>,
    /// I/O thread pool for async blocking I/O operations.
    io_pool: Arc<IoPool>,
    /// Event-driven I/O reactor handle (accessed via global_reactor()).
    #[cfg(mio_supported)]
    #[allow(dead_code)]
    reactor: Option<ReactorHandle>,
    /// Per-worker dispatch-overhead counters. One entry per worker thread,
    /// printed on shutdown when `NERA_SCHED_STATS=1`.
    worker_stats: Vec<Arc<WorkerStats>>,
    /// Phase 8: per-worker inboxes for direct dispatch. See
    /// `docs/PHASE8_WORKER_LOCAL_DISPATCH_PLAN.md` for the full design.
    worker_inboxes: Arc<Vec<Arc<SegQueue<Arc<ProcessEntry>>>>>,
}

impl Scheduler {
    /// Create a new scheduler that will use `num_threads` worker threads.
    pub fn new(num_threads: usize) -> Self {
        let n = num_threads.max(1);
        let worker_stats = (0..n).map(|_| Arc::new(WorkerStats::new())).collect();
        let worker_inboxes = Arc::new(
            (0..n).map(|_| Arc::new(SegQueue::new())).collect::<Vec<_>>(),
        );
        Scheduler {
            num_threads: n,
            injector: Arc::new(Injector::new()),
            process_table: Arc::new(DashMap::new()),
            next_pid: Arc::new(crate::process::ProcessIdAllocator::new(n)),
            shutdown: Arc::new(AtomicBool::new(false)),
            runnable_count: Arc::new(AtomicI64::new(0)),
            alive_count: Arc::new(AtomicI64::new(0)),
            any_crash: Arc::new(AtomicBool::new(false)),
            initial_pid: Arc::new(crate::process::AtomicProcessId::new(ProcessId::new(0))),
            registered_names: Arc::new(RwLock::new(HashMap::new())),
            timer_wheel: Arc::new(TimerWheel::new()),
            worker_notify: Arc::new((Mutex::new(()), Condvar::new())),
            // On mio platforms TCP I/O uses the reactor; pool is only for file I/O.
            #[cfg(mio_supported)]
            io_pool: Arc::new(IoPool::new(2)),
            #[cfg(not(mio_supported))]
            io_pool: Arc::new(IoPool::new(n.min(4))),
            #[cfg(mio_supported)]
            reactor: None,
            worker_stats,
            worker_inboxes,
        }
    }

    /// Allocate the next unique PID.
    pub fn next_pid(&self) -> ProcessId {
        self.next_pid.next()
    }

    #[inline]
    pub fn try_next_pid(&self) -> Option<ProcessId> { self.next_pid.try_next() }


    /// Spawn a new process and push it into the global injector queue.
    /// Returns the process's PID.
    pub fn spawn_process(&self, process: Process) -> ProcessId {
        let pid = process.id;
        // Capture the initial (main) process PID on first spawn.
        let _ = self
            .initial_pid
            .compare_exchange(ProcessId::new(0), pid, Ordering::SeqCst, Ordering::SeqCst);
        let arc = Arc::new(ProcessEntry::new(process));
        self.process_table.insert(pid, Arc::clone(&arc));
        self.alive_count.fetch_add(1, Ordering::AcqRel);
        self.runnable_count.fetch_add(1, Ordering::AcqRel);
        self.injector.push(arc);
        // Phase 8 commit 2: best-effort targeted unpark via the global
        // handle. If the global is stale (e.g. between tests), the
        // unpark is wasted but harmless — self.injector is authoritative
        // and self's workers will pick the item up within the 2ms
        // park_timeout. Cannot safely push into the global handle's
        // worker_inboxes because they belong to a different scheduler.
        if let Some(sched) = global_scheduler() {
            let target = sched.dispatch_rr.fetch_add(1, Ordering::Relaxed) as usize
                % sched.num_workers.max(1);
            if let Some(t) = sched.worker_threads.get(target).and_then(|o| o.get()) {
                t.unpark();
            }
        }
        pid
    }

    /// Look up a process by PID.
    pub fn get_process(&self, pid: ProcessId) -> Option<Arc<ProcessEntry>> {
        self.process_table.get(&pid).map(|r| r.clone())
    }

    /// Signal all worker threads to shut down.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// Run the scheduler. Blocks until all processes have exited (or
    /// entered a waiting state with no ready processes) or until
    /// `shutdown()` is called.
    ///
    /// Returns `true` if all processes exited normally, `false` if any
    /// process crashed with an uncaught exception.
    /// # Safety
    /// All scheduled processes, including those added during execution, must
    /// contain valid process-owned roots. Their compiled functions and Rust
    /// callbacks must preserve the runtime context/heap invariants throughout
    /// execution. No outside code may invalidate stored graphs or their owners.
    /// ```compile_fail,E0133
    /// use nera_runtime::scheduler::Scheduler;
    /// Scheduler::new(1).run();
    /// ```
    pub unsafe fn run(&self) -> bool {
        // Spawn the mio reactor thread before registering the global handle.
        #[cfg(mio_supported)]
        let (reactor_handle, reactor_join) = crate::reactor::spawn_reactor();

        // Each run owns its thread registry, even when a previous test has
        // already initialized the global OnceLock.
        let worker_threads = Arc::new(
            (0..self.num_threads).map(|_| OnceLock::new()).collect::<Vec<_>>(),
        );

        // Register the global scheduler handle so FFI functions can access it.
        set_global_scheduler(Arc::new(SchedulerHandle {
            injector: Arc::clone(&self.injector),
            process_table: Arc::clone(&self.process_table),
            next_pid: Arc::clone(&self.next_pid),
            runnable_count: Arc::clone(&self.runnable_count),
            alive_count: Arc::clone(&self.alive_count),
            registered_names: Arc::clone(&self.registered_names),
            timer_wheel: Arc::clone(&self.timer_wheel),
            worker_notify: Arc::clone(&self.worker_notify),
            io_pool: Arc::clone(&self.io_pool),
            #[cfg(mio_supported)]
            reactor: reactor_handle.clone(),
            worker_inboxes: Arc::clone(&self.worker_inboxes),
            num_workers: self.num_threads,
            worker_threads: Arc::clone(&worker_threads),
            dispatch_rr: AtomicU32::new(0),
        }));
        let mut workers: Vec<Worker<Arc<ProcessEntry>>> = Vec::new();
        let mut stealers: Vec<Stealer<Arc<ProcessEntry>>> = Vec::new();

        for _ in 0..self.num_threads {
            let w = Worker::new_fifo();
            stealers.push(w.stealer());
            workers.push(w);
        }

        let stealers = Arc::new(stealers);

        // I/O pool workers are spawned lazily on first submit() call.

        // Dispatch-overhead instrumentation: if NERA_SCHED_STATS=1, spawn a
        // background thread that prints per-second aggregate scheduler
        // counters (pops, parks, spin_iters, pops/park ratio, avg park
        // duration). Used for the Phase 5 park/unpark audit and the
        // Phase 7 dispatch-latency decomposition. The last line before
        // the harness terminates the server gives the steady-state
        // reading.
        let stats_handle = if std::env::var("NERA_SCHED_STATS").ok().as_deref() == Some("1") {
            let stats = self.worker_stats.clone();
            let shutdown = Arc::clone(&self.shutdown);
            let runnable = Arc::clone(&self.runnable_count);
            let alive = Arc::clone(&self.alive_count);
            let timers = Arc::clone(&self.timer_wheel);
            let processes = Arc::clone(&self.process_table);
            Some(thread::Builder::new()
                .name("nera-sched-stats".into())
                .spawn(move || {
                    let mut prev_pops: u64 = 0;
                    let mut prev_parks: u64 = 0;
                    let mut prev_spin: u64 = 0;
                    let mut prev_park_ns: u64 = 0;
                    let mut prev_rescues: u64 = 0;
                    eprintln!(
                        "[sched-stats] period=1s; columns: pops/s parks/s spin_iters/s rescues/s pops_per_park avg_park_us park_wall_pct"
                    );
                    loop {
                        thread::sleep(Duration::from_secs(1));
                        if shutdown.load(Ordering::Acquire) {
                            break;
                        }
                        let mut pops = 0u64;
                        let mut parks = 0u64;
                        let mut spin = 0u64;
                        let mut park_ns = 0u64;
                        let mut rescues = 0u64;
                        for s in stats.iter() {
                            pops += s.pops.load(Ordering::Relaxed);
                            parks += s.parks.load(Ordering::Relaxed);
                            spin += s.spin_iters.load(Ordering::Relaxed);
                            park_ns += s.park_ns_total.load(Ordering::Relaxed);
                            rescues += s.spin_rescues.load(Ordering::Relaxed);
                        }
                        let dp = pops.saturating_sub(prev_pops);
                        let dk = parks.saturating_sub(prev_parks);
                        let ds = spin.saturating_sub(prev_spin);
                        let dns = park_ns.saturating_sub(prev_park_ns);
                        let dr = rescues.saturating_sub(prev_rescues);
                        let ratio = if dk > 0 { dp as f64 / dk as f64 } else { f64::INFINITY };
                        // Average park duration in µs (per-park, not per-worker).
                        let avg_park_us = if dk > 0 { (dns as f64 / dk as f64) / 1000.0 } else { 0.0 };
                        // Fraction of aggregate worker wall spent parked over
                        // the last second. Denominator is (num_workers * 1s)
                        // expressed in ns.
                        let num_workers = stats.len() as u64;
                        let wall_ns = num_workers * 1_000_000_000;
                        let park_pct = if wall_ns > 0 { 100.0 * dns as f64 / wall_ns as f64 } else { 0.0 };
                        eprintln!(
                            "[sched-stats] pops={:>8} parks={:>8} spin={:>8} rescues={:>8} pops/park={:>8.2} avg_park_us={:>8.1} park_wall_pct={:>5.1}",
                            dp, dk, ds, dr, ratio, avg_park_us, park_pct,
                        );
                        eprintln!("[sched-state] runnable={} alive={} timers_pending={} next_deadline={:?}",
                            runnable.load(Ordering::Acquire), alive.load(Ordering::Acquire),
                            timers.has_pending_timers(), timers.next_deadline());
                        for entry in processes.iter() {
                            if let Some(proc) = entry.try_lock_for_runtime() {
                                eprintln!("[sched-process] pid={} status={:?} atomic={} wait={:?} deadline={} inbox_empty={} mailbox_empty={}",
                                    proc.id, proc.status(), entry.published_status(),
                                    proc.wait_reason(), proc.context.receive_timeout_deadline,
                                    entry.inbox.is_empty(), proc.mailbox.is_empty());
                            }
                        }
                        prev_pops = pops;
                        prev_parks = parks;
                        prev_spin = spin;
                        prev_park_ns = park_ns;
                        prev_rescues = rescues;
                    }
                })
                .expect("failed to spawn stats thread"))
        } else {
            None
        };

        // Spawn the timer thread before workers.
        let timer_handle = {
            let timer_wheel = Arc::clone(&self.timer_wheel);
            let process_table = Arc::clone(&self.process_table);
            let injector = Arc::clone(&self.injector);
            let runnable_count = Arc::clone(&self.runnable_count);
            let shutdown = Arc::clone(&self.shutdown);
            thread::Builder::new()
                .name("nera-timer".into())
                .spawn(move || {
                    timer_thread_loop(
                        timer_wheel,
                        process_table,
                        injector,
                        runnable_count,
                        shutdown,
                    );
                })
                .expect("failed to spawn timer thread")
        };

        let mut handles = Vec::new();

        for (worker_id, worker) in workers.into_iter().enumerate() {
            let injector = Arc::clone(&self.injector);
            let stealers = Arc::clone(&stealers);
            let process_table = Arc::clone(&self.process_table);
            let shutdown = Arc::clone(&self.shutdown);
            let runnable_count = Arc::clone(&self.runnable_count);
            let alive_count = Arc::clone(&self.alive_count);
            let any_crash = Arc::clone(&self.any_crash);
            let initial_pid = Arc::clone(&self.initial_pid);
            let num_threads = self.num_threads;
            let registered_names = Arc::clone(&self.registered_names);
            let timer_wheel = Arc::clone(&self.timer_wheel);
            let io_pool = Arc::clone(&self.io_pool);
            let stats = Arc::clone(&self.worker_stats[worker_id]);
            let inbox = Arc::clone(&self.worker_inboxes[worker_id]);
            let run_threads = Arc::clone(&worker_threads);
            #[cfg(mio_supported)]
            let reactor = reactor_handle.clone();

            let handle = thread::Builder::new()
                .name(format!("nera-worker-{}", worker_id))
                .spawn(move || {
                    worker_loop(
                        worker_id,
                        worker,
                        injector,
                        stealers,
                        process_table,
                        shutdown,
                        runnable_count,
                        alive_count,
                        any_crash,
                        initial_pid,
                        num_threads,
                        registered_names,
                        timer_wheel,
                        io_pool,
                        stats,
                        inbox,
                        run_threads,
                        thread::park_timeout,
                        #[cfg(mio_supported)]
                        reactor,
                    );
                })
                .expect("failed to spawn worker thread");

            // Registration may follow an early shutdown. A late worker sees
            // the flag on entry, so it cannot miss a wake and go to sleep.
            let _ = worker_threads[worker_id].set(handle.thread().clone());

            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.join();
        }

        // Shut down the timer thread, I/O pool, and reactor.
        self.shutdown.store(true, Ordering::SeqCst);
        self.timer_wheel.notify();
        #[cfg(mio_supported)]
        reactor_handle.shutdown();
        self.io_pool.shutdown();
        // Close all remaining sockets to unblock I/O pool threads that
        // are stuck in blocking syscalls (accept, recv, etc.).
        crate::socket::shutdown_all();
        let _ = timer_handle.join();
        #[cfg(mio_supported)]
        let _ = reactor_join.join();
        self.io_pool.join();

        if let Some(h) = stats_handle {
            let _ = h.join();
        }

        !self.any_crash.load(Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------
// EXIT / DOWN signal propagation
// ---------------------------------------------------------------------------

/// Propagate EXIT signals to linked processes and DOWN signals to monitoring
/// processes when a process exits. Called from the 3 exit points in
/// `worker_loop` AFTER dropping the dying process's MutexGuard but BEFORE
/// removing it from the process table (so its heap stays alive for deep-copy).
///
/// Deadlock safety: the dying process's lock is NOT held. Each linked/monitoring
/// process is locked individually, and the lock is dropped before any recursive
/// propagation.
#[expect(clippy::too_many_arguments, reason = "Explicit scheduler and GC state avoids changing runtime ownership and locking boundaries")]
fn propagate_exit_signals(
    dead_pid: ProcessId,
    exit_reason: crate::term::ErlTerm,
    links: HashSet<ProcessId>,
    monitors_incoming: HashMap<MonitorRef, ProcessId>,
    process_table: &ProcessTable,
    injector: &Arc<Injector<Arc<ProcessEntry>>>,
    runnable_count: &Arc<AtomicI64>,
    alive_count: &Arc<AtomicI64>,
    any_crash: &Arc<AtomicBool>,
    initial_pid: &Arc<crate::process::AtomicProcessId>,
    registered_names: &Arc<RwLock<HashMap<u32, ProcessId>>>,
    timer_wheel: &Arc<TimerWheel>,
    io_pool: &Arc<IoPool>,
    #[cfg(mio_supported)] reactor: &ReactorHandle,
) {
    use crate::atom_table::*;
    use crate::heap::FragmentHeap;
    use crate::message::{FragmentElem, MessageFragment};
    use crate::term::{
        BoxedHeader, ErlTerm, REFERENCE_ID_WORD_INDEX, REFERENCE_TOTAL_WORDS,
    };

    let is_kill = exit_reason.as_atom_id() == Some(ATOM_KILL);
    let is_normal = exit_reason.as_atom_id() == Some(ATOM_NORMAL);
    let dead_pid_term = ErlTerm::pid(dead_pid);

    // --- Process links ---
    for linked_pid in &links {
        let Some(entry) = process_table.get(linked_pid).map(|r| r.clone()) else {
            continue;
        };
        let mut proc = entry.lock_for_runtime();

        // Remove the dead process from the linked process's link set
        proc.otp.links.remove(&dead_pid);

        if proc.status() == ProcessStatus::Exited {
            continue;
        }

        if proc.otp.trap_exit {
            // Deliver {'EXIT', DeadPid, Reason} as a message via the lock-free
            // inbox to preserve FIFO ordering with regular send_message calls.
            let exit_atom = ErlTerm::atom(ATOM_EXIT_SIGNAL);
            let reason_atom = if is_kill {
                ErlTerm::atom(ATOM_KILLED)
            } else {
                exit_reason
            };
            // Fast path: {'EXIT', Pid, Reason} — all 3 elements are usually
            // immediates (atom, pid, atom). Use SimpleTuple to avoid allocating
            // a 2 KiB ProcessHeap for each EXIT signal.
            let fragment = if dead_pid_term.is_immediate() && reason_atom.is_immediate() {
                MessageFragment::SimpleTuple(vec![
                    FragmentElem::Immediate(crate::message::ImmediateTerm::new(exit_atom)),
                    FragmentElem::Immediate(crate::message::ImmediateTerm::new(dead_pid_term)),
                    FragmentElem::Immediate(crate::message::ImmediateTerm::new(reason_atom)),
                ])
            } else {
                let mut frag_heap = FragmentHeap::new_with_capacity(64);
                // SAFETY: the dying process remains in the table, exited and unmoving,
                // until signal propagation completes; the PID is immediate.
                let pid_copy = unsafe { frag_heap.copy_raw_term(dead_pid_term) };
                let reason_copy = unsafe { frag_heap.copy_raw_term(reason_atom) };
                let msg = frag_heap.alloc_tuple(&[exit_atom, pid_copy, reason_copy]);
                // SAFETY: this root was built/copied entirely into the transferred heap.
                unsafe { crate::message::MessageFragment::from_owned_heap(frag_heap, msg) }
            };
            drop(proc);

            entry.inbox.push(fragment);
            entry.has_exit_signals.store(true, Ordering::Release);
            // Try waking the process. try_wake() handles receive-waiting,
            // try_wake_io() handles IO-waiting. Only one CAS can succeed.
            // The worker that runs the process cancels its receive timer;
            // see nera_rt_send_message.
            if entry.try_wake() || entry.try_wake_io() {
                runnable_count.fetch_add(1, Ordering::AcqRel);
                injector.push(entry);
                // Best-effort unpark via global handle; if stale, workers
                // pick up the injector item on the 2ms park_timeout.
                if let Some(sched) = global_scheduler() {
                    for t in sched.worker_threads.iter() {
                        if let Some(t) = t.get() {
                            t.unpark();
                            break;
                        }
                    }
                }
            }
        } else if is_kill || !is_normal {
            // Kill the linked process: either reason is 'kill' (cascade as 'killed')
            // or reason is non-normal (cascade with same reason)
            let cascade_reason = if is_kill {
                ErlTerm::atom(ATOM_KILLED)
            } else {
                exit_reason
            };

            // Only decrement runnable_count if the process was actually
            // runnable. Waiting processes were already decremented when
            // they entered the wait state.
            let was_runnable = matches!(proc.status(), ProcessStatus::Ready | ProcessStatus::Running);

            proc.exit_reason = Some(cascade_reason);
            proc.transition(ProcessState::Exited);

            // Extract this process's links/monitors for recursive propagation
            let their_links = std::mem::take(&mut proc.otp.links);
            let their_monitors = std::mem::take(&mut proc.otp.monitors_incoming);
            let their_pid = *linked_pid;

            drop(proc);

            // Remove from process table and propagate recursively
            if was_runnable {
                runnable_count.fetch_sub(1, Ordering::AcqRel);
            }
            // Set any_crash if cascade is killing the initial (main) process
            // with a non-normal reason. Other cascade kills don't affect exit code.
            if *linked_pid == initial_pid.load(Ordering::SeqCst) {
                any_crash.store(true, Ordering::SeqCst);
            }

            propagate_exit_signals(
                their_pid,
                cascade_reason,
                their_links,
                their_monitors,
                process_table,
                injector,
                runnable_count,
                alive_count,
                any_crash,
                initial_pid,
                registered_names,
                timer_wheel,
                io_pool,
                #[cfg(mio_supported)]
                reactor,
            );

            timer_wheel.remove_for_pid(their_pid);
            io_pool.cancel_for_pid(their_pid);
            #[cfg(mio_supported)]
            reactor.cancel_for_pid(their_pid);
            crate::socket::remove_all_for_pid(their_pid);
            unregister_dead_process(their_pid, registered_names);
            crate::ets::delete_tables_owned_by(their_pid);
            alive_count.fetch_sub(1, Ordering::AcqRel);
            process_table.remove(&their_pid);
        }
        // else: normal exit + no trap_exit = do nothing (link already removed)
    }

    // --- Process monitors (incoming: others monitor this process) ---
    for (mon_ref, owner_pid) in &monitors_incoming {
        let Some(entry) = process_table.get(owner_pid).map(|r| r.clone()) else {
            continue;
        };
        let mut proc = entry.lock_for_runtime();

        // Remove from the owner's outgoing monitors. When demonitor/1,2 has
        // already removed it, the owner must get no 'DOWN', as on BEAM: the
        // owner may have flushed its mailbox and moved on.
        if proc.otp.monitors_outgoing.remove(mon_ref).is_none() {
            continue;
        }

        if proc.status() == ProcessStatus::Exited {
            continue;
        }

        // Deliver {'DOWN', Ref, process, DeadPid, Reason} via lock-free inbox
        // to preserve FIFO ordering with regular send_message calls.
        let down_atom = ErlTerm::atom(ATOM_DOWN);
        let process_atom = ErlTerm::atom(ATOM_PROCESS);
        let mut frag_heap = FragmentHeap::new_with_capacity(16);
        // SAFETY: the dying process remains in the table, exited and unmoving,
        // until signal propagation completes; the PID is immediate.
        let pid_copy = unsafe { frag_heap.copy_raw_term(dead_pid_term) };
        let reason_copy = unsafe { frag_heap.copy_raw_term(exit_reason) };

        // Allocate a reference term for the monitor ref on the fragment heap
        let ref_term = {
            let ptr = frag_heap.alloc_words(REFERENCE_TOTAL_WORDS);
            unsafe {
                *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Reference, 0).0;
                *ptr.add(REFERENCE_ID_WORD_INDEX) = *mon_ref;
            }
            // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
            unsafe { ErlTerm::boxed_unchecked(ptr as *const u64) }
        };

        let msg =
            frag_heap.alloc_tuple(&[down_atom, ref_term, process_atom, pid_copy, reason_copy]);
        // SAFETY: this root was built/copied entirely into the transferred heap.
        let fragment = unsafe { crate::message::MessageFragment::from_owned_heap(frag_heap, msg) };
        // Push while the owner is locked: a running owner holds its own lock,
        // so it cannot demonitor between our removal above and this push and
        // then miss the 'DOWN' when it flushes its mailbox.
        entry.inbox.push(fragment);
        drop(proc);

        // The worker cancels the owner's receive timer, as for a link.
        if entry.try_wake() {
            runnable_count.fetch_add(1, Ordering::AcqRel);
            injector.push(entry);
            if let Some(sched) = global_scheduler() {
                for t in sched.worker_threads.iter() {
                    if let Some(t) = t.get() {
                        t.unpark();
                        break;
                    }
                }
            }
        }
    }
}

/// Remove a process's registered name (if any) from the global registry.
/// Called when a process exits, before removing it from the process table.
fn unregister_dead_process(
    pid: ProcessId,
    registered_names: &Arc<RwLock<HashMap<u32, ProcessId>>>,
) {
    let mut names = registered_names.write();
    names.retain(|_name, &mut p| p != pid);
}

/// Shared cleanup for all process-death paths. Propagates EXIT/DOWN signals,
/// cancels timers and I/O, cleans up sockets, unregisters names, deletes
/// owned ETS tables, and removes the process from the process table.
#[expect(clippy::too_many_arguments, reason = "Explicit scheduler and GC state avoids changing runtime ownership and locking boundaries")]
fn cleanup_dead_process(
    pid: ProcessId,
    exit_reason: crate::term::ErlTerm,
    links: HashSet<ProcessId>,
    monitors_in: HashMap<MonitorRef, ProcessId>,
    process_table: &ProcessTable,
    injector: &Arc<Injector<Arc<ProcessEntry>>>,
    runnable_count: &Arc<AtomicI64>,
    alive_count: &Arc<AtomicI64>,
    any_crash: &Arc<AtomicBool>,
    initial_pid: &Arc<crate::process::AtomicProcessId>,
    registered_names: &Arc<RwLock<HashMap<u32, ProcessId>>>,
    timer_wheel: &Arc<TimerWheel>,
    io_pool: &Arc<IoPool>,
    #[cfg(mio_supported)] reactor: &ReactorHandle,
) {
    propagate_exit_signals(
        pid,
        exit_reason,
        links,
        monitors_in,
        process_table,
        injector,
        runnable_count,
        alive_count,
        any_crash,
        initial_pid,
        registered_names,
        timer_wheel,
        io_pool,
        #[cfg(mio_supported)]
        reactor,
    );
    timer_wheel.remove_for_pid(pid);
    io_pool.cancel_for_pid(pid);
    #[cfg(mio_supported)]
    reactor.cancel_for_pid(pid);
    crate::socket::remove_all_for_pid(pid);
    unregister_dead_process(pid, registered_names);
    crate::ets::delete_tables_owned_by(pid);
    alive_count.fetch_sub(1, Ordering::AcqRel);
    process_table.remove(&pid);
}

// ---------------------------------------------------------------------------
// Timer thread
// ---------------------------------------------------------------------------

fn timer_thread_loop(
    timer_wheel: Arc<TimerWheel>,
    process_table: ProcessTable,
    injector: Arc<Injector<Arc<ProcessEntry>>>,
    runnable_count: Arc<AtomicI64>,
    shutdown: Arc<AtomicBool>,
) {
    use crate::timer::current_time_ms;

    // Inserts and shutdown wake the timer thread; this bound is a fallback.
    const IDLE_WAIT: Duration = Duration::from_millis(100);

    loop {
        if shutdown.load(Ordering::SeqCst) {
            return;
        }

        timer_wheel.wait_for_next_deadline(IDLE_WAIT, &shutdown);

        if shutdown.load(Ordering::SeqCst) {
            return;
        }

        // Fire all expired timers.
        let now = current_time_ms();
        let expired = timer_wheel.pop_expired(now);
        let mut woke_count = expired.len();

        for entry in expired {
            wake_process_for_timer(entry.owner_pid, &process_table, &injector, &runnable_count);
        }
        woke_count += timer_wheel.fire_due_message_timers(|delivery| {
            deliver_timer_message(delivery, &process_table, &injector, &runnable_count)
        });

        // Wake idle workers if we made processes runnable.
        // Phase 8: unpark all worker threads directly instead of the
        // condvar broadcast. Workers now park via thread::park_timeout
        // and only respond to Thread::unpark, not condvar notify.
        if woke_count > 0 {
            if let Some(sched) = global_scheduler() {
                for t in sched.worker_threads.iter() {
                    if let Some(t) = t.get() {
                        t.unpark();
                    }
                }
            }
        }
    }
}

/// Publish a fired `send_after`/`start_timer` message. Returns true when it
/// woke its receiver; the caller unparks workers after the batch.
fn deliver_timer_message(
    delivery: crate::timer::MessageDelivery,
    process_table: &ProcessTable,
    injector: &Arc<Injector<Arc<ProcessEntry>>>,
    runnable_count: &Arc<AtomicI64>,
) -> bool {
    let Some(entry) = process_table.get(&delivery.dest).map(|r| r.clone()) else {
        return false;
    };
    entry.inbox.push(delivery.message);
    // The worker that runs the receiver cancels its receive timer; see
    // nera_rt_send_message.
    if !entry.try_wake() {
        return false;
    }
    runnable_count.fetch_add(1, Ordering::AcqRel);
    injector.push(entry);
    true
}

fn wake_process_for_timer(
    pid: ProcessId,
    process_table: &ProcessTable,
    injector: &Arc<Injector<Arc<ProcessEntry>>>,
    runnable_count: &Arc<AtomicI64>,
) {
    let entry = process_table.get(&pid).map(|r| r.clone());
    if let Some(entry) = entry {
        // Lock-free wake: CAS Waiting → Ready. No need to lock the Mutex.
        if entry.try_wake() {
            runnable_count.fetch_add(1, Ordering::AcqRel);
            injector.push(entry);
            // Caller (timer_thread_loop) broadcasts unpark to all worker
            // threads after processing the expired batch.
        }
    }
}

/// The two moments `publish_wait_with` lets a test act in: under the
/// process lock before Waiting is published, and after the lock is released.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaitWindow {
    BeforePublish,
    AfterPublish,
}

/// Publish `state` for a process leaving its slice to wait, releasing its
/// runnable slot. Returns true when the wait already has a wake condition:
/// the process is Ready again, owns a new slot, and the caller queues it.
fn publish_wait(
    proc: RuntimeProcessGuard<'_>,
    entry: &ProcessEntry,
    state: ProcessState,
    timer_wheel: &TimerWheel,
    runnable_count: &AtomicI64,
) -> bool {
    publish_wait_with(proc, entry, state, timer_wheel, runnable_count, false, |_| {})
}

/// Return a woken process that found nothing to do to the wait it was in.
/// Its timer, if still pending, already belongs to that wait and is kept:
/// registering it again would leave one more heap entry per such wake
/// until the deadline.
fn rewait(
    proc: RuntimeProcessGuard<'_>,
    entry: &ProcessEntry,
    state: ProcessState,
    timer_wheel: &TimerWheel,
    runnable_count: &AtomicI64,
) -> bool {
    publish_wait_with(proc, entry, state, timer_wheel, runnable_count, true, |_| {})
}

fn publish_wait_with(
    mut proc: RuntimeProcessGuard<'_>,
    entry: &ProcessEntry,
    state: ProcessState,
    timer_wheel: &TimerWheel,
    runnable_count: &AtomicI64,
    same_wait: bool,
    mut window: impl FnMut(WaitWindow),
) -> bool {
    let pid = proc.id;
    let deadline = match state {
        ProcessState::Waiting(WaitReason::Receive { timeout_deadline_ms }) => timeout_deadline_ms,
        _ => None,
    };
    // Register the timer while the process is still locked. Once Waiting is
    // published, a message can wake the process and another worker can run
    // it into a later timed receive; a registration made after that would
    // replace the later timer with this stale one, and the later receive
    // would wait with no timer at all.
    // A timer that fires between the pending check and the publish is
    // caught by the race check below.
    if let Some(deadline) = deadline {
        if !(same_wait && timer_wheel.has_pending_for(pid)) {
            timer_wheel.insert(deadline, pid);
        }
    }
    window(WaitWindow::BeforePublish);
    proc.transition(state);
    runnable_count.fetch_sub(1, Ordering::AcqRel);
    drop(proc);
    window(WaitWindow::AfterPublish);

    // CRITICAL RACE CHECK: a message may have reached the lock-free inbox,
    // or the timer thread may have taken this receive's expired timer, while
    // the process was not yet published Waiting. Either wake CAS then failed
    // on the earlier state. The timer thread clears its map entry before its
    // CAS, so a timer missing here has already fired or will see Waiting.
    let wake = !entry.inbox.is_empty()
        || (deadline.is_some() && !timer_wheel.has_pending_for(pid));
    if wake && entry.try_wake() {
        runnable_count.fetch_add(1, Ordering::AcqRel);
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Worker loop
// ---------------------------------------------------------------------------

fn drain_worker_inbox(inbox: &SegQueue<Arc<ProcessEntry>>, local: &Worker<Arc<ProcessEntry>>) {
    // An inbox can be replenished concurrently. Never let dispatch traffic
    // prevent the worker from reaching an actual process time slice.
    for _ in 0..64 {
        let Some(entry) = inbox.pop() else {
            break;
        };
        local.push(entry);
    }
}

fn stop_workers(
    shutdown: &AtomicBool,
    timer_wheel: &TimerWheel,
    io_pool: &IoPool,
    worker_threads: &[OnceLock<thread::Thread>],
) {
    if !shutdown.swap(true, Ordering::SeqCst) {
        timer_wheel.notify();
        io_pool.shutdown();
        crate::socket::shutdown_all();
        for slot in worker_threads {
            if let Some(worker) = slot.get() {
                worker.unpark();
            }
        }
    }
}

#[expect(clippy::too_many_arguments, reason = "Explicit scheduler and GC state avoids changing runtime ownership and locking boundaries")]
fn worker_loop(
    worker_id: usize,
    local: Worker<Arc<ProcessEntry>>,
    injector: Arc<Injector<Arc<ProcessEntry>>>,
    stealers: Arc<Vec<Stealer<Arc<ProcessEntry>>>>,
    process_table: ProcessTable,
    shutdown: Arc<AtomicBool>,
    runnable_count: Arc<AtomicI64>,
    alive_count: Arc<AtomicI64>,
    any_crash: Arc<AtomicBool>,
    initial_pid: Arc<crate::process::AtomicProcessId>,
    num_threads: usize,
    registered_names: Arc<RwLock<HashMap<u32, ProcessId>>>,
    timer_wheel: Arc<TimerWheel>,
    io_pool: Arc<IoPool>,
    stats: Arc<WorkerStats>,
    inbox: Arc<SegQueue<Arc<ProcessEntry>>>,
    worker_threads: Arc<Vec<OnceLock<thread::Thread>>>,
    park: impl Fn(Duration),
    #[cfg(mio_supported)] reactor: ReactorHandle,
) {
    // Phase 8 commit 4: identify this thread as a worker so FFI
    // send_message paths can choose between same-worker inbox push and
    // cross-worker injector push (work-stealing).
    CURRENT_WORKER.with(|c| c.set(worker_id as u8));

    let mut idle_spins: u32 = 0;
    let mut dispatches_since_global_poll = 0;

    loop {
        if shutdown.load(Ordering::SeqCst) {
            return;
        }
        if alive_count.load(Ordering::Acquire) <= 0 {
            stop_workers(&shutdown, &timer_wheel, &io_pool, &worker_threads);
            return;
        }

        // Phase 8: drain per-worker inbox into local deque. Producers
        // (commit 2: reactor dispatch; commit 3: send_message/spawn)
        // push directly here. Draining first ensures any items pushed
        // between our last park and this point start entering the local queue.
        // Bounded batches ensure a busy inbox cannot prevent execution.
        drain_worker_inbox(&inbox, &local);

        // Local yields can keep this deque nonempty indefinitely. Periodically
        // prefer one global item, rather than consulting the injector only when
        // local work runs out. Keep intervening local dispatches for locality
        // and to prevent a busy injector from starving local processes.
        let global = if dispatches_since_global_poll >= 16 {
            dispatches_since_global_poll = 0;
            match injector.steal() {
                Steal::Success(p) => Some(p),
                _ => None,
            }
        } else {
            None
        };
        let maybe_proc = global
            .or_else(|| local.pop())
            .or_else(|| {
                // 2. Try injector (steal batch for better throughput).
                match injector.steal_batch_and_pop(&local) {
                    Steal::Success(p) => Some(p),
                    _ => None,
                }
            })
            .or_else(|| {
                // 3. Try stealing from a random peer.
                if num_threads <= 1 {
                    return None;
                }
                // Simple round-robin starting from a pseudo-random offset.
                let start =
                    (worker_id.wrapping_mul(7).wrapping_add(idle_spins as usize)) % num_threads;
                for offset in 0..num_threads {
                    let idx = (start + offset) % num_threads;
                    if idx == worker_id {
                        continue;
                    }
                    match stealers[idx].steal() {
                        Steal::Success(p) => return Some(p),
                        _ => continue,
                    }
                }
                None
            });

        let proc_entry = match maybe_proc {
            Some(p) => {
                dispatches_since_global_poll += 1;
                stats.pops.fetch_add(1, Ordering::Relaxed);
                if idle_spins > 0 {
                    // This pop followed at least one failed retry —
                    // the spin-before-park loop caught work before
                    // needing to park. `idle_spins` itself is reset
                    // further down (~line 1037), after the Exited /
                    // Waiting checks; we deliberately read it here
                    // so that corner-case Exited/Waiting pops still
                    // count as rescues if they came from a spin.
                    stats.spin_rescues.fetch_add(1, Ordering::Relaxed);
                }
                p
            }
            None => {
                idle_spins += 1;
                // Spin briefly before sleeping on condvar to avoid OS thread
                // wakeup latency (~10-100µs) on the p99.9 path. If work
                // arrives during the spin, we pick it up immediately.
                if idle_spins <= 3 {
                    stats.spin_iters.fetch_add(1, Ordering::Relaxed);
                    for _ in 0..64 {
                        std::hint::spin_loop();
                    }
                    // Re-check queues before sleeping.
                    continue;
                }
                // Phase 8: park via thread::park_timeout instead of the
                // shared condvar. This responds to Thread::unpark() from
                // dispatch_to_worker (targeted wake), which the condvar
                // cannot. The 2ms timeout is the same fallback as before
                // — it bounds termination-detection latency and catches
                // any work pushed via the legacy injector path (timer,
                // spawn, FFI send_message until commit 3 migrates those).
                stats.parks.fetch_add(1, Ordering::Relaxed);
                let park_start = std::time::Instant::now();
                park(Duration::from_millis(2));
                if shutdown.load(Ordering::SeqCst) { return; }
                stats
                    .park_ns_total
                    .fetch_add(park_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
                // Termination detection: use the atomic runnable_count as a
                // fast check, combined with a process table scan for correctness.
                if idle_spins > 3 && runnable_count.load(Ordering::Acquire) <= 0 {
                    park(Duration::from_millis(2));
                    if shutdown.load(Ordering::SeqCst) { return; }
                    if runnable_count.load(Ordering::Acquire) <= 0
                        && should_terminate(
                            &process_table,
                            &injector,
                            &timer_wheel,
                            &io_pool,
                            &alive_count,
                            #[cfg(mio_supported)]
                            &reactor,
                        ) {
                            park(Duration::from_millis(1));
                            if shutdown.load(Ordering::SeqCst) { return; }
                            if runnable_count.load(Ordering::Acquire) <= 0
                                && should_terminate(
                                    &process_table,
                                    &injector,
                                    &timer_wheel,
                                    &io_pool,
                                    &alive_count,
                                    #[cfg(mio_supported)]
                                    &reactor,
                                )
                            {
                                stop_workers(&shutdown, &timer_wheel, &io_pool, &worker_threads);
                                return;
                            }
                        }
                }
                continue;
            }
        };

        // Execute the process for one time-slice.
        let mut proc = proc_entry.lock_for_runtime();

        // Drain lock-free inbox into process mailbox.
        proc.drain_inbox();

        // Process EXIT signals in the mailbox (from exit/2 or link cascade).
        // Only scan when signals are actually pending (avoids O(mailbox) scan per wake).
        if proc_entry.has_exit_signals.swap(false, Ordering::Acquire) {
            // SAFETY: the worker holds this process's exclusive guard. Inbox
            // draining installs owned fragments on its heap, and GC preserves
            // mailbox roots. No callback or collection runs during the scan;
            // the returned reason is stored immediately, then the process exits
            // without collecting its heap before exit propagation.
            if let Some(exit_reason) = unsafe { crate::process::process_exit_signals(&mut proc) } {
                let pid = proc.id;
                proc.exit_reason = Some(exit_reason);
                proc.transition(ProcessState::Exited);
                // Always decrement: any process picked from the work queue has a
                // matching runnable_count slot (spawned, yielded, or woken via
                // try_wake/try_wake_io).
                runnable_count.fetch_sub(1, Ordering::AcqRel);
                // Check if this was the initial process crashing
                if pid == initial_pid.load(Ordering::SeqCst) {
                    let is_normal =
                        exit_reason.as_atom_id() == Some(crate::atom_table::ATOM_NORMAL);
                    if !is_normal {
                        any_crash.store(true, Ordering::SeqCst);
                    }
                }
                let links = std::mem::take(&mut proc.otp.links);
                let monitors_in = std::mem::take(&mut proc.otp.monitors_incoming);
                drop(proc);
                cleanup_dead_process(
                    pid,
                    exit_reason,
                    links,
                    monitors_in,
                    &process_table,
                    &injector,
                    &runnable_count,
                    &alive_count,
                    &any_crash,
                    &initial_pid,
                    &registered_names,
                    &timer_wheel,
                    &io_pool,
                    #[cfg(mio_supported)]
                    &reactor,
                );
                continue;
            }
        } // has_exit_signals

        // Skip processes that have already exited or are waiting.
        if proc.status() == ProcessStatus::Exited {
            // Already counted as non-runnable; just drop the Arc.
            drop(proc);
            drop(proc_entry);
            idle_spins += 1;
            continue;
        }
        if proc.status() == ProcessStatus::Waiting {
            // Safety net: drain_inbox may have delivered messages that arrived
            // via the lock-free inbox after the process entered receive_wait.
            let has_mail = !proc.mailbox.is_empty();
            let waiting_for_receive = matches!(
                proc.wait_reason(),
                Some(WaitReason::Receive { .. }) | None
            );

            // Check if the receive timeout has expired (timer-woken process).
            let deadline = proc.context.receive_timeout_deadline;
            let timeout_expired =
                waiting_for_receive && deadline > 0 && crate::timer::current_time_ms() >= deadline;

            if (has_mail && waiting_for_receive) || timeout_expired {
                proc.transition_to(ProcessState::Ready);
                timer_wheel.cancel_for_pid(proc.id);
                // Fall through to run.
            } else {
                // Process is still waiting. Do NOT re-inject — the timer
                // thread or send_message will wake it when appropriate.
                // The readiness notification made this process runnable,
                // even if draining it produced no Erlang message;
                // publish_wait releases that slot before its race check
                // can acquire a new one. That check also covers a
                // MessageFragment pushed while the state was READY, whose
                // try_wake() failed: without it the fragment (and any
                // in_flight flag it carries) is stranded, and the reactor
                // spins on stale edge-trigger events for a half-closed peer.
                let waiting = proc.scheduling_state();
                if rewait(proc, &proc_entry, waiting, &timer_wheel, &runnable_count) {
                    local.push(proc_entry);
                    continue;
                }

                drop(proc_entry);
                idle_spins += 1;
                continue;
            }
        }

        // We found a process to actually run — reset idle counter
        // and update sticky affinity so the next dispatch targets us.
        idle_spins = 0;
        proc_entry
            .last_worker
            .store(worker_id as u8, Ordering::Relaxed);

        proc.transition(ProcessState::Running);
        proc.reductions = if proc.otp.low_priority { REDUCTION_LIMIT / 8 } else { REDUCTION_LIMIT };

        // Wake from hibernate: if this process has a pending hibernate MFA,
        // resolve the function and set up initial_fn/initial_args.
        if let Some((mod_id, fun_id, args)) = proc.hibernate_mfa.take() {
            let arity = args.len() as u32;
            if let Some(func) = crate::entry::global_function_table().lookup(mod_id, fun_id, arity)
            {
                // Deep-copy args to the (fresh) process heap
                let mut heap_args = Vec::with_capacity(args.len());
                for arg in args {
                    let copied = arg.copy_to_heap(&mut proc.heap);
                    heap_args.push(copied);
                }
                proc.initial_fn = Some(func);
                proc.initial_args = heap_args;
            } else {
                // Function not found: kill the process
                eprintln!(
                    "Hibernate wake: function not found: {}:{}/{}",
                    crate::atom_table::global_atom_table()
                        .resolve(mod_id)
                        .unwrap_or_else(|| "?".to_string()),
                    crate::atom_table::global_atom_table()
                        .resolve(fun_id)
                        .unwrap_or_else(|| "?".to_string()),
                    arity
                );
                proc.exit_reason = Some(crate::term::ErlTerm::atom(crate::atom_table::ATOM_BADARG));
                proc.transition(ProcessState::Exited);
                runnable_count.fetch_sub(1, Ordering::AcqRel);
                let pid = proc.id;
                let exit_reason = proc.exit_reason.unwrap();
                let links = std::mem::take(&mut proc.otp.links);
                let monitors_in = std::mem::take(&mut proc.otp.monitors_incoming);
                drop(proc);
                cleanup_dead_process(
                    pid,
                    exit_reason,
                    links,
                    monitors_in,
                    &process_table,
                    &injector,
                    &runnable_count,
                    &alive_count,
                    &any_crash,
                    &initial_pid,
                    &registered_names,
                    &timer_wheel,
                    &io_pool,
                    #[cfg(mio_supported)]
                    &reactor,
                );
                continue;
            }
        }

        // Run one slice.
        // SAFETY: Scheduler::run's caller guarantees valid processes and
        // callbacks; this worker exclusively owns the locked process here.
        unsafe { proc.run_slice(); }

        // Pending hibernate arguments still live in thread-local raw terms.
        // Own them in STATUS_HIBERNATE before any moving GC; hibernate then
        // collects only persistent state, avoiding a redundant collection here.
        // Explicit collection suspends first so all compiled callers have rooted
        // their live values. Do not also perform pressure/idle GC on this slice.
        let collected = unsafe { proc.collect_requested_garbage() };
        if !collected && proc.context.status == STATUS_RECEIVE_WAIT {
            // Choose the idle/full or pressure collection here, never both
            // before and after the state transition. Check heap limits below.
            unsafe { proc.collect_garbage_idle(); }
        } else if !collected && proc.heap.needs_gc() && proc.context.status != STATUS_HIBERNATE {
            // SAFETY: compiled execution has returned under the process lock;
            // the process roots are live and no heap object borrow is retained.
            unsafe { proc.collect_garbage() };
        }
        if proc.heap.exceeds_max_heap_size() {
            #[cfg(debug_assertions)]
            eprintln!("Process {} killed: exceeded max_heap_size", proc.id);
            proc.transition_to(ProcessState::Exited);
            proc.exit_reason = Some(crate::term::ErlTerm::atom(crate::atom_table::ATOM_KILLED));
        }

        // If execution requested completion (including a Rust callback
        // calling finish),
        // honour that immediately regardless of context.status.
        if proc.status() == ProcessStatus::Exited {
            proc.transition(ProcessState::Exited);
            runnable_count.fetch_sub(1, Ordering::AcqRel);
            let pid = proc.id;
            let exit_reason = proc
                .exit_reason
                .unwrap_or(crate::term::ErlTerm::atom(crate::atom_table::ATOM_NORMAL));
            let links = std::mem::take(&mut proc.otp.links);
            let monitors_in = std::mem::take(&mut proc.otp.monitors_incoming);
            drop(proc);
            cleanup_dead_process(
                pid,
                exit_reason,
                links,
                monitors_in,
                &process_table,
                &injector,
                &runnable_count,
                &alive_count,
                &any_crash,
                &initial_pid,
                &registered_names,
                &timer_wheel,
                &io_pool,
                #[cfg(mio_supported)]
                &reactor,
            );
            continue;
        }

        // Determine what to do next based on the process context status.
        let ctx_status = proc.context.status;

        match ctx_status {
            STATUS_YIELD => {
                proc.transition(ProcessState::Ready);
                drop(proc);
                local.push(proc_entry);
            }
            STATUS_RECEIVE_WAIT => {
                let deadline = proc.context.receive_timeout_deadline;
                let reason = WaitReason::Receive {
                    timeout_deadline_ms: (deadline > 0).then_some(deadline),
                };
                if publish_wait(proc, &proc_entry, ProcessState::Waiting(reason),
                                &timer_wheel, &runnable_count) {
                    local.push(proc_entry);
                    continue;
                }
                // Do NOT re-inject — timer thread or send_message will wake.
            }
            STATUS_IO_WAIT => {
                proc.transition(ProcessState::Waiting(WaitReason::IoWait));
                runnable_count.fetch_sub(1, Ordering::AcqRel);

                // CRITICAL RACE CHECK: A MessageFragment may have been pushed
                // to the lock-free inbox while the process was RUNNING.  In
                // that case try_wake() failed (wrong state) and the fragment
                // is stranded — no normal code path will drain it until the
                // process becomes runnable again.
                //
                // This matters especially for ActiveReadReady fragments from
                // the reactor: they carry an `in_flight` flag that MUST be
                // cleared, otherwise the reactor will reregister and spin
                // forever on stale edge-trigger events (e.g., when the peer
                // half-closes, EV_READ re-fires immediately on every
                // reregister).  drain_inbox calls handle_active_read_on_worker
                // which clears in_flight and is safe when the socket has gone
                // passive (early-returns after clearing the flag).
                proc.drain_inbox();
                drop(proc);
            }
            STATUS_OK => {
                if proc.continuation.is_none()
                    && proc.initial_fn.is_none()
                    && proc.resume_continuations.is_empty()
                    && proc.rust_fn.is_none()
                {
                    // Nera keeps the initial process alive while it has links
                    // after its entry function completes. This preserves ETS
                    // ownership and link topology. On BEAM, the equivalent
                    // caller must explicitly wait after invoking the entry;
                    // a plain `erl -s` startup process terminates on return.
                    // The process will only exit when it receives an EXIT signal
                    // from a linked process or when the runtime shuts down.
                    let is_initial = proc.id == initial_pid.load(Ordering::Relaxed);
                    if is_initial && !proc.otp.links.is_empty() {
                        proc.transition(ProcessState::Waiting(WaitReason::Receive {
                            timeout_deadline_ms: None,
                        }));
                        runnable_count.fetch_sub(1, Ordering::AcqRel);
                        drop(proc);
                        // Do not re-inject — the process sleeps until an EXIT
                        // signal or message wakes it, at which point it will
                        // exit normally (no more code to run).
                        continue;
                    }

                    proc.transition(ProcessState::Exited);
                    runnable_count.fetch_sub(1, Ordering::AcqRel);
                    let pid = proc.id;
                    let exit_reason = crate::term::ErlTerm::atom(crate::atom_table::ATOM_NORMAL);
                    let links = std::mem::take(&mut proc.otp.links);
                    let monitors_in = std::mem::take(&mut proc.otp.monitors_incoming);
                    drop(proc);
                    propagate_exit_signals(
                        pid,
                        exit_reason,
                        links,
                        monitors_in,
                        &process_table,
                        &injector,
                        &runnable_count,
                        &alive_count,
                        &any_crash,
                        &initial_pid,
                        &registered_names,
                        &timer_wheel,
                        &io_pool,
                        #[cfg(mio_supported)]
                        &reactor,
                    );
                    timer_wheel.remove_for_pid(pid);
                    io_pool.cancel_for_pid(pid);
                    #[cfg(mio_supported)]
                    reactor.cancel_for_pid(pid);
                    crate::socket::remove_all_for_pid(pid);
                    unregister_dead_process(pid, &registered_names);
                    crate::ets::delete_tables_owned_by(pid);
                    alive_count.fetch_sub(1, Ordering::AcqRel);
                    process_table.remove(&pid);
                } else {
                    // More work to do.
                    proc.transition(ProcessState::Ready);
                    drop(proc);
                    local.push(proc_entry);
                }
            }
            STATUS_HIBERNATE => {
                // Hibernate: discard heap, save MFA for wake-up, suspend.

                // Retrieve the MFA from thread-local (set by nera_rt_hibernate).
                let mfa = crate::ffi::process::HIBERNATE_MFA.with(|cell| cell.borrow_mut().take());

                if let Some((mod_id, fun_id, args)) = mfa {
                    // SAFETY: hibernate arguments belong to the locked, live process.
                    unsafe { proc.retain_hibernate_args(mod_id, fun_id, args); }
                }

                // SAFETY: the process is locked and compiled execution has
                // returned; retained roots still belong to its live heap.
                unsafe { proc.hibernate() };
                let has_queued_messages = !proc.mailbox.is_empty();
                proc.transition(ProcessState::Waiting(WaitReason::Receive {
                    timeout_deadline_ms: None,
                }));
                runnable_count.fetch_sub(1, Ordering::AcqRel);
                drop(proc);

                // Race check: message may have arrived
                if (has_queued_messages || !proc_entry.inbox.is_empty())
                    && proc_entry.try_wake() {
                        runnable_count.fetch_add(1, Ordering::AcqRel);
                        local.push(proc_entry);
                        continue;
                    }
            }
            _ => {
                // STATUS_EXCEPTION or unknown: print crash report and mark exited.
                let exit_reason;
                if ctx_status == STATUS_EXCEPTION {
                    let pid = proc.id;
                    let class_name = match proc.context.exception_class {
                        0 => "throw",
                        1 => "error",
                        2 => "exit",
                        _ => "unknown",
                    };
                    exit_reason = crate::term::ErlTerm(proc.context.result);
                    let is_exit_class = proc.context.exception_class == 2;
                    if !is_exit_class {
                        // SAFETY: the process guard keeps its heap and borrowed
                        // owners live. Formatting cannot reenter or run moving GC.
                        let reason_str = unsafe { crate::bif::format_term(exit_reason) };
                        eprintln!(
                            "Error in process <0.{}.0>: {}:{}",
                            pid, class_name, reason_str
                        );
                        let trace = &proc.exception_trace;
                        if !trace.is_empty() {
                            let atoms = crate::atom_table::global_atom_table();
                            eprintln!("Stacktrace (most recent call last):");
                            for frame in trace.iter().rev() {
                                let mod_name = atoms
                                    .resolve(frame.module)
                                    .unwrap_or_else(|| "?".to_string());
                                let fun_name = atoms
                                    .resolve(frame.function)
                                    .unwrap_or_else(|| "?".to_string());
                                if frame.line > 0 {
                                    eprintln!(
                                        "  {}:{}/{} ({}.erl, line {})",
                                        mod_name, fun_name, frame.arity, mod_name, frame.line
                                    );
                                } else {
                                    eprintln!("  {}:{}/{}", mod_name, fun_name, frame.arity);
                                }
                            }
                        }
                        if pid == initial_pid.load(Ordering::SeqCst) {
                            any_crash.store(true, Ordering::SeqCst);
                        }
                    }
                } else {
                    exit_reason = crate::term::ErlTerm::atom(crate::atom_table::ATOM_NORMAL);
                }
                proc.transition(ProcessState::Exited);
                runnable_count.fetch_sub(1, Ordering::AcqRel);
                let pid = proc.id;
                let links = std::mem::take(&mut proc.otp.links);
                let monitors_in = std::mem::take(&mut proc.otp.monitors_incoming);
                drop(proc);
                cleanup_dead_process(
                    pid,
                    exit_reason,
                    links,
                    monitors_in,
                    &process_table,
                    &injector,
                    &runnable_count,
                    &alive_count,
                    &any_crash,
                    &initial_pid,
                    &registered_names,
                    &timer_wheel,
                    &io_pool,
                    #[cfg(mio_supported)]
                    &reactor,
                );
            }
        }
    }
}

/// Check if the scheduler should terminate: all processes have exited
/// or all remaining processes are waiting (deadlock) and the injector
/// is empty of ready processes.
///
/// A process is considered "potentially runnable" if:
/// - Its status is Ready or Running, OR
/// - Its status is Waiting and a wake condition exists (mailbox message,
///   published timed-receive deadline, or pending timer callback).
///
/// A timed receive owns its liveness across registration and expiration
/// handoffs, until the wake CAS publishes Ready.
fn should_terminate(
    process_table: &ProcessTable,
    _injector: &Arc<Injector<Arc<ProcessEntry>>>,
    timer_wheel: &Arc<TimerWheel>,
    io_pool: &Arc<IoPool>,
    alive_count: &Arc<AtomicI64>,
    #[cfg(mio_supported)] reactor: &ReactorHandle,
) -> bool {
    // Fast path: all processes have exited and been cleaned up.
    if alive_count.load(Ordering::Acquire) <= 0 {
        return true;
    }

    // If there are pending timers, processes will be woken in the future.
    if timer_wheel.has_pending_timers() {
        return false;
    }
    // If there are pending I/O operations, processes will be woken when they complete.
    if io_pool.has_pending() {
        return false;
    }
    // If the reactor has pending operations, processes will be woken on I/O readiness.
    #[cfg(mio_supported)]
    if reactor.has_pending() {
        return false;
    }

    processes_are_blocked(process_table)
}

/// Final process-state scan, also covering timer registration/expiry handoffs.
fn processes_are_blocked(process_table: &ProcessTable) -> bool {
    // Collect process entry Arcs, then check each individually.
    if process_table.is_empty() {
        return true;
    }
    let entries: Vec<Arc<ProcessEntry>> = process_table.iter().map(|r| r.value().clone()).collect();
    // Check if any process is still runnable.
    for entry in &entries {
        // Fast pre-filter via atomic_status (no Mutex needed).
        let astatus = entry.published_status();
        match astatus {
            ASTATUS_READY | ASTATUS_RUNNING => {
                return false;
            }
            ASTATUS_IO_WAIT => {
                // Process is waiting for I/O — will be woken by io_pool or reactor.
                return false;
            }
            ASTATUS_WAITING => {
                // Check lock-free inbox first (no lock needed).
                if !entry.inbox.is_empty() {
                    return false;
                }
                // Need Mutex to check wait_reason and mailbox.
                let proc = entry.lock_for_runtime();
                // The receive deadline bridges the expiry handoff: the timer
                // thread removes the entry before its wake CAS publishes
                // READY. That is not deadlock. publish_wait registers the
                // timer before WAITING, but stays conservative here.
                if matches!(proc.wait_reason(), Some(WaitReason::Receive {
                    timeout_deadline_ms: Some(_),
                })) {
                    return false;
                }
                let waiting_for_receive = matches!(
                    proc.wait_reason(),
                    Some(WaitReason::Receive { .. }) | None
                );
                if waiting_for_receive && !proc.mailbox.is_empty() {
                    return false;
                }
            }
            ASTATUS_EXITED => {
                // Still in the table: its exit cleanup has released its
                // runnable slot but not yet delivered the EXIT and DOWN
                // signals that can wake a linked or monitoring process.
                return false;
            }
            _ => {}
        }
    }
    // All processes are Waiting with empty mailboxes.
    true
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{ProcessInner, ProcessStatus};
    use crate::term::ErlTerm;
    use std::sync::atomic::AtomicU64;
    use std::time::{Duration, Instant};

    // NERA-42: execute the actual worker idle path with controlled park returns.
    #[test]
    fn shutdown_never_reparks_workers() {
        for stop_at in [0, 1, 2, 3] {
            let scheduler = Scheduler::new(1);
            if stop_at != 0 {
                // A real live process blocked forever on receive exercises
                // the slower deadlock-confirmation path, not the all-exited path.
                let pid = scheduler.next_pid();
                scheduler.spawn_process(Process::new(pid));
                let Steal::Success(entry) = scheduler.injector.steal() else {
                    panic!("new process must be queued");
                };
                entry.lock_for_runtime().transition(ProcessState::Waiting(
                    WaitReason::Receive { timeout_deadline_ms: None },
                ));
                scheduler.runnable_count.fetch_sub(1, Ordering::AcqRel);
            }
            let worker = Worker::new_fifo();
            let stealers = Arc::new(vec![worker.stealer()]);
            let parks = std::cell::Cell::new(0);
            #[cfg(mio_supported)]
            let (reactor, join) = crate::reactor::spawn_reactor();
            worker_loop(0, worker, scheduler.injector.clone(), stealers,
                scheduler.process_table.clone(), scheduler.shutdown.clone(),
                scheduler.runnable_count.clone(), scheduler.alive_count.clone(),
                scheduler.any_crash.clone(), scheduler.initial_pid.clone(), 1,
                scheduler.registered_names.clone(), scheduler.timer_wheel.clone(),
                scheduler.io_pool.clone(), scheduler.worker_stats[0].clone(),
                scheduler.worker_inboxes[0].clone(),
                Arc::new(vec![]),
                |_| {
                    let count = parks.get() + 1;
                    parks.set(count);
                    assert!(stop_at > 0 && count <= stop_at, "worker reparked after shutdown");
                    if count == stop_at { scheduler.shutdown.store(true, Ordering::SeqCst); }
                },
                #[cfg(mio_supported)]
                reactor.clone());
            assert_eq!(parks.get(), stop_at);
            #[cfg(mio_supported)]
            { reactor.shutdown(); join.join().unwrap(); }
        }
    }

    // NERA-42: every thread in this run must receive the shutdown wake,
    // independently of the process-global scheduler OnceLock.
    #[test]
    fn shutdown_unparks_this_runs_workers() {
        let scheduler = Scheduler::new(2);
        let slots = Arc::new((0..2).map(|_| OnceLock::new()).collect::<Vec<_>>());
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let (done, completed) = std::sync::mpsc::channel();
        let mut handles = Vec::new();
        for id in 0..2 {
            let slots = slots.clone();
            let barrier = barrier.clone();
            let done = done.clone();
            let shutdown = scheduler.shutdown.clone();
            handles.push(thread::spawn(move || {
                slots[id].set(thread::current()).unwrap();
                barrier.wait();
                // Unpark before park leaves a token; unpark during park wakes.
                // Ignore unrelated spurious wakeups.
                loop {
                    thread::park();
                    if shutdown.load(Ordering::SeqCst) { break; }
                }
                done.send(id).unwrap();
            }));
        }
        barrier.wait();
        stop_workers(&scheduler.shutdown, &scheduler.timer_wheel, &scheduler.io_pool, &slots);
        let woke = (0..2).all(|_| completed.recv_timeout(Duration::from_secs(5)).is_ok());
        // Bound a broken test without leaving blocked threads behind.
        for handle in &handles { handle.thread().unpark(); }
        for handle in handles { handle.join().unwrap(); }
        assert!(woke, "shutdown must unpark all of this run's workers");
    }

    // gen_server and supervisor start (demonitor, then flush any 'DOWN'
    // without waiting): a monitor its owner already removed with demonitor
    // must deliver no 'DOWN' when the monitored process exits, as on BEAM.
    // Otherwise the stray 'DOWN' lands in the owner's mailbox after the
    // flush.
    #[test]
    fn exit_delivers_down_only_for_monitors_still_held() {
        let scheduler = Scheduler::new(1);
        let owner_pid = scheduler.spawn_process(Process::new(scheduler.next_pid()));
        let dead_pid = scheduler.next_pid();
        let (held, removed) = (1_u64, 2_u64);
        let owner = scheduler.process_table.get(&owner_pid).unwrap().clone();
        owner.lock_for_runtime().add_outgoing_monitor(held, dead_pid);
        let monitors_incoming: HashMap<MonitorRef, ProcessId> =
            [(held, owner_pid), (removed, owner_pid)].into_iter().collect();
        #[cfg(mio_supported)]
        let (reactor, join) = crate::reactor::spawn_reactor();
        propagate_exit_signals(
            dead_pid,
            ErlTerm::atom(crate::atom_table::ATOM_NORMAL),
            HashSet::new(),
            monitors_incoming,
            &scheduler.process_table,
            &scheduler.injector,
            &scheduler.runnable_count,
            &scheduler.alive_count,
            &scheduler.any_crash,
            &scheduler.initial_pid,
            &scheduler.registered_names,
            &scheduler.timer_wheel,
            &scheduler.io_pool,
            #[cfg(mio_supported)]
            &reactor,
        );
        #[cfg(mio_supported)]
        { reactor.shutdown(); join.join().unwrap(); }
        assert_eq!(owner.inbox.len(), 1, "only the held monitor delivers a 'DOWN'");
        assert!(owner.lock_for_runtime().otp.monitors_outgoing.is_empty());
    }

    #[test]
    fn spawn_and_run_single_process() {
        let scheduler = Scheduler::new(2);
        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = Arc::clone(&counter);

        let pid = scheduler.next_pid();
        let mut proc = Process::new(pid);
        proc.rust_fn = Some(Box::new(move |inner: &mut ProcessInner| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            inner.finish();
        }));
        scheduler.spawn_process(proc);
        unsafe { scheduler.run() };

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn spawn_many_processes_all_complete() {
        let scheduler = Scheduler::new(4);
        let counter = Arc::new(AtomicU64::new(0));
        let n = 100;

        for _ in 0..n {
            let counter_clone = Arc::clone(&counter);
            let pid = scheduler.next_pid();
            let mut proc = Process::new(pid);
            proc.rust_fn = Some(Box::new(move |inner: &mut ProcessInner| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                inner.finish();
            }));
            scheduler.spawn_process(proc);
        }

        unsafe { scheduler.run() };
        assert_eq!(counter.load(Ordering::SeqCst), n);
    }

    #[test]
    fn process_with_no_fn_exits_immediately() {
        let scheduler = Scheduler::new(1);
        let pid = scheduler.next_pid();
        let proc = Process::new(pid);
        scheduler.spawn_process(proc);
        unsafe { scheduler.run() };
        // Should terminate without hanging.
    }

    #[test]
    fn yield_and_reschedule() {
        let scheduler = Scheduler::new(2);
        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = Arc::clone(&counter);

        let pid = scheduler.next_pid();
        let mut proc = Process::new(pid);
        proc.rust_fn = Some(Box::new(move |inner: &mut ProcessInner| {
            let count = counter_clone.fetch_add(1, Ordering::SeqCst);
            if count < 3 {
                // Yield: will be rescheduled.
                inner.context.status = STATUS_YIELD;
            } else {
                // Done.
                inner.finish();
            }
        }));
        scheduler.spawn_process(proc);
        unsafe { scheduler.run() };

        // The function should have been called 4 times (0, 1, 2, 3).
        assert_eq!(counter.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn single_worker_global_queue_progresses_while_local_process_yields() {
        let scheduler = Arc::new(Scheduler::new(1));
        let turns = Arc::new(AtomicU64::new(0));
        let observed = Arc::new(AtomicU64::new(u64::MAX));
        let running = Arc::clone(&scheduler);
        let count = Arc::clone(&turns);
        let result = Arc::clone(&observed);
        let mut busy = Process::new(scheduler.next_pid());
        busy.rust_fn = Some(Box::new(move |inner| {
            let turn = count.fetch_add(1, Ordering::SeqCst) + 1;
            if turn == 1 {
                // Arrives after the initial injector batch was taken. The busy
                // process keeps the local deque nonempty until it terminates.
                let mut observer = Process::new(running.next_pid());
                let count = Arc::clone(&count);
                let result = Arc::clone(&result);
                observer.rust_fn = Some(Box::new(move |inner| {
                    result.store(count.load(Ordering::SeqCst), Ordering::SeqCst);
                    inner.finish();
                }));
                running.spawn_process(observer);
            }
            if turn == 128 {
                inner.finish();
            } else {
                inner.context.status = STATUS_YIELD;
            }
        }));
        scheduler.spawn_process(busy);
        assert!(unsafe { scheduler.run() });
        eprintln!(
            "global observer after {} local slices",
            observed.load(Ordering::SeqCst)
        );
        assert!(
            observed.load(Ordering::SeqCst) <= 32,
            "global work waited {} local slices",
            observed.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn worker_inbox_drain_leaves_time_for_execution() {
        let inbox = SegQueue::new();
        let local = Worker::new_fifo();
        for pid in 1..=1024 {
            inbox.push(Arc::new(ProcessEntry::new(Process::new(crate::process::ProcessId::new(pid)))));
        }
        drain_worker_inbox(&inbox, &local);
        assert!(!local.is_empty());
        assert!(
            !inbox.is_empty(),
            "draining a busy inbox must leave a bounded batch for the next turn"
        );
        let mut count = 0;
        while !inbox.is_empty() || !local.is_empty() {
            drain_worker_inbox(&inbox, &local);
            while local.pop().is_some() {
                count += 1;
            }
        }
        assert_eq!(count, 1024, "bounded draining must not drop queue entries");
    }

    #[test]
    fn yield_fairness() {
        let scheduler = Scheduler::new(4);
        let counters = Arc::new((0..4).map(|_| AtomicU64::new(0)).collect::<Vec<_>>());

        for idx in 0..4 {
            let counters_clone = Arc::clone(&counters);
            let pid = scheduler.next_pid();
            let mut proc = Process::new(pid);
            proc.rust_fn = Some(Box::new(move |inner: &mut ProcessInner| {
                let step = counters_clone[idx].fetch_add(1, Ordering::SeqCst);
                if step < 10 {
                    inner.context.status = STATUS_YIELD;
                } else {
                    inner.finish();
                }
            }));
            scheduler.spawn_process(proc);
        }

        assert!(unsafe { scheduler.run() });
        for counter in counters.iter() {
            assert_eq!(counter.load(Ordering::SeqCst), 11);
        }
    }

    // Reproduce the exact handoff states without sleeps or relying on worker
    // timing. Process state, timer heap and runnable count are all local.
    fn receive_timer_handoff_snapshot(expired_before_wake: bool) -> bool {
        let scheduler = Scheduler::new(1);
        let pid = scheduler.next_pid();
        scheduler.spawn_process(Process::new(pid));
        let entry = match scheduler.injector.steal() {
            Steal::Success(entry) => entry,
            _ => panic!("new process must be queued"),
        };
        {
            let mut proc = entry.lock_for_runtime();
            proc.context.receive_timeout_deadline = 1;
            proc.transition(ProcessState::Waiting(WaitReason::Receive {
                timeout_deadline_ms: Some(1),
            }));
            scheduler.runnable_count.fetch_sub(1, Ordering::AcqRel);
        }
        // First window: WAITING was published, but insert has not run yet.
        if expired_before_wake {
            scheduler.timer_wheel.insert(1, pid);
            let expired = scheduler.timer_wheel.pop_expired(1);
            assert_eq!(expired.len(), 1);
            assert_eq!(expired[0].owner_pid, pid);
            // Second window: the expired batch has been extracted, but the
            // timer thread has not yet called wake_process_for_timer.
        }
        assert_eq!(entry.published_status(), ASTATUS_WAITING);
        assert_eq!(scheduler.runnable_count.load(Ordering::Acquire), 0);
        assert!(!scheduler.timer_wheel.has_pending_timers());
        assert!(entry.inbox.is_empty());
        // Every asynchronous fast-path counter is empty here. Inspect the
        // exact process-state predicate used by should_terminate, without a
        // global reactor or other tests' pending I/O masking the decision.
        let terminated = processes_are_blocked(&scheduler.process_table);
        if !expired_before_wake {
            scheduler.timer_wheel.insert(1, pid);
            assert_eq!(scheduler.timer_wheel.pop_expired(1).len(), 1);
        }
        // Complete the handoff and verify that exactly one runnable slot and
        // one queue entry are published, including a duplicate wake attempt.
        wake_process_for_timer(pid, &scheduler.process_table, &scheduler.injector,
            &scheduler.runnable_count);
        wake_process_for_timer(pid, &scheduler.process_table, &scheduler.injector,
            &scheduler.runnable_count);
        assert_eq!(entry.published_status(), ASTATUS_READY);
        assert_eq!(scheduler.runnable_count.load(Ordering::Acquire), 1);
        assert!(matches!(scheduler.injector.steal(), Steal::Success(_)));
        assert!(scheduler.injector.is_empty());
        assert!(!processes_are_blocked(&scheduler.process_table));
        // An untimed receive with no message really is blocked; the fix must
        // not turn ordinary deadlock detection into an infinite wait.
        entry.lock_for_runtime().transition(ProcessState::Waiting(
            WaitReason::Receive { timeout_deadline_ms: None }));
        scheduler.runnable_count.fetch_sub(1, Ordering::AcqRel);
        assert!(processes_are_blocked(&scheduler.process_table));
        terminated
    }

    #[test]
    fn receive_timer_registration_handoff_cannot_terminate() {
        assert!(!receive_timer_handoff_snapshot(false),
            "timed receive must stay live before timer registration");
    }

    #[test]
    fn receive_timer_expiration_handoff_cannot_terminate() {
        assert!(!receive_timer_handoff_snapshot(true),
            "expired receive must stay live until its wake is published");
    }

    // The three tests below replay, step by step, how a timed receive lost its
    // timer. Cowlib's cow_cookie EUnit suite then hung natively in a few runs
    // out of a hundred: every process waiting, one of them in a receive whose
    // `after` deadline had passed, and no timer left to wake it.
    fn running_process(scheduler: &Scheduler) -> (ProcessId, Arc<ProcessEntry>) {
        let pid = scheduler.next_pid();
        scheduler.spawn_process(Process::new(pid));
        let entry = match scheduler.injector.steal() {
            Steal::Success(entry) => entry,
            _ => panic!("new process must be queued"),
        };
        entry.lock_for_runtime().transition(ProcessState::Running);
        (pid, entry)
    }

    fn timed_wait(deadline_ms: u64) -> ProcessState {
        ProcessState::Waiting(WaitReason::Receive { timeout_deadline_ms: Some(deadline_ms) })
    }

    fn wake_expired(scheduler: &Scheduler, now_ms: u64) -> usize {
        let expired = scheduler.timer_wheel.pop_expired(now_ms);
        for timer in &expired {
            wake_process_for_timer(timer.owner_pid, &scheduler.process_table,
                &scheduler.injector, &scheduler.runnable_count);
        }
        expired.len()
    }

    #[test]
    fn stale_receive_timer_cannot_replace_a_later_one() {
        let scheduler = Scheduler::new(1);
        let (pid, entry) = running_process(&scheduler);
        let requeued = publish_wait_with(entry.lock_for_runtime(), &entry, timed_wait(10),
            &scheduler.timer_wheel, &scheduler.runnable_count, false, |window| {
                if window != WaitWindow::AfterPublish {
                    return;
                }
                // Another worker, before this one continues: a message wakes
                // the process, which runs into a later timed receive.
                assert!(entry.try_wake());
                scheduler.runnable_count.fetch_add(1, Ordering::AcqRel);
                let mut proc = entry.lock_for_runtime();
                scheduler.timer_wheel.cancel_for_pid(pid);
                proc.transition(ProcessState::Running);
                assert!(!publish_wait(proc, &entry, timed_wait(20),
                    &scheduler.timer_wheel, &scheduler.runnable_count));
            });
        assert!(!requeued, "the later receive has its own timer");
        assert_eq!(entry.published_status(), ASTATUS_WAITING);
        assert_eq!(wake_expired(&scheduler, 10), 0,
            "the first receive's timer must not stand in for the later one");
        assert_eq!(wake_expired(&scheduler, 20), 1);
        assert_eq!(entry.published_status(), ASTATUS_READY);
        assert_eq!(scheduler.runnable_count.load(Ordering::Acquire), 1);
    }

    #[test]
    fn receive_timer_taken_before_waiting_is_published_still_wakes() {
        let scheduler = Scheduler::new(1);
        let (pid, entry) = running_process(&scheduler);
        let requeued = publish_wait_with(entry.lock_for_runtime(), &entry, timed_wait(1),
            &scheduler.timer_wheel, &scheduler.runnable_count, false, |window| {
                if window != WaitWindow::BeforePublish {
                    return;
                }
                // The timer thread takes the due timer; its wake fails
                // because the process is still running.
                assert_eq!(wake_expired(&scheduler, 1), 1);
                assert_ne!(entry.published_status(), ASTATUS_READY);
                assert!(scheduler.process_table.contains_key(&pid));
            });
        assert!(requeued, "the expired receive must be queued again");
        assert_eq!(entry.published_status(), ASTATUS_READY);
        assert_eq!(scheduler.runnable_count.load(Ordering::Acquire), 1);
        assert!(scheduler.injector.is_empty(), "the failed timer wake queued nothing");
    }

    #[test]
    fn timer_expiring_during_a_wake_without_mail_still_wakes() {
        let scheduler = Scheduler::new(1);
        let (_, entry) = running_process(&scheduler);
        assert!(!publish_wait(entry.lock_for_runtime(), &entry, timed_wait(1),
            &scheduler.timer_wheel, &scheduler.runnable_count));
        // A wake that brings no message, such as an ignored exit signal.
        assert!(entry.try_wake());
        scheduler.runnable_count.fetch_add(1, Ordering::AcqRel);
        // The deadline passes while the process is Ready: the timer thread
        // takes the timer and its wake fails.
        assert_eq!(wake_expired(&scheduler, 1), 1);
        assert_eq!(scheduler.runnable_count.load(Ordering::Acquire), 1);
        // The worker read the clock just before the deadline, found no
        // message, and returns the process to its wait.
        let proc = entry.lock_for_runtime();
        let waiting = proc.scheduling_state();
        assert_eq!(waiting, timed_wait(1));
        let requeued = rewait(proc, &entry, waiting,
            &scheduler.timer_wheel, &scheduler.runnable_count);
        if !requeued {
            wake_expired(&scheduler, 1);
        }
        assert_eq!(entry.published_status(), ASTATUS_READY, "the receive must time out");
        assert_eq!(scheduler.runnable_count.load(Ordering::Acquire), 1);
    }

    // Found in review of the fix above: returning a woken process to its wait
    // registered its still-pending timer again, so each wake that brought
    // nothing (socket readiness, an ignored exit signal) left one more heap
    // entry until the deadline.
    #[test]
    fn waking_without_work_keeps_a_single_timer() {
        let scheduler = Scheduler::new(1);
        let (_, entry) = running_process(&scheduler);
        assert!(!publish_wait(entry.lock_for_runtime(), &entry, timed_wait(1_000),
            &scheduler.timer_wheel, &scheduler.runnable_count));
        for _ in 0..100 {
            assert!(entry.try_wake());
            scheduler.runnable_count.fetch_add(1, Ordering::AcqRel);
            let proc = entry.lock_for_runtime();
            let waiting = proc.scheduling_state();
            assert!(!rewait(proc, &entry, waiting,
                &scheduler.timer_wheel, &scheduler.runnable_count));
        }
        assert_eq!(scheduler.timer_wheel.queued_entries(), 1);
        assert_eq!(scheduler.runnable_count.load(Ordering::Acquire), 0);
        assert_eq!(wake_expired(&scheduler, 1_000), 1, "the wait keeps its timeout");
        assert_eq!(entry.published_status(), ASTATUS_READY);
    }

    // The same EUnit run also ended early about once in 300 runs, before EUnit
    // reported: a test process had exited and released its runnable slot, but
    // its EXIT signal had not yet reached the linked EUnit process. The idle
    // check saw only waiting processes and stopped the runtime.
    #[test]
    fn exit_cleanup_in_progress_is_not_deadlock() {
        let scheduler = Scheduler::new(1);
        let (_, linked) = running_process(&scheduler);
        let untimed = ProcessState::Waiting(WaitReason::Receive { timeout_deadline_ms: None });
        assert!(!publish_wait(linked.lock_for_runtime(), &linked, untimed,
            &scheduler.timer_wheel, &scheduler.runnable_count));
        // The worker's exit path: Exited is published and the slot released
        // before cleanup delivers signals and removes the process.
        let (dying_pid, dying) = running_process(&scheduler);
        dying.lock_for_runtime().transition(ProcessState::Exited);
        scheduler.runnable_count.fetch_sub(1, Ordering::AcqRel);
        assert_eq!(scheduler.runnable_count.load(Ordering::Acquire), 0);
        assert!(!processes_are_blocked(&scheduler.process_table),
            "the exit signal can still wake the linked process");
        // Cleanup finished and woke nobody: now every process is blocked.
        scheduler.process_table.remove(&dying_pid);
        assert!(processes_are_blocked(&scheduler.process_table));
    }

    // send_after and start_timer messages now come from the timer thread. Like
    // a send, their delivery must not cancel the receiver's receive timer: by
    // then the receiver can be running into a later `receive ... after`.
    #[test]
    fn timer_delivery_leaves_the_receive_timer_to_the_worker() {
        let scheduler = Scheduler::new(1);
        let (pid, entry) = running_process(&scheduler);
        assert!(!publish_wait(entry.lock_for_runtime(), &entry, timed_wait(10),
            &scheduler.timer_wheel, &scheduler.runnable_count));
        let delivery = crate::timer::MessageDelivery {
            dest: pid,
            message: crate::message::MessageFragment::Immediate(
                crate::message::ImmediateTerm::new(ErlTerm::atom(crate::atom_table::ATOM_OK))),
        };
        assert!(deliver_timer_message(delivery, &scheduler.process_table,
            &scheduler.injector, &scheduler.runnable_count));
        assert_eq!(entry.published_status(), ASTATUS_READY);
        assert_eq!(scheduler.runnable_count.load(Ordering::Acquire), 1);
        assert!(matches!(scheduler.injector.steal(), Steal::Success(_)));
        assert!(scheduler.timer_wheel.has_pending_for(pid),
            "the worker that runs the receiver cancels its timer, not the timer thread");
    }

    #[test]
    fn receive_timeout_wakeup() {
        let scheduler = Scheduler::new(2);
        let call_count = Arc::new(AtomicU64::new(0));
        let start_ms = Arc::new(AtomicU64::new(0));
        let elapsed_ms = Arc::new(AtomicU64::new(0));

        let call_count_clone = Arc::clone(&call_count);
        let start_ms_clone = Arc::clone(&start_ms);
        let elapsed_ms_clone = Arc::clone(&elapsed_ms);

        let pid = scheduler.next_pid();
        let mut proc = Process::new(pid);
        let start = crate::timer::current_time_ms();
        start_ms.store(start, Ordering::SeqCst);
        proc.context.receive_timeout_deadline = start + 50;
        proc.rust_fn = Some(Box::new(move |inner: &mut ProcessInner| {
            let turn = call_count_clone.fetch_add(1, Ordering::SeqCst);
            let now = crate::timer::current_time_ms();
            if turn == 0 {
                inner.context.status = STATUS_RECEIVE_WAIT;
            } else {
                let start = start_ms_clone.load(Ordering::SeqCst);
                elapsed_ms_clone.store(now.saturating_sub(start), Ordering::SeqCst);
                inner.finish();
            }
        }));

        scheduler.spawn_process(proc);
        assert!(unsafe { scheduler.run() });

        assert!(call_count.load(Ordering::SeqCst) >= 2);
        assert!(elapsed_ms.load(Ordering::SeqCst) >= 40);
    }

    #[test]
    fn deadlock_detection() {
        let scheduler = Scheduler::new(2);
        let calls = Arc::new(AtomicU64::new(0));

        for _ in 0..3 {
            let calls_clone = Arc::clone(&calls);
            let pid = scheduler.next_pid();
            let mut proc = Process::new(pid);
            proc.rust_fn = Some(Box::new(move |inner: &mut ProcessInner| {
                calls_clone.fetch_add(1, Ordering::SeqCst);
                inner.context.status = STATUS_RECEIVE_WAIT;
            }));
            scheduler.spawn_process(proc);
        }

        let started = Instant::now();
        assert!(unsafe { scheduler.run() });
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn yield_preserves_scheduling() {
        let scheduler = Scheduler::new(2);
        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = Arc::clone(&counter);

        let pid = scheduler.next_pid();
        let mut proc = Process::new(pid);
        proc.rust_fn = Some(Box::new(move |inner: &mut ProcessInner| {
            let step = counter_clone.fetch_add(1, Ordering::SeqCst);
            if step < 100 {
                inner.context.status = STATUS_YIELD;
            } else {
                inner.finish();
            }
        }));

        scheduler.spawn_process(proc);
        assert!(unsafe { scheduler.run() });
        assert_eq!(counter.load(Ordering::SeqCst), 101);
    }

    #[cfg(mio_supported)]
    #[test]
    fn stale_tcp_readiness_releases_runnable_slot() {
        // A reactor notification can wake a receiver without delivering an
        // Erlang message (closed/passive socket, partial line or WouldBlock).
        // Queue exactly that state, without depending on socket timing.
        let scheduler = Arc::new(Scheduler::new(1));
        let pid = scheduler.next_pid();
        let mut proc = Process::new(pid);
        proc.transition_to(ProcessState::Waiting(WaitReason::Receive { timeout_deadline_ms: None }));
        scheduler.spawn_process(proc);
        let in_flight = Arc::new(AtomicBool::new(true));
        scheduler.process_table.get(&pid).unwrap().inbox.push(
            crate::message::MessageFragment::ActiveReadReady {
                stream_handle: crate::socket::SocketHandle::try_from_raw((1 << 32) | 123).unwrap(),
                in_flight: Arc::clone(&in_flight),
            },
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let running = Arc::clone(&scheduler);
        let worker = thread::spawn(move || {
            tx.send(unsafe { running.run() }).unwrap();
        });
        let completed = rx.recv_timeout(Duration::from_secs(2));
        // Bound the regression even on the broken scheduler; do not leave
        // a hung worker behind when the assertion fails.
        if completed.is_err() {
            scheduler.shutdown();
        }
        worker.join().unwrap();
        assert!(!in_flight.load(Ordering::Acquire), "readiness must be drained");
        assert_eq!(scheduler.runnable_count.load(Ordering::Acquire), 0,
            "a wake without a message must release its runnable slot");
        assert!(completed.expect("idle scheduler must terminate"));
    }

    #[test]
    fn mailbox_message_passing_between_processes() {
        // Producer sends a message to consumer via shared scheduler.
        let scheduler = Scheduler::new(2);
        let result = Arc::new(AtomicU64::new(0));

        let consumer_pid_val = scheduler.next_pid();
        let producer_pid_val = scheduler.next_pid();

        let process_table = Arc::clone(&scheduler.process_table);
        let runnable_count_clone = Arc::clone(&scheduler.runnable_count);

        // Consumer: wait for a message, then read it and exit.
        let result_clone = Arc::clone(&result);
        let mut consumer = Process::new(consumer_pid_val);
        consumer.rust_fn = Some(Box::new(move |inner: &mut ProcessInner| {
            if inner.mailbox.is_empty() {
                inner.context.status = STATUS_RECEIVE_WAIT;
            } else {
                inner.mailbox.reset_scan();
                if let Some((idx, msg)) = inner.mailbox.try_match_next() {
                    let val = msg.as_small_int().unwrap_or(0);
                    result_clone.store(val as u64, Ordering::SeqCst);
                    inner.mailbox.remove_at(idx);
                    inner.finish();
                }
            }
        }));

        // Producer: send a message to the consumer and exit.
        let table_for_producer = Arc::clone(&process_table);
        let rc = Arc::clone(&runnable_count_clone);
        let injector_for_producer = Arc::clone(&scheduler.injector);
        let mut producer = Process::new(producer_pid_val);
        producer.rust_fn = Some(Box::new(move |inner: &mut ProcessInner| {
            // Send message 42 to the consumer.
            let consumer_arc_opt = table_for_producer.get(&consumer_pid_val).map(|r| r.clone());
            if let Some(consumer_arc) = consumer_arc_opt {
                let was_waiting;
                {
                    let mut consumer_proc = consumer_arc.lock_for_runtime();
                    consumer_proc.mailbox.send(ErlTerm::small_int(42));
                    // Wake consumer.
                    let waiting_for_receive = matches!(
                        consumer_proc.wait_reason(),
                        Some(WaitReason::Receive { .. }) | None
                    );
                    was_waiting =
                        consumer_proc.status() == ProcessStatus::Waiting && waiting_for_receive;
                    if was_waiting {
                        consumer_proc.transition_to(ProcessState::Ready);
                    }
                }
                if was_waiting {
                    rc.fetch_add(1, Ordering::SeqCst);
                    injector_for_producer.push(consumer_arc);
                }
            }
            inner.finish();
        }));

        scheduler.spawn_process(consumer);
        scheduler.spawn_process(producer);
        unsafe { scheduler.run() };

        assert_eq!(result.load(Ordering::SeqCst), 42);
    }
    #[test]
    fn single_scheduler_does_not_recollect_unchanged_idle_heap() {
        let scheduler = Scheduler::new(1);
        let turns = Arc::new(AtomicU64::new(0));
        let observed = turns.clone();
        let mut proc = Process::new(scheduler.next_pid());
        let mut list = crate::term::ErlTerm::nil();
        for _ in 0..300_000 {
            list = proc.heap.alloc_cons(crate::term::ErlTerm::small_int(65), list);
        }
        proc.mailbox.send(list);
        // Expired timeout keeps the test deterministic without a latency bound.
        proc.context.receive_timeout_deadline = crate::timer::current_time_ms();
        let mut promoted = None;
        proc.rust_fn = Some(Box::new(move |inner| {
            let turn = observed.fetch_add(1, Ordering::SeqCst);
            if turn == 11 {
                assert_eq!(inner.heap.bytes_allocated(), 0);
                inner.finish();
                return;
            }
            if turn == 1 {
                promoted = Some(inner.mailbox.get(0));
            } else if (2..=10).contains(&turn) {
                assert_eq!(Some(inner.mailbox.get(0)), promoted,
                    "unchanged live list was copied again at an idle boundary");
            }
            if turn == 10 {
                inner.mailbox.remove_at(0);
            }
            inner.context.status = STATUS_RECEIVE_WAIT;
        }));
        scheduler.spawn_process(proc);
        assert!(unsafe { scheduler.run() });
        assert_eq!(turns.load(Ordering::SeqCst), 12);
    }

}
