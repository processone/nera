//! Type conversion BIFs: atom_to_list, integer_to_list, etc.

#![allow(unused_unsafe)]

use crate::bif;
use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;

/// erlang:atom_to_list/1
#[no_mangle]
pub unsafe extern "C" fn nera_rt_atom_to_list(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_atom_to_list(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:list_to_atom/1
#[no_mangle]
pub unsafe extern "C" fn nera_rt_list_to_atom(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_list_to_atom(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:integer_to_list/1
#[no_mangle]
pub unsafe extern "C" fn nera_rt_integer_to_list(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_integer_to_list(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:integer_to_list/2 — convert integer to list of digit codepoints in given base.
/// Base must be 2..36.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_integer_to_list_2(
    ctx: *mut ProcessContext,
    term: u64,
    base_term: u64,
) -> u64 {
    use crate::term::ErlTerm;
    let base = match ErlTerm(base_term).as_small_int() {
        Some(b) if (2..=36).contains(&b) => b as u32,
        _ => return raise_badarg(ctx),
    };
    let t = ErlTerm(term);
    let s = if let Some(v) = t.as_small_int() {
        if v < 0 {
            let mag = (-(v as i128)) as u128;
            format!("-{}", super::binary_ops::radix_fmt(mag, base))
        } else {
            super::binary_ops::radix_fmt(v as u128, base)
        }
    } else if let Some(big) = crate::bigint::term_to_bigint(t) {
        use num_traits::Signed;
        if big.is_negative() {
            let mag = (-&big).to_biguint().unwrap();
            format!("-{}", mag.to_str_radix(base))
        } else {
            big.to_biguint().unwrap().to_str_radix(base)
        }
    } else {
        return raise_badarg(ctx);
    };
    // Erlang uses uppercase hex digits
    let s = s.to_ascii_uppercase();
    with_ctx_heap(ctx, |heap| {
        let codepoints: Vec<ErlTerm> = s.chars().map(|c| ErlTerm::small_int(c as i64)).collect();
        heap.alloc_list_from_slice(&codepoints).0
    })
}

/// erlang:list_to_integer/1
#[no_mangle]
pub unsafe extern "C" fn nera_rt_list_to_integer(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_list_to_integer(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:float_to_list/1
#[no_mangle]
pub unsafe extern "C" fn nera_rt_float_to_list(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_float_to_list(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:list_to_tuple/1
#[no_mangle]
pub unsafe extern "C" fn nera_rt_list_to_tuple(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_list_to_tuple(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:tuple_to_list/1
#[no_mangle]
pub unsafe extern "C" fn nera_rt_tuple_to_list(ctx: *mut ProcessContext, term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_tuple_to_list(heap, &[ErlTerm(term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:setelement/3
#[no_mangle]
pub unsafe extern "C" fn nera_rt_setelement(ctx: *mut ProcessContext, index: u64, tuple: u64, value: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_setelement(heap, &[ErlTerm(index), ErlTerm(tuple), ErlTerm(value)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:make_tuple/2
#[no_mangle]
pub unsafe extern "C" fn nera_rt_make_tuple_2(ctx: *mut ProcessContext, arity: u64, init_value: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_make_tuple_2(heap, &[ErlTerm(arity), ErlTerm(init_value)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:make_tuple/3
#[no_mangle]
pub unsafe extern "C" fn nera_rt_make_tuple_3(ctx: *mut ProcessContext, arity: u64, default_value: u64, init_list: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_make_tuple_3(heap, &[ErlTerm(arity), ErlTerm(default_value), ErlTerm(init_list)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:append_element/2
#[no_mangle]
pub unsafe extern "C" fn nera_rt_append_element(ctx: *mut ProcessContext, tuple: u64, element: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_append_element(heap, &[ErlTerm(tuple), ErlTerm(element)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}
