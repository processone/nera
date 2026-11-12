//! mmap-backed bump regions for ETS and persistent terms.
//!
//! The pool owns cursors; each returned allocation retains its mapping.
//! Outstanding allocations therefore survive pool clear/drop and table deletion.

use std::sync::Arc;

const DEFAULT_REGION_SIZE: usize = 2 * 1024 * 1024;

/// Owner of one mapping. Its address and capacity never change.
/// Raw word accesses require the caller to own the allocated range and to
/// finish initialization before publishing it to readers.
pub struct EtsRegion {
    base: *mut u8,
    capacity: usize,
}

// SAFETY: the mapping is owned, has no thread affinity, and is released only
// by the final owner's Drop. Moving the owner does not move mapped words.
unsafe impl Send for EtsRegion {}
// SAFETY: shared methods do not mutate or expose Rust references to the words.
// The pool assigns disjoint ranges under &mut self; raw-pointer users must
// enforce initialization/read exclusivity. Arc keeps the entire mapping live.
unsafe impl Sync for EtsRegion {}

impl EtsRegion {
    fn new(size: usize) -> Option<Self> {
        let base = mmap_alloc(size);
        if base.is_null() {
            None
        } else {
            Some(Self {
                base,
                capacity: size,
            })
        }
    }
}

impl Drop for EtsRegion {
    fn drop(&mut self) {
        munmap_free(self.base, self.capacity);
    }
}

/// A disjoint word reservation that keeps its mapping alive.
/// Fields are private so a pointer cannot be paired with a different owner.
/// Raw accesses must respect the reserved length and initialization/aliasing.
/// Moving this handle does not move its words; it is deliberately not Clone.
pub struct EtsAllocation {
    _region: Arc<EtsRegion>,
    words: std::ptr::NonNull<u64>,
    len_words: usize,
}

// SAFETY: moving transfers the exclusive reservation and its mapping owner.
// It has no thread affinity; raw accesses still require initialization and
// aliasing guarantees. Shared access is not automatically Sync.
unsafe impl Send for EtsAllocation {}

impl EtsAllocation {
    #[inline]
    pub fn len_words(&self) -> usize { self.len_words }

    #[inline]
    pub fn as_ptr(&self) -> *const u64 { self.words.as_ptr() }

    /// Pointer to uninitialized reserved words. Dereferencing is unsafe;
    /// no access may outlive this owner or exceed len_words().
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut u64 { self.words.as_ptr() }
}

/// The cursor belongs to the pool, not to the shared mapping owner.
struct RegionSlot {
    region: Option<Arc<EtsRegion>>,
    used: usize,
}

impl RegionSlot {
    fn new(size: usize) -> Self {
        Self {
            region: Some(Arc::new(
                EtsRegion::new(size).expect("EtsRegionPool: mmap failed"),
            )),
            used: 0,
        }
    }

    fn try_alloc(&mut self, n_bytes: usize) -> Option<EtsAllocation> {
        debug_assert!(n_bytes.is_multiple_of(8));
        let region = self.region.as_ref()?;
        if n_bytes > region.capacity - self.used {
            return None;
        }
        // SAFETY: the checked range lies in this live mapping and is word-aligned.
        let ptr = unsafe { region.base.add(self.used).cast() };
        self.used += n_bytes;
        Some(EtsAllocation {
            _region: Arc::clone(region),
            // SAFETY: a successful mapping and its aligned in-bounds offset are non-null.
            words: unsafe { std::ptr::NonNull::new_unchecked(ptr) },
            len_words: n_bytes / std::mem::size_of::<u64>(),
        })
    }
}

/// Pool-owned cursors with independently owned mappings.
pub struct EtsRegionPool {
    regions: Vec<RegionSlot>,
    current: usize,
}

impl Default for EtsRegionPool {
    fn default() -> Self {
        Self::new()
    }
}

impl EtsRegionPool {
    pub fn new() -> Self {
        Self {
            regions: Vec::new(),
            current: 0,
        }
    }

    /// Reserve a disjoint word range and return its owning handle.
    /// Its pointers remain valid while the returned allocation remains alive.
    /// Reading or writing it is unsafe and must respect initialization/aliasing.
    /// Allocation ownership cannot be split from its pointer by tuple destructuring:
    /// ```compile_fail
    /// use nera_runtime::ets_pool::EtsRegionPool;
    /// let mut pool = EtsRegionPool::new();
    /// let (owner, pointer) = pool.alloc(8);
    /// ```
    pub fn alloc(&mut self, n_words: usize) -> EtsAllocation {
        let n_bytes = n_words.checked_mul(8).expect("ETS allocation overflow");
        if let Some(slot) = self.regions.get_mut(self.current) {
            if let Some(allocation) = slot.try_alloc(n_bytes) {
                return allocation;
            }
        }
        let size = if n_bytes > DEFAULT_REGION_SIZE {
            n_bytes.checked_add(4095).expect("ETS region size overflow") & !4095
        } else {
            DEFAULT_REGION_SIZE
        };
        let mut slot = RegionSlot::new(size);
        let allocation = slot
            .try_alloc(n_bytes)
            .expect("new region has sufficient capacity");
        self.regions.push(slot);
        self.current = self.regions.len() - 1;
        allocation
    }

    /// Release pool ownership. Outstanding allocations keep their mappings live.
    pub fn clear(&mut self) {
        self.regions.clear();
        self.current = 0;
    }

    /// Release mappings with no owners outside the pool. Arc's refcount replaces
    /// the old separate live_count; allocation/drop still use one increment/decrement.
    pub fn reclaim_dead_regions(&mut self) {
        for slot in &mut self.regions {
            if slot
                .region
                .as_ref()
                .is_some_and(|r| Arc::strong_count(r) == 1)
            {
                slot.region = None;
                slot.used = 0;
            }
        }
    }

    /// Bytes retained by this pool (excludes mappings retained only by borrowers).
    pub fn mapped_bytes(&self) -> usize {
        self.regions
            .iter()
            .filter_map(|s| s.region.as_ref())
            .map(|r| r.capacity)
            .sum()
    }
    pub fn used_bytes(&self) -> usize {
        self.regions.iter().map(|s| s.used).sum()
    }
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }
    pub fn live_region_count(&self) -> usize {
        self.regions.iter().filter(|s| s.region.is_some()).count()
    }
}

// ---------------------------------------------------------------------------
// Platform-specific mmap/munmap
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn mmap_alloc(size: usize) -> *mut u8 {
    unsafe {
        let ptr = libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        );
        if ptr == libc::MAP_FAILED {
            std::ptr::null_mut()
        } else {
            ptr as *mut u8
        }
    }
}

#[cfg(unix)]
fn munmap_free(ptr: *mut u8, size: usize) {
    unsafe {
        libc::munmap(ptr as *mut libc::c_void, size);
    }
}

#[cfg(windows)]
fn mmap_alloc(size: usize) -> *mut u8 {
    use std::ffi::c_void;
    const MEM_COMMIT: u32 = 0x1000;
    const MEM_RESERVE: u32 = 0x2000;
    const PAGE_READWRITE: u32 = 0x04;
    extern "system" {
        fn VirtualAlloc(addr: *mut c_void, size: usize, typ: u32, prot: u32) -> *mut c_void;
    }
    unsafe {
        let ptr = VirtualAlloc(
            std::ptr::null_mut(),
            size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if ptr.is_null() {
            std::ptr::null_mut()
        } else {
            ptr as *mut u8
        }
    }
}

#[cfg(windows)]
fn munmap_free(ptr: *mut u8, _size: usize) {
    use std::ffi::c_void;
    const MEM_RELEASE: u32 = 0x8000;
    extern "system" {
        fn VirtualFree(addr: *mut c_void, size: usize, typ: u32) -> i32;
    }
    unsafe {
        VirtualFree(ptr as *mut c_void, 0, MEM_RELEASE);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservations_keep_their_bounds_and_do_not_overlap() {
        let mut pool = EtsRegionPool::new();
        let mut first = pool.alloc(3);
        let mut second = pool.alloc(5);
        let empty = pool.alloc(0);
        assert_eq!((first.len_words(), second.len_words(), empty.len_words()), (3, 5, 0));
        let a = first.as_mut_ptr();
        let b = second.as_mut_ptr();
        // SAFETY: both reservations are live, exclusive and within one mapping.
        unsafe {
            assert_eq!(a.add(3), b);
            *a.add(2) = 42;
            *b = 99;
        }
        pool.clear();
        drop(pool);
        // SAFETY: the two allocation handles independently retain their ranges.
        unsafe { assert_eq!((*a.add(2), *b), (42, 99)); }
    }

    #[test]
    fn allocation_returns_an_owning_handle() {
        fn assert_owner<T>(_: &T) {
            assert!(
                std::mem::needs_drop::<T>(),
                "a pool allocation must carry a dropping owner, not a borrowed raw pointer"
            );
        }
        let mut pool = EtsRegionPool::new();
        let allocation = pool.alloc(8);
        assert_owner(&allocation);
    }

    #[test]
    fn test_region_alloc_exhaustion() {
        let mut slot = RegionSlot::new(4096);
        let mut first = slot.try_alloc(4088).unwrap();
        let ptr = first.as_mut_ptr();
        assert!(slot.try_alloc(16).is_none());
        let mut last_owner = slot.try_alloc(8).unwrap();
        let last = last_owner.as_mut_ptr();
        assert_eq!(slot.used, 4096);
        unsafe {
            *ptr = 42;
            *last = 99;
        }
        drop(slot);
        unsafe {
            assert_eq!(*ptr, 42);
            assert_eq!(*last, 99);
        }
        drop(first);
    }

    #[test]
    fn pool_drop_keeps_allocation_alive_across_threads() {
        let mut pool = EtsRegionPool::new();
        drop(pool.alloc(3)); // Ensure the retained reservation is not at region base.
        let mut owner = pool.alloc(2);
        let ptr = owner.as_mut_ptr();
        unsafe {
            *ptr = 42;
            *ptr.add(1) = 99;
        }
        let weak = Arc::downgrade(&owner._region);
        pool.clear();
        drop(pool);
        std::thread::spawn(move || {
            // This owner retains the mapping, with no pool/table left alive.
            let words = owner.as_ptr();
            unsafe {
                assert_eq!(*words, 42);
                assert_eq!(*words.add(1), 99);
            }
        })
        .join()
        .unwrap();
        assert!(
            weak.upgrade().is_none(),
            "final owner must release the region"
        );
    }

    #[test]
    fn test_pool_basic_alloc() {
        let mut pool = EtsRegionPool::new();

        // First alloc creates a region
        let mut allocation = pool.alloc(10);
        let word_ptr = allocation.as_mut_ptr(); // 10 words = 80 bytes
        assert_eq!(Arc::strong_count(&allocation._region), 2);
        assert!(!word_ptr.is_null());
        assert_eq!(pool.region_count(), 1);

        // Write and read back
        unsafe {
            *word_ptr = 0xDEAD_BEEF;
            *(word_ptr.add(1)) = 0xCAFE_BABE;
            assert_eq!(*word_ptr, 0xDEAD_BEEF);
            assert_eq!(*(word_ptr.add(1)), 0xCAFE_BABE);
        }

        // One allocation owner in addition to the pool.
        assert_eq!(Arc::strong_count(&allocation._region) - 1, 1);
    }

    #[test]
    fn test_pool_pointer_stability() {
        let mut pool = EtsRegionPool::new();

        // Allocate from first region and save the region pointer
        let mut rp1 = pool.alloc(10);
        let wp1 = rp1.as_mut_ptr();
        unsafe {
            *wp1 = 0x1111;
        }

        // Fill the first region to force a new region
        let words_to_fill = DEFAULT_REGION_SIZE / 8;
        let _allocation = pool.alloc(words_to_fill); // this should trigger region 2

        // rp1 retains its region even after the pool grows
        unsafe {
            assert_eq!(Arc::strong_count(&rp1._region) - 1, 1);
            // The data written through wp1 should still be accessible
            assert_eq!(*wp1, 0x1111);
        }
    }

    #[test]
    fn test_pool_multi_region_growth() {
        let mut pool = EtsRegionPool::new();

        // Allocate enough to fill the default 2MB region
        let words_per_alloc = 1024; // 8KB per alloc
        let allocs_to_fill = DEFAULT_REGION_SIZE / (words_per_alloc * 8); // 256 allocs

        for _ in 0..allocs_to_fill {
            let mut allocation = pool.alloc(words_per_alloc);
            let wp = allocation.as_mut_ptr();
            assert!(!wp.is_null());
        }

        // Should have used exactly one region
        assert_eq!(pool.region_count(), 1);

        // One more alloc should trigger a new region
        let mut allocation = pool.alloc(words_per_alloc);
            let wp = allocation.as_mut_ptr();
        assert!(!wp.is_null());
        assert_eq!(pool.region_count(), 2);
    }

    #[test]
    fn test_pool_clear() {
        let mut pool = EtsRegionPool::new();

        // Allocate across multiple regions
        let words_per_alloc = DEFAULT_REGION_SIZE / 8; // fill one region per alloc
        for _ in 0..3 {
            let mut allocation = pool.alloc(words_per_alloc);
            let wp = allocation.as_mut_ptr();
            assert!(!wp.is_null());
        }
        assert!(pool.region_count() >= 3);
        assert!(pool.mapped_bytes() >= 3 * DEFAULT_REGION_SIZE);

        // Clear — all regions munmap'd
        pool.clear();
        assert_eq!(pool.region_count(), 0);
        assert_eq!(pool.mapped_bytes(), 0);
        assert_eq!(pool.used_bytes(), 0);
    }

    #[test]
    fn test_pool_owners_and_reclaim() {
        let mut pool = EtsRegionPool::new();

        // Allocate 3 entries in the same region
        let rp1 = pool.alloc(10);
        let rp2 = pool.alloc(10);
        let rp3 = pool.alloc(10);

        // All should be in the same region
        assert!(Arc::ptr_eq(&rp1._region, &rp2._region));
        assert!(Arc::ptr_eq(&rp2._region, &rp3._region));
        assert_eq!(pool.region_count(), 1);

        assert_eq!(Arc::strong_count(&rp1._region) - 1, 3);

        // Dropping entries releases their mapping ownership.
        drop(rp1);
        // Still 2 live — should not reclaim
        pool.reclaim_dead_regions();
        assert_eq!(pool.mapped_bytes(), DEFAULT_REGION_SIZE);

        drop(rp2);
        drop(rp3);
        // Now 0 live — should reclaim
        pool.reclaim_dead_regions();
        // Region slot still exists but memory is unmapped
        assert_eq!(pool.region_count(), 1);
        assert_eq!(pool.live_region_count(), 0);
        assert_eq!(pool.mapped_bytes(), 0);
    }

    #[test]
    fn test_large_alloc_custom_region() {
        let mut pool = EtsRegionPool::new();

        // Allocate more than DEFAULT_REGION_SIZE
        let huge_words = (DEFAULT_REGION_SIZE / 8) + 1000;
        let mut allocation = pool.alloc(huge_words);
        let wp = allocation.as_mut_ptr();
        assert!(!wp.is_null());
        assert_eq!(pool.region_count(), 1);
        // Region should be larger than default
        assert!(pool.mapped_bytes() > DEFAULT_REGION_SIZE);
    }
}
