use super::*;
use std::{hint::black_box, time::Instant};
#[test]
#[ignore]
fn process_entry_performance_probe() {
    let mut heap = ProcessHeap::new();
    let one = ErlTerm::small_int(123);
    let two = ErlTerm::small_int(124);
    let float = heap.alloc_float(123.5);
    let big = crate::bigint::bigint_to_term(&mut heap, num_bigint::BigInt::from(1u64) << 100);
    let near = heap.alloc_float(2f64.powi(100));
    for (label, a, b) in [("small", one, two), ("mixed", one, float), ("big_mixed", big, float), ("big_near", big, near)] {
        let start = Instant::now();
        for _ in 0..100_000 {
            black_box(unsafe { crate::ffi::compare::compare_numeric(black_box(a), black_box(b)) as i64 });
        }
        println!("{label} {}", start.elapsed().as_micros());
    }
    let start = Instant::now();
    for _ in 0..100 {
        let id = crate::ets::ets_new(0, &[("ordered_set".into(), ErlTerm::nil())], 1).unwrap();
        let table = ErlTerm::small_int(id as i64);
        for i in 0..128 {
            let tuple = heap.alloc_tuple(&[ErlTerm::small_int(i), one]);
            unsafe { crate::ets::ets_insert(table, tuple).unwrap(); }
        }
        for i in 0..128 { black_box(unsafe { crate::ets::ets_member(table, ErlTerm::small_int(i)) }); }
        assert!(crate::ets::ets_delete_table(table));
    }
    println!("ordered_set {}", start.elapsed().as_micros());
}
