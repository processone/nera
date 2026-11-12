//! Pretty-printing for IR, useful for debugging with --emit-ir.

use std::fmt;
use crate::ir::*;

impl fmt::Display for IrModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "module {:?}", self.name)?;
        writeln!(f)?;
        for func in &self.functions {
            write!(f, "{}", func)?;
            writeln!(f)?;
        }
        Ok(())
    }
}

impl fmt::Display for IrFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "function {}(", self.mangled_name)?;
        for (i, param) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "v{}", param.0)?;
        }
        writeln!(f, "):")?;

        for block in &self.blocks {
            write!(f, "{}", block)?;
        }
        Ok(())
    }
}

impl fmt::Display for BasicBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "  bb{}(", self.id.0)?;
        for (i, param) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "v{}", param.0)?;
        }
        writeln!(f, "):")?;

        for inst in &self.instructions {
            writeln!(f, "    {}", inst)?;
        }
        writeln!(f, "    {}", self.terminator)?;
        Ok(())
    }
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Instruction::Const(v, lit) => write!(f, "v{} = const {}", v.0, lit),
            Instruction::BinOp(v, op, a, b) => {
                write!(f, "v{} = {:?}(v{}, v{})", v.0, op, a.0, b.0)
            }
            Instruction::UnaryOp(v, op, a) => {
                write!(f, "v{} = {:?}(v{})", v.0, op, a.0)
            }
            Instruction::MakeTuple(v, elems) => {
                write!(f, "v{} = make_tuple(", v.0)?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "v{}", e.0)?;
                }
                write!(f, ")")
            }
            Instruction::MakeCons(v, h, t) => {
                write!(f, "v{} = make_cons(v{}, v{})", v.0, h.0, t.0)
            }
            Instruction::TupleElement(v, tup, idx) => {
                write!(f, "v{} = tuple_element(v{}, {})", v.0, tup.0, idx)
            }
            Instruction::ListHead(v, list) => {
                write!(f, "v{} = list_head(v{})", v.0, list.0)
            }
            Instruction::ListTail(v, list) => {
                write!(f, "v{} = list_tail(v{})", v.0, list.0)
            }
            Instruction::Call(v, target, args) => {
                write!(f, "v{} = call {:?}(", v.0, target)?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "v{}", a.0)?;
                }
                write!(f, ")")
            }
            Instruction::GetSelf(v) => write!(f, "v{} = self()", v.0),
            Instruction::Spawn(v, m, fun, args) => {
                write!(f, "v{} = spawn({:?}, {:?}, [", v.0, m, fun)?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "v{}", a.0)?;
                }
                write!(f, "])")
            }
            Instruction::Send(v, pid, msg) => {
                write!(f, "v{} = send(v{}, v{})", v.0, pid.0, msg.0)
            }
            Instruction::SendDirect(v, pid, elems) => {
                write!(f, "v{} = send_direct(v{}, {{", v.0, pid.0)?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "v{}", e.0)?;
                }
                write!(f, "}})")
            }
            Instruction::CheckReductions => write!(f, "check_reductions"),
            Instruction::YieldCheck => write!(f, "yield_check"),
            Instruction::TypeTest(v, kind, val) => {
                write!(f, "v{} = {:?}(v{})", v.0, kind, val.0)
            }
            Instruction::Move(dst, src) => {
                write!(f, "v{} = move v{}", dst.0, src.0)
            }
            Instruction::TupleSize(v, tup) => {
                write!(f, "v{} = tuple_size(v{})", v.0, tup.0)
            }
            Instruction::MakeBinary(v, size) => {
                write!(f, "v{} = make_binary(v{})", v.0, size.0)
            }
            Instruction::BinaryPutInteger(bin, off, val, size, flags) => {
                write!(
                    f,
                    "binary_put_integer(v{}, off=v{}, val=v{}, bits=v{}, s={} e={})",
                    bin.0, off.0, val.0, size.0, flags.signedness, flags.endianness
                )
            }
            Instruction::BinaryPutFloat(bin, off, val, size, flags) => {
                write!(
                    f,
                    "binary_put_float(v{}, off=v{}, val=v{}, bits=v{}, e={})",
                    bin.0, off.0, val.0, size.0, flags.endianness
                )
            }
            Instruction::BinaryPutBinary(dest, off, src, count) => {
                write!(
                    f,
                    "binary_put_binary(v{}, bit_off=v{}, src=v{}, bits=v{})",
                    dest.0, off.0, src.0, count.0
                )
            }
            Instruction::BinaryGetInteger(v, bin, off, size, flags) => {
                write!(
                    f,
                    "v{} = binary_get_integer(v{}, off=v{}, bits=v{}, s={} e={})",
                    v.0, bin.0, off.0, size.0, flags.signedness, flags.endianness
                )
            }
            Instruction::BinaryGetFloat(v, bin, off, size, flags) => {
                write!(
                    f,
                    "v{} = binary_get_float(v{}, off=v{}, bits=v{}, e={})",
                    v.0, bin.0, off.0, size.0, flags.endianness
                )
            }
            Instruction::BinaryGetBinary(v, bin, off, size) => {
                write!(
                    f,
                    "v{} = binary_get_binary(v{}, bit_off=v{}, bits=v{})",
                    v.0, bin.0, off.0, size.0
                )
            }
            Instruction::BinarySize(v, bin) => {
                write!(f, "v{} = bit_size(v{})", v.0, bin.0)
            }
            Instruction::MakeMap(v, pairs) => {
                write!(f, "v{} = make_map(", v.0)?;
                for (i, p) in pairs.iter().enumerate() {
                    if i > 0 && i % 2 == 0 { write!(f, ", ")?; }
                    if i % 2 == 0 { write!(f, "v{}", p.0)?; }
                    else { write!(f, " => v{}", p.0)?; }
                }
                write!(f, ")")
            }
            Instruction::MapGet(v, map, key) => {
                write!(f, "v{} = map_get(v{}, v{})", v.0, map.0, key.0)
            }
            Instruction::MapPut(v, map, key, val) => {
                write!(f, "v{} = map_put(v{}, v{}, v{})", v.0, map.0, key.0, val.0)
            }
            Instruction::MapUpdateExact(v, map, key, val) => {
                write!(f, "v{} = map_update_exact(v{}, v{}, v{})", v.0, map.0, key.0, val.0)
            }
            Instruction::RaiseException(v, class, reason) => {
                write!(f, "v{} = raise_exception({:?}, v{})", v.0, class, reason.0)
            }
            Instruction::GetExceptionClass(v) => {
                write!(f, "v{} = get_exception_class()", v.0)
            }
            Instruction::GetExceptionReason(v) => {
                write!(f, "v{} = get_exception_reason()", v.0)
            }
            Instruction::GetExceptionStacktrace(v) => {
                write!(f, "v{} = get_exception_stacktrace()", v.0)
            }
            Instruction::ClearException => write!(f, "clear_exception"),
            Instruction::CheckException(v) => {
                write!(f, "v{} = check_exception()", v.0)
            }
            Instruction::MakeClosure(v, name, arity, env_vars) => {
                write!(f, "v{} = make_closure({}/{}, [", v.0, name, arity)?;
                for (i, e) in env_vars.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "v{}", e.0)?;
                }
                write!(f, "])")
            }
            Instruction::MakeRemoteFunRef(v, mod_id, fun_id, arity) => {
                write!(f, "v{} = make_remote_fun_ref(atom{}:atom{}/{})", v.0, mod_id.0, fun_id.0, arity)
            }
            Instruction::MakeDynamicRemoteFunRef(v, mod_v, fun_v, arity_v) => {
                write!(f, "v{} = make_dynamic_remote_fun_ref(v{}, v{}, v{})", v.0, mod_v.0, fun_v.0, arity_v.0)
            }
            Instruction::BsMatchBitOffset(result, input) => {
                write!(f, "v{} = bs_match_bit_offset(v{})", result.0, input.0)
            }
            Instruction::BsStartMatch(base_ptr, total_bits, bin) => {
                write!(f, "(v{}, v{}) = bs_start_match(v{})", base_ptr.0, total_bits.0, bin.0)
            }
            Instruction::BsGetIntegerInline(result, base_ptr, offset, count, flags) => {
                write!(f, "v{} = bs_get_integer_inline(v{}, v{}, v{}, {:?})",
                    result.0, base_ptr.0, offset.0, count.0, flags)
            }
            Instruction::BsMaterialize(result, orig, offset, count) => {
                write!(f, "v{} = bs_materialize(v{}, v{}, v{})",
                    result.0, orig.0, offset.0, count.0)
            }
            Instruction::BsMatchStart2(match_ctx, base_ptr, total_bits, input) => {
                write!(f, "(v{}, v{}, v{}) = bs_match_start2(v{})",
                    match_ctx.0, base_ptr.0, total_bits.0, input.0)
            }
            Instruction::BsMatchCtxAdvance(match_ctx, bit_offset) => {
                write!(f, "bs_match_ctx_advance(v{}, v{})",
                    match_ctx.0, bit_offset.0)
            }
            Instruction::BinaryPutUtf8(bits_written, bin, off, val) => {
                write!(f, "v{} = binary_put_utf8(v{}, off=v{}, val=v{})",
                    bits_written.0, bin.0, off.0, val.0)
            }
            Instruction::BinaryPutUtf16(bits_written, bin, off, val, flags) => {
                write!(f, "v{} = binary_put_utf16(v{}, off=v{}, val=v{}, e={})",
                    bits_written.0, bin.0, off.0, val.0, flags.endianness)
            }
            Instruction::BinaryPutUtf32(bits_written, bin, off, val, flags) => {
                write!(f, "v{} = binary_put_utf32(v{}, off=v{}, val=v{}, e={})",
                    bits_written.0, bin.0, off.0, val.0, flags.endianness)
            }
            Instruction::BsGetUtf8(cp, bits_consumed, base, off, total) => {
                write!(f, "(v{}, v{}) = bs_get_utf8(v{}, v{}, v{})",
                    cp.0, bits_consumed.0, base.0, off.0, total.0)
            }
            Instruction::BsGetUtf16(cp, bits_consumed, base, off, total, flags) => {
                write!(f, "(v{}, v{}) = bs_get_utf16(v{}, v{}, v{}, e={})",
                    cp.0, bits_consumed.0, base.0, off.0, total.0, flags.endianness)
            }
            Instruction::BsGetUtf32(cp, bits_consumed, base, off, total, flags) => {
                write!(f, "(v{}, v{}) = bs_get_utf32(v{}, v{}, v{}, e={})",
                    cp.0, bits_consumed.0, base.0, off.0, total.0, flags.endianness)
            }
            Instruction::BinaryResize(bin, new_size) => {
                write!(f, "binary_resize(v{}, v{})", bin.0, new_size.0)
            }
            Instruction::SetLine(line) => {
                write!(f, "set_line {}", line)
            }
        }
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Literal::Integer(n) => write!(f, "{}", n),
            Literal::BigInteger(n) => write!(f, "{}", n),
            Literal::Float(n) => {
                // Always print with decimal point to distinguish from integers
                if n.fract() == 0.0 {
                    write!(f, "{:.1}", n)
                } else {
                    write!(f, "{}", n)
                }
            }
            Literal::Atom(id) => write!(f, "atom({})", id.0),
            Literal::Nil => write!(f, "[]"),
        }
    }
}

impl fmt::Display for Terminator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Terminator::Return(v) => write!(f, "return v{}", v.0),
            Terminator::Jump(block, args) => {
                write!(f, "jump bb{}(", block.0)?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "v{}", a.0)?;
                }
                write!(f, ")")
            }
            Terminator::Branch(cond, t, e) => {
                write!(f, "branch v{} ? bb{} : bb{}", cond.0, t.0, e.0)
            }
            Terminator::Switch { scrutinee, cases, default } => {
                write!(f, "switch v{} [", scrutinee.0)?;
                for (lit, block) in cases {
                    write!(f, " {} => bb{},", lit, block.0)?;
                }
                write!(f, " default => bb{} ]", default.0)
            }
            Terminator::Receive { clauses, timeout, wait_block } => {
                write!(f, "receive [")?;
                for c in clauses {
                    write!(f, " match bb{} -> bb{},", c.match_block.0, c.body_block.0)?;
                }
                if let Some((t, b)) = timeout {
                    write!(f, " after v{} -> bb{}", t.0, b.0)?;
                }
                write!(f, " wait bb{} ]", wait_block.0)
            }
            Terminator::TailCall(target, args) => {
                write!(f, "tail_call {:?}(", target)?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "v{}", a.0)?;
                }
                write!(f, ")")
            }
            Terminator::Unreachable => write!(f, "unreachable"),
        }
    }
}
