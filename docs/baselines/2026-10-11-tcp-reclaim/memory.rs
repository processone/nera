use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::collections::{HashMap, VecDeque};
use std::hint::black_box;
static LIVE: AtomicUsize = AtomicUsize::new(0);
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() { LIVE.fetch_add(l.size(), Relaxed); }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l); }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, n) };
        if !q.is_null() { LIVE.fetch_sub(l.size(), Relaxed); LIVE.fetch_add(n, Relaxed); }
        q
    }
}
#[global_allocator] static ALLOC: Counting = Counting;
mod process { pub type ProcessId = u64; }
#[path = "../../../crates/nera-runtime/src/reactor/accept_waiters.rs"]
mod accept_waiters;
fn main() {
    println!("queued,listeners,old_live_bytes,new_live_bytes,extra_bytes,old_after_cancel,new_after_cancel");
    for n in [0,1,4,16,64,256,2048,32768] {
        for listeners in [1,16] {
            if listeners > n.max(1) { continue; }
            let initial = LIVE.load(Relaxed);
            let mut old: HashMap<u64, VecDeque<u64>> = HashMap::new();
            for pid in 0..n { old.entry((pid % listeners) as u64).or_default().push_back(pid as u64); }
            black_box(&old);
            let old_bytes = LIVE.load(Relaxed) - initial;
            for pid in 0..n { old.retain(|_, q| { q.retain(|&p| p != pid as u64); !q.is_empty() }); }
            let old_retained = LIVE.load(Relaxed) - initial;
            drop(old);
            assert_eq!(LIVE.load(Relaxed), initial);
            let mut new = accept_waiters::AcceptWaiters::default();
            for pid in 0..n { new.push((pid % listeners) as u64, pid as u64); }
            black_box(&new);
            let new_bytes = LIVE.load(Relaxed) - initial;
            for pid in 0..n { new.cancel(pid as u64); }
            assert_eq!(new.pop(0), None);
            new.remove(0);
            let new_retained = LIVE.load(Relaxed) - initial;
            drop(new);
            assert_eq!(LIVE.load(Relaxed), initial);
            println!("{n},{listeners},{old_bytes},{new_bytes},{},{old_retained},{new_retained}",new_bytes as isize-old_bytes as isize);
        }
    }
}
