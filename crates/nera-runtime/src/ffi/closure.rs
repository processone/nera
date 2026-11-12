//! Closure operations: allocation, remote fun refs, call dispatch.

#![allow(unused_unsafe)]

use crate::atom_table::global_atom_table;
use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;
use super::exception::nera_rt_error;

/// Build an error reason without retaining a context or entering compiled code.
fn make_badfun_reason(heap: &mut crate::heap::ProcessHeap, term: ErlTerm) -> ErlTerm {
    let tag = ErlTerm::atom(global_atom_table().intern("badfun"));
    heap.alloc_tuple(&[tag, term])
}

fn make_badarity_reason(
    heap: &mut crate::heap::ProcessHeap,
    closure: ErlTerm,
    args: &[u64],
) -> ErlTerm {
    let tag = ErlTerm::atom(global_atom_table().intern("badarity"));
    let mut args_list = ErlTerm::nil();
    for &arg in args.iter().rev() {
        args_list = heap.alloc_cons(ErlTerm::from_raw(arg), args_list);
    }
    let inner = heap.alloc_tuple(&[closure, args_list]);
    heap.alloc_tuple(&[tag, inner])
}

/// Allocate a closure on the process heap.
///
/// Layout: `[header(BOXED_CLOSURE, env_count) | fn_ptr | arity | env0 | env1 | ...]`
///
/// - `fn_ptr`: raw function pointer (as u64) to the ErlFn wrapper
/// - `arity`: the Erlang-level arity (number of user arguments)
/// - `env_ptr`: pointer to array of captured variable terms
/// - `env_count`: number of captured variables
#[no_mangle]
pub unsafe extern "C" fn nera_rt_alloc_closure(
    ctx: *mut ProcessContext,
    fn_ptr: u64,
    arity: u32,
    env_ptr: *const u64,
    env_count: u32,
) -> u64 {
    with_ctx_heap(ctx, |heap| {
        let total_words = 3 + env_count as usize;
        let ptr = heap.alloc_words(total_words);
        unsafe {
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Closure, env_count as u64).0;
            *ptr.add(1) = fn_ptr;
            *ptr.add(2) = arity as u64;
            for i in 0..env_count as usize {
                *ptr.add(3 + i) = *env_ptr.add(i);
            }
        }
        ErlTerm::boxed_unchecked(ptr as *const u64).0
    })
}

/// Allocate a remote function reference closure on the process heap.
///
/// Layout: `[header(BOXED_CLOSURE, 2) | 0 (null fn_ptr sentinel) | arity | mod_atom | fun_atom]`
///
/// The null fn_ptr (0) distinguishes remote fun refs from regular closures.
/// At call time, `nera_rt_call_closure` detects fn_ptr == 0 and dispatches
/// via `nera_rt_apply` using the stored module/function atoms.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_alloc_remote_fun_ref(
    ctx: *mut ProcessContext,
    mod_atom: u64,
    fun_atom: u64,
    arity: u32,
) -> u64 {
    with_ctx_heap(ctx, |heap| {
        // env_count = 2 (mod_atom and fun_atom stored as env vars)
        let total_words = 3 + 2;
        let ptr = heap.alloc_words(total_words);
        unsafe {
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Closure, 2).0;
            *ptr.add(1) = 0u64; // null fn_ptr — sentinel for remote fun ref
            *ptr.add(2) = arity as u64;
            *ptr.add(3) = mod_atom; // env[0] = module atom term
            *ptr.add(4) = fun_atom; // env[1] = function atom term
        }
        ErlTerm::boxed_unchecked(ptr as *const u64).0
    })
}

/// Call a closure with arguments.
///
/// Extracts fn_ptr, arity, and captured env from the closure term.
/// Combines env vars + user args into a single array and calls through fn_ptr.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_call_closure(
    ctx: *mut ProcessContext,
    closure: u64,
    args_ptr: *const u64,
    nargs: u32,
) -> u64 {
    call_closure_impl(ctx, closure, args_ptr, nargs, true)
}

/// Ordinary calls resolve nested chains; tail-loop dispatch leaves them pending.
pub(super) unsafe fn call_closure_impl(
    ctx: *mut ProcessContext,
    closure: u64,
    args_ptr: *const u64,
    nargs: u32,
    resolve: bool,
) -> u64 {
    let term = ErlTerm(closure);

    if !term.is_boxed() {
        let reason = with_ctx_heap(ctx, |heap| make_badfun_reason(heap, term)).to_raw();
        return nera_rt_error(ctx, reason);
    }

    let boxed_ptr = term.as_boxed_ptr().unwrap();
    let header = unsafe { BoxedHeader(*boxed_ptr) };

    if header.subtype() != BOXED_CLOSURE {
        let reason = with_ctx_heap(ctx, |heap| make_badfun_reason(heap, term)).to_raw();
        return nera_rt_error(ctx, reason);
    }

    let env_count = header.arity() as usize;
    let fn_ptr = unsafe { *boxed_ptr.add(1) };
    let expected_arity = unsafe { *boxed_ptr.add(2) } as u32;

    if nargs != expected_arity {
        let args = if nargs == 0 { &[] } else {
            // SAFETY: the FFI caller supplies nargs readable argument words.
            std::slice::from_raw_parts(args_ptr, nargs as usize)
        };
        let reason = with_ctx_heap(ctx, |heap| make_badarity_reason(heap, term, args)).to_raw();
        return nera_rt_error(ctx, reason);
    }

    // fn_ptr == 0 means this is a remote fun ref (fun M:F/A).
    // env[0] = module atom, env[1] = function atom.
    // Dispatch via nera_rt_apply.
    if fn_ptr == 0 {
        let mod_atom = unsafe { *boxed_ptr.add(3) };
        let fun_atom = unsafe { *boxed_ptr.add(4) };

        // Build an Erlang list from the user arguments (in reverse for proper order)
        let mut args_list = ErlTerm::nil();
        for i in (0..nargs as usize).rev() {
            let arg = unsafe { *args_ptr.add(i) };
            args_list = with_ctx_heap(ctx, |heap| {
                heap.alloc_cons(ErlTerm(arg), args_list)
            });
        }

        return super::apply::apply_impl(ctx, mod_atom, fun_atom, args_list.0, resolve);
    }

    // Build combined args: [env0, env1, ..., arg0, arg1, ...]
    let total_args = env_count + nargs as usize;
    let mut combined_args: Vec<u64> = Vec::with_capacity(total_args);

    // Copy env vars from the closure
    for i in 0..env_count {
        combined_args.push(unsafe { *boxed_ptr.add(3 + i) });
    }

    // Copy user args
    for i in 0..nargs as usize {
        combined_args.push(unsafe { *args_ptr.add(i) });
    }

    // Cast fn_ptr to ErlFn and call
    let func: crate::process::ErlFn =
        unsafe { std::mem::transmute(fn_ptr) };
    // Reentry boundary: only copied words remain, no context/heap references.
    // The callee must root arguments before any collection; do not read the
    // original closure or argument storage after this call.
    // SAFETY: the context and argument storage are live and exclusively
    // accessible here; no heap borrow crosses this reentry boundary.
    unsafe { func(ctx, combined_args.as_ptr(), total_args as u32); }
    // Resolve any pending tail calls before returning to caller
    if resolve { unsafe { super::helpers::resolve_tail_calls(ctx); } }

    // Return result (set by the called wrapper)
    if !ctx.is_null() {
        unsafe { (*ctx).result }
    } else {
        ErlTerm::nil().0
    }
}

/// Tail-call variant of `nera_rt_call_closure`: takes arguments as an Erlang
/// list (heap-allocated, survives across stack frames) rather than a raw pointer.
/// Called from the trampoline in `run_slice`, not from compiled code.
pub unsafe fn tail_call_closure(
    ctx: *mut ProcessContext,
    closure: u64,
    args_list: u64,
) -> u64 {
    let mut arg_terms: Vec<u64> = Vec::new();
    let mut current = ErlTerm(args_list);
    while current.is_cons() {
        if let Some(ptr) = current.as_cons_ptr() {
            arg_terms.push(unsafe { (*ptr).head.0 });
            current = unsafe { (*ptr).tail };
        } else {
            break;
        }
    }
    call_closure_impl(ctx, closure, arg_terms.as_ptr(), arg_terms.len() as u32, false)
}


/// Resolve the source MFA of a compiled closure or external function reference.
unsafe fn closure_mfa(fun: ErlTerm) -> Option<(u32, u32, u32)> {
    if !fun.is_closure() { return None; }
    let ptr = fun.as_boxed_ptr()?;
    let function = unsafe { *ptr.add(1) };
    let arity = unsafe { *ptr.add(2) } as u32;
    if function == 0 {
        return Some((unsafe { ErlTerm(*ptr.add(3)) }.as_atom_id()?,
                     unsafe { ErlTerm(*ptr.add(4)) }.as_atom_id()?, arity));
    }
    let table = crate::entry::global_function_table().table.read();
    table.iter().find_map(|(&(module, name, _), &wrapper)|
        (wrapper as usize as u64 == function).then_some((module, name, arity)))
}

/// The metadata fields required by EUnit: module, name and user arity.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_fun_info_2(ctx: *mut ProcessContext, fun: u64, item: u64) -> u64 {
    let Some((module, name, arity)) = (unsafe { closure_mfa(ErlTerm(fun)) }) else { return raise_badarg(ctx); };
    let Some(key) = ErlTerm(item).as_atom_id().and_then(|id| global_atom_table().resolve(id)) else { return raise_badarg(ctx); };
    let value = match key.as_str() {
        "module" => ErlTerm::atom(module),
        "name" => ErlTerm::atom(name),
        "arity" => ErlTerm::small_int(arity as i64),
        _ => return raise_badarg(ctx),
    };
    with_ctx_heap(ctx, |heap| heap.alloc_tuple(&[ErlTerm(item), value]).0)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_fun_info_mfa(ctx: *mut ProcessContext, fun: u64) -> u64 {
    let Some((module, name, arity)) = (unsafe { closure_mfa(ErlTerm(fun)) }) else { return raise_badarg(ctx); };
    with_ctx_heap(ctx, |heap| heap.alloc_tuple(&[ErlTerm::atom(module), ErlTerm::atom(name), ErlTerm::small_int(arity as i64)]).0)
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use crate::heap::ProcessHeap;

    // Simulate generated code reentering allocation and collecting its arguments.
    extern "C" fn reenter_and_collect(ctx: *mut ProcessContext, args: *const u64, count: u32) {
        assert_eq!(count, 1);
        unsafe {
            let mut roots = [ErlTerm::from_raw(*args)];
            super::super::alloc::nera_rt_alloc_float(ctx, 1.5f64.to_bits());
            with_ctx_heap(ctx, |heap| unsafe { heap.collect_full(&mut roots) });
            (*ctx).result = roots[0].to_raw();
        }
    }

    #[test]
    fn closure_reentry_releases_heap_before_collection() {
        for use_process_heap in [false, true] {
            let mut heap = ProcessHeap::new();
            let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
            if use_process_heap {
                ctx.heap_ptr = &mut heap;
            }
            // SAFETY: this test exclusively owns the context and backing heap.
            // Arguments are copied and rooted by the simulated callee before GC.
            unsafe {
                let captured =
                    with_ctx_heap(&mut ctx, |heap| heap.alloc_tuple(&[ErlTerm::small_int(42)]));
                let closure = nera_rt_alloc_closure(
                    &mut ctx,
                    reenter_and_collect as *const () as u64,
                    0,
                    &captured.to_raw(),
                    1,
                );
                let result =
                    ErlTerm::from_raw(nera_rt_call_closure(&mut ctx, closure, std::ptr::null(), 0));
                let ptr = result.as_boxed_ptr().unwrap();
                assert_eq!(BoxedHeader(*ptr).arity(), 1);
                assert_eq!(ErlTerm::from_raw(*ptr.add(1)).as_small_int(), Some(42));
                // The context's heap remains usable after returning through FFI.
                with_ctx_heap(&mut ctx, |heap| heap.alloc_float(2.0));
            }
        }
    }
}
