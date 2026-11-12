//! Allocation traffic, not live memory. Enabled only in the measurement build.
use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;
thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[u64; 4]> = const { Cell::new([0; 4]) };
}
fn add(index: usize, bytes: usize) {
    ACTIVE.with(|a| {
        if a.get() {
            COUNTS.with(|c| {
                let mut v = c.get();
                v[index] += 1;
                v[index + 1] += bytes as u64;
                c.set(v);
            });
        }
    });
}
pub fn heap(bytes: usize) {
    add(2, bytes);
}
pub fn start() {
    COUNTS.with(|c| c.set([0; 4]));
    ACTIVE.with(|a| a.set(true));
}
pub fn stop() -> [u64; 4] {
    ACTIVE.with(|a| a.set(false));
    COUNTS.with(Cell::get)
}
pub struct Counting;
// SAFETY: every operation forwards the original pointer/layout to jemalloc.
// Counters use nonallocating TLS and do not access allocated storage.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        add(0, l.size());
        unsafe { tikv_jemallocator::Jemalloc.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        add(0, l.size());
        unsafe { tikv_jemallocator::Jemalloc.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        add(0, size);
        unsafe { tikv_jemallocator::Jemalloc.realloc(p, l, size) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { tikv_jemallocator::Jemalloc.dealloc(p, l) }
    }
}
