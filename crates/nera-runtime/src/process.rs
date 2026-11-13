//! Erlang process representation and mailbox.
//!
//! Each Erlang process has its own heap, a mailbox for message passing,
//! a reduction counter for preemptive scheduling, and a status field.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use crossbeam_queue::SegQueue;
use parking_lot::Mutex;

use crate::heap::ProcessHeap;
use crate::message::MessageFragment;
#[cfg(mio_supported)]
use crate::socket::{ActiveMode, PacketMode};
use crate::term::ErlTerm;

thread_local! {
    /// Saved variables for the current resume. Set by run_slice before calling
    /// the resume continuation, read by compiled code via nera_rt_load_resume_var.
    pub static RESUME_SAVED_VARS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

// ---------------------------------------------------------------------------
// Types and constants
// ---------------------------------------------------------------------------

/// A process identifier that fits its term payload without the external marker.
/// Zero is permitted for unassigned contexts; scheduler-issued identifiers start at 1.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidProcessId(pub u64);
impl std::fmt::Display for InvalidProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "process identifier exceeds its term domain: {}", self.0)
    }
}
impl std::error::Error for InvalidProcessId {}

impl ProcessId {
    pub const MAX: u64 = (!crate::term::EXTERNAL_BIT) >> crate::term::TAG_BITS;

    #[inline]
    pub const fn new(value: u64) -> Self {
        assert!(value <= Self::MAX, "process identifier exceeds its term domain");
        Self(value)
    }

    #[inline]
    pub const fn try_new(value: u64) -> Option<Self> {
        if value <= Self::MAX { Some(Self(value)) } else { None }
    }

    #[inline]
    pub const fn get(self) -> u64 { self.0 }

    /// # Safety
    /// value must fit ProcessId::MAX. Used after masking valid PID words or
    /// reading an atomic whose only writers accept validated identifiers.
    #[inline]
    pub(crate) const unsafe fn from_valid_raw(value: u64) -> Self {
        debug_assert!(value <= Self::MAX);
        Self(value)
    }
}
impl TryFrom<u64> for ProcessId {
    type Error = InvalidProcessId;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::try_new(value).ok_or(InvalidProcessId(value))
    }
}
impl From<ProcessId> for u64 {
    fn from(value: ProcessId) -> u64 { value.get() }
}
impl std::fmt::Display for ProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.get().fmt(f)
    }
}

/// Atomic storage whose private raw word is always a validated identifier.
pub(crate) struct AtomicProcessId(std::sync::atomic::AtomicU64);
impl AtomicProcessId {
    pub(crate) const fn new(value: ProcessId) -> Self {
        Self(std::sync::atomic::AtomicU64::new(value.get()))
    }
    #[inline]
    pub(crate) fn load(&self, order: std::sync::atomic::Ordering) -> ProcessId {
        // SAFETY: constructors and writers only accept validated identifiers.
        unsafe { ProcessId::from_valid_raw(self.0.load(order)) }
    }
    #[inline]
    pub(crate) fn store(&self, value: ProcessId, order: std::sync::atomic::Ordering) {
        self.0.store(value.get(), order);
    }
    pub(crate) fn compare_exchange(&self, current: ProcessId, new: ProcessId,
        success: std::sync::atomic::Ordering, failure: std::sync::atomic::Ordering,
    ) -> Result<ProcessId, ProcessId> {
        // SAFETY: all stored words satisfy the private invariant above.
        self.0.compare_exchange(current.get(), new.get(), success, failure)
            .map(|v| unsafe { ProcessId::from_valid_raw(v) })
            .map_err(|v| unsafe { ProcessId::from_valid_raw(v) })
    }
}

/// Unique nonzero identifiers. Multi-worker schedulers reserve disjoint blocks
/// of 64 per calling thread; single-worker schedulers use checked direct increments.
/// Unused IDs are abandoned when a thread exits or switches allocators. They are
/// never reclaimed. The global counter saturates at the first unencodable value;
/// already-reserved local IDs remain valid after global reservation exhaustion.
pub(crate) struct ProcessIdAllocator {
    cursor: std::sync::atomic::AtomicU64,
    use_blocks: bool,
}

struct PidReservation {
    // A Weak keeps the allocation identity alive without retaining the scheduler.
    // Even after its owner drops, its address cannot be reused while cached here.
    owner: std::sync::Weak<ProcessIdAllocator>,
    next: u64,
    end: u64,
}

thread_local! {
    // One bounded cache per OS thread, shared by Scheduler and SchedulerHandle.
    // Allocation invokes no callbacks and never holds this borrow across FFI/GC.
    static PID_RESERVATION: std::cell::RefCell<PidReservation> = const {
        std::cell::RefCell::new(PidReservation {
            owner: std::sync::Weak::new(), next: 0, end: 0,
        })
    };
}

impl ProcessIdAllocator {
    const BLOCK_SIZE: u64 = 64;

    pub(crate) fn new(workers: usize) -> Self {
        Self { cursor: std::sync::atomic::AtomicU64::new(1), use_blocks: workers > 1 }
    }

    #[cfg(test)]
    pub(crate) fn starting_at(value: u64) -> Self {
        assert!((1..=ProcessId::MAX + 1).contains(&value));
        Self { cursor: std::sync::atomic::AtomicU64::new(value), use_blocks: true }
    }

    /// Reserve a disjoint range, truncating the last block before any overflow.
    fn reserve(&self) -> Option<(u64, u64)> {
        use std::sync::atomic::Ordering::Relaxed;
        let start = self.cursor.try_update(Relaxed, Relaxed, |value| {
            if value <= ProcessId::MAX {
                Some(value + Self::BLOCK_SIZE.min(ProcessId::MAX + 1 - value))
            } else {
                None
            }
        }).ok()?;
        let end = start + Self::BLOCK_SIZE.min(ProcessId::MAX + 1 - start);
        Some((start, end))
    }

    #[inline]
    pub(crate) fn try_next(self: &Arc<Self>) -> Option<ProcessId> {
        if !self.use_blocks {
            // Single-worker schedulers avoid TLS overhead. External callers can
            // still race safely: this is the same bounded atomic allocator.
            use std::sync::atomic::Ordering::Relaxed;
            let value = self.cursor.try_update(Relaxed, Relaxed, |value| {
                if value <= ProcessId::MAX { Some(value + 1) } else { None }
            }).ok()?;
            // SAFETY: the successful update proves the returned value fits.
            return Some(unsafe { ProcessId::from_valid_raw(value) });
        }
        PID_RESERVATION.with(|slot| {
            let mut local = slot.borrow_mut();
            let same_owner = std::ptr::eq(local.owner.as_ptr(), Arc::as_ptr(self));
            if !same_owner || local.next == local.end {
                let (start, end) = self.reserve()?;
                if !same_owner {
                    local.owner = Arc::downgrade(self);
                }
                local.next = start;
                local.end = end;
            }
            let value = local.next;
            local.next += 1;
            // SAFETY: reserve only returns nonzero ranges within ProcessId::MAX.
            // The private cursor is used once per ID and only by its owning thread;
            // the Weak identity prevents reuse across unrelated allocators.
            Some(unsafe { ProcessId::from_valid_raw(value) })
        })
    }

    #[inline]
    pub(crate) fn next(self: &Arc<Self>) -> ProcessId {
        self.try_next().expect("process identifier domain exhausted")
    }
}

/// Maximum number of reductions a process is allowed before yielding.
pub const REDUCTION_LIMIT: i64 = 4000;

/// Maximum call-stack depth for non-tail recursion before raising
/// `{error, system_limit}`.  Tail calls are not affected because they
/// reuse the current frame.
pub const STACK_DEPTH_LIMIT: usize = 10_000;

/// How many reductions each back-edge yield check costs.
/// Increasing this reduces the effective check frequency:
/// e.g., COST=4 means ~1000 yield checks per slice instead of 4000.
/// The total time-slice duration stays the same (REDUCTION_LIMIT).
pub const YIELD_CHECK_COST: i64 = 1;

/// Process lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    Ready,
    Running,
    Waiting,
    Exited,
}

/// A complete scheduler lifecycle transition. Waiting always carries its cause.
/// This is Rust-only; generated code continues to use `ProcessContext::status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessState {
    Ready,
    Running,
    Waiting(WaitReason),
    Exited,
}

impl ProcessState {
    #[inline]
    fn atomic_status(self) -> u32 {
        match self {
            Self::Ready => ASTATUS_READY,
            Self::Running => ASTATUS_RUNNING,
            Self::Waiting(WaitReason::IoWait) => ASTATUS_IO_WAIT,
            Self::Waiting(_) => ASTATUS_WAITING,
            Self::Exited => ASTATUS_EXITED,
        }
    }
}

// ---------------------------------------------------------------------------
// ProcessEntry — lock-free wrapper for the process table
// ---------------------------------------------------------------------------

/// Atomic status values mirroring `ProcessStatus` for lock-free CAS operations.
pub const ASTATUS_READY: u32 = 0;
pub const ASTATUS_RUNNING: u32 = 1;
pub const ASTATUS_WAITING: u32 = 2;
pub const ASTATUS_EXITED: u32 = 3;
/// IO_WAIT: process is blocked on I/O (not wakeable by message delivery).
/// Only the I/O completion path (io_pool or reactor) should CAS from this.
pub const ASTATUS_IO_WAIT: u32 = 4;

/// Wrapper around a `Process` that provides a lock-free inbox for message
/// passing. The process table stores `Arc<ProcessEntry>` instead of
/// `Arc<Mutex<Process>>`.
///
/// Raw status mutation must not bypass the transition/wake protocol:
///
/// ```compile_fail
/// use nera_runtime::process::{Process, ProcessEntry};
/// use std::sync::atomic::Ordering;
/// let entry = ProcessEntry::new(Process::new(nera_runtime::process::ProcessId::new(1)));
/// entry.atomic_status.store(99, Ordering::Release);
/// ```
///
/// Lock ownership must be obtained through the entry, rather than exposing
/// its mutex or supplying a different process to drain:
///
/// ```compile_fail
/// use nera_runtime::process::{Process, ProcessEntry};
/// let entry = ProcessEntry::new(Process::new(nera_runtime::process::ProcessId::new(1)));
/// let _guard = entry.proc.lock();
/// ```
///
/// ```compile_fail
/// use nera_runtime::process::{Process, ProcessEntry};
/// let entry = ProcessEntry::new(Process::new(nera_runtime::process::ProcessId::new(1)));
/// let mut unrelated = Process::new(nera_runtime::process::ProcessId::new(2));
/// entry.drain_inbox(&mut unrelated);
/// ```
///
/// Scheduling fields and replacement of a locked process are not public mutations:
///
/// ```compile_fail
/// use nera_runtime::process::{Process, ProcessStatus};
/// let mut process = Process::new(nera_runtime::process::ProcessId::new(1));
/// process.status = ProcessStatus::Waiting;
/// ```
///
/// ```compile_fail
/// use nera_runtime::process::{Process, WaitReason};
/// let mut process = Process::new(nera_runtime::process::ProcessId::new(1));
/// process.otp.wait_reason = Some(WaitReason::IoWait);
/// ```
///
/// ```compile_fail
/// use nera_runtime::process::{Process, ProcessEntry};
/// let entry = ProcessEntry::new(Process::new(nera_runtime::process::ProcessId::new(1)));
/// let mut guard = entry.lock();
/// *guard = Process::new(nera_runtime::process::ProcessId::new(2));
/// ```
///
/// The hot path (cross-process send) only touches `inbox` (lock-free push)
/// and `atomic_status` (CAS for wake detection). The cold path (running the
/// process, GC, exit cleanup) locks `proc`.
pub struct ProcessEntry {
    /// Lock-free MPSC inbox. Senders push `MessageFragment`s here without
    /// needing to lock the target process's Mutex.
    pub inbox: SegQueue<MessageFragment>,
    /// Atomic mirror of `ProcessStatus` for lock-free wake detection.
    /// Senders CAS this from `WAITING → READY` to wake a process.
    atomic_status: AtomicU32,
    /// Set when an EXIT signal is pushed to the inbox. The worker checks
    /// this before scanning the mailbox for EXIT tuples, avoiding O(N)
    /// scans on every wake when no signals are pending.
    pub has_exit_signals: AtomicBool,
    /// The actual process state, protected by a Mutex.
    proc: Mutex<Process>,
    /// Sticky worker affinity for Phase 8 worker-local dispatch.
    /// Updated to the running worker's id when a worker actually executes
    /// this process (not when the process is pushed into a worker's inbox).
    /// `u8::MAX` (255) means "unhomed — use round-robin on first dispatch".
    /// Read by dispatchers to decide which worker's inbox to target.
    pub last_worker: AtomicU8,
    /// Group leader PID, independently accessible without locking the process.
    group_leader: AtomicProcessId,
}

impl ProcessEntry {
    /// Create an entry ready for its initial queue insertion.
    /// The atomic state starts Ready even for a pre-existing waiting process:
    /// its initial queue item already owns the runnable slot. Publishing Waiting
    /// here would let a sender enqueue it a second time before its first slice.
    pub fn new(process: Process) -> Self {
        ProcessEntry {
            inbox: SegQueue::new(),
            atomic_status: AtomicU32::new(ASTATUS_READY),
            has_exit_signals: AtomicBool::new(false),
            group_leader: AtomicProcessId::new(process.id),
            proc: Mutex::new(process),
            last_worker: AtomicU8::new(u8::MAX),
        }
    }

    pub fn group_leader(&self) -> ProcessId {
        self.group_leader.load(Ordering::Acquire)
    }

    pub fn set_group_leader(&self, pid: ProcessId) {
        self.group_leader.store(pid, Ordering::Release);
    }

    /// Observe the published scheduling status without acquiring the process lock.
    #[inline]
    pub fn published_status(&self) -> u32 {
        self.atomic_status.load(Ordering::Acquire)
    }

    /// Lock this entry's process, retaining the association for guarded operations.
    #[inline]
    pub fn lock(&self) -> ProcessGuard<'_> {
        ProcessGuard {
            entry: self,
            process: self.proc.lock(),
        }
    }

    /// Attempt to lock without blocking, including during reentrant runtime calls.
    #[inline]
    pub fn try_lock(&self) -> Option<ProcessGuard<'_>> {
        self.proc.try_lock().map(|process| ProcessGuard {
            entry: self,
            process,
        })
    }

    /// Runtime-only mutable access; ordinary callers cannot replace a locked
    /// process or bypass published transitions through mutable dereferencing.
    #[inline]
    pub(crate) fn lock_for_runtime(&self) -> RuntimeProcessGuard<'_> {
        RuntimeProcessGuard(self.lock())
    }

    #[inline]
    pub(crate) fn try_lock_for_runtime(&self) -> Option<RuntimeProcessGuard<'_>> {
        self.try_lock().map(RuntimeProcessGuard)
    }

    /// Lock a ready or receive-waiting process so a sender can append a
    /// message straight to its mailbox, bypassing the inbox.
    ///
    /// Returns `None` when the sender must push to the inbox instead: the
    /// process is running, waiting for I/O or exited, another thread holds
    /// its lock, or its inbox still holds messages. Inbox messages reach the
    /// mailbox only when the process next runs, so a message appended
    /// directly would overtake them, including an earlier message from the
    /// same sender. Erlang delivers the messages one process sends another in
    /// the order they were sent.
    #[inline]
    pub(crate) fn try_lock_for_direct_send(&self) -> Option<RuntimeProcessGuard<'_>> {
        let status = self.published_status();
        if status != ASTATUS_WAITING && status != ASTATUS_READY {
            return None;
        }
        let guard = self.try_lock_for_runtime()?;
        // Only a lock holder drains the inbox, so while this guard is held
        // nothing is between the inbox and the mailbox, and the sender's own
        // earlier pushes are visible here.
        self.inbox.is_empty().then_some(guard)
    }

    /// Attempt to wake a process waiting for a message via CAS (`Waiting → Ready`).
    /// Only wakes RECEIVE_WAIT processes — NOT IO_WAIT processes.
    /// Returns `true` if the transition succeeded.
    pub fn try_wake(&self) -> bool {
        self.atomic_status
            .compare_exchange(
                ASTATUS_WAITING,
                ASTATUS_READY,
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    /// Attempt to wake a process waiting for I/O via CAS (`IO_WAIT → Ready`).
    /// Only the I/O completion path should call this.
    /// Returns `true` if the transition succeeded.
    pub fn try_wake_io(&self) -> bool {
        self.atomic_status
            .compare_exchange(
                ASTATUS_IO_WAIT,
                ASTATUS_READY,
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_ok()
    }
}

/// A process lock tied to the entry that owns its inbox and published status.
///
/// Obtain it through [`ProcessEntry::lock`] or [`ProcessEntry::try_lock`].
/// Dropping it releases the existing mutex; no second lock is acquired by its
/// transition or inbox operations. Shared dereferencing permits observation;
/// full mutable access and process replacement are reserved to the runtime.
///
/// ```
/// use nera_runtime::process::{Process, ProcessEntry};
/// let entry = ProcessEntry::new(Process::new(nera_runtime::process::ProcessId::new(1)));
/// let mut process = entry.lock();
/// assert!(entry.try_lock().is_none());
/// process.drain_inbox();
/// drop(process);
/// assert!(entry.try_lock().is_some());
/// ```
#[must_use = "dropping the guard releases the process lock"]
pub struct ProcessGuard<'a> {
    entry: &'a ProcessEntry,
    process: parking_lot::MutexGuard<'a, Process>,
}

impl std::ops::Deref for ProcessGuard<'_> {
    type Target = Process;

    #[inline]
    fn deref(&self) -> &Process {
        &self.process
    }
}

impl ProcessGuard<'_> {
    /// Publish a worker-owned transition for this locked entry.
    /// Update the locked fields before the existing Release store. Queue
    /// insertion, runnable accounting and inbox race checks stay in the scheduler;
    /// wake CAS operations still reconcile the locked state on the next slice.
    #[inline(always)]
    pub(crate) fn transition(&mut self, next: ProcessState) {
        self.process.transition_to(next);
        self.entry
            .atomic_status
            .store(next.atomic_status(), Ordering::Release);
    }

    /// Drain all messages from the lock-free inbox into the process mailbox.
    /// The guard supplies both the entry and its locked destination process.
    ///
    /// Batch-collects all fragments first, then processes them in a tight
    /// loop. This reduces per-message overhead from repeated SegQueue pops
    /// interleaved with heap operations.
    #[inline]
    pub fn drain_inbox(&mut self) {
        drain_inbox_into(self.entry, &mut self.process);
    }
}

/// Full mutable access is confined to scheduler, I/O and FFI execution scopes.
/// It wraps the same owning guard and never acquires another lock.
#[must_use = "dropping the guard releases the process lock"]
#[repr(transparent)]
pub(crate) struct RuntimeProcessGuard<'a>(ProcessGuard<'a>);

impl std::ops::Deref for RuntimeProcessGuard<'_> {
    type Target = Process;
    #[inline]
    fn deref(&self) -> &Process {
        &self.0.process
    }
}

impl std::ops::DerefMut for RuntimeProcessGuard<'_> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Process {
        &mut self.0.process
    }
}

impl RuntimeProcessGuard<'_> {
    #[inline(always)]
    pub(crate) fn transition(&mut self, next: ProcessState) {
        self.0.transition(next);
    }

    #[inline]
    pub(crate) fn drain_inbox(&mut self) {
        self.0.drain_inbox();
    }
}

// Only the owning guard supplies this pair. Keep the original reference-based
// core so the inlined adapter adds no guard-pointer indirection to the loop.
fn drain_inbox_into(entry: &ProcessEntry, proc: &mut Process) {
    // Fast path: single pop attempt for the common case (0-1 messages).
    let first = match entry.inbox.pop() {
        Some(f) => f,
        None => return,
    };

    // Check if there's more than one message.
    let second = entry.inbox.pop();
    if second.is_none() {
        // Single message — process inline, no Vec allocation.
        match first {
            MessageFragment::ActiveReadReady {
                stream_handle,
                in_flight,
                ..
            } => {
                if proc.status == ProcessStatus::Exited {
                    // Process is dead — skip the read to avoid wasting
                    // I/O on a heap that will be discarded.  Clear
                    // in_flight so the reactor doesn't spin re-arming
                    // the edge trigger.
                    in_flight.store(false, std::sync::atomic::Ordering::Release);
                } else {
                    handle_active_read_on_worker(proc, stream_handle, &in_flight);
                }
            }
            other => {
                let msg = other.copy_to_heap(&mut proc.heap);
                proc.mailbox.send(msg);
            }
        }
        return;
    }

    // Multiple messages: batch-collect all remaining fragments.
    let mut fragments = Vec::with_capacity(16);
    fragments.push(first);
    fragments.push(second.unwrap());
    while let Some(frag) = entry.inbox.pop() {
        fragments.push(frag);
    }

    // Process all fragments in a tight loop.
    for frag in fragments {
        match frag {
            MessageFragment::ActiveReadReady {
                stream_handle,
                in_flight,
                ..
            } => {
                if proc.status == ProcessStatus::Exited {
                    in_flight.store(false, std::sync::atomic::Ordering::Release);
                } else {
                    handle_active_read_on_worker(proc, stream_handle, &in_flight);
                }
            }
            other => {
                let msg = other.copy_to_heap(&mut proc.heap);
                proc.mailbox.send(msg);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Worker-side active-mode TCP read loop
// ---------------------------------------------------------------------------

/// Handle an active-mode readiness notification on a worker thread.
/// Performs the actual `recvfrom` syscall(s), builds `{tcp, Socket, Data}`
/// directly on the process heap, and re-arms the mio edge trigger.
#[cfg(mio_supported)]
fn handle_active_read_on_worker(
    proc: &mut Process,
    stream_handle: crate::socket::SocketHandle,
    in_flight: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use std::io::ErrorKind;
    use std::sync::atomic::Ordering;

    // Read socket options (active_mode, packet_mode) fresh — avoids stale values.
    let opts = match crate::socket::get_opts(stream_handle) {
        Some(opts) if opts.active != ActiveMode::Passive => opts,
        _ => {
            // Socket closed or went passive between dispatch and drain.
            in_flight.store(false, Ordering::Release);
            return;
        }
    };

    // Get raw fd (and hold Arc<TcpStream> alive so fd cannot be reused).
    let stream = match crate::socket::get_stream(stream_handle) {
        Ok(s) => s,
        Err(_) => {
            in_flight.store(false, Ordering::Release);
            return;
        }
    };
    let fd = {
        use std::os::unix::io::AsRawFd;
        stream.as_raw_fd()
    };

    let active_mode = opts.active;
    let packet_mode = opts.packet;
    // Use set_active_local (not set_active) throughout — the reactor's
    // lazy deregister handles cleanup when it sees active==0 on the next
    // event, avoiding a DeregisterActive → kevent syscall per cycle.
    if packet_mode == PacketMode::Line {
        // ── Line mode: peek + consume_exact with per-socket line buffer ──
        //
        // Uses peek to find newlines, then consume_exact to remove only
        // the delivered line from the kernel buffer. Excess data (e.g. a
        // data block after the command line) stays in the kernel for
        // subsequent gen_tcp:recv calls in raw mode.
        //
        // When a partial line has no newline, the partial data is consumed
        // from the kernel into a per-socket line buffer so the fd is no
        // longer readable — preventing edge-triggered kqueue spin. The
        // line buffer is prepended on the next readability event.
        let mut line_buf = crate::socket::take_line_buffer(stream_handle);
        let mut peek_buf = [0u8; 8192];
        let mut got_eof = false;
        loop {
            match crate::socket::peek_recv_fd(fd, &mut peek_buf) {
                Ok(0) => {
                    got_eof = true;
                    break;
                }
                Ok(n) => {
                    if let Some(ppos) = peek_buf[..n].iter().position(|&b| b == b'\n') {
                        // Newline found in peeked data — consume up to and
                        // including the newline, leaving excess in kernel.
                        let consume_len = ppos + 1;
                        let mut tmp = vec![0u8; consume_len];
                        let _ = crate::socket::consume_exact_fd(fd, &mut tmp);

                        // Build the line: line_buf (partial) + peek_buf[..ppos],
                        // stripping trailing \r.
                        let line_end = if ppos > 0 && peek_buf[ppos - 1] == b'\r' {
                            ppos - 1
                        } else if ppos == 0 && line_buf.last() == Some(&b'\r') {
                            // \r was last byte of line_buf, \n is first byte of peek
                            line_buf.pop();
                            0
                        } else {
                            ppos
                        };

                        if line_buf.is_empty() {
                            build_tcp_data(proc, stream_handle, &peek_buf[..line_end]);
                        } else {
                            line_buf.extend_from_slice(&peek_buf[..line_end]);
                            build_tcp_data(proc, stream_handle, &line_buf);
                            line_buf.clear();
                        }

                        if active_mode == ActiveMode::Once {
                            crate::socket::set_active_local(stream_handle, ActiveMode::Passive);
                            // line_buf is empty; remaining data in kernel.
                            break;
                        }
                        // active == true: continue loop for more lines
                    } else {
                        // No newline — consume all peeked data into line_buf
                        // so the kernel buffer drains (prevents kqueue spin).
                        let mut tmp = vec![0u8; n];
                        let _ = crate::socket::consume_exact_fd(fd, &mut tmp);
                        line_buf.extend_from_slice(&peek_buf[..n]);
                        break; // wait for next readability event
                    }
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(ref e)
                    if e.kind() == ErrorKind::ConnectionReset
                        || e.kind() == ErrorKind::ConnectionAborted =>
                {
                    got_eof = true;
                    break;
                }
                Err(ref e) => {
                    build_tcp_error(proc, stream_handle, e);
                    crate::socket::set_active_local(stream_handle, ActiveMode::Passive);
                    break;
                }
            }
        }
        if got_eof {
            // Deliver any remaining partial line before tcp_closed,
            // matching BEAM behavior for incomplete lines at EOF.
            if !line_buf.is_empty() {
                let end = if line_buf.last() == Some(&b'\r') {
                    line_buf.len() - 1
                } else {
                    line_buf.len()
                };
                if end > 0 {
                    build_tcp_data(proc, stream_handle, &line_buf[..end]);
                }
                line_buf.clear();
            }
            build_tcp_closed(proc, stream_handle);
            crate::socket::set_active_local(stream_handle, ActiveMode::Passive);
        }
        // Save any remaining partial-line data for the next event.
        crate::socket::set_line_buffer(stream_handle, line_buf);
    } else {
        // ── Raw mode: consume whatever is available ──
        let mut buf = [0u8; 4096];
        loop {
            match crate::socket::consume_fd(fd, &mut buf) {
                Ok(0) => {
                    build_tcp_closed(proc, stream_handle);
                    crate::socket::set_active_local(stream_handle, ActiveMode::Passive);
                    break;
                }
                Ok(n) => {
                    build_tcp_data(proc, stream_handle, &buf[..n]);
                    if active_mode == ActiveMode::Once {
                        crate::socket::set_active_local(stream_handle, ActiveMode::Passive);
                        break;
                    }
                    // active == ActiveMode::Active (true): continue loop
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(ref e)
                    if e.kind() == ErrorKind::ConnectionReset
                        || e.kind() == ErrorKind::ConnectionAborted =>
                {
                    build_tcp_closed(proc, stream_handle);
                    crate::socket::set_active_local(stream_handle, ActiveMode::Passive);
                    break;
                }
                Err(ref e) => {
                    build_tcp_error(proc, stream_handle, e);
                    crate::socket::set_active_local(stream_handle, ActiveMode::Passive);
                    break;
                }
            }
        }
    }

    // Clear in_flight, then ask the reactor to re-arm the edge trigger —
    // but only if the socket is still active.  For {active, once} the read
    // above set active=0, so this skips RearmActive (avoids unnecessary
    // reregister → ghost event → lazy deregister cycle).  For {active, true}
    // that reads until WouldBlock, active stays >0 and re-arm is needed.
    in_flight.store(false, Ordering::Release);
    let still_active = crate::socket::get_opts(stream_handle).is_some_and(|o| o.active != ActiveMode::Passive);
    if still_active {
        if let Some(reactor) = crate::reactor::global_reactor() {
            reactor.send_command(crate::reactor::ReactorCommand::RearmActive { stream_handle });
        }
    }
}

// Non-mio platforms use reader threads that deliver owned message fragments,
// never reactor readiness notifications. Release a synthetic/stale notification
// without referencing Unix descriptors or the unavailable reactor module.
#[cfg(not(mio_supported))]
fn handle_active_read_on_worker(
    _proc: &mut Process,
    _stream_handle: crate::socket::SocketHandle,
    in_flight: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    in_flight.store(false, Ordering::Release);
}

// ---------------------------------------------------------------------------
// Message construction helpers — build directly on process heap
// ---------------------------------------------------------------------------

/// Build `{tcp, Socket, Data}` on the process heap and push to mailbox.
#[cfg(mio_supported)]
fn build_tcp_data(proc: &mut Process, handle: crate::socket::SocketHandle, bytes: &[u8]) {
    use crate::atom_table::global_atom_table;
    use crate::term::ErlTerm;

    let tcp_atom_id = global_atom_table().intern("tcp");
    let data_bin = proc.heap.alloc_binary_auto(bytes);
    let sock_term = handle.to_term();
    let msg = proc
        .heap
        .alloc_tuple(&[ErlTerm::atom(tcp_atom_id), sock_term, data_bin]);
    proc.mailbox.send(msg);
}

/// Build `{tcp_closed, Socket}` on the process heap and push to mailbox.
#[cfg(mio_supported)]
fn build_tcp_closed(proc: &mut Process, handle: crate::socket::SocketHandle) {
    use crate::atom_table::global_atom_table;
    use crate::term::ErlTerm;

    let tcp_closed_id = global_atom_table().intern("tcp_closed");
    let sock_term = handle.to_term();
    let msg = proc
        .heap
        .alloc_tuple(&[ErlTerm::atom(tcp_closed_id), sock_term]);
    proc.mailbox.send(msg);
}

/// Build `{tcp_error, Socket, Reason}` on the process heap and push to mailbox.
#[cfg(mio_supported)]
fn build_tcp_error(proc: &mut Process, handle: crate::socket::SocketHandle, err: &std::io::Error) {
    use crate::atom_table::global_atom_table;
    use crate::term::ErlTerm;

    let tcp_error_id = global_atom_table().intern("tcp_error");
    let sock_term = handle.to_term();
    let reason_id = crate::ffi::helpers::io_error_to_atom_id(err);
    let msg = proc.heap.alloc_tuple(&[
        ErlTerm::atom(tcp_error_id),
        sock_term,
        ErlTerm::atom(reason_id),
    ]);
    proc.mailbox.send(msg);
}

// ---------------------------------------------------------------------------
// OTP state and monitor/timer types
// ---------------------------------------------------------------------------

/// Reference identifier for a process monitor relationship.
pub type MonitorRef = u64;
/// Runtime timer identifier owned by a process.
pub type TimerId = u64;
/// Atom ID used as a registered process name.
pub type RegisteredName = u32;

/// Why a process is currently in `ProcessStatus::Waiting`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitReason {
    /// Waiting in a `receive` loop. Optional absolute timeout in ms.
    Receive { timeout_deadline_ms: Option<u64> },
    /// Placeholder for future link signal wait states.
    LinkSignal,
    /// Placeholder for future monitor signal wait states.
    MonitorSignal,
    /// Waiting on a timer wake-up. Optional absolute timeout in ms.
    Timer {
        timer_id: TimerId,
        deadline_ms: Option<u64>,
    },
    /// Waiting for an async I/O operation to complete in the I/O pool.
    IoWait,
}

/// The result of a completed async I/O operation.
///
/// Uses raw Rust types (not `ErlTerm`) because I/O pool threads cannot
/// allocate on process heaps. The FFI function converts this to heap-allocated
/// terms when the process resumes.
#[derive(Debug)]
pub enum IoResult {
    /// Successful read: raw bytes (for recv, read_file).
    OkBytes(Vec<u8>),
    /// Successful operation that produced a handle (for accept).
    OkHandle(crate::socket::SocketHandle),
    /// Successful write/send with no payload.
    Ok,
    /// A socket was closed while its operation was pending.
    Closed,
    /// I/O error.
    Err(std::io::Error),
}

/// Long-lived OTP-oriented per-process state.
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct OtpProcessState {
    /// Current wait reason when status is `Waiting`.
    wait_reason: Option<WaitReason>,
    /// Bi-directional link peers (PIDs).
    pub links: HashSet<ProcessId>,
    /// Outgoing monitors: ref -> target pid.
    pub monitors_outgoing: HashMap<MonitorRef, ProcessId>,
    /// Incoming monitors: ref -> owner pid.
    pub monitors_incoming: HashMap<MonitorRef, ProcessId>,
    /// Optional registered name for this process (atom id).
    pub registered_name: Option<RegisteredName>,
    /// Timer ids currently owned by this process.
    pub owned_timers: HashSet<TimerId>,
    /// Whether EXIT signals are trapped as messages instead of killing.
    pub trap_exit: bool,
    /// Low priority receives one eighth of the normal reduction quantum.
    pub low_priority: bool,
}


// ---------------------------------------------------------------------------
// ProcessContext - the C-ABI context passed to compiled code
// ---------------------------------------------------------------------------

/// Result of one compiled execution slice; distinct from scheduler lifecycle state.
/// Generated code must store only these discriminants into the context slot.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionStatus {
    Ok = 0,
    Yield = 1,
    ReceiveWait = 2,
    Exception = 3,
    IoWait = 4,
    Hibernate = 5,
    TailCall = 6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidExecutionStatus(pub u32);
impl std::fmt::Display for InvalidExecutionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown execution status {}", self.0)
    }
}
impl std::error::Error for InvalidExecutionStatus {}

impl TryFrom<u32> for ExecutionStatus {
    type Error = InvalidExecutionStatus;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Ok),
            1 => Ok(Self::Yield),
            2 => Ok(Self::ReceiveWait),
            3 => Ok(Self::Exception),
            4 => Ok(Self::IoWait),
            5 => Ok(Self::Hibernate),
            6 => Ok(Self::TailCall),
            other => Err(InvalidExecutionStatus(other)),
        }
    }
}

pub const STATUS_OK: ExecutionStatus = ExecutionStatus::Ok;
pub const STATUS_YIELD: ExecutionStatus = ExecutionStatus::Yield;
pub const STATUS_RECEIVE_WAIT: ExecutionStatus = ExecutionStatus::ReceiveWait;
pub const STATUS_EXCEPTION: ExecutionStatus = ExecutionStatus::Exception;
pub const STATUS_IO_WAIT: ExecutionStatus = ExecutionStatus::IoWait;
pub const STATUS_HIBERNATE: ExecutionStatus = ExecutionStatus::Hibernate;
pub const STATUS_TAIL_CALL: ExecutionStatus = ExecutionStatus::TailCall;

/// One stack frame entry for crash reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct StackFrame {
    pub module: u32,
    pub function: u32,
    pub arity: u32,
    /// Source line number (1-based). 0 means unknown.
    pub line: u32,
}

/// Compact stack frame used by compiled code. Only stores a function
/// index (into the global func_info_table) and a line number.
/// The full MFA is reconstructed on demand for exception traces.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactFrame {
    pub func_index: u32,
    pub line: u32,
}

/// The context passed to compiled code via pointer.
/// ```compile_fail
/// use nera_runtime::process::ProcessContext;
/// fn corrupt(ctx: &mut ProcessContext) { ctx.status = 123; }
/// ```
///
/// Compiled functions receive a `*mut ProcessContext` as their first
/// argument so they can decrement reductions and signal yield/wait.
/// Process execution contexts may move between workers, but are not shared
/// concurrently without the owning process lock.
///
/// ```compile_fail
/// use nera_runtime::process::ProcessContext;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ProcessContext>();
/// ```
#[repr(C)]
pub struct ProcessContext {
    pub pid: ProcessId,
    /// Remaining reductions. Compiled code decrements this.
    pub reductions: i64,
    /// Set by compiled code using a declared ExecutionStatus discriminant.
    pub status: ExecutionStatus,
    _padding: u32,
    /// The return value or exception term (raw `ErlTerm` bits).
    pub result: u64,
    /// Secondary return value. Used by `nera_rt_bs_start_match2` to return
    /// the match context term alongside base_ptr (return value) and
    /// total_bits (result).
    pub result2: u64,
    /// Pointer to the process's heap. Set by the scheduler before calling
    /// compiled code. FFI functions use this instead of a thread-local heap.
    pub heap_ptr: *mut ProcessHeap,
    /// Pointer to the process's mailbox. Set by the scheduler before calling
    /// compiled code so FFI functions can access the mailbox without locking
    /// the process table (which would deadlock since we're already inside
    /// the process's locked context).
    pub mailbox_ptr: *mut Mailbox,
    /// Set to 1 by the runtime before calling a resume continuation.
    /// Compiled code checks this flag at function entry: if non-zero,
    /// it jumps directly to the receive-msg check block, skipping any
    /// pre-receive side effects (like sends).
    pub resume_receive: u32,
    /// Exception class: 0=throw, 1=error, 2=exit.
    pub exception_class: u32,
    /// Absolute deadline (monotonic ms) for receive timeout.
    /// 0 means no timeout (wait indefinitely).
    pub receive_timeout_deadline: u64,
    /// Pointer to the process call stack (Vec<StackFrame>) maintained by
    /// runtime FFI hooks. Set by the scheduler before calling compiled code.
    pub stack_ptr: *mut Vec<StackFrame>,
    /// Pointer to the captured exception trace (Vec<StackFrame>) used for
    /// crash reporting. Set by the scheduler before calling compiled code.
    pub exception_trace_ptr: *mut Vec<StackFrame>,
    /// Pointer to the process dictionary (Vec<(ErlTerm, ErlTerm)>).
    /// Set by run_slice before calling compiled code so FFI functions
    /// can access the dictionary without re-locking the process Mutex.
    pub dict_ptr: *mut Vec<(ErlTerm, ErlTerm)>,
    /// Pointer to the OTP process state (links, monitors, trap_exit).
    /// Set by run_slice before calling compiled code so FFI functions
    /// can modify links/monitors without re-locking the process Mutex.
    pub otp_ptr: *mut OtpProcessState,
    /// Inline heap allocation cursor. Points to the next free word in the
    /// pre-allocated inline region. Compiled code bumps this forward.
    pub heap_cursor: *mut u64,
    /// End of the inline heap region. When heap_cursor + needed > heap_limit,
    /// compiled code falls back to FFI for allocation + refill.
    pub heap_limit: *mut u64,
    /// Cursor-based compact call stack for compiled code. Points to next free slot.
    pub stack_cursor: *mut CompactFrame,
    /// End of the compact stack array (base + STACK_DEPTH_LIMIT).
    pub stack_limit: *mut CompactFrame,
    /// Start of the compact stack array.
    pub stack_base: *mut CompactFrame,
    /// Tail call trampoline: kind of pending tail call (0=none, 1=apply, 2=closure).
    pub tail_call_kind: u32,
    _tail_padding: u32,
    /// For apply tail calls: module atom term.
    pub tail_call_mod: u64,
    /// For apply tail calls: function atom term.
    pub tail_call_fun: u64,
    /// For apply tail calls: args list term. For closure tail calls: closure term.
    pub tail_call_arg1: u64,
    /// For closure tail calls: args list term.
    pub tail_call_arg2: u64,
    /// Pointer to the SchedulerHandle for this runtime instance.
    /// Set by run_slice() before calling compiled code so FFI functions
    /// can access the scheduler without the global singleton. Its Arc owner must
    /// keep the handle alive through compiled execution and nested FFI calls;
    /// this raw pointer does not extend the owner's lifetime.
    pub runtime: *const crate::scheduler::SchedulerHandle,
    /// Private, unfinished native binary work. Contains no terms or borrowed data.
    /// Dropped with the process on exit, including cancellation while yielded.
    pub(crate) native_work: Option<Box<dyn crate::native_work::BinaryWork>>,
    /// Native list decode state has no terms; the remaining list is rooted in
    /// the continuation. Dropping the process releases any unfinished output.
    pub(crate) base64_list_decode: Option<Box<crate::base64::ListDecoder>>,
    /// Explicit garbage_collect/0 waits for a full collection at the next safe point.
    pub(crate) full_gc_requested: bool,
    /// The current receive's `after` value is not a valid timeout. Written by
    /// every `receive ... after` when it starts and read only by that
    /// receive's timeout check, which raises `timeout_value` once the mailbox
    /// scan finds no match: a matching message is received first, as on BEAM.
    pub(crate) receive_timeout_invalid: bool,
}

/// ProcessContext field offsets (source of truth for codegen/runtime ABI).
pub const CTX_OFFSET_PID: i32 = std::mem::offset_of!(ProcessContext, pid) as i32;
pub const CTX_OFFSET_REDUCTIONS: i32 = std::mem::offset_of!(ProcessContext, reductions) as i32;
pub const CTX_OFFSET_STATUS: i32 = std::mem::offset_of!(ProcessContext, status) as i32;
pub const CTX_OFFSET_RESULT: i32 = std::mem::offset_of!(ProcessContext, result) as i32;
pub const CTX_OFFSET_RESULT2: i32 = std::mem::offset_of!(ProcessContext, result2) as i32;
pub const CTX_OFFSET_HEAP_PTR: i32 = std::mem::offset_of!(ProcessContext, heap_ptr) as i32;
pub const CTX_OFFSET_MAILBOX_PTR: i32 = std::mem::offset_of!(ProcessContext, mailbox_ptr) as i32;
pub const CTX_OFFSET_RESUME_RECEIVE: i32 =
    std::mem::offset_of!(ProcessContext, resume_receive) as i32;
pub const CTX_OFFSET_EXCEPTION_CLASS: i32 =
    std::mem::offset_of!(ProcessContext, exception_class) as i32;
pub const CTX_OFFSET_RECEIVE_TIMEOUT: i32 =
    std::mem::offset_of!(ProcessContext, receive_timeout_deadline) as i32;
pub const CTX_OFFSET_STACK_PTR: i32 = std::mem::offset_of!(ProcessContext, stack_ptr) as i32;
pub const CTX_OFFSET_EXCEPTION_TRACE_PTR: i32 =
    std::mem::offset_of!(ProcessContext, exception_trace_ptr) as i32;
pub const CTX_OFFSET_HEAP_CURSOR: i32 = std::mem::offset_of!(ProcessContext, heap_cursor) as i32;
pub const CTX_OFFSET_HEAP_LIMIT: i32 = std::mem::offset_of!(ProcessContext, heap_limit) as i32;
pub const CTX_OFFSET_STACK_CURSOR: i32 = std::mem::offset_of!(ProcessContext, stack_cursor) as i32;
pub const CTX_OFFSET_STACK_LIMIT: i32 = std::mem::offset_of!(ProcessContext, stack_limit) as i32;
pub const CTX_OFFSET_STACK_BASE: i32 = std::mem::offset_of!(ProcessContext, stack_base) as i32;
pub const CTX_OFFSET_TAIL_CALL_KIND: i32 =
    std::mem::offset_of!(ProcessContext, tail_call_kind) as i32;
pub const CTX_OFFSET_TAIL_CALL_MOD: i32 =
    std::mem::offset_of!(ProcessContext, tail_call_mod) as i32;
pub const CTX_OFFSET_TAIL_CALL_FUN: i32 =
    std::mem::offset_of!(ProcessContext, tail_call_fun) as i32;
pub const CTX_OFFSET_TAIL_CALL_ARG1: i32 =
    std::mem::offset_of!(ProcessContext, tail_call_arg1) as i32;
pub const CTX_OFFSET_TAIL_CALL_ARG2: i32 =
    std::mem::offset_of!(ProcessContext, tail_call_arg2) as i32;
pub const CTX_OFFSET_RUNTIME: i32 = std::mem::offset_of!(ProcessContext, runtime) as i32;

/// Size of a CompactFrame in bytes (2 × u32 = 8).
pub const COMPACT_FRAME_SIZE: i32 = std::mem::size_of::<CompactFrame>() as i32;
/// Field offsets within CompactFrame.
pub const CF_OFFSET_FUNC_INDEX: i32 = std::mem::offset_of!(CompactFrame, func_index) as i32;
pub const CF_OFFSET_LINE: i32 = std::mem::offset_of!(CompactFrame, line) as i32;

// SAFETY: execution contexts move with their owning Process between workers.
// The process mutex grants exclusive execution; run_slice refreshes pointers to
// the heap, mailbox, stacks, dictionary and OTP state before compiled code runs.
// Raw-pointer dereferences remain unsafe and require that exclusive ownership.
// No safe context method dereferences these pointers. Sync is deliberately not
// implemented: the pointees include mutable, non-Sync process-local storage.
unsafe impl Send for ProcessContext {}

impl ProcessContext {
    pub fn new(pid: ProcessId) -> Self {
        ProcessContext {
            pid,
            reductions: REDUCTION_LIMIT,
            status: STATUS_OK,
            _padding: 0,
            result: ErlTerm::nil().0,
            result2: 0,
            heap_ptr: std::ptr::null_mut(),
            mailbox_ptr: std::ptr::null_mut(),
            resume_receive: 0,
            exception_class: 0,
            receive_timeout_deadline: 0,
            stack_ptr: std::ptr::null_mut(),
            exception_trace_ptr: std::ptr::null_mut(),
            dict_ptr: std::ptr::null_mut(),
            otp_ptr: std::ptr::null_mut(),
            heap_cursor: std::ptr::null_mut(),
            heap_limit: std::ptr::null_mut(),
            stack_cursor: std::ptr::null_mut(),
            stack_limit: std::ptr::null_mut(),
            stack_base: std::ptr::null_mut(),
            tail_call_kind: 0,
            _tail_padding: 0,
            tail_call_mod: 0,
            tail_call_fun: 0,
            tail_call_arg1: 0,
            tail_call_arg2: 0,
            runtime: std::ptr::null(),
            native_work: None,
            base64_list_decode: None,
            full_gc_requested: false,
            receive_timeout_invalid: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Function pointer types for compiled code
// ---------------------------------------------------------------------------

/// Function pointer type for compiled Erlang functions.
/// ```compile_fail
/// use nera_runtime::process::ErlFn;
/// fn call_without_contract(f: ErlFn) {
///     f(std::ptr::null_mut(), std::ptr::null(), 0);
/// }
/// ```
/// # Safety
/// ctx must identify the live exclusively executing process context, with all
/// referenced runtime storage valid. args must contain nargs readable words
/// (including a non-null aligned pointer when nonempty) whose term graphs are
/// valid on entry; nargs must match this function's calling convention, including
/// any captured environment arguments. The callee must root arguments before collection and obey
/// the runtime reentrancy contract; callers must not retain heap borrows across
/// the call or read stale argument graphs after a moving GC. The function's
/// code must remain loaded throughout every call and stored continuation.
/// Context PID and status writes must preserve their declared typed domains.
pub type ErlFn = unsafe extern "C" fn(ctx: *mut ProcessContext, args: *const u64, nargs: u32);

/// A continuation: a function to call next, with arguments.
pub struct Continuation {
    pub func: ErlFn,
    pub args: Vec<ErlTerm>,
    /// Which receive point to resume at (1-indexed; 0 = none).
    pub resume_receive_index: u32,
    /// Variables that are live across the receive point. These are saved
    /// by compiled code before entering receive-wait and restored when
    /// the process resumes.
    pub saved_vars: Vec<ErlTerm>,
}

// ---------------------------------------------------------------------------
// Process
// ---------------------------------------------------------------------------

/// Type alias for the Rust-level callback used for testing the scheduler.
pub type RustFn = Box<dyn FnMut(&mut ProcessInner) + Send>;

pub struct Process {
    pub id: ProcessId,
    status: ProcessStatus,
    pub heap: ProcessHeap,
    pub mailbox: Mailbox,
    pub reductions: i64,
    pub context: ProcessContext,
    pub continuation: Option<Continuation>,
    pub initial_fn: Option<ErlFn>,
    pub initial_args: Vec<ErlTerm>,
    /// Resume continuation stack: set by compiled code (via FFI) before a
    /// receive-wait return. The stack is ordered innermost-first: index 0
    /// is the function that owns the actual `receive`, and subsequent
    /// entries are the callers that need to be resumed after the inner
    /// function completes. When the process is woken, the innermost
    /// continuation is popped and called first. When it completes, the
    /// next continuation is popped and called (getting the result via
    /// ctx.result), until the stack is empty.
    pub resume_continuations: Vec<Continuation>,
    /// Compact call stack used by compiled code (func_index + line).
    /// Pre-allocated to STACK_DEPTH_LIMIT for cursor-based addressing.
    pub compact_stack: Vec<CompactFrame>,
    /// Captured stack trace for the last raised exception in this process.
    pub exception_trace: Vec<StackFrame>,
    /// Long-lived OTP-oriented state (waiting reason, links, monitors,
    /// registration hooks, timer ownership).
    pub otp: OtpProcessState,
    /// Process dictionary: list of (key, value) pairs. Accessed via
    /// `put/2`, `get/0,1`, `erase/1`, `get_keys/0,1` BIFs.
    pub dictionary: Vec<(ErlTerm, ErlTerm)>,
    /// The reason this process exited. Set by the scheduler before
    /// calling propagate_exit_signals. `normal` for normal exit,
    /// the exception reason for crashes.
    pub exit_reason: Option<ErlTerm>,
    /// Optional Rust-level callback used for testing the scheduler
    /// without compiled code.
    pub rust_fn: Option<RustFn>,
    /// MFA to run when waking from hibernate. Set by `erlang:hibernate/3`.
    /// (module_atom_id, function_atom_id, args_list_terms)
    pub hibernate_mfa: Option<(u32, u32, Vec<crate::message::MessageFragment>)>,
}

/// A borrow-friendly subset of `Process` fields passed to `rust_fn`
/// callbacks. This avoids borrow conflicts with the closure itself.
pub struct ProcessInner {
    pub id: ProcessId,
    status: ProcessStatus,
    pub heap: ProcessHeap,
    pub mailbox: Mailbox,
    pub reductions: i64,
    pub context: ProcessContext,
}

impl ProcessInner {
    /// Request normal completion of this Rust callback's process. The worker
    /// publishes the exit after the callback returns, before queue accounting.
    #[inline]
    pub fn finish(&mut self) {
        self.status = ProcessStatus::Exited;
    }

    #[inline]
    pub fn status(&self) -> ProcessStatus {
        self.status
    }
}

impl Process {
    /// Read the locked lifecycle status. Published wake ownership is tracked
    /// separately by ProcessEntry until the worker reconciles a claimed wake.
    #[inline]
    pub fn status(&self) -> ProcessStatus {
        self.status
    }

    /// Read the locked wait cause without exposing independent state mutation.
    #[inline]
    pub fn wait_reason(&self) -> Option<WaitReason> {
        self.otp.wait_reason
    }

    /// Create a new process with the given PID.
    pub fn new(id: ProcessId) -> Self {
        Process {
            id,
            status: ProcessStatus::Ready,
            heap: ProcessHeap::new(),
            mailbox: Mailbox::new(),
            reductions: REDUCTION_LIMIT,
            context: ProcessContext::new(id),
            continuation: None,
            initial_fn: None,
            initial_args: Vec::new(),
            resume_continuations: Vec::new(),
            compact_stack: Vec::new(),
            exception_trace: Vec::new(),
            otp: OtpProcessState::default(),
            dictionary: Vec::new(),
            exit_reason: None,

            rust_fn: None,
            hibernate_mfa: None,
        }
    }

    /// Create a new process with a specific initial heap size (in words).
    /// Used by `spawn_opt` with `{min_heap_size, N}`.
    pub fn new_with_heap_size(id: ProcessId, heap_words: usize) -> Self {
        Process {
            id,
            status: ProcessStatus::Ready,
            heap: ProcessHeap::new_with_capacity(heap_words),
            mailbox: Mailbox::new(),
            reductions: REDUCTION_LIMIT,
            context: ProcessContext::new(id),
            continuation: None,
            initial_fn: None,
            initial_args: Vec::new(),
            resume_continuations: Vec::new(),
            compact_stack: Vec::new(),
            exception_trace: Vec::new(),
            otp: OtpProcessState::default(),
            dictionary: Vec::new(),
            exit_reason: None,

            rust_fn: None,
            hibernate_mfa: None,
        }
    }

    /// Create a new process with a compiled entry function.
    pub fn with_entry(id: ProcessId, func: ErlFn, args: Vec<ErlTerm>) -> Self {
        Process {
            id,
            status: ProcessStatus::Ready,
            heap: ProcessHeap::new(),
            mailbox: Mailbox::new(),
            reductions: REDUCTION_LIMIT,
            context: ProcessContext::new(id),
            continuation: None,
            initial_fn: Some(func),
            initial_args: args,
            resume_continuations: Vec::new(),
            compact_stack: Vec::new(),
            exception_trace: Vec::new(),
            otp: OtpProcessState::default(),
            dictionary: Vec::new(),
            exit_reason: None,

            rust_fn: None,
            hibernate_mfa: None,
        }
    }

    /// Read the locked state, retaining compatibility with legacy receive waits
    /// that have no explicit reason yet.
    #[inline]
    pub(crate) fn scheduling_state(&self) -> ProcessState {
        match self.status {
            ProcessStatus::Ready => ProcessState::Ready,
            ProcessStatus::Running => ProcessState::Running,
            ProcessStatus::Exited => ProcessState::Exited,
            ProcessStatus::Waiting => {
                ProcessState::Waiting(self.otp.wait_reason.unwrap_or(WaitReason::Receive {
                    timeout_deadline_ms: self.receive_timeout_deadline_ms(),
                }))
            }
        }
    }

    /// Update mutex-protected fields without publishing the atomic state.
    /// I/O completion uses this before its wake CAS; a Release store there
    /// would consume the wake transition and lose queue ownership.
    #[inline(always)]
    pub(crate) fn transition_to(&mut self, next: ProcessState) {
        let (status, reason) = match next {
            ProcessState::Ready => (ProcessStatus::Ready, None),
            ProcessState::Running => (ProcessStatus::Running, None),
            ProcessState::Waiting(reason) => (ProcessStatus::Waiting, Some(reason)),
            ProcessState::Exited => (ProcessStatus::Exited, None),
        };
        self.status = status;
        self.otp.wait_reason = reason;
    }

    /// Return the active wait timeout deadline (if any), with backward-compatible
    /// fallback to the receive timeout field when wait_reason is not set.
    pub fn receive_timeout_deadline_ms(&self) -> Option<u64> {
        match self.otp.wait_reason {
            Some(WaitReason::Receive {
                timeout_deadline_ms,
            }) => timeout_deadline_ms,
            Some(WaitReason::Timer { deadline_ms, .. }) => deadline_ms,
            _ => {
                if self.context.receive_timeout_deadline > 0 {
                    Some(self.context.receive_timeout_deadline)
                } else {
                    None
                }
            }
        }
    }

    /// Set the registered process name atom.
    pub fn set_registered_name(&mut self, atom_id: RegisteredName) {
        self.otp.registered_name = Some(atom_id);
    }

    /// Clear any registered process name.
    pub fn clear_registered_name(&mut self) {
        self.otp.registered_name = None;
    }

    /// Add a process link.
    pub fn add_link(&mut self, other: ProcessId) -> bool {
        self.otp.links.insert(other)
    }

    /// Remove a process link.
    pub fn remove_link(&mut self, other: ProcessId) -> bool {
        self.otp.links.remove(&other)
    }

    /// Add an outgoing monitor (this process monitors `target` via `reference`).
    pub fn add_outgoing_monitor(
        &mut self,
        reference: MonitorRef,
        target: ProcessId,
    ) -> Option<ProcessId> {
        self.otp.monitors_outgoing.insert(reference, target)
    }

    /// Remove an outgoing monitor by reference.
    pub fn remove_outgoing_monitor(&mut self, reference: MonitorRef) -> Option<ProcessId> {
        self.otp.monitors_outgoing.remove(&reference)
    }

    /// Add an incoming monitor (another process monitors this process).
    pub fn add_incoming_monitor(
        &mut self,
        reference: MonitorRef,
        owner: ProcessId,
    ) -> Option<ProcessId> {
        self.otp.monitors_incoming.insert(reference, owner)
    }

    /// Remove an incoming monitor by reference.
    pub fn remove_incoming_monitor(&mut self, reference: MonitorRef) -> Option<ProcessId> {
        self.otp.monitors_incoming.remove(&reference)
    }

    /// Add a timer to this process's ownership set.
    pub fn add_owned_timer(&mut self, timer_id: TimerId) -> bool {
        self.otp.owned_timers.insert(timer_id)
    }

    /// Remove a timer from this process's ownership set.
    pub fn remove_owned_timer(&mut self, timer_id: TimerId) -> bool {
        self.otp.owned_timers.remove(&timer_id)
    }

    /// Perform garbage collection on this process's heap.
    ///
    /// Gathers all GC roots from:
    ///   - `context.result` (the current return value)
    ///   - `initial_args` (entry-point arguments)
    ///   - `resume_continuations[].args` and `.saved_vars`
    ///   - `continuation.args` and `.saved_vars`
    ///   - `mailbox.messages` (all queued messages)
    ///
    /// After the copying collection, all root locations are updated to
    /// point into the new heap arena.
    /// # Safety
    /// All stored roots and their reachable graphs must be current, initialized
    /// terms owned by this process heap or its retained external owners. No
    /// compiled execution, object borrow or concurrent heap access may be active.
    /// Do not reuse saved terms or pointers after this moving collection; use the
    /// updated process fields. Public raw-term fields do not enforce validity.
    /// ```compile_fail,E0133
    /// use nera_runtime::process::Process;
    /// Process::new(nera_runtime::process::ProcessId::new(1)).collect_garbage();
    /// ```
    pub unsafe fn collect_garbage(&mut self) {
        let mut roots = Vec::new();

        // 1. context.result
        roots.push(ErlTerm(self.context.result));

        // 2. initial_args
        let initial_args_start = roots.len();
        roots.extend_from_slice(&self.initial_args);
        let initial_args_end = roots.len();

        // 3. resume_continuations — track layout for scatter-back
        let mut cont_layout: Vec<(usize, usize, usize)> = Vec::new(); // (start, args_len, saved_len)
        for cont in &self.resume_continuations {
            let start = roots.len();
            roots.extend_from_slice(&cont.args);
            let args_len = cont.args.len();
            roots.extend_from_slice(&cont.saved_vars);
            let saved_len = cont.saved_vars.len();
            cont_layout.push((start, args_len, saved_len));
        }

        // 4. continuation (if any)
        let cont_start = roots.len();
        let mut cont_args_len = 0;
        let mut cont_saved_len = 0;
        if let Some(ref cont) = self.continuation {
            roots.extend_from_slice(&cont.args);
            cont_args_len = cont.args.len();
            roots.extend_from_slice(&cont.saved_vars);
            cont_saved_len = cont.saved_vars.len();
        }

        // 5. mailbox messages
        let mailbox_start = roots.len();
        let mailbox_roots = self.mailbox.gather_roots();
        roots.extend_from_slice(&mailbox_roots);
        let mailbox_len = mailbox_roots.len();

        // 6. process dictionary keys and values
        let dict_start = roots.len();
        for (key, val) in &self.dictionary {
            roots.push(*key);
            roots.push(*val);
        }
        let dict_pairs = self.dictionary.len();

        if roots.is_empty() {
            return;
        }

        // SAFETY: roots were gathered from the exclusively accessed process;
        // validity and absence of object borrows are guaranteed by the caller.
        unsafe {
            self.collect_garbage_with_roots(
                &mut roots,
                initial_args_start,
                initial_args_end,
                &cont_layout,
                cont_start,
                cont_args_len,
                cont_saved_len,
                mailbox_start,
                mailbox_len,
                dict_start,
                dict_pairs,
                false,
            );
        }
    }

    /// Force a full (major) garbage collection.  Used for idle heap shrinking
    /// when the old generation has accumulated dead data after a burst.
    /// # Safety
    /// All stored roots and their reachable graphs must be current, initialized
    /// terms owned by this process heap or its retained external owners. No
    /// compiled execution, object borrow or concurrent heap access may be active.
    /// Do not reuse saved terms or pointers after this moving collection; use the
    /// updated process fields. Public raw-term fields do not enforce validity.
    /// ```compile_fail,E0133
    /// use nera_runtime::process::Process;
    /// Process::new(nera_runtime::process::ProcessId::new(1)).collect_garbage_full();
    /// ```
    pub unsafe fn collect_garbage_full(&mut self) {
        unsafe { self.collect_garbage_full_or_idle(false); }
    }

    /// Fulfil garbage_collect/0 only after compiled frames have saved their roots.
    /// # Safety
    /// Same complete-root and exclusive-access requirements as collect_garbage_full.
    pub(crate) unsafe fn collect_requested_garbage(&mut self) -> bool {
        if !std::mem::take(&mut self.context.full_gc_requested) {
            return false;
        }
        unsafe { self.collect_garbage_full(); }
        true
    }

    /// Collect at the receive boundary, choosing at most one collection.
    /// # Safety
    /// Same exclusive-access and complete-root requirements as collect_garbage.
    pub(crate) unsafe fn collect_garbage_idle(&mut self) {
        if self.heap.should_shrink() {
            unsafe { self.collect_garbage_full_or_idle(true); }
        } else if self.heap.needs_gc() {
            unsafe { self.collect_garbage(); }
        }
    }

    /// # Safety
    /// Same requirements as collect_garbage_full; idle only permits skipping
    /// an unchanged large live graph when no allocation/limit pressure exists.
    unsafe fn collect_garbage_full_or_idle(&mut self, idle: bool) {
        let mut roots = Vec::new();

        // 1. context.result
        roots.push(ErlTerm(self.context.result));

        // 2. initial_args
        let initial_args_start = roots.len();
        roots.extend_from_slice(&self.initial_args);
        let initial_args_end = roots.len();

        // 3. resume_continuations — track layout for scatter-back
        let mut cont_layout: Vec<(usize, usize, usize)> = Vec::new();
        for cont in &self.resume_continuations {
            let start = roots.len();
            roots.extend_from_slice(&cont.args);
            let args_len = cont.args.len();
            roots.extend_from_slice(&cont.saved_vars);
            let saved_len = cont.saved_vars.len();
            cont_layout.push((start, args_len, saved_len));
        }

        // 4. continuation (if any)
        let cont_start = roots.len();
        let mut cont_args_len = 0;
        let mut cont_saved_len = 0;
        if let Some(ref cont) = self.continuation {
            roots.extend_from_slice(&cont.args);
            cont_args_len = cont.args.len();
            roots.extend_from_slice(&cont.saved_vars);
            cont_saved_len = cont.saved_vars.len();
        }

        // 5. mailbox messages
        let mailbox_start = roots.len();
        let mailbox_roots = self.mailbox.gather_roots();
        roots.extend_from_slice(&mailbox_roots);
        let mailbox_len = mailbox_roots.len();

        // 6. process dictionary keys and values
        let dict_start = roots.len();
        for (key, val) in &self.dictionary {
            roots.push(*key);
            roots.push(*val);
        }
        let dict_pairs = self.dictionary.len();

        if roots.is_empty() || (idle && !self.heap.should_collect_idle(&roots)) {
            return;
        }

        // SAFETY: roots were gathered from the exclusively accessed process;
        // validity and absence of object borrows are guaranteed by the caller.
        unsafe {
            self.collect_garbage_with_roots(
                &mut roots,
                initial_args_start,
                initial_args_end,
                &cont_layout,
                cont_start,
                cont_args_len,
                cont_saved_len,
                mailbox_start,
                mailbox_len,
                dict_start,
                dict_pairs,
                true,
            );
        }
    }

    /// Preserve hibernate arguments independently of the heap being discarded.
    /// # Safety
    /// Every argument must reference a live, initialized graph owned by this process.
    pub(crate) unsafe fn retain_hibernate_args(&mut self, module: u32, function: u32, args: Vec<ErlTerm>) {
        let args = args.into_iter().map(|arg| {
            // SAFETY: the caller holds exclusive process access; all argument
            // graphs remain live and unmoving until their fragments own them.
            unsafe { crate::message::MessageFragment::from_term(arg) }
        }).collect();
        self.hibernate_mfa = Some((module, function, args));
    }

    /// Hibernate, discarding execution state and reclaiming unreachable storage.
    /// Queued messages and process dictionary keys/values retain their complete
    /// graphs. With empty or immediate-only retained state, reset the heap; otherwise
    /// compact only the retained process state with a full collection.
    ///
    /// # Safety
    /// All mailbox and dictionary graphs must be live, initialized and owned by
    /// the process heap or its retained external owners. No compiled execution,
    /// object borrow or concurrent heap access may be active. Saved terms and
    /// pointers must not be reused afterward: collection may move their storage.
    /// ```compile_fail,E0133
    /// use nera_runtime::process::Process;
    /// Process::new(nera_runtime::process::ProcessId::new(1)).hibernate();
    /// ```
    pub unsafe fn hibernate(&mut self) {
        // Discard execution roots before gathering the state that must survive.
        self.continuation = None;
        self.initial_fn = None;
        self.initial_args.clear();
        self.resume_continuations.clear();
        self.compact_stack.clear();
        self.exception_trace.clear();
        self.context = ProcessContext::new(self.id);

        let has_heap_roots = self.mailbox.messages().any(|term| !term.is_immediate())
            || self.dictionary.iter().any(|(key, value)| !key.is_immediate() || !value.is_immediate());
        if !has_heap_roots {
            // Empty or immediate-only state needs no copying collection.
            let max_heap_size = self.heap.max_heap_size_words();
            let fullsweep_after = self.heap.get_fullsweep_after();
            self.heap = ProcessHeap::new();
            self.heap.set_max_heap_size(max_heap_size);
            self.heap.set_fullsweep_after(fullsweep_after);
        } else {
            // SAFETY: only persistent roots remain, and the caller guarantees
            // their validity and exclusive access through this collection.
            unsafe { self.collect_garbage_full(); }
        }
    }

    /// Shared implementation: run GC on gathered roots, then scatter back.
    #[expect(clippy::too_many_arguments, reason = "Explicit scheduler and GC state avoids changing runtime ownership and locking boundaries")]
    /// # Safety
    /// `roots` contains every retained valid graph; layout indexes identify its
    /// owning process fields. No competing heap access or object borrow is live.
    unsafe fn collect_garbage_with_roots(
        &mut self,
        roots: &mut [ErlTerm],
        initial_args_start: usize,
        initial_args_end: usize,
        cont_layout: &[(usize, usize, usize)],
        cont_start: usize,
        cont_args_len: usize,
        cont_saved_len: usize,
        mailbox_start: usize,
        mailbox_len: usize,
        dict_start: usize,
        dict_pairs: usize,
        force_major: bool,
    ) {
        // --- Copying collection (updates roots in-place) ---
        if force_major {
            unsafe { self.heap.collect_full(roots); }
        } else {
            unsafe { self.heap.collect(roots); }
        }

        // --- Scatter updated roots back to their sources ---

        // context.result
        self.context.result = roots[0].0;

        // initial_args
        if initial_args_end > initial_args_start {
            self.initial_args[..].copy_from_slice(&roots[initial_args_start..initial_args_end]);
        }

        // resume_continuations
        for (i, &(start, args_len, saved_len)) in cont_layout.iter().enumerate() {
            if args_len > 0 {
                self.resume_continuations[i].args[..]
                    .copy_from_slice(&roots[start..start + args_len]);
            }
            if saved_len > 0 {
                self.resume_continuations[i].saved_vars[..]
                    .copy_from_slice(&roots[start + args_len..start + args_len + saved_len]);
            }
        }

        // continuation
        if let Some(cont) = &mut self.continuation {
            if cont_args_len > 0 {
                cont.args[..].copy_from_slice(&roots[cont_start..cont_start + cont_args_len]);
            }
            if cont_saved_len > 0 {
                cont.saved_vars[..].copy_from_slice(
                    &roots[cont_start + cont_args_len..cont_start + cont_args_len + cont_saved_len],
                );
            }
        }

        // mailbox messages
        if mailbox_len > 0 {
            // SAFETY: roots were gathered from the unchanged live mailbox slots;
            // the owning heap's collector updated them without creating sentinels.
            unsafe {
                self.mailbox.scatter_roots(&roots[mailbox_start..mailbox_start + mailbox_len]);
            }
        }

        // process dictionary
        if dict_pairs > 0 {
            for i in 0..dict_pairs {
                self.dictionary[i].0 = roots[dict_start + i * 2]; // key
                self.dictionary[i].1 = roots[dict_start + i * 2 + 1]; // value
            }
        }
    }

    /// Execute pending tail calls in a trampoline loop.
    ///
    /// When compiled code returns with `STATUS_TAIL_CALL`, the target info
    /// is stored in the `ProcessContext.tail_call_*` fields. This method
    /// dispatches the requested call from the `run_slice` stack frame
    /// (preventing native stack growth) and repeats until the callee
    /// returns a non-tail-call status.
    fn trampoline_dispatch(&mut self) {
        while self.context.status == STATUS_TAIL_CALL {
            self.context.status = STATUS_OK;
            let kind = self.context.tail_call_kind;
            self.context.tail_call_kind = 0;
            match kind {
                1 => {
                    // Apply: nera_rt_apply(ctx, mod_atom, fun_atom, args_list)
                    let mod_atom = self.context.tail_call_mod;
                    let fun_atom = self.context.tail_call_fun;
                    let args_list = self.context.tail_call_arg1;
                    unsafe {
                        let result = crate::ffi::apply::nera_rt_apply(
                            &mut self.context as *mut ProcessContext,
                            mod_atom,
                            fun_atom,
                            args_list,
                        );
                        // Only store result if the call completed normally.
                        // If it returned STATUS_TAIL_CALL, the loop continues.
                        if self.context.status == STATUS_OK {
                            self.context.result = result;
                        }
                    }
                }
                2 => {
                    // Closure: tail_call_closure(ctx, closure, args_list)
                    let closure = self.context.tail_call_arg1;
                    let args_list = self.context.tail_call_arg2;
                    unsafe {
                        let result = crate::ffi::closure::tail_call_closure(
                            &mut self.context as *mut ProcessContext,
                            closure,
                            args_list,
                        );
                        if self.context.status == STATUS_OK {
                            self.context.result = result;
                        }
                    }
                }
                _ => break,
            }
            self.reductions = self.context.reductions;
        }
    }

    /// Execute one scheduling time-slice of this process.
    ///
    /// Priority: `rust_fn` > `resume_continuations` > `continuation` > `initial_fn`.
    /// If none is set the process is marked exited.
    /// # Safety
    /// Process roots must reference current initialized graphs owned by its heap
    /// or retained owners. Entry/resume functions and Rust callbacks must obey
    /// the runtime context/heap contracts and leave valid roots on return. No
    /// competing execution or heap object borrow may be active.
    /// ```compile_fail,E0133
    /// use nera_runtime::process::Process;
    /// Process::new(nera_runtime::process::ProcessId::new(1)).run_slice();
    /// ```
    pub unsafe fn run_slice(&mut self) {
        if let Some(mut rust_fn) = self.rust_fn.take() {
            // Split out the fields the callback can access.
            let mut inner = ProcessInner {
                id: self.id,
                status: self.status,
                heap: std::mem::take(&mut self.heap),
                mailbox: std::mem::take(&mut self.mailbox),
                reductions: self.reductions,
                context: ProcessContext::new(self.id),
            };
            inner.context.reductions = self.reductions;
            rust_fn(&mut inner);
            // Write back.
            self.status = inner.status;
            self.heap = inner.heap;
            self.mailbox = inner.mailbox;
            self.reductions = inner.context.reductions;
            self.context.status = inner.context.status;
            self.context.result = inner.context.result;
            // Put the closure back so the process can be re-scheduled.
            self.rust_fn = Some(rust_fn);
        } else if !self.resume_continuations.is_empty() {
            // Resume from receive_wait: the stack is ordered innermost-first.
            // Pop the first continuation (the function owning the receive),
            // call it, then continue returning or unwinding through outer
            // continuations until the stack is empty or execution suspends.
            let cont = self.resume_continuations.remove(0);
            self.context.reductions = self.reductions;
            self.context.status = STATUS_OK;
            self.context.heap_ptr = &mut self.heap as *mut ProcessHeap;
            self.context.heap_cursor = std::ptr::null_mut();
            self.context.heap_limit = std::ptr::null_mut();
            self.context.mailbox_ptr = &mut self.mailbox as *mut Mailbox;
            self.context.stack_ptr = std::ptr::null_mut();
            self.context.exception_trace_ptr = &mut self.exception_trace as *mut Vec<StackFrame>;
            self.context.dict_ptr = &mut self.dictionary as *mut Vec<(ErlTerm, ErlTerm)>;
            self.context.otp_ptr = &mut self.otp as *mut OtpProcessState;
            self.context.runtime = crate::scheduler::global_scheduler()
                .map(|s| std::sync::Arc::as_ptr(s) as *const _)
                .unwrap_or(std::ptr::null());
            self.compact_stack.resize(
                STACK_DEPTH_LIMIT,
                CompactFrame {
                    func_index: 0,
                    line: 0,
                },
            );
            self.context.stack_base = self.compact_stack.as_mut_ptr();
            self.context.stack_cursor = self.compact_stack.as_mut_ptr();
            self.context.stack_limit =
                unsafe { self.compact_stack.as_mut_ptr().add(STACK_DEPTH_LIMIT) };

            self.exception_trace.clear();
            // Tell compiled code which receive to resume at
            self.context.resume_receive = cont.resume_receive_index;
            // Make saved variables available to compiled code via thread-local
            let saved: Vec<u64> = cont.saved_vars.iter().map(|t| t.0).collect();
            RESUME_SAVED_VARS.with(|cell| {
                *cell.borrow_mut() = saved;
            });
            let args: Vec<u64> = cont.args.iter().map(|t| t.0).collect();
            // SAFETY: the context and argument storage are live and exclusively
            // accessible here; no heap borrow crosses this reentry boundary.
            unsafe {
                (cont.func)(
                    &mut self.context as *mut ProcessContext,
                    args.as_ptr(),
                    args.len() as u32,
                );
            }
            self.trampoline_dispatch();
            self.reductions = self.context.reductions;
            // Collect any new continuations pushed during this execution
            let mut new_conts = crate::ffi::take_pending_resumes();

            // Resume outer callers on success or unwind through them on exception.
            // Preserve exception state so the nearest protected call can catch it.
            while matches!(self.context.status, STATUS_OK | STATUS_EXCEPTION)
                && !self.resume_continuations.is_empty()
            {
                let outer = self.resume_continuations.remove(0);
                self.context.resume_receive = outer.resume_receive_index;
                // The result from the inner call is already in ctx.result.
                let saved: Vec<u64> = outer.saved_vars.iter().map(|t| t.0).collect();
                RESUME_SAVED_VARS.with(|cell| {
                    *cell.borrow_mut() = saved;
                });
                let args: Vec<u64> = outer.args.iter().map(|t| t.0).collect();
                // SAFETY: the context and argument storage are live and exclusively
                // accessible here; no heap borrow crosses this reentry boundary.
                unsafe {
                    (outer.func)(
                        &mut self.context as *mut ProcessContext,
                        args.as_ptr(),
                        args.len() as u32,
                    );
                }
                self.trampoline_dispatch();
                self.reductions = self.context.reductions;
                // Collect any new continuations from this call
                new_conts.extend(crate::ffi::take_pending_resumes());
            }

            // No sync needed — stack_cursor is authoritative

            // If any new continuations were set (e.g., the function
            // re-entered receive_wait), prepend them to the stack.
            if !new_conts.is_empty() {
                new_conts.append(&mut self.resume_continuations);
                self.resume_continuations = new_conts;
            }
            // If the function completed normally, clear initial_fn
            if self.context.status == STATUS_OK {
                self.initial_fn = None;
            }
        } else if let Some(cont) = self.continuation.take() {
            self.context.reductions = self.reductions;
            self.context.status = STATUS_OK;
            self.context.heap_ptr = &mut self.heap as *mut ProcessHeap;
            self.context.heap_cursor = std::ptr::null_mut();
            self.context.heap_limit = std::ptr::null_mut();
            self.context.mailbox_ptr = &mut self.mailbox as *mut Mailbox;
            self.context.stack_ptr = std::ptr::null_mut();
            self.context.exception_trace_ptr = &mut self.exception_trace as *mut Vec<StackFrame>;
            self.context.dict_ptr = &mut self.dictionary as *mut Vec<(ErlTerm, ErlTerm)>;
            self.context.otp_ptr = &mut self.otp as *mut OtpProcessState;
            self.context.runtime = crate::scheduler::global_scheduler()
                .map(|s| std::sync::Arc::as_ptr(s) as *const _)
                .unwrap_or(std::ptr::null());
            self.compact_stack.resize(
                STACK_DEPTH_LIMIT,
                CompactFrame {
                    func_index: 0,
                    line: 0,
                },
            );
            self.context.stack_base = self.compact_stack.as_mut_ptr();
            self.context.stack_cursor = self.compact_stack.as_mut_ptr();
            self.context.stack_limit =
                unsafe { self.compact_stack.as_mut_ptr().add(STACK_DEPTH_LIMIT) };

            self.exception_trace.clear();
            let args: Vec<u64> = cont.args.iter().map(|t| t.0).collect();
            // SAFETY: the context and argument storage are live and exclusively
            // accessible here; no heap borrow crosses this reentry boundary.
            unsafe {
                (cont.func)(
                    &mut self.context as *mut ProcessContext,
                    args.as_ptr(),
                    args.len() as u32,
                );
            }
            self.trampoline_dispatch();
            self.reductions = self.context.reductions;
            // Check if compiled code set new resume continuations
            let new_conts = crate::ffi::take_pending_resumes();
            if !new_conts.is_empty() {
                self.resume_continuations = new_conts;
            }
        } else if let Some(func) = self.initial_fn {
            self.context.reductions = self.reductions;
            self.context.status = STATUS_OK;
            self.context.heap_ptr = &mut self.heap as *mut ProcessHeap;
            self.context.heap_cursor = std::ptr::null_mut();
            self.context.heap_limit = std::ptr::null_mut();
            self.context.mailbox_ptr = &mut self.mailbox as *mut Mailbox;
            self.context.stack_ptr = std::ptr::null_mut();
            self.context.exception_trace_ptr = &mut self.exception_trace as *mut Vec<StackFrame>;
            self.context.dict_ptr = &mut self.dictionary as *mut Vec<(ErlTerm, ErlTerm)>;
            self.context.otp_ptr = &mut self.otp as *mut OtpProcessState;
            self.context.runtime = crate::scheduler::global_scheduler()
                .map(|s| std::sync::Arc::as_ptr(s) as *const _)
                .unwrap_or(std::ptr::null());
            self.compact_stack.resize(
                STACK_DEPTH_LIMIT,
                CompactFrame {
                    func_index: 0,
                    line: 0,
                },
            );
            self.context.stack_base = self.compact_stack.as_mut_ptr();
            self.context.stack_cursor = self.compact_stack.as_mut_ptr();
            self.context.stack_limit =
                unsafe { self.compact_stack.as_mut_ptr().add(STACK_DEPTH_LIMIT) };

            self.exception_trace.clear();
            let args: Vec<u64> = self.initial_args.iter().map(|t| t.0).collect();
            // SAFETY: run_slice's contract supplies valid entry code and roots;
            // the refreshed context and copied argument buffer remain live.
            unsafe {
                (func)(
                    &mut self.context as *mut ProcessContext,
                    args.as_ptr(),
                    args.len() as u32,
                );
            }
            self.trampoline_dispatch();
            self.reductions = self.context.reductions;
            // Check if compiled code set new resume continuations
            let new_conts = crate::ffi::take_pending_resumes();
            if !new_conts.is_empty() {
                self.resume_continuations = new_conts;
            }
            // Only clear initial_fn when the function completed normally.
            if self.context.status != STATUS_RECEIVE_WAIT
                && self.context.status != STATUS_YIELD
                && self.context.status != STATUS_IO_WAIT
            {
                self.initial_fn = None;
            }
        } else {
            // Nothing to run -- mark exited.
            self.status = ProcessStatus::Exited;
        }
    }
}

// ---------------------------------------------------------------------------
// Exit signal processing
// ---------------------------------------------------------------------------

/// Scan the process mailbox for `{'EXIT', FromPid, Reason}` tuples and
/// process them according to BEAM semantics:
///
/// - `kill` reason: always kill (untrappable) — returns `Some(ATOM_KILLED)`
/// - `trap_exit=true` + non-kill: leave in mailbox as regular message
/// - `trap_exit=false` + `normal`: silently remove (BEAM ignores normal exits)
/// - `trap_exit=false` + non-normal, non-kill: kill — returns `Some(reason)`
///
/// Returns `Some(exit_reason)` if the process should die.
///
/// Raw mailbox traversal is not a safe public operation:
///
/// ```compile_fail
/// use nera_runtime::process::{Process, process_exit_signals};
/// let mut process = Process::new(nera_runtime::process::ProcessId::new(1));
/// process_exit_signals(&mut process);
/// ```
///
/// # Safety
///
/// Every non-tombstone mailbox term must be a current, initialized runtime term.
/// Boxed storage must have the complete layout described by its header and
/// remain live in the process heap or its retained owners throughout the scan.
/// The caller must exclude concurrent/reentrant mutation, collection or heap
/// replacement. A returned heap-backed reason must be rooted before any operation
/// that could move or release its storage.
pub(crate) unsafe fn process_exit_signals(proc: &mut Process) -> Option<ErlTerm> {
    use crate::atom_table::{ATOM_EXIT_SIGNAL, ATOM_KILL, ATOM_KILLED, ATOM_NORMAL};
    use crate::term::{BoxedHeader, BOXED_TUPLE, TOMBSTONE_VALUE};

    // Removal changes logical indices, but does not move/reallocate ring storage.
    // Scan the original initialized physical slots in FIFO order instead.
    let slots = proc.mailbox.slot_count();
    if slots == 0 {
        return None;
    }
    let head = proc.mailbox.head;
    let mask = proc.mailbox.capacity - 1;
    let buffer = proc.mailbox.buf;
    for offset in 0..slots {
        let physical = (head.wrapping_add(offset) & mask) as usize;
        // SAFETY: each original occupied slot was initialized. This loop only
        // tombstones/removes slots: no resize or append invalidates the buffer.
        let msg = ErlTerm(unsafe { *buffer.add(physical) });

        // Skip tombstones.
        if msg.0 == TOMBSTONE_VALUE {
            continue;
        }

        // Check if this is a {'EXIT', FromPid, Reason} tuple
        if !msg.is_boxed() {
            continue;
        }
        let Some(ptr) = msg.as_boxed_ptr() else {
            continue;
        };
        // SAFETY: the caller guarantees current, complete boxed storage.
        let header = unsafe { BoxedHeader(*ptr) };
        if header.subtype() != BOXED_TUPLE || header.arity() != 3 {
            continue;
        }
        // SAFETY: the valid header identifies a complete three-element tuple.
        let tag = unsafe { ErlTerm(*ptr.add(1)) };
        if tag.as_atom_id() != Some(ATOM_EXIT_SIGNAL) {
            continue;
        }
        // This is an EXIT signal tuple: {'EXIT', FromPid, Reason}
        // SAFETY: the same live tuple contains its third element.
        let reason = unsafe { ErlTerm(*ptr.add(3)) };
        let reason_atom = reason.as_atom_id();

        if reason_atom == Some(ATOM_KILL) {
            // Untrappable kill — always die
            // SAFETY: this live slot belongs to the unchanged scan buffer.
            unsafe {
                proc.mailbox.remove_scanned_slot(physical);
            }
            return Some(ErlTerm::atom(ATOM_KILLED));
        }

        if proc.otp.trap_exit {
            // trap_exit=true: keep in mailbox as a regular message
            continue;
        }

        // trap_exit=false
        if reason_atom == Some(ATOM_NORMAL) {
            // SAFETY: this live slot was just read from the stable scan buffer;
            // no intervening operation changes storage or removes this slot.
            unsafe {
                proc.mailbox.remove_scanned_slot(physical);
            }
            continue;
        }

        // Non-normal, non-kill: kill the process with this reason
        // SAFETY: this live slot belongs to the unchanged scan buffer.
        unsafe {
            proc.mailbox.remove_scanned_slot(physical);
        }
        return Some(reason);
    }

    None
}

// ---------------------------------------------------------------------------
// Mailbox (FastMailbox — #[repr(C)] ring buffer)
// ---------------------------------------------------------------------------

/// Initial ring buffer capacity (must be power of 2).
const FAST_MAILBOX_INITIAL_CAP: u32 = 16;

/// A per-process message queue backed by a power-of-2 ring buffer.
///
/// `#[repr(C)]` so Cranelift-generated code can inline field accesses at
/// known offsets for `receive_peek` and `receive_accept`.
#[repr(C)]
pub struct Mailbox {
    /// Pointer to heap-allocated ring buffer of `u64` (ErlTerm values).
    buf: *mut u64,
    /// Always a power of 2 (enables `& (cap - 1)` instead of `%`).
    capacity: u32,
    /// Index of the first element in the ring buffer.
    head: u32,
    /// Total occupied slots (live messages + tombstones).
    len: u32,
    /// Selective receive scan position (offset from head).
    scan_pos: u32,
    /// Number of tombstoned (removed) slots within `[head, head+len)`.
    tombstone_count: u32,
}

// Field offsets for codegen inlining.
pub const FAST_MAILBOX_OFFSET_BUF: i32 = std::mem::offset_of!(Mailbox, buf) as i32;
pub const FAST_MAILBOX_OFFSET_CAPACITY: i32 = std::mem::offset_of!(Mailbox, capacity) as i32;
pub const FAST_MAILBOX_OFFSET_HEAD: i32 = std::mem::offset_of!(Mailbox, head) as i32;
pub const FAST_MAILBOX_OFFSET_LEN: i32 = std::mem::offset_of!(Mailbox, len) as i32;
pub const FAST_MAILBOX_OFFSET_SCAN_POS: i32 = std::mem::offset_of!(Mailbox, scan_pos) as i32;
pub const FAST_MAILBOX_OFFSET_TOMBSTONE_COUNT: i32 =
    std::mem::offset_of!(Mailbox, tombstone_count) as i32;

// SAFETY: buf is an owned allocation with no thread affinity. Moving a Mailbox
// transfers the allocation, and mutation requires &mut self (the scheduler uses
// the process mutex). Its Drop frees the allocation exactly once. Stored term
// pointers are used only under the owning process/heap validity contract.
unsafe impl Send for Mailbox {}

impl Default for Mailbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Mailbox {
    fn drop(&mut self) {
        if !self.buf.is_null() && self.capacity > 0 {
            let layout =
                std::alloc::Layout::array::<u64>(self.capacity as usize).expect("valid layout");
            unsafe {
                std::alloc::dealloc(self.buf as *mut u8, layout);
            }
        }
    }
}

impl Mailbox {
    pub fn new() -> Self {
        Mailbox {
            buf: std::ptr::null_mut(),
            capacity: 0,
            head: 0,
            len: 0,
            scan_pos: 0,
            tombstone_count: 0,
        }
    }

    /// Compute the physical index for a logical offset from head.
    #[inline(always)]
    fn physical_index(&self, logical: u32) -> usize {
        ((self.head.wrapping_add(logical)) & (self.capacity - 1)) as usize
    }

    /// Total occupied slots (live + tombstones).
    #[inline]
    fn slot_count(&self) -> u32 {
        self.len
    }

    /// Number of live (non-tombstoned) messages.
    #[inline]
    pub fn alive_count(&self) -> usize {
        (self.len - self.tombstone_count) as usize
    }

    /// Grow the ring buffer to at least `min_cap` (rounded up to power of 2).
    /// When no scan is active, tombstones are stripped during copy (free
    /// compaction on resize).  When a selective receive scan **is** active
    /// (`scan_pos > 0`), all slots (including tombstones) are preserved so
    /// that `scan_pos` remains valid.
    fn grow(&mut self, min_cap: u32) {
        // Validate capacity and byte layout before allocation or mailbox mutation.
        let new_cap = min_cap.checked_next_power_of_two()
            .expect("mailbox capacity exceeds the u32 power-of-two domain")
            .max(FAST_MAILBOX_INITIAL_CAP);
        let new_layout = std::alloc::Layout::array::<u64>(new_cap as usize)
            .expect("mailbox allocation layout exceeds the addressable size");
        let strip_tombstones = self.scan_pos == 0;
        let kept = if strip_tombstones {
            self.len - self.tombstone_count
        } else {
            self.len
        };
        let new_buf = unsafe { std::alloc::alloc(new_layout) as *mut u64 };
        assert!(!new_buf.is_null(), "allocation failed");

        let mut write = 0u32;
        if self.len > 0 && !self.buf.is_null() {
            for i in 0..self.len {
                let src_idx = self.physical_index(i);
                let val = unsafe { *self.buf.add(src_idx) };
                if strip_tombstones && val == crate::term::TOMBSTONE_VALUE {
                    continue;
                }
                unsafe {
                    *new_buf.add(write as usize) = val;
                }
                write += 1;
            }
        }
        debug_assert_eq!(write, kept);

        // Free old buffer.
        if !self.buf.is_null() && self.capacity > 0 {
            let old_layout =
                std::alloc::Layout::array::<u64>(self.capacity as usize).expect("valid layout");
            unsafe {
                std::alloc::dealloc(self.buf as *mut u8, old_layout);
            }
        }

        self.buf = new_buf;
        self.capacity = new_cap;
        self.head = 0;
        self.len = kept;
        if strip_tombstones {
            self.tombstone_count = 0;
        }
    }

    /// Append a message to the end of the mailbox.
    /// # Panics
    /// Rejects the internal tombstone sentinel before mutation in all profiles.
    pub fn send(&mut self, msg: ErlTerm) {
        assert!(
            !msg.is_tombstone(),
            "send: cannot send a tombstone sentinel"
        );
        if self.buf.is_null() || self.len >= self.capacity {
            // Try to reclaim space from tombstones before growing — but only
            // when no selective receive scan is active, since compaction
            // invalidates scan_pos.
            if self.tombstone_count > 0 && self.scan_pos == 0 {
                self.compact();
                if self.len < self.capacity {
                    // Room after compaction — skip grow.
                    let idx = self.physical_index(self.len);
                    unsafe {
                        *self.buf.add(idx) = msg.0;
                    }
                    self.len += 1;
                    return;
                }
            }
            self.grow(self.len.checked_add(1).expect("mailbox message count overflow"));
        }
        let idx = self.physical_index(self.len);
        unsafe {
            *self.buf.add(idx) = msg.0;
        }
        self.len += 1;
    }

    /// Number of live messages currently in the mailbox.
    #[inline]
    pub fn len(&self) -> usize {
        (self.len - self.tombstone_count) as usize
    }

    /// Whether the mailbox has no live messages.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == self.tombstone_count
    }

    /// Reset the scan position to the beginning for a new `receive`.
    /// May trigger compaction if tombstone ratio is high.
    #[inline]
    pub fn reset_scan(&mut self) {
        self.scan_pos = 0;
        self.maybe_compact();
    }

    /// Advance the scan position and return the next live candidate message
    /// along with its logical index. Returns `None` when all slots have been
    /// scanned. Tombstoned slots are skipped transparently.
    pub fn try_match_next(&mut self) -> Option<(usize, ErlTerm)> {
        while self.scan_pos < self.len {
            let logical = self.scan_pos;
            let phys = self.physical_index(logical);
            let val = unsafe { *self.buf.add(phys) };
            self.scan_pos += 1;
            if val != crate::term::TOMBSTONE_VALUE {
                return Some((logical as usize, ErlTerm(val)));
            }
        }
        None
    }

    /// Remove and return the message at logical `index` by writing a
    /// tombstone sentinel. O(1) instead of the old O(n) shift.
    ///
    /// Head-tombstones are trimmed immediately to maintain the invariant
    /// that `head` never points to a tombstone.
    /// # Panics
    /// Rejects out-of-range indices and already removed slots before mutation.
    pub fn remove_at(&mut self, index: usize) -> ErlTerm {
        assert!(index < self.len as usize, "remove_at: index out of bounds");
        let idx32 = index as u32;
        let phys = self.physical_index(idx32);
        let msg = ErlTerm(unsafe { *self.buf.add(phys) });
        assert!(!msg.is_tombstone(), "remove_at: double-remove of tombstone");

        // Write tombstone sentinel.
        unsafe {
            *self.buf.add(phys) = crate::term::TOMBSTONE_VALUE;
        }
        self.tombstone_count += 1;

        // Maintain invariant: head is never a tombstone.
        self.trim_head_tombstones();

        msg
    }

    /// Tombstone a live physical slot already established by a bounded scan.
    /// # Safety
    /// `physical` must identify an occupied, non-tombstone slot in the current
    /// buffer. No resize, append or removal may invalidate that proof.
    #[inline]
    unsafe fn remove_scanned_slot(&mut self, physical: usize) {
        debug_assert!(physical < self.capacity as usize);
        // SAFETY: caller establishes initialized, live storage within capacity.
        debug_assert!(!ErlTerm(unsafe { *self.buf.add(physical) }).is_tombstone());
        // SAFETY: caller establishes the writable slot; &mut self is exclusive.
        unsafe { *self.buf.add(physical) = crate::term::TOMBSTONE_VALUE; }
        self.tombstone_count += 1;
        self.trim_head_tombstones();
    }

    /// Accept (remove) the message most recently returned by `try_match_next`.
    /// That message sits at `scan_pos - 1` because `try_match_next` increments
    /// scan_pos after returning the message.
    pub fn accept_last(&mut self) -> ErlTerm {
        assert!(self.scan_pos > 0, "accept_last: no message was scanned");
        self.remove_at((self.scan_pos - 1) as usize)
    }

    /// Advance `head` past any leading tombstones, shrinking `len` and
    /// `tombstone_count` correspondingly. Adjusts `scan_pos` so it
    /// remains valid relative to the new head.
    fn trim_head_tombstones(&mut self) {
        while self.tombstone_count > 0 && self.len > 0 {
            let phys = self.physical_index(0);
            if unsafe { *self.buf.add(phys) } != crate::term::TOMBSTONE_VALUE {
                break;
            }
            self.head = (self.head + 1) & (self.capacity - 1);
            self.len -= 1;
            self.tombstone_count -= 1;
            if self.scan_pos > 0 {
                self.scan_pos -= 1;
            }
        }
    }

    /// Compact the ring buffer by copying live messages contiguously
    /// and eliminating all tombstones. Only safe when no receive scan
    /// is in progress (`scan_pos == 0`).
    fn compact(&mut self) {
        if self.tombstone_count == 0 {
            return;
        }
        debug_assert_eq!(self.scan_pos, 0, "compact: must not run during active scan");
        let alive = self.len - self.tombstone_count;
        let mut write_pos: u32 = 0;
        for read_pos in 0..self.len {
            let phys_r = self.physical_index(read_pos);
            let val = unsafe { *self.buf.add(phys_r) };
            if val != crate::term::TOMBSTONE_VALUE {
                if write_pos != read_pos {
                    let phys_w = self.physical_index(write_pos);
                    unsafe {
                        *self.buf.add(phys_w) = val;
                    }
                }
                write_pos += 1;
            }
        }
        debug_assert_eq!(write_pos, alive);
        self.len = alive;
        self.tombstone_count = 0;
    }

    /// Trigger compaction when tombstones exceed 50% of slots, with a
    /// minimum threshold to avoid compacting tiny mailboxes.
    fn maybe_compact(&mut self) {
        if self.scan_pos == 0 && self.tombstone_count > 8 && self.tombstone_count * 2 >= self.len {
            self.compact();
        }
    }

    /// Iterate over all live messages in the mailbox (read-only).
    pub fn messages(&self) -> MailboxIter<'_> {
        MailboxIter {
            mailbox: self,
            pos: 0,
        }
    }

    /// Gather all live messages into a Vec for GC root scanning.
    /// Tombstones are skipped.
    pub fn gather_roots(&self) -> Vec<ErlTerm> {
        let alive = self.alive_count();
        let mut roots = Vec::with_capacity(alive);
        for i in 0..self.len {
            let phys = self.physical_index(i);
            let val = unsafe { *self.buf.add(phys) };
            if val != crate::term::TOMBSTONE_VALUE {
                roots.push(ErlTerm(val));
            }
        }
        roots
    }

    /// Scatter GC-updated roots back into the ring buffer.
    /// `roots` must have exactly `alive_count()` elements.
    /// Tombstone slots are preserved.
    /// This GC operation is unavailable to safe library clients.
    /// ```compile_fail
    /// use nera_runtime::{process::Mailbox, term::ErlTerm};
    /// let mut mailbox = Mailbox::new();
    /// mailbox.send(ErlTerm::small_int(1));
    /// mailbox.scatter_roots(&[ErlTerm::small_int(2)]);
    /// ```
    /// # Safety
    /// Each root must be a valid, current GC-updated term for the owning
    /// process heap, never the internal tombstone sentinel. The live slot set
    /// must be unchanged since roots were gathered.
    pub(crate) unsafe fn scatter_roots(&mut self, roots: &[ErlTerm]) {
        assert_eq!(roots.len(), self.alive_count());
        debug_assert!(roots.iter().all(|root| !root.is_tombstone()));
        let mut root_idx = 0;
        for i in 0..self.len {
            let phys = self.physical_index(i);
            let val = unsafe { *self.buf.add(phys) };
            if val != crate::term::TOMBSTONE_VALUE {
                unsafe {
                    *self.buf.add(phys) = roots[root_idx].0;
                }
                root_idx += 1;
            }
        }
        debug_assert_eq!(root_idx, roots.len());
    }

    /// Get the slot at logical index `i`, including tombstones.
    ///
    /// # Panics
    /// Panics if `i` is outside the occupied slots, in all build profiles.
    #[inline]
    pub fn get(&self, i: usize) -> ErlTerm {
        assert!(i < self.len as usize, "mailbox get: index out of bounds");
        let phys = self.physical_index(i as u32);
        // SAFETY: the bounds check establishes an initialized occupied slot.
        ErlTerm(unsafe { *self.buf.add(phys) })
    }
}

/// Iterator over live mailbox messages (skips tombstones).
pub struct MailboxIter<'a> {
    mailbox: &'a Mailbox,
    pos: u32,
}

impl<'a> Iterator for MailboxIter<'a> {
    type Item = ErlTerm;
    fn next(&mut self) -> Option<ErlTerm> {
        while self.pos < self.mailbox.len {
            let phys = self.mailbox.physical_index(self.pos);
            self.pos += 1;
            let val = unsafe { *self.mailbox.buf.add(phys) };
            if val != crate::term::TOMBSTONE_VALUE {
                return Some(ErlTerm(val));
            }
        }
        None
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.mailbox.len - self.pos) as usize;
        (0, Some(remaining))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn context_execution_status_domain_and_abi() {
        let states = [ExecutionStatus::Ok, ExecutionStatus::Yield,
            ExecutionStatus::ReceiveWait, ExecutionStatus::Exception,
            ExecutionStatus::IoWait, ExecutionStatus::Hibernate, ExecutionStatus::TailCall];
        assert_eq!(std::mem::size_of::<ExecutionStatus>(), 4);
        assert_eq!(std::mem::align_of::<ExecutionStatus>(), std::mem::align_of::<u32>());
        assert_eq!(std::mem::size_of::<ProcessId>(), 8);
        assert_eq!(CTX_OFFSET_PID, 0);
        assert_eq!(CTX_OFFSET_STATUS, 16);
        assert_eq!(CTX_OFFSET_RESULT, 24);
        let mut ctx = ProcessContext::new(ProcessId::new(17));
        for (raw, state) in (0..=6).zip(states) {
            assert_eq!(state as u32, raw);
            assert_eq!(ExecutionStatus::try_from(raw), Ok(state));
            ctx.status = state;
            // SAFETY: read the initialized repr(u32) slot as its ABI word.
            assert_eq!(unsafe { std::ptr::addr_of!(ctx.status).cast::<u32>().read() }, raw);
        }
        for raw in [7, 16, u32::MAX] {
            assert_eq!(ExecutionStatus::try_from(raw), Err(InvalidExecutionStatus(raw)));
        }
    }

    #[test]
    fn process_identifier_single_worker_path_is_bounded_under_external_contention() {
        use std::sync::atomic::Ordering::Relaxed;
        let ids = Arc::new(ProcessIdAllocator {
            cursor: std::sync::atomic::AtomicU64::new(ProcessId::MAX - 127),
            use_blocks: false,
        });
        let values = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8).map(|_| {
                let ids = &ids;
                scope.spawn(move || (0..256).filter_map(|_| ids.try_next()).collect::<Vec<_>>())
            }).collect();
            handles.into_iter().flat_map(|h| h.join().unwrap()).collect::<Vec<_>>()
        });
        assert_eq!(values.len(), 128);
        assert_eq!(values.into_iter().collect::<std::collections::HashSet<_>>().len(), 128);
        assert_eq!(ids.cursor.load(Relaxed), ProcessId::MAX + 1);
        assert_eq!(ids.try_next(), None);
        let fresh = Arc::new(ProcessIdAllocator::new(1));
        assert_eq!(fresh.next().get(), 1);
        assert_eq!(fresh.cursor.load(Relaxed), 2);
    }

    #[test]
    fn process_identifier_block_reservation_keeps_remaining_ids_private() {
        use std::sync::atomic::Ordering::Relaxed;
        let ids = Arc::new(ProcessIdAllocator::new(2));
        let first = ids.try_next().unwrap();
        assert_eq!(first.get(), 1);
        assert_eq!(ids.cursor.load(Relaxed), 65, "one allocation must reserve 64 identifiers");
        let other = std::thread::scope(|scope| scope.spawn(|| ids.try_next().unwrap()).join().unwrap());
        assert_eq!(other.get(), 65);
        assert_eq!(ids.try_next().unwrap().get(), 2);
    }

    #[test]
    fn process_identifier_blocks_are_disjoint_across_threads_and_refills() {
        let ids = Arc::new(ProcessIdAllocator::new(2));
        let barrier = std::sync::Barrier::new(8);
        let values = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8).map(|_| {
                let ids = &ids;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    (0..1025).map(|_| ids.try_next().unwrap()).collect::<Vec<_>>()
                })
            }).collect();
            handles.into_iter().flat_map(|h| h.join().unwrap()).collect::<Vec<_>>()
        });
        assert_eq!(values.len(), 8 * 1025);
        let unique: std::collections::HashSet<_> = values.into_iter().collect();
        assert_eq!(unique.len(), 8 * 1025);
        assert!(unique.iter().all(|id| id.get() > 0 && id.get() <= ProcessId::MAX));
    }

    #[test]
    fn process_identifier_cache_tracks_allocator_identity_without_retaining_it() {
        let first = Arc::new(ProcessIdAllocator::new(2));
        let second = Arc::new(ProcessIdAllocator::new(2));
        assert_eq!(first.next().get(), 1);
        assert_eq!(second.next().get(), 1);
        assert_eq!(first.next().get(), 65); // abandoned IDs cannot be reused
        assert_eq!(second.next().get(), 65);
        let weak = Arc::downgrade(&second);
        drop(second);
        assert!(weak.upgrade().is_none(), "TLS must not retain the allocator owner");
        let replacement = Arc::new(ProcessIdAllocator::new(2));
        assert_eq!(replacement.next().get(), 1);
    }

    #[test]
    fn process_identifier_final_partial_block_survives_global_exhaustion() {
        use std::sync::atomic::Ordering::Relaxed;
        let ids = Arc::new(ProcessIdAllocator::starting_at(ProcessId::MAX - 1));
        assert_eq!(ids.next().get(), ProcessId::MAX - 1);
        assert_eq!(ids.cursor.load(Relaxed), ProcessId::MAX + 1);
        assert_eq!(std::thread::scope(|s| s.spawn(|| ids.try_next()).join().unwrap()), None);
        assert_eq!(ids.next().get(), ProcessId::MAX);
        assert_eq!(ids.try_next(), None);
        assert_eq!(ids.cursor.load(Relaxed), ProcessId::MAX + 1);
    }

    #[test]
    fn process_identifier_allocator_saturates_without_reuse() {
        use std::sync::atomic::Ordering::Relaxed;
        let ids = Arc::new(ProcessIdAllocator::starting_at(ProcessId::MAX - 2));
        let mut values = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8).map(|_| {
                let ids = &ids;
                scope.spawn(move || (0..8).filter_map(|_| ids.try_next()).collect::<Vec<_>>())
            }).collect();
            handles.into_iter().flat_map(|handle| handle.join().unwrap()).collect::<Vec<_>>()
        });
        values.sort();
        assert_eq!(values, (ProcessId::MAX - 2..=ProcessId::MAX).map(ProcessId::new).collect::<Vec<_>>());
        assert_eq!(ids.cursor.load(Relaxed), ProcessId::MAX + 1);
        assert_eq!(ids.try_next(), None);
        assert!(std::panic::catch_unwind(|| ids.next()).is_err());
        assert_eq!(ids.cursor.load(Relaxed), ProcessId::MAX + 1);
    }

    #[test]
    fn mailbox_rejects_tombstone_send_without_mutation() {
        for existing in [0, 3] {
            let mut mb = Mailbox::new();
            for i in 0..existing {
                mb.send(ErlTerm::small_int(i));
            }
            let before = (
                mb.buf,
                mb.capacity,
                mb.head,
                mb.len,
                mb.scan_pos,
                mb.tombstone_count,
            );
            let messages: Vec<_> = mb.messages().collect();
            let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                mb.send(ErlTerm(crate::term::TOMBSTONE_VALUE))
            }));
            assert!(rejected.is_err(), "tombstone was accepted in release");
            assert_eq!(
                (
                    mb.buf,
                    mb.capacity,
                    mb.head,
                    mb.len,
                    mb.scan_pos,
                    mb.tombstone_count
                ),
                before
            );
            assert_eq!(mb.messages().collect::<Vec<_>>(), messages);
        }
    }

    #[test]
    fn mailbox_rejects_double_remove_without_mutation() {
        let mut mb = Mailbox::new();
        for i in 0..3 {
            mb.send(ErlTerm::small_int(i));
        }
        mb.remove_at(1);
        assert!(mb.try_match_next().is_some());
        let before = (
            mb.buf,
            mb.capacity,
            mb.head,
            mb.len,
            mb.scan_pos,
            mb.tombstone_count,
        );
        let messages: Vec<_> = mb.messages().collect();
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mb.remove_at(1)));
        assert!(rejected.is_err(), "double removal was accepted in release");
        assert_eq!(
            (
                mb.buf,
                mb.capacity,
                mb.head,
                mb.len,
                mb.scan_pos,
                mb.tombstone_count
            ),
            before
        );
        assert_eq!(mb.messages().collect::<Vec<_>>(), messages);
    }
    #[test]
    fn mailbox_get_rejects_out_of_range_in_release() {
        let mut mb = Mailbox::new();
        // Initialize the whole ring: even the pre-fix unchecked read is valid
        // memory, so RED demonstrates a missing bounds check without UB.
        for i in 0..16 {
            mb.send(ErlTerm::small_int(i));
        }
        for index in [16, usize::MAX] {
            let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mb.get(index)));
            assert!(rejected.is_err(), "out-of-range index {index} was accepted");
        }
    }

    #[test]
    fn exit_scan_keeps_next_signal_after_head_removal() {
        use crate::atom_table::{ATOM_EXIT_SIGNAL, ATOM_KILL, ATOM_KILLED, ATOM_NORMAL};
        let mut proc = Process::new(crate::process::ProcessId::new(1));
        for reason in [ATOM_NORMAL, ATOM_KILL] {
            let signal = proc.heap.alloc_tuple(&[
                ErlTerm::atom(ATOM_EXIT_SIGNAL),
                ErlTerm::pid(crate::process::ProcessId::new(2)),
                ErlTerm::atom(reason),
            ]);
            proc.mailbox.send(signal);
        }
        // Fill the ring so the pre-fix stale-length read cannot read uninitialized
        // slots; it still incorrectly skips the kill after removing the head.
        for i in 0..14 {
            proc.mailbox.send(ErlTerm::small_int(i));
        }
        // SAFETY: all mailbox terms are initialized on this live, unmoving heap.
        assert_eq!(
            unsafe { process_exit_signals(&mut proc) },
            Some(ErlTerm::atom(ATOM_KILLED))
        );
        assert_eq!(
            proc.mailbox.messages().collect::<Vec<_>>(),
            (0..14).map(ErlTerm::small_int).collect::<Vec<_>>()
        );
    }

    #[test]
    fn exit_reason_stays_owned_through_scan_and_collection() {
        use crate::atom_table::ATOM_EXIT_SIGNAL;
        let entry = ProcessEntry::new(Process::new(crate::process::ProcessId::new(1)));
        let mut proc = entry.lock_for_runtime();
        let bytes = vec![42; 256];
        let binary = proc.heap.alloc_binary_auto(&bytes);
        let reason = proc.heap.alloc_tuple(&[binary, ErlTerm::small_int(7)]);
        let signal =
            proc.heap
                .alloc_tuple(&[ErlTerm::atom(ATOM_EXIT_SIGNAL), ErlTerm::pid(crate::process::ProcessId::new(2)), reason]);
        proc.mailbox.send(signal);
        // SAFETY: all roots are immediate or initialized on this process heap,
        // held exclusively; the mailbox roots are updated by the collector.
        unsafe { proc.collect_garbage_full() };
        // SAFETY: the collector updated the owned mailbox graph, the guard keeps
        // it exclusively live, and the returned reason is rooted immediately.
        proc.context.result = unsafe { process_exit_signals(&mut proc) }.unwrap().to_raw();
        assert!(proc.mailbox.is_empty());
        // SAFETY: context.result is a collected root; no raw term is reused.
        unsafe { proc.collect_garbage_full() };
        let reason = ErlTerm(proc.context.result);
        // SAFETY: the reason is the current, immutable tuple on this live heap.
        let ptr = reason.as_boxed_ptr().unwrap();
        let header = unsafe { crate::term::BoxedHeader(*ptr) };
        assert_eq!(header.subtype(), crate::term::BOXED_TUPLE);
        assert_eq!(header.arity(), 2);
        // SAFETY: the initialized tuple has two fields and no mutation intervenes.
        let binary = unsafe { ErlTerm(*ptr.add(1)) };
        let value = unsafe { ErlTerm(*ptr.add(2)) };
        assert_eq!(value, ErlTerm::small_int(7));
        // SAFETY: binary belongs to the current reason graph kept alive by proc.
        assert_eq!(
            unsafe { binary.borrow(&proc.heap) }.binary_bytes(),
            Some(bytes.as_slice())
        );
    }

    #[test]
    fn mailbox_get_rejects_empty_and_preserves_valid_slots() {
        let mut mb = Mailbox::new();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mb.get(0))).is_err());
        for i in 0..32 {
            mb.send(ErlTerm::small_int(i));
        }
        for _ in 0..20 {
            mb.remove_at(0);
        }
        for i in 32..48 {
            mb.send(ErlTerm::small_int(i));
        }
        // Wrapped ring with an interior tombstone: get addresses slots, not
        // the live-message count, and must not silently change that contract.
        mb.remove_at(3);
        assert!(mb.get(3).is_tombstone());
        assert_eq!(mb.get(0), ErlTerm::small_int(20));
        assert_eq!(mb.get(mb.slot_count() as usize - 1), ErlTerm::small_int(47));
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || mb.get(mb.slot_count() as usize)
        ))
        .is_err());
    }

    #[test]
    fn exit_scan_handles_wrapping_tombstones_and_consecutive_normals() {
        use crate::atom_table::{ATOM_EXIT_SIGNAL, ATOM_KILL, ATOM_KILLED, ATOM_NORMAL};
        for trap_exit in [false, true] {
            let mut proc = Process::new(crate::process::ProcessId::new(1));
            proc.otp.trap_exit = trap_exit;
            let normal = proc.heap.alloc_tuple(&[
                ErlTerm::atom(ATOM_EXIT_SIGNAL),
                ErlTerm::pid(crate::process::ProcessId::new(2)),
                ErlTerm::atom(ATOM_NORMAL),
            ]);
            let kill = proc.heap.alloc_tuple(&[
                ErlTerm::atom(ATOM_EXIT_SIGNAL),
                ErlTerm::pid(crate::process::ProcessId::new(2)),
                ErlTerm::atom(ATOM_KILL),
            ]);
            for _ in 0..16 {
                proc.mailbox.send(ErlTerm::small_int(0));
            }
            for _ in 0..14 {
                proc.mailbox.remove_at(0);
            }
            proc.mailbox.send(normal);
            proc.mailbox.send(ErlTerm::small_int(99));
            proc.mailbox.send(normal);
            proc.mailbox.send(normal);
            proc.mailbox.send(kill);
            proc.mailbox.remove_at(3); // a tombstone between EXIT signals
            proc.mailbox.remove_at(0);
            proc.mailbox.remove_at(0); // head wraps during the next signal removal
            // SAFETY: all mailbox terms are initialized on this live, unmoving heap.
            assert_eq!(
                unsafe { process_exit_signals(&mut proc) },
                Some(ErlTerm::atom(ATOM_KILLED))
            );
            let remaining: Vec<_> = proc.mailbox.messages().collect();
            assert_eq!(remaining, if trap_exit { vec![normal; 3] } else { vec![] });
        }
        let mut proc = Process::new(crate::process::ProcessId::new(1));
        let normal = proc.heap.alloc_tuple(&[
            ErlTerm::atom(ATOM_EXIT_SIGNAL),
            ErlTerm::pid(crate::process::ProcessId::new(2)),
            ErlTerm::atom(ATOM_NORMAL),
        ]);
        for _ in 0..3 {
            proc.mailbox.send(normal);
        }
        // SAFETY: every queued tuple is initialized on this live, unmoving heap.
        assert_eq!(unsafe { process_exit_signals(&mut proc) }, None);
        assert!(proc.mailbox.is_empty());
    }
    #[test]
    fn mailbox_rejects_overflowing_growth_without_mutation() {
        for active_scan in [false, true] {
            let mut mb = Mailbox::new();
            for i in 0..8 { mb.send(ErlTerm::small_int(i)); }
            mb.remove_at(4);
            if active_scan { assert!(mb.try_match_next().is_some()); }
            let original = (mb.buf, mb.capacity, mb.head, mb.len, mb.scan_pos, mb.tombstone_count);
            let messages: Vec<_> = mb.messages().collect();
            for request in [(1u32 << 31) + 1, u32::MAX] {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mb.grow(request)));
                assert!(result.is_err(), "out-of-domain growth must be rejected");
                assert_eq!((mb.buf, mb.capacity, mb.head, mb.len, mb.scan_pos, mb.tombstone_count), original);
                assert_eq!(mb.messages().collect::<Vec<_>>(), messages);
            }
            mb.send(ErlTerm::small_int(99));
            assert_eq!(mb.messages().last(), Some(ErlTerm::small_int(99)));
        }
    }

    #[test]
    fn hibernate_retains_mailbox_and_dictionary_graphs() {
        use crate::binary::BinaryData;
        let mut process = Process::new(crate::process::ProcessId::new(1));
        let data = std::sync::Arc::new(BinaryData::new(vec![7; 256]));
        let live = std::sync::Arc::downgrade(&data);
        let binary = process.heap.alloc_refc_binary_from_arc(data, 0, 256);
        let key = process.heap.alloc_binary(b"heap dictionary key");
        let list = process.heap.alloc_list_from_slice(&[ErlTerm::small_int(42), binary]);
        let message = process.heap.alloc_tuple(&[binary, list]);
        process.mailbox.send(message);
        process.dictionary.push((key, message));
        process.heap.set_max_heap_size(100000);
        process.heap.set_fullsweep_after(3);
        // SAFETY: these process-owned roots are live and exclusively accessed.
        // Promote first to exercise old-generation retention too.
        unsafe { process.collect_garbage_full() };
        let garbage = std::sync::Arc::new(BinaryData::new(vec![9; 256]));
        let dead = std::sync::Arc::downgrade(&garbage);
        let dead_term = process.heap.alloc_refc_binary_from_arc(garbage, 0, 256);
        process.initial_args.push(dead_term);
        process.context.result = dead_term.to_raw();
        unsafe { process.hibernate() };
        // Check ownership before reading term memory; the baseline fails here
        // without dereferencing any freed pointer.
        assert!(live.upgrade().is_some(), "hibernation freed a live binary owner");
        assert!(dead.upgrade().is_none(), "discarded execution roots survived");
        assert_eq!(process.heap.max_heap_size_words(), 100000);
        assert_eq!(process.heap.get_fullsweep_after(), 3);
        assert!(process.heap.bytes_allocated() > 0);
        let mailbox_term = process.mailbox.messages().next().unwrap();
        assert_eq!(mailbox_term, process.dictionary[0].1, "shared graph was duplicated");
        // SAFETY: hibernation retained these graphs on the process heap.
        unsafe {
            assert_eq!(process.dictionary[0].0.borrow(&process.heap).binary_bytes(), Some(&b"heap dictionary key"[..]));
            let tuple = mailbox_term.as_boxed_ptr().unwrap();
            let retained_binary = ErlTerm::from_raw(*tuple.add(1));
            assert_eq!(retained_binary.borrow(&process.heap).binary_bytes(), Some(&vec![7; 256][..]));
        }
        unsafe { process.collect_garbage_full() };
        assert!(live.upgrade().is_some());
    }

    #[test]
    fn hibernate_retains_external_storage() {
        let mut process = Process::new(crate::process::ProcessId::new(1));
        let storage = std::sync::Arc::new(vec![
            crate::term::BoxedHeader::new(crate::term::BoxedSubtype::Float, 1).0,
            42.0_f64.to_bits(),
        ]);
        let owner = std::sync::Arc::downgrade(&storage);
        let term = ErlTerm::boxed(storage.as_ptr()).with_external_bit();
        process.heap.retain_borrowed_heap(storage);
        process.dictionary.push((ErlTerm::small_int(1), term));
        // SAFETY: the initialized external graph is anchored by this heap.
        unsafe { process.hibernate(); }
        assert!(owner.upgrade().is_some(), "external storage owner was lost");
        // SAFETY: hibernation preserved this immutable, initialized external graph.
        unsafe {
            assert_eq!(*process.dictionary[0].1.as_boxed_ptr().unwrap().add(1), 42.0_f64.to_bits());
            process.collect_garbage_full();
        }
        assert!(owner.upgrade().is_some());
    }

    #[test]
    fn hibernate_reclaims_heap_with_immediate_state() {
        let mut process = Process::new(crate::process::ProcessId::new(1));
        let term = process.heap.alloc_float(42.0);
        process.context.result = term.to_raw();
        process.initial_args.push(term);
        process.mailbox.send(ErlTerm::small_int(7));
        process.dictionary.push((ErlTerm::small_int(1), ErlTerm::small_int(2)));
        // SAFETY: retained roots are immediate; discarded execution roots are live.
        unsafe { process.hibernate(); }
        assert_eq!(process.heap.bytes_allocated(), 0);
        assert_eq!(process.mailbox.messages().collect::<Vec<_>>(), vec![ErlTerm::small_int(7)]);
        assert_eq!(process.dictionary, vec![(ErlTerm::small_int(1), ErlTerm::small_int(2))]);
        assert!(process.initial_args.is_empty());
        assert_eq!(process.context.result, ErlTerm::nil().to_raw());
    }

    #[test]
    fn hibernate_arguments_own_their_source_storage() {
        let mut process = Process::new(crate::process::ProcessId::new(1));
        let term = process.heap.alloc_binary(b"owned hibernate argument");
        // SAFETY: term was allocated on this process heap.
        unsafe { process.retain_hibernate_args(0, 0, vec![term]); }
        unsafe { process.hibernate() };
        let (_, _, args) = process.hibernate_mfa.take().unwrap();
        let copied = args.into_iter().next().unwrap().copy_to_heap(&mut process.heap);
        // SAFETY: the consumed fragment transferred/copied its storage to the heap.
        assert_eq!(unsafe { copied.borrow(&process.heap) }.binary_bytes(), Some(&b"owned hibernate argument"[..]));
    }

    #[test]
    fn mailbox_send_and_len() {
        let mut mb = Mailbox::new();
        assert!(mb.is_empty());
        mb.send(ErlTerm::small_int(1));
        mb.send(ErlTerm::small_int(2));
        assert_eq!(mb.len(), 2);
        assert!(!mb.is_empty());
    }

    #[test]
    fn mailbox_scan() {
        let mut mb = Mailbox::new();
        mb.send(ErlTerm::small_int(10));
        mb.send(ErlTerm::small_int(20));
        mb.send(ErlTerm::small_int(30));

        let (idx0, msg0) = mb.try_match_next().unwrap();
        assert_eq!(idx0, 0);
        assert_eq!(msg0.as_small_int(), Some(10));

        let (idx1, msg1) = mb.try_match_next().unwrap();
        assert_eq!(idx1, 1);
        assert_eq!(msg1.as_small_int(), Some(20));

        let (idx2, msg2) = mb.try_match_next().unwrap();
        assert_eq!(idx2, 2);
        assert_eq!(msg2.as_small_int(), Some(30));

        assert!(mb.try_match_next().is_none());
    }

    #[test]
    fn mailbox_remove_at() {
        let mut mb = Mailbox::new();
        mb.send(ErlTerm::small_int(1));
        mb.send(ErlTerm::small_int(2));
        mb.send(ErlTerm::small_int(3));

        // Remove the middle message.
        let removed = mb.remove_at(1);
        assert_eq!(removed.as_small_int(), Some(2));
        assert_eq!(mb.len(), 2);

        // Remaining: [1, 3]
        mb.reset_scan();
        let (_, m0) = mb.try_match_next().unwrap();
        assert_eq!(m0.as_small_int(), Some(1));
        let (_, m1) = mb.try_match_next().unwrap();
        assert_eq!(m1.as_small_int(), Some(3));
    }

    #[test]
    fn mailbox_reset_scan() {
        let mut mb = Mailbox::new();
        mb.send(ErlTerm::small_int(42));
        let _ = mb.try_match_next();
        assert!(mb.try_match_next().is_none());

        mb.reset_scan();
        let (_, msg) = mb.try_match_next().unwrap();
        assert_eq!(msg.as_small_int(), Some(42));
    }

    #[test]
    fn mailbox_messages_iter() {
        let mut mb = Mailbox::new();
        mb.send(ErlTerm::small_int(5));
        mb.send(ErlTerm::small_int(6));
        let all: Vec<_> = mb.messages().collect();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].as_small_int(), Some(5));
        assert_eq!(all[1].as_small_int(), Some(6));
    }

    #[test]
    fn mailbox_gather_scatter_roots() {
        let mut mb = Mailbox::new();
        mb.send(ErlTerm::small_int(10));
        mb.send(ErlTerm::small_int(20));
        mb.send(ErlTerm::small_int(30));

        let mut roots = mb.gather_roots();
        assert_eq!(roots.len(), 3);
        // Simulate GC updating roots
        roots[1] = ErlTerm::small_int(99);
        // SAFETY: gathered slots are unchanged; only valid immediate roots were updated.
        unsafe { mb.scatter_roots(&roots); }
        assert_eq!(mb.get(1).as_small_int(), Some(99));
    }

    #[test]
    fn mailbox_ring_buffer_wrap() {
        let mut mb = Mailbox::new();
        // Fill and drain to force wrap-around
        for i in 0..20 {
            mb.send(ErlTerm::small_int(i));
        }
        // Remove from front to advance head
        for _ in 0..15 {
            mb.remove_at(0);
        }
        // Now head is advanced, add more to wrap
        for i in 20..30 {
            mb.send(ErlTerm::small_int(i));
        }
        // Verify all messages are correct
        assert_eq!(mb.len(), 15); // 5 remaining + 10 new
        for (idx, msg) in mb.messages().enumerate() {
            let expected = (15 + idx) as i64;
            assert_eq!(
                msg.as_small_int(),
                Some(expected),
                "mismatch at idx {}",
                idx
            );
        }
    }

    // ---------------------------------------------------------------
    // Tombstone-specific tests
    // ---------------------------------------------------------------

    #[test]
    fn mailbox_tombstone_basic() {
        let mut mb = Mailbox::new();
        mb.send(ErlTerm::small_int(1));
        mb.send(ErlTerm::small_int(2));
        mb.send(ErlTerm::small_int(3));

        // Remove middle — becomes tombstone
        let removed = mb.remove_at(1);
        assert_eq!(removed.as_small_int(), Some(2));
        assert_eq!(mb.len(), 2); // alive count
        assert_eq!(mb.alive_count(), 2);

        // Scan should yield [1, 3], skipping tombstone
        mb.reset_scan();
        let (_, m0) = mb.try_match_next().unwrap();
        assert_eq!(m0.as_small_int(), Some(1));
        let (_, m1) = mb.try_match_next().unwrap();
        assert_eq!(m1.as_small_int(), Some(3));
        assert!(mb.try_match_next().is_none());
    }

    #[test]
    fn mailbox_tombstone_skip_during_scan() {
        let mut mb = Mailbox::new();
        for i in 0..5 {
            mb.send(ErlTerm::small_int(i));
        }
        // Remove indices 1 and 3 (values 1 and 3).
        // With tombstones, indices don't shift, so index 3 is still value 3.
        mb.remove_at(1);
        mb.remove_at(3);

        assert_eq!(mb.len(), 3); // alive: values 0, 2, 4

        mb.reset_scan();
        let vals: Vec<i64> =
            std::iter::from_fn(|| mb.try_match_next().map(|(_, m)| m.as_small_int().unwrap()))
                .collect();
        assert_eq!(vals, vec![0, 2, 4]);
    }

    #[test]
    fn mailbox_trim_head_tombstones() {
        let mut mb = Mailbox::new();
        mb.send(ErlTerm::small_int(1));
        mb.send(ErlTerm::small_int(2));
        mb.send(ErlTerm::small_int(3));

        // Remove head — should be immediately trimmed
        mb.remove_at(0);
        // After trim: head advances, len shrinks, tombstone_count stays 0
        assert_eq!(mb.len(), 2);
        assert_eq!(mb.tombstone_count, 0);

        // Remove new head
        mb.remove_at(0);
        assert_eq!(mb.len(), 1);
        assert_eq!(mb.tombstone_count, 0);

        mb.reset_scan();
        let (_, m) = mb.try_match_next().unwrap();
        assert_eq!(m.as_small_int(), Some(3));
    }

    #[test]
    fn mailbox_accept_with_tombstones() {
        let mut mb = Mailbox::new();
        mb.send(ErlTerm::small_int(10));
        mb.send(ErlTerm::small_int(20));
        mb.send(ErlTerm::small_int(30));

        // Simulate selective receive: scan past 10 and 20, accept 30
        mb.reset_scan();
        let _ = mb.try_match_next(); // 10
        let _ = mb.try_match_next(); // 20
        let (_, msg) = mb.try_match_next().unwrap(); // 30
        assert_eq!(msg.as_small_int(), Some(30));
        let accepted = mb.accept_last();
        assert_eq!(accepted.as_small_int(), Some(30));

        // 10 and 20 remain
        assert_eq!(mb.len(), 2);
        mb.reset_scan();
        let (_, m0) = mb.try_match_next().unwrap();
        assert_eq!(m0.as_small_int(), Some(10));
        let (_, m1) = mb.try_match_next().unwrap();
        assert_eq!(m1.as_small_int(), Some(20));
        assert!(mb.try_match_next().is_none());
    }

    #[test]
    fn mailbox_compaction_trigger() {
        let mut mb = Mailbox::new();
        // Send 20 messages
        for i in 0..20 {
            mb.send(ErlTerm::small_int(i));
        }
        // Remove 15 non-head messages to build up tombstones
        // Remove from back to front to avoid head-trim clearing them
        for i in (5..20).rev() {
            mb.remove_at(i);
        }
        // Now we have 5 alive messages at front (0..5), tombstones 5..20
        // tombstone_count may be less because head-trim ate some, but
        // the middle/tail ones remain as tombstones.
        assert_eq!(mb.len(), 5);

        // Reset scan triggers maybe_compact
        mb.reset_scan();
        assert_eq!(
            mb.tombstone_count, 0,
            "compaction should have cleared tombstones"
        );

        // All 5 messages intact
        let vals: Vec<i64> =
            std::iter::from_fn(|| mb.try_match_next().map(|(_, m)| m.as_small_int().unwrap()))
                .collect();
        assert_eq!(vals, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn mailbox_gather_scatter_with_tombstones() {
        let mut mb = Mailbox::new();
        mb.send(ErlTerm::small_int(10));
        mb.send(ErlTerm::small_int(20));
        mb.send(ErlTerm::small_int(30));
        mb.send(ErlTerm::small_int(40));
        mb.send(ErlTerm::small_int(50));

        // Tombstone indices 1 and 3 (values 20, 40)
        mb.remove_at(1);
        mb.remove_at(3);

        let mut roots = mb.gather_roots();
        assert_eq!(roots.len(), 3); // alive: 10, 30, 50
        assert_eq!(roots[0].as_small_int(), Some(10));
        assert_eq!(roots[1].as_small_int(), Some(30));
        assert_eq!(roots[2].as_small_int(), Some(50));

        // Simulate GC updating a root
        roots[1] = ErlTerm::small_int(99);
        // SAFETY: gathered slots are unchanged; only valid immediate roots were updated.
        unsafe { mb.scatter_roots(&roots); }

        // Verify: index 2 (value 30) was updated to 99
        mb.reset_scan();
        let vals: Vec<i64> =
            std::iter::from_fn(|| mb.try_match_next().map(|(_, m)| m.as_small_int().unwrap()))
                .collect();
        assert_eq!(vals, vec![10, 99, 50]);
    }

    #[test]
    fn mailbox_grow_with_tombstones() {
        let mut mb = Mailbox::new();
        // Fill to capacity (16 initial)
        for i in 0..14 {
            mb.send(ErlTerm::small_int(i));
        }
        // Tombstone half of them (non-head)
        for i in (7..14).rev() {
            mb.remove_at(i);
        }
        assert_eq!(mb.len(), 7);

        // Send enough to fill remaining capacity — should compact instead of grow
        for i in 100..110 {
            mb.send(ErlTerm::small_int(i));
        }
        assert_eq!(mb.tombstone_count, 0);
        assert_eq!(mb.len(), 17); // 7 + 10

        // Verify all messages intact and ordered
        mb.reset_scan();
        let vals: Vec<i64> =
            std::iter::from_fn(|| mb.try_match_next().map(|(_, m)| m.as_small_int().unwrap()))
                .collect();
        let expected: Vec<i64> = (0..7).chain(100..110).collect();
        assert_eq!(vals, expected);
    }

    /// Regression: `send()` must not compact or strip tombstones while a
    /// selective receive scan is active (`scan_pos > 0`).  Before the fix,
    /// `send()` called `compact()` unconditionally when the buffer was full,
    /// which corrupted `scan_pos` and caused "compact: must not run during
    /// active scan" panics or silent message loss.
    #[test]
    fn mailbox_send_during_active_scan_no_compact() {
        let mut mb = Mailbox::new();
        // Fill 14 slots (initial capacity = 16).
        for i in 0..14 {
            mb.send(ErlTerm::small_int(i));
        }
        // Tombstone half to create compaction pressure.
        for i in (7..14).rev() {
            mb.remove_at(i);
        }
        // 7 alive + 7 tombstones = 14 slots used, 2 free.

        // Start a selective receive scan — advance into the mailbox.
        mb.reset_scan();
        let (_, first) = mb.try_match_next().unwrap();
        assert_eq!(first.as_small_int(), Some(0));
        // scan_pos is now > 0.
        assert!(mb.scan_pos > 0);

        // Send enough messages to fill the remaining capacity and force
        // a grow/compact path inside send().
        for i in 100..106 {
            mb.send(ErlTerm::small_int(i));
        }
        // scan_pos must still be valid (not reset or corrupted).
        assert!(mb.scan_pos > 0, "scan_pos was corrupted during send");

        // Continue scanning — we should see the rest of the original
        // messages (1..7) followed by the newly sent ones (100..106).
        let mut remaining = Vec::new();
        while let Some((_, m)) = mb.try_match_next() {
            remaining.push(m.as_small_int().unwrap());
        }
        let expected: Vec<i64> = (1..7).chain(100..106).collect();
        assert_eq!(remaining, expected);
    }

    #[test]
    fn mailbox_is_empty_all_tombstoned() {
        let mut mb = Mailbox::new();
        mb.send(ErlTerm::small_int(1));
        mb.send(ErlTerm::small_int(2));
        mb.send(ErlTerm::small_int(3));

        // Remove all — head-trim clears them immediately
        mb.remove_at(0);
        mb.remove_at(0);
        mb.remove_at(0);
        assert!(mb.is_empty());
        assert_eq!(mb.len(), 0);
        assert_eq!(mb.tombstone_count, 0);
    }

    #[test]
    fn mailbox_iter_skips_tombstones() {
        let mut mb = Mailbox::new();
        for i in 0..5 {
            mb.send(ErlTerm::small_int(i));
        }
        mb.remove_at(1);
        mb.remove_at(3);
        let msgs: Vec<i64> = mb.messages().map(|m| m.as_small_int().unwrap()).collect();
        assert_eq!(msgs, vec![0, 2, 4]);
    }

    #[test]
    fn process_new_defaults() {
        let p = Process::new(crate::process::ProcessId::new(1));
        assert_eq!(p.id, crate::process::ProcessId::new(1));
        assert_eq!(p.status, ProcessStatus::Ready);
        assert_eq!(p.reductions, REDUCTION_LIMIT);
        assert!(p.mailbox.is_empty());
        assert!(p.otp.wait_reason.is_none());
        assert!(p.otp.links.is_empty());
        assert!(p.otp.monitors_outgoing.is_empty());
        assert!(p.otp.monitors_incoming.is_empty());
        assert!(p.otp.registered_name.is_none());
        assert!(p.otp.owned_timers.is_empty());
    }

    #[test]
    fn process_wait_transitions_preserve_timeout_fallback() {
        let mut p = Process::new(crate::process::ProcessId::new(1));
        assert_eq!(p.receive_timeout_deadline_ms(), None);

        p.context.receive_timeout_deadline = 123;
        p.transition_to(ProcessState::Waiting(WaitReason::Receive { timeout_deadline_ms: p.receive_timeout_deadline_ms() }));
        assert_eq!(
            p.otp.wait_reason,
            Some(WaitReason::Receive {
                timeout_deadline_ms: Some(123)
            })
        );
        assert_eq!(p.receive_timeout_deadline_ms(), Some(123));

        p.transition_to(ProcessState::Waiting(WaitReason::Timer {
            timer_id: 7,
            deadline_ms: Some(456),
        }));
        assert_eq!(p.receive_timeout_deadline_ms(), Some(456));

        p.transition_to(ProcessState::Ready);
        // Backward-compatibility path while old context timeout field exists.
        assert_eq!(p.receive_timeout_deadline_ms(), Some(123));
    }

    #[test]
    fn process_otp_relationship_helpers() {
        let mut p = Process::new(crate::process::ProcessId::new(1));

        p.set_registered_name(42);
        assert_eq!(p.otp.registered_name, Some(42));
        p.clear_registered_name();
        assert_eq!(p.otp.registered_name, None);

        assert!(p.add_link(crate::process::ProcessId::new(2)));
        assert!(!p.add_link(crate::process::ProcessId::new(2)));
        assert!(p.remove_link(crate::process::ProcessId::new(2)));
        assert!(!p.remove_link(crate::process::ProcessId::new(2)));

        assert_eq!(p.add_outgoing_monitor(10, crate::process::ProcessId::new(3)), None);
        assert_eq!(p.add_outgoing_monitor(10, crate::process::ProcessId::new(4)), Some(crate::process::ProcessId::new(3)));
        assert_eq!(p.remove_outgoing_monitor(10), Some(crate::process::ProcessId::new(4)));
        assert_eq!(p.remove_outgoing_monitor(10), None);

        assert_eq!(p.add_incoming_monitor(11, crate::process::ProcessId::new(8)), None);
        assert_eq!(p.add_incoming_monitor(11, crate::process::ProcessId::new(9)), Some(crate::process::ProcessId::new(8)));
        assert_eq!(p.remove_incoming_monitor(11), Some(crate::process::ProcessId::new(9)));
        assert_eq!(p.remove_incoming_monitor(11), None);

        assert!(p.add_owned_timer(99));
        assert!(!p.add_owned_timer(99));
        assert!(p.remove_owned_timer(99));
        assert!(!p.remove_owned_timer(99));
    }

    #[test]
    fn process_run_slice_no_fn_exits() {
        let mut p = Process::new(crate::process::ProcessId::new(1));
        unsafe { p.run_slice(); }
        assert_eq!(p.status, ProcessStatus::Exited);
    }

    #[test]
    fn process_run_slice_with_rust_fn() {
        let mut p = Process::new(crate::process::ProcessId::new(1));
        p.rust_fn = Some(Box::new(|inner: &mut ProcessInner| {
            inner.finish();
            inner.context.reductions -= 100;
        }));
        unsafe { p.run_slice(); }
        assert_eq!(p.status, ProcessStatus::Exited);
        assert_eq!(p.reductions, REDUCTION_LIMIT - 100);
    }

    // ---------------------------------------------------------------
    // ASTATUS_IO_WAIT and try_wake / try_wake_io separation
    // ---------------------------------------------------------------

    #[test]
    fn guard_drains_and_publishes_only_its_own_entry() {
        use crate::message::ImmediateTerm;
        let first = ProcessEntry::new(Process::new(crate::process::ProcessId::new(1)));
        let second = ProcessEntry::new(Process::new(crate::process::ProcessId::new(2)));
        first
            .inbox
            .push(MessageFragment::Immediate(ImmediateTerm::new(
                ErlTerm::small_int(11),
            )));
        second
            .inbox
            .push(MessageFragment::Immediate(ImmediateTerm::new(
                ErlTerm::small_int(22),
            )));
        let mut guard = first.lock_for_runtime();
        assert!(first.try_lock_for_runtime().is_none());
        guard.drain_inbox();
        guard.transition(ProcessState::Waiting(WaitReason::IoWait));
        assert_eq!(
            guard.mailbox.messages().collect::<Vec<_>>(),
            vec![ErlTerm::small_int(11)]
        );
        assert_eq!(first.published_status(), ASTATUS_IO_WAIT);
        assert_eq!(second.published_status(), ASTATUS_READY);
        assert_eq!(second.inbox.len(), 1);
        assert!(second.try_lock_for_runtime().unwrap().mailbox.is_empty());
        drop(guard);
        let mut guard = first.try_lock_for_runtime().expect("drop releases the original mutex");
        guard.transition(ProcessState::Exited);
        assert_eq!(first.published_status(), ASTATUS_EXITED);
        drop(guard);
        let mut guard = second.try_lock_for_runtime().unwrap();
        guard.drain_inbox();
        assert_eq!(
            guard.mailbox.messages().collect::<Vec<_>>(),
            vec![ErlTerm::small_int(22)]
        );
        assert_eq!(guard.scheduling_state(), ProcessState::Ready);
    }

    #[test]
    fn public_guard_observes_state_without_consuming_wake_ownership() {
        let entry = ProcessEntry::new(Process::new(crate::process::ProcessId::new(1)));
        entry.lock_for_runtime().transition(ProcessState::Waiting(WaitReason::IoWait));
        let guard = entry.lock();
        assert_eq!(guard.status(), ProcessStatus::Waiting);
        assert_eq!(guard.wait_reason(), Some(WaitReason::IoWait));
        assert_eq!(entry.published_status(), ASTATUS_IO_WAIT);
        assert!(entry.try_lock().is_none());
        assert!(entry.try_wake_io());
        assert!(!entry.try_wake_io());
        // A claimed wake owns the runnable slot but does not mutate locked fields.
        assert_eq!(entry.published_status(), ASTATUS_READY);
        assert_eq!(guard.status(), ProcessStatus::Waiting);
        assert_eq!(guard.wait_reason(), Some(WaitReason::IoWait));
        drop(guard);
        let mut runtime = entry.try_lock_for_runtime().unwrap();
        runtime.transition(ProcessState::Running);
        assert_eq!(runtime.status(), ProcessStatus::Running);
        assert_eq!(runtime.wait_reason(), None);
        assert_eq!(entry.published_status(), ASTATUS_RUNNING);
        drop(runtime);
        assert_eq!(entry.lock().status(), ProcessStatus::Running);
    }

    #[test]
    fn published_transitions_keep_wait_reason_and_wake_kind_together() {
        let entry = ProcessEntry::new(Process::new(crate::process::ProcessId::new(1)));
        for reason in [
            WaitReason::Receive {
                timeout_deadline_ms: Some(123),
            },
            WaitReason::IoWait,
            WaitReason::Timer {
                timer_id: 7,
                deadline_ms: Some(456),
            },
        ] {
            let mut process = entry.lock_for_runtime();
            process.transition(ProcessState::Waiting(reason));
            assert_eq!(process.status, ProcessStatus::Waiting);
            assert_eq!(process.otp.wait_reason, Some(reason));
            if reason == WaitReason::IoWait {
                assert!(!entry.try_wake());
                assert!(entry.try_wake_io());
            } else {
                assert!(!entry.try_wake_io());
                assert!(entry.try_wake());
            }
            assert!(!entry.try_wake());
            assert!(!entry.try_wake_io());
            for state in [
                ProcessState::Ready,
                ProcessState::Running,
                ProcessState::Exited,
            ] {
                process.transition(state);
                assert_eq!(process.scheduling_state(), state);
                assert_eq!(process.otp.wait_reason, None);
                assert_eq!(
                    entry.atomic_status.load(Ordering::Acquire),
                    state.atomic_status()
                );
            }
        }
    }

    #[test]
    fn io_completion_keeps_cas_ownership_until_publication() {
        let entry = ProcessEntry::new(Process::new(crate::process::ProcessId::new(1)));
        let mut process = entry.lock_for_runtime();
        process.transition(ProcessState::Waiting(WaitReason::IoWait));
        process.context.result = ErlTerm::small_int(42).to_raw();
        process.transition_to(ProcessState::Ready);
        assert_eq!(entry.atomic_status.load(Ordering::Acquire), ASTATUS_IO_WAIT);
        drop(process);
        assert!(entry.try_wake_io());
        assert!(!entry.try_wake_io());
        assert_eq!(entry.lock_for_runtime().context.result, ErlTerm::small_int(42).to_raw());
    }

    #[test]
    fn concurrent_wakers_claim_one_runnable_slot() {
        use std::sync::{Arc, Barrier};
        for reason in [
            WaitReason::Receive {
                timeout_deadline_ms: None,
            },
            WaitReason::IoWait,
        ] {
            let entry = Arc::new(ProcessEntry::new(Process::new(crate::process::ProcessId::new(1))));
            entry.lock_for_runtime().transition(ProcessState::Waiting(reason));
            let barrier = Arc::new(Barrier::new(8));
            let threads: Vec<_> = (0..8)
                .map(|_| {
                    let entry = Arc::clone(&entry);
                    let barrier = Arc::clone(&barrier);
                    std::thread::spawn(move || {
                        barrier.wait();
                        if reason == WaitReason::IoWait {
                            entry.try_wake_io()
                        } else {
                            entry.try_wake()
                        }
                    })
                })
                .collect();
            assert_eq!(
                threads
                    .into_iter()
                    .map(|t| t.join().unwrap())
                    .filter(|won| *won)
                    .count(),
                1
            );
        }
    }

    #[test]
    fn try_wake_only_wakes_waiting_not_io_wait() {
        let proc = Process::new(crate::process::ProcessId::new(1));
        let entry = ProcessEntry::new(proc);

        // Start in ASTATUS_WAITING (receive wait)
        entry.atomic_status.store(ASTATUS_WAITING, Ordering::SeqCst);
        assert!(entry.try_wake());
        assert_eq!(entry.atomic_status.load(Ordering::SeqCst), ASTATUS_READY);

        // Now set to IO_WAIT — try_wake should fail
        entry.atomic_status.store(ASTATUS_IO_WAIT, Ordering::SeqCst);
        assert!(!entry.try_wake());
        assert_eq!(entry.atomic_status.load(Ordering::SeqCst), ASTATUS_IO_WAIT);
    }

    #[test]
    fn try_wake_io_only_wakes_io_wait_not_waiting() {
        let proc = Process::new(crate::process::ProcessId::new(1));
        let entry = ProcessEntry::new(proc);

        // Start in ASTATUS_IO_WAIT
        entry.atomic_status.store(ASTATUS_IO_WAIT, Ordering::SeqCst);
        assert!(entry.try_wake_io());
        assert_eq!(entry.atomic_status.load(Ordering::SeqCst), ASTATUS_READY);

        // Now set to WAITING (receive wait) — try_wake_io should fail
        entry.atomic_status.store(ASTATUS_WAITING, Ordering::SeqCst);
        assert!(!entry.try_wake_io());
        assert_eq!(entry.atomic_status.load(Ordering::SeqCst), ASTATUS_WAITING);
    }

    #[test]
    fn try_wake_io_fails_when_already_ready() {
        let proc = Process::new(crate::process::ProcessId::new(1));
        let entry = ProcessEntry::new(proc);

        entry.atomic_status.store(ASTATUS_READY, Ordering::SeqCst);
        assert!(!entry.try_wake_io());
        assert_eq!(entry.atomic_status.load(Ordering::SeqCst), ASTATUS_READY);
    }

    #[test]
    fn no_double_wake_io_and_message() {
        // Simulates the race: message delivery and I/O completion both try
        // to wake the same IO_WAIT process. Only one should succeed.
        let proc = Process::new(crate::process::ProcessId::new(1));
        let entry = Arc::new(ProcessEntry::new(proc));
        entry.atomic_status.store(ASTATUS_IO_WAIT, Ordering::SeqCst);

        // Simulate exit signal delivery trying both paths:
        // try_wake() fails (IO_WAIT != WAITING), try_wake_io() succeeds
        let wake1 = entry.try_wake();
        let wake2 = entry.try_wake_io();
        assert!(!wake1);
        assert!(wake2);
        assert_eq!(entry.atomic_status.load(Ordering::SeqCst), ASTATUS_READY);

        // Now a second waker (I/O completion) tries — both fail
        let wake3 = entry.try_wake_io();
        let wake4 = entry.try_wake();
        assert!(!wake3);
        assert!(!wake4);
    }

    #[test]
    fn concurrent_try_wake_io_only_one_succeeds() {
        use std::sync::atomic::AtomicUsize;

        let proc = Process::new(crate::process::ProcessId::new(1));
        let entry = Arc::new(ProcessEntry::new(proc));
        entry.atomic_status.store(ASTATUS_IO_WAIT, Ordering::SeqCst);

        let success_count = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(std::sync::Barrier::new(4));

        let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::new();
        for _ in 0..4 {
            let e = Arc::clone(&entry);
            let cnt = Arc::clone(&success_count);
            let b = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                b.wait();
                if e.try_wake_io() {
                    cnt.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Exactly one thread should have succeeded
        assert_eq!(success_count.load(Ordering::SeqCst), 1);
        assert_eq!(entry.atomic_status.load(Ordering::SeqCst), ASTATUS_READY);
    }

    /// Regression: drain_inbox must skip ActiveReadReady for exited processes
    /// instead of calling handle_active_read_on_worker (which would waste I/O
    /// reading from the socket and writing to a dead heap).
    #[test]
    fn drain_inbox_skips_active_read_for_exited_process() {
        use crate::message::MessageFragment;
        use std::sync::atomic::AtomicBool;

        let mut proc = Process::new(crate::process::ProcessId::new(1));
        proc.transition_to(ProcessState::Exited);
        let entry = ProcessEntry::new(proc);

        let in_flight = Arc::new(AtomicBool::new(true));
        // Push an ActiveReadReady fragment with a bogus stream handle.
        // If drain_inbox does NOT skip it, handle_active_read_on_worker
        // will call get_opts() with an invalid handle and clear in_flight
        // via the early return path — but on a real system the handle
        // could still be valid (race window), so we must skip before
        // reaching the handler at all.
        entry.inbox.push(MessageFragment::ActiveReadReady {
            stream_handle: crate::socket::SocketHandle::try_from_raw((1 << 32) | 123).unwrap(), // absent socket
            in_flight: Arc::clone(&in_flight),
        });

        // Also push a normal message so we verify those are still drained.
        entry
            .inbox
            .push(MessageFragment::Immediate(crate::message::ImmediateTerm::new(ErlTerm::small_int(42))));

        let mut proc = entry.lock_for_runtime();
        proc.drain_inbox();

        // in_flight must have been cleared by the skip path.
        assert!(
            !in_flight.load(Ordering::SeqCst),
            "in_flight should be cleared when skipping ActiveReadReady for exited process"
        );

        // The normal message should still be delivered to the mailbox.
        assert!(
            !proc.mailbox.is_empty(),
            "normal messages should still be drained for exited processes"
        );
    }
    #[test]
    fn idle_collection_retains_live_state_and_reclaims_it_after_root_drop() {
        let mut proc = Process::new(crate::process::ProcessId::new(1));
        let mut list = ErlTerm::nil();
        for _ in 0..300_000 {
            list = proc.heap.alloc_cons(ErlTerm::small_int(65), list);
        }
        proc.dictionary.push((ErlTerm::small_int(1), list));
        // The first idle boundary promotes; subsequent waits do not move the
        // same live list again. No clock-dependent timing assertion is needed.
        unsafe { proc.collect_garbage_idle(); }
        let pointer = proc.dictionary[0].1;
        for i in 0..10 {
            proc.context.result = ErlTerm::small_int(i).0;
            unsafe { proc.collect_garbage_idle(); }
            assert_eq!(proc.dictionary[0].1, pointer);
        }
        proc.dictionary.clear();
        unsafe { proc.collect_garbage_idle(); }
        assert_eq!(proc.heap.bytes_allocated(), 0);
    }

}
