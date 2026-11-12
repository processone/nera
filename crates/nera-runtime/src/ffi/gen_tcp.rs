//! TCP socket operations: listen, accept, recv, send, close.

#![allow(unused_unsafe)]

use crate::socket::{ActiveMode, PacketMode};
use crate::atom_table::*;
use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;


/// Spawn an OS thread that reads one packet/line from the socket and
/// delivers it as `{tcp, Socket, Data}` to the controlling process.
/// On error/close, delivers `{tcp_closed, Socket}` or `{tcp_error, Socket, Reason}`.
/// If active mode was `once` (1), resets active to `false` (0) after delivery.
/// Only used on non-mio platforms (fallback path).
#[cfg(not(mio_supported))]
fn spawn_active_reader(handle: crate::socket::SocketHandle, dest_pid: crate::process::ProcessId, active: ActiveMode, packet: PacketMode) {
    if let Some(sched) = crate::scheduler::global_scheduler() {
        let process_table = std::sync::Arc::clone(&sched.process_table);
        let injector = sched.injector.clone();
        let runnable_count = std::sync::Arc::clone(&sched.runnable_count);
        let timer_wheel = std::sync::Arc::clone(&sched.timer_wheel);
        let worker_notify = std::sync::Arc::clone(&sched.worker_notify);

        std::thread::spawn(move || {
            // Read data based on packet mode
            let result = if packet == PacketMode::Line {
                // line mode
                crate::socket::recv_line(handle)
            } else {
                // raw mode
                crate::socket::recv(handle)
            };

            // Intern atoms for the message tuple tags
            let tcp_atom_id = global_atom_table().intern("tcp");
            let tcp_closed_atom_id = global_atom_table().intern("tcp_closed");
            let tcp_error_atom_id = global_atom_table().intern("tcp_error");

            // Build the message as a MessageFragment
            let fragment = match result {
                Ok(bytes) => {
                    // Build {tcp, Socket, Data} on a fragment heap
                    let mut heap = crate::heap::FragmentHeap::new();
                    let data_bin = heap.alloc_binary_auto(&bytes);
                    let sock_term = handle.to_term();
                    let msg = heap.alloc_tuple(&[
                        ErlTerm::atom(tcp_atom_id),
                        sock_term,
                        data_bin,
                    ]);
                    // SAFETY: this root was built/copied entirely into the transferred heap.
                    unsafe { crate::message::MessageFragment::from_owned_heap(heap, msg) }
                }
                Err(e) => {
                    let mut heap = crate::heap::FragmentHeap::new();
                    let sock_term = handle.to_term();
                    if e.kind() == std::io::ErrorKind::ConnectionAborted
                        || e.kind() == std::io::ErrorKind::ConnectionReset
                        || e.kind() == std::io::ErrorKind::UnexpectedEof
                    {
                        // {tcp_closed, Socket}
                        let msg = heap.alloc_tuple(&[
                            ErlTerm::atom(tcp_closed_atom_id),
                            sock_term,
                        ]);
                        // SAFETY: this root was built/copied entirely into the transferred heap.
                        unsafe { crate::message::MessageFragment::from_owned_heap(heap, msg) }
                    } else {
                        // {tcp_error, Socket, Reason}
                        let reason_str = format!("{}", e);
                        let reason_bin = heap.alloc_binary_auto(reason_str.as_bytes());
                        let msg = heap.alloc_tuple(&[
                            ErlTerm::atom(tcp_error_atom_id),
                            sock_term,
                            reason_bin,
                        ]);
                        // SAFETY: this root was built/copied entirely into the transferred heap.
                        unsafe { crate::message::MessageFragment::from_owned_heap(heap, msg) }
                    }
                }
            };

            // If active was `once`, set active back to false
            if active == ActiveMode::Once {
                crate::socket::set_active(handle, ActiveMode::Passive);
            }

            // Deliver the message to the controlling process
            let entry = process_table.get(&dest_pid).map(|r| r.clone());
            if let Some(entry) = entry {
                entry.inbox.push(fragment);
                if entry.try_wake() {
                    runnable_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    injector.push(entry);
                    timer_wheel.cancel_for_pid(dest_pid);
                    if let Some(sched) = crate::scheduler::global_scheduler() {
                        for t in sched.worker_threads.iter() {
                            if let Some(t) = t.get() {
                                t.unpark();
                                break;
                            }
                        }
                    }
                }
            }
        });
    }
}


// Validate numeric arguments once at the FFI boundary, before network I/O.
#[inline]
fn tcp_port(term: ErlTerm) -> Option<u16> {
    u16::try_from(term.as_small_int()?).ok()
}

#[inline]
fn recv_length(term: ErlTerm) -> Option<usize> {
    let length = usize::try_from(term.as_small_int()?).ok()?;
    // A byte Vec cannot represent a length above isize::MAX.
    (length <= isize::MAX as usize).then_some(length)
}

/// gen_tcp:listen(Port, _Opts) -> {ok, Handle} | {error, Reason}
///
/// Binds a TCP listener on 0.0.0.0:Port and registers it in the socket
/// registry. The options list is ignored (minimal implementation).
/// Returns {ok, Handle} where Handle is a small integer, or {error, Reason}.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_gen_tcp_listen(
    ctx: *mut ProcessContext,
    port_term: u64,
    opts_term: u64,
) -> u64 {
    let Some(port) = tcp_port(ErlTerm(port_term)) else {
        return raise_badarg(ctx);
    };

    // Parse listener options for {packet, line} etc.
    let mut packet_mode = PacketMode::Raw; // default raw
    let mut cursor = ErlTerm(opts_term);
    while cursor.is_cons() {
        if let Some(cell_ptr) = cursor.as_cons_ptr() {
            let (elem, tail) = unsafe { ((*cell_ptr).head, (*cell_ptr).tail) };
            if elem.is_boxed() {
                if let Some(tptr) = elem.as_boxed_ptr() {
                    let header = unsafe { BoxedHeader(*tptr) };
                    if header.subtype() == BOXED_TUPLE && header.arity() == 2 {
                        let key = unsafe { ErlTerm(*tptr.add(1)) };
                        let val = unsafe { ErlTerm(*tptr.add(2)) };
                        if let Some(key_atom) = key.as_atom_id() {
                            let key_name = global_atom_table().resolve(key_atom);
                            if key_name.as_deref() == Some("packet") {
                                if let Some(val_atom) = val.as_atom_id() {
                                    let val_name = global_atom_table().resolve(val_atom);
                                    match val_name.as_deref() {
                                        Some("line") => packet_mode = PacketMode::Line,
                                        Some("raw") => packet_mode = PacketMode::Raw,
                                        _ => {}
                                    }
                                } else if let Some(int_val) = val.as_small_int() {
                                    if int_val == 0 { packet_mode = PacketMode::Raw; }
                                }
                            }
                        }
                    }
                }
            }
            cursor = tail;
        } else {
            break;
        }
    }

    match crate::socket::listen(port) {
        Ok(handle) => {
            // Set the controlling process so the listener is cleaned up on exit
            let self_pid = if !ctx.is_null() { unsafe { (*ctx).pid } } else { crate::process::ProcessId::new(0) };
            crate::socket::set_controlling_pid(handle, self_pid);
            // Store parsed options on the listener socket
            if packet_mode != PacketMode::Raw {
                crate::socket::set_packet(handle, packet_mode);
            }
            with_ctx_heap(ctx, |heap| {
                let ok_atom = ErlTerm::atom(ATOM_OK);
                let handle_term = handle.to_term();
                heap.alloc_tuple(&[ok_atom, handle_term]).0
            })
        }
        Err(e) => {
            with_ctx_heap(ctx, |heap| {
                let error_atom = ErlTerm::atom(ATOM_ERROR);
                let reason_atom = ErlTerm::atom(io_error_to_atom_id(&e));
                heap.alloc_tuple(&[error_atom, reason_atom]).0
            })
        }
    }
}

/// The FFI caller guarantees a live context and exclusive non-reentrant heap access.
unsafe fn socket_error(ctx: *mut ProcessContext, reason: u32) -> u64 {
    with_ctx_heap(ctx, |heap| heap.alloc_tuple(&[
        ErlTerm::atom(ATOM_ERROR), ErlTerm::atom(reason),
    ]).0)
}

/// gen_tcp:accept(LSock) -> {ok, Handle} | {error, Reason}
///
/// On mio-supported platforms: tries non-blocking accept first (fast path).
/// If WouldBlock, registers with the reactor and suspends.
/// On other platforms: submits to the blocking I/O pool.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_gen_tcp_accept(
    ctx: *mut ProcessContext,
    lsock_term: u64,
) -> u64 {
    let Some(handle) = crate::socket::SocketHandle::from_term(ErlTerm(lsock_term)) else {
        return socket_error(ctx, crate::atom_table::ATOM_CLOSED);
    };
    let self_pid = if !ctx.is_null() { unsafe { (*ctx).pid } } else { crate::process::ProcessId::new(0) };

    #[cfg(mio_supported)]
    {
        match crate::socket::try_accept(handle) {
            Ok(stream_handle) => {
                crate::socket::set_controlling_pid(stream_handle, self_pid);
                with_ctx_heap(ctx, |heap| {
                    let ok_atom = ErlTerm::atom(ATOM_OK);
                    let handle_term = stream_handle.to_term();
                    heap.alloc_tuple(&[ok_atom, handle_term]).0
                })
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                submit_to_reactor_and_wait(
                    ctx,
                    crate::reactor::PendingOp::Accept {
                        pid: self_pid,
                        listener_handle: handle,
                    },
                )
            }
            Err(e) => {
                with_ctx_heap(ctx, |heap| {
                    let error_atom = ErlTerm::atom(ATOM_ERROR);
                    // The listener may close before this call reaches the reactor.
                    let reason = if e.kind() == std::io::ErrorKind::NotFound {
                        ATOM_CLOSED
                    } else {
                        io_error_to_atom_id(&e)
                    };
                    let reason_atom = ErlTerm::atom(reason);
                    heap.alloc_tuple(&[error_atom, reason_atom]).0
                })
            }
        }
    }

    #[cfg(not(mio_supported))]
    {
        use crate::process::IoResult;
        submit_io_and_wait(ctx, Box::new(move || {
            match crate::socket::accept(handle) {
                Ok(stream_handle) => {
                    crate::socket::set_controlling_pid(stream_handle, self_pid);
                    IoResult::OkHandle(stream_handle)
                }
                Err(e) => IoResult::Err(e),
            }
        }))
    }
}

/// gen_tcp:recv/2 uses the same validated length and exact-read path as recv/3.
///
/// # Safety
/// Requires the context and live-term invariants of nera_rt_gen_tcp_recv_3.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_gen_tcp_recv(
    ctx: *mut ProcessContext,
    sock_term: u64,
    length_term: u64,
) -> u64 {
    nera_rt_gen_tcp_recv_3(ctx, sock_term, length_term, ErlTerm::nil().0)
}

/// gen_tcp:send(Sock, Data) -> ok | {error, Reason}
///
/// Extracts bytes from the process heap first (I/O threads/reactor can't
/// access the heap), then tries non-blocking write on mio platforms.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_gen_tcp_send(
    ctx: *mut ProcessContext,
    sock_term: u64,
    data_term: u64,
) -> u64 {
    let handle = crate::socket::SocketHandle::from_term(ErlTerm(sock_term));
    let data = ErlTerm(data_term);
    let self_pid = if !ctx.is_null() { unsafe { (*ctx).pid } } else { crate::process::ProcessId::new(0) };

    let Some(bytes) = iodata_to_bytes(data) else {
        return socket_error(ctx, crate::atom_table::ATOM_BADARG);
    };

    let Some(handle) = handle else { return socket_error(ctx, crate::atom_table::ATOM_ENOENT); };

    #[cfg(mio_supported)]
    {
        match crate::socket::try_send(handle, &bytes) {
            Ok(n) if n == bytes.len() => {
                // All bytes written immediately.
                ErlTerm::atom(ATOM_OK).0
            }
            Ok(n) => {
                // Partial write — reactor handles the rest.
                submit_to_reactor_and_wait(
                    ctx,
                    crate::reactor::PendingOp::Send {
                        pid: self_pid,
                        stream_handle: handle,
                        buf: bytes,
                        offset: n,
                    },
                )
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                submit_to_reactor_and_wait(
                    ctx,
                    crate::reactor::PendingOp::Send {
                        pid: self_pid,
                        stream_handle: handle,
                        buf: bytes,
                        offset: 0,
                    },
                )
            }
            Err(e) => {
                with_ctx_heap(ctx, |heap| {
                    let error_atom = ErlTerm::atom(ATOM_ERROR);
                    let reason_atom = ErlTerm::atom(io_error_to_atom_id(&e));
                    heap.alloc_tuple(&[error_atom, reason_atom]).0
                })
            }
        }
    }

    #[cfg(not(mio_supported))]
    {
        use crate::process::IoResult;
        submit_io_and_wait(ctx, Box::new(move || {
            match crate::socket::send(handle, &bytes) {
                Ok(()) => IoResult::Ok,
                Err(e) => IoResult::Err(e),
            }
        }))
    }
}

/// Extract a Rust String from an Erlang term.
///
/// Handles:
/// - Erlang strings (list of character codes)
/// - Atoms (e.g., `localhost`)
/// - Binaries (e.g., <<"127.0.0.1">>)
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn term_to_string(term: ErlTerm) -> Option<String> {
    // Atom (e.g., localhost)
    if let Some(atom_id) = term.as_atom_id() {
        return global_atom_table().resolve(atom_id);
    }
    // Binary
    if term.is_binary() {
        if let Some((ptr, len)) = unsafe { binary_data(term) } {
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
            return String::from_utf8(bytes.to_vec()).ok();
        }
    }
    // Tuple address: {A, B, C, D} → "A.B.C.D"
    if term.is_boxed() {
        if let Some(ptr) = term.as_boxed_ptr() {
            let header = unsafe { crate::term::BoxedHeader(*ptr) };
            if header.subtype() == crate::term::BOXED_TUPLE && header.arity() == 4 {
                let mut octets = [0u8; 4];
                let mut valid = true;
                for (i, item) in octets[..4].iter_mut().enumerate() {
                    let elem = unsafe { ErlTerm(*ptr.add(1 + i)) };
                    if let Some(v) = elem.as_small_int() {
                        if (0..=255).contains(&v) {
                            *item = v as u8;
                        } else {
                            valid = false;
                            break;
                        }
                    } else {
                        valid = false;
                        break;
                    }
                }
                if valid {
                    return Some(format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3]));
                }
            }
        }
    }
    // Erlang string (list of character codes)
    if term.is_nil() {
        return Some(String::new());
    }
    if term.is_cons() {
        let mut result = String::new();
        let mut cursor = term;
        while cursor.is_cons() {
            {
                let ptr = cursor.as_cons_ptr()?;
                let (head, tail) = unsafe { ((*ptr).head, (*ptr).tail) };
                {
                    let ch = head.as_small_int()?;
                    if (0..=127).contains(&ch) {
                        result.push(ch as u8 as char);
                    } else {
                        return None;
                    }
                }
                cursor = tail;
            }
        }
        if cursor.is_nil() {
            return Some(result);
        }
    }
    None
}

/// Parse socket options from an Erlang list.
///
/// Returns (active_mode, packet_mode). active_mode: None = not specified,
/// Some(mode) selects passive, once or continuous delivery.
/// # Safety
/// The complete input term graph must be initialized, live and unmoved by GC
/// until this call returns. Binary backing bytes must not be concurrently mutated.
unsafe fn parse_socket_opts(opts_term: ErlTerm) -> (Option<ActiveMode>, PacketMode) {
    let mut packet_mode = PacketMode::Raw;
    let mut active_mode: Option<ActiveMode> = None;
    let mut cursor = opts_term;
    while cursor.is_cons() {
        if let Some(cell_ptr) = cursor.as_cons_ptr() {
            let (elem, tail) = unsafe { ((*cell_ptr).head, (*cell_ptr).tail) };
            if elem.is_boxed() {
                if let Some(tptr) = elem.as_boxed_ptr() {
                    let header = unsafe { BoxedHeader(*tptr) };
                    if header.subtype() == BOXED_TUPLE && header.arity() == 2 {
                        let key = unsafe { ErlTerm(*tptr.add(1)) };
                        let val = unsafe { ErlTerm(*tptr.add(2)) };
                        if let Some(key_atom) = key.as_atom_id() {
                            let key_name = global_atom_table().resolve(key_atom);
                            match key_name.as_deref() {
                                Some("packet") => {
                                    if let Some(val_atom) = val.as_atom_id() {
                                        let val_name = global_atom_table().resolve(val_atom);
                                        match val_name.as_deref() {
                                            Some("line") => packet_mode = PacketMode::Line,
                                            Some("raw") => packet_mode = PacketMode::Raw,
                                            _ => {}
                                        }
                                    } else if let Some(int_val) = val.as_small_int() {
                                        if int_val == 0 { packet_mode = PacketMode::Raw; }
                                    }
                                }
                                Some("active") => {
                                    if let Some(val_atom) = val.as_atom_id() {
                                        let val_name = global_atom_table().resolve(val_atom);
                                        match val_name.as_deref() {
                                            Some("false") => active_mode = Some(ActiveMode::Passive),
                                            Some("once") => active_mode = Some(ActiveMode::Once),
                                            Some("true") => active_mode = Some(ActiveMode::Active),
                                            _ => {}
                                        }
                                    } else if let Some(int_val) = val.as_small_int() {
                                        if int_val == 0 { active_mode = Some(ActiveMode::Passive); }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            cursor = tail;
        } else {
            break;
        }
    }
    (active_mode, packet_mode)
}

/// gen_tcp:connect(Address, Port, Options) -> {ok, Handle} | {error, Reason}
///
/// Connects to a remote TCP endpoint. The address can be an Erlang string
/// (list of char codes), a binary, or an atom. Options are the same as
/// listen: [{active, false}, {packet, raw|line}].
///
/// Submits the blocking connect to the I/O pool so the scheduler thread
/// is not blocked.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_gen_tcp_connect(
    ctx: *mut ProcessContext,
    addr_term: u64,
    port_term: u64,
    opts_term: u64,
) -> u64 {
    let Some(port) = tcp_port(ErlTerm(port_term)) else {
        return raise_badarg(ctx);
    };
    let self_pid = if !ctx.is_null() { unsafe { (*ctx).pid } } else { crate::process::ProcessId::new(0) };

    let addr_str = match term_to_string(ErlTerm(addr_term)) {
        Some(s) => s,
        None => return raise_badarg(ctx),
    };

    let (_active_mode, packet_mode) = parse_socket_opts(ErlTerm(opts_term));

    // Submit blocking connect to the I/O pool.
    use crate::process::IoResult;
    submit_io_and_wait(ctx, Box::new(move || {
        match crate::socket::connect(&addr_str, port) {
            Ok(stream_handle) => {
                crate::socket::set_controlling_pid(stream_handle, self_pid);
                if packet_mode != PacketMode::Raw {
                    crate::socket::set_packet(stream_handle, packet_mode);
                }
                IoResult::OkHandle(stream_handle)
            }
            Err(e) => IoResult::Err(e),
        }
    }))
}

/// inet:port(Socket) -> {ok, Port} | {error, badarg}
///
/// Returns the local port number bound to the socket.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_inet_port(
    ctx: *mut ProcessContext,
    sock_term: u64,
) -> u64 {
    let Some(handle) = crate::socket::SocketHandle::from_term(ErlTerm(sock_term)) else {
        return socket_error(ctx, crate::atom_table::ATOM_BADARG);
    };

    match crate::socket::local_port(handle) {
        Some(port) => {
            with_ctx_heap(ctx, |heap| {
                let ok_atom = ErlTerm::atom(ATOM_OK);
                let port_term = ErlTerm::small_int(port as i64);
                heap.alloc_tuple(&[ok_atom, port_term]).0
            })
        }
        None => {
            with_ctx_heap(ctx, |heap| {
                let error_atom = ErlTerm::atom(ATOM_ERROR);
                let reason_atom = ErlTerm::atom(ATOM_BADARG);
                heap.alloc_tuple(&[error_atom, reason_atom]).0
            })
        }
    }
}

/// gen_tcp:close(Sock) -> ok
///
/// Closes a socket and removes it from the registry.
/// Always returns the atom `ok`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_gen_tcp_close(
    _ctx: *mut ProcessContext,
    sock_term: u64,
) -> u64 {
    let Some(handle) = crate::socket::SocketHandle::from_term(ErlTerm(sock_term)) else {
        return ErlTerm::atom(ATOM_OK).0;
    };
    crate::socket::remove(handle);
    ErlTerm::atom(ATOM_OK).0
}

/// gen_tcp:recv/3 — receive data from a socket with timeout (ms).
/// Timeout is currently ignored (acts like infinity).
/// If length > 0, reads exactly that many bytes. If length == 0, reads
/// any available data (same as recv/2).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_gen_tcp_recv_3(
    ctx: *mut ProcessContext,
    sock_term: u64,
    length_term: u64,
    _timeout_term: u64,
) -> u64 {
    let Some(handle) = crate::socket::SocketHandle::from_term(ErlTerm(sock_term)) else {
        return socket_error(ctx, crate::atom_table::ATOM_ENOENT);
    };
    let Some(length) = recv_length(ErlTerm(length_term)) else {
        return raise_badarg(ctx);
    };
    let self_pid = if !ctx.is_null() { unsafe { (*ctx).pid } } else { crate::process::ProcessId::new(0) };

    if length > 0 {
        let len = length;

        #[cfg(mio_supported)]
        {
            // Try reading the exact amount non-blocking first.
            use std::io::Read;
            match crate::socket::get_stream(handle) {
                Ok(stream) => {
                    let mut buf = Vec::with_capacity(len.min(4096));
                    let mut tmp = [0u8; 4096];
                    let read_len = len.min(tmp.len());
                    match (&*stream).read(&mut tmp[..read_len]) {
                        Ok(0) => {
                            with_ctx_heap(ctx, |heap| {
                                let error_atom = ErlTerm::atom(ATOM_ERROR);
                                let closed_atom = ErlTerm::atom(
                                    global_atom_table().intern("closed"),
                                );
                                heap.alloc_tuple(&[error_atom, closed_atom]).0
                            })
                        }
                        Ok(n) => {
                            buf.extend_from_slice(&tmp[..n]);
                            if buf.len() >= len {
                                buf.truncate(len);
                                return with_ctx_heap(ctx, |heap| {
                                    let ok_atom = ErlTerm::atom(ATOM_OK);
                                    let binary = heap.alloc_binary_auto(&buf);
                                    heap.alloc_tuple(&[ok_atom, binary]).0
                                });
                            }
                            // Got some bytes but not enough — reactor
                            // will accumulate the rest.
                            submit_to_reactor_and_wait(
                                ctx,
                                crate::reactor::PendingOp::RecvExact {
                                    pid: self_pid,
                                    stream_handle: handle,
                                    target_len: len,
                                    buf,
                                },
                            )
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            submit_to_reactor_and_wait(
                                ctx,
                                crate::reactor::PendingOp::RecvExact {
                                    pid: self_pid,
                                    stream_handle: handle,
                                    target_len: len,
                                    buf: Vec::new(),
                                },
                            )
                        }
                        Err(e) => {
                            with_ctx_heap(ctx, |heap| {
                                let error_atom = ErlTerm::atom(ATOM_ERROR);
                                let reason_atom = ErlTerm::atom(io_error_to_atom_id(&e));
                                heap.alloc_tuple(&[error_atom, reason_atom]).0
                            })
                        }
                    }
                }
                Err(e) => {
                    with_ctx_heap(ctx, |heap| {
                        let error_atom = ErlTerm::atom(ATOM_ERROR);
                        let reason_atom = ErlTerm::atom(io_error_to_atom_id(&e));
                        heap.alloc_tuple(&[error_atom, reason_atom]).0
                    })
                }
            }
        }

        #[cfg(not(mio_supported))]
        {
            use crate::process::IoResult;
            return submit_io_and_wait(ctx, Box::new(move || {
                match crate::socket::recv_exact(handle, len) {
                    Ok(bytes) => IoResult::OkBytes(bytes),
                    Err(e) => IoResult::Err(e),
                }
            }));
        }
    } else {
        // length == 0: read any available data (same as recv/2)
        #[cfg(mio_supported)]
        {
            match crate::socket::try_recv(handle) {
                Ok(bytes) => {
                    with_ctx_heap(ctx, |heap| {
                        let ok_atom = ErlTerm::atom(ATOM_OK);
                        let binary = heap.alloc_binary_auto(&bytes);
                        heap.alloc_tuple(&[ok_atom, binary]).0
                    })
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    submit_to_reactor_and_wait(
                        ctx,
                        crate::reactor::PendingOp::Recv {
                            pid: self_pid,
                            stream_handle: handle,
                        },
                    )
                }
                Err(e) => {
                    with_ctx_heap(ctx, |heap| {
                        let error_atom = ErlTerm::atom(ATOM_ERROR);
                        let reason_atom = ErlTerm::atom(io_error_to_atom_id(&e));
                        heap.alloc_tuple(&[error_atom, reason_atom]).0
                    })
                }
            }
        }

        #[cfg(not(mio_supported))]
        {
            use crate::process::IoResult;
            return submit_io_and_wait(ctx, Box::new(move || {
                match crate::socket::recv(handle) {
                    Ok(bytes) => IoResult::OkBytes(bytes),
                    Err(e) => IoResult::Err(e),
                }
            }));
        }
    }
}

/// gen_tcp:controlling_process/2 — change the controlling process of a socket.
/// Updates the socket's controlling_pid so active-mode messages are
/// delivered to the new owner.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_gen_tcp_controlling_process(
    _ctx: *mut ProcessContext,
    sock_term: u64,
    pid_term: u64,
) -> u64 {
    let Some(handle) = crate::socket::SocketHandle::from_term(ErlTerm(sock_term)) else {
        return ErlTerm::atom(ATOM_OK).0;
    };
    let pid = ErlTerm(pid_term).as_pid_id().unwrap_or(crate::process::ProcessId::new(0));
    crate::socket::set_controlling_pid(handle, pid);
    ErlTerm::atom(ATOM_OK).0
}

// ---------------------------------------------------------------------------
// Inline-read helpers for setopts fast path (mio platforms only)
// ---------------------------------------------------------------------------

/// Deliver {tcp, Socket, Data} directly to the calling process's mailbox.
/// Allocates on the process heap — no MessageFragment, no cross-thread copy.
#[cfg(mio_supported)]
/// # Safety
/// Requires the context/heap invariants of `with_ctx_heap` and live input terms.
/// The non-null context must also provide exclusive access to its mailbox.
unsafe fn deliver_tcp_data_to_self(ctx: *mut ProcessContext, handle: crate::socket::SocketHandle, bytes: &[u8]) {
    use crate::atom_table::global_atom_table;

    let tcp_atom_id = global_atom_table().intern("tcp");
    with_ctx_heap(ctx, |heap| {
        let data_bin = heap.alloc_binary_auto(bytes);
        let sock_term = handle.to_term();
        let msg = heap.alloc_tuple(&[
            ErlTerm::atom(tcp_atom_id),
            sock_term,
            data_bin,
        ]);
        unsafe {
            let mailbox_ptr = (*ctx).mailbox_ptr;
            if !mailbox_ptr.is_null() {
                (*mailbox_ptr).send(msg);
            }
        }
    });
}

/// Deliver {tcp_closed, Socket} directly to the calling process's mailbox.
#[cfg(mio_supported)]
/// # Safety
/// Requires the context/heap invariants of `with_ctx_heap` and live input terms.
/// The non-null context must also provide exclusive access to its mailbox.
unsafe fn deliver_tcp_closed_to_self(ctx: *mut ProcessContext, handle: crate::socket::SocketHandle) {
    use crate::atom_table::global_atom_table;

    let tcp_closed_atom_id = global_atom_table().intern("tcp_closed");
    with_ctx_heap(ctx, |heap| {
        let sock_term = handle.to_term();
        let msg = heap.alloc_tuple(&[
            ErlTerm::atom(tcp_closed_atom_id),
            sock_term,
        ]);
        unsafe {
            let mailbox_ptr = (*ctx).mailbox_ptr;
            if !mailbox_ptr.is_null() {
                (*mailbox_ptr).send(msg);
            }
        }
    });
}

/// Deliver {tcp_error, Socket, Reason} directly to the calling process's mailbox.
#[cfg(mio_supported)]
/// # Safety
/// Requires the context/heap invariants of `with_ctx_heap` and live input terms.
/// The non-null context must also provide exclusive access to its mailbox.
unsafe fn deliver_tcp_error_to_self(ctx: *mut ProcessContext, handle: crate::socket::SocketHandle, error: std::io::Error) {
    use crate::atom_table::global_atom_table;

    let tcp_error_atom_id = global_atom_table().intern("tcp_error");
    with_ctx_heap(ctx, |heap| {
        let sock_term = handle.to_term();
        let reason_str = format!("{}", error);
        let reason_bin = heap.alloc_binary_auto(reason_str.as_bytes());
        let msg = heap.alloc_tuple(&[
            ErlTerm::atom(tcp_error_atom_id),
            sock_term,
            reason_bin,
        ]);
        unsafe {
            let mailbox_ptr = (*ctx).mailbox_ptr;
            if !mailbox_ptr.is_null() {
                (*mailbox_ptr).send(msg);
            }
        }
    });
}

/// inet:setopts/2 — set socket options.
/// Supports {active, once|true|false}, {packet, line|raw|0}.
/// When active mode is set to `once` or `true`, spawns an OS thread to
/// read one packet/line from the socket and deliver it as a
/// `{tcp, Socket, Data}` message to the controlling process.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_inet_setopts(
    ctx: *mut ProcessContext,
    sock_term: u64,
    opts_term: u64,
) -> u64 {
    let Some(handle) = crate::socket::SocketHandle::from_term(ErlTerm(sock_term)) else {
        return ErlTerm::atom(ATOM_OK).0;
    };

    // Parse the options list: [{active, once}, {packet, line}, ...]
    let mut new_active: Option<ActiveMode> = None;
    let mut new_packet: Option<PacketMode> = None;
    let mut cursor = ErlTerm(opts_term);

    while cursor.is_cons() {
        if let Some(cell_ptr) = cursor.as_cons_ptr() {
            let (elem, tail) = unsafe { ((*cell_ptr).head, (*cell_ptr).tail) };
            // Each element should be a 2-tuple {Key, Value}
            if elem.is_boxed() {
                if let Some(tptr) = elem.as_boxed_ptr() {
                    let header = unsafe { BoxedHeader(*tptr) };
                    if header.subtype() == BOXED_TUPLE && header.arity() == 2 {
                        let key = unsafe { ErlTerm(*tptr.add(1)) };
                        let val = unsafe { ErlTerm(*tptr.add(2)) };

                        if let Some(key_atom) = key.as_atom_id() {
                            let key_name = global_atom_table().resolve(key_atom);
                            match key_name.as_deref() {
                                Some("active") => {
                                    if let Some(val_atom) = val.as_atom_id() {
                                        let val_name = global_atom_table().resolve(val_atom);
                                        match val_name.as_deref() {
                                            Some("once") => new_active = Some(ActiveMode::Once),
                                            Some("true") => new_active = Some(ActiveMode::Active),
                                            Some("false") => new_active = Some(ActiveMode::Passive),
                                            _ => {}
                                        }
                                    } else if let Some(int_val) = val.as_small_int() {
                                        // {active, false} can also be passed as integer 0
                                        if int_val == 0 {
                                            new_active = Some(ActiveMode::Passive);
                                        }
                                    }
                                }
                                Some("packet") => {
                                    if let Some(val_atom) = val.as_atom_id() {
                                        let val_name = global_atom_table().resolve(val_atom);
                                        match val_name.as_deref() {
                                            Some("line") => new_packet = Some(PacketMode::Line),
                                            Some("raw") => new_packet = Some(PacketMode::Raw),
                                            _ => {}
                                        }
                                    } else if let Some(int_val) = val.as_small_int() {
                                        // {packet, 0} means raw
                                        if int_val == 0 {
                                            new_packet = Some(PacketMode::Raw);
                                        }
                                    }
                                }
                                _ => {
                                    // Unknown option — ignore for forward compatibility
                                }
                            }
                        }
                    }
                }
            }
            cursor = tail;
        } else {
            break;
        }
    }

    // Apply options — combine packet + passive-active into one per-socket lock
    let need_deregister = if new_packet.is_some() || matches!(new_active, Some(ActiveMode::Passive)) {
        crate::socket::with_entry_opts(handle, |opts| {
            if let Some(p) = new_packet {
                opts.packet = p;
            }
            if matches!(new_active, Some(ActiveMode::Passive)) {
                opts.active = ActiveMode::Passive;
                if opts.reactor_registered {
                    opts.reactor_registered = false;
                    return true;
                }
            }
            false
        }).unwrap_or(false)
    } else {
        false
    };
    // For active modes (once=1, true=2), defer set_active until AFTER
    // the fastpath inline read. Otherwise the reactor sees active>0 and
    // may read from the same fd concurrently with the worker fastpath,
    // causing a data race on the kernel socket buffer.
    #[cfg(mio_supported)]
    if need_deregister {
        if let Some(reactor) = crate::reactor::global_reactor() {
            reactor.send_command(crate::reactor::ReactorCommand::DeregisterActive { handle });
        }
    }

    // If active mode was turned on, set controlling_pid to current process
    // (if not already set) and spawn reader thread.
    if let Some(active) = new_active {
        if active == ActiveMode::Once || active == ActiveMode::Active {
            let self_pid = if !ctx.is_null() {
                unsafe { (*ctx).pid }
            } else {
                crate::process::ProcessId::new(0)
            };

            // Read opts + set controlling pid in a single per-socket lock
            let (controlling_pid, packet_mode) = crate::socket::with_entry_opts(handle, |opts| {
                let pid = if opts.controlling_pid.get() != 0 {
                    opts.controlling_pid
                } else {
                    opts.controlling_pid = self_pid;
                    self_pid
                };
                (pid, new_packet.unwrap_or(opts.packet))
            }).unwrap_or((self_pid, new_packet.unwrap_or(PacketMode::Raw)));

            // On mio platforms, try an inline non-blocking read on the worker
            // thread first (fast path). If data is immediately available, we
            // deliver {tcp, Socket, Data} directly to the process mailbox —
            // no reactor round-trip, no MessageFragment, no cross-thread copy.
            // On WouldBlock, fall through to reactor registration (slow path).
            // On non-mio platforms, spawn an OS thread per socket.
            #[cfg(mio_supported)]
            {
                use std::os::unix::io::AsRawFd;

                let mut fastpath_done = false;

                if let Ok(stream) = crate::socket::get_stream(handle) {
                    let fd = stream.as_raw_fd();

                    if packet_mode == PacketMode::Line {
                        // Line mode: peek for newline. If a complete line
                        // is found, consume and deliver it inline (fast path).
                        // If only partial data exists, consume it into the
                        // per-socket line buffer so the kernel buffer drains
                        // (preventing edge-triggered kqueue spin when the
                        // reactor registers the fd).
                        let mut peek_buf = [0u8; 4096];
                        match crate::socket::peek_recv_fd(fd, &mut peek_buf) {
                            Ok(0) => {
                                // Connection closed — fast path, no reactor involved
                                deliver_tcp_closed_to_self(ctx, handle);
                                crate::socket::set_active_local(handle, ActiveMode::Passive);
                                fastpath_done = true;
                            }
                            Ok(n) => {
                                if let Some(pos) = peek_buf[..n].iter().position(|&b| b == b'\n') {
                                    // Complete line available — consume and deliver
                                    if crate::socket::consume_exact_fd(fd, &mut peek_buf[..pos + 1]).is_ok() {
                                        let end = if pos > 0 && peek_buf[pos - 1] == b'\r' { pos - 1 } else { pos };
                                        crate::reactor::COUNTER_SETOPTS_FASTPATH.fetch_add(
                                            1, std::sync::atomic::Ordering::Relaxed,
                                        );
                                        deliver_tcp_data_to_self(ctx, handle, &peek_buf[..end]);
                                        if active == ActiveMode::Once {
                                            // Fast path once — no reactor involved
                                            crate::socket::set_active_local(handle, ActiveMode::Passive);
                                        } else {
                                            // Continuous — set active and register for persistent readiness
                                            crate::socket::set_active_local(handle, active);
                                            crate::socket::mark_reactor_registered(handle, true);
                                            if let Some(reactor) = crate::reactor::global_reactor() {
                                                reactor.send_command(crate::reactor::ReactorCommand::RegisterActive {
                                                    stream_handle: handle,
                                                    controlling_pid,
                                                });
                                            }
                                        }
                                        fastpath_done = true;
                                    }
                                    // consume_exact failed — fall through to reactor
                                } else {
                                    // No complete line — consume the partial data
                                    // into the line buffer so the kernel buffer
                                    // drains (avoids kqueue spin on re-register).
                                    let mut consume_buf = vec![0u8; n];
                                    let _ = crate::socket::consume_exact_fd(fd, &mut consume_buf);
                                    crate::socket::set_line_buffer(handle, peek_buf[..n].to_vec());
                                    // Register with reactor for more data.
                                    crate::socket::set_active_local(handle, active);
                                    crate::socket::mark_reactor_registered(handle, true);
                                    if let Some(reactor) = crate::reactor::global_reactor() {
                                        reactor.send_command(crate::reactor::ReactorCommand::RegisterActive {
                                            stream_handle: handle,
                                            controlling_pid,
                                        });
                                    }
                                    fastpath_done = true;
                                }
                            }
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                // No data — fall through to reactor registration
                            }
                            Err(e) => {
                                // Error — fast path, no reactor involved
                                deliver_tcp_error_to_self(ctx, handle, e);
                                crate::socket::set_active_local(handle, ActiveMode::Passive);
                                fastpath_done = true;
                            }
                        }
                    } else {
                        // Raw mode: try single read
                        let mut read_buf = [0u8; 4096];
                        match crate::socket::consume_fd(fd, &mut read_buf) {
                            Ok(0) => {
                                // Connection closed — fast path, no reactor involved
                                deliver_tcp_closed_to_self(ctx, handle);
                                crate::socket::set_active_local(handle, ActiveMode::Passive);
                                fastpath_done = true;
                            }
                            Ok(n) => {
                                crate::reactor::COUNTER_SETOPTS_FASTPATH.fetch_add(
                                    1, std::sync::atomic::Ordering::Relaxed,
                                );
                                deliver_tcp_data_to_self(ctx, handle, &read_buf[..n]);
                                if active == ActiveMode::Once {
                                    // Fast path once — no reactor involved
                                    crate::socket::set_active_local(handle, ActiveMode::Passive);
                                } else {
                                    // Continuous — set active and register for persistent readiness
                                    crate::socket::set_active_local(handle, active);
                                    crate::socket::mark_reactor_registered(handle, true);
                                    if let Some(reactor) = crate::reactor::global_reactor() {
                                        reactor.send_command(crate::reactor::ReactorCommand::RegisterActive {
                                            stream_handle: handle,
                                            controlling_pid,
                                        });
                                    }
                                }
                                fastpath_done = true;
                            }
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                // No data — fall through to reactor registration
                            }
                            Err(ref e) if e.kind() == std::io::ErrorKind::ConnectionAborted
                                || e.kind() == std::io::ErrorKind::ConnectionReset =>
                            {
                                // Connection closed — fast path, no reactor involved
                                deliver_tcp_closed_to_self(ctx, handle);
                                crate::socket::set_active_local(handle, ActiveMode::Passive);
                                fastpath_done = true;
                            }
                            Err(e) => {
                                // Error — fast path, no reactor involved
                                deliver_tcp_error_to_self(ctx, handle, e);
                                crate::socket::set_active_local(handle, ActiveMode::Passive);
                                fastpath_done = true;
                            }
                        }
                    }
                }

                // Slow path: register with reactor for async readiness notification.
                // Set active NOW — after the fastpath read has completed, so
                // the reactor won't race with the worker on the same fd.
                if !fastpath_done {
                    crate::socket::set_active_local(handle, active);
                    crate::socket::mark_reactor_registered(handle, true);
                    if let Some(reactor) = crate::reactor::global_reactor() {
                        reactor.send_command(crate::reactor::ReactorCommand::RegisterActive {
                            stream_handle: handle,
                            controlling_pid,
                        });
                    }
                }
            }
            #[cfg(not(mio_supported))]
            {
                spawn_active_reader(handle, controlling_pid, active, packet_mode);
            }
        }
    }

    ErlTerm::atom(ATOM_OK).0
}

#[cfg(test)]
mod argument_tests {
    use super::*;

    #[test]
    fn ports_require_an_integer_in_the_u16_domain() {
        for value in [0, 1, 65535] {
            assert_eq!(tcp_port(ErlTerm::small_int(value)), Some(value as u16));
        }
        for value in [-1, 65536, crate::term::SMALL_INT_MAX] {
            assert_eq!(tcp_port(ErlTerm::small_int(value)), None);
        }
        assert_eq!(tcp_port(ErlTerm::nil()), None);
    }

    #[test]
    fn receive_lengths_require_nonnegative_representable_integers() {
        for value in [0, 1, 4096] {
            assert_eq!(recv_length(ErlTerm::small_int(value)), Some(value as usize));
        }
        assert_eq!(recv_length(ErlTerm::small_int(-1)), None);
        assert_eq!(recv_length(ErlTerm::nil()), None);
        let maximum = usize::try_from(crate::term::SMALL_INT_MAX).ok()
            .filter(|&n| n <= isize::MAX as usize);
        assert_eq!(recv_length(ErlTerm::small_int(crate::term::SMALL_INT_MAX)), maximum);
    }
}
