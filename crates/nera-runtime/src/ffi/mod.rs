//! C-ABI FFI functions called by compiled Erlang code.
//!
//! These functions provide the bridge between Cranelift-generated native
//! code and the Rust runtime. Each function takes a `*mut ProcessContext`
//! as its first argument.
//!
//! # Safety
//!
//! All public functions in submodules are marked `unsafe` because they
//! dereference raw pointers (primarily `*mut ProcessContext`). They are
//! called from Cranelift-generated machine code which guarantees valid
//! pointers at call sites.
//!
//! Heap access is a short, non-reentrant borrow through `with_ctx_heap`.
//! Its unsafe caller guarantees exclusive access to the process heap; the
//! reference-based callback cannot return a borrow of it. Heap allocation
//! does not collect. Before invoking compiled code, release all context,
//! heap and term-storage references; copy arguments and let the callee root
//! them before GC. Reacquire context fields after returning, since a nested
//! call may collect or replace process state. A raw `ErlTerm` is not a GC root.
//!
//! Scheduler access is scoped by `with_sched_from_ctx`: its callback cannot
//! return a scheduler reference. Unlike heap access, this shared borrow permits
//! reentrant runtime calls and GC, provided the scheduler owner remains live.
//! The helper copies the runtime pointer without retaining a context borrow.
//!
//! New adapters should decode ABI words/arrays, call reference-based Rust
//! operations, then encode the result. Raw-pointer helper functions must
//! expose their validity and aliasing requirements as unsafe contracts.
//!
//! # Module organisation
//!
//! - [`helpers`] — shared helpers, thread-locals, statics
//! - [`alloc`] — cons, tuple, float, bigint allocation
//! - [`arith`] — arithmetic + bitwise operations
//! - [`compare`] — comparison ops + Erlang term ordering
//! - [`type_check`] — `is_*` predicates + `abs`/`max`/`min`
//! - [`list_ops`] — list operations (head, tail, reverse, keyfind, …)
//! - [`tuple_ops`] — tuple element access, tuple_size
//! - [`map_ops`] — map allocation, get, put, merge, keys, values, …
//! - [`binary_ops`] — binary alloc, pattern matching, split, conversions
//! - [`io_format`] — `io:format`, `error_logger`, term pretty-printing
//! - [`process`] — spawn, send, link, monitor, register, process_info
//! - [`receive`] — selective receive protocol, resume state
//! - [`exception`] — throw, error, exit, stack tracing
//! - [`convert`] — atom_to_list, integer_to_list, etc.
//! - [`closure`] — closure alloc, remote fun refs, call dispatch
//! - [`apply`] — `erlang:apply/3` dynamic dispatch
//! - [`ets`] — ETS table operations
//! - [`timer`] — send_after, start_timer, cancel_timer
//! - [`gen_tcp`] — TCP socket operations
//! - [`file_io`] — file:read_file, file:write_file
//! - [`misc`] — intern_atom, make_ref, monotonic_time, process dict, get_self

pub(crate) mod helpers;
mod alloc;
mod base64;
mod base64_list;
mod native_work;
mod arith;
pub(crate) mod compare;
mod type_check;
pub(crate) mod unicode;
mod list_ops;
mod tuple_ops;
mod map_ops;
mod binary_ops;
mod crypto_ops;
mod string_ops;
mod io_format;
pub(crate) mod process;
mod receive;
mod exception;
mod convert;
pub(crate) mod closure;
pub(crate) mod apply;
mod ets;
mod persistent_term;
mod counters;
mod atomics;
mod zlib;
mod timer;
mod gen_tcp;
mod file_io;
mod misc;

// Re-exports for other runtime modules.
pub(crate) use helpers::binary_data;
pub use helpers::binary_data_pub;
pub use receive::take_pending_resumes;
pub(crate) use exception::nera_rt_get_exception_stacktrace;

#[cfg(test)]
mod tests {
    use crate::process::{ProcessContext, STATUS_EXCEPTION, STATUS_OK};
    use crate::term::ErlTerm;
    use super::exception::{nera_rt_stack_push, nera_rt_stack_pop, nera_rt_error, nera_rt_clear_exception};

    #[test]
    fn stack_push_and_pop_updates_call_stack() {
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.stack_ptr = &mut stack;
        ctx.exception_trace_ptr = &mut trace;

        // SAFETY: ctx points to a valid ProcessContext with valid stack/trace pointers.
        unsafe {
            nera_rt_stack_push(&mut ctx, ErlTerm::atom(10).0, ErlTerm::atom(20).0, 2, 0);
        }
        assert_eq!(stack.len(), 1);
        assert_eq!(stack[0].module, 10);
        assert_eq!(stack[0].function, 20);
        assert_eq!(stack[0].arity, 2);
        assert_eq!(stack[0].line, 0);

        // SAFETY: ctx points to a valid ProcessContext with valid stack/trace pointers.
        unsafe {
            nera_rt_stack_pop(&mut ctx);
        }
        assert!(stack.is_empty());
    }

    #[test]
    fn exception_capture_snapshots_and_clears_trace() {
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.stack_ptr = &mut stack;
        ctx.exception_trace_ptr = &mut trace;

        // SAFETY: ctx points to a valid ProcessContext with valid stack/trace pointers.
        unsafe {
            nera_rt_stack_push(&mut ctx, ErlTerm::atom(100).0, ErlTerm::atom(200).0, 1, 0);
            nera_rt_stack_push(&mut ctx, ErlTerm::atom(100).0, ErlTerm::atom(201).0, 2, 0);
        }
        let reason = ErlTerm::small_int(42).0;
        // SAFETY: ctx points to a valid ProcessContext with valid stack/trace pointers.
        let _ = unsafe { nera_rt_error(&mut ctx, reason) };

        assert_eq!(ctx.status, STATUS_EXCEPTION);
        assert_eq!(ctx.result, reason);
        assert_eq!(trace.len(), 2);
        assert_eq!(trace[0].function, 200);
        assert_eq!(trace[1].function, 201);

        // SAFETY: ctx points to a valid ProcessContext with valid stack/trace pointers.
        unsafe {
            nera_rt_clear_exception(&mut ctx);
        }
        assert_eq!(ctx.status, STATUS_OK);
        assert!(trace.is_empty());
    }

    // -----------------------------------------------------------------------
    // Helper: create a ProcessContext suitable for FFI tests.
    // heap_ptr is null → with_ctx_heap falls back to thread-local heap.
    // -----------------------------------------------------------------------
    fn make_test_ctx(stack: &mut Vec<crate::process::StackFrame>,
                     trace: &mut Vec<crate::process::StackFrame>) -> ProcessContext {
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.stack_ptr = stack;
        ctx.exception_trace_ptr = trace;
        ctx
    }

    /// Assert that after an FFI call, the context is in a badarg exception state.
    fn assert_badarg(ctx: &ProcessContext) {
        assert_eq!(ctx.status, STATUS_EXCEPTION, "expected STATUS_EXCEPTION");
        assert_eq!(ctx.exception_class, 1, "expected exception_class=1 (error)");
        // The result should be the badarg atom.
        let badarg_atom = ErlTerm::atom(crate::atom_table::ATOM_BADARG);
        assert_eq!(ctx.result, badarg_atom.0, "expected badarg reason");
    }

    // =======================================================================
    // list_ops: length, element, hd, tl
    // =======================================================================

    #[test]
    fn length_raises_badarg_on_non_list() {
        use super::list_ops::nera_rt_length;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_list = ErlTerm::atom(42).0; // atom, not a list
        unsafe { nera_rt_length(&mut ctx, not_a_list); }
        assert_badarg(&ctx);
    }

    #[test]
    fn element_raises_badarg_on_non_tuple() {
        use super::list_ops::nera_rt_element;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let index = ErlTerm::small_int(1).0;
        let not_a_tuple = ErlTerm::atom(42).0;
        unsafe { nera_rt_element(&mut ctx, index, not_a_tuple); }
        assert_badarg(&ctx);
    }

    #[test]
    fn hd_raises_badarg_on_non_list() {
        use super::list_ops::nera_rt_hd;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_list = ErlTerm::small_int(123).0;
        unsafe { nera_rt_hd(&mut ctx, not_a_list); }
        assert_badarg(&ctx);
    }

    #[test]
    fn tl_raises_badarg_on_non_list() {
        use super::list_ops::nera_rt_tl;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_list = ErlTerm::small_int(123).0;
        unsafe { nera_rt_tl(&mut ctx, not_a_list); }
        assert_badarg(&ctx);
    }

    #[test]
    fn hd_raises_badarg_on_empty_list() {
        use super::list_ops::nera_rt_hd;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        unsafe { nera_rt_hd(&mut ctx, ErlTerm::nil().0); }
        assert_badarg(&ctx);
    }

    // =======================================================================
    // type_check: abs, max, min
    // =======================================================================

    #[test]
    fn abs_raises_badarg_on_non_number() {
        use super::type_check::nera_rt_abs;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_number = ErlTerm::atom(42).0; // atom, not a number
        unsafe { nera_rt_abs(&mut ctx, not_a_number); }
        assert_badarg(&ctx);
    }

    #[test]
    fn round_raises_badarg_on_non_number() {
        use super::type_check::nera_rt_round;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_number = ErlTerm::atom(42).0;
        unsafe { nera_rt_round(&mut ctx, not_a_number); }
        assert_badarg(&ctx);
    }

    #[test]
    fn trunc_raises_badarg_on_non_number() {
        use super::type_check::nera_rt_trunc;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_number = ErlTerm::atom(42).0;
        unsafe { nera_rt_trunc(&mut ctx, not_a_number); }
        assert_badarg(&ctx);
    }

    #[test]
    fn float_raises_badarg_on_non_number() {
        use super::type_check::nera_rt_float;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_number = ErlTerm::atom(42).0;
        unsafe { nera_rt_float(&mut ctx, not_a_number); }
        assert_badarg(&ctx);
    }

    #[test]
    fn ceil_raises_badarg_on_non_number() {
        use super::type_check::nera_rt_ceil;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_number = ErlTerm::atom(42).0;
        unsafe { nera_rt_ceil(&mut ctx, not_a_number); }
        assert_badarg(&ctx);
    }

    #[test]
    fn floor_raises_badarg_on_non_number() {
        use super::type_check::nera_rt_floor;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_number = ErlTerm::atom(42).0;
        unsafe { nera_rt_floor(&mut ctx, not_a_number); }
        assert_badarg(&ctx);
    }

    #[test]
    fn size_raises_badarg_on_non_tuple_non_binary() {
        use super::type_check::nera_rt_size;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_valid = ErlTerm::atom(42).0;
        unsafe { nera_rt_size(&mut ctx, not_valid); }
        assert_badarg(&ctx);
    }

    #[test]
    fn phash2_2_raises_badarg_on_invalid_range() {
        use super::type_check::nera_rt_phash2_2;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let term = ErlTerm::small_int(42).0;
        let bad_range = ErlTerm::small_int(0).0;
        unsafe { nera_rt_phash2_2(&mut ctx, term, bad_range); }
        assert_badarg(&ctx);
    }

    // =======================================================================
    // convert: atom_to_list, list_to_atom, integer_to_list, list_to_integer,
    //          float_to_list, list_to_tuple, tuple_to_list, setelement,
    //          make_tuple_2, make_tuple_3
    // =======================================================================

    #[test]
    fn atom_to_list_raises_badarg_on_non_atom() {
        use super::convert::nera_rt_atom_to_list;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_an_atom = ErlTerm::small_int(123).0;
        unsafe { nera_rt_atom_to_list(&mut ctx, not_an_atom); }
        assert_badarg(&ctx);
    }

    #[test]
    fn list_to_atom_raises_badarg_on_non_list() {
        use super::convert::nera_rt_list_to_atom;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_list = ErlTerm::small_int(123).0;
        unsafe { nera_rt_list_to_atom(&mut ctx, not_a_list); }
        assert_badarg(&ctx);
    }

    #[test]
    fn integer_to_list_raises_badarg_on_non_integer() {
        use super::convert::nera_rt_integer_to_list;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_an_int = ErlTerm::atom(42).0; // atom, not integer
        unsafe { nera_rt_integer_to_list(&mut ctx, not_an_int); }
        assert_badarg(&ctx);
    }

    #[test]
    fn list_to_integer_raises_badarg_on_non_list() {
        use super::convert::nera_rt_list_to_integer;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_list = ErlTerm::atom(42).0;
        unsafe { nera_rt_list_to_integer(&mut ctx, not_a_list); }
        assert_badarg(&ctx);
    }

    #[test]
    fn float_to_list_raises_badarg_on_non_float() {
        use super::convert::nera_rt_float_to_list;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_float = ErlTerm::atom(42).0;
        unsafe { nera_rt_float_to_list(&mut ctx, not_a_float); }
        assert_badarg(&ctx);
    }

    #[test]
    fn list_to_tuple_raises_badarg_on_non_list() {
        use super::convert::nera_rt_list_to_tuple;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_list = ErlTerm::atom(42).0;
        unsafe { nera_rt_list_to_tuple(&mut ctx, not_a_list); }
        assert_badarg(&ctx);
    }

    #[test]
    fn tuple_to_list_raises_badarg_on_non_tuple() {
        use super::convert::nera_rt_tuple_to_list;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_tuple = ErlTerm::small_int(42).0;
        unsafe { nera_rt_tuple_to_list(&mut ctx, not_a_tuple); }
        assert_badarg(&ctx);
    }

    #[test]
    fn setelement_raises_badarg_on_bad_index() {
        use super::convert::nera_rt_setelement;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        // Index 0 is invalid (Erlang is 1-indexed)
        let index = ErlTerm::small_int(0).0;
        let not_a_tuple = ErlTerm::atom(42).0;
        let value = ErlTerm::atom(10).0;
        unsafe { nera_rt_setelement(&mut ctx, index, not_a_tuple, value); }
        assert_badarg(&ctx);
    }

    // =======================================================================
    // io_format: io:format/2, io:format/1
    // =======================================================================

    #[test]
    fn io_format_raises_badarg_on_non_string_fmt() {
        use super::io_format::nera_rt_io_format;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_string = ErlTerm::small_int(123).0;
        let args = ErlTerm::nil().0;
        unsafe { nera_rt_io_format(&mut ctx, not_a_string, args); }
        assert_badarg(&ctx);
    }

    #[test]
    fn io_format_1_raises_badarg_on_non_string() {
        use super::io_format::nera_rt_io_format_1;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_string = ErlTerm::small_int(123).0;
        unsafe { nera_rt_io_format_1(&mut ctx, not_a_string); }
        assert_badarg(&ctx);
    }

    // =======================================================================
    // binary_ops: list_to_existing_atom
    // =======================================================================

    #[test]
    fn list_to_existing_atom_raises_badarg_on_non_list() {
        use super::binary_ops::nera_rt_list_to_existing_atom;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_list = ErlTerm::small_int(123).0;
        unsafe { nera_rt_list_to_existing_atom(&mut ctx, not_a_list); }
        assert_badarg(&ctx);
    }

    // =======================================================================
    // process: spawn, spawn_closure, send_message (argument validation)
    // =======================================================================

    #[test]
    fn spawn_raises_badarg_on_non_atom_module() {
        use super::process::nera_rt_spawn;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_an_atom = ErlTerm::small_int(123).0;
        let fun = ErlTerm::atom(10).0;
        let args = ErlTerm::nil().0;
        unsafe { nera_rt_spawn(&mut ctx, not_an_atom, fun, args); }
        assert_badarg(&ctx);
    }

    #[test]
    fn spawn_raises_badarg_on_non_atom_function() {
        use super::process::nera_rt_spawn;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let module = ErlTerm::atom(10).0;
        let not_an_atom = ErlTerm::small_int(123).0;
        let args = ErlTerm::nil().0;
        unsafe { nera_rt_spawn(&mut ctx, module, not_an_atom, args); }
        assert_badarg(&ctx);
    }

    #[test]
    fn spawn_closure_raises_badarg_on_non_closure() {
        use super::process::nera_rt_spawn_closure;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_closure = ErlTerm::small_int(123).0;
        unsafe { nera_rt_spawn_closure(&mut ctx, not_a_closure); }
        assert_badarg(&ctx);
    }

    #[test]
    fn spawn_link_raises_badarg_on_non_atom_module() {
        use super::process::nera_rt_spawn_link;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_an_atom = ErlTerm::small_int(123).0;
        let fun = ErlTerm::atom(10).0;
        let args = ErlTerm::nil().0;
        unsafe { nera_rt_spawn_link(&mut ctx, not_an_atom, fun, args); }
        assert_badarg(&ctx);
    }

    #[test]
    fn spawn_link_closure_raises_badarg_on_non_closure() {
        use super::process::nera_rt_spawn_link_closure;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_closure = ErlTerm::small_int(123).0;
        unsafe { nera_rt_spawn_link_closure(&mut ctx, not_a_closure); }
        assert_badarg(&ctx);
    }

    #[test]
    fn send_raises_badarg_on_non_pid_non_atom() {
        use super::process::nera_rt_send_message;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_pid = ErlTerm::small_int(123).0; // integer, not PID or atom
        let msg = ErlTerm::atom(10).0;
        unsafe { nera_rt_send_message(&mut ctx, not_a_pid, msg); }
        assert_badarg(&ctx);
    }

    // =======================================================================
    // ets: ets_new, ets_give_away
    // =======================================================================

    #[test]
    fn ets_new_raises_badarg_on_non_atom_name() {
        use super::ets::nera_rt_ets_new;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_an_atom = ErlTerm::small_int(123).0;
        let opts = ErlTerm::nil().0;
        unsafe { nera_rt_ets_new(&mut ctx, not_an_atom, opts); }
        assert_badarg(&ctx);
    }

    #[test]
    fn ets_give_away_raises_badarg_on_non_pid() {
        use super::ets::nera_rt_ets_give_away;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let table = ErlTerm::small_int(1).0;
        let not_a_pid = ErlTerm::small_int(123).0;
        let gift = ErlTerm::nil().0;
        unsafe { nera_rt_ets_give_away(&mut ctx, table, not_a_pid, gift); }
        assert_badarg(&ctx);
    }

    // =======================================================================
    // misc: monotonic_time_1
    // =======================================================================

    #[test]
    fn monotonic_time_1_raises_badarg_on_non_atom_unit() {
        use super::misc::nera_rt_monotonic_time_1;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_an_atom = ErlTerm::small_int(123).0;
        unsafe { nera_rt_monotonic_time_1(&mut ctx, not_an_atom); }
        assert_badarg(&ctx);
    }

    // =======================================================================
    // apply: nera_rt_apply
    // =======================================================================

    #[test]
    fn apply_raises_badarg_on_non_atom_module() {
        use super::apply::nera_rt_apply;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_an_atom = ErlTerm::small_int(123).0;
        let fun = ErlTerm::atom(10).0;
        let args = ErlTerm::nil().0;
        unsafe { nera_rt_apply(&mut ctx, not_an_atom, fun, args); }
        assert_badarg(&ctx);
    }

    // =======================================================================
    // exception: raise/3
    // =======================================================================

    #[test]
    fn raise_throw_sets_exception_class_0() {
        use super::exception::nera_rt_raise;
        use crate::atom_table::global_atom_table;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let table = global_atom_table();
        let throw_atom = ErlTerm::atom(table.intern("throw"));
        let reason = ErlTerm::small_int(42);
        unsafe { nera_rt_raise(&mut ctx, throw_atom.0, reason.0, ErlTerm::nil().0); }
        assert_eq!(ctx.status, STATUS_EXCEPTION);
        assert_eq!(ctx.exception_class, 0); // throw
        assert_eq!(ctx.result, reason.0);
    }

    #[test]
    fn raise_error_sets_exception_class_1() {
        use super::exception::nera_rt_raise;
        use crate::atom_table::global_atom_table;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let table = global_atom_table();
        let error_atom = ErlTerm::atom(table.intern("error"));
        let reason = ErlTerm::atom(table.intern("badmatch"));
        unsafe { nera_rt_raise(&mut ctx, error_atom.0, reason.0, ErlTerm::nil().0); }
        assert_eq!(ctx.status, STATUS_EXCEPTION);
        assert_eq!(ctx.exception_class, 1); // error
        assert_eq!(ctx.result, reason.0);
    }

    #[test]
    fn raise_exit_sets_exception_class_2() {
        use super::exception::nera_rt_raise;
        use crate::atom_table::global_atom_table;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let table = global_atom_table();
        let exit_atom = ErlTerm::atom(table.intern("exit"));
        let reason = ErlTerm::atom(table.intern("normal"));
        unsafe { nera_rt_raise(&mut ctx, exit_atom.0, reason.0, ErlTerm::nil().0); }
        assert_eq!(ctx.status, STATUS_EXCEPTION);
        assert_eq!(ctx.exception_class, 2); // exit
        assert_eq!(ctx.result, reason.0);
    }

    #[test]
    fn raise_badarg_on_invalid_class() {
        use super::exception::nera_rt_raise;
        use crate::atom_table::global_atom_table;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let table = global_atom_table();
        let bad_class = ErlTerm::atom(table.intern("not_a_class"));
        let reason = ErlTerm::small_int(1);
        unsafe { nera_rt_raise(&mut ctx, bad_class.0, reason.0, ErlTerm::nil().0); }
        // Should raise an error (badarg)
        assert_eq!(ctx.status, STATUS_EXCEPTION);
        assert_eq!(ctx.exception_class, 1); // error
        assert_eq!(ctx.result, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
    }

    #[test]
    fn raise_badarg_on_non_atom_class() {
        use super::exception::nera_rt_raise;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_atom = ErlTerm::small_int(42);
        let reason = ErlTerm::small_int(1);
        unsafe { nera_rt_raise(&mut ctx, not_atom.0, reason.0, ErlTerm::nil().0); }
        assert_badarg(&ctx);
    }

    #[test]
    fn raise_badarg_on_non_list_stacktrace() {
        use super::exception::nera_rt_raise;
        use crate::atom_table::global_atom_table;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let table = global_atom_table();
        let throw_atom = ErlTerm::atom(table.intern("throw"));
        let reason = ErlTerm::small_int(1);
        let not_a_list = ErlTerm::small_int(99); // integer, not a list
        unsafe { nera_rt_raise(&mut ctx, throw_atom.0, reason.0, not_a_list.0); }
        // Should raise error (badarg) because stacktrace is not a list
        assert_eq!(ctx.status, STATUS_EXCEPTION);
        assert_eq!(ctx.exception_class, 1); // error
    }

    #[test]
    fn raise_does_not_capture_current_stack() {
        use super::exception::{nera_rt_raise, nera_rt_stack_push};
        use crate::atom_table::global_atom_table;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let table = global_atom_table();

        // Push some frames onto the call stack
        unsafe {
            nera_rt_stack_push(&mut ctx, ErlTerm::atom(10).0, ErlTerm::atom(20).0, 1, 5);
            nera_rt_stack_push(&mut ctx, ErlTerm::atom(11).0, ErlTerm::atom(21).0, 2, 10);
        }
        assert_eq!(stack.len(), 2);

        // raise with empty stacktrace
        let throw_atom = ErlTerm::atom(table.intern("throw"));
        let reason = ErlTerm::small_int(42);
        unsafe { nera_rt_raise(&mut ctx, throw_atom.0, reason.0, ErlTerm::nil().0); }

        // The exception trace should be empty (raise does NOT capture current stack)
        assert!(trace.is_empty());
    }

    #[test]
    fn raise_with_stacktrace_populates_trace() {
        use super::exception::nera_rt_raise;
        use crate::atom_table::global_atom_table;
        use crate::heap::ProcessHeap;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut heap = ProcessHeap::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        ctx.heap_ptr = &mut heap;
        let table = global_atom_table();

        let mod_atom = ErlTerm::atom(table.intern("my_mod"));
        let fun_atom = ErlTerm::atom(table.intern("my_fun"));
        let arity = ErlTerm::small_int(2);

        // Build info list: [{line, 42}]
        let line_atom = ErlTerm::atom(table.intern("line"));
        let line_tuple = heap.alloc_tuple(&[line_atom, ErlTerm::small_int(42)]);
        let info = heap.alloc_cons(line_tuple, ErlTerm::nil());

        // Build stacktrace: [{my_mod, my_fun, 2, [{line, 42}]}]
        let frame_tuple = heap.alloc_tuple(&[mod_atom, fun_atom, arity, info]);
        let stacktrace = heap.alloc_cons(frame_tuple, ErlTerm::nil());

        let error_atom = ErlTerm::atom(table.intern("error"));
        let reason = ErlTerm::atom(table.intern("badmatch"));
        unsafe { nera_rt_raise(&mut ctx, error_atom.0, reason.0, stacktrace.0); }

        assert_eq!(ctx.status, STATUS_EXCEPTION);
        assert_eq!(ctx.exception_class, 1);
        assert_eq!(ctx.result, reason.0);

        // Trace should have one frame from the provided stacktrace
        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0].module, table.intern("my_mod"));
        assert_eq!(trace[0].function, table.intern("my_fun"));
        assert_eq!(trace[0].arity, 2);
        assert_eq!(trace[0].line, 42);
    }

    #[test]
    fn raise_with_multi_frame_stacktrace() {
        use super::exception::nera_rt_raise;
        use crate::atom_table::global_atom_table;
        use crate::heap::ProcessHeap;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut heap = ProcessHeap::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        ctx.heap_ptr = &mut heap;
        let table = global_atom_table();

        // Frame 1: {mod_a, fun_a, 1, [{line, 10}]}
        let mod_a = ErlTerm::atom(table.intern("mod_a"));
        let fun_a = ErlTerm::atom(table.intern("fun_a"));
        let line_atom = ErlTerm::atom(table.intern("line"));
        let lt1 = heap.alloc_tuple(&[line_atom, ErlTerm::small_int(10)]);
        let info1 = heap.alloc_cons(lt1, ErlTerm::nil());
        let frame1 = heap.alloc_tuple(&[mod_a, fun_a, ErlTerm::small_int(1), info1]);

        // Frame 2: {mod_b, fun_b, 3, [{line, 20}]}
        let mod_b = ErlTerm::atom(table.intern("mod_b"));
        let fun_b = ErlTerm::atom(table.intern("fun_b"));
        let lt2 = heap.alloc_tuple(&[line_atom, ErlTerm::small_int(20)]);
        let info2 = heap.alloc_cons(lt2, ErlTerm::nil());
        let frame2 = heap.alloc_tuple(&[mod_b, fun_b, ErlTerm::small_int(3), info2]);

        // Build list: [frame1, frame2]
        let tail = heap.alloc_cons(frame2, ErlTerm::nil());
        let st = heap.alloc_cons(frame1, tail);

        let throw_atom = ErlTerm::atom(table.intern("throw"));
        let reason = ErlTerm::small_int(99);
        unsafe { nera_rt_raise(&mut ctx, throw_atom.0, reason.0, st.0); }

        assert_eq!(trace.len(), 2);
        assert_eq!(trace[0].module, table.intern("mod_a"));
        assert_eq!(trace[0].line, 10);
        assert_eq!(trace[1].module, table.intern("mod_b"));
        assert_eq!(trace[1].line, 20);
    }

    #[test]
    fn raise_with_3_element_tuple_no_info() {
        use super::exception::nera_rt_raise;
        use crate::atom_table::global_atom_table;
        use crate::heap::ProcessHeap;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut heap = ProcessHeap::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        ctx.heap_ptr = &mut heap;
        let table = global_atom_table();

        // {mod, fun, 1} — no info element
        let frame = heap.alloc_tuple(&[
            ErlTerm::atom(table.intern("mod_c")),
            ErlTerm::atom(table.intern("fun_c")),
            ErlTerm::small_int(1),
        ]);
        let st = heap.alloc_cons(frame, ErlTerm::nil());

        let error_atom = ErlTerm::atom(table.intern("error"));
        let reason = ErlTerm::small_int(0);
        unsafe { nera_rt_raise(&mut ctx, error_atom.0, reason.0, st.0); }

        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0].module, table.intern("mod_c"));
        assert_eq!(trace[0].function, table.intern("fun_c"));
        assert_eq!(trace[0].arity, 1);
        assert_eq!(trace[0].line, 0); // no info → line=0
    }

    // =======================================================================
    // process: process_info/1
    // =======================================================================

    #[test]
    fn process_info_1_raises_badarg_on_non_pid() {
        use super::process::nera_rt_process_info_1;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        let not_a_pid = ErlTerm::small_int(123).0;
        unsafe { nera_rt_process_info_1(&mut ctx, not_a_pid); }
        assert_badarg(&ctx);
    }

    #[test]
    fn process_info_1_returns_undefined_for_unknown_pid() {
        use super::process::nera_rt_process_info_1;
        use crate::atom_table::ATOM_UNDEFINED;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        // PID 999999 doesn't exist
        let pid = ErlTerm::pid(crate::process::ProcessId::new(999999)).0;
        let result = unsafe { nera_rt_process_info_1(&mut ctx, pid) };
        assert_eq!(result, ErlTerm::atom(ATOM_UNDEFINED).0);
    }

    #[test]
    fn process_info_1_self_returns_list() {
        use super::process::nera_rt_process_info_1;
        use crate::heap::ProcessHeap;
        use crate::process::{Mailbox, OtpProcessState};
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut heap = ProcessHeap::new();
        let mut mailbox = Mailbox::new();
        let mut otp = OtpProcessState::default();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        ctx.pid = crate::process::ProcessId::new(42);
        ctx.heap_ptr = &mut heap;
        ctx.mailbox_ptr = &mut mailbox;
        ctx.otp_ptr = &mut otp;

        let self_pid = ErlTerm::pid(crate::process::ProcessId::new(42)).0;
        let result = unsafe { nera_rt_process_info_1(&mut ctx, self_pid) };
        let result_term = ErlTerm(result);

        // Result should be a proper list (cons cells ending in nil)
        assert!(result_term.is_cons(), "process_info/1 should return a list");

        // Walk the list and count elements
        let mut count = 0;
        let mut cursor = result_term;
        while cursor.is_cons() {
            count += 1;
            unsafe {
                let cell = &*cursor.as_cons_ptr().unwrap();
                // Each element should be a tuple
                assert!(cell.head.is_boxed(), "each info item should be a tuple");
                cursor = cell.tail;
            }
        }
        assert!(cursor.is_nil(), "should be a proper list");
        // All process_info keys except registered_name (skipped when None):
        // message_queue_len, messages, links, monitors, monitored_by,
        // trap_exit, status, reductions, heap_size, memory, dictionary,
        // current_stacktrace, group_leader, priority
        assert_eq!(count, 14);
    }

    #[test]
    fn process_info_1_self_with_registered_name() {
        use super::process::nera_rt_process_info_1;
        use crate::heap::ProcessHeap;
        use crate::process::{Mailbox, OtpProcessState};
        use crate::atom_table::global_atom_table;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut heap = ProcessHeap::new();
        let mut mailbox = Mailbox::new();
        let mut otp = OtpProcessState::default();
        let table = global_atom_table();
        otp.registered_name = Some(table.intern("my_server"));
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        ctx.pid = crate::process::ProcessId::new(42);
        ctx.heap_ptr = &mut heap;
        ctx.mailbox_ptr = &mut mailbox;
        ctx.otp_ptr = &mut otp;

        let self_pid = ErlTerm::pid(crate::process::ProcessId::new(42)).0;
        let result = unsafe { nera_rt_process_info_1(&mut ctx, self_pid) };
        let result_term = ErlTerm(result);
        assert!(result_term.is_cons());

        // Count: should be 15 (all 14 fields + registered_name)
        let mut count = 0;
        let mut cursor = result_term;
        while cursor.is_cons() {
            count += 1;
            unsafe { cursor = (*cursor.as_cons_ptr().unwrap()).tail; }
        }
        assert_eq!(count, 15);
    }

    #[test]
    fn process_info_1_self_with_links() {
        use super::process::nera_rt_process_info_1;
        use crate::heap::ProcessHeap;
        use crate::process::{Mailbox, OtpProcessState};
        use crate::atom_table::global_atom_table;
        use crate::term::BoxedHeader;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut heap = ProcessHeap::new();
        let mut mailbox = Mailbox::new();
        let mut otp = OtpProcessState::default();
        otp.links.insert(crate::process::ProcessId::new(100));
        otp.links.insert(crate::process::ProcessId::new(200));
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        ctx.pid = crate::process::ProcessId::new(42);
        ctx.heap_ptr = &mut heap;
        ctx.mailbox_ptr = &mut mailbox;
        ctx.otp_ptr = &mut otp;

        let self_pid = ErlTerm::pid(crate::process::ProcessId::new(42)).0;
        let result = unsafe { nera_rt_process_info_1(&mut ctx, self_pid) };
        let result_term = ErlTerm(result);

        // Walk the list to find {links, [Pid1, Pid2]}
        let table = global_atom_table();
        let links_atom_id = table.intern("links");
        let mut cursor = result_term;
        let mut found_links = false;
        while cursor.is_cons() {
            unsafe {
                let cell = &*cursor.as_cons_ptr().unwrap();
                let elem = cell.head;
                if let Some(ptr) = elem.as_boxed_ptr() {
                    let header = BoxedHeader(*ptr);
                    if header.subtype() == crate::term::BOXED_TUPLE && header.arity() == 2 {
                        let key = ErlTerm(*ptr.add(1));
                        if key.as_atom_id() == Some(links_atom_id) {
                            found_links = true;
                            let links_list = ErlTerm(*ptr.add(2));
                            // Count pids in the links list
                            let mut link_count = 0;
                            let mut lc = links_list;
                            while lc.is_cons() {
                                link_count += 1;
                                lc = (*lc.as_cons_ptr().unwrap()).tail;
                            }
                            assert_eq!(link_count, 2);
                        }
                    }
                }
                cursor = cell.tail;
            }
        }
        assert!(found_links, "should contain links tuple");
    }

    #[test]
    fn process_info_1_self_trap_exit_true() {
        use super::process::nera_rt_process_info_1;
        use crate::heap::ProcessHeap;
        use crate::process::{Mailbox, OtpProcessState};
        use crate::atom_table::{global_atom_table, ATOM_TRUE};
        use crate::term::BoxedHeader;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut heap = ProcessHeap::new();
        let mut mailbox = Mailbox::new();
        let mut otp = OtpProcessState::default();
        otp.trap_exit = true;
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        ctx.pid = crate::process::ProcessId::new(42);
        ctx.heap_ptr = &mut heap;
        ctx.mailbox_ptr = &mut mailbox;
        ctx.otp_ptr = &mut otp;

        let self_pid = ErlTerm::pid(crate::process::ProcessId::new(42)).0;
        let result = unsafe { nera_rt_process_info_1(&mut ctx, self_pid) };
        let result_term = ErlTerm(result);

        let table = global_atom_table();
        let trap_exit_id = table.intern("trap_exit");
        let mut cursor = result_term;
        let mut found = false;
        while cursor.is_cons() {
            unsafe {
                let cell = &*cursor.as_cons_ptr().unwrap();
                if let Some(ptr) = cell.head.as_boxed_ptr() {
                    let header = BoxedHeader(*ptr);
                    if header.subtype() == crate::term::BOXED_TUPLE && header.arity() == 2 {
                        let key = ErlTerm(*ptr.add(1));
                        if key.as_atom_id() == Some(trap_exit_id) {
                            let val = ErlTerm(*ptr.add(2));
                            assert_eq!(val.as_atom_id(), Some(ATOM_TRUE));
                            found = true;
                        }
                    }
                }
                cursor = cell.tail;
            }
        }
        assert!(found, "should contain trap_exit tuple");
    }

    #[test]
    fn process_info_1_self_status_is_running() {
        use super::process::nera_rt_process_info_1;
        use crate::heap::ProcessHeap;
        use crate::process::{Mailbox, OtpProcessState};
        use crate::atom_table::global_atom_table;
        use crate::term::BoxedHeader;
        let mut stack = Vec::new();
        let mut trace = Vec::new();
        let mut heap = ProcessHeap::new();
        let mut mailbox = Mailbox::new();
        let mut otp = OtpProcessState::default();
        let mut ctx = make_test_ctx(&mut stack, &mut trace);
        ctx.pid = crate::process::ProcessId::new(42);
        ctx.heap_ptr = &mut heap;
        ctx.mailbox_ptr = &mut mailbox;
        ctx.otp_ptr = &mut otp;

        let self_pid = ErlTerm::pid(crate::process::ProcessId::new(42)).0;
        let result = unsafe { nera_rt_process_info_1(&mut ctx, self_pid) };
        let result_term = ErlTerm(result);

        let table = global_atom_table();
        let status_id = table.intern("status");
        let running_id = table.intern("running");
        let mut cursor = result_term;
        let mut found = false;
        while cursor.is_cons() {
            unsafe {
                let cell = &*cursor.as_cons_ptr().unwrap();
                if let Some(ptr) = cell.head.as_boxed_ptr() {
                    let header = BoxedHeader(*ptr);
                    if header.subtype() == crate::term::BOXED_TUPLE && header.arity() == 2 {
                        let key = ErlTerm(*ptr.add(1));
                        if key.as_atom_id() == Some(status_id) {
                            let val = ErlTerm(*ptr.add(2));
                            assert_eq!(val.as_atom_id(), Some(running_id));
                            found = true;
                        }
                    }
                }
                cursor = cell.tail;
            }
        }
        assert!(found, "should contain status tuple");
    }
}
