#![allow(unused_unsafe)]
use nera_runtime::{process::Process, term::ErlTerm};
use std::hint::black_box;
use std::time::Instant;
const N: u64 = 1_000_000;

fn resets(immediate: bool) -> u128 {
    let mut process = Process::new(1);
    if immediate {
        process.dictionary.push((ErlTerm::small_int(1), ErlTerm::small_int(2)));
        process.mailbox.send(ErlTerm::small_int(7));
    }
    let start = Instant::now();
    for i in 0..N {
        let dead = process.heap.alloc_tuple(&[ErlTerm::small_int(i as i64); 4]);
        process.initial_args.push(dead);
        // SAFETY: retained roots are immediate; this thread owns the process.
        unsafe { process.hibernate(); }
        black_box(&process);
    }
    assert_eq!(process.heap.bytes_allocated(), 0);
    if immediate {
        assert_eq!(process.dictionary[0].1, ErlTerm::small_int(2));
        assert_eq!(process.mailbox.messages().next(), Some(ErlTerm::small_int(7)));
    }
    start.elapsed().as_micros()
}

fn collections() -> u128 {
    let mut process = Process::new(1);
    process.initial_args.push(ErlTerm::nil());
    let start = Instant::now();
    for i in 0..N {
        process.initial_args[0] = process.heap.alloc_tuple(&[ErlTerm::small_int(i as i64); 4]);
        // SAFETY: complete process-owned roots, no other access or object borrow.
        unsafe { process.collect_garbage_full(); }
        black_box(&process);
    }
    // SAFETY: the updated root still belongs to the live process heap.
    unsafe {
        assert_eq!(*process.initial_args[0].as_boxed_ptr().unwrap().add(1), ErlTerm::small_int((N - 1) as i64).to_raw());
    }
    start.elapsed().as_micros()
}

fn retained() -> u128 {
    let mut process = Process::new(1);
    let binary = process.heap.alloc_binary_auto(&[7; 256]);
    let message = process.heap.alloc_tuple(&[binary, ErlTerm::small_int(42)]);
    process.mailbox.send(message);
    process.dictionary.push((ErlTerm::small_int(1), message));
    let start = Instant::now();
    for _ in 0..N {
        // SAFETY: the complete retained graph belongs to this exclusively owned heap.
        unsafe { process.hibernate(); }
        black_box(&process);
    }
    assert_eq!(process.mailbox.messages().next(), Some(process.dictionary[0].1));
    unsafe {
        let binary = ErlTerm::from_raw(*process.dictionary[0].1.as_boxed_ptr().unwrap().add(1));
        assert_eq!(binary.borrow(&process.heap).binary_bytes(), Some(&[7; 256][..]));
    }
    start.elapsed().as_micros()
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("retained") {
        println!("retained {}", retained());
    } else {
        println!("empty {}", resets(false));
        println!("immediate {}", resets(true));
        println!("gc {}", collections());
    }
}
