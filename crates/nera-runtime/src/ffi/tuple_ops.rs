//! Tuple operations: element access, tuple_size.

#![allow(unused_unsafe)]

use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;

/// Get a tuple element (0-indexed in the FFI, unlike Erlang's 1-indexed).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_tuple_element(ctx: *mut ProcessContext, tuple: u64, index: u32) -> u64 {
    let term = ErlTerm(tuple);
    match term.as_boxed_ptr() {
        Some(ptr) => unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_TUPLE || (index as u64) >= header.arity() {
                return raise_badarg(ctx);
            }

            *ptr.add(1 + index as usize)
        },
        None => {
            raise_badarg(ctx)
        }
    }
}

/// Get tuple size.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_tuple_size(ctx: *mut ProcessContext, tuple: u64) -> u64 {
    let term = ErlTerm(tuple);
    match term.as_boxed_ptr() {
        Some(ptr) => unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_TUPLE {
                return raise_badarg(ctx);
            }
            ErlTerm::small_int(header.arity() as i64).0
        },
        None => raise_badarg(ctx),
    }
}

