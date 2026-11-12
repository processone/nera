//! Selective receive protocol, resume state, and thread-local continuations.

#![allow(unused_unsafe)]

use std::cell::RefCell;
use crate::process::{ProcessContext, STATUS_RECEIVE_WAIT};
use crate::term::{ErlTerm, NO_MESSAGE_VALUE};


// Thread-local storage for the resume continuation stack set by compiled code
// before returning from a receive-wait. This avoids deadlocks: the worker
// loop already holds the process lock, so the FFI function can't re-lock it.
// Instead, the FFI pushes to this thread-local, and `run_slice()` reads it
// after the compiled code returns.
//
// The stack is ordered innermost-first: index 0 is the function owning the
// actual `receive`, and subsequent entries are the callers that also need
// to be resumed (at their call-site resume points) once the inner function
// completes.
thread_local! {
    static RESUME_CONTINUATIONS: RefCell<Vec<crate::process::Continuation>> = const { RefCell::new(Vec::new()) };
    static PENDING_SAVED_VARS: RefCell<Vec<crate::term::ErlTerm>> = const { RefCell::new(Vec::new()) };
}

/// Read and take all pending resume continuations.
/// Called by `run_slice()` after compiled code returns.
pub fn take_pending_resumes() -> Vec<crate::process::Continuation> {
    RESUME_CONTINUATIONS.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

/// Peek at the first message in the mailbox without removing it.
/// Resets the scan position to 0 and returns the first candidate message,
/// or `NO_MESSAGE_VALUE` if the mailbox is empty. This is the
/// selective-receive entry point. The sentinel is not `[]`: `[]` is a message
/// like any other.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_receive_peek(ctx: *mut ProcessContext) -> u64 {
    if ctx.is_null() {
        return NO_MESSAGE_VALUE;
    }
    let mailbox_ptr = unsafe { (*ctx).mailbox_ptr };
    if mailbox_ptr.is_null() {
        return NO_MESSAGE_VALUE;
    }
    let mailbox = unsafe { &mut *mailbox_ptr };
    mailbox.reset_scan();
    if let Some((_idx, msg)) = mailbox.try_match_next() {
        msg.0
    } else {
        NO_MESSAGE_VALUE
    }
}

/// Advance to the next message in the mailbox without removing any.
/// Returns the next candidate, or `NO_MESSAGE_VALUE` if all messages have
/// been scanned. Used in the selective-receive scan loop when the current
/// message did not match any receive clause.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_receive_next(ctx: *mut ProcessContext) -> u64 {
    if ctx.is_null() {
        return NO_MESSAGE_VALUE;
    }
    let mailbox_ptr = unsafe { (*ctx).mailbox_ptr };
    if mailbox_ptr.is_null() {
        return NO_MESSAGE_VALUE;
    }
    let mailbox = unsafe { &mut *mailbox_ptr };
    if let Some((_idx, msg)) = mailbox.try_match_next() {
        msg.0
    } else {
        NO_MESSAGE_VALUE
    }
}

/// Accept (remove) the message most recently returned by peek/next.
/// Must be called after a successful pattern match in a receive clause.
/// Returns the removed message term.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_receive_accept(ctx: *mut ProcessContext) -> u64 {
    if ctx.is_null() {
        return ErlTerm::nil().0;
    }
    let mailbox_ptr = unsafe { (*ctx).mailbox_ptr };
    if mailbox_ptr.is_null() {
        return ErlTerm::nil().0;
    }
    let mailbox = unsafe { &mut *mailbox_ptr };
    let message = mailbox.accept_last();
    (*ctx).receive_timeout_deadline = 0;
    message.0
}

/// Signal that receive found no matching message; sets STATUS_RECEIVE_WAIT.
/// The compiled code should return after calling this so the scheduler
/// can put the process to sleep.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_receive_wait(ctx: *mut ProcessContext) -> u64 {
    if !ctx.is_null() {
        unsafe {
            (*ctx).status = STATUS_RECEIVE_WAIT;
        }
    }
    ErlTerm::nil().0
}

/// Erlang receive timeouts admit exactly infinity or 0..=u32::MAX milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReceiveTimeout {
    Infinite,
    After(u32),
}

impl ReceiveTimeout {
    #[inline]
    fn from_term(term: ErlTerm) -> Option<Self> {
        if let Some(ms) = term.as_small_int() {
            return u32::try_from(ms).ok().map(Self::After);
        }
        (term == ErlTerm::atom(crate::atom_table::ATOM_INFINITY)).then_some(Self::Infinite)
    }

    fn deadline_at(self, now: u64) -> u64 {
        match self {
            Self::Infinite => 0,
            Self::After(ms) => now.max(1).saturating_add(u64::from(ms)),
        }
    }
}

/// Validate the timeout before scanning any mailbox message.
///
/// # Safety
/// ctx must be null or a live, exclusively accessed ProcessContext satisfying
/// raise_error's trace invariants. This call must not race execution or GC.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_receive_set_timeout(ctx: *mut ProcessContext, timeout_term: u64) -> u64 {
    let Some(timeout) = ReceiveTimeout::from_term(ErlTerm(timeout_term)) else {
        return super::helpers::raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_TIMEOUT_VALUE).0);
    };
    if let Some(ctx) = ctx.as_mut() {
        let now = match timeout {
            ReceiveTimeout::Infinite => 0,
            ReceiveTimeout::After(_) => crate::timer::current_time_ms(),
        };
        ctx.receive_timeout_deadline = timeout.deadline_at(now);
    }
    ErlTerm::nil().0
}

/// Check whether the receive timeout has expired.
/// Returns the atom `true` if the deadline has passed, `false` otherwise.
/// Also clears the deadline when it has expired.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_receive_check_timeout(ctx: *mut ProcessContext) -> u64 {
    use crate::atom_table::{ATOM_TRUE, ATOM_FALSE};
    if ctx.is_null() {
        return ErlTerm::atom(ATOM_FALSE).0;
    }
    let deadline = unsafe { (*ctx).receive_timeout_deadline };
    if deadline == 0 {
        return ErlTerm::atom(ATOM_FALSE).0;
    }
    let now = crate::timer::current_time_ms();
    if now >= deadline {
        unsafe { (*ctx).receive_timeout_deadline = 0; }
        ErlTerm::atom(ATOM_TRUE).0
    } else {
        ErlTerm::atom(ATOM_FALSE).0
    }
}

/// Save variables that are live across the current receive point.
/// Called by compiled code before `receive_wait` so these values are
/// preserved in the continuation and restored when the process resumes.
///
/// `vars` points to an array of `nvars` u64 ErlTerm values.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_save_resume_vars(
    _ctx: *mut ProcessContext,
    vars: *const u64,
    nvars: u32,
) {
    let saved: Vec<crate::term::ErlTerm> = (0..nvars as usize)
        .map(|i| unsafe { crate::term::ErlTerm(*vars.add(i)) })
        .collect();
    PENDING_SAVED_VARS.with(|cell| {
        *cell.borrow_mut() = saved;
    });
}

/// Load a saved variable by index. Called by compiled code at a
/// recv_entry_block when resuming from receive-wait. The saved variables
/// were stored in the continuation and made available via a thread-local
/// by `run_slice()` before calling the resume function.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_load_resume_var(
    _ctx: *mut ProcessContext,
    index: u32,
) -> u64 {
    crate::process::RESUME_SAVED_VARS.with(|cell| {
        let vars = cell.borrow();
        if (index as usize) < vars.len() {
            vars[index as usize]
        } else {
            crate::term::ErlTerm::nil().0
        }
    })
}

/// Push a resume continuation onto the thread-local stack.
/// Called by compiled code before returning from a receive-wait so the
/// scheduler knows which function to re-call when the process is woken.
///
/// The innermost function (owning the `receive`) calls this first, then
/// each caller in the chain also pushes its own continuation. The stack
/// ends up ordered innermost-first.
///
/// `func` is an ErlFn-compatible wrapper function pointer.
/// `args` points to an array of `nargs` u64 ErlTerm values (the function's
/// current parameters).
///
/// Also picks up any pending saved variables set by `nera_rt_save_resume_vars`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_set_resume(
    _ctx: *mut ProcessContext,
    func: crate::process::ErlFn,
    args: *const u64,
    nargs: u32,
    receive_index: u32,
) {
    let arg_terms: Vec<crate::term::ErlTerm> = (0..nargs as usize)
        .map(|i| unsafe { crate::term::ErlTerm(*args.add(i)) })
        .collect();

    // Pick up any saved variables that were set before this call
    let saved_vars = PENDING_SAVED_VARS.with(|cell| {
        std::mem::take(&mut *cell.borrow_mut())
    });

    RESUME_CONTINUATIONS.with(|cell| {
        cell.borrow_mut().push(crate::process::Continuation {
            func,
            args: arg_terms,
            resume_receive_index: receive_index,
            saved_vars,
        });
    });
}


#[cfg(test)]
mod timeout_tests {
    use super::*;

    #[test]
    fn timeout_domain_and_deadline_sentinel() {
        use crate::atom_table::{ATOM_INFINITY, ATOM_TIMEOUT_VALUE};
        assert_eq!(ReceiveTimeout::from_term(ErlTerm::atom(ATOM_INFINITY)), Some(ReceiveTimeout::Infinite));
        for ms in [0, 1, u32::MAX] {
            assert_eq!(ReceiveTimeout::from_term(ErlTerm::small_int(i64::from(ms))), Some(ReceiveTimeout::After(ms)));
        }
        for ms in [-1, i64::from(u32::MAX) + 1, crate::term::SMALL_INT_MAX] {
            assert_eq!(ReceiveTimeout::from_term(ErlTerm::small_int(ms)), None);
        }
        assert_eq!(ReceiveTimeout::from_term(ErlTerm::nil()), None);
        assert_eq!(ReceiveTimeout::Infinite.deadline_at(100), 0);
        assert_eq!(ReceiveTimeout::After(0).deadline_at(0), 1);
        assert_eq!(ReceiveTimeout::After(50).deadline_at(100), 150);
        assert_eq!(ReceiveTimeout::After(u32::MAX).deadline_at(u64::MAX), u64::MAX);
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(0));
        ctx.receive_timeout_deadline = 123;
        // SAFETY: ctx is exclusively owned; default trace pointers are null.
        unsafe { nera_rt_receive_set_timeout(&mut ctx, ErlTerm::small_int(-1).0); }
        assert_eq!(ctx.receive_timeout_deadline, 123);
        assert_eq!(ctx.result, ErlTerm::atom(ATOM_TIMEOUT_VALUE).0);
    }
}
