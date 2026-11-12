//! Comparison operations and Erlang term ordering.

#![allow(unused_unsafe)]

use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;

pub(crate) fn bool_to_term(val: bool) -> u64 {
    use crate::atom_table::{ATOM_TRUE, ATOM_FALSE};
    ErlTerm::atom(if val { ATOM_TRUE } else { ATOM_FALSE }).0
}

/// Compare two numeric terms exactly, including mixed integer/float inputs.
/// # Safety
/// Both terms must be numeric. Their storage must remain live, initialized,
/// immutable and unmoved throughout the call.
pub(crate) unsafe fn compare_numeric(a: ErlTerm, b: ErlTerm) -> std::cmp::Ordering {
    use crate::bigint::term_to_bigint;
    use std::cmp::Ordering;

    // The common integer path needs neither boxed reads nor BigInt allocation.
    if let (Some(a), Some(b)) = (a.as_small_int(), b.as_small_int()) {
        return a.cmp(&b);
    }
    match (term_as_float(a), term_as_float(b)) {
        (Some(a), Some(b)) => {
            // Erlang numeric equality treats signed zeros equally. For raw Rust
            // NaN inputs, retain a deterministic order consistent with mixed cases.
            if a == b { Ordering::Equal } else { a.total_cmp(&b) }
        }
        (None, Some(float)) => compare_integer_float(a, float),
        (Some(float), None) => compare_integer_float(b, float).reverse(),
        (None, None) => {
            let a = term_to_bigint(a).expect("numeric integer term");
            let b = term_to_bigint(b).expect("numeric integer term");
            a.cmp(&b)
        }
    }
}

/// Compare without rounding an integer to f64.
#[inline]
fn compare_i64_float(integer: i64, float: f64) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    // Bounds and special values are handled before Rust's saturating cast.
    if float.is_nan() {
        return if float.is_sign_negative() { Ordering::Greater } else { Ordering::Less };
    }
    if float >= 9223372036854775808.0 { return Ordering::Less; }
    if float < -9223372036854775808.0 { return Ordering::Greater; }
    match integer.cmp(&(float as i64)) {
        Ordering::Equal => {
            let fraction = float.fract();
            if fraction > 0.0 { Ordering::Less }
            else if fraction < 0.0 { Ordering::Greater }
            else { Ordering::Equal }
        }
        order => order,
    }
}

/// # Safety
/// integer must be a live, valid integer term with immutable backing storage.
unsafe fn compare_integer_float(integer: ErlTerm, float: f64) -> std::cmp::Ordering {
    use num_traits::ToPrimitive;
    use std::cmp::Ordering;
    if let Some(integer) = integer.as_small_int() {
        return compare_i64_float(integer, float);
    }
    let integer = crate::bigint::term_to_bigint(integer).expect("numeric integer term");
    if let Some(integer) = integer.to_i64() {
        return compare_i64_float(integer, float);
    }
    if float.is_nan() {
        return if float.is_sign_negative() { Ordering::Greater } else { Ordering::Less };
    }
    if float.is_infinite() {
        return if float.is_sign_negative() { Ordering::Greater } else { Ordering::Less };
    }
    let negative = integer.sign() == num_bigint::Sign::Minus;
    if negative != float.is_sign_negative() || float == 0.0 {
        return if negative { Ordering::Less } else { Ordering::Greater };
    }
    // Disjoint magnitudes can be ordered without allocating a float integer.
    let float_bits = ((float.to_bits() >> 52) & 0x7ff) as i32 - 1023 + 1;
    let magnitude = integer.bits().cmp(&(float_bits.max(0) as u64));
    if magnitude != Ordering::Equal {
        return if negative { magnitude.reverse() } else { magnitude };
    }
    // Here the finite float has magnitude >= 2^63 and no fractional bits.
    // Its exact integer is a 53-bit mantissa shifted left. Compare its virtual
    // words against the integer's magnitude, without allocating that integer.
    let mantissa = (float.to_bits() & ((1u64 << 52) - 1)) | (1u64 << 52);
    let shift = float_bits as u32 - 53;
    let low_word = (shift / 64) as usize;
    let offset = shift % 64;
    let words = integer.bits().div_ceil(64) as usize;
    for (index, digit) in (0..words).rev().zip(integer.iter_u64_digits().rev()) {
        let expected = if index == low_word {
            mantissa << offset
        } else if index == low_word + 1 && offset != 0 {
            mantissa >> (64 - offset)
        } else { 0 };
        let order = digit.cmp(&expected);
        if order != Ordering::Equal {
            return if negative { order.reverse() } else { order };
        }
    }
    Ordering::Equal
}

/// Return an ordering rank for Erlang term ordering:
/// number < atom < reference < fun < port < pid < tuple < map < nil < list < bitstring
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn term_order_rank(t: ErlTerm) -> u8 {
    use crate::term::*;
    if is_numeric(t) { return 0; } // number
    match t.tag() {
        TAG_ATOM => 1,
        TAG_PID => 5,
        TAG_NIL => 8, // nil (empty list) comes after map, before cons
        TAG_CONS => 9,
        TAG_BOXED => {
            if let Some(ptr) = t.as_boxed_ptr() {
                let hdr = BoxedHeader(unsafe { *ptr });
                match hdr.subtype() {
                    BOXED_REFERENCE => 2, // reference
                    BOXED_CLOSURE => 3,   // fun
                    BOXED_TUPLE => 6,
                    BOXED_MAP => 7,
                    subtype if is_binary_subtype(subtype) => 10,
                    _ => 11,
                }
            } else { 11 }
        }
        _ => 11,
    }
}

/// Compare two Erlang terms. Returns <0 if a<b, 0 if equal, >0 if a>b.
/// Follows Erlang term ordering: number < atom < reference < fun < port < pid < tuple < map < nil < list < bitstring.
/// Numeric comparison coerces int/float except within map keys, which use exact ordering.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub(crate) unsafe fn compare_terms(a: ErlTerm, b: ErlTerm) -> i64 {
    compare_terms_mode(a, b, false)
}

/// Largest map that BEAM stores as a sorted flat map. Larger maps are hash
/// tries whose iteration order depends on ERTS internal hashing, which is not
/// modelled: they keep insertion order.
pub(crate) const FLATMAP_MAX_KEYS: usize = 32;

/// Pair indices of the map at `ptr` (header word pointer) in BEAM iteration
/// order: maps of up to 32 keys iterate in ascending exact key order.
/// # Safety
/// `ptr` must point at a live map with `npairs` complete pairs, unmoved by GC.
pub(crate) unsafe fn map_iteration_order(ptr: *const u64, npairs: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..npairs).collect();
    if npairs > 1 && npairs <= FLATMAP_MAX_KEYS {
        order.sort_by(|&a, &b| {
            let ka = ErlTerm(*ptr.add(1 + a * 2));
            let kb = ErlTerm(*ptr.add(1 + b * 2));
            // BEAM sorts flat-map atom keys by atom-table index (creation
            // order), not by name; every other pair uses exact term order.
            match (ka.as_atom_id(), kb.as_atom_id()) {
                (Some(x), Some(y)) => x.cmp(&y),
                _ => compare_terms_mode(ka, kb, true).cmp(&0),
            }
        });
    }
    order
}

/// Map keys require exact ordering throughout their graph, including map values
/// nested inside a key. Ordinary map values retain the caller's comparison mode.
/// The term lifetime contract is the same as compare_terms's.
unsafe fn compare_terms_mode(a: ErlTerm, b: ErlTerm, exact: bool) -> i64 {
    use crate::term::*;

    // Fast path: identical raw values
    if a.0 == b.0 { return 0; }
    // Distinct atoms order by name, as on BEAM, never by creation order.
    if let (Some(a), Some(b)) = (a.as_atom_id(), b.as_atom_id()) {
        return crate::atom_table::global_atom_table().compare(a, b) as i64;
    }

    // Numeric comparison with bigint-aware full precision
    let a_num = is_numeric(a);
    let b_num = is_numeric(b);

    if a_num && b_num {
        if exact {
            // Erlang orders every integer before every float in map keys,
            // recursively, and distinguishes floating-point signed zeros.
            match (term_as_float(a), term_as_float(b)) {
                (None, Some(_)) => return -1,
                (Some(_), None) => return 1,
                (Some(a), Some(b)) => return a.total_cmp(&b) as i64,
                (None, None) => {}
            }
        }
        return compare_numeric(a, b) as i64;
    }
    if a_num { return -1; }  // number < non-number
    if b_num { return 1; }   // non-number > number

    // Same-type structural comparison
    // Cons cells: compare heads, then tails recursively
    if let (Some(pa), Some(pb)) = (a.as_cons_ptr(), b.as_cons_ptr()) {
        unsafe {
            let r = compare_terms_mode((*pa).head, (*pb).head, exact);
            if r != 0 { return r; }
            return compare_terms_mode((*pa).tail, (*pb).tail, exact);
        }
    }

    // Tuples: compare arity, then elements left-to-right
    if a.is_boxed() && b.is_boxed() {
        if let (Some(pa), Some(pb)) = (a.as_boxed_ptr(), b.as_boxed_ptr()) {
            unsafe {
                let ha = BoxedHeader(*pa);
                let hb = BoxedHeader(*pb);
                if ha.subtype() == BOXED_TUPLE && hb.subtype() == BOXED_TUPLE {
                    let aa = ha.arity();
                    let ab = hb.arity();
                    if aa != ab { return if aa < ab { -1 } else { 1 }; }
                    for i in 1..=aa as usize {
                        let r = compare_terms_mode(ErlTerm(*pa.add(i)), ErlTerm(*pb.add(i)), exact);
                        if r != 0 { return r; }
                    }
                    return 0;
                }
                // Maps: compare sizes, then all exact keys in canonical order,
                // then values in the caller's mode. Insertion order is irrelevant.
                if ha.subtype() == BOXED_MAP && hb.subtype() == BOXED_MAP {
                    let na = ha.arity() as usize;
                    let nb = hb.arity() as usize;
                    if na != nb { return if na < nb { -1 } else { 1 }; }
                    let mut ka: Vec<(ErlTerm, ErlTerm)> = (0..na)
                        .map(|i| (
                            ErlTerm(*pa.add(1 + i * 2)),
                            ErlTerm(*pa.add(1 + i * 2 + 1)),
                        ))
                        .collect();
                    let mut kb: Vec<(ErlTerm, ErlTerm)> = (0..nb)
                        .map(|i| (
                            ErlTerm(*pb.add(1 + i * 2)),
                            ErlTerm(*pb.add(1 + i * 2 + 1)),
                        ))
                        .collect();
                    ka.sort_by(|x, y| match compare_terms_mode(x.0, y.0, true) {
                        r if r < 0 => std::cmp::Ordering::Less,
                        r if r > 0 => std::cmp::Ordering::Greater,
                        _ => std::cmp::Ordering::Equal,
                    });
                    kb.sort_by(|x, y| match compare_terms_mode(x.0, y.0, true) {
                        r if r < 0 => std::cmp::Ordering::Less,
                        r if r > 0 => std::cmp::Ordering::Greater,
                        _ => std::cmp::Ordering::Equal,
                    });
                    for i in 0..na {
                        let r = compare_terms_mode(ka[i].0, kb[i].0, true);
                        if r != 0 { return r; }
                    }
                    for i in 0..na {
                        let r = compare_terms_mode(ka[i].1, kb[i].1, exact);
                        if r != 0 { return r; }
                    }
                    return 0;
                }
                // References: compare by unique ID
                if ha.subtype() == BOXED_REFERENCE && hb.subtype() == BOXED_REFERENCE {
                    if ha.arity() != hb.arity() {
                        return if ha.arity() < hb.arity() { -1 } else { 1 };
                    }
                    let id_a = *pa.add(REFERENCE_ID_WORD_INDEX);
                    let id_b = *pb.add(REFERENCE_ID_WORD_INDEX);
                    return if id_a < id_b { -1 } else if id_a > id_b { 1 } else { 0 };
                }
                // Binaries: compare byte-by-byte when inline bytes are available,
                // otherwise compare descriptor fields deterministically.
                if is_binary_subtype(ha.subtype()) && is_binary_subtype(hb.subtype()) {
                    let bits_a = ha.arity() as usize; // bit count
                    let bits_b = hb.arity() as usize;
                    let byte_len_a = bits_a.div_ceil(8);
                    let byte_len_b = bits_b.div_ceil(8);
                    let ta = ErlTerm::boxed_unchecked(pa);
                    let tb = ErlTerm::boxed_unchecked(pb);
                    if let (Some((data_a, _)), Some((data_b, _))) = (binary_data(ta), binary_data(tb)) {
                        // Compare full bytes first
                        let full_bytes_a = bits_a / 8;
                        let full_bytes_b = bits_b / 8;
                        let min_full = full_bytes_a.min(full_bytes_b);
                        for i in 0..min_full {
                            let ba = *data_a.add(i);
                            let bb = *data_b.add(i);
                            if ba != bb { return if ba < bb { -1 } else { 1 }; }
                        }
                        // If one has more full bytes than the other's total,
                        // compare the extra full byte against the partial trailing bits
                        let trailing_a = bits_a % 8;
                        let trailing_b = bits_b % 8;
                        if full_bytes_a == full_bytes_b {
                            // Same number of full bytes; compare trailing bits
                            if trailing_a > 0 && trailing_b > 0 {
                                let min_trail = trailing_a.min(trailing_b);
                                let mask = 0xFFu8 << (8 - min_trail);
                                let ba = *data_a.add(full_bytes_a) & mask;
                                let bb = *data_b.add(full_bytes_b) & mask;
                                if ba != bb { return if ba < bb { -1 } else { 1 }; }
                            }
                            return if bits_a < bits_b { -1 } else if bits_a > bits_b { 1 } else { 0 };
                        }
                        // Different number of full bytes — the shorter one ran out
                        return if bits_a < bits_b { -1 } else if bits_a > bits_b { 1 } else { 0 };
                    }
                    if byte_len_a != byte_len_b {
                        return if byte_len_a < byte_len_b { -1 } else { 1 };
                    }
                    if ha.subtype() == BOXED_REFC_BINARY && hb.subtype() == BOXED_REFC_BINARY {
                        let owner_a = *pa.add(REFC_BINARY_PTR_WORD_INDEX);
                        let owner_b = *pb.add(REFC_BINARY_PTR_WORD_INDEX);
                        if owner_a != owner_b {
                            return if owner_a < owner_b { -1 } else { 1 };
                        }
                        let off_a = *pa.add(REFC_BINARY_OFFSET_WORD_INDEX);
                        let off_b = *pb.add(REFC_BINARY_OFFSET_WORD_INDEX);
                        if off_a != off_b {
                            return if off_a < off_b { -1 } else { 1 };
                        }
                        return 0;
                    }
                    if ha.subtype() != hb.subtype() {
                        return if ha.subtype() < hb.subtype() { -1 } else { 1 };
                    }
                    return if ta.0 < tb.0 { -1 } else { 1 };
                }
            }
        }
    }

    // Different types: compare by term ordering rank
    let ra = term_order_rank(a);
    let rb = term_order_rank(b);
    if ra != rb { return if ra < rb { -1 } else { 1 }; }

    // Same type, non-structural: compare raw tagged values (pids, funs).
    // Atoms were ordered by name above.
    if a.0 < b.0 { -1 } else { 1 }
}

/// Compare two terms. Returns an ErlTerm: true atom if a < b, false otherwise.
/// Handles int/float mixed comparisons.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_compare_lt(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    let _ = ctx;
    let result = compare_terms(ErlTerm(a), ErlTerm(b));
    bool_to_term(result < 0)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_compare_gt(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    let _ = ctx;
    let result = compare_terms(ErlTerm(a), ErlTerm(b));
    bool_to_term(result > 0)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_compare_lte(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    let _ = ctx;
    let result = compare_terms(ErlTerm(a), ErlTerm(b));
    bool_to_term(result <= 0)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_compare_gte(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    let _ = ctx;
    let result = compare_terms(ErlTerm(a), ErlTerm(b));
    bool_to_term(result >= 0)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_compare_eq(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    let _ = ctx;
    // == does numeric coercion (1 == 1.0 is true)
    let result = compare_terms(ErlTerm(a), ErlTerm(b));
    bool_to_term(result == 0)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_compare_neq(ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    let _ = ctx;
    let result = compare_terms(ErlTerm(a), ErlTerm(b));
    bool_to_term(result != 0)
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_exact_eq(_ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    bool_to_term(exact_equal(ErlTerm(a), ErlTerm(b)))
}

#[no_mangle]
pub unsafe extern "C" fn nera_rt_exact_neq(_ctx: *mut ProcessContext, a: u64, b: u64) -> u64 {
    bool_to_term(!exact_equal(ErlTerm(a), ErlTerm(b)))
}

/// Exact numeric equality applies at every depth of a compound term.
/// Both graphs must remain live, immutable and unmoved for the comparison.
#[inline]
pub(super) unsafe fn exact_equal(mut a: ErlTerm, mut b: ErlTerm) -> bool {
    // Flat lists, including strings, need no heap-allocated work buffer.
    loop {
        if a == b { return true; }
        if a.is_immediate() && b.is_immediate() { return false; }
        let (Some(ap), Some(bp)) = (a.as_cons_ptr(), b.as_cons_ptr()) else { break; };
        let (av, bv) = ((*ap).head, (*bp).head);
        if av != bv {
            if av.is_immediate() && bv.is_immediate() { return false; }
            break; // Nested heads retain the iterative structural comparison.
        }
        a = (*ap).tail;
        b = (*bp).tail;
    }
    // Flat tuples are common list elements. Compare their immediate fields
    // inline, deferring nested graphs to the iterative traversal below.
    if let (Some(ap), Some(bp)) = (a.as_boxed_ptr(), b.as_boxed_ptr()) {
        let ah = BoxedHeader(*ap);
        let bh = BoxedHeader(*bp);
        if ah.subtype() == BOXED_TUPLE && bh.subtype() == BOXED_TUPLE {
            if ah.arity() != bh.arity() { return false; }
            for i in 1..=ah.arity() as usize {
                let av = ErlTerm(*ap.add(i));
                let bv = ErlTerm(*bp.add(i));
                if av == bv { continue; }
                if av.is_immediate() && bv.is_immediate() { return false; }
                return exact_equal_slow(a, b);
            }
            return true;
        }
    }
    exact_equal_slow(a, b)
}

/// The input graph contract is the same as exact_equal's.
unsafe fn exact_equal_slow(a: ErlTerm, b: ErlTerm) -> bool {
    if a == b { return true; }
    if a.is_immediate() && b.is_immediate() { return false; }
    // Start on the stack; flat tuples of immediate values need no work buffer.
    let mut pending = Vec::new();
    let mut first = Some((a, b));
    while let Some((a, b)) = first.take().or_else(|| pending.pop()) {
        if a == b { continue; }
        match (term_as_float(a), term_as_float(b)) {
            // OTP 27+ exact equality distinguishes positive and negative zero.
            (Some(a), Some(b)) => { if a.to_bits() != b.to_bits() { return false; } continue; }
            (Some(_), None) | (None, Some(_)) => return false,
            _ => {}
        }
        if let (Some(a), Some(b)) = (a.as_cons_ptr(), b.as_cons_ptr()) {
            pending.push(((*a).tail, (*b).tail));
            pending.push(((*a).head, (*b).head));
            continue;
        }
        if let (Some(a_ptr), Some(b_ptr)) = (a.as_boxed_ptr(), b.as_boxed_ptr()) {
            let ah = BoxedHeader(*a_ptr);
            let bh = BoxedHeader(*b_ptr);
            if ah.subtype() == BOXED_TUPLE && bh.subtype() == BOXED_TUPLE {
                if ah.arity() != bh.arity() { return false; }
                for i in 1..=ah.arity() as usize {
                    let a = ErlTerm(*a_ptr.add(i));
                    let b = ErlTerm(*b_ptr.add(i));
                    if a == b { continue; }
                    if a.is_immediate() && b.is_immediate() { return false; }
                    pending.push((a, b));
                }
                continue;
            }
            if ah.subtype() == BOXED_MAP && bh.subtype() == BOXED_MAP {
                let count = ah.arity() as usize;
                if count != bh.arity() as usize { return false; }
                // Maps may have different insertion orders. Match exact keys
                // before comparing values, without conflating 1 and 1.0.
                for i in 0..count {
                    let key = ErlTerm(*a_ptr.add(1 + i * 2));
                    let Some(j) = (0..count).find(|&j| exact_equal(key, ErlTerm(*b_ptr.add(1 + j * 2)))) else { return false; };
                    pending.push((ErlTerm(*a_ptr.add(2 + i * 2)), ErlTerm(*b_ptr.add(2 + j * 2))));
                }
                continue;
            }
        }
        if compare_terms(a, b) != 0 { return false; }
    }
    true
}


#[cfg(test)]
mod numeric_order_tests {
    use super::*;
    use crate::heap::ProcessHeap;

    #[test]
    fn numeric_order_handles_boundaries_and_fractional_values() {
        use num_bigint::BigInt;
        use std::cmp::Ordering::*;
        let mut heap = ProcessHeap::new();
        let cases = [
            (BigInt::from(0), 0.5, Less),
            (BigInt::from(0), -0.5, Greater),
            (BigInt::from(0), -0.0, Equal),
            (BigInt::from(-3), -3.5, Greater),
            (BigInt::from(-4), -3.5, Less),
            (BigInt::from(-(1i64 << 53) - 1), -((1u64 << 53) as f64), Less),
            (BigInt::from(i64::MAX), 9223372036854775808.0, Less),
            (BigInt::from(i64::MIN), -9223372036854775808.0, Equal),
            (BigInt::from(i64::MIN) - 1, -9223372036854775808.0, Less),
            (BigInt::from(1) << 100usize, 2f64.powi(100), Equal),
            ((BigInt::from(1) << 100usize) + 1, 2f64.powi(100), Greater),
            (-(BigInt::from(1) << 100usize) - 1, -2f64.powi(100), Less),
            (BigInt::from(1) << 2048usize, f64::INFINITY, Less),
            (-(BigInt::from(1) << 2048usize), f64::NEG_INFINITY, Greater),
            (BigInt::from(1), f64::from_bits(1), Greater),
            (BigInt::from(0), f64::from_bits(1), Less),
        ];
        for (integer, value, expected) in cases {
            let integer = crate::bigint::bigint_to_term(&mut heap, integer);
            let float = heap.alloc_float(value);
            // SAFETY: these numeric roots remain in the live, uncollected heap.
            unsafe {
                assert_eq!(compare_numeric(integer, float), expected, "float {value}");
                assert_eq!(compare_numeric(float, integer), expected.reverse());
            }
        }
    }

    #[test]
    fn numeric_order_compares_large_float_mantissas_exactly() {
        use num_bigint::BigInt;
        use num_traits::FromPrimitive;
        use std::cmp::Ordering;
        let mut heap = ProcessHeap::new();
        // Cover mantissas spanning word boundaries, zero low words, and the
        // largest finite exponent, using the library conversion as an oracle.
        for exponent in [63u64, 64, 65, 100, 115, 116, 127, 128, 255, 511, 1023] {
            for fraction in [0, 1, 0x123456789abcd, (1u64 << 52) - 1] {
                for negative in [false, true] {
                    let bits = ((exponent + 1023) << 52) | fraction
                        | if negative { 1u64 << 63 } else { 0 };
                    let value = f64::from_bits(bits);
                    let exact = BigInt::from_f64(value).unwrap();
                    let float = heap.alloc_float(value);
                    for delta in [-1, 0, 1] {
                        let integer = crate::bigint::bigint_to_term(&mut heap, &exact + delta);
                        // SAFETY: both numeric graphs remain owned by this heap.
                        unsafe {
                            assert_eq!(compare_numeric(integer, float), delta.cmp(&0));
                            assert_eq!(compare_numeric(float, integer), 0.cmp(&delta));
                        }
                    }
                    // SAFETY: this float remains live and immutable.
                    assert_eq!(unsafe { compare_numeric(float, float) }, Ordering::Equal);
                }
            }
        }
    }

    #[test]
    fn numeric_order_is_transitive_across_representations() {
        use num_bigint::BigInt;
        let mut heap = ProcessHeap::new();
        let big = crate::bigint::bigint_to_term(&mut heap, BigInt::from(1) << 100usize);
        let bigger = crate::bigint::bigint_to_term(&mut heap, (BigInt::from(1) << 100usize) + 1);
        let values = [
            heap.alloc_float(-f64::NAN),
            heap.alloc_float(f64::NEG_INFINITY),
            ErlTerm::small_int(-((1 << 53) + 1)),
            heap.alloc_float(-((1u64 << 53) as f64)),
            heap.alloc_float(-0.0), ErlTerm::small_int(0), heap.alloc_float(0.0),
            ErlTerm::small_int(1 << 53), heap.alloc_float((1u64 << 53) as f64),
            ErlTerm::small_int((1 << 53) + 1), big, heap.alloc_float(2f64.powi(100)),
            bigger, heap.alloc_float(f64::INFINITY),
            heap.alloc_float(f64::NAN),
        ];
        // SAFETY: all numeric storage remains live and immutable.
        unsafe {
            for (i, &a) in values.iter().enumerate() {
                for (j, &b) in values.iter().enumerate() {
                    let order = compare_numeric(a, b);
                    assert_eq!(order, compare_numeric(b, a).reverse());
                    if i <= j { assert!(!order.is_gt()); }
                    for &c in &values {
                        if order.is_le() && compare_numeric(b, c).is_le() {
                            assert!(compare_numeric(a, c).is_le());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn numeric_order_preserves_integer_precision() {
        let mut heap = ProcessHeap::new();
        let float = heap.alloc_float((1u64 << 53) as f64);
        let exact = ErlTerm::small_int(1 << 53);
        let greater = ErlTerm::small_int((1 << 53) + 1);
        // SAFETY: heap and immediate terms remain live and immutable.
        unsafe {
            assert_eq!(compare_numeric(exact, float), std::cmp::Ordering::Equal);
            assert_eq!(compare_numeric(greater, float), std::cmp::Ordering::Greater);
            assert_eq!(compare_numeric(float, greater), std::cmp::Ordering::Less);
        }
    }
}
