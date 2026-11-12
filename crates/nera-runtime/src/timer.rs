//! Timer wheel for `receive...after` timeouts and message timers.
//!
//! Replaces the polling-based timeout check in the scheduler worker loop
//! with a min-heap of timer entries and a dedicated timer thread. Processes
//! waiting on a `receive...after` deadline register a timer entry;
//! `erlang:send_after/3` and `erlang:start_timer/3` register a message timer
//! (see `message`). The timer thread sleeps until the earliest deadline of
//! either kind, then wakes the process or delivers the message.
//!
//! Receive-timer cancellation is lazy: when a process is woken by a message
//! before its timeout fires, the `pid_to_timer` mapping is removed. The stale
//! heap entry is skipped on the next `pop_expired` call.

mod message;
mod wait;

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::process::ProcessId;
pub(crate) use message::MessageDelivery;
use message::MessageTimers;
use wait::{Precision, Waiter};

// ---------------------------------------------------------------------------
// TimerEntry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TimerEntry {
    pub id: u64,
    pub deadline_ms: u64,
    pub owner_pid: ProcessId,
}

// Ordered by deadline ascending (min-heap).
// BinaryHeap is a max-heap, so we reverse the comparison.
impl Eq for TimerEntry {}

impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse: smaller deadline = higher priority in BinaryHeap
        other
            .deadline_ms
            .cmp(&self.deadline_ms)
            .then_with(|| other.id.cmp(&self.id))
    }
}

impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// ---------------------------------------------------------------------------
// TimerWheel
// ---------------------------------------------------------------------------

/// A min-heap based timer manager. Supports O(log n) insert, O(log n) pop,
/// and O(1) lazy cancellation.
pub struct TimerWheel {
    heap: Mutex<BinaryHeap<TimerEntry>>,
    next_id: AtomicU64,
    /// Maps each process to its currently active timer id.
    /// A process can have at most one active receive timer.
    pid_to_timer: Mutex<HashMap<ProcessId, u64>>,
    /// Wakes the timer thread when a new (possibly earlier) timer is
    /// inserted or when shutdown is requested.
    waiter: Waiter,
    /// Number of active entries in pid_to_timer. Allows cancel_for_pid and
    /// remove_for_pid to skip the Mutex when no timers are active (the
    /// common case for pure message-passing workloads).
    active_count: std::sync::atomic::AtomicUsize,
    /// `send_after`/`start_timer` timers, keyed by their unique Erlang
    /// reference. Independent of a process's receive timer and its
    /// cancellation.
    messages: MessageTimers,
}

impl TimerWheel {
    pub fn new() -> Self {
        TimerWheel {
            heap: Mutex::new(BinaryHeap::new()),
            next_id: AtomicU64::new(1),
            pid_to_timer: Mutex::new(HashMap::new()),
            waiter: Waiter::new(),
            active_count: std::sync::atomic::AtomicUsize::new(0),
            messages: MessageTimers::default(),
        }
    }

    /// Register a `send_after`/`start_timer` timer under its reference id,
    /// due after `delay`. The timer thread delivers it; no thread is started.
    pub(crate) fn register_message_timer(&self, id: u64, delay: Duration, delivery: MessageDelivery) {
        if self.messages.register(id, current_time_ns(), delay, delivery) {
            // The timer thread may be sleeping past this new earliest deadline.
            self.waiter.wake();
        }
    }

    /// Returns the milliseconds left, or None once delivered or cancelled. A
    /// synchronous cancellation cannot return before a winning delivery has
    /// published its message and wake. It never retracts an already sent message.
    pub(crate) fn cancel_message_timer(&self, id: u64) -> Option<u64> {
        self.messages.cancel(id, current_time_ns())
    }

    /// Deliver every message timer now due, earliest first. `deliver`
    /// publishes one message and returns whether it woke the receiver;
    /// returns how many it woke. Called by the timer thread.
    pub(crate) fn fire_due_message_timers(&self, deliver: impl FnMut(MessageDelivery) -> bool) -> usize {
        self.messages.fire_due(current_time_ns(), deliver)
    }

    /// Insert a timer for a process. Returns the timer id.
    /// Wakes the timer thread so it can recalculate its sleep duration.
    pub fn insert(&self, deadline_ms: u64, pid: ProcessId) -> u64 {
        let id = self.next_id.fetch_add(1, AtomicOrdering::Relaxed);
        let entry = TimerEntry {
            id,
            deadline_ms,
            owner_pid: pid,
        };
        // Publish the heap entry and its active mapping atomically with
        // respect to pop_expired, which takes these locks in the same order.
        // An already-due entry must not be discarded as cancelled while
        // insert is still between the two updates.
        let mut heap = self.heap.lock();
        heap.push(entry);
        if self.pid_to_timer.lock().insert(pid, id).is_none() {
            self.active_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        drop(heap);
        // Wake timer thread to recalculate sleep.
        self.waiter.wake();
        id
    }

    /// Cancel the active timer for a process (lazy cancellation).
    /// Called when a message wakes the process before its timeout fires.
    /// Fast path: skips the Mutex entirely when no timers are active.
    pub fn cancel_for_pid(&self, pid: ProcessId) {
        if self.active_count.load(std::sync::atomic::Ordering::Relaxed) == 0 {
            return;
        }
        if self.pid_to_timer.lock().remove(&pid).is_some() {
            self.active_count
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Remove all timers for a process (process exit cleanup).
    /// Equivalent to cancel — the heap entry becomes stale.
    pub fn remove_for_pid(&self, pid: ProcessId) {
        if self.active_count.load(std::sync::atomic::Ordering::Relaxed) == 0 {
            return;
        }
        if self.pid_to_timer.lock().remove(&pid).is_some() {
            self.active_count
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Pop all expired timer entries whose deadline <= `now_ms` and whose
    /// pid still maps to that timer id (not cancelled).
    pub fn pop_expired(&self, now_ms: u64) -> Vec<TimerEntry> {
        let mut heap = self.heap.lock();
        let mut pid_map = self.pid_to_timer.lock();
        let mut expired = Vec::new();

        loop {
            let should_pop = match heap.peek() {
                Some(entry) => entry.deadline_ms <= now_ms,
                None => false,
            };
            if !should_pop {
                break;
            }
            let entry = heap.pop().unwrap();
            // Check if this timer is still active (not cancelled/replaced).
            if let Some(&current_id) = pid_map.get(&entry.owner_pid) {
                if current_id == entry.id {
                    pid_map.remove(&entry.owner_pid);
                    self.active_count
                        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    expired.push(entry);
                }
                // else: stale entry (pid has a newer timer), skip
            }
            // else: cancelled, skip
        }

        expired
    }

    /// Whether `pid` still has a timer that has not expired, been cancelled
    /// or been removed. Reads the map pop_expired clears before its wake.
    pub fn has_pending_for(&self, pid: ProcessId) -> bool {
        self.pid_to_timer.lock().contains_key(&pid)
    }

    /// Heap entries, cancelled ones included, for tests that check none pile up.
    #[cfg(test)]
    pub(crate) fn queued_entries(&self) -> usize {
        self.heap.lock().len()
    }

    /// Return the deadline of the earliest pending timer, or None.
    pub fn next_deadline(&self) -> Option<u64> {
        self.heap.lock().peek().map(|e| e.deadline_ms)
    }

    /// Check for active receive timers or undelivered message timers.
    pub fn has_pending_timers(&self) -> bool {
        self.active_count.load(std::sync::atomic::Ordering::Relaxed) > 0
            || self.messages.has_pending()
    }

    /// Sleep until the earliest timer is due, so the caller can pop it.
    /// Returns early when a timer is inserted or shutdown is notified; with
    /// no timer pending, sleeps at most `idle`. May also return early for no
    /// reason: callers recheck shutdown and expired timers each time.
    pub fn wait_for_next_deadline(&self, idle: Duration, shutdown: &std::sync::atomic::AtomicBool) {
        self.wait_for_next_deadline_with(idle, shutdown, || {});
    }

    fn wait_for_next_deadline_with(&self, idle: Duration, shutdown: &std::sync::atomic::AtomicBool,
                                   before_wait: impl FnOnce()) {
        // insert() and notify() leave a pending wake: one that lands after
        // these reads ends the wait below at once instead of being lost.
        if shutdown.load(AtomicOrdering::SeqCst) {
            return;
        }
        let receive = self.next_deadline()
            .map(|deadline| Duration::from_millis(deadline.saturating_sub(current_time_ms())));
        let message = self.messages.next_deadline_ns().map(|deadline| {
            let left = deadline.saturating_sub(current_time_ns());
            Duration::from_nanos(u64::try_from(left).unwrap_or(u64::MAX))
        });
        let (duration, precision) = match receive.into_iter().chain(message).min() {
            Some(wait) if wait.is_zero() => return,
            Some(wait) => (wait, Precision::Exact),
            None => (idle, Precision::Relaxed),
        };
        before_wait();
        self.waiter.wait(duration, precision);
    }

    /// Wake the timer thread (e.g., on shutdown).
    pub fn notify(&self) {
        self.waiter.wake();
    }
}

impl Default for TimerWheel {
    fn default() -> Self {
        Self::new()
    }
}

/// Monotonic epoch — initialized on first call.
static TIMER_EPOCH: OnceLock<Instant> = OnceLock::new();

/// Get the current monotonic time as milliseconds since the timer epoch.
/// Immune to wall-clock adjustments (NTP, manual changes).
///
/// Always returns >= 1 so that deadline 0 remains a valid "no timeout"
/// sentinel in ProcessContext.receive_timeout_deadline.
pub fn current_time_ms() -> u64 {
    let epoch = TIMER_EPOCH.get_or_init(Instant::now);
    (epoch.elapsed().as_millis() as u64).max(1)
}

/// Nanoseconds since the same epoch. Message timers keep this precision so
/// that none fires before its full delay has passed.
pub(crate) fn current_time_ns() -> u128 {
    TIMER_EPOCH.get_or_init(Instant::now).elapsed().as_nanos()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // NERA-42: force shutdown+notify before the timer reads shutdown.
    #[test]
    fn shutdown_before_timer_wait_is_not_lost() {
        let wheel = Arc::new(TimerWheel::new());
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let barrier = Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            let waiter = scope.spawn(|| {
                barrier.wait();
                wheel.wait_for_next_deadline_with(Duration::ZERO, &shutdown,
                    || panic!("timer attempted a wait after shutdown notification"));
            });
            shutdown.store(true, AtomicOrdering::SeqCst);
            wheel.notify();
            barrier.wait();
            waiter.join().unwrap();
        });
    }

    // A timer inserted after the timer thread read its next deadline must
    // end that wait. Before, the wake was lost and the thread slept on to
    // its earlier deadline or 100 ms idle bound, so the timer fired late.
    #[test]
    fn insert_after_deadline_read_wakes_timer_wait() {
        const IDLE: Duration = Duration::from_secs(30);
        let wheel = TimerWheel::new();
        let shutdown = std::sync::atomic::AtomicBool::new(false);
        let started = Instant::now();
        wheel.wait_for_next_deadline_with(IDLE, &shutdown, || {
            wheel.insert(current_time_ms() + 5, ProcessId::new(1));
        });
        assert!(started.elapsed() < IDLE / 3, "timer wait missed an insert");
        // Later waits sleep until that deadline, not for the idle bound.
        let mut fired = Vec::new();
        while fired.is_empty() && started.elapsed() < IDLE / 3 {
            wheel.wait_for_next_deadline(IDLE, &shutdown);
            fired = wheel.pop_expired(current_time_ms());
        }
        assert!(started.elapsed() < IDLE / 3, "timer wait ignored a pending deadline");
        assert_eq!(fired.len(), 1);
    }

    #[test]
    fn due_timer_skips_the_wait() {
        let wheel = TimerWheel::new();
        let shutdown = std::sync::atomic::AtomicBool::new(false);
        wheel.insert(current_time_ms(), ProcessId::new(1));
        wheel.wait_for_next_deadline_with(Duration::from_secs(30), &shutdown,
            || panic!("timer waited although a deadline was already due"));
    }

    // A message timer registered while the timer thread computes its sleep
    // must end that sleep, and later waits must end at its deadline, not the
    // idle bound. The timer thread now delivers every send_after/start_timer.
    #[test]
    fn message_timer_registered_after_deadline_read_wakes_timer_wait() {
        const IDLE: Duration = Duration::from_secs(30);
        let wheel = TimerWheel::new();
        let shutdown = std::sync::atomic::AtomicBool::new(false);
        let started = Instant::now();
        wheel.wait_for_next_deadline_with(IDLE, &shutdown, || {
            wheel.register_message_timer(1, Duration::from_millis(5), message::test_delivery(1));
        });
        assert!(started.elapsed() < IDLE / 3, "timer wait missed a message timer");
        let mut delivered = 0;
        while delivered == 0 && started.elapsed() < IDLE / 3 {
            wheel.wait_for_next_deadline(IDLE, &shutdown);
            delivered = wheel.fire_due_message_timers(|_| true);
        }
        assert!(started.elapsed() >= Duration::from_millis(5), "message timer fired early");
        assert!(started.elapsed() < IDLE / 3, "timer wait ignored a message deadline");
        assert_eq!(delivered, 1);
        assert!(!wheel.has_pending_timers());
    }

    #[test]
    fn message_timers_remain_pending_until_delivered() {
        let timers = TimerWheel::new();
        timers.register_message_timer(1, Duration::ZERO, message::test_delivery(1));
        timers.register_message_timer(2, Duration::from_secs(60), message::test_delivery(2));
        let pid = ProcessId::new(1);
        timers.insert(1, pid);
        timers.cancel_for_pid(pid);
        timers.remove_for_pid(pid);
        assert!(timers.pop_expired(u64::MAX).is_empty());
        assert!(timers.has_pending_timers(), "receive cancellation must not cancel sends");
        assert_eq!(timers.fire_due_message_timers(|_| true), 1);
        assert!(timers.has_pending_timers(), "each timer owns its pending slot");
        assert!(timers.cancel_message_timer(2).is_some());
        assert!(!timers.has_pending_timers());
    }

    #[test]
    fn concurrent_expiration_does_not_lose_published_timers() {
        // Timers may already be due when the scheduler registers them.
        // Expiration must never see a heap entry before its active mapping.
        const COUNT: u64 = 20_000;
        let timers = Arc::new(TimerWheel::new());
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader = Arc::clone(&timers);
        let finished = Arc::clone(&done);
        let consumer = std::thread::spawn(move || {
            let mut fired = Vec::new();
            while !finished.load(AtomicOrdering::Acquire) {
                fired.extend(reader.pop_expired(u64::MAX));
                std::thread::yield_now();
            }
            fired.extend(reader.pop_expired(u64::MAX));
            fired
        });
        for pid in 1..=COUNT {
            timers.insert(1, crate::process::ProcessId::new(pid));
        }
        done.store(true, AtomicOrdering::Release);
        let mut fired: Vec<_> = consumer.join().unwrap().into_iter()
            .map(|entry| entry.owner_pid).collect();
        fired.sort_unstable();
        assert_eq!(fired.len(), COUNT as usize, "expiration lost an inserted timer");
        assert_eq!(fired, (1..=COUNT).map(crate::process::ProcessId::new).collect::<Vec<_>>());
        assert!(!timers.has_pending_timers());
    }

    #[test]
    fn test_insert_and_pop_expired() {
        let tw = TimerWheel::new();
        let now = 1000u64;

        tw.insert(now + 100, crate::process::ProcessId::new(1)); // deadline 1100
        tw.insert(now + 50, crate::process::ProcessId::new(2)); // deadline 1050
        tw.insert(now + 200, crate::process::ProcessId::new(3)); // deadline 1200

        // At now=1060, only PID 2 (deadline 1050) should fire.
        let expired = tw.pop_expired(1060);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].owner_pid, crate::process::ProcessId::new(2));

        // At now=1150, PID 1 (deadline 1100) should fire.
        let expired = tw.pop_expired(1150);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].owner_pid, crate::process::ProcessId::new(1));

        // At now=1250, PID 3 (deadline 1200) should fire.
        let expired = tw.pop_expired(1250);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].owner_pid, crate::process::ProcessId::new(3));

        // Nothing left.
        let expired = tw.pop_expired(9999);
        assert!(expired.is_empty());
    }

    #[test]
    fn test_pop_multiple_at_once() {
        let tw = TimerWheel::new();

        tw.insert(100, crate::process::ProcessId::new(1));
        tw.insert(200, crate::process::ProcessId::new(2));
        tw.insert(150, crate::process::ProcessId::new(3));

        // All three expired at now=300.
        let expired = tw.pop_expired(300);
        assert_eq!(expired.len(), 3);
        // Should come out in deadline order (min-heap).
        assert_eq!(expired[0].owner_pid, crate::process::ProcessId::new(1));
        assert_eq!(expired[1].owner_pid, crate::process::ProcessId::new(3));
        assert_eq!(expired[2].owner_pid, crate::process::ProcessId::new(2));
    }

    #[test]
    fn test_cancel_before_expiry() {
        let tw = TimerWheel::new();
        tw.insert(100, crate::process::ProcessId::new(1));
        tw.insert(200, crate::process::ProcessId::new(2));

        // Cancel PID 1 before it fires.
        tw.cancel_for_pid(crate::process::ProcessId::new(1));

        let expired = tw.pop_expired(300);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].owner_pid, crate::process::ProcessId::new(2));
    }

    #[test]
    fn test_stale_entry_replaced() {
        let tw = TimerWheel::new();
        let _id1 = tw.insert(100, crate::process::ProcessId::new(1)); // old timer for PID 1
        let _id2 = tw.insert(300, crate::process::ProcessId::new(1)); // new timer for PID 1 (replaces old)

        // At now=200, old entry (deadline 100) is popped but it's stale
        // because PID 1's active timer is id2 (deadline 300).
        let expired = tw.pop_expired(200);
        assert!(expired.is_empty());

        // At now=400, the new timer fires.
        let expired = tw.pop_expired(400);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].owner_pid, crate::process::ProcessId::new(1));
    }

    #[test]
    fn test_has_pending_timers() {
        let tw = TimerWheel::new();
        assert!(!tw.has_pending_timers());

        tw.insert(100, crate::process::ProcessId::new(1));
        assert!(tw.has_pending_timers());

        tw.cancel_for_pid(crate::process::ProcessId::new(1));
        assert!(!tw.has_pending_timers());
    }

    #[test]
    fn test_remove_for_pid() {
        let tw = TimerWheel::new();
        tw.insert(100, crate::process::ProcessId::new(1));
        tw.insert(200, crate::process::ProcessId::new(2));

        tw.remove_for_pid(crate::process::ProcessId::new(1));
        assert!(tw.has_pending_timers()); // PID 2 still pending

        tw.remove_for_pid(crate::process::ProcessId::new(2));
        assert!(!tw.has_pending_timers());

        // Expired entries are skipped.
        let expired = tw.pop_expired(999);
        assert!(expired.is_empty());
    }

    #[test]
    fn test_next_deadline() {
        let tw = TimerWheel::new();
        assert_eq!(tw.next_deadline(), None);

        tw.insert(200, crate::process::ProcessId::new(1));
        assert_eq!(tw.next_deadline(), Some(200));

        tw.insert(100, crate::process::ProcessId::new(2));
        assert_eq!(tw.next_deadline(), Some(100));

        // Pop the earliest.
        let _ = tw.pop_expired(150);
        assert_eq!(tw.next_deadline(), Some(200));
    }

    #[test]
    fn test_concurrent_insert_cancel() {
        use std::thread;

        let tw = Arc::new(TimerWheel::new());

        let mut handles = Vec::new();
        for i in 0..10u64 {
            let tw_clone = Arc::clone(&tw);
            handles.push(thread::spawn(move || {
                for j in 0..100u64 {
                    let pid = i * 1000 + j;
                    tw_clone.insert(pid + 1000, crate::process::ProcessId::new(pid));
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Cancel half of them.
        for i in 0..10u64 {
            for j in 0..50u64 {
                let pid = i * 1000 + j;
                tw.cancel_for_pid(crate::process::ProcessId::new(pid));
            }
        }

        // Pop all.
        let expired = tw.pop_expired(u64::MAX);
        // 10 threads * 100 timers = 1000 inserted.
        // 10 * 50 = 500 cancelled.
        // But: each PID only has ONE active timer (the last insert wins).
        // PIDs 0..49 in each batch were cancelled. PIDs 50..99 remain.
        assert_eq!(expired.len(), 500);
    }

    #[test]
    fn test_after_zero_no_timer() {
        // Simulates `after 0` behavior: deadline = now, checked immediately.
        // In practice the compiled code handles this without ever reaching
        // STATUS_RECEIVE_WAIT, so no timer is registered. This test verifies
        // that even if a timer with deadline <= now were inserted, it fires
        // immediately.
        let tw = TimerWheel::new();
        let now = current_time_ms();
        tw.insert(now, crate::process::ProcessId::new(1));
        let expired = tw.pop_expired(now);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].owner_pid, crate::process::ProcessId::new(1));
    }
}
