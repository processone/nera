//! Tagged pointer representation for Erlang terms.
//!
//! A 64-bit tagged value. Low 3 bits encode the primary tag:
//!
//! | Tag (bits 2..0) | Meaning                                            |
//! |-----------------|----------------------------------------------------|
//! | `000`           | Boxed pointer (tuple, float, bigint, binary, closure, map) |
//! | `001`           | Cons cell pointer                                  |
//! | `010`           | Small integer (61-bit signed, shifted left by 3)   |
//! | `011`           | Atom (atom index shifted left by 3)                |
//! | `100`           | Pid (process id shifted left by 3)                 |
//! | `101`           | Nil (empty list)                                   |
//!
//! Boxed objects start with a header word where the low 4 bits encode the
//! boxed subtype tag and the upper bits encode arity/size:
//!
//! | Subtype (bits 3..0) | Meaning  |
//! |---------------------|----------|
//! | `0x0`               | Tuple    |
//! | `0x1`               | Float    |
//! | `0x2`               | BigInt   |
//! | `0x3`               | Heap Binary   |
//! | `0x4`               | Closure  |
//! | `0x5`               | Map      |
//! | `0x6`               | Refc Binary Descriptor |
//! | `0x7`               | Reference  |

use crate::binary::BinaryData;
use crate::heap::ProcessHeap;
use crate::process::ProcessId;
use std::fmt;
use std::marker::PhantomData;

// ---------------------------------------------------------------------------
// Primary tag constants (low 3 bits)
// ---------------------------------------------------------------------------

pub const TAG_BOXED: u64 = 0b000;
pub const TAG_CONS: u64 = 0b001;
pub const TAG_SMALL_INT: u64 = 0b010;
pub const TAG_ATOM: u64 = 0b011;
pub const TAG_PID: u64 = 0b100;
pub const TAG_NIL: u64 = 0b101;

pub const TAG_MASK: u64 = 0b111;
pub const TAG_BITS: u32 = 3;

/// Tombstone sentinel — tag `0b110` is unused by any ErlTerm constructor.
/// Used by the mailbox to mark removed slots without shifting.
pub const TOMBSTONE_VALUE: u64 = 0b110;

/// Returned by the receive scan (`nera_rt_receive_peek` and
/// `nera_rt_receive_next`) when no live message remains. Any term, `[]`
/// included, can be a message, so the end of the scan must not be a term.
/// The tombstone is never a term, and `Mailbox::send` refuses it as a message.
pub const NO_MESSAGE_VALUE: u64 = TOMBSTONE_VALUE;

/// Bit 63 marks a term as "external" — it points into ETS storage
/// (Arc-owned CompactHeap), not the process arena. GC must not
/// relocate or forward external terms.
pub const EXTERNAL_BIT: u64 = 1u64 << 63;

/// Minimum value representable as a small integer (61-bit signed).
pub const SMALL_INT_MIN: i64 = -(1_i64 << 60);
/// Maximum value representable as a small integer (61-bit signed).
pub const SMALL_INT_MAX: i64 = (1_i64 << 60) - 1;

/// Returns true if `val` fits in the small integer range (61-bit signed).
#[inline]
pub fn fits_small_int(val: i64) -> bool {
    (SMALL_INT_MIN..=SMALL_INT_MAX).contains(&val)
}

// ---------------------------------------------------------------------------
// Boxed subtype header tags (low 4 bits of header word)
// ---------------------------------------------------------------------------

/// Known boxed object kinds, with explicit discriminants matching the runtime ABI.
///
/// This selects a header tag; it does not establish the validity of an object's
/// payload or the lifetime of its storage. Raw header inspection remains separate.
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxedSubtype {
    Tuple = 0x0,
    Float = 0x1,
    BigInt = 0x2,
    HeapBinary = 0x3,
    Closure = 0x4,
    Map = 0x5,
    RefcBinary = 0x6,
    Reference = 0x7,
    MatchContext = 0x8,
}

/// A raw value that does not name a supported boxed object kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidBoxedSubtype(pub u64);

impl std::fmt::Display for InvalidBoxedSubtype {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown boxed subtype {:#x}", self.0)
    }
}

impl std::error::Error for InvalidBoxedSubtype {}

impl TryFrom<u64> for BoxedSubtype {
    type Error = InvalidBoxedSubtype;

    fn try_from(raw: u64) -> Result<Self, Self::Error> {
        match raw {
            0x0 => Ok(Self::Tuple),
            0x1 => Ok(Self::Float),
            0x2 => Ok(Self::BigInt),
            0x3 => Ok(Self::HeapBinary),
            0x4 => Ok(Self::Closure),
            0x5 => Ok(Self::Map),
            0x6 => Ok(Self::RefcBinary),
            0x7 => Ok(Self::Reference),
            0x8 => Ok(Self::MatchContext),
            _ => Err(InvalidBoxedSubtype(raw)),
        }
    }
}

pub const BOXED_TUPLE: u64 = BoxedSubtype::Tuple as u64;
pub const BOXED_FLOAT: u64 = BoxedSubtype::Float as u64;
pub const BOXED_BIGINT: u64 = BoxedSubtype::BigInt as u64;
pub const BOXED_HEAP_BINARY: u64 = BoxedSubtype::HeapBinary as u64;
/// Backward-compatible alias for the on-heap binary subtype.
pub const BOXED_BINARY: u64 = BOXED_HEAP_BINARY;
pub const BOXED_CLOSURE: u64 = BoxedSubtype::Closure as u64;
pub const BOXED_MAP: u64 = BoxedSubtype::Map as u64;
pub const BOXED_REFC_BINARY: u64 = BoxedSubtype::RefcBinary as u64;
pub const BOXED_REFERENCE: u64 = BoxedSubtype::Reference as u64;
/// Match context for binary pattern matching — an in-place cursor that
/// avoids sub-binary allocation during iterative binary parsing.
/// Layout: [header(MATCH_CTX, 3) | orig_binary | bit_offset | total_bits]
pub const BOXED_MATCH_CTX: u64 = BoxedSubtype::MatchContext as u64;

pub const BOXED_TAG_MASK: u64 = 0xF;
pub const BOXED_TAG_BITS: u32 = 4;

// ---------------------------------------------------------------------------
// Reserved boxed layouts (future-facing ABI)
// ---------------------------------------------------------------------------

/// Bigint sign encoding stored in the sign word after the header.
pub const BIGINT_SIGN_NON_NEGATIVE: u64 = 0;
pub const BIGINT_SIGN_NEGATIVE: u64 = 1;
/// Bigint layout:
/// [ header(BIGINT, limb_count), sign, limb0, limb1, ... ].
pub const BIGINT_SIGN_WORD_INDEX: usize = 1;
pub const BIGINT_FIRST_LIMB_WORD_INDEX: usize = 2;
#[inline]
pub const fn bigint_total_words(limb_count: usize) -> usize {
    BIGINT_FIRST_LIMB_WORD_INDEX + limb_count
}

/// Refc-binary descriptor layout:
/// [ header(REFC_BINARY, bit_len), refc_ptr, byte_offset ].
pub const REFC_BINARY_PTR_WORD_INDEX: usize = 1;
pub const REFC_BINARY_OFFSET_WORD_INDEX: usize = 2;
pub const REFC_BINARY_TOTAL_WORDS: usize = 3;

/// Distinct reference payload domains; all remain Erlang reference terms.
/// Resource kinds reuse header metadata without changing the two-word layout.
#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReferenceKind {
    Ordinary = 0,
    Atomics = 1,
    Counters = 2,
    Zlib = 3,
}

impl ReferenceKind {
    pub(crate) const fn header(self) -> BoxedHeader {
        BoxedHeader(((self as u64) << BOXED_TAG_BITS) | BOXED_REFERENCE)
    }
}

/// Reference layout:
/// [ header(REFERENCE, ReferenceKind + immutable flags), unique_id or typed resource pointer ].
pub const REFERENCE_ID_WORD_INDEX: usize = 1;
pub const REFERENCE_TOTAL_WORDS: usize = 2;

/// Match context layout:
/// [ header(MATCH_CTX, 3), orig_binary, bit_offset, total_bits ].
/// orig_binary is a tagged ErlTerm (GC scans it).
/// bit_offset and total_bits are raw u64 (not tagged).
pub const MATCH_CTX_ORIG_BINARY_INDEX: usize = crate::match_context::BINARY_OFFSET as usize / 8;
pub const MATCH_CTX_BIT_OFFSET_INDEX: usize = crate::match_context::BIT_OFFSET as usize / 8;
pub const MATCH_CTX_TOTAL_BITS_INDEX: usize = crate::match_context::END_OFFSET as usize / 8;
pub const MATCH_CTX_TOTAL_WORDS: usize = crate::match_context::BYTE_SIZE / 8;

// ---------------------------------------------------------------------------
// Sentinel value used by Nil
// ---------------------------------------------------------------------------

/// The raw bit pattern for Nil. Only the tag bits matter, upper bits are zero.
const NIL_VALUE: u64 = TAG_NIL;

// ---------------------------------------------------------------------------
// ConsCell
// ---------------------------------------------------------------------------

/// A cons cell stored on the process heap.
#[repr(C)]
pub struct ConsCell {
    pub head: ErlTerm,
    pub tail: ErlTerm,
}

// ---------------------------------------------------------------------------
// BoxedHeader
// ---------------------------------------------------------------------------

/// The first word of every boxed object on the heap.
///
/// Layout: `[ arity/size (60 bits) | subtype tag (4 bits) ]`
#[derive(Clone, Copy, Debug)]
pub struct BoxedHeader(pub u64);

impl BoxedHeader {
    /// Create a new header for a boxed object.
    ///
    /// Construction must select a known subtype rather than masking raw integers:
    /// ```compile_fail
    /// use nera_runtime::term::BoxedHeader;
    /// let _ = BoxedHeader::new(0x10, 0);
    /// ```
    /// ```compile_fail
    /// use nera_runtime::term::BoxedHeader;
    /// let _ = BoxedHeader::try_new(0x10, 0);
    /// ```
    #[inline]
    pub fn new(subtype: BoxedSubtype, arity: u64) -> Self {
        Self::try_new(subtype, arity).expect("boxed header arity exceeds its bit domain")
    }

    #[inline]
    pub fn try_new(subtype: BoxedSubtype, arity: u64) -> Option<Self> {
        if arity > u64::MAX >> BOXED_TAG_BITS { return None; }
        Some(BoxedHeader((arity << BOXED_TAG_BITS) | subtype as u64))
    }

    /// Replace the arity of an existing header without changing its raw tag.
    ///
    /// This also preserves reserved tags when inspecting raw headers. It does
    /// not validate the payload or ensure its allocation fits the new arity.
    #[inline]
    pub fn with_arity(self, arity: u64) -> Self {
        self.try_with_arity(arity).expect("boxed header arity exceeds its bit domain")
    }

    #[inline]
    pub fn try_with_arity(self, arity: u64) -> Option<Self> {
        if arity > u64::MAX >> BOXED_TAG_BITS { return None; }
        Some(Self((arity << BOXED_TAG_BITS) | self.subtype()))
    }

    /// The boxed subtype tag (low 4 bits).
    #[inline]
    pub fn subtype(&self) -> u64 {
        self.0 & BOXED_TAG_MASK
    }

    /// The arity / size stored in the upper bits.
    #[inline]
    pub fn arity(&self) -> u64 {
        self.0 >> BOXED_TAG_BITS
    }
}

// ---------------------------------------------------------------------------
// ErlTerm
// ---------------------------------------------------------------------------

/// An unrooted 64-bit tagged Erlang term.
///
/// Copying a term copies its bits, not its backing allocation or GC root.
/// Tag tests, raw-pointer extraction and `Debug` inspect bits only. Operations
/// that read heap memory are unsafe and require the caller to keep the object
/// alive and use the updated term after any moving collection.
///
/// Reading a heap header requires an explicit unsafe boundary, even when the
/// term came from an allocator: copying GC can invalidate previously saved bits.
///
/// ```compile_fail,E0133
/// use nera_runtime::heap::ProcessHeap;
/// let mut heap = ProcessHeap::new();
/// let term = heap.alloc_tuple(&[]);
/// term.boxed_subtype();
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ErlTerm(pub(crate) u64);

impl ErlTerm {
    /// Bind a currently valid term to the heap that keeps its storage alive.
    /// The view adds no runtime validation or allocation.
    ///
    /// # Safety
    /// Any referenced object must be live and have the runtime layout. `heap`
    /// must own its storage (including borrowed ETS storage and binary owners).
    /// The term must be the current value after any moving GC. All referenced
    /// storage must remain immutable for the borrow, including against writes
    /// through raw pointers or from generated code.
    ///
    /// ```
    /// use nera_runtime::heap::ProcessHeap;
    /// let mut heap = ProcessHeap::new();
    /// let term = heap.alloc_binary(b"hello");
    /// // SAFETY: term was just allocated in heap and no collection intervened.
    /// let view = unsafe { term.borrow(&heap) };
    /// assert_eq!(view.binary_bytes(), Some(b"hello".as_slice()));
    /// ```
    ///
    /// A live view prevents moving collection:
    ///
    /// ```compile_fail,E0502
    /// use nera_runtime::heap::ProcessHeap;
    /// let mut heap = ProcessHeap::new();
    /// let term = heap.alloc_binary(b"hello");
    /// let view = unsafe { term.borrow(&heap) };
    /// unsafe { heap.collect_full(&mut [term]); }
    /// assert!(view.is_binary());
    /// ```
    ///
    /// A live view also prevents dropping the backing heap:
    ///
    /// ```compile_fail,E0505
    /// use nera_runtime::heap::ProcessHeap;
    /// let mut heap = ProcessHeap::new();
    /// let term = heap.alloc_binary(b"hello");
    /// let view = unsafe { term.borrow(&heap) };
    /// drop(heap);
    /// assert!(view.is_binary());
    /// ```
    #[inline]
    pub unsafe fn borrow<'heap>(self, _heap: &'heap ProcessHeap) -> BorrowedTerm<'heap> {
        BorrowedTerm {
            term: self,
            _heap: PhantomData,
        }
    }

    /// Store ABI bits without dereferencing them or asserting heap validity.
    /// Heap-reading operations remain unsafe, including for allocator results.
    #[inline]
    pub const fn from_raw(bits: u64) -> Self {
        Self(bits)
    }

    /// Return the unchanged 64-bit ABI representation.
    #[inline]
    pub const fn to_raw(self) -> u64 {
        self.0
    }

    // -- Constructors -------------------------------------------------------

    /// Create a small integer term.
    ///
    /// # Panics
    /// Panics if the value does not fit in 61 signed bits, including in release.
    #[inline]
    pub fn small_int(val: i64) -> Self {
        Self::try_small_int(val).expect("small integer outside signed 61-bit domain")
    }

    /// Encode a value only if it fits the immediate integer representation.
    #[inline]
    pub fn try_small_int(val: i64) -> Option<Self> {
        fits_small_int(val).then_some(ErlTerm(((val as u64) << TAG_BITS) | TAG_SMALL_INT))
    }

    /// Create an atom term from an atom table index.
    #[inline]
    pub fn atom(id: u32) -> Self {
        ErlTerm(((id as u64) << TAG_BITS) | TAG_ATOM)
    }

    /// Create a Pid term from a process id.
    /// ```compile_fail
    /// use nera_runtime::term::ErlTerm;
    /// let _ = ErlTerm::pid(1);
    /// ```
    #[inline]
    pub fn pid(id: ProcessId) -> Self {
        ErlTerm((id.get() << TAG_BITS) | TAG_PID)
    }

    /// The empty list `[]`.
    #[inline]
    pub fn nil() -> Self {
        ErlTerm(NIL_VALUE)
    }

    /// Encode an 8-byte-aligned cons address; panic on misalignment in all builds.
    /// This only encodes the address, including null. It does not establish that
    /// storage is live or initialized, or extend its lifetime.
    #[inline]
    pub fn cons(ptr: *const ConsCell) -> Self {
        Self::try_cons(ptr).expect("cons pointer must be 8-byte aligned")
    }

    /// Encode a cons address only if its tag bits are clear.
    #[inline]
    pub fn try_cons(ptr: *const ConsCell) -> Option<Self> {
        if ptr as u64 & TAG_MASK != 0 {
            return None;
        }
        Some(ErlTerm((ptr as u64) | TAG_CONS))
    }

    /// Encode an allocator/runtime-proven aligned address without a release check.
    /// # Safety
    /// The address must be 8-byte aligned. This does not promise live storage;
    /// dereferencing the resulting term has its own validity/ownership contract.
    #[inline]
    pub(crate) unsafe fn cons_unchecked(ptr: *const ConsCell) -> Self {
        let addr = ptr as u64;
        debug_assert!(addr & TAG_MASK == 0, "cons pointer must be 8-byte aligned");
        ErlTerm(addr | TAG_CONS)
    }

    /// Encode an 8-byte-aligned boxed address; panic on misalignment in all builds.
    /// This only encodes the address, including null. It does not establish that
    /// storage has a valid runtime layout, is live, or remains owned.
    #[inline]
    pub fn boxed(ptr: *const u64) -> Self {
        Self::try_boxed(ptr).expect("boxed pointer must be 8-byte aligned")
    }

    /// Encode a boxed address only if its tag bits are clear.
    #[inline]
    pub fn try_boxed(ptr: *const u64) -> Option<Self> {
        if ptr as u64 & TAG_MASK != 0 {
            return None;
        }
        Some(ErlTerm((ptr as u64) | TAG_BOXED))
    }

    /// Encode an allocator/runtime-proven aligned address without a release check.
    /// # Safety
    /// The address must be 8-byte aligned. Heap reads separately require live,
    /// initialized storage with the runtime layout and retained owners.
    #[inline]
    pub(crate) unsafe fn boxed_unchecked(ptr: *const u64) -> Self {
        let addr = ptr as u64;
        debug_assert!(addr & TAG_MASK == 0, "boxed pointer must be 8-byte aligned");
        ErlTerm(addr | TAG_BOXED)
    }

    // -- Tag tests ----------------------------------------------------------

    #[inline]
    pub fn tag(&self) -> u64 {
        (self.0 & !EXTERNAL_BIT) & TAG_MASK
    }

    #[inline]
    pub fn is_small_int(&self) -> bool {
        self.tag() == TAG_SMALL_INT
    }

    #[inline]
    pub fn is_atom(&self) -> bool {
        self.tag() == TAG_ATOM
    }

    #[inline]
    pub fn is_pid(&self) -> bool {
        self.tag() == TAG_PID
    }

    #[inline]
    pub fn is_nil(&self) -> bool {
        self.0 == NIL_VALUE
    }

    #[inline]
    pub fn is_cons(&self) -> bool {
        self.tag() == TAG_CONS
    }

    #[inline]
    pub fn is_boxed(&self) -> bool {
        // Boxed tag is 0b000, but nil is 0b101. A raw zero would also
        // match, but that is an invalid term so we accept it.
        let clean = self.0 & !EXTERNAL_BIT;
        self.tag() == TAG_BOXED && clean != 0
    }

    /// Return the boxed subtype tag if this term is boxed.
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn boxed_subtype(&self) -> Option<u64> {
        if !self.is_boxed() {
            return None;
        }
        let header = unsafe { BoxedHeader(*(self.as_boxed_ptr().unwrap())) };
        Some(header.subtype())
    }

    /// True for cons or nil (a proper or improper list head).
    #[inline]
    pub fn is_list(&self) -> bool {
        self.is_cons() || self.is_nil()
    }

    /// True if this is a boxed bigint (reserved layout).
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn is_bigint(&self) -> bool {
        matches!(self.boxed_subtype(), Some(BOXED_BIGINT))
    }

    /// True if this is an on-heap boxed binary.
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn is_heap_binary(&self) -> bool {
        matches!(self.boxed_subtype(), Some(BOXED_HEAP_BINARY))
    }

    /// True if this is a boxed tuple.
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn is_tuple(&self) -> bool {
        matches!(self.boxed_subtype(), Some(BOXED_TUPLE))
    }

    /// True if this term is an immediate value (no heap pointer).
    /// Immediates: small integers, atoms, pids, nil.
    #[inline]
    pub fn is_immediate(&self) -> bool {
        matches!(self.tag(), TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL)
    }

    /// True if this term is a mailbox tombstone sentinel.
    #[inline(always)]
    pub fn is_tombstone(&self) -> bool {
        self.0 == TOMBSTONE_VALUE
    }

    /// True if this is a refc-binary descriptor (reserved layout).
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn is_refc_binary(&self) -> bool {
        matches!(self.boxed_subtype(), Some(BOXED_REFC_BINARY))
    }

    /// True if this is a binary match context (in-place cursor).
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn is_match_ctx(&self) -> bool {
        matches!(self.boxed_subtype(), Some(BOXED_MATCH_CTX))
    }

    /// True if this is a byte-aligned binary (bit_size % 8 == 0).
    /// Does NOT include match contexts — use `is_binary_or_match_ctx()` for that.
    /// For sub-byte bitstrings (e.g., `<<1:3>>`), returns false.
    /// Use `is_bitstring()` to match any binary/bitstring.
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn is_binary(&self) -> bool {
        if let Some(sub) = self.boxed_subtype() {
            if sub == BOXED_HEAP_BINARY || sub == BOXED_REFC_BINARY {
                let hdr_ptr = (self.0 & !(TAG_MASK | EXTERNAL_BIT)) as *const u64;
                let header = unsafe { BoxedHeader(*hdr_ptr) };
                header.arity() % 8 == 0
            } else {
                false
            }
        } else {
            false
        }
    }

    /// True if this is any binary/bitstring representation (heap or refc),
    /// regardless of sub-byte trailing bits. Does NOT include match contexts.
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn is_bitstring(&self) -> bool {
        self.is_heap_binary() || self.is_refc_binary()
    }

    /// True if this is a binary OR a match context (for pattern matching guards).
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn is_binary_or_match_ctx(&self) -> bool {
        self.is_binary() || self.is_match_ctx()
    }

    /// True if this is a boxed closure.
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn is_closure(&self) -> bool {
        matches!(self.boxed_subtype(), Some(BOXED_CLOSURE))
    }

    /// True if this is a boxed map.
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn is_map(&self) -> bool {
        matches!(self.boxed_subtype(), Some(BOXED_MAP))
    }

    /// True if this is a boxed reference.
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn is_reference(&self) -> bool {
        matches!(self.boxed_subtype(), Some(BOXED_REFERENCE))
    }

    // -- Extractors ---------------------------------------------------------

    /// Extract the signed integer value.
    #[inline]
    pub fn as_small_int(&self) -> Option<i64> {
        if self.is_small_int() {
            // Arithmetic right shift to restore sign.
            Some((self.0 as i64) >> TAG_BITS)
        } else {
            None
        }
    }

    /// Extract the atom table index.
    #[inline]
    pub fn as_atom_id(&self) -> Option<u32> {
        if self.is_atom() {
            Some((self.0 >> TAG_BITS) as u32)
        } else {
            None
        }
    }

    /// Extract the process id.
    #[inline]
    pub fn as_pid_id(&self) -> Option<ProcessId> {
        if self.0 & (TAG_MASK | EXTERNAL_BIT) == TAG_PID {
            // SAFETY: the tag/external mask proves the payload fits ProcessId::MAX.
            Some(unsafe { ProcessId::from_valid_raw(self.0 >> TAG_BITS) })
        } else {
            None
        }
    }

    /// Extract the cons cell pointer.
    #[inline]
    pub fn as_cons_ptr(&self) -> Option<*const ConsCell> {
        if self.is_cons() {
            Some((self.0 & !(TAG_MASK | EXTERNAL_BIT)) as *const ConsCell)
        } else {
            None
        }
    }

    /// Extract the boxed object pointer (points to the header word).
    #[inline]
    pub fn as_boxed_ptr(&self) -> Option<*const u64> {
        if self.is_boxed() {
            Some((self.0 & !(TAG_MASK | EXTERNAL_BIT)) as *const u64)
        } else {
            None
        }
    }

    /// Return the byte size of a binary term. Returns `None` if not a binary.
    /// Header arity stores **bit** count; byte size = (bits + 7) / 8.
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn binary_size(&self) -> Option<usize> {
        let subtype = self.boxed_subtype()?;
        if subtype == BOXED_HEAP_BINARY || subtype == BOXED_REFC_BINARY {
            let hdr_ptr = (self.0 & !(TAG_MASK | EXTERNAL_BIT)) as *const u64;
            let header = unsafe { BoxedHeader(*hdr_ptr) };
            let bit_count = header.arity() as usize;
            Some(bit_count.div_ceil(8))
        } else {
            None
        }
    }

    /// Return the bit size of a binary term. Returns `None` if not a binary.
    /// Header arity stores the bit count directly.
    ///
    /// # Safety
    /// If this is a boxed term, its pointer must reference a live, initialized
    /// object with the runtime layout. It must not have been moved by GC.
    #[inline]
    pub unsafe fn binary_bit_size(&self) -> Option<usize> {
        let subtype = self.boxed_subtype()?;
        if subtype == BOXED_HEAP_BINARY || subtype == BOXED_REFC_BINARY {
            let hdr_ptr = (self.0 & !(TAG_MASK | EXTERNAL_BIT)) as *const u64;
            let header = unsafe { BoxedHeader(*hdr_ptr) };
            Some(header.arity() as usize)
        } else {
            None
        }
    }

    /// Return a byte slice over the raw binary data of a **heap** binary.
    ///
    /// Returns `None` for non-binaries and for refc-binary descriptors.
    /// For a unified accessor that handles both types, use
    /// [`binary_bytes_any()`](Self::binary_bytes_any).
    ///
    /// # Safety
    /// The term must reference a valid object, unmoved by GC, for the entire
    /// returned borrow. No bytes may be mutated while the slice is borrowed.
    #[inline]
    pub unsafe fn binary_bytes(&self) -> Option<&[u8]> {
        if !self.is_heap_binary() {
            return None;
        }
        let byte_len = self.binary_size()?;
        let hdr_ptr = (self.0 & !(TAG_MASK | EXTERNAL_BIT)) as *const u64;
        let data_ptr = hdr_ptr.add(1) as *const u8;
        Some(std::slice::from_raw_parts(data_ptr, byte_len))
    }

    /// Return a byte slice over the raw binary data of a **refc** binary.
    ///
    /// The slice is formed by dereferencing the raw `Arc<BinaryData>` pointer
    /// stored in the descriptor and applying the byte offset.
    ///
    /// # Safety
    /// The backing `Arc<BinaryData>` must still be alive (i.e. the owning
    /// process's `refc_binaries` list has not been dropped). The descriptor
    /// must be live and unmoved, its window in bounds, and the bytes immutable
    /// for the entire returned borrow.
    #[inline]
    pub unsafe fn refc_binary_bytes(&self) -> Option<&[u8]> {
        if !self.is_refc_binary() {
            return None;
        }
        let byte_len = self.binary_size()?;
        let hdr_ptr = (self.0 & !(TAG_MASK | EXTERNAL_BIT)) as *const u64;
        let raw_ptr = *hdr_ptr.add(REFC_BINARY_PTR_WORD_INDEX) as *const BinaryData;
        let offset = *hdr_ptr.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
        let vec_ref = &*raw_ptr;
        Some(&vec_ref[offset..offset + byte_len])
    }

    /// Return a byte slice over the raw binary data for **any** binary type
    /// (heap binary or refc binary).
    ///
    /// # Safety
    /// For heap binaries the pointer must be on a valid heap.  For refc
    /// binaries the backing `Arc` must still be alive. Both the object and
    /// backing bytes must remain valid, unmoved and immutable for the entire
    /// returned borrow; any descriptor window must be in bounds.
    #[inline]
    pub unsafe fn binary_bytes_any(&self) -> Option<&[u8]> {
        if self.is_heap_binary() {
            self.binary_bytes()
        } else if self.is_refc_binary() {
            self.refc_binary_bytes()
        } else {
            None
        }
    }

    // -- External bit helpers -----------------------------------------------

    /// True if this term has the EXTERNAL_BIT set (points into ETS storage).
    #[inline]
    pub fn is_external(&self) -> bool {
        self.0 & EXTERNAL_BIT != 0
    }

    /// Return a copy of this term with EXTERNAL_BIT set.
    #[inline]
    pub fn with_external_bit(&self) -> ErlTerm {
        ErlTerm(self.0 | EXTERNAL_BIT)
    }

    /// Return a copy of this term with EXTERNAL_BIT cleared.
    #[inline]
    pub fn strip_external_bit(&self) -> ErlTerm {
        ErlTerm(self.0 & !EXTERNAL_BIT)
    }
}

/// A read-only term view tied to its owning heap's lifetime.
///
/// Construct with [`ErlTerm::borrow`]. The borrow prevents safe Rust from
/// collecting or dropping the heap while these accessors or their returned
/// slices are in use. The view remains one word and performs no rooting,
/// allocation, reference counting or runtime validity checks.
#[derive(Clone, Copy)]
pub struct BorrowedTerm<'heap> {
    term: ErlTerm,
    _heap: PhantomData<&'heap ProcessHeap>,
}

impl BorrowedTerm<'_> {
    /// Create an independently owned message fragment while the source is borrowed.
    #[inline]
    pub fn to_message_fragment(self) -> crate::message::MessageFragment {
        // SAFETY: the view retains the initialized, immutable source graph.
        unsafe { crate::message::MessageFragment::from_term(self.term) }
    }

    /// Deep-copy into an independently owned heap, preserving shared substructure.
    /// The source cannot be collected while the borrowed view is still used.
    /// ```compile_fail,E0502
    /// use nera_runtime::{heap::ProcessHeap, term::ErlTerm};
    /// let mut source = ProcessHeap::new();
    /// let mut target = ProcessHeap::new();
    /// let term = source.alloc_tuple(&[ErlTerm::small_int(42)]);
    /// let view = unsafe { term.borrow(&source) };
    /// unsafe { source.collect_full(&mut [term]); }
    /// let _ = view.copy_to_heap(&mut target);
    /// ```
    #[inline]
    pub fn copy_to_heap(self, target: &mut ProcessHeap) -> ErlTerm {
        // SAFETY: the view borrows the live, immutable source graph. The
        // exclusive target borrow prevents using the source heap as target.
        unsafe { crate::message::deep_copy_term(self.term, target) }
    }

    /// Convert an integer to an owned arbitrary-precision value.
    ///
    /// A conversion cannot use a view across a moving collection:
    /// ```compile_fail,E0502
    /// use nera_runtime::{heap::ProcessHeap, term::ErlTerm};
    /// let mut heap = ProcessHeap::new();
    /// let term = ErlTerm::small_int(1);
    /// let view = unsafe { term.borrow(&heap) };
    /// unsafe { heap.collect_full(&mut [term]); }
    /// let _ = view.to_bigint();
    /// ```
    #[inline]
    pub fn to_bigint(self) -> Option<num_bigint::BigInt> {
        // SAFETY: this view keeps the initialized, immutable storage borrowed.
        unsafe { crate::bigint::term_to_bigint(self.term) }
    }

    /// Convert an integer to a floating-point value, possibly losing precision.
    #[inline]
    pub fn integer_to_f64(self) -> Option<f64> {
        // SAFETY: this view keeps the initialized, immutable storage borrowed.
        unsafe { crate::bigint::bigint_to_f64(self.term) }
    }

    /// Borrow-checked counterpart of [`ErlTerm::boxed_subtype`].
    #[inline]
    pub fn boxed_subtype(&self) -> Option<u64> {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.boxed_subtype() }
    }

    /// Borrow-checked counterpart of [`ErlTerm::is_bigint`].
    #[inline]
    pub fn is_bigint(&self) -> bool {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.is_bigint() }
    }

    /// Borrow-checked counterpart of [`ErlTerm::is_heap_binary`].
    #[inline]
    pub fn is_heap_binary(&self) -> bool {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.is_heap_binary() }
    }

    /// Borrow-checked counterpart of [`ErlTerm::is_tuple`].
    #[inline]
    pub fn is_tuple(&self) -> bool {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.is_tuple() }
    }

    /// Borrow-checked counterpart of [`ErlTerm::is_refc_binary`].
    #[inline]
    pub fn is_refc_binary(&self) -> bool {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.is_refc_binary() }
    }

    /// Borrow-checked counterpart of [`ErlTerm::is_match_ctx`].
    #[inline]
    pub fn is_match_ctx(&self) -> bool {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.is_match_ctx() }
    }

    /// Borrow-checked counterpart of [`ErlTerm::is_binary`].
    #[inline]
    pub fn is_binary(&self) -> bool {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.is_binary() }
    }

    /// Borrow-checked counterpart of [`ErlTerm::is_bitstring`].
    #[inline]
    pub fn is_bitstring(&self) -> bool {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.is_bitstring() }
    }

    /// Borrow-checked counterpart of [`ErlTerm::is_binary_or_match_ctx`].
    #[inline]
    pub fn is_binary_or_match_ctx(&self) -> bool {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.is_binary_or_match_ctx() }
    }

    /// Borrow-checked counterpart of [`ErlTerm::is_closure`].
    #[inline]
    pub fn is_closure(&self) -> bool {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.is_closure() }
    }

    /// Borrow-checked counterpart of [`ErlTerm::is_map`].
    #[inline]
    pub fn is_map(&self) -> bool {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.is_map() }
    }

    /// Borrow-checked counterpart of [`ErlTerm::is_reference`].
    #[inline]
    pub fn is_reference(&self) -> bool {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.is_reference() }
    }

    /// Borrow-checked counterpart of [`ErlTerm::binary_size`].
    #[inline]
    pub fn binary_size(&self) -> Option<usize> {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.binary_size() }
    }

    /// Borrow-checked counterpart of [`ErlTerm::binary_bit_size`].
    #[inline]
    pub fn binary_bit_size(&self) -> Option<usize> {
        // SAFETY: construction binds a valid term to its live, borrowed owner.
        unsafe { self.term.binary_bit_size() }
    }

    /// Read either inline or reference-counted binary bytes.
    /// The slice cannot outlive the view's borrow of the heap.
    #[inline]
    pub fn binary_bytes(&self) -> Option<&[u8]> {
        // SAFETY: the constructor guarantees live, immutable backing storage
        // throughout the heap borrow, including any reference-counted owner.
        unsafe { self.term.binary_bytes_any() }
    }
}

// ---------------------------------------------------------------------------
// Debug
// ---------------------------------------------------------------------------

impl fmt::Debug for ErlTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ext = if self.is_external() { "Ext:" } else { "" };
        if self.is_nil() {
            write!(f, "Nil")
        } else if self.is_small_int() {
            write!(f, "SmallInt({})", self.as_small_int().unwrap())
        } else if self.is_atom() {
            write!(f, "Atom({})", self.as_atom_id().unwrap())
        } else if self.is_pid() {
            write!(f, "Pid({})", self.as_pid_id().unwrap())
        } else if self.is_cons() {
            write!(f, "{}Cons@{:#x}", ext, self.0 & !(TAG_MASK | EXTERNAL_BIT))
        } else if self.is_boxed() {
            write!(f, "{}Boxed@{:#x}", ext, self.0 & !(TAG_MASK | EXTERNAL_BIT))
        } else {
            write!(f, "Unknown({:#x})", self.0)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_id_domain_preserves_all_accepted_bits() {
        assert_eq!(ProcessId::MAX, (1u64 << 60) - 1);
        for value in [0, 1, 1u64 << 32, ProcessId::MAX] {
            let id = ProcessId::try_from(value).unwrap();
            assert_eq!(id.get(), value);
            let term = ErlTerm::pid(id);
            assert_eq!(term.to_raw(), (value << 3) | TAG_PID);
            assert_eq!(term.as_pid_id(), Some(id));
            assert!(!term.is_external());
            // SAFETY: immediate PIDs contain no heap pointers.
            let copied = unsafe { crate::message::MessageFragment::from_term(term) }
                .copy_to_heap(&mut ProcessHeap::new());
            assert_eq!(copied.as_pid_id(), Some(id));
        }
        for value in [ProcessId::MAX + 1, 1u64 << 61, u64::MAX] {
            assert_eq!(ProcessId::try_new(value), None);
            assert_eq!(ProcessId::try_from(value), Err(crate::process::InvalidProcessId(value)));
        }
        let marked = ErlTerm::from_raw(EXTERNAL_BIT | TAG_PID);
        assert!(marked.as_pid_id().is_none(), "reserved bits must not forge a ProcessId");
    }

    #[test]
    fn process_id_domain_rejects_reserved_or_truncated_bits() {
        for value in [1u64 << 60, 1u64 << 61, u64::MAX] {
            assert!(std::panic::catch_unwind(|| ProcessId::new(value)).is_err());
        }
    }

    #[test]
    fn boxed_header_rejects_oversized_arity() {
        let result = std::panic::catch_unwind(|| BoxedHeader::new(BoxedSubtype::HeapBinary, (u64::MAX >> BOXED_TAG_BITS) + 1));
        assert!(result.is_err(), "oversized header arity must not silently truncate");
    }

    #[test]
    fn small_int_rejects_out_of_domain_values() {
        for value in [SMALL_INT_MIN - 1, SMALL_INT_MAX + 1, i64::MIN, i64::MAX] {
            assert_eq!(ErlTerm::try_small_int(value), None);
            assert!(std::panic::catch_unwind(|| ErlTerm::small_int(value)).is_err());
        }
        for value in [SMALL_INT_MIN, 0, SMALL_INT_MAX] {
            assert_eq!(ErlTerm::small_int(value).as_small_int(), Some(value));
        }
    }

    #[test]
    fn borrowed_views_follow_gc_roots_and_binary_windows() {
        assert_eq!(std::mem::size_of::<BorrowedTerm<'_>>(), 8);
        let mut heap = ProcessHeap::new();
        let owner = std::sync::Arc::new(BinaryData::new((0..128).collect()));
        let term = heap.alloc_refc_binary_from_arc(std::sync::Arc::clone(&owner), 17, 80);
        let empty =
            heap.alloc_refc_binary_from_arc(owner, 128, 0);
        let mut roots = [term, empty];
        unsafe { heap.collect_full(&mut roots) };
        // SAFETY: these are the collector-updated roots owned by heap.
        let view = unsafe { roots[0].borrow(&heap) };
        assert!(view.is_refc_binary());
        assert!(view.is_binary());
        assert_eq!(view.binary_bit_size(), Some(640));
        assert_eq!(view.binary_bytes().unwrap(), &(17..97).collect::<Vec<u8>>());
        // SAFETY: the second updated root owns an empty window at the end.
        let empty_view = unsafe { roots[1].borrow(&heap) };
        assert_eq!(empty_view.binary_bytes(), Some([].as_slice()));
    }

    #[test]
    fn raw_bits_and_debug_do_not_require_live_heap_objects() {
        assert_eq!(std::mem::size_of::<ErlTerm>(), std::mem::size_of::<u64>());
        assert_eq!(std::mem::align_of::<ErlTerm>(), std::mem::align_of::<u64>());
        // Includes null, an invalid boxed address, sentinels and external bits.
        for bits in [0, 8, 9, TOMBSTONE_VALUE, EXTERNAL_BIT | 8, u64::MAX] {
            let term = ErlTerm::from_raw(bits);
            assert_eq!(term.to_raw(), bits);
            let _ = term.tag();
            let _ = term.as_boxed_ptr();
            let _ = term.as_cons_ptr();
            let _ = format!("{term:?}");
        }
    }

    #[test]
    fn small_int_roundtrip() {
        for val in [0i64, 1, -1, 42, -42, i32::MAX as i64, i32::MIN as i64] {
            let term = ErlTerm::small_int(val);
            assert!(term.is_small_int());
            assert!(!term.is_atom());
            assert!(!term.is_cons());
            assert!(!term.is_nil());
            assert!(!term.is_pid());
            assert_eq!(term.as_small_int(), Some(val));
        }
    }

    #[test]
    fn atom_roundtrip() {
        for id in [0u32, 1, 42, 1000, u32::MAX >> 3] {
            let term = ErlTerm::atom(id);
            assert!(term.is_atom());
            assert!(!term.is_small_int());
            assert_eq!(term.as_atom_id(), Some(id));
        }
    }

    #[test]
    fn pid_roundtrip() {
        for id in [0u64, 1, 100, 0xFFFF_FFFF] {
            let term = ErlTerm::pid(crate::process::ProcessId::new(id));
            assert!(term.is_pid());
            assert!(!term.is_small_int());
            assert_eq!(term.as_pid_id(), Some(crate::process::ProcessId::new(id)));
        }
    }

    #[test]
    fn nil_term() {
        let term = ErlTerm::nil();
        assert!(term.is_nil());
        assert!(term.is_list());
        assert!(!term.is_cons());
        assert!(!term.is_small_int());
        assert!(!term.is_atom());
    }

    #[test]
    fn pointer_encoding_rejects_every_misalignment_in_release() {
        let storage = [0u64; 2];
        let bytes = storage.as_ptr().cast::<u8>();
        for offset in 1..8 {
            // Both addresses are within live storage; they are only encoded,
            // never dereferenced. RED is safe even if the constructor accepts it.
            let pointer = bytes.wrapping_add(offset);
            let boxed = std::panic::catch_unwind(|| ErlTerm::boxed(pointer.cast()));
            assert!(boxed.is_err(), "boxed accepted byte offset {offset}");
            let cons = std::panic::catch_unwind(|| ErlTerm::cons(pointer.cast()));
            assert!(cons.is_err(), "cons accepted byte offset {offset}");
        }
    }

    #[test]
    fn checked_pointer_encoding_preserves_aligned_addresses_and_tags() {
        let storage = [0u64; 2];
        let bytes = storage.as_ptr().cast::<u8>();
        for offset in 0..8 {
            let pointer = bytes.wrapping_add(offset);
            let boxed = ErlTerm::try_boxed(pointer.cast());
            let cons = ErlTerm::try_cons(pointer.cast());
            if offset == 0 {
                assert_eq!(boxed.unwrap().as_boxed_ptr(), Some(pointer.cast()));
                assert_eq!(cons.unwrap().as_cons_ptr(), Some(pointer.cast()));
                // SAFETY: the array base has 8-byte alignment; encoding only.
                assert_eq!(boxed, Some(unsafe { ErlTerm::boxed_unchecked(pointer.cast()) }));
                assert_eq!(cons, Some(unsafe { ErlTerm::cons_unchecked(pointer.cast()) }));
            } else {
                assert_eq!(boxed, None);
                assert_eq!(cons, None);
            }
        }
        // Encoding null does not authorize dereferencing it.
        assert_eq!(ErlTerm::try_boxed(std::ptr::null()), Some(ErlTerm::boxed(std::ptr::null())));
        assert_eq!(ErlTerm::try_cons(std::ptr::null()), Some(ErlTerm::cons(std::ptr::null())));
    }

    #[test]
    fn cons_roundtrip() {
        let cell = ConsCell {
            head: ErlTerm::small_int(1),
            tail: ErlTerm::nil(),
        };
        let ptr: *const ConsCell = &cell;
        let term = ErlTerm::cons(ptr);
        assert!(term.is_cons());
        assert!(term.is_list());
        assert!(!term.is_nil());
        assert_eq!(term.as_cons_ptr(), Some(ptr));

        // Dereference and check contents.
        unsafe {
            let cell_ref = &*term.as_cons_ptr().unwrap();
            assert_eq!(cell_ref.head.as_small_int(), Some(1));
            assert!(cell_ref.tail.is_nil());
        }
    }

    #[test]
    fn boxed_roundtrip() {
        // Simulate a tuple {10, 20} on the stack: [header, 10, 20]
        #[repr(align(8))]
        struct Aligned([u64; 3]);
        let mut data = Aligned([0u64; 3]);
        data.0[0] = BoxedHeader::new(BoxedSubtype::Tuple, 2).0;
        data.0[1] = ErlTerm::small_int(10).0;
        data.0[2] = ErlTerm::small_int(20).0;

        let ptr: *const u64 = data.0.as_ptr();
        let term = ErlTerm::boxed(ptr);
        assert!(term.is_boxed());
        assert!(!term.is_cons());

        unsafe {
            let header = BoxedHeader(*term.as_boxed_ptr().unwrap());
            assert_eq!(header.subtype(), BOXED_TUPLE);
            assert_eq!(header.arity(), 2);

            let elem0 = ErlTerm(*term.as_boxed_ptr().unwrap().add(1));
            let elem1 = ErlTerm(*term.as_boxed_ptr().unwrap().add(2));
            assert_eq!(elem0.as_small_int(), Some(10));
            assert_eq!(elem1.as_small_int(), Some(20));
        }
    }

    #[test]
    fn boxed_subtype_abi_and_checked_decoding() {
        let kinds = [
            BoxedSubtype::Tuple, BoxedSubtype::Float, BoxedSubtype::BigInt,
            BoxedSubtype::HeapBinary, BoxedSubtype::Closure, BoxedSubtype::Map,
            BoxedSubtype::RefcBinary, BoxedSubtype::Reference, BoxedSubtype::MatchContext,
        ];
        assert_eq!(std::mem::size_of::<BoxedSubtype>(), 8);
        for (raw, kind) in (0u64..=8).zip(kinds) {
            assert_eq!(kind as u64, raw);
            assert_eq!(BoxedSubtype::try_from(raw), Ok(kind));
            for arity in [0, 255, u64::MAX >> 4] {
                assert_eq!(BoxedHeader::new(kind, arity).0, (arity << 4) | raw);
            }
        }
        for raw in (9..=16).chain([0x100, u64::MAX]) {
            assert_eq!(BoxedSubtype::try_from(raw), Err(InvalidBoxedSubtype(raw)));
        }
        assert_eq!(BOXED_BINARY, BOXED_HEAP_BINARY);
        assert!(BoxedHeader::try_new(BoxedSubtype::Tuple, 1 << 60).is_none());
    }

    #[test]
    fn boxed_header_resize_preserves_raw_tags_and_checks_arity() {
        for raw in 0..=15 {
            let header = BoxedHeader((128 << 4) | raw);
            for arity in [0, 64, u64::MAX >> 4] {
                let resized = header.with_arity(arity);
                assert_eq!(resized.0, (arity << 4) | raw);
                assert_eq!(resized.subtype(), raw);
                assert_eq!(resized.arity(), arity);
            }
            assert!(header.try_with_arity(1 << 60).is_none());
            assert!(std::panic::catch_unwind(|| header.with_arity(1 << 60)).is_err());
        }
    }

    #[test]
    fn boxed_header_subtypes() {
        let h = BoxedHeader::new(BoxedSubtype::Float, 1);
        assert_eq!(h.subtype(), BOXED_FLOAT);
        assert_eq!(h.arity(), 1);

        let h = BoxedHeader::new(BoxedSubtype::Tuple, 255);
        assert_eq!(h.subtype(), BOXED_TUPLE);
        assert_eq!(h.arity(), 255);
    }

    #[test]
    fn boxed_refc_binary_is_binary_without_inline_bytes() {
        #[repr(align(8))]
        struct Aligned([u64; REFC_BINARY_TOTAL_WORDS]);
        let mut data = Aligned([0; REFC_BINARY_TOTAL_WORDS]);
        data.0[0] = BoxedHeader::new(BoxedSubtype::RefcBinary, 16 * 8).0; // arity = bits
        data.0[REFC_BINARY_PTR_WORD_INDEX] = 0xDEAD_BEEF;
        data.0[REFC_BINARY_OFFSET_WORD_INDEX] = 4;

        let term = ErlTerm::boxed(data.0.as_ptr());
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(term.is_binary());
            assert!(term.is_refc_binary());
            assert!(!term.is_heap_binary());
            assert_eq!(term.binary_size(), Some(16));
        }
        unsafe {
            assert!(term.binary_bytes().is_none());
        }
    }

    #[test]
    fn debug_formatting() {
        assert_eq!(format!("{:?}", ErlTerm::small_int(42)), "SmallInt(42)");
        assert_eq!(format!("{:?}", ErlTerm::atom(0)), "Atom(0)");
        assert_eq!(format!("{:?}", ErlTerm::nil()), "Nil");
        assert_eq!(format!("{:?}", ErlTerm::pid(crate::process::ProcessId::new(7))), "Pid(7)");
    }

    #[test]
    fn negative_small_int() {
        let term = ErlTerm::small_int(-100);
        assert!(term.is_small_int());
        assert_eq!(term.as_small_int(), Some(-100));
    }

    #[test]
    fn extractor_returns_none_on_wrong_tag() {
        let int_term = ErlTerm::small_int(5);
        assert_eq!(int_term.as_atom_id(), None);
        assert_eq!(int_term.as_pid_id(), None);
        assert_eq!(int_term.as_cons_ptr(), None);
        assert_eq!(int_term.as_boxed_ptr(), None);

        let atom_term = ErlTerm::atom(1);
        assert_eq!(atom_term.as_small_int(), None);
    }

    // -----------------------------------------------------------------------
    // Property-based tests (proptest)
    // -----------------------------------------------------------------------

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn small_int_round_trip(v in SMALL_INT_MIN..=SMALL_INT_MAX) {
                let term = ErlTerm::small_int(v);
                prop_assert!(term.is_small_int(), "is_small_int false for {}", v);
                prop_assert!(!term.is_atom());
                prop_assert!(!term.is_nil());
                prop_assert!(!term.is_pid());
                prop_assert!(!term.is_cons());
                prop_assert!(term.is_immediate());
                prop_assert_eq!(term.as_small_int(), Some(v));
            }

            #[test]
            fn atom_round_trip(id in 0u32..1_000_000) {
                let term = ErlTerm::atom(id);
                prop_assert!(term.is_atom(), "is_atom false for id={}", id);
                prop_assert!(!term.is_small_int());
                prop_assert!(!term.is_nil());
                prop_assert!(!term.is_pid());
                prop_assert!(term.is_immediate());
                prop_assert_eq!(term.as_atom_id(), Some(id));
            }

            #[test]
            fn pid_round_trip(id in 0u64..(1u64 << 60)) {
                let term = ErlTerm::pid(crate::process::ProcessId::new(id));
                prop_assert!(term.is_pid(), "is_pid false for id={}", id);
                prop_assert!(!term.is_small_int());
                prop_assert!(!term.is_atom());
                prop_assert!(!term.is_nil());
                prop_assert!(term.is_immediate());
                prop_assert_eq!(term.as_pid_id(), Some(ProcessId::new(id)));
            }

            #[test]
            fn boxed_header_round_trip(
                subtype in 0u64..=0x8,
                arity in 0u64..(1u64 << 58),
            ) {
                let hdr = BoxedHeader::new(BoxedSubtype::try_from(subtype).unwrap(), arity);
                prop_assert_eq!(hdr.subtype(), subtype, "subtype mismatch");
                prop_assert_eq!(hdr.arity(), arity, "arity mismatch");
            }

            #[test]
            fn fits_small_int_consistent(v in prop::num::i64::ANY) {
                let fits = fits_small_int(v);
                prop_assert_eq!(fits, (SMALL_INT_MIN..=SMALL_INT_MAX).contains(&v));
                if fits {
                    let term = ErlTerm::small_int(v);
                    prop_assert_eq!(term.as_small_int(), Some(v),
                        "round-trip failed for v={}", v);
                }
            }

            #[test]
            fn tag_discrimination_small_int(v in SMALL_INT_MIN..=SMALL_INT_MAX) {
                // Exactly one tag predicate must be true for any immediate.
                let term = ErlTerm::small_int(v);
                let tags = [
                    term.is_small_int(),
                    term.is_atom(),
                    term.is_pid(),
                    term.is_nil(),
                    term.is_cons(),
                    term.is_boxed(),
                ];
                let true_count = tags.iter().filter(|&&b| b).count();
                prop_assert_eq!(true_count, 1,
                    "expected exactly 1 tag, got {} for SmallInt({})", true_count, v);
            }

            #[test]
            fn tag_discrimination_atom(id in 0u32..100_000) {
                let term = ErlTerm::atom(id);
                let tags = [
                    term.is_small_int(),
                    term.is_atom(),
                    term.is_pid(),
                    term.is_nil(),
                    term.is_cons(),
                    term.is_boxed(),
                ];
                let true_count = tags.iter().filter(|&&b| b).count();
                prop_assert_eq!(true_count, 1,
                    "expected exactly 1 tag, got {} for Atom({})", true_count, id);
            }

            #[test]
            fn tag_discrimination_pid(id in 0u64..(1u64 << 60)) {
                let term = ErlTerm::pid(crate::process::ProcessId::new(id));
                let tags = [
                    term.is_small_int(),
                    term.is_atom(),
                    term.is_pid(),
                    term.is_nil(),
                    term.is_cons(),
                    term.is_boxed(),
                ];
                let true_count = tags.iter().filter(|&&b| b).count();
                prop_assert_eq!(true_count, 1,
                    "expected exactly 1 tag, got {} for Pid({})", true_count, id);
            }

            #[test]
            fn small_int_preserves_sign(v in SMALL_INT_MIN..=SMALL_INT_MAX) {
                let decoded = ErlTerm::small_int(v).as_small_int().unwrap();
                if v < 0 {
                    prop_assert!(decoded < 0, "sign lost: encoded {}, got {}", v, decoded);
                } else if v > 0 {
                    prop_assert!(decoded > 0, "sign lost: encoded {}, got {}", v, decoded);
                } else {
                    prop_assert_eq!(decoded, 0);
                }
            }
        }
    }
}
