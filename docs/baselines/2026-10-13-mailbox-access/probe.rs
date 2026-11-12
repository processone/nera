use nera_runtime::{process::{Mailbox, Process, process_exit_signals}, term::ErlTerm};
use nera_runtime::atom_table::{ATOM_EXIT_SIGNAL, ATOM_NORMAL};
use std::{hint::black_box, time::Instant};
fn main() {
    let mut mailbox = Mailbox::new();
    for i in 0..32 { mailbox.send(ErlTerm::small_int(i)); }
    let start = Instant::now();
    for _ in 0..5_000_000 {
        for i in 0..32 { black_box(mailbox.get(black_box(i))); }
    }
    println!("get {}", start.elapsed().as_micros());
    let mut proc = Process::new(1);
    let normal = proc.heap.alloc_tuple(&[ErlTerm::atom(ATOM_EXIT_SIGNAL), ErlTerm::pid(2), ErlTerm::atom(ATOM_NORMAL)]);
    let mut nanos = 0;
    for _ in 0..500_000 {
        proc.mailbox = Mailbox::new();
        // Keep an immediate head so baseline and changed versions scan the same
        // slots. All removed EXIT signals are non-head slots, with no head trim.
        for i in 0..16 { proc.mailbox.send(ErlTerm::small_int(i)); }
        for _ in 0..16 { proc.mailbox.send(normal); }
        let start = Instant::now();
        assert_eq!(black_box(process_exit_signals(black_box(&mut proc))), None);
        nanos += start.elapsed().as_nanos();
        assert_eq!(proc.mailbox.alive_count(),16);
    }
    println!("exit {}", nanos / 1000);
}
