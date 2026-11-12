//! Suspend/resume mechanism: set_resume, call-site status checks, yield checks.
//!
//! # Architectural constraint: pure-call optimization is NOT feasible
//!
//! We investigated eliminating post-call status checks for "pure" functions
//! (those that never suspend — no receive, yield, remote/closure/apply calls).
//! The hypothesis was that pure calls can skip the full status check (which
//! includes `emit_set_resume` with `use_var()` on all live variables) since
//! they can only return normally or raise an exception.
//!
//! **This optimization does not work** due to a fundamental interaction between
//! Cranelift's `CallConv::Tail` register allocation and variable liveness:
//!
//! - `emit_set_resume()` in the bailout block of `emit_call_site_status_check()`
//!   calls `use_var()` on all live-across variables. Even though the bailout
//!   path is almost never taken, these `use_var()` calls are essential: they
//!   signal to Cranelift's register allocator that these variables must be
//!   preserved across the preceding function call.
//!
//! - Under `CallConv::Tail`, without these `use_var()` references, Cranelift
//!   does not properly save/restore caller variables across calls. This causes
//!   silent data corruption (wrong values after call returns) in programs that
//!   mix pure and non-pure calls within the same function.
//!
//! - The bug manifested as `lists:flatmap` returning truncated results at
//!   N>=300 elements. It was confirmed that ANY simplified status check (even
//!   an unconditional jump) that omits `emit_set_resume` triggers the bug.
//!   Delegating to the full `emit_call_site_status_check` fixed it.
//!
//! **DO NOT attempt to re-implement pure-call status check elimination**
//! without first solving the Cranelift variable liveness problem (e.g., by
//! switching away from `CallConv::Tail` or by inserting explicit `use_var()`
//! keep-alive references without the full resume machinery).

use std::collections::HashMap;

use cranelift_codegen::ir::{types, AbiParam, InstBuilder, MemFlags, Signature};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;

use nera_ir::ir::VarId;
use crate::term_layout::*;
use super::{FuncTranslator, TranslateError};

impl FuncTranslator {
    pub(super) fn emit_set_resume(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        var_map: &HashMap<VarId, Variable>,
        ctx_var: Variable,
    ) -> Result<(), TranslateError> {
        let wrapper_id = match self.current_wrapper_func_id {
            Some(id) => id,
            None => return Ok(()), // No wrapper — nothing to do
        };

        // Save live-across variables BEFORE calling set_resume, so the
        // runtime can attach them to the continuation.
        // Resume index 0 means "resume from function entry", so there is no
        // suspend-point-specific live-variable set to preserve.
        let live_vars: Vec<VarId> = if self.current_receive_index == 0 {
            Vec::new()
        } else {
            let recv_0idx = (self.current_receive_index - 1) as usize;
            if recv_0idx < self.current_live_across_vars.len() {
                self.current_live_across_vars[recv_0idx].clone()
            } else {
                Vec::new()
            }
        };

        if !live_vars.is_empty() {
            // Declare save_resume_vars: (ctx: i64, vars: i64, nvars: i32)
            let save_vars_id = {
                let name = "nera_rt_save_resume_vars";
                if let Some(id) = self.runtime_funcs.get(name) {
                    *id
                } else {
                    let mut sig = Signature::new(self.c_call_conv);
                    sig.params.push(AbiParam::new(types::I64)); // ctx
                    sig.params.push(AbiParam::new(types::I64)); // vars ptr
                    sig.params.push(AbiParam::new(types::I32)); // nvars
                    let func_id = module
                        .declare_function(name, Linkage::Import, &sig)
                        .map_err(|e| TranslateError::Module(e.to_string()))?;
                    self.runtime_funcs.insert(name.to_string(), func_id);
                    func_id
                }
            };
            let save_vars_ref = module.declare_func_in_func(save_vars_id, builder.func);

            let ctx_val_save = builder.use_var(ctx_var);
            let nvars = live_vars.len();
            let vars_slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                (nvars * 8) as u32,
                3, // align to 8 bytes
            ));
            for (i, var_id) in live_vars.iter().enumerate() {
                if let Some(var) = var_map.get(var_id) {
                    let val = builder.use_var(*var);
                    builder.ins().stack_store(val, vars_slot, (i * 8) as i32);
                }
            }
            let vars_addr = builder.ins().stack_addr(types::I64, vars_slot, 0);
            let nvars_val = builder.ins().iconst(types::I32, nvars as i64);
            builder.ins().call(save_vars_ref, &[ctx_val_save, vars_addr, nvars_val]);
        }

        // Declare set_resume: (ctx: i64, func: i64, args: i64, nargs: i32, recv_idx: i32)
        let set_resume_id = {
            let name = "nera_rt_set_resume";
            if let Some(id) = self.runtime_funcs.get(name) {
                *id
            } else {
                let mut sig = Signature::new(self.c_call_conv);
                sig.params.push(AbiParam::new(types::I64)); // ctx
                sig.params.push(AbiParam::new(types::I64)); // func ptr
                sig.params.push(AbiParam::new(types::I64)); // args ptr
                sig.params.push(AbiParam::new(types::I32)); // nargs
                sig.params.push(AbiParam::new(types::I32)); // receive_index
                let func_id = module
                    .declare_function(name, Linkage::Import, &sig)
                    .map_err(|e| TranslateError::Module(e.to_string()))?;
                self.runtime_funcs.insert(name.to_string(), func_id);
                func_id
            }
        };

        let set_resume_ref = module.declare_func_in_func(set_resume_id, builder.func);

        // Get wrapper function address
        let wrapper_ref = module.declare_func_in_func(wrapper_id, builder.func);
        let wrapper_addr = builder.ins().func_addr(types::I64, wrapper_ref);

        let ctx_val = builder.use_var(ctx_var);
        let param_vars = self.current_param_vars.clone();
        let nargs = param_vars.len();
        let recv_idx_val = builder.ins().iconst(types::I32, self.current_receive_index as i64);

        if nargs == 0 {
            // No args: pass null pointer and 0
            let null_ptr = builder.ins().iconst(types::I64, 0);
            let nargs_val = builder.ins().iconst(types::I32, 0);
            builder.ins().call(set_resume_ref, &[ctx_val, wrapper_addr, null_ptr, nargs_val, recv_idx_val]);
        } else {
            // Create a stack slot for the parameter values
            let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                (nargs * 8) as u32,
                3, // align to 8 bytes
            ));

            for (i, param_var) in param_vars.iter().enumerate() {
                if let Some(var) = var_map.get(param_var) {
                    let val = builder.use_var(*var);
                    builder.ins().stack_store(val, slot, (i * 8) as i32);
                }
            }

            let slot_addr = builder.ins().stack_addr(types::I64, slot, 0);
            let nargs_val = builder.ins().iconst(types::I32, nargs as i64);
            builder.ins().call(set_resume_ref, &[ctx_val, wrapper_addr, slot_addr, nargs_val, recv_idx_val]);
        }

        Ok(())
    }

    /// After a non-tail local call, emit a check of ctx.status.
    /// If STATUS_OK: jump to the continuation block where normal flow continues.
    /// If STATUS_EXCEPTION: return immediately to propagate the exception.
    /// Any other status (YIELD, RECEIVE_WAIT, IO_WAIT, etc.): save caller
    /// state via set_resume with the call-site resume index, and return nil.
    ///
    /// `cs_idx` is the 0-based call-site index.
    /// `num_recv_points` is the number of receive points (call-site resume
    /// indices are offset by this amount).
    #[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
    pub(super) fn emit_call_site_status_check(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        var_map: &HashMap<VarId, Variable>,
        ctx_var: Variable,
        cs_idx: usize,
        num_recv_points: usize,
        call_cont_blocks: &[cranelift_codegen::ir::Block],
        is_try_protected: bool,
    ) -> Result<(), TranslateError> {
        // Only emit the full check if we have a wrapper (needed for set_resume)
        if self.current_wrapper_func_id.is_none() {
            if is_try_protected {
                // Inside a try block — don't propagate exceptions.
                // The CheckException instruction will handle them.
                builder.ins().jump(call_cont_blocks[cs_idx], &[]);
                builder.switch_to_block(call_cont_blocks[cs_idx]);
            } else {
                // No wrapper — can't set up a resume. Still check for exceptions.
                let ctx_val = builder.use_var(ctx_var);
                let status = builder.ins().load(
                    types::I32,
                    MemFlags::trusted(),
                    ctx_val,
                    CTX_OFFSET_STATUS,
                );
                let ok_const = builder.ins().iconst(types::I32, STATUS_OK as i64);
                let is_ok = builder.ins().icmp(IntCC::Equal, status, ok_const);
                let exc_block = builder.create_block();
                builder.set_cold_block(exc_block);
                builder.ins().brif(is_ok, call_cont_blocks[cs_idx], &[], exc_block, &[]);

                // Exception block: return nil (exception info is in ctx)
                builder.switch_to_block(exc_block);
                let nil_val = builder.ins().iconst(types::I64, NIL_VALUE as i64);
                self.emit_stack_pop_and_return(module, builder, ctx_var, nil_val)?;

                builder.switch_to_block(call_cont_blocks[cs_idx]);
            }
            return Ok(());
        }

        if is_try_protected {
            // Inside a try block — exceptions are handled by CheckException,
            // so both OK and EXCEPTION continue to the call_cont block.
            // Only suspend statuses (YIELD, RECEIVE_WAIT, IO_WAIT) bail out.
            let ctx_val = builder.use_var(ctx_var);
            let status = builder.ins().load(
                types::I32,
                MemFlags::trusted(),
                ctx_val,
                CTX_OFFSET_STATUS,
            );
            let ok_const = builder.ins().iconst(types::I32, STATUS_OK as i64);
            let is_ok = builder.ins().icmp(IntCC::Equal, status, ok_const);
            let check_exc_block = builder.create_block();

            builder.ins().brif(is_ok, call_cont_blocks[cs_idx], &[], check_exc_block, &[]);

            // Exception also continues (CheckException will catch it)
            builder.switch_to_block(check_exc_block);
            let exc_const = builder.ins().iconst(types::I32, STATUS_EXCEPTION as i64);
            let is_exc = builder.ins().icmp(IntCC::Equal, status, exc_const);
            let bailout_block = builder.create_block();
            builder.set_cold_block(bailout_block);
            builder.ins().brif(is_exc, call_cont_blocks[cs_idx], &[], bailout_block, &[]);

            // Bail-out block: save state and return (yield/receive_wait/io_wait)
            builder.switch_to_block(bailout_block);
            let saved_recv_idx = self.current_receive_index;
            self.current_receive_index = (num_recv_points + cs_idx + 1) as u32;
            self.emit_set_resume(module, builder, var_map, ctx_var)?;
            self.current_receive_index = saved_recv_idx;
            let nil_val = builder.ins().iconst(types::I64, NIL_VALUE as i64);
            self.emit_stack_pop_and_return(module, builder, ctx_var, nil_val)?;

            builder.switch_to_block(call_cont_blocks[cs_idx]);
            return Ok(());
        }

        // Check ctx.status: OK → continue, EXCEPTION → exc, else → bailout
        let ctx_val = builder.use_var(ctx_var);
        let status = builder.ins().load(
            types::I32,
            MemFlags::trusted(),
            ctx_val,
            CTX_OFFSET_STATUS,
        );

        // First check: status == OK → continue normally
        let ok_const = builder.ins().iconst(types::I32, STATUS_OK as i64);
        let is_ok = builder.ins().icmp(IntCC::Equal, status, ok_const);
        let not_ok_block = builder.create_block();
        builder.ins().brif(is_ok, call_cont_blocks[cs_idx], &[], not_ok_block, &[]);

        // === Not OK: distinguish exception from everything else ===
        builder.switch_to_block(not_ok_block);
        let exc_const = builder.ins().iconst(types::I32, STATUS_EXCEPTION as i64);
        let is_exc = builder.ins().icmp(IntCC::Equal, status, exc_const);

        let bailout_block = builder.create_block();   // yield/receive_wait/io_wait handler
        builder.set_cold_block(bailout_block);
        let exc_block = builder.create_block();        // exception handler
        builder.set_cold_block(exc_block);

        builder.ins().brif(is_exc, exc_block, &[], bailout_block, &[]);

        // === Bail-out block (yield/receive_wait/io_wait): save state and return ===
        builder.switch_to_block(bailout_block);

        // The resume index for this call-site is (num_recv_points + cs_idx + 1) (1-based)
        let saved_recv_idx = self.current_receive_index;
        self.current_receive_index = (num_recv_points + cs_idx + 1) as u32;

        // Save live-across variables and call set_resume
        self.emit_set_resume(module, builder, var_map, ctx_var)?;

        // Restore current_receive_index
        self.current_receive_index = saved_recv_idx;

        // Return nil (the actual result will come via ctx.result on resume)
        let nil_val = builder.ins().iconst(types::I64, NIL_VALUE as i64);
        self.emit_stack_pop_and_return(module, builder, ctx_var, nil_val)?;

        // === Exception block: propagate exception by returning immediately ===
        builder.switch_to_block(exc_block);
        let nil_val2 = builder.ins().iconst(types::I64, NIL_VALUE as i64);
        self.emit_stack_pop_and_return(module, builder, ctx_var, nil_val2)?;

        // === Continue in the continuation block ===
        builder.switch_to_block(call_cont_blocks[cs_idx]);

        Ok(())
    }

    /// After a YieldCheck instruction, decrement reductions using reds_var
    /// (register-only) and check if exhausted. If so, save state and yield.
    /// Otherwise, jump to the continuation block.
    ///
    /// `yc_idx` is the 0-based yield-check index.
    /// `num_recv_points` + `num_call_sites` are used to compute the resume index.
    #[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
    pub(super) fn emit_yield_check_status(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        var_map: &HashMap<VarId, Variable>,
        ctx_var: Variable,
        yc_idx: usize,
        num_recv_points: usize,
        num_call_sites: usize,
        yield_check_cont_blocks: &[cranelift_codegen::ir::Block],
    ) -> Result<(), TranslateError> {
        // If no wrapper, we can't set up a resume — just decrement and continue.
        if self.current_wrapper_func_id.is_none() {
            // Use reds_var (register) for decrement, no memory sync needed.
            let reds_v = self.reds_var.unwrap();
            let reds = builder.use_var(reds_v);
            let cost = builder.ins().iconst(types::I64, YIELD_CHECK_COST);
            let new_reds = builder.ins().isub(reds, cost);
            builder.def_var(reds_v, new_reds);
            builder.ins().jump(yield_check_cont_blocks[yc_idx], &[]);
            builder.switch_to_block(yield_check_cont_blocks[yc_idx]);
            return Ok(());
        }

        // Decrement reductions using reds_var (register-only in hot path).
        let reds_v = self.reds_var.unwrap();
        let reds = builder.use_var(reds_v);
        let cost = builder.ins().iconst(types::I64, YIELD_CHECK_COST);
        let new_reds = builder.ins().isub(reds, cost);
        builder.def_var(reds_v, new_reds);
        let zero = builder.ins().iconst(types::I64, 0);
        let exhausted = builder.ins().icmp(IntCC::SignedLessThanOrEqual, new_reds, zero);

        let yield_block = builder.create_block();
        builder.set_cold_block(yield_block);
        builder.ins().brif(exhausted, yield_block, &[], yield_check_cont_blocks[yc_idx], &[]);

        // --- Yield block: write reds_var back, set STATUS_YIELD, save continuation, return ---
        builder.switch_to_block(yield_block);
        // Write reds_var back to ctx.reductions before yielding.
        self.sync_reds_to_ctx(builder, ctx_var);
        let ctx_val2 = builder.use_var(ctx_var);
        let yield_status = builder.ins().iconst(types::I32, STATUS_YIELD as i64);
        builder.ins().store(
            MemFlags::trusted(),
            yield_status,
            ctx_val2,
            CTX_OFFSET_STATUS,
        );

        // The resume index for this yield check is
        // (num_recv_points + num_call_sites + yc_idx + 1) (1-based)
        let saved_recv_idx = self.current_receive_index;
        self.current_receive_index = (num_recv_points + num_call_sites + yc_idx + 1) as u32;
        self.emit_set_resume(module, builder, var_map, ctx_var)?;
        self.current_receive_index = saved_recv_idx;

        let nil_val = builder.ins().iconst(types::I64, NIL_VALUE as i64);
        self.emit_stack_pop_and_return(module, builder, ctx_var, nil_val)?;

        // --- Continue in the continuation block ---
        builder.switch_to_block(yield_check_cont_blocks[yc_idx]);

        Ok(())
    }
}
