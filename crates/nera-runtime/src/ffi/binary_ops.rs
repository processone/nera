//! Binary operations: allocation, pattern matching, split, conversions.

#![allow(unused_unsafe)]

use crate::binary::BinaryData;
use crate::binary::bit_copy::copy_bits;
use crate::match_context::MatchContext;
use crate::atom_table::global_atom_table;
use crate::bif;
use crate::heap::{clone_arc_from_raw, REFC_BINARY_THRESHOLD};
use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;

mod bit_window;
mod float_segment;
use float_segment::{encode_half, decode_half};
use bit_window::{BitReader, BitWriter};


/// Read the pattern of `binary:match/2` or `binary:split/2,3`: a nonempty
/// binary, or a nonempty proper list of nonempty binaries. `None` is badarg.
/// Every element is checked, even when an earlier one would already match.
///
/// # Safety
/// The pattern term graph is live, initialized, immutable and unmoved while
/// the returned slices are borrowed.
unsafe fn search_patterns<'a>(pattern: ErlTerm) -> Option<Vec<&'a [u8]>> {
    let bytes = |term: ErlTerm| -> Option<&'a [u8]> {
        // A bitstring with a partial last byte is not a binary pattern.
        if !unsafe { term.is_binary() } { return None; }
        let (ptr, size) = unsafe { binary_data(term) }?;
        (size > 0).then(|| unsafe { std::slice::from_raw_parts(ptr, size) })
    };
    if unsafe { pattern.is_binary() } { return Some(vec![bytes(pattern)?]); }
    let mut patterns = Vec::new();
    let mut cursor = pattern;
    while let Some(cons) = cursor.as_cons_ptr() {
        let (head, tail) = unsafe { ((*cons).head, (*cons).tail) };
        patterns.push(bytes(head)?);
        cursor = tail;
    }
    (cursor.is_nil() && !patterns.is_empty()).then_some(patterns)
}

/// The earliest match of any pattern as `(offset, length)`, preferring the
/// longest pattern at that offset, which is the match BEAM's binary:match/2
/// and binary:split/2,3 pick. The order of the patterns does not matter.
/// Every pattern is nonempty, as `search_patterns` returns them.
fn first_match(haystack: &[u8], patterns: &[&[u8]]) -> Option<(usize, usize)> {
    if let [needle] = patterns {
        let offset = haystack.windows(needle.len()).position(|w| w == *needle)?;
        return Some((offset, needle.len()));
    }
    // Patterns are nonempty, so comparing first bytes skips most offsets.
    haystack.iter().enumerate().find_map(|(offset, &byte)| {
        let rest = &haystack[offset..];
        patterns.iter().filter(|p| p[0] == byte && rest.starts_with(p)).map(|p| p.len()).max()
            .map(|length| (offset, length))
    })
}

/// A nonnegative bit count representable by the tagged binary construction ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SegmentBits(usize);

impl SegmentBits {
    fn new(size: i64, unit: usize) -> Result<Self, u32> {
        use crate::atom_table::{ATOM_BADARG, ATOM_SYSTEM_LIMIT};
        if size < 0 { return Err(ATOM_BADARG); }
        let bits = usize::try_from(size).ok().and_then(|n| n.checked_mul(unit))
            .filter(|&n| n as u64 <= SMALL_INT_MAX as u64)
            .ok_or(ATOM_SYSTEM_LIMIT)?;
        Ok(Self(bits))
    }
}

/// # Safety
/// The input is a live initialized term graph, immutable and unmoved until return.
unsafe fn segment_bits(size: ErlTerm, unit: usize) -> Result<SegmentBits, u32> {
    if let Some(n) = size.as_small_int() { return SegmentBits::new(n, unit); }
    // Canonical positive bigints exceed this ABI's bit-count domain. Classify
    // them without truncating; other terms and negative integers are badarg.
    match unsafe { crate::bigint::term_to_bigint(size) } {
        Some(n) if n.sign() != num_bigint::Sign::Minus => Err(crate::atom_table::ATOM_SYSTEM_LIMIT),
        _ => Err(crate::atom_table::ATOM_BADARG),
    }
}

/// Validate each segment before summing sizes or allocating the destination.
/// # Safety
/// Requires the context invariants of raise_error and a live, unmoved size term.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_segment_size(
    ctx: *mut ProcessContext, size: u64, unit: u64,
) -> u64 {
    let Some(unit) = ErlTerm(unit).as_small_int().filter(|n| (1..=256).contains(n)) else {
        return raise_badarg(ctx);
    };
    match segment_bits(ErlTerm(size), unit as usize) {
        Ok(bits) => ErlTerm::small_int(bits.0 as i64).0,
        Err(reason) => raise_error(ctx, ErlTerm::atom(reason).0),
    }
}

/// Validate the implicit size of an all-source binary/bitstring segment.
/// # Safety
/// Requires raise_badarg's context contract and a live initialized source graph.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_source_size(ctx: *mut ProcessContext, source: u64, unit: u64) -> u64 {
    let Some(unit) = ErlTerm(unit).as_small_int().filter(|n| (1..=256).contains(n)) else { return raise_badarg(ctx); };
    let Some(bits) = ErlTerm(source).binary_bit_size() else { return raise_badarg(ctx); };
    if !bits.is_multiple_of(unit as usize) { return raise_badarg(ctx); }
    ErlTerm::small_int(bits as i64).0
}

/// Allocate a zeroed binary of the given **bit** size.
/// `bit_size_term` is a tagged small integer representing total bits.
/// Returns a tagged binary ErlTerm with header storing bit count.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_alloc_binary(ctx: *mut ProcessContext, bit_size_term: u64) -> u64 {
    let bit_size = match segment_bits(ErlTerm(bit_size_term), 1) {
        Ok(bits) => bits.0,
        Err(reason) => return raise_error(ctx, ErlTerm::atom(reason).0),
    };
    let byte_size = bit_size.div_ceil(8);
    // Constructors first create a whole-byte descriptor, before patching the
    // exact bit length. Validate that rounded representation before allocating.
    if byte_size > isize::MAX as usize
        || byte_size as u64 > (u64::MAX >> BOXED_TAG_BITS) / 8 {
        return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_SYSTEM_LIMIT).0);
    }
    with_ctx_heap(ctx, |heap| {
        if byte_size > REFC_BINARY_THRESHOLD {
            // Auto-promote large binaries to refc (matches BEAM behaviour).
            let data: std::sync::Arc<BinaryData> = std::sync::Arc::new(BinaryData::new(vec![0u8; byte_size]));
            let term = heap.alloc_refc_binary_from_arc(data, 0, byte_size);
            // Patch header to exact bit count (alloc_refc_binary_from_arc writes byte_size*8)
            if bit_size != byte_size * 8 {
                let ptr = term.as_boxed_ptr().unwrap() as *mut u64;
                *ptr = BoxedHeader::new(crate::term::BoxedSubtype::RefcBinary, bit_size as u64).0;
            }
            term.0
        } else {
            let term = heap.alloc_binary_zeroed(byte_size);
            // Patch header to exact bit count (alloc_binary_zeroed writes byte_size*8)
            if bit_size != byte_size * 8 {
                let ptr = term.as_boxed_ptr().unwrap() as *mut u64;
                unsafe { *ptr = BoxedHeader::new(crate::term::BoxedSubtype::HeapBinary, bit_size as u64).0; }
            }
            term.0
        }
    })
}

/// Write an integer value into a binary at a given BIT offset.
/// Returns its tagged bit count or nil with badarg, before any invalid write.
/// # Safety
/// The context satisfies raise_badarg's contract. All input terms are live,
/// initialized and unmoved; destination backing bytes are exclusively writable.
/// No outstanding byte borrows or GC/reentry may overlap this operation.
///
/// Arguments (all tagged ErlTerms):
/// - `bin`: the binary term
/// - `bit_offset`: bit offset into the binary (tagged small int)
/// - `value`: the integer value to write
/// - `bit_size`: size in bits (tagged small int)
/// - `flags`: packed flags (low 8 bits = signedness, next 8 bits = endianness)
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_put_integer(
    ctx: *mut ProcessContext,
    bin: u64,
    bit_offset_term: u64,
    value: u64,
    bit_size: u64,
    flags: u64,
) -> u64 {
    let Some(bit_off) = nonnegative_offset(bit_offset_term) else { return raise_badarg(ctx); };
    let Some(bits) = nonnegative_offset(bit_size) else { return raise_badarg(ctx); };
    let Some((data_ptr, total)) = binary_data_bits(ErlTerm(bin)) else { return raise_badarg(ctx); };
    if total.checked_sub(bit_off).is_none_or(|left| bits > left) { return raise_badarg(ctx); }
    let endianness = ((flags >> 8) & 0xFF) as u8;
    let endianness = if endianness == 2 { u8::from(cfg!(target_endian = "little")) } else { endianness };
    let value = ErlTerm(value);
    let val = if let Some(n) = value.as_small_int() { n } else {
        // Own numeric bytes before exposing mutable destination storage.
        let Some(big) = crate::bigint::term_to_bigint(value) else { return raise_badarg(ctx); };
        let pad = if big.sign() == num_bigint::Sign::Minus { 0xff } else { 0 };
        let bytes = big.to_signed_bytes_le();
        write_big_integer_bits(data_ptr.cast_mut(), bit_off, &bytes, pad, bits, endianness);
        return ErlTerm::small_int(bits as i64).0;
    };
    if bits == 0 { return ErlTerm::small_int(0).0; }
    let data_ptr = data_ptr.cast_mut();
    // Fast path: byte-aligned offset AND whole-byte size
    if bit_off.is_multiple_of(8) && bits.is_multiple_of(8) {
        let offset = bit_off / 8;
        let byte_count = bits / 8;
        let ival = val as u64;
        // Sign-extend byte for padding positions beyond 8 bytes:
        // 0x00 for non-negative, 0xFF for negative values.
        let pad = if val < 0 { 0xFFu8 } else { 0x00u8 };
        match endianness {
            0 => {
                // Big-endian: MSB first
                // For byte_count > 8, leading bytes are sign-extended padding.
                for i in 0..byte_count {
                    let pos = byte_count - 1 - i; // byte position from LSB
                    *data_ptr.add(offset + i) = if pos < 8 {
                        ((ival >> (pos * 8)) & 0xFF) as u8
                    } else {
                        pad
                    };
                }
            }
            1 => {
                // Little-endian: LSB first
                // For byte_count > 8, trailing bytes are sign-extended padding.
                for i in 0..byte_count {
                    *data_ptr.add(offset + i) = if i < 8 {
                        ((ival >> (i * 8)) & 0xFF) as u8
                    } else {
                        pad
                    };
                }
            }
            _ => {
                // Native endianness
                let bytes = val.to_ne_bytes();
                for i in 0..byte_count {
                    *data_ptr.add(offset + i) = bytes.get(i).copied().unwrap_or(pad);
                }
            }
        }
        return ErlTerm::small_int(bits as i64).0;
    }

    if bits <= 64 {
        write_integer_bits(data_ptr, bit_off, val as u64, bits, endianness);
    } else {
        write_big_integer_bits(data_ptr, bit_off, &val.to_le_bytes(),
            if val < 0 { 0xff } else { 0 }, bits, endianness);
    }
    ErlTerm::small_int(bits as i64).0
}

/// Write an owned two's-complement integer, respecting byte order and partial
/// final bytes. The caller checks that all destination bits are in bounds.
unsafe fn write_big_integer_bits(
    data: *mut u8, offset: usize, bytes: &[u8], pad: u8, bits: usize, endian: u8,
) {
    let little = endian == 1 || (endian == 2 && cfg!(target_endian = "little"));
    for wire_bit in 0..bits {
        let value_bit = if little {
            let start = (wire_bit / 8) * 8;
            start + (bits - start).min(8) - 1 - wire_bit % 8
        } else {
            bits - 1 - wire_bit
        };
        let byte = bytes.get(value_bit / 8).copied().unwrap_or(pad);
        let bit = (byte >> (value_bit % 8)) & 1;
        let target = offset + wire_bit;
        let ptr = unsafe { data.add(target / 8) };
        let mask = 1u8 << (7 - target % 8);
        unsafe { *ptr = (*ptr & !mask) | (bit << (7 - target % 8)); }
    }
}

/// Write `bit_count` bits of `value` into `data` starting at `bit_offset`.
/// Uses read-modify-write on each affected byte. Value is written MSB-first
/// for big-endian, LSB-first for little-endian.
unsafe fn write_integer_bits(
    data: *mut u8,
    bit_offset: usize,
    value: u64,
    bit_count: usize,
    endianness: u8,
) {
    if bit_count == 0 || bit_count > 64 {
        return;
    }

    // Convert value to big-endian bit order (MSB at position 0) for writing.
    // For little-endian, we reverse the byte order of the value first.
    let val = if endianness == 1 {
        let full = bit_count / 8;
        let partial = bit_count % 8;
        let mut wire = 0_u64;
        for i in 0..full { wire = (wire << 8) | ((value >> (i * 8)) & 0xff); }
        if partial != 0 { wire = (wire << partial) | ((value >> (full * 8)) & ((1 << partial) - 1)); }
        wire
    } else { value };

    // Write bit-by-bit into the destination, MSB first.
    // We process byte-by-byte for efficiency.
    let first_byte = bit_offset / 8;
    let last_byte = (bit_offset + bit_count - 1) / 8;

    for byte_idx in first_byte..=last_byte {
        // Determine which bits of this byte we're writing
        let byte_bit_start = byte_idx * 8;
        let write_start = if bit_offset > byte_bit_start { bit_offset } else { byte_bit_start };
        let write_end = if bit_offset + bit_count < byte_bit_start + 8 {
            bit_offset + bit_count
        } else {
            byte_bit_start + 8
        };
        let bits_in_byte = write_end - write_start;
        let shift_in_byte = (byte_bit_start + 8) - write_end; // distance from LSB

        // Extract the corresponding bits from val
        let val_bit_start = write_start - bit_offset;
        let val_shift = bit_count - val_bit_start - bits_in_byte;
        let mask = ((1u64 << bits_in_byte) - 1) as u8;
        let val_bits = ((val >> val_shift) & mask as u64) as u8;

        // Read-modify-write the byte
        let existing = *data.add(byte_idx);
        let clear_mask = !(mask << shift_in_byte);
        *data.add(byte_idx) = (existing & clear_mask) | (val_bits << shift_in_byte);
    }
}

/// Only IEEE widths supported by Erlang float segments can reach byte access.
#[derive(Clone, Copy)]
#[repr(u8)]
enum FloatWidth { Half = 16, Single = 32, Double = 64 }

impl FloatWidth {
    #[inline]
    fn from_term(term: ErlTerm) -> Option<Self> {
        match term.as_small_int()? {
            16 => Some(Self::Half), 32 => Some(Self::Single), 64 => Some(Self::Double), _ => None,
        }
    }
}

/// Validate a dynamic float width before allocation, including unit scaling.
/// # Safety
/// The context satisfies raise_error's contract; size is a live initialized,
/// immutable term graph, unmoved until return. Unit is a tagged integer.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_float_segment_size(ctx: *mut ProcessContext, size: u64, unit: u64) -> u64 {
    let Some(unit) = ErlTerm(unit).as_small_int().filter(|u| (1..=256).contains(u)) else {
        return raise_badarg(ctx);
    };
    let bits = match segment_bits(ErlTerm(size), unit as usize) {
        Ok(bits) => bits.0,
        Err(reason) => return raise_error(ctx, ErlTerm::atom(reason).0),
    };
    if bits.div_ceil(8) as u64 > (u64::MAX >> BOXED_TAG_BITS) / 8 {
        return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_SYSTEM_LIMIT).0);
    }
    match bits {
        16 | 32 | 64 => ErlTerm::small_int(bits as i64).0,
        _ => raise_badarg(ctx),
    }
}

/// Write a finite numeric value as an IEEE float at an arbitrary bit offset.
/// Returns a tagged bit count, or nil with exception status on failure.
/// # Safety
/// Requires write_binary_bytes's context and exclusive binary-access contract;
/// the numeric input is initialized, live and unmoved until conversion ends.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_put_float(
    ctx: *mut ProcessContext, bin: u64, offset: u64, value: u64, bit_size: u64, flags: u64,
) -> u64 {
    let Some(width) = FloatWidth::from_term(ErlTerm(bit_size)) else { return raise_badarg(ctx); };
    let Some(value) = extract_numeric(ErlTerm(value)).filter(|f| f.is_finite()) else {
        return raise_badarg(ctx);
    };
    let little = match (flags >> 8) & 0xFF { 0 => false, 1 => true, _ => cfg!(target_endian = "little") };
    match width {
        FloatWidth::Half => {
            let raw = encode_half(value);
            let bytes = if little { raw.to_le_bytes() } else { raw.to_be_bytes() };
            write_binary_bytes(ctx, bin, offset, &bytes)
        }
        FloatWidth::Single => {
            let bytes = if little { (value as f32).to_le_bytes() } else { (value as f32).to_be_bytes() };
            write_binary_bytes(ctx, bin, offset, &bytes)
        }
        FloatWidth::Double => {
            let bytes = if little { value.to_le_bytes() } else { value.to_be_bytes() };
            write_binary_bytes(ctx, bin, offset, &bytes)
        }
    }
}

/// Decode before borrowing the destination heap: no input borrow survives allocation.
/// # Safety
/// The input is a live immutable runtime graph, including any backing bytes,
/// unmoved until decoding finishes. Match-context metadata has the runtime layout.
#[inline]
unsafe fn read_float(term: ErlTerm, offset: u64, size: u64, flags: u64) -> Option<f64> {
    let width = FloatWidth::from_term(ErlTerm(size))?;
    let relative = nonnegative_offset(offset)?;
    let window = MatchContext::view(term)?;
    let offset = window.resolve(relative, width as usize)?;
    let (data, _) = binary_data_bits(window.binary())?;
    let shift = (offset % 8) as u32;
    let count = width as usize / 8 + usize::from(shift != 0);
    // SAFETY: the logical range was checked before forming this scoped borrow;
    // count covers the encoding and its carry byte inside the live backing.
    let bytes = std::slice::from_raw_parts(data.add(offset / 8), count);
    let reader = BitReader::new(bytes, shift)?;
    let little = match (flags >> 8) & 0xFF { 0 => false, 1 => true, _ => cfg!(target_endian = "little") };
    let value = match width {
        FloatWidth::Half => {
            let bytes = reader.read::<2>()?;
            decode_half(if little { u16::from_le_bytes(bytes) } else { u16::from_be_bytes(bytes) })?
        }
        FloatWidth::Single => {
            let bytes = reader.read::<4>()?;
            if little { f32::from_le_bytes(bytes) as f64 } else { f32::from_be_bytes(bytes) as f64 }
        }
        FloatWidth::Double => {
            let bytes = reader.read::<8>()?;
            if little { f64::from_le_bytes(bytes) } else { f64::from_be_bytes(bytes) }
        }
    };
    value.is_finite().then_some(value)
}

/// Extract a float at a bit offset. Nil signals pattern failure, including
/// nonfinite encodings, invalid widths and ranges; failure allocates nothing.
/// # Safety
/// The context satisfies with_ctx_heap's exclusive-access contract. Input
/// storage satisfies read_float's contract and remains live until decoding ends.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_get_float(
    ctx: *mut ProcessContext, bin: u64, offset: u64, bit_size: u64, flags: u64,
) -> u64 {
    match read_float(ErlTerm(bin), offset, bit_size, flags) {
        Some(value) => with_ctx_heap(ctx, |heap| heap.alloc_float(value).0),
        None => ErlTerm::nil().0,
    }
}

/// Copy a source bitstring into a destination under construction.
/// Byte-aligned copies use the bulk path; other copies preserve surrounding bits.
/// Returns a tagged bit count, or nil with badarg before any invalid write.
/// # Safety
/// Context and live terms satisfy the integer writer's contract. Source bytes
/// are readable; destination bytes are exclusively writable except for possible
/// source overlap, handled by directional bitwise memmove. No GC/reentry.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_put_binary(
    ctx: *mut ProcessContext,
    dest: u64,
    bit_offset_term: u64,
    src: u64,
    bit_count_term: u64,
) -> u64 {
    let Some(offset) = nonnegative_offset(bit_offset_term) else { return raise_badarg(ctx); };
    let Some(count) = nonnegative_offset(bit_count_term) else { return raise_badarg(ctx); };
    let (Some((target, destination_bits)), Some((input, source_bits))) =
        (binary_data_bits(ErlTerm(dest)), binary_data_bits(ErlTerm(src))) else { return raise_badarg(ctx); };
    if count > source_bits || destination_bits.checked_sub(offset).is_none_or(|left| count > left) {
        return raise_badarg(ctx);
    }
    copy_bits(target.cast_mut(), offset, input, 0, count);
    ErlTerm::small_int(count as i64).0
}

/// Extract an integer value from a binary at a given BIT offset.
///
/// Arguments (all tagged ErlTerms):
/// - `bin`: the binary term
/// - `bit_offset`: bit offset into the binary
/// - `bit_size`: number of bits to extract
/// - `flags`: packed flags (signedness + endianness)
///
/// Returns an integer, promoting values outside the immediate range, or nil
/// when the logical range or arguments are invalid.
/// # Safety
/// Context satisfies with_ctx_heap's contract; input storage is initialized,
/// live and immutable through decoding, with no GC/reentry while borrowed.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_get_integer(
    ctx: *mut ProcessContext,
    bin: u64,
    bit_offset_term: u64,
    bit_size: u64,
    flags: u64,
) -> u64 {
    let Some(width) = nonnegative_offset(bit_size) else { return ErlTerm::nil().0; };
    let Some(relative) = nonnegative_offset(bit_offset_term) else { return ErlTerm::nil().0; };
    let Some(window) = MatchContext::view(ErlTerm(bin)) else { return ErlTerm::nil().0; };
    let Some(offset) = window.resolve(relative, width) else { return ErlTerm::nil().0; };
    let Some((data, _)) = binary_data_bits(window.binary()) else { return ErlTerm::nil().0; };
    nera_rt_binary_get_integer_raw(ctx, data as u64, ErlTerm::small_int(offset as i64).0, bit_size, flags)
}

// The value has already been read before any allocation; binary input pointers
// are never retained across heap access. The caller provides a valid context.
#[inline]
unsafe fn binary_integer_term(ctx: *mut ProcessContext, value: u64, signedness: u8) -> u64 {
    if signedness == 1 {
        integer_term(ctx, value as i64)
    } else if value <= SMALL_INT_MAX as u64 {
        ErlTerm::small_int(value as i64).0
    } else {
        with_ctx_heap(ctx, |heap| crate::bigint::bigint_to_term(heap, value.into()).0)
    }
}

/// Read `bit_count` bits from `data` starting at `bit_offset`.
/// Returns the value as a u64 (sign-extended if signedness==1).
unsafe fn read_integer_bits(
    data: *const u8,
    bit_offset: usize,
    bit_count: usize,
    signedness: u8,
    endianness: u8,
) -> u64 {
    if bit_count == 0 || bit_count > 64 {
        return 0;
    }

    // Read bits MSB-first from the binary data into a raw value.
    let mut raw: u64 = 0;
    let first_byte = bit_offset / 8;
    let last_byte = (bit_offset + bit_count - 1) / 8;

    for byte_idx in first_byte..=last_byte {
        let byte_bit_start = byte_idx * 8;
        let read_start = if bit_offset > byte_bit_start { bit_offset } else { byte_bit_start };
        let read_end = if bit_offset + bit_count < byte_bit_start + 8 {
            bit_offset + bit_count
        } else {
            byte_bit_start + 8
        };
        let bits_in_byte = read_end - read_start;
        let shift_in_byte = (byte_bit_start + 8) - read_end;

        let byte_val = *data.add(byte_idx);
        let mask = ((1u32 << bits_in_byte) - 1) as u8;
        let extracted = (byte_val >> shift_in_byte) & mask;

        raw = (raw << bits_in_byte) | (extracted as u64);
    }

    // For little-endian, reverse the byte order of the extracted value.
    let little = endianness == 1 || (endianness == 2 && cfg!(target_endian = "little"));
    let val = if little {
        let full = bit_count / 8;
        let partial = bit_count % 8;
        let mut value = 0_u64;
        for i in 0..full {
            value |= ((raw >> (bit_count - (i + 1) * 8)) & 0xff) << (i * 8);
        }
        if partial != 0 { value |= (raw & ((1 << partial) - 1)) << (full * 8); }
        value
    } else { raw };

    // Sign extension
    if signedness == 1 && bit_count < 64 {
        let sign_bit = 1u64 << (bit_count - 1);
        if val & sign_bit != 0 {
            val | !((1u64 << bit_count) - 1)
        } else {
            val
        }
    } else {
        val
    }
}

/// Extract a sub-binary.
///
/// Arguments (all tagged ErlTerms):
/// - `bin`: the source binary
/// - `bit_offset`: start offset in bits (must be multiple of 8)
/// - `bit_count`: number of bits to extract (must be multiple of 8)
///
/// Returns a new binary term.  When the source is a refc binary and the
/// requested sub-binary is large (> `REFC_BINARY_THRESHOLD`), a new refc
/// descriptor is created sharing the same backing `Arc` — zero copy.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_get_binary(
    ctx: *mut ProcessContext,
    bin: u64,
    bit_offset_term: u64,
    bit_count_term: u64,
) -> u64 {
    let bin_term = ErlTerm(bin);
    let bit_off = ErlTerm(bit_offset_term).as_small_int().unwrap_or(0) as usize;
    let bit_count = ErlTerm(bit_count_term).as_small_int().unwrap_or(0) as usize;
    let offset = bit_off / 8;
    let count = bit_count / 8;

    unsafe {
        let ptr = match bin_term.as_boxed_ptr() {
            Some(p) => p,
            None => return with_ctx_heap(ctx, |heap| heap.alloc_binary(&[]).0),
        };
        let header = BoxedHeader(*ptr);

        // Sub-binary optimisation for refc binaries: share the backing Arc
        // instead of copying bytes when the result is large enough.
        if header.subtype() == BOXED_REFC_BINARY {
            let parent_len = (header.arity() as usize).div_ceil(8);
            if offset + count > parent_len {
                return with_ctx_heap(ctx, |heap| heap.alloc_binary(&[]).0);
            }
            let parent_offset = *ptr.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
            let raw_arc_ptr = *ptr.add(REFC_BINARY_PTR_WORD_INDEX);

            if count > REFC_BINARY_THRESHOLD {
                // Large sub-binary: share the backing Arc.
                let cloned_arc = clone_arc_from_raw(raw_arc_ptr);
                return with_ctx_heap(ctx, |heap| {
                    heap.alloc_refc_binary_from_arc(
                        cloned_arc,
                        parent_offset + offset,
                        count,
                    ).0
                });
            }
            // Small sub-binary: copy bytes into a heap binary.
            let vec_ref = &*(raw_arc_ptr as *const BinaryData);
            let start = parent_offset + offset;
            let slice = &vec_ref[start..start + count];
            return with_ctx_heap(ctx, |heap| heap.alloc_binary(slice).0);
        }

        // Heap binary (or any other): copy bytes.
        if let Some((data_ptr, bin_size)) = binary_data(bin_term) {
            if offset + count > bin_size {
                return with_ctx_heap(ctx, |heap| heap.alloc_binary(&[]).0);
            }
            let slice = std::slice::from_raw_parts(data_ptr.add(offset), count);
            with_ctx_heap(ctx, |heap| heap.alloc_binary(slice).0)
        } else {
            with_ctx_heap(ctx, |heap| heap.alloc_binary(&[]).0)
        }
    }
}

/// Get the BIT size of a bitstring (bit_size/1).
/// For match contexts, returns remaining bits (total_bits - bit_offset).
/// Raises `badarg` for a term that is not a bitstring; a guard turns that
/// raise into a failed test.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_size(ctx: *mut ProcessContext, bin: u64) -> u64 {
    match MatchContext::view(ErlTerm(bin)) {
        Some(window) => ErlTerm::small_int(window.remaining() as i64).0,
        None => raise_badarg(ctx),
    }
}

/// Get the BYTE size of a bitstring (byte_size/1), counting a partial final
/// byte. For match contexts, returns remaining bytes: (total_bits - bit_offset + 7) / 8.
/// Raises `badarg` for a term that is not a bitstring; a guard turns that
/// raise into a failed test.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_byte_size(ctx: *mut ProcessContext, bin: u64) -> u64 {
    match MatchContext::view(ErlTerm(bin)) {
        Some(window) => ErlTerm::small_int(window.remaining().div_ceil(8) as i64).0,
        None => raise_badarg(ctx),
    }
}

/// Initialize a binary match context (without allocating a match_ctx term).
/// Returns the raw (untagged) data pointer.
/// Stores remaining BIT count as a tagged small int in ctx.result.
/// Handles match_ctx inputs (reuse) for recursive binary matching.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bs_start_match(ctx: *mut ProcessContext, bin: u64) -> u64 {
    let Some(window) = MatchContext::view(ErlTerm(bin)) else {
        (*ctx).result = ErlTerm::small_int(0).0;
        return 0;
    };
    (*ctx).result = ErlTerm::small_int(window.remaining() as i64).0;
    binary_data(window.binary()).map_or(0, |(data, _)| data.add(window.offset() as usize / 8) as u64)
}

/// Start or reuse a validated matching context. No byte borrow crosses heap access.
/// # Safety
/// Context satisfies with_ctx_heap's contract. Input is a live immutable runtime
/// graph with valid matching invariants, unmoved through this call. Heap bump
/// allocation does not collect; the returned pointer must not survive a later GC.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bs_start_match2(ctx: *mut ProcessContext, input: u64) -> u64 {
    let Some(window) = MatchContext::view(ErlTerm(input)) else {
        (*ctx).result = ErlTerm::small_int(0).0;
        (*ctx).result2 = 0;
        return 0;
    };
    let (data, _) = binary_data(window.binary()).unwrap();
    let result = if window.binary().0 == input {
        with_ctx_heap(ctx, |heap| heap.alloc_match_ctx(window)).0
    } else { input };
    (*ctx).result = ErlTerm::small_int(window.remaining() as i64).0;
    (*ctx).result2 = result;
    data.add(window.offset() as usize / 8) as u64
}

/// Extract an integer from a raw data pointer (match context variant).
/// base_ptr is an untagged raw pointer to binary data.
/// bit_offset and bit_count are tagged small ints (in BITS).
/// flags: low 8 bits = signedness (0=unsigned, 1=signed), bits 8-15 = endianness (0=big, 1=little, 2=native)
/// # Safety
/// The caller establishes a nonnegative offset/count and readable initialized
/// backing through offset + count bits, live and immutable until decoding ends.
/// No input borrow survives destination heap allocation; ctx satisfies
/// with_ctx_heap's exclusive-access contract. Generated inline loads rely on
/// the same dominating pattern size checks.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_get_integer_raw(
    ctx: *mut ProcessContext,
    base_ptr: u64,
    bit_offset_term: u64,
    bit_count_term: u64,
    flags: u64,
) -> u64 {
    let Some(bit_off) = nonnegative_offset(bit_offset_term) else { return ErlTerm::nil().0; };
    let Some(bit_count) = nonnegative_offset(bit_count_term) else { return ErlTerm::nil().0; };
    let signedness = (flags & 0xFF) as u8;
    let endianness = ((flags >> 8) & 0xFF) as u8;
    if base_ptr == 0 || bit_off.checked_add(bit_count).is_none() { return ErlTerm::nil().0; }
    if bit_count == 0 { return ErlTerm::small_int(0).0; }
    if bit_count > 64 {
        return read_big_integer(ctx, base_ptr as *const u8, bit_off, bit_count, flags);
    }

    unsafe {
        let data_ptr = base_ptr as *const u8;

        // Fast path: byte-aligned
        if bit_off.is_multiple_of(8) && bit_count.is_multiple_of(8) {
            let offset = bit_off / 8;
            let count = bit_count / 8;
            if count > 8 {
                return ErlTerm::small_int(0).0;
            }
            let mut result: u64 = 0;
            match endianness {
                0 => {
                    for i in 0..count {
                        result = (result << 8) | (*data_ptr.add(offset + i) as u64);
                    }
                }
                1 => {
                    for i in (0..count).rev() {
                        result = (result << 8) | (*data_ptr.add(offset + i) as u64);
                    }
                }
                _ => {
                    let mut bytes = [0u8; 8];
                    for (i, item) in bytes[..count.min(8)].iter_mut().enumerate() {
                        *item = *data_ptr.add(offset + i);
                    }
                    result = i64::from_ne_bytes(bytes) as u64;
                }
            }
            if signedness == 1 && count < 8 {
                let bits = count * 8;
                let sign_bit = 1u64 << (bits - 1);
                if result & sign_bit != 0 {
                    result |= !((1u64 << bits) - 1);
                }
            }
            return binary_integer_term(ctx, result, signedness);
        }

        // Slow path: bit-level extraction
        let result = read_integer_bits(data_ptr, bit_off, bit_count, signedness, endianness);
        binary_integer_term(ctx, result, signedness)
    }
}

/// Decode a range established by the caller before allocating on the heap.
/// # Safety
/// `data` covers offset + bits initialized bits, live and immutable until the
/// owned BigInt has been built. No byte borrow survives process heap access.
unsafe fn read_big_integer(ctx: *mut ProcessContext, data: *const u8, offset: usize, bits: usize, flags: u64) -> u64 {
    let length = (offset % 8 + bits).div_ceil(8);
    let input = std::slice::from_raw_parts(data.add(offset / 8), length);
    let value = decode_big_integer(input, offset % 8, bits, flags);
    with_ctx_heap(ctx, |heap| crate::bigint::bigint_to_term(heap, value).0)
}

/// The adapter supplies the exact bounded byte window; indexing stays checked.
fn decode_big_integer(input: &[u8], offset: usize, bits: usize, flags: u64) -> num_bigint::BigInt {
    let little = (flags >> 8) & 0xff;
    let little = little == 1 || (little == 2 && cfg!(target_endian = "little"));
    let mut bytes = vec![0; bits.div_ceil(8)];
    let mut position = offset;
    for (i, byte) in bytes.iter_mut().enumerate() {
        let count = if little { (bits - i * 8).min(8) }
            else if i == 0 { (bits - 1) % 8 + 1 } else { 8 };
        // A chunk occupies one or two backing bytes. The slice bounds are
        // checked by ordinary Rust indexing and were established above.
        let first = input[position / 8] as u16;
        let shift = position % 8;
        let next = if shift + count > 8 { input[position / 8 + 1] as u16 } else { 0 };
        *byte = (((first << 8 | next) >> (16 - shift - count)) & ((1 << count) - 1)) as u8;
        position += count;
    }
    let mut value = if little { num_bigint::BigInt::from_bytes_le(num_bigint::Sign::Plus, &bytes) }
        else { num_bigint::BigInt::from_bytes_be(num_bigint::Sign::Plus, &bytes) };
    if flags & 0xff == 1 {
        let sign = if little { bytes[bytes.len() - 1] >> ((bits - 1) % 8) }
            else { bytes[0] >> ((bits - 1) % 8) };
        if sign & 1 != 0 { value -= num_bigint::BigInt::from(1) << bits; }
    }
    value
}

/// Materialize a sub-binary from a match context or plain binary.
/// bit_offset and bit_count are in BITS (tagged small ints).
///
/// When `orig_binary` is a match context, extracts the underlying binary
/// and computes the ABSOLUTE bit offset by adding the match context's
/// stored bit_offset to the relative `bit_offset_term`.
/// # Safety
/// Context/heap access obeys `with_ctx_heap`'s contract. The complete source
/// graph has initialized runtime layouts and live owners, and stays immutable
/// and unmoved through this call. Bump allocation does not collect or reenter;
/// no borrowed source bytes may overlap destination access.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bs_materialize(
    ctx: *mut ProcessContext,
    orig_binary: u64,
    bit_offset_term: u64,
    bit_count_term: u64,
) -> u64 {
    let term = ErlTerm(orig_binary);
    let Some(relative_offset) = ErlTerm(bit_offset_term).as_small_int().filter(|n| *n >= 0) else {
        return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
    };
    let Some(bit_count) = ErlTerm(bit_count_term).as_small_int().filter(|n| *n >= 0) else {
        return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
    };
    let Some(window) = MatchContext::view(term) else { return raise_badarg(ctx); };
    let count = bit_count as usize;
    let Some(offset) = window.resolve(relative_offset as usize, count) else { return raise_badarg(ctx); };
    let binary = window.binary();
    if offset.is_multiple_of(8) && count.is_multiple_of(8) {
        // Preserve the existing zero-copy path for aligned refcounted binaries.
        return nera_rt_binary_get_binary(ctx, binary.0,
            ErlTerm::small_int(offset as i64).0, bit_count_term);
    }

    let (data, _) = binary_data(binary).unwrap();
    let length = count.div_ceil(8);
    if length <= REFC_BINARY_THRESHOLD {
        with_ctx_heap(ctx, |heap| {
            // Allocation only bumps/adds stable arena storage; it cannot run
            // moving GC or reenter. No borrowed source slice spans this access.
            let result = heap.alloc_binary_zeroed(length);
            let target = result.as_boxed_ptr().unwrap().cast_mut();
            copy_bits(target.add(1).cast::<u8>(),0,data,offset,count);
            *target = BoxedHeader::new(BoxedSubtype::HeapBinary,count as u64).0;
            result.0
        })
    } else {
        // This Vec becomes the final owner, rather than being copied again.
        // Zero initialization keeps the unused bits in its last byte clear.
        let mut bytes = vec![0;length];
        copy_bits(bytes.as_mut_ptr(),0,data,offset,count);
        let owner = std::sync::Arc::new(BinaryData::new(bytes));
        let range = crate::binary::BinarySlice::new_bits(owner,0,count)
            .expect("materialized window fits its exact final buffer");
        with_ctx_heap(ctx, |heap| heap.alloc_binary_slice(range).0)
    }
}

/// erlang:integer_to_binary/1 — convert integer to binary representation.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_integer_to_binary(ctx: *mut ProcessContext, term: u64) -> u64 {
    let t = ErlTerm(term);
    let s = if let Some(v) = t.as_small_int() {
        v.to_string()
    } else if let Some(big) = crate::bigint::term_to_bigint(t) {
        big.to_string()
    } else {
        return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
    };
    with_ctx_heap(ctx, |heap| {
        heap.alloc_binary_auto(s.as_bytes()).0
    })
}

/// erlang:integer_to_binary/2 — convert integer to binary representation in given base.
/// Base must be 2..36.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_integer_to_binary_2(
    ctx: *mut ProcessContext,
    term: u64,
    base_term: u64,
) -> u64 {
    let base = match ErlTerm(base_term).as_small_int() {
        Some(b) if (2..=36).contains(&b) => b as u32,
        _ => return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
    };
    let t = ErlTerm(term);
    let s = if let Some(v) = t.as_small_int() {
        if v < 0 {
            // Negative: format magnitude then prepend '-'
            let mag = (-(v as i128)) as u128;
            format!("-{}", radix_fmt(mag, base))
        } else {
            radix_fmt(v as u128, base)
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
        return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
    };
    // Erlang uses uppercase hex digits (e.g. "FF" not "ff")
    let s = s.to_ascii_uppercase();
    with_ctx_heap(ctx, |heap| {
        heap.alloc_binary_auto(s.as_bytes()).0
    })
}

/// Format a u128 in the given radix (2..36) as a String.
pub(super) fn radix_fmt(mut val: u128, base: u32) -> String {
    if val == 0 {
        return "0".to_string();
    }
    const DIGITS: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut buf = Vec::new();
    let base = base as u128;
    while val > 0 {
        buf.push(DIGITS[(val % base) as usize]);
        val /= base;
    }
    buf.reverse();
    String::from_utf8(buf).unwrap()
}

/// Parse an optionally signed run of ASCII digits in `base` (2..=36).
pub(super) fn parse_integer_in_base(bytes: &[u8], base: u32) -> Option<num_bigint::BigInt> {
    let digits = match bytes.first() { Some(b'+' | b'-') => &bytes[1..], _ => bytes };
    if digits.is_empty() || !digits.iter().all(|byte| match byte {
        b'0'..=b'9' => u32::from(byte - b'0') < base,
        b'a'..=b'z' => u32::from(byte - b'a') + 10 < base,
        b'A'..=b'Z' => u32::from(byte - b'A') + 10 < base,
        _ => false,
    }) { return None; }
    num_bigint::BigInt::parse_bytes(bytes, base)
}

/// erlang:binary_to_integer/2 — parse a binary with an explicit base.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_to_integer_2(ctx: *mut ProcessContext, term: u64, base: u64) -> u64 {
    let Some(base @ 2..=36) = ErlTerm(base).as_small_int() else { return raise_badarg(ctx); };
    let input = ErlTerm(term);
    if !input.is_binary() { return raise_badarg(ctx); }
    let (pointer, length) = binary_data(input).unwrap();
    let bytes = std::slice::from_raw_parts(pointer, length);
    match parse_integer_in_base(bytes, base as u32) {
        Some(integer) => with_ctx_heap(ctx, |heap| crate::bigint::bigint_to_term(heap, integer).0),
        None => raise_badarg(ctx),
    }
}

/// erlang:list_to_integer/2 — parse a flat list of ASCII characters with an
/// explicit base, with the same grammar as binary_to_integer/2.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_list_to_integer_2(ctx: *mut ProcessContext, term: u64, base: u64) -> u64 {
    let Some(base @ 2..=36) = ErlTerm(base).as_small_int() else { return raise_badarg(ctx); };
    let mut bytes = Vec::new();
    let mut cursor = ErlTerm(term);
    while let Some(ptr) = cursor.as_cons_ptr() {
        match (*ptr).head.as_small_int() {
            Some(c @ 0..=127) => bytes.push(c as u8),
            _ => return raise_badarg(ctx),
        }
        cursor = (*ptr).tail;
    }
    if !cursor.is_nil() { return raise_badarg(ctx); }
    match parse_integer_in_base(&bytes, base as u32) {
        Some(integer) => with_ctx_heap(ctx, |heap| crate::bigint::bigint_to_term(heap, integer).0),
        None => raise_badarg(ctx),
    }
}

/// erlang:binary_to_integer/1 — parse binary as integer.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_to_integer(ctx: *mut ProcessContext, term: u64) -> u64 {
    let bytes = match unsafe { binary_data(ErlTerm(term)) } {
        Some((ptr, len)) => unsafe { std::slice::from_raw_parts(ptr, len) },
        None => return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
    };
    let s = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
    };
    // Try small int first (fast path)
    if let Ok(val) = s.parse::<i64>() {
        if crate::term::fits_small_int(val) {
            return ErlTerm::small_int(val).0;
        }
    }
    // Fall back to BigInt
    use num_bigint::BigInt;
    match s.parse::<BigInt>() {
        Ok(big) => with_ctx_heap(ctx, |heap| crate::bigint::bigint_to_term(heap, big).0),
        Err(_) => raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
    }
}

/// erlang:list_to_binary/1 — convert a list of bytes (or iolist) to a binary.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_list_to_binary(ctx: *mut ProcessContext, term: u64) -> u64 {
    let mut buf = Vec::new();
    if !flatten_iolist(ErlTerm(term), &mut buf) {
        return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
    }
    with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(&buf).0)
}

/// Count byte-sized leaves without copying binary payloads or recursing on the
/// host stack. Only list heads may be bytes; the root and tails must be lists
/// or binaries. The limit parameter makes the system-limit path testable.
///
/// # Safety
/// The complete root graph must be live and initialized for the heap borrow.
unsafe fn measure_iolist(
    root: ErlTerm, heap: &crate::heap::ProcessHeap, limit: usize,
) -> Result<usize, u32> {
    let mut pending = vec![(root, false)];
    let mut total = 0usize;
    while let Some((term, allow_byte)) = pending.pop() {
        if term.is_nil() { continue; }
        if let Some(ptr) = term.as_cons_ptr() {
            // No allocation or collection can move these cells while the
            // immutable heap borrow is held.
            let cell = unsafe { &*ptr };
            pending.push((cell.tail, false));
            pending.push((cell.head, true));
            continue;
        }
        let view = unsafe { term.borrow(heap) };
        let size = if view.is_binary() {
            view.binary_size().ok_or(crate::atom_table::ATOM_BADARG)?
        } else if allow_byte && term.as_small_int().is_some_and(|n| (0..=255).contains(&n)) {
            1
        } else {
            return Err(crate::atom_table::ATOM_BADARG);
        };
        total = total.checked_add(size).filter(|n| *n <= limit)
            .ok_or(crate::atom_table::ATOM_SYSTEM_LIMIT)?;
    }
    Ok(total)
}

/// erlang:iolist_size/1 — byte count of an iolist or binary.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_iolist_size(ctx: *mut ProcessContext, term: u64) -> u64 {
    let size = unsafe { with_ctx_heap(ctx, |heap| measure_iolist(ErlTerm(term), heap, usize::MAX)) };
    match size {
        Ok(n) => unsafe { with_ctx_heap(ctx, |heap|
            crate::bigint::bigint_to_term(heap, num_bigint::BigInt::from(n)).0) },
        Err(reason) => unsafe { raise_error(ctx, ErlTerm::atom(reason).0) },
    }
}

/// erlang:iolist_to_binary/1 — flatten an iolist to a single binary.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_iolist_to_binary(ctx: *mut ProcessContext, term: u64) -> u64 {
    let t = ErlTerm(term);
    // If already a binary, return as-is
    if t.is_binary() {
        return term;
    }
    let mut buf = Vec::new();
    if !flatten_iolist(t, &mut buf) {
        return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
    }
    with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(&buf).0)
}

/// Bits packed back to back; the bits after `bits` in the last byte stay zero.
#[derive(Default)]
struct PackedBits {
    bytes: Vec<u8>,
    bits: usize,
}

impl PackedBits {
    /// Append count bits read from the start of source.
    /// # Safety
    /// Source covers count initialized bits, readable for this call.
    unsafe fn append(&mut self, source: *const u8, count: usize) -> Result<(), u32> {
        let end = self.bits.checked_add(count).ok_or(crate::atom_table::ATOM_SYSTEM_LIMIT)?;
        self.bytes.resize(end.div_ceil(8), 0);
        copy_bits(self.bytes.as_mut_ptr(), self.bits, source, 0, count);
        self.bits = end;
        Ok(())
    }

    /// Allocate the packed bits as one bitstring: inline up to the
    /// reference-counted threshold, shared storage above it.
    fn into_term(mut self, heap: &mut crate::heap::ProcessHeap) -> Result<ErlTerm, u32> {
        if self.bytes.len() <= REFC_BINARY_THRESHOLD {
            let term = heap.alloc_binary(&self.bytes);
            // SAFETY: lower the exact bit length of this fresh descriptor; its
            // whole bytes stay allocated, and the bits after the end are zero.
            unsafe {
                let ptr = term.as_boxed_ptr().unwrap().cast_mut();
                *ptr = BoxedHeader(*ptr).with_arity(self.bits as u64).0;
            }
            return Ok(term);
        }
        self.bytes.shrink_to_fit();
        let bits = self.bits;
        crate::binary::BinarySlice::new_bits(std::sync::Arc::new(BinaryData::new(self.bytes)), 0, bits)
            .map(|slice| heap.alloc_binary_slice(slice))
            .ok_or(crate::atom_table::ATOM_SYSTEM_LIMIT)
    }
}

/// Pack a bitstring list at bit granularity, without recursing on the host
/// stack. List heads may be bytes, bitstrings or nested lists; tails may be
/// lists, [] or a bitstring. Any other element is badarg.
/// # Safety
/// The complete root graph is live and initialized, and nothing may move it
/// during this call. This function does not allocate on the process heap.
unsafe fn pack_bitstring_list(root: ErlTerm) -> Result<PackedBits, u32> {
    use crate::atom_table::ATOM_BADARG;
    let mut packed = PackedBits::default();
    // Tails still to read after a nested list ends.
    let mut outer_tails = Vec::new();
    let mut cursor = root;
    loop {
        while let Some(cell) = cursor.as_cons_ptr() {
            let (head, tail) = ((*cell).head, (*cell).tail);
            cursor = tail;
            if head.is_cons() {
                outer_tails.push(cursor);
                cursor = head;
            } else if let Some((data, bits)) = binary_data_bits(head) {
                packed.append(data, bits)?;
            } else if let Some(value) = head.as_small_int() {
                let byte = u8::try_from(value).map_err(|_| ATOM_BADARG)?;
                packed.append(&byte, 8)?;
            } else if !head.is_nil() {
                return Err(ATOM_BADARG);
            }
        }
        if let Some((data, bits)) = binary_data_bits(cursor) {
            packed.append(data, bits)?;
        } else if !cursor.is_nil() {
            return Err(ATOM_BADARG);
        }
        match outer_tails.pop() {
            Some(tail) => cursor = tail,
            None => return Ok(packed),
        }
    }
}

/// erlang:list_to_bitstring/1 — join bytes and bitstrings, nested in lists,
/// without padding partial bytes. Binary comprehensions also finish with
/// this call, so a body such as `<<X:4>>` or `<<B:1>>` packs exactly.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_list_to_bitstring(ctx: *mut ProcessContext, term: u64) -> u64 {
    let root = ErlTerm(term);
    if !root.is_cons() && !root.is_nil() {
        return raise_badarg(ctx);
    }
    let result = pack_bitstring_list(root)
        .and_then(|packed| with_ctx_heap(ctx, |heap| packed.into_term(heap)));
    match result {
        Ok(bitstring) => bitstring.0,
        Err(reason) => raise_error(ctx, ErlTerm::atom(reason).0),
    }
}

/// erlang:bitstring_to_list/1 — bytes followed by any remaining short bitstring.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bitstring_to_list(ctx: *mut ProcessContext, term: u64) -> u64 {
    let input = ErlTerm(term);
    let Some(bits) = input.binary_bit_size() else { return raise_badarg(ctx); };
    let (data, _) = binary_data(input).unwrap();
    let bytes = std::slice::from_raw_parts(data, bits.div_ceil(8)).to_vec();
    with_ctx_heap(ctx, |heap| {
        let mut elements: Vec<_> = bytes[..bits / 8].iter()
            .map(|byte| ErlTerm::small_int(*byte as i64)).collect();
        let remainder = bits % 8;
        if remainder != 0 {
            let tail = heap.alloc_binary(&[bytes[bits / 8] & (0xff << (8 - remainder))]);
            *tail.as_boxed_ptr().unwrap().cast_mut() = BoxedHeader::new(
                crate::term::BoxedSubtype::HeapBinary, remainder as u64).0;
            elements.push(tail);
        }
        heap.alloc_list_from_slice(&elements).0
    })
}

/// erlang:binary_to_list/1 — convert a binary to a list of byte integers.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_to_list(ctx: *mut ProcessContext, term: u64) -> u64 {
    // Read the descriptor once and validate its logical length before exposing
    // bytes: binary_data rounds partial bitstrings up to a complete byte.
    let (data, bits) = match unsafe { binary_data_bits(ErlTerm(term)) } {
        Some(parts) if parts.1.is_multiple_of(8) => parts,
        _ => return raise_badarg(ctx),
    };
    let bytes = unsafe { std::slice::from_raw_parts(data, bits / 8) };
    with_ctx_heap(ctx, |heap| {
        let elems: Vec<ErlTerm> = bytes.iter().map(|&b| ErlTerm::small_int(b as i64)).collect();
        heap.alloc_list_from_slice(&elems).0
    })
}

/// erlang:atom_to_binary/1 — convert atom to binary (UTF-8).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_atom_to_binary(ctx: *mut ProcessContext, term: u64) -> u64 {
    atom_to_binary_with(ctx, term, AtomTextEncoding::Utf8)
}

/// Shared body of `atom_to_binary/1,2`.
unsafe fn atom_to_binary_with(
    ctx: *mut ProcessContext,
    term: u64,
    encoding: AtomTextEncoding,
) -> u64 {
    let t = ErlTerm(term);
    let atom_id = match t.as_atom_id() {
        Some(id) => id,
        None => return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
    };
    let name = match global_atom_table().resolve(atom_id) {
        Some(s) => s,
        None => return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
    };
    match encoding {
        AtomTextEncoding::Utf8 => with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(name.as_bytes()).0),
        AtomTextEncoding::Latin1 => {
            let bytes: Result<Vec<u8>, _> = name.chars().map(|c| u8::try_from(c as u32)).collect();
            match bytes {
                Ok(bytes) => with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(&bytes).0),
                Err(_) => raise_badarg(ctx),
            }
        }
    }
}

/// erlang:atom_to_binary/2 — `Encoding` is `latin1`, `unicode` or `utf8`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_atom_to_binary_2(
    ctx: *mut ProcessContext,
    term: u64,
    encoding: u64,
) -> u64 {
    match AtomTextEncoding::from_term(ErlTerm(encoding)) {
        Some(encoding) => atom_to_binary_with(ctx, term, encoding),
        None => raise_badarg(ctx),
    }
}

/// erlang:float_to_binary/1 — convert float to binary using default formatting.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_float_to_binary(ctx: *mut ProcessContext, term: u64) -> u64 {
    let val = if ErlTerm(term).is_boxed() {
        unsafe {
            let ptr = match ErlTerm(term).as_boxed_ptr() {
                Some(p) => p,
                None => return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
            };
            let header = crate::term::BoxedHeader(*ptr);
            if header.subtype() != crate::term::BOXED_FLOAT {
                return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
            }
            f64::from_bits(*ptr.add(1))
        }
    } else {
        return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
    };
    let s = format!("{:.20e}", val);
    with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(s.as_bytes()).0)
}

/// erlang:float_to_binary/2 — convert float to binary with formatting options.
/// Supports: [short], [{decimals, N}], [{scientific, N}], [compact].
#[no_mangle]
pub unsafe extern "C" fn nera_rt_float_to_binary_2(
    ctx: *mut ProcessContext,
    term: u64,
    opts_term: u64,
) -> u64 {
    let val = if ErlTerm(term).is_boxed() {
        unsafe {
            let ptr = match ErlTerm(term).as_boxed_ptr() {
                Some(p) => p,
                None => return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
            };
            let header = crate::term::BoxedHeader(*ptr);
            if header.subtype() != crate::term::BOXED_FLOAT {
                return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
            }
            f64::from_bits(*ptr.add(1))
        }
    } else {
        return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
    };

    // Parse options list to determine formatting.
    // Options: short | compact | {decimals, N} | {scientific, N}
    let mut short = false;
    let mut compact = false;
    let mut decimals: Option<usize> = None;
    let mut scientific: Option<usize> = None;

    let mut cur = ErlTerm(opts_term);
    while !cur.is_nil() {
        if let Some(cell_ptr) = cur.as_cons_ptr() {
            let cell = unsafe { &*cell_ptr };
            let hd = cell.head;
            let tl = cell.tail;
            // Check for atom options: short, compact
            if let Some(atom_id) = hd.as_atom_id() {
                if let Some(name) = global_atom_table().resolve(atom_id) {
                    match name.as_str() {
                        "short" => short = true,
                        "compact" => compact = true,
                        _ => {}
                    }
                }
            }
            // Check for tuple options: {decimals, N}, {scientific, N}
            if hd.is_boxed() {
                if let Some(ptr) = hd.as_boxed_ptr() {
                    let header = unsafe { crate::term::BoxedHeader(*ptr) };
                    if header.subtype() == crate::term::BOXED_TUPLE && header.arity() == 2 {
                        let key = unsafe { ErlTerm(*ptr.add(1)) };
                        let value = unsafe { ErlTerm(*ptr.add(2)) };
                        if let (Some(kid), Some(n)) = (key.as_atom_id(), value.as_small_int()) {
                            if let Some(kname) = global_atom_table().resolve(kid) {
                                match kname.as_str() {
                                    "decimals" if n >= 0 => decimals = Some(n as usize),
                                    "scientific" if n >= 0 => scientific = Some(n as usize),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            cur = tl;
        } else {
            break;
        }
    }

    let s = if short {
        format_short_float(val)
    } else if let Some(d) = scientific {
        format!("{:.prec$e}", val, prec = d)
    } else if let Some(d) = decimals {
        let s = format!("{:.prec$}", val, prec = d);
        if compact {
            // Remove trailing zeros after decimal point, but keep at least one digit
            trim_trailing_zeros(&s)
        } else {
            s
        }
    } else {
        // Default: scientific notation with 20 digits (matches float_to_list/1)
        format!("{:.20e}", val)
    };

    with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(s.as_bytes()).0)
}

/// Erlang short formatting uses nearest, ties-to-even shortest digits, then
/// chooses the shorter notation (fixed on ties). At |value| >= 2^53 it always
/// uses scientific notation, even when a fixed spelling would be shorter.
fn format_short_float(value: f64) -> String {
    if value == 0.0 || !value.is_finite() {
        return if value == 0.0 {
            if value.is_sign_negative() {
                "-0.0"
            } else {
                "0.0"
            }
            .to_owned()
        } else {
            value.to_string()
        };
    }
    let mut buffer = ryu::Buffer::new();
    let raw = buffer.format_finite(value.abs());
    let (mantissa, exponent) = match raw.split_once('e') {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().unwrap()),
        None => (raw, 0),
    };
    let point = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let digits = mantissa.replace('.', "");
    let leading = digits.bytes().take_while(|&c| c == b'0').count();
    let exponent = exponent + point - leading as i32 - 1;
    let digits = digits[leading..].trim_end_matches('0');
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let fraction = if digits.len() == 1 { "0" } else { &digits[1..] };
    let scientific = format!("{sign}{}.{fraction}e{exponent}", &digits[..1]);
    if value.abs() >= 9_007_199_254_740_992.0 {
        return scientific;
    }
    let point = exponent + 1;
    let fixed = if point <= 0 {
        format!("{sign}0.{}{digits}", "0".repeat((-point) as usize))
    } else if point as usize >= digits.len() {
        format!(
            "{sign}{digits}{}.0",
            "0".repeat(point as usize - digits.len())
        )
    } else {
        let (whole, fraction) = digits.split_at(point as usize);
        format!("{sign}{whole}.{fraction}")
    };
    if scientific.len() < fixed.len() {
        scientific
    } else {
        fixed
    }
}

/// Trim trailing zeros from a decimal number string, keeping at least one digit after the dot.
fn trim_trailing_zeros(s: &str) -> String {
    if let Some(dot_pos) = s.find('.') {
        let trimmed = s.trim_end_matches('0');
        // Keep at least one digit after the dot
        if trimmed.len() <= dot_pos + 1 {
            s[..dot_pos + 2].to_string()
        } else {
            trimmed.to_string()
        }
    } else {
        s.to_string()
    }
}

/// OTP's limit on atom name length, in characters rather than bytes.
const MAX_ATOM_CHARACTERS: usize = 255;

/// Text encodings accepted by the atom/binary conversion BIFs.
#[derive(Clone, Copy)]
enum AtomTextEncoding {
    Latin1,
    Utf8,
}

impl AtomTextEncoding {
    /// Accept `latin1`, `unicode` or `utf8`; any other term is not an encoding.
    fn from_term(term: ErlTerm) -> Option<Self> {
        match global_atom_table().resolve(term.as_atom_id()?)?.as_str() {
            "latin1" => Some(Self::Latin1),
            "unicode" | "utf8" => Some(Self::Utf8),
            _ => None,
        }
    }
}

/// Decode an atom name: latin1 maps each byte to one character, utf8 must
/// be valid. Like OTP, input longer than any valid name (one byte per latin1
/// character, at most four per UTF-8 character) raises `system_limit` before
/// it is decoded or validated. Shorter invalid UTF-8 raises `badarg`.
fn decode_atom_name(
    bytes: &[u8],
    encoding: AtomTextEncoding,
) -> Result<std::borrow::Cow<'_, str>, u32> {
    let max_bytes = match encoding {
        AtomTextEncoding::Latin1 => MAX_ATOM_CHARACTERS,
        AtomTextEncoding::Utf8 => 4 * MAX_ATOM_CHARACTERS,
    };
    if bytes.len() > max_bytes {
        return Err(crate::atom_table::ATOM_SYSTEM_LIMIT);
    }
    let name: std::borrow::Cow<'_, str> = match encoding {
        AtomTextEncoding::Latin1 => bytes.iter().map(|&b| char::from(b)).collect(),
        AtomTextEncoding::Utf8 => std::str::from_utf8(bytes)
            .map_err(|_| crate::atom_table::ATOM_BADARG)?
            .into(),
    };
    if name.chars().count() > MAX_ATOM_CHARACTERS {
        return Err(crate::atom_table::ATOM_SYSTEM_LIMIT);
    }
    Ok(name)
}

/// Shared body of `binary_to_atom/1,2` and `binary_to_existing_atom/1,2`.
unsafe fn binary_to_atom_with(
    ctx: *mut ProcessContext,
    term: u64,
    encoding: AtomTextEncoding,
    existing_only: bool,
) -> u64 {
    // Read the exact bit length: a bitstring is not a binary, and
    // binary_data would round its partial last byte up.
    let bytes = match unsafe { binary_data_bits(ErlTerm(term)) } {
        Some((data, bits)) if bits.is_multiple_of(8) => unsafe {
            std::slice::from_raw_parts(data, bits / 8)
        },
        _ => return raise_badarg(ctx),
    };
    match decode_atom_name(bytes, encoding) {
        Ok(name) if existing_only => match global_atom_table().lookup_by_name(&name) {
            Some(id) => ErlTerm::atom(id).0,
            None => raise_badarg(ctx),
        },
        Ok(name) => ErlTerm::atom(global_atom_table().intern(&name)).0,
        // Existing-atom conversion never creates an atom; even an overlong
        // name raises badarg, not binary_to_atom's system_limit.
        Err(_) if existing_only => raise_badarg(ctx),
        Err(reason) => raise_error(ctx, ErlTerm::atom(reason).0),
    }
}

/// erlang:binary_to_atom/1 — same as `binary_to_atom(Binary, utf8)`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_to_atom(ctx: *mut ProcessContext, term: u64) -> u64 {
    binary_to_atom_with(ctx, term, AtomTextEncoding::Utf8, false)
}

/// erlang:binary_to_atom/2 — `Encoding` is `latin1`, `unicode` or `utf8`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_to_atom_2(
    ctx: *mut ProcessContext,
    term: u64,
    encoding: u64,
) -> u64 {
    match AtomTextEncoding::from_term(ErlTerm(encoding)) {
        Some(encoding) => binary_to_atom_with(ctx, term, encoding, false),
        None => raise_badarg(ctx),
    }
}

/// erlang:list_to_existing_atom/1 — convert list to atom only if it already exists.
/// Raises `badarg` if the atom is not already in the atom table.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_list_to_existing_atom(ctx: *mut ProcessContext, term: u64) -> u64 {
    let s = match bif::term_to_string(ErlTerm(term)) {
        Ok(s) => s,
        Err(()) => return raise_badarg(ctx),
    };
    match global_atom_table().lookup_by_name(&s) {
        Some(id) => ErlTerm::atom(id).0,
        None => raise_badarg(ctx),
    }
}

/// erlang:binary_to_existing_atom/1 — convert binary to atom only if it already exists.
/// Defaults to UTF-8 encoding.
/// Raises `badarg` if the atom is not already in the atom table.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_to_existing_atom_1(
    ctx: *mut ProcessContext,
    term: u64,
) -> u64 {
    binary_to_atom_with(ctx, term, AtomTextEncoding::Utf8, true)
}

/// erlang:binary_to_existing_atom/2 — `Encoding` is `latin1`, `unicode` or `utf8`.
/// Raises `badarg` for invalid input, overlong names or atoms that do not exist.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_to_existing_atom(
    ctx: *mut ProcessContext,
    term: u64,
    encoding: u64,
) -> u64 {
    match AtomTextEncoding::from_term(ErlTerm(encoding)) {
        Some(encoding) => binary_to_atom_with(ctx, term, encoding, true),
        None => raise_badarg(ctx),
    }
}

/// erlang:binary_to_float/1 — parse a binary string as a float.
/// The binary must represent a valid Erlang float literal (must contain a dot).
/// E.g. <<"3.14">> → 3.14, <<"1.0e10">> → 1.0e10.
/// Raises `badarg` if the binary is not a valid float representation.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_to_float(ctx: *mut ProcessContext, term: u64) -> u64 {
    let bytes = match unsafe { binary_data(ErlTerm(term)) } {
        Some((ptr, len)) => unsafe { std::slice::from_raw_parts(ptr, len) },
        None => return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
    };
    let s = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
    };
    // Erlang requires the string to contain a decimal point (pure integers raise badarg).
    if !s.contains('.') {
        return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
    }
    match s.parse::<f64>() {
        Ok(val) if val.is_finite() => {
            with_ctx_heap(ctx, |heap| heap.alloc_float(val).0)
        }
        _ => raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
    }
}

/// binary:split/2 — split binary at the first match of the pattern or patterns.
/// Returns a list of binaries.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_split_2(
    ctx: *mut ProcessContext,
    subject_term: u64,
    pattern_term: u64,
) -> u64 {
    // Delegate to split/3 with empty options list
    nera_rt_binary_split_3(ctx, subject_term, pattern_term, ErlTerm::nil().0)
}

/// binary:split/3 — split binary on a pattern or list of patterns with options.
/// Each split is at the earliest match, the longest pattern at that offset.
/// Options: `global` splits on all non-overlapping matches; `trim_all` removes
/// empty parts.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_split_3(
    ctx: *mut ProcessContext,
    subject_term: u64,
    pattern_term: u64,
    opts_term: u64,
) -> u64 {
    let subject_bytes = match unsafe { binary_data(ErlTerm(subject_term)) } {
        Some((ptr, len)) => unsafe { std::slice::from_raw_parts(ptr, len) },
        None => return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
    };
    let Some(patterns) = (unsafe { search_patterns(ErlTerm(pattern_term)) }) else {
        return raise_badarg(ctx);
    };

    // Read splitting and empty-field options independently of their order.
    let (global, trim_all) = {
        let mut g = false;
        let mut t = false;
        let mut cursor = ErlTerm(opts_term);
        while cursor.is_cons() {
            if let Some(ptr) = cursor.as_cons_ptr() {
                unsafe {
                    let elem = (*ptr).head;
                    if let Some(atom_id) = elem.as_atom_id() {
                        if let Some(name) = global_atom_table().resolve(atom_id) {
                            if name == "global" {
                                g = true;
                            } else if name == "trim_all" {
                                t = true;
                            }
                        }
                    }
                    cursor = (*ptr).tail;
                }
            } else {
                break;
            }
        }
        (g, t)
    };

    // Split at the first match, or at every match after the previous one with
    // `global`; the rest after the last match, possibly empty, ends the list.
    let mut parts: Vec<&[u8]> = Vec::new();
    let mut start = 0;
    while let Some((offset, length)) = first_match(&subject_bytes[start..], &patterns) {
        parts.push(&subject_bytes[start..start + offset]);
        start += offset + length;
        if !global {
            break;
        }
    }
    parts.push(&subject_bytes[start..]);

    if trim_all {
        parts.retain(|part| !part.is_empty());
    }

    // Build list of binaries
    with_ctx_heap(ctx, |heap| {
        let terms: Vec<ErlTerm> = parts.iter().map(|p| heap.alloc_binary_auto(p)).collect();
        heap.alloc_list_from_slice(&terms).0
    })
}

/// binary:part/3 — extract a sub-binary.
/// part(Binary, Pos, Len) returns binary from Pos with length Len.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_part(
    ctx: *mut ProcessContext,
    subject_term: u64,
    pos_term: u64,
    len_term: u64,
) -> u64 {
    let subject = ErlTerm(subject_term);
    if !unsafe { subject.is_binary() } {
        return raise_badarg(ctx);
    }
    let (data, size) = unsafe { binary_data(subject).unwrap() };
    let pos = match ErlTerm(pos_term).as_small_int() {
        Some(p) if p >= 0 => p as usize,
        _ => return raise_badarg(ctx),
    };
    let len = match ErlTerm(len_term).as_small_int() {
        Some(l) => l,
        _ => return raise_badarg(ctx),
    };
    let count = len.unsigned_abs() as usize;
    let start = if len < 0 {
        match pos.checked_sub(count) {
            Some(start) => start,
            None => return raise_badarg(ctx),
        }
    } else {
        pos
    };
    if start > size || count > size - start {
        return raise_badarg(ctx);
    }
    // Keep the original allocation alive, including when slicing an existing view.
    unsafe {
        let ptr = subject.as_boxed_ptr().unwrap();
        if BoxedHeader(*ptr).subtype() == BOXED_REFC_BINARY && count > REFC_BINARY_THRESHOLD {
            let offset = *ptr.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
            let backing = clone_arc_from_raw(*ptr.add(REFC_BINARY_PTR_WORD_INDEX));
            return with_ctx_heap(ctx, |heap| {
                heap.alloc_refc_binary_from_arc(backing, offset + start, count)
                    .0
            });
        }
        with_ctx_heap(ctx, |heap| {
            heap.alloc_binary_auto(std::slice::from_raw_parts(data.add(start), count))
                .0
        })
    }
}

/// Storage retained by a binary or bitstring, including the parent of a view.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_referenced_byte_size(
    ctx: *mut ProcessContext,
    subject_term: u64,
) -> u64 {
    let subject = ErlTerm(subject_term);
    if !unsafe { subject.is_bitstring() } {
        return raise_badarg(ctx);
    }
    let size = unsafe {
        let ptr = subject.as_boxed_ptr().unwrap();
        let header = BoxedHeader(*ptr);
        if header.subtype() == BOXED_REFC_BINARY {
            let backing = *ptr.add(REFC_BINARY_PTR_WORD_INDEX) as *const BinaryData;
            (&*backing).len()
        } else {
            // BEAM reports whole bytes for inline bitstrings.
            header.arity() as usize / 8
        }
    };
    unsafe { integer_term(ctx, size as i64) }
}

/// binary:at/2 — return the byte at position Pos in the binary.
/// Raises badarg if Pos is out of range.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_at(
    ctx: *mut ProcessContext,
    subject_term: u64,
    pos_term: u64,
) -> u64 {
    let bytes = match unsafe { binary_data(ErlTerm(subject_term)) } {
        Some((ptr, len)) => unsafe { std::slice::from_raw_parts(ptr, len) },
        None => return raise_badarg(ctx),
    };
    let pos = match ErlTerm(pos_term).as_small_int() {
        Some(p) if p >= 0 && (p as usize) < bytes.len() => p as usize,
        _ => return raise_badarg(ctx),
    };
    ErlTerm::small_int(bytes[pos] as i64).0
}

/// binary:last/1 — the final byte of a nonempty binary.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_last(ctx: *mut ProcessContext, subject: u64) -> u64 {
    let subject = ErlTerm(subject);
    if !subject.is_binary() { return raise_badarg(ctx); }
    let (data, len) = binary_data(subject).unwrap();
    if len == 0 { return raise_badarg(ctx); }
    ErlTerm::small_int(*data.add(len - 1) as i64).0
}

/// binary:longest_common_prefix/1 — byte count shared by every binary in a list.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_longest_common_prefix(
    ctx: *mut ProcessContext, subjects: u64,
) -> u64 {
    let mut cursor = ErlTerm(subjects);
    let mut first: Option<&[u8]> = None;
    let mut common = 0;
    while let Some(cons) = cursor.as_cons_ptr() {
        let subject = unsafe { (*cons).head };
        if !subject.is_binary() { return raise_badarg(ctx); }
        let (ptr, len) = unsafe { binary_data(subject).unwrap() };
        // No allocation, suspension or moving GC while these slices are borrowed.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        if let Some(prefix) = first {
            common = prefix[..common].iter().zip(bytes).take_while(|(a, b)| a == b).count();
        } else {
            first = Some(bytes);
            common = len;
        }
        // Validate the whole list even after the common prefix becomes empty.
        cursor = unsafe { (*cons).tail };
    }
    if !cursor.is_nil() || first.is_none() { return raise_badarg(ctx); }
    unsafe { integer_term(ctx, common as i64) }
}

/// binary:match/2 — earliest match, preferring the longest at the same offset.
/// Supports a binary or a nonempty proper list of nonempty binaries.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_match(
    ctx: *mut ProcessContext, subject: u64, pattern: u64,
) -> u64 {
    binary_match_in_scope(ctx, subject, pattern, None)
}

/// binary:match/3 — as match/2, restricted by a `{scope, {Start, Length}}`
/// option. A negative Length searches the `Length` bytes before Start. Offsets
/// in the result are relative to the whole subject. Unknown or malformed
/// options and a scope outside the subject raise badarg.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_match_3(
    ctx: *mut ProcessContext, subject: u64, pattern: u64, options: u64,
) -> u64 {
    let mut scope = None;
    let mut cursor = ErlTerm(options);
    if !cursor.is_nil() && !cursor.is_cons() { return raise_badarg(ctx); }
    while let Some(ptr) = cursor.as_cons_ptr() {
        let option = (*ptr).head;
        cursor = (*ptr).tail;
        let Some([key, range]) = pair_elements(option) else { return raise_badarg(ctx); };
        if key.as_atom_id() != global_atom_table().lookup_by_name("scope") { return raise_badarg(ctx); }
        let Some([start, length]) = pair_elements(range) else { return raise_badarg(ctx); };
        let (Some(start), Some(length)) = (start.as_small_int(), length.as_small_int())
            else { return raise_badarg(ctx); };
        scope = Some((start, length));
    }
    // BEAM ignores whatever follows the last cell of an improper option list.
    binary_match_in_scope(ctx, subject, pattern, scope)
}

/// The two elements of a 2-tuple term, or None for any other term.
unsafe fn pair_elements(term: ErlTerm) -> Option<[ErlTerm; 2]> {
    let ptr = term.as_boxed_ptr()?;
    let header = BoxedHeader(*ptr);
    if header.subtype() != BOXED_TUPLE || header.arity() != 2 { return None; }
    Some([ErlTerm(*ptr.add(1)), ErlTerm(*ptr.add(2))])
}

unsafe fn binary_match_in_scope(
    ctx: *mut ProcessContext, subject: u64, pattern: u64, scope: Option<(i64, i64)>,
) -> u64 {
    let subject = ErlTerm(subject);
    if !subject.is_binary() { return raise_badarg(ctx); }
    let (data, len) = binary_data(subject).unwrap();
    let whole = std::slice::from_raw_parts(data, len);
    let (base, bytes) = match scope {
        None => (0, whole),
        Some((start, length)) => {
            let end = start + length;
            let (low, high) = if length < 0 { (end, start) } else { (start, end) };
            if start < 0 || start as usize > len || low < 0 || high as usize > len {
                return raise_badarg(ctx);
            }
            (low as usize, &whole[low as usize..high as usize])
        }
    };
    // Validate every pattern, even after a match or for an empty subject.
    // No Erlang heap allocation occurs while any binary pointer is borrowed.
    let Some(patterns) = search_patterns(ErlTerm(pattern)) else { return raise_badarg(ctx) };
    match first_match(bytes, &patterns) {
        Some((offset, size)) => with_ctx_heap(ctx, |heap| heap.alloc_tuple(&[
            ErlTerm::small_int((base + offset) as i64), ErlTerm::small_int(size as i64),
        ]).0),
        None => ErlTerm::atom(global_atom_table().intern("nomatch")).0,
    }
}

/// binary:copy/1 — return a fresh copy of the binary.
/// Since Nera binaries are immutable, this is mainly useful to ensure
/// a binary is not a sub-binary referencing a larger allocation.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_copy_1(
    ctx: *mut ProcessContext,
    subject_term: u64,
) -> u64 {
    let bytes = match unsafe { binary_data(ErlTerm(subject_term)) } {
        Some((ptr, len)) => unsafe { std::slice::from_raw_parts(ptr, len) },
        None => return raise_badarg(ctx),
    };
    with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(bytes).0)
}

/// binary:copy/2 — return a binary consisting of Subject repeated N times.
/// copy(<<>>, _) → <<>>, copy(_, 0) → <<>>.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_copy_2(
    ctx: *mut ProcessContext,
    subject_term: u64,
    n_term: u64,
) -> u64 {
    let bytes = match unsafe { binary_data(ErlTerm(subject_term)) } {
        Some((ptr, len)) => unsafe { std::slice::from_raw_parts(ptr, len) },
        None => return raise_badarg(ctx),
    };
    let n = match ErlTerm(n_term).as_small_int() {
        Some(n) if n >= 0 => n as usize,
        _ => return raise_badarg(ctx),
    };
    if n == 0 || bytes.is_empty() {
        return with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(&[]).0);
    }
    let mut result = Vec::with_capacity(bytes.len() * n);
    for _ in 0..n {
        result.extend_from_slice(bytes);
    }
    with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(&result).0)
}

// =====================================================================
// UTF-8/16/32 binary segment operations
// =====================================================================

/// Validate the full tagged integer before narrowing to Unicode's scalar domain.
#[inline]
fn unicode_scalar(term: ErlTerm) -> Option<char> {
    let value = term.as_small_int()?;
    if !(0..=char::MAX as i64).contains(&value) { return None; }
    // Narrow only after validating the complete integer range.
    char::from_u32(value as u32)
}

#[inline]
fn nonnegative_offset(term: u64) -> Option<usize> {
    usize::try_from(ErlTerm(term).as_small_int()?).ok()
}

/// Write encoded bytes into the exact logical bit range, before exposing it.
/// # Safety
/// The context satisfies raise_badarg's contract. The binary is initialized,
/// live, unmoved and exclusively writable (including any refc backing storage)
/// until return. No GC or reentrant call may run while its bytes are borrowed.
#[inline(always)]
unsafe fn write_binary_bytes(ctx: *mut ProcessContext, bin: u64, offset: u64, bytes: &[u8]) -> u64 {
    let term = ErlTerm(bin);
    let Some(offset) = nonnegative_offset(offset) else { return raise_badarg(ctx); };
    let Some((data, total)) = binary_data_bits(term) else { return raise_badarg(ctx); };
    // Reject against logical bits, not the rounded backing byte length.
    if total.checked_sub(offset).is_none_or(|remaining| bytes.len() > remaining / 8) {
        return raise_badarg(ctx);
    }
    let length = total.div_ceil(8);
    let written = {
        // SAFETY: the caller guarantees exclusive access for this bounded borrow.
        let destination = std::slice::from_raw_parts_mut(data.cast_mut(), length);
        BitWriter::new(destination, total, offset).and_then(|mut writer| writer.write(bytes))
    };
    if written.is_none() { return raise_badarg(ctx); }
    ErlTerm::small_int((bytes.len() * 8) as i64).0
}

/// Encode one Unicode scalar into a binary at a bit offset.
/// # Safety
/// Requires write_binary_bytes's context and exclusive binary-access contract.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_put_utf8(
    ctx: *mut ProcessContext, bin: u64, bit_offset_term: u64, codepoint_term: u64,
) -> u64 {
    let Some(cp) = unicode_scalar(ErlTerm(codepoint_term)) else { return raise_badarg(ctx); };
    let mut buffer = [0; 4];
    write_binary_bytes(ctx, bin, bit_offset_term, cp.encode_utf8(&mut buffer).as_bytes())
}

/// UTF-16/32 flags bits 8..16 select big (0), little (1) or native (2)
/// byte order. Native is the target's order, as for integer segments.
fn utf_little_endian(flags: u64) -> bool {
    let endian = (flags >> 8) & 0xFF;
    endian == 1 || (endian == 2 && cfg!(target_endian = "little"))
}

/// Encode one Unicode scalar as UTF-16. flags bits 8..16 select endianness.
/// # Safety
/// Requires write_binary_bytes's context and exclusive binary-access contract.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_put_utf16(
    ctx: *mut ProcessContext, bin: u64, bit_offset_term: u64, codepoint_term: u64, flags: u64,
) -> u64 {
    let Some(cp) = unicode_scalar(ErlTerm(codepoint_term)) else { return raise_badarg(ctx); };
    let little = utf_little_endian(flags);
    if cp.len_utf16() == 1 {
        let unit = cp as u16;
        let bytes = if little { unit.to_le_bytes() } else { unit.to_be_bytes() };
        write_binary_bytes(ctx, bin, bit_offset_term, &bytes)
    } else {
        let mut units = [0; 2];
        cp.encode_utf16(&mut units);
        let high = if little { units[0].to_le_bytes() } else { units[0].to_be_bytes() };
        let low = if little { units[1].to_le_bytes() } else { units[1].to_be_bytes() };
        write_binary_bytes(ctx, bin, bit_offset_term, &[high[0], high[1], low[0], low[1]])
    }
}

/// Encode one Unicode scalar as UTF-32. flags bits 8..16 select endianness.
/// # Safety
/// Requires write_binary_bytes's context and exclusive binary-access contract.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_put_utf32(
    ctx: *mut ProcessContext, bin: u64, bit_offset_term: u64, codepoint_term: u64, flags: u64,
) -> u64 {
    let Some(cp) = unicode_scalar(ErlTerm(codepoint_term)) else { return raise_badarg(ctx); };
    let bytes = if utf_little_endian(flags) { (cp as u32).to_le_bytes() } else { (cp as u32).to_be_bytes() };
    write_binary_bytes(ctx, bin, bit_offset_term, &bytes)
}

/// Borrow initialized backing bytes only while decoding. Invalid ranges fail
/// before pointer access; the borrow cannot escape the callback.
/// # Safety
/// `base` is null or identifies total.div_ceil(8) readable, initialized bytes
/// in one allocation. They stay live, immutable and unmoved until return.
/// `ctx` is live, exclusive and disjoint from the bytes, and writable on success.
#[inline(always)]
unsafe fn get_utf(
    ctx: *mut ProcessContext, base: u64, offset: u64, total: u64,
    decode: impl for<'a> FnOnce(BitReader<'a>) -> Option<(char, usize)>,
) -> u64 {
    let failure = ErlTerm::small_int(0).0;
    let (Some(offset), Some(total)) = (nonnegative_offset(offset), nonnegative_offset(total)) else {
        return failure;
    };
    let Some(remaining) = total.checked_sub(offset) else { return failure; };
    if base == 0 || remaining < 8 { return failure; }
    let shift = (offset % 8) as u32;
    let length = remaining / 8 + usize::from(shift != 0);
    if length > isize::MAX as usize { return failure; }
    let result = {
        // SAFETY: offset <= total and this window contains only complete
        // logical bytes plus a carry byte. Both fit the caller's allocation.
        // Its scoped borrow cannot escape the decoder callback.
        let start = (base as *const u8).add(offset / 8);
        let bytes = std::slice::from_raw_parts(start, length);
        BitReader::new(bytes, shift).and_then(decode)
    };
    let Some((cp, bits)) = result else { return failure; };
    (*ctx).result = ErlTerm::small_int(cp as i64).0;
    ErlTerm::small_int(bits as i64).0
}

fn decode_utf8(reader: BitReader<'_>) -> Option<(char, usize)> {
    let [first] = reader.read::<1>()?;
    let (cp, min, bits) = match first {
        0..=0x7F => return Some((first as char, 8)),
        0xC2..=0xDF => {
            let [_, b] = reader.read::<2>()?;
            if b & 0xC0 != 0x80 { return None; }
            (((first & 0x1F) as u32) << 6 | (b & 0x3F) as u32, 0x80, 16)
        }
        0xE0..=0xEF => {
            let [_, b, c] = reader.read::<3>()?;
            if b & 0xC0 != 0x80 || c & 0xC0 != 0x80 { return None; }
            (((first & 0xF) as u32) << 12 | ((b & 0x3F) as u32) << 6 | (c & 0x3F) as u32, 0x800, 24)
        }
        0xF0..=0xF4 => {
            let [_, b, c, d] = reader.read::<4>()?;
            if b & 0xC0 != 0x80 || c & 0xC0 != 0x80 || d & 0xC0 != 0x80 { return None; }
            (((first & 7) as u32) << 18 | ((b & 0x3F) as u32) << 12
                | ((c & 0x3F) as u32) << 6 | (d & 0x3F) as u32, 0x10000, 32)
        }
        _ => return None,
    };
    if cp < min { return None; }
    Some((char::from_u32(cp)?, bits))
}

fn decode_utf16(reader: BitReader<'_>, little: bool) -> Option<(char, usize)> {
    let pair = reader.read::<2>()?;
    let first = if little { u16::from_le_bytes(pair) } else { u16::from_be_bytes(pair) };
    let cp = if (0xD800..=0xDBFF).contains(&first) {
        let [_, _, a, b] = reader.read::<4>()?;
        let second = if little { u16::from_le_bytes([a, b]) } else { u16::from_be_bytes([a, b]) };
        if !(0xDC00..=0xDFFF).contains(&second) { return None; }
        let cp = 0x10000 + (((first as u32 - 0xD800) << 10) | (second as u32 - 0xDC00));
        return Some((char::from_u32(cp)?, 32));
    } else { first as u32 };
    Some((char::from_u32(cp)?, 16))
}

/// Decode UTF-8 at a bit offset. Zero means match failure; success sets ctx.result.
/// # Safety
/// Requires get_utf's backing-storage and context contract.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bs_get_utf8(
    ctx: *mut ProcessContext, base_ptr: u64, bit_offset_term: u64, total_bits_term: u64,
) -> u64 {
    get_utf(ctx, base_ptr, bit_offset_term, total_bits_term, decode_utf8)
}

/// Decode UTF-16 at a bit offset. Zero means match failure; success sets ctx.result.
/// # Safety
/// Requires get_utf's backing-storage and context contract.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bs_get_utf16(
    ctx: *mut ProcessContext, base_ptr: u64, bit_offset_term: u64, total_bits_term: u64, flags: u64,
) -> u64 {
    get_utf(ctx, base_ptr, bit_offset_term, total_bits_term,
        |reader| decode_utf16(reader, utf_little_endian(flags)))
}

/// Decode UTF-32 at a bit offset. Zero means match failure; success sets ctx.result.
/// # Safety
/// Requires get_utf's backing-storage and context contract.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_bs_get_utf32(
    ctx: *mut ProcessContext, base_ptr: u64, bit_offset_term: u64, total_bits_term: u64, flags: u64,
) -> u64 {
    get_utf(ctx, base_ptr, bit_offset_term, total_bits_term, |reader| {
        let bytes = reader.read::<4>()?;
        let cp = if utf_little_endian(flags) { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) };
        Some((char::from_u32(cp)?, 32))
    })
}

/// Resize a binary to a smaller actual size after variable-width UTF writes.
/// `new_bit_size_term` is a tagged small int representing the new **bit** count.
/// The binary must have been allocated with at least this many bits.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_resize(
    _ctx: *mut ProcessContext,
    bin: u64,
    new_bit_size_term: u64,
) {
    let bin_term = ErlTerm(bin);
    let new_bit_size = ErlTerm(new_bit_size_term).as_small_int().unwrap_or(0) as usize;

    unsafe {
        if let Some(ptr) = bin_term.as_boxed_ptr() {
            let header = BoxedHeader(*ptr);
            let old_bit_size = header.arity() as usize;
            if new_bit_size <= old_bit_size {
                // Update the header with the new bit size, keeping the same subtype
                let ptr_mut = ptr as *mut u64;
                *ptr_mut = header.with_arity(new_bit_size as u64).0;
            }
        }
    }
}


#[cfg(test)]
mod integer_domain_tests {
    use super::*;

    // Independent bit-by-bit oracle deliberately does not use runtime bit helpers.
    fn splice_bits(bytes: &mut [u8], offset: usize, encoded: &[u8]) {
        for (i, byte) in encoded.iter().enumerate() {
            for bit in 0..8 {
                let dest = offset + i * 8 + bit;
                let mask = 1 << (7 - dest % 8);
                bytes[dest / 8] = (bytes[dest / 8] & !mask)
                    | (((byte >> (7 - bit)) & 1) << (7 - dest % 8));
            }
        }
    }

    unsafe fn put_unicode(ctx: *mut ProcessContext, bin: ErlTerm, offset: ErlTerm,
                          cp: ErlTerm, kind: usize, endian: u64) -> u64 {
        match kind {
            8 => nera_rt_binary_put_utf8(ctx, bin.0, offset.0, cp.0),
            16 => nera_rt_binary_put_utf16(ctx, bin.0, offset.0, cp.0, endian << 8),
            _ => nera_rt_binary_put_utf32(ctx, bin.0, offset.0, cp.0, endian << 8),
        }
    }

    unsafe fn get_unicode(ctx: *mut ProcessContext, data: *const u8, offset: ErlTerm,
                          total: ErlTerm, kind: usize, endian: u64) -> u64 {
        match kind {
            8 => nera_rt_bs_get_utf8(ctx, data as u64, offset.0, total.0),
            16 => nera_rt_bs_get_utf16(ctx, data as u64, offset.0, total.0, endian << 8),
            _ => nera_rt_bs_get_utf32(ctx, data as u64, offset.0, total.0, endian << 8),
        }
    }

    #[test]
    fn unicode_rejects_truncation_and_invalid_scalars_without_writing() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        let bin = heap.alloc_binary(&[0xA5; 8]);
        let float = heap.alloc_float(65.0);
        let big = crate::bigint::bigint_to_term(&mut heap, (1_u128 << 80).into());
        for kind in [8, 16, 32] {
            for value in [-1, 0xD800, 0xDFFF, 0x110000, 0x1_0000_0041, SMALL_INT_MAX]
                .map(ErlTerm::small_int).into_iter().chain([float, big, ErlTerm::nil()]) {
                ctx.result = ErlTerm::nil().0;
                unsafe { put_unicode(&mut ctx, bin, ErlTerm::small_int(0), value, kind, 0); }
                assert_eq!(ctx.result, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0,
                           "kind={kind}, value={value:?}");
                assert_eq!(unsafe { bin.binary_bytes_any().unwrap() }, &[0xA5; 8]);
            }
        }
    }

    #[test]
    fn unicode_bit_windows_preserve_neighbors_and_decode_at_every_offset() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        for cp in ['\0', 'A', '\u{7FF}', '\u{800}', '\u{D7FF}', '\u{E000}', '\u{FFFF}', '\u{10000}', '\u{10FFFF}'] {
            for kind in [8, 16, 32] {
                // NERA-104: native (2) is the target's byte order, as on BEAM.
                for endian in 0..=2 {
                    let little = endian == 1 || (endian == 2 && cfg!(target_endian = "little"));
                    let encoded = match kind {
                        8 => cp.encode_utf8(&mut [0; 4]).as_bytes().to_vec(),
                        16 => cp.encode_utf16(&mut [0; 2]).iter().flat_map(|n|
                            if little { n.to_le_bytes() } else { n.to_be_bytes() }).collect(),
                        _ => if little { (cp as u32).to_le_bytes().to_vec() }
                             else { (cp as u32).to_be_bytes().to_vec() },
                    };
                    for offset in 0..8 {
                        let mut expected = [0xA5; 8];
                        splice_bits(&mut expected, offset, &encoded);
                        let bin = heap.alloc_binary(&[0xA5; 8]);
                        let off = ErlTerm::small_int(offset as i64);
                        let bits = (encoded.len() * 8) as i64;
                        // SAFETY: heap and oracle storage stay live, exclusive and unmoved.
                        unsafe {
                            let written = put_unicode(&mut ctx, bin, off,
                                ErlTerm::small_int(cp as i64), kind, endian);
                            assert_eq!(written, ErlTerm::small_int(bits).0);
                            assert_eq!(bin.binary_bytes_any().unwrap(), &expected,
                                       "kind={kind}, endian={endian}, offset={offset}, cp={cp:?}");
                            let result = get_unicode(&mut ctx, expected.as_ptr(), off,
                                ErlTerm::small_int(offset as i64 + bits), kind, endian);
                            assert_eq!(result, written);
                            assert_eq!(ctx.result, ErlTerm::small_int(cp as i64).0);
                            for available in 0..bits {
                                assert_eq!(get_unicode(&mut ctx, expected.as_ptr(), off,
                                    ErlTerm::small_int(offset as i64 + available), kind, endian),
                                    ErlTerm::small_int(0).0);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn unicode_bit_windows_reject_invalid_ranges_before_access() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        let bin = heap.alloc_binary(&[0xA5; 4]);
        for kind in [8, 16, 32] {
            for offset in [ErlTerm::small_int(-1), ErlTerm::small_int(SMALL_INT_MAX),
                           ErlTerm::nil(), ErlTerm::small_int(33), ErlTerm::small_int(32)] {
                unsafe {
                    ctx.result = ErlTerm::nil().0;
                    put_unicode(&mut ctx, bin, offset, ErlTerm::small_int(65), kind, 0);
                    assert_eq!(ctx.result, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
                    assert_eq!(bin.binary_bytes_any().unwrap(), &[0xA5; 4]);
                    assert_eq!(get_unicode(&mut ctx, [0u8; 4].as_ptr(), offset,
                        ErlTerm::small_int(32), kind, 0), ErlTerm::small_int(0).0);
                }
            }
        }
        // Logical length, not byte rounding, bounds writes.
        unsafe {
            let bin = ErlTerm(nera_rt_alloc_binary(&mut ctx, ErlTerm::small_int(7).0));
            // null heap pointer uses the live thread-local heap; no collection occurs.
            nera_rt_binary_put_utf8(&mut ctx, bin.0, ErlTerm::small_int(0).0,
                                    ErlTerm::small_int(65).0);
            assert_eq!(ctx.result, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
            assert_eq!(bin.binary_bytes_any().unwrap(), &[0]);
        }
    }

    #[test]
    fn unicode_decoders_reject_malformed_encodings_without_publishing_a_result() {
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        let cases: &[(usize, &[u8])] = &[
            (8, &[0x80]), (8, &[0xC0, 0x80]), (8, &[0xC2, 0]),
            (8, &[0xE0, 0x80, 0x80]), (8, &[0xED, 0xA0, 0x80]),
            (8, &[0xF0, 0x80, 0x80, 0x80]), (8, &[0xF4, 0x90, 0x80, 0x80]),
            (8, &[0xF5, 0x80, 0x80, 0x80]), (8, &[0xFF]),
            (16, &[0xD8, 0]), (16, &[0xDC, 0]), (16, &[0xD8, 0, 0, 65]),
            (32, &[0, 0, 0xD8, 0]), (32, &[0, 0x11, 0, 0]),
        ];
        for &(kind, bytes) in cases {
            for offset in 0..8 {
                let mut data = [0xA5; 5];
                splice_bits(&mut data, offset, bytes);
                ctx.result = ErlTerm::nil().0;
                // SAFETY: this stack allocation contains every declared bit,
                // remains immutable and live, and is disjoint from the context.
                unsafe {
                    assert_eq!(get_unicode(&mut ctx, data.as_ptr(),
                        ErlTerm::small_int(offset as i64),
                        ErlTerm::small_int((offset + bytes.len() * 8) as i64), kind, 0),
                        ErlTerm::small_int(0).0);
                }
                assert_eq!(ctx.result, ErlTerm::nil().0);
            }
        }
    }

    #[test]
    fn float_segments_reject_bad_domains_without_writing_or_allocating() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let bin = heap.alloc_binary(&[0xA5; 16]);
        let one = heap.alloc_float(1.0);
        let before = heap.bytes_allocated();
        unsafe {
            for value in [ErlTerm::nil(), ErlTerm::atom(crate::atom_table::ATOM_BADARG)] {
                ctx.result = ErlTerm::nil().0;
                nera_rt_binary_put_float(&mut ctx, bin.0, ErlTerm::small_int(0).0,
                    value.0, ErlTerm::small_int(64).0, 0);
                assert_eq!(ctx.result, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
                assert_eq!(bin.binary_bytes_any().unwrap(), &[0xA5; 16]);
            }
            for bits in [-1, 0, 8, 24, 48, 65, SMALL_INT_MAX] {
                ctx.result = ErlTerm::nil().0;
                nera_rt_binary_put_float(&mut ctx, bin.0, ErlTerm::small_int(0).0,
                    one.0, ErlTerm::small_int(bits).0, 0);
                assert_eq!(ctx.result, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
                assert_eq!(bin.binary_bytes_any().unwrap(), &[0xA5; 16]);
                assert_eq!(nera_rt_binary_get_float(&mut ctx, bin.0,
                    ErlTerm::small_int(0).0, ErlTerm::small_int(bits).0, 0), ErlTerm::nil().0);
            }
        }
        assert_eq!(heap.bytes_allocated(), before);
    }

    #[test]
    fn float_segments_preserve_neighbors_and_roundtrip_at_every_bit_offset() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        for value in [0.0_f64, -0.0, 1.0, -2.5, 65504.0] {
            let term = heap.alloc_float(value);
            for bits in [16, 32, 64] {
                for little in [false, true] {
                    let bytes = match bits {
                        16 => {
                            let raw: u16 = match value {
                                v if v.to_bits() == (-0.0_f64).to_bits() => 0x8000,
                                0.0 => 0, 1.0 => 0x3C00, -2.5 => 0xC100,
                                _ => 0x7BFF,
                            };
                            if little { raw.to_le_bytes().to_vec() } else { raw.to_be_bytes().to_vec() }
                        }
                        32 => if little { (value as f32).to_le_bytes().to_vec() }
                              else { (value as f32).to_be_bytes().to_vec() },
                        _ => if little { value.to_le_bytes().to_vec() } else { value.to_be_bytes().to_vec() },
                    };
                    for offset in 0..8 {
                        let mut expected = [0xA5; 16];
                        splice_bits(&mut expected, offset, &bytes);
                        let bin = heap.alloc_binary(&[0xA5; 16]);
                        unsafe {
                            nera_rt_binary_put_float(&mut ctx, bin.0,
                                ErlTerm::small_int(offset as i64).0, term.0,
                                ErlTerm::small_int(bits).0, (little as u64) << 8);
                            assert_eq!(bin.binary_bytes_any().unwrap(), &expected,
                                "bits={bits}, offset={offset}, value={value}");
                            let input = heap.alloc_binary(&expected);
                            let decoded = ErlTerm(nera_rt_binary_get_float(&mut ctx, input.0,
                                ErlTerm::small_int(offset as i64).0, ErlTerm::small_int(bits).0,
                                (little as u64) << 8));
                            assert_eq!(term_as_float(decoded).unwrap().to_bits(), value.to_bits());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn float_segments_respect_match_context_logical_end() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let binary = heap.alloc_binary(&[0; 16]);
        for width in [16, 32, 64] {
            for offset in 0..8 {
                let cursor = heap.alloc_match_ctx(unsafe { MatchContext::new(binary, offset, offset + width - 1) }.unwrap());
                let before = heap.bytes_allocated();
                // SAFETY: initialized backing/context are live and immutable;
                // the shortened logical window must fail before allocation.
                let result = unsafe { nera_rt_binary_get_float(&mut ctx, cursor.0,
                    ErlTerm::small_int(0).0, ErlTerm::small_int(width as i64).0, 0) };
                assert_eq!(result, ErlTerm::nil().0);
                assert_eq!(heap.bytes_allocated(), before);
            }
        }
    }

    #[test]
    fn float_segments_reject_nonfinite_and_truncated_reads_without_allocation() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        for bytes in [&[0x7C,0][..], &[0x7E,0], &[0x7F,0x80,0,0],
                      &[0x7F,0xC0,0,0], &[0x7F,0xF0,0,0,0,0,0,0],
                      &[0x7F,0xF8,0,0,0,0,0,0]] {
            let bin = heap.alloc_binary(bytes);
            let before = heap.bytes_allocated();
            unsafe {
                assert_eq!(nera_rt_binary_get_float(&mut ctx, bin.0,
                    ErlTerm::small_int(0).0, ErlTerm::small_int((bytes.len()*8) as i64).0, 0),
                    ErlTerm::nil().0);
            }
            assert_eq!(heap.bytes_allocated(), before);
        }
        let one = heap.alloc_float(1.0);
        for bits in [16,32,64] {
            for offset in 0..8 {
                unsafe {
                    let bin = ErlTerm(nera_rt_alloc_binary(&mut ctx,
                        ErlTerm::small_int(bits+offset-1).0));
                    let before = heap.bytes_allocated();
                    ctx.result = ErlTerm::nil().0;
                    nera_rt_binary_put_float(&mut ctx, bin.0, ErlTerm::small_int(offset).0,
                        one.0, ErlTerm::small_int(bits).0, 0);
                    assert_eq!(ctx.result, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
                    assert!(bin.binary_bytes_any().unwrap().iter().all(|b| *b==0));
                    assert_eq!(nera_rt_binary_get_float(&mut ctx, bin.0,
                        ErlTerm::small_int(offset).0, ErlTerm::small_int(bits).0, 0), ErlTerm::nil().0);
                    assert_eq!(heap.bytes_allocated(), before);
                }
            }
        }
    }

    #[test]
    fn segment_sizes_reject_invalid_domains_before_allocation() {
        use crate::atom_table::{ATOM_BADARG, ATOM_SYSTEM_LIMIT};
        assert_eq!(SegmentBits::new(-1, 8), Err(ATOM_BADARG));
        assert_eq!(SegmentBits::new(0, 256), Ok(SegmentBits(0)));
        assert_eq!(SegmentBits::new(3, 8), Ok(SegmentBits(24)));
        assert_eq!(SegmentBits::new(SMALL_INT_MAX, 256), Err(ATOM_SYSTEM_LIMIT));
        assert_eq!(SegmentBits::new(SMALL_INT_MAX, 1),
            usize::try_from(SMALL_INT_MAX).map(SegmentBits).map_err(|_| ATOM_SYSTEM_LIMIT));
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let positive_big = crate::bigint::bigint_to_term(&mut heap, (SMALL_INT_MAX as i128 + 1).into());
        let negative_big = crate::bigint::bigint_to_term(&mut heap, (SMALL_INT_MIN as i128 - 1).into());
        let float = heap.alloc_float(1.5);
        let before = heap.bytes_allocated();
        // SAFETY: the context is exclusive, all graphs are live, no GC occurs.
        unsafe {
            for (input, reason) in [(float, ATOM_BADARG), (ErlTerm::nil(), ATOM_BADARG),
                (positive_big, ATOM_SYSTEM_LIMIT), (negative_big, ATOM_BADARG)] {
                assert_eq!(segment_bits(input, 1), Err(reason));
                nera_rt_alloc_binary(&mut ctx, input.0);
                assert_eq!(ctx.result, ErlTerm::atom(reason).0);
            }
        }
        // Rounding the largest tagged bit count to bytes would overflow the
        // intermediate whole-byte descriptor; reject it without allocating.
        unsafe { nera_rt_alloc_binary(&mut ctx, ErlTerm::small_int(SMALL_INT_MAX).0); }
        assert_eq!(ctx.result, ErlTerm::atom(ATOM_SYSTEM_LIMIT).0);
        assert_eq!(heap.bytes_allocated(), before);
    }

    #[test]
    fn boxed_binary_resize_preserves_subtype_and_rejects_growth() {
        let mut heap = crate::heap::ProcessHeap::new();
        let binaries = [heap.alloc_binary(&[42; 16]), heap.alloc_binary_auto(&[42; 256])];
        for binary in binaries {
            // SAFETY: the heap owns these initialized binary allocations throughout
            // the calls; resizing only shrinks their header, without allocation or GC.
            unsafe {
                let ptr = binary.as_boxed_ptr().unwrap();
                let original = BoxedHeader(*ptr);
                nera_rt_binary_resize(std::ptr::null_mut(), binary.0, ErlTerm::small_int(64).0);
                assert_eq!(BoxedHeader(*ptr).subtype(), original.subtype());
                assert_eq!(BoxedHeader(*ptr).arity(), 64);
                assert_eq!(binary.binary_bytes_any().unwrap(), &[42; 8]);
                nera_rt_binary_resize(std::ptr::null_mut(), binary.0, ErlTerm::small_int(128).0);
                assert_eq!(BoxedHeader(*ptr).arity(), 64);
            }
        }
    }

    #[cfg(not(target_env = "msvc"))]
    #[test]
    fn shifted_binary_copies_allocate_no_temporary_storage() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let binary = heap.alloc_binary_auto(&[0xa5;256]);
        let allocated = tikv_jemalloc_ctl::thread::allocatedp::read().unwrap();
        let before = allocated.get();
        for _ in 0..100 {
            for bits in [0,13,128,2045] {
                let result = unsafe { nera_rt_binary_put_binary(&mut ctx,binary.0,
                    ErlTerm::small_int(3).0,binary.0,ErlTerm::small_int(bits).0) };
                std::hint::black_box(result);
            }
        }
        assert_eq!(allocated.get(),before);
    }

    #[test]
    fn materialize_respects_context_end_before_allocation() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let binary = heap.alloc_binary(&[0xa5; 16]);
        let cursor = heap.alloc_match_ctx(unsafe { MatchContext::new(binary, 8, 24) }.unwrap());
        let before = heap.bytes_allocated();
        // SAFETY: valid live input; requested bytes exceed the cursor window,
        // but remain within backing storage, so the old behavior fails safely.
        let result = unsafe { nera_rt_bs_materialize(&mut ctx, cursor.0,
            ErlTerm::small_int(0).0, ErlTerm::small_int(24).0) };
        assert_eq!(result, ErlTerm::nil().0);
        assert_eq!(ctx.result, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
        assert_eq!(heap.bytes_allocated(), before);
    }

    #[test]
    fn integer_writes_validate_values_zero_width_and_exact_logical_bounds() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let bin = heap.alloc_binary(&[0xa5; 2]);
        unsafe {
            for bits in [0, 8] {
                assert_eq!(nera_rt_binary_put_integer(&mut ctx, bin.0,
                    ErlTerm::small_int(0).0, ErlTerm::nil().0, ErlTerm::small_int(bits).0, 0), ErlTerm::nil().0);
                assert_eq!(ctx.result, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
                assert_eq!(bin.binary_bytes_any().unwrap(), &[0xa5; 2]);
            }
            let short = ErlTerm(nera_rt_alloc_binary(&mut ctx, ErlTerm::small_int(7).0));
            let before = heap.bytes_allocated();
            assert_eq!(nera_rt_binary_put_integer(&mut ctx, short.0, ErlTerm::small_int(0).0,
                ErlTerm::small_int(255).0, ErlTerm::small_int(8).0, 0), ErlTerm::nil().0);
            assert_eq!(short.binary_bytes_any().unwrap(), &[0]);
            assert_eq!(nera_rt_binary_get_integer(&mut ctx, short.0, ErlTerm::small_int(0).0,
                ErlTerm::small_int(8).0, 0), ErlTerm::nil().0);
            assert_eq!(heap.bytes_allocated(), before);
        }
    }

    #[test]
    fn binary_writes_reject_invalid_sources_without_touching_destination() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let bin = heap.alloc_binary(&[0xa5; 2]);
        let source = heap.alloc_binary(&[42]);
        unsafe {
            for (input, count) in [(ErlTerm::nil(), 0), (ErlTerm::nil(), 8), (source, 16)] {
                assert_eq!(nera_rt_binary_put_binary(&mut ctx, bin.0, ErlTerm::small_int(0).0,
                    input.0, ErlTerm::small_int(count).0), ErlTerm::nil().0);
                assert_eq!(ctx.result, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
                assert_eq!(bin.binary_bytes_any().unwrap(), &[0xa5; 2]);
            }
        }
    }

    #[test]
    fn integer_decoding_matches_bit_oracle_for_all_widths_and_offsets() {
        use num_bigint::BigInt;
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let bytes: Vec<u8> = (0..24).map(|i| (i * 71 + 0xa5) as u8).collect();
        let binary = heap.alloc_binary(&bytes);
        for bits in 0..=160 {
            for offset in 0..8 {
                for endian in 0..=2 {
                    let little = endian == 1 || (endian == 2 && cfg!(target_endian = "little"));
                    let mut unsigned = BigInt::from(0);
                    for wire in 0..bits {
                        let source = offset + wire;
                        let bit = (bytes[source / 8] >> (7 - source % 8)) & 1;
                        let value_bit = if little {
                            let start = wire / 8 * 8;
                            start + (bits - start).min(8) - 1 - wire % 8
                        } else { bits - 1 - wire };
                        unsigned |= BigInt::from(bit) << value_bit;
                    }
                    for signed in 0..=1 {
                        let mut expected = unsigned.clone();
                        if signed == 1 && bits != 0 && unsigned.bit((bits - 1) as u64) {
                            expected -= BigInt::from(1) << bits;
                        }
                        unsafe {
                            let term = ErlTerm(nera_rt_binary_get_integer(&mut ctx, binary.0,
                                ErlTerm::small_int(offset as i64).0, ErlTerm::small_int(bits as i64).0,
                                signed | (endian << 8)));
                            assert_eq!(crate::bigint::term_to_bigint(term), Some(expected),
                                "bits={bits}, offset={offset}, endian={endian}, signed={signed}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn wide_integer_extraction_preserves_sign_and_unsigned_range() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        for value in [42_u64, SMALL_INT_MAX as u64 + 1, 1_u64 << 63, u64::MAX] {
            for signed in [0, 1] {
                for little in [0, 1] {
                    let bytes = if little == 1 {
                        value.to_le_bytes()
                    } else {
                        value.to_be_bytes()
                    };
                    let binary = heap.alloc_binary(&bytes);
                    let expected = if signed == 1 {
                        num_bigint::BigInt::from(value as i64)
                    } else {
                        num_bigint::BigInt::from(value)
                    };
                    let flags = signed | (little << 8);
                    // SAFETY: both the binary and raw bytes remain live, and
                    // the context exclusively owns heap access during each call.
                    unsafe {
                        let boxed = nera_rt_binary_get_integer(
                            &mut ctx,
                            binary.0,
                            ErlTerm::small_int(0).0,
                            ErlTerm::small_int(64).0,
                            flags,
                        );
                        let raw = nera_rt_binary_get_integer_raw(
                            &mut ctx,
                            bytes.as_ptr() as u64,
                            ErlTerm::small_int(0).0,
                            ErlTerm::small_int(64).0,
                            flags,
                        );
                        assert_eq!(
                            crate::bigint::term_to_bigint(ErlTerm(boxed)),
                            Some(expected.clone())
                        );
                        assert_eq!(crate::bigint::term_to_bigint(ErlTerm(raw)), Some(expected));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod short_float_tests {
    use super::*;

    #[test]
    fn short_float_boundaries_match_beam() {
        for (value, expected) in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (100.0, "100.0"),
            // Exact halfway value 1331752170181752.25: choose the even last digit.
            (f64::from_bits(0x4312_ece2_dc21_51e1), "1331752170181752.2"),
            (9_007_199_254_740_991.0, "9007199254740991.0"),
            (9_007_199_254_740_992.0, "9.007199254740992e15"),
            (563_761_063_810_007_800.0, "5.637610638100078e17"),
            (1000.0, "1.0e3"),
            (0.0001, "0.0001"),
            (0.00001, "1.0e-5"),
            (1.234e-4, "1.234e-4"),
            (99.99, "99.99"),
            (-9.9e100, "-9.9e100"),
            (0.1 + 0.2, "0.30000000000000004"),
            (f64::from_bits(1), "5.0e-324"),
            (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
            (f64::MAX, "1.7976931348623157e308"),
        ] {
            let text = format_short_float(value);
            assert_eq!(text, expected);
            assert_eq!(text.parse::<f64>().unwrap().to_bits(), value.to_bits());
        }
    }

    #[test]
    fn short_float_uses_compact_scientific_notation() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let value = heap.alloc_float(9.9e100);
        let short = ErlTerm::atom(global_atom_table().intern("short"));
        let opts = heap.alloc_list_from_slice(&[short]);
        // All arguments and the result remain live on this exclusively owned heap.
        unsafe {
            let result = ErlTerm(nera_rt_float_to_binary_2(&mut ctx, value.0, opts.0));
            let (data, len) = binary_data(result).unwrap();
            assert_eq!(std::slice::from_raw_parts(data, len), b"9.9e100");
        }
    }
}

#[cfg(test)]
mod binary_sharing_tests {
    use super::*;

    #[test]
    fn nested_view_survives_gc_and_copy_releases_parent() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let backing = std::sync::Arc::new(BinaryData::new((0..256).map(|i| i as u8).collect()));
        let weak = std::sync::Arc::downgrade(&backing);
        let parent = heap.alloc_refc_binary_from_arc(backing, 0, 256);
        // Only rooted terms are used after collection; raw bytes are read while live.
        unsafe {
            let view = nera_rt_binary_part(&mut ctx, parent.0,
                ErlTerm::small_int(10).0, ErlTerm::small_int(101).0);
            let nested = ErlTerm(nera_rt_binary_part(&mut ctx, view,
                ErlTerm::small_int(5).0, ErlTerm::small_int(65).0));
            let mut roots = [nested];
            unsafe { heap.collect_full(&mut roots) };
            assert!(weak.upgrade().is_some());
            assert_eq!(ErlTerm(nera_rt_binary_referenced_byte_size(&mut ctx, roots[0].0)).as_small_int(), Some(256));
            let (ptr, len) = binary_data(roots[0]).unwrap();
            assert_eq!(std::slice::from_raw_parts(ptr, len), &(15..80).collect::<Vec<u8>>());
            roots[0] = ErlTerm(nera_rt_binary_copy_1(&mut ctx, roots[0].0));
            unsafe { heap.collect_full(&mut roots) };
            assert!(weak.upgrade().is_none());
            assert_eq!(ErlTerm(nera_rt_binary_referenced_byte_size(&mut ctx, roots[0].0)).as_small_int(), Some(65));
            let (ptr, len) = binary_data(roots[0]).unwrap();
            assert_eq!(std::slice::from_raw_parts(ptr, len), &(15..80).collect::<Vec<u8>>());
        }
    }

    #[test]
    fn binary_part_retains_backing_allocation() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let parent = heap.alloc_binary_auto(&[42; 256]);
        // Terms remain live on this exclusively owned heap.
        unsafe {
            let view = ErlTerm(nera_rt_binary_part(
                &mut ctx,
                parent.0,
                ErlTerm::small_int(10).0,
                ErlTerm::small_int(101).0,
            ));
            let backing =
                *view.as_boxed_ptr().unwrap().add(REFC_BINARY_PTR_WORD_INDEX) as *const BinaryData;
            assert_eq!((&*backing).len(), 256);
        }
    }
}

#[cfg(test)]
mod last_match_tests {
    use super::*;

    #[test]
    fn byte_values_and_longest_match_at_earliest_offset() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        for byte in 0..=255 {
            let subject = heap.alloc_binary(&[0, byte]);
            unsafe {
                assert_eq!(ErlTerm(nera_rt_binary_last(&mut ctx, subject.0)).as_small_int(), Some(byte as i64));
            }
        }
        let subject = heap.alloc_binary(b"zabcd");
        let short = heap.alloc_binary(b"ab");
        let long = heap.alloc_binary(b"abc");
        let later = heap.alloc_binary(b"bcd");
        for order in [[short, long, later], [later, long, short]] {
            let patterns = heap.alloc_list_from_slice(&order);
            unsafe {
                let result = ErlTerm(nera_rt_binary_match(&mut ctx, subject.0, patterns.0));
                let ptr = result.as_boxed_ptr().unwrap();
                assert_eq!(BoxedHeader(*ptr).subtype(), BOXED_TUPLE);
                assert_eq!(ErlTerm(*ptr.add(1)).as_small_int(), Some(1));
                assert_eq!(ErlTerm(*ptr.add(2)).as_small_int(), Some(3));
            }
        }
    }

    #[test]
    fn invalid_pattern_after_match_is_still_rejected() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let subject = heap.alloc_binary(b"abc");
        let valid = heap.alloc_binary(b"a");
        let empty = heap.alloc_binary(b"");
        let patterns = heap.alloc_list_from_slice(&[valid, empty]);
        unsafe { nera_rt_binary_match(&mut ctx, subject.0, patterns.0); }
        assert_eq!(ctx.status, crate::process::STATUS_EXCEPTION);
        assert_eq!(ctx.result, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
    }
}

#[cfg(test)]
mod iolist_size_tests {
    use super::*;

    #[test]
    fn shared_payloads_are_counted_without_materializing_a_binary() {
        let mut heap = crate::heap::ProcessHeap::new();
        let binary = heap.alloc_binary_auto(&vec![7; 1024 * 1024]);
        let root = heap.alloc_list_from_slice(&vec![binary; 128]);
        // SAFETY: the graph is initialized and remains immutably borrowed.
        assert_eq!(unsafe { measure_iolist(root, &heap, usize::MAX) }, Ok(128 * 1024 * 1024));
        assert_eq!(unsafe { measure_iolist(root, &heap, 1024 * 1024) },
                   Err(crate::atom_table::ATOM_SYSTEM_LIMIT));
    }

    #[test]
    fn binary_tail_and_byte_tail_have_different_validity() {
        let mut heap = crate::heap::ProcessHeap::new();
        let tail = heap.alloc_binary(b"xy");
        let valid = heap.alloc_cons(ErlTerm::small_int(7), tail);
        let invalid = heap.alloc_cons(ErlTerm::small_int(7), ErlTerm::small_int(8));
        // SAFETY: all reachable cells and the binary stay live in this heap.
        assert_eq!(unsafe { measure_iolist(valid, &heap, usize::MAX) }, Ok(3));
        assert_eq!(unsafe { measure_iolist(invalid, &heap, usize::MAX) },
                   Err(crate::atom_table::ATOM_BADARG));
        assert_eq!(unsafe { measure_iolist(ErlTerm::small_int(7), &heap, usize::MAX) },
                   Err(crate::atom_table::ATOM_BADARG));
    }
}

#[cfg(test)]
mod binary_list_domain_tests {
    use super::*;

    #[test]
    fn binary_to_list_rejects_partial_heap_and_refc_bitstrings() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        for length in [1, 16, 65, 256] {
            for remainder in 1..8 {
                let input = heap.alloc_binary_auto(&vec![0xff; length]);
                let bits = (length - 1) * 8 + remainder;
                // SAFETY: change only the exact length of this fresh descriptor;
                // its complete backing allocation remains owned by the heap.
                unsafe {
                    let ptr = input.as_boxed_ptr().unwrap().cast_mut();
                    *ptr = BoxedHeader(*ptr).with_arity(bits as u64).0;
                    ctx.status = crate::process::STATUS_OK;
                    let output = nera_rt_binary_to_list(&mut ctx, input.0);
                    assert_eq!(ctx.status, crate::process::STATUS_EXCEPTION,
                        "accepted {bits}-bit input");
                    assert_eq!(ctx.result, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
                    assert_eq!(output, ErlTerm::nil().0);
                }
            }
        }
    }
}

#[cfg(test)]
mod materialize_bits_tests {
    use super::*;

    #[cfg(not(target_env = "msvc"))]
    #[test]
    fn small_materializations_allocate_no_temporary_bytes() {
        let mut source = crate::heap::ProcessHeap::new();
        let binary = source.alloc_binary(&[0xa5;32]);
        let mut target = crate::heap::ProcessHeap::new_with_capacity(4096);
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut target;
        let allocated = tikv_jemalloc_ctl::thread::allocatedp::read().unwrap();
        let before = allocated.get();
        for _ in 0..100 {
            // SAFETY: source is live and the target has ample arena capacity.
            let result = ErlTerm(unsafe { nera_rt_bs_materialize(&mut ctx,binary.0,
                ErlTerm::small_int(3).0,ErlTerm::small_int(13).0) });
            assert_eq!(unsafe { result.binary_bit_size() },Some(13));
            assert_eq!(unsafe { result.binary_bytes_any() }.unwrap(),&[0x2d,0x28]);
        }
        assert_eq!(allocated.get()-before,0,"a small window must be filled in its final heap storage");
    }

    #[cfg(not(target_env = "msvc"))]
    #[test]
    fn large_materializations_allocate_only_final_bytes_and_metadata() {
        let mut source = crate::heap::ProcessHeap::new();
        let binary = source.alloc_binary_auto(&[0xa5;2048]);
        let mut target = crate::heap::ProcessHeap::new_with_capacity(4096);
        target.alloc_binary_auto(&[0;65]); // Reserve the binary-owner vector before counting.
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut target;
        let allocated = tikv_jemalloc_ctl::thread::allocatedp::read().unwrap();
        let before = allocated.get();
        // 8191 significant bits need a 1 KiB final buffer. The budget permits
        // final storage and owner metadata, but excludes another 1 KiB buffer.
        let result = ErlTerm(unsafe { nera_rt_bs_materialize(&mut ctx,binary.0,
            ErlTerm::small_int(3).0,ErlTerm::small_int(8191).0) });
        let bytes = allocated.get()-before;
        assert_eq!(unsafe { result.binary_bit_size() },Some(8191));
        assert!(bytes <= 1024+256,"allocated {bytes} bytes for a 1 KiB final buffer");
    }

    #[test]
    fn materialized_windows_outlive_the_source_and_moving_gc() {
        for length in [16_usize,65,256] {
            let bytes: Vec<u8> = (0..length).map(|i| (i*71+0xa5) as u8).collect();
            for offset in 0..8 {
                for count in [0,1,7,8,13,length*8-7] {
                    let mut expected = vec![0;count.div_ceil(8)];
                    for bit in 0..count {
                        let pos = offset+bit;
                        expected[bit/8] |= ((bytes[pos/8] >> (7-pos%8)) & 1) << (7-bit%8);
                    }
                    let mut source = crate::heap::ProcessHeap::new();
                    let binary = source.alloc_binary_auto(&bytes);
                    let window = unsafe { MatchContext::new(binary,offset as u64,(offset+count) as u64) }.unwrap();
                    let cursor = source.alloc_match_ctx(window);
                    let mut target = crate::heap::ProcessHeap::new();
                    let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
                    ctx.heap_ptr = &mut target;
                    let result = ErlTerm(unsafe { nera_rt_bs_materialize(&mut ctx,cursor.0,
                        ErlTerm::small_int(0).0,ErlTerm::small_int(count as i64).0) });
                    drop(source);
                    let mut roots = [result];
                    unsafe { target.collect_full(&mut roots); }
                    assert_eq!(unsafe { roots[0].binary_bit_size() },Some(count));
                    assert_eq!(unsafe { roots[0].binary_bytes_any() }.unwrap(),expected);
                }
            }
        }
    }

    #[test]
    fn materialization_keeps_unaligned_offsets_and_partial_last_bytes() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        for length in [3, 256] {
            let bytes = vec![0b10100101; length];
            let binary = heap.alloc_binary_auto(&bytes);
            for offset in 0..8 {
                for count in [1_usize, 7, 8, 13, 16, length * 8 - 7] {
                    let expected: Vec<u8> = (0..count.div_ceil(8)).map(|byte| {
                        let mut value = 0;
                        for bit in 0..8 {
                            let relative = byte * 8 + bit;
                            if relative < count {
                                let source = offset + relative;
                                value |= ((bytes[source / 8] >> (7 - source % 8)) & 1) << (7 - bit);
                            }
                        }
                        value
                    }).collect();
                    // SAFETY: source and cursor remain live in the owned heap;
                    // requested windows fit inside the source binary.
                    unsafe {
                        let cursor = heap.alloc_match_ctx(MatchContext::new(binary, offset as u64, (length * 8) as u64).unwrap());
                        for (source, relative) in [(binary, offset), (cursor, 0)] {
                            let result = ErlTerm(nera_rt_bs_materialize(&mut ctx, source.0,
                                ErlTerm::small_int(relative as i64).0,
                                ErlTerm::small_int(count as i64).0));
                            assert_eq!(result.binary_bit_size(), Some(count));
                            assert_eq!(result.binary_bytes_any().unwrap(), expected.as_slice());
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod bitstring_list_tests {
    use super::*;

    #[test]
    fn appending_short_bitstring_preserves_destination_prefix() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let destination = heap.alloc_binary(&[0xa0]);
        let source = heap.alloc_binary(&[0xf8]);
        // SAFETY: both terms belong to this live heap; copy exactly five bits.
        unsafe {
            nera_rt_binary_put_binary(&mut ctx, destination.0, ErlTerm::small_int(3).0,
                source.0, ErlTerm::small_int(5).0);
            assert_eq!(destination.binary_bytes_any().unwrap(), &[0xbf]);
        }
    }

    #[test]
    fn conversion_keeps_complete_bytes() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let input = heap.alloc_binary(&[255, 0]);
        // SAFETY: context and all heap terms stay live throughout the conversion.
        let result = unsafe { ErlTerm(nera_rt_bitstring_to_list(&mut ctx, input.0)) };
        assert_ne!(result, ErlTerm::nil());
    }
}

#[cfg(test)]
mod binary_integer_base_tests {
    use super::*;
    #[test]
    fn parses_hexadecimal_binary() {
        let mut heap = crate::heap::ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let input = heap.alloc_binary(b"ff");
        // SAFETY: the context and input allocation remain live during parsing.
        let value = unsafe { ErlTerm(nera_rt_binary_to_integer_2(&mut ctx, input.0, ErlTerm::small_int(16).0)) };
        assert_eq!(value.as_small_int(), Some(255));
    }
}
