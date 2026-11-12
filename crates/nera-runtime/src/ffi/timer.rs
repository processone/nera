//! Timer operations: send_after, start_timer, cancel_timer, system_time.

#![allow(unused_unsafe)]

use crate::atom_table::global_atom_table;
use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;
use super::misc::nera_rt_make_ref;

/// Both message-timer APIs register the returned reference before dispatch.
unsafe fn message_timer(ctx: *mut ProcessContext, time_term: u64, dest_term: u64, msg_term: u64, timeout_tuple: bool) -> u64 {
    let time_ms = match ErlTerm(time_term).as_small_int() {
        Some(t) if t >= 0 => t as u64,
        _ => return raise_badarg(ctx),
    };
    let dest_pid = if let Some(id) = ErlTerm(dest_term).as_pid_id() {
        id
    } else if let Some(atom_id) = ErlTerm(dest_term).as_atom_id() {
        let found = with_sched_from_ctx(ctx, |scheduler| {
            scheduler.and_then(|sched| sched.registered_names.read().get(&atom_id).copied())
        });
        match found {
            Some(pid) => pid,
            None => return raise_badarg(ctx),
        }
    } else {
        return raise_badarg(ctx);
    };
    let reference = nera_rt_make_ref(ctx);
    let id = *ErlTerm(reference).as_boxed_ptr().unwrap().add(REFERENCE_ID_WORD_INDEX);
    let message = if timeout_tuple {
        let timeout = ErlTerm::atom(global_atom_table().intern("timeout"));
        with_ctx_heap(ctx, |heap| heap.alloc_tuple(&[timeout, ErlTerm(reference), ErlTerm(msg_term)]))
    } else {
        ErlTerm(msg_term)
    };
    let fragment = crate::message::MessageFragment::from_term(message);
    with_sched_from_ctx(ctx, |scheduler| {
        if let Some(sched) = scheduler {
            let process_table = std::sync::Arc::clone(&sched.process_table);
            let injector = sched.injector.clone();
            let runnable_count = std::sync::Arc::clone(&sched.runnable_count);
            let timer_wheel = std::sync::Arc::clone(&sched.timer_wheel);
            let timer = timer_wheel.register_message_timer(id, std::time::Duration::from_millis(time_ms));
            std::thread::spawn(move || timer.run(|| {
                let entry = process_table.get(&dest_pid).map(|r| r.clone());
                if let Some(entry) = entry {
                    entry.inbox.push(fragment);
                    // The worker cancels the receive timer; see nera_rt_send_message.
                    if entry.try_wake() {
                        runnable_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        injector.push(entry);
                        if let Some(sched) = crate::scheduler::global_scheduler() {
                            for t in sched.worker_threads.iter() {
                                if let Some(t) = t.get() {
                                    t.unpark();
                                    break;
                                }
                            }
                        }
                    }
                }
            }));
        }
    });
    reference
}

/// erlang:send_after/3 — send Msg to Pid after Time milliseconds.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_send_after(ctx: *mut ProcessContext, time_term: u64, dest_term: u64, msg_term: u64) -> u64 {
    message_timer(ctx, time_term, dest_term, msg_term, false)
}

/// erlang:start_timer/3 — send {timeout, Ref, Msg} to Pid after Time ms.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_start_timer(ctx: *mut ProcessContext, time_term: u64, dest_term: u64, msg_term: u64) -> u64 {
    message_timer(ctx, time_term, dest_term, msg_term, true)
}

/// erlang:cancel_timer/1 — synchronous cancellation with remaining-time info.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_cancel_timer(ctx: *mut ProcessContext, ref_term: u64) -> u64 {
    nera_rt_cancel_timer_2(ctx, ref_term, ErlTerm::nil().0)
}

/// erlang:cancel_timer/2 — cancel with async/info boolean options.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_cancel_timer_2(ctx: *mut ProcessContext, ref_term: u64, opts_term: u64) -> u64 {
    let reference = ErlTerm(ref_term);
    if !reference.is_reference() {
        return raise_badarg(ctx);
    }
    let mut asynchronous = false;
    let mut info = true;
    let mut opts = ErlTerm(opts_term);
    while opts.is_cons() {
        let cell = &*opts.as_cons_ptr().unwrap();
        let option = cell.head;
        if !option.is_tuple() || BoxedHeader(*option.as_boxed_ptr().unwrap()).arity() != 2 {
            return raise_badarg(ctx);
        }
        let ptr = option.as_boxed_ptr().unwrap();
        let pair = [ErlTerm(*ptr.add(1)), ErlTerm(*ptr.add(2))];
        let value = if pair[1] == ErlTerm::atom(crate::atom_table::ATOM_TRUE) {
            true
        } else if pair[1] == ErlTerm::atom(crate::atom_table::ATOM_FALSE) {
            false
        } else {
            return raise_badarg(ctx);
        };
        match pair[0].as_atom_id().and_then(|id| global_atom_table().resolve(id)).as_deref() {
            Some("async") => asynchronous = value,
            Some("info") => info = value,
            _ => return raise_badarg(ctx),
        }
        opts = cell.tail;
    }
    if !opts.is_nil() {
        return raise_badarg(ctx);
    }
    // Resource references have a different payload domain, never a timer ID.
    let remaining = if *reference.as_boxed_ptr().unwrap() == ReferenceKind::Ordinary.header().0 {
        let id = *reference.as_boxed_ptr().unwrap().add(REFERENCE_ID_WORD_INDEX);
        with_sched_from_ctx(ctx, |scheduler| scheduler.and_then(|s| s.timer_wheel.cancel_message_timer(id)))
    } else {
        None
    };
    let result = remaining.map_or(ErlTerm::atom(crate::atom_table::ATOM_FALSE).0,
                                  |ms| integer_term(ctx, ms as i64));
    if asynchronous && info && !ctx.is_null() {
        // Cancellation may complete immediately; only the information is sent
        // as a message, just as async=true permits on BEAM.
        let tag = ErlTerm::atom(global_atom_table().intern("cancel_timer"));
        let message = with_ctx_heap(ctx, |heap| heap.alloc_tuple(&[tag, reference, ErlTerm(result)]));
        super::process::nera_rt_send_message(ctx, ErlTerm::pid((*ctx).pid).0, message.0);
    }
    if asynchronous || !info { ErlTerm::atom(crate::atom_table::ATOM_OK).0 } else { result }
}

/// erlang:system_time/1 — returns wall-clock time in the specified unit.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_system_time_1(ctx: *mut ProcessContext, unit_term: u64) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as i64;
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
                _ => return ErlTerm::small_int(0).0,
            };
            return integer_term(ctx, value);
        }
    }
    ErlTerm::small_int(0).0
}

/// Convert the system clock to whole Unix seconds, flooring before the epoch.
fn unix_seconds(time: std::time::SystemTime) -> Option<i64> {
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs()).ok(),
        Err(error) => {
            let duration = error.duration();
            let seconds = i64::try_from(duration.as_secs()).ok()?;
            seconds
                .checked_neg()?
                .checked_sub(i64::from(duration.subsec_nanos() != 0))
        }
    }
}

/// Reentrant UTC conversion: does not read or mutate the process timezone.
fn utc_components(seconds: i64) -> Option<[i64; 6]> {
    // time_t can be narrower than i64 on 32-bit Unix targets.
    #[allow(clippy::useless_conversion)]
    let time: libc::time_t = seconds.try_into().ok()?;
    let mut date = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: time is initialized; date points to writable tm storage. Both
    // platform functions initialize the result completely on success.
    unsafe {
        #[cfg(unix)]
        if libc::gmtime_r(&time, date.as_mut_ptr()).is_null() {
            return None;
        }
        #[cfg(windows)]
        if libc::gmtime_s(date.as_mut_ptr(), &time) != 0 {
            return None;
        }
        #[cfg(not(any(unix, windows)))]
        return None;
        let date = date.assume_init();
        Some([
            i64::from(date.tm_year) + 1900,
            i64::from(date.tm_mon) + 1,
            i64::from(date.tm_mday),
            i64::from(date.tm_hour),
            i64::from(date.tm_min),
            i64::from(date.tm_sec),
        ])
    }
}

/// erlang:universaltime/0 — current UTC date/time, independent of local TZ.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_universaltime(ctx: *mut ProcessContext) -> u64 {
    let Some(parts) = unix_seconds(std::time::SystemTime::now()).and_then(utc_components) else {
        return raise_badarg(ctx);
    };
    with_ctx_heap(ctx, |heap| {
        let terms = parts.map(ErlTerm::small_int);
        let date = heap.alloc_tuple(&terms[..3]);
        let time = heap.alloc_tuple(&terms[3..]);
        heap.alloc_tuple(&[date, time]).0
    })
}

#[cfg(test)]
mod utc_tests {
    use super::*;
    #[test]
    fn known_utc_boundaries() {
        for (seconds, expected) in [
            (0, [1970, 1, 1, 0, 0, 0]),
            (86399, [1970, 1, 1, 23, 59, 59]),
            (86400, [1970, 1, 2, 0, 0, 0]),
            (946684799, [1999, 12, 31, 23, 59, 59]),
            (946684800, [2000, 1, 1, 0, 0, 0]),
            (951782400, [2000, 2, 29, 0, 0, 0]),
            (951868800, [2000, 3, 1, 0, 0, 0]),
            (2147483648, [2038, 1, 19, 3, 14, 8]),
        ] {
            assert_eq!(utc_components(seconds), Some(expected));
        }
    }
    #[test]
    fn clock_seconds_floor_subseconds() {
        use std::time::{Duration, UNIX_EPOCH};
        assert_eq!(unix_seconds(UNIX_EPOCH), Some(0));
        assert_eq!(
            unix_seconds(UNIX_EPOCH + Duration::from_nanos(999999999)),
            Some(0)
        );
        assert_eq!(unix_seconds(UNIX_EPOCH - Duration::from_nanos(1)), Some(-1));
        assert_eq!(unix_seconds(UNIX_EPOCH - Duration::from_secs(1)), Some(-1));
        assert_eq!(
            unix_seconds(UNIX_EPOCH - Duration::from_millis(1001)),
            Some(-2)
        );
    }
}
