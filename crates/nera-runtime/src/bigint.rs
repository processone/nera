//! Bigint ↔ term conversion helpers.
//!
//! Centralises the conversion between heap-allocated BOXED_BIGINT terms
//! and `num_bigint::BigInt` values, used by arithmetic FFI functions and
//! BIFs.

use num_bigint::{BigInt, Sign};
use crate::heap::ProcessHeap;
use crate::term::*;

// ---------------------------------------------------------------------------
// Term → BigInt extraction
// ---------------------------------------------------------------------------

/// Extract an integer (small int or bigint) from a term as a `BigInt`.
/// Returns `None` if the term is not an integer.
/// # Safety
/// Referenced storage must be live, initialized with the runtime layout, immutable and unmoved for this
/// call. Use `BorrowedTerm::to_bigint` for a borrow-checked conversion.
///
/// ```compile_fail,E0133
/// use nera_runtime::{bigint::term_to_bigint, term::ErlTerm};
/// let _ = term_to_bigint(ErlTerm::small_int(1));
/// ```
pub unsafe fn term_to_bigint(term: ErlTerm) -> Option<BigInt> {
    if let Some(val) = term.as_small_int() {
        return Some(BigInt::from(val));
    }
    if term.is_boxed() {
        unsafe {
            let ptr = term.as_boxed_ptr()?;
            let header = BoxedHeader(*ptr);
            if header.subtype() == BOXED_BIGINT {
                let limb_count = header.arity() as usize;
                let sign_word = *ptr.add(BIGINT_SIGN_WORD_INDEX);
                let sign = if sign_word == BIGINT_SIGN_NEGATIVE {
                    Sign::Minus
                } else if limb_count == 0 {
                    Sign::NoSign
                } else {
                    Sign::Plus
                };
                let mut bytes = Vec::with_capacity(limb_count * 8);
                for i in 0..limb_count {
                    // Convert each stored u64 limb explicitly to little-endian
                    // bytes, independently of the host's byte order.
                    let limb = *ptr.add(BIGINT_FIRST_LIMB_WORD_INDEX + i);
                    bytes.extend_from_slice(&limb.to_le_bytes());
                }
                // Trim trailing zero bytes
                while bytes.last() == Some(&0) && !bytes.is_empty() {
                    bytes.pop();
                }
                if bytes.is_empty() {
                    return Some(BigInt::from(0));
                }
                return Some(BigInt::from_bytes_le(sign, &bytes));
            }
        }
    }
    None
}

/// Extract a numeric value as f64 from a bigint term (lossy).
/// Used for mixed int/float comparisons.
/// # Safety
/// The same storage requirements as [`term_to_bigint`] apply.
///
/// ```compile_fail,E0133
/// use nera_runtime::{bigint::bigint_to_f64, term::ErlTerm};
/// let _ = bigint_to_f64(ErlTerm::small_int(1));
/// ```
pub unsafe fn bigint_to_f64(term: ErlTerm) -> Option<f64> {
    use num_traits::ToPrimitive;
    term_to_bigint(term).and_then(|b| b.to_f64())
}

// ---------------------------------------------------------------------------
// BigInt → Term storage
// ---------------------------------------------------------------------------

/// Store a BigInt result on the heap. If the value fits in a small int,
/// returns a tagged small int (no allocation). Otherwise allocates a
/// BOXED_BIGINT on the heap.
pub fn bigint_to_term(heap: &mut ProcessHeap, value: BigInt) -> ErlTerm {
    use num_traits::ToPrimitive;
    // Try small int first
    if let Some(val) = value.to_i64() {
        if fits_small_int(val) {
            return ErlTerm::small_int(val);
        }
    }
    alloc_bigint(heap, &value)
}

/// Allocate a BOXED_BIGINT on the heap from a `BigInt` value.
/// Layout: `[header(BIGINT, limb_count), sign, limb0, limb1, ...]`
/// Limbs are stored as little-endian u64 words.
pub fn alloc_bigint(heap: &mut ProcessHeap, value: &BigInt) -> ErlTerm {
    let (sign, bytes) = value.to_bytes_le();

    // Convert bytes to u64 limbs (little-endian, zero-pad to full words)
    let limb_count = bytes.len().div_ceil(8);

    let total_words = bigint_total_words(limb_count);
    let ptr = heap.alloc_words(total_words);
    unsafe {
        *ptr = BoxedHeader::new(crate::term::BoxedSubtype::BigInt, limb_count as u64).0;
        *ptr.add(BIGINT_SIGN_WORD_INDEX) = match sign {
            Sign::Minus => BIGINT_SIGN_NEGATIVE,
            _ => BIGINT_SIGN_NON_NEGATIVE,
        };
        for (i, chunk) in bytes.chunks(8).enumerate() {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            *ptr.add(BIGINT_FIRST_LIMB_WORD_INDEX + i) = u64::from_le_bytes(word);
        }
    }
    // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
    unsafe { ErlTerm::boxed_unchecked(ptr as *const u64) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_partial_limbs_and_signs() {
        let mut heap = ProcessHeap::new();
        for len in 1..=33 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 1) as u8).collect();
            for sign in [Sign::Plus, Sign::Minus] {
                let value = BigInt::from_bytes_le(sign, &bytes);
                let term = alloc_bigint(&mut heap, &value);
                // SAFETY: the initialized term belongs to this live heap.
                assert_eq!(unsafe { term.borrow(&heap) }.to_bigint(), Some(value));
            }
        }
        let term = alloc_bigint(&mut heap, &BigInt::from(0));
        // SAFETY: includes the explicitly boxed zero representation.
        assert_eq!(unsafe { term.borrow(&heap) }.to_bigint(), Some(BigInt::from(0)));
    }

    #[test]
    fn borrowed_conversions_return_owned_values() {
        let mut heap = ProcessHeap::new();
        let value = BigInt::from(1u64) << 100usize;
        let term = bigint_to_term(&mut heap, value.clone());
        // SAFETY: the term was allocated in this heap, with no intervening GC.
        let view = unsafe { term.borrow(&heap) };
        let owned = view.to_bigint().unwrap();
        assert_eq!(view.integer_to_f64(), Some(2f64.powi(100)));
        drop(heap);
        assert_eq!(owned, value);
    }

    #[test]
    fn roundtrip_positive_bigint() {
        let mut heap = ProcessHeap::new();
        let val = BigInt::from(1_i128 << 100);
        let term = alloc_bigint(&mut heap, &val);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { term.is_bigint() });
        let recovered = unsafe { term.borrow(&heap) }.to_bigint().unwrap();
        assert_eq!(recovered, val);
    }

    #[test]
    fn roundtrip_negative_bigint() {
        let mut heap = ProcessHeap::new();
        let val = BigInt::from(-(1_i128 << 80));
        let term = alloc_bigint(&mut heap, &val);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { term.is_bigint() });
        let recovered = unsafe { term.borrow(&heap) }.to_bigint().unwrap();
        assert_eq!(recovered, val);
    }

    #[test]
    fn bigint_to_term_uses_small_int_when_possible() {
        let mut heap = ProcessHeap::new();
        let val = BigInt::from(42);
        let term = bigint_to_term(&mut heap, val);
        assert!(term.is_small_int());
        assert_eq!(term.as_small_int(), Some(42));
    }

    #[test]
    fn bigint_to_term_negative_small() {
        let mut heap = ProcessHeap::new();
        let val = BigInt::from(-100);
        let term = bigint_to_term(&mut heap, val);
        assert!(term.is_small_int());
        assert_eq!(term.as_small_int(), Some(-100));
    }

    #[test]
    fn bigint_to_term_promotes_large_value() {
        let mut heap = ProcessHeap::new();
        let val = BigInt::from(1_i128 << 61);
        let term = bigint_to_term(&mut heap, val.clone());
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { term.is_bigint() });
        let recovered = unsafe { term.borrow(&heap) }.to_bigint().unwrap();
        assert_eq!(recovered, val);
    }

    #[test]
    fn term_to_bigint_from_small_int() {
        let heap = ProcessHeap::new();
        let term = ErlTerm::small_int(999);
        let val = unsafe { term.borrow(&heap) }.to_bigint().unwrap();
        assert_eq!(val, BigInt::from(999));
    }

    #[test]
    fn term_to_bigint_from_non_integer_returns_none() {
        let heap = ProcessHeap::new();
        let term = ErlTerm::atom(0);
        assert!(unsafe { term.borrow(&heap) }.to_bigint().is_none());
    }

    #[test]
    fn roundtrip_zero() {
        let mut heap = ProcessHeap::new();
        let val = BigInt::from(0);
        // Zero should become a small int
        let term = bigint_to_term(&mut heap, val);
        assert!(term.is_small_int());
        assert_eq!(term.as_small_int(), Some(0));
    }

    #[test]
    fn roundtrip_large_factorial() {
        let mut heap = ProcessHeap::new();
        // 50! = 30414093201713378043612608166979581188299763898377856820553615673507270386838265
        let mut fac = BigInt::from(1u64);
        for i in 2..=50u64 {
            fac *= BigInt::from(i);
        }
        let term = bigint_to_term(&mut heap, fac.clone());
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { term.is_bigint() });
        let recovered = unsafe { term.borrow(&heap) }.to_bigint().unwrap();
        assert_eq!(recovered, fac);
    }
}
