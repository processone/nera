//! Shared helpers, thread-locals, and statics used by FFI submodules.

#![allow(unused_unsafe)]

/// Access the scheduler for one operation, without allowing its borrow to escape.
/// Falls back to the global scheduler for a null context or runtime pointer.
///
/// # Safety
/// A non-null `ctx` must be aligned and readable until its runtime pointer is
/// copied. A non-null runtime pointer must identify a live `SchedulerHandle`
/// whose owner keeps it alive and shared-accessible until `f` returns, including
/// during unwinding. The callback may reenter the runtime or collect: no borrow
/// of the context, process or heap is held here. Such reentrancy must not destroy
/// the scheduler while this operation is active.
#[inline(always)]
pub(crate) unsafe fn with_sched_from_ctx<R>(
    ctx: *mut crate::process::ProcessContext,
    f: impl for<'sched> FnOnce(Option<&'sched crate::scheduler::SchedulerHandle>) -> R,
) -> R {
    // Copy the pointer before calling user code; never retain a context reference.
    let rt = if ctx.is_null() {
        std::ptr::null()
    } else {
        unsafe { (*ctx).runtime }
    };
    let scheduler = if !rt.is_null() {
        // SAFETY: the caller keeps the scheduler live for the callback's scope.
        Some(unsafe { &*rt })
    } else {
        crate::scheduler::global_scheduler().map(|arc| arc.as_ref())
    };
    f(scheduler)
}

use crate::binary::BinaryData;
use std::cell::RefCell;
use std::sync::atomic::AtomicU64;
use std::sync::OnceLock;
use std::time::Instant;

use crate::atom_table::{
    global_atom_table, ATOM_BADARG, ATOM_BADARITH,
    ATOM_EACCES, ATOM_EADDRINUSE, ATOM_EADDRNOTAVAIL,
    ATOM_ECONNREFUSED, ATOM_ECONNRESET, ATOM_ECONNABORTED,
    ATOM_ENOTCONN, ATOM_EPIPE, ATOM_ETIMEDOUT, ATOM_ENOENT, ATOM_EINVAL,
    ATOM_EEXIST, ATOM_EISDIR, ATOM_ENOSPC, ATOM_EMFILE, ATOM_ENFILE,
};
use crate::heap::ProcessHeap;
use crate::io_pool::IoWork;
use crate::process::{ProcessContext, STATUS_IO_WAIT};
use crate::term::{
    BoxedHeader, ErlTerm, BOXED_BINARY, BOXED_FLOAT, BOXED_REFC_BINARY,
    REFC_BINARY_OFFSET_WORD_INDEX, REFC_BINARY_PTR_WORD_INDEX,
};

// ---------------------------------------------------------------------------
// Thread-local and static state
// ---------------------------------------------------------------------------

thread_local! {
    pub(super) static PROCESS_HEAP: RefCell<ProcessHeap> = RefCell::new(ProcessHeap::new());
}

/// Global counter for unique reference IDs.
pub(super) static NEXT_REF_ID: AtomicU64 = AtomicU64::new(1);

/// Monotonic clock epoch — initialized on first call to monotonic_time.
pub(super) static MONOTONIC_START: OnceLock<Instant> = OnceLock::new();

// ---------------------------------------------------------------------------
// Core helpers
// ---------------------------------------------------------------------------

/// Returns nanoseconds elapsed since the monotonic epoch.
pub(super) fn monotonic_nanos() -> i64 {
    let start = MONOTONIC_START.get_or_init(Instant::now);
    start.elapsed().as_nanos() as i64
}

/// Borrow the process heap for one non-reentrant operation.
///
/// # Safety
/// `ctx` is null or points to a live, aligned context owned by this thread.
/// Its non-null `heap_ptr` must point to a live, exclusively accessible heap.
/// The callback must not access that heap through another pointer, invoke
/// compiled code, or run GC through the context. Allocation does not collect.
/// Terms returned by the callback are unrooted and must be rooted before GC.
/// No context reference is retained while the callback runs.
#[inline]
pub(super) unsafe fn with_ctx_heap<F, R>(ctx: *mut ProcessContext, f: F) -> R
where
    F: for<'heap> FnOnce(&'heap mut ProcessHeap) -> R,
{
    let heap = if ctx.is_null() {
        None
    } else {
        // SAFETY: the caller guarantees context validity and exclusive heap access.
        unsafe { (*ctx).heap_ptr.as_mut() }
    };
    with_heap(heap, f)
}

/// Encode a signed machine integer, allocating only outside the immediate range.
///
/// # Safety
/// The context must satisfy `with_ctx_heap`'s exclusive heap-access contract.
#[inline]
pub(super) unsafe fn integer_term(ctx: *mut ProcessContext, value: i64) -> u64 {
    if let Some(term) = ErlTerm::try_small_int(value) {
        term.0
    } else {
        with_ctx_heap(ctx, |heap| crate::bigint::bigint_to_term(heap, value.into()).0)
    }
}

/// Reference-based implementation; the callback cannot return a heap borrow.
#[inline]
fn with_heap<F, R>(heap: Option<&mut ProcessHeap>, f: F) -> R
where
    F: for<'heap> FnOnce(&'heap mut ProcessHeap) -> R,
{
    match heap {
        Some(heap) => f(heap),
        None => PROCESS_HEAP.with(|cell| f(&mut cell.borrow_mut())),
    }
}

// ---------------------------------------------------------------------------
// Error-raising helpers
// ---------------------------------------------------------------------------

/// Snapshot the current call stack into `ctx.exception_trace_ptr`.
/// Reads compact frames from the cursor-based stack and reconstructs
/// full StackFrames using the global func_info_table. Falls back to
/// stack_ptr (Vec<StackFrame>) for tests and legacy callers. Captured traces
/// are innermost-first, like Erlang stacktrace lists supplied to raise/3.
/// # Safety
/// The context and the pointees accessed by this operation must be live and
/// exclusively accessible on this thread; no competing process execution or
/// GC may run during the call. Null is allowed only for error helpers.
/// Stack bounds must describe initialized contiguous frames in one allocation.
/// Stack storage and the writable trace must be disjoint from each other and
/// the context. A null context, trace, or legacy stack is supported.
pub(super) unsafe fn capture_exception_trace(ctx: *mut ProcessContext) {
    let Some(ctx) = (unsafe { ctx.as_ref() }) else {
        return;
    };
    let Some(trace) = (unsafe { ctx.exception_trace_ptr.as_mut() }) else {
        return;
    };
    let base = ctx.stack_base;
    let cursor = ctx.stack_cursor;
    let frames = if !base.is_null() && cursor > base {
        let depth =
            (cursor as usize - base as usize) / std::mem::size_of::<crate::process::CompactFrame>();
        // SAFETY: the FFI contract guarantees a valid stack range, disjoint
        // from the exclusively borrowed exception trace.
        TraceFrames::Compact(unsafe { std::slice::from_raw_parts(base, depth) })
    } else {
        TraceFrames::Full(unsafe { ctx.stack_ptr.as_ref() }.map_or(&[], |stack| stack.as_slice()))
    };
    fill_exception_trace(trace, frames);
}

enum TraceFrames<'a> {
    Compact(&'a [crate::process::CompactFrame]),
    Full(&'a [crate::process::StackFrame]),
}

fn fill_exception_trace(trace: &mut Vec<crate::process::StackFrame>, frames: TraceFrames<'_>) {
    trace.clear();
    match frames {
        TraceFrames::Full(frames) => trace.extend(frames.iter().rev().copied()),
        TraceFrames::Compact(frames) => {
            let info_table = crate::entry::global_func_info_table();
            trace.extend(frames.iter().rev().map(|cf| {
                let (module, function, arity) = info_table
                    .get(cf.func_index as usize)
                    .copied()
                    .unwrap_or((0, 0, 0));
                crate::process::StackFrame {
                    module,
                    function,
                    arity,
                    line: cf.line,
                }
            }));
        }
    }
}

/// Helper: raise an error exception and return nil.
/// Used by FFI functions when they encounter bad arguments.
/// # Safety
/// The context and the pointees accessed by this operation must be live and
/// exclusively accessible on this thread; no competing process execution or
/// GC may run during the call. Null is allowed only for error helpers.
pub(super) unsafe fn raise_error(ctx: *mut ProcessContext, reason: u64) -> u64 {
    unsafe { capture_exception_trace(ctx) };
    if let Some(ctx) = unsafe { ctx.as_mut() } {
        set_error(ctx, ErlTerm::from_raw(reason));
    }
    ErlTerm::nil().to_raw()
}

fn set_error(ctx: &mut ProcessContext, reason: ErlTerm) {
    ctx.status = crate::process::STATUS_EXCEPTION;
    ctx.result = reason.to_raw();
    ctx.exception_class = 1; // error
}

/// Raise a `badarith` error exception.
/// # Safety
/// The context and the pointees accessed by this operation must be live and
/// exclusively accessible on this thread; no competing process execution or
/// GC may run during the call. Null is allowed only for error helpers.
pub(super) unsafe fn raise_badarith(ctx: *mut ProcessContext) -> u64 {
    raise_error(ctx, ErlTerm::atom(ATOM_BADARITH).0)
}

/// Raise a `badarg` error exception.
/// # Safety
/// The context and the pointees accessed by this operation must be live and
/// exclusively accessible on this thread; no competing process execution or
/// GC may run during the call. Null is allowed only for error helpers.
pub(super) unsafe fn raise_badarg(ctx: *mut ProcessContext) -> u64 {
    raise_error(ctx, ErlTerm::atom(ATOM_BADARG).0)
}

// ---------------------------------------------------------------------------
// Boolean / atom helpers
// ---------------------------------------------------------------------------

pub(super) fn atom_undefined() -> u64 {
    ErlTerm::atom(global_atom_table().intern("undefined")).0
}

// ---------------------------------------------------------------------------
// Numeric helpers
// ---------------------------------------------------------------------------

/// Extract a float value from a boxed float term, or None.
/// # Safety
/// Referenced storage must have the runtime layout and remain live,
/// initialized, immutable and unmoved.
pub(super) unsafe fn term_as_float(term: ErlTerm) -> Option<f64> {
    if term.is_boxed() {
        unsafe {
            let ptr = term.as_boxed_ptr()?;
            let header = BoxedHeader(*ptr);
            if header.subtype() == BOXED_FLOAT {
                return Some(f64::from_bits(*ptr.add(1)));
            }
        }
    }
    None
}

/// Extract a numeric value from a term as f64.
/// Works for small integers, boxed floats, and bigints (lossy conversion).
/// # Safety
/// Referenced storage must have the runtime layout and remain live,
/// initialized, immutable and unmoved.
pub(super) unsafe fn extract_numeric(term: ErlTerm) -> Option<f64> {
    term.as_small_int()
        .map(|i| i as f64)
        .or_else(|| term_as_float(term))
        .or_else(|| crate::bigint::bigint_to_f64(term))
}

/// Check if a term is any numeric type (small int, bigint, or float).
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub(super) unsafe fn is_numeric(term: ErlTerm) -> bool {
    term.is_small_int() || term.is_bigint() || term_as_float(term).is_some()
}

// ---------------------------------------------------------------------------
// Binary data access
// ---------------------------------------------------------------------------

#[inline]
pub(super) fn is_binary_subtype(subtype: u64) -> bool {
    subtype == BOXED_BINARY || subtype == BOXED_REFC_BINARY
}

/// Access the raw data pointer and byte length of a binary term.
/// Works for both heap binaries (inline bytes) and refc binaries
/// (off-heap `Arc<BinaryData>` backing store).
///
/// # Safety
/// The caller must ensure the term's pointer is valid.  For refc
/// binaries the backing `Arc` must still be alive.
pub(crate) unsafe fn binary_data(term: ErlTerm) -> Option<(*const u8, usize)> {
    binary_data_bits(term).map(|(data, bits)| (data, bits.div_ceil(8)))
}

/// Copy the data pointer and exact logical bit length from one descriptor read.
/// # Safety
/// The term and any refc backing storage are live, initialized runtime objects,
/// immutable and unmoved while these raw parts are consumed. Mutation additionally
/// requires exclusive access to the backing bytes and no outstanding byte borrows.
#[inline(always)]
pub(super) unsafe fn binary_data_bits(term: ErlTerm) -> Option<(*const u8, usize)> {
    let ptr = term.as_boxed_ptr()?;
    let header = BoxedHeader(*ptr);
    let bits = header.arity() as usize;
    match header.subtype() {
        BOXED_BINARY => Some((ptr.add(1).cast(), bits)),
        BOXED_REFC_BINARY => {
            let backing = *ptr.add(REFC_BINARY_PTR_WORD_INDEX) as *const BinaryData;
            let offset = *ptr.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
            Some(((*backing).as_ptr().add(offset), bits))
        }
        _ => None,
    }
}

/// Public wrapper for `binary_data` for use by other runtime modules (e.g. ets).
///
/// # Safety
/// Caller must ensure the term pointer is valid.
pub unsafe fn binary_data_pub(term: ErlTerm) -> Option<(*const u8, usize)> {
    binary_data(term)
}

// ---------------------------------------------------------------------------
// I/O helpers
// ---------------------------------------------------------------------------

/// Copy validated iodata into an owned buffer for asynchronous I/O.
/// Only a binary or a list is permitted at the root; byte integers are leaves.
///
/// # Safety
/// The complete term graph must remain live and unmoved by GC for this call.
/// Binary backing bytes must not be concurrently mutated.
#[inline]
pub(super) unsafe fn iodata_to_bytes(term: ErlTerm) -> Option<Vec<u8>> {
    if term.is_binary() {
        return Some(term.binary_bytes_any()?.to_vec());
    }
    if !term.is_cons() && !term.is_nil() {
        return None;
    }
    let mut buf = Vec::new();
    flatten_iolist(term, &mut buf).then_some(buf)
}

/// Recursively flatten an Erlang iolist to bytes.
///
/// An iolist is either:
/// - a binary
/// - a byte integer (0–255)
/// - a list of iolists
///
/// Returns false if the term is not a valid iolist.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub(super) unsafe fn flatten_iolist(term: ErlTerm, buf: &mut Vec<u8>) -> bool {
    if term.is_nil() {
        return true;
    }
    if term.is_binary() {
        if let Some((ptr, len)) = unsafe { binary_data(term) } {
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
            buf.extend_from_slice(bytes);
            return true;
        }
        return false;
    }
    if let Some(val) = term.as_small_int() {
        if (0..=255).contains(&val) {
            buf.push(val as u8);
            return true;
        }
        return false;
    }
    if term.is_cons() {
        let mut cursor = term;
        while cursor.is_cons() {
            if let Some(ptr) = cursor.as_cons_ptr() {
                unsafe {
                    let head = (*ptr).head;
                    // Keep flat byte lists on a direct loop, without one
                    // recursive call per byte. Only nested data recurses.
                    if let Some(value) = head.as_small_int() {
                        let Ok(byte) = u8::try_from(value) else { return false; };
                        buf.push(byte);
                    } else if !flatten_iolist(head, buf) {
                        return false;
                    }
                    cursor = (*ptr).tail;
                }
            } else {
                return false;
            }
        }
        // Improper list tail: must be nil or binary
        if cursor.is_nil() {
            return true;
        }
        if cursor.is_binary() {
            return flatten_iolist(cursor, buf);
        }
        return false;
    }
    false
}

/// Map an `io::Error` to a well-known POSIX atom ID.
///
/// This avoids interning arbitrary error strings at runtime, which would
/// grow the atom table unboundedly. All reason atoms are pre-registered
/// builtins, matching Erlang's `inet` error reason convention.
pub(crate) fn io_error_to_atom_id(e: &std::io::Error) -> u32 {
    use std::io::ErrorKind;
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
            // Map raw OS error codes for cases not covered by ErrorKind.
            // EISDIR=21, ENFILE=23, EMFILE=24, ENOSPC=28 on both macOS and Linux.
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

/// Submit blocking I/O work to the I/O pool and set STATUS_IO_WAIT.
///
/// Returns nil (unused — the codegen call-site status check sees
/// STATUS_IO_WAIT and bails out, saving a resume continuation. When the
/// I/O completes, `io_pool::wake_process_with_result` converts the result
/// to an ErlTerm and stores it in `ctx.result`. The resume block loads
/// `ctx.result` into the call's result variable.)
/// # Safety
/// The context and the pointees accessed by this operation must be live and
/// exclusively accessible on this thread; no competing process execution or
/// GC may run during the call. Null is allowed only for error helpers.
pub(super) unsafe fn submit_io_and_wait(ctx: *mut ProcessContext, work: IoWork) -> u64 {
    let pid = unsafe { (*ctx).pid };
    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        if let Some(sched) = scheduler {
            sched.io_pool.submit(pid, work);
        }
        unsafe {
            (*ctx).status = STATUS_IO_WAIT;
        }
        ErlTerm::nil().0
    })
}

/// Submit a pending I/O operation to the mio reactor and set STATUS_IO_WAIT.
///
/// Same suspend/resume contract as `submit_io_and_wait`: the codegen
/// call-site sees STATUS_IO_WAIT, saves a resume continuation, and bails
/// out. When the reactor completes the operation, it calls
/// `wake_process_with_result` which stores the result in `ctx.result`
/// and re-enqueues the process.
#[cfg(mio_supported)]
/// # Safety
/// The context and the pointees accessed by this operation must be live and
/// exclusively accessible on this thread; no competing process execution or
/// GC may run during the call. Null is allowed only for error helpers.
pub(super) unsafe fn submit_to_reactor_and_wait(
    ctx: *mut ProcessContext,
    op: crate::reactor::PendingOp,
) -> u64 {
    if let Some(reactor) = crate::reactor::global_reactor() {
        reactor.register_op(op);
    }
    unsafe {
        (*ctx).status = STATUS_IO_WAIT;
    }
    ErlTerm::nil().0
}

/// Resolve a tail-call chain iteratively. Dispatch performed by this loop must
/// leave its own tail calls pending; ordinary calls made by compiled code must
/// resolve their nested chains before continuing, even while this loop is active.
pub(super) unsafe fn resolve_tail_calls(ctx: *mut ProcessContext) {
    use crate::process::STATUS_TAIL_CALL;
    if ctx.is_null() || (*ctx).status != STATUS_TAIL_CALL {
        return;
    }
    while (*ctx).status == STATUS_TAIL_CALL {
        (*ctx).status = crate::process::STATUS_OK;
        let kind = (*ctx).tail_call_kind;
        (*ctx).tail_call_kind = 0;
        match kind {
            1 => {
                let mod_atom = (*ctx).tail_call_mod;
                let fun_atom = (*ctx).tail_call_fun;
                let args_list = (*ctx).tail_call_arg1;
                let result = super::apply::apply_impl(ctx, mod_atom, fun_atom, args_list, false);
                if (*ctx).status == crate::process::STATUS_OK {
                    (*ctx).result = result;
                }
            }
            2 => {
                let closure = (*ctx).tail_call_arg1;
                let args_list = (*ctx).tail_call_arg2;
                let result = super::closure::tail_call_closure(ctx, closure, args_list);
                if (*ctx).status == crate::process::STATUS_OK {
                    (*ctx).result = result;
                }
            }
            _ => break,
        }
    }
}

/// FFI entry point for `resolve_tail_calls`, callable from codegen.
/// Used after local function calls that may have returned via the
/// tail-call trampoline (STATUS_TAIL_CALL).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_resolve_tail_calls(ctx: *mut ProcessContext) {
    resolve_tail_calls(ctx);
}

#[cfg(test)]
mod heap_boundary_tests {
    use super::*;

    #[test]
    fn fallback_borrow_is_released_on_unwind() {
        let result = std::panic::catch_unwind(|| {
            with_heap(None, |_| panic!("callback failed"));
        });
        assert!(result.is_err());
        with_heap(None, |heap| {
            heap.alloc_float(1.0);
        });
    }

    #[test]
    fn heap_is_resolved_again_after_context_changes() {
        let mut first = ProcessHeap::new();
        let mut second = ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut first;
        // SAFETY: each heap is exclusively owned; neither callback retains a borrow.
        unsafe {
            with_ctx_heap(&mut ctx, |heap| {
                heap.alloc_float(1.0);
            });
        }
        let first_bytes = first.bytes_allocated();
        ctx.heap_ptr = &mut second;
        unsafe {
            with_ctx_heap(&mut ctx, |heap| {
                heap.alloc_float(2.0);
            });
        }
        assert_eq!(first.bytes_allocated(), first_bytes);
        assert!(second.bytes_allocated() > 0);
    }
}

#[cfg(test)]
mod nested_tail_tests {
    use super::*;
    use crate::process::STATUS_TAIL_CALL;

    extern "C" fn final_value(ctx: *mut ProcessContext, _: *const u64, _: u32) {
        unsafe { (*ctx).result = ErlTerm::small_int(42).0; }
    }

    extern "C" fn continue_after_nested_call(ctx: *mut ProcessContext, args: *const u64, _: u32) {
        unsafe {
            (*ctx).tail_call_kind = 2;
            (*ctx).tail_call_arg1 = *args;
            (*ctx).tail_call_arg2 = ErlTerm::nil().0;
            (*ctx).status = STATUS_TAIL_CALL;
            resolve_tail_calls(ctx);
            let value = ErlTerm((*ctx).result).as_small_int().unwrap_or(-100);
            (*ctx).result = ErlTerm::small_int(value + 1).0;
        }
    }

    #[test]
    fn nested_tail_resolution_preserves_caller_continuation() {
        let mut heap = ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        unsafe {
            let inner = super::super::closure::nera_rt_alloc_closure(
                &mut ctx, final_value as *const () as u64, 0, std::ptr::null(), 0);
            let outer = super::super::closure::nera_rt_alloc_closure(
                &mut ctx, continue_after_nested_call as *const () as u64, 0, &inner, 1);
            ctx.tail_call_kind = 2;
            ctx.tail_call_arg1 = outer;
            ctx.tail_call_arg2 = ErlTerm::nil().0;
            ctx.status = STATUS_TAIL_CALL;
            resolve_tail_calls(&mut ctx);
        }
        assert_eq!(ErlTerm(ctx.result).as_small_int(), Some(43));
        assert_eq!(ctx.status, crate::process::STATUS_OK);
    }
}

#[cfg(test)]
mod scheduler_scope_tests {
    use super::*;
    use crate::scheduler::SchedulerHandle;
    use std::sync::atomic::{AtomicI64, AtomicU32};
    use std::sync::Arc;

    #[test]
    fn local_scheduler_allows_reentry_and_rejects_pid_exhaustion() {
        #[cfg(mio_supported)]
        let (reactor, reactor_thread) = crate::reactor::spawn_reactor();
        let owner = Arc::new(SchedulerHandle {
            injector: Arc::new(crossbeam_deque::Injector::new()),
            process_table: Arc::new(dashmap::DashMap::new()),
            next_pid: Arc::new(crate::process::ProcessIdAllocator::starting_at(crate::process::ProcessId::MAX + 1)),
            runnable_count: Arc::new(AtomicI64::new(0)),
            alive_count: Arc::new(AtomicI64::new(0)),
            registered_names: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
            timer_wheel: Arc::new(crate::timer::TimerWheel::new()),
            worker_notify: Arc::new((parking_lot::Mutex::new(()), parking_lot::Condvar::new())),
            io_pool: Arc::new(crate::io_pool::IoPool::new(1)),
            #[cfg(mio_supported)]
            reactor: reactor.clone(),
            worker_inboxes: Arc::new(Vec::new()),
            num_workers: 0,
            worker_threads: Arc::new(Vec::new()),
            dispatch_rr: AtomicU32::new(0),
        });
        let mut context = ProcessContext::new(crate::process::ProcessId::new(1));
        context.runtime = Arc::as_ptr(&owner);
        let ctx = &mut context as *mut ProcessContext;
        // SAFETY: the context is local; owner outlives both callbacks. No context
        // reference is retained while the raw pointer is used for reentry.
        unsafe {
            with_sched_from_ctx(ctx, |outer| {
                let outer = outer.unwrap();
                assert!(std::ptr::eq(outer, owner.as_ref()));
                (*ctx).result = 42;
                with_sched_from_ctx(ctx, |inner| {
                    assert!(std::ptr::eq(inner.unwrap(), outer));
                    (*ctx).result += 1;
                });
                assert_eq!((*ctx).result, 43);
                assert_eq!(Arc::strong_count(&owner), 1);
            });
        }
        assert_eq!(context.result, 43);
        unsafe extern "C" fn entry(_ctx: *mut ProcessContext, _args: *const u64, _nargs: u32) {}
        let mut heap = ProcessHeap::new();
        let words = heap.alloc_words(3);
        // SAFETY: initialize a zero-capture closure with a live zero-arity entry.
        unsafe {
            *words = BoxedHeader::new(crate::term::BoxedSubtype::Closure, 0).0;
            *words.add(1) = entry as *const () as u64;
            *words.add(2) = 0;
        }
        context.heap_ptr = &mut heap;
        let closure = ErlTerm::boxed(words);
        // SAFETY: context/heap/scheduler and compiled closure remain live;
        // no heap borrow crosses the call. Exhaustion must prevent publication.
        let result = unsafe { crate::ffi::process::nera_rt_spawn_closure(&mut context, closure.to_raw()) };
        assert_eq!(result, ErlTerm::nil().to_raw());
        assert_eq!(context.status, crate::process::ExecutionStatus::Exception);
        assert_eq!(context.result, ErlTerm::atom(crate::atom_table::ATOM_SYSTEM_LIMIT).to_raw());
        assert!(owner.process_table.is_empty());
        assert!(owner.injector.is_empty());
        assert_eq!(owner.alive_count.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(owner.runnable_count.load(std::sync::atomic::Ordering::Relaxed), 0);

        #[cfg(mio_supported)]
        {
            reactor.shutdown();
            reactor_thread.join().unwrap();
        }
    }

    #[test]
    fn null_context_and_runtime_preserve_global_fallback() {
        let mut context = ProcessContext::new(crate::process::ProcessId::new(1));
        let expected = crate::scheduler::global_scheduler().map(Arc::as_ptr);
        // SAFETY: null is supported; context is live and its runtime is null.
        unsafe {
            for result in [
                with_sched_from_ctx(std::ptr::null_mut(), |s| s.map(std::ptr::from_ref)),
                with_sched_from_ctx(&mut context, |s| s.map(std::ptr::from_ref)),
            ] {
                // Another scheduler test can initialize the singleton concurrently.
                if expected.is_some() {
                    assert_eq!(result, expected);
                } else if result.is_some() {
                    assert_eq!(result, crate::scheduler::global_scheduler().map(Arc::as_ptr));
                }
            }
        }
    }
}

#[cfg(test)]
mod iodata_tests {
    use super::*;
    use crate::heap::ProcessHeap;

    #[test]
    fn owned_iodata_checks_leaf_and_tail_domains() {
        let mut heap = ProcessHeap::new();
        let binary = heap.alloc_binary(b"AB");
        let tail = heap.alloc_cons(ErlTerm::small_int(255), binary);
        let nested = heap.alloc_cons(tail, ErlTerm::nil());
        let valid = heap.alloc_cons(ErlTerm::small_int(0), nested);
        // SAFETY: the heap owns the complete initialized graph; no GC occurs.
        unsafe {
            assert_eq!(iodata_to_bytes(valid), Some(vec![0, 255, 65, 66]));
            assert_eq!(iodata_to_bytes(binary), Some(b"AB".to_vec()));
            assert_eq!(iodata_to_bytes(ErlTerm::nil()), Some(vec![]));
            assert_eq!(iodata_to_bytes(ErlTerm::small_int(65)), None);
            for value in [-1, 256, crate::term::SMALL_INT_MAX] {
                let invalid = heap.alloc_cons(ErlTerm::small_int(value), ErlTerm::nil());
                assert_eq!(iodata_to_bytes(invalid), None);
            }
            let improper = heap.alloc_cons(ErlTerm::small_int(65), ErlTerm::small_int(7));
            assert_eq!(iodata_to_bytes(improper), None);
        }
    }
}
