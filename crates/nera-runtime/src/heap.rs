//! Per-process heap with generational (young/old) copying GC.
//!
//! Each Erlang process owns a `ProcessHeap` with two generations:
//! - **Young**: `YoungGen` (multi-chunk Vec<MaybeUninit<u64>>-backed bump allocator) for new allocations.
//! - **Old**: `bumpalo::Bump` arena for tenured (long-lived) data.
//!
//! Minor GC copies live young data into old. Major GC copies all live data
//! (young + old) into a fresh Bump, then installs it as the new old generation.
//! No write barrier is needed because Erlang data is immutable — old-gen terms
//! never contain pointers to young-gen terms.

use crate::binary::BinaryData;
use bumpalo::Bump;
use std::alloc::Layout;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::term::{
    bigint_total_words, BoxedHeader, ConsCell, ErlTerm, BOXED_BIGINT, BOXED_BINARY, BOXED_CLOSURE,
    BOXED_FLOAT, BOXED_MAP, BOXED_MATCH_CTX, BOXED_REFC_BINARY, BOXED_REFERENCE, BOXED_TUPLE,
    MATCH_CTX_TOTAL_WORDS, REFC_BINARY_OFFSET_WORD_INDEX, REFC_BINARY_PTR_WORD_INDEX,
    REFC_BINARY_TOTAL_WORDS, REFERENCE_TOTAL_WORDS, TAG_ATOM,
    TAG_BOXED, TAG_CONS, TAG_MASK, TAG_NIL, TAG_PID, TAG_SMALL_INT, EXTERNAL_BIT,
};

/// Default young-gen GC threshold: 2 KiB.
/// Starts small to match BEAM's ~233-word initial heap; adaptive GC sizing
/// grows it for long-lived processes.
pub const DEFAULT_GC_THRESHOLD: usize = 2 * 1024;

/// Default old-gen GC threshold: 1 MiB.
pub const DEFAULT_OLD_GC_THRESHOLD: usize = 1024 * 1024;

/// Default binary virtual heap threshold: 256 KiB.
/// Matches BEAM's `BIN_VH_MIN_SIZE` (32K words on 64-bit = 256 KiB).
/// GC triggers when the virtual binary overhead exceeds this threshold.
const DEFAULT_BINARY_VHEAP_THRESHOLD: usize = 256 * 1024;

/// Fixed per-reference overhead charged to binary_vheap_size for each refc
/// binary descriptor on the process heap.  This replaces charging the full
/// backing-store length (which penalises large shared binaries unfairly —
/// an Arc clone is O(1) regardless of data size).
///
/// BEAM uses `sizeof(ProcBin)` (~10 words = 80 bytes on 64-bit).  We use a
/// similar constant.  With the default 256 KiB threshold this allows ~3,200
/// live refc binary references before triggering GC, independent of data size.
pub(crate) const REFC_BINARY_VHEAP_OVERHEAD: usize = 80;

/// Initial inline allocation chunk size: 64 words = 512 bytes.
const INLINE_CHUNK_MIN_WORDS: usize = 64;
/// Maximum inline allocation chunk size: 512 words = 4 KiB.
const INLINE_CHUNK_MAX_WORDS: usize = 512;

/// Default young generation capacity in words (256 words = 2 KiB).
/// BEAM starts at ~233 words. Short-lived processes never GC and stay small.
/// Long-lived processes grow via adaptive GC sizing.
const DEFAULT_YOUNG_CAPACITY_WORDS: usize = 256;

/// Force a major GC after this many minor GCs (matches BEAM default).
const DEFAULT_MAX_MINOR_GCS: u32 = 65535;

// ---------------------------------------------------------------------------
// YoungGen — young generation allocator
// ---------------------------------------------------------------------------

/// A single contiguous bump-allocation chunk backed by `Vec<MaybeUninit<u64>>`.
struct BumpChunk {
    storage: Vec<std::mem::MaybeUninit<u64>>,
    cursor: usize, // word offset of next free slot
}

impl BumpChunk {
    fn new(capacity_words: usize) -> Self {
        let storage = vec![std::mem::MaybeUninit::uninit(); capacity_words];
        BumpChunk { storage, cursor: 0 }
    }

    #[inline]
    fn alloc_words(&mut self, n: usize) -> Option<*mut u64> {
        // cursor never exceeds capacity; subtract before comparing so even
        // usize::MAX is rejected without wrapping the reservation end.
        if n > self.storage.capacity() - self.cursor {
            return None;
        }
        let ptr = unsafe { self.storage.as_mut_ptr().add(self.cursor).cast::<u64>() };
        self.cursor += n;
        Some(ptr)
    }

    fn contains(&self, ptr: *const u64) -> bool {
        let base = self.storage.as_ptr() as usize;
        let end = base + self.storage.capacity() * 8;
        let addr = ptr as usize;
        addr >= base && addr < end
    }

    #[allow(dead_code)]
    fn remaining(&self) -> usize {
        self.storage.capacity() - self.cursor
    }
}

/// Young generation allocator: a chain of `BumpChunk`s.
///
/// When the current chunk fills, a new larger chunk is added. All chunks
/// are considered "young" — the `contains()` check iterates over all chunks
/// (typically 1-3 chunks due to doubling growth).
pub struct YoungGen {
    current: BumpChunk,
    overflow: Vec<BumpChunk>,
    bytes_in_overflow: usize,
}

impl YoungGen {
    fn new(capacity_words: usize) -> Self {
        YoungGen {
            current: BumpChunk::new(capacity_words),
            overflow: Vec::new(),
            bytes_in_overflow: 0,
        }
    }

    /// Allocate `n` words, growing as needed. Panics on an invalid capacity.
    #[inline]
    fn alloc_words(&mut self, n: usize) -> *mut u64 {
        if let Some(ptr) = self.current.alloc_words(n) {
            return ptr;
        }
        self.grow_and_alloc(n)
    }

    #[cold]
    #[inline(never)]
    fn grow_and_alloc(&mut self, n: usize) -> *mut u64 {
        // Current chunk is full; create a bigger one.
        // Growth is a cold path. Validate byte layout before allocating.
        let new_cap = self.current.storage.capacity().saturating_mul(2).max(n);
        Layout::array::<u64>(new_cap).expect("young heap capacity overflow");
        let old = std::mem::replace(&mut self.current, BumpChunk::new(new_cap));
        self.bytes_in_overflow += old.cursor * 8;
        self.overflow.push(old);
        self.current.alloc_words(n).expect("fresh chunk must fit")
    }

    /// Check if a pointer falls within any young-gen chunk.
    fn contains(&self, ptr: *const u64) -> bool {
        self.current.contains(ptr) || self.overflow.iter().any(|c| c.contains(ptr))
    }

    /// Total bytes allocated across all chunks.
    #[allow(dead_code)]
    fn bytes_used(&self) -> usize {
        self.current.cursor * 8 + self.bytes_in_overflow
    }

    /// Adopt all chunks from `other` into this young gen's overflow list.
    /// After this call, `other` is effectively empty (consumed by move).
    /// All pointers into `other`'s chunks remain valid because the backing
    /// `Vec<MaybeUninit<u64>>` storage is moved, not copied.
    fn adopt_chunks(&mut self, other: YoungGen) {
        // Adopt current chunk (only if it has data).
        if other.current.cursor > 0 {
            self.bytes_in_overflow += other.current.cursor * 8;
            self.overflow.push(other.current);
        }
        // Adopt overflow chunks.
        for chunk in other.overflow {
            self.bytes_in_overflow += chunk.cursor * 8;
            self.overflow.push(chunk);
        }
    }
}

/// Clone an `Arc<BinaryData>` given a raw pointer previously obtained from
/// `Arc::as_ptr()`.
///
/// This is the standard pattern for sharing an `Arc` across an FFI / raw
/// pointer boundary: reconstruct a temporary `Arc`, clone it (which
/// atomically increments the strong count), then `mem::forget` the
/// temporary so we don't decrement the original owner's reference.
///
/// # Safety
///
/// `raw_ptr` must have been obtained from `Arc::as_ptr()` on a live
/// `Arc<BinaryData>` whose strong count is > 0 for the duration of this call.
pub(crate) unsafe fn clone_arc_from_raw(raw_ptr: u64) -> Arc<BinaryData> {
    let ptr = raw_ptr as *const BinaryData;
    let temp = Arc::from_raw(ptr);
    debug_assert!(
        Arc::strong_count(&temp) >= 1,
        "clone_arc_from_raw: Arc strong_count is 0 — use-after-free"
    );
    let cloned = Arc::clone(&temp);
    std::mem::forget(temp);
    cloned
}

/// Binaries larger than this threshold are allocated as reference-counted
/// off-heap binaries instead of inline heap binaries.  Matches BEAM's
/// threshold of 64 bytes.
pub const REFC_BINARY_THRESHOLD: usize = 64;

// ---------------------------------------------------------------------------
// ProcessHeap
// ---------------------------------------------------------------------------

/// Backing owners cannot be removed through the public API while raw term
/// descriptors may still refer to them. Only collection releases stale owners.
///
/// ```compile_fail
/// use nera_runtime::heap::ProcessHeap;
/// let mut heap = ProcessHeap::new();
/// heap.refc_binaries.clear();
/// ```
///
/// ```compile_fail
/// use nera_runtime::heap::ProcessHeap;
/// let mut heap = ProcessHeap::new();
/// heap.borrowed_heaps.clear();
/// ```
pub struct ProcessHeap {
    /// Young generation: multi-chunk bump allocator for new allocations.
    young: YoungGen,
    /// Old generation: bumpalo arena for tenured data.
    old: Bump,
    /// Bytes allocated in young gen since last minor GC.
    young_bytes_allocated: usize,
    /// Bytes allocated in old gen (cumulative, reset on major GC).
    old_bytes_allocated: usize,
    /// Young-gen GC trigger threshold (256 KiB default, adaptive).
    gc_threshold: usize,
    /// Old-gen GC trigger threshold (1 MiB default, adaptive).
    old_gc_threshold: usize,
    /// Pointer-root identities after the last complete trace of a large heap.
    /// Comparison-only words, never dereferenced or retained as GC roots.
    /// Erlang graphs are immutable; immediate counters, order and duplicates
    /// do not change reachability. A different set conservatively retries GC.
    last_full_roots: Option<Vec<u64>>,
    /// Number of minor GCs since last major GC.
    minor_gc_count: u32,
    /// Force a major GC after this many minor GCs.
    max_minor_gcs: u32,
    /// Maximum heap size in words. 0 = no limit (default).
    max_heap_size: usize,
    /// Off-heap backing stores for reference-counted binaries.
    ///
    /// This Vec is the **sole Rust owner** of all `Arc<BinaryData>` backing stores
    /// referenced by refc binary descriptors on this heap. On-heap descriptors
    /// store a raw pointer obtained via `Arc::as_ptr()`; those raw pointers
    /// are only valid as long as the corresponding Arc exists here.
    ///
    /// Invariants:
    /// - Minor GC retains old-generation backing stores without traversing
    ///   old terms; major GC drops those no longer reachable.
    /// - Message send clones Arcs into the `MessageFragment` before any GC can
    ///   run, so in-flight messages always keep their backing stores alive.
    /// - The bump arena does not call `Drop`, so this Vec is the only mechanism
    ///   that ensures the backing data is freed when no longer needed.
    refc_binaries: Vec<Arc<BinaryData>>,
    /// Backing stores referenced by promoted descriptors. Minor GC skips old
    /// container contents, so these owners must survive until major GC traces
    /// the whole heap again. Young garbage can still be swept immediately.
    old_refc_ptrs: HashSet<u64>,
    /// Count of unique Arc<BinaryData> raw addresses, used for vheap accounting.
    /// Only charge `REFC_BINARY_VHEAP_OVERHEAD` once per unique backing store.
    refc_unique_count: usize,
    /// Sum of byte-lengths of all refc binaries currently tracked by this
    /// process (in `refc_binaries`).  Approximates off-heap memory pressure.
    binary_vheap_size: usize,
    /// GC triggers when `binary_vheap_size >= binary_vheap_threshold`.
    binary_vheap_threshold: usize,
    /// Borrowed ETS heaps kept alive for zero-copy lookup results.
    /// Arc references prevent the backing CompactHeap from being freed
    /// while the process holds external pointers into it.
    /// Type-erased via `dyn Any + Send + Sync` to avoid a circular
    /// dependency between heap.rs and ets.rs.
    borrowed_heaps: Vec<Arc<dyn std::any::Any + Send + Sync>>,
    /// Current inline allocation chunk size in words. Starts small (64 words
    /// = 512 bytes) and doubles on each refill up to 512 words (4 KiB).
    inline_chunk_words: usize,
}

/// Allocation-only storage for message fragments and process-info snapshots.
///
/// The underlying heap is private: this type exposes neither GC nor mutable
/// heap access nor borrowed-owner registration. All storage stays in the young
/// generation and binary owners are retained by allocation/copy operations.
/// There is intentionally no conversion from a general ProcessHeap.
#[repr(transparent)]
pub(crate) struct FragmentHeap {
    heap: ProcessHeap,
}

impl FragmentHeap {
    #[cfg(any(not(mio_supported), test))]
    #[inline(always)]
    pub(crate) fn new() -> Self {
        Self { heap: ProcessHeap::new() }
    }

    #[inline(always)]
    pub(crate) fn new_with_capacity(words: usize) -> Self {
        Self { heap: ProcessHeap::new_with_capacity(words) }
    }

    #[inline(always)]
    pub(crate) fn alloc_words(&mut self, words: usize) -> *mut u64 {
        self.heap.alloc_words(words)
    }

    #[inline(always)]
    pub(crate) fn alloc_tuple(&mut self, elements: &[ErlTerm]) -> ErlTerm {
        self.heap.alloc_tuple(elements)
    }

    #[cfg(any(not(mio_supported), test))]
    #[inline(always)]
    pub(crate) fn alloc_binary_auto(&mut self, bytes: &[u8]) -> ErlTerm {
        self.heap.alloc_binary_auto(bytes)
    }

    /// Copy from an owner-bound view without exposing the destination heap.
    #[inline(always)]
    pub(crate) fn copy_term(&mut self, source: crate::term::BorrowedTerm<'_>) -> ErlTerm {
        source.copy_to_heap(&mut self.heap)
    }

    /// # Safety
    /// The entire source graph must be initialized, immutable and live for this
    /// copy, with well-formed binary windows. No moving source GC may run.
    #[inline(always)]
    pub(crate) unsafe fn copy_raw_term(&mut self, source: ErlTerm) -> ErlTerm {
        // SAFETY: source validity is guaranteed by this method's caller.
        unsafe { crate::message::deep_copy_term(source, &mut self.heap) }
    }
}

/// A borrowed view of complete key/value pairs for map allocation.
///
/// Flat runtime buffers are checked once when forming the view. Native Rust
/// callers can use two-element arrays to express pairing without validation.
/// Neither constructor allocates or copies terms. This describes pairing only;
/// it does not validate key uniqueness or ownership of pointed-to term graphs.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct MapPairs<'a> {
    terms: &'a [ErlTerm],
}

impl<'a> MapPairs<'a> {
    #[inline]
    pub fn new(pairs: &'a [[ErlTerm; 2]]) -> Self {
        Self {
            terms: pairs.as_flattened(),
        }
    }

    /// Reject an incomplete final pair without touching the destination heap.
    #[inline]
    pub fn from_flat(terms: &'a [ErlTerm]) -> Option<Self> {
        terms.len().is_multiple_of(2).then_some(Self { terms })
    }

    /// Number of key/value pairs.
    #[inline]
    pub fn len(self) -> usize {
        self.terms.len() / 2
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.terms.is_empty()
    }
}

impl Default for ProcessHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessHeap {
    /// Create a new process heap with default thresholds.
    pub fn new() -> Self {
        ProcessHeap {
            young: YoungGen::new(DEFAULT_YOUNG_CAPACITY_WORDS),
            old: Bump::new(),
            young_bytes_allocated: 0,
            old_bytes_allocated: 0,
            gc_threshold: DEFAULT_GC_THRESHOLD,
            old_gc_threshold: DEFAULT_OLD_GC_THRESHOLD,
            last_full_roots: None,
            minor_gc_count: 0,
            max_minor_gcs: DEFAULT_MAX_MINOR_GCS,
            max_heap_size: 0,
            refc_binaries: Vec::new(),
            old_refc_ptrs: HashSet::new(),
            refc_unique_count: 0,
            binary_vheap_size: 0,
            binary_vheap_threshold: DEFAULT_BINARY_VHEAP_THRESHOLD,
            borrowed_heaps: Vec::new(),
            inline_chunk_words: INLINE_CHUNK_MIN_WORDS,
        }
    }

    /// Create a new process heap with a specific young-gen capacity (in words).
    /// Use this when the default 256-word heap is too small (e.g., `min_heap_size`
    /// from `spawn_opt`) or when a small fragment heap is sufficient.
    pub fn new_with_capacity(capacity_words: usize) -> Self {
        let cap = capacity_words.max(16); // floor at 16 words (128 bytes)
        ProcessHeap {
            young: YoungGen::new(cap),
            old: Bump::new(),
            young_bytes_allocated: 0,
            old_bytes_allocated: 0,
            gc_threshold: (cap * 8).max(DEFAULT_GC_THRESHOLD),
            old_gc_threshold: DEFAULT_OLD_GC_THRESHOLD,
            last_full_roots: None,
            minor_gc_count: 0,
            max_minor_gcs: DEFAULT_MAX_MINOR_GCS,
            max_heap_size: 0,
            refc_binaries: Vec::new(),
            old_refc_ptrs: HashSet::new(),
            refc_unique_count: 0,
            binary_vheap_size: 0,
            binary_vheap_threshold: DEFAULT_BINARY_VHEAP_THRESHOLD,
            borrowed_heaps: Vec::new(),
            inline_chunk_words: INLINE_CHUNK_MIN_WORDS,
        }
    }

    /// Allocate `n` words (each 8 bytes) from the young generation.
    ///
    /// # Safety
    /// The returned pointer is valid for the lifetime of the current generation.
    /// Callers must initialise the memory before reading.
    pub fn alloc_words(&mut self, n: usize) -> *mut u64 {
        let ptr = self.young.alloc_words(n);
        self.young_bytes_allocated += n * 8;
        ptr
    }

    /// Pre-allocate a chunk for inline (Cranelift-emitted) bump allocation.
    /// Returns (cursor, limit) pointers. Allocated from the young generation.
    /// Chunk size starts at 512 bytes and doubles on each refill up to 4 KiB.
    pub fn refill_inline_region(&mut self) -> (*mut u64, *mut u64) {
        let words = self.inline_chunk_words;
        let ptr = self.alloc_words(words);
        let limit = unsafe { ptr.add(words) };
        // Grow for next refill (double up to cap)
        if self.inline_chunk_words < INLINE_CHUNK_MAX_WORDS {
            self.inline_chunk_words = std::cmp::min(self.inline_chunk_words * 2, INLINE_CHUNK_MAX_WORDS);
        }
        (ptr, limit)
    }

    /// Allocate a cons cell and return a tagged cons pointer term.
    pub fn alloc_cons(&mut self, head: ErlTerm, tail: ErlTerm) -> ErlTerm {
        let ptr = self.alloc_words(2);
        unsafe {
            // ConsCell is repr(C): [head, tail], each 8 bytes.
            *ptr = head.0;
            *ptr.add(1) = tail.0;
        }
        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
        unsafe { ErlTerm::cons_unchecked(ptr as *const ConsCell) }
    }

    /// Allocate a tuple `{e0, e1, ..., e(n-1)}` and return a tagged boxed
    /// pointer term. The on-heap layout is `[header, e0, e1, ...]`.
    pub fn alloc_tuple(&mut self, elements: &[ErlTerm]) -> ErlTerm {
        let arity = elements.len();
        let ptr = self.alloc_words(1 + arity); // header + elements
        unsafe {
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Tuple, arity as u64).0;
            for (i, elem) in elements.iter().enumerate() {
                *ptr.add(1 + i) = elem.0;
            }
        }
        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
        unsafe { ErlTerm::boxed_unchecked(ptr as *const u64) }
    }

    /// Store a validated matching descriptor without changing its ABI.
    /// This retains its source as a GC-traced term, like tuple/list allocation.
    pub fn alloc_match_ctx(&mut self, context: crate::match_context::MatchContext) -> ErlTerm {
        let ptr = self.alloc_words(MATCH_CTX_TOTAL_WORDS);
        // SAFETY: repr(C) descriptor has u64 alignment and exactly four words.
        unsafe {
            ptr.cast::<crate::match_context::MatchContext>().write(context);
            ErlTerm::boxed_unchecked(ptr)
        }
    }

    /// Allocate a boxed float and return a tagged boxed pointer term.
    /// On-heap layout: `[header(FLOAT,1), f64-bits]`.
    pub fn alloc_float(&mut self, val: f64) -> ErlTerm {
        let ptr = self.alloc_words(2);
        unsafe {
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Float, 1).0;
            *ptr.add(1) = val.to_bits();
        }
        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
        unsafe { ErlTerm::boxed_unchecked(ptr as *const u64) }
    }

    /// Validate the header and padded allocation size before touching the heap.
    #[inline(always)]
    fn binary_layout(byte_len: usize) -> (BoxedHeader, usize, usize) {
        let max_header_bytes = ((u64::MAX >> crate::term::BOXED_TAG_BITS) / 8)
            .min(usize::MAX as u64) as usize;
        // Leave one aligned word for the header. This limit guarantees that
        // rounding, word addition and byte multiplication all fit usize and
        // Layout's isize bound, including on 32-bit targets.
        let max_layout_bytes = ((isize::MAX as usize) / 8 - 1) * 8;
        assert!(byte_len <= max_header_bytes.min(max_layout_bytes), "binary size exceeds header or allocation capacity");
        let header = BoxedHeader::new(crate::term::BoxedSubtype::HeapBinary, (byte_len as u64) * 8);
        let data_words = byte_len.div_ceil(8);
        let words = data_words + 1;
        let padded = data_words * 8;
        (header, words, padded)
    }

    /// Allocate a binary from a byte slice and return a tagged boxed pointer
    /// term. On-heap layout: `[header(BINARY, byte_count), raw_bytes_padded...]`
    ///
    /// The header's arity field stores the **bit** length (= byte_len * 8).
    /// The raw bytes are stored immediately after the header, padded up to
    /// word alignment.
    pub fn alloc_binary(&mut self, bytes: &[u8]) -> ErlTerm {
        let byte_len = bytes.len();
        let (header, total_words, padded) = Self::binary_layout(byte_len);
        let ptr = self.alloc_words(total_words);
        unsafe {
            *ptr = header.0;
            // Zero-initialise data region (for padding bytes)
            let data_ptr = ptr.add(1) as *mut u8;
            std::ptr::write_bytes(data_ptr, 0, padded);
            // Copy actual bytes
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), data_ptr, byte_len);
        }
        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
        unsafe { ErlTerm::boxed_unchecked(ptr as *const u64) }
    }

    /// Allocate a zero-initialised binary of the given byte length.
    /// Used during binary construction where bytes will be filled in later.
    /// Header stores bit count (byte_len * 8).
    pub fn alloc_binary_zeroed(&mut self, byte_len: usize) -> ErlTerm {
        let (header, total_words, padded) = Self::binary_layout(byte_len);
        let ptr = self.alloc_words(total_words);
        unsafe {
            *ptr = header.0;
            let data_ptr = ptr.add(1) as *mut u8;
            std::ptr::write_bytes(data_ptr, 0, padded);
        }
        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
        unsafe { ErlTerm::boxed_unchecked(ptr as *const u64) }
    }

    /// Allocate a reference-counted binary.  The byte data lives in an
    /// `Arc<BinaryData>` outside the arena; a 3-word descriptor is placed on
    /// the arena heap:
    ///
    /// ```text
    /// [header(REFC_BINARY, len * 8) | raw_arc_ptr | byte_offset]
    /// ```
    ///
    /// `offset` and `len` select a window into the backing bytes.
    /// Invalid windows panic before allocation.
    /// The `Arc` is pushed into `self.refc_binaries` to anchor ownership.
    pub fn alloc_refc_binary_from_arc(
        &mut self,
        data: Arc<BinaryData>,
        offset: usize,
        len: usize,
    ) -> ErlTerm {
        let range = crate::binary::BinarySlice::new(data, offset, len)
            .expect("reference-counted binary window or bit length out of bounds");
        self.alloc_binary_slice(range)
    }

    /// Consume a validated shared binary window without repeating its bounds check.
    #[inline(always)]
    pub fn alloc_binary_slice(&mut self, range: crate::binary::BinarySlice) -> ErlTerm {
        let (data, offset, bits) = range.into_bit_parts();
        let bit_len = bits as u64;
        let ptr = self.alloc_words(REFC_BINARY_TOTAL_WORDS);
        // `Arc::as_ptr` returns `*const BinaryData` — a thin pointer that
        // fits in a single u64.  We store it raw; `clone_arc_from_raw()`
        // can reconstruct a temporary `Arc` from it later.
        let raw_arc_ptr = Arc::as_ptr(&data) as u64;
        unsafe {
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::RefcBinary, bit_len).0;
            *ptr.add(REFC_BINARY_PTR_WORD_INDEX) = raw_arc_ptr;
            *ptr.add(REFC_BINARY_OFFSET_WORD_INDEX) = offset as u64;
        }
        self.retain_refc_binary(data);
        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
        unsafe { ErlTerm::boxed_unchecked(ptr as *const u64) }
    }

    /// Keep a binary descriptor's backing store alive and charge its virtual
    /// heap overhead. Adding an owner cannot invalidate existing terms; only GC
    /// removes owners after tracing roots. Does not allocate heap words or run GC.
    #[inline]
    pub(crate) fn retain_refc_binary(&mut self, data: Arc<BinaryData>) {
        self.binary_vheap_size += REFC_BINARY_VHEAP_OVERHEAD;
        self.refc_binaries.push(data);
    }

    /// Retain immutable external storage referenced by zero-copy lookup terms.
    /// Collection releases these owners only when no external terms survive.
    #[inline]
    pub(crate) fn retain_borrowed_heap(&mut self, owner: Arc<dyn std::any::Any + Send + Sync>) {
        self.borrowed_heaps.push(owner);
    }

    #[cfg(test)]
    pub(crate) fn refc_binary_count(&self) -> usize {
        self.refc_binaries.len()
    }

    #[cfg(test)]
    pub(crate) fn borrowed_heap_count(&self) -> usize {
        self.borrowed_heaps.len()
    }

    /// Allocate a binary, automatically choosing between an inline heap
    /// binary (<= 64 bytes) and a reference-counted off-heap binary
    /// (> 64 bytes).
    pub fn alloc_binary_auto(&mut self, bytes: &[u8]) -> ErlTerm {
        if bytes.len() <= REFC_BINARY_THRESHOLD {
            self.alloc_binary(bytes)
        } else {
            let data: Arc<BinaryData> = Arc::new(BinaryData::new(bytes.to_vec()));
            let len = bytes.len();
            self.alloc_refc_binary_from_arc(data, 0, len)
        }
    }

    /// Move allocation-only fragment storage without copying its objects.
    /// FragmentHeap cannot collect, promote objects or register borrowed owners;
    /// its complete set of storage/owners is therefore transferable by design.
    /// The operation is internal; owned MessageFragments expose safe transfer.
    /// A general heap, including one with promoted objects, is not adoptable.
    /// ```compile_fail
    /// use nera_runtime::heap::ProcessHeap;
    /// let mut fragment = ProcessHeap::new();
    /// let term = fragment.alloc_float(1.0);
    /// unsafe { fragment.collect_full(&mut [term]); }
    /// ProcessHeap::new().adopt_fragment(fragment);
    /// ```
    #[inline(always)]
    pub(crate) fn adopt_fragment(&mut self, fragment: FragmentHeap) {
        let fragment = fragment.heap;
        // FragmentHeap construction/operations guarantee empty old gen and no
        // borrowed owners; no runtime validation or general-heap conversion.
        self.young.adopt_chunks(fragment.young);
        self.young_bytes_allocated += fragment.young_bytes_allocated;
        self.binary_vheap_size += fragment.binary_vheap_size;
        self.refc_binaries.extend(fragment.refc_binaries);
    }

    /// Allocate a map from interleaved key-value pairs.
    /// Pairing is established by MapPairs before allocation.
    /// On-heap layout: `[header(MAP, npairs), k1, v1, k2, v2, ...]`
    /// The header's arity field stores the number of key-value pairs.
    /// A flat term slice cannot be used directly as map pairs.
    /// ```compile_fail
    /// use nera_runtime::{heap::ProcessHeap, term::ErlTerm};
    /// ProcessHeap::new().alloc_map(&[ErlTerm::small_int(1)]);
    /// ```
    pub fn alloc_map(&mut self, pairs: MapPairs<'_>) -> ErlTerm {
        let npairs = pairs.len();
        let pairs = pairs.terms;
        let total_words = 1 + pairs.len(); // header + all k/v elements
        let ptr = self.alloc_words(total_words);
        unsafe {
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Map, npairs as u64).0;
            for (i, term) in pairs.iter().enumerate() {
                *ptr.add(1 + i) = term.0;
            }
        }
        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
        unsafe { ErlTerm::boxed_unchecked(ptr as *const u64) }
    }

    /// Build a proper list `[e0, e1, ..., e(n-1)]` from a slice.
    /// Returns `nil` for an empty slice.
    pub fn alloc_list_from_slice(&mut self, elems: &[ErlTerm]) -> ErlTerm {
        let mut list = ErlTerm::nil();
        for elem in elems.iter().rev() {
            list = self.alloc_cons(*elem, list);
        }
        list
    }

    /// Set the `fullsweep_after` limit. Returns the old value.
    pub fn set_fullsweep_after(&mut self, n: u32) -> u32 {
        let old = self.max_minor_gcs;
        self.max_minor_gcs = n;
        old
    }

    /// Get the current `fullsweep_after` limit.
    pub fn get_fullsweep_after(&self) -> u32 {
        self.max_minor_gcs
    }

    /// Returns the max_heap_size in words (0 = no limit).
    pub fn max_heap_size_words(&self) -> usize {
        self.max_heap_size
    }

    /// Set the maximum heap size in words. 0 = no limit. Returns the old value.
    pub fn set_max_heap_size(&mut self, words: usize) -> usize {
        let old = self.max_heap_size;
        self.max_heap_size = words;
        old
    }

    /// Returns `true` if the heap exceeds `max_heap_size` (and a limit is set).
    pub fn exceeds_max_heap_size(&self) -> bool {
        self.max_heap_size > 0 && self.bytes_allocated() > self.max_heap_size.saturating_mul(8)
    }

    /// Returns `true` if any GC trigger condition is met.
    pub fn needs_gc(&self) -> bool {
        self.young_bytes_allocated >= self.gc_threshold
            || self.old_bytes_allocated >= self.old_gc_threshold
            || self.binary_vheap_size >= self.binary_vheap_threshold
            || self.exceeds_max_heap_size()
    }

    /// Returns `true` if a major (full) GC should run instead of minor.
    /// Binary vheap pressure does NOT trigger major GC — minor GC sweeps
    /// refc binaries too, so it's sufficient for binary-heavy workloads.
    fn needs_major_gc(&self) -> bool {
        self.old_bytes_allocated >= self.old_gc_threshold
            || self.minor_gc_count >= self.max_minor_gcs
    }

    /// Returns `true` if the heap is oversized and should be compacted.
    /// Used by the scheduler when a process enters an idle state (receive-wait,
    /// IO-wait) to shrink heaps that grew during a burst.
    ///
    /// A major GC is needed because dead old-gen data can only be reclaimed
    /// by a full collection. The threshold is generous (4× default) to avoid
    /// compacting processes with legitimately large working sets.
    pub fn should_shrink(&self) -> bool {
        self.old_bytes_allocated > DEFAULT_OLD_GC_THRESHOLD * 4
    }

    /// Decide whether an oversized idle heap needs a full collection.
    pub(crate) fn should_collect_idle(&self, roots: &[ErlTerm]) -> bool {
        self.should_shrink() && (self.needs_gc() || !self.same_full_roots(roots))
    }

    fn pointer_roots(roots: &[ErlTerm]) -> Vec<u64> {
        let mut pointers: Vec<_> = roots.iter()
            .filter(|term| term.is_cons() || term.is_boxed())
            .map(|term| term.0).collect();
        pointers.sort_unstable();
        pointers.dedup();
        pointers
    }

    fn same_full_roots(&self, roots: &[ErlTerm]) -> bool {
        self.last_full_roots.as_ref()
            .is_some_and(|previous| *previous == Self::pointer_roots(roots))
    }

    fn remember_full_roots(&mut self, roots: &[ErlTerm]) {
        self.last_full_roots = self.should_shrink().then(|| Self::pointer_roots(roots));
    }

    /// Current byte count of allocations made via this heap (young + old).
    pub fn bytes_allocated(&self) -> usize {
        self.young_bytes_allocated + self.old_bytes_allocated
    }

    /// Perform garbage collection, dispatching to minor or major as needed.
    ///
    /// Minor GC: copies live young data into old. Old-gen terms are returned as-is.
    /// Major GC: copies ALL live data into a fresh Bump, installed as new old gen.
    /// # Safety
    /// Every root and reachable object must be live, initialized and have the
    /// runtime layout, owned by this heap or its retained external owners. Roots
    /// must contain current pointers after previous collections. Supply every
    /// root that will be used afterward; collection invalidates saved terms and
    /// pointers not updated through this slice. No object borrow or concurrent
    /// access to the collected storage may remain active during collection.
    /// Calling collection requires an explicit root-validity boundary.
    /// ```compile_fail,E0133
    /// use nera_runtime::heap::ProcessHeap;
    /// ProcessHeap::new().collect(&mut []);
    /// ```
    pub unsafe fn collect(&mut self, roots: &mut [ErlTerm]) {
        if self.needs_major_gc() {
            self.collect_major(roots);
        } else {
            self.collect_minor(roots);
        }
    }

    /// Force a major (full) garbage collection.  Used by the scheduler for
    /// idle heap shrinking — when a process enters receive-wait with an
    /// oversized old gen, a full GC compacts both generations and returns
    /// the memory to the OS.
    /// # Safety
    /// Every root and reachable object must be live, initialized and have the
    /// runtime layout, owned by this heap or its retained external owners. Roots
    /// must contain current pointers after previous collections. Supply every
    /// root that will be used afterward; collection invalidates saved terms and
    /// pointers not updated through this slice. No object borrow or concurrent
    /// access to the collected storage may remain active during collection.
    /// ```compile_fail,E0133
    /// use nera_runtime::heap::ProcessHeap;
    /// ProcessHeap::new().collect_full(&mut []);
    /// ```
    pub unsafe fn collect_full(&mut self, roots: &mut [ErlTerm]) {
        self.collect_major(roots);
    }

    /// Minor GC: promote live young-gen data to old gen.
    fn collect_minor(&mut self, roots: &mut [ErlTerm]) {
        let old_young = std::mem::replace(
            &mut self.young,
            YoungGen::new(0), // temporary placeholder
        );

        let mut copier = GcCopier::new_minor_with_hint(&old_young, &mut self.old, self.young_bytes_allocated);

        // Copy roots — old-gen terms are returned as-is.
        for root in roots.iter_mut() {
            *root = copier.copy_term(*root);
        }

        let promoted_bytes = copier.bytes_copied;
        let live_refc_ptrs = copier.live_refc_ptrs;
        let has_external_terms = copier.has_external_terms;

        // Give a large, freshly traced promotion headroom instead of copying
        // it again on the next scheduler slice. Do not double the accumulated
        // old size: smaller promotions must still reach the existing threshold.
        let traced_all = self.old_bytes_allocated == 0;
        self.old_bytes_allocated += promoted_bytes;
        self.old_gc_threshold = self.old_gc_threshold.max(promoted_bytes.saturating_mul(2));
        if traced_all {
            self.remember_full_roots(roots);
        } else if promoted_bytes != 0 {
            // New old objects can later die even if the root set returns to
            // its earlier identity. The old snapshot no longer covers them.
            self.last_full_roots = None;
        }

        // Create fresh young gen with adaptive capacity.
        let young_capacity = std::cmp::max(promoted_bytes / 8 * 2, DEFAULT_YOUNG_CAPACITY_WORDS);
        self.young = YoungGen::new(young_capacity);
        self.young_bytes_allocated = 0;

        // Adaptive young threshold.
        self.gc_threshold = std::cmp::max(promoted_bytes.saturating_mul(2), DEFAULT_GC_THRESHOLD);
        self.minor_gc_count += 1;

        // Shrink heuristic: if old gen is large but we're barely promoting
        // anything, schedule a major GC next time to compact.  This handles
        // the case where a process burst-allocated a lot of data, most of it
        // became garbage, but old_gc_threshold is too high to trigger normally.
        // We check: old gen > 4× default AND promoted < 25% of old gen.
        if self.old_bytes_allocated > DEFAULT_OLD_GC_THRESHOLD * 4
            && promoted_bytes < self.old_bytes_allocated / 4
            && !self.same_full_roots(roots)
        {
            // Lower old_gc_threshold to trigger major GC on next needs_gc().
            self.old_gc_threshold = self.old_bytes_allocated;
        }

        // Reset inline chunk sizing.
        self.inline_chunk_words = INLINE_CHUNK_MIN_WORDS;

        // Descriptors copied by a minor GC are now old. Previously promoted
        // descriptors may be hidden inside untraversed old containers.
        self.old_refc_ptrs.extend(live_refc_ptrs);
        self.refc_binaries.retain(|arc| {
            self.old_refc_ptrs.contains(&(Arc::as_ptr(arc) as u64))
        });
        self.update_binary_vheap();
        self.sweep_borrowed_heaps(has_external_terms);

        // Drop old young gen (old_young goes out of scope).
    }

    /// Major GC: copy all live data (young + old) into a fresh Bump.
    fn collect_major(&mut self, roots: &mut [ErlTerm]) {
        let mut copier = GcCopier::new_major_with_hint(self.young_bytes_allocated + self.old_bytes_allocated);
        for root in roots.iter_mut() {
            *root = copier.copy_term(*root);
        }

        let bytes_copied = copier.bytes_copied;
        let live_refc_ptrs = copier.live_refc_ptrs.clone();
        let has_external_terms = copier.has_external_terms;

        // Install copier's arena as the new old gen.
        self.old = copier.into_arena();
        self.old_bytes_allocated = bytes_copied;

        // Create fresh empty young gen.
        let young_capacity = DEFAULT_YOUNG_CAPACITY_WORDS;
        self.young = YoungGen::new(young_capacity);
        self.young_bytes_allocated = 0;

        // Reset minor GC counter.
        self.minor_gc_count = 0;

        // Adaptive thresholds.
        self.gc_threshold = std::cmp::max(bytes_copied.saturating_mul(2), DEFAULT_GC_THRESHOLD);
        self.old_gc_threshold = std::cmp::max(bytes_copied.saturating_mul(2), DEFAULT_OLD_GC_THRESHOLD);
        self.remember_full_roots(roots);

        // Reset inline chunk sizing.
        self.inline_chunk_words = INLINE_CHUNK_MIN_WORDS;

        // Sweep refc binaries, binary vheap, borrowed heaps.
        self.sweep_refc_binaries(&live_refc_ptrs);
        self.old_refc_ptrs = live_refc_ptrs;
        self.update_binary_vheap();
        self.sweep_borrowed_heaps(has_external_terms);
    }

    /// Sweep stale refc binaries: retain only those whose raw pointer was
    /// seen during the live-data copy phase.
    fn sweep_refc_binaries(&mut self, live_refc_ptrs: &HashSet<u64>) {
        if !self.refc_binaries.is_empty() {
            self.refc_binaries.retain(|arc| {
                let raw_ptr = Arc::as_ptr(arc) as u64;
                live_refc_ptrs.contains(&raw_ptr)
            });
        }
    }

    /// Recalculate binary virtual heap from surviving Arcs and adapt threshold.
    ///
    /// Uses **unique** Arc count for the live vheap size (the actual off-heap
    /// pressure), but sets the threshold high enough to accommodate the total
    /// descriptor count growing back to its pre-GC level without re-triggering.
    /// This prevents pathological GC cycling when a process accumulates many
    /// descriptors to the same shared refc binary (e.g., 100K messages all
    /// referencing the same 10KB binary).
    fn update_binary_vheap(&mut self) {
        let total = self.refc_binaries.len();
        let unique = if total <= 1 {
            total
        } else {
            let mut seen = HashSet::with_capacity(total.min(256));
            for arc in &self.refc_binaries {
                seen.insert(Arc::as_ptr(arc) as usize);
            }
            seen.len()
        };
        self.refc_unique_count = unique;
        // Live vheap is based on unique Arcs (actual off-heap pressure).
        let unique_vheap = unique * REFC_BINARY_VHEAP_OVERHEAD;
        // Threshold must tolerate total descriptors re-accumulating.
        let total_vheap = total * REFC_BINARY_VHEAP_OVERHEAD;
        self.binary_vheap_size = unique_vheap;
        self.binary_vheap_threshold = std::cmp::max(
            total_vheap.saturating_mul(2),
            DEFAULT_BINARY_VHEAP_THRESHOLD,
        );
    }

    /// Conservative borrowed_heaps sweep.
    fn sweep_borrowed_heaps(&mut self, has_external_terms: bool) {
        if !self.borrowed_heaps.is_empty() && !has_external_terms {
            self.borrowed_heaps.clear();
        }
    }
}

// ---------------------------------------------------------------------------
// GcCopier — unified copier for both major and minor GC
// ---------------------------------------------------------------------------

/// Allocation target for GC copying.
enum GcTarget<'a> {
    /// Major GC: copies into a fresh owned arena (replaces old gen).
    Owned(Bump),
    /// Minor GC: promotes into the existing old-gen arena.
    Borrowed(&'a mut Bump),
}

/// An immediate without the external bit: the collector copies it as itself
/// with no side effect.
#[inline]
pub(crate) fn is_plain_immediate(word: u64) -> bool {
    word & EXTERNAL_BIT == 0 && matches!(word & TAG_MASK, TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL)
}

/// Term slots of an object already allocated in the target arena whose
/// values are still to be copied: the `len` words at `dst` receive copies of
/// the `len` source words at `src`.
struct PendingSlots {
    dst: *mut u64,
    src: *const u64,
    len: usize,
}

/// Unified GC copier that handles both major and minor collection.
///
/// - **Major GC** (`young_ref: None`): copies all live data into a fresh
///   `Bump` arena.  Every heap pointer is relocated.
/// - **Minor GC** (`young_ref: Some`): copies only young-gen data into the
///   old-gen arena.  Old-gen pointers are returned as-is.
struct GcCopier<'a> {
    target: GcTarget<'a>,
    /// Reference to the old young gen for contains() checks (minor GC only).
    young_ref: Option<&'a YoungGen>,
    /// Work stack of copied objects whose term slots still need copying.
    /// It replaces native recursion, so nesting depth costs heap, not stack.
    pending: Vec<PendingSlots>,
    /// Maps old address → new address for already-copied objects.
    forwarding: HashMap<usize, usize>,
    /// Raw Arc pointers encountered during live-data traversal.
    live_refc_ptrs: HashSet<u64>,
    /// Set to true if any external term was encountered.
    has_external_terms: bool,
    /// Bytes copied (allocated in major, promoted in minor).
    bytes_copied: usize,
}

impl<'a> GcCopier<'a> {
    /// Create a copier for major GC (fresh arena, no young-gen filter).
    fn new_major_with_hint(bytes_hint: usize) -> Self {
        // Estimate ~3 words (24 bytes) per heap object on average.
        let obj_estimate = bytes_hint / 24;
        GcCopier {
            target: GcTarget::Owned(Bump::new()),
            young_ref: None,
            pending: Vec::new(),
            forwarding: HashMap::with_capacity(obj_estimate),
            live_refc_ptrs: HashSet::new(),
            has_external_terms: false,
            bytes_copied: 0,
        }
    }

    /// Create a copier for minor GC (promote into old gen, skip old-gen terms).
    fn new_minor_with_hint(young_ref: &'a YoungGen, old: &'a mut Bump, young_bytes: usize) -> Self {
        let obj_estimate = young_bytes / 24;
        GcCopier {
            target: GcTarget::Borrowed(old),
            young_ref: Some(young_ref),
            pending: Vec::new(),
            forwarding: HashMap::with_capacity(obj_estimate),
            live_refc_ptrs: HashSet::new(),
            has_external_terms: false,
            bytes_copied: 0,
        }
    }

    /// Allocate `n` words in the target arena.
    fn alloc_words(&mut self, n: usize) -> *mut u64 {
        let layout = Layout::array::<u64>(n).expect("GC allocation capacity overflow");
        let ptr = match &mut self.target {
            GcTarget::Owned(bump) => bump.alloc_layout(layout),
            GcTarget::Borrowed(bump) => bump.alloc_layout(layout),
        };
        self.bytes_copied += layout.size();
        ptr.as_ptr() as *mut u64
    }

    /// Check if a pointer is in the young generation (minor GC only).
    /// Returns `false` for major GC (all pointers need copying).
    #[inline]
    fn is_in_young(&self, ptr: *const u64) -> bool {
        self.young_ref.is_some_and(|y| y.contains(ptr))
    }

    /// Returns `true` if this is a minor GC.
    #[inline]
    fn is_minor(&self) -> bool {
        self.young_ref.is_some()
    }

    /// For old-gen boxed terms during minor GC, track refc binary Arc
    /// pointers so the sweep phase doesn't drop them.
    fn track_old_gen_refc(&mut self, ptr: usize) {
        if self.is_minor() {
            let header_ptr = ptr as *const u64;
            let header = unsafe { BoxedHeader(*header_ptr) };
            if header.subtype() == BOXED_REFC_BINARY {
                let raw_arc_ptr = unsafe { *header_ptr.add(REFC_BINARY_PTR_WORD_INDEX) };
                self.live_refc_ptrs.insert(raw_arc_ptr);
            }
        }
    }

    /// Extract the owned arena (major GC only). Panics if called on minor GC.
    fn into_arena(self) -> Bump {
        match self.target {
            GcTarget::Owned(bump) => bump,
            GcTarget::Borrowed(_) => panic!("into_arena called on minor GC copier"),
        }
    }

    /// Queue the `len` term slots of a freshly allocated copy for copying.
    #[inline]
    fn push_pending(&mut self, dst: *mut u64, src: *const u64, len: usize) {
        if len != 0 {
            self.pending.push(PendingSlots { dst, src, len });
        }
    }

    /// Copy a term and everything it reaches into the target arena.
    ///
    /// Objects are copied depth first, in the order a recursive copy would
    /// use, but the slots still to copy wait on the heap-allocated `pending`
    /// stack rather than in native frames. A list nested hundreds of
    /// thousands of levels deep in head position, or a deeply nested tuple,
    /// map or closure, costs one pending entry per open level instead of a
    /// native stack frame. A proper list needs at most one entry.
    /// Re-entrant: a nested call only drains the entries it pushed.
    fn copy_term(&mut self, term: ErlTerm) -> ErlTerm {
        let base = self.pending.len();
        let copied = self.copy_object(term);
        while self.pending.len() > base {
            let mut slots = self.pending.pop().expect("pending slots above base");
            loop {
                let queued = self.pending.len();
                // SAFETY: `src` is a term word of a live source object, which
                // the collector never modifies; `dst` is the matching word
                // reserved in its copy, written exactly once here.
                unsafe {
                    let word = *slots.src;
                    // Immediates skip `copy_object`.
                    *slots.dst = if is_plain_immediate(word) {
                        word
                    } else {
                        self.copy_object(ErlTerm(word)).0
                    };
                }
                slots.len -= 1;
                if slots.len == 0 {
                    break;
                }
                // SAFETY: slots remain, so both pointers stay inside the
                // source object and its reserved copy.
                unsafe {
                    slots.dst = slots.dst.add(1);
                    slots.src = slots.src.add(1);
                }
                if self.pending.len() != queued {
                    // The slot's object queued its own slots: copy them first,
                    // as recursion would, then this object's remaining slots.
                    let child = self.pending.pop().expect("child slots just queued");
                    self.pending.push(slots);
                    self.pending.push(child);
                    break;
                }
            }
        }
        copied
    }

    /// Copy the cons cells of a list along its tail while their heads are
    /// immediates, without touching `pending`, as most lists and strings
    /// need. The first cell whose head needs copying queues its head and
    /// tail and ends the run, so the head's subtree is copied before the
    /// rest of the list, as recursion would. Returns the relocated list.
    fn copy_list(&mut self, list: ErlTerm) -> ErlTerm {
        let mut first = None;
        let mut prev_tail: *mut u64 = std::ptr::null_mut();
        let mut cursor = list;
        loop {
            let mut next = None;
            let copied = if cursor.tag() != TAG_CONS || cursor.is_external() {
                // The terminator: nil, an improper tail or an external list.
                self.copy_object(cursor)
            } else {
                let old_ptr = (cursor.0 & !(TAG_MASK | EXTERNAL_BIT)) as usize;
                if self.is_minor() && !self.is_in_young(old_ptr as *const u64) {
                    // Minor GC: not in young gen → already in old, kept as-is.
                    cursor
                } else if let Some(&new_addr) = self.forwarding.get(&old_ptr) {
                    // SAFETY: the collector stores only aligned allocation addresses in forwarding words.
                    unsafe { ErlTerm::cons_unchecked(new_addr as *const ConsCell) }
                } else {
                    let new_ptr = self.alloc_words(2);
                    self.forwarding.insert(old_ptr, new_ptr as usize);
                    // SAFETY: `old_ptr` is a live source cons cell (head word
                    // first, then tail); `new_ptr` reserves its two-word copy.
                    unsafe {
                        let old_cell = &*(old_ptr as *const ConsCell);
                        if is_plain_immediate(old_cell.head.0) {
                            *new_ptr = old_cell.head.0;
                            next = Some((new_ptr.add(1), old_cell.tail));
                        } else {
                            self.push_pending(new_ptr, old_ptr as *const u64, 2);
                        }
                    }
                    // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                    unsafe { ErlTerm::cons_unchecked(new_ptr as *const ConsCell) }
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

    /// Copy one object (or a run of list cells, see `copy_list`) into the
    /// target arena, queueing its term slots on `pending` instead of copying
    /// them. Returns the relocated term.
    fn copy_object(&mut self, term: ErlTerm) -> ErlTerm {
        // External terms point into borrowed ETS heaps — preserve as-is.
        if term.is_external() {
            self.has_external_terms = true;
            return term;
        }
        match term.tag() {
            TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL => term,

            TAG_CONS => self.copy_list(term),

            TAG_BOXED => {
                if term.0 == 0 {
                    return term;
                }
                let old_ptr = (term.0 & !(TAG_MASK | EXTERNAL_BIT)) as usize;

                // Minor GC: not in young gen → already in old, return as-is.
                if self.is_minor() && !self.is_in_young(old_ptr as *const u64) {
                    self.track_old_gen_refc(old_ptr);
                    return term;
                }

                // Already forwarded?
                if let Some(&new_addr) = self.forwarding.get(&old_ptr) {
                    // SAFETY: the collector stores only aligned allocation addresses in forwarding words.
                    return unsafe { ErlTerm::boxed_unchecked(new_addr as *const u64) };
                }

                let old_header_ptr = old_ptr as *const u64;
                let header = unsafe { BoxedHeader(*old_header_ptr) };
                let arity = header.arity() as usize;
                let subtype = header.subtype();

                match subtype {
                    BOXED_TUPLE => {
                        let total_words = 1 + arity;
                        let new_ptr = self.alloc_words(total_words);
                        self.forwarding.insert(old_ptr, new_ptr as usize);
                        unsafe { *new_ptr = header.0; }
                        // SAFETY: elements follow the header in both objects.
                        let (dst, src) = unsafe { (new_ptr.add(1), old_header_ptr.add(1)) };
                        self.push_pending(dst, src, arity);
                        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                        unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) }
                    }
                    BOXED_FLOAT => {
                        let new_ptr = self.alloc_words(2);
                        self.forwarding.insert(old_ptr, new_ptr as usize);
                        unsafe {
                            *new_ptr = header.0;
                            *new_ptr.add(1) = *old_header_ptr.add(1);
                        }
                        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                        unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) }
                    }
                    BOXED_BIGINT => {
                        let total_words = bigint_total_words(arity);
                        let new_ptr = self.alloc_words(total_words);
                        self.forwarding.insert(old_ptr, new_ptr as usize);
                        unsafe {
                            std::ptr::copy_nonoverlapping(old_header_ptr, new_ptr, total_words);
                        }
                        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                        unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) }
                    }
                    BOXED_REFERENCE => {
                        let new_ptr = self.alloc_words(REFERENCE_TOTAL_WORDS);
                        self.forwarding.insert(old_ptr, new_ptr as usize);
                        unsafe {
                            std::ptr::copy_nonoverlapping(old_header_ptr, new_ptr, REFERENCE_TOTAL_WORDS);
                        }
                        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                        unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) }
                    }
                    BOXED_MAP => {
                        let total_elems = arity * 2;
                        let total_words = 1 + total_elems;
                        let new_ptr = self.alloc_words(total_words);
                        self.forwarding.insert(old_ptr, new_ptr as usize);
                        unsafe { *new_ptr = header.0; }
                        // SAFETY: keys and values follow the header in both objects.
                        let (dst, src) = unsafe { (new_ptr.add(1), old_header_ptr.add(1)) };
                        self.push_pending(dst, src, total_elems);
                        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                        unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) }
                    }
                    BOXED_BINARY => {
                        // arity = bit count
                        let bit_count = arity;
                        let byte_count = bit_count.div_ceil(8);
                        let data_words = byte_count.div_ceil(8);
                        let total_words = 1 + data_words;
                        let new_ptr = self.alloc_words(total_words);
                        self.forwarding.insert(old_ptr, new_ptr as usize);
                        unsafe {
                            std::ptr::copy_nonoverlapping(old_header_ptr, new_ptr, total_words);
                        }
                        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                        unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) }
                    }
                    BOXED_CLOSURE => {
                        let env_count = arity;
                        let total_words = 3 + env_count;
                        let new_ptr = self.alloc_words(total_words);
                        self.forwarding.insert(old_ptr, new_ptr as usize);
                        unsafe {
                            *new_ptr = header.0;
                            *new_ptr.add(1) = *old_header_ptr.add(1); // fn_ptr
                            *new_ptr.add(2) = *old_header_ptr.add(2); // arity
                        }
                        // SAFETY: the environment follows the three fixed words in both objects.
                        let (dst, src) = unsafe { (new_ptr.add(3), old_header_ptr.add(3)) };
                        self.push_pending(dst, src, env_count);
                        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                        unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) }
                    }
                    BOXED_REFC_BINARY => {
                        let total_words = REFC_BINARY_TOTAL_WORDS;
                        let new_ptr = self.alloc_words(total_words);
                        self.forwarding.insert(old_ptr, new_ptr as usize);
                        unsafe {
                            std::ptr::copy_nonoverlapping(old_header_ptr, new_ptr, total_words);
                        }
                        let raw_arc_ptr = unsafe { *old_header_ptr.add(REFC_BINARY_PTR_WORD_INDEX) };
                        self.live_refc_ptrs.insert(raw_arc_ptr);
                        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                        unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) }
                    }
                    BOXED_MATCH_CTX => {
                        let new_ptr = self.alloc_words(MATCH_CTX_TOTAL_WORDS);
                        self.forwarding.insert(old_ptr, new_ptr as usize);
                        unsafe {
                            let context = crate::match_context::MatchContext::view(term).unwrap();
                            let binary = self.copy_term(context.binary());
                            new_ptr.cast::<crate::match_context::MatchContext>().write(context.relocated(binary));
                            ErlTerm::boxed_unchecked(new_ptr)
                        }
                    }
                    _ => {
                        let total_words = 1 + arity;
                        let new_ptr = self.alloc_words(total_words);
                        self.forwarding.insert(old_ptr, new_ptr as usize);
                        unsafe {
                            std::ptr::copy_nonoverlapping(old_header_ptr, new_ptr, total_words);
                        }
                        // SAFETY: the allocation reserves u64-aligned words (and ConsCell alignment is 8).
                        unsafe { ErlTerm::boxed_unchecked(new_ptr as *const u64) }
                    }
                }
            }
            _ => term,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crate::term::{
        BoxedHeader, BIGINT_FIRST_LIMB_WORD_INDEX, BIGINT_SIGN_NEGATIVE, BIGINT_SIGN_WORD_INDEX,
        BOXED_BIGINT, BOXED_TUPLE,
    };

    #[test]
    fn fragment_adoption_moves_storage_and_binary_owners_without_copying() {
        let mut source = ProcessHeap::new();
        let data = Arc::new(BinaryData::new(vec![7; 256]));
        let weak = Arc::downgrade(&data);
        let binary = source.alloc_refc_binary_from_arc(data, 0, 256);
        let graph = source.alloc_tuple(&[binary, binary]);
        let mut roots = [graph];
        // SAFETY: complete, current roots owned by the exclusively accessed heap.
        unsafe { source.collect_full(&mut roots); }
        let mut fragment = FragmentHeap::new_with_capacity(1);
        // SAFETY: the updated source graph belongs to the still-live source heap.
        let copied = fragment.copy_term(unsafe { roots[0].borrow(&source) });
        let address = copied.as_boxed_ptr().unwrap();
        drop(source);
        assert_eq!(weak.strong_count(), 1, "fragment must own one binary reference");
        let mut target = ProcessHeap::new();
        let existing = target.alloc_float(42.0);
        let before = target.bytes_allocated();
        target.adopt_fragment(fragment);
        assert_eq!(copied.as_boxed_ptr().unwrap(), address);
        assert_eq!(weak.strong_count(), 1, "adoption added an Arc clone");
        assert_eq!(target.bytes_allocated(), before + 48);
        // SAFETY: adoption transferred the complete initialized graph and owners.
        unsafe {
            let first = ErlTerm(*address.add(1));
            let second = ErlTerm(*address.add(2));
            assert_eq!(first, second, "source sharing was lost");
            assert_eq!(first.borrow(&target).binary_bytes(), Some(&vec![7; 256][..]));
            let mut roots = [copied, existing];
            target.collect_full(&mut roots);
            let binary = ErlTerm(*roots[0].as_boxed_ptr().unwrap().add(1));
            assert_eq!(binary.borrow(&target).binary_bytes(), Some(&vec![7; 256][..]));
        }
        assert_eq!(weak.strong_count(), 1);
        drop(target);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn fragment_adoption_owns_binary_allocations_across_overflow_chunks() {
        let mut fragment = FragmentHeap::new();
        let binary = fragment.alloc_binary_auto(&[9; 256]);
        for _ in 0..200 {
            fragment.alloc_tuple(&[binary; 4]);
        }
        let root = fragment.alloc_tuple(&[binary]);
        let mut target = ProcessHeap::new();
        target.adopt_fragment(fragment);
        // SAFETY: all allocations were created in the consumed fragment heap.
        unsafe {
            let binary = ErlTerm(*root.as_boxed_ptr().unwrap().add(1));
            assert_eq!(binary.borrow(&target).binary_bytes(), Some(&vec![9; 256][..]));
            target.collect_full(&mut [root]);
        }
    }

    #[test]
    fn binary_size_rejection_preserves_heap() {
        let mut heap = ProcessHeap::new();
        let root = heap.alloc_binary(b"retained");
        let before = heap.bytes_allocated();
        let mut requests = vec![usize::MAX];
        if let Ok(size) = usize::try_from((u64::MAX >> crate::term::BOXED_TAG_BITS) / 8 + 1) {
            requests.push(size);
        }
        let max = ((u64::MAX >> crate::term::BOXED_TAG_BITS) / 8)
            .min(((isize::MAX as u64) / 8 - 1) * 8) as usize;
        let (header, words, _) = ProcessHeap::binary_layout(max);
        assert_eq!(header.arity(), (max as u64) * 8);
        assert!(std::alloc::Layout::array::<u64>(words).is_ok());
        requests.push(max + 1);
        for size in requests {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| heap.alloc_binary_zeroed(size)));
            assert!(result.is_err());
            assert_eq!(heap.bytes_allocated(), before);
            // SAFETY: the rejected allocation cannot invalidate the live root.
            assert_eq!(unsafe { root.borrow(&heap) }.binary_bytes(), Some(&b"retained"[..]));
        }
        let (header, words, padded) = ProcessHeap::binary_layout(9);
        assert_eq!((header.arity(), words, padded), (72, 3, 16));
        assert_eq!(ProcessHeap::binary_layout(0).1, 1);
    }

    #[test]
    fn oversized_process_request_preserves_existing_allocation() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_cons(ErlTerm::small_int(7), ErlTerm::nil());
        let bytes = heap.bytes_allocated();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            heap.alloc_words(usize::MAX);
        })).is_err());
        assert_eq!(heap.bytes_allocated(), bytes);
        // SAFETY: failed reservation did not collect or invalidate the cons.
        assert_eq!(unsafe { (*term.as_cons_ptr().unwrap()).head }.as_small_int(), Some(7));
        heap.set_max_heap_size(usize::MAX / 8 + 1);
        assert!(!heap.exceeds_max_heap_size());
    }

    #[test]
    fn chunk_rejects_wrapping_request_without_advancing() {
        let mut chunk = BumpChunk::new(16);
        chunk.alloc_words(1).unwrap();
        assert!(chunk.alloc_words(usize::MAX).is_none());
        assert_eq!(chunk.cursor, 1);
        assert!(chunk.alloc_words(15).is_some());
        assert!(chunk.alloc_words(1).is_none());
    }

    #[test]
    fn gc_rejects_wrapping_word_layout() {
        let mut copier = GcCopier::new_major_with_hint(0);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            copier.alloc_words(usize::MAX / 8 + 1);
        })).is_err());
        assert_eq!(copier.bytes_copied, 0);
    }

    #[test]
    fn alloc_cons_and_read() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_cons(ErlTerm::small_int(1), ErlTerm::nil());
        assert!(term.is_cons());
        unsafe {
            let cell = &*term.as_cons_ptr().unwrap();
            assert_eq!(cell.head.as_small_int(), Some(1));
            assert!(cell.tail.is_nil());
        }
    }

    #[test]
    fn alloc_list_from_slice() {
        let mut heap = ProcessHeap::new();
        let elems = [
            ErlTerm::small_int(10),
            ErlTerm::small_int(20),
            ErlTerm::small_int(30),
        ];
        let list = heap.alloc_list_from_slice(&elems);

        // Walk the list and collect values.
        let mut values = vec![];
        let mut current = list;
        while current.is_cons() {
            unsafe {
                let cell = &*current.as_cons_ptr().unwrap();
                values.push(cell.head.as_small_int().unwrap());
                current = cell.tail;
            }
        }
        assert!(current.is_nil());
        assert_eq!(values, vec![10, 20, 30]);
    }

    #[test]
    fn alloc_tuple_and_read() {
        let mut heap = ProcessHeap::new();
        let elems = [ErlTerm::small_int(7), ErlTerm::atom(0)];
        let term = heap.alloc_tuple(&elems);
        assert!(term.is_boxed());
        unsafe {
            let hdr_ptr = term.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*hdr_ptr);
            assert_eq!(header.subtype(), BOXED_TUPLE);
            assert_eq!(header.arity(), 2);
            assert_eq!(ErlTerm(*hdr_ptr.add(1)).as_small_int(), Some(7));
            assert_eq!(ErlTerm(*hdr_ptr.add(2)).as_atom_id(), Some(0));
        }
    }

    #[test]
    fn alloc_float_and_read() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_float(3.25);
        assert!(term.is_boxed());
        unsafe {
            let hdr_ptr = term.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*hdr_ptr);
            assert_eq!(header.subtype(), crate::term::BOXED_FLOAT);
            assert_eq!(header.arity(), 1);
            let bits = *hdr_ptr.add(1);
            assert_eq!(f64::from_bits(bits), 3.25);
        }
    }

    #[test]
    fn gc_preserves_list() {
        let mut heap = ProcessHeap::new();
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(1),
            ErlTerm::small_int(2),
            ErlTerm::small_int(3),
        ]);

        let mut roots = [list];
        unsafe { heap.collect(&mut roots) };

        // Walk the list after GC.
        let mut values = vec![];
        let mut current = roots[0];
        while current.is_cons() {
            unsafe {
                let cell = &*current.as_cons_ptr().unwrap();
                values.push(cell.head.as_small_int().unwrap());
                current = cell.tail;
            }
        }
        assert!(current.is_nil());
        assert_eq!(values, vec![1, 2, 3]);
    }

    #[test]
    fn gc_preserves_tuple() {
        let mut heap = ProcessHeap::new();
        let inner_list =
            heap.alloc_list_from_slice(&[ErlTerm::small_int(10), ErlTerm::small_int(20)]);
        let tuple = heap.alloc_tuple(&[ErlTerm::atom(0), inner_list]);

        let mut roots = [tuple];
        unsafe { heap.collect(&mut roots) };

        // Verify tuple after GC.
        unsafe {
            let hdr_ptr = roots[0].as_boxed_ptr().unwrap();
            let header = BoxedHeader(*hdr_ptr);
            assert_eq!(header.subtype(), BOXED_TUPLE);
            assert_eq!(header.arity(), 2);
            assert_eq!(ErlTerm(*hdr_ptr.add(1)).as_atom_id(), Some(0));

            // Verify inner list.
            let list_term = ErlTerm(*hdr_ptr.add(2));
            assert!(list_term.is_cons());
            let cell = &*list_term.as_cons_ptr().unwrap();
            assert_eq!(cell.head.as_small_int(), Some(10));
        }
    }

    #[test]
    fn gc_reduces_tracked_bytes() {
        let mut heap = ProcessHeap::new();
        // Allocate a bunch of data, only keep roots to a small portion.
        for _ in 0..100 {
            heap.alloc_list_from_slice(&[
                ErlTerm::small_int(1),
                ErlTerm::small_int(2),
                ErlTerm::small_int(3),
            ]);
        }
        let keep = heap.alloc_cons(ErlTerm::small_int(42), ErlTerm::nil());
        let before = heap.bytes_allocated();

        let mut roots = [keep];
        unsafe { heap.collect(&mut roots) };

        let after = heap.bytes_allocated();
        assert!(after < before, "GC should have freed unreachable data");

        // Verify the kept data is intact.
        unsafe {
            let cell = &*roots[0].as_cons_ptr().unwrap();
            assert_eq!(cell.head.as_small_int(), Some(42));
            assert!(cell.tail.is_nil());
        }
    }

    #[test]
    fn gc_preserves_float() {
        let mut heap = ProcessHeap::new();
        let f = heap.alloc_float(2.625);
        let mut roots = [f];
        unsafe { heap.collect(&mut roots) };

        unsafe {
            let hdr_ptr = roots[0].as_boxed_ptr().unwrap();
            let bits = *hdr_ptr.add(1);
            assert_eq!(f64::from_bits(bits), 2.625);
        }
    }

    #[test]
    fn alloc_binary_and_read() {
        let mut heap = ProcessHeap::new();
        let data = &[1u8, 2, 3, 4, 5];
        let term = heap.alloc_binary(data);
        assert!(term.is_boxed());
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(term.is_binary());
            assert_eq!(term.binary_size(), Some(5));
        }
        unsafe {
            let bytes = term.binary_bytes().unwrap();
            assert_eq!(bytes, &[1, 2, 3, 4, 5]);
        }
    }

    #[test]
    fn alloc_empty_binary() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_binary(&[]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(term.is_binary());
            assert_eq!(term.binary_size(), Some(0));
        }
        unsafe {
            let bytes = term.binary_bytes().unwrap();
            assert_eq!(bytes, &[]);
        }
    }

    #[test]
    fn alloc_binary_zeroed_and_fill() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_binary_zeroed(3);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(term.is_binary());
            assert_eq!(term.binary_size(), Some(3));
        }

        // Write some bytes into the binary
        unsafe {
            let hdr_ptr = term.as_boxed_ptr().unwrap() as *mut u64;
            let data_ptr = hdr_ptr.add(1) as *mut u8;
            *data_ptr = 10;
            *data_ptr.add(1) = 20;
            *data_ptr.add(2) = 30;

            let bytes = term.binary_bytes().unwrap();
            assert_eq!(bytes, &[10, 20, 30]);
        }
    }

    #[test]
    fn gc_preserves_binary() {
        let mut heap = ProcessHeap::new();
        let data = &[0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE];
        let bin = heap.alloc_binary(data);

        // Allocate some garbage to trigger GC benefit
        for _ in 0..50 {
            heap.alloc_cons(ErlTerm::small_int(0), ErlTerm::nil());
        }

        let mut roots = [bin];
        unsafe { heap.collect(&mut roots) };

        // Verify binary is intact after GC
        let after = roots[0];
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(after.is_binary());
            assert_eq!(after.binary_size(), Some(6));
        }
        unsafe {
            let bytes = after.binary_bytes().unwrap();
            assert_eq!(bytes, &[0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE]);
        }
    }

    #[test]
    fn gc_preserves_binary_in_tuple() {
        let mut heap = ProcessHeap::new();
        let bin = heap.alloc_binary(&[1, 2, 3]);
        let tuple = heap.alloc_tuple(&[ErlTerm::small_int(42), bin]);

        let mut roots = [tuple];
        unsafe { heap.collect(&mut roots) };

        // Verify tuple and contained binary survive GC
        unsafe {
            let hdr_ptr = roots[0].as_boxed_ptr().unwrap();
            let header = BoxedHeader(*hdr_ptr);
            assert_eq!(header.subtype(), BOXED_TUPLE);
            assert_eq!(header.arity(), 2);
            assert_eq!(ErlTerm(*hdr_ptr.add(1)).as_small_int(), Some(42));

            let bin_after = ErlTerm(*hdr_ptr.add(2));
            assert!(bin_after.is_binary());
            assert_eq!(bin_after.binary_bytes().unwrap(), &[1, 2, 3]);
        }
    }

    #[test]
    fn gc_preserves_reserved_bigint_layout() {
        let mut heap = ProcessHeap::new();
        let limb_count = 3usize;
        let total_words = crate::term::bigint_total_words(limb_count);
        let ptr = heap.alloc_words(total_words);
        unsafe {
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::BigInt, limb_count as u64).0;
            *ptr.add(BIGINT_SIGN_WORD_INDEX) = BIGINT_SIGN_NEGATIVE;
            *ptr.add(BIGINT_FIRST_LIMB_WORD_INDEX) = 0xAA;
            *ptr.add(BIGINT_FIRST_LIMB_WORD_INDEX + 1) = 0xBB;
            *ptr.add(BIGINT_FIRST_LIMB_WORD_INDEX + 2) = 0xCC;
        }
        let term = ErlTerm::boxed(ptr as *const u64);
        let mut roots = [term];
        unsafe { heap.collect(&mut roots) };

        unsafe {
            let after_ptr = roots[0].as_boxed_ptr().unwrap();
            let header = BoxedHeader(*after_ptr);
            assert_eq!(header.subtype(), BOXED_BIGINT);
            assert_eq!(header.arity(), limb_count as u64);
            assert_eq!(*after_ptr.add(BIGINT_SIGN_WORD_INDEX), BIGINT_SIGN_NEGATIVE);
            assert_eq!(*after_ptr.add(BIGINT_FIRST_LIMB_WORD_INDEX), 0xAA);
            assert_eq!(*after_ptr.add(BIGINT_FIRST_LIMB_WORD_INDEX + 1), 0xBB);
            assert_eq!(*after_ptr.add(BIGINT_FIRST_LIMB_WORD_INDEX + 2), 0xCC);
        }
    }

    #[test]
    fn refc_binary_rejects_invalid_window_before_allocation() {
        let mut heap = ProcessHeap::new();
        let bytes_before = heap.bytes_allocated();
        for (offset, len) in [(4, 1), (usize::MAX, 2), (1, usize::MAX)] {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                heap.alloc_refc_binary_from_arc(Arc::new(BinaryData::new(vec![0; 4])), offset, len);
            }));
            assert!(result.is_err(), "invalid window ({offset}, {len}) accepted");
            assert_eq!(heap.bytes_allocated(), bytes_before);
            assert!(heap.refc_binaries.is_empty());
        }
    }

    #[test]
    fn gc_preserves_refc_binary_descriptor() {
        let mut heap = ProcessHeap::new();
        let data: Vec<u8> = (0..100).collect();
        let arc = Arc::new(BinaryData::new(data));
        let term = heap.alloc_refc_binary_from_arc(arc.clone(), 0, 100);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(term.is_refc_binary());
            assert_eq!(term.binary_size(), Some(100));
        }

        // Allocate garbage
        for _ in 0..50 {
            heap.alloc_cons(ErlTerm::small_int(0), ErlTerm::nil());
        }

        let mut roots = [term];
        unsafe { heap.collect(&mut roots) };

        let after = roots[0];
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(after.is_refc_binary());
            assert!(after.is_binary());
            assert_eq!(after.binary_size(), Some(100));
        }
        unsafe {
            let bytes = after.refc_binary_bytes().unwrap();
            assert_eq!(bytes[0], 0);
            assert_eq!(bytes[99], 99);
        }
        // Arc should still be alive after GC
        assert_eq!(heap.refc_binaries.len(), 1);
    }

    #[test]
    fn map_pairs_validate_without_copying_or_heap_mutation() {
        let mut heap = ProcessHeap::new();
        let terms: Vec<_> = (0..6).map(ErlTerm::small_int).collect();
        let before = heap.bytes_allocated();
        for odd in [1, 3, 5] {
            assert!(MapPairs::from_flat(&terms[..odd]).is_none());
            assert_eq!(heap.bytes_allocated(), before);
        }
        let view = MapPairs::from_flat(&terms).unwrap();
        assert_eq!(view.terms.as_ptr(), terms.as_ptr());
        assert_eq!(view.len(), 3);
        assert!(!view.is_empty());
        assert!(MapPairs::new(&[]).is_empty());
        assert!(MapPairs::from_flat(&[]).unwrap().is_empty());
        let root = heap.alloc_map(view);
        assert_eq!(heap.bytes_allocated(), before + 7 * 8);
        // SAFETY: root is freshly allocated, initialized, and owned by heap.
        unsafe {
            let ptr = root.as_boxed_ptr().unwrap();
            assert_eq!(BoxedHeader(*ptr).arity(), 3);
            for (i, term) in terms.iter().enumerate() {
                assert_eq!(*ptr.add(i + 1), term.0);
            }
        }
    }

    #[test]
    fn map_pairs_typed_entries_preserve_binary_and_list_roots() {
        let mut heap = ProcessHeap::new();
        let key = heap.alloc_binary_auto(&[7; 256]);
        let list = heap.alloc_list_from_slice(&[ErlTerm::small_int(42)]);
        let entries = [[key, list], [ErlTerm::atom(1), ErlTerm::small_int(99)]];
        let view = MapPairs::new(&entries);
        assert_eq!(view.terms.as_ptr(), entries.as_ptr().cast());
        let root = heap.alloc_map(view);
        let mut roots = [root];
        // SAFETY: the complete graph is owned by this exclusively borrowed heap.
        unsafe {
            heap.collect_full(&mut roots);
            let ptr = roots[0].as_boxed_ptr().unwrap();
            assert_eq!(BoxedHeader(*ptr).arity(), 2);
            assert_eq!(
                ErlTerm(*ptr.add(1)).borrow(&heap).binary_bytes(),
                Some(&[7; 256][..])
            );
            let cell = &*ErlTerm(*ptr.add(2)).as_cons_ptr().unwrap();
            assert_eq!(cell.head, ErlTerm::small_int(42));
            assert_eq!(cell.tail, ErlTerm::nil());
            assert_eq!(ErlTerm(*ptr.add(4)), ErlTerm::small_int(99));
        }
    }

    #[test]
    fn alloc_map_and_read() {
        let mut heap = ProcessHeap::new();
        // Map: #{1 => 10, 2 => 20}
        let pairs = [
            ErlTerm::small_int(1),
            ErlTerm::small_int(10),
            ErlTerm::small_int(2),
            ErlTerm::small_int(20),
        ];
        let term = heap.alloc_map(MapPairs::from_flat(&pairs).expect("complete key/value pairs"));
        assert!(term.is_boxed());
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { term.is_map() });
        unsafe {
            let hdr_ptr = term.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*hdr_ptr);
            assert_eq!(header.subtype(), crate::term::BOXED_MAP);
            assert_eq!(header.arity(), 2); // 2 pairs
                                           // k1=1, v1=10, k2=2, v2=20
            assert_eq!(ErlTerm(*hdr_ptr.add(1)).as_small_int(), Some(1));
            assert_eq!(ErlTerm(*hdr_ptr.add(2)).as_small_int(), Some(10));
            assert_eq!(ErlTerm(*hdr_ptr.add(3)).as_small_int(), Some(2));
            assert_eq!(ErlTerm(*hdr_ptr.add(4)).as_small_int(), Some(20));
        }
    }

    #[test]
    fn alloc_empty_map() {
        let mut heap = ProcessHeap::new();
        let term = heap.alloc_map(MapPairs::new(&[]));
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { term.is_map() });
        unsafe {
            let hdr_ptr = term.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*hdr_ptr);
            assert_eq!(header.subtype(), crate::term::BOXED_MAP);
            assert_eq!(header.arity(), 0);
        }
    }

    #[test]
    fn gc_preserves_map() {
        let mut heap = ProcessHeap::new();
        let inner_list =
            heap.alloc_list_from_slice(&[ErlTerm::small_int(10), ErlTerm::small_int(20)]);
        // Map with a list value: #{42 => [10, 20]}
        let map = heap.alloc_map(MapPairs::from_flat(&[ErlTerm::small_int(42), inner_list]).expect("complete key/value pairs"));

        // Allocate garbage
        for _ in 0..50 {
            heap.alloc_cons(ErlTerm::small_int(0), ErlTerm::nil());
        }

        let mut roots = [map];
        unsafe { heap.collect(&mut roots) };

        // Verify map after GC
        let after = roots[0];
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { after.is_map() });
        unsafe {
            let hdr_ptr = after.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*hdr_ptr);
            assert_eq!(header.arity(), 1); // 1 pair
            assert_eq!(ErlTerm(*hdr_ptr.add(1)).as_small_int(), Some(42)); // key

            // value should be a list [10, 20]
            let val = ErlTerm(*hdr_ptr.add(2));
            assert!(val.is_cons());
            let cell = &*val.as_cons_ptr().unwrap();
            assert_eq!(cell.head.as_small_int(), Some(10));
        }
    }

    #[test]
    fn gc_preserves_reference() {
        let mut heap = ProcessHeap::new();

        // Allocate a reference: [header(REFERENCE, 0), unique_id]
        let ptr = heap.alloc_words(REFERENCE_TOTAL_WORDS);
        unsafe {
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Reference, 0).0;
            *ptr.add(1) = 42_u64; // unique_id
        }
        let ref_term = ErlTerm::boxed(ptr as *const u64);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { ref_term.is_reference() });

        // Allocate garbage
        for _ in 0..50 {
            heap.alloc_cons(ErlTerm::small_int(0), ErlTerm::nil());
        }

        let mut roots = [ref_term];
        unsafe { heap.collect(&mut roots) };

        // Verify reference survived GC
        let after = roots[0];
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { after.is_reference() });
        unsafe {
            let hdr_ptr = after.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*hdr_ptr);
            assert_eq!(header.subtype(), BOXED_REFERENCE);
            assert_eq!(*hdr_ptr.add(1), 42_u64); // unique_id preserved
        }
    }

    #[test]
    fn needs_gc_threshold() {
        let mut heap = ProcessHeap::new();
        assert!(!heap.needs_gc());
        // Allocate many small chunks in the young gen past the threshold.
        let chunk = 64; // 64 words = 512 bytes
        let chunks_needed = DEFAULT_GC_THRESHOLD / (chunk * 8) + 1;
        for _ in 0..chunks_needed {
            heap.alloc_words(chunk);
        }
        assert!(heap.needs_gc());
    }

    // -----------------------------------------------------------------------
    // Refc binary tests
    // -----------------------------------------------------------------------

    #[test]
    fn alloc_binary_auto_small_is_heap_binary() {
        let mut heap = ProcessHeap::new();
        let data = vec![42u8; 64]; // exactly at threshold
        let term = heap.alloc_binary_auto(&data);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(term.is_heap_binary());
            assert!(!term.is_refc_binary());
            assert_eq!(term.binary_size(), Some(64));
        }
        assert!(heap.refc_binaries.is_empty());
    }

    #[test]
    fn alloc_binary_auto_large_is_refc_binary() {
        let mut heap = ProcessHeap::new();
        let data: Vec<u8> = (0..65).collect(); // one byte over threshold
        let term = heap.alloc_binary_auto(&data);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(term.is_refc_binary());
            assert!(!term.is_heap_binary());
            assert_eq!(term.binary_size(), Some(65));
        }
        assert_eq!(heap.refc_binaries.len(), 1);
        unsafe {
            let bytes = term.refc_binary_bytes().unwrap();
            assert_eq!(bytes, &data[..]);
        }
    }

    #[test]
    fn alloc_refc_binary_with_offset() {
        let mut heap = ProcessHeap::new();
        let data: Vec<u8> = (0..200).collect();
        let arc = Arc::new(BinaryData::new(data));
        // Sub-binary: offset 50, length 100
        let term = heap.alloc_refc_binary_from_arc(arc, 50, 100);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(term.is_refc_binary());
            assert_eq!(term.binary_size(), Some(100));
        }
        unsafe {
            let bytes = term.refc_binary_bytes().unwrap();
            assert_eq!(bytes[0], 50);
            assert_eq!(bytes[99], 149);
        }
    }

    #[test]
    fn binary_bytes_any_works_for_both_types() {
        let mut heap = ProcessHeap::new();

        // Heap binary
        let hb = heap.alloc_binary(&[1, 2, 3]);
        unsafe {
            let bytes = hb.binary_bytes_any().unwrap();
            assert_eq!(bytes, &[1, 2, 3]);
        }

        // Refc binary
        let data: Vec<u8> = (0..100).collect();
        let arc = Arc::new(BinaryData::new(data));
        let rb = heap.alloc_refc_binary_from_arc(arc, 10, 20);
        unsafe {
            let bytes = rb.binary_bytes_any().unwrap();
            assert_eq!(bytes.len(), 20);
            assert_eq!(bytes[0], 10);
            assert_eq!(bytes[19], 29);
        }
    }

    #[test]
    fn binary_header_stores_bit_count() {
        let mut heap = ProcessHeap::new();

        // Byte-aligned binary: 5 bytes = 40 bits
        let bin = heap.alloc_binary(&[1, 2, 3, 4, 5]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert_eq!(bin.binary_bit_size(), Some(40));
            assert_eq!(bin.binary_size(), Some(5));
            assert!(bin.is_binary());
            assert!(bin.is_bitstring());
        }

        // Verify header arity is bits
        unsafe {
            let ptr = bin.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*ptr);
            assert_eq!(header.arity(), 40);
        }
    }

    #[test]
    fn binary_zeroed_header_stores_bit_count() {
        let mut heap = ProcessHeap::new();
        let bin = heap.alloc_binary_zeroed(10);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert_eq!(bin.binary_bit_size(), Some(80));
            assert_eq!(bin.binary_size(), Some(10));
        }
    }

    #[test]
    fn refc_binary_header_stores_bit_count() {
        let mut heap = ProcessHeap::new();
        let data = Arc::new(BinaryData::new(vec![0u8; 100]));
        let bin = heap.alloc_refc_binary_from_arc(data, 0, 100);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert_eq!(bin.binary_bit_size(), Some(800));
            assert_eq!(bin.binary_size(), Some(100));
            assert!(bin.is_binary());
            assert!(bin.is_bitstring());
        }
    }

    #[test]
    fn sub_byte_binary_header() {
        let mut heap = ProcessHeap::new();
        // Allocate 1 byte, then patch header to 3 bits (simulating <<5:3>>)
        let bin = heap.alloc_binary(&[0xA0]); // high bits
        unsafe {
            let ptr = bin.as_boxed_ptr().unwrap() as *mut u64;
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::HeapBinary, 3).0; // 3 bits
        }
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert_eq!(bin.binary_bit_size(), Some(3));
            assert_eq!(bin.binary_size(), Some(1)); // ceil(3/8) = 1
            assert!(!bin.is_binary());   // not byte-aligned
            assert!(bin.is_bitstring()); // is a bitstring
        }
    }

    #[test]
    fn gc_preserves_sub_byte_binary() {
        let mut heap = ProcessHeap::new();
        let bin = heap.alloc_binary(&[0xA0]);
        unsafe {
            let ptr = bin.as_boxed_ptr().unwrap() as *mut u64;
            *ptr = BoxedHeader::new(crate::term::BoxedSubtype::HeapBinary, 3).0; // 3 bits
        }
        let mut roots = [bin];
        unsafe { heap.collect(&mut roots) };
        let new_bin = roots[0];
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert_eq!(new_bin.binary_bit_size(), Some(3));
            assert_eq!(new_bin.binary_size(), Some(1));
        }
        unsafe {
            let bytes = new_bin.binary_bytes().unwrap();
            assert_eq!(bytes[0], 0xA0);
        }
    }

    #[test]
    fn gc_sweeps_stale_refc_binaries() {
        let mut heap = ProcessHeap::new();
        let data1: Vec<u8> = vec![1; 100];
        let data2: Vec<u8> = vec![2; 100];
        let arc1 = Arc::new(BinaryData::new(data1));
        let arc2 = Arc::new(BinaryData::new(data2));
        let arc1_clone = arc1.clone();
        let arc2_clone = arc2.clone();

        let _term1 = heap.alloc_refc_binary_from_arc(arc1, 0, 100);
        let term2 = heap.alloc_refc_binary_from_arc(arc2, 0, 100);
        assert_eq!(heap.refc_binaries.len(), 2);

        // Only keep term2 as a root — term1 becomes garbage
        let mut roots = [term2];
        unsafe { heap.collect(&mut roots) };

        // Only arc2 should survive
        assert_eq!(heap.refc_binaries.len(), 1);
        // arc1_clone should be the last remaining reference (strong_count == 1)
        assert_eq!(Arc::strong_count(&arc1_clone), 1);
        // arc2_clone + the heap's copy = 2
        assert_eq!(Arc::strong_count(&arc2_clone), 2);
    }

    #[test]
    fn gc_preserves_multiple_descriptors_sharing_one_arc() {
        let mut heap = ProcessHeap::new();
        let data: Vec<u8> = (0..200).collect();
        let arc = Arc::new(BinaryData::new(data));

        // Two sub-binaries sharing the same Arc
        let sub1 = heap.alloc_refc_binary_from_arc(arc.clone(), 0, 100);
        let sub2 = heap.alloc_refc_binary_from_arc(arc.clone(), 100, 100);
        assert_eq!(heap.refc_binaries.len(), 2);
        // arc (original) + 2 in heap + sub1/sub2 clones passed in = 3 total
        // Actually: arc (local) + heap[0] + heap[1] = 3
        assert_eq!(Arc::strong_count(&arc), 3);

        let mut roots = [sub1, sub2];
        unsafe { heap.collect(&mut roots) };

        // Both should survive GC
        assert_eq!(heap.refc_binaries.len(), 2);
        unsafe {
            let bytes1 = roots[0].refc_binary_bytes().unwrap();
            assert_eq!(bytes1[0], 0);
            assert_eq!(bytes1[99], 99);
            let bytes2 = roots[1].refc_binary_bytes().unwrap();
            assert_eq!(bytes2[0], 100);
            assert_eq!(bytes2[99], 199);
        }
    }

    // -----------------------------------------------------------------------
    // Binary virtual heap tests (Phase 2.5)
    // -----------------------------------------------------------------------

    #[test]
    fn binary_vheap_triggers_gc() {
        let mut heap = ProcessHeap::new();
        // On-heap allocation is far below the 256 KiB threshold.
        // But accumulating refc binaries should push binary_vheap_size
        // past binary_vheap_threshold and trigger needs_gc().
        assert!(!heap.needs_gc());

        // Each refc binary charges REFC_BINARY_VHEAP_OVERHEAD (80 bytes).
        // We need > 256 KiB => 256*1024/80 + 1 = 3277 binaries.
        let bin_size = 10 * 1024;
        let count = DEFAULT_BINARY_VHEAP_THRESHOLD / REFC_BINARY_VHEAP_OVERHEAD + 1;
        let mut last_term = ErlTerm::nil();
        for _ in 0..count {
            let data = Arc::new(BinaryData::new(vec![0u8; bin_size]));
            last_term = heap.alloc_refc_binary_from_arc(data, 0, bin_size);
        }

        // But binary_vheap_size should exceed the binary vheap threshold.
        assert!(heap.binary_vheap_size >= DEFAULT_BINARY_VHEAP_THRESHOLD,
            "binary_vheap_size ({}) should be >= threshold ({})",
            heap.binary_vheap_size, DEFAULT_BINARY_VHEAP_THRESHOLD);
        assert!(heap.needs_gc());

        // After GC with only last_term as root, needs_gc() should be false
        // because the threshold doubles and most binaries were swept.
        let mut roots = [last_term];
        unsafe { heap.collect(&mut roots) };
        // Only 1 root kept, so most binaries were swept.
        assert_eq!(heap.refc_binaries.len(), 1);
        assert_eq!(heap.binary_vheap_size, REFC_BINARY_VHEAP_OVERHEAD);
        assert!(!heap.needs_gc());
    }

    #[test]
    fn binary_vheap_threshold_grows_with_live_binaries() {
        let mut heap = ProcessHeap::new();

        // Allocate 2× default threshold worth of vheap overhead and keep
        // all binaries alive.  count = 2 * 256K / 80 = 6554.
        let bin_size = 1024;
        let count = DEFAULT_BINARY_VHEAP_THRESHOLD * 2 / REFC_BINARY_VHEAP_OVERHEAD;
        let mut terms = Vec::new();
        for _ in 0..count {
            let data = Arc::new(BinaryData::new(vec![0u8; bin_size]));
            terms.push(heap.alloc_refc_binary_from_arc(data, 0, bin_size));
        }
        assert!(heap.needs_gc());

        // GC with all terms as roots — everything survives.
        unsafe { heap.collect(&mut terms) };

        // All binaries survived.
        assert_eq!(heap.refc_binaries.len(), count);
        // binary_vheap_size should equal count * overhead.
        let expected_vheap = count * REFC_BINARY_VHEAP_OVERHEAD;
        assert_eq!(heap.binary_vheap_size, expected_vheap);
        // Threshold should have grown to at least 2× the live size.
        assert!(heap.binary_vheap_threshold >= expected_vheap * 2,
            "threshold ({}) should be >= 2 * live size ({})",
            heap.binary_vheap_threshold, expected_vheap * 2);
        // With the grown threshold, needs_gc should be false.
        assert!(!heap.needs_gc());
    }

    #[test]
    fn binary_vheap_shrinks_after_sweep() {
        let mut heap = ProcessHeap::new();

        // Allocate many refc binaries but only keep one as a root.
        let bin_size = 10 * 1024; // 10 KiB each
        let count = 50;
        let mut last_term = ErlTerm::nil();
        for i in 0..count {
            let data = Arc::new(BinaryData::new(vec![i as u8; bin_size]));
            last_term = heap.alloc_refc_binary_from_arc(data, 0, bin_size);
        }

        let vheap_before = heap.binary_vheap_size;
        assert_eq!(vheap_before, count * REFC_BINARY_VHEAP_OVERHEAD);

        // Only keep the last binary.
        let mut roots = [last_term];
        unsafe { heap.collect(&mut roots) };

        // binary_vheap_size should have dropped to just the one survivor.
        assert_eq!(heap.binary_vheap_size, REFC_BINARY_VHEAP_OVERHEAD);
        assert!(heap.binary_vheap_size < vheap_before);
        assert_eq!(heap.refc_binaries.len(), 1);
        // Threshold resets to default (1 × 80 × 2 = 160 < 256 KiB default).
        assert_eq!(heap.binary_vheap_threshold, DEFAULT_BINARY_VHEAP_THRESHOLD);
    }

    #[test]
    fn binary_vheap_dedup_same_arc() {
        // Allocate the SAME Arc multiple times (e.g., receiving 100K messages
        // all referencing the same binary). After GC, update_binary_vheap
        // should count only 1 unique Arc, not N descriptors.
        let mut heap = ProcessHeap::new();
        let shared_data = Arc::new(BinaryData::new(vec![0u8; 10240]));

        // Create 100 descriptors all pointing to the same Arc.
        let n = 100;
        let mut terms = Vec::new();
        for _ in 0..n {
            let term = heap.alloc_refc_binary_from_arc(shared_data.clone(), 0, 10240);
            terms.push(term);
        }
        assert_eq!(heap.refc_binaries.len(), n);
        // Per-alloc charging counts each descriptor.
        assert_eq!(heap.binary_vheap_size, n * REFC_BINARY_VHEAP_OVERHEAD);

        // GC with all terms as roots — everything survives.
        unsafe { heap.collect(&mut terms) };
        assert_eq!(heap.refc_binaries.len(), n);

        // After GC, update_binary_vheap should have deduped: only 1 unique Arc.
        assert_eq!(heap.refc_unique_count, 1);
        assert_eq!(heap.binary_vheap_size, REFC_BINARY_VHEAP_OVERHEAD);

        // Threshold should tolerate N descriptors re-accumulating.
        assert!(heap.binary_vheap_threshold >= n * REFC_BINARY_VHEAP_OVERHEAD * 2,
            "threshold ({}) should be >= 2 * total descriptors * overhead ({})",
            heap.binary_vheap_threshold, n * REFC_BINARY_VHEAP_OVERHEAD * 2);

        // With the high threshold, allocating N more same-Arc descriptors
        // should NOT trigger GC.
        for _ in 0..n {
            heap.alloc_refc_binary_from_arc(shared_data.clone(), 0, 10240);
        }
        assert!(!heap.needs_gc(),
            "should not need GC after re-accumulating same-Arc descriptors");
    }

    /// Regression test for GC threshold ratchet: after a process grows its
    /// heap to a large size and then GCs down to a small live set, the
    /// threshold should shrink to match.  Previously the threshold would
    /// stay at peak size, meaning the process would never GC again.
    #[test]
    fn gc_threshold_shrinks_after_data_drops() {
        let mut heap = ProcessHeap::new();

        // Allocate enough to push past the default threshold multiple times
        // so the threshold ratchets up.
        let big_size = DEFAULT_GC_THRESHOLD / 8 + 1; // words to exceed threshold
        for _ in 0..big_size {
            heap.alloc_words(1);
        }
        assert!(heap.needs_gc());
        assert!(heap.bytes_allocated() > DEFAULT_GC_THRESHOLD);

        // GC with a single small root — nearly all data is dead.
        let live = heap.alloc_tuple(&[ErlTerm::small_int(42)]);
        let mut roots = [live];
        unsafe { heap.collect(&mut roots) };

        // After GC, threshold should be max(live*2, DEFAULT).
        // Since live data is tiny, threshold should reset to DEFAULT.
        assert_eq!(heap.gc_threshold, DEFAULT_GC_THRESHOLD);
        assert!(heap.bytes_allocated() < DEFAULT_GC_THRESHOLD);
        assert!(!heap.needs_gc());
    }

    // -----------------------------------------------------------------------
    // Generational GC tests
    // -----------------------------------------------------------------------

    #[test]
    fn young_gen_basic() {
        let mut yg = YoungGen::new(100);
        let p1 = yg.alloc_words(10);
        let p2 = yg.alloc_words(10);
        assert_ne!(p1, p2);
        assert!(yg.contains(p1));
        assert!(yg.contains(p2));
        assert_eq!(yg.bytes_used(), 20 * 8);
        // Overflow creates new chunk (never fails).
        let p3 = yg.alloc_words(200);
        assert!(yg.contains(p3));
        assert_eq!(yg.bytes_used(), (20 + 200) * 8);
    }

    #[test]
    fn young_gen_contains_rejects_outside() {
        let yg = YoungGen::new(100);
        let stack_var: u64 = 0;
        assert!(!yg.contains(&stack_var as *const u64));
    }

    #[test]
    fn minor_gc_promotes_young_to_old() {
        let mut heap = ProcessHeap::new();
        let list = heap.alloc_list_from_slice(&[
            ErlTerm::small_int(1),
            ErlTerm::small_int(2),
            ErlTerm::small_int(3),
        ]);
        assert!(heap.young_bytes_allocated > 0);
        let old_before = heap.old_bytes_allocated;

        let mut roots = [list];
        heap.collect_minor(&mut roots);

        // Data should have moved to old gen.
        assert!(heap.old_bytes_allocated > old_before);
        assert_eq!(heap.young_bytes_allocated, 0);
        assert_eq!(heap.minor_gc_count, 1);

        // Verify data integrity.
        let mut values = vec![];
        let mut current = roots[0];
        while current.is_cons() {
            unsafe {
                let cell = &*current.as_cons_ptr().unwrap();
                values.push(cell.head.as_small_int().unwrap());
                current = cell.tail;
            }
        }
        assert!(current.is_nil());
        assert_eq!(values, vec![1, 2, 3]);
    }

    #[test]
    fn minor_gc_skips_old_gen_data() {
        let mut heap = ProcessHeap::new();
        // Allocate data, then do a minor GC to promote it to old.
        let old_data = heap.alloc_tuple(&[ErlTerm::small_int(42)]);
        let mut roots = [old_data];
        heap.collect_minor(&mut roots);
        let old_after_first_gc = heap.old_bytes_allocated;

        // Now allocate new young data that references the old tuple.
        let new_list = heap.alloc_cons(roots[0], ErlTerm::nil());
        let mut roots2 = [new_list];
        heap.collect_minor(&mut roots2);

        // Old gen should have grown by only the new cons cell + the list pointer,
        // NOT by re-copying the old tuple.
        let expected_new = 2 * 8; // one cons cell = 2 words
        assert_eq!(heap.old_bytes_allocated, old_after_first_gc + expected_new);

        // Verify the tuple inside the list is still intact.
        unsafe {
            let cell = &*roots2[0].as_cons_ptr().unwrap();
            let hdr_ptr = cell.head.as_boxed_ptr().unwrap();
            let header = BoxedHeader(*hdr_ptr);
            assert_eq!(header.subtype(), BOXED_TUPLE);
            assert_eq!(ErlTerm(*hdr_ptr.add(1)).as_small_int(), Some(42));
        }
    }

    #[test]
    fn major_gc_compacts_both_generations() {
        let mut heap = ProcessHeap::new();
        // Promote garbage AND useful data to old gen via minor GC.
        let garbage1 = heap.alloc_tuple(&[ErlTerm::small_int(99)]);
        let garbage2 = heap.alloc_tuple(&[ErlTerm::small_int(88)]);
        let keep = heap.alloc_tuple(&[ErlTerm::small_int(42)]);
        // Promote all three to old gen.
        let mut all_roots = [garbage1, garbage2, keep];
        heap.collect_minor(&mut all_roots);

        let old_before_major = heap.old_bytes_allocated;
        // old gen now has 3 tuples.
        assert!(old_before_major > 0);

        // Major GC with only `keep` as root — garbage1 and garbage2 are dead.
        let mut roots = [all_roots[2]]; // keep
        heap.collect_major(&mut roots);

        // Old gen should be smaller (2 garbage tuples collected).
        assert!(heap.old_bytes_allocated < old_before_major,
            "old_bytes after major ({}) should be < before ({})",
            heap.old_bytes_allocated, old_before_major);
        assert_eq!(heap.minor_gc_count, 0);
        assert_eq!(heap.young_bytes_allocated, 0);

        // Data still intact.
        unsafe {
            let hdr_ptr = roots[0].as_boxed_ptr().unwrap();
            assert_eq!(ErlTerm(*hdr_ptr.add(1)).as_small_int(), Some(42));
        }
    }

    #[test]
    fn collect_dispatches_major_when_old_threshold_exceeded() {
        let mut heap = ProcessHeap::new();
        // Lower old threshold to make it easier to trigger.
        heap.old_gc_threshold = 128;

        // Promote data past the old threshold.
        // Each tuple = 3 words = 24 bytes. Need ~6 to exceed 128.
        for _ in 0..8 {
            let t = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
            let mut r = [t];
            heap.collect_minor(&mut r);
        }
        assert!(heap.old_bytes_allocated >= 128,
            "old_bytes ({}) should be >= 128", heap.old_bytes_allocated);
        assert!(heap.needs_major_gc());

        let keep = heap.alloc_cons(ErlTerm::small_int(1), ErlTerm::nil());
        let mut roots = [keep];
        unsafe { heap.collect(&mut roots) }; // should dispatch to major

        assert_eq!(heap.minor_gc_count, 0); // major GC resets this
    }

    #[test]
    fn minor_gc_preserves_refc_binary_in_old_gen() {
        let mut heap = ProcessHeap::new();
        let data: Vec<u8> = (0..100).collect();
        let arc = Arc::new(BinaryData::new(data));
        let arc_clone = arc.clone();
        let term = heap.alloc_refc_binary_from_arc(arc, 0, 100);

        // Promote to old gen.
        let mut roots = [term];
        heap.collect_minor(&mut roots);
        assert_eq!(heap.refc_binaries.len(), 1);

        // Do another minor GC with the refc binary in old gen.
        // The sweep must still see the old-gen refc binary as live.
        heap.collect_minor(&mut roots);
        assert_eq!(heap.refc_binaries.len(), 1);
        assert_eq!(Arc::strong_count(&arc_clone), 2); // our clone + heap's copy

        // Verify data.
        unsafe {
            let bytes = roots[0].refc_binary_bytes().unwrap();
            assert_eq!(bytes[0], 0);
            assert_eq!(bytes[99], 99);
        }
    }

    #[test]
    fn minor_gc_preserves_binary_behind_old_match_context() {
        let mut heap = ProcessHeap::new();
        let backing = Arc::new(BinaryData::new((0..200).collect()));
        let weak = Arc::downgrade(&backing);
        let binary = heap.alloc_refc_binary_from_arc(backing, 50, 100);
        let descriptor = unsafe { crate::match_context::MatchContext::new(binary, 8, 800) }.unwrap();
        let context = heap.alloc_match_ctx(descriptor);
        let mut roots = [context];
        heap.collect_minor(&mut roots); // Promote context and its binary.
        heap.collect_minor(&mut roots); // Old context is not traversed again.
        assert!(weak.upgrade().is_some(), "live binary backing was freed");
        unsafe {
            let binary = ErlTerm(*roots[0].as_boxed_ptr().unwrap().add(1));
            assert_eq!(binary.refc_binary_bytes().unwrap(), &(50..150).collect::<Vec<u8>>());
        }
    }

    #[test]
    fn minor_gc_retains_nested_old_binaries_but_sweeps_young_garbage() {
        let mut heap = ProcessHeap::new();
        let backing = Arc::new(BinaryData::new(vec![42; 200]));
        let old_weak = Arc::downgrade(&backing);
        let first = heap.alloc_refc_binary_from_arc(backing.clone(), 10, 90);
        let second = heap.alloc_refc_binary_from_arc(backing, 100, 90);
        let pair = heap.alloc_tuple(&[first, second]);
        let list = heap.alloc_cons(pair, ErlTerm::nil());
        let map = heap.alloc_map(MapPairs::from_flat(&[ErlTerm::small_int(1), list]).expect("complete key/value pairs"));
        let mut roots = [map];
        heap.collect_minor(&mut roots);

        let garbage = Arc::new(BinaryData::new(vec![7; 100]));
        let young_weak = Arc::downgrade(&garbage);
        heap.alloc_refc_binary_from_arc(garbage, 0, 100);
        heap.collect_minor(&mut roots);
        assert!(old_weak.upgrade().is_some());
        assert!(young_weak.upgrade().is_none());

        // Major collection traces through the nested map/list/tuple roots.
        unsafe { heap.collect_full(&mut roots) };
        heap.collect_minor(&mut roots);
        assert!(old_weak.upgrade().is_some());

        // Dead old objects and their backing stores are reclaimed together.
        heap.collect_minor(&mut []);
        assert!(old_weak.upgrade().is_some());
        unsafe { heap.collect_full(&mut []) };
        assert!(old_weak.upgrade().is_none());
        assert!(heap.old_refc_ptrs.is_empty());
        assert_eq!(heap.binary_vheap_size, 0);
    }

    /// Regression: the collector copied list heads and tuple elements
    /// recursively on the native stack, so a process holding an iolist built
    /// by prepending (`[Acc, Chunk]`) crashed once a collection copied about
    /// 13,000 levels (fewer on a test thread's stack). Minor and major
    /// collections must copy such terms, and a minor one must stop at the
    /// promoted levels, keeping every level and the one shared chunk binary.
    #[test]
    fn collections_copy_deep_head_nesting_without_native_recursion() {
        const HALF: usize = 100_000;

        /// Wrap `levels` more levels around the iolist and the tuple chain.
        fn nest(heap: &mut ProcessHeap, roots: &mut [ErlTerm; 2], chunk: ErlTerm, levels: usize) {
            for _ in 0..levels {
                let rest = heap.alloc_cons(chunk, ErlTerm::nil());
                roots[0] = heap.alloc_cons(roots[0], rest);
                roots[1] = heap.alloc_tuple(&[roots[1], chunk]);
            }
        }

        /// Walk both terms to the bottom; return their depth, their shared
        /// chunk and the two terms found `mark` levels down.
        unsafe fn walk(heap: &ProcessHeap, roots: &[ErlTerm; 2], mark: usize) -> (usize, ErlTerm, [ErlTerm; 2]) {
            let outer = unsafe { &*roots[0].as_cons_ptr().unwrap() };
            let chunk = unsafe { (*outer.tail.as_cons_ptr().unwrap()).head };
            assert_eq!(unsafe { chunk.borrow(heap) }.binary_bytes(), Some(&b"chunk"[..]));
            let (mut list, mut tuple, mut depth) = (roots[0], roots[1], 0);
            let mut marked = [ErlTerm::nil(); 2];
            while !list.is_nil() {
                if depth == mark {
                    marked = [list, tuple];
                }
                let cell = unsafe { &*list.as_cons_ptr().expect("level is a list") };
                let rest = unsafe { &*cell.tail.as_cons_ptr().expect("level has a chunk") };
                assert_eq!((rest.head, rest.tail), (chunk, ErlTerm::nil()), "list level {depth}");
                let words = tuple.as_boxed_ptr().expect("level is a tuple");
                assert_eq!(unsafe { BoxedHeader(*words) }.subtype(), BOXED_TUPLE);
                assert_eq!(unsafe { ErlTerm(*words.add(2)) }, chunk, "tuple level {depth}");
                (list, tuple) = (cell.head, unsafe { ErlTerm(*words.add(1)) });
                depth += 1;
            }
            assert!(tuple.is_nil(), "tuple chain is deeper than the list");
            (depth, chunk, marked)
        }

        let mut heap = ProcessHeap::new();
        let chunk = heap.alloc_binary(b"chunk");
        let mut roots = [ErlTerm::nil(), ErlTerm::nil()];
        nest(&mut heap, &mut roots, chunk, HALF);
        heap.collect_minor(&mut roots); // Promote the inner half.
        // SAFETY: roots were just updated by the collection over this heap.
        let (depth, chunk, _) = unsafe { walk(&heap, &roots, 0) };
        assert_eq!(depth, HALF);
        let promoted = roots;

        // Young outer levels over promoted inner ones: the minor collection
        // copies the young half and keeps pointers to the promoted half.
        nest(&mut heap, &mut roots, chunk, HALF);
        heap.collect_minor(&mut roots);
        // SAFETY: as above, after each collection.
        unsafe {
            let (depth, _, inner) = walk(&heap, &roots, HALF);
            assert_eq!(depth, 2 * HALF);
            assert_eq!(inner, promoted, "the minor collection copied promoted levels");
            heap.collect_full(&mut roots);
            assert_eq!(walk(&heap, &roots, 0).0, 2 * HALF);
        }
    }

    #[test]
    fn should_shrink_detects_oversized_old_gen() {
        let mut heap = ProcessHeap::new();
        assert!(!heap.should_shrink());

        // Promote a lot of data to old gen.
        // Each tuple = 3 words = 24 bytes. Need > 4 MiB = ~174760 tuples.
        // should_shrink threshold is 4× DEFAULT_OLD_GC_THRESHOLD.
        let batch = 500;
        for _ in 0..360 {
            let mut terms: Vec<ErlTerm> = Vec::new();
            for _ in 0..batch {
                terms.push(heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]));
            }
            heap.collect_minor(&mut terms);
        }
        assert!(heap.old_bytes_allocated > DEFAULT_OLD_GC_THRESHOLD * 4);
        assert!(heap.should_shrink());

        // After a major GC with no roots, old gen is tiny.
        unsafe { heap.collect_full(&mut []) };
        assert!(!heap.should_shrink());
        assert!(heap.old_bytes_allocated < DEFAULT_OLD_GC_THRESHOLD);
    }

    #[test]
    fn minor_gc_schedules_major_when_old_bloated() {
        let mut heap = ProcessHeap::new();
        // Promote lots of data to old gen.
        let mut terms: Vec<ErlTerm> = Vec::new();
        let batch = 5000;
        for _ in 0..batch {
            terms.push(heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]));
        }
        heap.collect_minor(&mut terms);

        // Artificially inflate old_bytes_allocated to simulate dead data
        // (as if many minor GCs promoted data that's now unreachable).
        heap.old_bytes_allocated = DEFAULT_OLD_GC_THRESHOLD * 5;
        let inflated_old = heap.old_bytes_allocated;

        // Minor GC with tiny promotion should lower old_gc_threshold.
        let tiny = heap.alloc_cons(ErlTerm::small_int(1), ErlTerm::nil());
        let mut roots = [tiny];
        heap.collect_minor(&mut roots);

        // The shrink heuristic should have lowered old_gc_threshold.
        assert!(heap.old_gc_threshold <= inflated_old + 16, // promoted_bytes was tiny
            "old_gc_threshold ({}) should have been lowered to trigger major GC",
            heap.old_gc_threshold);
        // Next needs_gc should want a major GC.
        assert!(heap.needs_major_gc());
    }
    #[test]
    fn large_first_promotion_does_not_request_another_gc() {
        let mut heap = ProcessHeap::new();
        let mut list = ErlTerm::nil();
        for _ in 0..300_000 {
            list = heap.alloc_cons(ErlTerm::small_int(65), list);
        }
        let mut roots = [list];
        unsafe { heap.collect(&mut roots); }
        assert_eq!(heap.minor_gc_count, 1);
        assert_eq!(heap.old_bytes_allocated, 300_000 * 16);
        assert!(!heap.needs_gc(), "freshly traced live promotion must not trigger a second GC");
        assert!(!heap.should_collect_idle(&roots), "the same live graph needs no idle full sweep");
    }

    #[test]
    fn idle_gc_skips_unchanged_roots_but_reclaims_dropped_roots() {
        let mut heap = ProcessHeap::new();
        let mut list = ErlTerm::nil();
        for _ in 0..300_000 {
            list = heap.alloc_cons(ErlTerm::small_int(65), list);
        }
        let backing = Arc::new(BinaryData::new(vec![42; 200]));
        let weak = Arc::downgrade(&backing);
        let binary = heap.alloc_refc_binary_from_arc(backing, 0, 200);
        let mut roots = [list, binary, ErlTerm::small_int(1)];
        unsafe { heap.collect_full(&mut roots); }
        // Changing counters and allocating unreachable young data does not
        // change the old live graph. Ignore order and duplicate roots too.
        roots[2] = ErlTerm::small_int(2);
        heap.alloc_cons(ErlTerm::nil(), ErlTerm::nil());
        assert!(!heap.should_collect_idle(&[roots[1], roots[0], roots[0], roots[2]]));
        assert!(weak.upgrade().is_some());
        // No allocation is required for old data to become garbage.
        assert!(heap.should_collect_idle(&[]));
        unsafe { heap.collect_full(&mut []); }
        assert_eq!(heap.bytes_allocated(), 0);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn idle_gc_cache_never_overrides_pressure_or_explicit_collection() {
        let mut heap = ProcessHeap::new();
        let mut list = ErlTerm::nil();
        for _ in 0..300_000 {
            list = heap.alloc_cons(ErlTerm::small_int(65), list);
        }
        let mut roots = [list];
        unsafe { heap.collect_full(&mut roots); }
        assert!(!heap.should_collect_idle(&roots));
        heap.set_max_heap_size(1);
        assert!(heap.should_collect_idle(&roots));
        heap.set_max_heap_size(0);
        // No live young objects: a minor GC must preserve the cache rather
        // than schedule a major merely because the working set is large.
        heap.alloc_words(heap.gc_threshold / 8);
        assert!(heap.should_collect_idle(&roots));
        heap.collect_minor(&mut roots);
        assert!(!heap.needs_gc());
        assert!(!heap.should_collect_idle(&roots));
        // Binary virtual heap pressure also overrides the idle cache.
        for _ in 0..4000 {
            heap.alloc_binary_auto(&[0; 100]);
        }
        assert!(heap.should_collect_idle(&roots));
        unsafe { heap.collect_full(&mut roots); }
        assert_eq!(heap.refc_binary_count(), 0);
        let before = roots[0];
        unsafe { heap.collect_full(&mut roots); }
        assert_ne!(roots[0], before, "explicit full collection must always run");
        heap.set_fullsweep_after(0);
        unsafe { heap.collect(&mut roots); }
        assert_eq!(heap.minor_gc_count, 0);
    }

    #[test]
    fn new_promotions_invalidate_idle_root_snapshot() {
        let mut heap = ProcessHeap::new();
        let mut list = ErlTerm::nil();
        for _ in 0..300_000 {
            list = heap.alloc_cons(ErlTerm::small_int(65), list);
        }
        let mut roots = [list, ErlTerm::nil()];
        unsafe { heap.collect_full(&mut roots); }
        roots[1] = heap.alloc_cons(ErlTerm::small_int(66), ErlTerm::nil());
        heap.collect_minor(&mut roots);
        roots[1] = ErlTerm::nil();
        assert!(heap.should_collect_idle(&roots), "newly promoted garbage must be reclaimable");
        unsafe { heap.collect_full(&mut roots); }
        assert_eq!(heap.bytes_allocated(), 300_000 * 16);
        assert!(!heap.should_collect_idle(&roots));
    }

    #[test]
    fn large_promotion_into_existing_old_gen_has_live_headroom() {
        let mut heap = ProcessHeap::new();
        let small = heap.alloc_cons(ErlTerm::small_int(1), ErlTerm::nil());
        let mut roots = [small, ErlTerm::nil()];
        unsafe { heap.collect(&mut roots); }
        for _ in 0..300_000 {
            roots[1] = heap.alloc_cons(ErlTerm::small_int(65), roots[1]);
        }
        unsafe { heap.collect(&mut roots); }
        assert!(!heap.needs_gc(), "a large live promotion needs headroom even with an existing old generation");
        // Unlike the empty-old case, old garbage has not been traced, so idle
        // shrinking must still be allowed to perform a complete collection.
        assert!(heap.should_collect_idle(&roots));
    }

}
