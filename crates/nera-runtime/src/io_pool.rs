//! I/O thread pool for async blocking I/O operations.
//!
//! When an FFI function would block (TCP accept/recv/send, file read/write),
//! it submits a work item to this pool and sets `STATUS_IO_WAIT`. The pool
//! executes the blocking operation on a dedicated I/O thread, then wakes
//! the process by storing the result and re-enqueuing it — identical to how
//! `send_message` wakes a `RECEIVE_WAIT` process.
//!
//! Cancellation is lazy: when a process exits while I/O is in flight, the
//! `pending` mapping is removed. The I/O thread discards the result.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use parking_lot::{Condvar, Mutex};

use crate::process::{IoResult, ProcessId, ProcessStatus};
use crate::term::ErlTerm;
use crate::atom_table::{ATOM_OK, ATOM_ERROR, ATOM_EINVAL};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

pub type IoRequestId = u64;
pub type IoWork = Box<dyn FnOnce() -> IoResult + Send>;

struct IoRequest {
    id: IoRequestId,
    owner_pid: ProcessId,
    work: IoWork,
}

// ---------------------------------------------------------------------------
// IoPool
// ---------------------------------------------------------------------------

/// A fixed-size pool of OS threads that execute blocking I/O operations.
/// Workers are spawned lazily on the first `submit()` call.
pub struct IoPool {
    num_threads: usize,
    next_id: AtomicU64,
    queue: Arc<Mutex<VecDeque<IoRequest>>>,
    queue_cvar: Arc<Condvar>,
    shutdown: Arc<AtomicBool>,
    /// Active (not-yet-completed, not-cancelled) request per process.
    /// A process can have at most one pending I/O request.
    pending: Arc<Mutex<HashMap<ProcessId, IoRequestId>>>,
    /// Whether worker threads have been spawned.
    started: AtomicBool,
    /// Worker thread handles (populated on first submit).
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl IoPool {
    /// Create a new I/O pool. Does not spawn threads yet — they are
    /// lazily spawned on the first `submit()` call.
    pub fn new(num_threads: usize) -> Self {
        IoPool {
            num_threads: num_threads.max(1),
            next_id: AtomicU64::new(1),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            queue_cvar: Arc::new(Condvar::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            started: AtomicBool::new(false),
            handles: Mutex::new(Vec::new()),
        }
    }

    /// Ensure worker threads are spawned (double-checked locking).
    fn ensure_started(&self) {
        if self.started.load(Ordering::Acquire) {
            return;
        }
        let mut handles = self.handles.lock();
        if self.started.load(Ordering::Acquire) {
            return;
        }
        for i in 0..self.num_threads {
            let queue = Arc::clone(&self.queue);
            let queue_cvar = Arc::clone(&self.queue_cvar);
            let shutdown = Arc::clone(&self.shutdown);
            let pending = Arc::clone(&self.pending);

            let handle = thread::Builder::new()
                .name(format!("nera-io-{}", i))
                .spawn(move || {
                    io_worker_loop(queue, queue_cvar, shutdown, pending);
                })
                .expect("failed to spawn I/O worker thread");
            handles.push(handle);
        }
        self.started.store(true, Ordering::Release);
    }

    /// Submit a blocking I/O operation for the given process.
    /// Lazily spawns worker threads on first call.
    /// Returns the request ID.
    pub fn submit(&self, pid: ProcessId, work: IoWork) -> IoRequestId {
        self.ensure_started();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        {
            let mut p = self.pending.lock();
            p.insert(pid, id);
        }
        {
            let mut q = self.queue.lock();
            q.push_back(IoRequest {
                id,
                owner_pid: pid,
                work,
            });
        }
        self.queue_cvar.notify_one();
        id
    }

    /// Cancel any pending I/O for the given process (lazy cancellation).
    /// The I/O thread will discard the result when it completes.
    pub fn cancel_for_pid(&self, pid: ProcessId) {
        let mut p = self.pending.lock();
        p.remove(&pid);
    }

    /// Returns `true` if there are any non-cancelled I/O requests in flight.
    pub fn has_pending(&self) -> bool {
        let p = self.pending.lock();
        !p.is_empty()
    }

    /// Signal all I/O threads to shut down, wake them, and join.
    pub fn shutdown(&self) {
        self.shutdown_with(|| {});
    }

    fn shutdown_with(&self, before_notify: impl FnOnce()) {
        let _queue = self.queue.lock();
        self.shutdown.store(true, Ordering::SeqCst);
        before_notify();
        self.queue_cvar.notify_all();
    }

    /// Join all worker threads. Call after `shutdown()` and after closing
    /// sockets (to unblock threads stuck in blocking syscalls).
    pub fn join(&self) {
        let mut handles = self.handles.lock();
        for h in handles.drain(..) {
            let _ = h.join();
        }
    }
}

// ---------------------------------------------------------------------------
// I/O worker thread
// ---------------------------------------------------------------------------

fn io_worker_loop(
    queue: Arc<Mutex<VecDeque<IoRequest>>>,
    queue_cvar: Arc<Condvar>,
    shutdown: Arc<AtomicBool>,
    pending: Arc<Mutex<HashMap<ProcessId, IoRequestId>>>,
) {
    loop {
        // Dequeue a request (blocking wait)
        let request = {
            let mut q = queue.lock();
            loop {
                if shutdown.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(req) = q.pop_front() {
                    break req;
                }
                // Shutdown publishes and notifies under this queue lock.
                // Retain the timed fallback for periodic predicate checks.
                queue_cvar.wait_for(&mut q, std::time::Duration::from_millis(100));
            }
        };

        // Execute the blocking I/O operation
        let result = (request.work)();

        // Check if cancelled (process exited or was killed)
        let cancelled = {
            let mut p = pending.lock();
            match p.get(&request.owner_pid) {
                Some(&id) if id == request.id => {
                    p.remove(&request.owner_pid);
                    false
                }
                _ => true, // Different ID or not present → cancelled
            }
        };
        if cancelled {
            // If the cancelled I/O produced a socket handle (e.g. accept
            // succeeded after the owning process was killed), clean it up
            // to avoid orphaned FDs in the socket registry.
            if let IoResult::OkHandle(h) = &result {
                crate::socket::remove(*h);
            }
            continue;
        }

        // Wake the process with the I/O result
        wake_process_with_result(request.owner_pid, result);
    }
}

/// Convert an `IoResult` to an `ErlTerm` on the process heap and store it
/// in `ctx.result` so the call-site resume can pick it up. Then re-enqueue
/// the process. Follows the same pattern as `send_message` in `ffi.rs`.
pub(crate) fn wake_process_with_result(pid: ProcessId, result: IoResult) {
    use std::sync::atomic::Ordering;

    let sched = match crate::scheduler::global_scheduler() {
        Some(s) => s,
        None => return,
    };

    let entry = match sched.process_table.get(&pid) {
        Some(r) => Arc::clone(&r),
        None => return, // Process already removed
    };

    // I/O results must be stored in ctx.result, which requires the Mutex.
    {
        let mut proc = entry.lock_for_runtime();
        if proc.status() == ProcessStatus::Exited {
            return;
        }
        // Convert IoResult → ErlTerm on the process heap
        let term = io_result_to_term(&mut proc.heap, result);
        proc.context.result = term.0;
        // Update Mutex-protected status (for should_terminate correctness).
        proc.transition_to(crate::process::ProcessState::Ready);
    }

    // Use CAS to atomically transition IO_WAIT → READY. If a message
    // delivery already CAS'd WAITING → READY (which can't happen now
    // that IO_WAIT uses a distinct atomic value), we skip the double-push.
    // The CAS ensures only one waker increments runnable_count.
    if entry.try_wake_io() {
        sched.runnable_count.fetch_add(1, Ordering::AcqRel);
        sched.injector.push(entry);
        for t in sched.worker_threads.iter() {
            if let Some(t) = t.get() {
                t.unpark();
                break;
            }
        }
    }
}

/// Convert an `IoResult` into an `ErlTerm` allocated on the given heap.
///
/// Mapping:
/// - `Ok`           → atom `ok`
/// - `OkBytes(b)`   → `{ok, <<binary>>}`
/// - `OkHandle(h)`  → `{ok, H}`  (small int)
/// - `Closed`       → `{error, closed}`
/// - `Err(e)`       → `{error, Reason}` where Reason is a POSIX atom
pub(crate) fn io_result_to_term(heap: &mut crate::heap::ProcessHeap, result: IoResult) -> ErlTerm {
    match result {
        IoResult::Ok => ErlTerm::atom(ATOM_OK),
        IoResult::OkBytes(bytes) => {
            let ok_atom = ErlTerm::atom(ATOM_OK);
            let binary = heap.alloc_binary_auto(&bytes);
            ErlTerm(heap.alloc_tuple(&[ok_atom, binary]).0)
        }
        IoResult::OkHandle(h) => {
            let ok_atom = ErlTerm::atom(ATOM_OK);
            let handle_term = h.to_term();
            ErlTerm(heap.alloc_tuple(&[ok_atom, handle_term]).0)
        }
        IoResult::Closed => heap.alloc_tuple(&[
            ErlTerm::atom(ATOM_ERROR),
            ErlTerm::atom(crate::atom_table::ATOM_CLOSED),
        ]),
        IoResult::Err(e) => {
            let error_atom = ErlTerm::atom(ATOM_ERROR);
            let reason_atom = ErlTerm::atom(io_error_to_atom(e));
            ErlTerm(heap.alloc_tuple(&[error_atom, reason_atom]).0)
        }
    }
}

/// Map an `io::Error` to a well-known POSIX atom ID.
/// Mirrors the logic in `ffi.rs::io_error_to_atom_id`.
fn io_error_to_atom(e: std::io::Error) -> u32 {
    use std::io::ErrorKind;
    use crate::atom_table::{
        ATOM_EACCES, ATOM_EADDRINUSE, ATOM_EADDRNOTAVAIL,
        ATOM_ECONNREFUSED, ATOM_ECONNRESET, ATOM_ECONNABORTED,
        ATOM_ENOTCONN, ATOM_EPIPE, ATOM_ETIMEDOUT, ATOM_ENOENT,
        ATOM_EEXIST, ATOM_EISDIR, ATOM_ENOSPC, ATOM_EMFILE, ATOM_ENFILE,
    };
    match e.kind() {
        ErrorKind::PermissionDenied   => ATOM_EACCES,
        ErrorKind::AddrInUse          => ATOM_EADDRINUSE,
        ErrorKind::AddrNotAvailable   => ATOM_EADDRNOTAVAIL,
        ErrorKind::ConnectionRefused  => ATOM_ECONNREFUSED,
        ErrorKind::ConnectionReset    => ATOM_ECONNRESET,
        ErrorKind::ConnectionAborted  => ATOM_ECONNABORTED,
        ErrorKind::NotConnected       => ATOM_ENOTCONN,
        ErrorKind::BrokenPipe         => ATOM_EPIPE,
        ErrorKind::TimedOut           => ATOM_ETIMEDOUT,
        ErrorKind::NotFound           => ATOM_ENOENT,
        ErrorKind::AlreadyExists      => ATOM_EEXIST,
        _ => {
            match e.raw_os_error() {
                Some(21) => ATOM_EISDIR,
                Some(23) => ATOM_ENFILE,
                Some(24) => ATOM_EMFILE,
                Some(28) => ATOM_ENOSPC,
                _ => ATOM_EINVAL,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_and_cancel() {
        let pool = IoPool::new(1);

        // Submit lazily spawns workers
        let pid = 42;
        let id = pool.submit(crate::process::ProcessId::new(pid), Box::new(|| {
            // Simulate a slow I/O operation
            std::thread::sleep(std::time::Duration::from_millis(50));
            IoResult::Ok
        }));
        assert!(id > 0);
        pool.cancel_for_pid(crate::process::ProcessId::new(pid));

        // Give the worker time to process
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!pool.has_pending());

        pool.shutdown();
        pool.join();
    }

    // NERA-42: notification must be serialized with the worker's queue check.
    #[test]
    fn shutdown_notification_owns_queue_lock() {
        let pool = IoPool::new(1);
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let shutdown = scope.spawn(|| {
                barrier.wait();
                barrier.wait(); // parent has released its queue guard
                pool.shutdown_with(|| {
                    assert!(pool.shutdown.load(Ordering::SeqCst));
                    assert!(pool.queue.try_lock().is_none(), "notify without queue lock");
                });
            });
            // Worker owns the queue while deciding to wait. Release only after
            // the shutdown thread is started; no sleeps or timeout race probe.
            let queue = pool.queue.lock();
            assert!(!pool.shutdown.load(Ordering::SeqCst));
            barrier.wait();
            drop(queue);
            barrier.wait();
            shutdown.join().unwrap();
        });
    }

    #[test]
    fn shutdown_stops_workers() {
        let pool = IoPool::new(2);
        // Trigger lazy spawn
        pool.ensure_started();
        pool.shutdown();
        pool.join();
    }
}
