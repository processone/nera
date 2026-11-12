//! Miscellaneous FFI: intern_atom, register_function, make_ref, monotonic_time, process dictionary, get_self.

#![allow(unused_unsafe)]

use std::sync::atomic::Ordering;
use crate::atom_table::global_atom_table;
use crate::entry::global_function_table;
use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;
use super::compare::exact_equal;

/// erlang:self/0 - get current process PID.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_get_self(ctx: *mut ProcessContext) -> u64 {
    if ctx.is_null() {
        return ErlTerm::pid(crate::process::ProcessId::new(0)).0;
    }
    let pid = unsafe { (*ctx).pid };
    ErlTerm::pid(pid).0
}

/// erlang:node/0 — Nera runs as a local, non-distributed node.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_node(_ctx: *mut ProcessContext) -> u64 {
    ErlTerm::atom(global_atom_table().intern("nonode@nohost")).0
}

/// erlang:node/1 — all supported PIDs and references originate locally.
/// Ports and distributed identifiers are not represented by this runtime.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_node_1(ctx: *mut ProcessContext, term: u64) -> u64 {
    let term = ErlTerm(term);
    if term.is_pid() || term.is_reference() {
        nera_rt_node(ctx)
    } else {
        raise_badarg(ctx)
    }
}

/// Intern an atom string and return the atom ErlTerm.
/// Used by the linker to register module/function names at startup.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_intern_atom(ptr: *const u8, len: u32) -> u64 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let s = std::str::from_utf8(bytes).unwrap_or("?");
    let id = global_atom_table().intern(s);
    ErlTerm::atom(id).0
}

/// Register a compiled function in the global function table.
/// Called from generated startup code.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_register_function(
    mod_atom: u64,
    fun_atom: u64,
    arity: u32,
    func_ptr: crate::process::ErlFn,
) {
    let mod_id = ErlTerm(mod_atom).as_atom_id().unwrap_or(0);
    let fun_id = ErlTerm(fun_atom).as_atom_id().unwrap_or(0);
    global_function_table().register(mod_id, fun_id, arity, func_ptr);
}

/// Batch-intern atom strings and return the atom ErlTerms.
/// Called from generated startup code to reduce per-atom FFI overhead.
///
/// `strings_base`: pointer to concatenated UTF-8 atom strings.
/// `offsets`:      array of `count` (offset_u32, len_u32) pairs into `strings_base`.
/// `out`:          output array of `count` atom ErlTerms (caller-allocated).
/// `count`:        number of atoms.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_intern_atoms_batch(
    strings_base: *const u8,
    offsets: *const u32,  // pairs: [offset0, len0, offset1, len1, ...]
    out: *mut u64,
    count: u32,
) {
    let table = global_atom_table();
    for i in 0..count as usize {
        let off = unsafe { *offsets.add(i * 2) } as usize;
        let len = unsafe { *offsets.add(i * 2 + 1) } as usize;
        let bytes = unsafe { std::slice::from_raw_parts(strings_base.add(off), len) };
        let s = std::str::from_utf8(bytes).unwrap_or("?");
        let id = table.intern(s);
        unsafe { *out.add(i) = ErlTerm::atom(id).0 };
    }
}

/// Batch-register compiled functions in the global function table.
/// Called from generated startup code to acquire the write lock once.
///
/// `mod_terms`:  array of `count` module atom terms (u64).
/// `fun_terms`:  array of `count` function atom terms (u64).
/// `arities`:    array of `count` arities (u32).
/// `func_ptrs`:  array of `count` function pointers.
/// `count`:      number of functions.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_register_functions_batch(
    mod_terms: *const u64,
    fun_terms: *const u64,
    arities: *const u32,
    func_ptrs: *const crate::process::ErlFn,
    count: u32,
) {
    let ft = global_function_table();
    let mut table = ft.table.write(); // single write lock for all registrations
    for i in 0..count as usize {
        let mod_id = ErlTerm(unsafe { *mod_terms.add(i) }).as_atom_id().unwrap_or(0);
        let fun_id = ErlTerm(unsafe { *fun_terms.add(i) }).as_atom_id().unwrap_or(0);
        let arity = unsafe { *arities.add(i) };
        let func = unsafe { *func_ptrs.add(i) };
        table.insert((mod_id, fun_id, arity), func);
    }
}

/// Populate the global function info table. Called once at startup after
/// batch registration. Arrays are parallel: mod_ids[i], fun_ids[i], arities[i].
#[no_mangle]
pub unsafe extern "C" fn nera_rt_set_func_info_table(
    mod_ids: *const u32,
    fun_ids: *const u32,
    arities: *const u32,
    count: u32,
) {
    let mut table = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let m = unsafe { *mod_ids.add(i) };
        let f = unsafe { *fun_ids.add(i) };
        let a = unsafe { *arities.add(i) };
        table.push((m, f, a));
    }
    let _ = crate::entry::FUNC_INFO_TABLE.set(table);
}

/// Create a new unique reference on the process heap.
/// Returns a tagged boxed pointer.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_make_ref(ctx: *mut ProcessContext) -> u64 {
    let id = NEXT_REF_ID.fetch_add(1, Ordering::Relaxed);
    with_ctx_heap(ctx, |heap| {
        let ptr = heap.alloc_words(REFERENCE_TOTAL_WORDS);
        unsafe {
            *ptr = crate::term::ReferenceKind::Ordinary.header().0;
            *ptr.add(REFERENCE_ID_WORD_INDEX) = id;
        }
        ErlTerm::boxed_unchecked(ptr as *const u64).0
    })
}

/// `erlang:get_stacktrace/0` — legacy stacktrace accessor, deprecated in
/// OTP 21+ in favour of `try ... catch C:R:Stk -> Stk end`.
///
/// Many older libraries still call it (often inside backwards-compat macros).
/// We return the empty list for now — symbol presence and a valid term is
/// what callers need; collecting the actual current-process stacktrace is a
/// future enhancement.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_get_stacktrace(_ctx: *mut ProcessContext) -> u64 {
    ErlTerm::nil().0
}

/// erlang:monotonic_time/0 — returns nanoseconds since an arbitrary epoch.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_monotonic_time(ctx: *mut ProcessContext) -> u64 {
    integer_term(ctx, monotonic_nanos())
}

/// erlang:monotonic_time/1 — returns time in the specified unit.
/// Unit is an atom: nanosecond, microsecond, millisecond, second, native.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_monotonic_time_1(ctx: *mut ProcessContext, unit_term: u64) -> u64 {
    let nanos = monotonic_nanos();
    let unit = ErlTerm(unit_term);
    if let Some(atom_id) = unit.as_atom_id() {
        let atom_table = global_atom_table();
        if let Some(name) = atom_table.resolve(atom_id) {
            let value = match name.as_str() {
                "nanosecond"  => nanos,
                "microsecond" => nanos / 1_000,
                "millisecond" => nanos / 1_000_000,
                "second"      => nanos / 1_000_000_000,
                "native"      => nanos,
                _ => return raise_badarg(ctx),
            };
            return integer_term(ctx, value);
        }
    }
    raise_badarg(ctx)
}

/// os:getenv/1 — read a process environment variable by character-list name.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_os_getenv(ctx: *mut ProcessContext, name: u64) -> u64 {
    let mut pending = vec![(ErlTerm(name), true)];
    let mut key = String::new();
    while let Some((term, list_required)) = pending.pop() {
        if term.is_nil() { continue; }
        if term.is_cons() {
            let cell = &*term.as_cons_ptr().unwrap();
            pending.push((cell.tail, true));
            pending.push((cell.head, false));
            continue;
        }
        if list_required { return raise_badarg(ctx); }
        let Some(character) = term.as_small_int()
            .and_then(|n| u32::try_from(n).ok()).and_then(char::from_u32) else { return raise_badarg(ctx); };
        if character == '\0' || character == '=' { return raise_badarg(ctx); }
        key.push(character);
    }
    let Some(value) = std::env::var_os(key) else { return ErlTerm::atom(crate::atom_table::ATOM_FALSE).0; };
    let Some(value) = value.to_str() else { return raise_badarg(ctx); };
    let characters: Vec<_> = value.chars().map(|ch| ErlTerm::small_int(ch as i64)).collect();
    with_ctx_heap(ctx, |heap| heap.alloc_list_from_slice(&characters).0)
}

/// erlang:unique_integer/0 — unique within this runtime instance.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_unique_integer(ctx: *mut ProcessContext) -> u64 {
    static NEXT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
    match NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |value| value.checked_add(1)) {
        Ok(value) => integer_term(ctx, value),
        Err(_) => raise_error(ctx, ErlTerm::atom(global_atom_table().intern("system_limit")).0),
    }
}

/// `put(Key, Value)` — store a key-value pair, return old value or `undefined`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_put(ctx: *mut ProcessContext, key: u64, value: u64) -> u64 {
    let dict_ptr = unsafe { (*ctx).dict_ptr };
    if dict_ptr.is_null() {
        return atom_undefined();
    }
    let dict = unsafe { &mut *dict_ptr };
    let key_term = ErlTerm(key);
    let val_term = ErlTerm(value);
    // Dictionary keys use exact equality, including numeric types in compounds.
    for entry in dict.iter_mut() {
        if exact_equal(entry.0, key_term) {
            let old = entry.1;
            entry.1 = val_term;
            return old.0;
        }
    }
    dict.push((key_term, val_term));
    atom_undefined()
}

/// `get()` — enumerate the process dictionary without removing its entries.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_get_all(ctx: *mut ProcessContext) -> u64 {
    let dict_ptr = (*ctx).dict_ptr;
    if dict_ptr.is_null() { return ErlTerm::nil().0; }
    let entries = (&*dict_ptr).clone();
    with_ctx_heap(ctx, |heap| {
        let pairs: Vec<_> = entries.iter().map(|(key, value)| heap.alloc_tuple(&[*key, *value])).collect();
        heap.alloc_list_from_slice(&pairs).0
    })
}

/// `get(Key)` — return the value for Key, or `undefined`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_get(ctx: *mut ProcessContext, key: u64) -> u64 {
    let dict_ptr = unsafe { (*ctx).dict_ptr };
    if dict_ptr.is_null() {
        return atom_undefined();
    }
    let dict = unsafe { &*dict_ptr };
    let key_term = ErlTerm(key);
    for &(k, v) in dict.iter() {
        if exact_equal(k, key_term) {
            return v.0;
        }
    }
    atom_undefined()
}

/// `erase(Key)` — remove Key from the dictionary, return old value or `undefined`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_erase(ctx: *mut ProcessContext, key: u64) -> u64 {
    let dict_ptr = unsafe { (*ctx).dict_ptr };
    if dict_ptr.is_null() {
        return atom_undefined();
    }
    let dict = unsafe { &mut *dict_ptr };
    let key_term = ErlTerm(key);
    for i in 0..dict.len() {
        if exact_equal(dict[i].0, key_term) {
            let old = dict[i].1;
            dict.swap_remove(i);
            return old.0;
        }
    }
    atom_undefined()
}

/// `get_keys()` — return a list of all keys in the process dictionary.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_get_keys(ctx: *mut ProcessContext) -> u64 {
    let dict_ptr = unsafe { (*ctx).dict_ptr };
    if dict_ptr.is_null() {
        return ErlTerm::nil().0;
    }
    let dict = unsafe { &*dict_ptr };
    if dict.is_empty() {
        return ErlTerm::nil().0;
    }
    let keys: Vec<ErlTerm> = dict.iter().map(|(k, _)| *k).collect();
    with_ctx_heap(ctx, |heap| heap.alloc_list_from_slice(&keys).0)
}

/// `get_keys(Value)` — return keys whose dictionary values are exactly equal to Value.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_get_keys_1(ctx: *mut ProcessContext, value: u64) -> u64 {
    let dict_ptr = unsafe { (*ctx).dict_ptr };
    if dict_ptr.is_null() {
        return ErlTerm::nil().0;
    }
    let value = ErlTerm(value);
    let keys: Vec<ErlTerm> = unsafe { &*dict_ptr }.iter()
        .filter(|(_, stored)| unsafe { exact_equal(*stored, value) })
        .map(|(key, _)| *key)
        .collect();
    with_ctx_heap(ctx, |heap| heap.alloc_list_from_slice(&keys).0)
}

/// nera:memory_stats/0 — print jemalloc allocation stats to stderr
/// and return a list of {Key, Value} tuples.
///
/// Returns [{allocated, N}, {active, N}, {resident, N}, {mapped, N}]
/// where all values are in bytes.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_memory_stats(ctx: *mut ProcessContext) -> u64 {
    #[cfg(not(target_env = "msvc"))]
    {
        use tikv_jemalloc_ctl::{epoch, stats};

        // Advance the jemalloc epoch to refresh stats.
        let _ = epoch::advance();

        let allocated = stats::allocated::read().unwrap_or(0);
        let active = stats::active::read().unwrap_or(0);
        let resident = stats::resident::read().unwrap_or(0);
        let mapped = stats::mapped::read().unwrap_or(0);

        eprintln!(
            "JEMALLOC STATS: allocated={:.2}MB active={:.2}MB resident={:.2}MB mapped={:.2}MB",
            allocated as f64 / 1_048_576.0,
            active as f64 / 1_048_576.0,
            resident as f64 / 1_048_576.0,
            mapped as f64 / 1_048_576.0,
        );

        // Build result list: [{allocated, N}, {active, N}, {resident, N}, {mapped, N}]
        let atom_table = global_atom_table();
        let a_allocated = atom_table.intern("allocated");
        let a_active = atom_table.intern("active");
        let a_resident = atom_table.intern("resident");
        let a_mapped = atom_table.intern("mapped");

        with_ctx_heap(ctx, |heap| {
            let pairs = [
                (ErlTerm::atom(a_allocated), ErlTerm::small_int(allocated as i64)),
                (ErlTerm::atom(a_active), ErlTerm::small_int(active as i64)),
                (ErlTerm::atom(a_resident), ErlTerm::small_int(resident as i64)),
                (ErlTerm::atom(a_mapped), ErlTerm::small_int(mapped as i64)),
            ];
            let mut tuples = Vec::new();
            for (key, val) in &pairs {
                let ptr = heap.alloc_words(3);
                unsafe {
                    *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Tuple, 2).0;
                    *ptr.add(1) = key.0;
                    *ptr.add(2) = val.0;
                }
                tuples.push(ErlTerm::boxed_unchecked(ptr as *const u64));
            }
            heap.alloc_list_from_slice(&tuples).0
        })
    }
    #[cfg(target_env = "msvc")]
    {
        let _ = ctx;
        ErlTerm::nil().0
    }
}

// Linker-provided metadata symbols (defined in the metadata object generated by nera build).
extern "C" {
    static __nera_compiler_version: u8;
    static __nera_compiler_version_len: u8;
    static __nera_compile_date: u8;
    static __nera_compile_date_len: u8;
}

/// nera:version/0 — return the Nera compiler version that compiled this binary as a binary string.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_nera_version(ctx: *mut ProcessContext) -> u64 {
    let ptr = unsafe { &__nera_compiler_version as *const u8 };
    let len_ptr = unsafe { &__nera_compiler_version_len as *const u8 };
    let len = unsafe { *(len_ptr as *const u32) } as usize;
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(bytes).0)
}

/// nera:compile_date/0 — return the compilation date of this binary as a binary string.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_nera_compile_date(ctx: *mut ProcessContext) -> u64 {
    let ptr = unsafe { &__nera_compile_date as *const u8 };
    let len_ptr = unsafe { &__nera_compile_date_len as *const u8 };
    let len = unsafe { *(len_ptr as *const u32) } as usize;
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    with_ctx_heap(ctx, |heap| heap.alloc_binary_auto(bytes).0)
}


/// Wall-clock elapsed time and time since the preceding statistics call, in ms.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_statistics(ctx: *mut ProcessContext, item: u64) -> u64 {
    let key = ErlTerm(item).as_atom_id().and_then(|id| global_atom_table().resolve(id));
    if key.as_deref() != Some("wall_clock") { return raise_badarg(ctx); }
    static LAST_CALL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let total = crate::entry::runtime_start_instant().elapsed().as_millis() as u64;
    let previous = LAST_CALL.fetch_max(total, Ordering::AcqRel);
    with_ctx_heap(ctx, |heap| heap.alloc_tuple(&[
        ErlTerm::small_int(total as i64), ErlTerm::small_int(total.saturating_sub(previous) as i64)
    ]).0)
}

#[cfg(test)]
mod dictionary_tests {
    use super::*;
    use crate::{heap::ProcessHeap, process::ProcessId};

    #[test]
    fn getenv_reads_existing_path() {
        let mut heap = ProcessHeap::new();
        let mut ctx = ProcessContext::new(ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let name = heap.alloc_list_from_slice(&[80, 65, 84, 72].map(ErlTerm::small_int));
        // SAFETY: the context and name list belong to this live heap.
        let result = unsafe { nera_rt_os_getenv(&mut ctx, name.0) };
        assert_ne!(result, ErlTerm::atom(crate::atom_table::ATOM_FALSE).0);
    }

    #[test]
    fn unique_integers_do_not_repeat() {
        let mut ctx = ProcessContext::new(ProcessId::new(1));
        // SAFETY: ctx is valid; the first counter values are immediate integers.
        unsafe {
            assert_ne!(nera_rt_unique_integer(&mut ctx), nera_rt_unique_integer(&mut ctx));
        }
    }

    #[test]
    fn get_all_preserves_dictionary_entries() {
        let mut heap = ProcessHeap::new();
        let mut dictionary = vec![(ErlTerm::small_int(7), ErlTerm::small_int(42))];
        let mut ctx = ProcessContext::new(ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        ctx.dict_ptr = &mut dictionary;
        // SAFETY: the context, heap and dictionary remain live throughout this call.
        let result = unsafe { ErlTerm(nera_rt_get_all(&mut ctx)) };
        assert_ne!(result, ErlTerm::nil());
        assert_eq!(dictionary.len(), 1);
    }
}
