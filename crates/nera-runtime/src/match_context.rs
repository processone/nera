//! Validated, immutable matching windows. The repr(C) descriptor is also the
//! generated-code ABI; its fields can only be established through this contract.
use crate::term::{BoxedHeader, BoxedSubtype, ErlTerm, BOXED_BINARY, BOXED_REFC_BINARY, BOXED_MATCH_CTX};

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct MatchContext {
    header: u64,
    binary: ErlTerm,
    offset: u64,
    end: u64,
}

pub const BINARY_OFFSET: i32 = std::mem::offset_of!(MatchContext, binary) as i32;
pub const BIT_OFFSET: i32 = std::mem::offset_of!(MatchContext, offset) as i32;
pub const END_OFFSET: i32 = std::mem::offset_of!(MatchContext, end) as i32;
pub const BYTE_SIZE: usize = std::mem::size_of::<MatchContext>();

impl MatchContext {
    /// Check the source and offset <= end <= source bit length before allocation.
    /// # Safety
    /// Source storage is a live initialized runtime graph, immutable and unmoved
    /// for this call. This value retains raw terms, not ownership of their heap;
    /// subsequent unsafe graph access still requires that source to remain live.
    #[inline]
    pub unsafe fn new(binary: ErlTerm, offset: u64, end: u64) -> Option<Self> {
        let bits = binary.binary_bit_size()? as u64;
        if offset > end || end > bits { return None; }
        Some(Self { header: BoxedHeader::new(BoxedSubtype::MatchContext, 3).0, binary, offset, end })
    }

    /// Copy a descriptor, or make a full window over a plain binary/bitstring.
    /// # Safety
    /// Term is a live, initialized and immutable graph for this call. A matching
    /// context has this layout and was constructed with its range invariants.
    /// No borrowed pointer is retained by the returned value.
    #[inline]
    pub(crate) unsafe fn view(term: ErlTerm) -> Option<Self> {
        let ptr = term.as_boxed_ptr()?;
        let header = BoxedHeader(*ptr);
        match header.subtype() {
            BOXED_MATCH_CTX => Some(ptr.cast::<Self>().read()),
            BOXED_BINARY | BOXED_REFC_BINARY => Some(Self {
                header: BoxedHeader::new(BoxedSubtype::MatchContext, 3).0,
                binary: term, offset: 0, end: header.arity(),
            }),
            _ => None,
        }
    }

    #[inline] pub fn binary(self) -> ErlTerm { self.binary }
    #[inline] pub fn offset(self) -> u64 { self.offset }
    #[inline] pub fn end(self) -> u64 { self.end }
    #[inline] pub fn remaining(self) -> u64 { self.end - self.offset }

    /// Resolve a relative range within the logical window, without wrapping.
    #[inline]
    pub fn resolve(self, relative: usize, count: usize) -> Option<usize> {
        let remaining = usize::try_from(self.remaining()).ok()?.checked_sub(relative)?;
        if count > remaining { return None; }
        usize::try_from(self.offset).ok()?.checked_add(relative)
    }

    /// Advance a snapshot only when the new position still belongs to its window.
    #[inline]
    pub fn advanced(mut self, count: u64) -> Option<Self> {
        if count > self.remaining() { return None; }
        self.offset += count;
        Some(self)
    }

    /// Rebind only the traced pointer after GC; the logical range is unchanged.
    /// # Safety
    /// Binary is the collector's relocated copy of the original source, with
    /// identical logical size. It is initialized before this descriptor is used.
    #[inline]
    pub(crate) unsafe fn relocated(mut self, binary: ErlTerm) -> Self {
        self.binary = binary;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn context_range_and_traced_source_survive_moving_gc() {
        for size in [16,256] {
            let mut heap = crate::heap::ProcessHeap::new();
            let binary = heap.alloc_binary_auto(&vec![0xa5;size]);
            let context = unsafe { MatchContext::new(binary,3,99) }.unwrap();
            assert!(context.advanced(97).is_none());
            assert_eq!(context.advanced(96).unwrap().remaining(),0);
            let mut roots = [heap.alloc_match_ctx(context)];
            unsafe { heap.collect_full(&mut roots); }
            unsafe { heap.collect_full(&mut roots); }
            let copied = unsafe { MatchContext::view(roots[0]) }.unwrap();
            assert_eq!((copied.offset(),copied.end(),copied.remaining()),(3,99,96));
            assert_eq!(unsafe { copied.binary().binary_bit_size() },Some(size*8));
        }
    }

    #[test]
    fn checks_windows_and_resolves_without_overflow() {
        let mut heap = crate::heap::ProcessHeap::new();
        let binary = heap.alloc_binary(&[0; 16]);
        unsafe {
            for (offset,end) in [(25,24),(0,129),(u64::MAX,u64::MAX)] {
                assert!(MatchContext::new(binary,offset,end).is_none());
            }
            assert!(MatchContext::new(ErlTerm::nil(),0,0).is_none());
            let window = MatchContext::new(binary,3,19).unwrap();
            assert_eq!(window.remaining(),16);
            assert_eq!(window.resolve(0,16),Some(3));
            assert_eq!(window.resolve(16,0),Some(19));
            assert_eq!(window.resolve(17,0),None);
            assert_eq!(window.resolve(0,17),None);
            assert_eq!(window.resolve(usize::MAX,1),None);
        }
        assert_eq!(BYTE_SIZE,4 * std::mem::size_of::<u64>());
        assert_eq!((BINARY_OFFSET,BIT_OFFSET,END_OFFSET),(8,16,24));
    }
}
