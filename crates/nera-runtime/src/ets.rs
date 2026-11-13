//! ETS (Erlang Term Storage) — concurrent in-memory term tables.
//!
//! Provides a global registry of named tables, each storing tuples keyed
//! by a configurable element position. Tables support concurrent read/write
//! from multiple processes (public access).
//!
//! Terms stored in ETS are deep-copied (same as message passing) to ensure
//! process heap isolation.
//!
//! **Storage design**: Each stored tuple uses a `CompactHeap` backed by
//! an mmap'd region pool (`EtsRegionPool`).  Word buffers are bump-allocated
//! from large mmap'd regions instead of per-entry `Vec<u64>`. Regions return
//! memory directly to the OS when the pool and all
//! stored or borrowed entries release them. Zero-copy lookup results retain
//! their mapping even after the originating table is cleared or deleted.

use crate::process::{ProcessId, AtomicProcessId};
use crate::binary::BinaryData;
use std::cell::Cell;
use std::collections::{btree_map::Entry, BTreeMap, HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use crate::atom_table::ATOM_UNDEFINED;
use crate::ets_pool::{EtsAllocation, EtsRegionPool};
use crate::ffi::compare::compare_terms;
use crate::copy_stack::{copy_graph, SlotCopier, SlotStack, WorkStack};
use crate::heap::{is_plain_immediate, ProcessHeap};
use crate::term::{BoxedHeader, ErlTerm, BOXED_TUPLE};

/// A unique table identifier (small integer handle).
pub type TableId = u64;

/// Table type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableType {
    Set,
    OrderedSet,
    Bag,
    DuplicateBag,
}

/// Storage backend for ETS tables. Each variant owns its data structure and
/// is accessed behind the table-level RwLock semantics.
enum EtsStorage {
    Set(RwLock<HashMap<TermKey, StoredTuple>>),
    OrderedSet(RwLock<BTreeMap<OrdTermKey, StoredTuple>>),
    Bag(RwLock<HashMap<TermKey, Vec<StoredTuple>>>),
    DuplicateBag(RwLock<HashMap<TermKey, Vec<StoredTuple>>>),
}

/// A single ETS table.
///
/// Uses split lock domains for concurrency:
/// - `storage`: RwLock protecting the key→value store (brief write locks for insert/delete)
/// - `pool`: Mutex protecting the mmap bump allocator (brief lock for allocation/reclaim)
///
/// Inserts deep-copy outside these locks. Updates retain the storage write
/// lock across read/copy/replace to preserve the old terms and atomicity.
pub struct EtsTable {
    pub name: u32,        // atom ID
    pub table_type: TableType,
    pub keypos: usize,    // 1-based key position in tuples
    /// Created with `named_table`: only then does `name` reach the table.
    pub named: bool,
    /// Owning process PID.  Atomic for lock-free reads in `ets:info`.
    owner: AtomicProcessId,
    /// Key-value store dispatched by table type.
    storage: EtsStorage,
    /// mmap-backed region pool for word buffer allocation.
    pool: Mutex<EtsRegionPool>,
}

impl EtsTable {
    /// Observe the owner without exposing the raw atomic's invariant.
    #[inline]
    pub fn owner(&self) -> ProcessId { self.owner.load(Ordering::Relaxed) }
}

/// A compact word buffer for storing ETS terms, backed by an mmap'd
/// region from the table's `EtsRegionPool`.
///
/// Boxed terms are allocated as contiguous words in the region, and
/// `ErlTerm::boxed_unchecked()` pointers point into this buffer.  Reference-counted
/// binaries are owned via `refc_binaries`.
pub(crate) struct CompactHeap {
    /// Owned word reservation, absent for empty heaps.
    allocation: Option<EtsAllocation>,
    /// Number of u64 words currently used.
    words_len: usize,
    /// Ownership anchors for reference-counted binaries.
    refc_binaries: Vec<Arc<BinaryData>>,
}

impl CompactHeap {
    /// Consume an exclusive pool reservation, or create an empty heap.
    pub(crate) fn from_pool(allocation: Option<EtsAllocation>) -> Self {
        CompactHeap { allocation, words_len: 0, refc_binaries: Vec::new() }
    }

    /// Allocate `n` words from the pre-reserved buffer and return a pointer.
    ///
    /// Panics before changing the cursor if the reservation would be exceeded,
    /// including in release builds. Returned words are not initialized.
    #[inline]
    pub(crate) fn alloc_words(&mut self, n: usize) -> *mut u64 {
        let offset = self.words_len;
        let (words_ptr, capacity) = self.allocation.as_mut().map_or(
            (std::ptr::null_mut(), 0),
            |allocation| (allocation.as_mut_ptr(), allocation.len_words()),
        );
        // The cursor is private and always <= capacity. Subtraction avoids
        // overflowing offset + n when a caller requests an enormous allocation.
        assert!(n <= capacity - offset, "CompactHeap capacity exceeded");
        self.words_len = offset + n;
        if offset == 0 {
            // Empty heaps use a null base and may reserve zero words.
            words_ptr
        } else {
            // SAFETY: the checked cursor is within the owned pool reservation
            // (or one past it for a zero-length allocation).
            unsafe { words_ptr.add(offset) }
        }
    }
}

// SAFETY: `allocation` and `refc_binaries` own all backing storage. Moving this
// heap preserves allocation addresses. Construction has exclusive access to
// its reserved range, and mutation requires &mut self plus unsafe pointer use.
unsafe impl Send for CompactHeap {}
// SAFETY: shared access does not mutate the word buffer. Published heaps are
// immutable; raw-pointer operations must honor the construction/read boundary.
// Unlike a table lock, `allocation` keeps the mapping alive after table deletion.
unsafe impl Sync for CompactHeap {}

/// Shared backing storage for an ETS entry. Kept alive by Arc
/// references from the table and from processes holding zero-copy
/// lookup results.
pub struct StoredTupleInner {
    _heap: CompactHeap,
}

/// Deep-copied tuple stored in ETS (independent of any process heap).
/// All elements are stored in a compact word buffer.
///
/// The backing memory (`inner._heap`) is Arc-wrapped so that zero-copy
/// lookups can hold a reference to the storage without preventing
/// the table from being modified. `elements` stays outside the Arc
/// so that `update_counter` can mutate element slots in place.
struct StoredTuple {
    /// Arc-wrapped backing storage for all boxed terms in this tuple.
    inner: Arc<StoredTupleInner>,
    /// The element terms (immediates or pointers into `inner._heap.words`).
    elements: Vec<ErlTerm>,
}

/// Key for HashMap lookup — canonical representation.
/// Immediate terms (atoms, small ints, pids, nil) are stored inline as a
/// raw u64 to avoid `Vec<u8>` allocation on the hot path.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum TermKey {
    Immediate(u64),
    Boxed(Vec<u8>),
}

impl TermKey {
    /// # Safety
    /// All input terms and reachable objects must remain live and unmoved by GC
    /// for this call. Binary backing bytes must not be concurrently mutated.
    pub(crate) unsafe fn from_term(term: ErlTerm) -> Self {
        if term.is_atom() || term.is_small_int() || term.is_pid() || term.is_nil() {
            return TermKey::Immediate(term.0);
        }
        let mut bytes = Vec::new();
        Self::serialize_term(term, &mut bytes);
        TermKey::Boxed(bytes)
    }

    /// Append the canonical bytes of `term` to `out`. Parts still to
    /// serialize wait on a heap-allocated stack, so nesting depth costs heap,
    /// not native stack.
    /// # Safety
    /// All input terms and reachable objects must remain live and unmoved by GC
    /// for this call. Binary backing bytes must not be concurrently mutated.
    unsafe fn serialize_term(term: ErlTerm, out: &mut Vec<u8>) {
        let mut pending = WorkStack::new(&SPARE_KEY_PARTS);
        Self::serialize_part(KeyPart::Term(term), out, &mut pending);
        while let Some(part) = pending.pop() {
            Self::serialize_part(part, out, &mut pending);
        }
    }

    /// Serialize one term (or the next cell of a list), pushing the parts
    /// it contains on `pending` in reverse, so they pop in order.
    /// # Safety
    /// As for `serialize_term`.
    unsafe fn serialize_part(part: KeyPart, out: &mut Vec<u8>, pending: &mut WorkStack<KeyPart>) {
        let term = match part {
            KeyPart::Term(term) => term,
            KeyPart::ListRest(cursor) => {
                if let Some(ptr) = cursor.as_cons_ptr() {
                    unsafe {
                        pending.push(KeyPart::ListRest((*ptr).tail));
                        pending.push(KeyPart::Term((*ptr).head));
                    }
                } else {
                    out.push(b'E'); // end of list marker
                    pending.push(KeyPart::Term(cursor)); // tail (nil or improper)
                }
                return;
            }
        };
        // Atoms, small ints, pids, nil — just use the raw u64
        if term.is_atom() || term.is_small_int() || term.is_pid() || term.is_nil() {
            out.extend_from_slice(&term.0.to_le_bytes());
            return;
        }
        // Binary — serialize content bytes
        if term.is_binary() {
            out.push(b'B');
            if let Some((ptr, len)) = unsafe { crate::ffi::binary_data_pub(term) } {
                let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
                out.extend_from_slice(&(len as u64).to_le_bytes());
                out.extend_from_slice(slice);
            }
            return;
        }
        // Cons (list): each head, then the end marker and the tail
        if term.is_cons() {
            out.push(b'L');
            pending.push(KeyPart::ListRest(term));
            return;
        }
        // Tuple
        if term.is_boxed() {
            if let Some(ptr) = term.as_boxed_ptr() {
                let header = BoxedHeader(unsafe { *ptr });
                if header.subtype() == crate::term::BOXED_REFERENCE {
                    out.push(b'R');
                    out.extend_from_slice(&header.0.to_le_bytes());
                    out.extend_from_slice(&(*ptr.add(crate::term::REFERENCE_ID_WORD_INDEX)).to_le_bytes());
                    return;
                }
                if header.subtype() == BOXED_TUPLE {
                    let arity = header.arity() as usize;
                    out.push(b'T');
                    out.extend_from_slice(&(arity as u64).to_le_bytes());
                    for i in (0..arity).rev() {
                        pending.push(KeyPart::Term(ErlTerm(unsafe { *ptr.add(1 + i) })));
                    }
                    return;
                }
            }
        }
        // Fallback: raw u64
        out.extend_from_slice(&term.0.to_le_bytes());
    }
}

/// A part of a key still to serialize: a whole term, or the rest of a list
/// from a cell (or its tail) onward.
enum KeyPart {
    Term(ErlTerm),
    ListRest(ErlTerm),
}

thread_local! {
    static SPARE_KEY_PARTS: Cell<Vec<KeyPart>> = const { Cell::new(Vec::new()) };
    static SPARE_TERMS: Cell<Vec<ErlTerm>> = const { Cell::new(Vec::new()) };
}

/// Compare a stored ErlTerm against a pre-computed TermKey.
/// Short-circuits for immediate terms (atom/int/pid/nil) to avoid
/// serialisation and allocation entirely.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn terms_equal(a: ErlTerm, b: &TermKey) -> bool {
    if a.is_atom() || a.is_small_int() || a.is_pid() || a.is_nil() {
        return *b == TermKey::Immediate(a.0);
    }
    TermKey::from_term(a) == *b
}

/// Key for BTreeMap (ordered_set) — wraps a TermKey with Ord based on
/// Erlang term ordering via `compare_terms`.
///
/// Stores the original `ErlTerm` key (pointing into the StoredTuple's
/// CompactHeap) alongside the exact `TermKey` bytes for update bookkeeping.
/// Equality follows the same numeric/structural ordering as Ord.
#[derive(Clone)]
struct OrdTermKey {
    /// The key term — points into a StoredTuple's CompactHeap for stored
    /// entries, or into a process heap for transient lookup keys.
    term: ErlTerm,
    /// Exact serialization retained for replacement/update bookkeeping.
    eq_key: TermKey,
}

impl OrdTermKey {
    fn stored(stored: &StoredTuple, keypos: usize, eq_key: TermKey) -> Self {
        let term = stored.elements[keypos - 1];
        Self { term, eq_key }
    }
}

impl PartialEq for OrdTermKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for OrdTermKey {}

impl PartialOrd for OrdTermKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrdTermKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // SAFETY: boxed stored keys borrow their paired value's heap. Replacements
        // remove the complete old pair before dropping that heap. Transient keys
        // borrow live caller input for the duration of the operation.
        let r = unsafe { compare_terms(self.term, other.term) };
        if r < 0 {
            std::cmp::Ordering::Less
        } else if r > 0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    }
}

/// Count the number of heap words needed to deep-copy a term.
/// This is a pre-pass before `deep_copy_into_compact` to determine the
/// exact Vec capacity needed, preventing reallocation (and thus pointer
/// invalidation) during the copy phase. Terms still to count wait on a
/// heap-allocated stack, so nesting depth costs heap, not native stack.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub(crate) unsafe fn count_heap_words(term: ErlTerm) -> usize {
    if is_unboxed(term) {
        return 0;
    }
    let mut pending = WorkStack::new(&SPARE_TERMS);
    // SAFETY: forwarded from the caller, for the root and every term it reaches.
    let mut count = unsafe { object_heap_words(term, &mut pending) };
    while let Some(term) = pending.pop() {
        count += unsafe { object_heap_words(term, &mut pending) };
    }
    count
}

/// True for the terms ETS copies stand alone: atoms, small integers, pids and nil.
#[inline]
fn is_unboxed(term: ErlTerm) -> bool {
    term.is_atom() || term.is_small_int() || term.is_pid() || term.is_nil()
}

/// Words a term's own storage needs in a `CompactHeap` (for a list, every
/// cell along its tail), queueing the contained terms that need words of
/// their own on `pending`.
/// # Safety
/// As for `count_heap_words`.
unsafe fn object_heap_words(term: ErlTerm, pending: &mut WorkStack<ErlTerm>) -> usize {
    use crate::heap::REFC_BINARY_THRESHOLD;
    use crate::term::{BOXED_MAP, BOXED_FLOAT, BOXED_BIGINT,
                      BOXED_REFERENCE, BoxedHeader,
                      REFC_BINARY_TOTAL_WORDS, REFERENCE_TOTAL_WORDS};

    let mut queue = |term: ErlTerm| {
        if !is_unboxed(term) {
            pending.push(term);
        }
    };
    if is_unboxed(term) {
        return 0;
    }
    if term.is_cons() {
        let mut count = 0;
        let mut cursor = term;
        while cursor.is_cons() {
            if let Some(ptr) = cursor.as_cons_ptr() {
                unsafe {
                    count += 2;
                    queue((*ptr).head);
                    cursor = (*ptr).tail;
                }
            } else {
                break;
            }
        }
        queue(cursor); // tail
        return count;
    }
    if term.is_binary() {
        if let Some((_ptr, len)) = unsafe { crate::ffi::binary_data_pub(term) } {
            if len <= REFC_BINARY_THRESHOLD {
                return 1 + len.div_ceil(8); // header + data words
            } else {
                return REFC_BINARY_TOTAL_WORDS;
            }
        }
        return 0;
    }
    if term.is_boxed() {
        if let Some(ptr) = term.as_boxed_ptr() {
            let header = BoxedHeader(unsafe { *ptr });
            match header.subtype() {
                BOXED_TUPLE => {
                    let arity = header.arity() as usize;
                    for i in 0..arity {
                        queue(ErlTerm(unsafe { *ptr.add(1 + i) }));
                    }
                    return 1 + arity; // header + element slots
                }
                BOXED_MAP => {
                    let num_pairs = header.arity() as usize;
                    let total_elems = num_pairs * 2;
                    for i in 0..total_elems {
                        queue(ErlTerm(unsafe { *ptr.add(1 + i) }));
                    }
                    return 1 + total_elems;
                }
                BOXED_FLOAT => return 2,
                BOXED_BIGINT => {
                    let limb_count = header.arity() as usize;
                    return crate::term::bigint_total_words(limb_count);
                }
                BOXED_REFERENCE => return REFERENCE_TOTAL_WORDS,
                _ => return 0,
            }
        }
    }
    0
}

/// Deep-copy a tuple's elements from process heap into a StoredTuple.
///
/// Takes a pre-allocated `CompactHeap` (obtained from the pool under a brief
/// lock).  The deep copy itself runs without any lock — the CompactHeap buffer
/// is private to the caller until the StoredTuple is inserted into the map.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn deep_copy_tuple_to_stored(elements: &[ErlTerm], compact: CompactHeap) -> StoredTuple {
    let mut compact = compact;
    let copied_elems: Vec<ErlTerm> = elements.iter()
        .map(|&elem| deep_copy_into_compact(elem, &mut compact))
        .collect();
    StoredTuple {
        inner: Arc::new(StoredTupleInner { _heap: compact }),
        elements: copied_elems,
    }
}

/// Allocate a CompactHeap from the pool with enough capacity for `elements`.
/// Acquires the pool Mutex briefly for the bump allocation only.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn alloc_compact_for_elements(pool: &Mutex<EtsRegionPool>, elements: &[ErlTerm]) -> CompactHeap {
    let total_words: usize = elements.iter().map(|&e| count_heap_words(e)).sum();
    let allocation = if total_words > 0 {
        Some(pool.lock().unwrap().alloc(total_words))
    } else { None };
    CompactHeap::from_pool(allocation)
}

/// Deep-copy a term into a `CompactHeap` (flat word buffer), which must hold
/// the `count_heap_words` words the term needs.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub(crate) unsafe fn deep_copy_into_compact(term: ErlTerm, heap: &mut CompactHeap) -> ErlTerm {
    if is_unboxed(term) {
        return term;
    }
    // SAFETY: forwarded from the caller; compact heap words never move.
    unsafe { EtsCopier::copy(term, heap) }
}

/// Deep-copy a term into a `ProcessHeap` (used for lookup — copying stored
/// terms back into a process's bumpalo arena).
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub(crate) unsafe fn deep_copy_into_heap(term: ErlTerm, heap: &mut ProcessHeap) -> ErlTerm {
    if is_unboxed(term) {
        return term;
    }
    // SAFETY: forwarded from the caller; process heap words never move
    // during the copy.
    unsafe { EtsCopier::copy(term, heap) }
}

/// Where an ETS copy allocates: a stored entry's `CompactHeap`, or the
/// process heap a stored term is read back into.
trait CopyTarget {
    /// Allocate `n` uninitialized words that stay in place.
    fn alloc_words(&mut self, n: usize) -> *mut u64;

    /// Copy a boxed term that holds no other terms (a binary, float, bigint
    /// or reference); any other term is kept as it is.
    /// # Safety
    /// As for `deep_copy_into_compact`.
    unsafe fn copy_leaf(&mut self, term: ErlTerm) -> ErlTerm;
}

impl CopyTarget for CompactHeap {
    fn alloc_words(&mut self, n: usize) -> *mut u64 {
        CompactHeap::alloc_words(self, n)
    }

    unsafe fn copy_leaf(&mut self, term: ErlTerm) -> ErlTerm {
        use crate::heap::REFC_BINARY_THRESHOLD;
        use crate::term::{BOXED_FLOAT, BOXED_BIGINT, BOXED_REFERENCE,
                          REFC_BINARY_TOTAL_WORDS, REFC_BINARY_PTR_WORD_INDEX,
                          REFC_BINARY_OFFSET_WORD_INDEX, REFERENCE_TOTAL_WORDS,
                          REFERENCE_ID_WORD_INDEX};

        if term.is_binary() {
            if let Some((ptr, len)) = unsafe { crate::ffi::binary_data_pub(term) } {
                let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
                if len <= REFC_BINARY_THRESHOLD {
                    // Inline heap binary: [header | data words...]
                    let data_words = len.div_ceil(8);
                    let total_words = 1 + data_words;
                    let words = self.alloc_words(total_words);
                    unsafe {
                        *words = BoxedHeader::new(crate::term::BoxedSubtype::HeapBinary, (len * 8) as u64).0;
                        std::ptr::copy_nonoverlapping(
                            bytes.as_ptr(),
                            (words.add(1)) as *mut u8,
                            len,
                        );
                    }
                    return ErlTerm::boxed_unchecked(words as *const u64);
                } else {
                    // Refc binary: [header | arc_ptr | offset]
                    let data: Arc<BinaryData> = Arc::new(BinaryData::new(bytes.to_vec()));
                    let raw_arc_ptr = Arc::as_ptr(&data) as u64;
                    let words = self.alloc_words(REFC_BINARY_TOTAL_WORDS);
                    unsafe {
                        *words = BoxedHeader::new(crate::term::BoxedSubtype::RefcBinary, (len * 8) as u64).0;
                        *words.add(REFC_BINARY_PTR_WORD_INDEX) = raw_arc_ptr;
                        *words.add(REFC_BINARY_OFFSET_WORD_INDEX) = 0;
                    }
                    self.refc_binaries.push(data);
                    return ErlTerm::boxed_unchecked(words as *const u64);
                }
            }
            return term;
        }
        if let Some(ptr) = term.as_boxed_ptr() {
            let header = BoxedHeader(unsafe { *ptr });
            match header.subtype() {
                BOXED_FLOAT => {
                    let bits = unsafe { *ptr.add(1) };
                    let words = self.alloc_words(2);
                    unsafe {
                        *words = BoxedHeader::new(crate::term::BoxedSubtype::Float, 0).0;
                        *words.add(1) = bits;
                    }
                    return ErlTerm::boxed_unchecked(words as *const u64);
                }
                BOXED_BIGINT => {
                    if let Some(big) = crate::bigint::term_to_bigint(term) {
                        // Bigint: copy using a temporary ProcessHeap, then
                        // transfer the words to compact heap.
                        let mut tmp = ProcessHeap::new();
                        let tmp_term = crate::bigint::bigint_to_term(&mut tmp, big);
                        return copy_from_process_heap_to_compact(tmp_term, self);
                    }
                    return term;
                }
                BOXED_REFERENCE => {
                    let ref_id = unsafe { *ptr.add(REFERENCE_ID_WORD_INDEX) };
                    let words = self.alloc_words(REFERENCE_TOTAL_WORDS);
                    unsafe {
                        *words = header.0;
                        *words.add(REFERENCE_ID_WORD_INDEX) = ref_id;
                    }
                    return ErlTerm::boxed_unchecked(words as *const u64);
                }
                _ => return term,
            }
        }
        term
    }
}

impl CopyTarget for ProcessHeap {
    fn alloc_words(&mut self, n: usize) -> *mut u64 {
        ProcessHeap::alloc_words(self, n)
    }

    unsafe fn copy_leaf(&mut self, term: ErlTerm) -> ErlTerm {
        if term.is_binary() {
            if let Some((ptr, len)) = unsafe { crate::ffi::binary_data_pub(term) } {
                let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
                return self.alloc_binary_auto(bytes);
            }
            return term;
        }
        if let Some(ptr) = term.as_boxed_ptr() {
            let header = BoxedHeader(unsafe { *ptr });
            match header.subtype() {
                crate::term::BOXED_FLOAT => {
                    let bits = unsafe { *ptr.add(1) };
                    return self.alloc_float(f64::from_bits(bits));
                }
                crate::term::BOXED_BIGINT => {
                    if let Some(big) = crate::bigint::term_to_bigint(term) {
                        return crate::bigint::bigint_to_term(self, big);
                    }
                    return term;
                }
                crate::term::BOXED_REFERENCE => {
                    let ref_id = unsafe { *ptr.add(crate::term::REFERENCE_ID_WORD_INDEX) };
                    let words = self.alloc_words(crate::term::REFERENCE_TOTAL_WORDS);
                    unsafe {
                        *words = header.0;
                        *words.add(crate::term::REFERENCE_ID_WORD_INDEX) = ref_id;
                    }
                    return ErlTerm::boxed_unchecked(words);
                }
                _ => return term,
            }
        }
        term
    }
}

/// Copies a term into a `CopyTarget` through `copy_graph`: work still to
/// copy waits on a heap-allocated stack, so nesting depth costs heap, not
/// native stack. Lists, tuples and maps are copied here; other terms through
/// `CopyTarget::copy_leaf`. A subterm shared within the term is copied once
/// for each place that holds it.
struct EtsCopier<'a, T: CopyTarget> {
    target: &'a mut T,
    slots: SlotStack,
}

impl<T: CopyTarget> SlotCopier for EtsCopier<'_, T> {
    fn slots(&mut self) -> &mut SlotStack {
        &mut self.slots
    }

    unsafe fn copy_word(&mut self, word: u64) -> u64 {
        // SAFETY: forwarded from `copy_graph`'s caller.
        unsafe { self.copy_object(ErlTerm(word)).0 }
    }
}

impl<'a, T: CopyTarget> EtsCopier<'a, T> {
    /// # Safety
    /// As for `deep_copy_into_compact`, and `target` never moves words it
    /// has allocated.
    unsafe fn copy(term: ErlTerm, target: &'a mut T) -> ErlTerm {
        let mut copier = EtsCopier { target, slots: SlotStack::default() };
        // SAFETY: forwarded from the caller.
        ErlTerm(unsafe { copy_graph(&mut copier, term.0) })
    }

    /// Copy one term, queueing the elements of tuples and maps instead of
    /// copying them; a list copies a run of cells (see `copy_list`).
    unsafe fn copy_object(&mut self, term: ErlTerm) -> ErlTerm {
        if is_unboxed(term) {
            return term;
        }
        if term.is_cons() {
            return unsafe { self.copy_list(term) };
        }
        if !unsafe { term.is_binary() } {
            if let Some(ptr) = term.as_boxed_ptr() {
                let header = BoxedHeader(unsafe { *ptr });
                let (subtype, slots) = match header.subtype() {
                    BOXED_TUPLE => (crate::term::BoxedSubtype::Tuple, header.arity() as usize),
                    crate::term::BOXED_MAP => (crate::term::BoxedSubtype::Map, 2 * header.arity() as usize),
                    _ => return unsafe { self.target.copy_leaf(term) },
                };
                // [header | slots...], with the elements (or interleaved
                // keys and values) queued for copying.
                let words = self.target.alloc_words(1 + slots);
                unsafe {
                    *words = BoxedHeader::new(subtype, header.arity()).0;
                    self.slots.push(words.add(1), ptr.add(1), slots);
                    return ErlTerm::boxed_unchecked(words);
                }
            }
        }
        unsafe { self.target.copy_leaf(term) }
    }

    /// Copy the cells of a list along its tail while their heads are
    /// immediates. The first cell whose head needs copying queues its head
    /// and tail and ends the run, so the head is copied before the rest of
    /// the list.
    unsafe fn copy_list(&mut self, list: ErlTerm) -> ErlTerm {
        let mut first = None;
        let mut prev_tail: *mut u64 = std::ptr::null_mut();
        let mut cursor = list;
        loop {
            let mut next = None;
            let copied = match cursor.as_cons_ptr() {
                // The terminator: nil or an improper tail.
                None => unsafe { self.copy_object(cursor) },
                Some(cell) => {
                    let words = self.target.alloc_words(2);
                    // SAFETY: `cell` is a live source cons cell (head word
                    // first, then tail); `words` reserves its two-word copy.
                    unsafe {
                        let src = cell as *const u64;
                        let head = *src;
                        if is_plain_immediate(head) {
                            *words = head;
                            next = Some((words.add(1), (*cell).tail));
                        } else {
                            self.slots.push(words, src, 2);
                        }
                        ErlTerm::cons_unchecked(words as *const crate::term::ConsCell)
                    }
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
}

/// Deep-copy a term from a process heap into a `CompactHeap`.
/// Used for the bigint fallback path.
fn copy_from_process_heap_to_compact(term: ErlTerm, heap: &mut CompactHeap) -> ErlTerm {
    if term.is_atom() || term.is_small_int() || term.is_pid() || term.is_nil() {
        return term;
    }
    if term.is_boxed() {
        if let Some(ptr) = term.as_boxed_ptr() {
            let header = BoxedHeader(unsafe { *ptr });
            let subtype = header.subtype();
            if subtype == crate::term::BOXED_BIGINT {
                // header.arity() = limb_count for bigints
                let limb_count = header.arity() as usize;
                let total = crate::term::bigint_total_words(limb_count);
                let words = heap.alloc_words(total);
                unsafe {
                    std::ptr::copy_nonoverlapping(ptr, words, total);
                }
                // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                return unsafe { ErlTerm::boxed_unchecked(words as *const u64) };
            }
        }
    }
    term
}

/// Copy a stored term back into a process heap.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn copy_to_process_heap(term: ErlTerm, heap: &mut ProcessHeap) -> ErlTerm {
    if term.is_atom() || term.is_small_int() || term.is_pid() || term.is_nil() {
        return term;
    }
    deep_copy_into_heap(term, heap)
}

/// Store a tuple (deep-copied from process heap).
///
/// Split into phases to minimize lock hold times:
/// 1. Extract key/elements from process heap (no lock)
/// 2. Allocate from pool (brief Mutex)
/// 3. Deep copy into private CompactHeap (no lock)
/// 4. Insert into storage (brief write lock)
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn store_tuple(table: &EtsTable, tuple_term: ErlTerm) -> Result<bool, ()> {
    // Phase 1: Extract tuple elements from process heap (no lock needed)
    let ptr = match tuple_term.as_boxed_ptr() {
        Some(p) => p,
        None => return Err(()),
    };
    let header = BoxedHeader(unsafe { *ptr });
    if header.subtype() != BOXED_TUPLE {
        return Err(());
    }
    let arity = header.arity() as usize;
    if arity < table.keypos {
        return Err(());
    }

    let key_term = ErlTerm(unsafe { *ptr.add(table.keypos) });
    let key = TermKey::from_term(key_term);

    let elements: Vec<ErlTerm> = (0..arity)
        .map(|i| ErlTerm(unsafe { *ptr.add(1 + i) }))
        .collect();

    // Phase 2+3: Allocate from pool (brief lock) then deep copy (no lock)
    let compact = alloc_compact_for_elements(&table.pool, &elements);
    let stored = deep_copy_tuple_to_stored(&elements, compact);

    // Phase 4: Insert into storage (brief write lock).
    match &table.storage {
        EtsStorage::Set(map_lock) => {
            let old = map_lock.write().unwrap().insert(key, stored);
            Ok(old.is_some())
        }
        EtsStorage::OrderedSet(map_lock) => {
            // Build OrdTermKey from the deep-copied key in the stored tuple
            let ord_key = OrdTermKey::stored(&stored, table.keypos, key);
            let mut map = map_lock.write().unwrap();
            match map.entry(ord_key) {
                Entry::Vacant(entry) => {
                    entry.insert(stored);
                    Ok(false)
                }
                Entry::Occupied(mut entry) if entry.key().term.is_immediate() => {
                    // Immediate keys have no backing heap to invalidate.
                    entry.insert(stored);
                    Ok(true)
                }
                Entry::Occupied(entry) => {
                    // BTreeMap retains an existing key when replacing only the
                    // value. Detach both halves before releasing boxed key data.
                    let old = entry.remove_entry();
                    let key = TermKey::from_term(stored.elements[table.keypos - 1]);
                    map.insert(OrdTermKey::stored(&stored, table.keypos, key), stored);
                    drop(old);
                    Ok(true)
                }
            }
        }
        EtsStorage::Bag(map_lock) => {
            let mut map = map_lock.write().unwrap();
            let vec = map.entry(key).or_insert_with(Vec::new);
            // Pre-serialize the new tuple's elements once (immediates are
            // zero-alloc thanks to TermKey::Immediate).
            let new_keys: Vec<TermKey> = stored.elements.iter()
                .map(|e| TermKey::from_term(*e))
                .collect();
            let is_dup = vec.iter().any(|existing| {
                existing.elements.len() == new_keys.len()
                    && existing.elements.iter().zip(new_keys.iter()).all(|(a, nk)| {
                        terms_equal(*a, nk)
                    })
            });
            if !is_dup {
                vec.push(stored);
            }
            Ok(false)
        }
        EtsStorage::DuplicateBag(map_lock) => {
            let mut map = map_lock.write().unwrap();
            map.entry(key).or_insert_with(Vec::new).push(stored);
            Ok(false)
        }
    }
}

/// Build a zero-copy result tuple from a stored tuple.
fn stored_to_zero_copy(stored: &StoredTuple, heap: &mut ProcessHeap) -> ErlTerm {
    let arc = stored.inner.clone();
    let elems: Vec<ErlTerm> = stored.elements.iter()
        .map(|&elem| {
            if elem.is_atom() || elem.is_small_int()
                || elem.is_pid() || elem.is_nil()
            {
                elem
            } else {
                elem.with_external_bit()
            }
        })
        .collect();
    let result = heap.alloc_tuple(&elems);
    heap.retain_borrowed_heap(arc);
    result
}

/// Look up tuples by key using zero-copy: returns tagged pointers
/// that reference the Arc-owned ETS storage directly.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn lookup_tuples(table: &EtsTable, key_term: ErlTerm, heap: &mut ProcessHeap) -> Vec<ErlTerm> {
    let key = TermKey::from_term(key_term);
    match &table.storage {
        EtsStorage::Set(map_lock) => {
            let map = map_lock.read().unwrap();
            match map.get(&key) {
                Some(stored) => vec![stored_to_zero_copy(stored, heap)],
                None => Vec::new(),
            }
        }
        EtsStorage::OrderedSet(map_lock) => {
            let ord_key = OrdTermKey { term: key_term, eq_key: key };
            let map = map_lock.read().unwrap();
            match map.get(&ord_key) {
                Some(stored) => vec![stored_to_zero_copy(stored, heap)],
                None => Vec::new(),
            }
        }
        EtsStorage::Bag(map_lock) | EtsStorage::DuplicateBag(map_lock) => {
            let map = map_lock.read().unwrap();
            match map.get(&key) {
                Some(vec) => vec.iter().map(|stored| stored_to_zero_copy(stored, heap)).collect(),
                None => Vec::new(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Global table registry
// ---------------------------------------------------------------------------

struct EtsRegistry {
    tables: RwLock<HashMap<TableId, Arc<EtsTable>>>,
    /// Names of `named_table` tables.  Unnamed tables never appear here.
    name_to_id: RwLock<HashMap<u32, TableId>>,
    /// Reverse mapping: owner PID → set of table IDs owned by that process.
    pid_to_tables: RwLock<HashMap<ProcessId, HashSet<TableId>>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl EtsRegistry {
    fn new() -> Self {
        EtsRegistry {
            tables: RwLock::new(HashMap::new()),
            name_to_id: RwLock::new(HashMap::new()),
            pid_to_tables: RwLock::new(HashMap::new()),
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Create a table.  `None` if `named` and a named table already has
    /// this name (badarg in BEAM).
    ///
    /// Lock order: `tables` may be held while taking `name_to_id`
    /// (`delete_tables_owned_by`), never the other way round, so no path
    /// holds `name_to_id` while it takes `tables`.
    fn create_table(&self, name: u32, named: bool, table_type: TableType, keypos: usize, owner_pid: ProcessId) -> Option<TableId> {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let storage = match table_type {
            TableType::Set => EtsStorage::Set(RwLock::new(HashMap::new())),
            TableType::OrderedSet => EtsStorage::OrderedSet(RwLock::new(BTreeMap::new())),
            TableType::Bag => EtsStorage::Bag(RwLock::new(HashMap::new())),
            TableType::DuplicateBag => EtsStorage::DuplicateBag(RwLock::new(HashMap::new())),
        };
        let table = Arc::new(EtsTable {
            name,
            table_type,
            keypos,
            named,
            owner: AtomicProcessId::new(owner_pid),
            storage,
            pool: Mutex::new(EtsRegionPool::new()),
        });
        if named {
            // Check and claim the name under one lock, so that only one of
            // two processes creating the same named table succeeds.
            let mut name_map = self.name_to_id.write().unwrap();
            if name_map.contains_key(&name) {
                return None;
            }
            name_map.insert(name, id);
        }
        self.tables.write().unwrap().insert(id, table);
        self.pid_to_tables.write().unwrap()
            .entry(owner_pid)
            .or_default()
            .insert(id);
        Some(id)
    }

    /// Resolve a table ID or a table name to a table ID.  The name lock is
    /// released before the caller takes `tables` (see `create_table`).
    fn resolve_id(&self, id_or_name: ErlTerm) -> Option<TableId> {
        if let Some(id) = id_or_name.as_small_int() {
            return Some(id as u64);
        }
        let atom_id = id_or_name.as_atom_id()?;
        self.name_to_id.read().unwrap().get(&atom_id).copied()
    }

    fn delete_table(&self, id_or_name: ErlTerm) -> bool {
        let Some(table_id) = self.resolve_id(id_or_name) else {
            return false;
        };

        // Remove from tables map (drops the Arc<EtsTable> and all its data)
        let removed = {
            let mut tables = self.tables.write().unwrap();
            tables.remove(&table_id)
        };

        if let Some(table) = removed {
            if table.named {
                self.name_to_id.write().unwrap().remove(&table.name);
            }
            // Remove from owner's pid_to_tables
            let owner_pid = table.owner.load(Ordering::Relaxed);
            let mut pt = self.pid_to_tables.write().unwrap();
            if let Some(set) = pt.get_mut(&owner_pid) {
                set.remove(&table_id);
                if set.is_empty() {
                    pt.remove(&owner_pid);
                }
            }
            true
        } else {
            false
        }
    }

    /// Delete all tables owned by the given process.  Called on process death.
    fn delete_tables_owned_by(&self, pid: ProcessId) {
        let table_ids: HashSet<TableId> = {
            let mut pt = self.pid_to_tables.write().unwrap();
            match pt.remove(&pid) {
                Some(ids) => ids,
                None => return,
            }
        };

        let mut tables_map = self.tables.write().unwrap();
        let mut name_map = self.name_to_id.write().unwrap();
        for table_id in table_ids {
            if let Some(table) = tables_map.remove(&table_id) {
                if table.named {
                    name_map.remove(&table.name);
                }
            }
        }
    }

    /// Transfer ownership of a table to a new process.
    fn give_away(&self, id_or_name: ErlTerm, current_owner: ProcessId, new_owner: ProcessId) -> bool {
        let Some(table_id) = self.resolve_id(id_or_name) else {
            return false;
        };
        let Some(table) = self.tables.read().unwrap().get(&table_id).cloned() else {
            return false;
        };

        // Verify caller is current owner
        if table.owner.load(Ordering::Relaxed) != current_owner {
            return false;
        }

        // Update owner atomically
        table.owner.store(new_owner, Ordering::Relaxed);

        // Update pid_to_tables
        let mut pt = self.pid_to_tables.write().unwrap();
        if let Some(set) = pt.get_mut(&current_owner) {
            set.remove(&table_id);
            if set.is_empty() {
                pt.remove(&current_owner);
            }
        }
        pt.entry(new_owner).or_default().insert(table_id);
        true
    }

    fn get_table(&self, id_or_name: ErlTerm) -> Option<Arc<EtsTable>> {
        let table_id = self.resolve_id(id_or_name)?;
        self.tables.read().unwrap().get(&table_id).cloned()
    }
}

static ETS_REGISTRY: OnceLock<EtsRegistry> = OnceLock::new();

fn registry() -> &'static EtsRegistry {
    ETS_REGISTRY.get_or_init(EtsRegistry::new)
}

// ---------------------------------------------------------------------------
// Public API (called from FFI)
// ---------------------------------------------------------------------------

/// Create a new ETS table owned by `owner_pid`.  Returns `Some(table_id)`
/// on success, `None` if a named table with the same name already exists
/// (badarg in BEAM).
pub fn ets_new(name: u32, opts: &[(String, ErlTerm)], owner_pid: ProcessId) -> Option<TableId> {
    new_table(name, opts, owner_pid).map(|(id, _)| id)
}

/// Create a new ETS table and return what `ets:new/2` returns in Erlang:
/// the name for a `named_table`, as BEAM does, otherwise the table ID.
/// `None` if a named table with the same name already exists.
pub fn ets_new_handle(name: u32, opts: &[(String, ErlTerm)], owner_pid: ProcessId) -> Option<ErlTerm> {
    let (id, named) = new_table(name, opts, owner_pid)?;
    Some(if named { ErlTerm::atom(name) } else { ErlTerm::small_int(id as i64) })
}

fn new_table(name: u32, opts: &[(String, ErlTerm)], owner_pid: ProcessId) -> Option<(TableId, bool)> {
    let mut keypos: usize = 1;
    let mut table_type = TableType::Set;
    let mut is_named = false;

    for (opt_name, opt_val) in opts {
        match opt_name.as_str() {
            "keypos" => {
                if let Some(v) = opt_val.as_small_int() {
                    keypos = v as usize;
                }
            }
            "set" => table_type = TableType::Set,
            "ordered_set" => table_type = TableType::OrderedSet,
            "bag" => table_type = TableType::Bag,
            "duplicate_bag" => table_type = TableType::DuplicateBag,
            "named_table" => is_named = true,
            _ => {} // ignore unknown options
        }
    }

    // BEAM raises badarg if a named table with this name already exists.
    let id = registry().create_table(name, is_named, table_type, keypos, owner_pid)?;
    Some((id, is_named))
}

/// Delete all ETS tables owned by the given process.
/// Called from the scheduler when a process exits.
pub fn delete_tables_owned_by(pid: ProcessId) {
    registry().delete_tables_owned_by(pid);
}

/// Transfer ownership of a table to a new process.
/// Returns true on success, false if table not found or caller is not the owner.
pub fn ets_give_away(table_ref: ErlTerm, current_owner: ProcessId, new_owner: ProcessId) -> bool {
    registry().give_away(table_ref, current_owner, new_owner)
}

/// Invalid table or object passed to an ETS insertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InsertError;

impl From<()> for InsertError {
    fn from(_: ()) -> Self { Self }
}

/// Insert a tuple (or list of tuples) into a table.
///
/// Returns `Ok(true)` on success, `Err(InsertError)` on badarg (table not found,
/// invalid element, improper list tail).  BEAM semantics: `ets:insert/2`
/// always returns `true` on success or raises `badarg`.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn ets_insert(table_ref: ErlTerm, object: ErlTerm) -> Result<bool, InsertError> {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return Err(InsertError),
    };

    if object.is_cons() {
        // List of tuples — ALL elements must be valid tuples.
        // If any element is invalid or the list is improper, raise badarg.
        let mut any_replaced = false;
        let mut cursor = object;
        while cursor.is_cons() {
            if let Some(ptr) = cursor.as_cons_ptr() {
                unsafe {
                    let replaced = store_tuple(&table, (*ptr).head)?;
                    any_replaced |= replaced;
                    cursor = (*ptr).tail;
                }
            } else {
                // as_cons_ptr returned None despite is_cons() — should not happen
                return Err(InsertError);
            }
        }
        // Verify the tail is nil (proper list). An improper tail is badarg.
        if !cursor.is_nil() {
            return Err(InsertError);
        }
        // Batch reclaim after all inserts — one pool lock instead of N.
        if any_replaced {
            table.pool.lock().unwrap().reclaim_dead_regions();
        }
        Ok(true)
    } else {
        // Single tuple
        store_tuple(&table, object)?;
        Ok(true)
    }
}

/// Look up by key, returns list of matching tuples on the given heap.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn ets_lookup(table_ref: ErlTerm, key: ErlTerm, heap: &mut ProcessHeap) -> ErlTerm {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return ErlTerm::nil(),
    };
    let tuples = lookup_tuples(&table, key, heap);
    heap.alloc_list_from_slice(&tuples)
}

/// Delete an entire table (ets:delete/1).
pub fn ets_delete_table(table_ref: ErlTerm) -> bool {
    registry().delete_table(table_ref)
}

/// Delete a key from a table.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn ets_delete(table_ref: ErlTerm, key: ErlTerm) -> bool {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return false,
    };
    let term_key = TermKey::from_term(key);
    let removed = match &table.storage {
        EtsStorage::Set(map_lock) => map_lock.write().unwrap().remove(&term_key).is_some(),
        EtsStorage::OrderedSet(map_lock) => {
            let ord_key = OrdTermKey { term: key, eq_key: term_key };
            map_lock.write().unwrap().remove(&ord_key).is_some()
        }
        EtsStorage::Bag(map_lock) | EtsStorage::DuplicateBag(map_lock) => {
            map_lock.write().unwrap().remove(&term_key).is_some()
        }
    };
    if removed {
        table.pool.lock().unwrap().reclaim_dead_regions();
    }
    removed
}

/// Delete all objects from a table.
pub fn ets_delete_all_objects(table_ref: ErlTerm) -> bool {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return false,
    };
    match &table.storage {
        EtsStorage::Set(map_lock) => { *map_lock.write().unwrap() = HashMap::new(); }
        EtsStorage::OrderedSet(map_lock) => { *map_lock.write().unwrap() = BTreeMap::new(); }
        EtsStorage::Bag(map_lock) | EtsStorage::DuplicateBag(map_lock) => {
            *map_lock.write().unwrap() = HashMap::new();
        }
    }
    table.pool.lock().unwrap().reclaim_dead_regions();
    true
}

/// Update a specific element of a tuple in the table.
/// update_element(Tab, Key, {Pos, Value}) or update_element(Tab, Key, [{Pos, Value}])
///
/// The map write lock covers the complete read-modify-write operation. This
/// keeps the old backing storage alive and serializes updates with insertion,
/// deletion and counter operations. Immediate updates modify element slots
/// without allocation; boxed updates copy into a new immutable backing heap.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn ets_update_element(
    table_ref: ErlTerm,
    key: ErlTerm,
    updates: &[(usize, ErlTerm)],
) -> bool {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return false,
    };
    let term_key = TermKey::from_term(key);
    let replaced_heap = match &table.storage {
        EtsStorage::Set(map_lock) => {
            let mut map = map_lock.write().unwrap();
            let Some(stored) = map.get_mut(&term_key) else {
                return false;
            };
            update_stored_elements(stored, &table.pool, updates)
        }
        EtsStorage::OrderedSet(map_lock) => {
            let ord_key = OrdTermKey {
                term: key,
                eq_key: term_key,
            };
            let mut map = map_lock.write().unwrap();
            match map.entry(ord_key) {
                Entry::Vacant(_) => return false,
                Entry::Occupied(mut entry) if entry.key().term.is_immediate() => {
                    update_stored_elements(entry.get_mut(), &table.pool, updates)
                }
                Entry::Occupied(entry) => {
                    // Own both halves while copying. No boxed comparison key
                    // remains in the tree after its old heap is released.
                    let (_old_key, mut stored) = entry.remove_entry();
                    let replaced = update_stored_elements(&mut stored, &table.pool, updates);
                    let key = TermKey::from_term(stored.elements[table.keypos - 1]);
                    map.insert(OrdTermKey::stored(&stored, table.keypos, key), stored);
                    replaced
                }
            }
        }
        _ => return false,
    };
    if replaced_heap {
        table.pool.lock().unwrap().reclaim_dead_regions();
    }
    true
}

/// Update the exclusively borrowed entry. Raw inputs must satisfy the caller's
/// term-lifetime contract. Returns whether an old backing heap was replaced.
unsafe fn update_stored_elements(
    stored: &mut StoredTuple,
    pool: &Mutex<EtsRegionPool>,
    updates: &[(usize, ErlTerm)],
) -> bool {
    if updates.iter().all(|&(pos, value)| {
        value.is_immediate()
            && (pos == 0
                || stored
                    .elements
                    .get(pos - 1)
                    .is_none_or(ErlTerm::is_immediate))
    }) {
        // Lookup results have their own outer tuple; no published heap words
        // change. Keep the existing heap for all untouched boxed elements.
        for &(pos, value) in updates {
            if pos >= 1 && pos <= stored.elements.len() {
                stored.elements[pos - 1] = value;
            }
        }
        return false;
    }
    let Some(elements) = apply_element_updates(stored, updates) else {
        return false;
    };
    // Lock order is storage -> pool. Insertion reserves from the pool first,
    // but releases that lock before acquiring storage, so there is no cycle.
    let compact = alloc_compact_for_elements(pool, &elements);
    *stored = deep_copy_tuple_to_stored(&elements, compact);
    true
}

/// Helper for ets_update_element: apply updates to a stored tuple's elements.
/// Returns None if nothing changed, Some(new_elems) otherwise.
fn apply_element_updates(stored: &StoredTuple, updates: &[(usize, ErlTerm)]) -> Option<Vec<ErlTerm>> {
    let mut elems = stored.elements.clone();
    let mut any_changed = false;
    for &(pos, val) in updates {
        if pos >= 1 && pos <= elems.len() {
            elems[pos - 1] = val;
            any_changed = true;
        }
    }
    if any_changed { Some(elems) } else { None }
}

/// Select and delete matching records.
/// This is a simplified version that supports the pattern used by onecached:
/// ets:select_delete(Tab, [{{'_','_','_','$1','_'}, [{'<', '$1', Timestamp}], [true]}])
///
/// Supports a single wildcard/variable clause with an optional comparison guard.
/// Invalid or unsupported specifications fail before any deletion.
/// # Safety
/// Input terms must remain live, immutable and unmoved throughout the call.
pub unsafe fn ets_select_delete(table_ref: ErlTerm, match_spec: ErlTerm) -> Result<i64, EtsError> {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return Err(EtsError::InvalidTable),
    };

    let guard = parse_simple_match_spec(match_spec)?;
    if matches!(guard, DeleteSpec::None) { return Ok(0); }

    let should_delete = |stored: &StoredTuple| guard.matches(stored);

    let deleted = match &table.storage {
        EtsStorage::Set(map_lock) => {
            let mut map = map_lock.write().unwrap();
            let before = map.len();
            if guard.is_all() {
                map.clear();
                table.pool.lock().unwrap().clear();
            } else {
                map.retain(|_key, stored| !should_delete(stored));
            }
            before - map.len()
        }
        EtsStorage::OrderedSet(map_lock) => {
            let mut map = map_lock.write().unwrap();
            let before = map.len();
            if guard.is_all() {
                map.clear();
                table.pool.lock().unwrap().clear();
            } else {
                map.retain(|_key, stored| !should_delete(stored));
            }
            before - map.len()
        }
        EtsStorage::Bag(map_lock) | EtsStorage::DuplicateBag(map_lock) => {
            let mut map = map_lock.write().unwrap();
            let mut deleted = 0usize;
            if guard.is_all() {
                for vec in map.values() {
                    deleted += vec.len();
                }
                map.clear();
                table.pool.lock().unwrap().clear();
            } else {
                map.retain(|_key, vec| {
                    let before = vec.len();
                    vec.retain(|stored| !should_delete(stored));
                    deleted += before - vec.len();
                    !vec.is_empty()
                });
            }
            deleted
        }
    };

    if deleted > 0 {
        table.pool.lock().unwrap().reclaim_dead_regions();
    }
    Ok(deleted as i64)
}

/// Check if a key exists in the table.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn ets_member(table_ref: ErlTerm, key: ErlTerm) -> bool {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return false,
    };
    let term_key = TermKey::from_term(key);
    match &table.storage {
        EtsStorage::Set(map_lock) => map_lock.read().unwrap().contains_key(&term_key),
        EtsStorage::OrderedSet(map_lock) => {
            let ord_key = OrdTermKey { term: key, eq_key: term_key };
            map_lock.read().unwrap().contains_key(&ord_key)
        }
        EtsStorage::Bag(map_lock) | EtsStorage::DuplicateBag(map_lock) => {
            map_lock.read().unwrap().contains_key(&term_key)
        }
    }
}

/// Counter results own any big integers; no pointer into a table escapes its lock.
pub enum CounterResult {
    Single(ErlTerm),
    Multi(Vec<i64>),
    BigSingle(num_bigint::BigInt),
    BigMulti(Vec<num_bigint::BigInt>),
}

/// Atomically update integer counters, retaining an in-place small-integer path.
/// # Safety
/// Input terms and reachable objects must remain live, immutable and unmoved
/// throughout this call.
pub unsafe fn ets_update_counter(
    table_ref: ErlTerm,
    key: ErlTerm,
    update_op: ErlTerm,
) -> Result<CounterResult, EtsError> {
    let table = registry()
        .get_table(table_ref)
        .ok_or(EtsError::InvalidTable)?;
    if matches!(table.table_type, TableType::Bag | TableType::DuplicateBag) {
        return Err(EtsError::InvalidCounter);
    }
    let term_key = TermKey::from_term(key);
    let small = parse_counter_ops(update_op, table.keypos, |t| t.as_small_int());
    let big = if small.is_none() {
        Some(
            parse_counter_ops(update_op, table.keypos, |term| crate::bigint::term_to_bigint(term))
                .ok_or(EtsError::InvalidCounter)?,
        )
    } else {
        None
    };
    let is_list = update_op.is_cons() || update_op.is_nil();
    let mut replaced = false;
    let mut update = |stored: &mut StoredTuple| {
        if let Some(ops) = &small {
            if let Some(result) = apply_small_counter_ops(stored, ops, is_list) {
                return Ok(result);
            }
        }
        let converted;
        let ops = if let Some(ops) = &big {
            ops
        } else {
            converted = small
                .as_ref()
                .unwrap()
                .iter()
                .map(|op| CounterOp {
                    pos: op.pos,
                    incr: num_bigint::BigInt::from(op.incr),
                    threshold: op.threshold.map(|(t, v)| (t.into(), v.into())),
                })
                .collect::<Vec<_>>();
            &converted
        };
        let result = apply_big_counter_ops(stored, &table.pool, ops, is_list)?;
        replaced = true;
        Ok(result)
    };
    let result = match &table.storage {
        EtsStorage::Set(lock) => {
            let mut map = lock.write().unwrap();
            update(map.get_mut(&term_key).ok_or(EtsError::InvalidCounter)?)
        }
        EtsStorage::OrderedSet(lock) => {
            let mut map = lock.write().unwrap();
            let key = OrdTermKey {
                term: key,
                eq_key: term_key,
            };
            match map.entry(key) {
                Entry::Vacant(_) => Err(EtsError::InvalidCounter),
                Entry::Occupied(mut entry) => {
                    // Keep the common path in-place even for a boxed key.
                    if let Some(ops) = &small {
                        if let Some(result) = apply_small_counter_ops(entry.get_mut(), ops, is_list)
                        {
                            return Ok(result);
                        }
                    }
                    // A replacement heap also owns the comparison key. Remove
                    // both halves before releasing the old owner, then rekey.
                    let (old_key, mut stored) = entry.remove_entry();
                    let result = update(&mut stored);
                    map.insert(
                        OrdTermKey::stored(&stored, table.keypos, old_key.eq_key),
                        stored,
                    );
                    result
                }
            }
        }
        _ => Err(EtsError::InvalidCounter),
    };
    if replaced {
        table.pool.lock().unwrap().reclaim_dead_regions();
    }
    result
}

struct CounterOp<N> {
    pos: usize,
    incr: N,
    threshold: Option<(N, N)>,
}

// Parsing precedes locking and never changes table contents.
unsafe fn parse_counter_ops<N>(
    term: ErlTerm,
    keypos: usize,
    integer: impl Fn(ErlTerm) -> Option<N>,
) -> Option<Vec<CounterOp<N>>> {
    let parse = |term: ErlTerm| -> Option<CounterOp<N>> {
        if let Some(incr) = integer(term) {
            return Some(CounterOp {
                pos: keypos.checked_add(1)?,
                incr,
                threshold: None,
            });
        }
        let ptr = term.as_boxed_ptr()?;
        let header = BoxedHeader(*ptr);
        if header.subtype() != BOXED_TUPLE || !matches!(header.arity(), 2 | 4) {
            return None;
        }
        let pos = usize::try_from(ErlTerm(*ptr.add(1)).as_small_int()?).ok()?;
        if pos == 0 || pos == keypos {
            return None;
        }
        let incr = integer(ErlTerm(*ptr.add(2)))?;
        let threshold = if header.arity() == 4 {
            Some((
                integer(ErlTerm(*ptr.add(3)))?,
                integer(ErlTerm(*ptr.add(4)))?,
            ))
        } else {
            None
        };
        Some(CounterOp {
            pos,
            incr,
            threshold,
        })
    };
    if term.is_cons() || term.is_nil() {
        let mut cursor = term;
        let mut ops = Vec::new();
        while let Some(ptr) = cursor.as_cons_ptr() {
            ops.push(parse((*ptr).head)?);
            cursor = (*ptr).tail;
        }
        if !cursor.is_nil() {
            return None;
        }
        Some(ops)
    } else {
        Some(vec![parse(term)?])
    }
}

#[inline]
fn small_counter_value(old: i64, op: &CounterOp<i64>) -> i64 {
    // Two signed 61-bit values always add without overflowing i64.
    let value = old + op.incr;
    match op.threshold {
        Some((threshold, reset))
            if (op.incr >= 0 && value > threshold) || (op.incr < 0 && value < threshold) =>
        {
            reset
        }
        _ => value,
    }
}

/// None requests the staged path, after restoring every modified slot.
fn apply_small_counter_ops(
    stored: &mut StoredTuple,
    ops: &[CounterOp<i64>],
    is_list: bool,
) -> Option<CounterResult> {
    if !is_list {
        let op = &ops[0];
        let slot = stored.elements.get_mut(op.pos - 1)?;
        let value = ErlTerm::try_small_int(small_counter_value(slot.as_small_int()?, op))?;
        *slot = value;
        return Some(CounterResult::Single(value));
    }
    // Reuse the result allocation as a rollback log. Repeated positions are
    // restored in reverse order; overflow and invalid later targets are atomic.
    let mut values = Vec::with_capacity(ops.len());
    for op in ops {
        let next = stored
            .elements
            .get(op.pos - 1)
            .and_then(|term| term.as_small_int())
            .and_then(|old| {
                ErlTerm::try_small_int(small_counter_value(old, op)).map(|new| (old, new))
            });
        let Some((old, new)) = next else {
            for (op, old) in ops.iter().zip(&values).rev() {
                stored.elements[op.pos - 1] = ErlTerm::small_int(*old);
            }
            return None;
        };
        values.push(old);
        stored.elements[op.pos - 1] = new;
    }
    for (old, op) in values.iter_mut().zip(ops) {
        *old = small_counter_value(*old, op);
    }
    Some(CounterResult::Multi(values))
}

/// Stage arbitrary precision updates before publishing a new immutable owner.
/// Source terms are protected by the table write lock and caller's contract.
unsafe fn apply_big_counter_ops(
    stored: &mut StoredTuple,
    pool: &Mutex<EtsRegionPool>,
    ops: &[CounterOp<num_bigint::BigInt>],
    is_list: bool,
) -> Result<CounterResult, EtsError> {
    let mut heap = ProcessHeap::new();
    let mut elements = stored.elements.clone();
    let mut values = Vec::with_capacity(ops.len());
    for op in ops {
        let slot = elements
            .get_mut(op.pos - 1)
            .ok_or(EtsError::InvalidCounter)?;
        let old = crate::bigint::term_to_bigint(*slot).ok_or(EtsError::InvalidCounter)?;
        let mut value = old + &op.incr;
        if let Some((threshold, reset)) = &op.threshold {
            let negative = op.incr.sign() == num_bigint::Sign::Minus;
            if (!negative && value > *threshold) || (negative && value < *threshold) {
                value = reset.clone();
            }
        }
        *slot = crate::bigint::bigint_to_term(&mut heap, value.clone());
        values.push(value);
    }
    let compact = alloc_compact_for_elements(pool, &elements);
    *stored = deep_copy_tuple_to_stored(&elements, compact);
    Ok(if is_list {
        CounterResult::BigMulti(values)
    } else {
        CounterResult::BigSingle(values.pop().unwrap())
    })
}

/// Insert a tuple only if the key does not already exist.
/// Returns true if the insert happened, false if the key was already present.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn ets_insert_new(table_ref: ErlTerm, object: ErlTerm) -> bool {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return false,
    };

    // Helper: check if key exists in storage
    let key_exists = |term_key: &TermKey, key_term: ErlTerm| -> bool {
        match &table.storage {
            EtsStorage::Set(map_lock) => map_lock.read().unwrap().contains_key(term_key),
            EtsStorage::OrderedSet(map_lock) => {
                let ord_key = OrdTermKey { term: key_term, eq_key: term_key.clone() };
                map_lock.read().unwrap().contains_key(&ord_key)
            }
            EtsStorage::Bag(map_lock) | EtsStorage::DuplicateBag(map_lock) => {
                map_lock.read().unwrap().contains_key(term_key)
            }
        }
    };

    if object.is_cons() {
        let mut tuples = Vec::new();
        let mut cursor = object;
        while cursor.is_cons() {
            if let Some(ptr) = cursor.as_cons_ptr() {
                unsafe {
                    tuples.push((*ptr).head);
                    cursor = (*ptr).tail;
                }
            } else {
                break;
            }
        }

        // Check all keys exist
        for &tuple_term in &tuples {
            if let Some(key) = extract_key(&table, tuple_term) {
                let key_term = extract_key_term(&table, tuple_term).unwrap();
                if key_exists(&key, key_term) {
                    return false;
                }
            } else {
                return false;
            }
        }
        for &tuple_term in &tuples {
            let _ = store_tuple(&table, tuple_term);
        }
        true
    } else {
        if let Some(key) = extract_key(&table, object) {
            let key_term = extract_key_term(&table, object).unwrap();
            if key_exists(&key, key_term) {
                return false;
            }
            let _ = store_tuple(&table, object);
            true
        } else {
            false
        }
    }
}

/// Extract the key ErlTerm from a tuple term.
fn extract_key_term(table: &EtsTable, tuple_term: ErlTerm) -> Option<ErlTerm> {
    let ptr = tuple_term.as_boxed_ptr()?;
    let header = BoxedHeader(unsafe { *ptr });
    if header.subtype() != BOXED_TUPLE {
        return None;
    }
    let arity = header.arity() as usize;
    if arity < table.keypos {
        return None;
    }
    Some(ErlTerm(unsafe { *ptr.add(table.keypos) }))
}

/// Extract the TermKey from a tuple term without storing it.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn extract_key(table: &EtsTable, tuple_term: ErlTerm) -> Option<TermKey> {
    let ptr = tuple_term.as_boxed_ptr()?;
    let header = BoxedHeader(unsafe { *ptr });
    if header.subtype() != BOXED_TUPLE {
        return None;
    }
    let arity = header.arity() as usize;
    if arity < table.keypos {
        return None;
    }
    let key_term = ErlTerm(unsafe { *ptr.add(table.keypos) });
    Some(TermKey::from_term(key_term))
}

/// Copy a StoredTuple into a process heap tuple.
fn stored_to_heap_tuple(stored: &StoredTuple, heap: &mut ProcessHeap) -> ErlTerm {
    let elems: Vec<ErlTerm> = stored.elements.iter()
        // SAFETY: the stored value keeps its compact backing heap alive during this read.
        .map(|&elem| unsafe { copy_to_process_heap(elem, heap) })
        .collect();
    heap.alloc_tuple(&elems)
}

/// Return all entries as a list of tuples, copied into the process heap.
pub fn ets_tab2list(table_ref: ErlTerm, heap: &mut ProcessHeap) -> ErlTerm {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return ErlTerm::nil(),
    };
    let tuples: Vec<ErlTerm> = match &table.storage {
        EtsStorage::Set(map_lock) => {
            let map = map_lock.read().unwrap();
            map.values().map(|stored| stored_to_heap_tuple(stored, heap)).collect()
        }
        EtsStorage::OrderedSet(map_lock) => {
            let map = map_lock.read().unwrap();
            map.values().map(|stored| stored_to_heap_tuple(stored, heap)).collect()
        }
        EtsStorage::Bag(map_lock) | EtsStorage::DuplicateBag(map_lock) => {
            let map = map_lock.read().unwrap();
            let mut tuples = Vec::new();
            for vec in map.values() {
                for stored in vec {
                    tuples.push(stored_to_heap_tuple(stored, heap));
                }
            }
            tuples
        }
    };
    heap.alloc_list_from_slice(&tuples)
}

/// Extract and copy the key from a stored tuple.
fn stored_key_to_heap(stored: &StoredTuple, keypos: usize, heap: &mut ProcessHeap) -> ErlTerm {
    let key_idx = keypos - 1;
    if key_idx < stored.elements.len() {
        // SAFETY: the stored value keeps its compact backing heap alive during this read.
        unsafe { copy_to_process_heap(stored.elements[key_idx], heap) }
    } else {
        end_of_table_atom()
    }
}

/// Return the first key from the table, or '$end_of_table' if empty.
/// For ordered_set, returns the smallest key. For other types, arbitrary.
pub fn ets_first(table_ref: ErlTerm, heap: &mut ProcessHeap) -> ErlTerm {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return end_of_table_atom(),
    };
    match &table.storage {
        EtsStorage::Set(map_lock) => {
            let map = map_lock.read().unwrap();
            match map.values().next() {
                Some(stored) => stored_key_to_heap(stored, table.keypos, heap),
                None => end_of_table_atom(),
            }
        }
        EtsStorage::OrderedSet(map_lock) => {
            let map = map_lock.read().unwrap();
            match map.iter().next() {
                Some((_k, stored)) => stored_key_to_heap(stored, table.keypos, heap),
                None => end_of_table_atom(),
            }
        }
        EtsStorage::Bag(map_lock) | EtsStorage::DuplicateBag(map_lock) => {
            let map = map_lock.read().unwrap();
            match map.values().next() {
                Some(vec) => match vec.first() {
                    Some(stored) => stored_key_to_heap(stored, table.keypos, heap),
                    None => end_of_table_atom(),
                },
                None => end_of_table_atom(),
            }
        }
    }
}

/// Return the next key after `prev_key`, or '$end_of_table' if no more.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn ets_next(table_ref: ErlTerm, prev_key: ErlTerm, heap: &mut ProcessHeap) -> ErlTerm {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return end_of_table_atom(),
    };
    let search_key = TermKey::from_term(prev_key);

    match &table.storage {
        EtsStorage::Set(map_lock) => {
            let map = map_lock.read().unwrap();
            let mut found = false;
            for (key, stored) in map.iter() {
                if found {
                    return stored_key_to_heap(stored, table.keypos, heap);
                }
                if *key == search_key {
                    found = true;
                }
            }
            end_of_table_atom()
        }
        EtsStorage::OrderedSet(map_lock) => {
            let ord_key = OrdTermKey { term: prev_key, eq_key: search_key };
            let map = map_lock.read().unwrap();
            // Use range to find the next key after ord_key
            use std::ops::Bound;
            let mut range = map.range((Bound::Excluded(&ord_key), Bound::Unbounded));
            match range.next() {
                Some((_k, stored)) => stored_key_to_heap(stored, table.keypos, heap),
                None => end_of_table_atom(),
            }
        }
        EtsStorage::Bag(map_lock) | EtsStorage::DuplicateBag(map_lock) => {
            let map = map_lock.read().unwrap();
            let mut found = false;
            for (key, vec) in map.iter() {
                if found {
                    if let Some(stored) = vec.first() {
                        return stored_key_to_heap(stored, table.keypos, heap);
                    }
                }
                if *key == search_key {
                    found = true;
                }
            }
            end_of_table_atom()
        }
    }
}

/// Return the last key from the table, or '$end_of_table' if empty.
/// For ordered_set, returns the largest key.
pub fn ets_last(table_ref: ErlTerm, heap: &mut ProcessHeap) -> ErlTerm {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return end_of_table_atom(),
    };
    match &table.storage {
        EtsStorage::OrderedSet(map_lock) => {
            let map = map_lock.read().unwrap();
            match map.iter().next_back() {
                Some((_k, stored)) => stored_key_to_heap(stored, table.keypos, heap),
                None => end_of_table_atom(),
            }
        }
        // For non-ordered types, last is equivalent to first (undefined order)
        _ => ets_first(table_ref, heap),
    }
}

/// Return the previous key before `next_key`, or '$end_of_table' if no more.
/// For ordered_set, returns keys in reverse sorted order.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub unsafe fn ets_prev(table_ref: ErlTerm, next_key: ErlTerm, heap: &mut ProcessHeap) -> ErlTerm {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return end_of_table_atom(),
    };
    match &table.storage {
        EtsStorage::OrderedSet(map_lock) => {
            let search_key = TermKey::from_term(next_key);
            let ord_key = OrdTermKey { term: next_key, eq_key: search_key };
            let map = map_lock.read().unwrap();
            use std::ops::Bound;
            let mut range = map.range((Bound::Unbounded, Bound::Excluded(&ord_key)));
            match range.next_back() {
                Some((_k, stored)) => stored_key_to_heap(stored, table.keypos, heap),
                None => end_of_table_atom(),
            }
        }
        // For non-ordered types, prev behaves like next (undefined order)
        _ => ets_next(table_ref, next_key, heap),
    }
}

/// Return the atom '$end_of_table'.
fn end_of_table_atom() -> ErlTerm {
    let atom_table = crate::atom_table::global_atom_table();
    let id = atom_table.intern("$end_of_table");
    ErlTerm::atom(id)
}

/// Return table information.
/// ets:info/1 returns a proplist, ets:info/2 returns a single value.
pub fn ets_info(table_ref: ErlTerm, item: Option<&str>, heap: &mut ProcessHeap) -> ErlTerm {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return ErlTerm::atom(ATOM_UNDEFINED),
    };

    let storage_len = || -> usize {
        match &table.storage {
            EtsStorage::Set(m) => m.read().unwrap().len(),
            EtsStorage::OrderedSet(m) => m.read().unwrap().len(),
            EtsStorage::Bag(m) | EtsStorage::DuplicateBag(m) => {
                m.read().unwrap().values().map(|v| v.len()).sum()
            }
        }
    };

    let type_atom_name = match table.table_type {
        TableType::Set => "set",
        TableType::OrderedSet => "ordered_set",
        TableType::Bag => "bag",
        TableType::DuplicateBag => "duplicate_bag",
    };

    match item {
        Some("size") => {
            ErlTerm::small_int(storage_len() as i64)
        }
        Some("name") => {
            ErlTerm::atom(table.name)
        }
        Some("type") => {
            let atom_table = crate::atom_table::global_atom_table();
            ErlTerm::atom(atom_table.intern(type_atom_name))
        }
        Some("keypos") => {
            ErlTerm::small_int(table.keypos as i64)
        }
        Some("owner") => {
            let pid = table.owner.load(Ordering::Relaxed);
            ErlTerm::pid(pid)
        }
        Some("memory") => {
            let pool = table.pool.lock().unwrap();
            let region_count = pool.region_count();
            ErlTerm::small_int((region_count * 256 * 1024) as i64)
        }
        Some(_) => ErlTerm::atom(ATOM_UNDEFINED),
        None => {
            let atom_table = crate::atom_table::global_atom_table();
            let size = storage_len();
            let owner_pid = table.owner.load(Ordering::Relaxed);
            let pairs = [
                (atom_table.intern("size"), ErlTerm::small_int(size as i64)),
                (atom_table.intern("name"), ErlTerm::atom(table.name)),
                (atom_table.intern("type"), ErlTerm::atom(atom_table.intern(type_atom_name))),
                (atom_table.intern("keypos"), ErlTerm::small_int(table.keypos as i64)),
                (atom_table.intern("owner"), ErlTerm::pid(owner_pid)),
            ];

            let mut tuples = Vec::with_capacity(pairs.len());
            for (key, val) in &pairs {
                let elems = [ErlTerm::atom(*key), *val];
                tuples.push(heap.alloc_tuple(&elems));
            }
            heap.alloc_list_from_slice(&tuples)
        }
    }
}

/// Fold over all entries in the table.
/// Calls `fun(Element, Acc)` for each stored tuple.
/// This is implemented at the FFI level since it needs to call an Erlang function.
/// Here we provide the iterator: returns all stored tuples copied to process heap.
pub fn ets_tab2list_raw(table_ref: ErlTerm, heap: &mut ProcessHeap) -> Vec<ErlTerm> {
    let table = match registry().get_table(table_ref) {
        Some(t) => t,
        None => return Vec::new(),
    };
    match &table.storage {
        EtsStorage::Set(map_lock) => {
            let map = map_lock.read().unwrap();
            map.values().map(|stored| stored_to_heap_tuple(stored, heap)).collect()
        }
        EtsStorage::OrderedSet(map_lock) => {
            let map = map_lock.read().unwrap();
            map.values().map(|stored| stored_to_heap_tuple(stored, heap)).collect()
        }
        EtsStorage::Bag(map_lock) | EtsStorage::DuplicateBag(map_lock) => {
            let map = map_lock.read().unwrap();
            let mut tuples = Vec::new();
            for vec in map.values() {
                for stored in vec {
                    tuples.push(stored_to_heap_tuple(stored, heap));
                }
            }
            tuples
        }
    }
}

/// Return the number of entries in the table (for ets:info(Tab, size)).
pub fn ets_table_size(table_ref: ErlTerm) -> Option<usize> {
    let table = registry().get_table(table_ref)?;
    let size = match &table.storage {
        EtsStorage::Set(m) => m.read().unwrap().len(),
        EtsStorage::OrderedSet(m) => m.read().unwrap().len(),
        EtsStorage::Bag(m) | EtsStorage::DuplicateBag(m) => {
            m.read().unwrap().values().map(|v| v.len()).sum()
        }
    };
    Some(size)
}

// ---------------------------------------------------------------------------
// Match spec parsing (simplified)
// ---------------------------------------------------------------------------

/// Recoverable ETS argument failures, translated to `badarg` at the FFI boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtsError {
    InvalidTable,
    InvalidCounter,
    InvalidMatchSpec,
    UnsupportedMatchSpec,
}

enum DeleteSpec {
    All,
    None,
    Tuple {
        arity: usize,
        guard: Option<ComparisonGuard>,
    },
}
struct ComparisonGuard {
    position: usize,
    ordering: std::cmp::Ordering,
    inclusive: bool,
    threshold: i64,
}
impl DeleteSpec {
    fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }
    #[inline]
    fn matches(&self, stored: &StoredTuple) -> bool {
        match self {
            Self::All => true,
            Self::None => false,
            Self::Tuple { arity, guard } => {
                stored.elements.len() == *arity
                    && guard.as_ref().is_none_or(|guard| {
                        let term = stored.elements[guard.position];
                        let cmp = term.as_small_int().map_or_else(
                            // SAFETY: stored owns all of its immutable terms, and the
                            // table read/write lock protects it during comparison.
                            || unsafe {
                                compare_terms(term, ErlTerm::small_int(guard.threshold)).cmp(&0)
                            },
                            |value| value.cmp(&guard.threshold),
                        );
                        cmp == guard.ordering
                            || (guard.inclusive && cmp == std::cmp::Ordering::Equal)
                    })
            }
        }
    }
}

// These readers are only called under the operation's term lifetime contract.
// Fixed-size tuples and singleton lists are decoded without temporary allocations.
unsafe fn triple_fields(term: ErlTerm) -> Result<[ErlTerm; 3], EtsError> {
    let ptr = term.as_boxed_ptr().ok_or(EtsError::InvalidMatchSpec)?;
    let header = BoxedHeader(*ptr);
    if header.subtype() != BOXED_TUPLE || header.arity() != 3 {
        return Err(EtsError::InvalidMatchSpec);
    }
    Ok([
        ErlTerm(*ptr.add(1)),
        ErlTerm(*ptr.add(2)),
        ErlTerm(*ptr.add(3)),
    ])
}

unsafe fn single_list(term: ErlTerm) -> Result<Option<ErlTerm>, EtsError> {
    if term.is_nil() {
        return Ok(None);
    }
    let ptr = term.as_cons_ptr().ok_or(EtsError::InvalidMatchSpec)?;
    if !(*ptr).tail.is_nil() {
        return Err(EtsError::UnsupportedMatchSpec);
    }
    Ok(Some((*ptr).head))
}

/// Compile one wildcard/variable clause and an optional comparison guard.
/// Unsupported constructs are rejected before any table mutation.
unsafe fn parse_simple_match_spec(spec: ErlTerm) -> Result<DeleteSpec, EtsError> {
    use std::cmp::Ordering::{Greater, Less};
    let Some(clause) = single_list(spec)? else {
        return Ok(DeleteSpec::None);
    };
    let fields = triple_fields(clause)?;
    let wildcard = ErlTerm::atom(crate::atom_table::global_atom_table().intern("_"));
    let mut bindings = Vec::new();
    let arity = if fields[0] == wildcard {
        None
    } else {
        let ptr = fields[0].as_boxed_ptr().ok_or(EtsError::InvalidMatchSpec)?;
        let header = BoxedHeader(*ptr);
        if header.subtype() != BOXED_TUPLE {
            return Err(EtsError::InvalidMatchSpec);
        }
        let arity = header.arity() as usize;
        for pos in 0..arity {
            let term = ErlTerm(*ptr.add(pos + 1));
            if term == wildcard {
                continue;
            }
            let var = parse_dollar_var(term).ok_or(EtsError::UnsupportedMatchSpec)?;
            bindings.push((var, pos));
        }
        bindings.sort_unstable_by_key(|&(var, _)| var);
        if bindings.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(EtsError::UnsupportedMatchSpec);
        }
        Some(arity)
    };
    let guard = if let Some(guard) = single_list(fields[1])? {
        let parts = triple_fields(guard)?;
        let var = parse_dollar_var(parts[1]).ok_or(EtsError::InvalidMatchSpec)?;
        let binding = bindings.binary_search_by_key(&var, |&(var, _)| var)
            .map_err(|_| EtsError::InvalidMatchSpec)?;
        let position = bindings[binding].1;
        let threshold = parts[2]
            .as_small_int()
            .ok_or(EtsError::UnsupportedMatchSpec)?;
        let atom = parts[0].as_atom_id().ok_or(EtsError::InvalidMatchSpec)?;
        let name = crate::atom_table::global_atom_table()
            .resolve(atom)
            .ok_or(EtsError::InvalidMatchSpec)?;
        let (ordering, inclusive) = match name.as_str() {
            "<" => (Less, false),
            ">" => (Greater, false),
            "=<" => (Less, true),
            ">=" => (Greater, true),
            _ => return Err(EtsError::UnsupportedMatchSpec),
        };
        Some(ComparisonGuard {
            position,
            ordering,
            inclusive,
            threshold,
        })
    } else {
        None
    };
    let delete = match single_list(fields[2])? {
        Some(term) if term == ErlTerm::atom(crate::atom_table::ATOM_TRUE) => true,
        Some(term) if term == ErlTerm::atom(crate::atom_table::ATOM_FALSE) => false,
        _ => return Err(EtsError::UnsupportedMatchSpec),
    };
    Ok(if !delete {
        DeleteSpec::None
    } else if let Some(arity) = arity {
        DeleteSpec::Tuple { arity, guard }
    } else {
        DeleteSpec::All
    })
}

/// Parse '$1', '$2' etc. atoms to extract the variable position.
fn parse_dollar_var(term: ErlTerm) -> Option<usize> {
    let atom_id = term.as_atom_id()?;
    let atom_table = crate::atom_table::global_atom_table();
    let name = atom_table.resolve(atom_id)?;
    if let Some(index) = name.strip_prefix('$') {
        index.parse::<usize>().ok()
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {

    #[test]
    fn numeric_order_key_equality_matches_order() {
        let mut heap = crate::heap::ProcessHeap::new();
        let a = ErlTerm::small_int(1);
        let b = heap.alloc_float(1.0);
        // SAFETY: the keys borrow this live heap throughout their comparisons.
        unsafe {
            let a = OrdTermKey { term: a, eq_key: TermKey::from_term(a) };
            let b = OrdTermKey { term: b, eq_key: TermKey::from_term(b) };
            assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
            assert!(a == b, "ordered key equality must agree with cmp");
        }
    }
    use super::*;
    use crate::atom_table::global_atom_table;
    use crate::heap::MapPairs;

    /// Helper: create a unique ETS table for each test to avoid cross-test
    /// interference (the global registry is shared within a test binary).
    fn make_test_table(name_str: &str) -> (TableId, u32) {
        let atom_table = global_atom_table();
        let name_atom = atom_table.intern(name_str);
        let id = ets_new(name_atom, &[], crate::process::ProcessId::new(0)).expect("ets_new should succeed");
        (id, name_atom)
    }

    fn insert_tuple(table_id: TableId, key: i64, val: i64) {
        let mut heap = ProcessHeap::new();
        let k = ErlTerm::small_int(key);
        let v = ErlTerm::small_int(val);
        let tup = heap.alloc_tuple(&[k, v]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(ErlTerm::small_int(table_id as i64), tup) }.expect("ets_insert should succeed");
    }

    // ---------------------------------------------------------------
    // ets:delete/1 — table deletion
    // ---------------------------------------------------------------

    #[test]
    fn compact_heap_rejects_capacity_overflow_without_mutation() {
        let mut pool = EtsRegionPool::new();
        let mut allocation = pool.alloc(2);
        let ptr = allocation.as_mut_ptr();
        let mut heap = CompactHeap::from_pool(Some(allocation));
        assert_eq!(heap.alloc_words(1), ptr);
        for requested in [2, usize::MAX] {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                heap.alloc_words(requested);
            }));
            assert!(result.is_err(), "oversized allocation must panic in release too");
            assert_eq!(heap.words_len, 1, "failed allocation must not change the cursor");
        }
        assert_eq!(heap.alloc_words(1), unsafe { ptr.add(1) });
        assert_eq!(heap.alloc_words(0), unsafe { ptr.add(2) });
        assert_eq!(heap.words_len, 2);
    }

    #[test]
    fn compact_heap_empty_allocation() {
        // An empty heap requires neither mapping nor backing words.
        let mut heap = CompactHeap::from_pool(None);
        assert!(heap.alloc_words(0).is_null());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            heap.alloc_words(1);
        }));
        assert!(result.is_err());
        assert_eq!(heap.words_len, 0);
    }

    #[test]
    fn counter_batch_failure_leaves_tuple_unchanged() {
        for kind in ["set", "ordered_set"] {
            for invalid in [0, 3, 4] {
                let name = global_atom_table().intern("counter_batch_failure");
                let id = ets_new(name, &[(kind.into(), ErlTerm::nil())], crate::process::ProcessId::new(0)).unwrap();
                let table = ErlTerm::small_int(id as i64);
                let mut heap = ProcessHeap::new();
                let key = ErlTerm::small_int(7);
                let tuple = heap.alloc_tuple(&[key, ErlTerm::small_int(10), ErlTerm::nil()]);
                let first = heap.alloc_tuple(&[ErlTerm::small_int(2), ErlTerm::small_int(5)]);
                let last = heap.alloc_tuple(&[ErlTerm::small_int(invalid), ErlTerm::small_int(1)]);
                let ops = heap.alloc_cons(last, ErlTerm::nil());
                let ops = heap.alloc_cons(first, ops);
                // SAFETY: the source heap retains all arguments throughout the calls.
                unsafe {
                    ets_insert(table, tuple).unwrap();
                    assert!(ets_update_counter(table, key, ops).is_err());
                    let result = ets_lookup(table, key, &mut heap);
                    let tuple = (*result.as_cons_ptr().unwrap()).head.as_boxed_ptr().unwrap();
                    assert_eq!(ErlTerm(*tuple.add(2)).as_small_int(), Some(10));
                }
                assert!(ets_delete_table(table));
            }
        }
    }

    #[test]
    fn invalid_selection_never_deletes_and_empty_selection_is_explicit() {
        let (id, _) = make_test_table("invalid_selection_regression");
        let table = ErlTerm::small_int(id as i64);
        insert_tuple(id, 1, 2);
        let mut heap = ProcessHeap::new();
        let wildcard = ErlTerm::atom(global_atom_table().intern("_"));
        let body = heap.alloc_cons(ErlTerm::atom(crate::atom_table::ATOM_TRUE), ErlTerm::nil());
        let clause = heap.alloc_tuple(&[wildcard, ErlTerm::nil(), body]);
        let all = heap.alloc_cons(clause, ErlTerm::nil());
        let improper = heap.alloc_cons(clause, wildcard);
        let malformed = heap.alloc_cons(wildcard, ErlTerm::nil());
        let later_error = heap.alloc_cons(clause, malformed);
        // SAFETY: all match specifications remain owned by this heap.
        unsafe {
            for spec in [wildcard, improper, malformed, later_error] {
                assert!(ets_select_delete(table, spec).is_err());
                assert_eq!(ets_table_size(table), Some(1));
            }
            assert_eq!(ets_select_delete(table, ErlTerm::nil()), Ok(0));
            assert_eq!(ets_select_delete(table, all), Ok(1));
            assert!(ets_delete_table(table));
            assert_eq!(
                ets_select_delete(table, ErlTerm::nil()),
                Err(EtsError::InvalidTable)
            );
        }
    }

    #[test]
    fn counter_promotion_preserves_rollback_snapshots_and_ordered_keys() {
        use crate::term::SMALL_INT_MAX;
        for kind in ["set", "ordered_set"] {
            let name = global_atom_table().intern("counter_promotion_regression");
            let id = ets_new(name, &[(kind.into(), ErlTerm::nil())], crate::process::ProcessId::new(0)).unwrap();
            let table = ErlTerm::small_int(id as i64);
            let mut heap = ProcessHeap::new();
            let key = heap.alloc_tuple(&[ErlTerm::small_int(7)]);
            let tuple = heap.alloc_tuple(&[key, ErlTerm::small_int(SMALL_INT_MAX), ErlTerm::nil()]);
            let good = heap.alloc_tuple(&[ErlTerm::small_int(2), ErlTerm::small_int(1)]);
            let bad = heap.alloc_tuple(&[ErlTerm::small_int(3), ErlTerm::small_int(1)]);
            let ops = heap.alloc_list_from_slice(&[good, bad]);
            // SAFETY: the input heap and returned snapshot stay alive throughout.
            unsafe {
                ets_insert(table, tuple).unwrap();
                assert!(ets_update_counter(table, key, ops).is_err());
                let snapshot = ets_lookup(table, key, &mut heap);
                let CounterResult::BigSingle(value) = ets_update_counter(table, key, good).unwrap()
                else {
                    panic!("expected promotion");
                };
                assert_eq!(value, num_bigint::BigInt::from(SMALL_INT_MAX) + 1);
                let current = ets_lookup(table, key, &mut heap);
                let current = (*current.as_cons_ptr().unwrap())
                    .head
                    .as_boxed_ptr()
                    .unwrap();
                assert_eq!(
                    crate::bigint::term_to_bigint(ErlTerm(*current.add(2))),
                    Some(value)
                );
                assert!(ets_delete_table(table));
                let snapshot = (*snapshot.as_cons_ptr().unwrap())
                    .head
                    .as_boxed_ptr()
                    .unwrap();
                assert_eq!(
                    ErlTerm(*snapshot.add(2)).as_small_int(),
                    Some(SMALL_INT_MAX)
                );
            }
        }
    }

    #[test]
    fn ordered_replacement_keeps_comparison_key_alive() {
        let name = global_atom_table().intern("ordered_key_owner_regression");
        let id = ets_new(name, &[("ordered_set".into(), ErlTerm::nil())], crate::process::ProcessId::new(0)).unwrap();
        let table_ref = ErlTerm::small_int(id as i64);
        let table = registry().get_table(table_ref).unwrap();
        let mut heap = ProcessHeap::new();
        let key = heap.alloc_tuple(&[ErlTerm::small_int(42)]);
        let tuple = heap.alloc_tuple(&[key, ErlTerm::small_int(1)]);
        // SAFETY: heap retains all input terms throughout both insertions.
        unsafe { ets_insert(table_ref, tuple) }.unwrap();
        let EtsStorage::OrderedSet(lock) = &table.storage else {
            unreachable!()
        };
        let (old_key, owner) = {
            let map = lock.read().unwrap();
            let (key, value) = map.first_key_value().unwrap();
            (key.term, Arc::downgrade(&value.inner))
        };
        let replacement = heap.alloc_tuple(&[key, ErlTerm::small_int(2)]);
        unsafe { ets_insert(table_ref, replacement) }.unwrap();
        {
            let map = lock.read().unwrap();
            let (key, _) = map.first_key_value().unwrap();
            // Inspect ownership before ever dereferencing a possibly stale key.
            assert!(
                key.term != old_key || owner.upgrade().is_some(),
                "a retained comparison key must retain its backing owner"
            );
        }
        assert!(ets_delete_table(table_ref));
        drop(table);
        assert!(owner.upgrade().is_none());
    }

    #[test]
    fn concurrent_element_updates_preserve_other_fields() {
        for kind in ["set", "ordered_set"] {
            let name = global_atom_table().intern(&format!("atomic_fields_{kind}"));
            let id = ets_new(name, &[(kind.into(), ErlTerm::nil())], crate::process::ProcessId::new(0)).unwrap();
            let table = ErlTerm::small_int(id as i64);
            let mut heap = ProcessHeap::new();
            let object = heap.alloc_tuple(&[
                ErlTerm::small_int(7),
                ErlTerm::small_int(0),
                ErlTerm::small_int(0),
                ErlTerm::small_int(0),
                ErlTerm::small_int(0),
            ]);
            unsafe { ets_insert(table, object) }.unwrap();
            let barrier = std::sync::Barrier::new(4);
            let failures = std::sync::atomic::AtomicUsize::new(0);
            std::thread::scope(|scope| {
                for field in 0..4 {
                    let barrier = &barrier;
                    let failures = &failures;
                    scope.spawn(move || {
                        for round in 1..=200 {
                            barrier.wait();
                            // All inputs are immediate, so even the old racy path
                            // cannot dereference freed backing memory in this test.
                            assert!(unsafe {
                                ets_update_element(
                                    table,
                                    ErlTerm::small_int(7),
                                    &[(field + 2, ErlTerm::small_int(round))],
                                )
                            });
                            barrier.wait();
                            if field == 0 {
                                let mut receiver = ProcessHeap::new();
                                let result = unsafe {
                                    ets_lookup(table, ErlTerm::small_int(7), &mut receiver)
                                };
                                unsafe {
                                    let tuple = (*result.as_cons_ptr().unwrap())
                                        .head
                                        .as_boxed_ptr()
                                        .unwrap();
                                    for pos in 2..=5 {
                                        if ErlTerm(*tuple.add(pos)).as_small_int() != Some(round) {
                                            failures.fetch_add(1, Ordering::Relaxed);
                                        }
                                    }
                                }
                            }
                        }
                    });
                }
            });
            assert!(ets_delete_table(table));
            assert_eq!(
                failures.load(Ordering::Relaxed),
                0,
                "{kind}: updates to distinct fields must not overwrite each other"
            );
        }
    }

    #[test]
    fn element_update_cannot_resurrect_deleted_entry() {
        for kind in ["set", "ordered_set"] {
            let name = global_atom_table().intern(&format!("atomic_delete_{kind}"));
            let id = ets_new(name, &[(kind.into(), ErlTerm::nil())], crate::process::ProcessId::new(0)).unwrap();
            let table = ErlTerm::small_int(id as i64);
            let key = ErlTerm::small_int(1);
            let mut source = ProcessHeap::new();
            let object = source.alloc_tuple(&[key, ErlTerm::small_int(0)]);
            let barrier = std::sync::Barrier::new(3);
            let mut resurrected = 0;
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    for _ in 0..400 {
                        barrier.wait();
                        unsafe {
                            ets_update_element(table, key, &[(2, ErlTerm::small_int(1))]);
                        }
                        barrier.wait();
                    }
                });
                scope.spawn(|| {
                    for _ in 0..400 {
                        barrier.wait();
                        unsafe {
                            ets_delete(table, key);
                        }
                        barrier.wait();
                    }
                });
                for _ in 0..400 {
                    unsafe { ets_insert(table, object) }.unwrap();
                    barrier.wait();
                    barrier.wait();
                    let mut receiver = ProcessHeap::new();
                    if !unsafe { ets_lookup(table, key, &mut receiver) }.is_nil() {
                        resurrected += 1;
                    }
                }
            });
            assert!(ets_delete_table(table));
            assert_eq!(
                resurrected, 0,
                "{kind}: update must not resurrect a deleted entry"
            );
        }
    }

    #[test]
    fn boxed_update_preserves_lookup_snapshots_after_clear_and_gc() {
        for kind in ["set", "ordered_set"] {
            let name = global_atom_table().intern(&format!("boxed_update_snapshot_{kind}"));
            let id = ets_new(name, &[(kind.into(), ErlTerm::nil())], crate::process::ProcessId::new(0)).unwrap();
            let table = ErlTerm::small_int(id as i64);
            let mut source = ProcessHeap::new();
            let key = source.alloc_tuple(&[ErlTerm::small_int(7)]);
            let old = source.alloc_tuple(&[ErlTerm::small_int(10)]);
            let new = source.alloc_tuple(&[ErlTerm::small_int(20)]);
            let object = source.alloc_tuple(&[key, old, ErlTerm::small_int(0)]);
            unsafe { ets_insert(table, object) }.unwrap();
            let mut receiver = ProcessHeap::new();
            let before = unsafe { ets_lookup(table, key, &mut receiver) };
            // Exercise both in-place immediate mutation and heap replacement.
            assert!(unsafe { ets_update_element(table, key, &[(3, ErlTerm::small_int(1))]) });
            assert!(unsafe { ets_update_element(table, key, &[(2, new)]) });
            let after = unsafe { ets_lookup(table, key, &mut receiver) };
            assert!(ets_delete_all_objects(table));
            assert!(ets_delete_table(table));
            drop(source);
            let mut roots = [before, after];
            unsafe { receiver.collect_full(&mut roots) };
            for (root, value, counter) in [(roots[0], 10, 0), (roots[1], 20, 1)] {
                // SAFETY: receiver retains each snapshot across table clear/GC.
                unsafe {
                    let tuple = (*root.as_cons_ptr().unwrap()).head.as_boxed_ptr().unwrap();
                    let nested = ErlTerm(*tuple.add(2)).as_boxed_ptr().unwrap();
                    assert_eq!(ErlTerm(*nested.add(1)).as_small_int(), Some(value));
                    assert_eq!(ErlTerm(*tuple.add(3)).as_small_int(), Some(counter));
                }
            }
        }
    }

    #[test]
    fn storage_auto_traits_follow_ownership() {
        fn send_sync<T: Send + Sync>() {}
        fn send<T: Send>() {}
        send_sync::<CompactHeap>();
        send_sync::<StoredTupleInner>();
        send_sync::<StoredTuple>();
        send_sync::<OrdTermKey>();
        send_sync::<EtsTable>();
        send_sync::<crate::binary::BinaryData>();
        send::<crate::process::ProcessContext>();
        send::<crate::process::Mailbox>();
        send::<crate::message::MessageFragment>();
    }

    #[test]
    fn deleted_table_lookup_survives_thread_transfer_and_full_gc() {
        let (id, _) = make_test_table("deleted_table_cross_thread_gc");
        let table = ErlTerm::small_int(id as i64);
        let mut source = ProcessHeap::new();
        let nested = source.alloc_tuple(&[ErlTerm::small_int(42), ErlTerm::small_int(99)]);
        let tuple = source.alloc_tuple(&[ErlTerm::small_int(7), nested]);
        // SAFETY: source owns both tuple allocations throughout insertion.
        unsafe { ets_insert(table, tuple) }.unwrap();
        let mut receiver = ProcessHeap::new();
        // SAFETY: the table owns the source and receiver retains lookup storage.
        let result = unsafe { ets_lookup(table, ErlTerm::small_int(7), &mut receiver) };
        assert!(receiver.borrowed_heap_count() > 0);
        assert!(ets_delete_table(table));
        drop(source);

        std::thread::spawn(move || {
            let mut roots = [result];
            unsafe { receiver.collect_full(&mut roots) };
            // SAFETY: receiver owns the relocated root and retains its backing data.
            unsafe {
                let tuple = (*roots[0].as_cons_ptr().unwrap()).head.as_boxed_ptr().unwrap();
                assert_eq!(ErlTerm(*tuple.add(1)).as_small_int(), Some(7));
                let nested = ErlTerm(*tuple.add(2)).as_boxed_ptr().unwrap();
                assert_eq!(ErlTerm(*nested.add(1)).as_small_int(), Some(42));
                assert_eq!(ErlTerm(*nested.add(2)).as_small_int(), Some(99));
            }
        }).join().unwrap();
    }

    #[test]
    fn delete_table_removes_from_registry() {
        let (id, _name) = make_test_table("test_delete_table_1");

        // Insert some data
        insert_tuple(id, 1, 100);
        insert_tuple(id, 2, 200);

        // Verify data is there
        let mut heap = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(ErlTerm::small_int(id as i64), ErlTerm::small_int(1), &mut heap) };
        assert!(result.is_cons(), "should find key before deletion");

        // Delete the table
        assert!(ets_delete_table(ErlTerm::small_int(id as i64)));

        // Lookup on deleted table should return nil (table not found)
        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result2 = unsafe { ets_lookup(ErlTerm::small_int(id as i64), ErlTerm::small_int(1), &mut heap2) };
        assert!(result2.is_nil(), "lookup on deleted table should return []");
    }

    #[test]
    fn delete_table_by_name() {
        // Only a named_table is reachable by its name, as on BEAM.
        let name_atom = global_atom_table().intern("test_delete_table_by_name");
        let opts = [("named_table".to_string(), ErlTerm::nil())];
        let id = ets_new(name_atom, &opts, crate::process::ProcessId::new(0)).expect("ets_new should succeed");

        insert_tuple(id, 42, 999);

        // Delete by atom name
        assert!(ets_delete_table(ErlTerm::atom(name_atom)));

        // Table should be gone
        let mut heap = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(ErlTerm::atom(name_atom), ErlTerm::small_int(42), &mut heap) };
        assert!(result.is_nil(), "lookup after delete-by-name should return []");
    }

    #[test]
    fn delete_nonexistent_table_returns_false() {
        assert!(!ets_delete_table(ErlTerm::small_int(999_999)));
    }

    #[test]
    fn insert_after_delete_table_fails() {
        let (id, _name) = make_test_table("test_insert_after_delete");
        assert!(ets_delete_table(ErlTerm::small_int(id as i64)));

        // Insert should return Err (table not found → badarg in FFI)
        let mut heap = ProcessHeap::new();
        let tup = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { ets_insert(ErlTerm::small_int(id as i64), tup) }.is_err());
    }

    // ---------------------------------------------------------------
    // ets:delete_all_objects — capacity release
    // ---------------------------------------------------------------

    #[test]
    fn delete_all_objects_releases_capacity() {
        let (id, _name) = make_test_table("test_delete_all_cap");
        let table_ref = ErlTerm::small_int(id as i64);

        // Insert many entries to grow the HashMap
        for i in 0..1000 {
            insert_tuple(id, i, i * 10);
        }

        // Verify data is there
        let mut heap = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(500), &mut heap) };
        assert!(result.is_cons());

        // Get the table and check its capacity before clearing
        let table = registry().get_table(table_ref).unwrap();
        let cap_before = {
            match &table.storage {
                EtsStorage::Set(map_lock) => {
                    let map = map_lock.read().unwrap();
                    assert_eq!(map.len(), 1000);
                    map.capacity()
                }
                _ => panic!("expected Set storage"),
            }
        };
        assert!(cap_before >= 1000, "capacity should be >= 1000, got {}", cap_before);

        // delete_all_objects should replace the HashMap and clear the pool
        ets_delete_all_objects(table_ref);

        let (cap_after, pool_mapped) = {
            match &table.storage {
                EtsStorage::Set(map_lock) => {
                    let map = map_lock.read().unwrap();
                    assert_eq!(map.len(), 0);
                    (map.capacity(), table.pool.lock().unwrap().mapped_bytes())
                }
                _ => panic!("expected Set storage"),
            }
        };
        // A fresh HashMap has capacity 0
        assert_eq!(cap_after, 0, "capacity should be 0 after delete_all_objects, got {}", cap_after);
        // All mmap'd regions should be freed
        assert_eq!(pool_mapped, 0, "pool should have 0 mapped bytes after delete_all_objects");
    }

    // ---------------------------------------------------------------
    // Regression: table is usable after delete_all_objects
    // ---------------------------------------------------------------

    #[test]
    fn table_usable_after_delete_all_objects() {
        let (id, _name) = make_test_table("test_usable_after_clear");
        let table_ref = ErlTerm::small_int(id as i64);

        insert_tuple(id, 1, 100);
        ets_delete_all_objects(table_ref);

        // Table should still work after clearing
        insert_tuple(id, 2, 200);
        let mut heap = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(2), &mut heap) };
        assert!(result.is_cons(), "should find key after clear + re-insert");
    }

    // ---------------------------------------------------------------
    // CompactHeap pointer stability — nested terms
    // ---------------------------------------------------------------

    #[test]
    fn compact_heap_nested_tuple_roundtrip() {
        // This test catches the Vec reallocation bug: when storing a
        // tuple containing nested boxed terms, alloc_words returns
        // pointers that must remain valid after subsequent allocations.
        let (id, _name) = make_test_table("test_nested_tuple");
        let table_ref = ErlTerm::small_int(id as i64);

        let mut heap = ProcessHeap::new();
        // Build {key, {inner1, inner2, inner3}, value}
        let inner = heap.alloc_tuple(&[
            ErlTerm::small_int(10),
            ErlTerm::small_int(20),
            ErlTerm::small_int(30),
        ]);
        let outer = heap.alloc_tuple(&[
            ErlTerm::small_int(1),  // key
            inner,                   // nested tuple
            ErlTerm::small_int(99),  // value
        ]);

        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(table_ref, outer) }.unwrap();

        // Lookup and verify all nested values survived the copy
        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(1), &mut heap2) };
        assert!(result.is_cons(), "should find nested tuple entry");

        // Extract the stored tuple
        let stored_tuple = unsafe { (*result.as_cons_ptr().unwrap()).head };
        let ptr = stored_tuple.as_boxed_ptr().unwrap();
        let header = BoxedHeader(unsafe { *ptr });
        assert_eq!(header.arity(), 3, "outer tuple should have 3 elements");

        // Check key
        let key = ErlTerm(unsafe { *ptr.add(1) });
        assert_eq!(key.as_small_int(), Some(1));

        // Check nested tuple
        let nested = ErlTerm(unsafe { *ptr.add(2) });
        assert!(nested.is_boxed(), "second element should be a boxed tuple");
        let nptr = nested.as_boxed_ptr().unwrap();
        let nheader = BoxedHeader(unsafe { *nptr });
        assert_eq!(nheader.arity(), 3, "inner tuple should have 3 elements");
        assert_eq!(ErlTerm(unsafe { *nptr.add(1) }).as_small_int(), Some(10));
        assert_eq!(ErlTerm(unsafe { *nptr.add(2) }).as_small_int(), Some(20));
        assert_eq!(ErlTerm(unsafe { *nptr.add(3) }).as_small_int(), Some(30));

        // Check value
        let val = ErlTerm(unsafe { *ptr.add(3) });
        assert_eq!(val.as_small_int(), Some(99));
    }

    #[test]
    fn compact_heap_binary_roundtrip() {
        // Test that small (inline) and large (refc) binaries survive
        // the CompactHeap copy.
        let (id, _name) = make_test_table("test_binary_roundtrip");
        let table_ref = ErlTerm::small_int(id as i64);

        let mut heap = ProcessHeap::new();
        let small_bin = heap.alloc_binary_auto(b"hello");             // inline
        let large_bin = heap.alloc_binary_auto(&[0xAB; 100]);         // refc
        let tup = heap.alloc_tuple(&[ErlTerm::small_int(42), small_bin, large_bin]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(table_ref, tup) }.unwrap();

        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(42), &mut heap2) };
        assert!(result.is_cons());

        let stored = unsafe { (*result.as_cons_ptr().unwrap()).head };
        let ptr = stored.as_boxed_ptr().unwrap();

        // Check small binary
        let sb = ErlTerm(unsafe { *ptr.add(2) });
        if let Some((data_ptr, len)) = unsafe { crate::ffi::binary_data_pub(sb) } {
            let data = unsafe { std::slice::from_raw_parts(data_ptr, len) };
            assert_eq!(data, b"hello");
        } else {
            panic!("small binary not readable");
        }

        // Check large binary
        let lb = ErlTerm(unsafe { *ptr.add(3) });
        if let Some((data_ptr, len)) = unsafe { crate::ffi::binary_data_pub(lb) } {
            let data = unsafe { std::slice::from_raw_parts(data_ptr, len) };
            assert_eq!(len, 100);
            assert!(data.iter().all(|&b| b == 0xAB));
        } else {
            panic!("large binary not readable");
        }
    }

    #[test]
    fn compact_heap_list_roundtrip() {
        // Test that lists stored in ETS survive the CompactHeap copy.
        let (id, _name) = make_test_table("test_list_roundtrip");
        let table_ref = ErlTerm::small_int(id as i64);

        let mut heap = ProcessHeap::new();
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(10),
            ErlTerm::small_int(20),
            ErlTerm::small_int(30),
        ]);
        let tup = heap.alloc_tuple(&[ErlTerm::small_int(7), list]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(table_ref, tup) }.unwrap();

        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(7), &mut heap2) };
        assert!(result.is_cons());

        let stored = unsafe { (*result.as_cons_ptr().unwrap()).head };
        let ptr = stored.as_boxed_ptr().unwrap();
        let stored_list = ErlTerm(unsafe { *ptr.add(2) });

        // Walk the list and verify elements
        let mut cursor = stored_list;
        let mut values = Vec::new();
        while cursor.is_cons() {
            if let Some(cp) = cursor.as_cons_ptr() {
                unsafe {
                    values.push((*cp).head.as_small_int().unwrap());
                    cursor = (*cp).tail;
                }
            } else {
                break;
            }
        }
        assert_eq!(values, vec![10, 20, 30]);
        assert!(cursor.is_nil());
    }

    // ---------------------------------------------------------------
    // Phase 5 regression baseline: map roundtrip
    // ---------------------------------------------------------------

    #[test]
    fn compact_heap_map_roundtrip() {
        let (id, _name) = make_test_table("test_map_roundtrip");
        let table_ref = ErlTerm::small_int(id as i64);

        let mut heap = ProcessHeap::new();
        let atom_table = global_atom_table();
        let a_atom = ErlTerm::atom(atom_table.intern("a"));
        let b_atom = ErlTerm::atom(atom_table.intern("b"));

        // Build #{a => 1, b => [10, 20]}
        let list_val = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(10),
            ErlTerm::small_int(20),
        ]);
        let map = heap.alloc_map(MapPairs::from_flat(&[a_atom, ErlTerm::small_int(1), b_atom, list_val]).expect("complete key/value pairs"));
        let tup = heap.alloc_tuple(&[ErlTerm::small_int(42), map]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(table_ref, tup) }.unwrap();

        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(42), &mut heap2) };
        assert!(result.is_cons());

        let stored = unsafe { (*result.as_cons_ptr().unwrap()).head };
        let ptr = stored.as_boxed_ptr().unwrap();
        let map_term = ErlTerm(unsafe { *ptr.add(2) });
        assert!(map_term.is_boxed());
        let mptr = map_term.as_boxed_ptr().unwrap();
        let mheader = crate::term::BoxedHeader(unsafe { *mptr });
        assert_eq!(mheader.subtype(), crate::term::BOXED_MAP);
        assert_eq!(mheader.arity(), 2); // 2 pairs
    }

    // ---------------------------------------------------------------
    // Phase 5 regression baseline: float roundtrip
    // ---------------------------------------------------------------

    #[test]
    fn compact_heap_float_roundtrip() {
        let (id, _name) = make_test_table("test_float_roundtrip");
        let table_ref = ErlTerm::small_int(id as i64);

        let mut heap = ProcessHeap::new();
        let float_val = heap.alloc_float(3.12567);
        let tup = heap.alloc_tuple(&[ErlTerm::small_int(1), float_val]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(table_ref, tup) }.unwrap();

        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(1), &mut heap2) };
        assert!(result.is_cons());

        let stored = unsafe { (*result.as_cons_ptr().unwrap()).head };
        let ptr = stored.as_boxed_ptr().unwrap();
        let fterm = ErlTerm(unsafe { *ptr.add(2) });
        assert!(fterm.is_boxed());
        let fptr = fterm.as_boxed_ptr().unwrap();
        let fheader = crate::term::BoxedHeader(unsafe { *fptr });
        assert_eq!(fheader.subtype(), crate::term::BOXED_FLOAT);
        let bits = unsafe { *fptr.add(1) };
        let val = f64::from_bits(bits);
        assert!((val - 3.12567).abs() < 1e-10);
    }

    // ---------------------------------------------------------------
    // Phase 5 regression baseline: deeply nested roundtrip
    // ---------------------------------------------------------------

    #[test]
    fn compact_heap_deeply_nested_roundtrip() {
        // Build {key, {a, [1, {b, 3.25}]}}
        let (id, _name) = make_test_table("test_deeply_nested");
        let table_ref = ErlTerm::small_int(id as i64);

        let mut heap = ProcessHeap::new();
        let atom_table = global_atom_table();
        let a_atom = ErlTerm::atom(atom_table.intern("a"));
        let b_atom = ErlTerm::atom(atom_table.intern("b"));

        let float_val = heap.alloc_float(3.25);
        let inner_tup = heap.alloc_tuple(&[b_atom, float_val]);
        let list = heap.alloc_list_from_slice(&[ErlTerm::small_int(1), inner_tup]);
        let mid_tup = heap.alloc_tuple(&[a_atom, list]);
        let outer = heap.alloc_tuple(&[ErlTerm::small_int(99), mid_tup]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(table_ref, outer) }.unwrap();

        // Lookup and drill down all 5 levels
        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(99), &mut heap2) };
        assert!(result.is_cons());

        // Level 1: outer tuple
        let stored = unsafe { (*result.as_cons_ptr().unwrap()).head };
        let optr = stored.as_boxed_ptr().unwrap();
        assert_eq!(ErlTerm(unsafe { *optr.add(1) }).as_small_int(), Some(99));

        // Level 2: mid tuple {a, list}
        let mid = ErlTerm(unsafe { *optr.add(2) });
        let mptr = mid.as_boxed_ptr().unwrap();
        assert_eq!(ErlTerm(unsafe { *mptr.add(1) }).as_atom_id(), a_atom.as_atom_id());

        // Level 3: list [1, {b, 3.25}]
        let list_term = ErlTerm(unsafe { *mptr.add(2) });
        assert!(list_term.is_cons());
        let first = unsafe { (*list_term.as_cons_ptr().unwrap()).head };
        assert_eq!(first.as_small_int(), Some(1));
        let rest = unsafe { (*list_term.as_cons_ptr().unwrap()).tail };
        assert!(rest.is_cons());

        // Level 4: second list element = {b, 3.25}
        let second = unsafe { (*rest.as_cons_ptr().unwrap()).head };
        let sptr = second.as_boxed_ptr().unwrap();
        assert_eq!(ErlTerm(unsafe { *sptr.add(1) }).as_atom_id(), b_atom.as_atom_id());

        // Level 5: float
        let fterm = ErlTerm(unsafe { *sptr.add(2) });
        let fptr = fterm.as_boxed_ptr().unwrap();
        let val = f64::from_bits(unsafe { *fptr.add(1) });
        assert!((val - 3.25).abs() < 1e-10);
    }

    // ---------------------------------------------------------------
    // Phase 5 regression baseline: reference roundtrip
    // ---------------------------------------------------------------

    #[test]
    fn compact_heap_reference_roundtrip() {
        let (id, _name) = make_test_table("test_reference_roundtrip");
        let table_ref = ErlTerm::small_int(id as i64);

        let mut heap = ProcessHeap::new();
        // Manually allocate a BOXED_REFERENCE with unique_id=42
        let ptr = heap.alloc_words(crate::term::REFERENCE_TOTAL_WORDS);
        unsafe {
            *ptr = crate::term::BoxedHeader::new(crate::term::BoxedSubtype::Reference, 0).0;
            *ptr.add(crate::term::REFERENCE_ID_WORD_INDEX) = 42u64;
        }
        let ref_term = ErlTerm::boxed(ptr as *const u64);
        let tup = heap.alloc_tuple(&[ErlTerm::small_int(7), ref_term]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(table_ref, tup) }.unwrap();

        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(7), &mut heap2) };
        assert!(result.is_cons());

        let stored = unsafe { (*result.as_cons_ptr().unwrap()).head };
        let tptr = stored.as_boxed_ptr().unwrap();
        let rterm = ErlTerm(unsafe { *tptr.add(2) });
        let rptr = rterm.as_boxed_ptr().unwrap();
        let rheader = crate::term::BoxedHeader(unsafe { *rptr });
        assert_eq!(rheader.subtype(), crate::term::BOXED_REFERENCE);
        let uid = unsafe { *rptr.add(crate::term::REFERENCE_ID_WORD_INDEX) };
        assert_eq!(uid, 42);
    }

    // ---------------------------------------------------------------
    // Phase 5 regression baseline: refc binary Arc lifecycle
    // ---------------------------------------------------------------

    #[test]
    fn compact_heap_refc_binary_lifecycle() {
        let (id, _name) = make_test_table("test_refc_lifecycle");
        let table_ref = ErlTerm::small_int(id as i64);

        // Create a refc binary (>64 bytes)
        let data = vec![0xABu8; 100];
        let arc_data: Arc<BinaryData> = Arc::new(BinaryData::new(data));
        assert_eq!(Arc::strong_count(&arc_data), 1);

        let mut heap = ProcessHeap::new();
        let bin = heap.alloc_refc_binary_from_arc(arc_data.clone(), 0, 100);
        // arc_data (1) + heap.refc_binaries (1) = 2
        assert_eq!(Arc::strong_count(&arc_data), 2);

        let tup = heap.alloc_tuple(&[ErlTerm::small_int(1), bin]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(table_ref, tup) }.unwrap();
        // ETS deep-copies the binary by creating a NEW Arc (Arc::new(BinaryData::new(bytes.to_vec()))),
        // NOT cloning the original. So arc_data count stays at 2.
        assert_eq!(Arc::strong_count(&arc_data), 2);

        // Lookup into a new heap — deep_copy_into_heap reads the bytes from the
        // ETS compact heap and creates yet another independent Arc.
        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(1), &mut heap2) };
        assert!(result.is_cons());
        // arc_data count still 2 (lookup creates its own independent Arc)
        assert_eq!(Arc::strong_count(&arc_data), 2);

        // Verify the binary data is readable
        let stored = unsafe { (*result.as_cons_ptr().unwrap()).head };
        let sptr = stored.as_boxed_ptr().unwrap();
        let bterm = ErlTerm(unsafe { *sptr.add(2) });
        if let Some((data_ptr, len)) = unsafe { crate::ffi::binary_data_pub(bterm) } {
            let data = unsafe { std::slice::from_raw_parts(data_ptr, len) };
            assert_eq!(len, 100);
            assert!(data.iter().all(|&b| b == 0xAB));
        } else {
            panic!("refc binary not readable after lookup");
        }

        // Delete from ETS — drops the CompactHeap's independent Arc (doesn't affect arc_data)
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_delete(table_ref, ErlTerm::small_int(1)) };
        assert_eq!(Arc::strong_count(&arc_data), 2); // arc_data + heap

        // Drop the original process heap
        drop(heap);
        assert_eq!(Arc::strong_count(&arc_data), 1); // just arc_data

        // Verify the lookup result is still readable after ETS deletion
        if let Some((data_ptr, len)) = unsafe { crate::ffi::binary_data_pub(bterm) } {
            let data = unsafe { std::slice::from_raw_parts(data_ptr, len) };
            assert_eq!(len, 100);
            assert!(data.iter().all(|&b| b == 0xAB));
        } else {
            panic!("refc binary not readable after ETS delete");
        }
    }

    // ---------------------------------------------------------------
    // Phase 5 regression baseline: overwrite then lookup
    // ---------------------------------------------------------------

    #[test]
    fn overwrite_then_lookup_returns_new() {
        let (id, _name) = make_test_table("test_overwrite_new");
        let table_ref = ErlTerm::small_int(id as i64);

        // Insert with list value
        let mut heap = ProcessHeap::new();
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(1), ErlTerm::small_int(2), ErlTerm::small_int(3),
        ]);
        let tup1 = heap.alloc_tuple(&[ErlTerm::small_int(10), list]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(table_ref, tup1) }.unwrap();

        // Overwrite with float value
        let float_val = heap.alloc_float(2.625);
        let tup2 = heap.alloc_tuple(&[ErlTerm::small_int(10), float_val]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(table_ref, tup2) }.unwrap();

        // Lookup should return the float, not the list
        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(10), &mut heap2) };
        assert!(result.is_cons());

        let stored = unsafe { (*result.as_cons_ptr().unwrap()).head };
        let ptr = stored.as_boxed_ptr().unwrap();
        let val = ErlTerm(unsafe { *ptr.add(2) });
        assert!(val.is_boxed());
        let vptr = val.as_boxed_ptr().unwrap();
        let header = crate::term::BoxedHeader(unsafe { *vptr });
        assert_eq!(header.subtype(), crate::term::BOXED_FLOAT);
        let f = f64::from_bits(unsafe { *vptr.add(1) });
        assert!((f - 2.625).abs() < 1e-10);
    }

    // ---------------------------------------------------------------
    // Phase 5 regression baseline: lookup result survives delete
    // ---------------------------------------------------------------

    #[test]
    fn lookup_result_independent_after_delete() {
        let (id, _name) = make_test_table("test_result_after_delete");
        let table_ref = ErlTerm::small_int(id as i64);

        let mut heap = ProcessHeap::new();
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(10), ErlTerm::small_int(20), ErlTerm::small_int(30),
        ]);
        let tup = heap.alloc_tuple(&[ErlTerm::small_int(5), list]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(table_ref, tup) }.unwrap();

        // Lookup into process heap
        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(5), &mut heap2) };
        assert!(result.is_cons());

        // Delete the key from ETS
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_delete(table_ref, ErlTerm::small_int(5)) };

        // Verify the process heap copy is still readable
        let stored = unsafe { (*result.as_cons_ptr().unwrap()).head };
        let ptr = stored.as_boxed_ptr().unwrap();
        let stored_list = ErlTerm(unsafe { *ptr.add(2) });

        let mut cursor = stored_list;
        let mut values = Vec::new();
        while cursor.is_cons() {
            if let Some(cp) = cursor.as_cons_ptr() {
                unsafe {
                    values.push((*cp).head.as_small_int().unwrap());
                    cursor = (*cp).tail;
                }
            } else {
                break;
            }
        }
        assert_eq!(values, vec![10, 20, 30]);
    }

    // ---------------------------------------------------------------
    // Phase 5 regression baseline: GC after lookup
    // ---------------------------------------------------------------

    #[test]
    fn gc_after_lookup_preserves_result() {
        let (id, _name) = make_test_table("test_gc_after_lookup");
        let table_ref = ErlTerm::small_int(id as i64);

        let mut heap = ProcessHeap::new();
        let inner = heap.alloc_tuple(&[
            ErlTerm::small_int(100),
            ErlTerm::small_int(200),
        ]);
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(10),
            inner,
        ]);
        let tup = heap.alloc_tuple(&[ErlTerm::small_int(77), list]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(table_ref, tup) }.unwrap();

        // Lookup into a fresh heap
        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(77), &mut heap2) };
        assert!(result.is_cons());

        // Allocate garbage to force GC threshold
        for i in 0..5000 {
            heap2.alloc_tuple(&[ErlTerm::small_int(i), ErlTerm::small_int(i * 2)]);
        }

        // Run GC with the lookup result as a root
        let mut roots = vec![result];
        unsafe { heap2.collect(&mut roots) };
        let result_after_gc = roots[0];

        // Verify the result survived GC correctly
        assert!(result_after_gc.is_cons());
        let stored = unsafe { (*result_after_gc.as_cons_ptr().unwrap()).head };
        let ptr = stored.as_boxed_ptr().unwrap();
        assert_eq!(ErlTerm(unsafe { *ptr.add(1) }).as_small_int(), Some(77));

        // Drill into the list
        let stored_list = ErlTerm(unsafe { *ptr.add(2) });
        assert!(stored_list.is_cons());
        let first = unsafe { (*stored_list.as_cons_ptr().unwrap()).head };
        assert_eq!(first.as_small_int(), Some(10));

        // Second element is a tuple {100, 200}
        let rest = unsafe { (*stored_list.as_cons_ptr().unwrap()).tail };
        assert!(rest.is_cons());
        let second = unsafe { (*rest.as_cons_ptr().unwrap()).head };
        let sptr = second.as_boxed_ptr().unwrap();
        assert_eq!(ErlTerm(unsafe { *sptr.add(1) }).as_small_int(), Some(100));
        assert_eq!(ErlTerm(unsafe { *sptr.add(2) }).as_small_int(), Some(200));
    }

    // ---------------------------------------------------------------
    // Phase 5 regression baseline: multiple lookups independent
    // ---------------------------------------------------------------

    #[test]
    fn multiple_lookups_same_key_independent() {
        let (id, _name) = make_test_table("test_multi_lookup");
        let table_ref = ErlTerm::small_int(id as i64);

        let mut heap = ProcessHeap::new();
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(1), ErlTerm::small_int(2),
        ]);
        let tup = heap.alloc_tuple(&[ErlTerm::small_int(3), list]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe { ets_insert(table_ref, tup) }.unwrap();

        // Two lookups into the same heap
        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result_a = unsafe { ets_lookup(table_ref, ErlTerm::small_int(3), &mut heap2) };
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result_b = unsafe { ets_lookup(table_ref, ErlTerm::small_int(3), &mut heap2) };

        // Both should succeed
        assert!(result_a.is_cons());
        assert!(result_b.is_cons());

        // But they should be at different addresses (independent copies)
        let addr_a = result_a.as_cons_ptr().unwrap() as usize;
        let addr_b = result_b.as_cons_ptr().unwrap() as usize;
        assert_ne!(addr_a, addr_b, "two lookups of same key should produce independent copies");
    }

    // ---------------------------------------------------------------
    // Phase 5 regression baseline: delete_all_objects, result survives
    // ---------------------------------------------------------------

    #[test]
    fn delete_all_objects_lookup_result_survives() {
        let (id, _name) = make_test_table("test_dao_result_survives");
        let table_ref = ErlTerm::small_int(id as i64);

        // Insert 100 entries
        for i in 0..100i64 {
            let mut heap = ProcessHeap::new();
            let list = heap.alloc_list_from_slice(&[
                ErlTerm::small_int(i * 10),
                ErlTerm::small_int(i * 20),
            ]);
            let tup = heap.alloc_tuple(&[ErlTerm::small_int(i), list]);
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            unsafe { ets_insert(table_ref, tup) }.unwrap();
        }

        // Lookup key=50
        let mut heap2 = ProcessHeap::new();
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(50), &mut heap2) };
        assert!(result.is_cons());

        // Clear the table — pool munmaps everything
        ets_delete_all_objects(table_ref);

        // Verify the process heap copy is still valid
        let stored = unsafe { (*result.as_cons_ptr().unwrap()).head };
        let ptr = stored.as_boxed_ptr().unwrap();
        assert_eq!(ErlTerm(unsafe { *ptr.add(1) }).as_small_int(), Some(50));

        let stored_list = ErlTerm(unsafe { *ptr.add(2) });
        assert!(stored_list.is_cons());
        let first = unsafe { (*stored_list.as_cons_ptr().unwrap()).head };
        assert_eq!(first.as_small_int(), Some(500));
    }

    // ---------------------------------------------------------------
    // Phase 5 regression baseline: concurrent insert + lookup (Rust threads)
    // ---------------------------------------------------------------

    #[test]
    fn concurrent_insert_lookup_rust_threads() {
        let (id, _name) = make_test_table("test_conc_insert_lookup");
        let table_ref = ErlTerm::small_int(id as i64);

        // Pre-insert some entries so readers can find things
        for i in 0..100i64 {
            insert_tuple(id, i, i * 10);
        }

        let barrier = Arc::new(std::sync::Barrier::new(4));
        let mut handles = Vec::new();

        // 2 writer threads
        for writer_id in 0..2u64 {
            let b = barrier.clone();
            handles.push(std::thread::spawn(move || {
                b.wait();
                let base = 100 + writer_id * 500;
                for i in 0..500i64 {
                    insert_tuple(id, base as i64 + i, (base as i64 + i) * 10);
                }
            }));
        }

        // 2 reader threads
        for _ in 0..2 {
            let b = barrier.clone();
            handles.push(std::thread::spawn(move || {
                b.wait();
                for _ in 0..1000 {
                    let key = (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .subsec_nanos() as i64)
                        % 100;
                    let mut heap = ProcessHeap::new();
                    // SAFETY: the test owns the live backing storage and uses current GC roots.
                    let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(key), &mut heap) };
                    if result.is_cons() {
                        // Verify the result is a valid tuple
                        let stored = unsafe { (*result.as_cons_ptr().unwrap()).head };
                        assert!(stored.is_boxed());
                    }
                    // Result can also be nil if another thread hasn't inserted yet
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Verify final state — original keys should exist
        for i in 0..100i64 {
            let mut heap = ProcessHeap::new();
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(i), &mut heap) };
            assert!(result.is_cons(), "key {} should exist", i);
        }
    }

    // ---------------------------------------------------------------
    // Phase 5 regression baseline: concurrent delete + lookup (Rust threads)
    // ---------------------------------------------------------------

    #[test]
    fn concurrent_delete_lookup_rust_threads() {
        let (id, _name) = make_test_table("test_conc_delete_lookup");
        let table_ref = ErlTerm::small_int(id as i64);

        // Pre-insert 1000 entries
        for i in 0..1000i64 {
            insert_tuple(id, i, i * 10);
        }

        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut handles = Vec::new();

        // 1 deleter thread: delete even keys
        {
            let b = barrier.clone();
            handles.push(std::thread::spawn(move || {
                b.wait();
                for i in (0..1000i64).step_by(2) {
                    // SAFETY: the test owns the live backing storage and uses current GC roots.
                    unsafe { ets_delete(table_ref, ErlTerm::small_int(i)) };
                }
            }));
        }

        // 2 reader threads
        for _ in 0..2 {
            let b = barrier.clone();
            handles.push(std::thread::spawn(move || {
                b.wait();
                for i in 0..1000i64 {
                    let mut heap = ProcessHeap::new();
                    // SAFETY: the test owns the live backing storage and uses current GC roots.
                    let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(i), &mut heap) };
                    if result.is_cons() {
                        // Verify it's a valid tuple with correct key
                        let stored = unsafe { (*result.as_cons_ptr().unwrap()).head };
                        let ptr = stored.as_boxed_ptr().unwrap();
                        let key = ErlTerm(unsafe { *ptr.add(1) });
                        assert_eq!(key.as_small_int(), Some(i));
                    }
                    // [] is also valid (entry was deleted)
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Verify: odd keys must still exist
        for i in (1..1000i64).step_by(2) {
            let mut heap = ProcessHeap::new();
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            let result = unsafe { ets_lookup(table_ref, ErlTerm::small_int(i), &mut heap) };
            assert!(result.is_cons(), "odd key {} should still exist", i);
        }
    }

    // ---------------------------------------------------------------
    // Regression tests: ets:insert/2 list hardening
    // ---------------------------------------------------------------

    #[test]
    fn insert_list_of_tuples_inserts_all() {
        let (id, _name) = make_test_table("test_list_insert_all");
        let table_ref = ErlTerm::small_int(id as i64);

        // Build a proper list of 7 tuples (mimics onecached reset_stats)
        let mut heap = ProcessHeap::new();
        let atom_table = global_atom_table();
        let keys: Vec<u32> = (0..7).map(|i| atom_table.intern(&format!("key_{}", i))).collect();
        let tuples: Vec<ErlTerm> = keys.iter()
            .map(|&k| heap.alloc_tuple(&[ErlTerm::atom(k), ErlTerm::small_int(0)]))
            .collect();
        let list = heap.alloc_list_from_slice(&tuples);

        // Insert must succeed
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { ets_insert(table_ref, list) }.is_ok());

        // ALL 7 keys must be present
        for &k in &keys {
            let mut h = ProcessHeap::new();
            // SAFETY: the test owns the live backing storage and uses current GC roots.
            let result = unsafe { ets_lookup(table_ref, ErlTerm::atom(k), &mut h) };
            assert!(result.is_cons(), "key {:?} should exist after list insert", k);
        }
    }

    #[test]
    fn insert_list_with_non_tuple_element_fails() {
        let (id, _name) = make_test_table("test_list_non_tuple");
        let table_ref = ErlTerm::small_int(id as i64);

        // Build a list where one element is an atom (not a tuple)
        let mut heap = ProcessHeap::new();
        let tup = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(100)]);
        let bad_elem = ErlTerm::atom(0); // not a tuple
        let list = heap.alloc_cons(bad_elem, ErlTerm::nil());
        let list = heap.alloc_cons(tup, list);

        // Must fail (badarg) because second element is not a tuple
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { ets_insert(table_ref, list) }.is_err());
    }

    #[test]
    fn insert_improper_list_fails() {
        let (id, _name) = make_test_table("test_improper_list");
        let table_ref = ErlTerm::small_int(id as i64);

        // Build an improper list: [{1,2} | not_nil]
        let mut heap = ProcessHeap::new();
        let tup = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(100)]);
        let improper_tail = ErlTerm::small_int(42); // not nil
        let improper_list = heap.alloc_cons(tup, improper_tail);

        // Must fail because the tail is not nil
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { ets_insert(table_ref, improper_list) }.is_err());
    }

    #[test]
    fn insert_to_nonexistent_table_fails() {
        let mut heap = ProcessHeap::new();
        let tup = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { ets_insert(ErlTerm::small_int(999_999), tup) }.is_err());
    }

    // ---------------------------------------------------------------
    // Regression tests: ets:new/2 duplicate named table
    // ---------------------------------------------------------------

    #[test]
    fn new_duplicate_named_table_fails() {
        let atom_table = global_atom_table();
        let name = atom_table.intern("test_dup_named");
        let opts = vec![("named_table".to_string(), ErlTerm::nil())];

        // First create should succeed
        let id1 = ets_new(name, &opts, crate::process::ProcessId::new(0));
        assert!(id1.is_some());

        // Second create with same name should fail
        let id2 = ets_new(name, &opts, crate::process::ProcessId::new(0));
        assert!(id2.is_none(), "duplicate named_table should return None (badarg)");

        // Original table should still work
        let mut heap = ProcessHeap::new();
        let tup = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(42)]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { ets_insert(ErlTerm::atom(name), tup) }.is_ok());
    }

    #[test]
    fn new_without_named_table_allows_same_atom() {
        let atom_table = global_atom_table();
        let name = atom_table.intern("test_unnamed_dup");
        let opts: Vec<(String, ErlTerm)> = vec![]; // no named_table option

        // Both creates should succeed (not checking for duplicate name)
        let id1 = ets_new(name, &opts, crate::process::ProcessId::new(0));
        assert!(id1.is_some());

        let id2 = ets_new(name, &opts, crate::process::ProcessId::new(0));
        assert!(id2.is_some());
        assert_ne!(id1.unwrap(), id2.unwrap());
    }

    // Regression: a process exit releases the names of the named tables it
    // owned, and only those. The Erlang fixture ets_named_table_return cannot
    // check this deterministically: natively the monitor message is sent
    // before the exiting process's tables are deleted.
    #[test]
    fn owner_exit_releases_only_its_named_table() {
        let name = global_atom_table().intern("test_owner_exit_named");
        let named = [("named_table".to_string(), ErlTerm::nil())];
        let named_owner = crate::process::ProcessId::new(9_100_001);
        let unnamed_owner = crate::process::ProcessId::new(9_100_002);
        let tab = ErlTerm::atom(name);
        let undefined = ErlTerm::atom(ATOM_UNDEFINED);
        let mut heap = ProcessHeap::new();

        assert_eq!(ets_new_handle(name, &named, named_owner), Some(tab));
        let unnamed = ets_new_handle(name, &[], unnamed_owner).expect("an unnamed table may share the atom");
        assert!(unnamed.as_small_int().is_some());

        // The unnamed table's owner exits: the name still reaches the named table.
        delete_tables_owned_by(unnamed_owner);
        assert_eq!(ets_info(unnamed, Some("name"), &mut heap), undefined);
        assert_eq!(ets_info(tab, Some("owner"), &mut heap), ErlTerm::pid(named_owner));

        // The named table's owner exits: the name is free again.
        delete_tables_owned_by(named_owner);
        assert_eq!(ets_info(tab, Some("name"), &mut heap), undefined);
        assert_eq!(ets_new_handle(name, &named, named_owner), Some(tab));
        assert!(ets_delete_table(tab));
    }

    // Regression: a lookup by name held the name lock while it waited for
    // the table lock, and process-exit cleanup takes them in the other order,
    // so the two could block each other for good. Named tables are reached by
    // name more often now that ets:new/2 returns the name.
    #[test]
    fn name_lookups_and_owner_exit_cleanup_do_not_deadlock() {
        const ROUNDS: u64 = 50_000;
        let name = global_atom_table().intern("test_lock_order_named");
        let other = global_atom_table().intern("test_lock_order_unnamed");
        let named = [("named_table".to_string(), ErlTerm::nil())];
        let tab = ets_new_handle(name, &named, crate::process::ProcessId::new(9_100_010)).unwrap();
        let (done, finished) = std::sync::mpsc::channel();

        let lookups = done.clone();
        std::thread::spawn(move || {
            for i in 0..ROUNDS {
                // SAFETY: the key is an immediate term.
                let _ = unsafe { ets_member(tab, ErlTerm::small_int(i as i64)) };
            }
            lookups.send(()).unwrap();
        });
        std::thread::spawn(move || {
            for i in 0..ROUNDS {
                let owner = crate::process::ProcessId::new(9_200_000 + i);
                ets_new(other, &[], owner).unwrap();
                delete_tables_owned_by(owner);
            }
            done.send(()).unwrap();
        });

        for _ in 0..2 {
            finished
                .recv_timeout(std::time::Duration::from_secs(60))
                .expect("ETS name lookups and owner-exit cleanup deadlocked");
        }
        assert!(ets_delete_table(tab));
    }

    // Regression: the duplicate-name check released its lock before the name
    // was registered, so two processes creating the same named table at once
    // could both succeed and both be handed the name.
    #[test]
    fn concurrent_named_creates_admit_one() {
        const THREADS: u64 = 8;
        let named = [("named_table".to_string(), ErlTerm::nil())];
        for round in 0..200u64 {
            let name = global_atom_table().intern(&format!("test_concurrent_named_{round}"));
            let start = Arc::new(std::sync::Barrier::new(THREADS as usize));
            let creators: Vec<_> = (0..THREADS)
                .map(|i| {
                    let start = Arc::clone(&start);
                    let named = named.clone();
                    let owner = crate::process::ProcessId::new(9_300_000 + round * THREADS + i);
                    std::thread::spawn(move || {
                        start.wait();
                        ets_new_handle(name, &named, owner)
                    })
                })
                .collect();
            let created: Vec<_> = creators.into_iter().filter_map(|c| c.join().unwrap()).collect();
            assert_eq!(created, vec![ErlTerm::atom(name)], "round {round}");
            assert!(ets_delete_table(ErlTerm::atom(name)));
        }
    }
}
