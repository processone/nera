//! Checked, cooperative binary:copy/2. The shared adapter roots the input
//! across suspension and adopts the completed output without another copy.
use super::super::{helpers::*, native_work};
use crate::{native_work::BinaryWork, process::ProcessContext, term::*};

const BYTES_PER_REDUCTION: usize = 64;
const CHUNK_BYTES: usize = 64 * 1024;

/// OTP accepts an unsigned machine-word count, including boxed integers, but
/// rejects larger integers even when the subject is empty.
/// # Safety
/// The count is a live, initialized, immutable and unmoved term.
unsafe fn repetitions(term: ErlTerm) -> Option<usize> {
    if let Some(n) = term.as_small_int() {
        return usize::try_from(n).ok();
    }
    let ptr = term.as_boxed_ptr()?;
    let header = BoxedHeader(*ptr);
    if header.subtype() != BOXED_BIGINT
        || header.arity() != 1
        || *ptr.add(BIGINT_SIGN_WORD_INDEX) != BIGINT_SIGN_NON_NEGATIVE
    {
        return None;
    }
    usize::try_from(*ptr.add(BIGINT_FIRST_LIMB_WORD_INDEX)).ok()
}

fn result_len(input_len: usize, count: usize) -> Option<usize> {
    let len = input_len.checked_mul(count)?;
    // Refc binary descriptors encode the bit length in the boxed header.
    let bits = u64::try_from(len).ok()?.checked_mul(8)?;
    (bits <= u64::MAX >> BOXED_TAG_BITS).then_some(len)
}

struct Copy {
    output: Vec<u8>,
    len: usize,
}

impl Copy {
    fn new(input_len: usize, count: usize) -> Option<Self> {
        let len = result_len(input_len, count)?;
        let mut output = Vec::new();
        // Capacity overflow and allocator refusal must become Erlang errors,
        // not panics/aborts at the extern "C" boundary. Allocation itself is
        // synchronous; a successful reservation is not a physical-memory bound.
        output.try_reserve_exact(len).ok()?;
        Some(Self { output, len })
    }
}

impl BinaryWork for Copy {
    fn bytes_per_reduction(&self) -> usize {
        BYTES_PER_REDUCTION
    }
    fn max_bytes(&self) -> usize {
        CHUNK_BYTES
    }
    fn offset(&self) -> usize {
        self.output.len()
    }
    fn output(&mut self) -> &mut Vec<u8> {
        &mut self.output
    }
    fn step(&mut self, input: &[u8], limit: usize) -> Result<bool, crate::native_work::WorkError> {
        let end = self.output.len() + limit.min(self.len - self.output.len());
        while self.output.len() < end {
            let offset = self.output.len() % input.len();
            let count = (end - self.output.len()).min(input.len() - offset);
            self.output
                .extend_from_slice(&input[offset..offset + count]);
        }
        Ok(self.output.len() == self.len)
    }
}

/// Repeat a byte-aligned binary, validating both arguments before empty-result
/// shortcuts. Work is charged by output bytes, not the (possibly tiny) input.
/// # Safety
/// Generated code supplies an exclusive live context and live, unmoved terms.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_binary_copy_2(
    ctx: *mut ProcessContext,
    subject: u64,
    count: u64,
) -> u64 {
    if !ErlTerm(subject).is_binary() {
        return raise_badarg(ctx);
    }
    let Some(count) = repetitions(ErlTerm(count)) else {
        return raise_badarg(ctx);
    };
    let (_, len) = binary_data(ErlTerm(subject)).unwrap();
    let Some(work) = Copy::new(len, count) else {
        return raise_error(ctx, ErlTerm::atom(crate::atom_table::ATOM_SYSTEM_LIMIT).0);
    };
    native_work::start(ctx, subject, work)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        binary::BinaryData,
        native_work::budget,
        process::{Process, ProcessId, STATUS_OK, STATUS_YIELD},
    };
    use std::sync::Arc;

    // Intent: oversized copies must raise rather than overflow or truncate.
    // These are actual request sizes; no large input allocation is needed.
    #[test]
    fn request_size_checks_multiplication_and_binary_header() {
        assert_eq!(result_len(b"123456789".len(), SMALL_INT_MAX as usize), None);
        assert_eq!(
            result_len(b"0123456789abcdef0".len(), SMALL_INT_MAX as usize),
            None
        );
        let max_bytes = ((u64::MAX >> BOXED_TAG_BITS) / 8) as usize;
        assert_eq!(result_len(b"x".len(), max_bytes), Some(max_bytes));
        assert_eq!(result_len(b"x".len(), max_bytes + 1), None);
    }

    // Intent: every copy, including its final/empty slice, pays for output work.
    #[test]
    fn charges_output_and_obeys_remaining_reductions() {
        let mut process = Process::new(ProcessId::new(1));
        process.context.heap_ptr = &mut process.heap;
        let input = process.heap.alloc_binary(b"x");
        for len in [
            0,
            1,
            BYTES_PER_REDUCTION,
            BYTES_PER_REDUCTION + 1,
            CHUNK_BYTES,
        ] {
            process.context.reductions = 4000;
            unsafe {
                nera_rt_binary_copy_2(
                    &mut process.context,
                    input.0,
                    ErlTerm::small_int(len as i64).0,
                );
            }
            assert_eq!(process.context.status, STATUS_OK);
            let cost = len.div_ceil(BYTES_PER_REDUCTION).max(1) as i64;
            assert_eq!(process.context.reductions, 4000 - cost);
        }
        for remaining in [0, 1] {
            let mut work = Copy::new(1, CHUNK_BYTES + 1).unwrap();
            let limit = budget(&work, remaining);
            assert_eq!(limit, remaining as usize * BYTES_PER_REDUCTION);
            work.step(b"x", limit).unwrap();
            assert_eq!(work.offset(), limit);
        }
    }

    // Intent: suspended copying preserves input roots, partial repetition offsets
    // and caller state through full GC, then adopts (rather than recopies) output.
    #[test]
    fn suspended_copy_survives_gc_and_adopts_output() {
        let mut process = Process::new(ProcessId::new(2));
        let bytes: Vec<u8> = (0..=255).cycle().take(CHUNK_BYTES + 19).collect();
        let data = Arc::new(BinaryData::new(
            [b"prefix".as_slice(), &bytes, b"suffix"].concat(),
        ));
        let owner = Arc::downgrade(&data);
        let input = process
            .heap
            .alloc_refc_binary_from_arc(data, b"prefix".len(), bytes.len());
        process.context.heap_ptr = &mut process.heap;
        process.context.reductions = 1;
        unsafe {
            nera_rt_binary_copy_2(&mut process.context, input.0, ErlTerm::small_int(3).0);
        }
        assert_eq!(process.context.status, STATUS_YIELD);
        let work = process.context.native_work.as_mut().unwrap();
        assert_eq!(work.offset(), BYTES_PER_REDUCTION);
        let output_ptr = work.output().as_ptr();
        process.resume_continuations = super::super::super::receive::take_pending_resumes();
        let mut slices = 0;
        while !process.resume_continuations.is_empty() {
            unsafe {
                process.collect_garbage_full();
                process.reductions = 4000;
                process.run_slice();
            }
            assert!(owner.upgrade().is_some());
            slices += 1;
            assert!(slices <= (bytes.len() * 3).div_ceil(CHUNK_BYTES));
        }
        assert_eq!(process.context.status, STATUS_OK);
        assert!(process.context.native_work.is_none());
        let (ptr, len) = unsafe { binary_data(ErlTerm(process.context.result)).unwrap() };
        assert_eq!(ptr, output_ptr);
        assert_eq!(
            unsafe { std::slice::from_raw_parts(ptr, len) },
            bytes.repeat(3)
        );
        let last_bytes = (len - BYTES_PER_REDUCTION) % CHUNK_BYTES;
        assert_eq!(
            process.context.reductions,
            4000 - last_bytes.div_ceil(BYTES_PER_REDUCTION) as i64
        );
        drop(process);
        assert!(owner.upgrade().is_none());
    }

    // Intent: a second process must run during a single large copy, even with
    // only one worker. Observe actual copy progress, not a timing threshold.
    #[test]
    fn single_worker_observes_partial_copy() {
        use crate::scheduler::Scheduler;
        use std::sync::atomic::{AtomicUsize, Ordering};
        extern "C" fn entry(ctx: *mut ProcessContext, args: *const u64, _: u32) {
            unsafe {
                (*ctx).result = nera_rt_binary_copy_2(ctx, *args, *args.add(1));
            }
        }
        let scheduler = Scheduler::new(1);
        let len = 4 * 1024 * 1024;
        let mut producer = Process::with_entry(scheduler.next_pid(), entry, Vec::new());
        let input = producer.heap.alloc_binary(b"x");
        producer.initial_args = vec![input, ErlTerm::small_int(len as i64)];
        let pid = scheduler.spawn_process(producer);
        let entry = scheduler.get_process(pid).unwrap();
        let progress = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&progress);
        let mut observer = Process::new(scheduler.next_pid());
        observer.rust_fn = Some(Box::new(move |inner| {
            let producer = entry.lock_for_runtime();
            if let Some(work) = &producer.context.native_work {
                observed.store(work.offset(), Ordering::SeqCst);
            }
            inner.finish();
        }));
        scheduler.spawn_process(observer);
        assert!(unsafe { scheduler.run() });
        let copied = progress.load(Ordering::SeqCst);
        eprintln!("observer ran after {copied}/{len} output bytes");
        assert!(copied > 0 && copied < len);
    }

    // Intent: cancellation must release native work and its only input root.
    #[test]
    fn dropping_suspended_copy_releases_input() {
        let mut process = Process::new(ProcessId::new(3));
        let data = Arc::new(BinaryData::new(b"repeated source".repeat(100)));
        let owner = Arc::downgrade(&data);
        let input =
            process
                .heap
                .alloc_refc_binary_from_arc(data, 0, b"repeated source".len() * 100);
        process.context.heap_ptr = &mut process.heap;
        process.context.reductions = 0;
        unsafe {
            nera_rt_binary_copy_2(&mut process.context, input.0, ErlTerm::small_int(100).0);
        }
        assert_eq!(process.context.status, STATUS_YIELD);
        assert_eq!(process.context.native_work.as_ref().unwrap().offset(), 0);
        process.resume_continuations = super::super::super::receive::take_pending_resumes();
        drop(process);
        assert!(owner.upgrade().is_none());
    }
}
