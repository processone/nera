use nera_runtime::{heap::ProcessHeap, message::MessageFragment, term::ErlTerm};
use std::{hint::black_box, time::Instant};
fn main() {
    let start = Instant::now();
    for _ in 0..1000 {
        let mut heap = ProcessHeap::new();
        for _ in 0..1000 { black_box(heap.alloc_binary_zeroed(black_box(63))); }
    }
    let binary = start.elapsed().as_micros();
    let mut source = ProcessHeap::new();
    let bytes = source.alloc_binary_auto(&[42; 256]);
    let tuple = source.alloc_tuple(&[ErlTerm::small_int(7), bytes]);
    let start = Instant::now();
    for _ in 0..1000 {
        let mut target = ProcessHeap::new();
        let mut last = ErlTerm::nil();
        for _ in 0..1000 {
            // SAFETY: source owns the complete immutable graph throughout the probe.
            let fragment = unsafe { MessageFragment::from_term(black_box(tuple)) };
            let copied = fragment.copy_to_heap(&mut target);
            black_box(copied);
            last = copied;
        }
        // SAFETY: target owns the copied tuple and its retained binary backing.
        unsafe {
            let ptr = last.as_boxed_ptr().unwrap();
            assert_eq!(ErlTerm::from_raw(*ptr.add(1)).as_small_int(), Some(7));
            let binary = ErlTerm::from_raw(*ptr.add(2));
            assert_eq!(binary.borrow(&target).binary_bytes(), Some(&[42; 256][..]));
        }
    }
    println!("binary {}\nmessage {}", binary, start.elapsed().as_micros());
}
