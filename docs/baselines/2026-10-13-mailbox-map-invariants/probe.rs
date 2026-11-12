use nera_runtime::{heap::ProcessHeap, process::Mailbox, term::ErlTerm};
use std::{hint::black_box, time::Instant};
fn main() {
    let mut mb = Mailbox::new();
    for i in 0..128 { mb.send(ErlTerm::small_int(i)); }
    for _ in 0..128 { mb.remove_at(0); }
    let mut elapsed = 0;
    for _ in 0..50_000 {
        let start = Instant::now();
        for i in 0..128 { mb.send(black_box(ErlTerm::small_int(i))); }
        elapsed += start.elapsed().as_nanos();
        assert_eq!(mb.len(),128);
        for _ in 0..128 { mb.remove_at(0); }
    }
    println!("send {}", elapsed / 1000);
    let mut elapsed = 0;
    for _ in 0..50_000 {
        for i in 0..128 { mb.send(ErlTerm::small_int(i)); }
        let start = Instant::now();
        for i in (1..128).step_by(2) { black_box(mb.remove_at(i)); }
        elapsed += start.elapsed().as_nanos();
        assert_eq!(mb.len(),64);
        // Clearing through the logical head also trims the odd tombstones.
        for _ in 0..64 { mb.remove_at(0); }
    }
    println!("remove {}", elapsed / 1000);
    for (name, pairs, batches) in [("map_small",2usize,1000), ("map_large",32,100)] {
        let terms: Vec<_> = (0..pairs*2).map(|i| ErlTerm::small_int(i as i64)).collect();
        let start = Instant::now();
        for _ in 0..batches {
            let mut heap = ProcessHeap::new();
            for _ in 0..1000 {
                #[cfg(not(checked))]
                let root = heap.alloc_map(black_box(&terms));
                #[cfg(checked)]
                let root = heap.alloc_map(nera_runtime::heap::MapPairs::from_flat(black_box(&terms)).unwrap());
                black_box(root);
            }
        }
        println!("{name} {}",start.elapsed().as_micros());
    }
    let entries = [[ErlTerm::small_int(0), ErlTerm::small_int(1)], [ErlTerm::small_int(2), ErlTerm::small_int(3)]];
    let start = Instant::now();
    for _ in 0..1000 {
        let mut heap = ProcessHeap::new();
        for _ in 0..1000 {
            #[cfg(not(checked))]
            let root = heap.alloc_map(black_box(&entries[..]).as_flattened());
            #[cfg(checked)]
            let root = heap.alloc_map(nera_runtime::heap::MapPairs::new(black_box(&entries[..])));
            black_box(root);
        }
    }
    println!("map_typed {}", start.elapsed().as_micros());

}
