//! `counters` module — mutable integer counter arrays (OTP 22+).
//!
//! Provides arrays of 64-bit integer counters with atomic operations.
//! Each counter array is accessed via a raw pointer stored directly in
//! the Erlang reference term — no registry lookup on the hot path.
//!
//! All counter operations are lock-free (using `AtomicI64`).
//!
//! ## Lifetime
//!
//! Counter arrays are **node-retained** in the current implementation: they live
//! until the node shuts down. The `Arc::into_raw()` pointer embedded in the
//! reference term is backed by a second `Arc` held in the global registry,
//! so the array is never freed even when all Erlang references are GC'd.
//! Unlike OTP, this implementation does not yet reclaim unreferenced arrays.

use std::collections::HashMap;
use std::sync::atomic::AtomicI64;
use std::sync::{Arc, OnceLock, RwLock};

/// A single counter array.
pub(crate) struct CounterArray {
    slots: Vec<AtomicI64>,
}

impl CounterArray {
    /// Return a shared atomic cell using the public one-based index contract.
    #[inline(always)]
    pub(crate) fn slot(&self, index: usize) -> Option<&AtomicI64> {
        self.slots.get(index.checked_sub(1)?)
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    #[inline]
    pub(crate) fn memory_size(&self) -> usize {
        std::mem::size_of_val(self.slots.as_slice())
    }
}

// SAFETY: this is the sole native type associated with this resource kind.
unsafe impl crate::resource::ReferenceResource for CounterArray {
    const KIND: crate::term::ReferenceKind = crate::term::ReferenceKind::Counters;
}

/// Global registry retaining the native allocations. Hot-path ops bypass this entirely.
struct CountersRegistry {
    map: RwLock<HashMap<u64, Arc<CounterArray>>>,
}

static REGISTRY: OnceLock<CountersRegistry> = OnceLock::new();

fn registry() -> &'static CountersRegistry {
    REGISTRY.get_or_init(|| CountersRegistry {
        map: RwLock::new(HashMap::new()),
    })
}

/// Create a new counter array with `size` slots, all initialized to 0.
/// Returns a typed handle whose pointer is stored in the reference term.
pub(crate) fn counters_new(size: usize) -> crate::resource::ResourceRef<CounterArray> {
    let mut slots = Vec::with_capacity(size);
    for _ in 0..size {
        slots.push(AtomicI64::new(0));
    }
    let array = Arc::new(CounterArray { slots });
    // Leak an Arc reference — the raw pointer stored in Erlang terms keeps it alive.
    let resource = crate::resource::ResourceRef::from_immortal_arc(Arc::clone(&array));
    // Retain the existing registry ownership.
    registry().map.write().unwrap().insert(resource.address(), array);
    resource
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn slots_use_actual_length_and_one_based_bounds() {
        let array = CounterArray {
            slots: vec![AtomicI64::new(7), AtomicI64::new(9)],
        };
        assert_eq!(array.len(), 2);
        assert_eq!(array.memory_size(), 2 * std::mem::size_of::<AtomicI64>());
        assert!(array.slot(0).is_none());
        assert!(array.slot(3).is_none());
        assert!(array.slot(usize::MAX).is_none());
        assert_eq!(array.slot(1).unwrap().load(Ordering::Relaxed), 7);
        array.slot(2).unwrap().fetch_add(1, Ordering::Relaxed);
        assert_eq!(array.slot(2).unwrap().load(Ordering::Relaxed), 10);
    }
}
