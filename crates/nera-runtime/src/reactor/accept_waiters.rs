//! Ordered accept requests with direct lookup for process cancellation.
//!
//! Sequence numbers preserve FIFO independently of PID order. Each request is
//! indexed twice so cancellation visits only that PID's requests, including
//! duplicate requests and requests on different listeners.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::process::ProcessId;
use crate::socket::SocketHandle;

#[derive(Default)]
pub(super) struct AcceptWaiters {
    queues: HashMap<SocketHandle, BTreeMap<u64, ProcessId>>,
    by_pid: HashMap<ProcessId, HashSet<(SocketHandle, u64)>>,
    next_sequence: u64,
}

impl AcceptWaiters {
    pub(super) fn push(&mut self, listener: SocketHandle, pid: ProcessId) {
        let sequence = self.next_sequence;
        self.next_sequence = sequence.checked_add(1).expect("accept sequence exhausted");
        self.queues
            .entry(listener)
            .or_default()
            .insert(sequence, pid);
        self.by_pid
            .entry(pid)
            .or_default()
            .insert((listener, sequence));
    }

    pub(super) fn pop(&mut self, listener: SocketHandle) -> Option<ProcessId> {
        let queue = self.queues.get_mut(&listener)?;
        let (sequence, pid) = queue.pop_first()?;
        if queue.is_empty() {
            self.queues.remove(&listener);
        }
        self.unindex(pid, listener, sequence);
        self.compact();
        Some(pid)
    }

    pub(super) fn cancel(&mut self, pid: ProcessId) {
        let Some(requests) = self.by_pid.remove(&pid) else {
            return;
        };
        for (listener, sequence) in requests {
            let queue = self
                .queues
                .get_mut(&listener)
                .expect("indexed accept queue");
            queue.remove(&sequence);
            if queue.is_empty() {
                self.queues.remove(&listener);
            }
        }
        self.compact();
    }

    pub(super) fn remove(&mut self, listener: SocketHandle) {
        if let Some(queue) = self.queues.remove(&listener) {
            for (sequence, pid) in queue {
                self.unindex(pid, listener, sequence);
            }
        }
        self.compact();
    }

    fn compact(&mut self) {
        // Hysteresis: leave small caches alone and shrink only below 1/8
        // occupancy, to about twice the live size. Geometric reductions avoid
        // rebuilding on each cancellation or oscillating near the threshold.
        if should_shrink(self.queues.len(), self.queues.capacity()) {
            self.queues.shrink_to(self.queues.len().saturating_mul(2));
        }
        if should_shrink(self.by_pid.len(), self.by_pid.capacity()) {
            self.by_pid.shrink_to(self.by_pid.len().saturating_mul(2));
        }
    }

    fn unindex(&mut self, pid: ProcessId, listener: SocketHandle, sequence: u64) {
        let requests = self.by_pid.get_mut(&pid).expect("indexed accept process");
        requests.remove(&(listener, sequence));
        if requests.is_empty() {
            self.by_pid.remove(&pid);
        } else if should_shrink(requests.len(), requests.capacity()) {
            requests.shrink_to(requests.len().saturating_mul(2));
        }
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.queues.is_empty() && self.by_pid.is_empty()
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.queues.values().map(BTreeMap::len).sum()
    }

    #[cfg(test)]
    pub(super) fn contains(&self, pid: ProcessId) -> bool {
        self.by_pid.contains_key(&pid)
    }
}

fn should_shrink(len: usize, capacity: usize) -> bool {
    capacity > 256 && len <= capacity / 8
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::VecDeque;

    fn handle(index: u64) -> SocketHandle {
        SocketHandle::try_from_raw((1 << 32) | index).unwrap()
    }

    #[test]
    fn cancellation_preserves_fifo_and_cleans_both_indexes() {
        let mut q = AcceptWaiters::default();
        for (listener, pid) in [(1, 8), (1, 2), (1, 8), (1, 5), (2, 8), (2, 9)] {
            q.push(handle(listener), crate::process::ProcessId::new(pid));
        }
        q.cancel(crate::process::ProcessId::new(8));
        q.cancel(crate::process::ProcessId::new(8)); // idempotent, including absent PIDs
        assert_eq!(q.pop(handle(1)), Some(crate::process::ProcessId::new(2)));
        assert_eq!(q.pop(handle(1)), Some(crate::process::ProcessId::new(5)));
        assert_eq!(q.pop(handle(1)), None);
        assert_eq!(q.pop(handle(2)), Some(crate::process::ProcessId::new(9)));
        assert!(q.is_empty());
        q.push(handle(1), crate::process::ProcessId::new(8));
        q.push(handle(2), crate::process::ProcessId::new(8));
        q.remove(handle(1));
        q.push(handle(1), crate::process::ProcessId::new(5)); // reuse after deregistration
        q.cancel(crate::process::ProcessId::new(8));
        assert_eq!(q.pop(handle(1)), Some(crate::process::ProcessId::new(5)));
        assert!(q.is_empty());
    }

    #[test]
    fn large_peaks_release_spare_capacity_without_losing_survivors() {
        for path in ["cancel", "pop", "remove"] {
            let mut q = AcceptWaiters::default();
            for pid in 0..32768 {
                q.push(handle(pid), crate::process::ProcessId::new(pid));
            }
            for pid in 0..32704 {
                match path {
                    "cancel" => q.cancel(crate::process::ProcessId::new(pid)),
                    "pop" => assert_eq!(q.pop(handle(pid)), Some(crate::process::ProcessId::new(pid))),
                    _ => q.remove(handle(pid)),
                }
            }
            assert_eq!(q.len(), 64);
            assert!(
                q.by_pid.capacity() <= 1024,
                "{path}: PID index retained peak capacity"
            );
            assert!(
                q.queues.capacity() <= 1024,
                "{path}: listener index retained peak capacity"
            );
            for pid in 32704..32768 {
                assert_eq!(q.pop(handle(pid)), Some(crate::process::ProcessId::new(pid)));
            }
            assert!(q.is_empty());
            assert!(q.by_pid.capacity() <= 256);
            assert!(q.queues.capacity() <= 256);
            q.push(handle(1), crate::process::ProcessId::new(7));
            q.push(handle(1), crate::process::ProcessId::new(3));
            assert_eq!(q.pop(handle(1)), Some(crate::process::ProcessId::new(7)));
            assert_eq!(q.pop(handle(1)), Some(crate::process::ProcessId::new(3)));
        }
        // A PID may have many indexed requests; reclaim that nested set too.
        let mut q = AcceptWaiters::default();
        for _ in 0..32768 {
            q.push(handle(1), crate::process::ProcessId::new(7));
        }
        for _ in 0..32704 {
            assert_eq!(q.pop(handle(1)), Some(crate::process::ProcessId::new(7)));
        }
        assert_eq!(q.len(), 64);
        assert!(q.by_pid[&crate::process::ProcessId::new(7)].capacity() <= 1024);
        q.cancel(crate::process::ProcessId::new(7));
        assert!(q.is_empty());
    }

    proptest! {
        #[test]
        fn operations_match_fifo_reference(ops in prop::collection::vec((0u8..4, 0u64..8, 0u64..32), 0..1000)) {
            let mut actual = AcceptWaiters::default();
            let mut reference: HashMap<SocketHandle, VecDeque<ProcessId>> = HashMap::new();
            for (op, listener, pid) in ops {
                let listener = handle(listener);
                match op {
                    0 => {
                        actual.push(listener, crate::process::ProcessId::new(pid));
                        reference.entry(listener).or_default().push_back(crate::process::ProcessId::new(pid));
                    }
                    1 => {
                        actual.cancel(crate::process::ProcessId::new(pid));
                        for queue in reference.values_mut() { queue.retain(|&p| p != crate::process::ProcessId::new(pid)); }
                    }
                    2 => {
                        prop_assert_eq!(actual.pop(listener), reference.get_mut(&listener).and_then(VecDeque::pop_front));
                    }
                    _ => { actual.remove(listener); reference.remove(&listener); }
                }
                reference.retain(|_, q| !q.is_empty());
                prop_assert_eq!(actual.len(), reference.values().map(VecDeque::len).sum::<usize>());
                prop_assert_eq!(actual.is_empty(), reference.is_empty());
                let expected: HashSet<_> = reference.values().flat_map(|q| q.iter().copied()).collect();
                prop_assert_eq!(actual.by_pid.keys().copied().collect::<HashSet<_>>(), expected);
                // Verify every reverse entry, not just the set of live PIDs.
                let indexed: HashSet<_> = actual.by_pid.iter().flat_map(|(&p, refs)| refs.iter().map(move |&(l, s)| (l, s, p))).collect();
                let queued: HashSet<_> = actual.queues.iter().flat_map(|(&l, q)| q.iter().map(move |(&s, &p)| (l, s, p))).collect();
                prop_assert_eq!(indexed, queued);
            }
            for index in 0..8 {
                let listener = handle(index);
                while let Some(expected) = reference.get_mut(&listener).and_then(VecDeque::pop_front) {
                    prop_assert_eq!(actual.pop(listener), Some(expected));
                }
                prop_assert_eq!(actual.pop(listener), None);
            }
            prop_assert!(actual.is_empty());
        }
    }
}
