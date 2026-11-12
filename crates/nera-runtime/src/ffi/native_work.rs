//! Shared adapter for cooperative binary BIFs.
use super::helpers::{binary_data, raise_error, with_ctx_heap};
use super::receive::nera_rt_set_resume;
use crate::{
    binary::BinaryData,
    native_work::{budget, BinaryWork, NativeValue, WorkError},
    process::{ProcessContext, STATUS_YIELD},
    term::ErlTerm,
};
use std::sync::Arc;

unsafe fn step(
    ctx: *mut ProcessContext,
    work: &mut dyn BinaryWork,
    input: u64,
) -> Result<bool, WorkError> {
    let limit = budget(work, (*ctx).reductions);
    if limit == 0 {
        return Ok(false);
    }
    let (ptr, len) = binary_data(ErlTerm(input)).unwrap();
    let before = work.offset();
    // No GC or calls into compiled code while this temporary slice exists.
    let done = work.step(std::slice::from_raw_parts(ptr, len), limit);
    let consumed = work.offset() - before;
    assert!(consumed <= limit);
    let cost = consumed.div_ceil(work.bytes_per_reduction()).max(1) as i64;
    (*ctx).reductions = (*ctx).reductions.saturating_sub(cost);
    done
}

pub(super) unsafe fn start<W: BinaryWork + 'static>(
    ctx: *mut ProcessContext,
    input: u64,
    mut work: W,
) -> u64 {
    match step(ctx, &mut work, input) {
        Ok(true) => finish(ctx, std::mem::take(work.output())),
        Ok(false) => {
            (*ctx).native_work = Some(Box::new(work));
            suspend(ctx, input)
        }
        Err(error) => fail(ctx, error),
    }
}

unsafe fn suspend(ctx: *mut ProcessContext, input: u64) -> u64 {
    nera_rt_set_resume(ctx, resume, &input, 1, 0);
    (*ctx).reductions = 0;
    (*ctx).status = STATUS_YIELD;
    ErlTerm::nil().0
}

extern "C" fn resume(ctx: *mut ProcessContext, args: *const u64, _: u32) {
    // Input is rooted by the continuation, including across GC/worker migration.
    unsafe {
        let input = *args;
        let mut work = (*ctx).native_work.take().unwrap();
        match step(ctx, work.as_mut(), input) {
            Ok(true) => (*ctx).result = finish(ctx, std::mem::take(work.output())),
            Ok(false) => {
                (*ctx).native_work = Some(work);
                (*ctx).result = suspend(ctx, input);
            }
            Err(error) => {
                fail(ctx, error);
            }
        }
    }
}

pub(super) unsafe fn finish(ctx: *mut ProcessContext, output: Vec<u8>) -> u64 {
    with_ctx_heap(ctx, |heap| {
        if output.len() <= 64 {
            heap.alloc_binary(&output).0
        } else {
            let len = output.len();
            heap.alloc_refc_binary_from_arc(Arc::new(BinaryData::new(output)), 0, len)
                .0
        }
    })
}

pub(super) unsafe fn fail(ctx: *mut ProcessContext, error: WorkError) -> u64 {
    use crate::atom_table::global_atom_table;
    let atom = |name| ErlTerm::atom(global_atom_table().intern(name));
    let reason = match error {
        WorkError::Atom(name) => atom(name),
        WorkError::CaseClause(value) => {
            let value = match value {
                NativeValue::Atom(name) => atom(name),
                NativeValue::Integer(n) => ErlTerm::small_int(n),
            };
            with_ctx_heap(ctx, |heap| heap.alloc_tuple(&[atom("case_clause"), value]))
        }
    };
    raise_error(ctx, reason.0)
}
