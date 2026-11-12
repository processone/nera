use super::*;
use std::{hint::black_box, time::Instant};
#[test]
#[ignore]
fn process_entry_performance_probe() {
    for threads in [1, 2, 4, 8] {
        let scheduler = crate::scheduler::Scheduler::new(threads);
        let barrier = std::sync::Barrier::new(threads + 1);
        std::thread::scope(|scope| {
            for _ in 0..threads {
                let scheduler = &scheduler;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    for _ in 0..(800_000 / threads) { black_box(scheduler.next_pid()); }
                });
            }
            let start = Instant::now();
            barrier.wait();
            // scope joins before this outer measurement ends below.
            black_box(start);
        });
        // Separate synchronized timed run, reusing a live allocator.
        let start = Instant::now();
        std::thread::scope(|scope| {
            for _ in 0..threads {
                let scheduler = &scheduler;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    for _ in 0..(800_000 / threads) { black_box(scheduler.next_pid()); }
                });
            }
            barrier.wait();
        });
        println!("pid_{threads} {}", start.elapsed().as_micros());
    }
}
