//! Allocation-free bitwise memmove, operating a destination byte at a time.

/// Copy exactly count bits, preserving neighboring bits, including overlap.
/// # Safety
/// The source covers source_offset + count initialized bits, readable until its
/// bits have been copied; destination covers destination_offset + count writable
/// initialized bits. Both sums fit usize. The caller excludes concurrent access
/// and outstanding Rust byte borrows except source/destination raw overlap.
/// No GC/reentry may move storage during this call.
#[inline]
pub(crate) unsafe fn copy_bits(
    destination: *mut u8, destination_offset: usize,
    source: *const u8, source_offset: usize, count: usize,
) {
    if count == 0 { return; }
    if (destination_offset | source_offset | count) & 7 == 0 {
        std::ptr::copy(source.add(source_offset / 8), destination.add(destination_offset / 8), count / 8);
        return;
    }
    // Compare byte addresses plus bit remainders; multiplying addresses by 8
    // would overflow. Raw operations deliberately permit overlapping storage.
    let target_start = (destination.add(destination_offset / 8).addr(), destination_offset % 8);
    let source_start = (source.add(source_offset / 8).addr(), source_offset % 8);
    if target_start > source_start {
        let mut left = count;
        while left != 0 {
            let end = destination_offset + left;
            let n = ((end - 1) % 8 + 1).min(left);
            left -= n;
            copy_chunk(destination, destination_offset + left, source, source_offset + left, n);
        }
    } else {
        let mut copied = 0;
        while copied != count {
            let pos = destination_offset + copied;
            let n = (8 - pos % 8).min(count - copied);
            copy_chunk(destination, pos, source, source_offset + copied, n);
            copied += n;
        }
    }
}

#[inline(always)]
unsafe fn copy_chunk(target: *mut u8, offset: usize, source: *const u8, source_offset: usize, count: usize) {
    let shift = source_offset % 8;
    let first = *source.add(source_offset / 8) as u16;
    // Load the carry before writing, so even same-byte overlap is safe.
    let carry = if shift + count > 8 { *source.add(source_offset / 8 + 1) as u16 } else { 0 };
    let mask = ((1_u16 << count) - 1) as u8;
    let value = (((first << 8 | carry) >> (16 - shift - count)) as u8) & mask;
    let target_shift = 8 - offset % 8 - count;
    let byte = target.add(offset / 8);
    *byte = (*byte & !(mask << target_shift)) | (value << target_shift);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_offset_length_and_overlap_direction_matches_snapshot_oracle() {
        for source_byte in 0..5 {
            for target_byte in 0..5 {
                for source_bit in 0..8 {
                    for target_bit in 0..8 {
                        for count in 0..=129 {
                            let original: [u8; 32] = std::array::from_fn(|i| (i * 71 + 0xa5) as u8);
                            let mut expected = original;
                            let mut actual = original;
                            let src = source_byte * 8 + source_bit;
                            let dst = target_byte * 8 + target_bit;
                            for bit in 0..count {
                                let value = (original[(src+bit)/8] >> (7-(src+bit)%8)) & 1;
                                let pos = dst+bit;
                                expected[pos/8] = (expected[pos/8] & !(1 << (7-pos%8))) | (value << (7-pos%8));
                            }
                            unsafe { copy_bits(actual.as_mut_ptr(),dst,actual.as_ptr(),src,count); }
                            assert_eq!(actual,expected,"src={src}, dst={dst}, count={count}");
                        }
                    }
                }
            }
        }
    }
}
