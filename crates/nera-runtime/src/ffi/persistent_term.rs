//! FFI bindings for the `persistent_term` module (OTP 21+).

#![allow(unused_unsafe)]

use crate::heap::MapPairs;
use crate::atom_table::{global_atom_table, ATOM_OK, ATOM_TRUE};
use crate::process::ProcessContext;
use crate::term::ErlTerm;
use super::helpers::*;

/// persistent_term:put/2 — store a key-value pair.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_persistent_term_put(_ctx: *mut ProcessContext, key: u64, value: u64) -> u64 {
    crate::persistent_term::persistent_term_put(ErlTerm(key), ErlTerm(value));
    ErlTerm::atom(ATOM_OK).0
}

/// persistent_term:get/1 — retrieve a value by key; badarg if missing.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_persistent_term_get(ctx: *mut ProcessContext, key: u64) -> u64 {
    with_ctx_heap(ctx, |heap| {
        match crate::persistent_term::persistent_term_get(ErlTerm(key), heap) {
            Some(val) => val.0,
            None => raise_badarg(ctx),
        }
    })
}

/// persistent_term:get/2 — retrieve a value by key; return default if missing.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_persistent_term_get_2(ctx: *mut ProcessContext, key: u64, default: u64) -> u64 {
    with_ctx_heap(ctx, |heap| {
        match crate::persistent_term::persistent_term_get(ErlTerm(key), heap) {
            Some(val) => val.0,
            None => default,
        }
    })
}

/// persistent_term:erase/1 — remove a key-value pair.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_persistent_term_erase(_ctx: *mut ProcessContext, key: u64) -> u64 {
    crate::persistent_term::persistent_term_erase(ErlTerm(key));
    ErlTerm::atom(ATOM_TRUE).0
}

/// persistent_term:info/0 — returns #{count => N, memory => M}.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_persistent_term_info(ctx: *mut ProcessContext) -> u64 {
    let (count, memory) = crate::persistent_term::persistent_term_info();
    let at = global_atom_table();
    let count_atom = ErlTerm::atom(at.intern("count"));
    let memory_atom = ErlTerm::atom(at.intern("memory"));
    with_ctx_heap(ctx, |heap| {
        let pairs = [
            [count_atom, ErlTerm::small_int(count as i64)],
            [memory_atom, ErlTerm::small_int(memory as i64)],
        ];
        heap.alloc_map(MapPairs::new(&pairs)).0
    })
}
