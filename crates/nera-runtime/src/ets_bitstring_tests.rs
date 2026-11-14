//! Ownership checks for the bitstring storage regression. The Erlang fixture
//! owns observable lifetime/equality behavior; this test deterministically
//! rejects descriptor aliasing before either source allocation is released.

use super::*;
use crate::binary::BinarySlice;
use crate::term::BoxedSubtype;

#[test]
fn bitstring_copies_own_descriptors_in_both_directions() {
    // The reported values, plus a sub-byte value and the inline/refc boundary.
    let mut large = vec![b'x'; 200];
    large.push(0xc0); // <<3:2>>
    let mut boundary = vec![b'b'; crate::heap::REFC_BINARY_THRESHOLD];
    boundary.push(0x80); // <<1:1>>
    let cases = [
        (b"abcdefgh\x50".to_vec(), 4),
        (large, 6),
        (vec![0xa0], 5),
        (boundary, 7),
    ];

    for (bytes, unused_bits) in cases {
        let bits = bytes.len() * 8 - unused_bits;
        for refc_window in [false, true] {
            let mut source = ProcessHeap::new();
            let original = if refc_window {
                // A byte-offset window must not accidentally copy its prefix.
                let prefix = b"prefix";
                let backing = [prefix.as_slice(), &bytes, b"suffix"].concat();
                let window =
                    BinarySlice::new_bits(Arc::new(BinaryData::new(backing)), prefix.len(), bits)
                        .unwrap();
                source.alloc_binary_slice(window)
            } else {
                let term = source.alloc_binary(&bytes);
                // SAFETY: the test exclusively owns the new descriptor and
                // shortens its bit length within its initialized byte storage.
                unsafe {
                    *term.as_boxed_ptr().unwrap().cast_mut() =
                        BoxedHeader::new(BoxedSubtype::HeapBinary, bits as u64).0;
                }
                term
            };

            // SAFETY: the source is live, immutable and cannot collect here.
            let words = unsafe { count_heap_words(original) };
            let mut pool = EtsRegionPool::new();
            let mut stored = CompactHeap::from_pool(Some(pool.alloc(words)));
            let in_storage = unsafe { deep_copy_into_compact(original, &mut stored) };
            assert_ne!(
                in_storage.as_boxed_ptr(),
                original.as_boxed_ptr(),
                "storage must own the bitstring descriptor"
            );
            drop(source);

            let mut reader = ProcessHeap::new();
            // SAFETY: stored keeps every source descriptor/backing byte live.
            let read = unsafe { deep_copy_into_heap(in_storage, &mut reader) };
            assert_ne!(
                read.as_boxed_ptr(),
                in_storage.as_boxed_ptr(),
                "copying reads must own the bitstring descriptor"
            );
            drop(stored);
            drop(pool);

            let mut roots = [read];
            // SAFETY: the sole root belongs to reader; no raw borrows survive.
            unsafe { reader.collect_full(&mut roots) };
            // SAFETY: the root was updated by GC and reader owns its bytes.
            unsafe {
                assert_eq!(roots[0].binary_bit_size(), Some(bits));
                assert_eq!(roots[0].binary_bytes_any(), Some(bytes.as_slice()));
            }
        }
    }
}
