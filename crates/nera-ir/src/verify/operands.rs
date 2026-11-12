//! Exhaustive operand descriptions for verification. No wildcard arms: adding
//! an IR variant must force a decision about its reads, results and payloads.

use crate::ir::{BinFlags, CallTarget, Instruction, Terminator, VarId};

pub(super) fn instruction_operands(inst: &Instruction) -> (Vec<VarId>, Vec<VarId>) {
    use Instruction::*;
    match inst {
        Const(dst, _)
        | GetSelf(dst)
        | GetExceptionClass(dst)
        | GetExceptionReason(dst)
        | GetExceptionStacktrace(dst)
        | CheckException(dst)
        | MakeRemoteFunRef(dst, _, _, _) => (vec![*dst], vec![]),
        BinOp(dst, _, a, b) | MakeCons(dst, a, b) | Send(dst, a, b) | MapGet(dst, a, b) => {
            (vec![*dst], vec![*a, *b])
        }
        UnaryOp(dst, _, a)
        | TupleElement(dst, a, _)
        | ListHead(dst, a)
        | ListTail(dst, a)
        | TypeTest(dst, _, a)
        | Move(dst, a)
        | TupleSize(dst, a)
        | MakeBinary(dst, a)
        | BsMatchBitOffset(dst, a)
        | BinarySize(dst, a)
        | RaiseException(dst, _, a) => (vec![*dst], vec![*a]),
        MakeTuple(dst, args)
        | Spawn(dst, _, _, args)
        | MakeMap(dst, args)
        | MakeClosure(dst, _, _, args) => (vec![*dst], args.clone()),
        Call(dst, target, args) => {
            let mut uses = args.clone();
            uses.extend(target_operands(target));
            (vec![*dst], uses)
        }
        SendDirect(dst, pid, elems) => {
            let mut uses = vec![*pid];
            uses.extend(elems);
            (vec![*dst], uses)
        }
        CheckReductions | YieldCheck | ClearException | SetLine(_) => (vec![], vec![]),
        BinaryPutInteger(a, b, c, d, _)
        | BinaryPutFloat(a, b, c, d, _)
        | BinaryPutBinary(a, b, c, d) => (vec![], vec![*a, *b, *c, *d]),
        BinaryGetInteger(dst, a, b, c, _)
        | BinaryGetFloat(dst, a, b, c, _)
        | BinaryGetBinary(dst, a, b, c)
        | BsGetIntegerInline(dst, a, b, c, _)
        | BsMaterialize(dst, a, b, c)
        | MapPut(dst, a, b, c)
        | MapUpdateExact(dst, a, b, c)
        | MakeDynamicRemoteFunRef(dst, a, b, c)
        | BinaryPutUtf8(dst, a, b, c)
        | BinaryPutUtf16(dst, a, b, c, _)
        | BinaryPutUtf32(dst, a, b, c, _) => (vec![*dst], vec![*a, *b, *c]),
        BsStartMatch(a, b, input) => (vec![*a, *b], vec![*input]),
        BsMatchStart2(a, b, c, input) => (vec![*a, *b, *c], vec![*input]),
        BsMatchCtxAdvance(a, b) | BinaryResize(a, b) => (vec![], vec![*a, *b]),
        BsGetUtf8(a, b, base, off, total)
        | BsGetUtf16(a, b, base, off, total, _)
        | BsGetUtf32(a, b, base, off, total, _) => (vec![*a, *b], vec![*base, *off, *total]),
    }
}

fn target_operands(target: &CallTarget) -> Vec<VarId> {
    match target {
        CallTarget::Local(_) | CallTarget::Remote(_, _) | CallTarget::Bif(_) => vec![],
        CallTarget::Closure(v) => vec![*v],
        CallTarget::Apply(m, f) => vec![*m, *f],
    }
}

pub(super) fn terminator_operands(term: &Terminator) -> Vec<VarId> {
    match term {
        Terminator::Return(v) | Terminator::Branch(v, _, _) => vec![*v],
        Terminator::Jump(_, args) => args.clone(),
        Terminator::Switch { scrutinee, .. } => vec![*scrutinee],
        Terminator::TailCall(target, args) => {
            let mut uses = args.clone();
            uses.extend(target_operands(target));
            uses
        }
        // Reserved: validate its graph/shape, but do not invent binding semantics.
        Terminator::Receive { timeout, .. } => timeout.iter().map(|(v, _)| *v).collect(),
        Terminator::Unreachable => vec![],
    }
}

pub(super) fn binary_flags(inst: &Instruction) -> Option<BinFlags> {
    use Instruction::*;
    match inst {
        BinaryPutInteger(_, _, _, _, flags)
        | BinaryPutFloat(_, _, _, _, flags)
        | BinaryGetInteger(_, _, _, _, flags)
        | BinaryGetFloat(_, _, _, _, flags)
        | BsGetIntegerInline(_, _, _, _, flags)
        | BinaryPutUtf16(_, _, _, _, flags)
        | BinaryPutUtf32(_, _, _, _, flags)
        | BsGetUtf16(_, _, _, _, _, flags)
        | BsGetUtf32(_, _, _, _, _, flags) => Some(*flags),
        Const(..)
        | BinOp(..)
        | UnaryOp(..)
        | MakeTuple(..)
        | MakeCons(..)
        | TupleElement(..)
        | ListHead(..)
        | ListTail(..)
        | Call(..)
        | GetSelf(..)
        | Spawn(..)
        | Send(..)
        | SendDirect(..)
        | CheckReductions
        | YieldCheck
        | TypeTest(..)
        | Move(..)
        | TupleSize(..)
        | MakeBinary(..)
        | BinaryPutBinary(..)
        | BinaryGetBinary(..)
        | BinarySize(..)
        | BsMatchBitOffset(..)
        | BsStartMatch(..)
        | BsMaterialize(..)
        | BsMatchStart2(..)
        | BsMatchCtxAdvance(..)
        | MakeMap(..)
        | MapGet(..)
        | MapPut(..)
        | MapUpdateExact(..)
        | RaiseException(..)
        | GetExceptionClass(..)
        | GetExceptionReason(..)
        | GetExceptionStacktrace(..)
        | ClearException
        | CheckException(..)
        | MakeClosure(..)
        | MakeRemoteFunRef(..)
        | MakeDynamicRemoteFunRef(..)
        | BinaryPutUtf8(..)
        | BsGetUtf8(..)
        | BinaryResize(..)
        | SetLine(..) => None,
    }
}
