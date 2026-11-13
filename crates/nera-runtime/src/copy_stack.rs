//! Depth-first copying of term graphs without native recursion.
//!
//! A copier allocates each object it meets in its target first, then queues
//! the object's term slots on a [`SlotStack`] instead of copying them in a
//! nested call. Nesting depth costs one queued entry per open level on the
//! heap rather than a native stack frame, so a term nested hundreds of
//! thousands of levels deep (an iolist built by prepending, a chain of
//! tuples) copies as on BEAM. Message passing, spawned closure environments,
//! ETS and persistent terms copy through [`copy_graph`]; the garbage
//! collector's copier in `heap.rs` runs the same loop inline.

use std::cell::Cell;
use std::thread::LocalKey;

use crate::heap::is_plain_immediate;

/// Most entries a spare work-stack buffer keeps between uses; a buffer grown
/// past this by a deeply nested term is freed instead.
const SPARE_CAPACITY: usize = 256;

/// A work stack that takes its thread's spare buffer on its first push and
/// gives it back when dropped, so working through an ordinary term
/// allocates nothing, and a term with nothing to queue does not touch the
/// spare. A nested stack of the same kind finds the spare taken and
/// allocates its own buffer.
pub(crate) struct WorkStack<T: 'static> {
    items: Vec<T>,
    spare: &'static LocalKey<Cell<Vec<T>>>,
}

impl<T: 'static> WorkStack<T> {
    /// An empty stack that will use the spare buffer `spare` when it is free.
    pub(crate) fn new(spare: &'static LocalKey<Cell<Vec<T>>>) -> Self {
        WorkStack { items: Vec::new(), spare }
    }

    #[inline]
    pub(crate) fn push(&mut self, item: T) {
        if self.items.capacity() == 0 {
            self.items = self.spare.try_with(Cell::take).unwrap_or_default();
        }
        self.items.push(item);
    }

    #[inline]
    pub(crate) fn pop(&mut self) -> Option<T> {
        self.items.pop()
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }
}

impl<T: 'static> Drop for WorkStack<T> {
    fn drop(&mut self) {
        let capacity = self.items.capacity();
        if capacity != 0 && capacity <= SPARE_CAPACITY {
            self.items.clear();
            let items = std::mem::take(&mut self.items);
            // During thread teardown the spare is gone; the buffer is freed.
            let _ = self.spare.try_with(|spare| spare.set(items));
        }
    }
}

thread_local! {
    static SPARE_SLOTS: Cell<Vec<PendingSlots>> = const { Cell::new(Vec::new()) };
}

/// Term slots of an object already allocated in the target whose values are
/// still to be copied: the `len` words at `dst` receive copies of the `len`
/// source words at `src`.
struct PendingSlots {
    dst: *mut u64,
    src: *const u64,
    len: usize,
}

/// The work stack of a copy in progress.
pub(crate) struct SlotStack {
    pending: WorkStack<PendingSlots>,
}

impl Default for SlotStack {
    fn default() -> Self {
        SlotStack { pending: WorkStack::new(&SPARE_SLOTS) }
    }
}

impl SlotStack {
    /// Queue the `len` term slots at `src` for copying into `dst`, the
    /// matching words of an object just allocated in the target.
    #[inline]
    pub(crate) fn push(&mut self, dst: *mut u64, src: *const u64, len: usize) {
        if len != 0 {
            self.pending.push(PendingSlots { dst, src, len });
        }
    }
}

/// A term copier that queues the slots of each object it allocates.
pub(crate) trait SlotCopier {
    /// The copier's work stack.
    fn slots(&mut self) -> &mut SlotStack;

    /// Copy one term into the target and return the word to store for it.
    /// An object's copy is allocated here with its term slots queued on
    /// [`SlotCopier::slots`], not copied; a list may copy a run of cells
    /// along its tail, queuing the first cell whose head needs a copy.
    /// [`copy_graph`] may store an immediate slot without calling this.
    ///
    /// # Safety
    /// `word` and everything it reaches must stay live and unchanged until
    /// the copy completes.
    unsafe fn copy_word(&mut self, word: u64) -> u64;
}

/// Copy `root` and everything it reaches with `copier`.
///
/// Objects are copied depth first, in the order a recursive copy would use:
/// an object's slots in order, each slot's subtree before the next slot.
/// A list nested in head position, or a nested tuple, map or closure, costs
/// one queued entry per open level; a proper list needs at most one.
/// Re-entrant: a nested call only drains the entries it queued.
///
/// # Safety
/// The complete source graph must stay live and unchanged during the copy,
/// and target allocation must not move or free words already allocated.
/// Every entry the copier queues must name `len` readable source words and
/// `len` writable words reserved in their copy.
pub(crate) unsafe fn copy_graph<C: SlotCopier>(copier: &mut C, root: u64) -> u64 {
    let base = copier.slots().pending.len();
    // SAFETY: the caller keeps the source graph live and unchanged.
    let copied = unsafe { copier.copy_word(root) };
    while copier.slots().pending.len() > base {
        let mut slots = copier.slots().pending.pop().expect("pending slots above base");
        loop {
            let queued = copier.slots().pending.len();
            // SAFETY: `src` is a term word of a live source object, which no
            // copy modifies; `dst` is the matching word reserved in its copy,
            // written exactly once here.
            unsafe {
                let word = *slots.src;
                *slots.dst = if is_plain_immediate(word) { word } else { copier.copy_word(word) };
            }
            slots.len -= 1;
            if slots.len == 0 {
                break;
            }
            // SAFETY: slots remain, so both pointers stay inside the source
            // object and its reserved copy.
            unsafe {
                slots.dst = slots.dst.add(1);
                slots.src = slots.src.add(1);
            }
            if copier.slots().pending.len() != queued {
                // The slot's object queued its own slots: copy them first, as
                // recursion would, then this object's remaining slots.
                let stack = &mut copier.slots().pending;
                let child = stack.pop().expect("child slots just queued");
                stack.push(slots);
                stack.push(child);
                break;
            }
        }
    }
    copied
}
