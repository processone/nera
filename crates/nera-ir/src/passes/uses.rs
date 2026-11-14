//! Operand-use collection shared by the existing allocation-elision passes.
//! Kept separate from codegen liveness and verifier operands during extraction.

use crate::ir::{CallTarget, Instruction, Terminator, VarId};
use std::collections::HashSet;

/// Collect distinct reads for consumers that only ask whether a value is used.
pub(super) fn collect_instruction_uses(inst: &Instruction, uses: &mut HashSet<VarId>) {
    uses.extend(instruction_reads(inst));
}

/// Preserve every operand occurrence for single-use optimization decisions.
pub(super) fn instruction_reads(inst: &Instruction) -> Vec<VarId> {
    let mut uses = Vec::new();
    match inst {
        Instruction::Arithmetic(site) => uses.extend(site.operation.operands()),
        Instruction::Const(_, _) => {}
        Instruction::BinOp(_, _, a, b) => {
            uses.push(*a);
            uses.push(*b);
        }
        Instruction::UnaryOp(_, _, a) => {
            uses.push(*a);
        }
        Instruction::MakeTuple(_, args) => {
            for a in args {
                uses.push(*a);
            }
        }
        Instruction::MakeCons(_, h, t) => {
            uses.push(*h);
            uses.push(*t);
        }
        Instruction::TupleElement(_, t, _) => {
            uses.push(*t);
        }
        Instruction::ListHead(_, l) => {
            uses.push(*l);
        }
        Instruction::ListTail(_, l) => {
            uses.push(*l);
        }
        Instruction::Call(_, target, args) => {
            for a in args {
                uses.push(*a);
            }
            match target {
                CallTarget::Closure(v) => {
                    uses.push(*v);
                }
                CallTarget::Apply(m, f) => {
                    uses.push(*m);
                    uses.push(*f);
                }
                _ => {}
            }
        }
        Instruction::GetSelf(_) => {}
        Instruction::Spawn(_, _, _, args) => {
            for a in args {
                uses.push(*a);
            }
        }
        Instruction::Send(_, pid, msg) => {
            uses.push(*pid);
            uses.push(*msg);
        }
        Instruction::SendDirect(_, pid, elems) => {
            uses.push(*pid);
            for e in elems {
                uses.push(*e);
            }
        }
        Instruction::TypeTest(_, _, val) => {
            uses.push(*val);
        }
        Instruction::Move(_, src) => {
            uses.push(*src);
        }
        Instruction::TupleSize(_, t) => {
            uses.push(*t);
        }
        Instruction::MakeBinary(_, size) => {
            uses.push(*size);
        }
        Instruction::BinaryPutInteger(bin, off, val, size, _) => {
            uses.push(*bin);
            uses.push(*off);
            uses.push(*val);
            uses.push(*size);
        }
        Instruction::BinaryPutFloat(bin, off, val, size, _) => {
            uses.push(*bin);
            uses.push(*off);
            uses.push(*val);
            uses.push(*size);
        }
        Instruction::BinaryPutBinary(dest, off, src, count) => {
            uses.push(*dest);
            uses.push(*off);
            uses.push(*src);
            uses.push(*count);
        }
        Instruction::BinaryGetInteger(_, bin, off, size, _) => {
            uses.push(*bin);
            uses.push(*off);
            uses.push(*size);
        }
        Instruction::BinaryGetFloat(_, bin, off, size, _) => {
            uses.push(*bin);
            uses.push(*off);
            uses.push(*size);
        }
        Instruction::BinaryGetBinary(_, bin, off, size) => {
            uses.push(*bin);
            uses.push(*off);
            uses.push(*size);
        }
        Instruction::BsMatchBitOffset(_, bin) | Instruction::BinarySize(_, bin) => {
            uses.push(*bin);
        }
        Instruction::BsStartMatch(_, _, bin) => {
            uses.push(*bin);
        }
        Instruction::BsMatchStart2(_, _, _, input) => {
            uses.push(*input);
        }
        Instruction::BsMatchCtxAdvance(mc, off) => {
            uses.push(*mc);
            uses.push(*off);
        }
        Instruction::BsGetIntegerInline(_, base, off, count, _) => {
            uses.push(*base);
            uses.push(*off);
            uses.push(*count);
        }
        Instruction::BsMaterialize(_, orig, off, count) => {
            uses.push(*orig);
            uses.push(*off);
            uses.push(*count);
        }
        Instruction::MakeMap(_, pairs) => {
            for p in pairs {
                uses.push(*p);
            }
        }
        Instruction::MapGet(_, map, key) => {
            uses.push(*map);
            uses.push(*key);
        }
        Instruction::MapPut(_, map, key, val) => {
            uses.push(*map);
            uses.push(*key);
            uses.push(*val);
        }
        Instruction::MapUpdateExact(_, map, key, val) => {
            uses.push(*map);
            uses.push(*key);
            uses.push(*val);
        }
        Instruction::CheckReductions => {}
        Instruction::YieldCheck => {}
        Instruction::RaiseException(_, _, reason) => {
            uses.push(*reason);
        }
        Instruction::GetExceptionClass(_) => {}
        Instruction::GetExceptionReason(_) => {}
        Instruction::GetExceptionStacktrace(_) => {}
        Instruction::ClearException => {}
        Instruction::CheckException(_) => {}
        Instruction::MakeClosure(_, _, _, env_vars) => {
            for ev in env_vars {
                uses.push(*ev);
            }
        }
        Instruction::MakeRemoteFunRef(_, _, _, _) => {}
        Instruction::MakeDynamicRemoteFunRef(_, m, f, a) => {
            uses.push(*m);
            uses.push(*f);
            uses.push(*a);
        }
        Instruction::BinaryPutUtf8(_, bin, off, cp) => {
            uses.push(*bin);
            uses.push(*off);
            uses.push(*cp);
        }
        Instruction::BinaryPutUtf16(_, bin, off, cp, _) => {
            uses.push(*bin);
            uses.push(*off);
            uses.push(*cp);
        }
        Instruction::BinaryPutUtf32(_, bin, off, cp, _) => {
            uses.push(*bin);
            uses.push(*off);
            uses.push(*cp);
        }
        Instruction::BsGetUtf8(_, _, base, off, total) => {
            uses.push(*base);
            uses.push(*off);
            uses.push(*total);
        }
        Instruction::BsGetUtf16(_, _, base, off, total, _) => {
            uses.push(*base);
            uses.push(*off);
            uses.push(*total);
        }
        Instruction::BsGetUtf32(_, _, base, off, total, _) => {
            uses.push(*base);
            uses.push(*off);
            uses.push(*total);
        }
        Instruction::BinaryResize(bin, size) => {
            uses.push(*bin);
            uses.push(*size);
        }
        Instruction::SetLine(_) => {}
    }
    uses
}

/// Collect distinct terminator reads for set-based consumers.
pub(super) fn collect_terminator_uses(term: &Terminator, uses: &mut HashSet<VarId>) {
    uses.extend(terminator_reads(term));
}

pub(super) fn terminator_reads(term: &Terminator) -> Vec<VarId> {
    let mut uses = Vec::new();
    match term {
        Terminator::Return(v) => {
            uses.push(*v);
        }
        Terminator::Jump(_, args) => {
            for a in args {
                uses.push(*a);
            }
        }
        Terminator::Branch(v, _, _) => {
            uses.push(*v);
        }
        Terminator::Switch { scrutinee, .. } => {
            uses.push(*scrutinee);
        }
        Terminator::TailCall(target, args) => {
            for a in args {
                uses.push(*a);
            }
            match target {
                CallTarget::Closure(v) => {
                    uses.push(*v);
                }
                CallTarget::Apply(m, f) => {
                    uses.push(*m);
                    uses.push(*f);
                }
                _ => {}
            }
        }
        Terminator::Receive {
            clauses, timeout, ..
        } => {
            for c in clauses {
                uses.push(c.binding);
            }
            if let Some((t, _)) = timeout {
                uses.push(*t);
            }
        }
        Terminator::Unreachable => {}
    }
    uses
}
