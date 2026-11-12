use super::*;
use std::{hint::black_box, time::Instant};
#[test]
#[ignore]
fn process_entry_performance_probe() {
    let scheduler = crate::scheduler::Scheduler::new(1);
    let start = Instant::now();
    for _ in 0..1_000_000 { black_box(scheduler.next_pid()); }
    println!("pid_allocate {}", start.elapsed().as_micros());
    let start = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let scheduler = &scheduler;
            scope.spawn(move || { for _ in 0..100_000 { black_box(scheduler.next_pid()); } });
        }
    });
    println!("pid_contended {}", start.elapsed().as_micros());
    let pid = ProcessId::new(17);
    let start = Instant::now();
    for _ in 0..10_000_000 {
        black_box(ErlTerm::pid(black_box(pid)).as_pid_id());
    }
    println!("encode_decode {}", start.elapsed().as_micros());
    let mut ctx = ProcessContext::new(pid);
    let start = Instant::now();
    for _ in 0..10_000_000 {
        black_box(&mut ctx).status = black_box(STATUS_YIELD);
        black_box(ctx.status);
    }
    println!("status_write {}", start.elapsed().as_micros());
    let start = Instant::now();
    for _ in 0..10_000 {
        let entry = ProcessEntry::new(Process::new(scheduler.next_pid()));
        black_box(entry);
    }
    println!("process_create {}", start.elapsed().as_micros());
    let start = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let scheduler = &scheduler;
            scope.spawn(move || {
                for _ in 0..500 {
                    let process = Process::new(scheduler.next_pid());
                    black_box(scheduler.spawn_process(process));
                }
            });
        }
    });
    println!("spawn_contended {}", start.elapsed().as_micros());

}
