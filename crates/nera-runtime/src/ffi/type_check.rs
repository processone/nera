//! Type check predicates (`is_*`) and simple numeric BIFs (`abs`, `max`, `min`).

#![allow(unused_unsafe)]

use crate::atom_table::global_atom_table;
use crate::bif;
use crate::process::ProcessContext;
use crate::term::ErlTerm;
use super::helpers::*;


/// Type check BIFs
#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_integer(_ctx: *mut ProcessContext, term: u64) -> u64 {
    bif::bif_is_integer(&[ErlTerm(term)]).0
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_atom(_ctx: *mut ProcessContext, term: u64) -> u64 {
    bif::bif_is_atom(&[ErlTerm(term)]).0
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_list(_ctx: *mut ProcessContext, term: u64) -> u64 {
    bif::bif_is_list(&[ErlTerm(term)]).0
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_tuple(_ctx: *mut ProcessContext, term: u64) -> u64 {
    bif::bif_is_tuple(&[ErlTerm(term)]).0
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_pid(_ctx: *mut ProcessContext, term: u64) -> u64 {
    bif::bif_is_pid(&[ErlTerm(term)]).0
}

/// Check if a term is a byte-aligned binary (or match context — transparent to Erlang).
/// Returns atom 'true' or 'false'.
/// For sub-byte bitstrings, returns false.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_binary(_ctx: *mut ProcessContext, term: u64) -> u64 {
    let t = ErlTerm(term);
    if t.is_binary() || t.is_match_ctx() {
        ErlTerm::atom(2).0 // true
    } else {
        ErlTerm::atom(3).0 // false
    }
}

/// Check if a term is a bitstring (any binary representation, including sub-byte).
/// Returns atom 'true' or 'false'.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_bitstring(_ctx: *mut ProcessContext, term: u64) -> u64 {
    let t = ErlTerm(term);
    if t.is_bitstring() || t.is_match_ctx() {
        ErlTerm::atom(2).0 // true
    } else {
        ErlTerm::atom(3).0 // false
    }
}

/// Check if a term is a map.
/// Returns atom 'true' or 'false'.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_map(_ctx: *mut ProcessContext, term: u64) -> u64 {
    bif::bif_is_map(&[ErlTerm(term)]).0
}

/// erlang:is_function/1 — returns true if term is a closure.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_function(_ctx: *mut ProcessContext, term: u64) -> u64 {
    if ErlTerm(term).is_closure() {
        ErlTerm::atom(2).0 // true
    } else {
        ErlTerm::atom(3).0 // false
    }
}

/// erlang:is_function/2 — returns true if term is a closure with the given arity.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_function2(_ctx: *mut ProcessContext, term: u64, arity: u64) -> u64 {
    let t = ErlTerm(term);
    if !t.is_closure() {
        return ErlTerm::atom(3).0; // false
    }
    let expected = match ErlTerm(arity).as_small_int() {
        Some(n) => n as u64,
        None => return ErlTerm::atom(3).0, // false
    };
    let boxed_ptr = t.as_boxed_ptr().unwrap();
    let closure_arity = *boxed_ptr.add(2); // word[2] = arity
    if closure_arity == expected {
        ErlTerm::atom(2).0 // true
    } else {
        ErlTerm::atom(3).0 // false
    }
}

/// Check if a term is a reference.
/// Returns atom 'true' or 'false'.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_reference(_ctx: *mut ProcessContext, term: u64) -> u64 {
    let t = ErlTerm(term);
    let atom_table = global_atom_table();
    if t.is_reference() {
        let id = atom_table.intern("true");
        ErlTerm::atom(id).0
    } else {
        let id = atom_table.intern("false");
        ErlTerm::atom(id).0
    }
}

/// erlang:is_float/1
#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_float(_ctx: *mut ProcessContext, term: u64) -> u64 {
    bif::bif_is_float(&[ErlTerm(term)]).0
}

/// erlang:is_number/1
#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_number(_ctx: *mut ProcessContext, term: u64) -> u64 {
    bif::bif_is_number(&[ErlTerm(term)]).0
}

/// erlang:is_boolean/1
#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_boolean(_ctx: *mut ProcessContext, term: u64) -> u64 {
    bif::bif_is_boolean(&[ErlTerm(term)]).0
}

/// erlang:abs/1 -- absolute value of integer or float.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_abs(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_abs(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:round/1 -- round a number to the nearest integer.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_round(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_round(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:trunc/1 -- truncate a number towards zero.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_trunc(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_trunc(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:phash2/1 -- portable hash of an Erlang term.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_phash2(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_phash2(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:phash2/2 -- portable hash with a custom range.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_phash2_2(ctx: *mut ProcessContext, term: u64, range: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_phash2_range(heap, &[ErlTerm(term), ErlTerm(range)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:size/1 -- tuple_size for tuples, byte_size for binaries.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_size(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_size(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:ceil/1 -- smallest integer not less than the argument.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ceil(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_ceil(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:floor/1 -- largest integer not greater than the argument.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_floor(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_floor(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:float/1 -- convert a number to float.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_float(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_float(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:max/2 -- return the larger of two terms.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_max(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_max(heap, &[ErlTerm(a), ErlTerm(b)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:min/2 -- return the smaller of two terms.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_min(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_min(heap, &[ErlTerm(a), ErlTerm(b)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:term_to_binary/1 -- serialize a term to External Term Format.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_term_to_binary(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_term_to_binary(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:binary_to_term/1 -- deserialize a term from External Term Format.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_to_term(ctx: *mut ProcessContext, bin: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_binary_to_term(heap, &[ErlTerm(bin)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

