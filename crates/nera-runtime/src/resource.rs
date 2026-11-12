//! Typed direct references to node-retained resources.
//! The reference header identifies the pointee type before any pointer is read.

use std::ptr::NonNull;
use std::sync::Arc;

use crate::heap::ProcessHeap;
use crate::term::{ErlTerm, ReferenceKind, REFERENCE_ID_WORD_INDEX, REFERENCE_TOTAL_WORDS};

/// # Safety
/// KIND must uniquely identify this concrete pointee type throughout the
/// runtime, and must not be Ordinary. FLAGS_MASK may use only header bits above
/// the subtype and two-bit kind namespace. flags() must remain immutable and
/// return only permitted bits. Descriptor dispatch relies on this mapping.
pub(crate) unsafe trait ReferenceResource: Send + Sync + 'static {
    const KIND: ReferenceKind;
    const FLAGS_MASK: u64 = 0;
    fn flags(&self) -> u64 {
        0
    }
}

pub(crate) struct ResourceRef<T: ReferenceResource> {
    pointer: NonNull<T>,
    flags: u64,
}

impl<T: ReferenceResource> ResourceRef<T> {
    /// Transfer an Arc into the existing node-retained resource lifetime.
    pub(crate) fn from_immortal_arc(owner: Arc<T>) -> Self {
        let flags = owner.flags();
        debug_assert_eq!(T::FLAGS_MASK & 0x3f, 0);
        debug_assert_eq!(flags & !T::FLAGS_MASK, 0);
        // SAFETY: Arc::into_raw is non-null and its strong reference is retained.
        Self {
            pointer: unsafe { NonNull::new_unchecked(Arc::into_raw(owner).cast_mut()) },
            flags,
        }
    }

    /// # Safety
    /// term must be a live, initialized runtime graph. Resource descriptors in
    /// that graph must originate from to_term (or a header-preserving copy), with
    /// their native resource payload and immutable flags intact. Ordinary references are rejected.
    #[inline(always)]
    pub(crate) unsafe fn from_term(term: ErlTerm) -> Option<Self> {
        let descriptor = term.as_boxed_ptr()?;
        let header = descriptor.read();
        if header & !T::FLAGS_MASK != T::KIND.header().0 {
            return None;
        }
        let pointer = NonNull::new(descriptor.add(REFERENCE_ID_WORD_INDEX).read() as *mut T)?;
        Some(Self {
            pointer,
            flags: header & T::FLAGS_MASK,
        })
    }

    #[inline(always)]
    pub(crate) fn get(&self) -> &T {
        // SAFETY: construction establishes the pointee type and node-retained
        // lifetime. T is immutable through this API and supports shared access.
        unsafe { self.pointer.as_ref() }
    }

    #[inline(always)]
    pub(crate) fn flags(&self) -> u64 {
        self.flags
    }

    pub(crate) fn address(&self) -> u64 {
        self.pointer.as_ptr() as u64
    }

    pub(crate) fn to_term(&self, heap: &mut ProcessHeap) -> ErlTerm {
        let descriptor = heap.alloc_words(REFERENCE_TOTAL_WORDS);
        // SAFETY: initialize both reserved words; the Arc allocation stays live.
        unsafe {
            descriptor.write(T::KIND.header().0 | self.flags);
            descriptor
                .add(REFERENCE_ID_WORD_INDEX)
                .write(self.address());
            ErlTerm::boxed_unchecked(descriptor)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{atomics_module::AtomicArray, counters::CounterArray, term::*};

    #[test]
    fn resource_kind_rejects_plain_and_cross_type_references_before_pointer_use() {
        let mut heap = ProcessHeap::new();
        let array = crate::atomics_module::atomics_new(1, true);
        let atomic = array.to_term(&mut heap);
        let counter = crate::counters::counters_new(1).to_term(&mut heap);
        let plain_ptr = heap.alloc_words(REFERENCE_TOTAL_WORDS);
        // SAFETY: an ordinary reference may hold any numeric identity, including
        // the same bits as a native pointer, without becoming that resource.
        let plain = unsafe {
            plain_ptr.write(ReferenceKind::Ordinary.header().0);
            plain_ptr
                .add(REFERENCE_ID_WORD_INDEX)
                .write(array.address());
            ErlTerm::boxed_unchecked(plain_ptr)
        };
        // SAFETY: all descriptor roots are live and correctly initialized.
        unsafe {
            assert!(ResourceRef::<AtomicArray>::from_term(plain).is_none());
            assert!(ResourceRef::<AtomicArray>::from_term(counter).is_none());
            assert!(ResourceRef::<CounterArray>::from_term(atomic).is_none());
            assert!(ResourceRef::<AtomicArray>::from_term(atomic).is_some());
            assert!(plain.is_reference() && atomic.is_reference() && counter.is_reference());
            assert_ne!(crate::ffi::compare::compare_terms(plain, atomic), 0);
        }
    }

    #[test]
    fn resource_identity_survives_full_gc_message_copy_and_heap_drop() {
        for signed in [true, false] {
            let mut heap = ProcessHeap::new();
            let mut term = crate::atomics_module::atomics_new(1, signed).to_term(&mut heap);
            // SAFETY: complete live roots, with no outstanding heap/graph borrow.
            unsafe {
                heap.collect_full(std::slice::from_mut(&mut term));
            }
            // SAFETY: term was just rewritten to the live collected heap.
            let fragment = unsafe { crate::message::MessageFragment::from_term(term) };
            drop(heap);
            let mut receiver = ProcessHeap::new();
            let copy = fragment.copy_to_heap(&mut receiver);
            // SAFETY: receiver owns the descriptor; the native allocation is retained.
            let resource = unsafe { ResourceRef::<AtomicArray>::from_term(copy) }.unwrap();
            assert_eq!(resource.signed(), signed);
            resource.get().slot(1).unwrap().store(42, std::sync::atomic::Ordering::Relaxed);
            assert_eq!(
                resource.get().slot(1).unwrap().load(std::sync::atomic::Ordering::Relaxed),
                42
            );
        }
    }
}
