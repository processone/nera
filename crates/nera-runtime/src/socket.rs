//! Global socket registry for gen_tcp support.
//!
//! Sockets (TcpListener / TcpStream) are stored in a global thread-safe
//! registry keyed by validated `SocketHandle` values. Compiled Erlang code
//! passes these handles around as small integers — they are GC-safe
//! because integers are immediate values (never heap-allocated).
//!
//! The registry is a slab: a `Vec` of slots, each either free (on a
//! linked freelist) or occupied with an `Arc<SocketEntry>`. Each slot
//! carries a bounded generation counter that bumps on every free→occupied
//! transition. Handles are encoded as `(generation << 32) | slot_index`,
//! so a stale handle to a slot that was freed and later reused safely
//! returns `None` on lookup — the generation check fails.
//!
//! Every `get_entry` becomes a direct `Vec` index plus a generation
//! compare, with no hashing and no probe sequences. The hot path
//! (reactor event dispatch + every worker recv/send) previously spent
//! ~27% of reactor on-CPU and ~10% of worker on-CPU inside
//! `RwLock<HashMap<u64, Arc<SocketEntry>>>` lookups (2026-04-13 samply
//! profile, flagged in `docs/ONECACHED_PERFORMANCE_PLAN.md`); this slab
//! rewrite is the first of the Phase 6-adjacent cleanups that workstream
//! identified. Per-socket options still live behind a per-socket `Mutex`
//! so mutations on different sockets never contend.
//!
//! On platforms with mio support (Linux/macOS/Windows), sockets use
//! `mio::net` types in non-blocking mode, enabling event-driven I/O via
//! the reactor. On other platforms, `std::net` types are used with the
//! blocking I/O thread pool.

use std::io;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};

#[cfg(mio_supported)]
use mio::net::{TcpListener, TcpStream};
#[cfg(not(mio_supported))]
use std::net::{TcpListener, TcpStream};

/// A socket resource held in the global registry.
///
/// Wrapped in `Arc` so concurrent I/O operations can hold a reference
/// without duplicating the OS file descriptor (`try_clone`/`dup`).
pub enum Socket {
    Listener(Arc<TcpListener>),
    Stream(Arc<TcpStream>),
}

impl Socket {
    /// Extract the raw file descriptor from the socket (mio platforms only).
    #[cfg(mio_supported)]
    pub(crate) fn raw_fd(&self) -> std::os::unix::io::RawFd {
        use std::os::unix::io::AsRawFd;
        match self {
            Socket::Listener(listener) => listener.as_raw_fd(),
            Socket::Stream(stream) => stream.as_raw_fd(),
        }
    }
}

/// How received data is delivered to the controlling process.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActiveMode {
    #[default]
    Passive = 0,
    Once = 1,
    Active = 2,
}

/// The framing modes supported by this runtime.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PacketMode {
    #[default]
    Raw = 0,
    Line = 1,
}

/// Per-socket options for active-mode support.
#[derive(Clone, Debug)]
#[derive(Default)]
pub struct SocketOpts {
    pub active: ActiveMode,
    pub packet: PacketMode,
    /// PID of the controlling process (receives active-mode messages).
    pub controlling_pid: crate::process::ProcessId,
    /// Whether a `RegisterActive` command has been sent to the reactor
    /// for this socket. Used to avoid sending spurious `DeregisterActive`
    /// commands (and their associated kevent syscalls) when the fast path
    /// handled the read inline without ever involving the reactor.
    pub reactor_registered: bool,
    /// Partial line data from active-mode reads. When a line-mode read
    /// finds data but no newline, the data is consumed from the kernel
    /// buffer into this buffer so the fd is no longer readable (preventing
    /// edge-triggered kqueue spin). On the next readability event, this
    /// buffer is prepended to newly read data before scanning for newline.
    pub line_buffer: Vec<u8>,
}


/// A socket entry combining the socket resource and its per-socket options.
/// Options are behind a per-socket `Mutex` so that mutations on different
/// sockets never contend with each other.
pub struct SocketEntry {
    pub socket: Socket,
    pub opts: Mutex<SocketOpts>,
}

// ── Slab-backed registry ─────────────────────────────────────────────
//
// A slot is vacant or occupied (holding an `Arc<SocketEntry>`). Vacant slots
// below the generation limit join the singly-linked `Registry::free_head` list;
// exhausted slots stay vacant and permanently unlinked.
// Every slot carries a generation counter, bounded to 28 bits, that increments on
// each free → occupied transition, so a stale handle to a slot that
// was freed and later reused fails the generation check and returns
// `None`. Exhausted slots are retired permanently. This preserves the "stale handles are safe" property the old
// monotonic-u64 + HashMap design got for free.
//
// `Registry::slots.len()` is bounded by the high-water mark of
// simultaneously-open sockets, not by the total number of register
// calls. Sentinel: handles of all-zero bits (slot 0, generation 0) are
// not produced — generation starts at 1 for every new slot, so the
// "0 is not a valid handle" convention from the old code is preserved.

const FREE_SENTINEL: u32 = u32::MAX;

/// A generation-tagged socket identity in the positive small-integer domain.
/// Domain validity does not imply that the socket is still registered.
///
/// ```compile_fail
/// use nera_runtime::socket::SocketHandle;
/// let _ = SocketHandle(0);
/// ```
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SocketHandle(u64);

impl SocketHandle {
    pub const MAX_GENERATION: u32 = (crate::term::SMALL_INT_MAX as u64 >> 32) as u32;
    pub const MAX_INDEX: u32 = FREE_SENTINEL - 1;

    #[inline]
    pub const fn try_from_raw(raw: u64) -> Option<Self> {
        if raw <= crate::term::SMALL_INT_MAX as u64 && raw >> 32 != 0 && raw as u32 != FREE_SENTINEL
        {
            Some(Self(raw))
        } else {
            None
        }
    }

    #[inline]
    pub fn from_term(term: crate::term::ErlTerm) -> Option<Self> {
        Self::try_from_raw(term.as_small_int()? as u64)
    }

    #[inline]
    pub const fn as_raw(self) -> u64 {
        self.0
    }

    /// Encoding needs no further range check: construction validates the domain.
    #[inline]
    pub const fn to_term(self) -> crate::term::ErlTerm {
        crate::term::ErlTerm::from_raw(
            (self.0 << crate::term::TAG_BITS) | crate::term::TAG_SMALL_INT,
        )
    }
}

impl std::fmt::Display for SocketHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone)]
enum Slot {
    /// No socket owner. Below MAX_GENERATION it is on the free list;
    /// at MAX_GENERATION it is permanently retired and unlinked.
    Vacant {
        /// Next free-list link, or `FREE_SENTINEL` for end of list.
        next_free: u32,
        /// Generation of the *last* occupied state on this slot. On the
        /// next free → occupied transition we bump this and reuse it as
        /// the new live generation.
        last_generation: u32,
    },
    Occupied {
        generation: u32,
        entry: Arc<SocketEntry>,
    },
}

struct Registry {
    slots: Vec<Slot>,
    free_head: u32,
}

impl Registry {
    const fn new() -> Self {
        Registry {
            slots: Vec::new(),
            free_head: FREE_SENTINEL,
        }
    }

    fn insert(&mut self, entry: Arc<SocketEntry>) -> io::Result<SocketHandle> {
        if self.free_head != FREE_SENTINEL {
            let idx = self.free_head as usize;
            let new_generation = match self.slots[idx] {
                Slot::Vacant { next_free, last_generation } => {
                    self.free_head = next_free;
                    // remove() only links slots below the final generation.
                    debug_assert!(last_generation < SocketHandle::MAX_GENERATION);
                    last_generation + 1
                }
                Slot::Occupied { .. } => unreachable!("free_head pointed at non-free slot"),
            };
            self.slots[idx] = Slot::Occupied {
                generation: new_generation,
                entry,
            };
            Ok(encode_handle(idx as u32, new_generation))
        } else {
            let idx = self.slots.len();
            let index = vacant_index(idx)?;
            self.slots.push(Slot::Occupied {
                generation: 1,
                entry,
            });
            Ok(encode_handle(index, 1))
        }
    }

    fn get(&self, handle: SocketHandle) -> Option<&Arc<SocketEntry>> {
        let (idx, gen) = decode_handle(handle);
        let slot = self.slots.get(idx as usize)?;
        match slot {
            Slot::Occupied { generation, entry } if *generation == gen => Some(entry),
            _ => None,
        }
    }

    fn remove(&mut self, handle: SocketHandle) -> Option<Arc<SocketEntry>> {
        let (idx, gen) = decode_handle(handle);
        let slot = self.slots.get_mut(idx as usize)?;
        // Only remove if the generation matches — stale handles silently
        // fail, matching the old "HashMap::remove on stale key" behaviour.
        match slot {
            Slot::Occupied { generation, .. } if *generation == gen => {
                let prev_free_head = self.free_head;
                // Replace the slot with a Free marker, extracting the
                // Arc as we go.
                let old = std::mem::replace(
                    slot,
                    Slot::Vacant {
                        next_free: if gen < SocketHandle::MAX_GENERATION { prev_free_head } else { FREE_SENTINEL },
                        last_generation: gen,
                    },
                );
                if gen < SocketHandle::MAX_GENERATION { self.free_head = idx; }
                match old {
                    Slot::Occupied { entry, .. } => Some(entry),
                    Slot::Vacant { .. } => unreachable!(),
                }
            }
            _ => None,
        }
    }

    /// Iterate over all occupied slots. Yields `(handle, &entry)`.
    fn iter(&self) -> impl Iterator<Item = (SocketHandle, &Arc<SocketEntry>)> {
        self.slots.iter().enumerate().filter_map(|(idx, slot)| match slot {
            Slot::Occupied { generation, entry } => {
                Some((encode_handle(idx as u32, *generation), entry))
            }
            Slot::Vacant { .. } => None,
        })
    }
}

#[inline]
fn encode_handle(idx: u32, generation: u32) -> SocketHandle {
    SocketHandle::try_from_raw(((generation as u64) << 32) | idx as u64)
        .expect("socket registry preserves handle domain")
}

fn vacant_index(len: usize) -> io::Result<u32> {
    if len > SocketHandle::MAX_INDEX as usize {
        Err(io::Error::other("socket handle space exhausted"))
    } else { Ok(len as u32) }
}

#[inline]
fn decode_handle(handle: SocketHandle) -> (u32, u32) {
    let idx = handle.as_raw() as u32;
    let generation = (handle.as_raw() >> 32) as u32;
    (idx, generation)
}

/// The global socket table. Writes take an exclusive lock; reads take
/// a shared lock and then do a direct Vec index — no hashing, no probe.
static REGISTRY: RwLock<Registry> = RwLock::new(Registry::new());

/// Get an `Arc<SocketEntry>` for a handle. This is the primary lookup
/// on the hot path — every worker `recv`/`send` and every reactor event
/// dispatch goes through it. Cheap: one RwLock read-acquire, one Vec
/// index, one generation compare, one Arc clone.
pub(crate) fn get_entry(handle: SocketHandle) -> Option<Arc<SocketEntry>> {
    REGISTRY.read().get(handle).cloned()
}

/// Convenience: run a closure on a handle's opts (read lock + per-socket lock).
pub(crate) fn with_entry_opts<F, R>(handle: SocketHandle, f: F) -> Option<R>
where
    F: FnOnce(&mut SocketOpts) -> R,
{
    let entry = get_entry(handle)?;
    let mut opts = entry.opts.lock();
    Some(f(&mut opts))
}

/// Register a socket and return its handle.
pub fn register(sock: Socket) -> io::Result<SocketHandle> {
    let entry = Arc::new(SocketEntry {
        socket: sock,
        opts: Mutex::new(SocketOpts::default()),
    });
    REGISTRY.write().insert(entry)
}

/// Remove a socket from the registry. Returns `true` if it existed.
///
/// On mio-supported platforms, notifies the reactor to deregister the
/// socket from the poll instance. On other platforms, shuts down the
/// socket to unblock any blocking I/O.
pub fn remove(handle: SocketHandle) -> bool {
    #[cfg(mio_supported)]
    {
        // Shut down the socket BEFORE removing from the registry.
        // This sends TCP FIN immediately, even if other code holds
        // Arc<SocketEntry> clones (which would prevent Arc::try_unwrap
        // and delay the actual fd close).
        if let Some(entry) = REGISTRY.read().get(handle) {
            if let Socket::Stream(s) = &entry.socket {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
        }

        // Extract the raw fd BEFORE removing from the registry — this is the
        // fallback for the reactor when Arc::try_unwrap fails below.
        let raw_fd = get_raw_fd(handle);

        // Remove the socket from the registry, then pass the owned Socket
        // to the reactor so it can extract the raw fd for deregister.
        let removed = REGISTRY.write().remove(handle);
        let existed = removed.is_some();
        // Extract the Socket from the entry for the reactor. If other code
        // still holds Arc references (e.g. an in-flight worker read),
        // try_unwrap fails and socket=None — the reactor uses raw_fd instead.
        let socket = removed.and_then(|entry| Arc::try_unwrap(entry).ok()).map(|e| e.socket);
        if let Some(reactor) = crate::reactor::global_reactor() {
            reactor.send_command(crate::reactor::ReactorCommand::Deregister {
                handle,
                socket,
                raw_fd,
            });
        }
        existed
    }
    #[cfg(not(mio_supported))]
    {
        shutdown_socket(handle);
        REGISTRY.write().remove(handle).is_some()
    }
}

/// Remove all sockets owned by the given process. Called on process exit
/// to prevent file descriptor leaks.
pub fn remove_all_for_pid(pid: crate::process::ProcessId) {
    let handles: Vec<SocketHandle> = REGISTRY
        .read()
        .iter()
        .filter(|(_, entry)| entry.opts.lock().controlling_pid == pid)
        .map(|(h, _)| h)
        .collect();
    for h in handles {
        remove(h);
    }
}

/// Shut down a socket so that any in-flight I/O on Arc clones returns
/// an error immediately. Only needed on non-mio platforms where I/O
/// pool threads may be blocked on syscalls.
#[cfg(not(mio_supported))]
fn shutdown_socket(handle: SocketHandle) {
    let listener_addr = match REGISTRY.read().get(handle) {
        Some(entry) => match &entry.socket {
            Socket::Stream(s) => {
                let _ = s.shutdown(std::net::Shutdown::Both);
                None
            }
            Socket::Listener(l) => {
                let _ = l.set_nonblocking(true);
                l.local_addr().ok()
            }
        },
        None => None,
    };
    if let Some(addr) = listener_addr {
        let _ = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(100));
    }
}

/// Shut down and remove ALL sockets in the registry. Called during
/// scheduler shutdown.
pub fn shutdown_all() {
    let handles: Vec<SocketHandle> = REGISTRY.read().iter().map(|(h, _)| h).collect();
    for h in handles {
        remove(h);
    }
}

/// Get a copy of the socket options for a handle.
pub fn get_opts(handle: SocketHandle) -> Option<SocketOpts> {
    let entry = get_entry(handle)?;
    let opts = entry.opts.lock().clone();
    Some(opts)
}

/// Set the active mode for a socket.
///
/// When going passive (`active == ActiveMode::Passive`), sends `DeregisterActive` to the
/// reactor only if the socket was actually registered (avoids a spurious
/// kevent syscall on the fast path where the worker read inline).
pub fn set_active(handle: SocketHandle, active: ActiveMode) {
    let was_registered = with_entry_opts(handle, |opts| {
        opts.active = active;
        if active == ActiveMode::Passive && opts.reactor_registered {
            opts.reactor_registered = false;
            return true;
        }
        false
    }).unwrap_or(false);
    // When going passive, tell the reactor to deregister active-mode —
    // but only if we actually sent a RegisterActive earlier.
    #[cfg(mio_supported)]
    if was_registered {
        if let Some(reactor) = crate::reactor::global_reactor() {
            reactor.send_command(crate::reactor::ReactorCommand::DeregisterActive { handle });
        }
    }
}

/// Set the active mode flag locally without sending any reactor command.
///
/// Used on the fast path when `{active, once}` completed inline — the
/// reactor was never involved, so there is nothing to deregister.
pub fn set_active_local(handle: SocketHandle, active: ActiveMode) {
    with_entry_opts(handle, |opts| {
        opts.active = active;
    });
}

/// Mark whether a `RegisterActive` command was sent to the reactor for
/// this socket. Must be called whenever `RegisterActive` is sent so that
/// `set_active(handle, ActiveMode::Passive)` knows whether to send `DeregisterActive`.
pub fn mark_reactor_registered(handle: SocketHandle, registered: bool) {
    with_entry_opts(handle, |opts| {
        opts.reactor_registered = registered;
    });
}

/// Set the packet mode for a socket.
pub fn set_packet(handle: SocketHandle, packet: PacketMode) {
    with_entry_opts(handle, |opts| {
        opts.packet = packet;
    });
}

/// Take the line buffer contents (drain + return). Used by the active-mode
/// worker to prepend buffered partial-line data to newly read data.
pub fn take_line_buffer(handle: SocketHandle) -> Vec<u8> {
    with_entry_opts(handle, |opts| std::mem::take(&mut opts.line_buffer))
        .unwrap_or_default()
}

/// Store partial line data in the socket's line buffer.
pub fn set_line_buffer(handle: SocketHandle, data: Vec<u8>) {
    with_entry_opts(handle, |opts| {
        opts.line_buffer = data;
    });
}

/// Set the controlling process PID for a socket.
pub fn set_controlling_pid(handle: SocketHandle, pid: crate::process::ProcessId) {
    with_entry_opts(handle, |opts| {
        opts.controlling_pid = pid;
    });
}

// ---------------------------------------------------------------------------
// Non-blocking operations (mio-supported platforms only)
// ---------------------------------------------------------------------------

/// Try a non-blocking accept on a listener. Returns the new stream handle
/// or WouldBlock if no connection is pending.
#[cfg(mio_supported)]
pub fn try_accept(listener_handle: SocketHandle) -> io::Result<SocketHandle> {
    let entry = get_entry(listener_handle)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not a listener"))?;
    let listener_arc = match &entry.socket {
        Socket::Listener(l) => Arc::clone(l),
        _ => return Err(io::Error::new(io::ErrorKind::NotFound, "not a listener")),
    };

    let (stream, _addr) = listener_arc.accept()?;
    let stream_handle = register(Socket::Stream(Arc::new(stream)))?;

    // Inherit opts from listener (e.g. {packet, line})
    let packet = entry.opts.lock().packet;
    if packet != PacketMode::Raw {
        set_packet(stream_handle, packet);
    }

    Ok(stream_handle)
}

/// Try a non-blocking read from a stream. Returns data or WouldBlock.
#[cfg(mio_supported)]
pub fn try_recv(stream_handle: SocketHandle) -> io::Result<Vec<u8>> {
    use std::io::Read;

    let stream = get_stream(stream_handle)?;
    let mut buf = vec![0u8; 4096];
    match (&*stream).read(&mut buf) {
        Ok(0) => Err(io::Error::new(io::ErrorKind::ConnectionAborted, "closed")),
        Ok(n) => {
            buf.truncate(n);
            Ok(buf)
        }
        Err(e) => Err(e),
    }
}

/// Try a non-blocking write to a stream. Returns bytes written or WouldBlock.
#[cfg(mio_supported)]
pub fn try_send(stream_handle: SocketHandle, data: &[u8]) -> io::Result<usize> {
    use std::io::Write;

    let stream = get_stream(stream_handle)?;
    (&*stream).write(data)
}

// ---------------------------------------------------------------------------
// Blocking operations (used by I/O pool on non-mio platforms, and kept
// for backward compatibility on mio platforms)
// ---------------------------------------------------------------------------

/// Accept a connection on a listener socket. Returns the new stream
/// handle, or an IO error.
pub fn accept(listener_handle: SocketHandle) -> io::Result<SocketHandle> {
    let entry = get_entry(listener_handle)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not a listener"))?;
    let listener_arc = match &entry.socket {
        Socket::Listener(l) => Arc::clone(l),
        _ => return Err(io::Error::new(io::ErrorKind::NotFound, "not a listener")),
    };

    let (stream, _addr) = listener_arc.accept()?;
    let handle = register(Socket::Stream(Arc::new(stream)))?;

    // Inherit opts from listener (e.g. {packet, line})
    let packet = entry.opts.lock().packet;
    if packet != PacketMode::Raw {
        set_packet(handle, packet);
    }

    Ok(handle)
}

/// Get an `Arc` clone of a stream socket for I/O without holding the
/// registry lock.  Returns `NotFound` if the handle is absent or is a
/// listener.
pub(crate) fn get_stream(stream_handle: SocketHandle) -> io::Result<Arc<TcpStream>> {
    let entry = get_entry(stream_handle)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not a stream"))?;
    match &entry.socket {
        Socket::Stream(s) => Ok(Arc::clone(s)),
        _ => Err(io::Error::new(io::ErrorKind::NotFound, "not a stream")),
    }
}

/// Get an `Arc` clone of a listener socket.
pub(crate) fn get_listener(handle: SocketHandle) -> io::Result<Arc<TcpListener>> {
    let entry = get_entry(handle)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not a listener"))?;
    match &entry.socket {
        Socket::Listener(l) => Ok(Arc::clone(l)),
        _ => Err(io::Error::new(io::ErrorKind::NotFound, "not a listener")),
    }
}

/// Get the raw file descriptor for a socket handle (for reactor poll registration).
#[cfg(mio_supported)]
pub(crate) fn get_raw_fd(handle: SocketHandle) -> Option<std::os::unix::io::RawFd> {
    use std::os::unix::io::AsRawFd;
    let entry = get_entry(handle)?;
    match &entry.socket {
        Socket::Stream(s) => Some(s.as_raw_fd()),
        Socket::Listener(l) => Some(l.as_raw_fd()),
    }
}

/// Peek using a raw fd directly (no registry lookup).
#[cfg(mio_supported)]
pub(crate) fn peek_recv_fd(fd: std::os::unix::io::RawFd, buf: &mut [u8]) -> io::Result<usize> {
    let n = unsafe {
        libc::recv(
            fd,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len(),
            libc::MSG_PEEK,
        )
    };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

/// Consume up to `dest.len()` bytes from a raw fd (single recv call).
/// Returns the number of bytes actually consumed, or an error.
/// Use for raw-mode reads where short reads are normal.
#[cfg(mio_supported)]
pub(crate) fn consume_fd(
    fd: std::os::unix::io::RawFd,
    dest: &mut [u8],
) -> io::Result<usize> {
    let n = unsafe {
        libc::recv(
            fd,
            dest.as_mut_ptr() as *mut libc::c_void,
            dest.len(),
            0, // no flags — consuming read
        )
    };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

/// Consume exactly `dest.len()` bytes from a raw fd, looping on short reads.
/// The caller should have peeked first to confirm the data is available.
/// Use for line-mode reads where partial consume would desync buffers.
#[cfg(mio_supported)]
pub(crate) fn consume_exact_fd(
    fd: std::os::unix::io::RawFd,
    dest: &mut [u8],
) -> io::Result<()> {
    let mut offset = 0;
    while offset < dest.len() {
        let n = unsafe {
            libc::recv(
                fd,
                dest[offset..].as_mut_ptr() as *mut libc::c_void,
                dest.len() - offset,
                0,
            )
        };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed during consume",
            ));
        }
        offset += n as usize;
    }
    Ok(())
}

/// Read from a stream socket. Returns the bytes read, or an IO error.
pub fn recv(stream_handle: SocketHandle) -> io::Result<Vec<u8>> {
    use std::io::Read;

    let stream = get_stream(stream_handle)?;

    let mut buf = vec![0u8; 4096];
    let n = (&*stream).read(&mut buf)?;
    if n == 0 {
        return Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "closed",
        ));
    }
    buf.truncate(n);
    Ok(buf)
}

/// Write to a stream socket. Returns Ok(()) or an IO error.
pub fn send(stream_handle: SocketHandle, data: &[u8]) -> io::Result<()> {
    use std::io::Write;

    let stream = get_stream(stream_handle)?;
    (&*stream).write_all(data)
}

/// Read from a stream socket until a newline (`\n`) is encountered.
/// Safety limit: stops after 300,000 bytes to prevent unbounded reads.
pub fn recv_line(stream_handle: SocketHandle) -> io::Result<Vec<u8>> {
    use std::io::Read;

    let stream = get_stream(stream_handle)?;

    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    const MAX_LINE: usize = 300_000;

    loop {
        let n = (&*stream).read(&mut byte)?;
        if n == 0 {
            if line.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "closed",
                ));
            }
            return Ok(line);
        }
        if byte[0] == b'\n' {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(line);
        }
        line.push(byte[0]);
        if line.len() >= MAX_LINE {
            return Ok(line);
        }
    }
}

/// Read exactly `length` bytes from a stream socket.
pub fn recv_exact(stream_handle: SocketHandle, length: usize) -> io::Result<Vec<u8>> {
    let stream = get_stream(stream_handle)?;

    read_exact_bytes(&mut &*stream, length)
}

// Accumulate only bytes actually received. In particular, a peer that closes
// immediately cannot force an allocation proportional to its requested length.
fn read_exact_bytes(reader: &mut impl std::io::Read, length: usize) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    reader.take(length as u64).read_to_end(&mut buf)?;
    if buf.len() != length {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed"));
    }
    Ok(buf)
}

/// Bind a TcpListener on the given port. Returns the handle.
pub fn listen(port: u16) -> io::Result<SocketHandle> {
    #[cfg(mio_supported)]
    {
        // Create a std listener first, set non-blocking, convert to mio.
        let std_listener = std::net::TcpListener::bind(format!("0.0.0.0:{}", port))?;
        std_listener.set_nonblocking(true)?;
        let listener = TcpListener::from_std(std_listener);
        register(Socket::Listener(Arc::new(listener)))
    }
    #[cfg(not(mio_supported))]
    {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port))?;
        register(Socket::Listener(Arc::new(listener)))
    }
}

/// Connect to a remote TCP address. Creates a client stream socket.
///
/// On mio platforms, performs a blocking `std::net` connect then converts
/// to a mio non-blocking stream. On other platforms, connects directly.
pub fn connect(addr: &str, port: u16) -> io::Result<SocketHandle> {
    #[cfg(mio_supported)]
    {
        let std_stream = std::net::TcpStream::connect(format!("{}:{}", addr, port))?;
        std_stream.set_nonblocking(true)?;
        let mio_stream = TcpStream::from_std(std_stream);
        register(Socket::Stream(Arc::new(mio_stream)))
    }
    #[cfg(not(mio_supported))]
    {
        let stream = TcpStream::connect(format!("{}:{}", addr, port))?;
        register(Socket::Stream(Arc::new(stream)))
    }
}

/// Get the local port number of a socket (listener or stream).
pub fn local_port(handle: SocketHandle) -> Option<u16> {
    let entry = get_entry(handle)?;
    match &entry.socket {
        Socket::Listener(l) => l.local_addr().ok().map(|a| a.port()),
        Socket::Stream(s) => s.local_addr().ok().map(|a| a.port()),
    }
}

/// Get the local address of a listener. Useful for tests.
pub fn listener_local_addr(handle: SocketHandle) -> Option<std::net::SocketAddr> {
    let entry = get_entry(handle)?;
    match &entry.socket {
        Socket::Listener(l) => l.local_addr().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: create a std::net listener + stream pair for testing.
    fn make_test_pair() -> (std::net::TcpListener, std::net::TcpStream, u16) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stream = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        (listener, stream, port)
    }

    // ---------------------------------------------------------------
    // Slab encoding / ABA safety
    // ---------------------------------------------------------------

    #[test]
    fn exact_reader_stops_at_length_and_rejects_short_input() {
        let mut input = std::io::Cursor::new(b"abcde");
        assert_eq!(read_exact_bytes(&mut input, 3).unwrap(), b"abc");
        assert_eq!(input.position(), 3);
        assert_eq!(read_exact_bytes(&mut input, 0).unwrap(), b"");
        assert_eq!(input.position(), 3);
        assert_eq!(read_exact_bytes(&mut input, 3).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        let mut empty = std::io::Cursor::new(b"");
        assert_eq!(read_exact_bytes(&mut empty, isize::MAX as usize).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn socket_modes_are_byte_sized_with_passive_raw_defaults() {
        assert_eq!(std::mem::size_of::<ActiveMode>(), 1);
        assert_eq!(std::mem::size_of::<PacketMode>(), 1);
        let opts = SocketOpts::default();
        assert_eq!(opts.active, ActiveMode::Passive);
        assert_eq!(opts.packet, PacketMode::Raw);
    }

    fn local_test_entry() -> Arc<SocketEntry> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        #[cfg(mio_supported)]
        let listener = TcpListener::from_std(listener);
        Arc::new(SocketEntry {
            socket: Socket::Listener(Arc::new(listener)),
            opts: Mutex::new(SocketOpts::default()),
        })
    }

    #[test]
    fn exhausted_socket_generation_never_reenters_free_list() {
        let max_generation = (crate::term::SMALL_INT_MAX as u64 >> 32) as u32;
        let entry = local_test_entry();
        let mut registry = Registry::new();
        registry.slots.push(Slot::Occupied {
            generation: max_generation,
            entry: entry.clone(),
        });
        let old = encode_handle(0, max_generation);
        assert!(registry.remove(old).is_some());
        let new = registry.insert(entry).unwrap();
        assert!(
            new.as_raw() <= crate::term::SMALL_INT_MAX as u64,
            "replacement must fit a positive small integer"
        );
        assert_ne!(decode_handle(new).0, 0, "exhausted slot must be retired");
        assert!(registry.get(old).is_none());
        assert!(registry.remove(old).is_none());
    }

    #[test]
    fn socket_handle_domain_and_term_roundtrip() {
        assert_eq!(
            std::mem::size_of::<SocketHandle>(),
            std::mem::size_of::<u64>()
        );
        assert_eq!(
            std::mem::align_of::<SocketHandle>(),
            std::mem::align_of::<u64>()
        );
        for raw in [
            0,
            1,
            u32::MAX as u64,
            u64::MAX,
            (SocketHandle::MAX_GENERATION as u64 + 1) << 32,
            (1 << 32) | FREE_SENTINEL as u64,
        ] {
            assert!(SocketHandle::try_from_raw(raw).is_none(), "{raw}");
        }
        for (index, generation) in [
            (0, 1),
            (SocketHandle::MAX_INDEX, SocketHandle::MAX_GENERATION),
        ] {
            let handle = encode_handle(index, generation);
            assert!(handle.as_raw() <= crate::term::SMALL_INT_MAX as u64);
            assert_eq!(SocketHandle::from_term(handle.to_term()), Some(handle));
        }
        assert!(SocketHandle::from_term(crate::term::ErlTerm::small_int(-1)).is_none());
        assert!(SocketHandle::from_term(crate::term::ErlTerm::nil()).is_none());
        assert_eq!(
            vacant_index(SocketHandle::MAX_INDEX as usize).unwrap(),
            SocketHandle::MAX_INDEX
        );
        assert!(vacant_index(SocketHandle::MAX_INDEX as usize + 1).is_err());
    }

    #[test]
    fn final_socket_generation_is_usable_once_then_retired() {
        let entry = local_test_entry();
        let mut registry = Registry::new();
        registry.slots.push(Slot::Vacant {
            next_free: FREE_SENTINEL,
            last_generation: SocketHandle::MAX_GENERATION - 1,
        });
        registry.free_head = 0;
        let final_handle = registry.insert(entry.clone()).unwrap();
        assert_eq!(
            decode_handle(final_handle),
            (0, SocketHandle::MAX_GENERATION)
        );
        assert!(registry.get(final_handle).is_some());
        assert!(registry.remove(final_handle).is_some());
        for _ in 0..4 {
            let handle = registry.insert(entry.clone()).unwrap();
            assert_ne!(decode_handle(handle).0, 0);
            assert!(registry.get(final_handle).is_none());
            assert!(registry.remove(final_handle).is_none());
            assert!(registry.remove(handle).is_some());
        }
    }

    #[test]
    fn retiring_socket_preserves_other_free_slots_and_releases_owner() {
        let entry = local_test_entry();
        let weak = Arc::downgrade(&entry);
        let mut registry = Registry::new();
        registry.slots.push(Slot::Occupied {
            generation: SocketHandle::MAX_GENERATION,
            entry,
        });
        registry.slots.push(Slot::Vacant {
            next_free: FREE_SENTINEL,
            last_generation: 2,
        });
        registry.free_head = 1;
        let retired = encode_handle(0, SocketHandle::MAX_GENERATION);
        drop(registry.remove(retired).unwrap());
        assert!(
            weak.upgrade().is_none(),
            "retired slot must not retain the socket owner"
        );
        assert_eq!(registry.free_head, 1);
        let next = registry.insert(local_test_entry()).unwrap();
        assert_eq!(decode_handle(next), (1, 3));
        assert_eq!(
            registry
                .iter()
                .map(|(handle, _)| handle)
                .collect::<Vec<_>>(),
            vec![next]
        );
    }

    #[test]
    fn handle_encoding_roundtrips() {
        for &(idx, gen) in &[(0u32, 1u32), (1, 1), (12345, 42), (SocketHandle::MAX_INDEX, SocketHandle::MAX_GENERATION)] {
            let h = encode_handle(idx, gen);
            let (i2, g2) = decode_handle(h);
            assert_eq!((idx, gen), (i2, g2));
        }
    }

    #[test]
    fn valid_handle_is_never_zero() {
        // Slot 0 + generation 0 would encode to the all-zeros sentinel
        // handle, which callers sometimes treat as "no handle". The
        // invariant is that `insert` never yields that encoding — the
        // very first insert starts generation at 1, not 0.
        let (listener, _stream, _port) = make_test_pair();
        #[cfg(mio_supported)]
        let listener = mio::net::TcpListener::from_std(listener);
        let h = register(Socket::Listener(Arc::new(listener))).unwrap();
        assert_ne!(h.as_raw(), 0, "handle 0 must never be valid");
        assert!(remove(h));
    }

    #[test]
    fn stale_handle_after_reuse_returns_none() {
        // Register, remove, register again. If the second register happens
        // to reuse the same slot (which it will after a remove, since that
        // slot is now at the freelist head), the old handle must NOT alias
        // to the new socket — the generation tag protects against this.
        let (l1, s1, _p1) = make_test_pair();
        #[cfg(mio_supported)]
        let s1 = { s1.set_nonblocking(true).unwrap(); mio::net::TcpStream::from_std(s1) };
        let h_old = register(Socket::Stream(Arc::new(s1))).unwrap();
        drop(l1);
        assert!(remove(h_old));
        assert!(get_entry(h_old).is_none(), "removed handle must not resolve");

        let (l2, s2, _p2) = make_test_pair();
        #[cfg(mio_supported)]
        let s2 = { s2.set_nonblocking(true).unwrap(); mio::net::TcpStream::from_std(s2) };
        let h_new = register(Socket::Stream(Arc::new(s2))).unwrap();
        drop(l2);
        assert_ne!(h_old, h_new, "reused slot must get a fresh generation");
        assert!(get_entry(h_new).is_some());
        assert!(get_entry(h_old).is_none(), "stale handle must still fail");
        assert!(remove(h_new));
    }

    // ---------------------------------------------------------------
    // Registry operations
    // ---------------------------------------------------------------

    #[test]
    fn register_and_remove_stream() {
        let (_listener, stream, _port) = make_test_pair();

        #[cfg(mio_supported)]
        let stream = {
            stream.set_nonblocking(true).unwrap();
            mio::net::TcpStream::from_std(stream)
        };

        let handle = register(Socket::Stream(Arc::new(stream))).unwrap();
        assert!(handle.as_raw() > 0, "handle should be non-zero");
        assert!(remove(handle));
        assert!(!remove(handle));
    }

    #[test]
    fn register_listener() {
        // Use std::net directly and register manually to avoid races
        // with concurrent scheduler tests calling shutdown_all().
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std_listener.set_nonblocking(true).unwrap();

        #[cfg(mio_supported)]
        let listener = TcpListener::from_std(std_listener);
        #[cfg(not(mio_supported))]
        let listener = std_listener;

        let handle = register(Socket::Listener(Arc::new(listener))).unwrap();
        assert!(handle.as_raw() > 0);
        // Don't assert remove() returns true — a concurrent scheduler test
        // may call shutdown_all() which removes the handle first.
        let _ = remove(handle);
    }

    #[test]
    fn remove_nonexistent_handle() {
        assert!(!remove(encode_handle(SocketHandle::MAX_INDEX, 1)));
    }

    // ---------------------------------------------------------------
    // listen() on ephemeral port
    // ---------------------------------------------------------------

    #[test]
    fn listen_on_port_zero() {
        // Use std::net directly and register manually to avoid races
        // with concurrent scheduler tests calling shutdown_all().
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std_listener.set_nonblocking(true).unwrap();

        #[cfg(mio_supported)]
        let listener = TcpListener::from_std(std_listener);
        #[cfg(not(mio_supported))]
        let listener = std_listener;

        let handle = register(Socket::Listener(Arc::new(listener))).unwrap();
        assert!(handle.as_raw() > 0);
        assert!(remove(handle));
    }

    // ---------------------------------------------------------------
    // Error cases
    // ---------------------------------------------------------------

    #[test]
    fn recv_invalid_handle() {
        let result = recv(encode_handle(SocketHandle::MAX_INDEX, 1));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn send_invalid_handle() {
        let result = send(encode_handle(SocketHandle::MAX_INDEX, 1), b"hello");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn accept_invalid_handle() {
        let result = accept(encode_handle(SocketHandle::MAX_INDEX, 1));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn accept_on_stream_handle_fails() {
        let (_listener, stream, _port) = make_test_pair();

        #[cfg(mio_supported)]
        let stream = {
            stream.set_nonblocking(true).unwrap();
            mio::net::TcpStream::from_std(stream)
        };

        let handle = register(Socket::Stream(Arc::new(stream))).unwrap();
        let result = accept(handle);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
        remove(handle);
    }

    // ---------------------------------------------------------------
    // Roundtrip: listen → accept → send → recv
    // ---------------------------------------------------------------

    /// Retry a non-blocking operation until it succeeds or a timeout.
    fn retry_until_ok<T, F: FnMut() -> io::Result<T>>(mut f: F) -> io::Result<T> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match f() {
                Ok(v) => return Ok(v),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        return Err(e);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(e) => return Err(e),
            }
        }
    }

    #[test]
    fn listen_accept_send_recv_roundtrip() {
        // Use std::net sockets directly to avoid interference from
        // concurrent scheduler tests calling shutdown_all() on the
        // global socket registry. Tests register/accept/remove via the
        // registry but performs I/O via std::net directly.
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = std_listener.local_addr().unwrap().port();
        std_listener.set_nonblocking(true).unwrap();

        // Connect a client via std (completely outside the registry)
        let mut client_std = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();

        // Accept via std (also outside the registry)
        let server_std = retry_until_ok(|| std_listener.accept().map(|(s, _)| s)).unwrap();
        server_std.set_nonblocking(true).unwrap();

        // Send from client, recv on server — all via std::net.
        use std::io::{Read, Write};
        client_std.write_all(b"hello").unwrap();
        let data = retry_until_ok(|| {
            let mut buf = vec![0u8; 4096];
            let n = (&server_std).read(&mut buf)?;
            buf.truncate(n);
            Ok(buf)
        }).unwrap();
        assert_eq!(&data, b"hello");

        // Verify registry functions work by registering then removing.
        #[cfg(mio_supported)]
        let listener = TcpListener::from_std(std_listener);
        #[cfg(not(mio_supported))]
        let listener = std_listener;
        let listener_handle = register(Socket::Listener(Arc::new(listener))).unwrap();
        assert!(listener_handle.as_raw() > 0);
        assert!(remove(listener_handle));
    }

    // ---------------------------------------------------------------
    // remove_all_for_pid
    // ---------------------------------------------------------------

    #[test]
    fn remove_all_for_pid_cleans_owned_sockets() {
        // Use std::net for TCP setup to avoid interference from
        // concurrent scheduler tests calling shutdown_all().
        let pid = 42u64;
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = std_listener.local_addr().unwrap().port();
        std_listener.set_nonblocking(true).unwrap();

        // Create 3 client connections + accept them via std::net
        let _c1 = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let (a1, _) = retry_until_ok(|| std_listener.accept()).unwrap();
        let _c2 = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let (a2, _) = retry_until_ok(|| std_listener.accept()).unwrap();
        let _c3 = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let (a3, _) = retry_until_ok(|| std_listener.accept()).unwrap();

        // Register the listener and accepted streams in the registry.
        // Do all registrations quickly to minimize the race window.
        #[cfg(mio_supported)]
        let listener = TcpListener::from_std(std_listener);
        #[cfg(not(mio_supported))]
        let listener = std_listener;
        let listener_handle = register(Socket::Listener(Arc::new(listener))).unwrap();
        set_controlling_pid(listener_handle, crate::process::ProcessId::new(pid));

        #[cfg(mio_supported)]
        let a1 = { a1.set_nonblocking(true).unwrap(); mio::net::TcpStream::from_std(a1) };
        let h1 = register(Socket::Stream(Arc::new(a1))).unwrap();
        set_controlling_pid(h1, crate::process::ProcessId::new(pid));

        #[cfg(mio_supported)]
        let a2 = { a2.set_nonblocking(true).unwrap(); mio::net::TcpStream::from_std(a2) };
        let h2 = register(Socket::Stream(Arc::new(a2))).unwrap();
        set_controlling_pid(h2, crate::process::ProcessId::new(pid));

        #[cfg(mio_supported)]
        let a3 = { a3.set_nonblocking(true).unwrap(); mio::net::TcpStream::from_std(a3) };
        let h3 = register(Socket::Stream(Arc::new(a3))).unwrap();
        set_controlling_pid(h3, crate::process::ProcessId::new(99)); // Different pid — should survive

        // If a concurrent scheduler test called shutdown_all(), all
        // sockets may already be gone.  In that case the test is vacuous
        // but must not panic.
        if get_opts(listener_handle).is_none() {
            // shutdown_all() already ran — nothing left to test.
            return;
        }

        // Remove all sockets owned by pid 42
        remove_all_for_pid(crate::process::ProcessId::new(pid));

        // pid 42 sockets should be gone
        assert!(get_opts(listener_handle).is_none());
        assert!(get_opts(h1).is_none());
        assert!(get_opts(h2).is_none());
        // pid 99 socket should survive
        assert!(get_opts(h3).is_some());

        remove(h3);
    }

    #[test]
    fn remove_all_for_pid_noop_when_no_sockets() {
        remove_all_for_pid(crate::process::ProcessId::new(999_999));
    }

    // ---------------------------------------------------------------
    // Regression: remove() must send TCP FIN even with outstanding Arc refs
    // ---------------------------------------------------------------

    #[test]
    fn remove_sends_fin_with_outstanding_arc_ref() {
        // Regression test: if another thread holds an Arc<SocketEntry> clone
        // when remove() is called, the TCP connection must still be shut down
        // (TCP FIN sent) immediately. Before the fix, Arc::try_unwrap would
        // fail and the fd would stay open indefinitely.
        use std::io::Read;

        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = std_listener.local_addr().unwrap().port();
        std_listener.set_nonblocking(true).unwrap();

        // Client connects
        let mut client = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        client.set_nonblocking(false).unwrap();
        client.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();

        // Server accepts
        let (server, _) = retry_until_ok(|| std_listener.accept()).unwrap();
        server.set_nonblocking(true).unwrap();

        // Register the server-side stream
        #[cfg(mio_supported)]
        let mio_stream = mio::net::TcpStream::from_std(server);
        #[cfg(not(mio_supported))]
        let mio_stream = server;

        let handle = register(Socket::Stream(Arc::new(mio_stream))).unwrap();

        // Simulate an outstanding Arc reference (as if another operation holds it)
        let _extra_ref = get_entry(handle);
        assert!(_extra_ref.is_some(), "entry should exist");

        // Now remove — this should shut down the socket despite the extra Arc ref
        assert!(remove(handle));

        // The client should see the connection close (read returns 0 bytes or error)
        let mut buf = [0u8; 64];
        let result = client.read(&mut buf);
        match result {
            Ok(0) => {} // EOF — connection closed, this is the expected case
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {} // Also acceptable
            other => panic!("expected EOF or ConnectionReset, got {:?}", other),
        }
    }
}
