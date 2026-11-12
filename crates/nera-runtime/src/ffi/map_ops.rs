//! Map operations: allocation, get, put, update, merge, keys, values, etc.

#![allow(unused_unsafe)]

use crate::heap::MapPairs;
use crate::atom_table::global_atom_table;
use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;

/// Semantic term equality for map key comparison.
/// Uses `compare_terms` for boxed values (binaries, tuples, etc.) so that
/// two different allocations of the same binary content match correctly.
/// Atoms, small ints, pids, and nil use the fast bit-equality path.
#[inline]
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn keys_equal(a: ErlTerm, b: ErlTerm) -> bool {
    // Cons cells are heap graphs too: rebuilt list keys compare by value.
    // Exact equality also keeps integer/float distinctions inside compound keys.
    super::compare::exact_equal(a, b)
}

/// Raise `error:{badkey, Key}` — BEAM semantics for missing map key.
/// # Safety
/// Requires the context/heap invariants of `with_ctx_heap` and live input terms.
unsafe fn raise_badkey(ctx: *mut ProcessContext, key: ErlTerm) -> u64 {
    let atom_id = global_atom_table().intern("badkey");
    let reason = with_ctx_heap(ctx, |heap| {
        heap.alloc_tuple(&[ErlTerm::atom(atom_id), key])
    });
    raise_error(ctx, reason.0)
}

/// Raise `error:{badmap, Map}` — BEAM semantics for non-map argument.
/// # Safety
/// Requires the context/heap invariants of `with_ctx_heap` and live input terms.
unsafe fn raise_badmap(ctx: *mut ProcessContext, map: ErlTerm) -> u64 {
    let atom_id = global_atom_table().intern("badmap");
    let reason = with_ctx_heap(ctx, |heap| {
        heap.alloc_tuple(&[ErlTerm::atom(atom_id), map])
    });
    raise_error(ctx, reason.0)
}

/// Allocate a map from interleaved key-value pairs stored at `pairs_ptr`.
/// `npairs` is the number of key-value pairs (not the total element count).
/// The pairs array has `npairs * 2` u64 elements: [k1, v1, k2, v2, ...].
/// Returns a tagged map ErlTerm.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_alloc_map(
    ctx: *mut ProcessContext,
    npairs: u32,
    pairs_ptr: *const u64,
) -> u64 {
    let mut pairs = Vec::with_capacity(npairs as usize);
    for i in 0..npairs as usize {
        // SAFETY: the FFI caller supplies npairs initialized key/value pairs.
        pairs.push(unsafe {
            [
                ErlTerm(*pairs_ptr.add(i * 2)),
                ErlTerm(*pairs_ptr.add(i * 2 + 1)),
            ]
        });
    }
    with_ctx_heap(ctx, |heap| heap.alloc_map(MapPairs::new(&pairs)).0)
}

/// Get a value from a map by key.
/// Performs linear scan of key-value pairs.
/// Returns the value if found, or raises badkey (returns nil for now).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_get(
    ctx: *mut ProcessContext,
    map: u64,
    key: u64,
) -> u64 {
    let map_term = ErlTerm(map);
    let key_term = ErlTerm(key);

    if let Some(ptr) = map_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_MAP {
                return raise_badmap(ctx, map_term);
            }
            let npairs = header.arity() as usize;
            for i in 0..npairs {
                let k = ErlTerm(*ptr.add(1 + i * 2));
                if keys_equal(k, key_term) {
                    return *ptr.add(1 + i * 2 + 1);
                }
            }
        }
    } else {
        return raise_badmap(ctx, map_term);
    }
    // Key not found
    raise_badkey(ctx, key_term)
}

/// Create a new map with an added/updated key-value pair.
/// Returns a new map term.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_put(
    ctx: *mut ProcessContext,
    map: u64,
    key: u64,
    value: u64,
) -> u64 {
    let map_term = ErlTerm(map);
    let key_term = ErlTerm(key);
    let value_term = ErlTerm(value);

    if let Some(ptr) = map_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_MAP {
                return raise_badmap(ctx, map_term);
            }
            let npairs = header.arity() as usize;

            // Check if key already exists
            let mut existing_idx: Option<usize> = None;
            for i in 0..npairs {
                let k = ErlTerm(*ptr.add(1 + i * 2));
                if keys_equal(k, key_term) {
                    existing_idx = Some(i);
                    break;
                }
            }

            return with_ctx_heap(ctx, |heap| {
                if let Some(idx) = existing_idx {
                    // Update existing key: copy all pairs, replace value at idx
                    let mut pairs = Vec::with_capacity(npairs);
                    for i in 0..npairs {
                        let key = ErlTerm(*ptr.add(1 + i * 2));
                        let value = if i == idx { value_term } else { ErlTerm(*ptr.add(1 + i * 2 + 1)) };
                        pairs.push([key, value]);
                    }
                    heap.alloc_map(MapPairs::new(&pairs)).0
                } else {
                    // Add new key: copy all existing pairs + new pair
                    let mut pairs = Vec::with_capacity(npairs + 1);
                    for i in 0..npairs {
                        pairs.push([ErlTerm(*ptr.add(1 + i * 2)), ErlTerm(*ptr.add(1 + i * 2 + 1))]);
                    }
                    pairs.push([key_term, value_term]);
                    heap.alloc_map(MapPairs::new(&pairs)).0
                }
            });
        }
    }
    // Not a boxed pointer — not a map
    raise_badmap(ctx, map_term)
}

/// Exact-update a map key (`:=` operator): the key must already exist.
/// Raises `{badkey, Key}` if the key is not present, or `{badmap, Map}` if
/// the term is not a map.  Returns a new map term on success.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_update_exact(
    ctx: *mut ProcessContext,
    map: u64,
    key: u64,
    value: u64,
) -> u64 {
    let map_term = ErlTerm(map);
    let key_term = ErlTerm(key);
    let value_term = ErlTerm(value);

    if let Some(ptr) = map_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_MAP {
                return raise_badmap(ctx, map_term);
            }
            let npairs = header.arity() as usize;

            // Find the key
            let mut existing_idx: Option<usize> = None;
            for i in 0..npairs {
                let k = ErlTerm(*ptr.add(1 + i * 2));
                if keys_equal(k, key_term) {
                    existing_idx = Some(i);
                    break;
                }
            }

            if let Some(idx) = existing_idx {
                // Key exists — update it
                return with_ctx_heap(ctx, |heap| {
                    let mut pairs = Vec::with_capacity(npairs);
                    for i in 0..npairs {
                        let key = ErlTerm(*ptr.add(1 + i * 2));
                        let value = if i == idx { value_term } else { ErlTerm(*ptr.add(1 + i * 2 + 1)) };
                        pairs.push([key, value]);
                    }
                    heap.alloc_map(MapPairs::new(&pairs)).0
                });
            } else {
                // Key not found — raise {badkey, Key}
                return raise_badkey(ctx, key_term);
            }
        }
    }
    // Not a boxed pointer — not a map
    raise_badmap(ctx, map_term)
}

/// Get the number of key-value pairs in a map (map_size/1 and maps:size/1).
/// Returns a tagged small integer, or raises `{badmap, Term}` for a non-map;
/// a guard turns that raise into a failed test.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_size(ctx: *mut ProcessContext, map: u64) -> u64 {
    let map_term = ErlTerm(map);
    if let Some(ptr) = map_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() == BOXED_MAP {
                return ErlTerm::small_int(header.arity() as i64).0;
            }
        }
    }
    raise_badmap(ctx, map_term)
}

/// Check if a key exists in a map (maps:is_key/2 and erlang:is_map_key/2).
/// Returns atom 'true' or 'false', or raises {badmap, Map} for a non-map.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_is_key(
    ctx: *mut ProcessContext,
    key: u64,
    map: u64,
) -> u64 {
    let map_term = ErlTerm(map);
    let key_term = ErlTerm(key);

    if let Some(ptr) = map_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_MAP {
                return raise_badmap(ctx, map_term);
            }
            let npairs = header.arity() as usize;
            for i in 0..npairs {
                let k = ErlTerm(*ptr.add(1 + i * 2));
                if keys_equal(k, key_term) {
                    return ErlTerm::atom(crate::atom_table::ATOM_TRUE).0;
                }
            }
        }
    } else {
        return raise_badmap(ctx, map_term);
    }
    ErlTerm::atom(crate::atom_table::ATOM_FALSE).0
}

/// maps:find(Key, Map) -> {ok, Value} | error
/// Returns `{ok, Value}` if key exists, or the atom `error` if not found.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_find(
    ctx: *mut ProcessContext,
    key: u64,
    map: u64,
) -> u64 {
    let map_term = ErlTerm(map);
    let key_term = ErlTerm(key);

    if let Some(ptr) = map_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_MAP {
                return raise_badmap(ctx, map_term);
            }
            let npairs = header.arity() as usize;
            for i in 0..npairs {
                let k = ErlTerm(*ptr.add(1 + i * 2));
                if keys_equal(k, key_term) {
                    let value = ErlTerm(*ptr.add(1 + i * 2 + 1));
                    // Return {ok, Value}
                    let heap = &mut *(*ctx).heap_ptr;
                    return heap.alloc_tuple(&[ErlTerm::atom(crate::atom_table::ATOM_OK), value]).0;
                }
            }
        }
    } else {
        return raise_badmap(ctx, map_term);
    }
    // Key not found — return atom 'error'
    ErlTerm::atom(crate::atom_table::ATOM_ERROR).0
}

/// maps:remove(Key, Map) -> NewMap
/// Returns a new map with the key removed (or the same map if key wasn't present).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_remove(
    ctx: *mut ProcessContext,
    key: u64,
    map: u64,
) -> u64 {
    let map_term = ErlTerm(map);
    let key_term = ErlTerm(key);

    if let Some(ptr) = map_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_MAP {
                return raise_badmap(ctx, map_term);
            }
            let npairs = header.arity() as usize;

            // Find if key exists
            let mut found_idx = None;
            for i in 0..npairs {
                let k = ErlTerm(*ptr.add(1 + i * 2));
                if keys_equal(k, key_term) {
                    found_idx = Some(i);
                    break;
                }
            }

            match found_idx {
                None => {
                    // Key not present — return original map unchanged
                    map
                }
                Some(idx) => {
                    // Build a new map without the removed pair
                    let new_npairs = npairs - 1;
                    let heap = &mut *(*ctx).heap_ptr;
                    let new_ptr = heap.alloc_words(1 + new_npairs * 2);
                    *new_ptr = BoxedHeader::new(crate::term::BoxedSubtype::Map, new_npairs as u64).0;
                    let mut dst = 0;
                    for i in 0..npairs {
                        if i != idx {
                            *new_ptr.add(1 + dst * 2) = *ptr.add(1 + i * 2);
                            *new_ptr.add(1 + dst * 2 + 1) = *ptr.add(1 + i * 2 + 1);
                            dst += 1;
                        }
                    }
                    ErlTerm::boxed_unchecked(new_ptr).0
                }
            }
        }
    } else {
        raise_badmap(ctx, map_term)
    }
}

/// maps:new() -> #{}
/// Returns an empty map.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_new(ctx: *mut ProcessContext) -> u64 {
    with_ctx_heap(ctx, |heap| {
        heap.alloc_map(MapPairs::new(&[])).0
    })
}

/// maps:get(Key, Map, Default) -> Value | Default
/// Like maps:get/2 but returns Default instead of raising badkey.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_get_3(
    ctx: *mut ProcessContext,
    map: u64,
    key: u64,
    default: u64,
) -> u64 {
    let map_term = ErlTerm(map);
    let key_term = ErlTerm(key);

    if let Some(ptr) = map_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_MAP {
                return raise_badmap(ctx, map_term);
            }
            let npairs = header.arity() as usize;
            for i in 0..npairs {
                let k = ErlTerm(*ptr.add(1 + i * 2));
                if keys_equal(k, key_term) {
                    return *ptr.add(1 + i * 2 + 1);
                }
            }
        }
    } else {
        return raise_badmap(ctx, map_term);
    }
    // Key not found — return default
    default
}

/// maps:keys(Map) -> [Key]
/// Returns a list of all keys in the map.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_keys(ctx: *mut ProcessContext, map: u64) -> u64 {
    let map_term = ErlTerm(map);

    if let Some(ptr) = map_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_MAP {
                return raise_badmap(ctx, map_term);
            }
            let npairs = header.arity() as usize;
            let heap = &mut *(*ctx).heap_ptr;
            // Build list backwards from last pair to first — no reverse needed
            let mut list = ErlTerm::nil();
            for i in super::compare::map_iteration_order(ptr, npairs).into_iter().rev() {
                let k = ErlTerm(*ptr.add(1 + i * 2));
                list = heap.alloc_cons(k, list);
            }
            list.0
        }
    } else {
        raise_badmap(ctx, map_term)
    }
}

/// maps:values(Map) -> [Value]
/// Returns a list of all values in the map.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_values(ctx: *mut ProcessContext, map: u64) -> u64 {
    let map_term = ErlTerm(map);

    if let Some(ptr) = map_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_MAP {
                return raise_badmap(ctx, map_term);
            }
            let npairs = header.arity() as usize;
            let heap = &mut *(*ctx).heap_ptr;
            // Build list backwards from last pair to first
            let mut list = ErlTerm::nil();
            for i in super::compare::map_iteration_order(ptr, npairs).into_iter().rev() {
                let v = ErlTerm(*ptr.add(1 + i * 2 + 1));
                list = heap.alloc_cons(v, list);
            }
            list.0
        }
    } else {
        raise_badmap(ctx, map_term)
    }
}

/// maps:to_list(Map) -> [{Key, Value}]
/// Converts a map to a list of key-value tuples.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_to_list(ctx: *mut ProcessContext, map: u64) -> u64 {
    let map_term = ErlTerm(map);

    if let Some(ptr) = map_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_MAP {
                return raise_badmap(ctx, map_term);
            }
            let npairs = header.arity() as usize;
            let heap = &mut *(*ctx).heap_ptr;
            // Build list backwards from last pair to first
            let mut list = ErlTerm::nil();
            for i in super::compare::map_iteration_order(ptr, npairs).into_iter().rev() {
                let k = ErlTerm(*ptr.add(1 + i * 2));
                let v = ErlTerm(*ptr.add(1 + i * 2 + 1));
                let tuple = heap.alloc_tuple(&[k, v]);
                list = heap.alloc_cons(tuple, list);
            }
            list.0
        }
    } else {
        raise_badmap(ctx, map_term)
    }
}

/// maps:from_list([{Key, Value}]) -> Map
/// Creates a map from a list of key-value tuples.
/// Later keys override earlier ones (OTP semantics).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_from_list(ctx: *mut ProcessContext, list: u64) -> u64 {
    let list_term = ErlTerm(list);

    // Collect all {K, V} pairs from the list
    let mut pairs: Vec<[ErlTerm; 2]> = Vec::new();
    let mut cursor = list_term;
    while cursor.is_cons() {
        unsafe {
            let cell = &*cursor.as_cons_ptr().unwrap();
            let elem = cell.head;
            // Each element must be a 2-tuple
            if let Some(tptr) = elem.as_boxed_ptr() {
                let header = BoxedHeader(*tptr);
                if header.subtype() == crate::term::BOXED_TUPLE && header.arity() == 2 {
                    let k = ErlTerm(*tptr.add(1));
                    let v = ErlTerm(*tptr.add(2));
                    // Later keys override: update existing or append
                    if let Some(pos) = pairs.iter().position(|[pk, _]| keys_equal(*pk, k)) {
                        pairs[pos][1] = v;
                    } else {
                        pairs.push([k, v]);
                    }
                } else {
                    let atom_id = global_atom_table().intern("badarg");
                    return raise_error(ctx, ErlTerm::atom(atom_id).0);
                }
            } else {
                let atom_id = global_atom_table().intern("badarg");
                return raise_error(ctx, ErlTerm::atom(atom_id).0);
            }
            cursor = cell.tail;
        }
    }

    // Build the map
    with_ctx_heap(ctx, |heap| {
        heap.alloc_map(MapPairs::new(&pairs)).0
    })
}

/// maps:from_keys([Key], Value) -> Map
/// Creates a map from a list of keys, all set to the same value.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_from_keys(ctx: *mut ProcessContext, keys: u64, value: u64) -> u64 {
    let keys_term = ErlTerm(keys);
    let value_term = ErlTerm(value);

    // Collect keys, deduplicating (last wins, but all same value so order doesn't matter)
    let mut key_list: Vec<ErlTerm> = Vec::new();
    let mut cursor = keys_term;
    while cursor.is_cons() {
        unsafe {
            let cell = &*cursor.as_cons_ptr().unwrap();
            let k = cell.head;
            if !key_list.iter().any(|pk| keys_equal(*pk, k)) {
                key_list.push(k);
            }
            cursor = cell.tail;
        }
    }

    with_ctx_heap(ctx, |heap| {
        let pairs: Vec<_> = key_list.into_iter().map(|key| [key, value_term]).collect();
        heap.alloc_map(MapPairs::new(&pairs)).0
    })
}

/// maps:merge(Map1, Map2) -> Map3
/// Merges two maps. Keys in Map2 override keys in Map1.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_merge(
    ctx: *mut ProcessContext,
    map1: u64,
    map2: u64,
) -> u64 {
    let map1_term = ErlTerm(map1);
    let map2_term = ErlTerm(map2);

    let (ptr1, npairs1) = if let Some(ptr) = map1_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_MAP {
                return raise_badmap(ctx, map1_term);
            }
            (ptr, header.arity() as usize)
        }
    } else {
        return raise_badmap(ctx, map1_term);
    };

    let (ptr2, npairs2) = if let Some(ptr) = map2_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_MAP {
                return raise_badmap(ctx, map2_term);
            }
            (ptr, header.arity() as usize)
        }
    } else {
        return raise_badmap(ctx, map2_term);
    };

    unsafe {
        // Start with all pairs from Map1
        let mut pairs: Vec<[ErlTerm; 2]> = Vec::with_capacity(npairs1 + npairs2);
        for i in 0..npairs1 {
            let k = ErlTerm(*ptr1.add(1 + i * 2));
            let v = ErlTerm(*ptr1.add(1 + i * 2 + 1));
            pairs.push([k, v]);
        }
        // Add/override with pairs from Map2
        for i in 0..npairs2 {
            let k = ErlTerm(*ptr2.add(1 + i * 2));
            let v = ErlTerm(*ptr2.add(1 + i * 2 + 1));
            if let Some(pos) = pairs.iter().position(|[pk, _]| keys_equal(*pk, k)) {
                pairs[pos][1] = v;
            } else {
                pairs.push([k, v]);
            }
        }

        let heap = &mut *(*ctx).heap_ptr;
        heap.alloc_map(MapPairs::new(&pairs)).0
    }
}

/// maps:update(Key, Value, Map) -> NewMap
/// Updates an existing key. Raises {badkey, Key} if key doesn't exist.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_update(
    ctx: *mut ProcessContext,
    map: u64,
    key: u64,
    value: u64,
) -> u64 {
    // Same semantics as nera_rt_map_update_exact
    nera_rt_map_update_exact(ctx, map, key, value)
}

/// maps:take(Key, Map) -> {Value, NewMap} | error
/// Removes a key from a map and returns the value and the new map.
/// Returns atom `error` if the key is not present.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_map_take(
    ctx: *mut ProcessContext,
    map: u64,
    key: u64,
) -> u64 {
    let map_term = ErlTerm(map);
    let key_term = ErlTerm(key);

    if let Some(ptr) = map_term.as_boxed_ptr() {
        unsafe {
            let header = BoxedHeader(*ptr);
            if header.subtype() != BOXED_MAP {
                return raise_badmap(ctx, map_term);
            }
            let npairs = header.arity() as usize;

            // Find the key
            let mut found_idx = None;
            for i in 0..npairs {
                let k = ErlTerm(*ptr.add(1 + i * 2));
                if keys_equal(k, key_term) {
                    found_idx = Some(i);
                    break;
                }
            }

            match found_idx {
                None => {
                    // Key not present — return atom 'error'
                    ErlTerm::atom(crate::atom_table::ATOM_ERROR).0
                }
                Some(idx) => {
                    let value = ErlTerm(*ptr.add(1 + idx * 2 + 1));
                    // Build new map without the taken pair
                    let new_npairs = npairs - 1;
                    let heap = &mut *(*ctx).heap_ptr;
                    let new_ptr = heap.alloc_words(1 + new_npairs * 2);
                    *new_ptr = BoxedHeader::new(crate::term::BoxedSubtype::Map, new_npairs as u64).0;
                    let mut dst = 0;
                    for i in 0..npairs {
                        if i != idx {
                            *new_ptr.add(1 + dst * 2) = *ptr.add(1 + i * 2);
                            *new_ptr.add(1 + dst * 2 + 1) = *ptr.add(1 + i * 2 + 1);
                            dst += 1;
                        }
                    }
                    let new_map = ErlTerm::boxed_unchecked(new_ptr);
                    // Return {Value, NewMap}
                    heap.alloc_tuple(&[value, new_map]).0
                }
            }
        }
    } else {
        raise_badmap(ctx, map_term)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::ProcessHeap;

    #[test]
    fn keys_equal_same_term() {
        // Identical raw values must be equal.
        let a = ErlTerm::small_int(42);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { keys_equal(a, a) });
    }

    #[test]
    fn keys_equal_different_small_ints() {
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(!unsafe { keys_equal(ErlTerm::small_int(1), ErlTerm::small_int(2)) });
    }

    #[test]
    fn keys_equal_atoms() {
        let a = ErlTerm::atom(0);
        let b = ErlTerm::atom(0);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(keys_equal(a, b));
            assert!(!keys_equal(ErlTerm::atom(0), ErlTerm::atom(1)));
        }
    }

    #[test]
    fn keys_equal_binary_different_allocs() {
        // Two separately-allocated binaries with the same content must be equal.
        // This is the core regression test for the map binary key bug.
        let mut heap = ProcessHeap::new();
        let bin1 = heap.alloc_binary_auto(b"hello");
        let bin2 = heap.alloc_binary_auto(b"hello");
        // Different pointers
        assert_ne!(bin1.0, bin2.0);
        // But semantically equal
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { keys_equal(bin1, bin2) });
    }

    #[test]
    fn keys_equal_binary_different_content() {
        let mut heap = ProcessHeap::new();
        let bin1 = heap.alloc_binary_auto(b"hello");
        let bin2 = heap.alloc_binary_auto(b"world");
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(!unsafe { keys_equal(bin1, bin2) });
    }

    #[test]
    fn keys_equal_tuple_different_allocs() {
        // Two separately-allocated tuples with the same elements must be equal.
        let mut heap = ProcessHeap::new();
        let t1 = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        let t2 = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        assert_ne!(t1.0, t2.0);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { keys_equal(t1, t2) });
    }

    #[test]
    fn keys_equal_float_different_allocs() {
        // Two separately-allocated floats with the same value must be equal.
        let mut heap = ProcessHeap::new();
        let f1 = heap.alloc_float(3.25);
        let f2 = heap.alloc_float(3.25);
        assert_ne!(f1.0, f2.0);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { keys_equal(f1, f2) });
    }

    #[test]
    fn keys_equal_empty_binary() {
        let mut heap = ProcessHeap::new();
        let b1 = heap.alloc_binary_auto(b"");
        let b2 = heap.alloc_binary_auto(b"");
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { keys_equal(b1, b2) });
    }

    #[test]
    fn keys_equal_mixed_types() {
        // An atom and a binary should never be equal.
        let mut heap = ProcessHeap::new();
        let bin = heap.alloc_binary_auto(b"foo");
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(!keys_equal(ErlTerm::atom(0), bin));
            assert!(!keys_equal(ErlTerm::small_int(42), bin));
        }
    }

    #[test]
    fn keys_equal_binary_different_lengths() {
        let mut heap = ProcessHeap::new();
        let short = heap.alloc_binary_auto(b"ab");
        let long = heap.alloc_binary_auto(b"abc");
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(!keys_equal(short, long));
            assert!(!keys_equal(long, short));
        }
    }

    #[test]
    fn keys_equal_binary_prefix() {
        // "abc" != "abcd" even though one is a prefix of the other
        let mut heap = ProcessHeap::new();
        let bin1 = heap.alloc_binary_auto(b"abc");
        let bin2 = heap.alloc_binary_auto(b"abcd");
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(!unsafe { keys_equal(bin1, bin2) });
    }

    #[test]
    fn keys_equal_large_binary() {
        // Large binaries (> 64 bytes, may use refc) should still compare equal
        let mut heap = ProcessHeap::new();
        let data: Vec<u8> = (0..256).map(|i| (i % 256) as u8).collect();
        let bin1 = heap.alloc_binary_auto(&data);
        let bin2 = heap.alloc_binary_auto(&data);
        assert_ne!(bin1.0, bin2.0);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { keys_equal(bin1, bin2) });
    }

    #[test]
    fn keys_equal_large_binary_differ_last_byte() {
        let mut heap = ProcessHeap::new();
        let data1: Vec<u8> = (0..256).map(|i| (i % 256) as u8).collect();
        let mut data2 = data1.clone();
        data2[255] = 0x00; // data1[255] is 0xFF, so this creates a difference
        let bin1 = heap.alloc_binary_auto(&data1);
        let bin2 = heap.alloc_binary_auto(&data2);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(!unsafe { keys_equal(bin1, bin2) });
    }

    #[test]
    fn keys_equal_tuple_different_arity() {
        let mut heap = ProcessHeap::new();
        let t2 = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        let t3 = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2), ErlTerm::small_int(3)]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(!unsafe { keys_equal(t2, t3) });
    }

    #[test]
    fn keys_equal_tuple_with_binary_elements() {
        // Tuples containing binaries — deep structural equality
        let mut heap = ProcessHeap::new();
        let b1 = heap.alloc_binary_auto(b"key");
        let t1 = heap.alloc_tuple(&[b1, ErlTerm::small_int(1)]);
        let b2 = heap.alloc_binary_auto(b"key");
        let t2 = heap.alloc_tuple(&[b2, ErlTerm::small_int(1)]);
        assert_ne!(t1.0, t2.0);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { keys_equal(t1, t2) });
    }

    #[test]
    fn keys_equal_tuple_differ_in_element() {
        let mut heap = ProcessHeap::new();
        let t1 = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(2)]);
        let t2 = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(99)]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(!unsafe { keys_equal(t1, t2) });
    }

    #[test]
    fn keys_equal_empty_tuple() {
        let mut heap = ProcessHeap::new();
        let t1 = heap.alloc_tuple(&[]);
        let t2 = heap.alloc_tuple(&[]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { keys_equal(t1, t2) });
    }

    #[test]
    fn keys_equal_float_different_values() {
        let mut heap = ProcessHeap::new();
        let f1 = heap.alloc_float(1.0);
        let f2 = heap.alloc_float(2.0);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(!unsafe { keys_equal(f1, f2) });
    }

    #[test]
    fn keys_equal_nil() {
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(unsafe { keys_equal(ErlTerm::nil(), ErlTerm::nil()) });
    }

    #[test]
    fn keys_equal_boxed_vs_immediate() {
        // A tuple and a small int should never be equal
        let mut heap = ProcessHeap::new();
        let t = heap.alloc_tuple(&[ErlTerm::small_int(1)]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        unsafe {
            assert!(!keys_equal(t, ErlTerm::small_int(1)));
            assert!(!keys_equal(ErlTerm::small_int(1), t));
        }
    }

    #[test]
    fn keys_equal_float_vs_tuple() {
        // Different boxed subtypes must not be equal
        let mut heap = ProcessHeap::new();
        let f = heap.alloc_float(1.0);
        let t = heap.alloc_tuple(&[ErlTerm::small_int(1)]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(!unsafe { keys_equal(f, t) });
    }

    #[test]
    fn keys_equal_binary_vs_tuple() {
        let mut heap = ProcessHeap::new();
        let b = heap.alloc_binary_auto(b"\x00\x00\x00\x01");
        let t = heap.alloc_tuple(&[ErlTerm::small_int(1)]);
        // SAFETY: the test owns the live backing storage and uses current GC roots.
        assert!(!unsafe { keys_equal(b, t) });
    }
}

