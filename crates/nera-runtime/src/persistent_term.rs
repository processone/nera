//! `persistent_term` module — read-optimized global key-value storage (OTP 21+).
//!
//! Provides a global store where values can be any Erlang term.
//! Reads are fast (parking_lot RwLock read path). Writes are infrequent.
//!
//! Optimizations:
//! - **Fast path for immediate keys** (atoms, small ints): uses raw u64 as
//!   HashMap key, avoiding TermKey serialization (Vec alloc + recursive walk).
//! - **Immediate value flag**: skips deep_copy_into_heap when the stored value
//!   is an immediate (atom, small int, nil, pid).
//! - **parking_lot::RwLock**: faster uncontended read path than std RwLock.
//!
//! Terms are deep-copied into global storage (isolated from process heaps),
//! reusing the same deep-copy infrastructure as ETS.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use parking_lot::RwLock;

use crate::ets::{
    CompactHeap, TermKey,
    count_heap_words, deep_copy_into_compact, deep_copy_into_heap,
};
use crate::ets_pool::EtsRegionPool;
use crate::heap::ProcessHeap;
use crate::term::ErlTerm;

/// Check if an Erlang term is an immediate value (no heap allocation needed).
#[inline(always)]
fn is_immediate(term: ErlTerm) -> bool {
    term.is_atom() || term.is_small_int() || term.is_pid() || term.is_nil()
}

/// A stored value in the persistent term store.
struct StoredValue {
    root: ErlTerm,
    /// True if root is an immediate (no deep-copy needed on read).
    is_immediate: bool,
    /// Backing storage for non-immediate values. None for immediates.
    _heap: Option<CompactHeap>,
}

/// The global persistent term store.
///
/// Split into two maps for performance:
/// - `fast`: immediate keys (atoms, small ints) → O(1) lookup by raw u64
/// - `complex`: complex term keys → lookup via TermKey byte serialization
struct PersistentTermStore {
    fast: RwLock<HashMap<u64, StoredValue>>,
    complex: RwLock<HashMap<TermKey, StoredValue>>,
    pool: Mutex<EtsRegionPool>,
}

static STORE: OnceLock<PersistentTermStore> = OnceLock::new();

fn store() -> &'static PersistentTermStore {
    STORE.get_or_init(|| PersistentTermStore {
        fast: RwLock::new(HashMap::new()),
        complex: RwLock::new(HashMap::new()),
        pool: Mutex::new(EtsRegionPool::new()),
    })
}

/// Deep-copy a value into global storage, returning a StoredValue.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn store_value(s: &PersistentTermStore, value: ErlTerm) -> StoredValue {
    if is_immediate(value) {
        return StoredValue {
            root: value,
            is_immediate: true,
            _heap: None,
        };
    }

    let total_words = count_heap_words(value);
    let allocation = if total_words > 0 {
        Some(s.pool.lock().unwrap().alloc(total_words))
    } else { None };
    let mut compact = CompactHeap::from_pool(allocation);
    let copied_value = deep_copy_into_compact(value, &mut compact);

    StoredValue {
        root: copied_value,
        is_immediate: false,
        _heap: Some(compact),
    }
}

/// `persistent_term:put/2` — store a key-value pair.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub(crate) unsafe fn persistent_term_put(key: ErlTerm, value: ErlTerm) {
    let s = store();
    let stored = store_value(s, value);

    if is_immediate(key) {
        s.fast.write().insert(key.0, stored);
    } else {
        let term_key = TermKey::from_term(key);
        s.complex.write().insert(term_key, stored);
    }
}

/// `persistent_term:get/1` — retrieve a value by key.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub(crate) unsafe fn persistent_term_get(key: ErlTerm, heap: &mut ProcessHeap) -> Option<ErlTerm> {
    let s = store();

    if is_immediate(key) {
        let data = s.fast.read();
        let stored = data.get(&key.0)?;
        if stored.is_immediate {
            return Some(stored.root);
        }
        return Some(deep_copy_into_heap(stored.root, heap));
    }

    let term_key = TermKey::from_term(key);
    let data = s.complex.read();
    let stored = data.get(&term_key)?;
    if stored.is_immediate {
        return Some(stored.root);
    }
    Some(deep_copy_into_heap(stored.root, heap))
}

/// `persistent_term:erase/1` — remove a key-value pair.
///
/// After removing the entry (which releases its mapping owner), reclaim any
/// regions retained only by the pool. Otherwise unused mappings would accumulate.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
pub(crate) unsafe fn persistent_term_erase(key: ErlTerm) -> bool {
    let s = store();

    let removed = if is_immediate(key) {
        s.fast.write().remove(&key.0).is_some()
    } else {
        let term_key = TermKey::from_term(key);
        s.complex.write().remove(&term_key).is_some()
    };

    if removed {
        s.pool.lock().unwrap().reclaim_dead_regions();
    }

    removed
}

/// `persistent_term:info/0` — returns (count, memory_bytes).
pub(crate) fn persistent_term_info() -> (usize, usize) {
    let s = store();
    let fast_count = s.fast.read().len();
    let complex_count = s.complex.read().len();
    let count = fast_count + complex_count;
    let memory = count * 128; // rough estimate per entry
    (count, memory)
}
