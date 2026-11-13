//! Arithmetic and bitwise operations.

#![allow(unused_unsafe)]

use crate::process::ProcessContext;
use crate::term::ErlTerm;
use crate::atom_table::{ATOM_TRUE, ATOM_FALSE};
use super::helpers::*;

/// math:sqrt/1 — finite nonnegative numbers, coerced to an Erlang float.
///
/// # Safety
/// The context must be live and exclusively accessible, with a valid process
/// heap. Boxed input storage must remain live and unmoved for this call.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_math_sqrt(ctx: *mut ProcessContext, value: u64) -> u64 {
    let term = ErlTerm(value);
    if !is_numeric(term) {
        return raise_badarg(ctx);
    }
    let Some(number) = extract_numeric(term) else {
        return raise_badarith(ctx);
    };
    if !number.is_finite() || number < 0.0 {
        return raise_badarith(ctx);
    }
    with_ctx_heap(ctx, |heap| heap.alloc_float(number.sqrt()).0)
}

/// A float result, or `badarith` when it is infinite or NaN. BEAM raises for
/// float overflow, a zero divisor and math domain errors alike.
unsafe fn float_result(ctx: *mut ProcessContext, result: f64) -> u64 {
    if !result.is_finite() {
        return raise_badarith(ctx);
    }
    with_ctx_heap(ctx, |heap| heap.alloc_float(result).0)
}

/// Both operands of float arithmetic as floats, or `None` when either is not
/// a number or is an integer too large for a float, which BEAM rejects with
/// `badarith` rather than treating as infinity.
unsafe fn float_operands(a: ErlTerm, b: ErlTerm) -> Option<(f64, f64)> {
    let a = extract_numeric(a).filter(|number| number.is_finite())?;
    let b = extract_numeric(b).filter(|number| number.is_finite())?;
    Some((a, b))
}

/// A math argument as a float: nonnumbers raise `badarg`; numbers outside the
/// float range raise `badarith`, as on OTP.
unsafe fn math_argument(term: ErlTerm) -> Result<f64, bool> {
    if !is_numeric(term) {
        return Err(false);
    }
    match extract_numeric(term) {
        Some(number) if number.is_finite() => Ok(number),
        _ => Err(true),
    }
}

unsafe fn raise_math_argument(ctx: *mut ProcessContext, arithmetic: bool) -> u64 {
    if arithmetic { raise_badarith(ctx) } else { raise_badarg(ctx) }
}

/// math:exp/1 — e raised to a number, as a float.
///
/// # Safety
/// The context must be live and exclusively accessible, with a valid process
/// heap. Boxed input storage must remain live and unmoved for this call.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_math_exp(ctx: *mut ProcessContext, value: u64) -> u64 {
    match math_argument(ErlTerm(value)) {
        Ok(number) => float_result(ctx, number.exp()),
        Err(arithmetic) => raise_math_argument(ctx, arithmetic),
    }
}

/// math:log/1 — the natural logarithm of a positive number, as a float.
///
/// # Safety
/// The context must be live and exclusively accessible, with a valid process
/// heap. Boxed input storage must remain live and unmoved for this call.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_math_log(ctx: *mut ProcessContext, value: u64) -> u64 {
    match math_argument(ErlTerm(value)) {
        Ok(number) => float_result(ctx, number.ln()),
        Err(arithmetic) => raise_math_argument(ctx, arithmetic),
    }
}

/// math:pow/2 — X raised to Y, both coerced to floats, as C's pow().
///
/// # Safety
/// The context must be live and exclusively accessible, with a valid process
/// heap. Boxed input storage must remain live and unmoved for this call.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_math_pow(ctx: *mut ProcessContext, base: u64, exponent: u64) -> u64 {
    let (base, exponent) = (ErlTerm(base), ErlTerm(exponent));
    if !is_numeric(base) || !is_numeric(exponent) {
        return raise_badarg(ctx);
    }
    match (math_argument(base), math_argument(exponent)) {
        (Ok(x), Ok(y)) => float_result(ctx, x.powf(y)),
        _ => raise_badarith(ctx),
    }
}

/// Arithmetic add: int+int -> int, float+float -> float, int+float / float+int -> float.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_add(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    use crate::bigint::{term_to_bigint, bigint_to_term};
    let ta = ErlTerm(a);
    let tb = ErlTerm(b);
    // Fast path: both small ints with overflow check
    if let (Some(ai), Some(bi)) = (ta.as_small_int(), tb.as_small_int()) {
        if let Some(result) = ai.checked_add(bi) {
            if crate::term::fits_small_int(result) {
                return ErlTerm::small_int(result).0;
            }
        }
        return with_ctx_heap(ctx, |heap| {
            let r = num_bigint::BigInt::from(ai) + num_bigint::BigInt::from(bi);
            bigint_to_term(heap, r).0
        });
    }
    // Integer operands (at least one bigint)
    if let (Some(a_big), Some(b_big)) = (term_to_bigint(ta), term_to_bigint(tb)) {
        return with_ctx_heap(ctx, |heap| bigint_to_term(heap, a_big + b_big).0);
    }
    // Float coercion (int/float mixed)
    match float_operands(ta, tb) {
        Some((fa, fb)) => float_result(ctx, fa + fb),
        None => raise_badarith(ctx),
    }
}

/// Arithmetic subtract.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_sub(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    use crate::bigint::{term_to_bigint, bigint_to_term};
    let ta = ErlTerm(a);
    let tb = ErlTerm(b);
    if let (Some(ai), Some(bi)) = (ta.as_small_int(), tb.as_small_int()) {
        if let Some(result) = ai.checked_sub(bi) {
            if crate::term::fits_small_int(result) {
                return ErlTerm::small_int(result).0;
            }
        }
        return with_ctx_heap(ctx, |heap| {
            let r = num_bigint::BigInt::from(ai) - num_bigint::BigInt::from(bi);
            bigint_to_term(heap, r).0
        });
    }
    if let (Some(a_big), Some(b_big)) = (term_to_bigint(ta), term_to_bigint(tb)) {
        return with_ctx_heap(ctx, |heap| bigint_to_term(heap, a_big - b_big).0);
    }
    match float_operands(ta, tb) {
        Some((fa, fb)) => float_result(ctx, fa - fb),
        None => raise_badarith(ctx),
    }
}

/// Arithmetic multiply.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_mul(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    use crate::bigint::{term_to_bigint, bigint_to_term};
    let ta = ErlTerm(a);
    let tb = ErlTerm(b);
    if let (Some(ai), Some(bi)) = (ta.as_small_int(), tb.as_small_int()) {
        if let Some(result) = ai.checked_mul(bi) {
            if crate::term::fits_small_int(result) {
                return ErlTerm::small_int(result).0;
            }
        }
        return with_ctx_heap(ctx, |heap| {
            let r = num_bigint::BigInt::from(ai) * num_bigint::BigInt::from(bi);
            bigint_to_term(heap, r).0
        });
    }
    if let (Some(a_big), Some(b_big)) = (term_to_bigint(ta), term_to_bigint(tb)) {
        return with_ctx_heap(ctx, |heap| bigint_to_term(heap, a_big * b_big).0);
    }
    match float_operands(ta, tb) {
        Some((fa, fb)) => float_result(ctx, fa * fb),
        None => raise_badarith(ctx),
    }
}

/// Float division (/ operator). Always returns a float in Erlang; a zero
/// divisor, 0.0 and -0.0 included, raises `badarith`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_fdiv(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    match float_operands(ErlTerm(a), ErlTerm(b)) {
        Some((fa, fb)) if fb != 0.0 => float_result(ctx, fa / fb),
        _ => raise_badarith(ctx),
    }
}

/// Integer division (div operator). Always returns an integer.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_int_div(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    use crate::bigint::{term_to_bigint, bigint_to_term};
    use num_traits::Zero;
    let ta = ErlTerm(a);
    let tb = ErlTerm(b);
    if let (Some(a_big), Some(b_big)) = (term_to_bigint(ta), term_to_bigint(tb)) {
        if b_big.is_zero() { return raise_badarith(ctx); }
        return with_ctx_heap(ctx, |heap| bigint_to_term(heap, a_big / b_big).0);
    }
    raise_badarith(ctx)
}

/// Integer remainder (rem operator). Always returns an integer.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_int_rem(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    use crate::bigint::{term_to_bigint, bigint_to_term};
    use num_traits::Zero;
    let ta = ErlTerm(a);
    let tb = ErlTerm(b);
    if let (Some(a_big), Some(b_big)) = (term_to_bigint(ta), term_to_bigint(tb)) {
        if b_big.is_zero() { return raise_badarith(ctx); }
        return with_ctx_heap(ctx, |heap| bigint_to_term(heap, a_big % b_big).0);
    }
    raise_badarith(ctx)
}

/// Unary negation. Works on ints and floats.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_negate(ctx: *mut ProcessContext, a: u64) -> u64 {
    use crate::bigint::{term_to_bigint, bigint_to_term};
    let ta = ErlTerm(a);
    if let Some(big) = term_to_bigint(ta) {
        return with_ctx_heap(ctx, |heap| bigint_to_term(heap, -big).0);
    }
    if let Some(val) = term_as_float(ta) {
        return with_ctx_heap(ctx, |heap| heap.alloc_float(-val).0);
    }
    raise_badarith(ctx)
}

/// Bitwise AND. Works on small ints and bigints.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_band(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    use crate::bigint::{term_to_bigint, bigint_to_term};
    let ta = ErlTerm(a);
    let tb = ErlTerm(b);
    if let (Some(a_big), Some(b_big)) = (term_to_bigint(ta), term_to_bigint(tb)) {
        return with_ctx_heap(ctx, |heap| bigint_to_term(heap, a_big & b_big).0);
    }
    raise_badarith(ctx)
}

/// Bitwise OR. Works on small ints and bigints.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bor(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    use crate::bigint::{term_to_bigint, bigint_to_term};
    let ta = ErlTerm(a);
    let tb = ErlTerm(b);
    if let (Some(a_big), Some(b_big)) = (term_to_bigint(ta), term_to_bigint(tb)) {
        return with_ctx_heap(ctx, |heap| bigint_to_term(heap, a_big | b_big).0);
    }
    raise_badarith(ctx)
}

/// Bitwise XOR. Works on small ints and bigints.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bxor(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    use crate::bigint::{term_to_bigint, bigint_to_term};
    let ta = ErlTerm(a);
    let tb = ErlTerm(b);
    if let (Some(a_big), Some(b_big)) = (term_to_bigint(ta), term_to_bigint(tb)) {
        return with_ctx_heap(ctx, |heap| bigint_to_term(heap, a_big ^ b_big).0);
    }
    raise_badarith(ctx)
}

/// Bitwise left shift. Can produce arbitrarily large results.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bsl(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    use crate::bigint::{term_to_bigint, bigint_to_term};
    let ta = ErlTerm(a);
    let tb = ErlTerm(b);
    if let (Some(a_big), Some(b_int)) = (term_to_bigint(ta), tb.as_small_int()) {
        if b_int < 0 {
            // Negative shift = right shift
            let shift = (-b_int) as u64;
            return with_ctx_heap(ctx, |heap| bigint_to_term(heap, a_big >> shift).0);
        }
        return with_ctx_heap(ctx, |heap| bigint_to_term(heap, a_big << (b_int as u64)).0);
    }
    raise_badarith(ctx)
}

/// Bitwise right shift (arithmetic).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bsr(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    use crate::bigint::{term_to_bigint, bigint_to_term};
    let ta = ErlTerm(a);
    let tb = ErlTerm(b);
    if let (Some(a_big), Some(b_int)) = (term_to_bigint(ta), tb.as_small_int()) {
        if b_int < 0 {
            // Negative shift = left shift
            let shift = (-b_int) as u64;
            return with_ctx_heap(ctx, |heap| bigint_to_term(heap, a_big << shift).0);
        }
        return with_ctx_heap(ctx, |heap| bigint_to_term(heap, a_big >> (b_int as u64)).0);
    }
    raise_badarith(ctx)
}

/// Bitwise NOT. Works on small ints and bigints.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bnot(ctx: *mut ProcessContext, a: u64) -> u64 {
    use crate::bigint::{term_to_bigint, bigint_to_term};
    let ta = ErlTerm(a);
    if let Some(a_big) = term_to_bigint(ta) {
        // Erlang bnot X = -(X + 1)
        return with_ctx_heap(ctx, |heap| {
            bigint_to_term(heap, -(a_big + num_bigint::BigInt::from(1))).0
        });
    }
    raise_badarith(ctx)
}

// ---------------------------------------------------------------------------
// Boolean operators: and, or, xor, not
// These validate that both operands are boolean atoms (true/false) and raise
// badarg if not, matching OTP Erlang semantics.
// ---------------------------------------------------------------------------

fn is_boolean_term(t: ErlTerm) -> bool {
    if let Some(id) = t.as_atom_id() {
        id == ATOM_TRUE || id == ATOM_FALSE
    } else {
        false
    }
}

/// Logical `not`. Raises badarg if operand is not a boolean.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bool_not(ctx: *mut ProcessContext, a: u64) -> u64 {
    let ta = ErlTerm(a);
    if let Some(id) = ta.as_atom_id() {
        if id == ATOM_TRUE {
            return ErlTerm::atom(ATOM_FALSE).0;
        }
        if id == ATOM_FALSE {
            return ErlTerm::atom(ATOM_TRUE).0;
        }
    }
    raise_badarg(ctx)
}

/// Logical `and`. Raises badarg if either operand is not a boolean.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bool_and(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    let ta = ErlTerm(a);
    let tb = ErlTerm(b);
    if is_boolean_term(ta) && is_boolean_term(tb) {
        let a_true = ta.as_atom_id() == Some(ATOM_TRUE);
        let b_true = tb.as_atom_id() == Some(ATOM_TRUE);
        return if a_true && b_true {
            ErlTerm::atom(ATOM_TRUE).0
        } else {
            ErlTerm::atom(ATOM_FALSE).0
        };
    }
    raise_badarg(ctx)
}

/// Logical `or`. Raises badarg if either operand is not a boolean.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bool_or(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    let ta = ErlTerm(a);
    let tb = ErlTerm(b);
    if is_boolean_term(ta) && is_boolean_term(tb) {
        let a_true = ta.as_atom_id() == Some(ATOM_TRUE);
        let b_true = tb.as_atom_id() == Some(ATOM_TRUE);
        return if a_true || b_true {
            ErlTerm::atom(ATOM_TRUE).0
        } else {
            ErlTerm::atom(ATOM_FALSE).0
        };
    }
    raise_badarg(ctx)
}

/// Logical `xor`. Raises badarg if either operand is not a boolean.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bool_xor(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    let ta = ErlTerm(a);
    let tb = ErlTerm(b);
    if is_boolean_term(ta) && is_boolean_term(tb) {
        let a_true = ta.as_atom_id() == Some(ATOM_TRUE);
        let b_true = tb.as_atom_id() == Some(ATOM_TRUE);
        return if a_true ^ b_true {
            ErlTerm::atom(ATOM_TRUE).0
        } else {
            ErlTerm::atom(ATOM_FALSE).0
        };
    }
    raise_badarg(ctx)
}

