//! Process management: spawn, send, link, monitor, register, process_info.

#![allow(unused_unsafe)]

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use crate::atom_table::global_atom_table;
use crate::entry::global_function_table;
use crate::heap::{FragmentHeap, MapPairs, ProcessHeap};
use crate::message::{deep_copy_term, direct_copy_message};
use crate::process::{Process, ProcessContext, ProcessEntry};
use crate::term::*;
use super::helpers::*;

// Thread-local cache for the last send target. Avoids repeated RwLock +
// HashMap + Arc::clone on the process table when sending to the same PID.
thread_local! {
    static SEND_CACHE: RefCell<Option<(crate::process::ProcessId, Arc<ProcessEntry>)>> = const { RefCell::new(None) };
}

/// garbage_collect/0 must not move objects while compiled callers retain raw terms.
/// Suspend through the existing continuation protocol, then collect under the
/// scheduler's exclusive ownership before returning true to the caller.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_garbage_collect(ctx: *mut ProcessContext) -> u64 {
    unsafe {
        (*ctx).full_gc_requested = true;
        super::receive::nera_rt_set_resume(ctx, resume_garbage_collect, std::ptr::null(), 0, 0);
        (*ctx).reductions = 0;
        (*ctx).status = crate::process::STATUS_YIELD;
    }
    ErlTerm::nil().0
}

unsafe extern "C" fn resume_garbage_collect(ctx: *mut ProcessContext, _: *const u64, _: u32) {
    unsafe { (*ctx).result = ErlTerm::atom(crate::atom_table::ATOM_TRUE).0; }
}

/// Send a message to a process by PID.
/// Deep-copies the message to the receiver's heap.
/// Returns the message term (Erlang convention: Pid ! Msg evaluates to Msg).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_send_message(ctx: *mut ProcessContext, pid_term: u64, msg: u64) -> u64 {
    let pid_erl = ErlTerm(pid_term);

    // Resolve the target: either a PID directly, or an atom (registered name).
    let target_pid = if let Some(id) = pid_erl.as_pid_id() {
        id
    } else if let Some(atom_id) = pid_erl.as_atom_id() {
        // Look up registered name
        match with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
            match scheduler {
                Some(sched) => {
                    let names = sched.registered_names.read();
                    match names.get(&atom_id) {
                        Some(&pid) => Ok(pid),
                        None => Err(raise_badarg(ctx)), // Name not registered
                    }
                }
                None => Err(msg),
            }
        }) {
            Ok(pid) => pid,
            Err(result) => return result,
        }
    } else {
        return raise_badarg(ctx); // Not a PID and not an atom
    };

    // Self-send: push directly to our own mailbox via the ProcessContext
    // pointer. This avoids a deadlock since the worker thread already holds
    // the Mutex lock on the current process.
    if !ctx.is_null() {
        let self_pid = unsafe { (*ctx).pid };
        if self_pid == target_pid {
            let mailbox_ptr = unsafe { (*ctx).mailbox_ptr };
            if !mailbox_ptr.is_null() {
                let mailbox = unsafe { &mut *mailbox_ptr };
                mailbox.send(ErlTerm(msg));
            }
            return msg;
        }
    }

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => {
                return msg;
            }
        };

        // Try thread-local cache first to avoid RwLock + HashMap lookup
        let entry = SEND_CACHE.with(|cache| {
            let c = cache.borrow();
            if let Some((cached_pid, ref e)) = *c {
                if cached_pid == target_pid {
                    return Some(e.clone());
                }
            }
            None
        }).or_else(|| {
            let e = sched.process_table.get(&target_pid).map(|r| r.clone());
            if let Some(ref e) = e {
                SEND_CACHE.with(|cache| {
                    *cache.borrow_mut() = Some((target_pid, e.clone()));
                });
            }
            e
        });
        if let Some(entry) = entry {
            // Try direct-to-heap send when the target process is not being
            // executed by any worker and has nothing queued in its inbox.
            // This bypasses the SegQueue entirely, eliminating fragment
            // allocation + copy-to-heap overhead.
            let direct_sent = if let Some(mut proc) = entry.try_lock_for_direct_send() {
                let copied = direct_copy_message(ErlTerm(msg), &mut proc.heap);
                proc.mailbox.send(copied);
                drop(proc);
                true
            } else {
                false
            };
            // The receiver's timer is left to the worker that runs it, which
            // cancels it under the process lock. Once queued, the receiver can
            // run into a later timed receive, and cancelling here would remove
            // that receive's timer.
            if direct_sent {
                if entry.try_wake() {
                    sched.runnable_count.fetch_add(1, Ordering::AcqRel);
                    sched.dispatch_to_worker(entry);
                }
                return msg;
            }

            // Fallback: lock-free send via SegQueue fragment
            let fragment = crate::message::MessageFragment::from_term(ErlTerm(msg));
            entry.inbox.push(fragment);

            // Try to wake the process if it was waiting in a receive.
            if entry.try_wake() {
                sched.runnable_count.fetch_add(1, Ordering::AcqRel);
                sched.dispatch_to_worker(entry);
            }
        }
        msg
    })
}

// Keep the temporary root and deep-copy setup out of the simple send frame.
#[cold]
#[inline(never)]
unsafe fn copy_complex_tuple(heap: &mut ProcessHeap, arity: usize, raw: &[u64; 4]) -> ErlTerm {
    // The aligned tuple is a temporary root, not receiver-owned storage.
    // Copy the entire graph while it and the sender's children are live.
    // One copier preserves aliases and owns any binary descriptors it copies.
    let words = [
        BoxedHeader::new(crate::term::BoxedSubtype::Tuple, arity as u64).0,
        raw[0], raw[1], raw[2], raw[3],
    ];
    deep_copy_term(ErlTerm::boxed_unchecked(words.as_ptr()), heap)
}

// Build on the target heap. Only self-send may retain complex child pointers.
#[inline(always)]
unsafe fn build_tuple_on_heap(
    heap: &mut ProcessHeap, arity: usize, raw: &[u64; 4], copy_complex: bool,
) -> ErlTerm {
    // Try fast path: all elements are immediate or refc binary.
    let mut all_simple = true;
    for item in raw[..arity].iter() {
        let elem = ErlTerm(*item);
        let elem = if elem.is_external() { elem.strip_external_bit() } else { elem };
        match elem.tag() {
            TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL => {}
            TAG_BOXED if elem.is_refc_binary() => {}
            _ => { all_simple = false; break; }
        }
    }
    if all_simple {
        let tuple_ptr = heap.alloc_words(1 + arity);
        *tuple_ptr = BoxedHeader::new(crate::term::BoxedSubtype::Tuple, arity as u64).0;
        for (i, item) in raw[..arity].iter().enumerate() {
            let elem = ErlTerm(*item);
            let elem = if elem.is_external() { elem.strip_external_bit() } else { elem };
            let val = match elem.tag() {
                TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL => elem,
                _ => {
                    // Refc binary
                    let bp = elem.as_boxed_ptr().unwrap();
                    let bh = BoxedHeader(*bp);
                    let bit_count = bh.arity() as usize;
                    let raw_arc = *bp.add(REFC_BINARY_PTR_WORD_INDEX);
                    let off = *bp.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
                    let data = crate::heap::clone_arc_from_raw(raw_arc);
                    heap.alloc_binary_slice(crate::binary::BinarySlice::from_runtime_bits(data, off, bit_count))
                }
            };
            *tuple_ptr.add(1 + i) = val.0;
        }
        ErlTerm::boxed_unchecked(tuple_ptr as *const u64)
    } else if copy_complex {
        copy_complex_tuple(heap, arity, raw)
    } else {
        // Self-send stays within one heap; retain the existing children.
        let tuple_ptr = heap.alloc_words(1 + arity);
        *tuple_ptr = BoxedHeader::new(crate::term::BoxedSubtype::Tuple, arity as u64).0;
        for (i, item) in raw[..arity].iter().enumerate() {
            *tuple_ptr.add(1 + i) = *item;
        }
        ErlTerm::boxed_unchecked(tuple_ptr as *const u64)
    }
}

/// Fused MakeTuple + Send: constructs an isolated receiver tuple and returns
/// the sender's message tuple. Arity ≤ 4.
/// Signature: (ctx, pid, arity, e0, e1, e2, e3) — unused elems are 0.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_send_tuple(
    ctx: *mut ProcessContext,
    pid_term: u64,
    arity: u64,
    e0: u64, e1: u64, e2: u64, e3: u64,
) -> u64 {
    let arity = arity as usize;
    let raw_elems = [e0, e1, e2, e3];

    // Helper: build a fragment from raw elements (for lock-free path).
    unsafe fn build_fragment(arity: usize, raw: &[u64; 4]) -> crate::message::MessageFragment {
        use crate::message::{FragmentElem, MessageFragment};
        use crate::heap::clone_arc_from_raw;

        // Check if all elements are simple (immediate or refc binary).
        let mut all_simple = true;
        for item in raw[..arity].iter() {
            let elem = ErlTerm(*item);
            let elem = if elem.is_external() { elem.strip_external_bit() } else { elem };
            match elem.tag() {
                TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL => {}
                TAG_BOXED if elem.is_refc_binary() => {}
                _ => { all_simple = false; break; }
            }
        }

        if all_simple && arity <= 2 {
            let mut elems: [FragmentElem; 2] = Default::default();
            for i in 0..arity {
                let elem = ErlTerm(raw[i]);
                let elem = if elem.is_external() { elem.strip_external_bit() } else { elem };
                elems[i] = match elem.tag() {
                    TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL => FragmentElem::Immediate(crate::message::ImmediateTerm::new(elem)),
                    _ => {
                        let bp = elem.as_boxed_ptr().unwrap();
                        let bh = BoxedHeader(*bp);
                        let bit_count = bh.arity() as usize;
                        let raw_arc = *bp.add(REFC_BINARY_PTR_WORD_INDEX);
                        let off = *bp.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
                        let data = clone_arc_from_raw(raw_arc);
                        FragmentElem::RefcBinary(unsafe { crate::binary::BinarySlice::from_runtime_bits(data, off, bit_count) })
                    }
                };
            }
            return MessageFragment::InlineTuple { arity: crate::message::InlineArity::new(arity as u32), elems };
        }

        if all_simple {
            let mut elems = Vec::with_capacity(arity);
            for item in raw[..arity].iter() {
                let elem = ErlTerm(*item);
                let elem = if elem.is_external() { elem.strip_external_bit() } else { elem };
                elems.push(match elem.tag() {
                    TAG_SMALL_INT | TAG_ATOM | TAG_PID | TAG_NIL => FragmentElem::Immediate(crate::message::ImmediateTerm::new(elem)),
                    _ => {
                        let bp = elem.as_boxed_ptr().unwrap();
                        let bh = BoxedHeader(*bp);
                        let bit_count = bh.arity() as usize;
                        let raw_arc = *bp.add(REFC_BINARY_PTR_WORD_INDEX);
                        let off = *bp.add(REFC_BINARY_OFFSET_WORD_INDEX) as usize;
                        let data = clone_arc_from_raw(raw_arc);
                        FragmentElem::RefcBinary(unsafe { crate::binary::BinarySlice::from_runtime_bits(data, off, bit_count) })
                    }
                });
            }
            return MessageFragment::SimpleTuple(elems);
        }

        // Complex: build tuple on a temporary heap, deep-copy to fragment.
        let mut heap = ProcessHeap::new_with_capacity(64);
        let tuple_ptr = heap.alloc_words(1 + arity);
        *tuple_ptr = BoxedHeader::new(crate::term::BoxedSubtype::Tuple, arity as u64).0;
        for (i, item) in raw[..arity].iter().enumerate() {
            *tuple_ptr.add(1 + i) = *item;
        }
        let root = ErlTerm::boxed_unchecked(tuple_ptr as *const u64);
        // Need to deep-copy because raw elements reference sender heap.
        let mut frag_heap = FragmentHeap::new_with_capacity(64);
        let copied = unsafe { frag_heap.copy_raw_term(root) };
        // SAFETY: this root was built/copied entirely into the transferred heap.
        unsafe { crate::message::MessageFragment::from_owned_heap(frag_heap, copied) }
    }

    let pid_erl = ErlTerm(pid_term);

    // Resolve target PID (same logic as nera_rt_send_message).
    let target_pid = if let Some(id) = pid_erl.as_pid_id() {
        id
    } else if let Some(atom_id) = pid_erl.as_atom_id() {
        match with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
            match scheduler {
                Some(sched) => {
                    let names = sched.registered_names.read();
                    match names.get(&atom_id) {
                        Some(&pid) => Ok(pid),
                        None => Err(raise_badarg(ctx)),
                    }
                }
                None => Err(ErlTerm::nil().0),
            }
        }) {
            Ok(pid) => pid,
            Err(result) => return result,
        }
    } else {
        return raise_badarg(ctx);
    };

    // Self-send: build tuple on own heap, push to own mailbox.
    if !ctx.is_null() {
        let self_pid = (*ctx).pid;
        if self_pid == target_pid {
            let heap_ptr = (*ctx).heap_ptr;
            let mailbox_ptr = (*ctx).mailbox_ptr;
            if !heap_ptr.is_null() && !mailbox_ptr.is_null() {
                let heap = &mut *heap_ptr;
                let tuple = build_tuple_on_heap(heap, arity, &raw_elems, false);
                let mailbox = &mut *mailbox_ptr;
                mailbox.send(tuple);
                return tuple.0;
            }
            return ErlTerm::nil().0;
        }
    }

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return ErlTerm::nil().0,
        };

        // Resolve target process entry (reuse SEND_CACHE).
        let entry = SEND_CACHE.with(|cache| {
            let c = cache.borrow();
            if let Some((cached_pid, ref e)) = *c {
                if cached_pid == target_pid {
                    return Some(e.clone());
                }
            }
            None
        }).or_else(|| {
            let e = sched.process_table.get(&target_pid).map(|r| r.clone());
            if let Some(ref e) = e {
                SEND_CACHE.with(|cache| {
                    *cache.borrow_mut() = Some((target_pid, e.clone()));
                });
            }
            e
        });

        // Build a result tuple on the sender's heap for the return value (Erlang convention).
        // Actually, we need to return the message term. For SendDirect, the result is the
        // tuple itself. We must build it somewhere for the sender's return value.
        // Optimization: build on sender heap only if the sender actually uses the result.
        // For now, return ErlTerm::nil().0 — the codegen should know the result is unused in
        // the common case. Actually, Erlang says `Pid ! Msg` evaluates to `Msg`, so we
        // need to return the tuple. Let's build it on the sender heap.
        let return_val = if !ctx.is_null() {
            let heap_ptr = (*ctx).heap_ptr;
            if !heap_ptr.is_null() {
                let heap = &mut *heap_ptr;
                let tuple_ptr = heap.alloc_words(1 + arity);
                *tuple_ptr = BoxedHeader::new(crate::term::BoxedSubtype::Tuple, arity as u64).0;
                for (i, item) in raw_elems[..arity].iter().enumerate() {
                    *tuple_ptr.add(1 + i) = *item;
                }
                ErlTerm::boxed_unchecked(tuple_ptr as *const u64).0
            } else {
                ErlTerm::nil().0
            }
        } else {
            ErlTerm::nil().0
        };

        if let Some(entry) = entry {
            // As in nera_rt_send_message, a queued inbox keeps this send there.
            let direct_sent = if let Some(mut proc) = entry.try_lock_for_direct_send() {
                let tuple = build_tuple_on_heap(&mut proc.heap, arity, &raw_elems, true);
                proc.mailbox.send(tuple);
                drop(proc);
                true
            } else {
                false
            };
            // As in nera_rt_send_message, the worker cancels the timer.
            if direct_sent {
                if entry.try_wake() {
                    sched.runnable_count.fetch_add(1, Ordering::AcqRel);
                    sched.dispatch_to_worker(entry);
                }
                return return_val;
            }

            // Fallback: lock-free fragment path.
            let fragment = build_fragment(arity, &raw_elems);
            entry.inbox.push(fragment);

            if entry.try_wake() {
                sched.runnable_count.fetch_add(1, Ordering::AcqRel);
                sched.dispatch_to_worker(entry);
            }
        }
        return_val
    })
}

/// Spawn a new process running the given module:function/arity.
/// The function is looked up in the global function table.
/// Returns the new process's PID as a tagged PID term.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_spawn(
    ctx: *mut ProcessContext,
    mod_atom: u64,
    fun_atom: u64,
    args: u64,
) -> u64 {
    let mod_id = match ErlTerm(mod_atom).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };
    let fun_id = match ErlTerm(fun_atom).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    // Count args from the Erlang list to determine arity
    let mut arity = 0u32;
    let mut current = ErlTerm(args);
    while current.is_cons() {
        arity += 1;
        if let Some(ptr) = current.as_cons_ptr() {
            current = unsafe { (*ptr).tail };
        } else {
            break;
        }
    }

    // Look up the function
    let func = match global_function_table().lookup(mod_id, fun_id, arity) {
        Some(f) => f,
        None => {
            eprintln!(
                "spawn: function not found: {}:{}/{}",
                global_atom_table().resolve(mod_id).unwrap_or_else(|| "?".to_string()),
                global_atom_table().resolve(fun_id).unwrap_or_else(|| "?".to_string()),
                arity
            );
            return raise_badarg(ctx);
        }
    };

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return raise_badarg(ctx),
        };

        // Collect args from the list
        let mut arg_terms = Vec::new();
        let mut current = ErlTerm(args);
        while current.is_cons() {
            if let Some(ptr) = current.as_cons_ptr() {
                unsafe {
                    arg_terms.push(ErlTerm((*ptr).head.0));
                    current = (*ptr).tail;
                }
            } else {
                break;
            }
        }

        let Some(pid) = sched.try_next_pid() else {
            return super::helpers::raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_SYSTEM_LIMIT).to_raw());
        };
        let process = Process::with_entry(pid, func, arg_terms);
        let arc = std::sync::Arc::new(crate::process::ProcessEntry::new(process));
        // Newly spawned processes inherit the caller's group leader.
        if !ctx.is_null() {
            if let Some(parent) = sched.process_table.get(&unsafe { (*ctx).pid }) {
                arc.set_group_leader(parent.group_leader());
            }
        }
        sched.process_table.insert(pid, std::sync::Arc::clone(&arc));
        sched.alive_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.runnable_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.dispatch_to_worker(arc);

        ErlTerm::pid(pid).0
    })
}

/// Spawn a new process running the given closure (fun/0).
/// The closure's function pointer and captured environment variables are
/// extracted and set as the new process's initial_fn/initial_args so the
/// standard resume mechanism (receive...after, yield) works correctly.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_spawn_closure(
    ctx: *mut ProcessContext,
    closure_term: u64,
) -> u64 {
    let term = ErlTerm(closure_term);

    if !term.is_boxed() {
        return raise_badarg(ctx);
    }

    let boxed_ptr = match term.as_boxed_ptr() {
        Some(p) => p,
        None => return raise_badarg(ctx),
    };
    let header = unsafe { BoxedHeader(*boxed_ptr) };

    if header.subtype() != BOXED_CLOSURE {
        return raise_badarg(ctx);
    }

    let env_count = header.arity() as usize;
    let fn_ptr = unsafe { *boxed_ptr.add(1) };

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return raise_badarg(ctx),
        };

        let Some(pid) = sched.try_next_pid() else {
            return super::helpers::raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_SYSTEM_LIMIT).to_raw());
        };
        let mut process = Process::new(pid);

        // Deep-copy captured env vars to the new process's heap
        let mut arg_terms = Vec::with_capacity(env_count);
        for i in 0..env_count {
            let env_val = ErlTerm(unsafe { *boxed_ptr.add(3 + i) });
            let copied = deep_copy_term(env_val, &mut process.heap);
            arg_terms.push(copied);
        }

        // Set up the process entry: the closure's wrapper function with env vars as args
        let func: crate::process::ErlFn = unsafe { std::mem::transmute(fn_ptr) };
        process.initial_fn = Some(func);
        process.initial_args = arg_terms;

        let arc = std::sync::Arc::new(crate::process::ProcessEntry::new(process));
        // Newly spawned processes inherit the caller's group leader.
        if !ctx.is_null() {
            if let Some(parent) = sched.process_table.get(&unsafe { (*ctx).pid }) {
                arc.set_group_leader(parent.group_leader());
            }
        }
        sched.process_table.insert(pid, std::sync::Arc::clone(&arc));
        sched.alive_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.runnable_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.dispatch_to_worker(arc);

        ErlTerm::pid(pid).0
    })
}

/// `link(Pid)` — establish a bidirectional link between the current process
/// and the target. If the target doesn't exist, raises `noproc` error.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_link(ctx: *mut ProcessContext, pid_term: u64) -> u64 {
    let target_pid = match ErlTerm(pid_term).as_pid_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };
    let self_pid = unsafe { (*ctx).pid };
    if self_pid == target_pid {
        return ErlTerm::atom(crate::atom_table::ATOM_TRUE).0;
    }

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_NOPROC).0),
        };

        // Add link in target process
        let proc_arc = sched.process_table.get(&target_pid).map(|r| r.clone());
        match proc_arc {
            Some(arc) => {
                let mut proc = arc.lock_for_runtime();
                if proc.status() == crate::process::ProcessStatus::Exited {
                    return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_NOPROC).0);
                }
                proc.otp.links.insert(self_pid);
            }
            None => {
                return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_NOPROC).0);
            }
        }

        // Add link in self via ctx.otp_ptr
        let otp = unsafe { &mut *(*ctx).otp_ptr };
        otp.links.insert(target_pid);

        ErlTerm::atom(crate::atom_table::ATOM_TRUE).0
    })
}

/// `unlink(Pid)` — remove the bidirectional link.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_unlink(ctx: *mut ProcessContext, pid_term: u64) -> u64 {
    let target_pid = match ErlTerm(pid_term).as_pid_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };
    let self_pid = unsafe { (*ctx).pid };

    // Remove from self
    let otp = unsafe { &mut *(*ctx).otp_ptr };
    otp.links.remove(&target_pid);

    // Remove from target
    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        if let Some(sched) = scheduler {
            let proc_arc = sched.process_table.get(&target_pid).map(|r| r.clone());
            if let Some(arc) = proc_arc {
                let mut proc = arc.lock_for_runtime();
                proc.otp.links.remove(&self_pid);
            }
        }

        ErlTerm::atom(crate::atom_table::ATOM_TRUE).0
    })
}

/// `monitor(process, Pid)` — create a unidirectional monitor. Returns a reference.
/// If the target doesn't exist, immediately delivers `{'DOWN', Ref, process, Pid, noproc}`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_monitor(ctx: *mut ProcessContext, _type_term: u64, pid_term: u64) -> u64 {
    use crate::term::{BoxedHeader, REFERENCE_ID_WORD_INDEX, REFERENCE_TOTAL_WORDS};

    let target_pid = match ErlTerm(pid_term).as_pid_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    // Create a unique reference ID for this monitor
    let mon_ref_id = NEXT_REF_ID.fetch_add(1, Ordering::Relaxed);
    let self_pid = unsafe { (*ctx).pid };

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return raise_badarg(ctx),
        };

        // Try to register in target
        let target_alive = {
            let proc_arc = sched.process_table.get(&target_pid).map(|r| r.clone());
            match proc_arc {
                Some(arc) => {
                    let mut proc = arc.lock_for_runtime();
                    if proc.status() == crate::process::ProcessStatus::Exited {
                        false
                    } else {
                        proc.otp.monitors_incoming.insert(mon_ref_id, self_pid);
                        true
                    }
                }
                None => false,
            }
        };

        // Register in self
        let otp = unsafe { &mut *(*ctx).otp_ptr };
        otp.monitors_outgoing.insert(mon_ref_id, target_pid);

        // Allocate the reference term on caller's heap
        let ref_term = with_ctx_heap(ctx, |heap| {
            let ptr = heap.alloc_words(REFERENCE_TOTAL_WORDS);
            unsafe {
                *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Reference, 0).0;
                *ptr.add(REFERENCE_ID_WORD_INDEX) = mon_ref_id;
            }
            ErlTerm::boxed_unchecked(ptr as *const u64)
        });

        if !target_alive {
            // Target already dead: deliver {'DOWN', Ref, process, Pid, noproc} immediately
            let down_atom = ErlTerm::atom(crate::atom_table::ATOM_DOWN);
            let process_atom = ErlTerm::atom(crate::atom_table::ATOM_PROCESS);
            let pid_term = ErlTerm::pid(target_pid);
            let noproc_atom = ErlTerm::atom(crate::atom_table::ATOM_NOPROC);
            let msg = with_ctx_heap(ctx, |heap| {
                heap.alloc_tuple(&[down_atom, ref_term, process_atom, pid_term, noproc_atom])
            });
            let mailbox = unsafe { &mut *(*ctx).mailbox_ptr };
            mailbox.send(msg);
        }

        ref_term.0
    })
}

/// `demonitor(Ref, Options)` — remove a monitor with options.
///
/// Options like `flush` and `info` are accepted but currently ignored:
/// the underlying monitor is removed regardless. Flushing pending DOWN
/// messages from the mailbox is a TODO — poolboy and most callers use
/// `[flush]` defensively against the case where the demonitored process
/// died between the monitor and demonitor calls. When that race doesn't
/// happen (the common path), flush is a no-op anyway.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_demonitor_2(
    ctx: *mut ProcessContext,
    ref_term: u64,
    _options: u64,
) -> u64 {
    unsafe { nera_rt_demonitor(ctx, ref_term) }
}

/// `demonitor(Ref)` — remove a monitor.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_demonitor(ctx: *mut ProcessContext, ref_term: u64) -> u64 {
    use crate::term::{BoxedHeader, BOXED_REFERENCE, REFERENCE_ID_WORD_INDEX};

    let term = ErlTerm(ref_term);
    let mon_ref_id = match term.as_boxed_ptr() {
        Some(ptr) => {
            let header = unsafe { BoxedHeader(*ptr) };
            if header.subtype() != BOXED_REFERENCE {
                return raise_badarg(ctx);
            }
            unsafe { *ptr.add(REFERENCE_ID_WORD_INDEX) }
        }
        None => return raise_badarg(ctx),
    };

    // Remove from self's outgoing monitors
    let otp = unsafe { &mut *(*ctx).otp_ptr };
    let target_pid = otp.monitors_outgoing.remove(&mon_ref_id);

    // Remove from target's incoming monitors
    if let Some(target_pid) = target_pid {
        with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
            if let Some(sched) = scheduler {
                let proc_arc = sched.process_table.get(&target_pid).map(|r| r.clone());
                if let Some(arc) = proc_arc {
                    let mut proc = arc.lock_for_runtime();
                    proc.otp.monitors_incoming.remove(&mon_ref_id);
                }
            }
        });
    }

    ErlTerm::atom(crate::atom_table::ATOM_TRUE).0
}

/// `process_flag(Flag, Value)` — set a process flag.
///
/// Supported flags: `trap_exit`, `fullsweep_after`, `max_heap_size`,
/// and `priority` (`normal`/`low`, using weighted reduction quanta).
#[no_mangle]
pub unsafe extern "C" fn nera_rt_process_flag(ctx: *mut ProcessContext, flag: u64, value: u64) -> u64 {
    let flag_term = ErlTerm(flag);
    let value_term = ErlTerm(value);

    let flag_atom = match flag_term.as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    if global_atom_table().resolve(flag_atom).as_deref() == Some("priority") {
        let value = value_term.as_atom_id().and_then(|id| global_atom_table().resolve(id));
        let low = match value.as_deref() {
            Some("normal") => false,
            Some("low") => true,
            _ => return raise_badarg(ctx),
        };
        if ctx.is_null() || unsafe { (*ctx).otp_ptr.is_null() } { return raise_badarg(ctx); }
        let otp = unsafe { &mut *(*ctx).otp_ptr };
        let previous = if otp.low_priority { "low" } else { "normal" };
        otp.low_priority = low;
        if low {
            unsafe { (*ctx).reductions = (*ctx).reductions.min(crate::process::REDUCTION_LIMIT / 8); }
        }
        return ErlTerm::atom(global_atom_table().intern(previous)).0;
    }

    if flag_atom == crate::atom_table::ATOM_TRAP_EXIT {
        let otp = unsafe { &mut *(*ctx).otp_ptr };
        let old_val = otp.trap_exit;
        let old_term = if old_val {
            ErlTerm::atom(crate::atom_table::ATOM_TRUE)
        } else {
            ErlTerm::atom(crate::atom_table::ATOM_FALSE)
        };
        otp.trap_exit = value_term.as_atom_id() == Some(crate::atom_table::ATOM_TRUE);
        return old_term.0;
    }

    if flag_atom == crate::atom_table::ATOM_FULLSWEEP_AFTER {
        let Some(n) = fullsweep_after_value(value_term) else {
            return raise_badarg(ctx);
        };
        let heap = unsafe { &mut *(*ctx).heap_ptr };
        let old = heap.set_fullsweep_after(n);
        return ErlTerm::small_int(old as i64).0;
    }

    if flag_atom == crate::atom_table::ATOM_MAX_HEAP_SIZE {
        let atom = |name| ErlTerm::atom(global_atom_table().intern(name));
        let yes = ErlTerm::atom(crate::atom_table::ATOM_TRUE);
        let no = ErlTerm::atom(crate::atom_table::ATOM_FALSE);
        let size_key = atom("size");
        let kill_key = atom("kill");
        let log_key = atom("error_logger");
        let shared_key = atom("include_shared_binaries");
        let size = if value_term.is_map() {
            let get = |key: ErlTerm, default: ErlTerm| unsafe {
                ErlTerm(super::map_ops::nera_rt_map_get_3(ctx, value, key.0, default.0))
            };
            // This runtime supports its existing kill-on-limit policy only.
            // Reject other policies rather than accepting options without effect.
            if get(kill_key, yes) != yes || get(log_key, yes) != yes
                || get(shared_key, no) != no {
                return raise_badarg(ctx);
            }
            get(size_key, ErlTerm::nil())
        } else {
            value_term
        };
        let words = match size.as_small_int() {
            Some(v) if v >= 0 => v as usize,
            _ => return raise_badarg(ctx),
        };
        let heap = unsafe { &mut *(*ctx).heap_ptr };
        let old = heap.set_max_heap_size(words);
        return heap.alloc_map(MapPairs::new(&[
            [size_key, ErlTerm::small_int(old as i64)], [kill_key, yes],
            [log_key, yes], [shared_key, no],
        ])).0;
    }

    raise_badarg(ctx)
}

/// `register(Name, Pid)` — register the process `Pid` under atom `Name`.
///
/// Raises `badarg` if:
/// - `Name` is not an atom
/// - `Pid` is not a pid or does not refer to a living process
/// - `Name` is already registered
/// - `Pid` is already registered under a different name
#[no_mangle]
pub unsafe extern "C" fn nera_rt_register(ctx: *mut ProcessContext, name: u64, pid_term: u64) -> u64 {
    use crate::atom_table::ATOM_TRUE;

    let name_atom = match ErlTerm(name).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };
    let target_pid = match ErlTerm(pid_term).as_pid_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return raise_badarg(ctx),
        };

        // Check that the name is not already registered and that the process
        // is not already registered under a different name.
        {
            let names = sched.registered_names.read();
            if names.contains_key(&name_atom) {
                return raise_badarg(ctx);
            }
            // Check if this pid is already registered under any name
            if names.values().any(|&p| p == target_pid) {
                return raise_badarg(ctx);
            }
        }

        // Verify the target process exists and is alive, then set its registered_name.
        // If the target is the calling process, use otp_ptr directly (the worker
        // thread already holds the process Mutex — locking again would deadlock).
        let self_pid = if !ctx.is_null() { unsafe { (*ctx).pid } } else { crate::process::ProcessId::new(0) };
        if target_pid == self_pid && !ctx.is_null() {
            let otp = unsafe { &mut *(*ctx).otp_ptr };
            otp.registered_name = Some(name_atom);
        } else {
            let proc_arc = sched.process_table.get(&target_pid).map(|r| r.clone());
            match proc_arc {
                Some(arc) => {
                    let mut proc = arc.lock_for_runtime();
                    if proc.status() == crate::process::ProcessStatus::Exited {
                        return raise_badarg(ctx);
                    }
                    proc.set_registered_name(name_atom);
                }
                None => return raise_badarg(ctx),
            }
        }

        // Insert into the global registry.
        sched.registered_names.write().insert(name_atom, target_pid);

        ErlTerm::atom(ATOM_TRUE).0
    })
}

/// `whereis(Name)` — look up the PID registered under atom `Name`.
/// Returns the PID or `undefined` if not registered.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_whereis(ctx: *mut ProcessContext, name: u64) -> u64 {
    use crate::atom_table::ATOM_UNDEFINED;

    let name_atom = match ErlTerm(name).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return ErlTerm::atom(ATOM_UNDEFINED).0,
        };

        let names = sched.registered_names.read();
        match names.get(&name_atom) {
            Some(&pid) => ErlTerm::pid(pid).0,
            None => ErlTerm::atom(ATOM_UNDEFINED).0,
        }
    })
}

/// `unregister(Name)` — remove the registration for atom `Name`.
/// Raises `badarg` if `Name` is not registered.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_unregister(ctx: *mut ProcessContext, name: u64) -> u64 {
    use crate::atom_table::ATOM_TRUE;

    let name_atom = match ErlTerm(name).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return raise_badarg(ctx),
        };

        // Remove from global registry.
        let removed_pid = sched.registered_names.write().remove(&name_atom);
        match removed_pid {
            Some(pid) => {
                // Clear the process's registered_name field.
                // If the target is the calling process, use otp_ptr directly
                // to avoid deadlock (worker already holds process Mutex).
                let self_pid = if !ctx.is_null() { unsafe { (*ctx).pid } } else { crate::process::ProcessId::new(0) };
                if pid == self_pid && !ctx.is_null() {
                    let otp = unsafe { &mut *(*ctx).otp_ptr };
                    otp.registered_name = None;
                } else {
                    if let Some(proc_arc) = sched.process_table.get(&pid).map(|r| r.clone()) {
                        let mut proc = proc_arc.lock_for_runtime();
                        proc.clear_registered_name();
                    }
                }
                ErlTerm::atom(ATOM_TRUE).0
            }
            None => raise_badarg(ctx),
        }
    })
}

/// `registered()` — return a list of all registered atom names.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_registered(ctx: *mut ProcessContext) -> u64 {

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return ErlTerm::nil().0,
        };

        let names = sched.registered_names.read();
        with_ctx_heap(ctx, |heap| {
            let mut list = ErlTerm::nil();
            for &name_atom in names.keys() {
                list = heap.alloc_cons(ErlTerm::atom(name_atom), list);
            }
            list.0
        })
    })
}

// ---------------------------------------------------------------------------
// process_info support
// ---------------------------------------------------------------------------

/// Snapshot of the introspectable state of a process at a single point in
/// time. For other-process queries every field is captured under one lock;
/// for self-queries the fields are read directly from the ProcessContext.
///
/// Remote messages and dictionary terms are copied under the process lock
/// into an owned heap, so releasing the lock cannot invalidate the snapshot.
/// Self-query terms already belong to the caller and need no copy.
struct ProcInfoSnapshot {
    // Keep scalar snapshots compact; allocate an owner only for boxed term graphs.
    source_heap: Option<Box<FragmentHeap>>,
    registered_name: Option<u32>,
    group_leader: crate::process::ProcessId,
    low_priority: bool,
    message_queue_len: usize,
    messages: Vec<ErlTerm>,
    links: Vec<crate::process::ProcessId>,
    monitors: Vec<crate::process::ProcessId>,
    monitored_by: Vec<crate::process::ProcessId>,
    trap_exit: bool,
    status: &'static str,
    reductions: i64,
    heap_bytes: usize,
    dictionary: Vec<(ErlTerm, ErlTerm)>,
    stacktrace: Vec<crate::process::StackFrame>,
}

impl ProcInfoSnapshot {
    #[inline]
    fn adopt_into(&mut self, heap: &mut ProcessHeap) {
        if let Some(source) = self.source_heap.take() {
            heap.adopt_fragment(*source);
        }
    }
}

/// Gather a full snapshot of the target process's state. Returns `None`
/// if the process is not alive / not known.
unsafe fn gather_process_info_snapshot(
    ctx: *mut ProcessContext,
    target_pid: crate::process::ProcessId,
    copy_messages: bool,
    copy_dictionary: bool,
) -> Option<ProcInfoSnapshot> {
    let self_pid = if !ctx.is_null() { Some(unsafe { (*ctx).pid }) } else { None };

    if Some(target_pid) == self_pid {
        // Self-query fast path: read from ctx pointers, no locks.
        let mailbox = unsafe { (*ctx).mailbox_ptr };
        let (msgs, mlen) = if !mailbox.is_null() {
            let mb = unsafe { &*mailbox };
            (mb.messages().collect::<Vec<_>>(), mb.len())
        } else {
            (Vec::new(), 0)
        };

        let (reg_name, links, monitors_out, monitored_by, trap_exit) = {
            let otp_ptr = unsafe { (*ctx).otp_ptr };
            if !otp_ptr.is_null() {
                let otp = unsafe { &*otp_ptr };
                (
                    otp.registered_name,
                    otp.links.iter().copied().collect::<Vec<_>>(),
                    otp.monitors_outgoing.values().copied().collect::<Vec<_>>(),
                    otp.monitors_incoming.values().copied().collect::<Vec<_>>(),
                    otp.trap_exit,
                )
            } else {
                (None, vec![], vec![], vec![], false)
            }
        };

        let dict = {
            let dp = unsafe { (*ctx).dict_ptr };
            if !dp.is_null() {
                unsafe { (*dp).clone() }
            } else {
                Vec::new()
            }
        };

        let stacktrace = {
            let base = unsafe { (*ctx).stack_base };
            let cursor = unsafe { (*ctx).stack_cursor };
            if !base.is_null() && cursor > base {
                let depth = (cursor as usize - base as usize) / std::mem::size_of::<crate::process::CompactFrame>();
                let frames = unsafe { std::slice::from_raw_parts(base, depth) };
                let info_table = crate::entry::global_func_info_table();
                frames.iter().map(|cf| {
                    let (module, function, arity) = if (cf.func_index as usize) < info_table.len() {
                        info_table[cf.func_index as usize]
                    } else {
                        (0, 0, 0)
                    };
                    crate::process::StackFrame { module, function, arity, line: cf.line }
                }).collect()
            } else {
                Vec::new()
            }
        };

        let heap_bytes = {
            let hp = unsafe { (*ctx).heap_ptr };
            if !hp.is_null() {
                unsafe { (*hp).bytes_allocated() }
            } else {
                0
            }
        };

        let reductions = unsafe { (*ctx).reductions };

        return Some(ProcInfoSnapshot {
            source_heap: None,
            low_priority: unsafe { (*ctx).otp_ptr.as_ref().is_some_and(|otp| otp.low_priority) },
            group_leader: with_sched_from_ctx(ctx, |s| s.and_then(|s| s.process_table.get(&target_pid).map(|p| p.group_leader())).unwrap_or(target_pid)),
            registered_name: reg_name,
            message_queue_len: mlen,
            messages: msgs,
            links,
            monitors: monitors_out,
            monitored_by,
            trap_exit,
            status: "running",
            reductions,
            heap_bytes,
            dictionary: dict,
            stacktrace,
        });
    }

    // Other-process path: look up via the scheduler and capture everything
    // under a single proc lock.
    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = scheduler?;
        let arc = sched.process_table.get(&target_pid).map(|r| r.clone())?;
        let proc = arc.lock_for_runtime();
        let mut snapshot = snapshot_remote_process(&proc, copy_messages, copy_dictionary);
        snapshot.group_leader = arc.group_leader();
        Some(snapshot)
    })
}

#[inline]
fn snapshot_remote_process(proc: &Process, copy_messages: bool, copy_dictionary: bool) -> ProcInfoSnapshot {
    let status = match proc.status() {
        crate::process::ProcessStatus::Ready => "running",
        crate::process::ProcessStatus::Running => "running",
        crate::process::ProcessStatus::Waiting => "waiting",
        crate::process::ProcessStatus::Exited => "exiting",
    };
    let mut source_heap = None;
    let mut copy = |term: ErlTerm| {
        if term.is_immediate() {
            return term;
        }
        let target = source_heap.get_or_insert_with(|| Box::new(FragmentHeap::new_with_capacity(64)));
        // SAFETY: the source process is locked through the whole snapshot copy.
        target.copy_term(unsafe { term.borrow(&proc.heap) })
    };
    let messages = if copy_messages {
        proc.mailbox.messages().map(&mut copy).collect()
    } else { Vec::new() };
    let dictionary = if copy_dictionary {
        proc.dictionary.iter().map(|&(key, value)| (copy(key), copy(value))).collect()
    } else { Vec::new() };
    ProcInfoSnapshot {
        source_heap,
        group_leader: proc.id,
        low_priority: proc.otp.low_priority,
        registered_name: proc.otp.registered_name,
        message_queue_len: proc.mailbox.len(),
        messages,
        links: proc.otp.links.iter().copied().collect(),
        monitors: proc.otp.monitors_outgoing.values().copied().collect(),
        monitored_by: proc.otp.monitors_incoming.values().copied().collect(),
        trap_exit: proc.otp.trap_exit,
        status,
        reductions: proc.reductions,
        heap_bytes: proc.heap.bytes_allocated(),
        dictionary,
        stacktrace: proc.exception_trace.clone(),
    }
}

/// Build the value side of `{Item, Value}` for one item. Returns `None`
/// if `item_name` is not a recognised process_info key.
fn build_process_info_value(
    heap: &mut ProcessHeap,
    item_name: &str,
    snap: &ProcInfoSnapshot,
    _target_pid: crate::process::ProcessId,
) -> Option<ErlTerm> {
    use crate::atom_table::{ATOM_FALSE, ATOM_NORMAL, ATOM_PROCESS, ATOM_TRUE};
    let atom_table = crate::atom_table::global_atom_table();

    Some(match item_name {
        "registered_name" => match snap.registered_name {
            Some(n) => ErlTerm::atom(n),
            // OTP returns [] when queried via /2 and no name is set.
            None => ErlTerm::nil(),
        },
        "message_queue_len" => ErlTerm::small_int(snap.message_queue_len as i64),
        "messages" => {
            let mut list = ErlTerm::nil();
            for &msg in snap.messages.iter().rev() {
                list = heap.alloc_cons(msg, list);
            }
            list
        }
        "links" => {
            let mut list = ErlTerm::nil();
            for &pid in snap.links.iter().rev() {
                list = heap.alloc_cons(ErlTerm::pid(pid), list);
            }
            list
        }
        "monitors" => {
            // OTP returns a list of `{process, Pid}` tuples.
            let mut list = ErlTerm::nil();
            for &pid in snap.monitors.iter().rev() {
                let t = heap.alloc_tuple(&[ErlTerm::atom(ATOM_PROCESS), ErlTerm::pid(pid)]);
                list = heap.alloc_cons(t, list);
            }
            list
        }
        "monitored_by" => {
            let mut list = ErlTerm::nil();
            for &pid in snap.monitored_by.iter().rev() {
                list = heap.alloc_cons(ErlTerm::pid(pid), list);
            }
            list
        }
        "trap_exit" => {
            if snap.trap_exit {
                ErlTerm::atom(ATOM_TRUE)
            } else {
                ErlTerm::atom(ATOM_FALSE)
            }
        }
        "status" => ErlTerm::atom(atom_table.intern(snap.status)),
        "reductions" => ErlTerm::small_int(snap.reductions),
        "heap_size" => ErlTerm::small_int((snap.heap_bytes / 8) as i64),
        "memory" => ErlTerm::small_int(snap.heap_bytes as i64),
        "dictionary" => {
            let mut list = ErlTerm::nil();
            for &(k, v) in snap.dictionary.iter().rev() {
                let t = heap.alloc_tuple(&[k, v]);
                list = heap.alloc_cons(t, list);
            }
            list
        }
        "current_stacktrace" => {
            let line_atom = ErlTerm::atom(atom_table.intern("line"));
            let mut list = ErlTerm::nil();
            for frame in snap.stacktrace.iter().rev() {
                let line_tuple = heap.alloc_tuple(&[line_atom, ErlTerm::small_int(frame.line as i64)]);
                let info_list = heap.alloc_cons(line_tuple, ErlTerm::nil());
                let frame_tuple = heap.alloc_tuple(&[
                    ErlTerm::atom(frame.module),
                    ErlTerm::atom(frame.function),
                    ErlTerm::small_int(frame.arity as i64),
                    info_list,
                ]);
                list = heap.alloc_cons(frame_tuple, list);
            }
            list
        }
        "group_leader" => ErlTerm::pid(snap.group_leader),
        "priority" => ErlTerm::atom(if snap.low_priority { atom_table.intern("low") } else { ATOM_NORMAL }),
        _ => return None,
    })
}

/// Select only requested term graphs; scalar introspection should not copy a mailbox.
unsafe fn snapshot_term_fields(mut items: ErlTerm) -> (bool, bool) {
    static FIELD_ATOMS: std::sync::OnceLock<(u32, u32)> = std::sync::OnceLock::new();
    let &(messages_id, dictionary_id) = FIELD_ATOMS.get_or_init(|| {
        let atoms = global_atom_table();
        (atoms.intern("messages"), atoms.intern("dictionary"))
    });
    let mut messages = false;
    let mut dictionary = false;
    loop {
        let item = if let Some(ptr) = items.as_cons_ptr() {
            // SAFETY: the FFI caller keeps the item list live and immutable.
            let cell = unsafe { &*ptr };
            items = cell.tail;
            cell.head
        } else {
            let item = items;
            items = ErlTerm::nil();
            item
        };
        if let Some(id) = item.as_atom_id() {
            messages |= id == messages_id;
            dictionary |= id == dictionary_id;
        }
        if items.is_nil() { return (messages, dictionary); }
    }
}

/// `process_info(Pid, Item)` or `process_info(Pid, [Item, ...])`.
///
/// - Returns `{Item, Value}` for a single item.
/// - Returns a list of `{Item, Value}` when called with a list of items.
/// - Returns `undefined` if the process is dead or unknown.
/// - Raises `badarg` if `Pid` is not a pid, or if any `Item` is not a
///   recognised atom.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_process_info(ctx: *mut ProcessContext, pid_term: u64, item: u64) -> u64 {
    use crate::atom_table::ATOM_UNDEFINED;

    let pid_erl = ErlTerm(pid_term);
    let item_erl = ErlTerm(item);

    let target_pid = match pid_erl.as_pid_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    let atom_table = crate::atom_table::global_atom_table();

    // Gather under the source lock, copying only requested term graphs.
    // Reuse the scalar name during result construction instead of resolving it twice.
    let scalar_name = item_erl.as_atom_id().and_then(|id| atom_table.resolve(id));
    let (copy_messages, copy_dictionary) = if let Some(name) = scalar_name.as_deref() {
        (name == "messages", name == "dictionary")
    } else {
        unsafe { snapshot_term_fields(item_erl) }
    };
    let mut snap = match unsafe { gather_process_info_snapshot(ctx, target_pid, copy_messages, copy_dictionary) } {
        Some(s) => s,
        None => return ErlTerm::atom(ATOM_UNDEFINED).0,
    };

    // Decide list-arg vs single-atom form.
    if item_erl.is_nil() {
        // process_info(Pid, []) → []
        return ErlTerm::nil().0;
    }

    if item_erl.is_cons() {
        // Walk the list, resolving atom names first so we can bail out with
        // badarg without allocating.
        let mut names: Vec<String> = Vec::new();
        let mut cur = item_erl;
        while cur.is_cons() {
            let cons_ptr = match cur.as_cons_ptr() {
                Some(p) => p,
                None => return raise_badarg(ctx),
            };
            let head = unsafe { (*cons_ptr).head };
            let tail = unsafe { (*cons_ptr).tail };
            let aid = match head.as_atom_id() {
                Some(id) => id,
                None => return raise_badarg(ctx),
            };
            let name = match atom_table.resolve(aid) {
                Some(s) => s,
                None => return raise_badarg(ctx),
            };
            names.push(name);
            cur = tail;
        }
        if !cur.is_nil() {
            return raise_badarg(ctx); // improper list
        }

        // Build the result list. Validate every item name against the known
        // set first; unknown item → badarg.
        return with_ctx_heap(ctx, |heap| {
            snap.adopt_into(heap);
            let mut tuples: Vec<ErlTerm> = Vec::with_capacity(names.len());
            for name in &names {
                let value = match build_process_info_value(heap, name, &snap, target_pid) {
                    Some(v) => v,
                    None => {
                        // Unwind via the same raise path.
                        return raise_badarg(ctx);
                    }
                };
                let key_atom = ErlTerm::atom(atom_table.intern(name));
                tuples.push(heap.alloc_tuple(&[key_atom, value]));
            }
            let mut list = ErlTerm::nil();
            for t in tuples.into_iter().rev() {
                list = heap.alloc_cons(t, list);
            }
            list.0
        });
    }

    // Single-atom form.
    let item_atom = match item_erl.as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };
    let name = match scalar_name {
        Some(s) => s,
        None => return raise_badarg(ctx),
    };

    with_ctx_heap(ctx, |heap| {
        snap.adopt_into(heap);
        match build_process_info_value(heap, &name, &snap, target_pid) {
            Some(value) => {
                let key_atom = ErlTerm::atom(item_atom);
                heap.alloc_tuple(&[key_atom, value]).0
            }
            None => raise_badarg(ctx),
        }
    })
}

/// `erlang:process_info/1` — return a list of all info tuples for a process.
///
/// Returns a list of `{Item, Value}` tuples covering the standard items.
/// `registered_name` is omitted from the list when the process has no name
/// (matching OTP).
///
/// Returns `undefined` if the process is dead or unknown.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_process_info_1(ctx: *mut ProcessContext, pid_term: u64) -> u64 {
    use crate::atom_table::ATOM_UNDEFINED;

    let pid_erl = ErlTerm(pid_term);
    let target_pid = match pid_erl.as_pid_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    let mut snap = match unsafe { gather_process_info_snapshot(ctx, target_pid, true, true) } {
        Some(s) => s,
        None => return ErlTerm::atom(ATOM_UNDEFINED).0,
    };

    // The set of items /1 reports. registered_name is filtered out when
    // the process has no name (OTP behaviour).
    let items_all: &[&str] = &[
        "registered_name",
        "message_queue_len",
        "messages",
        "links",
        "monitors",
        "monitored_by",
        "trap_exit",
        "status",
        "reductions",
        "heap_size",
        "memory",
        "dictionary",
        "current_stacktrace",
        "group_leader",
        "priority",
    ];
    let include_reg_name = snap.registered_name.is_some();
    let atom_table = crate::atom_table::global_atom_table();

    with_ctx_heap(ctx, |heap| {
        snap.adopt_into(heap);
        let mut tuples: Vec<ErlTerm> = Vec::with_capacity(items_all.len());
        for &name in items_all {
            if name == "registered_name" && !include_reg_name {
                continue;
            }
            let value = build_process_info_value(heap, name, &snap, target_pid)
                .expect("process_info/1 item names are all known to build_process_info_value");
            let key_atom = ErlTerm::atom(atom_table.intern(name));
            tuples.push(heap.alloc_tuple(&[key_atom, value]));
        }

        let mut list = ErlTerm::nil();
        for t in tuples.into_iter().rev() {
            list = heap.alloc_cons(t, list);
        }
        list.0
    })
}

/// `spawn_link(Fun)` — spawn a new process running the given closure and
/// link it to the current process atomically.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_spawn_link_closure(
    ctx: *mut ProcessContext,
    closure_term: u64,
) -> u64 {
    let term = ErlTerm(closure_term);

    if !term.is_boxed() {
        return raise_badarg(ctx);
    }

    let boxed_ptr = match term.as_boxed_ptr() {
        Some(p) => p,
        None => return raise_badarg(ctx),
    };
    let header = unsafe { BoxedHeader(*boxed_ptr) };

    if header.subtype() != BOXED_CLOSURE {
        return raise_badarg(ctx);
    }

    let env_count = header.arity() as usize;
    let fn_ptr = unsafe { *boxed_ptr.add(1) };

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return raise_badarg(ctx),
        };

        let self_pid = unsafe { (*ctx).pid };
        let Some(pid) = sched.try_next_pid() else {
            return super::helpers::raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_SYSTEM_LIMIT).to_raw());
        };
        let mut process = Process::new(pid);

        // Deep-copy captured env vars to the new process's heap
        let mut arg_terms = Vec::with_capacity(env_count);
        for i in 0..env_count {
            let env_val = ErlTerm(unsafe { *boxed_ptr.add(3 + i) });
            let copied = deep_copy_term(env_val, &mut process.heap);
            arg_terms.push(copied);
        }

        let func: crate::process::ErlFn = unsafe { std::mem::transmute(fn_ptr) };
        process.initial_fn = Some(func);
        process.initial_args = arg_terms;

        // Establish bidirectional link BEFORE injecting
        process.otp.links.insert(self_pid);
        let otp = unsafe { &mut *(*ctx).otp_ptr };
        otp.links.insert(pid);

        let arc = std::sync::Arc::new(crate::process::ProcessEntry::new(process));
        // Newly spawned processes inherit the caller's group leader.
        if !ctx.is_null() {
            if let Some(parent) = sched.process_table.get(&unsafe { (*ctx).pid }) {
                arc.set_group_leader(parent.group_leader());
            }
        }
        sched.process_table.insert(pid, std::sync::Arc::clone(&arc));
        sched.alive_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.runnable_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.dispatch_to_worker(arc);

        ErlTerm::pid(pid).0
    })
}

/// `spawn_link(Module, Function, Args)` — spawn a new process and link it
/// to the current process atomically.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_spawn_link(
    ctx: *mut ProcessContext,
    mod_atom: u64,
    fun_atom: u64,
    args: u64,
) -> u64 {
    let mod_id = match ErlTerm(mod_atom).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };
    let fun_id = match ErlTerm(fun_atom).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    // Count args from the Erlang list to determine arity
    let mut arity = 0u32;
    let mut current = ErlTerm(args);
    while current.is_cons() {
        arity += 1;
        if let Some(ptr) = current.as_cons_ptr() {
            current = unsafe { (*ptr).tail };
        } else {
            break;
        }
    }

    // Look up the function
    let func = match global_function_table().lookup(mod_id, fun_id, arity) {
        Some(f) => f,
        None => {
            eprintln!(
                "spawn_link: function not found: {}:{}/{}",
                global_atom_table().resolve(mod_id).unwrap_or_else(|| "?".to_string()),
                global_atom_table().resolve(fun_id).unwrap_or_else(|| "?".to_string()),
                arity
            );
            return raise_badarg(ctx);
        }
    };

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return raise_badarg(ctx),
        };

        let self_pid = unsafe { (*ctx).pid };

        // Collect args from the list
        let mut arg_terms = Vec::new();
        let mut current = ErlTerm(args);
        while current.is_cons() {
            if let Some(ptr) = current.as_cons_ptr() {
                unsafe {
                    arg_terms.push(ErlTerm((*ptr).head.0));
                    current = (*ptr).tail;
                }
            } else {
                break;
            }
        }

        let Some(pid) = sched.try_next_pid() else {
            return super::helpers::raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_SYSTEM_LIMIT).to_raw());
        };
        let mut process = Process::with_entry(pid, func, arg_terms);

        // Establish bidirectional link BEFORE injecting
        process.otp.links.insert(self_pid);
        let otp = unsafe { &mut *(*ctx).otp_ptr };
        otp.links.insert(pid);

        let arc = std::sync::Arc::new(crate::process::ProcessEntry::new(process));
        // Newly spawned processes inherit the caller's group leader.
        if !ctx.is_null() {
            if let Some(parent) = sched.process_table.get(&unsafe { (*ctx).pid }) {
                arc.set_group_leader(parent.group_leader());
            }
        }
        sched.process_table.insert(pid, std::sync::Arc::clone(&arc));
        sched.alive_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.runnable_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.dispatch_to_worker(arc);

        ErlTerm::pid(pid).0
    })
}

/// `spawn_monitor(Fun)` — spawn a new process from a closure and establish
/// a monitor from the caller, atomically. Returns `{Pid, Ref}`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_spawn_monitor_closure(
    ctx: *mut ProcessContext,
    closure_term: u64,
) -> u64 {
    use crate::term::{BoxedHeader, BOXED_CLOSURE, REFERENCE_ID_WORD_INDEX, REFERENCE_TOTAL_WORDS};

    let term = ErlTerm(closure_term);

    if !term.is_boxed() {
        return raise_badarg(ctx);
    }

    let boxed_ptr = match term.as_boxed_ptr() {
        Some(p) => p,
        None => return raise_badarg(ctx),
    };
    let header = unsafe { BoxedHeader(*boxed_ptr) };

    if header.subtype() != BOXED_CLOSURE {
        return raise_badarg(ctx);
    }

    let env_count = header.arity() as usize;
    let fn_ptr = unsafe { *boxed_ptr.add(1) };

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return raise_badarg(ctx),
        };

        let self_pid = unsafe { (*ctx).pid };
        let Some(pid) = sched.try_next_pid() else {
            return super::helpers::raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_SYSTEM_LIMIT).to_raw());
        };
        let mut process = Process::new(pid);

        // Deep-copy captured env vars to the new process's heap
        let mut arg_terms = Vec::with_capacity(env_count);
        for i in 0..env_count {
            let env_val = ErlTerm(unsafe { *boxed_ptr.add(3 + i) });
            let copied = deep_copy_term(env_val, &mut process.heap);
            arg_terms.push(copied);
        }

        let func: crate::process::ErlFn = unsafe { std::mem::transmute(fn_ptr) };
        process.initial_fn = Some(func);
        process.initial_args = arg_terms;

        // Create monitor ref
        let mon_ref_id = NEXT_REF_ID.fetch_add(1, Ordering::Relaxed);

        // Register monitor: caller monitors the new process
        process.otp.monitors_incoming.insert(mon_ref_id, self_pid);
        let otp = unsafe { &mut *(*ctx).otp_ptr };
        otp.monitors_outgoing.insert(mon_ref_id, pid);

        let arc = std::sync::Arc::new(crate::process::ProcessEntry::new(process));
        // Newly spawned processes inherit the caller's group leader.
        if !ctx.is_null() {
            if let Some(parent) = sched.process_table.get(&unsafe { (*ctx).pid }) {
                arc.set_group_leader(parent.group_leader());
            }
        }
        sched.process_table.insert(pid, std::sync::Arc::clone(&arc));
        sched.alive_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.runnable_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.dispatch_to_worker(arc);

        // Return {Pid, Ref} tuple on caller's heap
        let pid_term = ErlTerm::pid(pid);
        let ref_term = with_ctx_heap(ctx, |heap| {
            let ptr = heap.alloc_words(REFERENCE_TOTAL_WORDS);
            unsafe {
                *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Reference, 0).0;
                *ptr.add(REFERENCE_ID_WORD_INDEX) = mon_ref_id;
            }
            ErlTerm::boxed_unchecked(ptr as *const u64)
        });

        with_ctx_heap(ctx, |heap| {
            heap.alloc_tuple(&[pid_term, ref_term]).0
        })
    })
}

/// `spawn_monitor(Module, Function, Args)` — spawn a new process and
/// establish a monitor from the caller, atomically. Returns `{Pid, Ref}`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_spawn_monitor(
    ctx: *mut ProcessContext,
    mod_atom: u64,
    fun_atom: u64,
    args: u64,
) -> u64 {
    use crate::term::{BoxedHeader, REFERENCE_ID_WORD_INDEX, REFERENCE_TOTAL_WORDS};
    let mod_id = match ErlTerm(mod_atom).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };
    let fun_id = match ErlTerm(fun_atom).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    // Count args from the Erlang list to determine arity
    let mut arity = 0u32;
    let mut current = ErlTerm(args);
    while current.is_cons() {
        arity += 1;
        if let Some(ptr) = current.as_cons_ptr() {
            current = unsafe { (*ptr).tail };
        } else {
            break;
        }
    }

    // Look up the function
    let func = match global_function_table().lookup(mod_id, fun_id, arity) {
        Some(f) => f,
        None => {
            eprintln!(
                "spawn_monitor: function not found: {}:{}/{}",
                global_atom_table().resolve(mod_id).unwrap_or_else(|| "?".to_string()),
                global_atom_table().resolve(fun_id).unwrap_or_else(|| "?".to_string()),
                arity
            );
            return raise_badarg(ctx);
        }
    };

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return raise_badarg(ctx),
        };

        let self_pid = unsafe { (*ctx).pid };

        // Collect args from the list
        let mut arg_terms = Vec::new();
        let mut current = ErlTerm(args);
        while current.is_cons() {
            if let Some(ptr) = current.as_cons_ptr() {
                unsafe {
                    arg_terms.push(ErlTerm((*ptr).head.0));
                    current = (*ptr).tail;
                }
            } else {
                break;
            }
        }

        let Some(pid) = sched.try_next_pid() else {
            return super::helpers::raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_SYSTEM_LIMIT).to_raw());
        };
        let mut process = Process::with_entry(pid, func, arg_terms);

        // Create monitor ref
        let mon_ref_id = NEXT_REF_ID.fetch_add(1, Ordering::Relaxed);

        // Register monitor: caller monitors the new process
        process.otp.monitors_incoming.insert(mon_ref_id, self_pid);
        let otp = unsafe { &mut *(*ctx).otp_ptr };
        otp.monitors_outgoing.insert(mon_ref_id, pid);

        let arc = std::sync::Arc::new(crate::process::ProcessEntry::new(process));
        // Newly spawned processes inherit the caller's group leader.
        if !ctx.is_null() {
            if let Some(parent) = sched.process_table.get(&unsafe { (*ctx).pid }) {
                arc.set_group_leader(parent.group_leader());
            }
        }
        sched.process_table.insert(pid, std::sync::Arc::clone(&arc));
        sched.alive_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.runnable_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.dispatch_to_worker(arc);

        // Return {Pid, Ref} tuple on caller's heap
        let pid_term = ErlTerm::pid(pid);
        let ref_term = with_ctx_heap(ctx, |heap| {
            let ptr = heap.alloc_words(REFERENCE_TOTAL_WORDS);
            unsafe {
                *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Reference, 0).0;
                *ptr.add(REFERENCE_ID_WORD_INDEX) = mon_ref_id;
            }
            ErlTerm::boxed_unchecked(ptr as *const u64)
        });

        with_ctx_heap(ctx, |heap| {
            heap.alloc_tuple(&[pid_term, ref_term]).0
        })
    })
}

/// erlang:is_process_alive/1 — check if a process is alive.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_is_process_alive(ctx: *mut ProcessContext, term: u64) -> u64 {
    let pid_id = match ErlTerm(term).as_pid_id() {
        Some(id) => id,
        None => return ErlTerm::atom(crate::atom_table::ATOM_FALSE).0,
    };
    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let alive = match scheduler {
            Some(sched) => sched.process_table.contains_key(&pid_id),
            None => false,
        };
        if alive {
            ErlTerm::atom(crate::atom_table::ATOM_TRUE).0
        } else {
            ErlTerm::atom(crate::atom_table::ATOM_FALSE).0
        }
    })
}

/// erlang:exit/2 — send an exit signal to another process.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_exit_2(ctx: *mut ProcessContext, pid_term: u64, reason: u64) -> u64 {
    let target_pid = match ErlTerm(pid_term).as_pid_id() {
        Some(id) => id,
        None => return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_BADARG).0),
    };
    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        if let Some(sched) = scheduler {
            let entry = sched.process_table.get(&target_pid).map(|r| r.clone());
            if let Some(entry) = entry {
                // Send EXIT signal via inbox (preserves FIFO ordering)
                let self_pid = if !ctx.is_null() { unsafe { (*ctx).pid } } else { crate::process::ProcessId::new(0) };
                let exit_msg = with_ctx_heap(ctx, |heap| {
                    let exit_atom = ErlTerm::atom(crate::atom_table::ATOM_EXIT_SIGNAL);
                    let from_pid = ErlTerm::pid(self_pid);
                    heap.alloc_tuple(&[exit_atom, from_pid, ErlTerm(reason)])
                });
                let fragment = crate::message::MessageFragment::from_term(exit_msg);
                entry.inbox.push(fragment);
                entry.has_exit_signals.store(true, std::sync::atomic::Ordering::Release);
                // Try waking the process. try_wake() handles receive-waiting
                // (ASTATUS_WAITING), try_wake_io() handles IO-waiting
                // (ASTATUS_IO_WAIT). Only one CAS can succeed.
                // The worker cancels the receive timer; see nera_rt_send_message.
                // An ignored signal returns the process to its wait, which
                // keeps its timer.
                if entry.try_wake() || entry.try_wake_io() {
                    sched.runnable_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                    sched.dispatch_to_worker(entry);
                }
            }
        }
        ErlTerm::atom(crate::atom_table::ATOM_TRUE).0
    })
}

/// `erlang:hibernate(Module, Function, Args)` — shrink process to zero heap
/// and suspend until a message arrives, then call `M:F(A)`.
///
/// This is a BIF that never returns to the caller. It sets
/// `STATUS_HIBERNATE` which the scheduler handles specially.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_hibernate(
    ctx: *mut ProcessContext,
    mod_atom: u64,
    fun_atom: u64,
    args: u64,
) -> u64 {
    let mod_id = match ErlTerm(mod_atom).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };
    let fun_id = match ErlTerm(fun_atom).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    // Collect args from the Erlang list
    let mut arg_terms = Vec::new();
    let mut current = ErlTerm(args);
    while current.is_cons() {
        if let Some(ptr) = current.as_cons_ptr() {
            unsafe {
                arg_terms.push(ErlTerm((*ptr).head.0));
                current = (*ptr).tail;
            }
        } else {
            break;
        }
    }

    // Store MFA in thread-local for the scheduler to pick up.
    // This is safe because we're in the worker thread and the scheduler
    // reads it immediately after run_slice returns.
    HIBERNATE_MFA.with(|cell| {
        *cell.borrow_mut() = Some((mod_id, fun_id, arg_terms));
    });

    unsafe {
        (*ctx).status = crate::process::STATUS_HIBERNATE;
    }

    // Return value doesn't matter — hibernate never returns to caller.
    ErlTerm::nil().0
}

thread_local! {
    /// Thread-local storage for hibernate MFA, set by `nera_rt_hibernate`
    /// and consumed by the scheduler in the same worker thread.
    pub static HIBERNATE_MFA: RefCell<Option<(u32, u32, Vec<ErlTerm>)>> = const { RefCell::new(None) };
}

/// `spawn_opt(Fun, Opts)` — spawn a process from a closure with options.
/// Options: `link`, `monitor`, `{fullsweep_after, N}`, `{max_heap_size, N}`, `{min_heap_size, N}`
/// Returns `{Pid, Ref}` if `monitor` is in opts, else `Pid`.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_spawn_opt_closure(
    ctx: *mut ProcessContext,
    closure_term: u64,
    opts_term: u64,
) -> u64 {
    let term = ErlTerm(closure_term);

    if !term.is_boxed() {
        return raise_badarg(ctx);
    }

    let boxed_ptr = match term.as_boxed_ptr() {
        Some(p) => p,
        None => return raise_badarg(ctx),
    };
    let header = unsafe { BoxedHeader(*boxed_ptr) };

    if header.subtype() != BOXED_CLOSURE {
        return raise_badarg(ctx);
    }

    let env_count = header.arity() as usize;
    let fn_ptr = unsafe { *boxed_ptr.add(1) };

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return raise_badarg(ctx),
        };

        let self_pid = unsafe { (*ctx).pid };

        // Parse options early so min_heap_size is available for process creation
        let Ok(opts) = parse_spawn_opts(ErlTerm(opts_term)) else {
            return raise_badarg(ctx);
        };

        let Some(pid) = sched.try_next_pid() else {
            return super::helpers::raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_SYSTEM_LIMIT).to_raw());
        };
        let mut process = if let Some(min_words) = opts.min_heap_size {
            Process::new_with_heap_size(pid, min_words)
        } else {
            Process::new(pid)
        };

        // Deep-copy captured env vars to the new process's heap
        let mut arg_terms = Vec::with_capacity(env_count);
        for i in 0..env_count {
            let env_val = ErlTerm(unsafe { *boxed_ptr.add(3 + i) });
            let copied = deep_copy_term(env_val, &mut process.heap);
            arg_terms.push(copied);
        }

        let func: crate::process::ErlFn = unsafe { std::mem::transmute(fn_ptr) };
        process.initial_fn = Some(func);
        process.initial_args = arg_terms;

        // Apply heap options
        if let Some(n) = opts.fullsweep_after {
            process.heap.set_fullsweep_after(n);
        }
        if let Some(n) = opts.max_heap_size {
            process.heap.set_max_heap_size(n);
        }

        // Link
        if opts.link {
            process.otp.links.insert(self_pid);
            let otp = unsafe { &mut *(*ctx).otp_ptr };
            otp.links.insert(pid);
        }

        // Monitor
        let mon_ref_id = if opts.monitor {
            let ref_id = NEXT_REF_ID.fetch_add(1, Ordering::Relaxed);
            process.otp.monitors_incoming.insert(ref_id, self_pid);
            let otp = unsafe { &mut *(*ctx).otp_ptr };
            otp.monitors_outgoing.insert(ref_id, pid);
            Some(ref_id)
        } else {
            None
        };

        let arc = std::sync::Arc::new(crate::process::ProcessEntry::new(process));
        // Newly spawned processes inherit the caller's group leader.
        if !ctx.is_null() {
            if let Some(parent) = sched.process_table.get(&unsafe { (*ctx).pid }) {
                arc.set_group_leader(parent.group_leader());
            }
        }
        sched.process_table.insert(pid, std::sync::Arc::clone(&arc));
        sched.alive_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.runnable_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.dispatch_to_worker(arc);

        let pid_term = ErlTerm::pid(pid);

        if let Some(ref_id) = mon_ref_id {
            use crate::term::{REFERENCE_ID_WORD_INDEX, REFERENCE_TOTAL_WORDS};
            let ref_term = with_ctx_heap(ctx, |heap| {
                let ptr = heap.alloc_words(REFERENCE_TOTAL_WORDS);
                unsafe {
                    *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Reference, 0).0;
                    *ptr.add(REFERENCE_ID_WORD_INDEX) = ref_id;
                }
                ErlTerm::boxed_unchecked(ptr as *const u64)
            });
            with_ctx_heap(ctx, |heap| {
                heap.alloc_tuple(&[pid_term, ref_term]).0
            })
        } else {
            pid_term.0
        }
    })
}

/// `spawn_opt(M, F, A, Opts)` — spawn a process with options.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_spawn_opt(
    ctx: *mut ProcessContext,
    mod_atom: u64,
    fun_atom: u64,
    args: u64,
    opts_term: u64,
) -> u64 {
    let mod_id = match ErlTerm(mod_atom).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };
    let fun_id = match ErlTerm(fun_atom).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    // Count args
    let mut arity = 0u32;
    let mut current = ErlTerm(args);
    while current.is_cons() {
        arity += 1;
        if let Some(ptr) = current.as_cons_ptr() {
            current = unsafe { (*ptr).tail };
        } else {
            break;
        }
    }

    // Look up the function
    let func = match global_function_table().lookup(mod_id, fun_id, arity) {
        Some(f) => f,
        None => {
            eprintln!(
                "spawn_opt: function not found: {}:{}/{}",
                global_atom_table().resolve(mod_id).unwrap_or_else(|| "?".to_string()),
                global_atom_table().resolve(fun_id).unwrap_or_else(|| "?".to_string()),
                arity
            );
            return raise_badarg(ctx);
        }
    };

    with_sched_from_ctx(ctx, #[inline(always)] |scheduler| {
        let sched = match scheduler {
            Some(s) => s,
            None => return raise_badarg(ctx),
        };

        let self_pid = unsafe { (*ctx).pid };

        // Collect args from the list
        let mut arg_terms = Vec::new();
        let mut current = ErlTerm(args);
        while current.is_cons() {
            if let Some(ptr) = current.as_cons_ptr() {
                unsafe {
                    arg_terms.push(ErlTerm((*ptr).head.0));
                    current = (*ptr).tail;
                }
            } else {
                break;
            }
        }

        // Parse options early so min_heap_size is available for process creation
        let Ok(opts) = parse_spawn_opts(ErlTerm(opts_term)) else {
            return raise_badarg(ctx);
        };

        let Some(pid) = sched.try_next_pid() else {
            return super::helpers::raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_SYSTEM_LIMIT).to_raw());
        };
        let mut process = Process::with_entry(pid, func, arg_terms);
        if let Some(min_words) = opts.min_heap_size {
            process.heap = ProcessHeap::new_with_capacity(min_words);
        }

        // Apply heap options
        if let Some(n) = opts.fullsweep_after {
            process.heap.set_fullsweep_after(n);
        }
        if let Some(n) = opts.max_heap_size {
            process.heap.set_max_heap_size(n);
        }

        // Link
        if opts.link {
            process.otp.links.insert(self_pid);
            let otp = unsafe { &mut *(*ctx).otp_ptr };
            otp.links.insert(pid);
        }

        // Monitor
        let mon_ref_id = if opts.monitor {
            let ref_id = NEXT_REF_ID.fetch_add(1, Ordering::Relaxed);
            process.otp.monitors_incoming.insert(ref_id, self_pid);
            let otp = unsafe { &mut *(*ctx).otp_ptr };
            otp.monitors_outgoing.insert(ref_id, pid);
            Some(ref_id)
        } else {
            None
        };

        let arc = std::sync::Arc::new(crate::process::ProcessEntry::new(process));
        // Newly spawned processes inherit the caller's group leader.
        if !ctx.is_null() {
            if let Some(parent) = sched.process_table.get(&unsafe { (*ctx).pid }) {
                arc.set_group_leader(parent.group_leader());
            }
        }
        sched.process_table.insert(pid, std::sync::Arc::clone(&arc));
        sched.alive_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.runnable_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        sched.dispatch_to_worker(arc);

        let pid_term = ErlTerm::pid(pid);

        if let Some(ref_id) = mon_ref_id {
            use crate::term::{REFERENCE_ID_WORD_INDEX, REFERENCE_TOTAL_WORDS};
            let ref_term = with_ctx_heap(ctx, |heap| {
                let ptr = heap.alloc_words(REFERENCE_TOTAL_WORDS);
                unsafe {
                    *ptr = BoxedHeader::new(crate::term::BoxedSubtype::Reference, 0).0;
                    *ptr.add(REFERENCE_ID_WORD_INDEX) = ref_id;
                }
                ErlTerm::boxed_unchecked(ptr as *const u64)
            });
            with_ctx_heap(ctx, |heap| {
                heap.alloc_tuple(&[pid_term, ref_term]).0
            })
        } else {
            pid_term.0
        }
    })
}

/// Parsed spawn options. Construction succeeds only for a proper options list.
#[derive(Default, Debug)]
struct SpawnOpts {
    link: bool,
    monitor: bool,
    fullsweep_after: Option<u32>,
    max_heap_size: Option<usize>,
    min_heap_size: Option<usize>,
}

#[derive(Debug)]
struct InvalidSpawnOptions;

#[inline]
fn fullsweep_after_value(term: ErlTerm) -> Option<u32> {
    u32::try_from(term.as_small_int()?).ok()
}

/// Parse a proper list and check numeric domains before creating a process.
///
/// # Safety
/// The complete options graph must remain live, initialized and unmoved by GC
/// for this call. No references into it are retained in the parsed result.
unsafe fn parse_spawn_opts(opts: ErlTerm) -> Result<SpawnOpts, InvalidSpawnOptions> {
    let mut result = SpawnOpts::default();
    let mut current = opts;
    while current.is_cons() {
        let ptr = current.as_cons_ptr().ok_or(InvalidSpawnOptions)?;
        let head = (*ptr).head;
        current = (*ptr).tail;

        if let Some(atom_id) = head.as_atom_id() {
            if atom_id == crate::atom_table::ATOM_LINK {
                result.link = true;
                continue;
            }
            if atom_id == crate::atom_table::ATOM_MONITOR_OPT {
                result.monitor = true;
                continue;
            }
            return Err(InvalidSpawnOptions);
        }

        let bptr = head.as_boxed_ptr().ok_or(InvalidSpawnOptions)?;
        let hdr = BoxedHeader(*bptr);
        if hdr.subtype() != BOXED_TUPLE || hdr.arity() != 2 {
            return Err(InvalidSpawnOptions);
        }
        let key = ErlTerm(*bptr.add(1)).as_atom_id().ok_or(InvalidSpawnOptions)?;
        let value = ErlTerm(*bptr.add(2));
        if key == crate::atom_table::ATOM_FULLSWEEP_AFTER {
            result.fullsweep_after = Some(fullsweep_after_value(value).ok_or(InvalidSpawnOptions)?);
        } else {
            let words = usize::try_from(value.as_small_int().ok_or(InvalidSpawnOptions)?)
                .map_err(|_| InvalidSpawnOptions)?;
            if key == crate::atom_table::ATOM_MAX_HEAP_SIZE {
                result.max_heap_size = Some(words);
            } else if key == crate::atom_table::ATOM_MIN_HEAP_SIZE {
                result.min_heap_size = Some(words);
            } else {
                return Err(InvalidSpawnOptions);
            }
        }
    }
    if !current.is_nil() {
        return Err(InvalidSpawnOptions);
    }
    Ok(result)
}


#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn spawn_options_require_proper_lists_and_checked_domains() {
        let mut heap = ProcessHeap::new();
        let key = ErlTerm::atom(crate::atom_table::ATOM_FULLSWEEP_AFTER);
        let maximum = heap.alloc_tuple(&[key, ErlTerm::small_int(u32::MAX as i64)]);
        let overflow = heap.alloc_tuple(&[key, ErlTerm::small_int(u32::MAX as i64 + 1)]);
        let valid = heap.alloc_cons(maximum, ErlTerm::nil());
        let invalid = heap.alloc_cons(overflow, ErlTerm::nil());
        let improper = heap.alloc_cons(maximum, ErlTerm::small_int(7));
        // SAFETY: all reachable terms are initialized and remain live in heap.
        unsafe {
            assert_eq!(parse_spawn_opts(valid).unwrap().fullsweep_after, Some(u32::MAX));
            assert!(parse_spawn_opts(invalid).is_err());
            assert!(parse_spawn_opts(improper).is_err());
            assert!(parse_spawn_opts(key).is_err());
            assert!(parse_spawn_opts(ErlTerm::nil()).is_ok());
        }
    }

    #[test]
    fn remote_snapshot_survives_source_collection_and_drop() {
        let mut source = Process::new(crate::process::ProcessId::new(101));
        let term = source.heap.alloc_binary(b"snapshot payload");
        source.mailbox.send(term);
        source.dictionary.push((ErlTerm::atom(0), term));
        let mut snapshot = snapshot_remote_process(&source, true, true);
        assert_ne!(snapshot.messages[0], term);
        unsafe { source.heap.collect_full(&mut []) };
        drop(source);
        let mut target = ProcessHeap::new();
        snapshot.adopt_into(&mut target);
        let message = snapshot.messages[0];
        let value = snapshot.dictionary[0].1;
        drop(snapshot);
        // SAFETY: adoption moved the independent snapshot storage to target.
        for term in [message, value] {
            assert_eq!(unsafe { term.borrow(&target) }.binary_bytes(), Some(&b"snapshot payload"[..]));
        }
    }
}

#[cfg(test)]
mod delivery_timer_tests {
    use super::*;
    use crate::process::{ProcessId, ProcessState, WaitReason, ASTATUS_READY};
    use crate::scheduler::SchedulerHandle;
    use std::sync::atomic::{AtomicI64, AtomicU32};

    // Cowlib's cow_cookie EUnit suite hung natively in a few runs out of a
    // hundred. A sender cancelled the receiver's receive timer after queueing
    // the receiver. By then the receiver could have run into a later
    // `receive ... after`, whose timer the cancel removed, so it never timed out.
    #[test]
    fn delivery_leaves_the_receive_timer_to_the_worker() {
        #[cfg(mio_supported)]
        let (reactor, reactor_thread) = crate::reactor::spawn_reactor();
        let owner = Arc::new(SchedulerHandle {
            injector: Arc::new(crossbeam_deque::Injector::new()),
            process_table: Arc::new(dashmap::DashMap::new()),
            next_pid: Arc::new(crate::process::ProcessIdAllocator::starting_at(2)),
            runnable_count: Arc::new(AtomicI64::new(0)),
            alive_count: Arc::new(AtomicI64::new(1)),
            registered_names: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
            timer_wheel: Arc::new(crate::timer::TimerWheel::new()),
            worker_notify: Arc::new((parking_lot::Mutex::new(()), parking_lot::Condvar::new())),
            io_pool: Arc::new(crate::io_pool::IoPool::new(1)),
            #[cfg(mio_supported)]
            reactor: reactor.clone(),
            worker_inboxes: Arc::new(Vec::new()),
            num_workers: 0,
            worker_threads: Arc::new(Vec::new()),
            dispatch_rr: AtomicU32::new(0),
        });
        // A receiver waiting in `receive ... after`, with its timer registered.
        let receiver = ProcessId::new(4_242);
        let entry = Arc::new(ProcessEntry::new(Process::new(receiver)));
        entry.lock_for_runtime().transition(ProcessState::Waiting(WaitReason::Receive {
            timeout_deadline_ms: Some(10),
        }));
        owner.process_table.insert(receiver, Arc::clone(&entry));
        owner.timer_wheel.insert(10, receiver);

        let mut sender = ProcessContext::new(ProcessId::new(1));
        sender.runtime = Arc::as_ptr(&owner);
        let message = ErlTerm::atom(crate::atom_table::ATOM_OK).0;
        // SAFETY: the sender context and its scheduler outlive the call; the
        // message is an immediate, so no sender heap is read.
        unsafe { nera_rt_send_message(&mut sender, ErlTerm::pid(receiver).0, message) };

        assert_eq!(entry.published_status(), ASTATUS_READY);
        assert_eq!(owner.runnable_count.load(Ordering::Acquire), 1);
        assert!(matches!(owner.injector.steal(), crossbeam_deque::Steal::Success(_)));
        assert!(owner.timer_wheel.has_pending_for(receiver),
            "the worker that runs the receiver cancels its timer, not the sender");
        #[cfg(mio_supported)]
        {
            reactor.shutdown();
            reactor_thread.join().unwrap();
        }
    }
}

#[cfg(test)]
mod delivery_order_tests {
    use super::*;
    use crate::process::{ProcessId, ProcessState, WaitReason};
    use crate::scheduler::SchedulerHandle;
    use std::sync::atomic::{AtomicI64, AtomicU32};

    // The receive_nil_message end-to-end test sometimes got [[], [], e]
    // instead of [[], e, []] on macOS arm64. A sender's first message went to
    // the inbox of a running receiver. The receiver's worker then published
    // its receive wait and released the lock, so the sender's second message
    // took the direct path into the mailbox. The first one left the inbox
    // only on the receiver's next run, behind the second.
    fn received_after_two_sends(send: impl Fn(&mut ProcessContext, ProcessId, i64)) -> Vec<i64> {
        #[cfg(mio_supported)]
        let (reactor, reactor_thread) = crate::reactor::spawn_reactor();
        let owner = Arc::new(SchedulerHandle {
            injector: Arc::new(crossbeam_deque::Injector::new()),
            process_table: Arc::new(dashmap::DashMap::new()),
            next_pid: Arc::new(crate::process::ProcessIdAllocator::starting_at(2)),
            runnable_count: Arc::new(AtomicI64::new(0)),
            alive_count: Arc::new(AtomicI64::new(1)),
            registered_names: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
            timer_wheel: Arc::new(crate::timer::TimerWheel::new()),
            worker_notify: Arc::new((parking_lot::Mutex::new(()), parking_lot::Condvar::new())),
            io_pool: Arc::new(crate::io_pool::IoPool::new(1)),
            #[cfg(mio_supported)]
            reactor: reactor.clone(),
            worker_inboxes: Arc::new(Vec::new()),
            num_workers: 0,
            worker_threads: Arc::new(Vec::new()),
            dispatch_rr: AtomicU32::new(0),
        });
        let receiver = ProcessId::new(4_243);
        let entry = Arc::new(ProcessEntry::new(Process::new(receiver)));
        owner.process_table.insert(receiver, Arc::clone(&entry));
        let mut sender = ProcessContext::new(ProcessId::new(1));
        sender.runtime = Arc::as_ptr(&owner);
        // Forget any entry an earlier test on this thread cached for the PID.
        SEND_CACHE.with(|cache| *cache.borrow_mut() = None);

        // The receiver is running, so the first message waits in its inbox.
        entry.lock_for_runtime().transition(ProcessState::Running);
        send(&mut sender, receiver, 1);
        assert_eq!(entry.inbox.len(), 1);
        // Its worker publishes the receive wait and releases the lock while
        // the first message is still in the inbox.
        entry.lock_for_runtime().transition(ProcessState::Waiting(WaitReason::Receive {
            timeout_deadline_ms: None,
        }));
        send(&mut sender, receiver, 2);

        // The receiver's next run drains its inbox into the mailbox.
        let mut proc = entry.lock_for_runtime();
        proc.drain_inbox();
        let received = proc.mailbox.messages().map(|message| {
            let number = match message.as_boxed_ptr() {
                // SAFETY: a boxed message is a 2-tuple on the heap of the
                // receiver, which stays locked and alive here.
                Some(tuple) => ErlTerm(unsafe { *tuple.add(2) }),
                None => message,
            };
            number.as_small_int().expect("a numbered message")
        }).collect();
        drop(proc);
        #[cfg(mio_supported)]
        {
            reactor.shutdown();
            reactor_thread.join().unwrap();
        }
        received
    }

    #[test]
    fn a_later_message_does_not_overtake_a_queued_one() {
        let received = received_after_two_sends(|sender, receiver, n| {
            // SAFETY: the sender context and its scheduler outlive the call;
            // the message is an immediate, so no sender heap is read.
            unsafe { nera_rt_send_message(sender, ErlTerm::pid(receiver).0, ErlTerm::small_int(n).0) };
        });
        assert_eq!(received, [1, 2]);
    }

    #[test]
    fn a_later_tuple_does_not_overtake_a_queued_one() {
        let received = received_after_two_sends(|sender, receiver, n| {
            let tag = ErlTerm::atom(crate::atom_table::ATOM_OK).0;
            // SAFETY: as above; both tuple elements are immediates.
            unsafe { nera_rt_send_tuple(sender, ErlTerm::pid(receiver).0, 2, tag, ErlTerm::small_int(n).0, 0, 0) };
        });
        assert_eq!(received, [1, 2]);
    }
}

#[cfg(test)]
mod send_tuple_tests {
    use super::*;

    #[test]
    fn direct_tuple_copy_does_not_borrow_sender_heap() {
        let mut sender = ProcessHeap::new();
        let binary = sender.alloc_binary_auto(&[42; 4096]);
        let list = sender.alloc_cons(binary, ErlTerm::nil());
        let mut receiver = ProcessHeap::new();
        let tuple = unsafe { build_tuple_on_heap(&mut receiver, 2, &[list.0, list.0, 0, 0], true) };
        let ptr = tuple.as_boxed_ptr().unwrap();
        let first = ErlTerm(unsafe { *ptr.add(1) });
        // Fail before freeing sender: the pre-fix failure never reads dangling data.
        assert_ne!(first, list, "receiver must own its copy of the sender's list");
        assert_eq!(first, ErlTerm(unsafe { *ptr.add(2) }), "preserve shared children");
        let received_binary = unsafe { (*first.as_cons_ptr().unwrap()).head };
        assert_ne!(received_binary, binary, "binary descriptors must be receiver-owned");
        drop(sender);
        let mut roots = [tuple];
        unsafe { receiver.collect_full(&mut roots); }
        let ptr = roots[0].as_boxed_ptr().unwrap();
        let first = ErlTerm(unsafe { *ptr.add(1) });
        assert_eq!(first, ErlTerm(unsafe { *ptr.add(2) }));
        let binary = unsafe { (*first.as_cons_ptr().unwrap()).head };
        assert_eq!(unsafe { binary.borrow(&receiver) }.binary_bytes(), Some(&[42; 4096][..]));
    }

    #[test]
    fn self_tuple_send_keeps_sender_children_and_return_value() {
        let mut heap = ProcessHeap::new();
        let list = heap.alloc_cons(ErlTerm::small_int(7), ErlTerm::nil());
        let mut mailbox = crate::process::Mailbox::new();
        let mut context = ProcessContext::new(crate::process::ProcessId::new(1));
        context.heap_ptr = &mut heap;
        context.mailbox_ptr = &mut mailbox;
        let result = ErlTerm(unsafe {
            nera_rt_send_tuple(&mut context, ErlTerm::pid(crate::process::ProcessId::new(1)).0, 2, list.0, list.0, 0, 0)
        });
        assert_eq!(mailbox.get(0), result);
        let ptr = result.as_boxed_ptr().unwrap();
        assert_eq!(ErlTerm(unsafe { *ptr.add(1) }), list);
        assert_eq!(ErlTerm(unsafe { *ptr.add(2) }), list);
    }
}

/// Return the current process's group leader without acquiring any process lock.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_group_leader(ctx: *mut ProcessContext) -> u64 {
    if ctx.is_null() { return raise_badarg(ctx); }
    let pid = unsafe { (*ctx).pid };
    with_sched_from_ctx(ctx, |scheduler| {
        scheduler.and_then(|s| s.process_table.get(&pid).map(|p| ErlTerm::pid(p.group_leader()).0))
            .unwrap_or_else(|| raise_badarg(ctx))
    })
}

/// Assign the group leader of a live local process.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_group_leader_2(ctx: *mut ProcessContext, leader: u64, target: u64) -> u64 {
    let (Some(leader), Some(target)) = (ErlTerm(leader).as_pid_id(), ErlTerm(target).as_pid_id()) else {
        return raise_badarg(ctx);
    };
    with_sched_from_ctx(ctx, |scheduler| {
        let Some(scheduler) = scheduler else { return raise_badarg(ctx); };
        let Some(leader_entry) = scheduler.process_table.get(&leader) else { return raise_badarg(ctx); };
        let Some(target_entry) = scheduler.process_table.get(&target) else { return raise_badarg(ctx); };
        if leader_entry.published_status() == crate::process::ASTATUS_EXITED || target_entry.published_status() == crate::process::ASTATUS_EXITED {
            return raise_badarg(ctx);
        }
        target_entry.set_group_leader(leader);
        ErlTerm::atom(crate::atom_table::ATOM_TRUE).0
    })
}

#[cfg(test)]
mod garbage_collect_tests {
    use super::*;
    use crate::binary::BinaryData;
    use crate::process::{ProcessId, STATUS_OK, STATUS_YIELD};

    #[test]
    fn worker_fulfils_explicit_gc_before_resuming_caller() {
        // NERA-21 F3: cow_hpack's GC must reclaim unreachable old backing
        // storage through the real worker, before resuming its caller. A BIF
        // success stub or a removed worker safe-point call must fail this test.
        use crate::binary::BinaryData;
        use crate::process::ProcessContext;
        use crate::scheduler::Scheduler;
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::{Mutex, Weak};

        static DEAD: Mutex<Option<Weak<BinaryData>>> = Mutex::new(None);
        static OBSERVED: AtomicU64 = AtomicU64::new(0);

        unsafe extern "C" fn observe(ctx: *mut ProcessContext, _: *const u64, _: u32) {
            let freed = DEAD.lock().unwrap().as_ref().unwrap().upgrade().is_none();
            let returned_true = unsafe { (*ctx).result == ErlTerm::atom(crate::atom_table::ATOM_TRUE).0 };
            eprintln!("worker_gc_observation: returned_true={returned_true} old_backing_reclaimed={freed}");
            OBSERVED.store(if freed && returned_true { 2 } else { 1 }, Ordering::SeqCst);
        }

        unsafe extern "C" fn request(ctx: *mut ProcessContext, _: *const u64, _: u32) {
            let result = unsafe { nera_rt_garbage_collect(ctx) };
            // Follow the compiled caller's continuation protocol, including
            // the non-suspending path so a success-only stub is observable.
            if unsafe { (*ctx).status == crate::process::STATUS_OK } {
                unsafe { (*ctx).result = result; observe(ctx, std::ptr::null(), 0); }
            } else {
                unsafe { super::super::receive::nera_rt_set_resume(ctx, observe, std::ptr::null(), 0, 0); }
            }
        }

        let scheduler = Scheduler::new(1);
        let mut process = Process::with_entry(scheduler.next_pid(), request, vec![]);
        let backing = Arc::new(BinaryData::new(vec![17; 1000]));
        let owner = Arc::downgrade(&backing);
        let term = process.heap.alloc_refc_binary_from_arc(backing, 0, 1000);
        process.dictionary.push((ErlTerm::small_int(1), term));
        unsafe { process.collect_garbage_full(); }
        process.dictionary.clear();
        assert!(owner.upgrade().is_some(), "fixture retains unreachable old storage");
        assert!(!process.heap.needs_gc(), "pressure GC must not mask a missing explicit GC");
        *DEAD.lock().unwrap() = Some(owner);
        OBSERVED.store(0, Ordering::SeqCst);
        scheduler.spawn_process(process);
        assert!(unsafe { scheduler.run() });
        assert_eq!(OBSERVED.load(Ordering::SeqCst), 2, "caller resumed without reclaiming old storage");
        *DEAD.lock().unwrap() = None;
    }

    #[test]
    fn explicit_gc_waits_for_roots_then_collects_old_garbage() {
        // cow_hpack's garbage_collect/0 is a real full collection, not a success stub.
        let mut process = Process::new(ProcessId::new(19));
        let live = Arc::new(BinaryData::new(vec![42; 1000]));
        let dead = Arc::new(BinaryData::new(vec![17; 1000]));
        let live_owner = Arc::downgrade(&live);
        let dead_owner = Arc::downgrade(&dead);
        let live_term = process.heap.alloc_refc_binary_from_arc(live, 0, 1000);
        let dead_term = process.heap.alloc_refc_binary_from_arc(dead, 0, 1000);
        process.dictionary = vec![(ErlTerm::small_int(1), live_term), (ErlTerm::small_int(2), dead_term)];
        unsafe { process.collect_garbage_full(); }
        process.dictionary.pop();
        assert!(dead_owner.upgrade().is_some(), "old binary retained until full GC");
        process.context.heap_ptr = &mut process.heap;
        unsafe { nera_rt_garbage_collect(&mut process.context); }
        assert_eq!(process.context.status, STATUS_YIELD);
        assert!(dead_owner.upgrade().is_some(), "BIF must not collect through compiled frames");
        process.resume_continuations = super::super::receive::take_pending_resumes();
        assert_eq!(process.resume_continuations.len(), 1);
        unsafe { assert!(process.collect_requested_garbage()); }
        assert!(dead_owner.upgrade().is_none(), "explicit GC must reclaim unreachable old backing storage");
        assert!(live_owner.upgrade().is_some());
        let (ptr, len) = unsafe { binary_data(process.dictionary[0].1).unwrap() };
        assert_eq!(unsafe { std::slice::from_raw_parts(ptr, len) }, &[42; 1000]);
        unsafe { assert!(!process.collect_requested_garbage()); process.run_slice(); }
        assert_eq!(process.context.status, STATUS_OK);
        assert_eq!(process.context.result, ErlTerm::atom(crate::atom_table::ATOM_TRUE).0);
    }
}
