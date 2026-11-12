//! ETS (Erlang Term Storage) table operations.

#![allow(unused_unsafe)]

use crate::atom_table::global_atom_table;
use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;

/// ets:new/2 — create a new ETS table.
/// Options: named_table, {keypos, N}, set, public, etc.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_new(ctx: *mut ProcessContext, name_term: u64, opts_term: u64) -> u64 {
    let name_atom = match ErlTerm(name_term).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    // Parse options list
    let mut opts = Vec::new();
    let mut cursor = ErlTerm(opts_term);
    while cursor.is_cons() {
        if let Some(ptr) = cursor.as_cons_ptr() {
            unsafe {
                let elem = (*ptr).head;
                // Check for atoms like 'set', 'public', 'named_table', etc.
                if let Some(atom_id) = elem.as_atom_id() {
                    if let Some(name) = global_atom_table().resolve(atom_id) {
                        opts.push((name, ErlTerm::nil()));
                    }
                }
                // Check for tuples like {keypos, N}
                if elem.is_boxed() {
                    if let Some(tptr) = elem.as_boxed_ptr() {
                        let header = crate::term::BoxedHeader(*tptr);
                        if header.subtype() == crate::term::BOXED_TUPLE && header.arity() == 2 {
                            let key = ErlTerm(*tptr.add(1));
                            let val = ErlTerm(*tptr.add(2));
                            if let Some(atom_id) = key.as_atom_id() {
                                if let Some(name) = global_atom_table().resolve(atom_id) {
                                    opts.push((name, val));
                                }
                            }
                        }
                    }
                }
                cursor = (*ptr).tail;
            }
        } else {
            break;
        }
    }

    let owner_pid = unsafe { (*ctx).pid };
    match crate::ets::ets_new_handle(name_atom, &opts, owner_pid) {
        Some(handle) => handle.0,
        None => raise_badarg(ctx),  // duplicate named table
    }
}

/// ets:insert/2 — insert a tuple or list of tuples.
/// Raises badarg if the table doesn't exist, an element is not a valid tuple,
/// or the list is improper.  Matches BEAM semantics.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_insert(ctx: *mut ProcessContext, table_term: u64, object_term: u64) -> u64 {
    match crate::ets::ets_insert(ErlTerm(table_term), ErlTerm(object_term)) {
        Ok(_) => ErlTerm::atom(crate::atom_table::ATOM_TRUE).0,
        Err(crate::ets::InsertError) => raise_badarg(ctx),
    }
}

/// ets:lookup/2 — look up tuples by key.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_lookup(ctx: *mut ProcessContext, table_term: u64, key_term: u64) -> u64 {
    with_ctx_heap(ctx, |heap| {
        crate::ets::ets_lookup(ErlTerm(table_term), ErlTerm(key_term), heap).0
    })
}

/// ets:delete/2 — delete a key from a table.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_delete(_ctx: *mut ProcessContext, table_term: u64, key_term: u64) -> u64 {
    crate::ets::ets_delete(ErlTerm(table_term), ErlTerm(key_term));
    ErlTerm::atom(crate::atom_table::ATOM_TRUE).0
}

/// ets:delete/1 — delete an entire table and free all its data.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_delete_table(_ctx: *mut ProcessContext, table_term: u64) -> u64 {
    crate::ets::ets_delete_table(ErlTerm(table_term));
    ErlTerm::atom(crate::atom_table::ATOM_TRUE).0
}

/// ets:delete_all_objects/1 — clear all objects from a table.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_delete_all_objects(_ctx: *mut ProcessContext, table_term: u64) -> u64 {
    crate::ets::ets_delete_all_objects(ErlTerm(table_term));
    ErlTerm::atom(crate::atom_table::ATOM_TRUE).0
}

/// ets:update_element/3 — update specific elements of a stored tuple.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_update_element(ctx: *mut ProcessContext, table_term: u64, key_term: u64, update_term: u64) -> u64 {
    let update = ErlTerm(update_term);
    let mut updates = Vec::new();

    if update.is_boxed() {
        // Single {Pos, Value} tuple
        if let Some(ptr) = update.as_boxed_ptr() {
            let header = crate::term::BoxedHeader(unsafe { *ptr });
            if header.subtype() == crate::term::BOXED_TUPLE && header.arity() == 2 {
                let pos = match ErlTerm(unsafe { *ptr.add(1) }).as_small_int() {
                    Some(p) => p as usize,
                    None => return raise_badarg(ctx),
                };
                let val = ErlTerm(unsafe { *ptr.add(2) });
                updates.push((pos, val));
            }
        }
    } else if update.is_cons() {
        // List of {Pos, Value} tuples
        let mut cursor = update;
        while cursor.is_cons() {
            if let Some(cptr) = cursor.as_cons_ptr() {
                unsafe {
                    let elem = (*cptr).head;
                    if let Some(ptr) = elem.as_boxed_ptr() {
                        let header = crate::term::BoxedHeader(*ptr);
                        if header.subtype() == crate::term::BOXED_TUPLE && header.arity() == 2 {
                            let pos = match ErlTerm(*ptr.add(1)).as_small_int() {
                                Some(p) => p as usize,
                                None => return raise_badarg(ctx),
                            };
                            let val = ErlTerm(*ptr.add(2));
                            updates.push((pos, val));
                        }
                    }
                    cursor = (*cptr).tail;
                }
            } else {
                break;
            }
        }
    }

    if crate::ets::ets_update_element(ErlTerm(table_term), ErlTerm(key_term), &updates) {
        ErlTerm::atom(crate::atom_table::ATOM_TRUE).0
    } else {
        ErlTerm::atom(crate::atom_table::ATOM_FALSE).0
    }
}

/// ets:select_delete/2 — delete matching records.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_select_delete(ctx: *mut ProcessContext, table_term: u64, match_spec_term: u64) -> u64 {
    match crate::ets::ets_select_delete(ErlTerm(table_term), ErlTerm(match_spec_term)) {
        Ok(count) => ErlTerm::small_int(count).0,
        Err(_) => raise_badarg(ctx),
    }
}

/// ets:member/2 — check if a key exists in the table.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_member(_ctx: *mut ProcessContext, table_term: u64, key_term: u64) -> u64 {
    if crate::ets::ets_member(ErlTerm(table_term), ErlTerm(key_term)) {
        ErlTerm::atom(crate::atom_table::ATOM_TRUE).0
    } else {
        ErlTerm::atom(crate::atom_table::ATOM_FALSE).0
    }
}

/// ets:update_counter/3 — atomically update integer counter(s).
/// Fast path: in-place read-modify-write, no deep-copy.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_update_counter(ctx: *mut ProcessContext, table_term: u64, key_term: u64, update_op_term: u64) -> u64 {
    match crate::ets::ets_update_counter(ErlTerm(table_term), ErlTerm(key_term), ErlTerm(update_op_term)) {
        Ok(crate::ets::CounterResult::Single(term)) => term.0,
        Ok(crate::ets::CounterResult::Multi(values)) => {
            with_ctx_heap(ctx, |heap| {
                // Build a proper list in reverse order via cons cells.
                let mut list = ErlTerm::nil();
                for &val in values.iter().rev() {
                    list = heap.alloc_cons(ErlTerm::small_int(val), list);
                }
                list.0
            })
        }
        Ok(crate::ets::CounterResult::BigSingle(value)) => with_ctx_heap(ctx, |heap| {
            crate::bigint::bigint_to_term(heap, value).0
        }),
        Ok(crate::ets::CounterResult::BigMulti(values)) => with_ctx_heap(ctx, |heap| {
            let mut list = ErlTerm::nil();
            for value in values.into_iter().rev() {
                let term = crate::bigint::bigint_to_term(heap, value);
                list = heap.alloc_cons(term, list);
            }
            list.0
        }),
        Err(_) => raise_badarg(ctx),
    }
}

/// ets:insert_new/2 — insert only if key does not exist.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_insert_new(_ctx: *mut ProcessContext, table_term: u64, object_term: u64) -> u64 {
    if crate::ets::ets_insert_new(ErlTerm(table_term), ErlTerm(object_term)) {
        ErlTerm::atom(crate::atom_table::ATOM_TRUE).0
    } else {
        ErlTerm::atom(crate::atom_table::ATOM_FALSE).0
    }
}

/// ets:tab2list/1 — return all entries as a list.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_tab2list(ctx: *mut ProcessContext, table_term: u64) -> u64 {
    with_ctx_heap(ctx, |heap| {
        crate::ets::ets_tab2list(ErlTerm(table_term), heap).0
    })
}

/// ets:first/1 — return an arbitrary key from the table.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_first(ctx: *mut ProcessContext, table_term: u64) -> u64 {
    with_ctx_heap(ctx, |heap| {
        crate::ets::ets_first(ErlTerm(table_term), heap).0
    })
}

/// ets:next/2 — return the next key after a given key.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_next(ctx: *mut ProcessContext, table_term: u64, prev_key_term: u64) -> u64 {
    with_ctx_heap(ctx, |heap| {
        crate::ets::ets_next(ErlTerm(table_term), ErlTerm(prev_key_term), heap).0
    })
}

/// ets:last/1 — return the last key from the table.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_last(ctx: *mut ProcessContext, table_term: u64) -> u64 {
    with_ctx_heap(ctx, |heap| {
        crate::ets::ets_last(ErlTerm(table_term), heap).0
    })
}

/// ets:prev/2 — return the previous key before a given key.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_prev(ctx: *mut ProcessContext, table_term: u64, next_key_term: u64) -> u64 {
    with_ctx_heap(ctx, |heap| {
        crate::ets::ets_prev(ErlTerm(table_term), ErlTerm(next_key_term), heap).0
    })
}

/// ets:info/1 — return table info as proplist.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_info_1(ctx: *mut ProcessContext, table_term: u64) -> u64 {
    with_ctx_heap(ctx, |heap| {
        crate::ets::ets_info(ErlTerm(table_term), None, heap).0
    })
}

/// ets:info/2 — return a specific info item.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_info_2(ctx: *mut ProcessContext, table_term: u64, item_term: u64) -> u64 {
    let item = ErlTerm(item_term);
    let item_name = if let Some(atom_id) = item.as_atom_id() {
        crate::atom_table::global_atom_table().resolve(atom_id)
    } else {
        None
    };
    with_ctx_heap(ctx, |heap| {
        crate::ets::ets_info(ErlTerm(table_term), item_name.as_deref(), heap).0
    })
}

/// ets:give_away/3 — transfer table ownership to another process.
/// Returns `true` on success, `false` if table not found or caller is not the owner.
/// Sends `{'ETS-TRANSFER', Tab, FromPid, GiftData}` to the new owner.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_ets_give_away(ctx: *mut ProcessContext, table_term: u64, pid_term: u64, gift_data_term: u64) -> u64 {
    let caller_pid = unsafe { (*ctx).pid };
    let new_owner = match ErlTerm(pid_term).as_pid_id() {
        Some(pid) => pid,
        None => return raise_badarg(ctx),
    };
    if crate::ets::ets_give_away(ErlTerm(table_term), caller_pid, new_owner) {
        // Send {'ETS-TRANSFER', Tab, FromPid, GiftData} to the new owner.
        let ets_transfer_atom = ErlTerm::atom(
            crate::atom_table::global_atom_table().intern("ETS-TRANSFER"),
        );
        let from_pid = ErlTerm::pid(caller_pid);
        super::process::nera_rt_send_tuple(
            ctx,
            ErlTerm::pid(new_owner).0,
            4,
            ets_transfer_atom.0,
            table_term,
            from_pid.0,
            gift_data_term,
        );
        ErlTerm::atom(crate::atom_table::ATOM_TRUE).0
    } else {
        ErlTerm::atom(crate::atom_table::ATOM_FALSE).0
    }
}

