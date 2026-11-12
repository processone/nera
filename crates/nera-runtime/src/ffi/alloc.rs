//! Allocation functions: cons cells, tuples, floats, bigints.
//!
//! # Safety
//! Entry points require the context/heap invariants of `with_ctx_heap` and
//! live input terms. Slow paths additionally require a non-null writable
//! context. Tuple input arrays must be readable for `arity` words. These
//! operations allocate without collecting or invoking compiled code.

#![deny(unsafe_op_in_unsafe_fn)]

use super::helpers::with_ctx_heap;
use crate::process::ProcessContext;
use crate::term::ErlTerm;

/// Allocate a cons cell on the process heap.
/// Returns the tagged ErlTerm (as u64).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_alloc_cons(ctx: *mut ProcessContext, head: u64, tail: u64) -> u64 {
    // SAFETY: the FFI caller provides an exclusively accessible process heap.
    unsafe {
        with_ctx_heap(ctx, |heap| {
            heap.alloc_cons(ErlTerm::from_raw(head), ErlTerm::from_raw(tail))
                .to_raw()
        })
    }
}

/// Slow path for inline cons allocation: allocate a cons cell from the heap,
/// refill the inline region, and update ctx.heap_cursor / ctx.heap_limit.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_alloc_cons_slow(
    ctx: *mut ProcessContext,
    head: u64,
    tail: u64,
) -> u64 {
    // End the heap borrow before updating the context.
    let (result, (cursor, limit)) = unsafe {
        with_ctx_heap(ctx, |heap| {
            let result = heap.alloc_cons(ErlTerm::from_raw(head), ErlTerm::from_raw(tail));
            (result, heap.refill_inline_region())
        })
    };
    unsafe { publish_inline_region(ctx, cursor, limit) };
    result.to_raw()
}

/// Allocate a tuple. The elements are passed as a pointer to an array of u64.
/// `arity` is the number of elements.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_alloc_tuple(
    ctx: *mut ProcessContext,
    arity: u32,
    elements: *const u64,
) -> u64 {
    let elems = unsafe { copy_terms(elements, arity) };
    unsafe { with_ctx_heap(ctx, |heap| heap.alloc_tuple(&elems).to_raw()) }
}

/// Slow path for inline tuple allocation: allocate a tuple from the heap,
/// refill the inline region, and update ctx.heap_cursor / ctx.heap_limit.
/// Elements are passed as a pointer to a stack-allocated array of u64.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_alloc_tuple_slow(
    ctx: *mut ProcessContext,
    arity: u32,
    elements: *const u64,
) -> u64 {
    let elems = unsafe { copy_terms(elements, arity) };
    let (result, (cursor, limit)) = unsafe {
        with_ctx_heap(ctx, |heap| {
            let result = heap.alloc_tuple(&elems);
            (result, heap.refill_inline_region())
        })
    };
    unsafe { publish_inline_region(ctx, cursor, limit) };
    result.to_raw()
}

/// Slow path for inline `BsMatchStart2`: create or reuse a match context.
/// Refill inline capacity only after allocating a fresh context. Reuse and
/// non-binary rejection do not consume inline space and must not abandon it.
/// Called when the inline fast path cannot handle the input (match_ctx reuse,
/// non-binary input, or inline region exhausted).
/// Returns base_ptr; stores total_bits in ctx.result, match_ctx in ctx.result2.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bs_start_match2_slow(ctx: *mut ProcessContext, input: u64) -> u64 {
    // No context or heap reference survives the nested FFI call.
    let base_ptr = unsafe { super::binary_ops::nera_rt_bs_start_match2(ctx, input) };
    // The callee returns the input context unchanged on reuse, zero on rejection,
    // and a distinct newly allocated context for a fresh binary/bitstring.
    // No collection occurs in that call, so raw term identity is stable here.
    let allocated_context = unsafe { (*ctx).result2 != 0 && (*ctx).result2 != input };
    if allocated_context {
        let (cursor, limit) = unsafe { with_ctx_heap(ctx, |heap| heap.refill_inline_region()) };
        unsafe { publish_inline_region(ctx, cursor, limit) };
    }
    base_ptr
}

/// Allocate a float on the process heap.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_alloc_float(ctx: *mut ProcessContext, bits: u64) -> u64 {
    unsafe { with_ctx_heap(ctx, |heap| heap.alloc_float(f64::from_bits(bits)).to_raw()) }
}

/// Allocate a bigint from a raw i64 value on the process heap.
/// Used by codegen for integer literals that exceed the small int range.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_alloc_bigint_i64(ctx: *mut ProcessContext, val: i64) -> u64 {
    use num_bigint::BigInt;
    unsafe {
        with_ctx_heap(ctx, |heap| {
            crate::bigint::bigint_to_term(heap, BigInt::from(val)).to_raw()
        })
    }
}

/// Allocate an arbitrary-precision literal from canonical decimal object data.
///
/// # Safety
/// `ptr` must point to `len` readable bytes containing a valid canonical integer.
/// Generated object data remains live for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_alloc_bigint_decimal(
    ctx: *mut ProcessContext, ptr: *const u8, len: usize,
) -> u64 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let value = num_bigint::BigInt::parse_bytes(bytes, 10)
        .expect("compiler-generated integer literal must be valid");
    unsafe { with_ctx_heap(ctx, |heap| crate::bigint::bigint_to_term(heap, value).to_raw()) }
}

/// Copy ABI words into typed values before borrowing the destination heap.
/// Zero elements permit a null pointer, as in the generated empty-tuple path.
///
/// # Safety
/// For nonzero count, `ptr` points to `count` initialized, readable words.
unsafe fn copy_terms(ptr: *const u64, count: u32) -> Vec<ErlTerm> {
    let words = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ptr, count as usize) }
    };
    words.iter().copied().map(ErlTerm::from_raw).collect()
}

/// # Safety
/// `ctx` points to a writable context; the region belongs to its process heap.
unsafe fn publish_inline_region(ctx: *mut ProcessContext, cursor: *mut u64, limit: *mut u64) {
    let ctx = unsafe { &mut *ctx };
    ctx.heap_cursor = cursor;
    ctx.heap_limit = limit;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::ProcessHeap;
    use crate::term::BoxedHeader;

    #[test]
    fn reused_match_context_does_not_reserve_another_inline_region() {
        for refcounted in [false, true] {
            let mut heap = ProcessHeap::new();
            let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
            ctx.heap_ptr = &mut heap;
            let bytes = [42; 128];
            let binary = if refcounted { heap.alloc_binary_auto(&bytes) }
                         else { heap.alloc_binary(&bytes) };
            // SAFETY: all terms belong to this live, exclusive heap; no GC runs.
            unsafe {
                let base = nera_rt_bs_start_match2_slow(&mut ctx, binary.0);
                let mc = ErlTerm(ctx.result2);
                *mc.as_boxed_ptr().unwrap().cast_mut().add(2) = 24;
                let region = (ctx.heap_cursor, ctx.heap_limit);
                let before = heap.bytes_allocated();
                for _ in 0..1024 {
                    assert_eq!(nera_rt_bs_start_match2_slow(&mut ctx, mc.0), base + 3);
                    assert_eq!(ctx.result2, mc.0);
                    assert_eq!(ErlTerm(ctx.result).as_small_int(), Some(1000));
                }
                assert_eq!(heap.bytes_allocated(), before,
                    "reusing a match context must not allocate or abandon inline capacity");
                assert_eq!((ctx.heap_cursor, ctx.heap_limit), region);
                // Reuse also needs no reservation after a GC clears the inline cursor.
                ctx.heap_cursor = std::ptr::null_mut();
                ctx.heap_limit = std::ptr::null_mut();
                assert_eq!(nera_rt_bs_start_match2_slow(&mut ctx, mc.0), base + 3);
                assert_eq!(heap.bytes_allocated(), before);
                assert!(ctx.heap_cursor.is_null());
            }
        }
    }

    #[test]
    fn failed_match_start_preserves_inline_capacity() {
        let mut heap = ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let (cursor, limit) = heap.refill_inline_region();
        ctx.heap_cursor = cursor;
        ctx.heap_limit = limit;
        let before = heap.bytes_allocated();
        // SAFETY: live exclusive heap/context; invalid terms are valid immediates.
        unsafe {
            for input in [ErlTerm::small_int(42), ErlTerm::nil()] {
                assert_eq!(nera_rt_bs_start_match2_slow(&mut ctx, input.0), 0);
                assert_eq!(ctx.result2, 0);
                assert_eq!(ErlTerm(ctx.result).as_small_int(), Some(0));
            }
        }
        assert_eq!(heap.bytes_allocated(), before);
        assert_eq!((ctx.heap_cursor, ctx.heap_limit), (cursor, limit));
    }

    #[test]
    fn fresh_binary_match_replenishes_exhausted_inline_capacity() {
        let mut heap = ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let binary = heap.alloc_binary(&[1, 2, 3]);
        let (_, limit) = heap.refill_inline_region();
        ctx.heap_cursor = limit;
        ctx.heap_limit = limit;
        // SAFETY: live exclusive heap/context, no GC, valid binary input.
        unsafe {
            assert_ne!(nera_rt_bs_start_match2_slow(&mut ctx, binary.0), 0);
            assert!(ErlTerm(ctx.result2).is_match_ctx());
            assert_eq!(ErlTerm(ctx.result).as_small_int(), Some(24));
        }
        assert!(!ctx.heap_cursor.is_null());
        assert!(ctx.heap_cursor < ctx.heap_limit);
    }

    #[test]
    fn tuple_adapters_accept_empty_null_array_and_publish_region() {
        let mut heap = ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        unsafe {
            let empty = ErlTerm::from_raw(nera_rt_alloc_tuple(&mut ctx, 0, std::ptr::null()));
            assert_eq!(BoxedHeader(*empty.as_boxed_ptr().unwrap()).arity(), 0);
            let elems = [ErlTerm::small_int(7).to_raw(), ErlTerm::nil().to_raw()];
            let tuple = ErlTerm::from_raw(nera_rt_alloc_tuple_slow(&mut ctx, 2, elems.as_ptr()));
            assert_eq!(*tuple.as_boxed_ptr().unwrap().add(1), elems[0]);
            assert_eq!(*tuple.as_boxed_ptr().unwrap().add(2), elems[1]);
            assert!(!ctx.heap_cursor.is_null());
            assert!(ctx.heap_cursor < ctx.heap_limit);
        }
    }
}
