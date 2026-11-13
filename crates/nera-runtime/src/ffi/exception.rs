//! Exception handling: throw, error, exit, stack tracing.

#![allow(unused_unsafe)]

use crate::atom_table::global_atom_table;
use crate::process::{ProcessContext, StackFrame};
use crate::term::ErlTerm;
use super::helpers::*;

/// Push one frame (module:function/arity) on the process call stack.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_stack_push(
    ctx: *mut ProcessContext,
    module_term: u64,
    function_term: u64,
    arity: u64,
    line: u64,
) {
    if ctx.is_null() {
        return;
    }
    let module_id = ErlTerm(module_term).as_atom_id().unwrap_or(0);
    let function_id = ErlTerm(function_term).as_atom_id().unwrap_or(0);
    unsafe {
        if (*ctx).stack_ptr.is_null() {
            return;
        }
        let stack = &mut *((*ctx).stack_ptr);
        if stack.len() >= crate::process::STACK_DEPTH_LIMIT {
            capture_exception_trace(ctx);
            (*ctx).status = crate::process::STATUS_EXCEPTION;
            (*ctx).result = ErlTerm::atom(crate::atom_table::ATOM_SYSTEM_LIMIT).0;
            (*ctx).exception_class = 1; // error
            return;
        }
        stack.push(StackFrame {
            module: module_id,
            function: function_id,
            arity: arity as u32,
            line: line as u32,
        });
    }
}

/// Update the line number of the top stack frame.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_stack_set_line(ctx: *mut ProcessContext, line: u64) {
    if ctx.is_null() {
        return;
    }
    unsafe {
        if (*ctx).stack_ptr.is_null() {
            return;
        }
        let stack = &mut *((*ctx).stack_ptr);
        if let Some(frame) = stack.last_mut() {
            frame.line = line as u32;
        }
    }
}

/// Pop one frame from the process call stack.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_stack_pop(ctx: *mut ProcessContext) {
    if ctx.is_null() {
        return;
    }
    unsafe {
        if (*ctx).stack_ptr.is_null() {
            return;
        }
        let stack = &mut *((*ctx).stack_ptr);
        let _ = stack.pop();
    }
}

/// Raise a throw exception. Sets ctx.status = EXCEPTION, stores reason and class.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_throw(ctx: *mut ProcessContext, reason: u64) -> u64 {
    capture_exception_trace(ctx);
    if !ctx.is_null() {
        unsafe {
            (*ctx).status = crate::process::STATUS_EXCEPTION;
            (*ctx).result = reason;
            (*ctx).exception_class = 0; // throw
        }
    }
    ErlTerm::nil().0
}

/// Raise an error exception.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_error(ctx: *mut ProcessContext, reason: u64) -> u64 {
    capture_exception_trace(ctx);
    if !ctx.is_null() {
        unsafe {
            (*ctx).status = crate::process::STATUS_EXCEPTION;
            (*ctx).result = reason;
            (*ctx).exception_class = 1; // error
        }
    }
    ErlTerm::nil().0
}

/// `erlang:error/2`: raise an error exception with `reason`.
///
/// On BEAM, `args` (normally the current function's arguments, or `none`)
/// replaces the arity in the top stacktrace frame, and any term is accepted.
/// Nera's stack frames record only arities, so `args` is not kept; the class
/// and reason are the same as on BEAM.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_error_2(ctx: *mut ProcessContext, reason: u64, _args: u64) -> u64 {
    unsafe { nera_rt_error(ctx, reason) }
}

/// Raise an exit exception.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_exit(ctx: *mut ProcessContext, reason: u64) -> u64 {
    capture_exception_trace(ctx);
    if !ctx.is_null() {
        unsafe {
            (*ctx).status = crate::process::STATUS_EXCEPTION;
            (*ctx).result = reason;
            (*ctx).exception_class = 2; // exit
        }
    }
    ErlTerm::nil().0
}

/// Immediately terminate the runtime with status 0.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_halt_0(_ctx: *mut ProcessContext) -> u64 {
    std::process::exit(0);
}

/// Immediately terminate the runtime with the given integer status code.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_halt_1(_ctx: *mut ProcessContext, status: u64) -> u64 {
    let term = ErlTerm(status);
    let code = term.as_small_int().unwrap_or(1) as i32;
    std::process::exit(code);
}

/// Check if an exception is pending. Returns the 'true' atom if so, 'false' otherwise.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_check_exception(ctx: *mut ProcessContext) -> u64 {
    if !ctx.is_null() {
        let status = unsafe { (*ctx).status };
        if status == crate::process::STATUS_EXCEPTION {
            return ErlTerm::atom(crate::atom_table::ATOM_TRUE).0;
        }
    }
    ErlTerm::atom(crate::atom_table::ATOM_FALSE).0
}

/// Get the exception class as an atom: 'throw', 'error', or 'exit'.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_get_exception_class(ctx: *mut ProcessContext) -> u64 {
    if !ctx.is_null() {
        let class = unsafe { (*ctx).exception_class };
        let atom_name = match class {
            0 => "throw",
            1 => "error",
            2 => "exit",
            _ => "throw",
        };
        let atom_id = global_atom_table().intern(atom_name);
        return ErlTerm::atom(atom_id).0;
    }
    ErlTerm::nil().0
}

/// Get the exception reason (stored in ctx.result during exception).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_get_exception_reason(ctx: *mut ProcessContext) -> u64 {
    if !ctx.is_null() {
        return unsafe { (*ctx).result };
    }
    ErlTerm::nil().0
}

/// Get the exception stacktrace as an Erlang list of `{Module, Function, Arity, Info}` tuples.
/// Each `Info` is `[{file, File}, {line, Line}]`.
/// This matches the OTP stacktrace format.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_get_exception_stacktrace(ctx: *mut ProcessContext) -> u64 {
    if ctx.is_null() {
        return ErlTerm::nil().0;
    }
    let trace_ptr = unsafe { (*ctx).exception_trace_ptr };
    if trace_ptr.is_null() {
        return ErlTerm::nil().0;
    }
    let frames: &Vec<StackFrame> = unsafe { &*trace_ptr };
    if frames.is_empty() {
        return ErlTerm::nil().0;
    }

    with_ctx_heap(ctx, |heap| {
        let atom_table = global_atom_table();
        let line_atom = ErlTerm::atom(atom_table.intern("line"));

        // Build list in reverse (last frame first on heap, then cons)
        let mut list = ErlTerm::nil();
        for frame in frames.iter().rev() {
            let mod_atom = ErlTerm::atom(frame.module);
            let fun_atom = ErlTerm::atom(frame.function);
            let arity_term = ErlTerm::small_int(frame.arity as i64);

            // Info list: [{file, ""}, {line, Line}]
            // We don't have file info in StackFrame, so just emit [{line, Line}]
            let line_term = ErlTerm::small_int(frame.line as i64);
            let line_tuple = heap.alloc_tuple(&[line_atom, line_term]);
            let info_list = heap.alloc_cons(line_tuple, ErlTerm::nil());

            // {Module, Function, Arity, Info}
            let frame_tuple = heap.alloc_tuple(&[mod_atom, fun_atom, arity_term, info_list]);
            list = heap.alloc_cons(frame_tuple, list);
        }
        list.0
    })
}

/// `erlang:raise/3` — re-raise an exception with a given class, reason, and stacktrace.
///
/// Class must be one of `throw`, `error`, `exit`.
/// Stacktrace must be a list of `{Module, Function, Arity, Info}` tuples
/// (or any list — we store the raw term and let the runtime format it on crash).
///
/// Unlike throw/error/exit, raise does NOT capture the current call stack.
/// Instead it uses the provided stacktrace to populate the exception trace.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_raise(
    ctx: *mut ProcessContext,
    class_term: u64,
    reason: u64,
    stacktrace: u64,
) -> u64 {
    use crate::atom_table::ATOM_BADARG;

    if ctx.is_null() {
        return ErlTerm::nil().0;
    }

    let class_erl = ErlTerm(class_term);
    let class_atom = match class_erl.as_atom_id() {
        Some(id) => id,
        None => return super::helpers::raise_badarg(ctx),
    };

    let table = global_atom_table();
    let throw_id = table.intern("throw");
    let error_id = table.intern("error");
    let exit_id = table.intern("exit");

    let exception_class: u32 = if class_atom == throw_id {
        0
    } else if class_atom == error_id {
        1
    } else if class_atom == exit_id {
        2
    } else {
        return super::helpers::raise_error(ctx, ErlTerm::atom(ATOM_BADARG).0);
    };

    // Validate that stacktrace is a list (proper list or nil)
    let st = ErlTerm(stacktrace);
    if !st.is_nil() && !st.is_cons() {
        return super::helpers::raise_error(ctx, ErlTerm::atom(ATOM_BADARG).0);
    }

    // Set exception state
    (*ctx).status = crate::process::STATUS_EXCEPTION;
    (*ctx).result = reason;
    (*ctx).exception_class = exception_class;

    // Populate exception trace from the stacktrace list.
    // Walk the list and extract {M, F, A, Info} tuples into StackFrame entries.
    if !(*ctx).exception_trace_ptr.is_null() {
        let trace = &mut *((*ctx).exception_trace_ptr);
        trace.clear();

        let mut cursor = st;
        while cursor.is_cons() {
            let cell = &*cursor.as_cons_ptr().unwrap();
            let elem = cell.head;
            // Try to extract {Module, Function, Arity, Info}
            if let Some(ptr) = elem.as_boxed_ptr() {
                let header = crate::term::BoxedHeader(*ptr);
                if header.subtype() == crate::term::BOXED_TUPLE && header.arity() >= 3 {
                    let mod_id = ErlTerm(*ptr.add(1)).as_atom_id().unwrap_or(0);
                    let fun_id = ErlTerm(*ptr.add(2)).as_atom_id().unwrap_or(0);
                    let arity_val = ErlTerm(*ptr.add(3)).as_small_int().unwrap_or(0) as u32;
                    // Extract line from Info (4th element) if present
                    let line = if header.arity() >= 4 {
                        extract_line_from_info(ErlTerm(*ptr.add(4)))
                    } else {
                        0
                    };
                    trace.push(StackFrame {
                        module: mod_id,
                        function: fun_id,
                        arity: arity_val,
                        line,
                    });
                }
            }
            cursor = cell.tail;
        }
    }

    ErlTerm::nil().0
}

/// Extract line number from a stacktrace info list.
/// Info is typically `[{line, N}, ...]`.
/// # Safety
/// The complete input term graph must be initialized, live and unmoved by GC
/// until this call returns. Binary backing bytes must not be concurrently mutated.
unsafe fn extract_line_from_info(info: ErlTerm) -> u32 {
    let table = global_atom_table();
    let line_id = table.intern("line");
    let mut cursor = info;
    while cursor.is_cons() {
        unsafe {
            let cell = &*cursor.as_cons_ptr().unwrap();
            // Look for {line, N} tuple
            if let Some(ptr) = cell.head.as_boxed_ptr() {
                let header = crate::term::BoxedHeader(*ptr);
                if header.subtype() == crate::term::BOXED_TUPLE && header.arity() == 2
                    && ErlTerm(*ptr.add(1)).as_atom_id() == Some(line_id) {
                        if let Some(n) = ErlTerm(*ptr.add(2)).as_small_int() {
                            return n as u32;
                        }
                    }
            }
            cursor = cell.tail;
        }
    }
    0
}

/// Clear exception state: set status back to OK.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_clear_exception(ctx: *mut ProcessContext) {
    if !ctx.is_null() {
        unsafe {
            (*ctx).status = crate::process::STATUS_OK;
            (*ctx).exception_class = 0;
            if !(*ctx).exception_trace_ptr.is_null() {
                (*(*ctx).exception_trace_ptr).clear();
            }
        }
    }
}

