//! FFI call emission, stack frame management, and exception checks.

use cranelift_codegen::ir::{types, BlockArg, InstBuilder, MemFlags, Value};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::Module;
use cranelift_object::ObjectModule;

use crate::term_layout::*;
use super::{declare_runtime_func, declare_runtime_func_void, FuncTranslator, TranslateError};

/// Arithmetic operations supported by the immediate integer fast path.
#[derive(Clone, Copy)]
pub(super) enum InlineArithmetic {
    Add,
    Subtract,
    Multiply,
}

/// Construction raises; matching returns nil; floats restrict the width domain.
#[derive(Clone, Copy)]
pub(super) enum SegmentSizeKind { Construction, Matching, Float }

impl FuncTranslator {
    pub(super) fn emit_ffi_call(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ffi_name: &str,
        ctx_var: Variable,
        args: &[Value],
    ) -> Result<Value, TranslateError> {
        let fid = declare_runtime_func(module, ffi_name, args.len(), &mut self.runtime_funcs, self.c_call_conv)?;
        let func_ref = module.declare_func_in_func(fid, builder.func);
        let ctx_val = builder.use_var(ctx_var);
        let mut call_args = vec![ctx_val];
        call_args.extend_from_slice(args);
        let call = builder.ins().call(func_ref, &call_args);
        Ok(builder.inst_results(call)[0])
    }

    /// Emit a call to a void FFI function (no return value used).
    pub(super) fn emit_ffi_call_void(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ffi_name: &str,
        ctx_var: Variable,
        args: &[Value],
    ) -> Result<(), TranslateError> {
        let fid = declare_runtime_func_void(module, ffi_name, args.len(), &mut self.runtime_funcs, self.c_call_conv)?;
        let func_ref = module.declare_func_in_func(fid, builder.func);
        let ctx_val = builder.use_var(ctx_var);
        let mut call_args = vec![ctx_val];
        call_args.extend_from_slice(args);
        builder.ins().call(func_ref, &call_args);
        Ok(())
    }

    /// Push this function frame onto the per-process stack trace.
    /// Push a compact frame (func_index + line=0) using cursor-based addressing.
    /// Pre-allocated array means no reallocation — just a pointer comparison
    /// for the overflow check.
    pub(super) fn emit_stack_push(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        func_index: u32,
    ) -> Result<(), TranslateError> {
        let ctx_val = builder.use_var(ctx_var);

        // Load cursor and limit (both pointers)
        let cursor = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val, CTX_OFFSET_STACK_CURSOR);
        let limit = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val, CTX_OFFSET_STACK_LIMIT);

        // Check cursor < limit (unsigned pointer comparison)
        let has_space = builder.ins().icmp(IntCC::UnsignedLessThan, cursor, limit);

        let fast_block = builder.create_block();
        let overflow_block = builder.create_block();
        builder.set_cold_block(overflow_block);
        let merge_block = builder.create_block();

        builder.ins().brif(has_space, fast_block, &[], overflow_block, &[]);

        // Fast path: store func_index at [cursor+0], advance cursor by 8
        builder.switch_to_block(fast_block);
        builder.seal_block(fast_block);

        let fi_val = builder.ins().iconst(types::I32, func_index as i64);
        builder.ins().store(MemFlags::trusted(), fi_val, cursor, CF_OFFSET_FUNC_INDEX);
        // line stays 0 (zeroinit from pre-allocation) — set_line will update it

        let new_cursor = builder.ins().iadd_imm(cursor, COMPACT_FRAME_SIZE as i64);
        let ctx_val2 = builder.use_var(ctx_var);
        builder.ins().store(MemFlags::trusted(), new_cursor, ctx_val2, CTX_OFFSET_STACK_CURSOR);

        builder.ins().jump(merge_block, &[]);

        // Cold overflow path: raise system_limit error via FFI (captures trace)
        builder.switch_to_block(overflow_block);
        builder.seal_block(overflow_block);

        let system_limit_term = builder.ins().iconst(types::I64,
            crate::term_layout::encode_atom(nera_runtime::atom_table::ATOM_SYSTEM_LIMIT) as i64);
        self.emit_ffi_call(module, builder, "nera_rt_error", ctx_var, &[system_limit_term])?;
        // Return only for an actual overflow. A resumed caller may arrive with
        // a pending exception that its continuation must handle. No frame was
        // pushed on this path, so do not pop one.
        let nil = builder.ins().iconst(types::I64, NIL_VALUE as i64);
        builder.ins().return_(&[nil]);

        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);

        Ok(())
    }

    /// Update the line number on the current compact frame.
    /// Current frame's line field is at cursor - COMPACT_FRAME_SIZE + CF_OFFSET_LINE = cursor - 4.
    pub(super) fn emit_set_line(
        &mut self,
        _module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        line: u32,
    ) -> Result<(), TranslateError> {
        let ctx_val = builder.use_var(ctx_var);

        // Load cursor (points past the current frame)
        let cursor = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val, CTX_OFFSET_STACK_CURSOR);

        // Store line at cursor - 4 (= cursor - COMPACT_FRAME_SIZE + CF_OFFSET_LINE)
        let line_val = builder.ins().iconst(types::I32, line as i64);
        let line_offset = -(COMPACT_FRAME_SIZE - CF_OFFSET_LINE);
        builder.ins().store(MemFlags::trusted(), line_val, cursor, line_offset);

        Ok(())
    }

    /// Pop one compact frame by decrementing the cursor.
    pub(super) fn emit_stack_pop(
        &mut self,
        _module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
    ) -> Result<(), TranslateError> {
        let ctx_val = builder.use_var(ctx_var);
        let cursor = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val, CTX_OFFSET_STACK_CURSOR);
        let new_cursor = builder.ins().iadd_imm(cursor, -(COMPACT_FRAME_SIZE as i64));
        builder.ins().store(MemFlags::trusted(), new_cursor, ctx_val, CTX_OFFSET_STACK_CURSOR);
        Ok(())
    }

    /// Pop the current frame, then return the provided value.
    pub(super) fn emit_stack_pop_and_return(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        ret_val: Value,
    ) -> Result<(), TranslateError> {
        self.emit_stack_pop(module, builder, ctx_var)?;
        builder.ins().return_(&[ret_val]);
        Ok(())
    }
    /// Emit inline small-int arithmetic with FFI fallback.
    /// Multiplication checks the scaled product before publishing a tagged term.
    #[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
    pub(super) fn emit_inline_arith(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        lhs: Value,
        rhs: Value,
        operation: InlineArithmetic,
        check_exception_in_slow_path: bool,
    ) -> Result<Value, TranslateError> {


        // Check both operands are small ints
        let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
        let tag_a = builder.ins().band(lhs, tag_mask);
        let tag_b = builder.ins().band(rhs, tag_mask);
        let small_int_tag = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
        let ok_a = builder.ins().icmp(IntCC::Equal, tag_a, small_int_tag);
        let ok_b = builder.ins().icmp(IntCC::Equal, tag_b, small_int_tag);
        let both = builder.ins().band(ok_a, ok_b);

        let fast_block = builder.create_block();
        let slow_block = builder.create_block();
        builder.set_cold_block(slow_block);
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        builder.ins().brif(both, fast_block, &[], slow_block, &[]);

        // Fast path: untag, compute, overflow check, retag
        builder.switch_to_block(fast_block);
        builder.seal_block(fast_block);
        let shift = builder.ins().iconst(types::I64, TAG_BITS as i64);
        let a_raw = builder.ins().sshr(lhs, shift);
        let (result, fits, ffi_name) = match operation {
            InlineArithmetic::Multiply => {
                // Clearing the tag leaves b * 8, which fits i64 for every
                // signed 61-bit operand. A checked a * (b * 8) therefore
                // proves that a * b fits 61 bits, without first truncating it.
                let scaled_b = builder.ins().band_imm(rhs, !(TAG_MASK as i64));
                let (scaled_product, overflow) = builder.ins().smul_overflow(a_raw, scaled_b);
                let fits = builder.ins().icmp_imm(IntCC::Equal, overflow, 0);
                (scaled_product, fits, "nera_rt_mul")
            }
            InlineArithmetic::Add | InlineArithmetic::Subtract => {
                // Adding/subtracting two signed 61-bit values cannot overflow i64.
                let b_raw = builder.ins().sshr(rhs, shift);
                let (raw, ffi_name) = match operation {
                    InlineArithmetic::Add => (builder.ins().iadd(a_raw, b_raw), "nera_rt_add"),
                    _ => (builder.ins().isub(a_raw, b_raw), "nera_rt_sub"),
                };
                let bias = builder.ins().iconst(types::I64, SMALL_INT_OVERFLOW_BIAS);
                let shifted = builder.ins().iadd(raw, bias);
                let range = builder.ins().iconst(types::I64, SMALL_INT_RANGE as i64);
                let fits = builder.ins().icmp(IntCC::UnsignedLessThan, shifted, range);
                (raw, fits, ffi_name)
            }
        };

        let retag_block = builder.create_block();
        builder.ins().brif(fits, retag_block, &[], slow_block, &[]);

        // Retag and jump to merge
        builder.switch_to_block(retag_block);
        builder.seal_block(retag_block);
        let tagged = match operation {
            InlineArithmetic::Multiply => result,
            _ => builder.ins().ishl(result, shift),
        };
        let tag_val = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
        let result_tagged = builder.ins().bor(tagged, tag_val);
        builder.ins().jump(merge_block, &[BlockArg::Value(result_tagged)]);

        // Slow path: FFI fallback
        builder.switch_to_block(slow_block);
        builder.seal_block(slow_block);
        let slow_ret = self.emit_ffi_call(module, builder, ffi_name, ctx_var, &[lhs, rhs])?;
        if check_exception_in_slow_path {
            // Check for exception here so the fast path (which never raises) avoids the cost.
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

        // Merge
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(builder.block_params(merge_block)[0])
    }

    /// Emit inline small-int div/rem with FFI fallback.
    /// Checks both small-int AND divisor != 0 before inlining.
    #[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
    pub(super) fn emit_inline_div_rem(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        lhs: Value,
        rhs: Value,
        ffi_name: &str,
        is_rem: bool,
        check_exception_in_slow_path: bool,
    ) -> Result<Value, TranslateError> {


        // Check both operands are small ints
        let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
        let tag_a = builder.ins().band(lhs, tag_mask);
        let tag_b = builder.ins().band(rhs, tag_mask);
        let small_int_tag = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
        let ok_a = builder.ins().icmp(IntCC::Equal, tag_a, small_int_tag);
        let ok_b = builder.ins().icmp(IntCC::Equal, tag_b, small_int_tag);
        let both = builder.ins().band(ok_a, ok_b);

        let check_zero_block = builder.create_block();
        let slow_block = builder.create_block();
        builder.set_cold_block(slow_block);
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        builder.ins().brif(both, check_zero_block, &[], slow_block, &[]);

        // Check divisor != 0
        builder.switch_to_block(check_zero_block);
        builder.seal_block(check_zero_block);
        let shift = builder.ins().iconst(types::I64, TAG_BITS as i64);
        let a_raw = builder.ins().sshr(lhs, shift);
        let b_raw = builder.ins().sshr(rhs, shift);
        let zero = builder.ins().iconst(types::I64, 0);
        let b_nonzero = builder.ins().icmp(IntCC::NotEqual, b_raw, zero);

        let compute_block = builder.create_block();
        builder.ins().brif(b_nonzero, compute_block, &[], slow_block, &[]);

        // Compute div/rem
        builder.switch_to_block(compute_block);
        builder.seal_block(compute_block);
        let result_raw = if is_rem {
            builder.ins().srem(a_raw, b_raw)
        } else {
            builder.ins().sdiv(a_raw, b_raw)
        };

        // Overflow check (covers MIN / -1 edge case for div)
        let bias = builder.ins().iconst(types::I64, SMALL_INT_OVERFLOW_BIAS);
        let shifted = builder.ins().iadd(result_raw, bias);
        let range = builder.ins().iconst(types::I64, SMALL_INT_RANGE as i64);
        let fits = builder.ins().icmp(IntCC::UnsignedLessThan, shifted, range);

        let retag_block = builder.create_block();
        builder.ins().brif(fits, retag_block, &[], slow_block, &[]);

        // Retag
        builder.switch_to_block(retag_block);
        builder.seal_block(retag_block);
        let tagged = builder.ins().ishl(result_raw, shift);
        let tag_val = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
        let result_tagged = builder.ins().bor(tagged, tag_val);
        builder.ins().jump(merge_block, &[BlockArg::Value(result_tagged)]);

        // Slow path: FFI
        builder.switch_to_block(slow_block);
        builder.seal_block(slow_block);
        let slow_ret = self.emit_ffi_call(module, builder, ffi_name, ctx_var, &[lhs, rhs])?;
        if check_exception_in_slow_path {
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

        // Merge
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(builder.block_params(merge_block)[0])
    }

    /// Inline validation for an all-source segment; only errors call the runtime.
    /// The unit is a nonzero compiler-generated constant in 1..=256.
    pub(super) fn emit_inline_binary_source_size(
        &mut self, module: &mut ObjectModule, builder: &mut FunctionBuilder,
        ctx_var: Variable, args: &[Value], check_exception: bool,
    ) -> Result<Value, TranslateError> {
        let source = args[0];
        let tag = builder.ins().band_imm(source, TAG_MASK as i64);
        let boxed = builder.ins().icmp_imm(IntCC::Equal, tag, TAG_BOXED as i64);
        let descriptor = builder.create_block();
        let fast = builder.create_block();
        let slow = builder.create_block();
        let merge = builder.create_block();
        builder.append_block_param(merge, types::I64);
        builder.ins().brif(boxed, descriptor, &[], slow, &[]);
        builder.switch_to_block(descriptor);
        builder.seal_block(descriptor);
        let pointer = builder.ins().band_imm(source, !(TAG_MASK | EXTERNAL_BIT) as i64);
        let header = builder.ins().load(types::I64, MemFlags::new(), pointer, 0);
        let subtype = builder.ins().band_imm(header, 15);
        let heap = builder.ins().icmp_imm(IntCC::Equal, subtype, BOXED_HEAP_BINARY as i64);
        let refc = builder.ins().icmp_imm(IntCC::Equal, subtype, BOXED_REFC_BINARY as i64);
        let binary = builder.ins().bor(heap, refc);
        let bits = builder.ins().ushr_imm(header, 4);
        let unit = builder.ins().sshr_imm(args[1], TAG_BITS as i64);
        let remainder = builder.ins().urem(bits, unit);
        let divisible = builder.ins().icmp_imm(IntCC::Equal, remainder, 0);
        let valid = builder.ins().band(binary, divisible);
        builder.ins().brif(valid, fast, &[], slow, &[]);
        builder.switch_to_block(fast);
        builder.seal_block(fast);
        let tagged = builder.ins().ishl_imm(bits, TAG_BITS as i64);
        let tagged = builder.ins().bor_imm(tagged, TAG_SMALL_INT as i64);
        builder.ins().jump(merge, &[BlockArg::Value(tagged)]);
        builder.switch_to_block(slow);
        builder.seal_block(slow);
        let result = self.emit_ffi_call(module, builder, "nera_rt_binary_source_size", ctx_var, args)?;
        if check_exception { self.emit_ffi_exception_check(module, builder, ctx_var)?; }
        builder.ins().jump(merge, &[BlockArg::Value(result)]);
        builder.switch_to_block(merge);
        builder.seal_block(merge);
        Ok(builder.block_params(merge)[0])
    }

    /// Check and scale a segment size without an FFI call on valid small ints.
    /// The unit is a compiler-generated constant in 1..=256.
    pub(super) fn emit_inline_segment_size(
        &mut self, module: &mut ObjectModule, builder: &mut FunctionBuilder,
        ctx_var: Variable, args: &[Value], check_exception: bool, kind: SegmentSizeKind,
    ) -> Result<Value, TranslateError> {
        let float_width = matches!(kind, SegmentSizeKind::Float);
        let match_size = matches!(kind, SegmentSizeKind::Matching);
        let size = args[0];
        let unit = builder.ins().sshr_imm(args[1], TAG_BITS as i64);
        let tag = builder.ins().band_imm(size, TAG_MASK as i64);
        let integer = builder.ins().icmp_imm(IntCC::Equal, tag, TAG_SMALL_INT as i64);
        let count = builder.ins().sshr_imm(size, TAG_BITS as i64);
        let max = builder.ins().iconst(types::I64, if float_width { 64 } else { nera_runtime::term::SMALL_INT_MAX });
        let limit = builder.ins().udiv(max, unit);
        // An unsigned comparison rejects negative values as well as overflow.
        let in_range = builder.ins().icmp(IntCC::UnsignedLessThanOrEqual, count, limit);
        let mut valid = builder.ins().band(integer, in_range);
        // Multiplication is wrapping IR; the range check above ensures only
        // bounded products can take the fast path, including float widths.
        let bits = builder.ins().imul(count, unit);
        if match_size {
            // Every rejected input is a failed pattern, never an exception.
            // Select the sentinel directly: no cold FFI call/merge is needed.
            let tagged = builder.ins().ishl_imm(bits, TAG_BITS as i64);
            let tagged = builder.ins().bor_imm(tagged, TAG_SMALL_INT as i64);
            let nil = builder.ins().iconst(types::I64, NIL_VALUE as i64);
            return Ok(builder.ins().select(valid, tagged, nil));
        }

        if float_width {
            let half = builder.ins().icmp_imm(IntCC::Equal, bits, 16);
            let single = builder.ins().icmp_imm(IntCC::Equal, bits, 32);
            let double = builder.ins().icmp_imm(IntCC::Equal, bits, 64);
            let supported = builder.ins().bor(half, single);
            let supported = builder.ins().bor(supported, double);
            valid = builder.ins().band(valid, supported);
        }
        let fast = builder.create_block();
        let slow = builder.create_block();
        let merge = builder.create_block();
        builder.append_block_param(merge, types::I64);
        builder.ins().brif(valid, fast, &[], slow, &[]);
        builder.switch_to_block(fast);
        builder.seal_block(fast);
        let tagged = builder.ins().ishl_imm(bits, TAG_BITS as i64);
        let tagged = builder.ins().bor_imm(tagged, TAG_SMALL_INT as i64);
        builder.ins().jump(merge, &[BlockArg::Value(tagged)]);
        builder.switch_to_block(slow);
        builder.seal_block(slow);
        let helper = if float_width { "nera_rt_float_segment_size" } else { "nera_rt_binary_segment_size" };
        let result = self.emit_ffi_call(module, builder, helper, ctx_var, args)?;
        if check_exception { self.emit_ffi_exception_check(module, builder, ctx_var)?; }
        builder.ins().jump(merge, &[BlockArg::Value(result)]);
        builder.switch_to_block(merge);
        builder.seal_block(merge);
        Ok(builder.block_params(merge)[0])
    }

    /// Emit inline small-int comparison with FFI fallback.
    /// Key insight: tagged small ints preserve signed ordering.
    #[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
    pub(super) fn emit_inline_cmp(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        lhs: Value,
        rhs: Value,
        cc: IntCC,
        ffi_name: &str,
    ) -> Result<Value, TranslateError> {


        // Check both operands are small ints
        let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
        let tag_a = builder.ins().band(lhs, tag_mask);
        let tag_b = builder.ins().band(rhs, tag_mask);
        let small_int_tag = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
        let ok_a = builder.ins().icmp(IntCC::Equal, tag_a, small_int_tag);
        let ok_b = builder.ins().icmp(IntCC::Equal, tag_b, small_int_tag);
        let both = builder.ins().band(ok_a, ok_b);

        let fast_block = builder.create_block();
        let slow_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        builder.ins().brif(both, fast_block, &[], slow_block, &[]);

        // Fast path: compare tagged values directly (ordering preserved)
        builder.switch_to_block(fast_block);
        builder.seal_block(fast_block);
        let cmp = builder.ins().icmp(cc, lhs, rhs);
        let true_val = self.load_true_atom(module, builder)?;
        let false_val = self.load_false_atom(module, builder)?;
        let fast_result = builder.ins().select(cmp, true_val, false_val);
        builder.ins().jump(merge_block, &[BlockArg::Value(fast_result)]);

        // Slow path: FFI for mixed types
        builder.switch_to_block(slow_block);
        builder.seal_block(slow_block);
        let slow_ret = self.emit_ffi_call(module, builder, ffi_name, ctx_var, &[lhs, rhs])?;
        builder.ins().jump(merge_block, &[BlockArg::Value(slow_ret)]);

        // Merge
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(builder.block_params(merge_block)[0])
    }

    /// Emit inline band/bor for small ints. These operate directly on tagged values
    /// because the identical tag bits (010) are preserved by AND and OR.
    /// Result always fits in small-int range.
    #[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
    pub(super) fn emit_inline_bitwise_direct(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        lhs: Value,
        rhs: Value,
        ffi_name: &str,
        op_fn: impl FnOnce(&mut FunctionBuilder, Value, Value) -> Value,
        check_exception_in_slow_path: bool,
    ) -> Result<Value, TranslateError> {
        let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
        let tag_a = builder.ins().band(lhs, tag_mask);
        let tag_b = builder.ins().band(rhs, tag_mask);
        let small_int_tag = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
        let ok_a = builder.ins().icmp(IntCC::Equal, tag_a, small_int_tag);
        let ok_b = builder.ins().icmp(IntCC::Equal, tag_b, small_int_tag);
        let both = builder.ins().band(ok_a, ok_b);

        let fast_block = builder.create_block();
        let slow_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        builder.ins().brif(both, fast_block, &[], slow_block, &[]);

        // Fast path: operate directly on tagged values
        builder.switch_to_block(fast_block);
        builder.seal_block(fast_block);
        let fast_result = op_fn(builder, lhs, rhs);
        builder.ins().jump(merge_block, &[BlockArg::Value(fast_result)]);

        // Slow path: FFI
        builder.switch_to_block(slow_block);
        builder.seal_block(slow_block);
        let slow_ret = self.emit_ffi_call(module, builder, ffi_name, ctx_var, &[lhs, rhs])?;
        if check_exception_in_slow_path {
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
        Ok(builder.block_params(merge_block)[0])
    }

    /// Emit inline bxor for small ints. XOR zeroes the tag bits (010^010=000),
    /// so we re-set them after the operation.
    pub(super) fn emit_inline_bxor(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        lhs: Value,
        rhs: Value,
        check_exception_in_slow_path: bool,
    ) -> Result<Value, TranslateError> {
        let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
        let tag_a = builder.ins().band(lhs, tag_mask);
        let tag_b = builder.ins().band(rhs, tag_mask);
        let small_int_tag = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
        let ok_a = builder.ins().icmp(IntCC::Equal, tag_a, small_int_tag);
        let ok_b = builder.ins().icmp(IntCC::Equal, tag_b, small_int_tag);
        let both = builder.ins().band(ok_a, ok_b);

        let fast_block = builder.create_block();
        let slow_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        builder.ins().brif(both, fast_block, &[], slow_block, &[]);

        // Fast path: xor tagged values, then re-set the tag
        builder.switch_to_block(fast_block);
        builder.seal_block(fast_block);
        let xored = builder.ins().bxor(lhs, rhs);
        let fast_result = builder.ins().bor(xored, small_int_tag);
        builder.ins().jump(merge_block, &[BlockArg::Value(fast_result)]);

        // Slow path: FFI
        builder.switch_to_block(slow_block);
        builder.seal_block(slow_block);
        let slow_ret = self.emit_ffi_call(module, builder, "nera_rt_bxor", ctx_var, &[lhs, rhs])?;
        if check_exception_in_slow_path {
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
        Ok(builder.block_params(merge_block)[0])
    }

    /// Emit inline bsl/bsr for small ints. Checks shift amount is in [0, 60] for bsl
    /// (with overflow check on result) or [0, 63] for bsr (no overflow possible).
    #[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
    pub(super) fn emit_inline_shift(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        lhs: Value,
        rhs: Value,
        ffi_name: &str,
        is_right: bool,
        check_exception_in_slow_path: bool,
    ) -> Result<Value, TranslateError> {
        let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
        let tag_a = builder.ins().band(lhs, tag_mask);
        let tag_b = builder.ins().band(rhs, tag_mask);
        let small_int_tag = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
        let ok_a = builder.ins().icmp(IntCC::Equal, tag_a, small_int_tag);
        let ok_b = builder.ins().icmp(IntCC::Equal, tag_b, small_int_tag);
        let both = builder.ins().band(ok_a, ok_b);

        let check_shift_block = builder.create_block();
        let slow_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        builder.ins().brif(both, check_shift_block, &[], slow_block, &[]);

        // Check shift amount is in valid range (unsigned compare handles negative too)
        builder.switch_to_block(check_shift_block);
        builder.seal_block(check_shift_block);
        let shift = builder.ins().iconst(types::I64, TAG_BITS as i64);
        let a_raw = builder.ins().sshr(lhs, shift);
        let b_raw = builder.ins().sshr(rhs, shift);

        let max_shift = if is_right { 64i64 } else { 61i64 };
        let max_shift_val = builder.ins().iconst(types::I64, max_shift);
        let shift_ok = builder.ins().icmp(IntCC::UnsignedLessThan, b_raw, max_shift_val);

        let compute_block = builder.create_block();
        builder.ins().brif(shift_ok, compute_block, &[], slow_block, &[]);

        builder.switch_to_block(compute_block);
        builder.seal_block(compute_block);

        if is_right {
            // bsr: arithmetic right shift, result always fits
            let result_raw = builder.ins().sshr(a_raw, b_raw);
            let tagged = builder.ins().ishl(result_raw, shift);
            let tag_val = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
            let result_tagged = builder.ins().bor(tagged, tag_val);
            builder.ins().jump(merge_block, &[BlockArg::Value(result_tagged)]);
        } else {
            // Include the tag bits in the shift, then reverse it arithmetically.
            // Equality proves both that no significant bits were lost and that
            // the result fits the signed small-integer payload. b_raw <= 60,
            // so total_shift <= 63 and the machine shift count cannot wrap.
            let total_shift = builder.ins().iadd_imm(b_raw, TAG_BITS as i64);
            let tagged = builder.ins().ishl(a_raw, total_shift);
            let restored = builder.ins().sshr(tagged, total_shift);
            let fits = builder.ins().icmp(IntCC::Equal, restored, a_raw);

            let retag_block = builder.create_block();
            builder.ins().brif(fits, retag_block, &[], slow_block, &[]);

            builder.switch_to_block(retag_block);
            builder.seal_block(retag_block);
            let tag_val = builder.ins().iconst(types::I64, TAG_SMALL_INT as i64);
            let result_tagged = builder.ins().bor(tagged, tag_val);
            builder.ins().jump(merge_block, &[BlockArg::Value(result_tagged)]);
        }

        // Slow path: FFI
        builder.switch_to_block(slow_block);
        builder.seal_block(slow_block);
        let slow_ret = self.emit_ffi_call(module, builder, ffi_name, ctx_var, &[lhs, rhs])?;
        if check_exception_in_slow_path {
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
        Ok(builder.block_params(merge_block)[0])
    }

    /// After a local function call, check if the callee returned via the
    /// tail-call trampoline (STATUS_TAIL_CALL).  If so, call
    /// `nera_rt_resolve_tail_calls` and load the resolved result from
    /// `ctx.result`.  Otherwise, use the direct return value.
    pub(super) fn emit_resolve_tail_call(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        raw_ret: Value,
    ) -> Result<Value, TranslateError> {
        let ctx_val = builder.use_var(ctx_var);
        let status = builder.ins().load(types::I32, MemFlags::trusted(), ctx_val, CTX_OFFSET_STATUS);
        let tc_const = builder.ins().iconst(types::I32, STATUS_TAIL_CALL as i64);
        let is_tail_call = builder.ins().icmp(IntCC::Equal, status, tc_const);

        let resolve_block = builder.create_block();
        builder.set_cold_block(resolve_block);
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        // Fast path: not a tail call — use return value directly
        builder.ins().brif(is_tail_call, resolve_block, &[], merge_block, &[BlockArg::Value(raw_ret)]);

        // Slow path: resolve tail call chain via FFI
        builder.switch_to_block(resolve_block);
        builder.seal_block(resolve_block);
        self.emit_ffi_call_void(module, builder, "nera_rt_resolve_tail_calls", ctx_var, &[])?;
        // Load resolved result from ctx.result
        let ctx_val2 = builder.use_var(ctx_var);
        let resolved = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val2, CTX_OFFSET_RESULT);
        builder.ins().jump(merge_block, &[BlockArg::Value(resolved)]);

        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(builder.block_params(merge_block)[0])
    }

    /// Emit inline cons cell allocation with FFI slow-path fallback.
    /// Fast path: load ctx.heap_cursor, check space (2 words = 16 bytes),
    /// write head+tail, bump cursor, tag as cons.
    /// Slow path: call nera_rt_alloc_cons_slow which also refills the inline region.
    pub(super) fn emit_inline_alloc_cons(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        head: Value,
        tail: Value,
    ) -> Result<Value, TranslateError> {
        let ctx_val = builder.use_var(ctx_var);

        // Load heap_cursor and heap_limit from ProcessContext
        let cursor = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val, CTX_OFFSET_HEAP_CURSOR);
        let limit = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val, CTX_OFFSET_HEAP_LIMIT);

        // Check if there's space for 2 words (16 bytes)
        let cons_size = builder.ins().iconst(types::I64, 16); // 2 * WORD_SIZE
        let new_cursor = builder.ins().iadd(cursor, cons_size);
        let has_space = builder.ins().icmp(IntCC::UnsignedLessThanOrEqual, new_cursor, limit);

        let fast_block = builder.create_block();
        let slow_block = builder.create_block();
        builder.set_cold_block(slow_block);
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        builder.ins().brif(has_space, fast_block, &[], slow_block, &[]);

        // Fast path: write head+tail at cursor, bump cursor, tag as cons
        builder.switch_to_block(fast_block);
        builder.seal_block(fast_block);

        // Write head at cursor[0], tail at cursor[1]
        builder.ins().store(MemFlags::trusted(), head, cursor, 0);
        builder.ins().store(MemFlags::trusted(), tail, cursor, 8);

        // Update ctx.heap_cursor = new_cursor
        let ctx_val2 = builder.use_var(ctx_var);
        builder.ins().store(MemFlags::trusted(), new_cursor, ctx_val2, CTX_OFFSET_HEAP_CURSOR);

        // Tag as cons: cursor | TAG_CONS
        let tag = builder.ins().iconst(types::I64, TAG_CONS as i64);
        let tagged = builder.ins().bor(cursor, tag);
        builder.ins().jump(merge_block, &[BlockArg::Value(tagged)]);

        // Slow path: FFI call (allocates + refills inline region)
        builder.switch_to_block(slow_block);
        builder.seal_block(slow_block);
        let slow_ret = self.emit_ffi_call(module, builder, "nera_rt_alloc_cons_slow", ctx_var, &[head, tail])?;
        builder.ins().jump(merge_block, &[BlockArg::Value(slow_ret)]);

        // Merge
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(builder.block_params(merge_block)[0])
    }

    /// Inline tuple allocation with bump-pointer fast path.
    /// Fast path: check space for (1 + arity) words, write header + elements, bump cursor, tag as boxed.
    /// Slow path: call nera_rt_alloc_tuple_slow which also refills the inline region.
    pub(super) fn emit_inline_alloc_tuple(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
        elem_vals: &[Value],
    ) -> Result<Value, TranslateError> {
        use crate::term_layout::{TAG_BOXED, BoxedSubtype, encode_boxed_header};

        let arity = elem_vals.len();
        let total_words = 1 + arity; // header + elements
        let total_bytes = (total_words * 8) as i64;

        let ctx_val = builder.use_var(ctx_var);

        // Load heap_cursor and heap_limit
        let cursor = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val, CTX_OFFSET_HEAP_CURSOR);
        let limit = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val, CTX_OFFSET_HEAP_LIMIT);

        // Check space
        let size_const = builder.ins().iconst(types::I64, total_bytes);
        let new_cursor = builder.ins().iadd(cursor, size_const);
        let has_space = builder.ins().icmp(IntCC::UnsignedLessThanOrEqual, new_cursor, limit);

        let fast_block = builder.create_block();
        let slow_block = builder.create_block();
        builder.set_cold_block(slow_block);
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        builder.ins().brif(has_space, fast_block, &[], slow_block, &[]);

        // Fast path: write header + elements at cursor, bump cursor, tag as boxed
        builder.switch_to_block(fast_block);
        builder.seal_block(fast_block);

        // Write header at cursor[0]
        let header_val = builder.ins().iconst(types::I64, encode_boxed_header(BoxedSubtype::Tuple, arity as u64) as i64);
        builder.ins().store(MemFlags::trusted(), header_val, cursor, 0);

        // Write each element at cursor[1..arity]
        for (i, &val) in elem_vals.iter().enumerate() {
            builder.ins().store(MemFlags::trusted(), val, cursor, ((1 + i) * 8) as i32);
        }

        // Update ctx.heap_cursor = new_cursor
        let ctx_val2 = builder.use_var(ctx_var);
        builder.ins().store(MemFlags::trusted(), new_cursor, ctx_val2, CTX_OFFSET_HEAP_CURSOR);

        // Tag as boxed: cursor | TAG_BOXED (TAG_BOXED = 0, so just cursor)
        let tagged = if TAG_BOXED == 0 {
            cursor
        } else {
            let tag = builder.ins().iconst(types::I64, TAG_BOXED as i64);
            builder.ins().bor(cursor, tag)
        };
        builder.ins().jump(merge_block, &[BlockArg::Value(tagged)]);

        // Slow path: stack-store elements, call nera_rt_alloc_tuple_slow
        builder.switch_to_block(slow_block);
        builder.seal_block(slow_block);

        // Create stack slot for elements
        let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            (arity * 8) as u32,
            3, // align to 8 bytes
        ));
        for (i, &val) in elem_vals.iter().enumerate() {
            builder.ins().stack_store(val, slot, (i * 8) as i32);
        }
        let slot_addr = builder.ins().stack_addr(types::I64, slot, 0);
        let arity_val = builder.ins().iconst(types::I32, arity as i64);

        // Declare nera_rt_alloc_tuple_slow (same signature as nera_rt_alloc_tuple)
        let fid = {
            let name = "nera_rt_alloc_tuple_slow";
            if let Some(fid) = self.runtime_funcs.get(name) {
                *fid
            } else {
                let mut sig = module.make_signature();
                sig.params.push(cranelift_codegen::ir::AbiParam::new(types::I64)); // ctx
                sig.params.push(cranelift_codegen::ir::AbiParam::new(types::I32)); // arity
                sig.params.push(cranelift_codegen::ir::AbiParam::new(types::I64)); // elements ptr
                sig.returns.push(cranelift_codegen::ir::AbiParam::new(types::I64));
                let fid = module.declare_function(name, cranelift_module::Linkage::Import, &sig)
                    .map_err(|e| TranslateError::Module(e.to_string()))?;
                self.runtime_funcs.insert(name.to_string(), fid);
                fid
            }
        };
        let func_ref = module.declare_func_in_func(fid, builder.func);
        let ctx_val3 = builder.use_var(ctx_var);
        let call = builder.ins().call(func_ref, &[ctx_val3, arity_val, slot_addr]);
        let slow_ret = builder.inst_results(call)[0];
        builder.ins().jump(merge_block, &[BlockArg::Value(slow_ret)]);

        // Merge
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(builder.block_params(merge_block)[0])
    }

    /// Emit inline boxed subtype check: tag == TAG_BOXED && (header & 0xF) == expected_subtype.
    /// Returns true/false atom.
    pub(super) fn emit_boxed_subtype_check(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        val: Value,
        expected_subtype: u64,
    ) -> Result<Value, TranslateError> {
        let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
        let tag = builder.ins().band(val, tag_mask);
        let boxed_tag = builder.ins().iconst(types::I64, TAG_BOXED as i64);
        let is_boxed = builder.ins().icmp(IntCC::Equal, tag, boxed_tag);

        let check_header_block = builder.create_block();
        let false_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        builder.ins().brif(is_boxed, check_header_block, &[], false_block, &[]);

        // Check header subtype
        builder.switch_to_block(check_header_block);
        builder.seal_block(check_header_block);
        // ETS lookup values can carry EXTERNAL_BIT; it is not an address bit.
        let ptr_mask = builder.ins().iconst(types::I64, !(TAG_MASK | EXTERNAL_BIT) as i64);
        let raw_ptr = builder.ins().band(val, ptr_mask);
        let header = builder.ins().load(types::I64, MemFlags::trusted(), raw_ptr, 0);
        let subtype_mask = builder.ins().iconst(types::I64, 0xF);
        let subtype = builder.ins().band(header, subtype_mask);
        let expected = builder.ins().iconst(types::I64, expected_subtype as i64);
        let matches = builder.ins().icmp(IntCC::Equal, subtype, expected);
        let true_val = self.load_true_atom(module, builder)?;
        let false_val = self.load_false_atom(module, builder)?;
        let result = builder.ins().select(matches, true_val, false_val);
        builder.ins().jump(merge_block, &[BlockArg::Value(result)]);

        // Not boxed → false
        builder.switch_to_block(false_block);
        builder.seal_block(false_block);
        let false_val2 = self.load_false_atom(module, builder)?;
        builder.ins().jump(merge_block, &[BlockArg::Value(false_val2)]);

        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(builder.block_params(merge_block)[0])
    }

    /// Emit inline boxed multi-subtype check: tag == TAG_BOXED && (header & 0xF) in {s1, s2, ...}.
    /// Returns true/false atom.
    pub(super) fn emit_boxed_multi_subtype_check(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        val: Value,
        expected_subtypes: &[u64],
    ) -> Result<Value, TranslateError> {
        let tag_mask = builder.ins().iconst(types::I64, TAG_MASK as i64);
        let tag = builder.ins().band(val, tag_mask);
        let boxed_tag = builder.ins().iconst(types::I64, TAG_BOXED as i64);
        let is_boxed = builder.ins().icmp(IntCC::Equal, tag, boxed_tag);

        let check_header_block = builder.create_block();
        let false_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        builder.ins().brif(is_boxed, check_header_block, &[], false_block, &[]);

        // Check header subtype against multiple values
        builder.switch_to_block(check_header_block);
        builder.seal_block(check_header_block);
        let ptr_mask = builder.ins().iconst(types::I64, !(TAG_MASK | EXTERNAL_BIT) as i64);
        let raw_ptr = builder.ins().band(val, ptr_mask);
        let header = builder.ins().load(types::I64, MemFlags::trusted(), raw_ptr, 0);
        let subtype_mask = builder.ins().iconst(types::I64, 0xF);
        let subtype = builder.ins().band(header, subtype_mask);

        // OR together all comparisons
        let first_expected = builder.ins().iconst(types::I64, expected_subtypes[0] as i64);
        let mut any_match = builder.ins().icmp(IntCC::Equal, subtype, first_expected);
        for &st in &expected_subtypes[1..] {
            let exp = builder.ins().iconst(types::I64, st as i64);
            let cmp = builder.ins().icmp(IntCC::Equal, subtype, exp);
            any_match = builder.ins().bor(any_match, cmp);
        }

        let true_val = self.load_true_atom(module, builder)?;
        let false_val = self.load_false_atom(module, builder)?;
        let result = builder.ins().select(any_match, true_val, false_val);
        builder.ins().jump(merge_block, &[BlockArg::Value(result)]);

        // Not boxed → false
        builder.switch_to_block(false_block);
        builder.seal_block(false_block);
        let false_val2 = self.load_false_atom(module, builder)?;
        builder.ins().jump(merge_block, &[BlockArg::Value(false_val2)]);

        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(builder.block_params(merge_block)[0])
    }

    /// Inline fast path for `receive_peek`:
    ///
    /// ```text
    /// mailbox_ptr = load ctx + CTX_OFFSET_MAILBOX_PTR
    /// store 0 → mailbox_ptr + SCAN_POS                  // reset_scan
    /// len = load mailbox_ptr + LEN
    /// cmp len == 0 → goto slow_path
    /// head = load mailbox_ptr + HEAD
    /// cap = load mailbox_ptr + CAPACITY
    /// idx = (head + 0) & (cap - 1)
    /// buf = load mailbox_ptr + BUF
    /// msg = load buf + idx * 8
    /// store 1 → mailbox_ptr + SCAN_POS                  // scan_pos++
    /// → return msg
    ///
    /// slow_path:
    ///   call nera_rt_receive_peek(ctx)          // NO_MESSAGE_VALUE when empty
    /// ```
    pub(super) fn emit_inline_receive_peek(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
    ) -> Result<Value, TranslateError> {
        use crate::term_layout::TOMBSTONE_VALUE;

        let ctx_val = builder.use_var(ctx_var);

        // Load mailbox_ptr from ctx
        let mailbox_ptr = builder.ins().load(
            types::I64, MemFlags::trusted(), ctx_val,
            CTX_OFFSET_MAILBOX_PTR,
        );

        // Reset scan_pos to 0
        let zero_i32 = builder.ins().iconst(types::I32, 0);
        builder.ins().store(MemFlags::trusted(), zero_i32, mailbox_ptr, FAST_MAILBOX_OFFSET_SCAN_POS);

        // Load len
        let len = builder.ins().load(types::I32, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_LEN);

        // Loop header: scan_pos parameter for tombstone skipping
        let loop_block = builder.create_block();
        builder.append_block_param(loop_block, types::I32); // scan_pos

        let fast_block = builder.create_block();
        let slow_block = builder.create_block();
        builder.set_cold_block(slow_block);
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        builder.ins().jump(loop_block, &[BlockArg::Value(zero_i32)]);

        // Loop header: check scan_pos < len
        builder.switch_to_block(loop_block);
        let scan_pos = builder.block_params(loop_block)[0];
        let has_more = builder.ins().icmp(IntCC::UnsignedLessThan, scan_pos, len);
        builder.ins().brif(has_more, fast_block, &[], slow_block, &[]);

        // Fast path: read from ring buffer
        builder.switch_to_block(fast_block);

        let head = builder.ins().load(types::I32, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_HEAD);
        let cap = builder.ins().load(types::I32, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_CAPACITY);
        let cap_minus_1 = builder.ins().iadd_imm(cap, -1);
        let raw_idx = builder.ins().iadd(head, scan_pos);
        let idx = builder.ins().band(raw_idx, cap_minus_1);
        let idx_64 = builder.ins().uextend(types::I64, idx);

        let buf = builder.ins().load(types::I64, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_BUF);
        let byte_offset = builder.ins().ishl_imm(idx_64, 3);
        let msg_addr = builder.ins().iadd(buf, byte_offset);
        let msg = builder.ins().load(types::I64, MemFlags::trusted(), msg_addr, 0);

        // scan_pos++
        let new_scan = builder.ins().iadd_imm(scan_pos, 1);

        // Check for tombstone
        let tombstone_val = builder.ins().iconst(types::I64, TOMBSTONE_VALUE as i64);
        let is_tombstone = builder.ins().icmp(IntCC::Equal, msg, tombstone_val);
        let skip_block = builder.create_block();
        let live_block = builder.create_block();
        builder.ins().brif(is_tombstone, skip_block, &[], live_block, &[]);

        // Skip block: store scan_pos and loop back
        builder.switch_to_block(skip_block);
        builder.ins().store(MemFlags::trusted(), new_scan, mailbox_ptr, FAST_MAILBOX_OFFSET_SCAN_POS);
        builder.ins().jump(loop_block, &[BlockArg::Value(new_scan)]);

        // Live block: store scan_pos and return msg
        builder.switch_to_block(live_block);
        builder.ins().store(MemFlags::trusted(), new_scan, mailbox_ptr, FAST_MAILBOX_OFFSET_SCAN_POS);
        builder.ins().jump(merge_block, &[BlockArg::Value(msg)]);

        // Seal all loop blocks
        builder.seal_block(loop_block);
        builder.seal_block(fast_block);
        builder.seal_block(skip_block);
        builder.seal_block(live_block);

        // Slow path: FFI call (no live messages found)
        builder.switch_to_block(slow_block);
        builder.seal_block(slow_block);
        let slow_ret = self.emit_ffi_call(module, builder, "nera_rt_receive_peek", ctx_var, &[])?;
        builder.ins().jump(merge_block, &[BlockArg::Value(slow_ret)]);

        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(builder.block_params(merge_block)[0])
    }

    /// Inline fast path for `receive_accept`:
    ///
    /// Common case: `scan_pos == 1` means the first message was accepted.
    /// Pop front: `head = (head + 1) & (cap - 1)`, `len -= 1`, `scan_pos = 0`.
    /// The message value is already in a Cranelift variable from the preceding peek.
    ///
    /// For selective receive (scan_pos != 1), fall back to FFI.
    pub(super) fn emit_inline_receive_accept(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
    ) -> Result<Value, TranslateError> {
        let ctx_val = builder.use_var(ctx_var);

        // Load mailbox_ptr
        let mailbox_ptr = builder.ins().load(
            types::I64, MemFlags::trusted(), ctx_val,
            CTX_OFFSET_MAILBOX_PTR,
        );

        // Load scan_pos
        let scan_pos = builder.ins().load(types::I32, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_SCAN_POS);

        // Check scan_pos == 1 (first message accepted — common case)
        let is_first = builder.ins().icmp_imm(IntCC::Equal, scan_pos, 1);

        let fast_block = builder.create_block();
        let slow_block = builder.create_block();
        builder.set_cold_block(slow_block);
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        builder.ins().brif(is_first, fast_block, &[], slow_block, &[]);

        // Fast path: pop front of ring buffer
        builder.switch_to_block(fast_block);
        builder.seal_block(fast_block);

        // Read the message value at head (before advancing head)
        let head = builder.ins().load(types::I32, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_HEAD);
        let cap = builder.ins().load(types::I32, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_CAPACITY);
        let cap_minus_1 = builder.ins().iadd_imm(cap, -1);

        let idx = builder.ins().band(head, cap_minus_1);
        let idx_64 = builder.ins().uextend(types::I64, idx);
        let buf = builder.ins().load(types::I64, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_BUF);
        let byte_offset = builder.ins().ishl_imm(idx_64, 3);
        let msg_addr = builder.ins().iadd(buf, byte_offset);
        let msg = builder.ins().load(types::I64, MemFlags::trusted(), msg_addr, 0);

        // Advance head: head = (head + 1) & (cap - 1)
        let new_head = builder.ins().iadd_imm(head, 1);
        let new_head_masked = builder.ins().band(new_head, cap_minus_1);
        builder.ins().store(MemFlags::trusted(), new_head_masked, mailbox_ptr, FAST_MAILBOX_OFFSET_HEAD);

        // len -= 1
        let len = builder.ins().load(types::I32, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_LEN);
        let new_len = builder.ins().iadd_imm(len, -1);
        builder.ins().store(MemFlags::trusted(), new_len, mailbox_ptr, FAST_MAILBOX_OFFSET_LEN);

        // scan_pos = 0
        let zero_i32 = builder.ins().iconst(types::I32, 0);
        builder.ins().store(MemFlags::trusted(), zero_i32, mailbox_ptr, FAST_MAILBOX_OFFSET_SCAN_POS);

        builder.ins().jump(merge_block, &[BlockArg::Value(msg)]);

        // Slow path: FFI call for selective receive (scan_pos != 1)
        builder.switch_to_block(slow_block);
        builder.seal_block(slow_block);
        let slow_ret = self.emit_ffi_call(module, builder, "nera_rt_receive_accept", ctx_var, &[])?;
        builder.ins().jump(merge_block, &[BlockArg::Value(slow_ret)]);

        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        let ctx = builder.use_var(ctx_var);
        let no_deadline = builder.ins().iconst(types::I64, 0);
        builder.ins().store(MemFlags::trusted(), no_deadline, ctx,
            nera_runtime::process::CTX_OFFSET_RECEIVE_TIMEOUT);
        Ok(builder.block_params(merge_block)[0])
    }

    /// Inline fast path for `receive_next`:
    ///
    /// ```text
    /// mailbox_ptr = load ctx + CTX_OFFSET_MAILBOX_PTR
    /// scan_pos = load mailbox_ptr + SCAN_POS
    /// len = load mailbox_ptr + LEN
    /// cmp scan_pos >= len → goto slow_path (returns NO_MESSAGE_VALUE)
    /// head = load mailbox_ptr + HEAD
    /// cap = load mailbox_ptr + CAPACITY
    /// idx = (head + scan_pos) & (cap - 1)
    /// buf = load mailbox_ptr + BUF
    /// msg = load buf + idx * 8
    /// store scan_pos + 1 → mailbox_ptr + SCAN_POS
    /// → return msg
    ///
    /// slow_path:
    ///   call nera_rt_receive_next(ctx)
    /// ```
    pub(super) fn emit_inline_receive_next(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
    ) -> Result<Value, TranslateError> {
        use crate::term_layout::TOMBSTONE_VALUE;

        let ctx_val = builder.use_var(ctx_var);

        let mailbox_ptr = builder.ins().load(
            types::I64, MemFlags::trusted(), ctx_val,
            CTX_OFFSET_MAILBOX_PTR,
        );

        // Loop header: load scan_pos (may be updated by tombstone skip)
        let loop_block = builder.create_block();
        builder.append_block_param(loop_block, types::I32); // scan_pos param

        let fast_block = builder.create_block();
        let slow_block = builder.create_block();
        builder.set_cold_block(slow_block);
        let merge_block = builder.create_block();
        builder.append_block_param(merge_block, types::I64);

        let scan_pos_init = builder.ins().load(types::I32, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_SCAN_POS);
        let len = builder.ins().load(types::I32, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_LEN);

        builder.ins().jump(loop_block, &[BlockArg::Value(scan_pos_init)]);

        // Loop header: check bounds
        builder.switch_to_block(loop_block);
        let scan_pos = builder.block_params(loop_block)[0];
        let has_more = builder.ins().icmp(IntCC::UnsignedLessThan, scan_pos, len);
        builder.ins().brif(has_more, fast_block, &[], slow_block, &[]);

        // Fast path: read from ring buffer
        builder.switch_to_block(fast_block);

        let head = builder.ins().load(types::I32, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_HEAD);
        let cap = builder.ins().load(types::I32, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_CAPACITY);
        let cap_minus_1 = builder.ins().iadd_imm(cap, -1);
        let raw_idx = builder.ins().iadd(head, scan_pos);
        let idx = builder.ins().band(raw_idx, cap_minus_1);
        let idx_64 = builder.ins().uextend(types::I64, idx);

        let buf = builder.ins().load(types::I64, MemFlags::trusted(), mailbox_ptr, FAST_MAILBOX_OFFSET_BUF);
        let byte_offset = builder.ins().ishl_imm(idx_64, 3);
        let msg_addr = builder.ins().iadd(buf, byte_offset);
        let msg = builder.ins().load(types::I64, MemFlags::trusted(), msg_addr, 0);

        // scan_pos++
        let new_scan = builder.ins().iadd_imm(scan_pos, 1);

        // Check for tombstone: if msg == TOMBSTONE_VALUE, skip (loop back)
        let tombstone_val = builder.ins().iconst(types::I64, TOMBSTONE_VALUE as i64);
        let is_tombstone = builder.ins().icmp(IntCC::Equal, msg, tombstone_val);
        let skip_block = builder.create_block();
        let live_block = builder.create_block();
        builder.ins().brif(is_tombstone, skip_block, &[], live_block, &[]);

        // Skip block: store new scan_pos and loop back
        builder.switch_to_block(skip_block);
        builder.ins().store(MemFlags::trusted(), new_scan, mailbox_ptr, FAST_MAILBOX_OFFSET_SCAN_POS);
        builder.ins().jump(loop_block, &[BlockArg::Value(new_scan)]);

        // Live message block: store scan_pos and return msg
        builder.switch_to_block(live_block);
        builder.ins().store(MemFlags::trusted(), new_scan, mailbox_ptr, FAST_MAILBOX_OFFSET_SCAN_POS);
        builder.ins().jump(merge_block, &[BlockArg::Value(msg)]);

        // Seal all loop blocks now that all predecessors are known
        builder.seal_block(loop_block);
        builder.seal_block(fast_block);
        builder.seal_block(skip_block);
        builder.seal_block(live_block);

        // Slow path: FFI (scan_pos >= len — no more messages)
        builder.switch_to_block(slow_block);
        builder.seal_block(slow_block);
        let slow_ret = self.emit_ffi_call(module, builder, "nera_rt_receive_next", ctx_var, &[])?;
        builder.ins().jump(merge_block, &[BlockArg::Value(slow_ret)]);

        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(builder.block_params(merge_block)[0])
    }

    /// A construction adapter returns nil on failure. Inspect its return value
    /// directly, avoiding a second read of process status on the valid path.
    pub(super) fn emit_ffi_nil_check(
        &mut self, module: &mut ObjectModule, builder: &mut FunctionBuilder,
        ctx_var: Variable, result: Value,
    ) -> Result<(), TranslateError> {
        let failed = builder.ins().icmp_imm(IntCC::Equal, result, NIL_VALUE as i64);
        let error = builder.create_block();
        let next = builder.create_block();
        builder.ins().brif(failed, error, &[], next, &[]);
        builder.switch_to_block(error);
        let nil = builder.ins().iconst(types::I64, NIL_VALUE as i64);
        self.emit_stack_pop_and_return(module, builder, ctx_var, nil)?;
        builder.switch_to_block(next);
        Ok(())
    }

    pub(super) fn emit_ffi_exception_check(
        &mut self,
        module: &mut ObjectModule,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
    ) -> Result<(), TranslateError> {
        let ctx_val = builder.use_var(ctx_var);
        let status = builder.ins().load(
            types::I32,
            MemFlags::trusted(),
            ctx_val,
            CTX_OFFSET_STATUS,
        );
        let exc_const = builder.ins().iconst(types::I32, STATUS_EXCEPTION as i64);
        let is_exc = builder.ins().icmp(IntCC::Equal, status, exc_const);
        let exc_ret_block = builder.create_block();
        let cont_block = builder.create_block();
        builder.ins().brif(is_exc, exc_ret_block, &[], cont_block, &[]);

        builder.switch_to_block(exc_ret_block);
        let nil = builder.ins().iconst(types::I64, NIL_VALUE as i64);
        self.emit_stack_pop_and_return(module, builder, ctx_var, nil)?;

        builder.switch_to_block(cont_block);
        Ok(())
    }
}
