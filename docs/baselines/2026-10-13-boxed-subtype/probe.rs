//! Temporarily included inside process.rs for the actual runtime constructors.
use super::*;
use crate::term::{BoxedHeader, BoxedSubtype};
use std::{hint::black_box, time::Instant};
#[test]
#[ignore]
fn process_entry_performance_probe() {
    let start = Instant::now();
    for _ in 0..10_000_000 {
        black_box(BoxedHeader::new(BoxedSubtype::Tuple, black_box(31)).0);
    }
    println!("header_static {}", start.elapsed().as_micros());
    let tags = [BoxedSubtype::Tuple, BoxedSubtype::Float, BoxedSubtype::BigInt, BoxedSubtype::HeapBinary, BoxedSubtype::Closure, BoxedSubtype::Map, BoxedSubtype::RefcBinary, BoxedSubtype::Reference, BoxedSubtype::MatchContext];
    let start = Instant::now();
    for i in 0..10_000_000 {
        black_box(BoxedHeader::new(black_box(tags[i % tags.len()]), black_box(31)).0);
    }
    println!("header_dynamic {}", start.elapsed().as_micros());
    let header = BoxedHeader::new(BoxedSubtype::HeapBinary, 128);
    let start = Instant::now();
    for _ in 0..10_000_000 {
        black_box(black_box(header).with_arity(black_box(64)).0);
    }
    println!("header_resize {}", start.elapsed().as_micros());
    let fields = [ErlTerm::small_int(7), ErlTerm::nil()];
    let pairs = [[fields[0], fields[1]], [ErlTerm::small_int(8), fields[0]]];
    let start = Instant::now();
    for _ in 0..1000 {
        let mut heap = ProcessHeap::new();
        for _ in 0..1000 {
            black_box(heap.alloc_tuple(black_box(&fields)));
            black_box(heap.alloc_float(black_box(1.5)));
            black_box(heap.alloc_map(crate::heap::MapPairs::new(black_box(&pairs))));
        }
    }
    println!("heap_boxed {}", start.elapsed().as_micros());
}
