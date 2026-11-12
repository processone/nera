//! FFI bindings for the `counters` module (OTP 22+).
//!
//! Hot-path operations (get/put/add/sub) use a raw pointer stored directly
//! in the reference term — no registry lookup, no RwLock, no Arc clone.

#![allow(unused_unsafe)]

use crate::heap::MapPairs;
use crate::atom_table::{global_atom_table, ATOM_OK};
use crate::counters::CounterArray;
use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;

/// Establish the resource type before any native pointer is dereferenced.
#[inline(always)]
unsafe fn extract_array(ctx: *mut ProcessContext, term: u64) -> Option<crate::resource::ResourceRef<CounterArray>> {
    let resource = crate::resource::ResourceRef::from_term(ErlTerm(term));
    if resource.is_none() { raise_badarg(ctx); }
    resource
}

/// Extract a 1-based index from a small integer term.
#[inline(always)]
unsafe fn extract_index(ctx: *mut ProcessContext, ix_term: u64) -> usize {
    match ErlTerm(ix_term).as_small_int() {
        Some(n) if n >= 1 => n as usize,
        _ => {
            raise_badarg(ctx);
            0 // sentinel — caller checks null array ptr first
        }
    }
}

/// counters:new/2 — create a new counter array.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_counters_new(ctx: *mut ProcessContext, size_term: u64, _opts_term: u64) -> u64 {
    let size = match ErlTerm(size_term).as_small_int() {
        Some(n) if n > 0 => n as usize,
        _ => return raise_badarg(ctx),
    };

    let resource = crate::counters::counters_new(size);

    with_ctx_heap(ctx, |heap| resource.to_term(heap).0)
}

/// counters:get/2
#[no_mangle]
pub unsafe extern "C" fn nera_rt_counters_get(ctx: *mut ProcessContext, ref_term: u64, ix_term: u64) -> u64 {
    let Some(resource) = extract_array(ctx, ref_term) else { return ErlTerm::nil().0; };
    let ix = extract_index(ctx, ix_term);
    if ix == 0 { return ErlTerm::nil().0; }
    let arr = resource.get();
    let Some(slot) = arr.slot(ix) else {
        return raise_badarg(ctx);
    };
    integer_term(ctx, slot.load(std::sync::atomic::Ordering::Relaxed))
}

/// counters:put/3
#[no_mangle]
pub unsafe extern "C" fn nera_rt_counters_put(ctx: *mut ProcessContext, ref_term: u64, ix_term: u64, val_term: u64) -> u64 {
    let Some(resource) = extract_array(ctx, ref_term) else { return ErlTerm::nil().0; };
    let ix = extract_index(ctx, ix_term);
    if ix == 0 { return ErlTerm::nil().0; }
    let value = match ErlTerm(val_term).as_small_int() {
        Some(v) => v,
        None => return raise_badarg(ctx),
    };
    let arr = resource.get();
    let Some(slot) = arr.slot(ix) else {
        return raise_badarg(ctx);
    };
    slot.store(value, std::sync::atomic::Ordering::Relaxed);
    ErlTerm::atom(ATOM_OK).0
}

/// counters:add/3
#[no_mangle]
pub unsafe extern "C" fn nera_rt_counters_add(ctx: *mut ProcessContext, ref_term: u64, ix_term: u64, incr_term: u64) -> u64 {
    let Some(resource) = extract_array(ctx, ref_term) else { return ErlTerm::nil().0; };
    let ix = extract_index(ctx, ix_term);
    if ix == 0 { return ErlTerm::nil().0; }
    let incr = match ErlTerm(incr_term).as_small_int() {
        Some(v) => v,
        None => return raise_badarg(ctx),
    };
    let arr = resource.get();
    let Some(slot) = arr.slot(ix) else {
        return raise_badarg(ctx);
    };
    slot.fetch_add(incr, std::sync::atomic::Ordering::Relaxed);
    ErlTerm::atom(ATOM_OK).0
}

/// counters:sub/3
#[no_mangle]
pub unsafe extern "C" fn nera_rt_counters_sub(ctx: *mut ProcessContext, ref_term: u64, ix_term: u64, decr_term: u64) -> u64 {
    let Some(resource) = extract_array(ctx, ref_term) else { return ErlTerm::nil().0; };
    let ix = extract_index(ctx, ix_term);
    if ix == 0 { return ErlTerm::nil().0; }
    let decr = match ErlTerm(decr_term).as_small_int() {
        Some(v) => v,
        None => return raise_badarg(ctx),
    };
    let arr = resource.get();
    let Some(slot) = arr.slot(ix) else {
        return raise_badarg(ctx);
    };
    slot.fetch_sub(decr, std::sync::atomic::Ordering::Relaxed);
    ErlTerm::atom(ATOM_OK).0
}

/// counters:info/1 — returns #{size => N, memory => M}
#[no_mangle]
pub unsafe extern "C" fn nera_rt_counters_info(ctx: *mut ProcessContext, ref_term: u64) -> u64 {
    let Some(resource) = extract_array(ctx, ref_term) else { return ErlTerm::nil().0; };
    let arr = resource.get();
    let at = global_atom_table();
    let size_atom = ErlTerm::atom(at.intern("size"));
    let memory_atom = ErlTerm::atom(at.intern("memory"));
    with_ctx_heap(ctx, |heap| {
        let pairs = [
            [size_atom, ErlTerm::small_int(arr.len() as i64)],
            [memory_atom, ErlTerm::small_int(arr.memory_size() as i64)],
        ];
        heap.alloc_map(MapPairs::new(&pairs)).0
    })
}
