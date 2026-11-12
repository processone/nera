//! Operand-use collection shared by the existing allocation-elision passes.
//! Kept separate from codegen liveness and verifier operands during extraction.

use crate::ir::{CallTarget, Instruction, Terminator, VarId};
use std::collections::HashSet;

/// Collect VarIds used (read) by an instruction. Self-contained version for nera-ir crate.
pub(super) fn collect_instruction_uses(inst: &Instruction, uses: &mut HashSet<VarId>) {
    match inst {
        Instruction::Const(_, _) => {}
        Instruction::BinOp(_, _, a, b) => {
            uses.insert(*a);
            uses.insert(*b);
        }
        Instruction::UnaryOp(_, _, a) => {
            uses.insert(*a);
        }
        Instruction::MakeTuple(_, args) => {
            for a in args {
                uses.insert(*a);
            }
        }
        Instruction::MakeCons(_, h, t) => {
            uses.insert(*h);
            uses.insert(*t);
        }
        Instruction::TupleElement(_, t, _) => {
            uses.insert(*t);
        }
        Instruction::ListHead(_, l) => {
            uses.insert(*l);
        }
        Instruction::ListTail(_, l) => {
            uses.insert(*l);
        }
        Instruction::Call(_, target, args) => {
            for a in args {
                uses.insert(*a);
            }
            match target {
                CallTarget::Closure(v) => {
                    uses.insert(*v);
                }
                CallTarget::Apply(m, f) => {
                    uses.insert(*m);
                    uses.insert(*f);
                }
                _ => {}
            }
        }
        Instruction::GetSelf(_) => {}
        Instruction::Spawn(_, _, _, args) => {
            for a in args {
                uses.insert(*a);
            }
        }
        Instruction::Send(_, pid, msg) => {
            uses.insert(*pid);
            uses.insert(*msg);
        }
        Instruction::SendDirect(_, pid, elems) => {
            uses.insert(*pid);
            for e in elems {
                uses.insert(*e);
            }
        }
        Instruction::TypeTest(_, _, val) => {
            uses.insert(*val);
        }
        Instruction::Move(_, src) => {
            uses.insert(*src);
        }
        Instruction::TupleSize(_, t) => {
            uses.insert(*t);
        }
        Instruction::MakeBinary(_, size) => {
            uses.insert(*size);
        }
        Instruction::BinaryPutInteger(bin, off, val, size, _) => {
            uses.insert(*bin);
            uses.insert(*off);
            uses.insert(*val);
            uses.insert(*size);
        }
        Instruction::BinaryPutFloat(bin, off, val, size, _) => {
            uses.insert(*bin);
            uses.insert(*off);
            uses.insert(*val);
            uses.insert(*size);
        }
        Instruction::BinaryPutBinary(dest, off, src, count) => {
            uses.insert(*dest);
            uses.insert(*off);
            uses.insert(*src);
            uses.insert(*count);
        }
        Instruction::BinaryGetInteger(_, bin, off, size, _) => {
            uses.insert(*bin);
            uses.insert(*off);
            uses.insert(*size);
        }
        Instruction::BinaryGetFloat(_, bin, off, size, _) => {
            uses.insert(*bin);
            uses.insert(*off);
            uses.insert(*size);
        }
        Instruction::BinaryGetBinary(_, bin, off, size) => {
            uses.insert(*bin);
            uses.insert(*off);
            uses.insert(*size);
        }
        Instruction::BsMatchBitOffset(_, bin) | Instruction::BinarySize(_, bin) => {
            uses.insert(*bin);
        }
        Instruction::BsStartMatch(_, _, bin) => {
            uses.insert(*bin);
        }
        Instruction::BsMatchStart2(_, _, _, input) => {
            uses.insert(*input);
        }
        Instruction::BsMatchCtxAdvance(mc, off) => {
            uses.insert(*mc);
            uses.insert(*off);
        }
        Instruction::BsGetIntegerInline(_, base, off, count, _) => {
            uses.insert(*base);
            uses.insert(*off);
            uses.insert(*count);
        }
        Instruction::BsMaterialize(_, orig, off, count) => {
            uses.insert(*orig);
            uses.insert(*off);
            uses.insert(*count);
        }
        Instruction::MakeMap(_, pairs) => {
            for p in pairs {
                uses.insert(*p);
            }
        }
        Instruction::MapGet(_, map, key) => {
            uses.insert(*map);
            uses.insert(*key);
        }
        Instruction::MapPut(_, map, key, val) => {
            uses.insert(*map);
            uses.insert(*key);
            uses.insert(*val);
        }
        Instruction::MapUpdateExact(_, map, key, val) => {
            uses.insert(*map);
            uses.insert(*key);
            uses.insert(*val);
        }
        Instruction::CheckReductions => {}
        Instruction::YieldCheck => {}
        Instruction::RaiseException(_, _, reason) => {
            uses.insert(*reason);
        }
        Instruction::GetExceptionClass(_) => {}
        Instruction::GetExceptionReason(_) => {}
        Instruction::GetExceptionStacktrace(_) => {}
        Instruction::ClearException => {}
        Instruction::CheckException(_) => {}
        Instruction::MakeClosure(_, _, _, env_vars) => {
            for ev in env_vars {
                uses.insert(*ev);
            }
        }
        Instruction::MakeRemoteFunRef(_, _, _, _) => {}
        Instruction::MakeDynamicRemoteFunRef(_, m, f, a) => {
            uses.insert(*m);
            uses.insert(*f);
            uses.insert(*a);
        }
        Instruction::BinaryPutUtf8(_, bin, off, cp) => {
            uses.insert(*bin);
            uses.insert(*off);
            uses.insert(*cp);
        }
        Instruction::BinaryPutUtf16(_, bin, off, cp, _) => {
            uses.insert(*bin);
            uses.insert(*off);
            uses.insert(*cp);
        }
        Instruction::BinaryPutUtf32(_, bin, off, cp, _) => {
            uses.insert(*bin);
            uses.insert(*off);
            uses.insert(*cp);
        }
        Instruction::BsGetUtf8(_, _, base, off, total) => {
            uses.insert(*base);
            uses.insert(*off);
            uses.insert(*total);
        }
        Instruction::BsGetUtf16(_, _, base, off, total, _) => {
            uses.insert(*base);
            uses.insert(*off);
            uses.insert(*total);
        }
        Instruction::BsGetUtf32(_, _, base, off, total, _) => {
            uses.insert(*base);
            uses.insert(*off);
            uses.insert(*total);
        }
        Instruction::BinaryResize(bin, size) => {
            uses.insert(*bin);
            uses.insert(*size);
        }
        Instruction::SetLine(_) => {}
    }
}

/// Collect VarIds used (read) by a terminator. Self-contained version for nera-ir crate.
pub(super) fn collect_terminator_uses(term: &Terminator, uses: &mut HashSet<VarId>) {
    match term {
        Terminator::Return(v) => {
            uses.insert(*v);
        }
        Terminator::Jump(_, args) => {
            for a in args {
                uses.insert(*a);
            }
        }
        Terminator::Branch(v, _, _) => {
            uses.insert(*v);
        }
        Terminator::Switch { scrutinee, .. } => {
            uses.insert(*scrutinee);
        }
        Terminator::TailCall(target, args) => {
            for a in args {
                uses.insert(*a);
            }
            match target {
                CallTarget::Closure(v) => {
                    uses.insert(*v);
                }
                CallTarget::Apply(m, f) => {
                    uses.insert(*m);
                    uses.insert(*f);
                }
                _ => {}
            }
        }
        Terminator::Receive {
            clauses, timeout, ..
        } => {
            for c in clauses {
                uses.insert(c.binding);
            }
            if let Some((t, _)) = timeout {
                uses.insert(*t);
            }
        }
        Terminator::Unreachable => {}
    }
}
