//! Event-driven I/O reactor using mio (epoll/kqueue).
//!
//! Replaces the blocking I/O thread pool for TCP operations. A single
//! reactor thread runs `mio::Poll`, dispatching readiness events to perform
//! non-blocking I/O and wake suspended Erlang processes.
//!
//! The reactor does NOT own socket objects — they live in the global socket
//! registry (`socket.rs`). Poll registration uses `SourceFd` to register
//! raw file descriptors obtained from the registry. This allows FFI threads
//! to attempt non-blocking I/O directly (fast path) while the reactor
//! handles the WouldBlock fallback.
//!
//! File I/O still uses the blocking thread pool (`io_pool.rs`) since regular
//! files do not support readiness polling on any OS.

use crate::socket::ActiveMode;
use std::collections::HashMap;

mod accept_waiters;
use accept_waiters::AcceptWaiters;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender};
use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token, Waker};

use crate::io_pool::wake_process_with_result;
use crate::process::{IoResult, ProcessId, ASTATUS_EXITED};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Token(0) is reserved for the Waker (command channel notification).
const WAKER_TOKEN: Token = Token(0);

/// Monotonic token counter shared between reactor and FFI threads.
/// Starts at 1 (0 is the Waker).
static NEXT_TOKEN: AtomicUsize = AtomicUsize::new(1);

/// Number of live reactor registrations (both pending ops and active-mode sockets).
/// Updated by the reactor thread on add/remove for lock-free `has_pending()`.
static PENDING_COUNT: AtomicUsize = AtomicUsize::new(0);

// -- Reactor diagnostic counters ------------------------------------------

/// Total readiness events dispatched (excluding waker events).
static COUNTER_EVENTS: AtomicUsize = AtomicUsize::new(0);
/// Events where the token was not found in the pending map (stale/spurious).
static COUNTER_MISSING_TOKEN: AtomicUsize = AtomicUsize::new(0);
/// Total active-mode reads dispatched to workers.
static COUNTER_ACTIVE_READS_DONE: AtomicUsize = AtomicUsize::new(0);
/// setopts fast-path hits: data was available immediately on the worker thread.
pub static COUNTER_SETOPTS_FASTPATH: AtomicUsize = AtomicUsize::new(0);

/// Reactor diagnostic snapshot.
#[derive(Debug, Clone)]
pub struct ReactorCounters {
    pub events: usize,
    pub missing_token: usize,
    pub active_reads_done: usize,
    pub setopts_fastpath: usize,
    pub pending: usize,
}

/// Read the current reactor diagnostic counters (lock-free).
pub fn reactor_counters() -> ReactorCounters {
    ReactorCounters {
        events: COUNTER_EVENTS.load(Ordering::Relaxed),
        missing_token: COUNTER_MISSING_TOKEN.load(Ordering::Relaxed),
        active_reads_done: COUNTER_ACTIVE_READS_DONE.load(Ordering::Relaxed),
        setopts_fastpath: COUNTER_SETOPTS_FASTPATH.load(Ordering::Relaxed),
        pending: PENDING_COUNT.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Pending operation descriptors
// ---------------------------------------------------------------------------

/// Describes what a process is waiting for on a socket.
pub enum PendingOp {
    /// Waiting for a new connection on a listener.
    Accept {
        pid: ProcessId,
        listener_handle: crate::socket::SocketHandle,
    },
    /// Waiting to read any available data from a stream.
    Recv {
        pid: ProcessId,
        stream_handle: crate::socket::SocketHandle,
    },
    /// Waiting to read exactly `target_len` bytes.
    RecvExact {
        pid: ProcessId,
        stream_handle: crate::socket::SocketHandle,
        target_len: usize,
        buf: Vec<u8>,
    },
    /// Waiting to read until a newline character.
    RecvLine {
        pid: ProcessId,
        stream_handle: crate::socket::SocketHandle,
        buf: Vec<u8>,
    },
    /// Waiting to write remaining bytes.
    Send {
        pid: ProcessId,
        stream_handle: crate::socket::SocketHandle,
        buf: Vec<u8>,
        offset: usize,
    },
}

impl PendingOp {
    /// The process ID that owns this pending operation.
    fn pid(&self) -> ProcessId {
        match self {
            PendingOp::Accept { pid, .. } => *pid,
            PendingOp::Recv { pid, .. } => *pid,
            PendingOp::RecvExact { pid, .. } => *pid,
            PendingOp::RecvLine { pid, .. } => *pid,
            PendingOp::Send { pid, .. } => *pid,
        }
    }

    /// The socket handle this operation is waiting on.
    fn socket_handle(&self) -> crate::socket::SocketHandle {
        match self {
            PendingOp::Accept { listener_handle, .. } => *listener_handle,
            PendingOp::Recv { stream_handle, .. } => *stream_handle,
            PendingOp::RecvExact { stream_handle, .. } => *stream_handle,
            PendingOp::RecvLine { stream_handle, .. } => *stream_handle,
            PendingOp::Send { stream_handle, .. } => *stream_handle,
        }
    }

    /// The mio Interest needed for this operation.
    fn interest(&self) -> Interest {
        match self {
            PendingOp::Accept { .. } => Interest::READABLE,
            PendingOp::Recv { .. } => Interest::READABLE,
            PendingOp::RecvExact { .. } => Interest::READABLE,
            PendingOp::RecvLine { .. } => Interest::READABLE,
            PendingOp::Send { .. } => Interest::WRITABLE,
        }
    }
}

// ---------------------------------------------------------------------------
// Commands sent from FFI threads to the reactor
// ---------------------------------------------------------------------------

pub enum ReactorCommand {
    /// Register a pending I/O operation for a process.
    Register {
        token: Token,
        op: PendingOp,
    },
    /// Deregister a socket from the poll (on close).
    /// The `socket` field carries the removed Socket so the reactor can
    /// extract the raw fd even after the registry entry is gone.
    /// `raw_fd` is a fallback extracted before registry removal — used when
    /// `Arc::try_unwrap` fails (other code holds a reference).
    Deregister {
        handle: crate::socket::SocketHandle,
        socket: Option<crate::socket::Socket>,
        raw_fd: Option<i32>,
    },
    /// Cancel all pending operations for a process (on exit).
    CancelForPid {
        pid: ProcessId,
    },
    /// Shut down the reactor thread.
    Shutdown,
    /// Register a socket for persistent active-mode readiness notifications.
    /// The reactor will push `ActiveReadReady` fragments to the worker — no
    /// actual read happens on the reactor thread.
    RegisterActive {
        stream_handle: crate::socket::SocketHandle,
        controlling_pid: ProcessId,
    },
    /// Deregister a socket from active-mode (called when socket goes passive
    /// or is closed).
    DeregisterActive {
        handle: crate::socket::SocketHandle,
    },
    /// Re-arm edge trigger for an active-mode socket after worker finishes
    /// reading. Sent by worker threads instead of using shared_registry
    /// to ensure reliable re-arm on the reactor's own poll.
    RearmActive {
        stream_handle: crate::socket::SocketHandle,
    },
}

/// Persistent active-mode registration. Kept across events — NOT removed
/// per readiness notification like `PendingOp`.
struct ActiveReg {
    controlling_pid: ProcessId,
    stream_handle: crate::socket::SocketHandle,
    /// Shared flag: reactor sets `true` before dispatch, worker clears after
    /// draining reads. Reactor skips dispatch when still `true`.
    in_flight: Arc<AtomicBool>,
}

// ---------------------------------------------------------------------------
// ReactorHandle — cloneable, send to the reactor from any thread
// ---------------------------------------------------------------------------

/// Tracks registration work from before enqueue until the reactor publishes
/// its live registration or finishes rejecting/discarding the command.
struct PendingRegistration(Arc<AtomicUsize>);

impl Drop for PendingRegistration {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

struct QueuedCommand {
    command: ReactorCommand,
    _registration: Option<PendingRegistration>,
}

/// A cheaply cloneable handle for communicating with the reactor thread.
#[derive(Clone)]
pub struct ReactorHandle {
    cmd_tx: Sender<QueuedCommand>,
    /// Shared by clones and queued/in-flight registration guards. Ordinary
    /// rearm/deregister traffic does not create a new future wake source.
    pending_registrations: Arc<AtomicUsize>,
    waker: Arc<Waker>,
    /// Coalescing flag: `true` means a wake-up has already been signalled
    /// and the reactor has not yet drained the command channel. Subsequent
    /// `send_command` calls observe the flag and skip the `waker.wake()`
    /// syscall — the pending wake will cause the reactor to drain all
    /// queued commands in one pass. Cleared by the reactor at the start
    /// of `drain_commands` (before draining) so any command pushed after
    /// that point reliably triggers a new wake.
    ///
    /// This is the dispatch-side half of Phase 5 workstream #1 — the
    /// 2026-04-14 park/unpark audit showed ~84% of worker wall time was
    /// in this single `Waker::wake` → `kevent(EVFILT_USER)` syscall due
    /// to kqueue lock contention across 8 workers hammering one shared
    /// kqueue at ~70k commands/sec.
    wake_pending: Arc<AtomicBool>,
}

impl ReactorHandle {
    /// Send a command to the reactor and wake it from poll() — coalescing
    /// back-to-back wakes via the `wake_pending` flag so that N commands
    /// pushed between reactor drains cost at most one `waker.wake()` syscall.
    pub fn send_command(&self, cmd: ReactorCommand) {
        let registration = if matches!(&cmd,
            ReactorCommand::Register { .. } | ReactorCommand::RegisterActive { .. }) {
            self.pending_registrations.fetch_add(1, Ordering::AcqRel);
            Some(PendingRegistration(Arc::clone(&self.pending_registrations)))
        } else {
            None
        };
        if self.cmd_tx.send(QueuedCommand { command: cmd, _registration: registration }).is_err() {
            // SendError drops the envelope and releases its guard as well.
            return;
        }
        // swap(true, Release) publishes the cmd_tx.send above before any
        // concurrent drain_commands observes wake_pending=true. Only the
        // worker that flipped the flag from false to true issues the
        // actual wake syscall; the rest ride for free.
        if !self.wake_pending.swap(true, Ordering::Release) {
            let _ = self.waker.wake();
        }
    }

    /// Allocate a unique Token and register a pending I/O operation.
    pub fn register_op(&self, op: PendingOp) {
        let token = Token(NEXT_TOKEN.fetch_add(1, Ordering::Relaxed));
        self.send_command(ReactorCommand::Register { token, op });
    }

    /// Cancel all pending operations for a process.
    pub fn cancel_for_pid(&self, pid: ProcessId) {
        self.send_command(ReactorCommand::CancelForPid { pid });
    }

    /// Includes registrations queued or being processed, not just live I/O.
    pub fn has_pending(&self) -> bool {
        // Read handoff ownership first: observing its release synchronizes
        // with publication of the live registration before the next load.
        self.pending_registrations.load(Ordering::Acquire) > 0
            || PENDING_COUNT.load(Ordering::Acquire) > 0
    }

    /// Shut down the reactor thread.
    pub fn shutdown(&self) {
        self.send_command(ReactorCommand::Shutdown);
    }
}

// ---------------------------------------------------------------------------
// IoReactor — the reactor state, owned by the reactor thread
// ---------------------------------------------------------------------------

struct IoReactor {
    poll: Poll,
    cmd_rx: Receiver<QueuedCommand>,
    shutdown: Arc<AtomicBool>,

    /// Shared with all `ReactorHandle` clones. See `ReactorHandle::wake_pending`
    /// for the coalescing protocol.
    wake_pending: Arc<AtomicBool>,

    /// Token → PendingOp mapping for active operations.
    pending: HashMap<Token, PendingOp>,

    /// Socket handle → Token. Tracks which handles are registered with poll
    /// and their current token. Used to decide register vs reregister.
    handle_to_token: HashMap<crate::socket::SocketHandle, Token>,

    /// Additional acceptors for each listener, in arrival order. The head
    /// lives in `pending`; all waiters share its one poll registration.
    accept_waiters: AcceptWaiters,

    /// Persistent active-mode registrations (NOT removed per event).
    /// Reactor dispatches readiness notifications; workers do actual reads.
    active_regs: HashMap<Token, ActiveReg>,

    /// Socket handle → Token for active registrations (lookup + dedup).
    active_handle_to_token: HashMap<crate::socket::SocketHandle, Token>,
}

impl IoReactor {
    /// The main reactor loop. Runs on a dedicated OS thread.
    ///
    /// **Wake-coalescing contract.** `wake_pending` is held `true` for the
    /// entire active phase of each iteration — draining, I/O event
    /// dispatch, and any cmds queued as a side effect of dispatch all fall
    /// inside the coalescing window, so concurrent worker `send_command`
    /// calls observe it and skip their `waker.wake()` syscall. The flag is
    /// cleared to `false` in exactly one place: right before we block in
    /// `poll.poll`. A `cmd_rx.is_empty()` re-check after the store
    /// catches workers that raced in between our last `try_recv` and the
    /// clear (saw `true`, skipped their wake, left the cmd in the channel).
    fn run(&mut self) {
        self.run_with_poll(|poll, events| {
            poll.poll(events, Some(std::time::Duration::from_millis(100)))
        });
    }

    // Keep the real command loop observable without a wall-clock assertion.
    fn run_with_poll(&mut self, mut poll: impl FnMut(&mut Poll, &mut Events) -> std::io::Result<()>) {
        let mut events = Events::with_capacity(256);
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                return;
            }

            // Drain queued commands and dispatch any I/O events from the
            // previous poll. Both phases may enqueue more commands as a
            // side effect (e.g. RearmActive after an active-read dispatch);
            // a second drain pass below catches those.
            self.drain_commands();
            if self.shutdown.load(Ordering::SeqCst) {
                return;
            }
            for event in events.iter() {
                if event.token() == WAKER_TOKEN {
                    // Already handled by drain_commands above; no-op.
                    continue;
                }
                self.handle_event(event.token());
            }
            self.drain_commands();
            if self.shutdown.load(Ordering::SeqCst) {
                return;
            }

            // About to block: publish wake_pending=false so the next worker
            // push triggers a fresh wake. Re-check the channel for racers
            // that pushed between our try_recv and the store.
            self.wake_pending.store(false, Ordering::SeqCst);
            if !self.cmd_rx.is_empty() {
                // A worker pushed after our last try_recv but before our
                // clear, and saw wake_pending=true so skipped its wake.
                // Reclaim the flag and drain another round.
                self.wake_pending.store(true, Ordering::SeqCst);
                continue;
            }

            // Poll for I/O events.
            match poll(&mut self.poll, &mut events) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    eprintln!("reactor: poll error: {}", e);
                    continue;
                }
            }
            // poll.poll returned. wake_pending may or may not already be
            // true at this point: if a worker's push woke us, it set the
            // flag. If the poll woke on an I/O event, the flag is still
            // false. Re-arm the coalescing regime for the next iteration.
            self.wake_pending.store(true, Ordering::SeqCst);
        }
    }

    /// Drain all queued commands. Does NOT touch `wake_pending` — the main
    /// `run` loop owns that flag, holding it `true` for the entire active
    /// phase of each iteration and only clearing it right before
    /// `poll.poll` blocks. This maximises the coalescing window so
    /// concurrent worker `send_command` calls all ride on a single wake.
    fn drain_commands(&mut self) {
        self.drain_commands_with(|_| {});
    }

    // The observation point lets tests freeze the actual processing handoff.
    // Production supplies a zero-capture no-op observer.
    fn drain_commands_with(&mut self, mut before_handle: impl FnMut(&Self)) {
        while let Ok(QueuedCommand { command: cmd, _registration }) = self.cmd_rx.try_recv() {
            // The guard survives dequeue and every handler, including an
            // early return or unwind. Drop only after the live state is set.
            before_handle(self);
            match cmd {
                ReactorCommand::Register { token, op } => {
                    self.register_pending(token, op);
                }
                ReactorCommand::Deregister { handle, socket, raw_fd } => {
                    self.deregister_handle(handle, socket, raw_fd);
                }
                ReactorCommand::CancelForPid { pid } => {
                    self.cancel_for_pid(pid);
                }
                ReactorCommand::RegisterActive { stream_handle, controlling_pid } => {
                    self.handle_register_active(stream_handle, controlling_pid);
                }
                ReactorCommand::DeregisterActive { handle } => {
                    self.handle_deregister_active(handle);
                }
                ReactorCommand::RearmActive { stream_handle } => {
                    if let Some(&token) = self.active_handle_to_token.get(&stream_handle) {
                        if let Some(fd) = crate::socket::get_raw_fd(stream_handle) {
                            let _ = self.poll.registry().reregister(
                                &mut SourceFd(&fd), token, Interest::READABLE,
                            );
                        }
                    }
                }
                ReactorCommand::Shutdown => {
                    let c = reactor_counters();
                    if c.events > 0 || c.setopts_fastpath > 0 {
                        eprintln!(
                            "reactor stats: events={} missing_token={} \
                             active_reads={} setopts_fastpath={} \
                             pending={} active_regs={}",
                            c.events, c.missing_token,
                            c.active_reads_done,
                            c.setopts_fastpath, c.pending,
                            self.active_regs.len(),
                        );
                    }
                    self.shutdown.store(true, Ordering::SeqCst);
                    return;
                }
            }
        }
    }

    fn register_pending(&mut self, token: Token, op: PendingOp) {
        let handle = op.socket_handle();
        let interest = op.interest();

        // Several processes may accept on the same listener. Keep the first
        // suspended process registered and queue the others behind it instead
        // of overwriting its token and leaving it in IO_WAIT forever.
        if let PendingOp::Accept { pid, .. } = &op {
            if let Some(token) = self.handle_to_token.get(&handle) {
                if matches!(self.pending.get(token), Some(PendingOp::Accept { .. })) {
                    self.accept_waiters.push(handle, *pid);
                    return;
                }
            }
        }

        // Other pending operations retain the one-op-per-handle policy.
        if let Some(&old_token) = self.handle_to_token.get(&handle) {
            if old_token != token
                && self.pending.remove(&old_token).is_some() {
                    PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);
                    self.accept_waiters.remove(handle);
                }
        }

        // If this handle has an active-mode registration, remove it.
        // The process is switching from active mode to passive (e.g., for
        // gen_tcp:recv during read_data_block). We must remove the active
        // registration so the new pending-op token gets the events.
        // Track whether the fd was already registered in mio (for register vs reregister).
        let had_active_reg = if let Some(active_token) = self.active_handle_to_token.remove(&handle) {
            if self.active_regs.remove(&active_token).is_some() {
                PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);
            }
            true
        } else {
            false
        };

        // Register/reregister the fd with poll via SourceFd.
        if let Some(fd) = crate::socket::get_raw_fd(handle) {
            let was_registered = self.handle_to_token.contains_key(&handle) || had_active_reg;
            let result = if was_registered {
                self.poll.registry().reregister(&mut SourceFd(&fd), token, interest)
            } else {
                self.poll.registry().register(&mut SourceFd(&fd), token, interest)
            };
            if let Err(e) = result {
                eprintln!("reactor: poll register error for handle {}: {}", handle, e);
                // Don't insert into pending — the fd isn't registered with
                // the poll, so we'd never get an event.  Wake the process
                // with an error so it doesn't hang in IO_WAIT forever.
                let pid = op.pid();
                let result = if matches!(&op, PendingOp::Accept { .. })
                    && crate::socket::get_listener(handle).is_err() {
                    IoResult::Closed
                } else {
                    IoResult::Err(io::Error::new(e.kind(), format!("reactor register: {}", e)))
                };
                wake_process_with_result(pid, result);
                return;
            }
        } else {
            // No fd available (socket already closed?). Don't register.
            let pid = op.pid();
            let result = if matches!(&op, PendingOp::Accept { .. }) {
                IoResult::Closed
            } else {
                IoResult::Err(io::Error::new(io::ErrorKind::NotFound, "socket fd not found"))
            };
            wake_process_with_result(pid, result);
            return;
        }

        self.handle_to_token.insert(handle, token);
        self.pending.insert(token, op);
        PENDING_COUNT.fetch_add(1, Ordering::Relaxed);

        // Eagerly attempt the I/O operation right after registration.
        // Data may have arrived between the worker's WouldBlock and this
        // registration, avoiding a full poll() round-trip.
        self.try_eager_dispatch(token);
    }

    /// Try to complete a pending operation immediately (non-blocking).
    /// If successful, removes the pending op and wakes the process.
    fn try_eager_dispatch(&mut self, token: Token) {
        let op_kind = match self.pending.get(&token) {
            Some(PendingOp::Accept { .. }) => 0,
            Some(PendingOp::Recv { .. }) => 1,
            Some(PendingOp::RecvExact { .. }) => 2,
            Some(PendingOp::RecvLine { .. }) => 3,
            Some(PendingOp::Send { .. }) => 4,
            _ => return,
        };
        match op_kind {
            0 => self.handle_accept(token),
            1 => self.handle_recv(token),
            2 => self.handle_recv_exact(token),
            3 => self.handle_recv_line(token),
            4 => self.handle_send(token),
            _ => {}
        }
    }

    fn deregister_handle(&mut self, handle: crate::socket::SocketHandle, socket: Option<crate::socket::Socket>, raw_fd: Option<i32>) {
        // Deregister from poll using the passed-in socket when available,
        // then raw_fd fallback (extracted before registry removal), then
        // the global registry as last resort.
        let fd = socket
            .as_ref()
            .map(crate::socket::Socket::raw_fd)
            .or(raw_fd)
            .or_else(|| crate::socket::get_raw_fd(handle));
        if let Some(fd) = fd {
            let _ = self.poll.registry().deregister(&mut SourceFd(&fd));
        }
        // Otherwise, the fd was already closed — OS auto-removes from poll.

        self.handle_to_token.remove(&handle);

        // Remove any pending ops for this handle.
        let to_remove: Vec<Token> = self
            .pending
            .iter()
            .filter(|(_, op)| op.socket_handle() == handle)
            .map(|(tok, _)| *tok)
            .collect();
        for tok in to_remove {
            let op = self.pending.remove(&tok).expect("pending socket operation");
            PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);
            if let PendingOp::Accept { pid, .. } = op {
                wake_process_with_result(pid, IoResult::Closed);
            }
        }

        // Every queued acceptor must complete as well as the registered head.
        // Removing bookkeeping alone strands its process in IO_WAIT forever.
        while let Some(pid) = self.accept_waiters.pop(handle) {
            wake_process_with_result(pid, IoResult::Closed);
        }

        // Also clean up any active-mode registration for this handle.
        if let Some(token) = self.active_handle_to_token.remove(&handle) {
            if self.active_regs.remove(&token).is_some() {
                PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    /// Lightweight deregister: remove from kqueue/epoll + clear handle_to_token.
    /// Does NOT scan pending ops.  Use after an op has already been removed
    /// from `self.pending` — this just stops spurious kqueue wakeups.
    fn deregister_fd(&mut self, handle: crate::socket::SocketHandle) {
        if let Some(fd) = crate::socket::get_raw_fd(handle) {
            let _ = self.poll.registry().deregister(&mut SourceFd(&fd));
        }
        self.handle_to_token.remove(&handle);
    }

    fn cancel_for_pid(&mut self, pid: ProcessId) {
        // A dead acceptor may be queued rather than the registered head.
        self.accept_waiters.cancel(pid);
        let to_remove: Vec<Token> = self
            .pending
            .iter()
            .filter(|(_, op)| op.pid() == pid)
            .map(|(tok, _)| *tok)
            .collect();
        for tok in to_remove {
            if let Some(op) = self.pending.remove(&tok) {
                PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);
                if let PendingOp::Accept { listener_handle, .. } = op {
                    if self.advance_accept(tok, listener_handle) {
                        // The fd might already be readable. Do not depend on
                        // another readiness edge to serve the surviving head.
                        self.handle_accept(tok);
                    }
                } else {
                    self.deregister_fd(op.socket_handle());
                }
            }
        }

        // Also clean up active-mode registrations for this pid.
        let active_to_remove: Vec<(Token, crate::socket::SocketHandle)> = self
            .active_regs
            .iter()
            .filter(|(_, reg)| reg.controlling_pid == pid)
            .map(|(&tok, reg)| (tok, reg.stream_handle))
            .collect();
        for (tok, handle) in active_to_remove {
            if self.active_regs.remove(&tok).is_some() {
                PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);
            }
            self.active_handle_to_token.remove(&handle);
            if let Some(fd) = crate::socket::get_raw_fd(handle) {
                let _ = self.poll.registry().deregister(&mut SourceFd(&fd));
            }
        }
    }

    fn handle_event(&mut self, token: Token) {
        COUNTER_EVENTS.fetch_add(1, Ordering::Relaxed);

        // 1. Check persistent active registrations first.
        if let Some(reg) = self.active_regs.get(&token) {
            let opts = crate::socket::get_opts(reg.stream_handle);

            // Socket removed — clean up the stale registration.
            if opts.is_none() {
                let handle = reg.stream_handle;
                self.deregister_active_by_token(token, handle);
                return;
            }

            let active_mode = opts.as_ref().map_or(ActiveMode::Passive, |o| o.active);

            if active_mode == ActiveMode::Passive {
                // Socket went passive — lazy deregister.  The registration
                // stays in active_regs but won't fire again until re-armed
                // (EV_CLEAR consumed the edge).  When the process calls
                // setopts, either the fastpath reads data directly or
                // RegisterActive re-registers the fd (EV_ADD re-checks
                // the buffer).
                return;
            }

            // ── {active, once} or {active, true}: dispatch to worker ──
            // The worker thread performs the actual socket read and delivers
            // the {tcp, Socket, Data} message.  For {active, once} the
            // worker sets active=0 after reading and does NOT re-arm
            // (RearmActive is skipped when active drops to 0).  For
            // {active, true} the worker re-arms after each read batch.
            //
            // Skip dispatch if the worker is still reading from a
            // previous notification.  Re-arm the edge so the event
            // isn't lost — the worker will re-peek after clearing
            // in_flight.
            if reg.in_flight.load(Ordering::Acquire) {
                if let Some(fd) = crate::socket::get_raw_fd(reg.stream_handle) {
                    let _ = self.poll.registry().reregister(
                        &mut SourceFd(&fd), token, Interest::READABLE,
                    );
                }
                return;
            }
            reg.in_flight.store(true, Ordering::Release);
            let fragment = crate::message::MessageFragment::ActiveReadReady {
                stream_handle: reg.stream_handle,
                in_flight: Arc::clone(&reg.in_flight),
            };
            self.deliver_fragment(reg.controlling_pid, fragment);
            COUNTER_ACTIVE_READS_DONE.fetch_add(1, Ordering::Relaxed);
            return;
        }

        // 2. Existing pending-op dispatch.
        let op_kind = match self.pending.get(&token) {
            Some(PendingOp::Accept { .. }) => 0,
            Some(PendingOp::Recv { .. }) => 1,
            Some(PendingOp::RecvExact { .. }) => 2,
            Some(PendingOp::RecvLine { .. }) => 3,
            Some(PendingOp::Send { .. }) => 4,
            None => {
                COUNTER_MISSING_TOKEN.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        match op_kind {
            0 => self.handle_accept(token),
            1 => self.handle_recv(token),
            2 => self.handle_recv_exact(token),
            3 => self.handle_recv_line(token),
            4 => self.handle_send(token),
            _ => {}
        }
    }

    // -- Event handlers ---------------------------------------------------

    /// Promote the next acceptor without changing the listener's poll token.
    /// Returns false only when no waiter remains and the fd is deregistered.
    fn advance_accept(&mut self, token: Token, listener_handle: crate::socket::SocketHandle) -> bool {
        let next = self.accept_waiters.pop(listener_handle);
        if let Some(pid) = next {
            self.pending.insert(token, PendingOp::Accept { pid, listener_handle });
            PENDING_COUNT.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            self.deregister_fd(listener_handle);
            false
        }
    }

    fn handle_accept(&mut self, token: Token) {
        // Drain ready connections for queued acceptors. On WouldBlock, leave
        // the head and its registration intact for the next readiness event.
        loop {
            let Some(PendingOp::Accept { pid, listener_handle }) = self.pending.remove(&token) else {
                return;
            };
            PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);
            let result = crate::socket::try_accept(listener_handle);
            match result {
                Ok(stream_handle) => {
                    crate::socket::set_controlling_pid(stream_handle, pid);
                    wake_process_with_result(pid, IoResult::OkHandle(stream_handle));
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    self.pending.insert(token, PendingOp::Accept { pid, listener_handle });
                    PENDING_COUNT.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                // Close can win before the queued deregistration is handled.
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    wake_process_with_result(pid, IoResult::Closed);
                }
                Err(e) => wake_process_with_result(pid, IoResult::Err(e)),
            }
            if !self.advance_accept(token, listener_handle) {
                return;
            }
        }
    }

    fn handle_recv(&mut self, token: Token) {
        let op = match self.pending.remove(&token) {
            Some(op) => op,
            None => return,
        };
        PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);

        if let PendingOp::Recv { pid, stream_handle } = op {
            match crate::socket::try_recv(stream_handle) {
                Ok(bytes) => {
                    self.deregister_fd(stream_handle);
                    wake_process_with_result(pid, IoResult::OkBytes(bytes));
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    self.pending.insert(token, PendingOp::Recv { pid, stream_handle });
                    PENDING_COUNT.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    self.deregister_fd(stream_handle);
                    wake_process_with_result(pid, IoResult::Err(e));
                }
            }
        }
    }

    fn handle_recv_exact(&mut self, token: Token) {
        let op = match self.pending.remove(&token) {
            Some(op) => op,
            None => return,
        };
        PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);

        if let PendingOp::RecvExact { pid, stream_handle, target_len, mut buf } = op {
            let stream = match crate::socket::get_stream(stream_handle) {
                Ok(s) => s,
                Err(e) => {
                    self.deregister_fd(stream_handle);
                    wake_process_with_result(pid, IoResult::Err(e));
                    return;
                }
            };

            use std::io::Read;
            let mut tmp = [0u8; 4096];
            loop {
                let remaining = target_len - buf.len();
                let read_len = remaining.min(tmp.len());
                match (&*stream).read(&mut tmp[..read_len]) {
                    Ok(0) => {
                        self.deregister_fd(stream_handle);
                        let e = io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed");
                        wake_process_with_result(pid, IoResult::Err(e));
                        return;
                    }
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.len() >= target_len {
                            buf.truncate(target_len);
                            self.deregister_fd(stream_handle);
                            wake_process_with_result(pid, IoResult::OkBytes(buf));
                            return;
                        }
                        // Continue draining until WouldBlock or enough data.
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        // Partial accumulation — re-insert and wait for more.
                        self.pending.insert(token, PendingOp::RecvExact {
                            pid, stream_handle, target_len, buf,
                        });
                        PENDING_COUNT.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    Err(e) => {
                        self.deregister_fd(stream_handle);
                        wake_process_with_result(pid, IoResult::Err(e));
                        return;
                    }
                }
            }
        }
    }

    fn handle_recv_line(&mut self, token: Token) {
        let op = match self.pending.remove(&token) {
            Some(op) => op,
            None => return,
        };
        PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);

        if let PendingOp::RecvLine { pid, stream_handle, mut buf } = op {
            // Hold Arc<TcpStream> alive so the fd cannot be closed/reused
            // while we operate on the raw fd in the hot loop.
            let stream = match crate::socket::get_stream(stream_handle) {
                Ok(s) => s,
                Err(e) => {
                    self.deregister_fd(stream_handle);
                    wake_process_with_result(pid, IoResult::Err(e));
                    return;
                }
            };
            let fd = {
                use std::os::unix::io::AsRawFd;
                stream.as_raw_fd()
            };

            // Peek + consume_exact using cached fd: peek to find newline,
            // then consume_exact to guarantee all bytes are removed from kernel.
            const MAX_LINE: usize = 300_000;
            let mut peek_buf = [0u8; 4096];
            loop {
                match crate::socket::peek_recv_fd(fd, &mut peek_buf) {
                    Ok(0) => {
                        self.deregister_fd(stream_handle);
                        if buf.is_empty() {
                            let e = io::Error::new(io::ErrorKind::ConnectionAborted, "closed");
                            wake_process_with_result(pid, IoResult::Err(e));
                        } else {
                            wake_process_with_result(pid, IoResult::OkBytes(buf));
                        }
                        return;
                    }
                    Ok(n) => {
                        if let Some(pos) = peek_buf[..n].iter().position(|&b| b == b'\n') {
                            // Found newline — consume_exact pos+1 bytes from kernel.
                            match crate::socket::consume_exact_fd(fd, &mut peek_buf[..pos + 1]) {
                                Ok(()) => {
                                    buf.extend_from_slice(&peek_buf[..pos]);
                                    if buf.last() == Some(&b'\r') {
                                        buf.pop();
                                    }
                                    self.deregister_fd(stream_handle);
                                    wake_process_with_result(pid, IoResult::OkBytes(buf));
                                }
                                Err(e) => {
                                    self.deregister_fd(stream_handle);
                                    wake_process_with_result(pid, IoResult::Err(e));
                                }
                            }
                            return;
                        }
                        // No newline — consume_exact all peeked bytes, append to buf.
                        match crate::socket::consume_exact_fd(fd, &mut peek_buf[..n]) {
                            Ok(()) => {
                                buf.extend_from_slice(&peek_buf[..n]);
                            }
                            Err(e) => {
                                self.deregister_fd(stream_handle);
                                wake_process_with_result(pid, IoResult::Err(e));
                                return;
                            }
                        }
                        if buf.len() >= MAX_LINE {
                            self.deregister_fd(stream_handle);
                            wake_process_with_result(pid, IoResult::OkBytes(buf));
                            return;
                        }
                        // Continue looping — more data might be available.
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        self.pending.insert(token, PendingOp::RecvLine {
                            pid, stream_handle, buf,
                        });
                        PENDING_COUNT.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    Err(e) => {
                        self.deregister_fd(stream_handle);
                        wake_process_with_result(pid, IoResult::Err(e));
                        return;
                    }
                }
            }
        }
    }

    fn handle_send(&mut self, token: Token) {
        let op = match self.pending.remove(&token) {
            Some(op) => op,
            None => return,
        };
        PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);

        if let PendingOp::Send { pid, stream_handle, buf, mut offset } = op {
            let stream = match crate::socket::get_stream(stream_handle) {
                Ok(s) => s,
                Err(e) => {
                    self.deregister_fd(stream_handle);
                    wake_process_with_result(pid, IoResult::Err(e));
                    return;
                }
            };

            use std::io::Write;
            loop {
                match (&*stream).write(&buf[offset..]) {
                    Ok(0) => {
                        self.deregister_fd(stream_handle);
                        let e = io::Error::new(io::ErrorKind::WriteZero, "write returned 0");
                        wake_process_with_result(pid, IoResult::Err(e));
                        return;
                    }
                    Ok(n) => {
                        offset += n;
                        if offset >= buf.len() {
                            self.deregister_fd(stream_handle);
                            wake_process_with_result(pid, IoResult::Ok);
                            return;
                        }
                        // Partial write — continue draining.
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        self.pending.insert(token, PendingOp::Send {
                            pid, stream_handle, buf, offset,
                        });
                        PENDING_COUNT.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    Err(e) => {
                        self.deregister_fd(stream_handle);
                        wake_process_with_result(pid, IoResult::Err(e));
                        return;
                    }
                }
            }
        }
    }

    // -- Persistent active-mode registration/deregistration -----------------

    fn handle_register_active(&mut self, stream_handle: crate::socket::SocketHandle, controlling_pid: ProcessId) {
        // If already registered under the same handle, reuse the existing
        // token — just reregister to re-arm the kqueue/epoll edge trigger.
        if let Some(&existing_token) = self.active_handle_to_token.get(&stream_handle) {
            // Update controlling_pid in case it changed.
            if let Some(reg) = self.active_regs.get_mut(&existing_token) {
                reg.controlling_pid = controlling_pid;
            }
            if let Some(fd) = crate::socket::get_raw_fd(stream_handle) {
                let _ = self.poll.registry().reregister(
                    &mut SourceFd(&fd), existing_token, Interest::READABLE,
                );
            }
        } else {
            // New registration — allocate a unique token.
            let token = Token(NEXT_TOKEN.fetch_add(1, Ordering::Relaxed));
            let in_flight = Arc::new(AtomicBool::new(false));

            if let Some(fd) = crate::socket::get_raw_fd(stream_handle) {
                // Check if this handle already has a pending-op registration.
                let was_registered = self.handle_to_token.contains_key(&stream_handle);
                let result = if was_registered {
                    self.poll.registry().reregister(
                        &mut SourceFd(&fd), token, Interest::READABLE,
                    )
                } else {
                    self.poll.registry().register(
                        &mut SourceFd(&fd), token, Interest::READABLE,
                    )
                };
                if let Err(e) = result {
                    eprintln!("reactor: active register error for handle {}: {}", stream_handle, e);
                    return;
                }
            } else {
                return; // socket gone
            }

            self.active_regs.insert(token, ActiveReg {
                controlling_pid,
                stream_handle,
                in_flight,
            });
            self.active_handle_to_token.insert(stream_handle, token);
            PENDING_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn handle_deregister_active(&mut self, handle: crate::socket::SocketHandle) {
        if let Some(token) = self.active_handle_to_token.remove(&handle) {
            if self.active_regs.remove(&token).is_some() {
                PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);
            }
            // Deregister from epoll/kqueue.
            if let Some(fd) = crate::socket::get_raw_fd(handle) {
                let _ = self.poll.registry().deregister(&mut SourceFd(&fd));
            }
        }
    }

    fn deregister_active_by_token(&mut self, token: Token, handle: crate::socket::SocketHandle) {
        if self.active_regs.remove(&token).is_some() {
            PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);
        }
        self.active_handle_to_token.remove(&handle);
        if let Some(fd) = crate::socket::get_raw_fd(handle) {
            let _ = self.poll.registry().deregister(&mut SourceFd(&fd));
        }
    }

    // -- Fragment delivery to process inbox ---------------------------------

    fn deliver_fragment(&self, dest_pid: ProcessId, fragment: crate::message::MessageFragment) {
        if let Some(sched) = crate::scheduler::global_scheduler() {
            let entry = sched.process_table.get(&dest_pid).map(|r| r.clone());
            if let Some(entry) = entry {
                // Best-effort skip for dead processes (TOCTOU: process may
                // die between this check and the inbox push — the
                // authoritative guard in drain_inbox catches that case).
                if entry.published_status() == ASTATUS_EXITED {
                    if let crate::message::MessageFragment::ActiveReadReady {
                        ref in_flight, ..
                    } = fragment
                    {
                        in_flight.store(false, std::sync::atomic::Ordering::Release);
                    }
                    return;
                }
                entry.inbox.push(fragment);
                if entry.try_wake() {
                    sched.runnable_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    sched.timer_wheel.cancel_for_pid(dest_pid);
                    // Phase 8: dispatch directly to the process's home
                    // worker's inbox and unpark that worker, instead of
                    // pushing to the global injector + broadcast notify.
                    sched.dispatch_to_worker(entry);
                }
                // If try_wake failed: the process is in READY/RUNNING and will
                // drain the lock-free inbox on its next scheduler pickup, or
                // the STATUS_RECEIVE_WAIT / STATUS_IO_WAIT / waiting-persist
                // race checks in the scheduler will catch the stranded
                // fragment.
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Reactor thread spawn / global accessor
// ---------------------------------------------------------------------------

/// Global reactor handle, set during scheduler startup.
static GLOBAL_REACTOR: std::sync::OnceLock<ReactorHandle> = std::sync::OnceLock::new();

/// Get the global reactor handle. Returns None if not yet initialized or
/// if the platform does not support mio.
pub fn global_reactor() -> Option<&'static ReactorHandle> {
    GLOBAL_REACTOR.get()
}

/// Spawn the reactor thread. Returns (ReactorHandle, JoinHandle).
pub fn spawn_reactor() -> (ReactorHandle, JoinHandle<()>) {
    let poll = Poll::new().expect("failed to create mio::Poll");
    let waker = Arc::new(Waker::new(poll.registry(), WAKER_TOKEN).expect("failed to create Waker"));

    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let wake_pending = Arc::new(AtomicBool::new(false));

    let handle = ReactorHandle {
        cmd_tx,
        pending_registrations: Arc::new(AtomicUsize::new(0)),
        waker: Arc::clone(&waker),
        wake_pending: Arc::clone(&wake_pending),
    };

    // Store the global reference.
    let _ = GLOBAL_REACTOR.set(handle.clone());

    let join_handle = thread::Builder::new()
        .name("nera-reactor".into())
        .spawn(move || {
            let mut reactor = IoReactor {
                poll,
                cmd_rx,
                shutdown: Arc::new(AtomicBool::new(false)),
                wake_pending,
                pending: HashMap::new(),
                handle_to_token: HashMap::new(),
                accept_waiters: AcceptWaiters::default(),
                active_regs: HashMap::new(),
                active_handle_to_token: HashMap::new(),
            };
            reactor.run();
        })
        .expect("failed to spawn reactor thread");

    (handle, join_handle)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Each scenario owns the socket registry and diagnostic counters. No
    // reactor thread is started: command handoffs are advanced explicitly.
    fn isolate_active_registration(name: &str) -> bool {
        const CHILD: &str = "NERA_TEST_ACTIVE_REGISTRATION";
        if std::env::var(CHILD).ok().as_deref() == Some(name) {
            return false;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &format!("reactor::tests::{name}"), "--nocapture"])
            .env(CHILD, name).output().unwrap();
        assert!(output.status.success(), "{}\n{}",
            String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        true
    }

    fn active_registration_fixture() -> (ReactorHandle, IoReactor) {
        let poll = Poll::new().unwrap();
        let waker = Arc::new(Waker::new(poll.registry(), WAKER_TOKEN).unwrap());
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let wake_pending = Arc::new(AtomicBool::new(false));
        let handle = ReactorHandle {
            cmd_tx, waker, wake_pending: Arc::clone(&wake_pending),
            pending_registrations: Arc::new(AtomicUsize::new(0)),
        };
        let reactor = IoReactor {
            poll, cmd_rx, shutdown: Arc::new(AtomicBool::new(false)), wake_pending,
            pending: HashMap::new(), handle_to_token: HashMap::new(),
            accept_waiters: AcceptWaiters::default(), active_regs: HashMap::new(),
            active_handle_to_token: HashMap::new(),
        };
        assert!(!handle.has_pending());
        (handle, reactor)
    }

    fn queue_active(handle: &ReactorHandle, socket: crate::socket::SocketHandle) {
        handle.send_command(ReactorCommand::RegisterActive {
            stream_handle: socket, controlling_pid: ProcessId::new(1),
        });
    }

    #[test]
    fn active_registration_is_pending_until_published() {
        if isolate_active_registration("active_registration_is_pending_until_published") { return; }
        let (handle, mut reactor) = active_registration_fixture();
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let socket = crate::socket::connect("127.0.0.1", listener.local_addr().unwrap().port()).unwrap();
        let (_peer, _) = listener.accept().unwrap();
        queue_active(&handle, socket);
        assert!(handle.has_pending(), "queued active registration must prevent termination");
        let mut observed = false;
        reactor.drain_commands_with(|r| {
            assert!(r.cmd_rx.is_empty());
            assert!(r.active_regs.is_empty());
            assert_eq!(PENDING_COUNT.load(Ordering::Acquire), 0);
            assert!(handle.has_pending(), "dequeued registration must stay pending during processing");
            observed = true;
        });
        assert!(observed, "observe the actual drain loop before handler publication");
        assert_eq!(reactor.active_regs.len(), 1);
        assert!(handle.has_pending(), "live registration must take over pending ownership");
        // Re-registering the same socket must not leave a second live entry.
        queue_active(&handle, socket);
        reactor.drain_commands();
        assert_eq!(reactor.active_regs.len(), 1);
        reactor.handle_deregister_active(socket);
        assert!(!handle.has_pending(), "deregistration must release all registration work");
        crate::socket::remove(socket);
    }

    #[test]
    fn active_registration_failures_release_pending_work() {
        if isolate_active_registration("active_registration_failures_release_pending_work") { return; }
        let (handle, mut reactor) = active_registration_fixture();
        let gone = crate::socket::SocketHandle::try_from_raw((1 << 32) | 123).unwrap();
        queue_active(&handle, gone);
        assert!(handle.has_pending(), "failed registration is pending until handled");
        reactor.drain_commands();
        assert!(reactor.active_regs.is_empty());
        assert!(!handle.has_pending(), "missing socket must not retain pending work");
        handle.register_op(PendingOp::Recv { pid: ProcessId::new(1), stream_handle: gone });
        assert!(handle.has_pending(), "ordinary registrations share the same queue handoff");
        drop(reactor.cmd_rx.try_recv().unwrap());
        assert!(!handle.has_pending());
        handle.cancel_for_pid(ProcessId::new(1));
        assert!(!handle.has_pending(), "cleanup alone does not create a wake source");
        reactor.drain_commands();
        queue_active(&handle, gone);
        let command = reactor.cmd_rx.try_recv().unwrap();
        assert!(handle.has_pending());
        drop(command); // discarded before processing
        assert!(!handle.has_pending());
        drop(reactor); // failed channel send must also release ownership
        for _ in 0..100 {
            queue_active(&handle, gone);
            assert!(!handle.has_pending(), "disconnected receiver must not leak pending work");
        }
    }

    #[test]
    fn active_registration_clones_keep_each_command_alive() {
        if isolate_active_registration("active_registration_clones_keep_each_command_alive") { return; }
        let (handle, mut reactor) = active_registration_fixture();
        let other = handle.clone();
        let gone = crate::socket::SocketHandle::try_from_raw((1 << 32) | 123).unwrap();
        queue_active(&handle, gone);
        queue_active(&other, gone);
        let first = reactor.cmd_rx.try_recv().unwrap();
        let second = reactor.cmd_rx.try_recv().unwrap();
        assert!(handle.has_pending(), "clones must share queued registration ownership");
        drop(first);
        assert!(other.has_pending(), "dropping one command must not release another");
        drop(second);
        assert!(!handle.has_pending());
        handle.shutdown();
        queue_active(&other, gone);
        reactor.drain_commands(); // returns on Shutdown, leaving registration queued
        assert!(handle.has_pending());
        drop(reactor);
        assert!(!other.has_pending(), "receiver shutdown must release undrained commands");
    }

    // NERA-42: neither a queued nor a wake-delivered Shutdown may poll again.
    #[test]
    fn shutdown_never_repolls() {
        for queued in [true, false] {
            let (handle, mut reactor) = active_registration_fixture();
            if queued { handle.shutdown(); }
            let mut polls = 0;
            reactor.run_with_poll(|_, _| {
                polls += 1;
                assert!(!queued && polls == 1, "poll after Shutdown");
                handle.shutdown();
                Ok(())
            });
            assert_eq!(polls, usize::from(!queued));
        }
    }

    #[test]
    fn reactor_start_and_shutdown() {
        let (handle, join) = spawn_reactor();
        assert!(!handle.has_pending());
        handle.shutdown();
        join.join().expect("reactor thread panicked");
    }

    // Scheduler tests shut down the global socket registry. Run this real-I/O
    // scenario in its own process so their cleanup cannot close our listener.
    #[test]
    fn concurrent_accepts_preserve_waiters() {
        const CHILD: &str = "NERA_TEST_CONCURRENT_ACCEPTS";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "reactor::tests::concurrent_accepts_preserve_waiters", "--nocapture"])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(output.status.success(), "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr));
            return;
        }

        let (_tx, cmd_rx) = crossbeam_channel::unbounded();
        let mut reactor = IoReactor {
            poll: Poll::new().unwrap(),
            cmd_rx,
            shutdown: Arc::new(AtomicBool::new(false)),
            wake_pending: Arc::new(AtomicBool::new(false)),
            pending: HashMap::new(),
            handle_to_token: HashMap::new(),
            accept_waiters: AcceptWaiters::default(),
            active_regs: HashMap::new(),
            active_handle_to_token: HashMap::new(),
        };
        let listener = crate::socket::listen(0).unwrap();
        let port = crate::socket::local_port(listener).unwrap();
        let first = Token(NEXT_TOKEN.fetch_add(1, Ordering::Relaxed));
        let second = Token(NEXT_TOKEN.fetch_add(1, Ordering::Relaxed));
        reactor.register_pending(first, PendingOp::Accept { pid: crate::process::ProcessId::new(41), listener_handle: listener });
        reactor.register_pending(second, PendingOp::Accept { pid: crate::process::ProcessId::new(42), listener_handle: listener });
        // No client exists yet: both requests must remain pending, in order.
        assert!(matches!(reactor.pending.get(&first), Some(PendingOp::Accept { pid, .. }) if *pid == ProcessId::new(41)),
            "a second accept must not discard the first suspended process");

        let _client1 = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        drive_until(&mut reactor, |r| !matches!(r.pending.get(&first), Some(PendingOp::Accept { pid, .. }) if *pid == ProcessId::new(41)));
        assert!(matches!(reactor.pending.get(&first), Some(PendingOp::Accept { pid, .. }) if *pid == ProcessId::new(42)),
            "accepting one connection must leave the next waiter registered");
        let _client2 = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        drive_until(&mut reactor, |r| r.pending.is_empty());
        assert!(!reactor.handle_to_token.contains_key(&listener));
        assert_eq!(PENDING_COUNT.load(Ordering::Relaxed), 0);

        // Exercise a large accept queue without keeping thousands of OS fds
        // open: connect and release one peer at a time, checking FIFO service.
        let pids: Vec<ProcessId> = (10_000..12_048).map(ProcessId::new).collect();
        let token = enqueue_acceptors(&mut reactor, listener, &pids);
        reactor.cancel_for_pid(pids[1024]); // queued waiter
        reactor.cancel_for_pid(pids[0]); // registered head
        for &pid in &pids[1..] {
            if pid == pids[1024] {
                continue;
            }
            assert_eq!(reactor.pending.get(&token).map(PendingOp::pid), Some(pid));
            let _client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            drive_until(&mut reactor, |r| r.pending.get(&token).is_none_or(|op| op.pid() != pid));
            crate::socket::remove_all_for_pid(pid);
        }
        assert!(reactor.pending.is_empty());
        assert!(reactor.accept_waiters.is_empty());
        assert_eq!(PENDING_COUNT.load(Ordering::Relaxed), 0);

        // Cancelling a head must also serve a connection already ready for
        // the next waiter, without needing a new edge on the listener.
        enqueue_acceptors(&mut reactor, listener, &[crate::process::ProcessId::new(51), crate::process::ProcessId::new(52)]);
        let _client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        reactor.cancel_for_pid(crate::process::ProcessId::new(51));
        drive_until(&mut reactor, |r| r.pending.is_empty());
        assert!(reactor.accept_waiters.is_empty());
        assert_eq!(PENDING_COUNT.load(Ordering::Relaxed), 0);

        // Closing the listener must discard the entire queue, not retain
        // stale waiters to be revived by a future registration.
        enqueue_acceptors(&mut reactor, listener, &[crate::process::ProcessId::new(61), crate::process::ProcessId::new(62), crate::process::ProcessId::new(63)]);
        reactor.deregister_handle(listener, None, crate::socket::get_raw_fd(listener));
        assert!(reactor.pending.is_empty());
        assert!(reactor.accept_waiters.is_empty());
        assert!(!reactor.handle_to_token.contains_key(&listener));
        assert_eq!(PENDING_COUNT.load(Ordering::Relaxed), 0);
        crate::socket::shutdown_all();
    }

    /// Opt-in release measurement, without timing assertions in normal CI:
    /// cargo test -p nera-runtime --release accept_cancellation_scaling -- --ignored --nocapture
    /// Synthetic pending state avoids OS fd limits and measures the real
    /// cancellation method, excluding setup, scheduling and socket syscalls.
    #[test]
    #[ignore = "release-only cancellation scaling measurement"]
    fn accept_cancellation_scaling() {
        use std::time::Instant;

        fn fixture(waiters: usize, listeners: usize) -> IoReactor {
            let (_tx, cmd_rx) = crossbeam_channel::unbounded();
            let mut reactor = IoReactor {
                poll: Poll::new().unwrap(),
                cmd_rx,
                shutdown: Arc::new(AtomicBool::new(false)),
                wake_pending: Arc::new(AtomicBool::new(false)),
                pending: HashMap::new(),
                handle_to_token: HashMap::new(),
                accept_waiters: AcceptWaiters::default(),
                active_regs: HashMap::new(),
                active_handle_to_token: HashMap::new(),
            };
            for listener in 0..listeners {
                let handle = crate::socket::SocketHandle::try_from_raw((1 << 32) | listener as u64).unwrap();
                let token = Token(listener + 1);
                // Heads are never cancelled: no socket access or changes to
                // the global PENDING_COUNT are needed by this measurement.
                reactor.pending.insert(token, PendingOp::Accept {
                    pid: ProcessId::new(ProcessId::MAX - handle.as_raw()),
                    listener_handle: handle,
                });
                reactor.handle_to_token.insert(handle, token);
            }
            for pid in 1..=waiters {
                reactor.accept_waiters.push(crate::socket::SocketHandle::try_from_raw((1 << 32) | (pid % listeners) as u64).unwrap(), crate::process::ProcessId::new(pid as u64));
            }
            reactor
        }

        println!("waiters,listeners,scenario,cancellations,median_us,p95_us");
        for waiters in [128, 2048, 8192, 32768] {
            for listeners in [1, 64] {
                for scenario in ["absent", "middle", "burst"] {
                    let samples = if scenario == "burst" { 7 } else { 31 };
                    let mut elapsed = Vec::new();
                    for sample in 0..=samples {
                        let mut reactor = fixture(waiters, listeners);
                        let started = Instant::now();
                        match scenario {
                            "absent" => reactor.cancel_for_pid(crate::process::ProcessId::new(0)),
                            "middle" => reactor.cancel_for_pid(crate::process::ProcessId::new((waiters / 2) as u64)),
                            _ => {
                                // Odd stride permutes every PID exactly once
                                // for these power-of-two queue sizes.
                                for i in 0..waiters {
                                    let pid = (i * (waiters / 2 + 1)) % waiters + 1;
                                    reactor.cancel_for_pid(std::hint::black_box(crate::process::ProcessId::new(pid as u64)));
                                }
                            }
                        }
                        let duration = started.elapsed();
                        std::hint::black_box(&reactor);
                        let remaining: usize = reactor.accept_waiters.len();
                        let expected = match scenario {
                            "absent" => waiters,
                            "middle" => waiters - 1,
                            _ => 0,
                        };
                        assert_eq!(remaining, expected);
                        assert_eq!(reactor.pending.len(), listeners);
                        if scenario == "middle" {
                            assert!(!reactor.accept_waiters.contains(crate::process::ProcessId::new((waiters / 2) as u64)));
                        }
                        if sample != 0 { elapsed.push(duration); }
                    }
                    elapsed.sort_unstable();
                    let median = elapsed[elapsed.len() / 2].as_secs_f64() * 1e6;
                    let p95 = elapsed[(elapsed.len() * 95).div_ceil(100) - 1].as_secs_f64() * 1e6;
                    let cancellations = if scenario == "burst" { waiters } else { 1 };
                    println!("{waiters},{listeners},{scenario},{cancellations},{median:.3},{p95:.3}");
                }
            }
        }
    }

    fn enqueue_acceptors(reactor: &mut IoReactor, listener: crate::socket::SocketHandle, pids: &[ProcessId]) -> Token {
        let first = Token(NEXT_TOKEN.fetch_add(1, Ordering::Relaxed));
        for (i, &pid) in pids.iter().enumerate() {
            let token = if i == 0 { first } else { Token(NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)) };
            reactor.register_pending(token, PendingOp::Accept { pid, listener_handle: listener });
        }
        first
    }

    fn drive_until(reactor: &mut IoReactor, done: impl Fn(&IoReactor) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut events = Events::with_capacity(16);
        while !done(reactor) {
            assert!(std::time::Instant::now() < deadline, "pending accept was never completed");
            reactor.poll.poll(&mut events, Some(std::time::Duration::from_millis(10))).unwrap();
            for event in events.iter() {
                reactor.handle_event(event.token());
            }
        }
    }

    #[test]
    fn token_allocation_is_monotonic() {
        let a = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        let b = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        assert!(b > a);
    }
}
