//! List operations: head, tail, length, append, reverse, keyfind, etc.

#![allow(unused_unsafe)]

use crate::atom_table::global_atom_table;
use crate::bif;
use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;
use super::compare::{compare_terms, exact_equal};

/// Get the head of a cons cell.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_list_head(ctx: *mut ProcessContext, list: u64) -> u64 {
    let term = ErlTerm(list);
    match term.as_cons_ptr() {
        Some(ptr) => unsafe { (*ptr).head.0 },
        None => raise_badarg(ctx),
    }
}

/// Get the tail of a cons cell.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_list_tail(ctx: *mut ProcessContext, list: u64) -> u64 {
    let term = ErlTerm(list);
    match term.as_cons_ptr() {
        Some(ptr) => unsafe { (*ptr).tail.0 },
        None => raise_badarg(ctx),
    }
}

/// erlang:length/1
#[no_mangle]
pub unsafe extern "C" fn nera_rt_length(ctx: *mut ProcessContext, list: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_length(heap, &[ErlTerm(list)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// erlang:hd/1
#[no_mangle]
pub unsafe extern "C" fn nera_rt_hd(ctx: *mut ProcessContext, list: u64) -> u64 {
    let term = ErlTerm(list);
    match term.as_cons_ptr() {
        Some(ptr) => unsafe { (*ptr).head.0 },
        None => raise_badarg(ctx),
    }
}

/// erlang:tl/1
#[no_mangle]
pub unsafe extern "C" fn nera_rt_tl(ctx: *mut ProcessContext, list: u64) -> u64 {
    let term = ErlTerm(list);
    match term.as_cons_ptr() {
        Some(ptr) => unsafe { (*ptr).tail.0 },
        None => raise_badarg(ctx),
    }
}

/// List append (++) operator.
/// Copies all elements of `left` and appends `right` at the end.
/// The left operand must be proper; the right operand may be any tail.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_list_append(ctx: *mut ProcessContext, left: u64, right: u64) -> u64 {
    with_ctx_heap(ctx, |heap| {
        let left_term = ErlTerm(left);
        let right_term = ErlTerm(right);

        // Collect elements from left list
        let mut elems = Vec::new();
        let mut cursor = left_term;
        while cursor.is_cons() {
            unsafe {
                let cell = &*cursor.as_cons_ptr().unwrap();
                elems.push(cell.head);
                cursor = cell.tail;
            }
        }
        if !cursor.is_nil() { return raise_badarg(ctx); }
        // The right operand may be an improper tail.
        if elems.is_empty() {
            return right_term.0;
        }

        // Build result: cons each element of left onto right, from back to front
        let mut result = right_term;
        for elem in elems.into_iter().rev() {
            result = heap.alloc_cons(elem, result);
        }
        result.0
    })
}

/// A lossy filter, never an equality decision. Only fully immediate tuples
/// have fingerprints: equal such tuples must have identical field words.
/// # Safety
/// The term graph must remain live, initialized, immutable and unmoved.
#[inline]
unsafe fn flat_tuple_fingerprint(term: ErlTerm) -> Option<u64> {
    let ptr = term.as_boxed_ptr()?;
    let header = BoxedHeader(*ptr);
    // Bound preparation work; wide tuples retain the existing comparison path.
    if header.subtype() != BOXED_TUPLE || !(1..=8).contains(&header.arity()) { return None; }
    let mut hash = header.arity();
    for i in 1..=header.arity() as usize {
        let field = ErlTerm(*ptr.add(i));
        if !field.is_immediate() { return None; }
        hash = hash.rotate_left(7) ^ field.to_raw();
    }
    Some(hash)
}

/// Keep each cached fingerprint paired with its term through removals.
struct FilteredCandidates(Vec<(Option<u64>, ErlTerm)>);

impl FilteredCandidates {
    /// # Safety
    /// All candidate graphs stay live, initialized, immutable and unmoved.
    unsafe fn new(terms: Vec<ErlTerm>) -> Self {
        Self(terms.into_iter().map(|term| (flat_tuple_fingerprint(term), term)).collect())
    }

    /// # Safety
    /// All graphs stay live, initialized, immutable and unmoved; no GC/callbacks.
    #[inline]
    unsafe fn take(&mut self, value: ErlTerm) -> bool {
        if self.0.is_empty() { return false; }
        let pos = if let Some(hash) = flat_tuple_fingerprint(value) {
            self.0.iter().position(|&(filter, term)|
                filter.is_none_or(|candidate| candidate == hash) && exact_equal(term, value))
        } else {
            self.0.iter().position(|&(_, term)| exact_equal(term, value))
        };
        if let Some(pos) = pos { self.0.remove(pos); true } else { false }
    }
}

/// Traverse once using a statically selected removal strategy. No state-machine
/// dispatch is needed per element in the immediate or structural paths.
/// # Safety
/// All graphs remain initialized, live and unmoved, including during take.
/// The callback must not mutate graph storage, collect or invoke Erlang code.
#[inline]
unsafe fn subtract_tail(
    mut cursor: ErlTerm, result: &mut Vec<ErlTerm>, mut take: impl FnMut(ErlTerm) -> bool,
) -> Result<(), ()> {
    while let Some(cell) = cursor.as_cons_ptr() {
        let cell = &*cell;
        if !take(cell.head) { result.push(cell.head); }
        cursor = cell.tail;
    }
    if cursor.is_nil() { Ok(()) } else { Err(()) }
}

/// Validate list spines and return surviving terms before borrowing a heap.
/// # Safety
/// Both graphs must remain live, initialized, immutable and unmoved throughout
/// this call and until the returned scalar terms have been consumed by the caller.
unsafe fn subtract_list_elements(mut left: ErlTerm, mut right: ErlTerm) -> Result<Vec<ErlTerm>, ()> {
    let mut terms = Vec::new();
    let mut immediate_only = true;
    while let Some(cell) = right.as_cons_ptr() {
        let cell = &*cell;
        immediate_only &= cell.head.is_immediate();
        terms.push(cell.head);
        right = cell.tail;
    }
    if !right.is_nil() { return Err(()); }
    let mut result = Vec::new();
    if immediate_only {
        subtract_tail(left, &mut result, |value| {
            if let Some(pos) = terms.iter().position(|&term| term == value) {
                terms.remove(pos);
                true
            } else { false }
        })?;
        return Ok(result);
    }

    // Delay preparation until scans are long enough to repay it. Duplicates
    // often consume candidates quickly and continue directly on the old path.
    let mut comparisons = 0_usize;
    while left.is_cons() && terms.len() >= 8 && comparisons < 64 {
        let cell = &*left.as_cons_ptr().unwrap();
        let pos = terms.iter().position(|&term| exact_equal(term, cell.head));
        comparisons = comparisons.saturating_add(pos.map_or(terms.len(), |i| i + 1));
        if let Some(pos) = pos { terms.remove(pos); } else { result.push(cell.head); }
        left = cell.tail;
    }
    if left.is_cons() && terms.len() >= 8 && comparisons >= 64
        && terms.iter().any(|&term| flat_tuple_fingerprint(term).is_some()) {
        let mut filtered = FilteredCandidates::new(terms);
        subtract_tail(left, &mut result, |value| filtered.take(value))?;
    } else {
        subtract_tail(left, &mut result, |value| {
            if let Some(pos) = terms.iter().position(|&term| exact_equal(term, value)) {
                terms.remove(pos);
                true
            } else { false }
        })?;
    }
    Ok(result)
}

/// List subtract (--) operator. Remove one occurrence for each right element.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_list_subtract(ctx: *mut ProcessContext, left: u64, right: u64) -> u64 {
    let result = match subtract_list_elements(ErlTerm(left), ErlTerm(right)) {
        Ok(result) => result,
        Err(()) => return raise_badarg(ctx),
    };
    with_ctx_heap(ctx, |heap| heap.alloc_list_from_slice(&result).0)
}

/// lists:reverse/1 — reverse a list.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_lists_reverse(ctx: *mut ProcessContext, list: u64) -> u64 {
    nera_rt_lists_reverse_2(ctx, list, ErlTerm::nil().0)
}

/// lists:reverse/2 — reverse a list and prepend onto a tail.
/// `lists:reverse(List, Tail)` returns the elements of List in reverse order,
/// with Tail appended at the end.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_lists_reverse_2(ctx: *mut ProcessContext, list: u64, tail: u64) -> u64 {
    with_ctx_heap(ctx, |heap| {
        let mut cursor = ErlTerm(list);
        let mut acc = ErlTerm(tail);
        while cursor.is_cons() {
            unsafe {
                let cell = &*cursor.as_cons_ptr().unwrap();
                acc = heap.alloc_cons(cell.head, acc);
                cursor = cell.tail;
            }
        }
        if !cursor.is_nil() {
            // Not a proper list — badarg
            return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
        }
        acc.0
    })
}

/// lists:member/2 — test for membership using exact (=:=) equality.
/// Returns `true` if Elem is in List, `false` otherwise.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_lists_member(_ctx: *mut ProcessContext, elem: u64, list: u64) -> u64 {
    use crate::atom_table::{ATOM_TRUE, ATOM_FALSE};
    let elem_term = ErlTerm(elem);
    let mut cursor = ErlTerm(list);
    while cursor.is_cons() {
        unsafe {
            let cell = &*cursor.as_cons_ptr().unwrap();
            if exact_equal(cell.head, elem_term) {
                return ErlTerm::atom(ATOM_TRUE).0;
            }
            cursor = cell.tail;
        }
    }
    ErlTerm::atom(ATOM_FALSE).0
}

/// Largest position `lists:keyfind/3` and its siblings accept. OTP raises
/// `badarg` unless the position is an immediate integer of at least 1, and on
/// 64-bit BEAM immediate integers stop at 2^59 - 1. Nera's own small integers
/// reach 2^60 - 1, so the bound is explicit here.
const KEY_POSITION_MAX: i64 = (1 << 59) - 1;

/// The 1-based tuple position for the key BIFs, or `None` where OTP raises `badarg`.
fn key_position(n: ErlTerm) -> Option<usize> {
    match n.as_small_int() {
        Some(pos) if (1..=KEY_POSITION_MAX).contains(&pos) => Some(pos as usize),
        _ => None,
    }
}

/// Shared search for `lists:keyfind/3`, `keymember/3` and `keysearch/3`: the
/// first tuple whose element at position N compares equal (`==`) to Key.
/// Non-tuples and tuples shorter than N are skipped. `Err(())` is an invalid
/// position, or a list argument that is not a list or reaches an improper tail
/// before a match; each BIF raises it as `badarg`. Like BEAM, the tail is only
/// checked once the walk reaches it, so a match before it is still returned.
/// # Safety
/// The key and list graphs must remain live, initialized, immutable and
/// unmoved throughout this call.
unsafe fn key_search(key: ErlTerm, n: ErlTerm, list: ErlTerm) -> Result<Option<ErlTerm>, ()> {
    let pos = key_position(n).ok_or(())?;
    let mut cursor = list;
    while let Some(cell) = cursor.as_cons_ptr() {
        let cell = &*cell;
        if let Some(ptr) = cell.head.as_boxed_ptr() {
            let header = BoxedHeader(*ptr);
            if header.subtype() == BOXED_TUPLE && (header.arity() as usize) >= pos
                && compare_terms(ErlTerm(*ptr.add(pos)), key) == 0 {
                return Ok(Some(cell.head));
            }
        }
        cursor = cell.tail;
    }
    if cursor.is_nil() { Ok(None) } else { Err(()) }
}

/// lists:keyfind/3 — search a list of tuples for a key at position N.
/// `keyfind(Key, N, TupleList)` where N is 1-indexed.
/// Returns the tuple if found, `false` otherwise; raises `badarg` for an
/// invalid position or a list that is not proper up to the match.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_lists_keyfind(
    ctx: *mut ProcessContext,
    key: u64,
    n: u64,
    list: u64,
) -> u64 {
    use crate::atom_table::ATOM_FALSE;
    match key_search(ErlTerm(key), ErlTerm(n), ErlTerm(list)) {
        Ok(Some(tuple)) => tuple.0,
        Ok(None) => ErlTerm::atom(ATOM_FALSE).0,
        Err(()) => raise_badarg(ctx),
    }
}

/// lists:keymember/3 — test whether a key exists in a list of tuples.
/// Returns `true` if a tuple with Key at position N exists, `false` otherwise;
/// raises `badarg` for an invalid position or a list that is not proper up to
/// the match.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_lists_keymember(
    ctx: *mut ProcessContext,
    key: u64,
    n: u64,
    list: u64,
) -> u64 {
    use crate::atom_table::{ATOM_TRUE, ATOM_FALSE};
    match key_search(ErlTerm(key), ErlTerm(n), ErlTerm(list)) {
        Ok(Some(_)) => ErlTerm::atom(ATOM_TRUE).0,
        Ok(None) => ErlTerm::atom(ATOM_FALSE).0,
        Err(()) => raise_badarg(ctx),
    }
}

/// lists:keysearch/3 — search a list of tuples for a key at position N.
/// Returns `{value, Tuple}` if found, `false` otherwise; raises `badarg` for
/// an invalid position or a list that is not proper up to the match.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_lists_keysearch(
    ctx: *mut ProcessContext,
    key: u64,
    n: u64,
    list: u64,
) -> u64 {
    use crate::atom_table::ATOM_FALSE;
    match key_search(ErlTerm(key), ErlTerm(n), ErlTerm(list)) {
        Ok(Some(tuple)) => with_ctx_heap(ctx, |heap| {
            let value_atom = ErlTerm::atom(global_atom_table().intern("value"));
            heap.alloc_tuple(&[value_atom, tuple]).0
        }),
        Ok(None) => ErlTerm::atom(ATOM_FALSE).0,
        Err(()) => raise_badarg(ctx),
    }
}

/// lists:append/1 — append a list of lists.
/// `append([L1, L2, ..., Ln])` returns `L1 ++ L2 ++ ... ++ Ln`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_lists_append(ctx: *mut ProcessContext, list_of_lists: u64) -> u64 {
    with_ctx_heap(ctx, |heap| {
        // Collect all sublists
        let mut sublists = Vec::new();
        let mut cursor = ErlTerm(list_of_lists);
        while cursor.is_cons() {
            unsafe {
                let cell = &*cursor.as_cons_ptr().unwrap();
                sublists.push(cell.head);
                cursor = cell.tail;
            }
        }
        if sublists.is_empty() {
            return ErlTerm::nil().0;
        }
        // Collect all elements from all sublists
        let mut all_elems = Vec::new();
        let mut last_tail = ErlTerm::nil();
        for (i, sublist) in sublists.iter().enumerate() {
            let mut cur = *sublist;
            while cur.is_cons() {
                unsafe {
                    let cell = &*cur.as_cons_ptr().unwrap();
                    all_elems.push(cell.head);
                    cur = cell.tail;
                }
            }
            // The last sublist's tail becomes the result tail (allows improper lists)
            if i == sublists.len() - 1 {
                last_tail = cur;
            }
        }
        if all_elems.is_empty() {
            return last_tail.0;
        }
        // Build result from back to front
        let mut result = last_tail;
        for elem in all_elems.into_iter().rev() {
            result = heap.alloc_cons(elem, result);
        }
        result.0
    })
}

/// lists:last/1 — return the last element of a non-empty list.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_lists_last(ctx: *mut ProcessContext, list: u64) -> u64 {
    let mut cursor = ErlTerm(list);
    if !cursor.is_cons() {
        // Empty list or non-list — badarg
        return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0);
    }
    let mut last = ErlTerm::nil();
    while cursor.is_cons() {
        unsafe {
            let cell = &*cursor.as_cons_ptr().unwrap();
            last = cell.head;
            cursor = cell.tail;
        }
    }
    last.0
}

/// lists:nth/2 — return the Nth element of a list (1-indexed).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_lists_nth(ctx: *mut ProcessContext, n: u64, list: u64) -> u64 {
    let n_term = ErlTerm(n);
    let pos = match n_term.as_small_int() {
        Some(i) if i >= 1 => i as usize,
        _ => return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
    };
    let mut cursor = ErlTerm(list);
    let mut idx = 1;
    while cursor.is_cons() {
        unsafe {
            let cell = &*cursor.as_cons_ptr().unwrap();
            if idx == pos {
                return cell.head.0;
            }
            idx += 1;
            cursor = cell.tail;
        }
    }
    // Index out of range
    raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0)
}

/// erlang:element/2 (1-indexed like Erlang)
#[no_mangle]
pub unsafe extern "C" fn nera_rt_element(ctx: *mut ProcessContext, index: u64, tuple: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_element(heap, &[ErlTerm(index), ErlTerm(tuple)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}


#[cfg(test)]
mod member_tests {
    use super::*;
    use crate::heap::ProcessHeap;

    #[test]
    fn member_uses_exact_numeric_equality() {
        // NERA-89: lists:member/2 must distinguish int/float and OTP 27+ signed zeros.
        let mut heap = ProcessHeap::new();
        let one = ErlTerm::small_int(1);
        let float_one = heap.alloc_float(1.0);
        let positive_zero = heap.alloc_float(0.0);
        let negative_zero = heap.alloc_float(-0.0);
        let cases = [
            (float_one, one, false),
            (one, float_one, false),
            (negative_zero, positive_zero, false),
            (positive_zero, negative_zero, false),
            (one, one, true),
            (float_one, heap.alloc_float(1.0), true),
            (positive_zero, heap.alloc_float(0.0), true),
            (negative_zero, heap.alloc_float(-0.0), true),
        ];
        let mut actual = Vec::new();
        let mut expected = Vec::new();
        for (query, candidate, same) in cases {
            let list = heap.alloc_list_from_slice(&[candidate]);
            // SAFETY: member does not use ctx; all terms stay live and no GC runs.
            actual.push(unsafe { nera_rt_lists_member(std::ptr::null_mut(), query.0, list.0) });
            expected.push(super::super::compare::bool_to_term(same));
        }
        assert_eq!(actual, expected);
    }

    #[cfg(not(target_env = "msvc"))]
    #[test]
    fn member_string_candidates_allocate_no_work_buffer() {
        // NERA-98 I1: exact membership must not add an allocation per string candidate.
        let mut heap = ProcessHeap::new();
        let mut string = |text: &str| {
            let chars: Vec<_> = text.bytes().map(|c| ErlTerm::small_int(c as i64)).collect();
            heap.alloc_list_from_slice(&chars)
        };
        let candidates = [string("POST"), string("PUT"), string("GET")];
        let get = string("GET");
        let patch = string("PATCH");
        let list = heap.alloc_list_from_slice(&candidates);
        let allocated = tikv_jemalloc_ctl::thread::allocatedp::read().unwrap();
        let before = allocated.get();
        for _ in 0..16 {
            // SAFETY: all graphs are live, immutable and uncollected; ctx is unused.
            unsafe {
                assert_eq!(nera_rt_lists_member(std::ptr::null_mut(), get.0, list.0),
                    super::super::compare::bool_to_term(true));
                assert_eq!(nera_rt_lists_member(std::ptr::null_mut(), patch.0, list.0),
                    super::super::compare::bool_to_term(false));
            }
        }
        assert_eq!(allocated.get() - before, 0, "flat strings need no comparison work buffer");
    }
}

#[cfg(test)]
mod subtraction_tests {
    use super::*;
    use crate::heap::ProcessHeap;
    use proptest::prelude::*;

    #[test]
    fn tuple_filter_collisions_still_require_exact_equality() {
        let mut heap = ProcessHeap::new();
        let a = heap.alloc_tuple(&[ErlTerm::small_int(0), ErlTerm::small_int(0)]);
        let b = heap.alloc_tuple(&[ErlTerm::small_int(1), ErlTerm::small_int(128)]);
        // SAFETY: both graphs belong to this live heap; no GC or mutation occurs.
        unsafe {
            assert_eq!(flat_tuple_fingerprint(a), flat_tuple_fingerprint(b));
            let mut candidates = FilteredCandidates::new(vec![a; 8]);
            for _ in 0..16 { assert!(!candidates.take(b)); }
            for _ in 0..8 { assert!(candidates.take(a)); }
            assert!(!candidates.take(a));
        }
    }

    proptest! {
        #[test]
        fn tuple_subtraction_matches_ordered_reference(
            left in prop::collection::vec((0_i64..4, 0_i64..10), 0..40),
            right in prop::collection::vec((0_i64..4, 0_i64..10), 0..30),
        ) {
            let mut expected = left.clone();
            for pair in &right {
                if let Some(pos) = expected.iter().position(|item| item == pair) {
                    expected.remove(pos);
                }
            }
            let mut heap = ProcessHeap::new();
            let mut tuple = |&(a,b): &(i64,i64)|
                heap.alloc_tuple(&[ErlTerm::small_int(a), ErlTerm::small_int(b)]);
            let right_terms: Vec<_> = right.iter().map(&mut tuple).collect();
            let left_terms: Vec<_> = left.iter().map(&mut tuple).collect();
            let right_list = heap.alloc_list_from_slice(&right_terms);
            let left_list = heap.alloc_list_from_slice(&left_terms);
            // SAFETY: all graphs are initialized and retained by heap; no GC runs.
            let actual = unsafe {
                subtract_list_elements(left_list, right_list).unwrap().into_iter()
                    .map(|term| {
                        let ptr = term.as_boxed_ptr().unwrap();
                        (ErlTerm(*ptr.add(1)).as_small_int().unwrap(),
                         ErlTerm(*ptr.add(2)).as_small_int().unwrap())
                    }).collect::<Vec<_>>()
            };
            prop_assert_eq!(actual, expected);
        }
    }
}
