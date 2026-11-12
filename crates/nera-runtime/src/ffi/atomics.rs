//! Typed direct resource access and 64-bit integer conversions for atomics.

use std::sync::atomic::Ordering;

use super::helpers::*;
use crate::atom_table::{global_atom_table, ATOM_FALSE, ATOM_OK, ATOM_TRUE};
use crate::atomics_module::AtomicArray;
use crate::heap::MapPairs;
use crate::process::ProcessContext;
use crate::resource::ResourceRef;
use crate::term::*;
use num_traits::ToPrimitive;

#[inline(always)]
unsafe fn extract_array(ctx: *mut ProcessContext, term: u64) -> Option<ResourceRef<AtomicArray>> {
    let resource = ResourceRef::from_term(ErlTerm(term));
    if resource.is_none() {
        raise_badarg(ctx);
    }
    resource
}

#[inline(always)]
unsafe fn extract_index(ctx: *mut ProcessContext, term: u64) -> usize {
    match ErlTerm(term).as_small_int() {
        Some(n) if n >= 1 => n as usize,
        _ => {
            raise_badarg(ctx);
            0
        }
    }
}

/// Storage values have the array's signedness; arithmetic deltas have their
/// own domain. Small nonnegative values are valid in both storage modes.
#[inline]
unsafe fn storage_value(term: ErlTerm, signed: bool) -> Option<i64> {
    if let Some(value) = term.as_small_int() {
        return if value >= 0 || signed {
            Some(value)
        } else {
            None
        };
    }
    large_storage_value(term, signed)
}

#[cold]
#[inline(never)]
unsafe fn large_storage_value(term: ErlTerm, signed: bool) -> Option<i64> {
    let value = crate::bigint::term_to_bigint(term)?;
    if signed {
        value.to_i64()
    } else {
        value.to_u64().map(|v| v as i64)
    }
}

#[inline]
unsafe fn arithmetic_delta(term: ErlTerm, subtract: bool) -> Option<i64> {
    if let Some(value) = term.as_small_int() {
        return Some(value);
    }
    large_arithmetic_delta(term, subtract)
}

#[cold]
#[inline(never)]
unsafe fn large_arithmetic_delta(term: ErlTerm, subtract: bool) -> Option<i64> {
    let value = crate::bigint::term_to_bigint(term)?;
    // OTP accepts additions in [-2^63, 2^64-1]. Subtraction validates the
    // negated delta against that domain, hence [-2^64+1, 2^63].
    let addition = if subtract { -value } else { value };
    let bits = addition
        .to_i64()
        .or_else(|| addition.to_u64().map(|v| v as i64))?;
    Some(if subtract { bits.wrapping_neg() } else { bits })
}

#[inline]
unsafe fn value_term(ctx: *mut ProcessContext, value: i64, signed: bool) -> u64 {
    // An unsigned comparison proves both small-integer bounds for the common
    // nonnegative case, independently of the resource's signedness.
    if (value as u64) <= SMALL_INT_MAX as u64 {
        return ErlTerm::small_int(value).0;
    }
    if signed {
        if let Some(term) = ErlTerm::try_small_int(value) {
            return term.0;
        }
    }
    large_value_term(ctx, value, signed)
}

#[cold]
#[inline(never)]
unsafe fn large_value_term(ctx: *mut ProcessContext, value: i64, signed: bool) -> u64 {
    with_ctx_heap(ctx, |heap| {
        crate::bigint::bigint_to_term(
            heap,
            if signed {
                value.into()
            } else {
                (value as u64).into()
            },
        )
        .0
    })
}

unsafe fn signed_option(mut options: ErlTerm) -> Option<bool> {
    let at = global_atom_table();
    let mut signed = true;
    while !options.is_nil() {
        let cons = options.as_cons_ptr()?;
        let option = (*cons).head.as_boxed_ptr()?;
        if BoxedHeader(*option).0 != BoxedHeader::new(BoxedSubtype::Tuple, 2).0 {
            return None;
        }
        let key = ErlTerm(*option.add(1)).as_atom_id()?;
        if at.resolve(key)? != "signed" {
            return None;
        }
        signed = match ErlTerm(*option.add(2)).as_atom_id()? {
            ATOM_TRUE => true,
            ATOM_FALSE => false,
            _ => return None,
        };
        options = (*cons).tail;
    }
    Some(signed)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_atomics_new(
    ctx: *mut ProcessContext,
    size_term: u64,
    opts_term: u64,
) -> u64 {
    let size = match ErlTerm(size_term).as_small_int() {
        Some(n) if n > 0 => n as usize,
        _ => return raise_badarg(ctx),
    };
    let Some(signed) = signed_option(ErlTerm(opts_term)) else {
        return raise_badarg(ctx);
    };
    let resource = crate::atomics_module::atomics_new(size, signed);
    with_ctx_heap(ctx, |heap| resource.to_term(heap).0)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_atomics_get(
    ctx: *mut ProcessContext,
    ref_term: u64,
    ix_term: u64,
) -> u64 {
    let Some(resource) = extract_array(ctx, ref_term) else {
        return ErlTerm::nil().0;
    };
    let ix = extract_index(ctx, ix_term);
    if ix == 0 {
        return ErlTerm::nil().0;
    }
    let arr = resource.get();
    let Some(slot) = arr.slot(ix) else {
        return raise_badarg(ctx);
    };
    value_term(
        ctx,
        slot.load(Ordering::Relaxed),
        resource.signed(),
    )
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_atomics_put(
    ctx: *mut ProcessContext,
    ref_term: u64,
    ix_term: u64,
    val_term: u64,
) -> u64 {
    let Some(resource) = extract_array(ctx, ref_term) else {
        return ErlTerm::nil().0;
    };
    let ix = extract_index(ctx, ix_term);
    if ix == 0 {
        return ErlTerm::nil().0;
    }
    let arr = resource.get();
    let Some(slot) = arr.slot(ix) else {
        return raise_badarg(ctx);
    };
    let Some(value) = storage_value(ErlTerm(val_term), resource.signed()) else {
        return raise_badarg(ctx);
    };
    slot.store(value, Ordering::Relaxed);
    ErlTerm::atom(ATOM_OK).0
}

/// Keep boxed delta conversion outside the entire small-integer operation,
/// so the common path need not preserve its resource/index across a cold call.
#[inline(always)]
unsafe fn update_array(
    ctx: *mut ProcessContext,
    ref_term: u64,
    ix_term: u64,
    delta: i64,
    subtract: bool,
    return_value: bool,
) -> u64 {
    let Some(resource) = extract_array(ctx, ref_term) else {
        return ErlTerm::nil().0;
    };
    let ix = extract_index(ctx, ix_term);
    if ix == 0 {
        return ErlTerm::nil().0;
    }
    let arr = resource.get();
    let Some(slot) = arr.slot(ix) else {
        return raise_badarg(ctx);
    };
    let ordering = if return_value {
        Ordering::AcqRel
    } else {
        Ordering::Relaxed
    };
    let old = if subtract {
        slot.fetch_sub(delta, ordering)
    } else {
        slot.fetch_add(delta, ordering)
    };
    if return_value {
        let value = if subtract {
            old.wrapping_sub(delta)
        } else {
            old.wrapping_add(delta)
        };
        value_term(ctx, value, resource.signed())
    } else {
        ErlTerm::atom(ATOM_OK).0
    }
}

#[cold]
#[inline(never)]
unsafe fn large_update_array(
    ctx: *mut ProcessContext,
    ref_term: u64,
    ix_term: u64,
    delta_term: u64,
    subtract: bool,
    return_value: bool,
) -> u64 {
    let Some(delta) = arithmetic_delta(ErlTerm(delta_term), subtract) else {
        return raise_badarg(ctx);
    };
    update_array(ctx, ref_term, ix_term, delta, subtract, return_value)
}

#[inline(always)]
unsafe fn update(
    ctx: *mut ProcessContext,
    ref_term: u64,
    ix_term: u64,
    delta_term: u64,
    subtract: bool,
    return_value: bool,
) -> u64 {
    if let Some(delta) = ErlTerm(delta_term).as_small_int() {
        update_array(ctx, ref_term, ix_term, delta, subtract, return_value)
    } else {
        large_update_array(ctx, ref_term, ix_term, delta_term, subtract, return_value)
    }
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_atomics_add(
    ctx: *mut ProcessContext,
    ref_term: u64,
    ix_term: u64,
    delta_term: u64,
) -> u64 {
    update(ctx, ref_term, ix_term, delta_term, false, false)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_atomics_sub(
    ctx: *mut ProcessContext,
    ref_term: u64,
    ix_term: u64,
    delta_term: u64,
) -> u64 {
    update(ctx, ref_term, ix_term, delta_term, true, false)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_atomics_add_get(
    ctx: *mut ProcessContext,
    ref_term: u64,
    ix_term: u64,
    delta_term: u64,
) -> u64 {
    update(ctx, ref_term, ix_term, delta_term, false, true)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_atomics_sub_get(
    ctx: *mut ProcessContext,
    ref_term: u64,
    ix_term: u64,
    delta_term: u64,
) -> u64 {
    update(ctx, ref_term, ix_term, delta_term, true, true)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_atomics_exchange(
    ctx: *mut ProcessContext,
    ref_term: u64,
    ix_term: u64,
    desired_term: u64,
) -> u64 {
    let Some(resource) = extract_array(ctx, ref_term) else {
        return ErlTerm::nil().0;
    };
    let ix = extract_index(ctx, ix_term);
    if ix == 0 {
        return ErlTerm::nil().0;
    }
    let arr = resource.get();
    let Some(slot) = arr.slot(ix) else {
        return raise_badarg(ctx);
    };
    let Some(desired) = storage_value(ErlTerm(desired_term), resource.signed()) else {
        return raise_badarg(ctx);
    };
    let old = slot.swap(desired, Ordering::AcqRel);
    value_term(ctx, old, resource.signed())
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_atomics_compare_exchange(
    ctx: *mut ProcessContext,
    ref_term: u64,
    ix_term: u64,
    expected_term: u64,
    desired_term: u64,
) -> u64 {
    let Some(resource) = extract_array(ctx, ref_term) else {
        return ErlTerm::nil().0;
    };
    let ix = extract_index(ctx, ix_term);
    if ix == 0 {
        return ErlTerm::nil().0;
    }
    let arr = resource.get();
    let Some(slot) = arr.slot(ix) else {
        return raise_badarg(ctx);
    };
    let Some(expected) = storage_value(ErlTerm(expected_term), resource.signed()) else {
        return raise_badarg(ctx);
    };
    let Some(desired) = storage_value(ErlTerm(desired_term), resource.signed()) else {
        return raise_badarg(ctx);
    };
    match slot.compare_exchange(expected, desired, Ordering::AcqRel, Ordering::Acquire)
    {
        Ok(_) => ErlTerm::atom(ATOM_OK).0,
        Err(actual) => value_term(ctx, actual, resource.signed()),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_atomics_info(ctx: *mut ProcessContext, ref_term: u64) -> u64 {
    let Some(resource) = extract_array(ctx, ref_term) else {
        return ErlTerm::nil().0;
    };
    let arr = resource.get();
    let at = global_atom_table();
    let keys = ["size", "max", "min", "memory"].map(|key| ErlTerm::atom(at.intern(key)));
    // The resource allocation is independent of the movable process heap.
    // Allocate bounds and the final map in one non-reentrant heap borrow.
    with_ctx_heap(ctx, |heap| {
        let max = crate::bigint::bigint_to_term(
            heap,
            if resource.signed() {
                i64::MAX.into()
            } else {
                u64::MAX.into()
            },
        );
        let min = crate::bigint::bigint_to_term(
            heap,
            if resource.signed() {
                i64::MIN.into()
            } else {
                0.into()
            },
        );
        let size = crate::bigint::bigint_to_term(heap, arr.len().into());
        let memory = crate::bigint::bigint_to_term(
            heap,
            arr.memory_size().into(),
        );
        heap.alloc_map(MapPairs::new(&[
            [keys[0], size],
            [keys[1], max],
            [keys[2], min],
            [keys[3], memory],
        ]))
        .0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::ProcessHeap;

    #[test]
    fn machine_integer_domains_match_storage_and_delta_rules() {
        let mut heap = ProcessHeap::new();
        for (value, signed, unsigned, add, sub) in [
            (
                -(num_bigint::BigInt::from(1) << 64usize) + 1,
                None,
                None,
                None,
                Some(1),
            ),
            (
                num_bigint::BigInt::from(i64::MIN) - 1,
                None,
                None,
                None,
                Some(i64::MAX),
            ),
            (
                i64::MIN.into(),
                Some(i64::MIN),
                None,
                Some(i64::MIN),
                Some(i64::MIN),
            ),
            ((-1).into(), Some(-1), None, Some(-1), Some(-1)),
            (
                i64::MAX.into(),
                Some(i64::MAX),
                Some(i64::MAX),
                Some(i64::MAX),
                Some(i64::MAX),
            ),
            (
                (num_bigint::BigInt::from(1) << 63usize),
                None,
                Some(i64::MIN),
                Some(i64::MIN),
                Some(i64::MIN),
            ),
            (u64::MAX.into(), None, Some(-1), Some(-1), None),
            (
                num_bigint::BigInt::from(u64::MAX) + 1,
                None,
                None,
                None,
                None,
            ),
        ] {
            let term = crate::bigint::bigint_to_term(&mut heap, value);
            // SAFETY: terms remain live and immutable in this local heap.
            unsafe {
                assert_eq!(storage_value(term, true), signed);
                assert_eq!(storage_value(term, false), unsigned);
                assert_eq!(arithmetic_delta(term, false), add);
                assert_eq!(arithmetic_delta(term, true), sub);
            }
        }
    }
}
