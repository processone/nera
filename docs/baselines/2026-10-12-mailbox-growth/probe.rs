use nera_runtime::{process::Mailbox, term::ErlTerm};
use std::{hint::black_box, time::Instant};
fn main() {
    let start = Instant::now();
    for _ in 0..20_000 {
        let mut mailbox = Mailbox::new();
        for i in 0..256 { mailbox.send(ErlTerm::small_int(i)); }
        assert_eq!(mailbox.len(), 256);
        black_box(&mailbox);
    }
    let growth = start.elapsed().as_micros();
    let mut mailbox = Mailbox::new();
    mailbox.send(ErlTerm::small_int(0));
    mailbox.remove_at(0);
    let start = Instant::now();
    for i in 0..1_000_000 {
        mailbox.send(ErlTerm::small_int(black_box(i)));
        let (index, value) = mailbox.try_match_next().unwrap();
        assert_eq!(value.as_small_int(), Some(i));
        mailbox.remove_at(index);
        mailbox.reset_scan();
    }
    assert!(mailbox.is_empty());
    println!("growth {}\nhot {}", growth, start.elapsed().as_micros());
}
