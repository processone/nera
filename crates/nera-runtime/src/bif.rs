//! Built-in functions (BIFs) for the Erlang runtime.
//!
//! These functions implement core Erlang operations that cannot be
//! expressed in Erlang itself: list manipulation, type checks, I/O,
//! tuple access, etc.

//!
//! Raw terms do not prove storage validity. Graph-reading APIs require an
//! explicit unsafe boundary, including list traversal, formatting and encoding.
//!
//! ```compile_fail,E0133
//! use nera_runtime::{bif, heap::ProcessHeap, term::ErlTerm};
//! let mut heap = ProcessHeap::new();
//! let _ = bif::bif_length(&mut heap, &[ErlTerm::nil()]);
//! ```
//! ```compile_fail,E0133
//! use nera_runtime::{bif, term::ErlTerm};
//! let _ = bif::format_term(ErlTerm::nil());
//! ```
//! ```compile_fail,E0133
//! use nera_runtime::{bif, heap::ProcessHeap, term::ErlTerm};
//! let mut heap = ProcessHeap::new();
//! let _ = bif::bif_term_to_binary(&mut heap, &[ErlTerm::nil()]);
//! ```

use crate::binary::BinaryData;
use crate::atom_table::{global_atom_table, ATOM_BADARG, ATOM_FALSE, ATOM_OK, ATOM_TRUE};
use crate::bigint::{bigint_to_term, term_to_bigint};
use crate::heap::{MapPairs, ProcessHeap};
use crate::term::{
    BoxedHeader, ErlTerm, BOXED_BIGINT, BOXED_BINARY, BOXED_CLOSURE, BOXED_FLOAT,
    BOXED_MAP, BOXED_REFC_BINARY, BOXED_REFERENCE, BOXED_TUPLE,
    REFC_BINARY_OFFSET_WORD_INDEX, REFC_BINARY_PTR_WORD_INDEX,
};

// ---------------------------------------------------------------------------
// I/O
// ---------------------------------------------------------------------------

/// `io:format/1` -- print a string (list of integers) to stdout.
///
/// Returns `{ok}` atom on success, `{error, badarg}` on bad input.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn bif_io_format_1(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    // io:format/1 is equivalent to io:format(Fmt, [])
    bif_io_format_2(heap, &[args[0], ErlTerm::nil()])
}

/// `io:format/2` -- formatted output with arguments.
///
/// Width, precision and padding are parsed by the shared io/io_lib formatter.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn bif_io_format_2(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 2 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let output = crate::io_format::format_output(args[0], args[1])
        .map_err(|()| ErlTerm::atom(ATOM_BADARG))?;

    print!("{}", output);
    use std::io::Write;
    let _ = std::io::stdout().flush();
    Ok(ErlTerm::atom(ATOM_OK))
}

// ---------------------------------------------------------------------------
// List operations
// ---------------------------------------------------------------------------

/// `erlang:length/1` -- length of a proper list.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_length(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let mut count: i64 = 0;
    let mut cursor = args[0];
    while cursor.is_cons() {
        count += 1;
        unsafe {
            let cell = &*cursor.as_cons_ptr().unwrap();
            cursor = cell.tail;
        }
    }
    if cursor.is_nil() {
        Ok(ErlTerm::small_int(count))
    } else {
        Err(ErlTerm::atom(ATOM_BADARG))
    }
}

/// `erlang:hd/1` -- head of a list.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_hd(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    match args[0].as_cons_ptr() {
        Some(ptr) => Ok(unsafe { (*ptr).head }),
        None => Err(ErlTerm::atom(ATOM_BADARG)),
    }
}

/// `erlang:tl/1` -- tail of a list.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_tl(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    match args[0].as_cons_ptr() {
        Some(ptr) => Ok(unsafe { (*ptr).tail }),
        None => Err(ErlTerm::atom(ATOM_BADARG)),
    }
}

// ---------------------------------------------------------------------------
// Type checks
// ---------------------------------------------------------------------------

/// `erlang:is_integer/1`
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn bif_is_integer(args: &[ErlTerm]) -> ErlTerm {
    if args.len() == 1 && (args[0].is_small_int() || args[0].is_bigint()) {
        ErlTerm::atom(ATOM_TRUE)
    } else {
        ErlTerm::atom(ATOM_FALSE)
    }
}

/// `erlang:is_atom/1`
pub fn bif_is_atom(args: &[ErlTerm]) -> ErlTerm {
    if args.len() == 1 && args[0].is_atom() {
        ErlTerm::atom(ATOM_TRUE)
    } else {
        ErlTerm::atom(ATOM_FALSE)
    }
}

/// `erlang:is_list/1`
pub fn bif_is_list(args: &[ErlTerm]) -> ErlTerm {
    if args.len() == 1 && args[0].is_list() {
        ErlTerm::atom(ATOM_TRUE)
    } else {
        ErlTerm::atom(ATOM_FALSE)
    }
}

/// `erlang:is_tuple/1`
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_is_tuple(args: &[ErlTerm]) -> ErlTerm {
    if args.len() == 1 && args[0].is_boxed() {
        unsafe {
            let hdr = BoxedHeader(*args[0].as_boxed_ptr().unwrap());
            if hdr.subtype() == BOXED_TUPLE {
                return ErlTerm::atom(ATOM_TRUE);
            }
        }
    }
    ErlTerm::atom(ATOM_FALSE)
}

/// `erlang:is_pid/1`
pub fn bif_is_pid(args: &[ErlTerm]) -> ErlTerm {
    if args.len() == 1 && args[0].is_pid() {
        ErlTerm::atom(ATOM_TRUE)
    } else {
        ErlTerm::atom(ATOM_FALSE)
    }
}

/// `erlang:is_binary/1`
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn bif_is_binary(args: &[ErlTerm]) -> ErlTerm {
    if args.len() == 1 && args[0].is_binary() {
        ErlTerm::atom(ATOM_TRUE)
    } else {
        ErlTerm::atom(ATOM_FALSE)
    }
}

/// `erlang:is_map/1`
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn bif_is_map(args: &[ErlTerm]) -> ErlTerm {
    if args.len() == 1 && args[0].is_map() {
        ErlTerm::atom(ATOM_TRUE)
    } else {
        ErlTerm::atom(ATOM_FALSE)
    }
}

/// `erlang:map_size/1`
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_map_size(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let ptr = match args[0].as_boxed_ptr() {
        Some(p) => p,
        None => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    unsafe {
        let header = BoxedHeader(*ptr);
        if header.subtype() != BOXED_MAP {
            return Err(ErlTerm::atom(ATOM_BADARG));
        }
        Ok(ErlTerm::small_int(header.arity() as i64))
    }
}

/// `erlang:size/1` -- returns tuple_size for tuples, byte_size for binaries.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn bif_size(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let term = args[0];

    // Check for binary first (binary_size works on various binary representations)
    if let Some(size) = term.binary_size() {
        return Ok(ErlTerm::small_int(size as i64));
    }

    // Check for tuple
    if let Some(ptr) = term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() == BOXED_TUPLE {
                return Ok(ErlTerm::small_int(header.arity() as i64));
            }
        }
    }

    Err(ErlTerm::atom(ATOM_BADARG))
}

/// `erlang:byte_size/1`
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn bif_byte_size(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    match args[0].binary_size() {
        Some(size) => Ok(ErlTerm::small_int(size as i64)),
        None => Err(ErlTerm::atom(ATOM_BADARG)),
    }
}

// ---------------------------------------------------------------------------
// Hashing
// ---------------------------------------------------------------------------

/// FNV-1a-style hash mixing.
#[inline]
fn hash_mix(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(16777619)
}

/// Compute a deterministic hash of an Erlang term.
///
/// Integers equal to floats must produce the same hash (e.g. 1 and 1.0).
/// # Safety
/// Source graphs obey the enclosing BIF live, immutable, unmoved input contract.
unsafe fn hash_erlang_term(term: ErlTerm) -> u64 {
    // Small integer
    if let Some(n) = term.as_small_int() {
        return hash_mix(0xcbf29ce484222325, n as u64);
    }

    // Atom
    if let Some(id) = term.as_atom_id() {
        return hash_mix(0xcbf29ce484222325 ^ 0xa1, id as u64);
    }

    // Pid
    if term.is_pid() {
        return hash_mix(0xcbf29ce484222325 ^ 0xb1, term.0);
    }

    // Nil
    if term.is_nil() {
        return 0xcbf29ce484222325 ^ 0xc1;
    }

    // Cons cell — iterate to avoid stack overflow
    if term.is_cons() {
        let mut hash = 0xcbf29ce484222325_u64 ^ 0xd1;
        let mut current = term;
        while current.is_cons() {
            if let Some(ptr) = current.as_cons_ptr() {
                unsafe {
                    hash = hash_mix(hash, hash_erlang_term((*ptr).head));
                    current = (*ptr).tail;
                }
            } else {
                break;
            }
        }
        // Hash the tail (nil or improper)
        if !current.is_nil() {
            hash = hash_mix(hash, hash_erlang_term(current));
        }
        return hash;
    }

    // Boxed types
    if term.is_boxed() {
        if let Some(ptr) = term.as_boxed_ptr() {
            unsafe {
                let header = BoxedHeader(*ptr);
                match header.subtype() {
                    BOXED_FLOAT => {
                        let bits = *ptr.add(1);
                        let val = f64::from_bits(bits);
                        // If float is a whole number, hash it as integer for consistency
                        if val.fract() == 0.0 && val.is_finite() {
                            return hash_mix(0xcbf29ce484222325, val as i64 as u64);
                        }
                        return hash_mix(0xcbf29ce484222325 ^ 0xe1, bits);
                    }
                    BOXED_TUPLE => {
                        let arity = header.arity() as usize;
                        let mut hash = hash_mix(0xcbf29ce484222325 ^ 0xf1, arity as u64);
                        for i in 1..=arity {
                            hash = hash_mix(hash, hash_erlang_term(ErlTerm(*ptr.add(i))));
                        }
                        return hash;
                    }
                    BOXED_BINARY | BOXED_REFC_BINARY => {
                        let size = header.arity() as usize;
                        let byte_len = size.div_ceil(8);
                        let data_ptr = if header.subtype() == BOXED_BINARY {
                            (ptr.add(1)) as *const u8
                        } else {
                            // Read the stable backing object to get the data pointer.
                            // The raw word is a *const BinaryData, not a direct data pointer.
                            let raw_arc_ptr = *ptr.add(REFC_BINARY_PTR_WORD_INDEX) as *const BinaryData;
                            let offset = *ptr.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
                            let vec_ref = &*raw_arc_ptr;
                            vec_ref.as_ptr().add(offset)
                        };
                        let mut hash = 0xcbf29ce484222325_u64 ^ 0xa2;
                        for i in 0..byte_len {
                            hash = hash_mix(hash, *data_ptr.add(i) as u64);
                        }
                        return hash;
                    }
                    BOXED_MAP => {
                        let num_pairs = header.arity() as usize;
                        // Order-independent: XOR individual pair hashes
                        let mut hash = 0_u64;
                        for i in 0..num_pairs {
                            let key = ErlTerm(*ptr.add(1 + i * 2));
                            let val = ErlTerm(*ptr.add(2 + i * 2));
                            let pair_hash = hash_mix(
                                hash_erlang_term(key),
                                hash_erlang_term(val),
                            );
                            hash ^= pair_hash;
                        }
                        return hash_mix(0xcbf29ce484222325 ^ 0xb2, hash);
                    }
                    BOXED_BIGINT => {
                        let limb_count = header.arity() as usize;
                        let mut hash = 0xcbf29ce484222325_u64 ^ 0xc2;
                        // Hash sign word
                        hash = hash_mix(hash, *ptr.add(1));
                        // Hash limbs
                        for i in 0..limb_count {
                            hash = hash_mix(hash, *ptr.add(2 + i));
                        }
                        return hash;
                    }
                    BOXED_REFERENCE => {
                        return hash_mix(0xcbf29ce484222325 ^ 0xd2, *ptr.add(1));
                    }
                    BOXED_CLOSURE => {
                        return hash_mix(0xcbf29ce484222325 ^ 0xe2, *ptr.add(1));
                    }
                    _ => {
                        // Unknown boxed type — hash raw bits
                        return hash_mix(0xcbf29ce484222325 ^ 0xff, term.0);
                    }
                }
            }
        }
    }

    // Fallback
    hash_mix(0xcbf29ce484222325, term.0)
}

/// `erlang:phash2/1` -- portable hash of an Erlang term.
///
/// Returns an integer in the range 0..(2^27 - 1).
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_phash2(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let hash = hash_erlang_term(args[0]);
    let result = (hash % (1 << 27)) as i64;
    Ok(ErlTerm::small_int(result))
}

/// `erlang:phash2/2` -- portable hash with a custom range.
///
/// Returns an integer in the range 0..(Range - 1).
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_phash2_range(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 2 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let range = match args[1].as_small_int() {
        Some(r) if r > 0 => r as u64,
        _ => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    let hash = hash_erlang_term(args[0]);
    let result = (hash % range) as i64;
    Ok(ErlTerm::small_int(result))
}

// ---------------------------------------------------------------------------
// Numeric operations
// ---------------------------------------------------------------------------

/// `erlang:abs/1` -- absolute value of an integer or float.
/// # Safety
/// Arguments must reference live storage with the runtime layout, unmoved and immutable
/// throughout the call. No moving GC may run while these raw terms are used.
pub unsafe fn bif_abs(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let term = args[0];

    // Small integer
    if let Some(val) = term.as_small_int() {
        let result = val.abs();
        if crate::term::fits_small_int(result) {
            return Ok(ErlTerm::small_int(result));
        }
        // abs(SMALL_INT_MIN) overflows small int range
        return Ok(crate::bigint::bigint_to_term(heap, num_bigint::BigInt::from(result)));
    }

    // Bigint
    if let Some(big) = crate::bigint::term_to_bigint(term) {
        use num_traits::Signed;
        return Ok(crate::bigint::bigint_to_term(heap, big.abs()));
    }

    // Boxed float
    if term.is_boxed() {
        unsafe {
            let ptr = term.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*ptr);
            if header.subtype() == BOXED_FLOAT {
                let bits = *ptr.add(1);
                let val = f64::from_bits(bits);
                return Ok(heap.alloc_float(val.abs()));
            }
        }
    }

    Err(ErlTerm::atom(ATOM_BADARG))
}

/// `erlang:round/1` -- round a number to the nearest integer.
///
/// Integers pass through unchanged. Floats are rounded to the nearest integer
/// using Rust's `f64::round()` (rounds half away from zero, matching Erlang).
/// # Safety
/// Arguments must reference live storage with the runtime layout, unmoved and immutable
/// throughout the call. No moving GC may run while these raw terms are used.
pub unsafe fn bif_round(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let term = args[0];

    // Small integer — pass through
    if term.as_small_int().is_some() {
        return Ok(term);
    }

    // Bigint — pass through
    if crate::bigint::term_to_bigint(term).is_some() {
        return Ok(term);
    }

    // Boxed float
    if term.is_boxed() {
        unsafe {
            let ptr = term.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*ptr);
            if header.subtype() == BOXED_FLOAT {
                let bits = *ptr.add(1);
                let val = f64::from_bits(bits);
                let rounded = val.round() as i64;
                if crate::term::fits_small_int(rounded) {
                    return Ok(ErlTerm::small_int(rounded));
                }
                return Ok(bigint_to_term(heap, num_bigint::BigInt::from(rounded)));
            }
        }
    }

    Err(ErlTerm::atom(ATOM_BADARG))
}

/// `erlang:trunc/1` -- truncate a number towards zero.
///
/// Integers pass through unchanged. Floats are truncated towards zero
/// using Rust's `f64::trunc()`.
/// # Safety
/// Arguments must reference live storage with the runtime layout, unmoved and immutable
/// throughout the call. No moving GC may run while these raw terms are used.
pub unsafe fn bif_trunc(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let term = args[0];

    // Small integer — pass through
    if term.as_small_int().is_some() {
        return Ok(term);
    }

    // Bigint — pass through
    if crate::bigint::term_to_bigint(term).is_some() {
        return Ok(term);
    }

    // Boxed float
    if term.is_boxed() {
        unsafe {
            let ptr = term.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*ptr);
            if header.subtype() == BOXED_FLOAT {
                let bits = *ptr.add(1);
                let val = f64::from_bits(bits);
                let truncated = val.trunc() as i64;
                if crate::term::fits_small_int(truncated) {
                    return Ok(ErlTerm::small_int(truncated));
                }
                return Ok(bigint_to_term(heap, num_bigint::BigInt::from(truncated)));
            }
        }
    }

    Err(ErlTerm::atom(ATOM_BADARG))
}

/// `erlang:ceil/1` -- return the smallest integer not less than the argument.
///
/// Integers pass through unchanged. Floats are rounded up using `f64::ceil()`.
/// # Safety
/// Arguments must reference live storage with the runtime layout, unmoved and immutable
/// throughout the call. No moving GC may run while these raw terms are used.
pub unsafe fn bif_ceil(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let term = args[0];

    // Small integer — pass through
    if term.as_small_int().is_some() {
        return Ok(term);
    }

    // Bigint — pass through
    if crate::bigint::term_to_bigint(term).is_some() {
        return Ok(term);
    }

    // Boxed float
    if term.is_boxed() {
        unsafe {
            let ptr = term.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*ptr);
            if header.subtype() == BOXED_FLOAT {
                let bits = *ptr.add(1);
                let val = f64::from_bits(bits);
                let ceiled = val.ceil() as i64;
                if crate::term::fits_small_int(ceiled) {
                    return Ok(ErlTerm::small_int(ceiled));
                }
                return Ok(bigint_to_term(heap, num_bigint::BigInt::from(ceiled)));
            }
        }
    }

    Err(ErlTerm::atom(ATOM_BADARG))
}

/// `erlang:floor/1` -- return the largest integer not greater than the argument.
///
/// Integers pass through unchanged. Floats are rounded down using `f64::floor()`.
/// # Safety
/// Arguments must reference live storage with the runtime layout, unmoved and immutable
/// throughout the call. No moving GC may run while these raw terms are used.
pub unsafe fn bif_floor(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let term = args[0];

    // Small integer — pass through
    if term.as_small_int().is_some() {
        return Ok(term);
    }

    // Bigint — pass through
    if crate::bigint::term_to_bigint(term).is_some() {
        return Ok(term);
    }

    // Boxed float
    if term.is_boxed() {
        unsafe {
            let ptr = term.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*ptr);
            if header.subtype() == BOXED_FLOAT {
                let bits = *ptr.add(1);
                let val = f64::from_bits(bits);
                let floored = val.floor() as i64;
                if crate::term::fits_small_int(floored) {
                    return Ok(ErlTerm::small_int(floored));
                }
                return Ok(bigint_to_term(heap, num_bigint::BigInt::from(floored)));
            }
        }
    }

    Err(ErlTerm::atom(ATOM_BADARG))
}

/// `erlang:float/1` -- convert a number to float.
///
/// Floats pass through unchanged. Integers (small and big) are converted to float.
/// # Safety
/// Arguments must reference live storage with the runtime layout, unmoved and immutable
/// throughout the call. No moving GC may run while these raw terms are used.
pub unsafe fn bif_float(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let term = args[0];

    // Small integer → float
    if let Some(val) = term.as_small_int() {
        return Ok(heap.alloc_float(val as f64));
    }

    // Bigint → float
    if let Some(big) = crate::bigint::term_to_bigint(term) {
        use num_traits::ToPrimitive;
        if let Some(f) = big.to_f64() {
            return Ok(heap.alloc_float(f));
        }
        return Err(ErlTerm::atom(ATOM_BADARG));
    }

    // Boxed float — pass through
    if term.is_boxed() {
        unsafe {
            let ptr = term.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*ptr);
            if header.subtype() == BOXED_FLOAT {
                return Ok(term);
            }
        }
    }

    Err(ErlTerm::atom(ATOM_BADARG))
}

/// `erlang:max/2` -- return the larger of two terms, the first when they
/// compare equal. The result is one of the arguments, unchanged.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_max(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 2 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    Ok(compare_and_pick(args[0], args[1], true))
}

/// `erlang:min/2` -- return the smaller of two terms, the first when they
/// compare equal. The result is one of the arguments, unchanged.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_min(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 2 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    Ok(compare_and_pick(args[0], args[1], false))
}

/// Helper for max/min: returns `a` or `b`, whichever is larger (if
/// `pick_max`) or smaller in the term order, and `a` when they compare equal,
/// as on BEAM. Neither argument is converted: an integer and a float compare
/// exactly, as with `<`, and the one picked comes back with its own type.
/// # Safety
/// Source graphs obey the enclosing BIF live, immutable, unmoved input contract.
unsafe fn compare_and_pick(a: ErlTerm, b: ErlTerm, pick_max: bool) -> ErlTerm {
    let order = crate::ffi::compare::compare_terms(a, b);
    let pick_a = if pick_max { order >= 0 } else { order <= 0 };
    if pick_a { a } else { b }
}

// ---------------------------------------------------------------------------
// Tuple operations
// ---------------------------------------------------------------------------

/// `erlang:element/2` -- get tuple element (1-indexed, like Erlang).
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_element(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 2 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let index = match args[0].as_small_int() {
        Some(i) if i >= 1 => i as u64,
        _ => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    let ptr = match args[1].as_boxed_ptr() {
        Some(p) => p,
        None => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    unsafe {
        let header = BoxedHeader(*ptr);
        if header.subtype() != BOXED_TUPLE {
            return Err(ErlTerm::atom(ATOM_BADARG));
        }
        let arity = header.arity();
        if index < 1 || index > arity {
            return Err(ErlTerm::atom(ATOM_BADARG));
        }
        Ok(ErlTerm(*ptr.add(index as usize)))
    }
}

/// `erlang:tuple_size/1`
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_tuple_size(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let ptr = match args[0].as_boxed_ptr() {
        Some(p) => p,
        None => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    unsafe {
        let header = BoxedHeader(*ptr);
        if header.subtype() != BOXED_TUPLE {
            return Err(ErlTerm::atom(ATOM_BADARG));
        }
        Ok(ErlTerm::small_int(header.arity() as i64))
    }
}

// ---------------------------------------------------------------------------
// Conversion BIFs
// ---------------------------------------------------------------------------

/// `erlang:atom_to_list/1` -- convert atom to list of codepoints.
pub fn bif_atom_to_list(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let id = match args[0].as_atom_id() {
        Some(id) => id,
        None => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    let table = global_atom_table();
    let name = match table.resolve(id) {
        Some(s) => s,
        None => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    let codepoints: Vec<ErlTerm> = name.chars().map(|c| ErlTerm::small_int(c as i64)).collect();
    Ok(heap.alloc_list_from_slice(&codepoints))
}

/// `erlang:list_to_atom/1` -- convert list of codepoints to atom.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_list_to_atom(_heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let s = match term_to_string(args[0]) {
        Ok(s) => s,
        Err(()) => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    let id = global_atom_table().intern(&s);
    Ok(ErlTerm::atom(id))
}

/// `erlang:integer_to_list/1` -- convert integer to list of digit codepoints.
/// # Safety
/// Arguments must reference live storage with the runtime layout, unmoved and immutable
/// throughout the call. No moving GC may run while these raw terms are used.
pub unsafe fn bif_integer_to_list(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    // Handle small int fast path
    if let Some(v) = args[0].as_small_int() {
        let s = v.to_string();
        let codepoints: Vec<ErlTerm> = s.chars().map(|c| ErlTerm::small_int(c as i64)).collect();
        return Ok(heap.alloc_list_from_slice(&codepoints));
    }
    // Handle bigint
    if let Some(big) = term_to_bigint(args[0]) {
        let s = big.to_string();
        let codepoints: Vec<ErlTerm> = s.chars().map(|c| ErlTerm::small_int(c as i64)).collect();
        return Ok(heap.alloc_list_from_slice(&codepoints));
    }
    Err(ErlTerm::atom(ATOM_BADARG))
}

/// `erlang:list_to_integer/1` -- convert list of digit codepoints to integer.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_list_to_integer(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let s = match term_to_string(args[0]) {
        Ok(s) => s,
        Err(()) => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    // Try small int first (fast path)
    if let Ok(val) = s.parse::<i64>() {
        if crate::term::fits_small_int(val) {
            return Ok(ErlTerm::small_int(val));
        }
    }
    // Fall back to BigInt for large numbers
    use num_bigint::BigInt;
    match s.parse::<BigInt>() {
        Ok(big) => Ok(bigint_to_term(heap, big)),
        Err(_) => Err(ErlTerm::atom(ATOM_BADARG)),
    }
}

/// `erlang:float_to_list/1` -- convert float to list of codepoints.
///
/// Uses Erlang's default scientific notation format.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_float_to_list(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let val = if args[0].is_boxed() {
        unsafe {
            let ptr = match args[0].as_boxed_ptr() {
                Some(p) => p,
                None => return Err(ErlTerm::atom(ATOM_BADARG)),
            };
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_FLOAT {
                return Err(ErlTerm::atom(ATOM_BADARG));
            }
            f64::from_bits(*ptr.add(1))
        }
    } else {
        return Err(ErlTerm::atom(ATOM_BADARG));
    };
    let s = format!("{:.20e}", val);
    let codepoints: Vec<ErlTerm> = s.chars().map(|c| ErlTerm::small_int(c as i64)).collect();
    Ok(heap.alloc_list_from_slice(&codepoints))
}

/// `erlang:list_to_tuple/1` -- convert a list to a tuple.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_list_to_tuple(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let mut elems = Vec::new();
    let mut cursor = args[0];
    while cursor.is_cons() {
        unsafe {
            let cell = &*cursor.as_cons_ptr().unwrap();
            elems.push(cell.head);
            cursor = cell.tail;
        }
    }
    if !cursor.is_nil() {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    Ok(heap.alloc_tuple(&elems))
}

/// `erlang:tuple_to_list/1` -- convert a tuple to a list.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_tuple_to_list(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let ptr = match args[0].as_boxed_ptr() {
        Some(p) => p,
        None => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    unsafe {
        let header = BoxedHeader(*ptr);
        if header.subtype() != BOXED_TUPLE {
            return Err(ErlTerm::atom(ATOM_BADARG));
        }
        let arity = header.arity() as usize;
        let elems: Vec<ErlTerm> = (0..arity).map(|i| ErlTerm(*ptr.add(1 + i))).collect();
        Ok(heap.alloc_list_from_slice(&elems))
    }
}

/// `erlang:setelement/3` -- set a tuple element (1-indexed).
///
/// Returns a new tuple with the element at `index` replaced by `value`.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_setelement(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 3 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let index = match args[0].as_small_int() {
        Some(i) if i >= 1 => i as usize,
        _ => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    let ptr = match args[1].as_boxed_ptr() {
        Some(p) => p,
        None => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    let value = args[2];
    unsafe {
        let header = BoxedHeader(*ptr);
        if header.subtype() != BOXED_TUPLE {
            return Err(ErlTerm::atom(ATOM_BADARG));
        }
        let arity = header.arity() as usize;
        if index < 1 || index > arity {
            return Err(ErlTerm::atom(ATOM_BADARG));
        }
        let mut elems: Vec<ErlTerm> = (0..arity).map(|i| ErlTerm(*ptr.add(1 + i))).collect();
        elems[index - 1] = value;
        Ok(heap.alloc_tuple(&elems))
    }
}

/// `erlang:make_tuple/2` -- create a tuple of given size filled with a default value.
///
/// `make_tuple(Arity, InitialValue)` returns a tuple of size `Arity` where every
/// element is `InitialValue`.
pub fn bif_make_tuple_2(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 2 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let arity = match args[0].as_small_int() {
        Some(a) if a >= 0 => a as usize,
        _ => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    let init_value = args[1];
    let elems = vec![init_value; arity];
    Ok(heap.alloc_tuple(&elems))
}

/// `erlang:make_tuple/3` -- create a tuple with specific positions set.
///
/// `make_tuple(Arity, DefaultValue, InitList)` returns a tuple of size `Arity`
/// where every element is `DefaultValue`, except for positions specified in
/// `InitList` (a list of `{Position, Value}` tuples, 1-indexed).
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_make_tuple_3(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 3 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let arity = match args[0].as_small_int() {
        Some(a) if a >= 0 => a as usize,
        _ => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    let default_value = args[1];
    let mut elems = vec![default_value; arity];

    // Walk InitList: a list of {Position, Value} tuples
    let mut cursor = args[2];
    while cursor.is_cons() {
        unsafe {
            let cell = &*cursor.as_cons_ptr().unwrap();
            let item = cell.head;
            // Each item must be a {Position, Value} tuple
            let ptr = match item.as_boxed_ptr() {
                Some(p) => p,
                None => return Err(ErlTerm::atom(ATOM_BADARG)),
            };
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_TUPLE || header.arity() != 2 {
                return Err(ErlTerm::atom(ATOM_BADARG));
            }
            let pos = match ErlTerm(*ptr.add(1)).as_small_int() {
                Some(p) if p >= 1 && (p as usize) <= arity => p as usize,
                _ => return Err(ErlTerm::atom(ATOM_BADARG)),
            };
            let val = ErlTerm(*ptr.add(2));
            elems[pos - 1] = val;
            cursor = cell.tail;
        }
    }
    if !cursor.is_nil() {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    Ok(heap.alloc_tuple(&elems))
}

/// `erlang:append_element/2` -- append an element to a tuple.
///
/// `append_element(Tuple, Element)` returns a new tuple with `Element`
/// appended at the end.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_append_element(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 2 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let ptr = match args[0].as_boxed_ptr() {
        Some(p) => p,
        None => return Err(ErlTerm::atom(ATOM_BADARG)),
    };
    unsafe {
        let header = BoxedHeader(*ptr);
        if header.subtype() != BOXED_TUPLE {
            return Err(ErlTerm::atom(ATOM_BADARG));
        }
        let arity = header.arity() as usize;
        let mut elems: Vec<ErlTerm> = (0..arity).map(|i| ErlTerm(*ptr.add(1 + i))).collect();
        elems.push(args[1]);
        Ok(heap.alloc_tuple(&elems))
    }
}

/// `erlang:is_float/1`
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_is_float(args: &[ErlTerm]) -> ErlTerm {
    if args.len() == 1 && args[0].is_boxed() {
        unsafe {
            if let Some(ptr) = args[0].as_boxed_ptr() {
                let header = BoxedHeader(*ptr);
                if header.subtype() == BOXED_FLOAT {
                    return ErlTerm::atom(ATOM_TRUE);
                }
            }
        }
    }
    ErlTerm::atom(ATOM_FALSE)
}

/// `erlang:is_number/1`
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn bif_is_number(args: &[ErlTerm]) -> ErlTerm {
    if args.len() != 1 {
        return ErlTerm::atom(ATOM_FALSE);
    }
    if args[0].is_small_int() || args[0].is_bigint() {
        return ErlTerm::atom(ATOM_TRUE);
    }
    bif_is_float(args)
}

/// `erlang:is_boolean/1`
pub fn bif_is_boolean(args: &[ErlTerm]) -> ErlTerm {
    if args.len() == 1 {
        if let Some(id) = args[0].as_atom_id() {
            if id == ATOM_TRUE || id == ATOM_FALSE {
                return ErlTerm::atom(ATOM_TRUE);
            }
        }
    }
    ErlTerm::atom(ATOM_FALSE)
}

// ---------------------------------------------------------------------------
// Formatting / conversion helpers
// ---------------------------------------------------------------------------

/// Convert an atom or Unicode chardata format to its text.
/// # Safety
/// Reachable source storage must remain live, immutable and unmoved.
pub(crate) unsafe fn format_string(term: ErlTerm) -> Result<String, ()> {
    if let Some(atom) = term.as_atom_id() {
        global_atom_table().resolve(atom).ok_or(())
    } else {
        crate::ffi::unicode::chardata_to_utf8(term)
    }
}

/// Convert an Erlang list of integer codepoints to a Rust `String`.
///
/// Returns `Err(())` if the term is not a proper list of valid codepoints.
/// We intentionally use `()` as the error type since no further error
/// context is needed.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
#[allow(clippy::result_unit_err)]
pub unsafe fn term_to_string(term: ErlTerm) -> Result<String, ()> {
    let mut s = String::new();
    let mut cursor = term;
    while cursor.is_cons() {
        unsafe {
            let cell = &*cursor.as_cons_ptr().unwrap();
            let ch = cell.head.as_small_int()
                .and_then(|cp| u32::try_from(cp).ok())
                .and_then(char::from_u32).ok_or(())?;
            s.push(ch);
            cursor = cell.tail;
        }
    }
    if cursor.is_nil() {
        Ok(s)
    } else {
        Err(())
    }
}

/// Format binary bytes for display, handling sub-byte trailing bits.
/// `bit_count` is the total number of significant bits.
fn format_binary_bytes(bytes: &[u8], bit_count: usize) -> String {
    let trailing_bits = bit_count % 8;
    if trailing_bits == 0 {
        // Byte-aligned: display as before
        let mut parts: Vec<String> = Vec::with_capacity(bytes.len());
        for b in bytes {
            parts.push(format!("{}", b));
        }
        format!("<<{}>>", parts.join(","))
    } else {
        // Sub-byte trailing: complete bytes + partial last byte
        let full_bytes = bit_count / 8;
        let mut parts: Vec<String> = Vec::with_capacity(full_bytes + 1);
        for b in &bytes[..full_bytes] {
            parts.push(format!("{}", b));
        }
        // Last partial byte: extract the significant high bits
        if full_bytes < bytes.len() {
            let last_byte = bytes[full_bytes];
            let value = last_byte >> (8 - trailing_bits);
            parts.push(format!("{}:{}", value, trailing_bits));
        }
        format!("<<{}>>", parts.join(","))
    }
}

/// Format an `ErlTerm` for display, similar to Erlang's `io_lib:format("~w", [Term])`.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn format_term(term: ErlTerm) -> String {
    if term.is_nil() {
        return "[]".to_string();
    }

    if let Some(val) = term.as_small_int() {
        return val.to_string();
    }

    if let Some(id) = term.as_atom_id() {
        let table = global_atom_table();
        return table.resolve(id).unwrap_or_else(|| format!("atom#{}", id));
    }

    if let Some(pid) = term.as_pid_id() {
        return format!("<0.{}.0>", pid);
    }

    if term.is_cons() {
        return format_list(term);
    }

    if term.is_boxed() {
        return format_boxed(term);
    }

    format!("#unknown<{:#x}>", term.0)
}

/// Format a cons-cell chain as an Erlang list: `[1,2,3]` for proper
/// lists or `[1,2|3]` for improper lists.
/// # Safety
/// Source graphs obey the enclosing BIF live, immutable, unmoved input contract.
unsafe fn format_list(term: ErlTerm) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut cursor = term;

    while cursor.is_cons() {
        unsafe {
            let cell = &*cursor.as_cons_ptr().unwrap();
            parts.push(format_term(cell.head));
            cursor = cell.tail;
        }
    }

    if cursor.is_nil() {
        format!("[{}]", parts.join(","))
    } else {
        format!("[{}|{}]", parts.join(","), format_term(cursor))
    }
}

/// Format a boxed term: tuple, float, or generic.
/// # Safety
/// Source graphs obey the enclosing BIF live, immutable, unmoved input contract.
unsafe fn format_boxed(term: ErlTerm) -> String {
    unsafe {
        let ptr = term.as_boxed_ptr().unwrap();
        let header = BoxedHeader(*ptr);
        let arity = header.arity() as usize;
        let subtype = header.subtype();

        match subtype {
            BOXED_TUPLE => {
                let mut elems: Vec<String> = Vec::with_capacity(arity);
                for i in 0..arity {
                    elems.push(format_term(ErlTerm(*ptr.add(1 + i))));
                }
                format!("{{{}}}", elems.join(","))
            }
            BOXED_FLOAT => {
                let bits = *ptr.add(1);
                let val = f64::from_bits(bits);
                // Use Erlang-like float formatting.
                if val.fract() == 0.0 && val.is_finite() {
                    format!("{:.1}", val)
                } else {
                    format!("{}", val)
                }
            }
            BOXED_BINARY => {
                // arity stores bit count; derive byte count
                let bit_count = arity;
                let byte_len = bit_count.div_ceil(8);
                let data_ptr = ptr.add(1) as *const u8;
                let bytes = std::slice::from_raw_parts(data_ptr, byte_len);
                format_binary_bytes(bytes, bit_count)
            }
            BOXED_REFC_BINARY => {
                // arity stores bit count; derive byte count
                let bit_count = arity;
                let byte_len = bit_count.div_ceil(8);
                let raw_arc_ptr = *ptr.add(REFC_BINARY_PTR_WORD_INDEX) as *const BinaryData;
                let offset = *ptr.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
                let vec_ref = &*raw_arc_ptr;
                let bytes = &vec_ref[offset..offset + byte_len];
                format_binary_bytes(bytes, bit_count)
            }
            BOXED_MAP => {
                // arity = number of key-value pairs
                let npairs = arity;
                let mut parts: Vec<String> = Vec::with_capacity(npairs);
                for i in crate::ffi::compare::map_iteration_order(ptr, npairs) {
                    let key = ErlTerm(*ptr.add(1 + i * 2));
                    let val = ErlTerm(*ptr.add(1 + i * 2 + 1));
                    parts.push(format!("{} => {}", format_term(key), format_term(val)));
                }
                format!("#{{{}}}", parts.join(","))
            }
            BOXED_CLOSURE => {
                let fun_arity = *ptr.add(2);
                format!("#Fun</{}>", fun_arity)
            }
            BOXED_BIGINT => {
                match term_to_bigint(term) {
                    Some(val) => val.to_string(),
                    None => format!("#bigint<limbs={}>", arity),
                }
            }
            BOXED_REFERENCE => {
                let id = *ptr.add(1);
                format!("#Ref<0.0.0.{}>", id)
            }
            _ => {
                format!("#boxed<type={}, arity={}>", subtype, arity)
            }
        }
    }
}

/// Format a term with a depth limit, as `~W` (`pretty = false`) or `~P`
/// (`pretty = true`) do. A negative depth is unlimited. Terms deeper than
/// the limit become `...`, following OTP's `io_lib:write/2` for `~W` and
/// `io_lib_pretty` for `~P`; the two differ only for maps. Leaves use
/// `format_term`, so `~P` shares `~p`'s rendering (no strings or line
/// breaks).
/// # Safety
/// Source graphs obey the enclosing BIF live, immutable, unmoved input contract.
pub(crate) unsafe fn format_term_limited(term: ErlTerm, depth: i64, pretty: bool) -> String {
    let mut out = String::new();
    write_limited(&mut out, term, depth, pretty);
    out
}

/// # Safety
/// Source graphs obey the enclosing BIF live, immutable, unmoved input contract.
unsafe fn write_limited(out: &mut String, term: ErlTerm, depth: i64, pretty: bool) {
    if depth == 0 {
        out.push_str("...");
        return;
    }
    let below = depth.saturating_sub(1);
    if let Some(cell) = term.as_cons_ptr() {
        if depth == 1 {
            out.push_str("[...]");
            return;
        }
        out.push('[');
        write_limited(out, (*cell).head, below, pretty);
        let (mut tail, mut d) = ((*cell).tail, below);
        loop {
            if tail.is_nil() {
                break;
            }
            if d == 1 {
                out.push_str("|...");
                break;
            }
            if let Some(next) = tail.as_cons_ptr() {
                out.push(',');
                write_limited(out, (*next).head, d.saturating_sub(1), pretty);
                tail = (*next).tail;
                d = d.saturating_sub(1);
            } else {
                out.push('|');
                write_limited(out, tail, d.saturating_sub(1), pretty);
                break;
            }
        }
        out.push(']');
        return;
    }
    let Some(ptr) = term.as_boxed_ptr() else {
        out.push_str(&format_term(term));
        return;
    };
    let header = BoxedHeader(*ptr);
    let arity = header.arity() as usize;
    match header.subtype() {
        BOXED_TUPLE if arity > 0 => {
            if depth == 1 {
                out.push_str("{...}");
                return;
            }
            out.push('{');
            write_limited(out, ErlTerm(*ptr.add(1)), below, pretty);
            let mut d = below;
            for i in 1..arity {
                if d == 1 {
                    out.push_str(",...");
                    break;
                }
                out.push(',');
                write_limited(out, ErlTerm(*ptr.add(1 + i)), d.saturating_sub(1), pretty);
                d = d.saturating_sub(1);
            }
            out.push('}');
        }
        BOXED_MAP if arity > 0 => {
            if depth == 1 {
                out.push_str(if pretty { "#{...}" } else { "#{}" });
                return;
            }
            out.push_str("#{");
            // Each association is written one level down, and each costs
            // one level of `left`. io_lib_pretty prints `,...` only when an
            // association remains; io_lib:write also prints it when the
            // count reaches 1 with nothing left.
            let order = crate::ffi::compare::map_iteration_order(ptr, arity);
            let mut left = depth;
            let mut cut = false;
            for (n, &i) in order.iter().enumerate() {
                if left == 1 {
                    cut = true;
                    break;
                }
                if n > 0 {
                    out.push(',');
                }
                write_limited(out, ErlTerm(*ptr.add(1 + i * 2)), below, pretty);
                out.push_str(" => ");
                write_limited(out, ErlTerm(*ptr.add(2 + i * 2)), below, pretty);
                left = left.saturating_sub(1);
            }
            if cut || (!pretty && left == 1) {
                out.push_str(",...");
            }
            out.push('}');
        }
        BOXED_BINARY | BOXED_REFC_BINARY if arity > 0 => {
            let bit_count = arity;
            let byte_len = bit_count.div_ceil(8);
            let bytes: &[u8] = if header.subtype() == BOXED_BINARY {
                std::slice::from_raw_parts(ptr.add(1) as *const u8, byte_len)
            } else {
                let data = &*(*ptr.add(REFC_BINARY_PTR_WORD_INDEX) as *const BinaryData);
                let offset = *ptr.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
                &data[offset..offset + byte_len]
            };
            // Whole bytes, then the trailing bits as Value:Size. Each element
            // costs one level; `...` replaces the rest once one level is left.
            let full = bit_count / 8;
            let trailing = bit_count % 8;
            let units = full + usize::from(trailing > 0);
            out.push_str("<<");
            let mut d = depth;
            for unit in 0..units {
                if d == 1 {
                    out.push_str("...");
                    break;
                }
                if unit < full {
                    out.push_str(&bytes[unit].to_string());
                } else {
                    out.push_str(&format!("{}:{}", bytes[full] >> (8 - trailing), trailing));
                }
                if unit + 1 < units {
                    out.push(',');
                }
                d = d.saturating_sub(1);
            }
            out.push_str(">>");
        }
        _ => out.push_str(&format_term(term)),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// External Term Format (ETF) — term_to_binary / binary_to_term
// ---------------------------------------------------------------------------
//
// Implements a subset of the Erlang External Term Format (version 131).
// Reference: https://www.erlang.org/doc/apps/erts/erl_ext_dist.html
//
// Supported tags:
//   SMALL_INTEGER_EXT (97)  — integers 0..255
//   INTEGER_EXT       (98)  — signed 32-bit integers
//   SMALL_BIG_EXT     (110) — bigints up to 255 bytes
//   LARGE_BIG_EXT     (111) — bigints > 255 bytes
//   FLOAT_EXT         (70)  — IEEE 754 double (NEW_FLOAT_EXT)
//   ATOM_UTF8_EXT     (118) — atom (UTF-8, 16-bit length)
//   SMALL_ATOM_UTF8_EXT (119) — atom (UTF-8, 8-bit length)
//   NIL_EXT           (106) — empty list
//   LIST_EXT          (108) — non-empty proper/improper list
//   STRING_EXT        (107) — list of bytes (optimised encoding)
//   SMALL_TUPLE_EXT   (104) — tuple with arity 0..255
//   LARGE_TUPLE_EXT   (105) — tuple with arity > 255
//   BINARY_EXT        (109) — binary
//   MAP_EXT           (116) — map
//   NEW_PID_EXT       (88)  — pid (simplified: local node only)

const ETF_VERSION: u8 = 131;
const SMALL_INTEGER_EXT: u8 = 97;
const INTEGER_EXT: u8 = 98;
const NEW_FLOAT_EXT: u8 = 70;
const ATOM_UTF8_EXT: u8 = 118;
const SMALL_ATOM_UTF8_EXT: u8 = 119;
const NIL_EXT: u8 = 106;
const LIST_EXT: u8 = 108;
const STRING_EXT: u8 = 107;
const SMALL_TUPLE_EXT: u8 = 104;
const LARGE_TUPLE_EXT: u8 = 105;
const BINARY_EXT: u8 = 109;
const MAP_EXT: u8 = 116;
const SMALL_BIG_EXT: u8 = 110;
const LARGE_BIG_EXT: u8 = 111;
const NEW_PID_EXT: u8 = 88;

/// Encode an Erlang term into ETF bytes (appended to `buf`).
/// # Safety
/// Source graphs obey the enclosing BIF live, immutable, unmoved input contract.
unsafe fn etf_encode(term: ErlTerm, buf: &mut Vec<u8>) {
    // Small integer
    if let Some(val) = term.as_small_int() {
        if (0..=255).contains(&val) {
            buf.push(SMALL_INTEGER_EXT);
            buf.push(val as u8);
            return;
        }
        if val >= i32::MIN as i64 && val <= i32::MAX as i64 {
            buf.push(INTEGER_EXT);
            buf.extend_from_slice(&(val as i32).to_be_bytes());
            return;
        }
        // Doesn't fit i32 — encode as bigint
        etf_encode_bigint_from_i64(val, buf);
        return;
    }

    // Atom
    if let Some(id) = term.as_atom_id() {
        let table = global_atom_table();
        if let Some(name) = table.resolve(id) {
            let bytes = name.as_bytes();
            if bytes.len() <= 255 {
                buf.push(SMALL_ATOM_UTF8_EXT);
                buf.push(bytes.len() as u8);
            } else {
                buf.push(ATOM_UTF8_EXT);
                buf.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
            }
            buf.extend_from_slice(bytes);
        } else {
            // Unknown atom — encode as empty atom
            buf.push(SMALL_ATOM_UTF8_EXT);
            buf.push(0);
        }
        return;
    }

    // Pid
    if let Some(pid_id) = term.as_pid_id() {
        // Simplified encoding: encode as NEW_PID_EXT with local node
        buf.push(NEW_PID_EXT);
        // Node atom: 'nonode@nohost'
        let node = b"nonode@nohost";
        buf.push(SMALL_ATOM_UTF8_EXT);
        buf.push(node.len() as u8);
        buf.extend_from_slice(node);
        // ID (4 bytes)
        buf.extend_from_slice(&(pid_id.get() as u32).to_be_bytes());
        // Serial (4 bytes) retains the high process-id bits.
        buf.extend_from_slice(&((pid_id.get() >> 32) as u32).to_be_bytes());
        // Creation (4 bytes)
        buf.extend_from_slice(&0u32.to_be_bytes());
        return;
    }

    // Nil
    if term.is_nil() {
        buf.push(NIL_EXT);
        return;
    }

    // Cons (list)
    if term.is_cons() {
        // Collect list elements
        let mut elements: Vec<ErlTerm> = Vec::new();
        let mut cursor = term;
        while cursor.is_cons() {
            unsafe {
                let cell = &*cursor.as_cons_ptr().unwrap();
                elements.push(cell.head);
                cursor = cell.tail;
            }
        }

        // Check if all elements are bytes (0..255 small ints) for STRING_EXT
        if cursor.is_nil() && !elements.is_empty() && elements.len() <= 65535 {
            let all_bytes = elements.iter().all(|e| {
                matches!(e.as_small_int(), Some(v) if (0..=255).contains(&v))
            });
            if all_bytes {
                buf.push(STRING_EXT);
                buf.extend_from_slice(&(elements.len() as u16).to_be_bytes());
                for e in &elements {
                    buf.push(e.as_small_int().unwrap() as u8);
                }
                return;
            }
        }

        buf.push(LIST_EXT);
        buf.extend_from_slice(&(elements.len() as u32).to_be_bytes());
        for e in &elements {
            etf_encode(*e, buf);
        }
        // Tail (nil for proper list, or the improper tail)
        etf_encode(cursor, buf);
        return;
    }

    // Boxed types
    if let Some(ptr) = term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            match header.subtype() {
                BOXED_FLOAT => {
                    let bits = *ptr.add(1);
                    buf.push(NEW_FLOAT_EXT);
                    buf.extend_from_slice(&bits.to_be_bytes());
                }
                BOXED_TUPLE => {
                    let arity = header.arity() as usize;
                    if arity <= 255 {
                        buf.push(SMALL_TUPLE_EXT);
                        buf.push(arity as u8);
                    } else {
                        buf.push(LARGE_TUPLE_EXT);
                        buf.extend_from_slice(&(arity as u32).to_be_bytes());
                    }
                    for i in 1..=arity {
                        etf_encode(ErlTerm(*ptr.add(i)), buf);
                    }
                }
                BOXED_BINARY | BOXED_REFC_BINARY => {
                    if let Some(bytes) = term.binary_bytes_any() {
                        buf.push(BINARY_EXT);
                        buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                        buf.extend_from_slice(bytes);
                    }
                }
                BOXED_MAP => {
                    let npairs = header.arity() as usize;
                    buf.push(MAP_EXT);
                    buf.extend_from_slice(&(npairs as u32).to_be_bytes());
                    for i in crate::ffi::compare::map_iteration_order(ptr, npairs) {
                        let key = ErlTerm(*ptr.add(1 + i * 2));
                        let val = ErlTerm(*ptr.add(2 + i * 2));
                        etf_encode(key, buf);
                        etf_encode(val, buf);
                    }
                }
                BOXED_BIGINT => {
                    if let Some(big) = term_to_bigint(term) {
                        use num_bigint::Sign;
                        let (sign, bytes) = big.to_bytes_le();
                        let sign_byte: u8 = if sign == Sign::Minus { 1 } else { 0 };
                        if bytes.len() <= 255 {
                            buf.push(SMALL_BIG_EXT);
                            buf.push(bytes.len() as u8);
                        } else {
                            buf.push(LARGE_BIG_EXT);
                            buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                        }
                        buf.push(sign_byte);
                        buf.extend_from_slice(&bytes);
                    }
                }
                BOXED_REFERENCE => {
                    // Encode references as a 2-tuple {ref, Id} for round-trip
                    buf.push(SMALL_TUPLE_EXT);
                    buf.push(2);
                    // atom 'ref'
                    buf.push(SMALL_ATOM_UTF8_EXT);
                    buf.push(3);
                    buf.extend_from_slice(b"ref");
                    // id as integer
                    let id = *ptr.add(1);
                    etf_encode(ErlTerm::small_int(id as i64), buf);
                }
                _ => {
                    // Unsupported boxed type — encode as atom 'undefined'
                    buf.push(SMALL_ATOM_UTF8_EXT);
                    buf.push(9);
                    buf.extend_from_slice(b"undefined");
                }
            }
        }
    }
}

/// Encode a large small integer (doesn't fit i32) as SMALL_BIG_EXT.
fn etf_encode_bigint_from_i64(val: i64, buf: &mut Vec<u8>) {
    let sign_byte: u8 = if val < 0 { 1 } else { 0 };
    let abs_val = if val == i64::MIN {
        val as u64 // handle overflow
    } else {
        val.unsigned_abs()
    };
    // Encode as little-endian bytes, trimming trailing zeros
    let bytes = abs_val.to_le_bytes();
    let len = 8 - bytes.iter().rev().take_while(|&&b| b == 0).count();
    let len = if len == 0 { 1 } else { len };
    buf.push(SMALL_BIG_EXT);
    buf.push(len as u8);
    buf.push(sign_byte);
    buf.extend_from_slice(&bytes[..len]);
}

/// Decode an ETF-encoded term from `data` starting at `pos`.
/// Returns `(term, new_pos)` on success.
fn etf_decode(heap: &mut ProcessHeap, data: &[u8], pos: usize) -> Result<(ErlTerm, usize), ()> {
    if pos >= data.len() {
        return Err(());
    }
    let tag = data[pos];
    let pos = pos + 1;

    match tag {
        SMALL_INTEGER_EXT => {
            if pos >= data.len() { return Err(()); }
            Ok((ErlTerm::small_int(data[pos] as i64), pos + 1))
        }

        INTEGER_EXT => {
            if pos + 4 > data.len() { return Err(()); }
            let val = i32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
            Ok((ErlTerm::small_int(val as i64), pos + 4))
        }

        NEW_FLOAT_EXT => {
            if pos + 8 > data.len() { return Err(()); }
            let bits = u64::from_be_bytes([
                data[pos], data[pos+1], data[pos+2], data[pos+3],
                data[pos+4], data[pos+5], data[pos+6], data[pos+7],
            ]);
            let val = f64::from_bits(bits);
            Ok((heap.alloc_float(val), pos + 8))
        }

        SMALL_ATOM_UTF8_EXT => {
            if pos >= data.len() { return Err(()); }
            let len = data[pos] as usize;
            let pos = pos + 1;
            if pos + len > data.len() { return Err(()); }
            let name = std::str::from_utf8(&data[pos..pos+len]).map_err(|_| ())?;
            let table = global_atom_table();
            let id = table.intern(name);
            Ok((ErlTerm::atom(id), pos + len))
        }

        ATOM_UTF8_EXT => {
            if pos + 2 > data.len() { return Err(()); }
            let len = u16::from_be_bytes([data[pos], data[pos+1]]) as usize;
            let pos = pos + 2;
            if pos + len > data.len() { return Err(()); }
            let name = std::str::from_utf8(&data[pos..pos+len]).map_err(|_| ())?;
            let table = global_atom_table();
            let id = table.intern(name);
            Ok((ErlTerm::atom(id), pos + len))
        }

        NIL_EXT => {
            Ok((ErlTerm::nil(), pos))
        }

        STRING_EXT => {
            if pos + 2 > data.len() { return Err(()); }
            let len = u16::from_be_bytes([data[pos], data[pos+1]]) as usize;
            let pos = pos + 2;
            if pos + len > data.len() { return Err(()); }
            // Build list of integers from the bytes
            let mut list = ErlTerm::nil();
            for i in (0..len).rev() {
                list = heap.alloc_cons(ErlTerm::small_int(data[pos + i] as i64), list);
            }
            Ok((list, pos + len))
        }

        LIST_EXT => {
            if pos + 4 > data.len() { return Err(()); }
            let count = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
            let mut pos = pos + 4;
            // Every element needs at least a tag byte, plus one for the tail.
            // Check before reserving memory from the untrusted count.
            if count >= data.len() - pos { return Err(()); }
            let mut elements = Vec::with_capacity(count);
            for _ in 0..count {
                let (elem, new_pos) = etf_decode(heap, data, pos)?;
                elements.push(elem);
                pos = new_pos;
            }
            // Decode tail
            let (tail, pos) = etf_decode(heap, data, pos)?;
            // Build list from back to front
            let mut list = tail;
            for elem in elements.into_iter().rev() {
                list = heap.alloc_cons(elem, list);
            }
            Ok((list, pos))
        }

        SMALL_TUPLE_EXT => {
            if pos >= data.len() { return Err(()); }
            let arity = data[pos] as usize;
            let mut pos = pos + 1;
            // NIL_EXT is a valid one-byte element; do not assume two bytes.
            if arity > data.len() - pos { return Err(()); }
            let mut elements = Vec::with_capacity(arity);
            for _ in 0..arity {
                let (elem, new_pos) = etf_decode(heap, data, pos)?;
                elements.push(elem);
                pos = new_pos;
            }
            Ok((heap.alloc_tuple(&elements), pos))
        }

        LARGE_TUPLE_EXT => {
            if pos + 4 > data.len() { return Err(()); }
            let arity = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
            let mut pos = pos + 4;
            // NIL_EXT is a valid one-byte element; do not assume two bytes.
            if arity > data.len() - pos { return Err(()); }
            let mut elements = Vec::with_capacity(arity);
            for _ in 0..arity {
                let (elem, new_pos) = etf_decode(heap, data, pos)?;
                elements.push(elem);
                pos = new_pos;
            }
            Ok((heap.alloc_tuple(&elements), pos))
        }

        BINARY_EXT => {
            if pos + 4 > data.len() { return Err(()); }
            let len = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
            let pos = pos + 4;
            if pos + len > data.len() { return Err(()); }
            Ok((heap.alloc_binary(&data[pos..pos+len]), pos + len))
        }

        MAP_EXT => {
            if pos + 4 > data.len() { return Err(()); }
            let npairs = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
            let mut pos = pos + 4;
            // A key and a value each need at least one tag byte. Division
            // avoids overflowing while validating the untrusted pair count.
            if npairs > (data.len() - pos) / 2 { return Err(()); }
            let mut pairs = Vec::with_capacity(npairs);
            for _ in 0..npairs {
                let (key, new_pos) = etf_decode(heap, data, pos)?;
                let (val, new_pos) = etf_decode(heap, data, new_pos)?;
                pairs.push([key, val]);
                pos = new_pos;
            }
            Ok((heap.alloc_map(MapPairs::new(&pairs)), pos))
        }

        SMALL_BIG_EXT => {
            if pos >= data.len() { return Err(()); }
            let n = data[pos] as usize;
            let pos = pos + 1;
            if pos + 1 + n > data.len() { return Err(()); }
            let sign_byte = data[pos];
            let pos = pos + 1;
            let bytes = &data[pos..pos+n];
            let sign = if sign_byte == 1 {
                num_bigint::Sign::Minus
            } else if bytes.iter().all(|&b| b == 0) {
                num_bigint::Sign::NoSign
            } else {
                num_bigint::Sign::Plus
            };
            let big = num_bigint::BigInt::from_bytes_le(sign, bytes);
            Ok((bigint_to_term(heap, big), pos + n))
        }

        LARGE_BIG_EXT => {
            if pos + 4 > data.len() { return Err(()); }
            let n = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
            let pos = pos + 4;
            if pos + 1 + n > data.len() { return Err(()); }
            let sign_byte = data[pos];
            let pos = pos + 1;
            let bytes = &data[pos..pos+n];
            let sign = if sign_byte == 1 {
                num_bigint::Sign::Minus
            } else if bytes.iter().all(|&b| b == 0) {
                num_bigint::Sign::NoSign
            } else {
                num_bigint::Sign::Plus
            };
            let big = num_bigint::BigInt::from_bytes_le(sign, bytes);
            Ok((bigint_to_term(heap, big), pos + n))
        }

        NEW_PID_EXT => {
            // Decode node atom
            let (_node, pos) = etf_decode(heap, data, pos)?;
            if pos + 12 > data.len() { return Err(()); }
            let id = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
            let serial = u32::from_be_bytes([data[pos+4], data[pos+5], data[pos+6], data[pos+7]]);
            let raw_id = (id as u64) | ((serial as u64) << 32);
            let pid = crate::process::ProcessId::try_new(raw_id).ok_or(())?;
            // Creation is ignored for this local-node representation.
            Ok((ErlTerm::pid(pid), pos + 12))
        }

        _ => Err(()) // Unknown tag
    }
}

/// `erlang:term_to_binary/1` -- serialize an Erlang term to ETF binary.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_term_to_binary(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let mut buf = Vec::new();
    buf.push(ETF_VERSION);
    etf_encode(args[0], &mut buf);
    Ok(heap.alloc_binary(&buf))
}

/// `erlang:binary_to_term/1` -- deserialize an ETF binary to an Erlang term.
/// # Safety
/// Every reachable input object must have the runtime layout and remain live,
/// immutable and unmoved throughout this call, including externally owned
/// storage. No moving GC or reentrant mutation may occur during graph access.
pub unsafe fn bif_binary_to_term(heap: &mut ProcessHeap, args: &[ErlTerm]) -> Result<ErlTerm, ErlTerm> {
    if args.len() != 1 {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    let bytes = unsafe {
        match args[0].binary_bytes_any() {
            Some(b) => b,
            None => return Err(ErlTerm::atom(ATOM_BADARG)),
        }
    };
    if bytes.is_empty() || bytes[0] != ETF_VERSION {
        return Err(ErlTerm::atom(ATOM_BADARG));
    }
    match etf_decode(heap, bytes, 1) {
        Ok((term, _pos)) => Ok(term),
        Err(()) => Err(ErlTerm::atom(ATOM_BADARG)),
    }
}

#[cfg(test)]
mod tests {
    // SAFETY for raw BIF calls below: fixtures are initialized in owned heaps
    // retained by each test; these calls neither move storage nor reenter Erlang.
    use super::*;
    use crate::atom_table::global_atom_table;
    use crate::heap::{MapPairs, ProcessHeap};
    use crate::term::ErlTerm;

    #[test]
    fn format_term_small_int() {
        assert_eq!(unsafe { format_term(ErlTerm::small_int(42)) }, "42");
        assert_eq!(unsafe { format_term(ErlTerm::small_int(-7)) }, "-7");
        assert_eq!(unsafe { format_term(ErlTerm::small_int(0)) }, "0");
    }

    #[test]
    fn format_term_atom() {
        let table = global_atom_table();
        let _ = table; // ensure builtins are registered
        assert_eq!(unsafe { format_term(ErlTerm::atom(0)) }, "ok");
        assert_eq!(unsafe { format_term(ErlTerm::atom(2)) }, "true");
    }

    #[test]
    fn format_term_nil() {
        assert_eq!(unsafe { format_term(ErlTerm::nil()) }, "[]");
    }

    #[test]
    fn format_term_pid() {
        assert_eq!(unsafe { format_term(ErlTerm::pid(crate::process::ProcessId::new(42))) }, "<0.42.0>");
    }

    #[test]
    fn format_term_list() {
        let mut heap = ProcessHeap::new();
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(1),
            ErlTerm::small_int(2),
            ErlTerm::small_int(3),
        ]);
        assert_eq!(unsafe { format_term(list) }, "[1,2,3]");
    }

    #[test]
    fn format_term_improper_list() {
        let mut heap = ProcessHeap::new();
        let improper = heap.alloc_cons(ErlTerm::small_int(1), ErlTerm::small_int(2));
        assert_eq!(unsafe { format_term(improper) }, "[1|2]");
    }

    #[test]
    fn format_term_tuple() {
        let mut heap = ProcessHeap::new();
        let tuple = heap.alloc_tuple(&[ErlTerm::atom(0), ErlTerm::small_int(99)]);
        assert_eq!(unsafe { format_term(tuple) }, "{ok,99}");
    }

    #[test]
    fn format_term_float() {
        let mut heap = ProcessHeap::new();
        let f = heap.alloc_float(3.25);
        assert_eq!(unsafe { format_term(f) }, "3.25");

        let f2 = heap.alloc_float(42.0);
        assert_eq!(unsafe { format_term(f2) }, "42.0");
    }

    #[test]
    fn term_to_string_checks_full_unicode_scalar_before_narrowing() {
        let mut heap = ProcessHeap::new();
        let valid = [0,0x7f,0x80,0xd7ff,0xe000,0x10ffff];
        let terms: Vec<_> = valid.iter().map(|&cp| ErlTerm::small_int(cp)).collect();
        let list = heap.alloc_list_from_slice(&terms);
        let expected: String = valid.iter().map(|&cp| char::from_u32(cp as u32).unwrap()).collect();
        // SAFETY: all source cells remain in this live heap; no GC/reentry.
        assert_eq!(unsafe { term_to_string(list) },Ok(expected));
        for cp in [-1,0xd800,0xdfff,0x110000,0x1_0000_0041,0x1_0010_ffff,
                   0x100_0000_0041,crate::term::SMALL_INT_MAX] {
            let list = heap.alloc_list_from_slice(&[ErlTerm::small_int(cp)]);
            assert_eq!(unsafe { term_to_string(list) },Err(()),"accepted {cp:#x}");
            assert_eq!(unsafe { bif_list_to_atom(&mut heap,&[list]) },Err(ErlTerm::atom(ATOM_BADARG)));
        }
    }

    #[test]
    fn term_to_string_basic() {
        let mut heap = ProcessHeap::new();
        // "Hi" = [72, 105]
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(72),
            ErlTerm::small_int(105),
        ]);
        assert_eq!(unsafe { term_to_string(list) }, Ok("Hi".to_string()));
    }

    #[test]
    fn term_to_string_empty() {
        assert_eq!(unsafe { term_to_string(ErlTerm::nil()) }, Ok(String::new()));
    }

    #[test]
    fn term_to_string_not_a_list() {
        assert_eq!(unsafe { term_to_string(ErlTerm::small_int(42)) }, Err(()));
    }

    #[test]
    fn bif_length_proper_list() {
        let mut heap = ProcessHeap::new();
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(1),
            ErlTerm::small_int(2),
            ErlTerm::small_int(3),
        ]);
        let result = unsafe { bif_length(&mut heap, &[list]) }.unwrap();
        assert_eq!(result.as_small_int(), Some(3));
    }

    #[test]
    fn bif_length_empty_list() {
        let mut heap = ProcessHeap::new();
        let result = unsafe { bif_length(&mut heap, &[ErlTerm::nil()]) }.unwrap();
        assert_eq!(result.as_small_int(), Some(0));
    }

    #[test]
    fn bif_length_improper_list_is_badarg() {
        let mut heap = ProcessHeap::new();
        let improper = heap.alloc_cons(ErlTerm::small_int(1), ErlTerm::small_int(2));
        assert!(unsafe { bif_length(&mut heap, &[improper]) }.is_err());
    }

    #[test]
    fn bif_hd_tl() {
        let mut heap = ProcessHeap::new();
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(10),
            ErlTerm::small_int(20),
        ]);
        assert_eq!(unsafe { bif_hd(&mut heap, &[list]) }.unwrap().as_small_int(), Some(10));
        let tail = unsafe { bif_tl(&mut heap, &[list]) }.unwrap();
        assert!(tail.is_cons());
    }

    #[test]
    fn bif_hd_tl_badarg() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_hd(&mut heap, &[ErlTerm::nil()]) }.is_err());
        assert!(unsafe { bif_tl(&mut heap, &[ErlTerm::small_int(1)]) }.is_err());
    }

    #[test]
    fn bif_type_checks() {
        assert_eq!(
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            unsafe { bif_is_integer(&[ErlTerm::small_int(1)]) }.as_atom_id(),
            Some(ATOM_TRUE)
        );
        assert_eq!(
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            unsafe { bif_is_integer(&[ErlTerm::atom(0)]) }.as_atom_id(),
            Some(ATOM_FALSE)
        );
        assert_eq!(
            bif_is_atom(&[ErlTerm::atom(0)]).as_atom_id(),
            Some(ATOM_TRUE)
        );
        assert_eq!(
            bif_is_atom(&[ErlTerm::small_int(1)]).as_atom_id(),
            Some(ATOM_FALSE)
        );
        assert_eq!(
            bif_is_list(&[ErlTerm::nil()]).as_atom_id(),
            Some(ATOM_TRUE)
        );
        assert_eq!(
            bif_is_list(&[ErlTerm::small_int(1)]).as_atom_id(),
            Some(ATOM_FALSE)
        );
        assert_eq!(
            bif_is_pid(&[ErlTerm::pid(crate::process::ProcessId::new(1))]).as_atom_id(),
            Some(ATOM_TRUE)
        );
        assert_eq!(
            bif_is_pid(&[ErlTerm::atom(0)]).as_atom_id(),
            Some(ATOM_FALSE)
        );
    }

    #[test]
    fn bif_is_tuple_check() {
        let mut heap = ProcessHeap::new();
        let tuple = heap.alloc_tuple(&[ErlTerm::small_int(1)]);
        assert_eq!(unsafe { bif_is_tuple(&[tuple]) }.as_atom_id(), Some(ATOM_TRUE));
        assert_eq!(
            unsafe { bif_is_tuple(&[ErlTerm::nil()]) }.as_atom_id(),
            Some(ATOM_FALSE)
        );
    }

    #[test]
    fn bif_element_basic() {
        let mut heap = ProcessHeap::new();
        let tuple = heap.alloc_tuple(&[
            ErlTerm::small_int(10),
            ErlTerm::small_int(20),
            ErlTerm::small_int(30),
        ]);
        // Erlang is 1-indexed.
        assert_eq!(
            unsafe { bif_element(&mut heap, &[ErlTerm::small_int(1), tuple]) }
                .unwrap()
                .as_small_int(),
            Some(10)
        );
        assert_eq!(
            unsafe { bif_element(&mut heap, &[ErlTerm::small_int(3), tuple]) }
                .unwrap()
                .as_small_int(),
            Some(30)
        );
    }

    #[test]
    fn bif_element_out_of_range() {
        let mut heap = ProcessHeap::new();
        let tuple = heap.alloc_tuple(&[ErlTerm::small_int(1)]);
        assert!(unsafe { bif_element(&mut heap, &[ErlTerm::small_int(0), tuple]) }.is_err());
        assert!(unsafe { bif_element(&mut heap, &[ErlTerm::small_int(2), tuple]) }.is_err());
    }

    #[test]
    fn bif_tuple_size_basic() {
        let mut heap = ProcessHeap::new();
        let tuple = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        assert_eq!(
            unsafe { bif_tuple_size(&mut heap, &[tuple]) }.unwrap().as_small_int(),
            Some(2)
        );
    }

    #[test]
    fn bif_tuple_size_not_tuple() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_tuple_size(&mut heap, &[ErlTerm::nil()]) }.is_err());
    }

    #[test]
    fn bif_is_binary_check() {
        let mut heap = ProcessHeap::new();
        let bin = heap.alloc_binary(&[1, 2, 3]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert_eq!(unsafe { bif_is_binary(&[bin]) }.as_atom_id(), Some(ATOM_TRUE));
        assert_eq!(
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            unsafe { bif_is_binary(&[ErlTerm::nil()]) }.as_atom_id(),
            Some(ATOM_FALSE)
        );
        assert_eq!(
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            unsafe { bif_is_binary(&[ErlTerm::small_int(42)]) }.as_atom_id(),
            Some(ATOM_FALSE)
        );
    }

    #[test]
    fn bif_byte_size_check() {
        let mut heap = ProcessHeap::new();
        let bin = heap.alloc_binary(&[1, 2, 3, 4, 5]);
        assert_eq!(
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            unsafe { bif_byte_size(&mut heap, &[bin]) }.unwrap().as_small_int(),
            Some(5)
        );
        // Empty binary
        let empty = heap.alloc_binary(&[]);
        assert_eq!(
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            unsafe { bif_byte_size(&mut heap, &[empty]) }.unwrap().as_small_int(),
            Some(0)
        );
        // Not a binary
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { bif_byte_size(&mut heap, &[ErlTerm::nil()]) }.is_err());
    }

    #[test]
    fn format_term_binary() {
        let mut heap = ProcessHeap::new();
        let bin = heap.alloc_binary(&[1, 2, 3]);
        assert_eq!(unsafe { format_term(bin) }, "<<1,2,3>>");

        let empty_bin = heap.alloc_binary(&[]);
        assert_eq!(unsafe { format_term(empty_bin) }, "<<>>");
    }

    #[test]
    fn format_term_sub_byte_bitstring() {
        let mut heap = ProcessHeap::new();
        // Create a binary and patch header to 3 bits (<<5:3>>)
        // 5 in 3 bits = 101, stored as high bits of byte: 0b10100000 = 0xA0
        let bin = heap.alloc_binary(&[0xA0]);
        unsafe {
            let ptr = bin.as_boxed_ptr().unwrap() as *mut u64;
            use crate::term::BoxedHeader;
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::HeapBinary, 3).0;
        }
        assert_eq!(unsafe { format_term(bin) }, "<<5:3>>");
    }

    #[test]
    fn format_binary_bytes_helper() {
        // Byte-aligned
        assert_eq!(format_binary_bytes(&[1, 2, 3], 24), "<<1,2,3>>");
        assert_eq!(format_binary_bytes(&[], 0), "<<>>");

        // Sub-byte: 5 in 3 bits → high bits 0xA0 = 0b10100000
        assert_eq!(format_binary_bytes(&[0xA0], 3), "<<5:3>>");

        // Mixed: 1 full byte + 4 bits
        // 0xFF followed by 0b11000000 (top 4 bits = 12)
        assert_eq!(format_binary_bytes(&[0xFF, 0xC0], 12), "<<255,12:4>>");
    }

    #[test]
    fn bif_is_map_check() {
        let mut heap = ProcessHeap::new();
        let map = heap.alloc_map(MapPairs::from_flat(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]).expect("complete key/value pairs"));
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert_eq!(unsafe { bif_is_map(&[map]) }.as_atom_id(), Some(ATOM_TRUE));
        assert_eq!(
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            unsafe { bif_is_map(&[ErlTerm::nil()]) }.as_atom_id(),
            Some(ATOM_FALSE)
        );
        assert_eq!(
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            unsafe { bif_is_map(&[ErlTerm::small_int(42)]) }.as_atom_id(),
            Some(ATOM_FALSE)
        );
    }

    #[test]
    fn bif_map_size_check() {
        let mut heap = ProcessHeap::new();
        let map = heap.alloc_map(MapPairs::from_flat(&[
            ErlTerm::small_int(1),
            ErlTerm::small_int(10),
            ErlTerm::small_int(2),
            ErlTerm::small_int(20),
        ]).expect("complete key/value pairs"));
        assert_eq!(
            unsafe { bif_map_size(&mut heap, &[map]) }.unwrap().as_small_int(),
            Some(2)
        );

        // Empty map
        let empty_map = heap.alloc_map(MapPairs::new(&[]));
        assert_eq!(
            unsafe { bif_map_size(&mut heap, &[empty_map]) }
                .unwrap()
                .as_small_int(),
            Some(0)
        );

        // Not a map
        assert!(unsafe { bif_map_size(&mut heap, &[ErlTerm::nil()]) }.is_err());
    }

    #[test]
    fn format_term_map() {
        let mut heap = ProcessHeap::new();
        let map = heap.alloc_map(MapPairs::from_flat(&[
            ErlTerm::small_int(1),
            ErlTerm::small_int(10),
        ]).expect("complete key/value pairs"));
        assert_eq!(unsafe { format_term(map) }, "#{1 => 10}");

        let empty_map = heap.alloc_map(MapPairs::new(&[]));
        assert_eq!(unsafe { format_term(empty_map) }, "#{}");
    }

    #[test]
    fn format_term_reference() {
        let mut heap = ProcessHeap::new();
        use crate::term::REFERENCE_TOTAL_WORDS;
        let ptr = heap.alloc_words(REFERENCE_TOTAL_WORDS);
        unsafe {
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Reference, 0).0;
            *ptr.add(1) = 7_u64;
        }
        let ref_term = ErlTerm::boxed(ptr as *const u64);
        assert_eq!(unsafe { format_term(ref_term) }, "#Ref<0.0.0.7>");
    }

    #[test]
    fn bif_abs_integer() {
        let mut heap = ProcessHeap::new();
        // Positive stays positive
        assert_eq!(
            unsafe { bif_abs(&mut heap, &[ErlTerm::small_int(42)]) }
                .unwrap()
                .as_small_int(),
            Some(42)
        );
        // Negative becomes positive
        assert_eq!(
            unsafe { bif_abs(&mut heap, &[ErlTerm::small_int(-7)]) }
                .unwrap()
                .as_small_int(),
            Some(7)
        );
        // Zero stays zero
        assert_eq!(
            unsafe { bif_abs(&mut heap, &[ErlTerm::small_int(0)]) }
                .unwrap()
                .as_small_int(),
            Some(0)
        );
    }

    #[test]
    fn bif_abs_float() {
        let mut heap = ProcessHeap::new();
        let neg_float = heap.alloc_float(-3.25);
        let result = unsafe { bif_abs(&mut heap, &[neg_float]) }.unwrap();
        assert!(result.is_boxed());
        unsafe {
            let ptr = result.as_boxed_ptr().unwrap();
            let bits = *ptr.add(1);
            assert_eq!(f64::from_bits(bits), 3.25);
        }

        let pos_float = heap.alloc_float(2.71);
        let result = unsafe { bif_abs(&mut heap, &[pos_float]) }.unwrap();
        unsafe {
            let ptr = result.as_boxed_ptr().unwrap();
            let bits = *ptr.add(1);
            assert_eq!(f64::from_bits(bits), 2.71);
        }
    }

    #[test]
    fn bif_abs_badarg() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_abs(&mut heap, &[ErlTerm::atom(0)]) }.is_err());
        assert!(unsafe { bif_abs(&mut heap, &[ErlTerm::nil()]) }.is_err());
    }

    // ── round/1 ──

    #[test]
    fn bif_round_integer() {
        let mut heap = ProcessHeap::new();
        assert_eq!(
            unsafe { bif_round(&mut heap, &[ErlTerm::small_int(42)]) }
                .unwrap()
                .as_small_int(),
            Some(42)
        );
        assert_eq!(
            unsafe { bif_round(&mut heap, &[ErlTerm::small_int(-7)]) }
                .unwrap()
                .as_small_int(),
            Some(-7)
        );
    }

    #[test]
    fn bif_round_float() {
        let mut heap = ProcessHeap::new();
        let f = heap.alloc_float(3.7);
        let result = unsafe { bif_round(&mut heap, &[f]) }.unwrap();
        assert_eq!(result.as_small_int(), Some(4));

        let f2 = heap.alloc_float(-2.3);
        let result2 = unsafe { bif_round(&mut heap, &[f2]) }.unwrap();
        assert_eq!(result2.as_small_int(), Some(-2));

        // Round half away from zero
        let f3 = heap.alloc_float(2.5);
        let result3 = unsafe { bif_round(&mut heap, &[f3]) }.unwrap();
        assert_eq!(result3.as_small_int(), Some(3));
    }

    #[test]
    fn bif_round_badarg() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_round(&mut heap, &[ErlTerm::atom(0)]) }.is_err());
        assert!(unsafe { bif_round(&mut heap, &[ErlTerm::nil()]) }.is_err());
    }

    // ── trunc/1 ──

    #[test]
    fn bif_trunc_integer() {
        let mut heap = ProcessHeap::new();
        assert_eq!(
            unsafe { bif_trunc(&mut heap, &[ErlTerm::small_int(42)]) }
                .unwrap()
                .as_small_int(),
            Some(42)
        );
    }

    #[test]
    fn bif_trunc_float() {
        let mut heap = ProcessHeap::new();
        let f = heap.alloc_float(3.7);
        let result = unsafe { bif_trunc(&mut heap, &[f]) }.unwrap();
        assert_eq!(result.as_small_int(), Some(3));

        let f2 = heap.alloc_float(-2.9);
        let result2 = unsafe { bif_trunc(&mut heap, &[f2]) }.unwrap();
        assert_eq!(result2.as_small_int(), Some(-2));
    }

    #[test]
    fn bif_trunc_badarg() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_trunc(&mut heap, &[ErlTerm::atom(0)]) }.is_err());
        assert!(unsafe { bif_trunc(&mut heap, &[ErlTerm::nil()]) }.is_err());
    }

    // ── float/1 ──

    #[test]
    fn bif_float_integer() {
        let mut heap = ProcessHeap::new();
        let result = unsafe { bif_float(&mut heap, &[ErlTerm::small_int(42)]) }.unwrap();
        assert!(result.is_boxed());
        unsafe {
            let ptr = result.as_boxed_ptr().unwrap();
            let bits = *ptr.add(1);
            assert_eq!(f64::from_bits(bits), 42.0);
        }
    }

    #[test]
    fn bif_float_float() {
        let mut heap = ProcessHeap::new();
        let f = heap.alloc_float(3.25);
        let result = unsafe { bif_float(&mut heap, &[f]) }.unwrap();
        // Float passes through — same value
        unsafe {
            let ptr = result.as_boxed_ptr().unwrap();
            let bits = *ptr.add(1);
            assert_eq!(f64::from_bits(bits), 3.25);
        }
    }

    #[test]
    fn bif_float_badarg() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_float(&mut heap, &[ErlTerm::atom(0)]) }.is_err());
        assert!(unsafe { bif_float(&mut heap, &[ErlTerm::nil()]) }.is_err());
    }

    #[test]
    fn bif_max_integers() {
        let mut heap = ProcessHeap::new();
        assert_eq!(
            unsafe { bif_max(&mut heap, &[ErlTerm::small_int(3), ErlTerm::small_int(7)]) }
                .unwrap()
                .as_small_int(),
            Some(7)
        );
        assert_eq!(
            unsafe { bif_max(&mut heap, &[ErlTerm::small_int(10), ErlTerm::small_int(2)]) }
                .unwrap()
                .as_small_int(),
            Some(10)
        );
        // Equal values
        assert_eq!(
            unsafe { bif_max(&mut heap, &[ErlTerm::small_int(5), ErlTerm::small_int(5)]) }
                .unwrap()
                .as_small_int(),
            Some(5)
        );
        // Negative values
        assert_eq!(
            unsafe { bif_max(&mut heap, &[ErlTerm::small_int(-3), ErlTerm::small_int(-7)]) }
                .unwrap()
                .as_small_int(),
            Some(-3)
        );
    }

    #[test]
    fn bif_min_integers() {
        let mut heap = ProcessHeap::new();
        assert_eq!(
            unsafe { bif_min(&mut heap, &[ErlTerm::small_int(3), ErlTerm::small_int(7)]) }
                .unwrap()
                .as_small_int(),
            Some(3)
        );
        assert_eq!(
            unsafe { bif_min(&mut heap, &[ErlTerm::small_int(10), ErlTerm::small_int(2)]) }
                .unwrap()
                .as_small_int(),
            Some(2)
        );
        // Negative values
        assert_eq!(
            unsafe { bif_min(&mut heap, &[ErlTerm::small_int(-3), ErlTerm::small_int(-7)]) }
                .unwrap()
                .as_small_int(),
            Some(-7)
        );
    }

    #[test]
    fn bif_max_min_floats() {
        let mut heap = ProcessHeap::new();
        let f1 = heap.alloc_float(1.5);
        let f2 = heap.alloc_float(2.5);
        let max_result = unsafe { bif_max(&mut heap, &[f1, f2]) }.unwrap();
        unsafe {
            let ptr = max_result.as_boxed_ptr().unwrap();
            let bits = *ptr.add(1);
            assert_eq!(f64::from_bits(bits), 2.5);
        }
        let min_result = unsafe { bif_min(&mut heap, &[f1, f2]) }.unwrap();
        unsafe {
            let ptr = min_result.as_boxed_ptr().unwrap();
            let bits = *ptr.add(1);
            assert_eq!(f64::from_bits(bits), 1.5);
        }
    }

    /// max/2 and min/2 return one of their arguments unchanged, the first
    /// when they compare equal, as on BEAM. They used to turn the integer of a
    /// mixed pair into a new float, so max(3, 2.5) returned 3.0.
    #[test]
    fn bif_max_min_mixed_int_float() {
        let mut heap = ProcessHeap::new();
        let three = ErlTerm::small_int(3);
        let two_and_half = heap.alloc_float(2.5);
        let three_float = heap.alloc_float(3.0);
        let pick = |heap: &mut ProcessHeap, max: bool, a: ErlTerm, b: ErlTerm| {
            let args = [a, b];
            unsafe { if max { bif_max(heap, &args) } else { bif_min(heap, &args) } }.unwrap()
        };
        assert_eq!(pick(&mut heap, true, three, two_and_half), three);
        assert_eq!(pick(&mut heap, true, two_and_half, three), three);
        assert_eq!(pick(&mut heap, false, three, two_and_half), two_and_half);
        assert_eq!(pick(&mut heap, true, three, three_float), three);
        assert_eq!(pick(&mut heap, true, three_float, three), three_float);
        assert_eq!(pick(&mut heap, false, three, three_float), three);
        assert_eq!(pick(&mut heap, false, three_float, three), three_float);
    }

    // -----------------------------------------------------------------------
    // Conversion BIF tests
    // -----------------------------------------------------------------------

    #[test]
    fn bif_atom_to_list_ok() {
        let mut heap = ProcessHeap::new();
        let table = global_atom_table();
        let id = table.intern("hello");
        let result = bif_atom_to_list(&mut heap, &[ErlTerm::atom(id)]).unwrap();
        assert_eq!(unsafe { term_to_string(result) }, Ok("hello".to_string()));
    }

    #[test]
    fn bif_atom_to_list_badarg() {
        let mut heap = ProcessHeap::new();
        assert!(bif_atom_to_list(&mut heap, &[ErlTerm::small_int(42)]).is_err());
    }

    #[test]
    fn bif_list_to_atom_ok() {
        let mut heap = ProcessHeap::new();
        // "hi" = [104, 105]
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(104),
            ErlTerm::small_int(105),
        ]);
        let result = unsafe { bif_list_to_atom(&mut heap, &[list]) }.unwrap();
        assert!(result.is_atom());
        let table = global_atom_table();
        let name = table.resolve(result.as_atom_id().unwrap()).unwrap();
        assert_eq!(name, "hi");
    }

    #[test]
    fn bif_list_to_atom_badarg() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_list_to_atom(&mut heap, &[ErlTerm::small_int(1)]) }.is_err());
    }

    #[test]
    fn bif_integer_to_list_positive() {
        let mut heap = ProcessHeap::new();
        let result = unsafe { bif_integer_to_list(&mut heap, &[ErlTerm::small_int(42)]) }.unwrap();
        assert_eq!(unsafe { term_to_string(result) }, Ok("42".to_string()));
    }

    #[test]
    fn bif_integer_to_list_negative() {
        let mut heap = ProcessHeap::new();
        let result = unsafe { bif_integer_to_list(&mut heap, &[ErlTerm::small_int(-7)]) }.unwrap();
        assert_eq!(unsafe { term_to_string(result) }, Ok("-7".to_string()));
    }

    #[test]
    fn bif_integer_to_list_badarg() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_integer_to_list(&mut heap, &[ErlTerm::atom(0)]) }.is_err());
    }

    #[test]
    fn bif_list_to_integer_ok() {
        let mut heap = ProcessHeap::new();
        // "123" = [49, 50, 51]
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(49),
            ErlTerm::small_int(50),
            ErlTerm::small_int(51),
        ]);
        let result = unsafe { bif_list_to_integer(&mut heap, &[list]) }.unwrap();
        assert_eq!(result.as_small_int(), Some(123));
    }

    #[test]
    fn bif_list_to_integer_negative() {
        let mut heap = ProcessHeap::new();
        // "-5" = [45, 53]
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(45),
            ErlTerm::small_int(53),
        ]);
        let result = unsafe { bif_list_to_integer(&mut heap, &[list]) }.unwrap();
        assert_eq!(result.as_small_int(), Some(-5));
    }

    #[test]
    fn bif_list_to_integer_badarg() {
        let mut heap = ProcessHeap::new();
        // "abc" is not a valid integer
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(97),
            ErlTerm::small_int(98),
            ErlTerm::small_int(99),
        ]);
        assert!(unsafe { bif_list_to_integer(&mut heap, &[list]) }.is_err());
    }

    #[test]
    fn bif_list_to_tuple_ok() {
        let mut heap = ProcessHeap::new();
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(1),
            ErlTerm::small_int(2),
            ErlTerm::small_int(3),
        ]);
        let result = unsafe { bif_list_to_tuple(&mut heap, &[list]) }.unwrap();
        assert_eq!(unsafe { format_term(result) }, "{1,2,3}");
    }

    #[test]
    fn bif_list_to_tuple_empty() {
        let mut heap = ProcessHeap::new();
        let result = unsafe { bif_list_to_tuple(&mut heap, &[ErlTerm::nil()]) }.unwrap();
        assert_eq!(unsafe { format_term(result) }, "{}");
    }

    #[test]
    fn bif_tuple_to_list_ok() {
        let mut heap = ProcessHeap::new();
        let tuple = heap.alloc_tuple(&[
            ErlTerm::small_int(10),
            ErlTerm::small_int(20),
        ]);
        let result = unsafe { bif_tuple_to_list(&mut heap, &[tuple]) }.unwrap();
        assert_eq!(unsafe { format_term(result) }, "[10,20]");
    }

    #[test]
    fn bif_tuple_to_list_badarg() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_tuple_to_list(&mut heap, &[ErlTerm::nil()]) }.is_err());
    }

    #[test]
    fn bif_setelement_ok() {
        let mut heap = ProcessHeap::new();
        let tuple = heap.alloc_tuple(&[
            ErlTerm::small_int(1),
            ErlTerm::small_int(2),
            ErlTerm::small_int(3),
        ]);
        let result = unsafe { bif_setelement(
            &mut heap,
            &[ErlTerm::small_int(2), tuple, ErlTerm::small_int(99)],
        ) }
        .unwrap();
        assert_eq!(unsafe { format_term(result) }, "{1,99,3}");
    }

    #[test]
    fn bif_setelement_badarg() {
        let mut heap = ProcessHeap::new();
        let tuple = heap.alloc_tuple(&[ErlTerm::small_int(1)]);
        // Index 0 is invalid (1-indexed)
        assert!(unsafe { bif_setelement(
            &mut heap,
            &[ErlTerm::small_int(0), tuple, ErlTerm::small_int(99)]
        ) }
        .is_err());
        // Index 2 out of range for 1-element tuple
        assert!(unsafe { bif_setelement(
            &mut heap,
            &[ErlTerm::small_int(2), tuple, ErlTerm::small_int(99)]
        ) }
        .is_err());
    }

    #[test]
    fn bif_is_float_check() {
        let mut heap = ProcessHeap::new();
        let f = heap.alloc_float(3.25);
        assert_eq!(unsafe { bif_is_float(&[f]) }.as_atom_id(), Some(ATOM_TRUE));
        assert_eq!(
            unsafe { bif_is_float(&[ErlTerm::small_int(1)]) }.as_atom_id(),
            Some(ATOM_FALSE)
        );
        assert_eq!(
            unsafe { bif_is_float(&[ErlTerm::nil()]) }.as_atom_id(),
            Some(ATOM_FALSE)
        );
    }

    #[test]
    fn bif_is_number_check() {
        let mut heap = ProcessHeap::new();
        let f = heap.alloc_float(1.0);
        assert_eq!(
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            unsafe { bif_is_number(&[ErlTerm::small_int(42)]) }.as_atom_id(),
            Some(ATOM_TRUE)
        );
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert_eq!(unsafe { bif_is_number(&[f]) }.as_atom_id(), Some(ATOM_TRUE));
        assert_eq!(
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            unsafe { bif_is_number(&[ErlTerm::atom(0)]) }.as_atom_id(),
            Some(ATOM_FALSE)
        );
    }

    #[test]
    fn bif_is_boolean_check() {
        assert_eq!(
            bif_is_boolean(&[ErlTerm::atom(ATOM_TRUE)]).as_atom_id(),
            Some(ATOM_TRUE)
        );
        assert_eq!(
            bif_is_boolean(&[ErlTerm::atom(ATOM_FALSE)]).as_atom_id(),
            Some(ATOM_TRUE)
        );
        assert_eq!(
            bif_is_boolean(&[ErlTerm::atom(0)]).as_atom_id(), // atom 'ok'
            Some(ATOM_FALSE)
        );
        assert_eq!(
            bif_is_boolean(&[ErlTerm::small_int(1)]).as_atom_id(),
            Some(ATOM_FALSE)
        );
    }

    #[test]
    fn bif_float_to_list_ok() {
        let mut heap = ProcessHeap::new();
        let f = heap.alloc_float(3.25);
        let result = unsafe { bif_float_to_list(&mut heap, &[f]) }.unwrap();
        let s = unsafe { term_to_string(result) }.unwrap();
        // Should be scientific notation
        assert!(s.contains('e') || s.contains('E'), "expected scientific notation: {}", s);
    }

    #[test]
    fn bif_float_to_list_badarg() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_float_to_list(&mut heap, &[ErlTerm::small_int(42)]) }.is_err());
    }

    // ── ceil/1 ──

    #[test]
    fn bif_ceil_integer() {
        let mut heap = ProcessHeap::new();
        assert_eq!(
            unsafe { bif_ceil(&mut heap, &[ErlTerm::small_int(42)]) }
                .unwrap()
                .as_small_int(),
            Some(42)
        );
        assert_eq!(
            unsafe { bif_ceil(&mut heap, &[ErlTerm::small_int(-7)]) }
                .unwrap()
                .as_small_int(),
            Some(-7)
        );
    }

    #[test]
    fn bif_ceil_float() {
        let mut heap = ProcessHeap::new();
        let f = heap.alloc_float(3.2);
        assert_eq!(unsafe { bif_ceil(&mut heap, &[f]) }.unwrap().as_small_int(), Some(4));

        let f2 = heap.alloc_float(-2.9);
        assert_eq!(unsafe { bif_ceil(&mut heap, &[f2]) }.unwrap().as_small_int(), Some(-2));

        let f3 = heap.alloc_float(5.0);
        assert_eq!(unsafe { bif_ceil(&mut heap, &[f3]) }.unwrap().as_small_int(), Some(5));
    }

    #[test]
    fn bif_ceil_badarg() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_ceil(&mut heap, &[ErlTerm::atom(0)]) }.is_err());
        assert!(unsafe { bif_ceil(&mut heap, &[ErlTerm::nil()]) }.is_err());
    }

    // ── floor/1 ──

    #[test]
    fn bif_floor_integer() {
        let mut heap = ProcessHeap::new();
        assert_eq!(
            unsafe { bif_floor(&mut heap, &[ErlTerm::small_int(42)]) }
                .unwrap()
                .as_small_int(),
            Some(42)
        );
        assert_eq!(
            unsafe { bif_floor(&mut heap, &[ErlTerm::small_int(-7)]) }
                .unwrap()
                .as_small_int(),
            Some(-7)
        );
    }

    #[test]
    fn bif_floor_float() {
        let mut heap = ProcessHeap::new();
        let f = heap.alloc_float(3.7);
        assert_eq!(unsafe { bif_floor(&mut heap, &[f]) }.unwrap().as_small_int(), Some(3));

        let f2 = heap.alloc_float(-2.1);
        assert_eq!(unsafe { bif_floor(&mut heap, &[f2]) }.unwrap().as_small_int(), Some(-3));

        let f3 = heap.alloc_float(5.0);
        assert_eq!(unsafe { bif_floor(&mut heap, &[f3]) }.unwrap().as_small_int(), Some(5));
    }

    #[test]
    fn bif_floor_badarg() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_floor(&mut heap, &[ErlTerm::atom(0)]) }.is_err());
        assert!(unsafe { bif_floor(&mut heap, &[ErlTerm::nil()]) }.is_err());
    }

    // ── size/1 ──

    #[test]
    fn bif_size_tuple() {
        let mut heap = ProcessHeap::new();
        let tuple = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert_eq!(unsafe { bif_size(&mut heap, &[tuple]) }.unwrap().as_small_int(), Some(2));
    }

    #[test]
    fn bif_size_binary() {
        let mut heap = ProcessHeap::new();
        let bin = heap.alloc_binary(&[1, 2, 3, 4, 5]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert_eq!(unsafe { bif_size(&mut heap, &[bin]) }.unwrap().as_small_int(), Some(5));
    }

    #[test]
    fn bif_size_badarg() {
        let mut heap = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(bif_size(&mut heap, &[ErlTerm::atom(0)]).is_err());
            assert!(bif_size(&mut heap, &[ErlTerm::nil()]).is_err());
            assert!(bif_size(&mut heap, &[ErlTerm::small_int(42)]).is_err());
        }
    }

    // ── phash2/1,2 ──

    #[test]
    fn bif_phash2_deterministic() {
        let mut heap = ProcessHeap::new();
        let term = ErlTerm::small_int(42);
        let h1 = unsafe { bif_phash2(&mut heap, &[term]) }.unwrap();
        let h2 = unsafe { bif_phash2(&mut heap, &[term]) }.unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn bif_phash2_range_check() {
        let mut heap = ProcessHeap::new();
        let term = ErlTerm::small_int(42);
        let h = unsafe { bif_phash2(&mut heap, &[term]) }.unwrap().as_small_int().unwrap();
        // Must be in 0..(2^27 - 1)
        assert!((0..(1 << 27)).contains(&h));
    }

    #[test]
    fn bif_phash2_atom() {
        let mut heap = ProcessHeap::new();
        let a = ErlTerm::atom(0);
        let h = unsafe { bif_phash2(&mut heap, &[a]) }.unwrap().as_small_int().unwrap();
        assert!((0..(1 << 27)).contains(&h));
    }

    #[test]
    fn bif_phash2_range_form() {
        let mut heap = ProcessHeap::new();
        let term = ErlTerm::small_int(100);
        let range = ErlTerm::small_int(10);
        let h = unsafe { bif_phash2_range(&mut heap, &[term, range]) }.unwrap().as_small_int().unwrap();
        assert!((0..10).contains(&h));
    }

    #[test]
    fn bif_phash2_range_badarg() {
        let mut heap = ProcessHeap::new();
        let term = ErlTerm::small_int(42);
        // Range 0 is invalid
        assert!(unsafe { bif_phash2_range(&mut heap, &[term, ErlTerm::small_int(0)]) }.is_err());
        // Negative range is invalid
        assert!(unsafe { bif_phash2_range(&mut heap, &[term, ErlTerm::small_int(-1)]) }.is_err());
        // Non-integer range
        assert!(unsafe { bif_phash2_range(&mut heap, &[term, ErlTerm::atom(0)]) }.is_err());
    }

    #[test]
    fn bif_phash2_compound() {
        let mut heap = ProcessHeap::new();
        let tuple = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        let h = unsafe { bif_phash2(&mut heap, &[tuple]) }.unwrap().as_small_int().unwrap();
        assert!((0..(1 << 27)).contains(&h));

        let list = heap.alloc_list_from_slice(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        let h2 = unsafe { bif_phash2(&mut heap, &[list]) }.unwrap().as_small_int().unwrap();
        assert!((0..(1 << 27)).contains(&h2));
    }

    #[test]
    fn phash2_refc_binary_matches_heap_binary() {
        use std::sync::Arc;
        let mut heap = ProcessHeap::new();
        // Content: bytes 0..10
        let content: Vec<u8> = (0u8..10).collect();

        // Heap binary (inline, ≤64 bytes)
        let heap_bin = heap.alloc_binary(&content);

        // Refc binary with the same first 10 bytes via Arc + offset
        let backing: Vec<u8> = (0u8..100).collect(); // >64 bytes → off-heap
        let arc = Arc::new(BinaryData::new(backing));
        let refc_bin = heap.alloc_refc_binary_from_arc(arc, 0, 10);

        let h1 = unsafe { bif_phash2(&mut heap, &[heap_bin]) }.unwrap().as_small_int().unwrap();
        let h2 = unsafe { bif_phash2(&mut heap, &[refc_bin]) }.unwrap().as_small_int().unwrap();
        assert_eq!(h1, h2, "same-content heap and refc binaries must hash identically");
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term round-trip tests
    // -----------------------------------------------------------------------

    #[test]
    fn etf_roundtrip_small_int() {
        let mut heap = ProcessHeap::new();
        let term = ErlTerm::small_int(42);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_small_int(), Some(42));
    }

    #[test]
    fn etf_roundtrip_negative_int() {
        let mut heap = ProcessHeap::new();
        let term = ErlTerm::small_int(-1000);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_small_int(), Some(-1000));
    }

    #[test]
    fn etf_roundtrip_large_int() {
        let mut heap = ProcessHeap::new();
        // Value that exceeds i32 but fits small int
        let val = 3_000_000_000_i64;
        let term = ErlTerm::small_int(val);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_small_int(), Some(val));
    }

    #[test]
    fn etf_roundtrip_atom() {
        let mut heap = ProcessHeap::new();
        let table = crate::atom_table::global_atom_table();
        let id = table.intern("hello");
        let term = ErlTerm::atom(id);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_atom_id(), Some(id));
    }

    #[test]
    fn etf_roundtrip_nil() {
        let mut heap = ProcessHeap::new();
        let bin = unsafe { bif_term_to_binary(&mut heap, &[ErlTerm::nil()]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert!(decoded.is_nil());
    }

    #[test]
    fn etf_roundtrip_float() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_float(3.25);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert!(decoded.is_boxed());
        unsafe {
            let ptr = decoded.as_boxed_ptr().unwrap();
            let val = f64::from_bits(*ptr.add(1));
            assert_eq!(val, 3.25);
        }
    }

    #[test]
    fn etf_roundtrip_tuple() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { decoded.is_tuple() });
        unsafe {
            let ptr = decoded.as_boxed_ptr().unwrap();
            let hdr = BoxedHeader(*ptr);
            assert_eq!(hdr.arity(), 2);
            assert_eq!(ErlTerm(*ptr.add(1)).as_small_int(), Some(1));
            assert_eq!(ErlTerm(*ptr.add(2)).as_small_int(), Some(2));
        }
    }

    #[test]
    fn etf_roundtrip_list() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_list_from_slice(&[ErlTerm::small_int(10), ErlTerm::small_int(20)]);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert!(decoded.is_cons());
        unsafe {
            let cell = &*decoded.as_cons_ptr().unwrap();
            assert_eq!(cell.head.as_small_int(), Some(10));
            let cell2 = &*cell.tail.as_cons_ptr().unwrap();
            assert_eq!(cell2.head.as_small_int(), Some(20));
            assert!(cell2.tail.is_nil());
        }
    }

    #[test]
    fn etf_roundtrip_binary() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_binary(b"hello world");
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(decoded.is_binary());
            assert_eq!(decoded.binary_size(), Some(11));
        }
        unsafe {
            let bytes = decoded.binary_bytes_any().unwrap();
            assert_eq!(bytes, b"hello world");
        }
    }

    #[test]
    fn etf_roundtrip_map() {
        let mut heap = ProcessHeap::new();
        let table = crate::atom_table::global_atom_table();
        let k = ErlTerm::atom(table.intern("key"));
        let v = ErlTerm::small_int(99);
        let term = heap.alloc_map(MapPairs::from_flat(&[k, v]).expect("complete key/value pairs"));
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { decoded.is_map() });
        unsafe {
            let ptr = decoded.as_boxed_ptr().unwrap();
            let hdr = BoxedHeader(*ptr);
            assert_eq!(hdr.arity(), 1); // 1 pair
            let dk = ErlTerm(*ptr.add(1));
            let dv = ErlTerm(*ptr.add(2));
            assert_eq!(dk.as_atom_id(), k.as_atom_id());
            assert_eq!(dv.as_small_int(), Some(99));
        }
    }

    #[test]
    fn etf_roundtrip_nested() {
        let mut heap = ProcessHeap::new();
        // {[1, 2], <<"abc">>}
        let list = heap.alloc_list_from_slice(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        let binary = heap.alloc_binary(b"abc");
        let tuple = heap.alloc_tuple(&[list, binary]);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[tuple]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { decoded.is_tuple() });
        unsafe {
            let ptr = decoded.as_boxed_ptr().unwrap();
            let hdr = BoxedHeader(*ptr);
            assert_eq!(hdr.arity(), 2);
            // First element is a list
            let elem1 = ErlTerm(*ptr.add(1));
            assert!(elem1.is_cons());
            // Second element is a binary
            let elem2 = ErlTerm(*ptr.add(2));
            assert!(elem2.is_binary());
            assert_eq!(elem2.binary_bytes_any().unwrap(), b"abc");
        }
    }

    #[test]
    fn etf_roundtrip_pid() {
        let mut heap = ProcessHeap::new();
        let term = ErlTerm::pid(crate::process::ProcessId::new(42));
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert!(decoded.is_pid());
        assert_eq!(decoded.as_pid_id(), Some(crate::process::ProcessId::new(42)));
    }

    #[test]
    fn etf_binary_to_term_badarg() {
        let mut heap = ProcessHeap::new();
        // Not a binary
        assert!(unsafe { bif_binary_to_term(&mut heap, &[ErlTerm::small_int(1)]) }.is_err());
        // Empty binary
        let empty = heap.alloc_binary(b"");
        assert!(unsafe { bif_binary_to_term(&mut heap, &[empty]) }.is_err());
        // Wrong version byte
        let bad = heap.alloc_binary(&[99, 97, 42]);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[bad]) }.is_err());
    }

    #[test]
    fn etf_roundtrip_bigint() {
        let mut heap = ProcessHeap::new();
        use num_bigint::BigInt;
        // Large positive bigint
        let big = BigInt::from(1_000_000_000_000_000_i64);
        let term = crate::bigint::bigint_to_term(&mut heap, big.clone());
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        let decoded_big = unsafe { decoded.borrow(&heap) }.to_bigint().unwrap();
        assert_eq!(decoded_big, big);

        // Large negative bigint
        let neg_big = BigInt::from(-1_000_000_000_000_000_i64);
        let term = crate::bigint::bigint_to_term(&mut heap, neg_big.clone());
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        let decoded_big = unsafe { decoded.borrow(&heap) }.to_bigint().unwrap();
        assert_eq!(decoded_big, neg_big);
    }

    #[test]
    fn etf_string_ext_optimization() {
        let mut heap = ProcessHeap::new();
        // A list of bytes [65, 66, 67] should use STRING_EXT encoding
        let term = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(65),
            ErlTerm::small_int(66),
            ErlTerm::small_int(67),
        ]);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        // Verify it uses STRING_EXT (tag 107)
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[0], 131); // version
            assert_eq!(bytes[1], 107); // STRING_EXT
        }
        // Verify round-trip
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert!(decoded.is_cons());
        unsafe {
            let cell = &*decoded.as_cons_ptr().unwrap();
            assert_eq!(cell.head.as_small_int(), Some(65));
        }
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — integer boundary tests
    // -----------------------------------------------------------------------

    #[test]
    fn etf_roundtrip_zero() {
        let mut heap = ProcessHeap::new();
        let bin = unsafe { bif_term_to_binary(&mut heap, &[ErlTerm::small_int(0)]) }.unwrap();
        // 0 fits in SMALL_INTEGER_EXT
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[0], 131);
            assert_eq!(bytes[1], 97); // SMALL_INTEGER_EXT
            assert_eq!(bytes[2], 0);
        }
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_small_int(), Some(0));
    }

    #[test]
    fn etf_roundtrip_int_255() {
        let mut heap = ProcessHeap::new();
        // 255 is the max for SMALL_INTEGER_EXT
        let bin = unsafe { bif_term_to_binary(&mut heap, &[ErlTerm::small_int(255)]) }.unwrap();
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[1], 97); // SMALL_INTEGER_EXT
            assert_eq!(bytes[2], 255);
        }
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_small_int(), Some(255));
    }

    #[test]
    fn etf_roundtrip_int_256() {
        let mut heap = ProcessHeap::new();
        // 256 must use INTEGER_EXT (doesn't fit SMALL_INTEGER_EXT)
        let bin = unsafe { bif_term_to_binary(&mut heap, &[ErlTerm::small_int(256)]) }.unwrap();
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[1], 98); // INTEGER_EXT
        }
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_small_int(), Some(256));
    }

    #[test]
    fn etf_roundtrip_int_neg1() {
        let mut heap = ProcessHeap::new();
        // -1 must use INTEGER_EXT (negative, so not SMALL_INTEGER_EXT)
        let bin = unsafe { bif_term_to_binary(&mut heap, &[ErlTerm::small_int(-1)]) }.unwrap();
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[1], 98); // INTEGER_EXT
        }
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_small_int(), Some(-1));
    }

    #[test]
    fn etf_roundtrip_i32_max() {
        let mut heap = ProcessHeap::new();
        let val = i32::MAX as i64;
        let bin = unsafe { bif_term_to_binary(&mut heap, &[ErlTerm::small_int(val)]) }.unwrap();
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[1], 98); // INTEGER_EXT
        }
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_small_int(), Some(val));
    }

    #[test]
    fn etf_roundtrip_i32_min() {
        let mut heap = ProcessHeap::new();
        let val = i32::MIN as i64;
        let bin = unsafe { bif_term_to_binary(&mut heap, &[ErlTerm::small_int(val)]) }.unwrap();
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[1], 98); // INTEGER_EXT
        }
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_small_int(), Some(val));
    }

    #[test]
    fn etf_roundtrip_beyond_i32_uses_bigext() {
        let mut heap = ProcessHeap::new();
        // i32::MAX + 1 doesn't fit INTEGER_EXT, must use SMALL_BIG_EXT
        let val = i32::MAX as i64 + 1;
        let bin = unsafe { bif_term_to_binary(&mut heap, &[ErlTerm::small_int(val)]) }.unwrap();
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[1], 110); // SMALL_BIG_EXT
        }
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_small_int(), Some(val));
    }

    #[test]
    fn etf_roundtrip_below_i32_uses_bigext() {
        let mut heap = ProcessHeap::new();
        // i32::MIN - 1 doesn't fit INTEGER_EXT, must use SMALL_BIG_EXT
        let val = i32::MIN as i64 - 1;
        let bin = unsafe { bif_term_to_binary(&mut heap, &[ErlTerm::small_int(val)]) }.unwrap();
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[1], 110); // SMALL_BIG_EXT
        }
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_small_int(), Some(val));
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — empty container tests
    // -----------------------------------------------------------------------

    #[test]
    fn etf_roundtrip_empty_tuple() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_tuple(&[]);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { decoded.is_tuple() });
        unsafe {
            let hdr = BoxedHeader(*decoded.as_boxed_ptr().unwrap());
            assert_eq!(hdr.arity(), 0);
        }
    }

    #[test]
    fn etf_roundtrip_empty_binary() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_binary(b"");
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(decoded.is_binary());
            assert_eq!(decoded.binary_size(), Some(0));
        }
    }

    #[test]
    fn etf_roundtrip_empty_map() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_map(MapPairs::new(&[]));
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { decoded.is_map() });
        unsafe {
            let hdr = BoxedHeader(*decoded.as_boxed_ptr().unwrap());
            assert_eq!(hdr.arity(), 0);
        }
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — float edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn etf_roundtrip_float_zero() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_float(0.0);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        unsafe {
            let val = f64::from_bits(*decoded.as_boxed_ptr().unwrap().add(1));
            assert_eq!(val, 0.0);
        }
    }

    #[test]
    fn etf_roundtrip_float_negative() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_float(-273.15);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        unsafe {
            let val = f64::from_bits(*decoded.as_boxed_ptr().unwrap().add(1));
            assert_eq!(val, -273.15);
        }
    }

    #[test]
    fn etf_roundtrip_float_infinity() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_float(f64::INFINITY);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        unsafe {
            let val = f64::from_bits(*decoded.as_boxed_ptr().unwrap().add(1));
            assert!(val.is_infinite() && val.is_sign_positive());
        }
    }

    #[test]
    fn etf_roundtrip_float_neg_infinity() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_float(f64::NEG_INFINITY);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        unsafe {
            let val = f64::from_bits(*decoded.as_boxed_ptr().unwrap().add(1));
            assert!(val.is_infinite() && val.is_sign_negative());
        }
    }

    #[test]
    fn etf_roundtrip_float_nan() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_float(f64::NAN);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        unsafe {
            let val = f64::from_bits(*decoded.as_boxed_ptr().unwrap().add(1));
            assert!(val.is_nan());
        }
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — atom edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn etf_roundtrip_atom_true_false() {
        let mut heap = ProcessHeap::new();
        let t = ErlTerm::atom(ATOM_TRUE);
        let f = ErlTerm::atom(ATOM_FALSE);

        let bin_t = unsafe { bif_term_to_binary(&mut heap, &[t]) }.unwrap();
        let bin_f = unsafe { bif_term_to_binary(&mut heap, &[f]) }.unwrap();

        let decoded_t = unsafe { bif_binary_to_term(&mut heap, &[bin_t]) }.unwrap();
        let decoded_f = unsafe { bif_binary_to_term(&mut heap, &[bin_f]) }.unwrap();

        assert_eq!(decoded_t.as_atom_id(), Some(ATOM_TRUE));
        assert_eq!(decoded_f.as_atom_id(), Some(ATOM_FALSE));
    }

    #[test]
    fn etf_roundtrip_atom_empty() {
        let mut heap = ProcessHeap::new();
        let table = crate::atom_table::global_atom_table();
        let id = table.intern("");
        let term = ErlTerm::atom(id);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        // Empty atom uses SMALL_ATOM_UTF8_EXT with length 0
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[1], 119); // SMALL_ATOM_UTF8_EXT
            assert_eq!(bytes[2], 0);   // length 0
        }
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_atom_id(), Some(id));
    }

    #[test]
    fn etf_roundtrip_atom_unicode() {
        let mut heap = ProcessHeap::new();
        let table = crate::atom_table::global_atom_table();
        let id = table.intern("café");
        let term = ErlTerm::atom(id);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_atom_id(), Some(id));
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — list edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn etf_list_with_non_byte_uses_list_ext() {
        let mut heap = ProcessHeap::new();
        // A list containing 256 (not a byte) must use LIST_EXT, not STRING_EXT
        let term = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(256),
            ErlTerm::small_int(1),
        ]);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[1], 108); // LIST_EXT
        }
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert!(decoded.is_cons());
        unsafe {
            let cell = &*decoded.as_cons_ptr().unwrap();
            assert_eq!(cell.head.as_small_int(), Some(256));
        }
    }

    #[test]
    fn etf_list_with_negative_uses_list_ext() {
        let mut heap = ProcessHeap::new();
        // A list containing -1 (not 0..255) must use LIST_EXT
        let term = heap.alloc_list_from_slice(&[ErlTerm::small_int(-1)]);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[1], 108); // LIST_EXT
        }
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        unsafe {
            let cell = &*decoded.as_cons_ptr().unwrap();
            assert_eq!(cell.head.as_small_int(), Some(-1));
            assert!(cell.tail.is_nil());
        }
    }

    #[test]
    fn etf_list_with_mixed_types_uses_list_ext() {
        let mut heap = ProcessHeap::new();
        let table = crate::atom_table::global_atom_table();
        // [1, hello, 3.0] — mixed types cannot use STRING_EXT
        let float_term = heap.alloc_float(3.0);
        let hello_id = table.intern("hello");
        let term = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(1),
            ErlTerm::atom(hello_id),
            float_term,
        ]);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[1], 108); // LIST_EXT
        }
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert!(decoded.is_cons());
        unsafe {
            let cell = &*decoded.as_cons_ptr().unwrap();
            assert_eq!(cell.head.as_small_int(), Some(1));
            let cell2 = &*cell.tail.as_cons_ptr().unwrap();
            assert!(cell2.head.is_atom());
            let cell3 = &*cell2.tail.as_cons_ptr().unwrap();
            let fval = f64::from_bits(*cell3.head.as_boxed_ptr().unwrap().add(1));
            assert_eq!(fval, 3.0);
            assert!(cell3.tail.is_nil());
        }
    }

    #[test]
    fn etf_roundtrip_single_element_list() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_list_from_slice(&[ErlTerm::small_int(42)]);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert!(decoded.is_cons());
        unsafe {
            let cell = &*decoded.as_cons_ptr().unwrap();
            assert_eq!(cell.head.as_small_int(), Some(42));
            assert!(cell.tail.is_nil());
        }
    }

    #[test]
    fn etf_roundtrip_long_list() {
        let mut heap = ProcessHeap::new();
        // Build a list of 500 integers (exceeds STRING_EXT if non-bytes)
        let elems: Vec<ErlTerm> = (0..500).map(|i| ErlTerm::small_int(i * 3)).collect();
        let term = heap.alloc_list_from_slice(&elems);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        // Walk the decoded list and verify
        let mut cursor = decoded;
        for i in 0..500 {
            assert!(cursor.is_cons(), "expected cons at position {}", i);
            unsafe {
                let cell = &*cursor.as_cons_ptr().unwrap();
                assert_eq!(cell.head.as_small_int(), Some(i * 3),
                    "mismatch at position {}", i);
                cursor = cell.tail;
            }
        }
        assert!(cursor.is_nil());
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — map edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn etf_roundtrip_map_multiple_pairs() {
        let mut heap = ProcessHeap::new();
        let table = crate::atom_table::global_atom_table();
        let pairs = [
            ErlTerm::atom(table.intern("a")), ErlTerm::small_int(1),
            ErlTerm::atom(table.intern("b")), ErlTerm::small_int(2),
            ErlTerm::atom(table.intern("c")), ErlTerm::small_int(3),
        ];
        let term = heap.alloc_map(MapPairs::from_flat(&pairs).expect("complete key/value pairs"));
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { decoded.is_map() });
        unsafe {
            let ptr = decoded.as_boxed_ptr().unwrap();
            let hdr = BoxedHeader(*ptr);
            assert_eq!(hdr.arity(), 3); // 3 pairs
            // Verify each pair
            for i in 0..3 {
                let k = ErlTerm(*ptr.add(1 + i * 2));
                let v = ErlTerm(*ptr.add(2 + i * 2));
                assert!(k.is_atom());
                assert_eq!(v.as_small_int(), Some(i as i64 + 1));
            }
        }
    }

    #[test]
    fn etf_roundtrip_map_with_complex_values() {
        let mut heap = ProcessHeap::new();
        let table = crate::atom_table::global_atom_table();
        // Map with a nested tuple as value: #{key => {1, 2}}
        let inner_tuple = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        let pairs = [
            ErlTerm::atom(table.intern("key")),
            inner_tuple,
        ];
        let term = heap.alloc_map(MapPairs::from_flat(&pairs).expect("complete key/value pairs"));
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { decoded.is_map() });
        unsafe {
            let ptr = decoded.as_boxed_ptr().unwrap();
            let v = ErlTerm(*ptr.add(2));
            assert!(v.is_tuple());
            let tptr = v.as_boxed_ptr().unwrap();
            assert_eq!(ErlTerm(*tptr.add(1)).as_small_int(), Some(1));
            assert_eq!(ErlTerm(*tptr.add(2)).as_small_int(), Some(2));
        }
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — deeply nested structures
    // -----------------------------------------------------------------------

    #[test]
    fn etf_roundtrip_deeply_nested() {
        let mut heap = ProcessHeap::new();
        // Build: {{{42}}}
        let inner = heap.alloc_tuple(&[ErlTerm::small_int(42)]);
        let mid = heap.alloc_tuple(&[inner]);
        let outer = heap.alloc_tuple(&[mid]);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[outer]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        // Unwrap 3 levels
        unsafe {
            let p1 = decoded.as_boxed_ptr().unwrap();
            assert_eq!(BoxedHeader(*p1).arity(), 1);
            let l2 = ErlTerm(*p1.add(1));
            let p2 = l2.as_boxed_ptr().unwrap();
            assert_eq!(BoxedHeader(*p2).arity(), 1);
            let l3 = ErlTerm(*p2.add(1));
            let p3 = l3.as_boxed_ptr().unwrap();
            assert_eq!(BoxedHeader(*p3).arity(), 1);
            assert_eq!(ErlTerm(*p3.add(1)).as_small_int(), Some(42));
        }
    }

    #[test]
    fn etf_roundtrip_list_of_tuples() {
        let mut heap = ProcessHeap::new();
        // [{1, a}, {2, b}]
        let table = crate::atom_table::global_atom_table();
        let t1 = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::atom(table.intern("a"))]);
        let t2 = heap.alloc_tuple(&[ErlTerm::small_int(2), ErlTerm::atom(table.intern("b"))]);
        let term = heap.alloc_list_from_slice(&[t1, t2]);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert!(decoded.is_cons());
        unsafe {
            let cell = &*decoded.as_cons_ptr().unwrap();
            assert!(cell.head.is_tuple());
            let tp = cell.head.as_boxed_ptr().unwrap();
            assert_eq!(ErlTerm(*tp.add(1)).as_small_int(), Some(1));
            let cell2 = &*cell.tail.as_cons_ptr().unwrap();
            assert!(cell2.head.is_tuple());
            assert!(cell2.tail.is_nil());
        }
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — error handling
    // -----------------------------------------------------------------------

    #[test]
    fn etf_term_to_binary_wrong_arity() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_term_to_binary(&mut heap, &[]) }.is_err());
        assert!(unsafe { bif_term_to_binary(&mut heap, &[ErlTerm::small_int(1), ErlTerm::small_int(2)]) }.is_err());
    }

    #[test]
    fn etf_binary_to_term_wrong_arity() {
        let mut heap = ProcessHeap::new();
        assert!(unsafe { bif_binary_to_term(&mut heap, &[]) }.is_err());
    }

    #[test]
    fn etf_binary_to_term_truncated_small_int() {
        let mut heap = ProcessHeap::new();
        // Version byte + SMALL_INTEGER_EXT tag, but missing the value byte
        let bad = heap.alloc_binary(&[131, 97]);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[bad]) }.is_err());
    }

    #[test]
    fn etf_binary_to_term_truncated_integer() {
        let mut heap = ProcessHeap::new();
        // Version byte + INTEGER_EXT tag, but only 2 of 4 value bytes
        let bad = heap.alloc_binary(&[131, 98, 0, 0]);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[bad]) }.is_err());
    }

    #[test]
    fn etf_binary_to_term_truncated_float() {
        let mut heap = ProcessHeap::new();
        // Version byte + NEW_FLOAT_EXT tag, but only 4 of 8 bytes
        let bad = heap.alloc_binary(&[131, 70, 0, 0, 0, 0]);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[bad]) }.is_err());
    }

    #[test]
    fn etf_binary_to_term_truncated_atom() {
        let mut heap = ProcessHeap::new();
        // Version byte + SMALL_ATOM_UTF8_EXT, len=5, but only 2 bytes of name
        let bad = heap.alloc_binary(&[131, 119, 5, b'h', b'i']);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[bad]) }.is_err());
    }

    #[test]
    fn etf_binary_to_term_truncated_binary() {
        let mut heap = ProcessHeap::new();
        // Version + BINARY_EXT, claims 10 bytes but only has 3
        let bad = heap.alloc_binary(&[131, 109, 0, 0, 0, 10, 1, 2, 3]);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[bad]) }.is_err());
    }

    #[test]
    fn etf_binary_to_term_truncated_tuple() {
        let mut heap = ProcessHeap::new();
        // Version + SMALL_TUPLE_EXT, arity=2, but only 1 element
        let bad = heap.alloc_binary(&[131, 104, 2, 97, 1]);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[bad]) }.is_err());
    }

    #[cfg(not(target_env = "msvc"))]
    #[test]
    fn etf_impossible_container_counts_reject_before_allocation() {
        let mut heap = ProcessHeap::new();
        let allocated = tikv_jemalloc_ctl::thread::allocatedp::read().unwrap();
        // Bounded counts keep RED safe: the old decoder reserves at most 64 KiB.
        for input in [
            &[104, 255][..],
            &[108, 0, 0, 16, 0, 106][..],
            &[105, 0, 0, 16, 0][..],
            &[116, 0, 0, 16, 0][..],
        ] {
            let before = allocated.get();
            assert!(etf_decode(&mut heap, input, 0).is_err());
            let bytes = allocated.get() - before;
            assert_eq!(bytes, 0, "reserved memory for impossible ETF container {input:?}");
        }
    }

    #[test]
    fn etf_maximum_container_counts_and_missing_list_tails_are_rejected() {
        let mut heap = ProcessHeap::new();
        for input in [
            &[108, 255, 255, 255, 255, 106][..],
            &[105, 255, 255, 255, 255][..],
            &[116, 255, 255, 255, 255][..],
            &[108, 0, 0, 0, 0][..],
            &[108, 0, 0, 0, 1, 106][..],
            &[116, 0, 0, 0, 1, 106][..],
        ] {
            assert!(etf_decode(&mut heap, input, 0).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn etf_container_bounds_allow_one_byte_terms_and_empty_containers() {
        let mut heap = ProcessHeap::new();
        for input in [
            &[104, 0][..], &[105, 0, 0, 0, 0][..],
            &[108, 0, 0, 0, 0, 106][..], &[116, 0, 0, 0, 0][..],
            &[104, 1, 106][..], &[105, 0, 0, 0, 1, 106][..],
            &[108, 0, 0, 0, 1, 106, 106][..],
            &[116, 0, 0, 0, 1, 106, 106][..],
        ] {
            let (_, end) = etf_decode(&mut heap, input, 0).unwrap();
            assert_eq!(end, input.len());
            // Existing ETF behavior permits trailing bytes.
            let mut trailing = input.to_vec();
            trailing.push(255);
            assert_eq!(etf_decode(&mut heap, &trailing, 0).unwrap().1, end);
        }
    }

    #[test]
    fn etf_binary_to_term_truncated_list() {
        let mut heap = ProcessHeap::new();
        // Version + LIST_EXT, count=2, but only 1 element and no tail
        let bad = heap.alloc_binary(&[131, 108, 0, 0, 0, 2, 97, 1]);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[bad]) }.is_err());
    }

    #[test]
    fn etf_binary_to_term_truncated_map() {
        let mut heap = ProcessHeap::new();
        // Version + MAP_EXT, npairs=1, but only key, no value
        let bad = heap.alloc_binary(&[131, 116, 0, 0, 0, 1, 97, 1]);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[bad]) }.is_err());
    }

    #[test]
    fn etf_binary_to_term_truncated_string_ext() {
        let mut heap = ProcessHeap::new();
        // Version + STRING_EXT, claims length=5 but only 2 bytes
        let bad = heap.alloc_binary(&[131, 107, 0, 5, 65, 66]);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[bad]) }.is_err());
    }

    #[test]
    fn etf_binary_to_term_truncated_small_big() {
        let mut heap = ProcessHeap::new();
        // Version + SMALL_BIG_EXT, n=4, sign=0, but only 2 bytes of data
        let bad = heap.alloc_binary(&[131, 110, 4, 0, 1, 2]);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[bad]) }.is_err());
    }

    #[test]
    fn etf_binary_to_term_unknown_tag() {
        let mut heap = ProcessHeap::new();
        // Version byte + unknown tag 255
        let bad = heap.alloc_binary(&[131, 255]);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[bad]) }.is_err());
    }

    #[test]
    fn etf_binary_to_term_only_version_byte() {
        let mut heap = ProcessHeap::new();
        // Just the version byte, no actual term data
        let bad = heap.alloc_binary(&[131]);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[bad]) }.is_err());
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — version byte and format verification
    // -----------------------------------------------------------------------

    #[test]
    fn etf_output_starts_with_version_131() {
        let mut heap = ProcessHeap::new();
        // Every encoded term must begin with version byte 131
        let cases: Vec<ErlTerm> = vec![
            ErlTerm::small_int(0),
            ErlTerm::nil(),
            ErlTerm::atom(ATOM_OK),
            ErlTerm::pid(crate::process::ProcessId::new(1)),
        ];
        for term in cases {
            let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
            unsafe {
                let bytes = bin.binary_bytes_any().unwrap();
                assert_eq!(bytes[0], 131, "version byte must be 131");
            }
        }
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — idempotence (encode→decode→encode)
    // -----------------------------------------------------------------------

    #[test]
    fn etf_idempotent_encode() {
        let mut heap = ProcessHeap::new();
        let table = crate::atom_table::global_atom_table();
        // Complex term: {hello, [1, 2, 3], <<"data">>, #{a => true}}
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(1),
            ErlTerm::small_int(2),
            ErlTerm::small_int(3),
        ]);
        let binary = heap.alloc_binary(b"data");
        let map = heap.alloc_map(MapPairs::from_flat(&[
            ErlTerm::atom(table.intern("a")),
            ErlTerm::atom(ATOM_TRUE),
        ]).expect("complete key/value pairs"));
        let term = heap.alloc_tuple(&[
            ErlTerm::atom(table.intern("hello")),
            list,
            binary,
            map,
        ]);

        let bin1 = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin1]) }.unwrap();
        let bin2 = unsafe { bif_term_to_binary(&mut heap, &[decoded]) }.unwrap();

        // The two serialized forms must be identical
        unsafe {
            let bytes1 = bin1.binary_bytes_any().unwrap();
            let bytes2 = bin2.binary_bytes_any().unwrap();
            assert_eq!(bytes1, bytes2, "encode→decode→encode must be idempotent");
        }
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — binary data integrity
    // -----------------------------------------------------------------------

    #[test]
    fn etf_roundtrip_binary_with_nulls() {
        let mut heap = ProcessHeap::new();
        let data = b"\x00\x00\xff\x00\xfe";
        let term = heap.alloc_binary(data);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        unsafe {
            assert_eq!(decoded.binary_bytes_any().unwrap(), data);
        }
    }

    #[test]
    fn etf_roundtrip_binary_all_byte_values() {
        let mut heap = ProcessHeap::new();
        let data: Vec<u8> = (0..=255).collect();
        let term = heap.alloc_binary(&data);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert_eq!(unsafe { decoded.binary_size() }, Some(256));
        unsafe {
            assert_eq!(decoded.binary_bytes_any().unwrap(), &data[..]);
        }
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — pid edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn etf_pid_preserves_domain_and_rejects_reserved_bits() {
        use crate::process::ProcessId;
        let mut heap = ProcessHeap::new();
        for raw in [u32::MAX as u64, 1u64 << 32, ProcessId::MAX] {
            let term = ErlTerm::pid(ProcessId::new(raw));
            let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
            let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
            assert_eq!(decoded.as_pid_id(), term.as_pid_id());
        }
        let term = ErlTerm::pid(ProcessId::new(1));
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        // SAFETY: the binary remains owned by this heap while its bytes are copied.
        let mut bytes = unsafe { bin.borrow(&heap) }.binary_bytes().unwrap().to_vec();
        let serial = bytes.len() - 8;
        bytes[serial] = 0x10; // PID bit 60 would occupy the external marker.
        let malformed = heap.alloc_binary(&bytes);
        assert!(unsafe { bif_binary_to_term(&mut heap, &[malformed]) }.is_err());
    }

    #[test]
    fn etf_roundtrip_pid_zero() {
        let mut heap = ProcessHeap::new();
        let term = ErlTerm::pid(crate::process::ProcessId::new(0));
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert!(decoded.is_pid());
        assert_eq!(decoded.as_pid_id(), Some(crate::process::ProcessId::new(0)));
    }

    #[test]
    fn etf_roundtrip_pid_large() {
        let mut heap = ProcessHeap::new();
        let term = ErlTerm::pid(crate::process::ProcessId::new(0xFFFF));
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert!(decoded.is_pid());
        assert_eq!(decoded.as_pid_id(), Some(crate::process::ProcessId::new(0xFFFF)));
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — large tuple
    // -----------------------------------------------------------------------

    #[test]
    fn etf_roundtrip_large_tuple() {
        let mut heap = ProcessHeap::new();
        // Tuple with 300 elements (exceeds SMALL_TUPLE_EXT 255 limit)
        let elems: Vec<ErlTerm> = (0..300).map(ErlTerm::small_int).collect();
        let term = heap.alloc_tuple(&elems);
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        unsafe {
            let bytes = bin.binary_bytes_any().unwrap();
            assert_eq!(bytes[1], 105); // LARGE_TUPLE_EXT
        }
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { decoded.is_tuple() });
        unsafe {
            let ptr = decoded.as_boxed_ptr().unwrap();
            let hdr = BoxedHeader(*ptr);
            assert_eq!(hdr.arity(), 300);
            // Spot-check first and last elements
            assert_eq!(ErlTerm(*ptr.add(1)).as_small_int(), Some(0));
            assert_eq!(ErlTerm(*ptr.add(300)).as_small_int(), Some(299));
        }
    }

    // -----------------------------------------------------------------------
    // term_to_binary / binary_to_term — bigint edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn etf_roundtrip_bigint_zero() {
        let mut heap = ProcessHeap::new();
        use num_bigint::BigInt;
        // BigInt zero — should round-trip via small int
        let big = BigInt::from(0);
        let term = crate::bigint::bigint_to_term(&mut heap, big.clone());
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        assert_eq!(decoded.as_small_int(), Some(0));
    }

    #[test]
    fn etf_roundtrip_very_large_bigint() {
        let mut heap = ProcessHeap::new();
        use num_bigint::BigInt;
        // 2^128 — a genuinely big integer
        let big = BigInt::from(1u128 << 127) * BigInt::from(2);
        let term = crate::bigint::bigint_to_term(&mut heap, big.clone());
        let bin = unsafe { bif_term_to_binary(&mut heap, &[term]) }.unwrap();
        let decoded = unsafe { bif_binary_to_term(&mut heap, &[bin]) }.unwrap();
        let decoded_big = unsafe { decoded.borrow(&heap) }.to_bigint().unwrap();
        assert_eq!(decoded_big, big);
    }
}
