//! IR instruction translation to Cranelift IR.

use std::collections::HashMap;

use cranelift_codegen::ir::{types, BlockArg, InstBuilder, MemFlags, Value};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::Module;
use cranelift_object::ObjectModule;

use nera_ir::ir::{CallTarget, Instruction, Literal, VarId};
use crate::term_layout::*;
use super::ffi_emit::SegmentSizeKind;
use super::{ModuleContext, declare_runtime_func,
            declare_tuple_element,
            FuncTranslator, TranslateError};

impl FuncTranslator {
    pub(super) fn translate_instruction(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        inst: &Instruction,
        var_map: &HashMap<VarId, Variable>,
        ctx_var: Variable,
        mod_ctx: &ModuleContext,
    ) -> Result<(), TranslateError> {
        self.last_branch_predicate = None;
        let protected_result = match inst {
            Instruction::Call(v, _, _) | Instruction::BinOp(v, _, _, _)
            | Instruction::UnaryOp(v, _, _) | Instruction::ListHead(v, _)
            | Instruction::ListTail(v, _) | Instruction::MapGet(v, _, _)
            | Instruction::MapPut(v, _, _, _) | Instruction::MapUpdateExact(v, _, _, _)
                | Instruction::MakeBinary(v, _)
                | Instruction::BinaryPutUtf8(v, _, _, _)
                | Instruction::BinaryPutUtf16(v, _, _, _, _)
                | Instruction::BinaryPutUtf32(v, _, _, _, _) => Some(v),
            _ => None,
        };
        let is_try_protected = protected_result.is_some_and(|v| self.current_try_protected_values.contains(v));
        match inst {
            Instruction::Const(result, lit) => {
                let val = match lit {
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
                    Literal::Float(f) => {
                        let fid = declare_runtime_func(module, "nera_rt_alloc_float", 1, &mut self.runtime_funcs, self.c_call_conv)?;
                        let func_ref = module.declare_func_in_func(fid, builder.func);
                        let ctx_val = builder.use_var(ctx_var);
                        let bits = builder.ins().iconst(types::I64, f.to_bits() as i64);
                        let call = builder.ins().call(func_ref, &[ctx_val, bits]);
                        builder.inst_results(call)[0]
                    }
                    Literal::Atom(id) => {
                        self.load_atom_term(module, builder, id.0)?
                    }
                    Literal::Nil => builder.ins().iconst(types::I64, NIL_VALUE as i64),
                };
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, val);
                }
            }

            Instruction::BinOp(result, op, lhs, rhs) => {
                let lhs_val = builder.use_var(var_map[lhs]);
                let rhs_val = builder.use_var(var_map[rhs]);

                // Track whether the op calls an FFI function that can raise
                let mut needs_exception_check = false;

                // For inline ops, the exception check is moved into the slow
                // (FFI) path only — the fast path can never raise, so we avoid
                // a redundant status load + branch on every operation.
                // In try-protected blocks the CheckException instruction
                // handles exceptions, so we skip the slow-path check too.
                let check_exc_in_slow = !is_try_protected;

                let val = match op {
                    // --- Inline small-int arithmetic fast paths ---
                    nera_syntax::ast::BinOp::Add | nera_syntax::ast::BinOp::Sub | nera_syntax::ast::BinOp::Mul => {
                        use super::ffi_emit::InlineArithmetic;
                        let operation = match op {
                            nera_syntax::ast::BinOp::Add => InlineArithmetic::Add,
                            nera_syntax::ast::BinOp::Sub => InlineArithmetic::Subtract,
                            _ => InlineArithmetic::Multiply,
                        };
                        self.emit_inline_arith(module, builder, ctx_var, lhs_val, rhs_val,
                            operation, check_exc_in_slow)?
                    }
                    nera_syntax::ast::BinOp::IntDiv => {
                        self.emit_inline_div_rem(module, builder, ctx_var, lhs_val, rhs_val, "nera_rt_int_div", false, check_exc_in_slow)?
                    }
                    nera_syntax::ast::BinOp::Rem => {
                        self.emit_inline_div_rem(module, builder, ctx_var, lhs_val, rhs_val, "nera_rt_int_rem", true, check_exc_in_slow)?
                    }
                    // --- Inline small-int comparison fast paths ---
                    nera_syntax::ast::BinOp::Lt | nera_syntax::ast::BinOp::Gt |
                    nera_syntax::ast::BinOp::LtEq | nera_syntax::ast::BinOp::GtEq |
                    nera_syntax::ast::BinOp::ExactEq | nera_syntax::ast::BinOp::ExactNotEq |
                    nera_syntax::ast::BinOp::Eq | nera_syntax::ast::BinOp::NotEq => {
                        let (cc, ffi_name) = match op {
                            nera_syntax::ast::BinOp::Lt => (IntCC::SignedLessThan, "nera_rt_compare_lt"),
                            nera_syntax::ast::BinOp::Gt => (IntCC::SignedGreaterThan, "nera_rt_compare_gt"),
                            nera_syntax::ast::BinOp::LtEq => (IntCC::SignedLessThanOrEqual, "nera_rt_compare_lte"),
                            nera_syntax::ast::BinOp::GtEq => (IntCC::SignedGreaterThanOrEqual, "nera_rt_compare_gte"),
                            nera_syntax::ast::BinOp::ExactEq | nera_syntax::ast::BinOp::Eq => (IntCC::Equal, if matches!(op, nera_syntax::ast::BinOp::ExactEq) { "nera_rt_exact_eq" } else { "nera_rt_compare_eq" }),
                            nera_syntax::ast::BinOp::ExactNotEq | nera_syntax::ast::BinOp::NotEq => (IntCC::NotEqual, if matches!(op, nera_syntax::ast::BinOp::ExactNotEq) { "nera_rt_exact_neq" } else { "nera_rt_compare_neq" }),
                            _ => unreachable!(),
                        };
                        self.emit_inline_cmp(module, builder, ctx_var, lhs_val, rhs_val, cc, ffi_name)?
                    }
                    // --- Non-inlined ops (FFI only) ---
                    // --- Inline small-int bitwise fast paths ---
                    nera_syntax::ast::BinOp::Band => {
                        // band on tagged small ints preserves the tag: (a&b) in high bits, 010&010=010
                        self.emit_inline_bitwise_direct(module, builder, ctx_var, lhs_val, rhs_val, "nera_rt_band", |b, a, c| b.ins().band(a, c), check_exc_in_slow)?
                    }
                    nera_syntax::ast::BinOp::Bor => {
                        // bor on tagged small ints preserves the tag: (a|b) in high bits, 010|010=010
                        self.emit_inline_bitwise_direct(module, builder, ctx_var, lhs_val, rhs_val, "nera_rt_bor", |b, a, c| b.ins().bor(a, c), check_exc_in_slow)?
                    }
                    nera_syntax::ast::BinOp::Bxor => {
                        // bxor zeroes the tag (010^010=000), need to re-set it
                        self.emit_inline_bxor(module, builder, ctx_var, lhs_val, rhs_val, check_exc_in_slow)?
                    }
                    nera_syntax::ast::BinOp::Bsl => {
                        self.emit_inline_shift(module, builder, ctx_var, lhs_val, rhs_val, "nera_rt_bsl", false, check_exc_in_slow)?
                    }
                    nera_syntax::ast::BinOp::Bsr => {
                        self.emit_inline_shift(module, builder, ctx_var, lhs_val, rhs_val, "nera_rt_bsr", true, check_exc_in_slow)?
                    }
                    nera_syntax::ast::BinOp::Andalso => {
                        // Short-circuit: if lhs is 'true', return rhs; else return lhs (false)
                        let true_val = self.load_true_atom(module, builder)?;
                        let is_true = builder.ins().icmp(IntCC::Equal, lhs_val, true_val);
                        builder.ins().select(is_true, rhs_val, lhs_val)
                    }
                    nera_syntax::ast::BinOp::Orelse => {
                        // Short-circuit: if lhs is 'true', return lhs (true); else return rhs
                        let true_val = self.load_true_atom(module, builder)?;
                        let is_true = builder.ins().icmp(IntCC::Equal, lhs_val, true_val);
                        builder.ins().select(is_true, lhs_val, rhs_val)
                    }
                    nera_syntax::ast::BinOp::And | nera_syntax::ast::BinOp::Or | nera_syntax::ast::BinOp::Xor => {
                        // Boolean ops: inline fast path when both operands are
                        // boolean atoms, FFI slow path that raises badarg otherwise.
                        let true_val = self.load_true_atom(module, builder)?;
                        let false_val = self.load_false_atom(module, builder)?;

                        // Check both operands are boolean (true or false atom)
                        let lhs_is_true = builder.ins().icmp(IntCC::Equal, lhs_val, true_val);
                        let lhs_is_false = builder.ins().icmp(IntCC::Equal, lhs_val, false_val);
                        let lhs_bool = builder.ins().bor(lhs_is_true, lhs_is_false);
                        let rhs_is_true = builder.ins().icmp(IntCC::Equal, rhs_val, true_val);
                        let rhs_is_false = builder.ins().icmp(IntCC::Equal, rhs_val, false_val);
                        let rhs_bool = builder.ins().bor(rhs_is_true, rhs_is_false);
                        let both_bool = builder.ins().band(lhs_bool, rhs_bool);

                        let fast_block = builder.create_block();
                        let slow_block = builder.create_block();
                        builder.set_cold_block(slow_block);
                        let merge_block = builder.create_block();
                        builder.append_block_param(merge_block, types::I64);

                        builder.ins().brif(both_bool, fast_block, &[], slow_block, &[]);

                        // Fast path: compute inline
                        builder.switch_to_block(fast_block);
                        builder.seal_block(fast_block);
                        let fast_result = match op {
                            nera_syntax::ast::BinOp::And => {
                                let both = builder.ins().band(lhs_is_true, rhs_is_true);
                                builder.ins().select(both, true_val, false_val)
                            }
                            nera_syntax::ast::BinOp::Or => {
                                let either = builder.ins().bor(lhs_is_true, rhs_is_true);
                                builder.ins().select(either, true_val, false_val)
                            }
                            nera_syntax::ast::BinOp::Xor => {
                                let only_one = builder.ins().bxor(lhs_is_true, rhs_is_true);
                                builder.ins().select(only_one, true_val, false_val)
                            }
                            _ => unreachable!(),
                        };
                        builder.ins().jump(merge_block, &[BlockArg::Value(fast_result)]);

                        // Slow path: FFI call that validates and raises badarg
                        builder.switch_to_block(slow_block);
                        builder.seal_block(slow_block);
                        let ffi_name = match op {
                            nera_syntax::ast::BinOp::And => "nera_rt_bool_and",
                            nera_syntax::ast::BinOp::Or => "nera_rt_bool_or",
                            nera_syntax::ast::BinOp::Xor => "nera_rt_bool_xor",
                            _ => unreachable!(),
                        };
                        let slow_ret = self.emit_ffi_call(module, builder, ffi_name, ctx_var, &[lhs_val, rhs_val])?;
                        if check_exc_in_slow {
                            let ctx_val = builder.use_var(ctx_var);
                            let status = builder.ins().load(types::I32, MemFlags::trusted(), ctx_val, CTX_OFFSET_STATUS);
                            let exc_const = builder.ins().iconst(types::I32, STATUS_EXCEPTION as i64);
                            let is_exc = builder.ins().icmp(IntCC::Equal, status, exc_const);
                            let exc_block = builder.create_block();
                            let slow_ok_block = builder.create_block();
                            builder.ins().brif(is_exc, exc_block, &[], slow_ok_block, &[]);

                            builder.switch_to_block(exc_block);
                            let nil = builder.ins().iconst(types::I64, NIL_VALUE as i64);
                            self.emit_stack_pop_and_return(module, builder, ctx_var, nil)?;

                            builder.switch_to_block(slow_ok_block);
                        }
                        builder.ins().jump(merge_block, &[BlockArg::Value(slow_ret)]);

                        builder.switch_to_block(merge_block);
                        builder.seal_block(merge_block);
                        builder.block_params(merge_block)[0]
                    }
                    nera_syntax::ast::BinOp::Div => {
                        needs_exception_check = true;
                        self.emit_ffi_call(module, builder, "nera_rt_fdiv", ctx_var, &[lhs_val, rhs_val])?
                    }
                    nera_syntax::ast::BinOp::ListAppend => {
                        needs_exception_check = true;
                        self.emit_ffi_call(module, builder, "nera_rt_list_append", ctx_var, &[lhs_val, rhs_val])?
                    }
                    nera_syntax::ast::BinOp::ListSubtract => {
                        needs_exception_check = true;
                        self.emit_ffi_call(module, builder, "nera_rt_list_subtract", ctx_var, &[lhs_val, rhs_val])?
                    }
                };

                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, val);
                }

                // Check for exception after arithmetic FFI calls
                // (skip if inside a try block — CheckException handles it)
                if needs_exception_check && !is_try_protected {
                    self.emit_ffi_exception_check(module, builder, ctx_var)?;
                }
            }

            Instruction::UnaryOp(result, op, operand) => {
                let operand_val = builder.use_var(var_map[operand]);

                let needs_exception_check = true;

                let val = match op {
                    nera_syntax::ast::UnaryOp::Neg => {
                        self.emit_ffi_call(module, builder, "nera_rt_negate", ctx_var, &[operand_val])?
                    }
                    nera_syntax::ast::UnaryOp::Not => {
                        // Logical not: true -> false, false -> true.
                        // Raises badarg if operand is not a boolean atom.
                        let true_atom = self.load_true_atom(module, builder)?;
                        let false_atom = self.load_false_atom(module, builder)?;
                        let is_true = builder.ins().icmp(IntCC::Equal, operand_val, true_atom);
                        let is_false = builder.ins().icmp(IntCC::Equal, operand_val, false_atom);
                        let is_bool = builder.ins().bor(is_true, is_false);

                        let fast_block = builder.create_block();
                        let slow_block = builder.create_block();
                        builder.set_cold_block(slow_block);
                        let merge_block = builder.create_block();
                        builder.append_block_param(merge_block, types::I64);

                        builder.ins().brif(is_bool, fast_block, &[], slow_block, &[]);

                        // Fast path: flip true<->false
                        builder.switch_to_block(fast_block);
                        builder.seal_block(fast_block);
                        let fast_result = builder.ins().select(is_true, false_atom, true_atom);
                        builder.ins().jump(merge_block, &[BlockArg::Value(fast_result)]);

                        // Slow path: FFI call that raises badarg
                        builder.switch_to_block(slow_block);
                        builder.seal_block(slow_block);
                        let slow_ret = self.emit_ffi_call(module, builder, "nera_rt_bool_not", ctx_var, &[operand_val])?;
                        builder.ins().jump(merge_block, &[BlockArg::Value(slow_ret)]);

                        builder.switch_to_block(merge_block);
                        builder.seal_block(merge_block);
                        builder.block_params(merge_block)[0]
                    }
                    nera_syntax::ast::UnaryOp::Bnot => {
                        self.emit_ffi_call(module, builder, "nera_rt_bnot", ctx_var, &[operand_val])?
                    }
                };

                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, val);
                }

                // Check for exception after negate FFI call
                // (skip if inside a try block — CheckException handles it)
                if needs_exception_check && !is_try_protected {
                    self.emit_ffi_exception_check(module, builder, ctx_var)?;
                }
            }

            Instruction::Move(dst, src) => {
                let src_val = builder.use_var(var_map[src]);
                if let Some(var) = var_map.get(dst) {
                    builder.def_var(*var, src_val);
                }
            }

            Instruction::MakeCons(result, head, tail) => {
                let head_val = builder.use_var(var_map[head]);
                let tail_val = builder.use_var(var_map[tail]);
                let ret = self.emit_inline_alloc_cons(module, builder, ctx_var, head_val, tail_val)?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::MakeTuple(result, elems) => {
                // Inline tuple allocation with bump-pointer fast path
                let elem_vals: Vec<Value> = elems.iter()
                    .map(|e| builder.use_var(var_map[e]))
                    .collect();
                let ret = self.emit_inline_alloc_tuple(module, builder, ctx_var, &elem_vals)?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::TupleElement(result, tuple, index) => {
                let fid = declare_tuple_element(module, &mut self.runtime_funcs, self.c_call_conv)?;
                let func_ref = module.declare_func_in_func(fid, builder.func);
                let ctx_val = builder.use_var(ctx_var);
                let tuple_val = builder.use_var(var_map[tuple]);
                let index_val = builder.ins().iconst(types::I32, *index as i64);
                let call = builder.ins().call(func_ref, &[ctx_val, tuple_val, index_val]);
                let ret = builder.inst_results(call)[0];
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::ListHead(result, list) => {
                // Inline hd/1: untag cons pointer, load word[0].
                // Falls back to FFI for badarg on non-cons input.
                let list_val = builder.use_var(var_map[list]);
                let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
                let tag = builder.ins().band(list_val, tag_mask);
                let cons_tag = builder.ins().iconst(types::I64, TAG_CONS as i64);
                let is_cons = builder.ins().icmp(IntCC::Equal, tag, cons_tag);

                let fast_block = builder.create_block();
                let slow_block = builder.create_block();
                let merge_block = builder.create_block();
                builder.append_block_param(merge_block, types::I64);

                builder.ins().brif(is_cons, fast_block, &[], slow_block, &[]);

                // Fast path: untag and load head (offset 0)
                builder.switch_to_block(fast_block);
                builder.seal_block(fast_block);
                let ptr_mask = builder.ins().iconst(types::I64, !(TAG_MASK | EXTERNAL_BIT) as i64);
                let raw_ptr = builder.ins().band(list_val, ptr_mask);
                let head = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, 0);
                builder.ins().jump(merge_block, &[BlockArg::Value(head)]);

                // Slow path: FFI call for badarg
                builder.switch_to_block(slow_block);
                builder.seal_block(slow_block);
                let slow_ret = self.emit_ffi_call(module, builder, "nera_rt_list_head", ctx_var, &[list_val])?;
                builder.ins().jump(merge_block, &[BlockArg::Value(slow_ret)]);

                builder.switch_to_block(merge_block);
                builder.seal_block(merge_block);
                let merged = builder.block_params(merge_block)[0];
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, merged);
                }

                // Check for exception from slow path (badarg on non-cons)
                if !is_try_protected {
                    self.emit_ffi_exception_check(module, builder, ctx_var)?;
                }
            }

            Instruction::ListTail(result, list) => {
                // Inline tl/1: untag cons pointer, load word[1].
                // Falls back to FFI for badarg on non-cons input.
                let list_val = builder.use_var(var_map[list]);
                let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
                let tag = builder.ins().band(list_val, tag_mask);
                let cons_tag = builder.ins().iconst(types::I64, TAG_CONS as i64);
                let is_cons = builder.ins().icmp(IntCC::Equal, tag, cons_tag);

                let fast_block = builder.create_block();
                let slow_block = builder.create_block();
                let merge_block = builder.create_block();
                builder.append_block_param(merge_block, types::I64);

                builder.ins().brif(is_cons, fast_block, &[], slow_block, &[]);

                // Fast path: untag and load tail (offset 8)
                builder.switch_to_block(fast_block);
                builder.seal_block(fast_block);
                let ptr_mask = builder.ins().iconst(types::I64, !(TAG_MASK | EXTERNAL_BIT) as i64);
                let raw_ptr = builder.ins().band(list_val, ptr_mask);
                let tail = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, 8);
                builder.ins().jump(merge_block, &[BlockArg::Value(tail)]);

                // Slow path: FFI call for badarg
                builder.switch_to_block(slow_block);
                builder.seal_block(slow_block);
                let slow_ret = self.emit_ffi_call(module, builder, "nera_rt_list_tail", ctx_var, &[list_val])?;
                builder.ins().jump(merge_block, &[BlockArg::Value(slow_ret)]);

                builder.switch_to_block(merge_block);
                builder.seal_block(merge_block);
                let merged = builder.block_params(merge_block)[0];
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, merged);
                }

                // Check for exception from slow path (badarg on non-cons)
                if !is_try_protected {
                    self.emit_ffi_exception_check(module, builder, ctx_var)?;
                }
            }

            Instruction::TupleSize(result, tuple) => {
                let tuple_val = builder.use_var(var_map[tuple]);
                let ret = self.emit_ffi_call(module, builder, "nera_rt_tuple_size", ctx_var, &[tuple_val])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::GetSelf(result) => {
                let ret = self.emit_ffi_call(module, builder, "nera_rt_get_self", ctx_var, &[])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::Send(result, pid, msg) => {
                let pid_val = builder.use_var(var_map[pid]);
                let msg_val = builder.use_var(var_map[msg]);
                let ret = self.emit_ffi_call(module, builder, "nera_rt_send_message", ctx_var, &[pid_val, msg_val])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::SendDirect(result, pid, elems) => {
                let pid_val = builder.use_var(var_map[pid]);
                let arity_val = builder.ins().iconst(types::I64, elems.len() as i64);
                // Pad elements to 4 (unused slots are 0).
                let zero = builder.ins().iconst(types::I64, 0);
                let e0 = if !elems.is_empty() { builder.use_var(var_map[&elems[0]]) } else { zero };
                let e1 = if elems.len() > 1 { builder.use_var(var_map[&elems[1]]) } else { zero };
                let e2 = if elems.len() > 2 { builder.use_var(var_map[&elems[2]]) } else { zero };
                let e3 = if elems.len() > 3 { builder.use_var(var_map[&elems[3]]) } else { zero };
                let ret = self.emit_ffi_call(module, builder, "nera_rt_send_tuple", ctx_var, &[pid_val, arity_val, e0, e1, e2, e3])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::Spawn(result, mod_atom, fun_atom, args) => {
                let fid = declare_runtime_func(module, "nera_rt_spawn", 3, &mut self.runtime_funcs, self.c_call_conv)?;
                let func_ref = module.declare_func_in_func(fid, builder.func);
                let ctx_val = builder.use_var(ctx_var);

                // Load mod and fun as runtime atom terms from global table
                let mod_val = self.load_atom_term(module, builder, mod_atom.0)?;
                let fun_val = self.load_atom_term(module, builder, fun_atom.0)?;

                // The args list: first element is the Erlang list of spawn arguments.
                // If args is empty, pass NIL. Otherwise pass the first element (already an Erlang list).
                let args_list = if args.is_empty() {
                    builder.ins().iconst(types::I64, NIL_VALUE as i64)
                } else {
                    builder.use_var(var_map[&args[0]])
                };

                let call = builder.ins().call(func_ref, &[ctx_val, mod_val, fun_val, args_list]);
                let ret = builder.inst_results(call)[0];
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::Call(result, target, args) => {
                // Sync reds_var to ctx.reductions before calls that may run
                // Erlang code (the callee reads ctx.reductions).
                match target {
                    CallTarget::Local(_) | CallTarget::Remote(_, _)
                    | CallTarget::Closure(_) | CallTarget::Apply(_, _) => {
                        self.sync_reds_to_ctx(builder, ctx_var);
                    }
                    _ => {}
                }

                let ctx_val = builder.use_var(ctx_var);
                let arg_vals: Vec<Value> = args.iter().map(|a| builder.use_var(var_map[a])).collect();

                let ret = if matches!(target, CallTarget::Bif(name) if name == "binary_segment_size" || name == "float_segment_size" || name == "binary_match_segment_size") {
                    let kind = match target {
                        CallTarget::Bif(name) if name == "float_segment_size" => SegmentSizeKind::Float,
                        CallTarget::Bif(name) if name == "binary_match_segment_size" => SegmentSizeKind::Matching,
                        _ => SegmentSizeKind::Construction,
                    };
                    self.emit_inline_segment_size(module, builder, ctx_var, &arg_vals, !is_try_protected, kind)?
                } else if matches!(target, CallTarget::Bif(name) if name == "binary_source_size") {
                    self.emit_inline_binary_source_size(module, builder, ctx_var, &arg_vals, !is_try_protected)?
                } else if matches!(target, CallTarget::Bif(name) if name == "receive_has_message") {
                    // The receive scan returns NO_MESSAGE_VALUE once it ends;
                    // any other value, `[]` included, is a message. The
                    // receive's Branch reads this predicate directly.
                    let found = builder.ins().icmp_imm(IntCC::NotEqual, arg_vals[0], NO_MESSAGE_VALUE as i64);
                    let true_val = self.load_true_atom(module, builder)?;
                    let false_val = self.load_false_atom(module, builder)?;
                    self.last_branch_predicate = Some((*result, found));
                    builder.ins().select(found, true_val, false_val)
                } else {
                    self.emit_call(module, builder, target, ctx_val, ctx_var, &arg_vals, var_map, mod_ctx)?
                };

                // Local and cross-module direct calls may return via the
                // tail-call trampoline: the callee sets STATUS_TAIL_CALL in ctx
                // and returns NIL.  We must resolve the pending tail call chain
                // and use ctx.result instead.  Remote calls can also hit this
                // path when the target is a co-compiled module (direct-linked).
                // For BIF/FFI targets the check is a fast no-op (status == OK).
                let final_ret = match target {
                    CallTarget::Local(_) | CallTarget::Remote(_, _) => {
                        self.emit_resolve_tail_call(module, builder, ctx_var, ret)?
                    }
                    _ => ret,
                };
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, final_ret);
                }

                // An invalid receive timeout raises here, once the mailbox
                // scan found no match; return before receive_wait can
                // overwrite the exception status.
                if matches!(target, CallTarget::Bif(name) if name == "receive_check_timeout")
                    && !is_try_protected {
                    self.emit_ffi_exception_check(module, builder, ctx_var)?;
                }

                // After calling receive_wait, emit set_resume so the
                // scheduler knows which function to re-call when woken.
                if matches!(target, CallTarget::Bif(name) if name == "receive_wait") {
                    self.emit_set_resume(module, builder, var_map, ctx_var)?;
                }
            }

            Instruction::TypeTest(result, kind, val) => {
                let val_raw = builder.use_var(var_map[val]);
                let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
                let tag = builder.ins().band(val_raw, tag_mask);

                let expected_tag = match kind {
                    nera_ir::ir::TypeTestKind::IsNil => TAG_NIL,
                    nera_ir::ir::TypeTestKind::IsAtom => TAG_ATOM,
                    nera_ir::ir::TypeTestKind::IsInteger => {
                        // Inline fast path: small int tag check → true.
                        // Slow path: FFI for boxed bigint check.
                        let small_int_tag = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
                        let is_small = builder.ins().icmp(IntCC::Equal, tag, small_int_tag);

                        let fast_block = builder.create_block();
                        let slow_block = builder.create_block();
                        let merge_block = builder.create_block();
                        builder.append_block_param(merge_block, types::I64);

                        builder.ins().brif(is_small, fast_block, &[], slow_block, &[]);

                        // Fast path: definitely an integer
                        builder.switch_to_block(fast_block);
                        builder.seal_block(fast_block);
                        let true_val_fast = self.load_true_atom(module, builder)?;
                        builder.ins().jump(merge_block, &[BlockArg::Value(true_val_fast)]);

                        // Slow path: might be boxed bigint
                        builder.switch_to_block(slow_block);
                        builder.seal_block(slow_block);
                        let slow_ret = self.emit_ffi_call(module, builder, "nera_rt_is_integer", ctx_var, &[val_raw])?;
                        builder.ins().jump(merge_block, &[BlockArg::Value(slow_ret)]);

                        builder.switch_to_block(merge_block);
                        builder.seal_block(merge_block);
                        let merged = builder.block_params(merge_block)[0];
                        if let Some(var) = var_map.get(result) {
                            builder.def_var(*var, merged);
                        }
                        return Ok(());
                    }
                    nera_ir::ir::TypeTestKind::IsPid => TAG_PID,
                    nera_ir::ir::TypeTestKind::IsList => {
                        let cons_tag = builder.ins().iconst(types::I64, TAG_CONS as i64);
                        let nil_val = builder.ins().iconst(types::I64, NIL_VALUE as i64);
                        let is_cons = builder.ins().icmp(IntCC::Equal, tag, cons_tag);
                        let is_nil = builder.ins().icmp(IntCC::Equal, val_raw, nil_val);
                        let is_list = builder.ins().bor(is_cons, is_nil);
                        let true_val = self.load_true_atom(module, builder)?;
                        let false_val = self.load_false_atom(module, builder)?;
                        let result_val = builder.ins().select(is_list, true_val, false_val);
                        if let Some(var) = var_map.get(result) {
                            builder.def_var(*var, result_val);
                        }
                        return Ok(());
                    }
                    nera_ir::ir::TypeTestKind::IsTuple => {
                        // Inline: TAG_BOXED && subtype == BOXED_TUPLE
                        let ret = self.emit_boxed_subtype_check(module, builder, val_raw, BOXED_TUPLE)?;
                        if let Some(var) = var_map.get(result) {
                            builder.def_var(*var, ret);
                        }
                        return Ok(());
                    }
                    nera_ir::ir::TypeTestKind::IsFloat => {
                        // Inline: TAG_BOXED && subtype == BOXED_FLOAT
                        let ret = self.emit_boxed_subtype_check(module, builder, val_raw, BOXED_FLOAT)?;
                        if let Some(var) = var_map.get(result) {
                            builder.def_var(*var, ret);
                        }
                        return Ok(());
                    }
                    nera_ir::ir::TypeTestKind::IsBinary => {
                        // Inline: TAG_BOXED && subtype in {BINARY, REFC_BINARY, MATCH_CTX}
                        // For BINARY/REFC_BINARY, additionally check arity % 8 == 0
                        // (sub-byte bitstrings are NOT binaries).
                        // Fall back to FFI for full check.
                        let ffi_ret = self.emit_ffi_call(module, builder, "nera_rt_is_binary", ctx_var, &[val_raw])?;
                        if let Some(var) = var_map.get(result) {
                            builder.def_var(*var, ffi_ret);
                        }
                        return Ok(());
                    }
                    nera_ir::ir::TypeTestKind::IsBitstring => {
                        // Inline: TAG_BOXED && subtype in {BINARY, REFC_BINARY, MATCH_CTX}
                        let ret = self.emit_boxed_multi_subtype_check(
                            module, builder, val_raw,
                            &[BOXED_BINARY, BOXED_REFC_BINARY, BOXED_MATCH_CTX],
                        )?;
                        if let Some(var) = var_map.get(result) {
                            builder.def_var(*var, ret);
                        }
                        return Ok(());
                    }
                    nera_ir::ir::TypeTestKind::IsNumber => {
                        // Inline: small int tag → true, OR boxed (FLOAT | BIGINT) → true
                        let small_int_tag = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
                        let is_small = builder.ins().icmp(IntCC::Equal, tag, small_int_tag);

                        let fast_block = builder.create_block();
                        let check_boxed_block = builder.create_block();
                        let merge_block = builder.create_block();
                        builder.append_block_param(merge_block, types::I64);

                        builder.ins().brif(is_small, fast_block, &[], check_boxed_block, &[]);

                        // Fast path: small int → true
                        builder.switch_to_block(fast_block);
                        builder.seal_block(fast_block);
                        let true_val_fast = self.load_true_atom(module, builder)?;
                        builder.ins().jump(merge_block, &[BlockArg::Value(true_val_fast)]);

                        // Check boxed float/bigint
                        builder.switch_to_block(check_boxed_block);
                        builder.seal_block(check_boxed_block);
                        let boxed_ret = self.emit_boxed_multi_subtype_check(
                            module, builder, val_raw,
                            &[BOXED_FLOAT, BOXED_BIGINT],
                        )?;
                        builder.ins().jump(merge_block, &[BlockArg::Value(boxed_ret)]);

                        builder.switch_to_block(merge_block);
                        builder.seal_block(merge_block);
                        let merged = builder.block_params(merge_block)[0];
                        if let Some(var) = var_map.get(result) {
                            builder.def_var(*var, merged);
                        }
                        return Ok(());
                    }
                    nera_ir::ir::TypeTestKind::IsMap => {
                        // Inline: TAG_BOXED && subtype == BOXED_MAP
                        let ret = self.emit_boxed_subtype_check(module, builder, val_raw, BOXED_MAP)?;
                        if let Some(var) = var_map.get(result) {
                            builder.def_var(*var, ret);
                        }
                        return Ok(());
                    }
                    nera_ir::ir::TypeTestKind::IsFunction => {
                        // Inline: TAG_BOXED && subtype == BOXED_CLOSURE
                        let ret = self.emit_boxed_subtype_check(module, builder, val_raw, BOXED_CLOSURE)?;
                        if let Some(var) = var_map.get(result) {
                            builder.def_var(*var, ret);
                        }
                        return Ok(());
                    }
                    nera_ir::ir::TypeTestKind::IsReference => {
                        // Inline: TAG_BOXED && subtype == BOXED_REFERENCE
                        let ret = self.emit_boxed_subtype_check(module, builder, val_raw, BOXED_REFERENCE)?;
                        if let Some(var) = var_map.get(result) {
                            builder.def_var(*var, ret);
                        }
                        return Ok(());
                    }
                };

                let expected = builder.ins().iconst(types::I64, expected_tag as i64);
                let cmp = builder.ins().icmp(IntCC::Equal, tag, expected);
                if matches!(kind, nera_ir::ir::TypeTestKind::IsNil) {
                    self.last_branch_predicate = Some((*result, cmp));
                }
                let true_val = self.load_true_atom(module, builder)?;
                let false_val = self.load_false_atom(module, builder)?;
                let result_val = builder.ins().select(cmp, true_val, false_val);

                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, result_val);
                }
            }

            Instruction::MakeBinary(result, size) => {
                let size_val = builder.use_var(var_map[size]);
                let ret = self.emit_ffi_call(module, builder, "nera_rt_alloc_binary", ctx_var, &[size_val])?;
                if !is_try_protected { self.emit_ffi_exception_check(module, builder, ctx_var)?; }
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::BinaryPutInteger(bin, offset, val, bit_size, flags) => {
                let bin_val = builder.use_var(var_map[bin]);
                let off_val = builder.use_var(var_map[offset]);
                let value_val = builder.use_var(var_map[val]);
                let size_val = builder.use_var(var_map[bit_size]);
                let flags_val = builder.ins().iconst(
                    types::I64,
                    ((flags.endianness as i64) << 8) | (flags.signedness as i64),
                );
                let written = self.emit_ffi_call(module, builder, "nera_rt_binary_put_integer", ctx_var, &[bin_val, off_val, value_val, size_val, flags_val])?;
                if !self.current_try_protected_values.contains(bin) {
                    self.emit_ffi_nil_check(module, builder, ctx_var, written)?;
                }
            }

            Instruction::BinaryPutFloat(bin, offset, val, bit_size, flags) => {
                let bin_val = builder.use_var(var_map[bin]);
                let off_val = builder.use_var(var_map[offset]);
                let value_val = builder.use_var(var_map[val]);
                let size_val = builder.use_var(var_map[bit_size]);
                let flags_val = builder.ins().iconst(
                    types::I64,
                    (flags.endianness as i64) << 8,
                );
                let written = self.emit_ffi_call(module, builder, "nera_rt_binary_put_float", ctx_var, &[bin_val, off_val, value_val, size_val, flags_val])?;
                // MakeBinary records this construction's lexical try scope.
                if !self.current_try_protected_values.contains(bin) {
                    self.emit_ffi_nil_check(module, builder, ctx_var, written)?;
                }
            }

            Instruction::BinaryPutBinary(dest, offset, src, byte_count) => {
                let dest_val = builder.use_var(var_map[dest]);
                let off_val = builder.use_var(var_map[offset]);
                let src_val = builder.use_var(var_map[src]);
                let count_val = builder.use_var(var_map[byte_count]);
                let written = self.emit_ffi_call(module, builder, "nera_rt_binary_put_binary", ctx_var, &[dest_val, off_val, src_val, count_val])?;
                if !self.current_try_protected_values.contains(dest) {
                    self.emit_ffi_nil_check(module, builder, ctx_var, written)?;
                }
            }

            Instruction::BinaryGetInteger(result, bin, offset, bit_size, flags) => {
                // Inline data pointer extraction from binary term, then use
                // nera_rt_binary_get_integer_raw (which takes raw base_ptr).
                let bin_val = builder.use_var(var_map[bin]);
                let off_val = builder.use_var(var_map[offset]);
                let size_val = builder.use_var(var_map[bit_size]);
                let flags_val = builder.ins().iconst(
                    types::I64,
                    ((flags.endianness as i64) << 8) | (flags.signedness as i64),
                );

                // Check: is input a boxed pointer?
                let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
                let tag = builder.ins().band(bin_val, tag_mask);
                let boxed_tag = builder.ins().iconst(types::I64, TAG_BOXED as i64);
                let is_boxed = builder.ins().icmp(IntCC::Equal, tag, boxed_tag);

                let check_subtype_block = builder.create_block();
                let fallback_block = builder.create_block();
                let heap_bin_block = builder.create_block();
                let refc_bin_block = builder.create_block();
                let refc_extract_block = builder.create_block();
                let raw_call_block = builder.create_block();
                let merge_block = builder.create_block();

                builder.ins().brif(is_boxed, check_subtype_block, &[], fallback_block, &[]);

                // Load header, check subtype
                builder.switch_to_block(check_subtype_block);
                let ptr_mask = builder.ins().iconst(types::I64, !(TAG_MASK | EXTERNAL_BIT) as i64);
                let raw_ptr = builder.ins().band(bin_val, ptr_mask);
                let header = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, 0);
                let subtag_mask = builder.ins().iconst(types::I64, 0xF);
                let subtype = builder.ins().band(header, subtag_mask);

                let heap_bin_tag = builder.ins().iconst(types::I64, BOXED_HEAP_BINARY as i64);
                let is_heap_bin = builder.ins().icmp(IntCC::Equal, subtype, heap_bin_tag);
                builder.ins().brif(is_heap_bin, heap_bin_block, &[], refc_bin_block, &[]);

                // Heap binary: data_ptr = ptr + 8
                builder.switch_to_block(heap_bin_block);
                let eight_c = builder.ins().iconst(types::I64, 8);
                let data_ptr_h = builder.ins().iadd(raw_ptr, eight_c);
                builder.ins().jump(raw_call_block, &[BlockArg::Value(data_ptr_h)]);

                // Refc binary check
                builder.switch_to_block(refc_bin_block);
                let refc_tag = builder.ins().iconst(types::I64, BOXED_REFC_BINARY as i64);
                let is_refc = builder.ins().icmp(IntCC::Equal, subtype, refc_tag);
                builder.ins().brif(is_refc, refc_extract_block, &[], fallback_block, &[]);

                // Refc binary: deref Arc<BinaryData>
                builder.switch_to_block(refc_extract_block);
                let raw_arc_ptr = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, 8);
                let byte_offset = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, 16);
                let backing_data_ptr = builder.ins().load(types::I64, MemFlags::new(), raw_arc_ptr, BINARY_DATA_PTR_BYTE_OFFSET);
                let data_ptr_r = builder.ins().iadd(backing_data_ptr, byte_offset);
                builder.ins().jump(raw_call_block, &[BlockArg::Value(data_ptr_r)]);

                // Call nera_rt_binary_get_integer_raw with extracted base pointer
                builder.switch_to_block(raw_call_block);
                let base_ptr_param = builder.append_block_param(raw_call_block, types::I64);
                let raw_ret = self.emit_ffi_call(module, builder, "nera_rt_binary_get_integer_raw", ctx_var, &[base_ptr_param, off_val, size_val, flags_val])?;
                builder.ins().jump(merge_block, &[BlockArg::Value(raw_ret)]);

                // Fallback: full FFI call with tagged binary
                builder.switch_to_block(fallback_block);
                let ffi_ret = self.emit_ffi_call(module, builder, "nera_rt_binary_get_integer", ctx_var, &[bin_val, off_val, size_val, flags_val])?;
                builder.ins().jump(merge_block, &[BlockArg::Value(ffi_ret)]);

                // Merge
                builder.switch_to_block(merge_block);
                let merge_param = builder.append_block_param(merge_block, types::I64);
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, merge_param);
                }
            }

            Instruction::BinaryGetFloat(result, bin, offset, bit_size, flags) => {
                let bin_val = builder.use_var(var_map[bin]);
                let off_val = builder.use_var(var_map[offset]);
                let size_val = builder.use_var(var_map[bit_size]);
                let flags_val = builder.ins().iconst(
                    types::I64,
                    (flags.endianness as i64) << 8,
                );
                let ret = self.emit_ffi_call(module, builder, "nera_rt_binary_get_float", ctx_var, &[bin_val, off_val, size_val, flags_val])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::BinaryGetBinary(result, bin, offset, byte_count) => {
                let bin_val = builder.use_var(var_map[bin]);
                let off_val = builder.use_var(var_map[offset]);
                let count_val = builder.use_var(var_map[byte_count]);
                let ret = self.emit_ffi_call(module, builder, "nera_rt_binary_get_binary", ctx_var, &[bin_val, off_val, count_val])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::BinarySize(result, bin) => {
                // Inline path for BOXED_MATCH_CTX, heap binary, and refc binary.
                // Falls back to FFI only for truly unknown subtypes.
                let bin_val = builder.use_var(var_map[bin]);

                // Check: is input a boxed pointer? (low 3 bits == TAG_BOXED == 0)
                let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
                let tag = builder.ins().band(bin_val, tag_mask);
                let boxed_tag = builder.ins().iconst(types::I64, TAG_BOXED as i64);
                let is_boxed = builder.ins().icmp(IntCC::Equal, tag, boxed_tag);

                let check_subtype_block = builder.create_block();
                let fallback_block = builder.create_block();
                let mc_inline_block = builder.create_block();
                let check_heap_bin_block = builder.create_block();
                let bin_inline_block = builder.create_block();
                let check_refc_block = builder.create_block();
                let merge_block = builder.create_block();

                builder.ins().brif(is_boxed, check_subtype_block, &[], fallback_block, &[]);

                // Load header and extract subtype
                builder.switch_to_block(check_subtype_block);
                let ptr_mask = builder.ins().iconst(types::I64, !(TAG_MASK | EXTERNAL_BIT) as i64);
                let raw_ptr = builder.ins().band(bin_val, ptr_mask);
                let header = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, 0);
                let subtag_mask = builder.ins().iconst(types::I64, 0xF);
                let subtype = builder.ins().band(header, subtag_mask);

                // Check match ctx first (most common in binary matching loops)
                let mc_tag = builder.ins().iconst(types::I64, BOXED_MATCH_CTX as i64);
                let is_mc = builder.ins().icmp(IntCC::Equal, subtype, mc_tag);
                builder.ins().brif(is_mc, mc_inline_block, &[], check_heap_bin_block, &[]);

                // Match ctx: remaining = total_bits - bit_offset
                builder.switch_to_block(mc_inline_block);
                let bit_offset = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, MATCH_CTX_BIT_OFFSET);
                let total_bits_mc = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, MATCH_CTX_END_OFFSET);
                let remaining = builder.ins().isub(total_bits_mc, bit_offset);
                let tag_bits = builder.ins().iconst(types::I64, TAG_BITS as i64);
                let shifted = builder.ins().ishl(remaining, tag_bits);
                let si_tag = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
                let tagged_mc = builder.ins().bor(shifted, si_tag);
                builder.ins().jump(merge_block, &[BlockArg::Value(tagged_mc)]);

                // Check heap binary
                builder.switch_to_block(check_heap_bin_block);
                let heap_bin_tag = builder.ins().iconst(types::I64, BOXED_HEAP_BINARY as i64);
                let is_heap_bin = builder.ins().icmp(IntCC::Equal, subtype, heap_bin_tag);
                builder.ins().brif(is_heap_bin, bin_inline_block, &[], check_refc_block, &[]);

                // Check refc binary
                builder.switch_to_block(check_refc_block);
                let refc_tag = builder.ins().iconst(types::I64, BOXED_REFC_BINARY as i64);
                let is_refc = builder.ins().icmp(IntCC::Equal, subtype, refc_tag);
                builder.ins().brif(is_refc, bin_inline_block, &[], fallback_block, &[]);

                // Heap/refc binary: arity field (header >> 4) is already bit count
                builder.switch_to_block(bin_inline_block);
                let four = builder.ins().iconst(types::I64, 4);
                let bit_count = builder.ins().ushr(header, four);
                let tag_bits2 = builder.ins().iconst(types::I64, TAG_BITS as i64);
                let shifted2 = builder.ins().ishl(bit_count, tag_bits2);
                let si_tag2 = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
                let tagged_bin = builder.ins().bor(shifted2, si_tag2);
                builder.ins().jump(merge_block, &[BlockArg::Value(tagged_bin)]);

                // Fallback: existing FFI call
                builder.switch_to_block(fallback_block);
                let ffi_ret = self.emit_ffi_call(module, builder, "nera_rt_binary_size", ctx_var, &[bin_val])?;
                builder.ins().jump(merge_block, &[BlockArg::Value(ffi_ret)]);

                // Merge
                builder.switch_to_block(merge_block);
                let merge_param = builder.append_block_param(merge_block, types::I64);
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, merge_param);
                }
            }

            Instruction::BsMatchBitOffset(result, input) => {
                let input_val = builder.use_var(var_map[input]);
                let boxed = builder.ins().band_imm(input_val, TAG_MASK as i64);
                let is_boxed = builder.ins().icmp_imm(IntCC::Equal, boxed, TAG_BOXED as i64);
                let inspect = builder.create_block();
                let context = builder.create_block();
                let zero = builder.create_block();
                let merge = builder.create_block();
                builder.ins().brif(is_boxed, inspect, &[], zero, &[]);
                builder.switch_to_block(inspect);
                let ptr = builder.ins().band_imm(input_val, !(TAG_MASK | EXTERNAL_BIT) as i64);
                let header = builder.ins().load(types::I64, MemFlags::new(), ptr, 0);
                let subtype = builder.ins().band_imm(header, 0xf);
                let is_context = builder.ins().icmp_imm(IntCC::Equal, subtype, BOXED_MATCH_CTX as i64);
                builder.ins().brif(is_context, context, &[], zero, &[]);
                builder.switch_to_block(context);
                let offset = builder.ins().load(types::I64, MemFlags::new(), ptr, MATCH_CTX_BIT_OFFSET);
                let remainder = builder.ins().band_imm(offset, 7);
                let shifted = builder.ins().ishl_imm(remainder, TAG_BITS as i64);
                let tagged = builder.ins().bor_imm(shifted, TAG_SMALL_INT as i64);
                builder.ins().jump(merge, &[BlockArg::Value(tagged)]);
                builder.switch_to_block(zero);
                let tagged_zero = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
                builder.ins().jump(merge, &[BlockArg::Value(tagged_zero)]);
                builder.switch_to_block(merge);
                let offset = builder.append_block_param(merge, types::I64);
                builder.def_var(var_map[result], offset);
            }

            Instruction::BsStartMatch(base_ptr, total_bits, input_bin) => {
                // Inline fast path for heap binary and refc binary inputs (no match ctx alloc).
                // Slow path (match_ctx input reuse, non-binary) → FFI.
                let bin_val = builder.use_var(var_map[input_bin]);

                // Check: is input a boxed pointer?
                let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
                let tag = builder.ins().band(bin_val, tag_mask);
                let boxed_tag = builder.ins().iconst(types::I64, TAG_BOXED as i64);
                let is_boxed = builder.ins().icmp(IntCC::Equal, tag, boxed_tag);

                let check_subtype_block = builder.create_block();
                let slow_block = builder.create_block();
                let heap_bin_block = builder.create_block();
                let refc_bin_block = builder.create_block();
                let refc_extract_block = builder.create_block();
                let tag_result_block = builder.create_block();
                let merge_block = builder.create_block();

                builder.ins().brif(is_boxed, check_subtype_block, &[], slow_block, &[]);

                // Check subtype
                builder.switch_to_block(check_subtype_block);
                let ptr_mask = builder.ins().iconst(types::I64, !(TAG_MASK | EXTERNAL_BIT) as i64);
                let raw_ptr = builder.ins().band(bin_val, ptr_mask);
                let header = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, 0);
                let subtag_mask = builder.ins().iconst(types::I64, 0xF);
                let subtype = builder.ins().band(header, subtag_mask);

                let heap_bin_tag = builder.ins().iconst(types::I64, BOXED_HEAP_BINARY as i64);
                let is_heap_bin = builder.ins().icmp(IntCC::Equal, subtype, heap_bin_tag);
                builder.ins().brif(is_heap_bin, heap_bin_block, &[], refc_bin_block, &[]);

                // Heap binary: data_ptr = ptr + 8, bit_count = header >> 4
                builder.switch_to_block(heap_bin_block);
                let four = builder.ins().iconst(types::I64, 4);
                let bit_count_h = builder.ins().ushr(header, four);
                let eight_c = builder.ins().iconst(types::I64, 8);
                let data_ptr_h = builder.ins().iadd(raw_ptr, eight_c);
                builder.ins().jump(tag_result_block, &[BlockArg::Value(data_ptr_h), BlockArg::Value(bit_count_h)]);

                // Refc binary check
                builder.switch_to_block(refc_bin_block);
                let refc_tag = builder.ins().iconst(types::I64, BOXED_REFC_BINARY as i64);
                let is_refc = builder.ins().icmp(IntCC::Equal, subtype, refc_tag);
                builder.ins().brif(is_refc, refc_extract_block, &[], slow_block, &[]);

                // Refc binary: deref Arc<BinaryData>
                builder.switch_to_block(refc_extract_block);
                let four_r = builder.ins().iconst(types::I64, 4);
                let bit_count_r = builder.ins().ushr(header, four_r);
                let raw_arc_ptr = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, 8);
                let byte_offset = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, 16);
                let backing_data_ptr = builder.ins().load(types::I64, MemFlags::new(), raw_arc_ptr, BINARY_DATA_PTR_BYTE_OFFSET);
                let data_ptr_r = builder.ins().iadd(backing_data_ptr, byte_offset);
                builder.ins().jump(tag_result_block, &[BlockArg::Value(data_ptr_r), BlockArg::Value(bit_count_r)]);

                // Tag total_bits as small int (header >> 4 is already bits)
                builder.switch_to_block(tag_result_block);
                let data_ptr_param = builder.append_block_param(tag_result_block, types::I64);
                let total_bits_raw = builder.append_block_param(tag_result_block, types::I64);
                let tag_bits_c = builder.ins().iconst(types::I64, TAG_BITS as i64);
                let total_shifted = builder.ins().ishl(total_bits_raw, tag_bits_c);
                let si_tag = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
                let total_tagged = builder.ins().bor(total_shifted, si_tag);
                builder.ins().jump(merge_block, &[BlockArg::Value(data_ptr_param), BlockArg::Value(total_tagged)]);

                // Slow path: FFI
                builder.switch_to_block(slow_block);
                let ret_ptr = self.emit_ffi_call(module, builder, "nera_rt_bs_start_match", ctx_var, &[bin_val])?;
                let ctx_val = builder.use_var(ctx_var);
                let slow_total = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val, CTX_OFFSET_RESULT);
                builder.ins().jump(merge_block, &[BlockArg::Value(ret_ptr), BlockArg::Value(slow_total)]);

                // Merge
                builder.switch_to_block(merge_block);
                let merge_base = builder.append_block_param(merge_block, types::I64);
                let merge_total = builder.append_block_param(merge_block, types::I64);
                if let Some(var) = var_map.get(base_ptr) {
                    builder.def_var(*var, merge_base);
                }
                if let Some(var) = var_map.get(total_bits) {
                    builder.def_var(*var, merge_total);
                }
            }

            Instruction::BsGetIntegerInline(result, base_ptr, bit_offset, bit_count, flags) => {
                // Inlined integer extraction from a match context.
                // base_ptr is UNTAGGED, bit_offset and bit_count are tagged small ints (in BITS).
                let base_val = builder.use_var(var_map[base_ptr]);
                let off_val = builder.use_var(var_map[bit_offset]);
                let count_val = builder.use_var(var_map[bit_count]);

                // Untag bit_offset and bit_count
                let tag_bits = builder.ins().iconst(types::I64, TAG_BITS as i64);
                let bit_off_raw = builder.ins().sshr(off_val, tag_bits);
                let bit_cnt_raw = builder.ins().sshr(count_val, tag_bits);

                // Check byte-alignment: bit_offset & 7 == 0
                let seven = builder.ins().iconst(types::I64, 7);
                let off_mod8 = builder.ins().band(bit_off_raw, seven);
                let zero = builder.ins().iconst(types::I64, 0);
                let is_aligned = builder.ins().icmp(IntCC::Equal, off_mod8, zero);

                // Determine which inline widths we can handle at compile time.
                // Flags are compile-time constants, so we generate only the needed path.
                // Endianness: 0=big (need bswap on LE host), 1=little (no swap), 2=native (no swap)
                let needs_bswap = flags.endianness == 0; // big-endian on LE host
                let is_signed = flags.signedness == 1;

                // Build width-check chain: 8, 16, 32, 64
                let inline8_block = builder.create_block();
                let check16_block = builder.create_block();
                let inline16_block = builder.create_block();
                let check32_block = builder.create_block();
                let inline32_block = builder.create_block();
                let check64_block = builder.create_block();
                let inline64_block = builder.create_block();
                let fallback_block = builder.create_block();
                let merge_block = builder.create_block();

                // First gate: must be byte-aligned
                builder.ins().brif(is_aligned, check16_block, &[], fallback_block, &[]);

                // Check bit_count == 8 → inline8
                // (We check 16 first since 8-bit is the most common fast path and we can
                // fall through the chain. Actually let's do ascending order for clarity.)
                // Re-structure: aligned → check8 → check16 → check32 → check64 → fallback
                // Let me redo the blocks more cleanly:
                // After is_aligned, jump to a dispatch chain.

                // Actually, let's restructure. After the alignment check passes, we dispatch on width.
                // Redefine: the is_aligned branch above goes to a width_dispatch block.

                // We already emitted the brif. Let's use check16_block as the width dispatch entry.
                // check16_block is really "width_dispatch" block.
                builder.switch_to_block(check16_block);
                let eight = builder.ins().iconst(types::I64, 8);
                let is_8 = builder.ins().icmp(IntCC::Equal, bit_cnt_raw, eight);
                builder.ins().brif(is_8, inline8_block, &[], check32_block, &[]);

                builder.switch_to_block(check32_block);
                let sixteen = builder.ins().iconst(types::I64, 16);
                let is_16 = builder.ins().icmp(IntCC::Equal, bit_cnt_raw, sixteen);
                builder.ins().brif(is_16, inline16_block, &[], check64_block, &[]);

                builder.switch_to_block(check64_block);
                let thirtytwo = builder.ins().iconst(types::I64, 32);
                let is_32 = builder.ins().icmp(IntCC::Equal, bit_cnt_raw, thirtytwo);
                builder.ins().brif(is_32, inline32_block, &[], inline64_block, &[]);

                // inline64_block: check if bit_count == 64, else fallback
                builder.switch_to_block(inline64_block);
                let sixtyfour = builder.ins().iconst(types::I64, 64);
                let is_64 = builder.ins().icmp(IntCC::Equal, bit_cnt_raw, sixtyfour);
                // For 64-bit we need to check small int range after loading
                let inline64_load_block = builder.create_block();
                builder.ins().brif(is_64, inline64_load_block, &[], fallback_block, &[]);

                // Helper: compute byte_offset = bit_off_raw >> 3, addr = base + byte_offset
                // We'll need this in each inline block, but Cranelift values are per-block.
                // We use the tag_bits value which is defined in the entry block above the brif.

                // --- Inline 8-bit ---
                builder.switch_to_block(inline8_block);
                let three_8 = builder.ins().iconst(types::I64, 3);
                let byte_off_8 = builder.ins().sshr(bit_off_raw, three_8);
                let addr_8 = builder.ins().iadd(base_val, byte_off_8);
                let raw_8 = builder.ins().load(types::I8, MemFlags::new(), addr_8, 0);
                let val_8 = if is_signed {
                    builder.ins().sextend(types::I64, raw_8)
                } else {
                    builder.ins().uextend(types::I64, raw_8)
                };
                // No bswap needed for single byte
                let shifted_8 = builder.ins().ishl(val_8, tag_bits);
                let tag_c_8 = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
                let tagged_8 = builder.ins().bor(shifted_8, tag_c_8);
                builder.ins().jump(merge_block, &[BlockArg::Value(tagged_8)]);

                // --- Inline 16-bit ---
                builder.switch_to_block(inline16_block);
                let three_16 = builder.ins().iconst(types::I64, 3);
                let byte_off_16 = builder.ins().sshr(bit_off_raw, three_16);
                let addr_16 = builder.ins().iadd(base_val, byte_off_16);
                let raw_16 = builder.ins().load(types::I16, MemFlags::new(), addr_16, 0);
                // On LE host: memory is LE. Big-endian data needs bswap.
                let swapped_16 = if needs_bswap {
                    builder.ins().bswap(raw_16)
                } else {
                    raw_16
                };
                let val_16 = if is_signed {
                    builder.ins().sextend(types::I64, swapped_16)
                } else {
                    builder.ins().uextend(types::I64, swapped_16)
                };
                let shifted_16 = builder.ins().ishl(val_16, tag_bits);
                let tag_c_16 = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
                let tagged_16 = builder.ins().bor(shifted_16, tag_c_16);
                builder.ins().jump(merge_block, &[BlockArg::Value(tagged_16)]);

                // --- Inline 32-bit ---
                builder.switch_to_block(inline32_block);
                let three_32 = builder.ins().iconst(types::I64, 3);
                let byte_off_32 = builder.ins().sshr(bit_off_raw, three_32);
                let addr_32 = builder.ins().iadd(base_val, byte_off_32);
                let raw_32 = builder.ins().load(types::I32, MemFlags::new(), addr_32, 0);
                let swapped_32 = if needs_bswap {
                    builder.ins().bswap(raw_32)
                } else {
                    raw_32
                };
                let val_32 = if is_signed {
                    builder.ins().sextend(types::I64, swapped_32)
                } else {
                    builder.ins().uextend(types::I64, swapped_32)
                };
                let shifted_32 = builder.ins().ishl(val_32, tag_bits);
                let tag_c_32 = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
                let tagged_32 = builder.ins().bor(shifted_32, tag_c_32);
                builder.ins().jump(merge_block, &[BlockArg::Value(tagged_32)]);

                // --- Inline 64-bit ---
                builder.switch_to_block(inline64_load_block);
                let three_64 = builder.ins().iconst(types::I64, 3);
                let byte_off_64 = builder.ins().sshr(bit_off_raw, three_64);
                let addr_64 = builder.ins().iadd(base_val, byte_off_64);
                let raw_64 = builder.ins().load(types::I64, MemFlags::new(), addr_64, 0);
                let swapped_64 = if needs_bswap {
                    builder.ins().bswap(raw_64)
                } else {
                    raw_64
                };
                // Check if value fits in small int range (61-bit signed)
                let min_val = builder.ins().iconst(types::I64, SMALL_INT_MIN);
                let max_val = builder.ins().iconst(types::I64, SMALL_INT_MAX);
                let fits = if is_signed {
                    let ge_min = builder.ins().icmp(IntCC::SignedGreaterThanOrEqual, swapped_64, min_val);
                    let le_max = builder.ins().icmp(IntCC::SignedLessThanOrEqual, swapped_64, max_val);
                    builder.ins().band(ge_min, le_max)
                } else {
                    builder.ins().icmp(IntCC::UnsignedLessThanOrEqual, swapped_64, max_val)
                };
                let inline64_tag_block = builder.create_block();
                builder.ins().brif(fits, inline64_tag_block, &[], fallback_block, &[]);

                builder.switch_to_block(inline64_tag_block);
                let shifted_64 = builder.ins().ishl(swapped_64, tag_bits);
                let tag_c_64 = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
                let tagged_64 = builder.ins().bor(shifted_64, tag_c_64);
                builder.ins().jump(merge_block, &[BlockArg::Value(tagged_64)]);

                // Fallback path: call nera_rt_binary_get_integer_raw(ctx, base_ptr, bit_offset, bit_count, flags)
                builder.switch_to_block(fallback_block);
                let flags_val = builder.ins().iconst(
                    types::I64,
                    ((flags.endianness as i64) << 8) | (flags.signedness as i64),
                );
                let fb_ret = self.emit_ffi_call(module, builder, "nera_rt_binary_get_integer_raw", ctx_var, &[base_val, off_val, count_val, flags_val])?;
                builder.ins().jump(merge_block, &[BlockArg::Value(fb_ret)]);

                // Merge block
                builder.switch_to_block(merge_block);
                let merge_param = builder.append_block_param(merge_block, types::I64);
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, merge_param);
                }
            }

            Instruction::BsMaterialize(result, orig_binary, bit_offset, bit_count) => {
                // Delegates to nera_rt_bs_materialize(ctx, orig_binary, bit_offset, bit_count)
                // orig_binary can be either an original binary term or a match_ctx term.
                let orig_val = builder.use_var(var_map[orig_binary]);
                let off_val = builder.use_var(var_map[bit_offset]);
                let count_val = builder.use_var(var_map[bit_count]);
                let ret = self.emit_ffi_call(module, builder, "nera_rt_bs_materialize", ctx_var, &[orig_val, off_val, count_val])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::BsMatchStart2(match_ctx, base_ptr, total_bits, input) => {
                // Inline fast path for heap binary and refc binary inputs:
                // 1. Extract data pointer and byte count
                // 2. Inline-allocate 4-word match context from heap_cursor
                // Slow path (match_ctx reuse, non-binary, heap exhaustion) → FFI
                let input_val = builder.use_var(var_map[input]);

                // Check: is input a boxed pointer? (low 3 bits == TAG_BOXED == 0)
                let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
                let tag = builder.ins().band(input_val, tag_mask);
                let boxed_tag = builder.ins().iconst(types::I64, TAG_BOXED as i64);
                let is_boxed = builder.ins().icmp(IntCC::Equal, tag, boxed_tag);

                let check_subtype_block = builder.create_block();
                let slow_block = builder.create_block();
                let heap_bin_block = builder.create_block();
                let refc_bin_block = builder.create_block();
                let alloc_mc_block = builder.create_block();
                let alloc_fast_block = builder.create_block();
                let merge_block = builder.create_block();

                builder.ins().brif(is_boxed, check_subtype_block, &[], slow_block, &[]);

                // Check subtype: heap binary (0x3) or refc binary (0x6)?
                // Anything else (match_ctx, tuple, etc.) → slow path
                builder.switch_to_block(check_subtype_block);
                let ptr_mask = builder.ins().iconst(types::I64, !(TAG_MASK | EXTERNAL_BIT) as i64);
                let raw_ptr = builder.ins().band(input_val, ptr_mask);
                let header = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, 0);
                let subtag_mask = builder.ins().iconst(types::I64, 0xF);
                let subtype = builder.ins().band(header, subtag_mask);

                let heap_bin_tag = builder.ins().iconst(types::I64, BOXED_HEAP_BINARY as i64);
                let is_heap_bin = builder.ins().icmp(IntCC::Equal, subtype, heap_bin_tag);
                builder.ins().brif(is_heap_bin, heap_bin_block, &[], refc_bin_block, &[]);

                // --- Heap binary: data_ptr = ptr + 8, bit_count = header >> 4 ---
                builder.switch_to_block(heap_bin_block);
                let four = builder.ins().iconst(types::I64, 4);
                let bit_count_h = builder.ins().ushr(header, four);
                let eight_h = builder.ins().iconst(types::I64, 8);
                let data_ptr_h = builder.ins().iadd(raw_ptr, eight_h);
                builder.ins().jump(alloc_mc_block, &[BlockArg::Value(data_ptr_h), BlockArg::Value(bit_count_h)]);

                // --- Refc binary: deref Arc<BinaryData> ---
                // Layout: [header | raw_arc_ptr | byte_offset]
                // raw_arc_ptr points to BinaryData, whose repr(C) prefix is the data pointer
                builder.switch_to_block(refc_bin_block);
                let refc_tag = builder.ins().iconst(types::I64, BOXED_REFC_BINARY as i64);
                let is_refc = builder.ins().icmp(IntCC::Equal, subtype, refc_tag);
                let refc_extract_block = builder.create_block();
                builder.ins().brif(is_refc, refc_extract_block, &[], slow_block, &[]);

                builder.switch_to_block(refc_extract_block);
                let four_r = builder.ins().iconst(types::I64, 4);
                let bit_count_r = builder.ins().ushr(header, four_r);
                // Word 1: raw_arc_ptr (pointer to Vec<u8>)
                let raw_arc_ptr = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, 8);
                // Word 2: byte_offset
                let byte_offset = builder.ins().load(types::I64, MemFlags::new(), raw_ptr, 16);
                // BinaryData data pointer at its repr(C) offset
                let backing_data_ptr = builder.ins().load(types::I64, MemFlags::new(), raw_arc_ptr, BINARY_DATA_PTR_BYTE_OFFSET);
                let data_ptr_r = builder.ins().iadd(backing_data_ptr, byte_offset);
                builder.ins().jump(alloc_mc_block, &[BlockArg::Value(data_ptr_r), BlockArg::Value(bit_count_r)]);

                // --- Allocate match context inline ---
                // Block params: data_ptr, bit_count (header >> 4 is already bits)
                builder.switch_to_block(alloc_mc_block);
                let data_ptr_param = builder.append_block_param(alloc_mc_block, types::I64);
                let total_bits_raw = builder.append_block_param(alloc_mc_block, types::I64);

                // Check inline heap space: cursor + 32 <= limit
                let ctx_val = builder.use_var(ctx_var);
                let cursor = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val, CTX_OFFSET_HEAP_CURSOR);
                let limit = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val, CTX_OFFSET_HEAP_LIMIT);
                let mc_size = builder.ins().iconst(types::I64, MATCH_CTX_ALLOC_BYTES);
                let new_cursor = builder.ins().iadd(cursor, mc_size);
                let has_space = builder.ins().icmp(IntCC::UnsignedLessThanOrEqual, new_cursor, limit);
                builder.ins().brif(has_space, alloc_fast_block, &[], slow_block, &[]);

                // --- Fast alloc: write match context at cursor ---
                builder.switch_to_block(alloc_fast_block);
                // Write header (MATCH_CTX_HEADER)
                let mc_header = builder.ins().iconst(types::I64, MATCH_CTX_HEADER as i64);
                builder.ins().store(MemFlags::trusted(), mc_header, cursor, 0);
                // Write orig_binary (tagged input term) at word 1
                builder.ins().store(MemFlags::trusted(), input_val, cursor, MATCH_CTX_BINARY_OFFSET);
                // Write bit_offset = 0 at word 2
                let zero = builder.ins().iconst(types::I64, 0);
                builder.ins().store(MemFlags::trusted(), zero, cursor, MATCH_CTX_BIT_OFFSET);
                // Write total_bits at word 3
                builder.ins().store(MemFlags::trusted(), total_bits_raw, cursor, MATCH_CTX_END_OFFSET);
                // Update ctx.heap_cursor
                let ctx_val2 = builder.use_var(ctx_var);
                builder.ins().store(MemFlags::trusted(), new_cursor, ctx_val2, CTX_OFFSET_HEAP_CURSOR);

                // Tag match_ctx: cursor | TAG_BOXED (TAG_BOXED=0, so just cursor)
                let mc_tagged = cursor; // TAG_BOXED == 0

                // Tag total_bits as small int
                let tag_bits_c = builder.ins().iconst(types::I64, TAG_BITS as i64);
                let total_shifted = builder.ins().ishl(total_bits_raw, tag_bits_c);
                let si_tag = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
                let total_tagged = builder.ins().bor(total_shifted, si_tag);

                builder.ins().jump(merge_block, &[
                    BlockArg::Value(data_ptr_param),
                    BlockArg::Value(total_tagged),
                    BlockArg::Value(mc_tagged),
                ]);

                // --- Slow path: call FFI ---
                builder.switch_to_block(slow_block);
                let slow_ret = self.emit_ffi_call(module, builder, "nera_rt_bs_start_match2_slow", ctx_var, &[input_val])?;
                // Read total_bits from ctx.result
                let ctx_val3 = builder.use_var(ctx_var);
                let slow_total = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val3, CTX_OFFSET_RESULT);
                // Read match_ctx from ctx.result2
                let slow_mc = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val3, CTX_OFFSET_RESULT2);
                builder.ins().jump(merge_block, &[
                    BlockArg::Value(slow_ret),
                    BlockArg::Value(slow_total),
                    BlockArg::Value(slow_mc),
                ]);

                // --- Merge block ---
                builder.switch_to_block(merge_block);
                let merge_base = builder.append_block_param(merge_block, types::I64);
                let merge_total = builder.append_block_param(merge_block, types::I64);
                let merge_mc = builder.append_block_param(merge_block, types::I64);

                if let Some(var) = var_map.get(base_ptr) {
                    builder.def_var(*var, merge_base);
                }
                if let Some(var) = var_map.get(total_bits) {
                    builder.def_var(*var, merge_total);
                }
                if let Some(var) = var_map.get(match_ctx) {
                    builder.def_var(*var, merge_mc);
                }
            }

            Instruction::BsMatchCtxAdvance(match_ctx, bit_offset) => {
                // The pattern's dominating size guards prove 0 <= delta <=
                // remaining. Only the exclusive tail continuation may advance;
                // no Rust reference or input pointer crosses a GC/reentry here.
                // Inline memory write: match_ctx.bit_offset += bit_offset
                // Match context layout: [header | orig_binary | bit_offset | total_bits]
                // bit_offset is at word 2 (byte offset 16 from the untagged pointer)
                let mc_val = builder.use_var(var_map[match_ctx]);
                let off_val = builder.use_var(var_map[bit_offset]);
                // Untag match_ctx pointer: clear low TAG bits
                let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
                let mc_ptr = builder.ins().band_not(mc_val, tag_mask);
                // Load current bit_offset from word 2 (byte offset 16)
                let old_off = builder.ins().load(types::I64, MemFlags::new(), mc_ptr, MATCH_CTX_BIT_OFFSET);
                // Untag the pattern bit_offset (it's a tagged small int)
                let shift = builder.ins().iconst(types::I64, TAG_BITS as i64);
                let off_raw = builder.ins().sshr(off_val, shift);
                // new_offset = old + pattern_offset
                let new_off = builder.ins().iadd(old_off, off_raw);
                // Store back to word 2
                builder.ins().store(MemFlags::new(), new_off, mc_ptr, MATCH_CTX_BIT_OFFSET);
            }

            Instruction::BinaryPutUtf8(bits_written, bin, offset, codepoint) => {
                let bin_val = builder.use_var(var_map[bin]);
                let off_val = builder.use_var(var_map[offset]);
                let cp_val = builder.use_var(var_map[codepoint]);
                let ret = self.emit_ffi_call(module, builder, "nera_rt_binary_put_utf8", ctx_var, &[bin_val, off_val, cp_val])?;
                if let Some(var) = var_map.get(bits_written) {
                    builder.def_var(*var, ret);
                }
                if !is_try_protected { self.emit_ffi_exception_check(module, builder, ctx_var)?; }
            }

            Instruction::BinaryPutUtf16(bits_written, bin, offset, codepoint, flags) => {
                let bin_val = builder.use_var(var_map[bin]);
                let off_val = builder.use_var(var_map[offset]);
                let cp_val = builder.use_var(var_map[codepoint]);
                let flags_val = builder.ins().iconst(
                    types::I64,
                    (flags.endianness as i64) << 8,
                );
                let ret = self.emit_ffi_call(module, builder, "nera_rt_binary_put_utf16", ctx_var, &[bin_val, off_val, cp_val, flags_val])?;
                if let Some(var) = var_map.get(bits_written) {
                    builder.def_var(*var, ret);
                }
                if !is_try_protected { self.emit_ffi_exception_check(module, builder, ctx_var)?; }
            }

            Instruction::BinaryPutUtf32(bits_written, bin, offset, codepoint, flags) => {
                let bin_val = builder.use_var(var_map[bin]);
                let off_val = builder.use_var(var_map[offset]);
                let cp_val = builder.use_var(var_map[codepoint]);
                let flags_val = builder.ins().iconst(
                    types::I64,
                    (flags.endianness as i64) << 8,
                );
                let ret = self.emit_ffi_call(module, builder, "nera_rt_binary_put_utf32", ctx_var, &[bin_val, off_val, cp_val, flags_val])?;
                if let Some(var) = var_map.get(bits_written) {
                    builder.def_var(*var, ret);
                }
                if !is_try_protected { self.emit_ffi_exception_check(module, builder, ctx_var)?; }
            }

            Instruction::BsGetUtf8(codepoint, bits_consumed, base_ptr, bit_offset, total_bits) => {
                let base_val = builder.use_var(var_map[base_ptr]);
                let off_val = builder.use_var(var_map[bit_offset]);
                let total_val = builder.use_var(var_map[total_bits]);
                let ret = self.emit_ffi_call(module, builder, "nera_rt_bs_get_utf8", ctx_var, &[base_val, off_val, total_val])?;
                if let Some(var) = var_map.get(bits_consumed) {
                    builder.def_var(*var, ret);
                }
                // Read codepoint from ctx.result
                let ctx_val = builder.use_var(ctx_var);
                let cp = builder.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    ctx_val,
                    CTX_OFFSET_RESULT,
                );
                if let Some(var) = var_map.get(codepoint) {
                    builder.def_var(*var, cp);
                }
            }

            Instruction::BsGetUtf16(codepoint, bits_consumed, base_ptr, bit_offset, total_bits, flags) => {
                let base_val = builder.use_var(var_map[base_ptr]);
                let off_val = builder.use_var(var_map[bit_offset]);
                let total_val = builder.use_var(var_map[total_bits]);
                let flags_val = builder.ins().iconst(
                    types::I64,
                    (flags.endianness as i64) << 8,
                );
                let ret = self.emit_ffi_call(module, builder, "nera_rt_bs_get_utf16", ctx_var, &[base_val, off_val, total_val, flags_val])?;
                if let Some(var) = var_map.get(bits_consumed) {
                    builder.def_var(*var, ret);
                }
                // Read codepoint from ctx.result
                let ctx_val = builder.use_var(ctx_var);
                let cp = builder.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    ctx_val,
                    CTX_OFFSET_RESULT,
                );
                if let Some(var) = var_map.get(codepoint) {
                    builder.def_var(*var, cp);
                }
            }

            Instruction::BsGetUtf32(codepoint, bits_consumed, base_ptr, bit_offset, total_bits, flags) => {
                let base_val = builder.use_var(var_map[base_ptr]);
                let off_val = builder.use_var(var_map[bit_offset]);
                let total_val = builder.use_var(var_map[total_bits]);
                let flags_val = builder.ins().iconst(
                    types::I64,
                    (flags.endianness as i64) << 8,
                );
                let ret = self.emit_ffi_call(module, builder, "nera_rt_bs_get_utf32", ctx_var, &[base_val, off_val, total_val, flags_val])?;
                if let Some(var) = var_map.get(bits_consumed) {
                    builder.def_var(*var, ret);
                }
                // Read codepoint from ctx.result
                let ctx_val = builder.use_var(ctx_var);
                let cp = builder.ins().load(
                    types::I64,
                    MemFlags::trusted(),
                    ctx_val,
                    CTX_OFFSET_RESULT,
                );
                if let Some(var) = var_map.get(codepoint) {
                    builder.def_var(*var, cp);
                }
            }

            Instruction::BinaryResize(bin, new_byte_size) => {
                let bin_val = builder.use_var(var_map[bin]);
                let size_val = builder.use_var(var_map[new_byte_size]);
                self.emit_ffi_call_void(module, builder, "nera_rt_binary_resize", ctx_var, &[bin_val, size_val])?;
            }

            Instruction::CheckReductions => {
                // No-op: reduction counting is handled exclusively by:
                // 1. Self-tail-call yield checks (terminators.rs) — inline
                //    decrement + conditional yield at every loop iteration.
                // 2. Back-edge YieldCheck (resume.rs) — decrement + yield at
                //    CFG back-edges detected by insert_backedge_yield_checks().
                //
                // Removing function-entry and pre-tail-call decrements avoids
                // redundant load/sub/store cycles. Non-tail-recursive functions
                // (like fib) don't yield mid-execution, but this is acceptable:
                // they eventually return, and callers have their own yield
                // points. BEAM has the same behavior for compiled code.
            }

            Instruction::YieldCheck => {
                // No-op here: the decrement + conditional yield logic is
                // emitted by emit_yield_check_status() in resume.rs, which
                // is called right after this instruction in the translation
                // loop. This avoids a redundant load of the reductions counter.
            }

            Instruction::SetLine(line) => {
                self.emit_set_line(module, builder, ctx_var, *line)?;
            }

            Instruction::MakeMap(result, pairs) => {
                // Call nera_rt_alloc_map(ctx, npairs, pairs_ptr)
                // Use the same declare_alloc_tuple pattern: custom signature
                let fid = {
                    let name = "nera_rt_alloc_map";
                    if let Some(fid) = self.runtime_funcs.get(name) {
                        *fid
                    } else {
                        let mut sig = module.make_signature();
                        sig.params.push(cranelift_codegen::ir::AbiParam::new(types::I64)); // ctx
                        sig.params.push(cranelift_codegen::ir::AbiParam::new(types::I32)); // npairs
                        sig.params.push(cranelift_codegen::ir::AbiParam::new(types::I64)); // pairs_ptr
                        sig.returns.push(cranelift_codegen::ir::AbiParam::new(types::I64));
                        let fid = module.declare_function(name, cranelift_module::Linkage::Import, &sig)
                            .map_err(|e| TranslateError::Module(e.to_string()))?;
                        self.runtime_funcs.insert(name.to_string(), fid);
                        fid
                    }
                };
                let func_ref = module.declare_func_in_func(fid, builder.func);
                let ctx_val = builder.use_var(ctx_var);
                let npairs = pairs.len() / 2;
                let npairs_val = builder.ins().iconst(types::I32, npairs as i64);

                // Spill pairs to stack
                let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                    cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                    (pairs.len() * 8) as u32,
                    3, // alignment log2 = 8
                ));
                for (i, p) in pairs.iter().enumerate() {
                    let val = builder.use_var(var_map[p]);
                    builder.ins().stack_store(val, slot, (i * 8) as i32);
                }
                let pairs_ptr = builder.ins().stack_addr(types::I64, slot, 0);

                let call = builder.ins().call(func_ref, &[ctx_val, npairs_val, pairs_ptr]);
                let ret = builder.inst_results(call)[0];
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::MapGet(result, map, key) => {
                let map_val = builder.use_var(var_map[map]);
                let key_val = builder.use_var(var_map[key]);
                let ret = self.emit_ffi_call(module, builder, "nera_rt_map_get", ctx_var, &[map_val, key_val])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }

                // Check for badmap/badkey exception
                // (skip if inside a try block — CheckException handles it)
                if !is_try_protected {
                    self.emit_ffi_exception_check(module, builder, ctx_var)?;
                }
            }

            Instruction::MapPut(result, map, key, value) => {
                let map_val = builder.use_var(var_map[map]);
                let key_val = builder.use_var(var_map[key]);
                let val_val = builder.use_var(var_map[value]);
                let ret = self.emit_ffi_call(module, builder, "nera_rt_map_put", ctx_var, &[map_val, key_val, val_val])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }

                // Check for badmap exception
                if !is_try_protected {
                    self.emit_ffi_exception_check(module, builder, ctx_var)?;
                }
            }

            Instruction::MapUpdateExact(result, map, key, value) => {
                let map_val = builder.use_var(var_map[map]);
                let key_val = builder.use_var(var_map[key]);
                let val_val = builder.use_var(var_map[value]);
                let ret = self.emit_ffi_call(module, builder, "nera_rt_map_update_exact", ctx_var, &[map_val, key_val, val_val])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }

                // Check for badmap/badkey exception
                if !is_try_protected {
                    self.emit_ffi_exception_check(module, builder, ctx_var)?;
                }
            }

            Instruction::RaiseException(result, class, reason) => {
                let ffi_name = match class {
                    nera_ir::ir::ExceptionClass::Throw => "nera_rt_throw",
                    nera_ir::ir::ExceptionClass::Error => "nera_rt_error",
                    nera_ir::ir::ExceptionClass::Exit => "nera_rt_exit",
                };
                let reason_val = builder.use_var(var_map[reason]);
                let ret = self.emit_ffi_call(module, builder, ffi_name, ctx_var, &[reason_val])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::CheckException(result) => {
                let ret = self.emit_ffi_call(module, builder, "nera_rt_check_exception", ctx_var, &[])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::GetExceptionClass(result) => {
                let ret = self.emit_ffi_call(module, builder, "nera_rt_get_exception_class", ctx_var, &[])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::GetExceptionReason(result) => {
                let ret = self.emit_ffi_call(module, builder, "nera_rt_get_exception_reason", ctx_var, &[])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::GetExceptionStacktrace(result) => {
                let ret = self.emit_ffi_call(module, builder, "nera_rt_get_exception_stacktrace", ctx_var, &[])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::ClearException => {
                self.emit_ffi_call_void(module, builder, "nera_rt_clear_exception", ctx_var, &[])?;
            }

            Instruction::MakeClosure(result, mangled_name, user_arity, env_vars) => {
                // Get the FuncId for the lifted lambda's *wrapper* function
                let wrapper_name = format!("{}_wrapper", mangled_name);
                let wrapper_fid = if let Some(&fid) = mod_ctx.func_ids.get(&wrapper_name) {
                    fid
                } else {
                    return Err(TranslateError::Codegen {
                        block_id: self.current_ir_block_id,
                        reason: format!(
                            "MakeClosure: wrapper not found for '{}'", mangled_name
                        ),
                    });
                };

                // Get function pointer to the wrapper
                let func_ref = module.declare_func_in_func(wrapper_fid, builder.func);
                let fn_ptr = builder.ins().func_addr(types::I64, func_ref);

                let arity_val = builder.ins().iconst(types::I64, *user_arity as i64);
                let env_count = env_vars.len();
                let env_count_val = builder.ins().iconst(types::I64, env_count as i64);

                if env_count > 0 {
                    // Spill env vars to a stack slot
                    let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                        (env_count * 8) as u32,
                        3, // alignment log2 = 8
                    ));
                    for (i, ev) in env_vars.iter().enumerate() {
                        let val = builder.use_var(var_map[ev]);
                        builder.ins().stack_store(val, slot, (i * 8) as i32);
                    }
                    let env_ptr = builder.ins().stack_addr(types::I64, slot, 0);

                    let ret = self.emit_ffi_call(module, builder, "nera_rt_alloc_closure", ctx_var,
                        &[fn_ptr, arity_val, env_ptr, env_count_val])?;
                    if let Some(var) = var_map.get(result) {
                        builder.def_var(*var, ret);
                    }
                } else {
                    // No env vars — pass null pointer
                    let null_ptr = builder.ins().iconst(types::I64, 0i64);
                    let ret = self.emit_ffi_call(module, builder, "nera_rt_alloc_closure", ctx_var,
                        &[fn_ptr, arity_val, null_ptr, env_count_val])?;
                    if let Some(var) = var_map.get(result) {
                        builder.def_var(*var, ret);
                    }
                }
            }

            Instruction::MakeRemoteFunRef(result, mod_id, fun_id, user_arity) => {
                // Load module and function atom terms
                let mod_val = self.load_atom_term(module, builder, mod_id.0)?;
                let fun_val = self.load_atom_term(module, builder, fun_id.0)?;
                let arity_val = builder.ins().iconst(types::I64, *user_arity as i64);

                // Call nera_rt_alloc_remote_fun_ref(ctx, mod_atom, fun_atom, arity)
                let ret = self.emit_ffi_call(module, builder, "nera_rt_alloc_remote_fun_ref", ctx_var,
                    &[mod_val, fun_val, arity_val])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }

            Instruction::MakeDynamicRemoteFunRef(result, mod_var, fun_var, arity_var) => {
                // mod and fun vars already hold tagged atom terms
                let mod_val = builder.use_var(var_map[mod_var]);
                let fun_val = builder.use_var(var_map[fun_var]);
                // Untag the small integer arity term
                let arity_term = builder.use_var(var_map[arity_var]);
                let shift = builder.ins().iconst(types::I64, TAG_BITS as i64);
                let arity_val = builder.ins().sshr(arity_term, shift);

                let ret = self.emit_ffi_call(module, builder, "nera_rt_alloc_remote_fun_ref", ctx_var,
                    &[mod_val, fun_val, arity_val])?;
                if let Some(var) = var_map.get(result) {
                    builder.def_var(*var, ret);
                }
            }
        }

        Ok(())
    }
}
