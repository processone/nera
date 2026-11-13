//! Inter-process message passing via deep copy.
//!
//! Erlang processes are fully isolated: they share no heap memory.
//! When a message is sent from one process to another the term is
//! deep-copied into the receiver's heap. Immediate values (small
//! integers, atoms, pids, nil) require no allocation.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::copy_stack::{copy_graph, SlotCopier, SlotStack};
use crate::heap::{clone_arc_from_raw, is_plain_immediate, FragmentHeap, ProcessHeap};
use crate::term::{
    bigint_total_words, BoxedHeader, ConsCell, ErlTerm, BOXED_BIGINT, BOXED_BINARY, BOXED_CLOSURE,
    BOXED_FLOAT, BOXED_MAP, BOXED_MATCH_CTX, BOXED_REFC_BINARY, BOXED_REFERENCE, BOXED_TUPLE,
    EXTERNAL_BIT, REFC_BINARY_OFFSET_WORD_INDEX, REFC_BINARY_PTR_WORD_INDEX,
    REFC_BINARY_TOTAL_WORDS, REFERENCE_TOTAL_WORDS, TAG_ATOM, TAG_BOXED,
    TAG_CONS, TAG_MASK, TAG_NIL, TAG_PID, TAG_SMALL_INT,
};
use crate::ffi::binary_data;

// ---------------------------------------------------------------------------
// FragmentElem — compact element for SimpleTuple fragments
// ---------------------------------------------------------------------------

/// Maximum number of elements stored inline (no Vec allocation).
/// Arity 2 is the hot case (`{msg, Bin}`); arity 3-4 falls through to SimpleTuple.
const MAX_INLINE_ELEMS: usize = 2;

/// A term that cannot contain a borrowed heap pointer.
#[derive(Clone, Copy)]
pub struct ImmediateTerm(ErlTerm);
impl ImmediateTerm {
    #[inline]
    pub fn new(term: ErlTerm) -> Self {
        assert!(term.is_immediate(), "fragment immediate must not contain a heap pointer");
        Self(term)
    }
    #[inline]
    pub fn get(self) -> ErlTerm { self.0 }
}

/// An inline tuple arity proven to fit the fixed payload capacity.
#[derive(Clone, Copy)]
pub struct InlineArity(u32);
impl InlineArity {
    #[inline]
    pub fn new(arity: u32) -> Self {
        assert!(arity as usize <= MAX_INLINE_ELEMS, "inline message arity exceeds capacity");
        Self(arity)
    }
    #[inline]
    pub fn get(self) -> u32 { self.0 }
}

/// Compact representation of a tuple element that doesn't need a heap.
pub enum FragmentElem {
    /// Atom, small int, pid, nil — stored as-is.
    Immediate(ImmediateTerm),
    /// Reference-counted binary — just the Arc (no heap descriptor needed).
    RefcBinary(crate::binary::BinarySlice),
}

impl Default for FragmentElem {
    fn default() -> Self {
        FragmentElem::Immediate(crate::message::ImmediateTerm::new(ErlTerm::nil()))
    }
}

// ---------------------------------------------------------------------------
// Direct copy — bypass fragments when receiver Mutex is available
// ---------------------------------------------------------------------------

/// Copy a message term directly to the target heap, bypassing the fragment
/// intermediate. Optimized for tuples of immediates + refc binaries (the
/// common message pattern). Falls back to `deep_copy_term` for complex terms.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn direct_copy_message(msg: ErlTerm, target: &mut ProcessHeap) -> ErlTerm {
    let msg = if msg.is_external() { msg.strip_external_bit() } else { msg };
    match msg.tag() {
        TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL => msg,
        TAG_BOXED if msg.is_refc_binary() => {
            unsafe {
                let ptr = msg.as_boxed_ptr().unwrap();
                let header = BoxedHeader(*ptr);
                let bit_count = header.arity() as usize;
                let raw_arc_ptr = *ptr.add(REFC_BINARY_PTR_WORD_INDEX);
                let offset = *ptr.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
                let data = clone_arc_from_raw(raw_arc_ptr);
                target.alloc_binary_slice(crate::binary::BinarySlice::from_runtime_bits(data, offset, bit_count))
            }
        }
        TAG_BOXED if msg.is_tuple() => {
            unsafe {
                let ptr = msg.as_boxed_ptr().unwrap();
                let header = BoxedHeader(*ptr);
                let arity = header.arity() as usize;
                // Fast path: all elements are immediate or refc binary
                let mut all_simple = true;
                for i in 0..arity {
                    let elem = ErlTerm(*ptr.add(1 + i));
                    let elem = if elem.is_external() { elem.strip_external_bit() } else { elem };
                    match elem.tag() {
                        TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL => {}
                        TAG_BOXED if elem.is_refc_binary() => {}
                        _ => { all_simple = false; break; }
                    }
                }
                if all_simple {
                    let tuple_ptr = target.alloc_words(1 + arity);
                    *tuple_ptr = BoxedHeader::new(crate::term::BoxedSubtype::Tuple, arity as u64).0;
                    for i in 0..arity {
                        let elem = ErlTerm(*ptr.add(1 + i));
                        let elem = if elem.is_external() { elem.strip_external_bit() } else { elem };
                        let val = match elem.tag() {
                            TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL => elem,
                            _ => {
                                // Refc binary (checked above)
                                let bp = elem.as_boxed_ptr().unwrap();
                                let bh = BoxedHeader(*bp);
                                let bit_count = bh.arity() as usize;
                                let raw_arc = *bp.add(REFC_BINARY_PTR_WORD_INDEX);
                                let off = *bp.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
                                let data = clone_arc_from_raw(raw_arc);
                                target.alloc_binary_slice(crate::binary::BinarySlice::from_runtime_bits(data, off, bit_count))
                            }
                        };
                        *tuple_ptr.add(1 + i) = val.0;
                    }
                    return ErlTerm::boxed_unchecked(tuple_ptr as *const u64);
                }
            }
            // Complex tuple: fall through to deep copy
            deep_copy_term(msg, target)
        }
        _ => deep_copy_term(msg, target),
    }
}

// ---------------------------------------------------------------------------
// MessageFragment — self-contained message for lock-free delivery
// ---------------------------------------------------------------------------

/// Boxed heap fragment — keeps the `MessageFragment` enum small.
/// Invalid roots cannot be paired with a heap through public field construction.
/// ```compile_fail
/// use nera_runtime::{message::HeapFragment, heap::ProcessHeap, term::ErlTerm};
/// let _ = HeapFragment { heap: ProcessHeap::new(), root: ErlTerm::nil() };
/// ```
pub struct HeapFragment {
    heap: FragmentHeap,
    root: ErlTerm,
}

/// A self-contained message with its own mini-heap for lock-free inter-process
/// message passing. The sender deep-copies the message into a fragment (no
/// target process lock needed), then pushes it to the receiver's lock-free
/// inbox (`SegQueue`). The receiving worker drains fragments into the process
/// mailbox under its own Mutex.
pub enum MessageFragment {
    /// Immediate value (atom, small int, pid, nil) — no heap needed.
    Immediate(ImmediateTerm),
    /// Heap-backed term with its own mini-heap (boxed to shrink the enum).
    Heap(Box<HeapFragment>),
    /// Reference-counted binary — avoids fragment heap allocation entirely.
    /// Only the Arc is cloned (one atomic increment); the receiver allocates
    /// a 3-word descriptor directly on its own heap.
    RefcBinary(crate::binary::BinarySlice),
    /// Tuple whose elements are all immediates or refc binaries.
    /// Avoids fragment heap allocation entirely — elements stored inline.
    SimpleTuple(Vec<FragmentElem>),
    /// Small tuple (arity ≤ 4) of immediates/refc binaries stored in a
    /// fixed-size array — zero heap allocation per message.
    InlineTuple {
        arity: InlineArity,
        elems: [FragmentElem; MAX_INLINE_ELEMS],
    },
    /// Reactor readiness notification: socket is readable, worker should do the read.
    /// Intercepted by `drain_inbox` before `copy_to_heap` — never reaches the mailbox.
    ActiveReadReady {
        stream_handle: crate::socket::SocketHandle,
        /// Shared flag — reactor sets true before dispatch, worker clears after draining reads.
        in_flight: Arc<AtomicBool>,
    },
}

impl MessageFragment {
    /// Transfer an already owned graph without another deep copy.
    /// # Safety
    /// The complete root graph must be owned by heap (or be immediate).
    pub(crate) unsafe fn from_owned_heap(heap: FragmentHeap, root: ErlTerm) -> Self {
        Self::Heap(Box::new(HeapFragment { heap, root }))
    }

    /// Deep-copy a term from the sender's heap into a new fragment.
    /// # Safety
    /// All input terms and reachable objects must remain live and unmoved by GC
    /// for this call. Binary windows must be well-formed and backing bytes must
    /// not be concurrently mutated.
    pub unsafe fn from_term(term: ErlTerm) -> Self {
        // External terms must be deep-copied with the bit stripped,
        // since the fragment outlives the process's borrowed_heaps.
        if term.is_external() {
            return Self::from_term(term.strip_external_bit());
        }
        match term.tag() {
            TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL => {
                MessageFragment::Immediate(crate::message::ImmediateTerm::new(term))
            }
            TAG_BOXED if term.is_refc_binary() => {
                // Fast path: bare refc binary — avoid fragment heap entirely.
                // Just clone the Arc (one atomic increment).
                unsafe {
                    let ptr = term.as_boxed_ptr().unwrap();
                    let header = BoxedHeader(*ptr);
                    let bit_count = header.arity() as usize;
                    let raw_arc_ptr = *ptr.add(REFC_BINARY_PTR_WORD_INDEX);
                    let offset = *ptr.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
                    let data = clone_arc_from_raw(raw_arc_ptr);
                    MessageFragment::RefcBinary(crate::binary::BinarySlice::from_runtime_bits(data, offset, bit_count))
                }
            }
            TAG_BOXED if term.is_tuple() => {
                // Fast path: tuple of immediates / refc binaries.
                // Arity ≤ 4 → InlineTuple (zero allocation).
                // Arity > 4 → SimpleTuple (one Vec allocation).
                // Falls through to Heap path if any element is complex.
                unsafe {
                    let ptr = term.as_boxed_ptr().unwrap();
                    let header = BoxedHeader(*ptr);
                    let arity = header.arity() as usize;
                    if arity <= MAX_INLINE_ELEMS {
                        // Try inline path — fixed-size array, no allocation.
                        let mut elems: [FragmentElem; MAX_INLINE_ELEMS] = Default::default();
                        let mut all_simple = true;
                        for (i, item) in elems[..arity].iter_mut().enumerate() {
                            let elem = ErlTerm(*ptr.add(1 + i));
                            let elem = if elem.is_external() { elem.strip_external_bit() } else { elem };
                            match elem.tag() {
                                TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL => {
                                    *item = FragmentElem::Immediate(crate::message::ImmediateTerm::new(elem));
                                }
                                TAG_BOXED if elem.is_refc_binary() => {
                                    let bp = elem.as_boxed_ptr().unwrap();
                                    let bh = BoxedHeader(*bp);
                                    let bit_count = bh.arity() as usize;
                                    let raw_arc = *bp.add(REFC_BINARY_PTR_WORD_INDEX);
                                    let off = *bp.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
                                    let data = clone_arc_from_raw(raw_arc);
                                    *item = FragmentElem::RefcBinary(crate::binary::BinarySlice::from_runtime_bits(data, off, bit_count));
                                }
                                _ => {
                                    all_simple = false;
                                    break;
                                }
                            }
                        }
                        if all_simple {
                            return MessageFragment::InlineTuple { arity: crate::message::InlineArity::new(arity as u32), elems };
                        }
                    } else {
                        // Larger tuple — try Vec-backed SimpleTuple.
                        let mut elems = Vec::with_capacity(arity);
                        let mut all_simple = true;
                        for i in 0..arity {
                            let elem = ErlTerm(*ptr.add(1 + i));
                            let elem = if elem.is_external() { elem.strip_external_bit() } else { elem };
                            match elem.tag() {
                                TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL => {
                                    elems.push(FragmentElem::Immediate(crate::message::ImmediateTerm::new(elem)));
                                }
                                TAG_BOXED if elem.is_refc_binary() => {
                                    let bp = elem.as_boxed_ptr().unwrap();
                                    let bh = BoxedHeader(*bp);
                                    let bit_count = bh.arity() as usize;
                                    let raw_arc = *bp.add(REFC_BINARY_PTR_WORD_INDEX);
                                    let off = *bp.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
                                    let data = clone_arc_from_raw(raw_arc);
                                    elems.push(FragmentElem::RefcBinary(crate::binary::BinarySlice::from_runtime_bits(data, off, bit_count)));
                                }
                                _ => {
                                    all_simple = false;
                                    break;
                                }
                            }
                        }
                        if all_simple {
                            return MessageFragment::SimpleTuple(elems);
                        }
                        // Fall through — drop partial elems, use Heap path
                    }
                }
                let mut heap = FragmentHeap::new_with_capacity(64);
                let root = unsafe { heap.copy_raw_term(term) };
                MessageFragment::Heap(Box::new(HeapFragment { heap, root }))
            }
            _ => {
                let mut heap = FragmentHeap::new_with_capacity(64);
                let root = unsafe { heap.copy_raw_term(term) };
                MessageFragment::Heap(Box::new(HeapFragment { heap, root }))
            }
        }
    }

    /// Copy the root term from this fragment into the target process heap.
    /// Consumes the fragment.
    pub fn copy_to_heap(self, target: &mut ProcessHeap) -> ErlTerm {
        match self {
            MessageFragment::Immediate(term) => term.get(),
            MessageFragment::RefcBinary(range) => {
                // Move the validated window and its Arc without another check or clone.
                target.alloc_binary_slice(range)
            }
            MessageFragment::InlineTuple { arity, elems } => {
                let mut arr = elems;
                let arity = arity.get() as usize;
                copy_simple_tuple_batch(target, arity, (0..arity).map(|i| std::mem::take(&mut arr[i])))
            }
            MessageFragment::SimpleTuple(elems) => {
                let arity = elems.len();
                copy_simple_tuple_batch(target, arity, elems.into_iter())
            }
            MessageFragment::Heap(frag) => {
                // Adopt the fragment's young-gen chunks directly into the
                // receiver's heap. All pointers in `root` remain valid because
                // the backing Vec<u64> storage is moved (not copied).
                target.adopt_fragment(frag.heap);
                frag.root
            }
            MessageFragment::ActiveReadReady { .. } => {
                panic!("ActiveReadReady must be intercepted by drain_inbox, not copy_to_heap");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Batch-allocated tuple copy (shared by InlineTuple + SimpleTuple)
// ---------------------------------------------------------------------------

/// Single-allocation copy for a slice of `FragmentElem`.
/// Pre-counts refc binaries, allocates tuple header + all binary descriptors
/// in one `alloc_words` call, then fills in elements.
fn copy_simple_tuple_batch(
    target: &mut ProcessHeap,
    arity: usize,
    elems: impl Iterator<Item = FragmentElem>,
) -> ErlTerm {
    // We can't pre-scan a consuming iterator, so use an upper bound:
    // allocate as if ALL elements are refc binaries.  Unused trailing
    // words are harmless (bump allocator, no per-word cost).
    let total_words = (1 + arity) + arity * REFC_BINARY_TOTAL_WORDS;
    let base_ptr = target.alloc_words(total_words);

    unsafe {
        *base_ptr = BoxedHeader::new(crate::term::BoxedSubtype::Tuple, arity as u64).0;

        // Refc binary descriptors are placed right after the tuple.
        let mut refc_offset = 1 + arity;

        for (i, elem) in elems.enumerate() {
            let val = match elem {
                FragmentElem::Immediate(t) => t.get(),
                FragmentElem::RefcBinary(range) => {
                    let (data, offset, bits) = range.into_bit_parts();
                    // Write the 3-word descriptor into the pre-allocated region.
                    let desc_ptr = base_ptr.add(refc_offset);
                    let raw_arc_ptr = Arc::as_ptr(&data) as u64;
                    *desc_ptr = BoxedHeader::new(crate::term::BoxedSubtype::RefcBinary, bits as u64).0;
                    *desc_ptr.add(REFC_BINARY_PTR_WORD_INDEX) = raw_arc_ptr;
                    *desc_ptr.add(REFC_BINARY_OFFSET_WORD_INDEX) = offset as u64;
                    target.retain_refc_binary(data);
                    let term = ErlTerm::boxed_unchecked(desc_ptr as *const u64);
                    refc_offset += REFC_BINARY_TOTAL_WORDS;
                    term
                }
            };
            *base_ptr.add(1 + i) = val.0;
        }
    }
    // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
    unsafe { ErlTerm::boxed_unchecked(base_ptr as *const u64) }
}

// ---------------------------------------------------------------------------
// Deep copy
// ---------------------------------------------------------------------------

/// Deep-copy a term into `target_heap`, maintaining process isolation.
///
/// Shared sub-structure within the source term is preserved (each source
/// address is copied at most once), but the result is completely
/// independent of the source heap. Work still to copy waits on a
/// heap-allocated stack (see `copy_stack`), so nesting depth costs heap,
/// not native stack.
///
/// Raw terms cannot establish source validity through a safe call:
/// ```compile_fail,E0133
/// use nera_runtime::{message::deep_copy_term, heap::ProcessHeap, term::ErlTerm};
/// let mut target = ProcessHeap::new();
/// let _ = deep_copy_term(ErlTerm::nil(), &mut target);
/// ```
/// # Safety
/// The complete source graph must remain initialized, immutable and live for
/// this call, including any external owners. No moving GC may run on it.
/// Target allocation must not invalidate source storage. Prefer
/// `BorrowedTerm::copy_to_heap` when a source owner is available.
pub unsafe fn deep_copy_term(term: ErlTerm, target_heap: &mut ProcessHeap) -> ErlTerm {
    let mut copier = TermCopier::new(target_heap);
    // SAFETY: the caller keeps the source graph live and immutable; the
    // target heap never moves words it has already allocated.
    ErlTerm(unsafe { copy_graph(&mut copier, term.0) })
}

// ---------------------------------------------------------------------------
// TermCopier
// ---------------------------------------------------------------------------

/// Copies a term graph into a process heap through [`copy_graph`], so a term
/// nested hundreds of thousands of levels deep copies without native
/// recursion.
struct TermCopier<'a> {
    target: &'a mut ProcessHeap,
    /// Maps source address -> copied ErlTerm (with tag) so that shared
    /// sub-structure is only copied once.
    forwarding: HashMap<usize, ErlTerm>,
    /// Copied objects whose term slots still need copying.
    slots: SlotStack,
}

impl SlotCopier for TermCopier<'_> {
    fn slots(&mut self) -> &mut SlotStack {
        &mut self.slots
    }

    unsafe fn copy_word(&mut self, word: u64) -> u64 {
        // SAFETY: forwarded from `copy_graph`'s caller.
        unsafe { self.copy_object(ErlTerm(word)).0 }
    }
}

impl<'a> TermCopier<'a> {
    fn new(target: &'a mut ProcessHeap) -> Self {
        TermCopier {
            target,
            forwarding: HashMap::new(),
            slots: SlotStack::default(),
        }
    }

    /// Look up a previously-forwarded address.
    #[inline]
    fn forwarded(&self, addr: usize) -> Option<ErlTerm> {
        self.forwarding.get(&addr).copied()
    }

    /// Record a forwarding entry.
    #[inline]
    fn record_forwarding(&mut self, addr: usize, new_term: ErlTerm) {
        self.forwarding.insert(addr, new_term);
    }

    /// Copy one object (or a run of list cells, see `copy_list`) into the
    /// target heap, queueing its term slots instead of copying them.
    /// Returns the copied term.
    unsafe fn copy_object(&mut self, term: ErlTerm) -> ErlTerm {
        // External terms point into ETS storage — strip the external bit
        // and deep-copy from the ETS CompactHeap into the target heap.
        let term = if term.is_external() { term.strip_external_bit() } else { term };
        match term.tag() {
            // Immediates require no heap allocation.
            TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL => term,
            TAG_CONS => unsafe { self.copy_list(term) },
            TAG_BOXED => unsafe { self.copy_boxed(term) },
            _ => term,
        }
    }

    /// Copy the cons cells of a list along its tail while their heads are
    /// immediates, as most lists and strings need. The first cell whose head
    /// needs copying queues its head and tail and ends the run, so the head's
    /// subtree is copied before the rest of the list, as recursion would. A
    /// cell copied before (shared structure) ends the run with its copy.
    unsafe fn copy_list(&mut self, list: ErlTerm) -> ErlTerm {
        let mut first = None;
        let mut prev_tail: *mut u64 = std::ptr::null_mut();
        let mut cursor = list;
        loop {
            let mut next = None;
            let copied = if cursor.tag() != TAG_CONS {
                // The terminator: nil or an improper tail.
                unsafe { self.copy_object(cursor) }
            } else {
                let src_addr = (cursor.0 & !(TAG_MASK | EXTERNAL_BIT)) as usize;
                if let Some(forwarded) = self.forwarded(src_addr) {
                    forwarded
                } else {
                    let new_ptr = self.target.alloc_words(2);
                    // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                    let new_term = unsafe { ErlTerm::cons_unchecked(new_ptr as *const ConsCell) };
                    self.record_forwarding(src_addr, new_term);
                    // SAFETY: `src_addr` is a live source cons cell (head word
                    // first, then tail); `new_ptr` reserves its two-word copy.
                    unsafe {
                        let src = src_addr as *const u64;
                        let head = *src;
                        if is_plain_immediate(head) {
                            *new_ptr = head;
                            next = Some((new_ptr.add(1), ErlTerm(*src.add(1))));
                        } else {
                            self.slots.push(new_ptr, src, 2);
                        }
                    }
                    new_term
                }
            };
            if first.is_none() {
                first = Some(copied);
            } else {
                // SAFETY: the previous cell's reserved tail word, written once.
                unsafe { *prev_tail = copied.0 };
            }
            match next {
                Some((tail_slot, tail)) => (prev_tail, cursor) = (tail_slot, tail),
                None => return first.expect("first cell copied"),
            }
        }
    }

    /// Copy one boxed object, queueing the term slots of tuples, maps and
    /// closures.
    unsafe fn copy_boxed(&mut self, term: ErlTerm) -> ErlTerm {
        if term.0 == 0 {
            return term;
        }
        let src_addr = (term.0 & !TAG_MASK) as usize;
        if let Some(forwarded) = self.forwarded(src_addr) {
            return forwarded;
        }

        let src_header_ptr = src_addr as *const u64;
        let header = unsafe { BoxedHeader(*src_header_ptr) };
        let arity = header.arity() as usize;
        let subtype = header.subtype();

        match subtype {
            BOXED_TUPLE => {
                // Allocate header + arity words.
                let new_ptr = self.target.alloc_words(1 + arity);
                // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                let new_term = unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) };
                self.record_forwarding(src_addr, new_term);
                // SAFETY: elements follow the header in both objects.
                unsafe {
                    *new_ptr = header.0;
                    self.slots.push(new_ptr.add(1), src_header_ptr.add(1), arity);
                }
                new_term
            }
            BOXED_FLOAT => {
                let new_ptr = self.target.alloc_words(2);
                // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                let new_term = unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) };
                self.record_forwarding(src_addr, new_term);
                unsafe {
                    *new_ptr = header.0;
                    *new_ptr.add(1) = *src_header_ptr.add(1);
                }
                new_term
            }
            BOXED_BIGINT => {
                // Reserved layout:
                // [header(BIGINT, limb_count), sign, limbs...]
                let total = bigint_total_words(arity);
                let new_ptr = self.target.alloc_words(total);
                // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                let new_term = unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) };
                self.record_forwarding(src_addr, new_term);
                unsafe {
                    std::ptr::copy_nonoverlapping(src_header_ptr, new_ptr, total);
                }
                new_term
            }
            BOXED_REFERENCE => {
                // [header(REFERENCE, 0), unique_id]
                let new_ptr = self.target.alloc_words(REFERENCE_TOTAL_WORDS);
                // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                let new_term = unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) };
                self.record_forwarding(src_addr, new_term);
                unsafe {
                    std::ptr::copy_nonoverlapping(src_header_ptr, new_ptr, REFERENCE_TOTAL_WORDS);
                }
                new_term
            }
            BOXED_MAP => {
                // arity = number of key-value pairs
                // total elements = arity * 2 (interleaved k/v)
                let total_elems = arity * 2;
                let total = 1 + total_elems;
                let new_ptr = self.target.alloc_words(total);
                // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                let new_term = unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) };
                self.record_forwarding(src_addr, new_term);
                // SAFETY: keys and values follow the header in both objects.
                unsafe {
                    *new_ptr = header.0;
                    self.slots.push(new_ptr.add(1), src_header_ptr.add(1), total_elems);
                }
                new_term
            }
            BOXED_BINARY => {
                // arity = bit count; derive byte count
                let bit_count = arity;
                let byte_count = bit_count.div_ceil(8);
                let data_words = byte_count.div_ceil(8);
                let total = 1 + data_words;
                let new_ptr = self.target.alloc_words(total);
                // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                let new_term = unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) };
                self.record_forwarding(src_addr, new_term);
                unsafe {
                    std::ptr::copy_nonoverlapping(src_header_ptr, new_ptr, total);
                }
                new_term
            }
            BOXED_REFC_BINARY => {
                // Descriptor layout:
                // [header(REFC_BINARY, byte_len), raw_arc_ptr, byte_offset]
                let total = REFC_BINARY_TOTAL_WORDS;
                let new_ptr = self.target.alloc_words(total);
                // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                let new_term = unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) };
                self.record_forwarding(src_addr, new_term);
                unsafe {
                    std::ptr::copy_nonoverlapping(src_header_ptr, new_ptr, total);
                }

                // Clone the backing Arc (atomic refcount increment)
                // and register in the target heap's ownership list.
                let raw_arc_ptr = unsafe {
                    *src_header_ptr.add(REFC_BINARY_PTR_WORD_INDEX)
                };
                let cloned_arc = unsafe { clone_arc_from_raw(raw_arc_ptr) };
                self.target.retain_refc_binary(cloned_arc);
                new_term
            }
            BOXED_CLOSURE => {
                // Closure layout: [header(CLOSURE, env_count) | fn_ptr | arity | env0 | env1 | ...]
                // header.arity() = env_count; fn_ptr and arity are raw words (not tagged terms).
                // Env vars ARE tagged ErlTerms and are queued for copying.
                let env_count = arity;
                let total_words = 3 + env_count;
                let new_ptr = self.target.alloc_words(total_words);
                // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                let new_term = unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) };
                self.record_forwarding(src_addr, new_term);

                // Copy header, fn_ptr, and closure arity as raw words; the
                // environment follows them in both objects.
                unsafe {
                    *new_ptr = header.0;
                    *new_ptr.add(1) = *src_header_ptr.add(1); // fn_ptr
                    *new_ptr.add(2) = *src_header_ptr.add(2); // callable arity
                    self.slots.push(new_ptr.add(3), src_header_ptr.add(3), env_count);
                }
                new_term
            }
            BOXED_MATCH_CTX => {
                // Materialize exactly the logical window before dropping
                // source ownership. Large output bytes become the owned
                // refc backing directly; small output lives in its heap.
                unsafe {
                    let window = crate::match_context::MatchContext::view(term).unwrap();
                    let count = window.remaining() as usize;
                    let length = count.div_ceil(8);
                    let (data, _) = binary_data(window.binary()).unwrap();
                    let (new_term, subtype) = if length > crate::heap::REFC_BINARY_THRESHOLD {
                        let mut bytes = vec![0; length];
                        crate::binary::bit_copy::copy_bits(bytes.as_mut_ptr(),0,data,window.offset() as usize,count);
                        let backing = Arc::new(crate::binary::BinaryData::new(bytes));
                        (self.target.alloc_refc_binary_from_arc(backing,0,length), crate::term::BoxedSubtype::RefcBinary)
                    } else {
                        let result = self.target.alloc_binary_zeroed(length);
                        let target = result.as_boxed_ptr().unwrap().add(1).cast_mut().cast::<u8>();
                        crate::binary::bit_copy::copy_bits(target,0,data,window.offset() as usize,count);
                        (result, crate::term::BoxedSubtype::HeapBinary)
                    };
                    if !count.is_multiple_of(8) {
                        *new_term.as_boxed_ptr().unwrap().cast_mut() = BoxedHeader::new(subtype,count as u64).0;
                    }
                    self.record_forwarding(src_addr, new_term);
                    new_term
                }
            }
            _ => {
                // Generic: copy header + arity data words verbatim.
                let total = 1 + arity;
                let new_ptr = self.target.alloc_words(total);
                // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                let new_term = unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) };
                self.record_forwarding(src_addr, new_term);
                unsafe {
                    std::ptr::copy_nonoverlapping(src_header_ptr, new_ptr, total);
                }
                new_term
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::MapPairs;
    use crate::binary::BinaryData;
    use crate::term::{
        bigint_total_words, BoxedHeader, BIGINT_FIRST_LIMB_WORD_INDEX, BIGINT_SIGN_NEGATIVE,
        BIGINT_SIGN_WORD_INDEX, BOXED_BIGINT, BOXED_MAP,
    };

    #[test]
    fn partial_refc_windows_keep_bit_length_through_all_message_paths() {
        for arity in [0,2,4] {
            let mut source = ProcessHeap::new();
            let binary = source.alloc_binary_auto(&[0xa5;65]);
            unsafe { *binary.as_boxed_ptr().unwrap().cast_mut() =
                BoxedHeader::new(crate::term::BoxedSubtype::RefcBinary,519).0; }
            let root = if arity == 0 { binary } else { source.alloc_tuple(&vec![binary;arity]) };
            let fragment = unsafe { MessageFragment::from_term(root) };
            let mut direct_heap = ProcessHeap::new();
            let direct = unsafe { direct_copy_message(root,&mut direct_heap) };
            drop(source);
            let mut fragment_heap = ProcessHeap::new();
            let copied = fragment.copy_to_heap(&mut fragment_heap);
            for term in [direct,copied] {
                let value = if arity == 0 { term } else { unsafe { ErlTerm(*term.as_boxed_ptr().unwrap().add(1)) } };
                assert_eq!(unsafe { value.binary_bit_size() },Some(519));
            }
        }
    }

    #[test]
    fn copied_match_windows_keep_exact_bits_after_source_drop_and_gc() {
        for (length, widths) in [(16, vec![0_usize,1,7,8,13,16,63,119]),
                                  (256, vec![0,1,7,8,13,16,511,519,1023])] {
            let bytes: Vec<u8> = (0..length).map(|i| (i * 71 + 0xa5) as u8).collect();
            for offset in 0..8 {
                for &bits in &widths {
                    let mut expected = vec![0; bits.div_ceil(8)];
                    for bit in 0..bits {
                        let value = (bytes[(offset+bit)/8] >> (7-(offset+bit)%8)) & 1;
                        expected[bit/8] |= value << (7-bit%8);
                    }
                    let mut source = ProcessHeap::new();
                    let binary = source.alloc_binary_auto(&bytes);
                    let descriptor = unsafe { crate::match_context::MatchContext::new(binary,
                        offset as u64, (offset+bits) as u64) }.unwrap();
                    let context = source.alloc_match_ctx(descriptor);
                    let mut target = ProcessHeap::new();
                    let copied = unsafe { context.borrow(&source) }.copy_to_heap(&mut target);
                    drop(source);
                    let mut roots = [copied];
                    unsafe { target.collect_full(&mut roots); }
                    unsafe {
                        assert_eq!(roots[0].binary_bit_size(), Some(bits));
                        assert_eq!(roots[0].binary_bytes_any().unwrap(), expected);
                    }
                }
            }
        }
    }

    #[test]
    fn borrowed_fragment_survives_source_drop() {
        let mut source = ProcessHeap::new();
        let bytes = source.alloc_binary_auto(&[7; 128]);
        let term = source.alloc_tuple(&[ErlTerm::small_int(42), bytes]);
        // SAFETY: source owns the graph throughout fragment creation.
        let fragment = unsafe { term.borrow(&source) }.to_message_fragment();
        drop(source);
        let mut target = ProcessHeap::new();
        let copied = fragment.copy_to_heap(&mut target);
        // SAFETY: the destination owns all transferred storage.
        unsafe {
            let words = copied.as_boxed_ptr().unwrap();
            assert_eq!(ErlTerm(*words.add(1)).as_small_int(), Some(42));
            let binary = ErlTerm(*words.add(2));
            assert_eq!(binary.borrow(&target).binary_bytes(), Some(&[7; 128][..]));
        }
    }

    #[test]
    fn invalid_fragment_components_are_rejected() {
        let invalid = ErlTerm::boxed(std::ptr::null());
        assert!(std::panic::catch_unwind(|| ImmediateTerm::new(invalid)).is_err());
        assert!(std::panic::catch_unwind(|| InlineArity::new(3)).is_err());
    }

    #[test]
    fn copy_immediate_is_identity() {
        let mut target = ProcessHeap::new();
        let int_term = ErlTerm::small_int(42);
        let copied = unsafe { deep_copy_term(int_term, &mut target) };
        assert_eq!(copied, int_term);

        let atom_term = ErlTerm::atom(3);
        let copied = unsafe { deep_copy_term(atom_term, &mut target) };
        assert_eq!(copied, atom_term);

        let nil_term = ErlTerm::nil();
        let copied = unsafe { deep_copy_term(nil_term, &mut target) };
        assert_eq!(copied, nil_term);
    }

    #[test]
    fn copy_list_is_deep() {
        let mut src_heap = ProcessHeap::new();
        let list = src_heap.alloc_list_from_slice(&[
            ErlTerm::small_int(1),
            ErlTerm::small_int(2),
            ErlTerm::small_int(3),
        ]);

        let mut target_heap = ProcessHeap::new();
        let copied = unsafe { list.borrow(&src_heap) }.copy_to_heap(&mut target_heap);

        // Verify the copied list has the same values.
        let mut values = vec![];
        let mut current = copied;
        while current.is_cons() {
            unsafe {
                let cell = &*current.as_cons_ptr().unwrap();
                values.push(cell.head.as_small_int().unwrap());
                current = cell.tail;
            }
        }
        assert!(current.is_nil());
        assert_eq!(values, vec![1, 2, 3]);

        // Verify independence: the pointers should be different.
        assert_ne!(
            list.as_cons_ptr().unwrap() as usize,
            copied.as_cons_ptr().unwrap() as usize,
        );
    }

    #[test]
    fn copy_tuple_containing_list() {
        let mut src_heap = ProcessHeap::new();
        let inner =
            src_heap.alloc_list_from_slice(&[ErlTerm::small_int(10), ErlTerm::small_int(20)]);
        let tuple = src_heap.alloc_tuple(&[ErlTerm::atom(0), inner]);

        let mut target_heap = ProcessHeap::new();
        let copied = unsafe { tuple.borrow(&src_heap) }.copy_to_heap(&mut target_heap);

        // Verify tuple structure.
        unsafe {
            let hdr_ptr = copied.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*hdr_ptr);
            assert_eq!(header.arity(), 2);
            assert_eq!(ErlTerm(*hdr_ptr.add(1)).as_atom_id(), Some(0));

            // Verify inner list.
            let inner_copy = ErlTerm(*hdr_ptr.add(2));
            assert!(inner_copy.is_cons());
            let cell = &*inner_copy.as_cons_ptr().unwrap();
            assert_eq!(cell.head.as_small_int(), Some(10));
        }
    }

    #[test]
    fn copy_shared_structure_uses_forwarding() {
        let mut src_heap = ProcessHeap::new();
        // Build a tuple that references the same list twice:
        // {L, L} where L = [1, 2]
        let shared_list =
            src_heap.alloc_list_from_slice(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        let tuple = src_heap.alloc_tuple(&[shared_list, shared_list]);

        let mut target_heap = ProcessHeap::new();
        let copied = unsafe { tuple.borrow(&src_heap) }.copy_to_heap(&mut target_heap);

        // Both tuple elements should point to the *same* copied list.
        unsafe {
            let hdr_ptr = copied.as_boxed_ptr().unwrap();
            let elem0 = ErlTerm(*hdr_ptr.add(1));
            let elem1 = ErlTerm(*hdr_ptr.add(2));
            assert!(elem0.is_cons());
            assert!(elem1.is_cons());
            assert_eq!(
                elem0.as_cons_ptr().unwrap() as usize,
                elem1.as_cons_ptr().unwrap() as usize,
                "shared structure should be forwarded, not duplicated"
            );
        }
    }

    #[test]
    fn copy_float() {
        let mut src_heap = ProcessHeap::new();
        let f = src_heap.alloc_float(1.234);

        let mut target_heap = ProcessHeap::new();
        let copied = unsafe { f.borrow(&src_heap) }.copy_to_heap(&mut target_heap);

        assert!(copied.is_boxed());
        // Different address in target heap.
        assert_ne!(
            f.as_boxed_ptr().unwrap() as usize,
            copied.as_boxed_ptr().unwrap() as usize,
        );
        unsafe {
            let bits = *copied.as_boxed_ptr().unwrap().add(1);
            assert_eq!(f64::from_bits(bits), 1.234);
        }
    }

    #[test]
    fn copy_binary() {
        let mut src_heap = ProcessHeap::new();
        let bin = src_heap.alloc_binary(&[0xDE, 0xAD, 0xBE, 0xEF]);

        let mut target_heap = ProcessHeap::new();
        let copied = unsafe { bin.borrow(&src_heap) }.copy_to_heap(&mut target_heap);

        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { copied.is_binary() });
        // Different address
        assert_ne!(
            bin.as_boxed_ptr().unwrap() as usize,
            copied.as_boxed_ptr().unwrap() as usize,
        );
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert_eq!(unsafe { copied.binary_size() }, Some(4));
        unsafe {
            assert_eq!(copied.binary_bytes().unwrap(), &[0xDE, 0xAD, 0xBE, 0xEF]);
        }
    }

    #[test]
    fn copy_reserved_bigint_layout() {
        let mut src_heap = ProcessHeap::new();
        let limb_count = 2usize;
        let total_words = bigint_total_words(limb_count);
        let ptr = src_heap.alloc_words(total_words);
        unsafe {
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::BigInt, limb_count as u64).0;
            *ptr.add(BIGINT_SIGN_WORD_INDEX) = BIGINT_SIGN_NEGATIVE;
            *ptr.add(BIGINT_FIRST_LIMB_WORD_INDEX) = 0x11;
            *ptr.add(BIGINT_FIRST_LIMB_WORD_INDEX + 1) = 0x22;
        }
        let bigint = ErlTerm::boxed(ptr as *const u64);

        let mut target_heap = ProcessHeap::new();
        let copied = unsafe { bigint.borrow(&src_heap) }.copy_to_heap(&mut target_heap);

        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { copied.is_bigint() });
        assert_ne!(
            bigint.as_boxed_ptr().unwrap() as usize,
            copied.as_boxed_ptr().unwrap() as usize
        );
        unsafe {
            let copied_ptr = copied.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*copied_ptr);
            assert_eq!(header.subtype(), BOXED_BIGINT);
            assert_eq!(header.arity(), limb_count as u64);
            assert_eq!(
                *copied_ptr.add(BIGINT_SIGN_WORD_INDEX),
                BIGINT_SIGN_NEGATIVE
            );
            assert_eq!(*copied_ptr.add(BIGINT_FIRST_LIMB_WORD_INDEX), 0x11);
            assert_eq!(*copied_ptr.add(BIGINT_FIRST_LIMB_WORD_INDEX + 1), 0x22);
        }
    }

    #[test]
    fn copy_refc_binary_clones_arc() {
        use std::sync::Arc;

        let mut src_heap = ProcessHeap::new();
        let data: Vec<u8> = (0..100).collect();
        let arc = Arc::new(BinaryData::new(data));
        let refc = src_heap.alloc_refc_binary_from_arc(arc.clone(), 10, 80);
        assert_eq!(Arc::strong_count(&arc), 2); // arc + src_heap

        let mut target_heap = ProcessHeap::new();
        let copied = unsafe { refc.borrow(&src_heap) }.copy_to_heap(&mut target_heap);

        // Arc strong count should have increased by 1
        assert_eq!(Arc::strong_count(&arc), 3); // arc + src_heap + target_heap
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(copied.is_refc_binary());
            assert_eq!(copied.binary_size(), Some(80));
        }
        assert_ne!(
            refc.as_boxed_ptr().unwrap() as usize,
            copied.as_boxed_ptr().unwrap() as usize,
        );
        // Both descriptors should point to the same backing data
        unsafe {
            let src_bytes = refc.refc_binary_bytes().unwrap();
            let dst_bytes = copied.refc_binary_bytes().unwrap();
            assert_eq!(src_bytes, dst_bytes);
            assert_eq!(src_bytes[0], 10);
            assert_eq!(src_bytes[79], 89);
        }
        // Target heap should own one Arc
        assert_eq!(target_heap.refc_binary_count(), 1);
    }

    #[test]
    fn copy_map() {
        let mut src_heap = ProcessHeap::new();
        let inner_list =
            src_heap.alloc_list_from_slice(&[ErlTerm::small_int(10), ErlTerm::small_int(20)]);
        // #{42 => [10, 20], 99 => nil}
        let map = src_heap.alloc_map(MapPairs::from_flat(&[
            ErlTerm::small_int(42),
            inner_list,
            ErlTerm::small_int(99),
            ErlTerm::nil(),
        ]).expect("complete key/value pairs"));

        let mut target_heap = ProcessHeap::new();
        let copied = unsafe { map.borrow(&src_heap) }.copy_to_heap(&mut target_heap);

        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { copied.is_map() });
        // Different address
        assert_ne!(
            map.as_boxed_ptr().unwrap() as usize,
            copied.as_boxed_ptr().unwrap() as usize,
        );
        unsafe {
            let hdr_ptr = copied.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*hdr_ptr);
            assert_eq!(header.subtype(), BOXED_MAP);
            assert_eq!(header.arity(), 2); // 2 pairs

            // First pair: key=42, value=[10, 20]
            assert_eq!(ErlTerm(*hdr_ptr.add(1)).as_small_int(), Some(42));
            let val = ErlTerm(*hdr_ptr.add(2));
            assert!(val.is_cons());
            let cell = &*val.as_cons_ptr().unwrap();
            assert_eq!(cell.head.as_small_int(), Some(10));

            // Second pair: key=99, value=nil
            assert_eq!(ErlTerm(*hdr_ptr.add(3)).as_small_int(), Some(99));
            assert!(ErlTerm(*hdr_ptr.add(4)).is_nil());
        }
    }

    #[test]
    fn copy_reference() {
        let mut src_heap = ProcessHeap::new();
        // Allocate a reference: [header(REFERENCE, 0), unique_id=99]
        let ptr = src_heap.alloc_words(REFERENCE_TOTAL_WORDS);
        unsafe {
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Reference, 0).0;
            *ptr.add(1) = 99_u64;
        }
        let ref_term = ErlTerm::boxed(ptr as *const u64);

        let mut target_heap = ProcessHeap::new();
        let copied = unsafe { ref_term.borrow(&src_heap) }.copy_to_heap(&mut target_heap);

        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { copied.is_reference() });
        // Different address
        assert_ne!(
            ref_term.as_boxed_ptr().unwrap() as usize,
            copied.as_boxed_ptr().unwrap() as usize,
        );
        unsafe {
            let hdr_ptr = copied.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*hdr_ptr);
            assert_eq!(header.subtype(), BOXED_REFERENCE);
            assert_eq!(*hdr_ptr.add(1), 99_u64); // unique_id preserved
        }
    }

    // -----------------------------------------------------------------------
    // MessageFragment tests
    // -----------------------------------------------------------------------

    #[test]
    fn fragment_immediate() {
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let frag = unsafe { MessageFragment::from_term(ErlTerm::small_int(42)) };
        assert!(matches!(frag, MessageFragment::Immediate(_)));
        let mut target = ProcessHeap::new();
        let result = frag.copy_to_heap(&mut target);
        assert_eq!(result.as_small_int(), Some(42));
    }

    #[test]
    fn fragment_refc_binary() {
        let mut src = ProcessHeap::new();
        let data: Vec<u8> = (0..200).collect();
        let arc = Arc::new(BinaryData::new(data));
        let bin = src.alloc_refc_binary_from_arc(arc.clone(), 0, 200);
        assert_eq!(Arc::strong_count(&arc), 2);

        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let frag = unsafe { MessageFragment::from_term(bin) };
        assert!(matches!(frag, MessageFragment::RefcBinary(_)));
        // from_term clones the Arc.
        assert_eq!(Arc::strong_count(&arc), 3);

        let mut target = ProcessHeap::new();
        let result = frag.copy_to_heap(&mut target);
        // copy_to_heap moves the Arc (no additional clone).
        assert_eq!(Arc::strong_count(&arc), 3);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(result.is_refc_binary());
            assert_eq!(result.binary_size(), Some(200));
        }
    }

    #[test]
    fn fragment_inline_tuple_immediates() {
        // {ok, 42} — arity 2, all immediates → InlineTuple
        let mut src = ProcessHeap::new();
        let tuple = src.alloc_tuple(&[ErlTerm::atom(1), ErlTerm::small_int(42)]);

        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let frag = unsafe { MessageFragment::from_term(tuple) };
        assert!(matches!(frag, MessageFragment::InlineTuple { arity, .. } if arity.get() == 2));

        let mut target = ProcessHeap::new();
        let result = frag.copy_to_heap(&mut target);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { result.is_tuple() });
        unsafe {
            let ptr = result.as_boxed_ptr().unwrap();
            assert_eq!(BoxedHeader(*ptr).arity(), 2);
            assert_eq!(ErlTerm(*ptr.add(1)).as_atom_id(), Some(1));
            assert_eq!(ErlTerm(*ptr.add(2)).as_small_int(), Some(42));
        }
    }

    #[test]
    fn fragment_inline_tuple_with_refc_binary() {
        // {msg, <<large binary>>} — arity 2, one refc binary → InlineTuple
        let mut src = ProcessHeap::new();
        let data: Vec<u8> = vec![0xAB; 1024];
        let arc = Arc::new(BinaryData::new(data));
        let bin = src.alloc_refc_binary_from_arc(arc.clone(), 0, 1024);
        let tuple = src.alloc_tuple(&[ErlTerm::atom(5), bin]);

        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let frag = unsafe { MessageFragment::from_term(tuple) };
        assert!(matches!(frag, MessageFragment::InlineTuple { arity, .. } if arity.get() == 2));
        // Arc cloned once during from_term.
        assert_eq!(Arc::strong_count(&arc), 3);

        let mut target = ProcessHeap::new();
        let result = frag.copy_to_heap(&mut target);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { result.is_tuple() });
        unsafe {
            let ptr = result.as_boxed_ptr().unwrap();
            assert_eq!(BoxedHeader(*ptr).arity(), 2);
            assert_eq!(ErlTerm(*ptr.add(1)).as_atom_id(), Some(5));
            let bin_elem = ErlTerm(*ptr.add(2));
            assert!(bin_elem.is_refc_binary());
            assert_eq!(bin_elem.binary_size(), Some(1024));
        }
        // Arc moved from fragment to target (no extra clone in copy_to_heap).
        assert_eq!(Arc::strong_count(&arc), 3);
        assert_eq!(target.refc_binary_count(), 1);
    }

    #[test]
    fn fragment_simple_tuple_large_arity() {
        // Arity 3 (> MAX_INLINE_ELEMS=2) with a refc binary → SimpleTuple
        let mut src = ProcessHeap::new();
        let data = Arc::new(BinaryData::new(vec![0xCDu8; 256]));
        let bin = src.alloc_refc_binary_from_arc(data.clone(), 0, 256);
        let tuple = src.alloc_tuple(&[
            ErlTerm::atom(10),
            ErlTerm::small_int(99),
            bin,
        ]);

        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let frag = unsafe { MessageFragment::from_term(tuple) };
        assert!(matches!(frag, MessageFragment::SimpleTuple(_)));

        let mut target = ProcessHeap::new();
        let result = frag.copy_to_heap(&mut target);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { result.is_tuple() });
        unsafe {
            let ptr = result.as_boxed_ptr().unwrap();
            assert_eq!(BoxedHeader(*ptr).arity(), 3);
            assert_eq!(ErlTerm(*ptr.add(1)).as_atom_id(), Some(10));
            assert_eq!(ErlTerm(*ptr.add(2)).as_small_int(), Some(99));
            let bin_elem = ErlTerm(*ptr.add(3));
            assert!(bin_elem.is_refc_binary());
            assert_eq!(bin_elem.binary_size(), Some(256));
        }
        assert_eq!(target.refc_binary_count(), 1);
    }

    #[test]
    fn fragment_heap_for_complex_tuple() {
        // Tuple containing a list → falls through to Heap variant
        let mut src = ProcessHeap::new();
        let list = src.alloc_list_from_slice(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        let tuple = src.alloc_tuple(&[ErlTerm::atom(0), list]);

        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let frag = unsafe { MessageFragment::from_term(tuple) };
        assert!(matches!(frag, MessageFragment::Heap(_)));

        let mut target = ProcessHeap::new();
        let result = frag.copy_to_heap(&mut target);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { result.is_tuple() });
        unsafe {
            let ptr = result.as_boxed_ptr().unwrap();
            assert_eq!(BoxedHeader(*ptr).arity(), 2);
            assert_eq!(ErlTerm(*ptr.add(1)).as_atom_id(), Some(0));
            let inner = ErlTerm(*ptr.add(2));
            assert!(inner.is_cons());
        }
    }

    #[test]
    fn fragment_inline_tuple_duplicate_refc_binary() {
        // {Bin, Bin} — same refc binary twice in a tuple
        let mut src = ProcessHeap::new();
        let data = Arc::new(BinaryData::new(vec![0xEFu8; 512]));
        let bin = src.alloc_refc_binary_from_arc(data.clone(), 0, 512);
        let tuple = src.alloc_tuple(&[bin, bin]); // same ErlTerm twice

        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let frag = unsafe { MessageFragment::from_term(tuple) };
        assert!(matches!(frag, MessageFragment::InlineTuple { arity, .. } if arity.get() == 2));
        // Two Arc clones (one per element).
        assert_eq!(Arc::strong_count(&data), 4);

        let mut target = ProcessHeap::new();
        let result = frag.copy_to_heap(&mut target);
        unsafe {
            let ptr = result.as_boxed_ptr().unwrap();
            let bin1 = ErlTerm(*ptr.add(1));
            let bin2 = ErlTerm(*ptr.add(2));
            assert!(bin1.is_refc_binary());
            assert!(bin2.is_refc_binary());
            assert_eq!(bin1.binary_size(), Some(512));
            assert_eq!(bin2.binary_size(), Some(512));
        }
        // Two descriptors, same backing data.
        assert_eq!(target.refc_binary_count(), 2);
    }

    #[test]
    fn fragment_batch_alloc_multiple_refc_binaries() {
        // SimpleTuple with 3 different refc binaries — tests batch allocation
        let mut src = ProcessHeap::new();
        let arc1 = Arc::new(BinaryData::new(vec![1u8; 100]));
        let arc2 = Arc::new(BinaryData::new(vec![2u8; 200]));
        let arc3 = Arc::new(BinaryData::new(vec![3u8; 300]));
        let bin1 = src.alloc_refc_binary_from_arc(arc1.clone(), 0, 100);
        let bin2 = src.alloc_refc_binary_from_arc(arc2.clone(), 0, 200);
        let bin3 = src.alloc_refc_binary_from_arc(arc3.clone(), 0, 300);
        let tuple = src.alloc_tuple(&[bin1, bin2, bin3]);

        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let frag = unsafe { MessageFragment::from_term(tuple) };
        assert!(matches!(frag, MessageFragment::SimpleTuple(_)));

        let mut target = ProcessHeap::new();
        let result = frag.copy_to_heap(&mut target);
        unsafe {
            let ptr = result.as_boxed_ptr().unwrap();
            assert_eq!(BoxedHeader(*ptr).arity(), 3);
            for i in 0..3 {
                let elem = ErlTerm(*ptr.add(1 + i));
                assert!(elem.is_refc_binary(), "element {} should be refc binary", i);
            }
            assert_eq!(ErlTerm(*ptr.add(1)).binary_size(), Some(100));
            assert_eq!(ErlTerm(*ptr.add(2)).binary_size(), Some(200));
            assert_eq!(ErlTerm(*ptr.add(3)).binary_size(), Some(300));
        }
        assert_eq!(target.refc_binary_count(), 3);
    }

    #[test]
    fn fragment_roundtrip_preserves_binary_data() {
        // End-to-end: create refc binary with specific data, fragment, copy,
        // verify byte content is identical.
        let mut src = ProcessHeap::new();
        let data: Vec<u8> = (0..=255).cycle().take(10240).collect();
        let arc = Arc::new(BinaryData::new(data.clone()));
        let bin = src.alloc_refc_binary_from_arc(arc, 10, 500);
        let tuple = src.alloc_tuple(&[ErlTerm::atom(7), bin]);

        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let frag = unsafe { MessageFragment::from_term(tuple) };
        let mut target = ProcessHeap::new();
        let result = frag.copy_to_heap(&mut target);

        unsafe {
            let ptr = result.as_boxed_ptr().unwrap();
            let bin_copy = ErlTerm(*ptr.add(2));
            assert_eq!(bin_copy.binary_size(), Some(500));
            let bytes = bin_copy.refc_binary_bytes().unwrap();
            assert_eq!(bytes, &data[10..510]);
        }
    }
}
