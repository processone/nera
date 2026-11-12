//! Resumable encoding adapter. Each call processes at most 48 KiB.
use super::helpers::{binary_data, raise_badarg, raise_error};
use super::native_work::start;
use crate::{
    atom_table::{ATOM_SYSTEM_LIMIT, ATOM_TRUE},
    base64::Encoder,
    process::ProcessContext,
    term::ErlTerm,
};

/// Encode a byte-aligned binary with boolean alphabet and padding flags.
/// The Erlang wrapper retains option and flat-list validation semantics.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_base64_encode_binary(
    ctx: *mut ProcessContext,
    input: u64,
    url: u64,
    pad: u64,
) -> u64 {
    // SAFETY: generated code supplies a live input and exclusive context. No
    // collection or compiled calls occur while the input slice is borrowed.
    if !unsafe { ErlTerm(input).is_binary() } {
        return unsafe { raise_badarg(ctx) };
    }
    let (_, len) = unsafe { binary_data(ErlTerm(input)).unwrap() };
    let yes = ErlTerm::atom(ATOM_TRUE).0;
    let Some(state) = Encoder::new(len, url == yes, pad == yes) else {
        return unsafe { raise_error(ctx, ErlTerm::atom(ATOM_SYSTEM_LIMIT).0) };
    };
    unsafe { start(ctx, input, state) }
}

/// Binary decoding uses the same rooted, budgeted adapter as encoding.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_base64_decode_binary(
    ctx: *mut ProcessContext,
    input: u64,
    url: u64,
    pad: u64,
) -> u64 {
    if !unsafe { ErlTerm(input).is_binary() } {
        return unsafe { raise_badarg(ctx) };
    }
    let (_, len) = unsafe { binary_data(ErlTerm(input)).unwrap() };
    let yes = ErlTerm::atom(ATOM_TRUE).0;
    let Some(state) = crate::base64::Decoder::new(len, url == yes, pad == yes) else {
        return unsafe { raise_error(ctx, ErlTerm::atom(ATOM_SYSTEM_LIMIT).0) };
    };
    unsafe { start(ctx, input, state) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom_table::global_atom_table;
    use crate::binary::BinaryData;
    use crate::{
        heap::ProcessHeap,
        process::{STATUS_OK, STATUS_YIELD},
    };
    use std::sync::Arc;

    #[test]
    fn decoder_survives_gc_and_adopts_output() {
        use crate::process::Process;
        let mut process = Process::new(crate::process::ProcessId::new(11));
        let data = Arc::new(BinaryData::new(vec![b'A'; 200_002]));
        let owner = Arc::downgrade(&data);
        let input = process.heap.alloc_refc_binary_from_arc(data, 2, 200_000);
        process.context.heap_ptr = &mut process.heap;
        let yes = ErlTerm::atom(ATOM_TRUE).0;
        unsafe {
            nera_rt_base64_decode_binary(&mut process.context, input.0, yes, yes);
        }
        assert_eq!(process.context.status, STATUS_YIELD);
        let output_ptr = process
            .context
            .native_work
            .as_mut()
            .unwrap()
            .output()
            .as_ptr();
        process.resume_continuations = super::super::receive::take_pending_resumes();
        let mut slices = 0;
        while !process.resume_continuations.is_empty() {
            unsafe {
                process.collect_garbage_full();
                process.reductions = 4000;
                process.run_slice();
            }
            assert!(owner.upgrade().is_some());
            slices += 1;
            assert!(slices < 10);
        }
        assert!(slices > 1);
        assert_eq!(process.context.status, STATUS_OK);
        assert!(process.context.native_work.is_none());
        let (ptr, len) = unsafe { binary_data(ErlTerm(process.context.result)).unwrap() };
        assert_eq!(ptr, output_ptr);
        assert_eq!(len, 150_000);
        assert!(unsafe { std::slice::from_raw_parts(ptr, len) }
            .iter()
            .all(|&b| b == 0));
        drop(process);
        assert!(owner.upgrade().is_none());
    }

    #[test]
    fn decoder_charges_whitespace_and_keeps_resumed_exception_reason() {
        let mut heap = ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(12));
        ctx.heap_ptr = &mut heap;
        let mut bytes = vec![b' '; 32 * 1024];
        bytes.extend_from_slice(b"Y===");
        let input = heap.alloc_binary_auto(&bytes);
        ctx.reductions = 1;
        let yes = ErlTerm::atom(ATOM_TRUE).0;
        unsafe {
            nera_rt_base64_decode_binary(&mut ctx, input.0, yes, yes);
        }
        assert_eq!(ctx.status, STATUS_YIELD);
        assert_eq!(ctx.native_work.as_ref().unwrap().offset(), 16);
        let mut slices = 0;
        while ctx.status == STATUS_YIELD {
            let pending = super::super::receive::take_pending_resumes();
            assert_eq!(pending.len(), 1);
            let cont = &pending[0];
            let args: Vec<_> = cont.args.iter().map(|t| t.0).collect();
            ctx.reductions = 4000;
            ctx.status = STATUS_OK;
            // SAFETY: the context and argument storage are live and exclusively
            // accessible here; no heap borrow crosses this reentry boundary.
            unsafe { (cont.func)(&mut ctx, args.as_ptr(), args.len() as u32); }
            slices += 1;
            assert!(slices < 10);
        }
        assert_eq!(ctx.status, crate::process::STATUS_EXCEPTION);
        assert_eq!(
            ctx.result,
            ErlTerm::atom(crate::atom_table::ATOM_BADARITH).0
        );
        assert_eq!(
            ctx.reductions,
            4000 - (bytes.len() - 16).div_ceil(16) as i64
        );
        assert!(ctx.native_work.is_none());
    }

    #[test]
    fn large_encode_yields_and_resumes_without_losing_bytes() {
        let mut heap = ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let bytes = vec![0xfb; 200_003];
        let input = heap.alloc_binary_auto(&bytes);
        let yes = ErlTerm::atom(global_atom_table().intern("true")).0;
        let expected = [b"-_v7".repeat(bytes.len() / 3), b"-_s=".to_vec()].concat();
        let result = unsafe { nera_rt_base64_encode_binary(&mut ctx, input.0, yes, yes) };
        assert_eq!(
            ctx.status, STATUS_YIELD,
            "large encoding must suspend the calling process"
        );
        let mut rounds = 0;
        let mut result = result;
        while ctx.status == STATUS_YIELD {
            let pending = super::super::receive::take_pending_resumes();
            assert_eq!(pending.len(), 1);
            let cont = &pending[0];
            let args: Vec<_> = cont.args.iter().map(|t| t.0).collect();
            ctx.status = STATUS_OK;
            ctx.reductions = 4000;
            // SAFETY: the context and argument storage are live and exclusively
            // accessible here; no heap borrow crosses this reentry boundary.
            unsafe { (cont.func)(&mut ctx, args.as_ptr(), args.len() as u32); }
            result = ctx.result;
            rounds += 1;
            assert!(rounds < 10);
        }
        assert!(rounds >= 2);
        let (ptr, len) = unsafe { binary_data(ErlTerm(result)).unwrap() };
        assert_eq!(unsafe { std::slice::from_raw_parts(ptr, len) }, expected);
    }
    #[test]
    fn suspended_encoder_survives_full_gc_and_adopts_output() {
        use crate::process::Process;
        let mut process = Process::new(crate::process::ProcessId::new(7));
        let data = Arc::new(BinaryData::new(vec![0; 150_003]));
        let owner = Arc::downgrade(&data);
        let input = process.heap.alloc_refc_binary_from_arc(data, 3, 150_000);
        process.context.heap_ptr = &mut process.heap;
        let no = ErlTerm::atom(global_atom_table().intern("false")).0;
        unsafe {
            nera_rt_base64_encode_binary(&mut process.context, input.0, no, no);
        }
        let output_ptr = process
            .context
            .native_work
            .as_mut()
            .unwrap()
            .output()
            .as_ptr();
        process.resume_continuations = super::super::receive::take_pending_resumes();
        let mut slices = 0;
        while !process.resume_continuations.is_empty() {
            // The continuation is the input's only Erlang root here.
            unsafe {
                process.collect_garbage_full();
            }
            assert!(owner.upgrade().is_some());
            unsafe {
                process.reductions = 4000;
                process.run_slice();
            }
            slices += 1;
            assert!(slices < 10);
        }
        assert!(slices >= 2);
        assert_eq!(process.context.status, STATUS_OK);
        assert!(process.context.native_work.is_none());
        let (ptr, len) = unsafe { binary_data(ErlTerm(process.context.result)).unwrap() };
        assert_eq!(
            ptr, output_ptr,
            "finalization must adopt the existing allocation"
        );
        assert_eq!(len, 200_000);
        assert!(unsafe { std::slice::from_raw_parts(ptr, len) }
            .iter()
            .all(|b| *b == b'A'));
        drop(process);
        assert!(owner.upgrade().is_none());
    }

    #[test]
    fn dropping_a_suspended_process_releases_its_input() {
        use crate::process::Process;
        let mut process = Process::new(crate::process::ProcessId::new(8));
        let data = Arc::new(BinaryData::new(vec![1; 100_000]));
        let owner = Arc::downgrade(&data);
        let input = process.heap.alloc_refc_binary_from_arc(data, 0, 100_000);
        process.context.heap_ptr = &mut process.heap;
        let no = ErlTerm::atom(global_atom_table().intern("false")).0;
        unsafe {
            nera_rt_base64_encode_binary(&mut process.context, input.0, no, no);
        }
        process.resume_continuations = super::super::receive::take_pending_resumes();
        assert_eq!(process.context.status, STATUS_YIELD);
        assert!(process.context.native_work.is_some());
        drop(process);
        assert!(owner.upgrade().is_none());
    }
    #[test]
    fn encoding_respects_remaining_budget_before_work() {
        let mut heap = ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        ctx.reductions = 1;
        let input = heap.alloc_binary_auto(&vec![0; 49152]);
        unsafe {
            nera_rt_base64_encode_binary(&mut ctx, input.0, 0, 0);
        }
        assert_eq!(ctx.status, STATUS_YIELD);
        assert!(ctx.native_work.as_ref().unwrap().offset() <= 32);
        super::super::receive::take_pending_resumes();
    }

    #[test]
    fn encoding_charges_empty_small_and_final_chunks() {
        let mut heap = ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let yes = ErlTerm::atom(ATOM_TRUE).0;
        for (len, cost) in [(0, 1), (1, 1), (32, 1), (33, 2), (49_152, 1536)] {
            ctx.reductions = 4000;
            let input = heap.alloc_binary_auto(&vec![0; len]);
            unsafe {
                nera_rt_base64_encode_binary(&mut ctx, input.0, yes, yes);
            }
            assert_eq!(ctx.status, STATUS_OK);
            assert_eq!(ctx.reductions, 4000 - cost, "input length {len}");
        }
        let input = heap.alloc_binary_auto(&vec![0; 49_153]);
        unsafe {
            nera_rt_base64_encode_binary(&mut ctx, input.0, yes, yes);
        }
        assert_eq!(ctx.status, STATUS_YIELD);
        let continuations = super::super::receive::take_pending_resumes();
        assert_eq!(continuations.len(), 1);
        let cont = &continuations[0];
        let args: Vec<_> = cont.args.iter().map(|t| t.0).collect();
        ctx.reductions = 4000;
        ctx.status = STATUS_OK;
        // SAFETY: the context and argument storage are live and exclusively
        // accessible here; no heap borrow crosses this reentry boundary.
        unsafe { (cont.func)(&mut ctx, args.as_ptr(), args.len() as u32); }
        assert_eq!(ctx.status, STATUS_OK);
        assert_eq!(ctx.reductions, 3999, "resumed final byte must be charged");
    }

    #[test]
    fn single_worker_repeated_small_encodes_share_cpu() {
        use crate::{
            process::Process,
            scheduler::Scheduler,
        };
        use std::sync::atomic::{AtomicUsize, Ordering};
        let scheduler = Arc::new(Scheduler::new(1));
        let calls = Arc::new(AtomicUsize::new(0));
        let first_slice = Arc::new(AtomicUsize::new(0));
        let observed = Arc::new(AtomicUsize::new(usize::MAX));
        let running = Arc::clone(&scheduler);
        let count = Arc::clone(&calls);
        let first = Arc::clone(&first_slice);
        let result = Arc::clone(&observed);
        let mut busy = Process::new(scheduler.next_pid());
        busy.rust_fn = Some(Box::new(move |inner| {
            if count.load(Ordering::SeqCst) == 0 {
                let mut observer = Process::new(running.next_pid());
                let count = Arc::clone(&count);
                let result = Arc::clone(&result);
                observer.rust_fn = Some(Box::new(move |inner| {
                    result.store(count.load(Ordering::SeqCst), Ordering::SeqCst);
                    inner.finish();
                }));
                running.spawn_process(observer);
            }
            inner.context.heap_ptr = &mut inner.heap;
            let input = inner.heap.alloc_binary_auto(&vec![0; 49_149]);
            let yes = ErlTerm::atom(crate::atom_table::ATOM_TRUE).0;
            while inner.context.reductions >= 1536 && count.load(Ordering::SeqCst) < 64 {
                // Same budget check as a generated Erlang loop back edge.
                inner.context.result =
                    unsafe { nera_rt_base64_encode_binary(&mut inner.context, input.0, yes, yes) };
                inner.context.reductions -= crate::process::YIELD_CHECK_COST;
                count.fetch_add(1, Ordering::SeqCst);
            }
            let n = count.load(Ordering::SeqCst);
            let _ = first.compare_exchange(0, n, Ordering::SeqCst, Ordering::SeqCst);
            if n == 64 {
                inner.finish();
            } else {
                inner.context.status = STATUS_YIELD;
            }
        }));
        scheduler.spawn_process(busy);
        assert!(unsafe { scheduler.run() });
        eprintln!(
            "small encodes: first slice={}, observer after={} calls",
            first_slice.load(Ordering::SeqCst),
            observed.load(Ordering::SeqCst)
        );
        assert!(
            first_slice.load(Ordering::SeqCst) <= 4,
            "one slice encoded {} near-threshold binaries",
            first_slice.load(Ordering::SeqCst)
        );
        assert!(
            observed.load(Ordering::SeqCst) < 64,
            "observer ran only after all encodings completed"
        );
    }

    #[test]
    fn single_worker_observer_runs_during_large_native_encoding() {
        use crate::{
            process::Process,
            scheduler::Scheduler,
        };
        use std::sync::atomic::{AtomicUsize, Ordering};
        extern "C" fn start(ctx: *mut ProcessContext, args: *const u64, _: u32) {
            unsafe {
                let yes = ErlTerm::atom(crate::atom_table::ATOM_TRUE).0;
                (*ctx).result = nera_rt_base64_encode_binary(ctx, *args, yes, yes);
            }
        }
        let scheduler = Scheduler::new(1);
        let len = 3 * 1024 * 1024;
        let mut producer = Process::with_entry(scheduler.next_pid(), start, Vec::new());
        let input = producer.heap.alloc_binary_auto(&vec![0; len]);
        producer.initial_args.push(input);
        let pid = scheduler.spawn_process(producer);
        let entry = scheduler.get_process(pid).unwrap();
        let progress = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&progress);
        let mut observer = Process::new(scheduler.next_pid());
        observer.rust_fn = Some(Box::new(move |inner| {
            let producer = entry.lock_for_runtime();
            if let Some(state) = &producer.context.native_work {
                observed.store(state.offset(), Ordering::SeqCst);
            }
            inner.finish();
        }));
        scheduler.spawn_process(observer);
        assert!(unsafe { scheduler.run() });
        let bytes = progress.load(Ordering::SeqCst);
        eprintln!("large encode: observer at {bytes}/{len} bytes");
        assert!(
            bytes > 0 && bytes < len,
            "observer did not run between native chunks: progress={bytes}"
        );
    }
}
