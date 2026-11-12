//! IR terminator translation to Cranelift IR.

use std::collections::HashMap;

use cranelift_codegen::ir::{types, InstBuilder, MemFlags, Value};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::Module;
use cranelift_object::ObjectModule;

use nera_ir::ir::{BlockId, CallTarget, Literal, Terminator, VarId};
use crate::term_layout::*;
use super::{ModuleContext, declare_runtime_func, FuncTranslator, TranslateError};

impl FuncTranslator {
    #[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
    pub(super) fn translate_terminator(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        term: &Terminator,
        var_map: &HashMap<VarId, Variable>,
        block_map: &HashMap<BlockId, cranelift_codegen::ir::Block>,
        ctx_var: Variable,
        mod_ctx: &ModuleContext,
        current_func_name: Option<&nera_syntax::ast::AtomId>,
        param_vars: &[VarId],
        loop_block: Option<cranelift_codegen::ir::Block>,
    ) -> Result<(), TranslateError> {
        let last_predicate = self.last_branch_predicate.take();
        match term {
            Terminator::Return(var) => {
                // Sync reds_var to ctx.reductions before returning.
                self.sync_reds_to_ctx(builder, ctx_var);
                let val = builder.use_var(var_map[var]);
                self.emit_stack_pop_and_return(module, builder, ctx_var, val)?;
            }

            Terminator::Jump(target, _args) => {
                if let Some(cl_block) = block_map.get(target) {
                    builder.ins().jump(*cl_block, &[]);
                } else {
                    let nil = builder.ins().iconst(types::I64, NIL_VALUE as i64);
                    self.emit_stack_pop_and_return(module, builder, ctx_var, nil)?;
                }
            }

            Terminator::Branch(cond, then_block, else_block) => {
                let is_true = if let Some((_, predicate)) = last_predicate.filter(|(var, _)| var == cond) {
                    predicate
                } else {
                    let cond_val = builder.use_var(var_map[cond]);
                    let true_val = self.load_true_atom(module, builder)?;
                    builder.ins().icmp(IntCC::Equal, cond_val, true_val)
                };

                let then_cl = block_map[then_block];
                let else_cl = block_map[else_block];
                builder.ins().brif(is_true, then_cl, &[], else_cl, &[]);
            }

            Terminator::Switch {
                scrutinee,
                cases,
                default,
            } => {
                let scr_val = builder.use_var(var_map[scrutinee]);
                let default_cl = block_map[default];

                let mut current_else = default_cl;
                for (lit, target) in cases.iter().rev() {
                    let target_cl = block_map[target];
                    let check_block = builder.create_block();

                    let expected = match lit {
                        Literal::Integer(n) => {
                            if fits_small_int(*n) {
                                builder.ins().iconst(types::I64, encode_small_int(*n) as i64)
                            } else {
                                // Value exceeds small int range — allocate bigint at runtime
                                let fid = declare_runtime_func(module, "nera_rt_alloc_bigint_i64", 1, &mut self.runtime_funcs, self.c_call_conv)?;
                                let func_ref = module.declare_func_in_func(fid, builder.func);
                                let ctx_val = builder.use_var(ctx_var);
                                let raw_val = builder.ins().iconst(types::I64, *n);
                                let call = builder.ins().call(func_ref, &[ctx_val, raw_val]);
                                builder.inst_results(call)[0]
                            }
                        }
                        Literal::BigInteger(n) => self.load_big_integer(module, builder, ctx_var, n)?,
                        Literal::Atom(id) => {
                            self.load_atom_term(module, builder, id.0)?
                        }
                        _ => builder.ins().iconst(types::I64, 0i64),
                    };

                    // For bigint literals, use FFI comparison for correctness
                    let is_bigint_literal = matches!(lit, Literal::BigInteger(_))
                        || matches!(lit, Literal::Integer(n) if !fits_small_int(*n));
                    let cmp = if is_bigint_literal {
                        let eq_result = self.emit_ffi_call(module, builder, "nera_rt_exact_eq", ctx_var, &[scr_val, expected])?;
                        let true_atom = self.load_true_atom(module, builder)?;
                        builder.ins().icmp(IntCC::Equal, eq_result, true_atom)
                    } else {
                        builder.ins().icmp(IntCC::Equal, scr_val, expected)
                    };
                    builder.ins().brif(cmp, target_cl, &[], current_else, &[]);

                    current_else = check_block;
                }
            }

            Terminator::TailCall(target, args) => {
                // Check for self-tail-call: optimize to loop.
                // Must match BOTH name AND arity to avoid confusing
                // e.g. reverse/1 calling reverse/2 as a self-call.
                let is_self_call = match target {
                    CallTarget::Local(name) => {
                        (current_func_name == Some(name))
                            && args.len() == param_vars.len()
                    }
                    _ => false,
                };

                if let Some(loop_block) = loop_block.filter(|_| is_self_call) {
                    // Self-tail-call: reassign parameters and jump to loop block.
                    // After reassignment, check if reductions are exhausted.
                    // If so, save current params and yield instead of looping.
                    let arg_vals: Vec<Value> = args.iter().map(|a| builder.use_var(var_map[a])).collect();
                    for (param, val) in param_vars.iter().zip(arg_vals.iter()) {
                        if let Some(var) = var_map.get(param) {
                            builder.def_var(*var, *val);
                        }
                    }

                    // Decrement and check reductions using reds_var (register-only).
                    // No memory load/store in the hot path — pure register ops.
                    let reds_v = self.reds_var.unwrap();
                    let reds = builder.use_var(reds_v);
                    let one = builder.ins().iconst(types::I64, 1);
                    let new_reds = builder.ins().isub(reds, one);
                    builder.def_var(reds_v, new_reds);
                    let zero = builder.ins().iconst(types::I64, 0);
                    let exhausted = builder.ins().icmp(IntCC::SignedLessThanOrEqual, new_reds, zero);

                    let yield_block = builder.create_block();
                    builder.set_cold_block(yield_block);
                    builder.ins().brif(exhausted, yield_block, &[], loop_block, &[]);

                    // --- Yield block: mark STATUS_YIELD, save continuation, return ---
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

                    let saved_recv_idx = self.current_receive_index;
                    self.current_receive_index = 0;
                    self.emit_set_resume(module, builder, var_map, ctx_var)?;
                    self.current_receive_index = saved_recv_idx;

                    let nil = builder.ins().iconst(types::I64, NIL_VALUE as i64);
                    self.emit_stack_pop_and_return(module, builder, ctx_var, nil)?;
                } else {
                    // Non-self tail call: true TCO via return_call for local targets
                    // Sync reds_var to ctx.reductions before the tail call.
                    self.sync_reds_to_ctx(builder, ctx_var);
                    let arg_vals: Vec<Value> = args.iter().map(|a| builder.use_var(var_map[a])).collect();

                    match target {
                        CallTarget::Local(name) => {
                            if let Some(mangled) = mod_ctx.module_funcs.get(&(name.0, arg_vals.len() as u32)) {
                                if let Some(&fid) = mod_ctx.func_ids.get(mangled) {
                                    // Pop stack trace BEFORE the tail call
                                    self.emit_stack_pop(module, builder, ctx_var)?;
                                    let func_ref = module.declare_func_in_func(fid, builder.func);
                                    let ctx_val = builder.use_var(ctx_var);
                                    let mut call_args = vec![ctx_val];
                                    call_args.extend_from_slice(&arg_vals);
                                    builder.ins().return_call(func_ref, &call_args);
                                } else {
                                    // Function not found: fallback to nil return
                                    let nil = builder.ins().iconst(types::I64, NIL_VALUE as i64);
                                    self.emit_stack_pop_and_return(module, builder, ctx_var, nil)?;
                                }
                            } else {
                                let nil = builder.ins().iconst(types::I64, NIL_VALUE as i64);
                                self.emit_stack_pop_and_return(module, builder, ctx_var, nil)?;
                            }
                        }
                        CallTarget::Remote(mod_atom, fun_atom) => {
                            // Remote tail call: try return_call for co-compiled targets
                            let mod_id = mod_atom.0;
                            let fun_id = fun_atom.0;
                            let arity = arg_vals.len() as u32;

                            let mod_name = self.resolve_atom(mod_id).map(|s| s.to_string());
                            let fun_name = self.resolve_atom(fun_id).map(|s| s.to_string());
                            let is_ffi = nera_ir::bif_table::lookup_ffi_symbol(
                                mod_name.as_deref().unwrap_or(""),
                                fun_name.as_deref().unwrap_or(""),
                                arity as usize,
                            ).is_some();

                            if !is_ffi {
                                // Check intra-module qualified calls (e.g. maps:map from within maps.erl)
                                let local_func = mod_ctx.module_funcs.get(&(fun_id, arity))
                                    .and_then(|mangled| mod_ctx.func_ids.get(mangled));
                                let xmod_func = mod_ctx.cross_module_funcs.get(&(mod_id, fun_id, arity));
                                if let Some(&fid) = local_func.filter(|_| mod_id == mod_ctx.module_name_id).or(xmod_func) {
                                    // Known function (local or cross-module): true TCO via return_call
                                    self.emit_stack_pop(module, builder, ctx_var)?;
                                    let func_ref = module.declare_func_in_func(fid, builder.func);
                                    let ctx_val = builder.use_var(ctx_var);
                                    let mut call_args = vec![ctx_val];
                                    call_args.extend_from_slice(&arg_vals);
                                    builder.ins().return_call(func_ref, &call_args);
                                } else {
                                    // Unknown remote: trampoline via STATUS_TAIL_CALL
                                    self.emit_tail_call_trampoline_apply(module, builder, ctx_var, mod_id, fun_id, &arg_vals)?;
                                }
                            } else {
                                // FFI/BIF: different calling convention, cannot return_call
                                let ctx_val = builder.use_var(ctx_var);
                                let ret = self.emit_remote_call(module, builder, mod_id, fun_id, ctx_val, &arg_vals, mod_ctx)?;
                                self.emit_stack_pop_and_return(module, builder, ctx_var, ret)?;
                            }
                        }
                        CallTarget::Bif(_) => {
                            // BIF: different calling convention, regular call + return
                            let ctx_val = builder.use_var(ctx_var);
                            let ret = self.emit_call(module, builder, target, ctx_val, ctx_var, &arg_vals, var_map, mod_ctx)?;
                            self.emit_stack_pop_and_return(module, builder, ctx_var, ret)?;
                        }
                        CallTarget::Apply(mod_var, fun_var) => {
                            // Dynamic apply: trampoline via STATUS_TAIL_CALL
                            let mod_val = builder.use_var(var_map[mod_var]);
                            let fun_val = builder.use_var(var_map[fun_var]);
                            self.emit_tail_call_trampoline_apply_vals(module, builder, ctx_var, mod_val, fun_val, &arg_vals)?;
                        }
                        CallTarget::Closure(var_id) => {
                            // Closure: trampoline via STATUS_TAIL_CALL
                            let closure_val = builder.use_var(var_map[var_id]);
                            self.emit_tail_call_trampoline_closure(module, builder, ctx_var, closure_val, &arg_vals)?;
                        }
                    }
                }
            }

            Terminator::Receive { .. } => {
                // Receive requires scheduler integration — return nil for now
                self.sync_reds_to_ctx(builder, ctx_var);
                let nil = builder.ins().iconst(types::I64, NIL_VALUE as i64);
                self.emit_stack_pop_and_return(module, builder, ctx_var, nil)?;
            }

            Terminator::Unreachable => {
                builder.ins().trap(cranelift_codegen::ir::TrapCode::user(1).unwrap());
            }
        }

        Ok(())
    }

    /// Emit trampoline signal for apply tail call (atom IDs known at compile time).
    /// Builds an Erlang args list on the heap, stores target info in ctx, sets STATUS_TAIL_CALL.
    fn emit_tail_call_trampoline_apply(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        mod_id: u32,
        fun_id: u32,
        arg_vals: &[Value],
    ) -> Result<(), TranslateError> {
        let mod_val = self.load_atom_term(module, builder, mod_id)?;
        let fun_val = self.load_atom_term(module, builder, fun_id)?;
        self.emit_tail_call_trampoline_apply_vals(module, builder, ctx_var, mod_val, fun_val, arg_vals)
    }

    /// Emit trampoline signal for apply tail call (mod/fun as Cranelift Values).
    fn emit_tail_call_trampoline_apply_vals(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        mod_val: Value,
        fun_val: Value,
        arg_vals: &[Value],
    ) -> Result<(), TranslateError> {
        let ctx_val = builder.use_var(ctx_var);
        // Build Erlang list from args (heap-allocated, survives return)
        let args_list = self.build_args_list(module, builder, ctx_var, arg_vals)?;
        // Store tail call request in ProcessContext
        let kind = builder.ins().iconst(types::I32, 1i64); // 1 = apply
        builder.ins().store(MemFlags::trusted(), kind, ctx_val, CTX_OFFSET_TAIL_CALL_KIND);
        builder.ins().store(MemFlags::trusted(), mod_val, ctx_val, CTX_OFFSET_TAIL_CALL_MOD);
        builder.ins().store(MemFlags::trusted(), fun_val, ctx_val, CTX_OFFSET_TAIL_CALL_FUN);
        builder.ins().store(MemFlags::trusted(), args_list, ctx_val, CTX_OFFSET_TAIL_CALL_ARG1);
        // Set STATUS_TAIL_CALL
        let tc_status = builder.ins().iconst(types::I32, STATUS_TAIL_CALL as i64);
        builder.ins().store(MemFlags::trusted(), tc_status, ctx_val, CTX_OFFSET_STATUS);
        // Pop stack frame and return
        let nil = builder.ins().iconst(types::I64, NIL_VALUE as i64);
        self.emit_stack_pop_and_return(module, builder, ctx_var, nil)
    }

    /// Emit trampoline signal for closure tail call.
    fn emit_tail_call_trampoline_closure(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        closure_val: Value,
        arg_vals: &[Value],
    ) -> Result<(), TranslateError> {
        let ctx_val = builder.use_var(ctx_var);
        // Build Erlang list from args
        let args_list = self.build_args_list(module, builder, ctx_var, arg_vals)?;
        // Store tail call request
        let kind = builder.ins().iconst(types::I32, 2i64); // 2 = closure
        builder.ins().store(MemFlags::trusted(), kind, ctx_val, CTX_OFFSET_TAIL_CALL_KIND);
        builder.ins().store(MemFlags::trusted(), closure_val, ctx_val, CTX_OFFSET_TAIL_CALL_ARG1);
        builder.ins().store(MemFlags::trusted(), args_list, ctx_val, CTX_OFFSET_TAIL_CALL_ARG2);
        // Set STATUS_TAIL_CALL
        let tc_status = builder.ins().iconst(types::I32, STATUS_TAIL_CALL as i64);
        builder.ins().store(MemFlags::trusted(), tc_status, ctx_val, CTX_OFFSET_STATUS);
        let nil = builder.ins().iconst(types::I64, NIL_VALUE as i64);
        self.emit_stack_pop_and_return(module, builder, ctx_var, nil)
    }

    /// Build an Erlang list (cons chain) from Cranelift Values, on the process heap.
    fn build_args_list(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        arg_vals: &[Value],
    ) -> Result<Value, TranslateError> {
        let cons_fid = declare_runtime_func(
            module, "nera_rt_alloc_cons", 2, &mut self.runtime_funcs, self.c_call_conv,
        )?;
        let cons_ref = module.declare_func_in_func(cons_fid, builder.func);
        let ctx_val = builder.use_var(ctx_var);
        let mut list = builder.ins().iconst(types::I64, NIL_VALUE as i64);
        for arg in arg_vals.iter().rev() {
            let call = builder.ins().call(cons_ref, &[ctx_val, *arg, list]);
            list = builder.inst_results(call)[0];
        }
        Ok(list)
    }
}
