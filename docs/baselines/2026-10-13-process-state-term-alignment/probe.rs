//! Temporarily included as a child of process.rs for a release test binary.
use super::*;
use std::{hint::black_box, time::Instant};

#[test]
#[ignore]
fn process_entry_performance_probe() {
    let entry = ProcessEntry::new(Process::new(1));
    let start = Instant::now();
    for _ in 0..2_000_000 {
        let mut proc = entry.lock_for_runtime(); // ADAPTER lock
        proc.transition(black_box(ProcessState::Running)); // ADAPTER transition
        black_box(proc.status());
    }
    println!("lock_transition {}", start.elapsed().as_micros());
    let start = Instant::now();
    for _ in 0..2_000_000 {
        let mut proc = entry.lock_for_runtime(); // ADAPTER lock
        proc.drain_inbox(); // ADAPTER drain
        black_box(proc.mailbox.len());
    }
    println!("lock_empty_drain {}", start.elapsed().as_micros());
    // Batches exercise the existing no-Vec (single) and Vec (multiple) paths.
    for batch in [1, 16] {
        let entry = ProcessEntry::new(Process::new(2));
        let start = Instant::now();
        for _ in 0..50_000 {
            for i in 0..batch {
                entry.inbox.push(MessageFragment::Immediate(crate::message::ImmediateTerm::new(ErlTerm::small_int(i))));
            }
            let mut proc = entry.lock_for_runtime(); // ADAPTER lock
            proc.drain_inbox(); // ADAPTER drain
            assert_eq!(proc.mailbox.len(), batch as usize);
            for _ in 0..batch { black_box(proc.mailbox.remove_at(0)); }
        }
        println!("drain_{batch} {}", start.elapsed().as_micros());
    }
    let mut proc = Process::new(3);
    for i in 0..128 { proc.mailbox.send(ErlTerm::small_int(i)); }
    // A non-EXIT boxed term also exercises header reads with unchanged storage.
    let tuple = proc.heap.alloc_tuple(&[ErlTerm::small_int(7)]);
    proc.mailbox.send(tuple);
    let start = Instant::now();
    for _ in 0..100_000 {
        // SAFETY: all terms are immediate or allocated on proc.heap, which
        // remains live and unmoving; no other access occurs during the scan.
        black_box(unsafe { process_exit_signals(black_box(&mut proc)) });
    }
    println!("exit_scan {}", start.elapsed().as_micros());
    let storage = [0u64; 4];
    let ptr = storage.as_ptr();
    let start = Instant::now();
    for _ in 0..5_000_000 { black_box(ErlTerm::boxed(black_box(ptr))); }
    println!("encode_public {}", start.elapsed().as_micros());
    let start = Instant::now();
    for _ in 0..5_000_000 { black_box(unsafe { ErlTerm::boxed_unchecked(black_box(ptr)) }); } // ADAPTER trusted
    println!("encode_trusted {}", start.elapsed().as_micros());
    let fields = [ErlTerm::small_int(7), ErlTerm::nil()];
    let start = Instant::now();
    for _ in 0..1000 {
        let mut heap = ProcessHeap::new();
        for _ in 0..1000 {
            black_box(heap.alloc_cons(black_box(fields[0]), black_box(fields[1])));
            black_box(heap.alloc_tuple(black_box(&fields)));
        }
    }
    println!("heap_terms {}", start.elapsed().as_micros());
}
