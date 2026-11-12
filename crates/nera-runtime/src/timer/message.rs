//! Message timers: `erlang:send_after/3` and `erlang:start_timer/3`.
//!
//! Pending timers wait in one registry ordered by deadline. The timer thread
//! that ends `receive ... after` waits also delivers them, so a pending timer
//! costs memory, not an OS thread.
//!
//! Cancellation and delivery each take a timer's message under that timer's
//! own lock; whichever comes first wins. Delivery keeps the lock until the
//! message is published and its receiver woken, so a synchronous cancellation
//! that loses never returns before the message is in the receiver's inbox.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use crate::message::MessageFragment;
use crate::process::ProcessId;

/// What a message timer sends, and to whom, when it fires.
pub(crate) struct MessageDelivery {
    pub(crate) dest: ProcessId,
    pub(crate) message: MessageFragment,
}

struct MessageTimer {
    /// The id of the Erlang reference that cancellation names.
    id: u64,
    /// Deadline in nanoseconds since the timer epoch, then registration order.
    key: (u128, u64),
    /// Present until the timer is cancelled or delivered.
    delivery: Mutex<Option<MessageDelivery>>,
}

#[derive(Default)]
struct Registry {
    /// Timers not yet due, in firing order.
    by_deadline: BTreeMap<(u128, u64), Arc<MessageTimer>>,
    /// Timers not yet cancelled or delivered. A firing timer stays here until
    /// its message is published, so a canceller finds it and waits.
    by_ref: HashMap<u64, Arc<MessageTimer>>,
    next_seq: u64,
}

#[derive(Default)]
pub(crate) struct MessageTimers {
    registry: Mutex<Registry>,
    /// Timers registered and neither cancelled nor delivered. Keeps deadlock
    /// detection from ending the program ahead of a delivery.
    pending: AtomicUsize,
}

/// Ends one delivery, even when it unwinds: forgets the timer and releases
/// its pending slot. Dropped while the timer's lock is still held.
struct Delivered<'a> {
    timers: &'a MessageTimers,
    timer: &'a Arc<MessageTimer>,
}

impl Drop for Delivered<'_> {
    fn drop(&mut self) {
        let mut registry = self.timers.registry.lock();
        // A canceller may have removed it first; never remove another timer.
        if registry.by_ref.get(&self.timer.id).is_some_and(|timer| Arc::ptr_eq(timer, self.timer)) {
            registry.by_ref.remove(&self.timer.id);
        }
        drop(registry);
        self.timers.pending.fetch_sub(1, Ordering::Release);
    }
}

impl MessageTimers {
    /// Register a timer due `delay` after `now_ns`. Returns true when it is
    /// now the earliest message timer, so the timer thread may need to wake
    /// up and shorten its wait.
    pub(crate) fn register(&self, id: u64, now_ns: u128, delay: Duration, delivery: MessageDelivery) -> bool {
        // Counted before it is visible, so deadlock detection never misses it.
        self.pending.fetch_add(1, Ordering::AcqRel);
        let mut registry = self.registry.lock();
        let key = (now_ns.saturating_add(delay.as_nanos()), registry.next_seq);
        registry.next_seq += 1;
        let timer = Arc::new(MessageTimer { id, key, delivery: Mutex::new(Some(delivery)) });
        registry.by_ref.insert(id, Arc::clone(&timer));
        registry.by_deadline.insert(key, timer);
        registry.by_deadline.first_key_value().is_some_and(|(first, _)| *first == key)
    }

    /// Cancel the timer registered under `id`. Returns the milliseconds that
    /// were left, rounded up, or None when it was already delivered or
    /// cancelled, or never existed. Waits for a delivery in progress.
    pub(crate) fn cancel(&self, id: u64, now_ns: u128) -> Option<u64> {
        let timer = {
            let mut registry = self.registry.lock();
            let timer = registry.by_ref.remove(&id)?;
            registry.by_deadline.remove(&timer.key);
            timer
        };
        let cancelled = timer.delivery.lock().take()?;
        self.pending.fetch_sub(1, Ordering::Release);
        drop(cancelled);
        let remaining_ns = timer.key.0.saturating_sub(now_ns);
        Some(u64::try_from(remaining_ns.div_ceil(1_000_000)).unwrap_or(u64::MAX))
    }

    /// Deadline of the earliest pending timer, in nanoseconds since the epoch.
    pub(crate) fn next_deadline_ns(&self) -> Option<u128> {
        self.registry.lock().by_deadline.first_key_value().map(|(key, _)| key.0)
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire) > 0
    }

    /// Deliver every timer due at `now_ns`, earliest first. `deliver` returns
    /// whether it woke the receiver; returns how many it woke.
    pub(crate) fn fire_due(&self, now_ns: u128, mut deliver: impl FnMut(MessageDelivery) -> bool) -> usize {
        let due: Vec<Arc<MessageTimer>> = {
            let mut registry = self.registry.lock();
            let mut due = Vec::new();
            while let Some(first) = registry.by_deadline.first_entry() {
                if first.key().0 > now_ns {
                    break;
                }
                due.push(first.remove());
            }
            due
        };
        let mut woken = 0;
        for timer in &due {
            let mut slot = timer.delivery.lock();
            // None: cancelled after it came due, before this delivery.
            let Some(delivery) = slot.take() else { continue };
            let _done = Delivered { timers: self, timer };
            if deliver(delivery) {
                woken += 1;
            }
        }
        woken
    }
}

/// A delivery to `pid` whose message is `pid` itself; tests use the pid as
/// the timer's label.
#[cfg(test)]
pub(crate) fn test_delivery(pid: u64) -> MessageDelivery {
    use crate::message::ImmediateTerm;
    use crate::term::ErlTerm;
    MessageDelivery {
        dest: ProcessId::new(pid),
        message: MessageFragment::Immediate(ImmediateTerm::new(ErlTerm::small_int(pid as i64))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u128 = 1_000_000;

    fn to(pid: u64) -> MessageDelivery {
        test_delivery(pid)
    }

    fn fire(timers: &MessageTimers, now_ns: u128) -> Vec<u64> {
        let mut fired = Vec::new();
        timers.fire_due(now_ns, |delivery| {
            fired.push(delivery.dest.get());
            true
        });
        fired
    }

    fn is_empty(timers: &MessageTimers) -> bool {
        let registry = timers.registry.lock();
        registry.by_ref.is_empty() && registry.by_deadline.is_empty()
    }

    // Moving timers from one sleeping thread each to a shared registry must
    // keep each one from firing before its delay, and fire them in order.
    #[test]
    fn due_timers_fire_earliest_first_and_never_early() {
        let timers = MessageTimers::default();
        assert!(timers.register(1, 0, Duration::from_millis(30), to(30)));
        assert!(timers.register(2, 0, Duration::from_millis(10), to(10)));
        assert!(!timers.register(3, 0, Duration::from_millis(20), to(20)));
        assert!(!timers.register(4, 0, Duration::from_millis(20), to(21)));
        assert_eq!(timers.next_deadline_ns(), Some(10 * MS));
        assert_eq!(fire(&timers, 10 * MS - 1), Vec::<u64>::new());
        assert_eq!(fire(&timers, 10 * MS), [10]);
        // Equal deadlines fire in registration order.
        assert_eq!(fire(&timers, 29 * MS), [20, 21]);
        assert_eq!(fire(&timers, 30 * MS), [30]);
        assert_eq!(timers.next_deadline_ns(), None);
        assert!(!timers.has_pending());
        assert!(is_empty(&timers));
    }

    #[test]
    fn cancelled_timer_never_delivers_and_releases_liveness() {
        // cow_http2_machine termination must cancel real work, not return a stub.
        let timers = MessageTimers::default();
        timers.register(17, 0, Duration::from_secs(60), to(17));
        timers.register(18, 0, Duration::ZERO, to(18));
        assert_eq!(timers.cancel(17, 3 * MS / 2), Some(59_999), "remaining time rounds up");
        assert!(timers.has_pending(), "the other timer still owns its slot");
        assert_eq!(timers.cancel(17, 2 * MS), None);
        let mut fired = Vec::new();
        timers.fire_due(u128::MAX, |delivery| {
            assert!(timers.has_pending(), "release must follow publication");
            fired.push(delivery.dest.get());
            true
        });
        assert_eq!(fired, [18]);
        assert!(!timers.has_pending());
        assert!(is_empty(&timers));
        assert_eq!(timers.cancel(18, 2 * MS), None, "a delivered timer cannot be cancelled");
    }

    #[test]
    fn delivery_wins_before_cancel_returns() {
        // Synchronous cancellation cannot overtake a delivery already publishing.
        let timers = Arc::new(MessageTimers::default());
        timers.register(19, 0, Duration::ZERO, to(19));
        let (publishing, started) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let (cancelled, cancel_result) = std::sync::mpsc::channel();
        let firing = Arc::clone(&timers);
        let worker = std::thread::spawn(move || firing.fire_due(0, |_| {
            publishing.send(()).unwrap();
            wait.recv().unwrap();
            true
        }));
        started.recv().unwrap();
        let cancelling = Arc::clone(&timers);
        let canceller = std::thread::spawn(move || cancelled.send(cancelling.cancel(19, 0)).unwrap());
        assert!(cancel_result.recv_timeout(Duration::from_millis(100)).is_err(),
                "cancel returned while the delivery was still publishing");
        release.send(()).unwrap();
        assert_eq!(cancel_result.recv().unwrap(), None);
        canceller.join().unwrap();
        assert_eq!(worker.join().unwrap(), 1);
        assert!(!timers.has_pending());
        assert!(is_empty(&timers));
    }

    #[test]
    fn delivery_that_unwinds_releases_its_timer() {
        // A failed delivery must not leave shutdown waiting forever.
        let timers = MessageTimers::default();
        timers.register(20, 0, Duration::ZERO, to(20));
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            timers.fire_due(0, |_| panic!("publication failure"));
        })).is_err());
        assert!(!timers.has_pending());
        assert!(is_empty(&timers));
    }
}
