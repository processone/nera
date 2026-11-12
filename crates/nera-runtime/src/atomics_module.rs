//! `atomics` module — atomic 64-bit integer arrays (OTP 22+).
//!
//! Provides arrays of 64-bit integers with atomic operations including
//! compare-and-swap (CAS). Each array is accessed via a raw pointer stored
//! directly in the Erlang reference term — no registry lookup on the hot path.
//!
//! ## Lifetime
//!
//! Atomic arrays are **node-retained** in the current implementation: they live
//! until the node shuts down. The `Arc::into_raw()` pointer embedded in the
//! reference term is backed by a second `Arc` held in the global registry,
//! so the array is never freed even when all Erlang references are GC'd.
//! Resource reclamation is separate from payload typing; unlike OTP, arrays
//! currently remain allocated until node shutdown.

use std::collections::HashMap;
use std::sync::atomic::AtomicI64;
use std::sync::{Arc, OnceLock, RwLock};

/// A single atomic integer array.
pub(crate) struct AtomicArray {
    slots: Vec<AtomicI64>,
    signed: bool,
}

impl AtomicArray {
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
unsafe impl crate::resource::ReferenceResource for AtomicArray {
    const KIND: crate::term::ReferenceKind = crate::term::ReferenceKind::Atomics;
    const FLAGS_MASK: u64 = Self::UNSIGNED_FLAG;
    fn flags(&self) -> u64 { if self.signed { 0 } else { Self::UNSIGNED_FLAG } }
}

impl AtomicArray {
    // Header bit 6 lies above the subtype/kind namespace and is copied by GC/ETS.
    pub(crate) const UNSIGNED_FLAG: u64 = 1 << 6;
}

impl crate::resource::ResourceRef<AtomicArray> {
    #[inline(always)]
    pub(crate) fn signed(&self) -> bool { self.flags() & AtomicArray::UNSIGNED_FLAG == 0 }
}

/// Global registry retaining the native allocations.
struct AtomicsRegistry {
    map: RwLock<HashMap<u64, Arc<AtomicArray>>>,
}

static REGISTRY: OnceLock<AtomicsRegistry> = OnceLock::new();

fn registry() -> &'static AtomicsRegistry {
    REGISTRY.get_or_init(|| AtomicsRegistry {
        map: RwLock::new(HashMap::new()),
    })
}

/// Create a new atomic array and return its typed, node-retained handle.
pub(crate) fn atomics_new(size: usize, signed: bool) -> crate::resource::ResourceRef<AtomicArray> {
    let mut slots = Vec::with_capacity(size);
    for _ in 0..size {
        slots.push(AtomicI64::new(0));
    }
    let array = Arc::new(AtomicArray { slots, signed });
    let resource = crate::resource::ResourceRef::from_immortal_arc(Arc::clone(&array));
    registry().map.write().unwrap().insert(resource.address(), array);
    resource
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn slots_use_actual_length_and_one_based_bounds() {
        let array = AtomicArray {
            slots: vec![AtomicI64::new(7), AtomicI64::new(9)],
            signed: true,
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
