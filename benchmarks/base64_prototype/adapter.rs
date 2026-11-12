//! Prototype ABI bridge, installed only in an isolated benchmark workspace.
use super::helpers::{binary_data, raise_badarg, with_ctx_heap};
use crate::{
    atom_table::global_atom_table, binary::BinaryData, process::ProcessContext, term::ErlTerm,
};
use std::sync::Arc;
#[path = "base64_codec.rs"]
mod codec;

unsafe fn convert(ctx: *mut ProcessContext, input: u64, url: u64, pad: u64, decode: bool) -> u64 {
    // SAFETY: generated calls provide live terms and an exclusive context.
    // No collection or calls into compiled Erlang occur while input is borrowed.
    let Some((ptr, len)) = (unsafe { binary_data(ErlTerm(input)) }) else {
        return unsafe { raise_badarg(ctx) };
    };
    let yes = ErlTerm::atom(global_atom_table().intern("true")).0;
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let result = if decode {
        codec::decode(bytes, url == yes, pad == yes)
    } else {
        Some(codec::encode(bytes, url == yes, pad == yes))
    };
    match result {
        None => ErlTerm::atom(global_atom_table().intern("fallback")).0,
        Some(out) => unsafe {
            with_ctx_heap(ctx, |heap| {
                if out.len() <= 64 {
                    heap.alloc_binary(&out).0
                } else {
                    let len = out.len();
                    heap.alloc_refc_binary_from_arc(Arc::new(BinaryData::new(out)), 0, len)
                        .0
                }
            })
        },
    }
}
#[no_mangle]
pub unsafe extern "C" fn nera_rt_base64_encode(
    ctx: *mut ProcessContext,
    input: u64,
    url: u64,
    pad: u64,
) -> u64 {
    unsafe { convert(ctx, input, url, pad, false) }
}
#[no_mangle]
pub unsafe extern "C" fn nera_rt_base64_decode(
    ctx: *mut ProcessContext,
    input: u64,
    url: u64,
    pad: u64,
) -> u64 {
    unsafe { convert(ctx, input, url, pad, true) }
}
#[no_mangle]
pub unsafe extern "C" fn nera_rt_base64_start(_ctx: *mut ProcessContext) -> u64 {
    crate::base64_metrics::start();
    ErlTerm::nil().0
}
#[no_mangle]
pub unsafe extern "C" fn nera_rt_base64_stop(ctx: *mut ProcessContext) -> u64 {
    let values = crate::base64_metrics::stop();
    unsafe {
        with_ctx_heap(ctx, |heap| {
            heap.alloc_tuple(&values.map(|v| ErlTerm::small_int(v as i64)))
                .0
        })
    }
}
