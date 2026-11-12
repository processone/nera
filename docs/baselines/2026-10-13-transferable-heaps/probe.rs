use nera_runtime::{heap::ProcessHeap, message::{MessageFragment, HeapFragment}, term::ErlTerm};
use std::{hint::black_box, time::Instant};
const ITEMS: usize = 1000;

fn transfer(source: &ProcessHeap, term: ErlTerm, expect: &[u8], batches: usize) -> u128 {
    let start = Instant::now();
    for _ in 0..batches {
        let mut target = ProcessHeap::new();
        let mut last = ErlTerm::nil();
        for _ in 0..ITEMS {
            // SAFETY: source owns the complete initialized graph and is immutable.
            let fragment = unsafe { term.borrow(source) }.to_message_fragment();
            last = black_box(fragment.copy_to_heap(&mut target));
        }
        // SAFETY: the consumed fragments own the graph now kept by target.
        unsafe {
            let nested = ErlTerm::from_raw(*last.as_boxed_ptr().unwrap().add(1));
            let bytes = ErlTerm::from_raw(*nested.as_boxed_ptr().unwrap().add(1));
            assert_eq!(bytes.borrow(&target).binary_bytes(), Some(expect));
        }
    }
    start.elapsed().as_micros()
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("layout") {
        println!("message {}\nheap_fragment {}", std::mem::size_of::<MessageFragment>(), std::mem::size_of::<HeapFragment>());
        return;
    }
    let mut source = ProcessHeap::new();
    let binary = source.alloc_binary_auto(&[42; 256]);
    let nested = source.alloc_tuple(&[binary, binary]);
    let tuple = source.alloc_tuple(&[nested, ErlTerm::small_int(7)]);
    println!("small {}", transfer(&source, tuple, &[42; 256], 1000));
    let list = source.alloc_list_from_slice(&[ErlTerm::small_int(1); 100]);
    let large = source.alloc_tuple(&[nested, list]);
    println!("large {}", transfer(&source, large, &[42; 256], 100));
}
