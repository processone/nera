//! Owned binary storage with a stable prefix for generated code.
//!
//! Heap descriptors hold an `Arc::as_ptr()` pointer to this object. Generated
//! code reads only `data`, whose offset is defined by `repr(C)`. The private
//! `Vec` remains responsible for allocation and deallocation; its layout is
//! never inspected and its allocation cannot be resized through this API.

use std::ops::Deref;
pub(crate) mod bit_copy;

#[repr(C)]
pub struct BinaryData {
    data: *mut u8,
    bytes: Vec<u8>,
}

pub const BINARY_DATA_PTR_BYTE_OFFSET: i32 = std::mem::offset_of!(BinaryData, data) as i32;

impl BinaryData {
    /// Take ownership without copying or reallocating the bytes.
    pub fn new(mut bytes: Vec<u8>) -> Self {
        let data = bytes.as_mut_ptr();
        Self { data, bytes }
    }

    /// Pointer used by the generated-code ABI. Writing through it requires
    /// exclusive access to the bytes, including absence of borrowed slices.
    #[inline]
    pub fn as_ptr(&self) -> *const u8 {
        self.data
    }
}

impl Deref for BinaryData {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

/// An owned bitstring window, starting at a byte boundary in shared storage.
/// Bounds and the exact boxed-header bit-length domain are validated together.
pub struct BinarySlice {
    data: std::sync::Arc<BinaryData>,
    offset: usize,
    bits: usize,
}

impl BinarySlice {
    #[inline]
    pub fn new(data: std::sync::Arc<BinaryData>, offset: usize, len: usize) -> Option<Self> {
        Self::new_bits(data, offset, len.checked_mul(8)?)
    }

    #[inline]
    pub fn new_bits(data: std::sync::Arc<BinaryData>, offset: usize, bits: usize) -> Option<Self> {
        if offset > data.len() || bits.div_ceil(8) > data.len() - offset { return None; }
        if u64::try_from(bits).ok()? > u64::MAX >> crate::term::BOXED_TAG_BITS { return None; }
        Some(Self { data, offset, bits })
    }

    /// Reuse the guarantees of an existing well-formed runtime descriptor.
    /// # Safety
    /// The rounded byte window fits data, and bits fits the boxed header domain.
    /// Used only while extracting descriptors from a live valid term graph.
    #[inline]
    pub(crate) unsafe fn from_runtime_bits(data: std::sync::Arc<BinaryData>, offset: usize, bits: usize) -> Self {
        debug_assert!(offset <= data.len() && bits.div_ceil(8) <= data.len() - offset);
        debug_assert!((bits as u64) <= u64::MAX >> crate::term::BOXED_TAG_BITS);
        Self { data, offset, bits }
    }

    #[inline]
    pub(crate) fn into_bit_parts(self) -> (std::sync::Arc<BinaryData>, usize, usize) {
        (self.data, self.offset, self.bits)
    }
}

// SAFETY: `data` points into the owned Vec allocation, which remains stable
// when the owner moves. Safe access is read-only. Runtime construction writes
// through raw pointers must finish before sharing or borrowing the bytes.
unsafe impl Send for BinaryData {}
// SAFETY: no safe method mutates or resizes the allocation. Shared access has
// the same requirements as sharing a Vec<u8>.
unsafe impl Sync for BinaryData {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn binary_windows_check_bounds_and_retain_storage() {
        let data = std::sync::Arc::new(BinaryData::new(vec![1, 2, 3, 4]));
        assert!(BinarySlice::new(data.clone(), 5, 0).is_none());
        assert!(BinarySlice::new(data.clone(), 1, usize::MAX).is_none());
        assert!(BinarySlice::new(data.clone(), usize::MAX, 1).is_none());
        let range = BinarySlice::new(data.clone(), 1, 2).unwrap();
        let empty = BinarySlice::new(data.clone(), 4, 0).unwrap();
        assert!(BinarySlice::new_bits(data.clone(), 3, 9).is_none());
        assert!(BinarySlice::new_bits(data.clone(), 4, 1).is_none());
        assert!(BinarySlice::new_bits(data.clone(), 0, usize::MAX).is_none());
        let partial = BinarySlice::new_bits(data.clone(), 1, 13).unwrap();
        drop(data);
        let mut heap = crate::heap::ProcessHeap::new();
        let root = heap.alloc_binary_slice(range);
        let empty_root = heap.alloc_binary_slice(empty);
        let partial_root = heap.alloc_binary_slice(partial);
        assert_eq!(unsafe { partial_root.binary_bit_size() }, Some(13));
        // SAFETY: heap now retains the windows' backing allocation.
        assert_eq!(unsafe { root.borrow(&heap) }.binary_bytes(), Some(&[2, 3][..]));
        assert_eq!(unsafe { empty_root.borrow(&heap) }.binary_bytes(), Some(&[][..]));
    }

    #[test]
    fn abi_pointer_survives_moves_without_copying() {
        for bytes in [Vec::new(), vec![1, 2, 3, 4], Vec::with_capacity(32)] {
            let original_ptr = bytes.as_ptr();
            let expected = bytes.clone();
            let owner = Arc::new(BinaryData::new(bytes));
            let shared = Arc::clone(&owner);
            drop(owner);
            // SAFETY: repr(C) defines the prefix and shared keeps it alive.
            let abi_ptr = unsafe {
                let base = Arc::as_ptr(&shared).cast::<u8>();
                *base
                    .add(BINARY_DATA_PTR_BYTE_OFFSET as usize)
                    .cast::<*const u8>()
            };
            assert_eq!(abi_ptr, original_ptr);
            assert_eq!(abi_ptr, shared.as_ptr());
            assert_eq!(&shared[..], expected);
        }
    }
}
