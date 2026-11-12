//! Cooperative Base64 decoding over list cells. The remaining list is rooted in the continuation;
//! native state never holds Erlang terms or pointers across a yield.
use super::native_work::{fail, finish};
use super::receive::nera_rt_set_resume;
use crate::native_work::WorkError;
use crate::{
    base64::ListDecoder,
    process::{ProcessContext, STATUS_YIELD},
    term::ErlTerm,
};

const CELLS_PER_REDUCTION: usize = 8;
const MAX_CELLS: usize = 8192;

unsafe fn step(
    ctx: *mut ProcessContext,
    tail: &mut ErlTerm,
    state: &mut ListDecoder,
) -> Result<bool, WorkError> {
    let limit = usize::try_from((*ctx).reductions.max(0))
        .unwrap_or(usize::MAX)
        .saturating_mul(CELLS_PER_REDUCTION)
        .min(MAX_CELLS);
    if limit == 0 {
        return Ok(false);
    }
    let mut consumed = 0usize;
    let result = (|| {
        while consumed < limit {
            if tail.is_nil() {
                return state.end().map(|()| true);
            }
            let Some(ptr) = tail.as_cons_ptr() else {
                return Err(WorkError::Atom("function_clause"));
            };
            // No Erlang heap allocation, GC, or compiled call inside this loop.
            let cell = &*ptr;
            let head = cell.head;
            *tail = cell.tail;
            consumed += 1;
            let symbol = if let Some(n) = head.as_small_int() {
                state.symbol(n)?
            } else if head.is_bigint() || head.boxed_subtype() == Some(crate::term::BOXED_FLOAT) {
                return Err(WorkError::Atom("badarg"));
            } else {
                return Err(WorkError::Atom("badarith"));
            };
            state.feed(symbol)?;
        }
        if tail.is_nil() {
            state.end().map(|()| true)
        } else {
            Ok(false)
        }
    })();
    (*ctx).reductions = (*ctx)
        .reductions
        .saturating_sub(consumed.div_ceil(CELLS_PER_REDUCTION).max(1) as i64);
    result
}
unsafe fn suspend(ctx: *mut ProcessContext, tail: ErlTerm) -> u64 {
    nera_rt_set_resume(ctx, resume, &tail.0, 1, 0);
    (*ctx).reductions = 0;
    (*ctx).status = STATUS_YIELD;
    ErlTerm::nil().0
}
extern "C" fn resume(ctx: *mut ProcessContext, args: *const u64, _: u32) {
    unsafe {
        let mut tail = ErlTerm(*args);
        let mut state = (*ctx).base64_list_decode.take().unwrap();
        match step(ctx, &mut tail, &mut state) {
            Ok(true) => (*ctx).result = finish(ctx, state.take_output()),
            Ok(false) => {
                (*ctx).base64_list_decode = Some(state);
                (*ctx).result = suspend(ctx, tail);
            }
            Err(e) => {
                fail(ctx, e);
            }
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn nera_rt_base64_decode_list(
    ctx: *mut ProcessContext,
    input: u64,
    url: u64,
    pad: u64,
) -> u64 {
    let yes = ErlTerm::atom(crate::atom_table::ATOM_TRUE).0;
    let Some(mut state) = ListDecoder::new(url == yes, pad == yes) else {
        return fail(ctx, WorkError::Atom("system_limit"));
    };
    let mut tail = ErlTerm(input);
    match step(ctx, &mut tail, &mut state) {
        Ok(true) => finish(ctx, state.take_output()),
        Ok(false) => {
            (*ctx).base64_list_decode = Some(Box::new(state));
            suspend(ctx, tail)
        }
        Err(e) => fail(ctx, e),
    }
}

#[cfg(test)]
mod tests {
    use super::super::receive::take_pending_resumes;
    use super::*;
    use crate::process::{Process, STATUS_EXCEPTION, STATUS_OK};
    use std::sync::Arc;
    fn input(process: &mut Process, len: usize, suffix: &[ErlTerm]) -> ErlTerm {
        let mut tail = process.heap.alloc_list_from_slice(suffix);
        for _ in 0..len {
            tail = process.heap.alloc_cons(ErlTerm::small_int(65), tail);
        }
        tail
    }
    #[test]
    fn base64_list_decoder_roots_only_tail_across_full_gc() {
        let mut p = Process::new(crate::process::ProcessId::new(81));
        let chars: Vec<_> = b"ABCD"
            .iter()
            .cycle()
            .take(40000)
            .map(|&c| ErlTerm::small_int(i64::from(c)))
            .collect();
        let data = p.heap.alloc_list_from_slice(&chars);
        let list = p.heap.alloc_cons(ErlTerm::small_int(32), data);
        p.context.heap_ptr = &mut p.heap;
        p.context.reductions = 1;
        let yes = ErlTerm::atom(crate::atom_table::ATOM_TRUE).0;
        unsafe {
            nera_rt_base64_decode_list(&mut p.context, list.0, 0, yes);
        }
        assert_eq!(p.context.status, STATUS_YIELD);
        assert_eq!(
            p.context
                .base64_list_decode
                .as_mut()
                .unwrap()
                .decoder_output_len(),
            3
        );
        p.resume_continuations = take_pending_resumes();
        let mut slices = 0;
        while !p.resume_continuations.is_empty() {
            unsafe {
                p.collect_garbage_full();
                p.reductions = 4000;
                p.run_slice();
            }
            slices += 1;
            assert!(slices < 10);
        }
        assert!(slices > 1);
        assert_eq!(p.context.status, STATUS_OK);
        assert!(p.context.base64_list_decode.is_none());
        let (ptr, len) =
            unsafe { super::super::helpers::binary_data(ErlTerm(p.context.result)).unwrap() };
        assert_eq!(len, 30000);
        let (chunks, remainder) = unsafe { std::slice::from_raw_parts(ptr, len) }.as_chunks::<3>();
        assert!(remainder.is_empty());
        assert!(chunks.iter().all(|chunk| *chunk == [0, 16, 131]));
    }
    #[test]
    fn base64_list_decoder_exhausted_budget_errors_and_drop() {
        let yes = ErlTerm::atom(crate::atom_table::ATOM_TRUE).0;
        for budget in [0, 1, 4000] {
            let mut p = Process::new(crate::process::ProcessId::new(82));
            let list = input(&mut p, 32768, &[ErlTerm::small_int(512)]);
            p.context.heap_ptr = &mut p.heap;
            p.context.reductions = budget;
            unsafe {
                nera_rt_base64_decode_list(&mut p.context, list.0, 0, yes);
            }
            assert_eq!(p.context.status, STATUS_YIELD);
            assert_eq!(
                p.context
                    .base64_list_decode
                    .as_mut()
                    .unwrap()
                    .decoder_output_len(),
                match budget {
                    0 => 0,
                    1 => 6,
                    _ => 6144,
                }
            );
            p.resume_continuations = take_pending_resumes();
            while !p.resume_continuations.is_empty() {
                unsafe {
                    p.collect_garbage_full();
                    p.reductions = 4000;
                    p.run_slice();
                }
            }
            assert_eq!(p.context.status, STATUS_EXCEPTION);
            assert_eq!(
                p.context.result,
                ErlTerm::atom(crate::atom_table::ATOM_BADARG).0
            );
            assert!(p.context.base64_list_decode.is_none());
        }
        let mut p = Process::new(crate::process::ProcessId::new(83));
        let data = Arc::new(crate::binary::BinaryData::new(vec![42; 1000]));
        let owner = Arc::downgrade(&data);
        let binary = p.heap.alloc_refc_binary_from_arc(data, 0, 1000);
        let list = input(&mut p, 32768, &[binary]);
        p.context.heap_ptr = &mut p.heap;
        unsafe {
            nera_rt_base64_decode_list(&mut p.context, list.0, 0, yes);
        }
        p.resume_continuations = take_pending_resumes();
        assert!(p.context.base64_list_decode.is_some());
        drop(p);
        assert!(owner.upgrade().is_none());
        assert!(take_pending_resumes().is_empty());
    }
    #[test]
    fn base64_list_decoder_single_scheduler_observer_runs_before_completion() {
        use crate::scheduler::Scheduler;
        use std::sync::atomic::{AtomicUsize, Ordering};
        extern "C" fn entry(ctx: *mut ProcessContext, args: *const u64, _: u32) {
            unsafe {
                (*ctx).result = nera_rt_base64_decode_list(
                    ctx,
                    *args,
                    0,
                    ErlTerm::atom(crate::atom_table::ATOM_TRUE).0,
                );
            }
        }
        let scheduler = Scheduler::new(1);
        let mut producer = Process::with_entry(scheduler.next_pid(), entry, Vec::new());
        let data = input(&mut producer, 262144, &[]);
        producer.initial_args.push(data);
        let pid = scheduler.spawn_process(producer);
        let process = scheduler.get_process(pid).unwrap();
        let progress = Arc::new(AtomicUsize::new(0));
        let observed = progress.clone();
        let mut observer = Process::new(scheduler.next_pid());
        observer.rust_fn = Some(Box::new(move |inner| {
            let mut producer = process.lock_for_runtime();
            if let Some(state) = &mut producer.context.base64_list_decode {
                observed.store(state.decoder_output_len(), Ordering::SeqCst);
            }
            inner.finish();
        }));
        scheduler.spawn_process(observer);
        assert!(unsafe { scheduler.run() });
        let len = progress.load(Ordering::SeqCst);
        assert!(len > 0 && len < 196608, "progress={len}");
    }
    #[test]
    fn charges_empty_final_and_whitespace_chunks() {
        let mut p = Process::new(crate::process::ProcessId::new(84));
        p.context.heap_ptr = &mut p.heap;
        let yes = ErlTerm::atom(crate::atom_table::ATOM_TRUE).0;
        for (len, cost) in [(0, 1), (4, 1), (8, 1), (12, 2), (8192, 1024)] {
            let data = input(&mut p, len, &[]);
            p.context.reductions = 4000;
            unsafe {
                nera_rt_base64_decode_list(&mut p.context, data.0, 0, yes);
            }
            assert_eq!(p.context.status, STATUS_OK);
            assert_eq!(p.context.reductions, 4000 - cost, "length {len}");
        }
        let spaces = p
            .heap
            .alloc_list_from_slice(&vec![ErlTerm::small_int(32); 8193]);
        p.context.reductions = 4000;
        unsafe {
            nera_rt_base64_decode_list(&mut p.context, spaces.0, 0, yes);
        }
        assert_eq!(p.context.status, STATUS_YIELD);
        let continuations = take_pending_resumes();
        assert_eq!(continuations.len(), 1);
        let cont = &continuations[0];
        let args: Vec<_> = cont.args.iter().map(|t| t.0).collect();
        p.context.reductions = 4000;
        p.context.status = STATUS_OK;
        // SAFETY: the context and argument storage are live and exclusively
        // accessible here; no heap borrow crosses this reentry boundary.
        unsafe { (cont.func)(&mut p.context, args.as_ptr(), args.len() as u32); }
        assert_eq!(p.context.status, STATUS_OK);
        assert_eq!(p.context.reductions, 3999);
        assert!(p.context.base64_list_decode.is_none());
    }
}
