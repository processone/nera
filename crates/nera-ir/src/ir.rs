//! Intermediate Representation for nera.
//!
//! A flat control-flow IR with mutable virtual registers, compiled pattern
//! decisions and lifted closures. Multiple instructions may write one VarId;
//! Cranelift constructs SSA later. See docs/IR_CONTRACT.md for the contract.

use nera_syntax::ast::{AtomId, BinOp, UnaryOp};

/// A compiled module in IR form.
#[derive(Debug, Clone)]
pub struct IrModule {
    pub name: AtomId,
    pub functions: Vec<IrFunction>,
    pub exports: Vec<IrExport>,
}

#[derive(Debug, Clone)]
pub struct IrExport {
    pub name: AtomId,
    pub arity: u32,
}

/// A function consists of basic blocks using mutable virtual registers.
#[derive(Debug, Clone)]
pub struct IrFunction {
    pub name: AtomId,
    pub arity: u32,
    pub mangled_name: String,
    pub blocks: Vec<BasicBlock>,
    /// Results of potentially raising operations lexically inside a try body.
    pub protected_values: Vec<VarId>,
    pub entry_block: BlockId,
    pub params: Vec<VarId>,
    /// Source line number of the function definition (1-based, 0 = unknown).
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VarId(pub u32);

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BlockId,
    /// Reserved: current lowering and native codegen require this to be empty.
    pub params: Vec<VarId>,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}

#[derive(Debug, Clone)]
pub enum Instruction {
    /// result = literal value
    Const(VarId, Literal),

    /// result = op(a, b)
    BinOp(VarId, BinOp, VarId, VarId),

    /// result = op(a)
    UnaryOp(VarId, UnaryOp, VarId),

    /// result = {e1, e2, ..., eN}
    MakeTuple(VarId, Vec<VarId>),

    /// result = [head | tail]
    MakeCons(VarId, VarId, VarId),

    /// result = element(index, tuple) — 0-indexed in IR
    TupleElement(VarId, VarId, u32),

    /// result = hd(list)
    ListHead(VarId, VarId),

    /// result = tl(list)
    ListTail(VarId, VarId),

    /// result = call target(args...)
    Call(VarId, CallTarget, Vec<VarId>),

    /// result = self()
    GetSelf(VarId),

    /// result = spawn(mod, fun, args_list)
    Spawn(VarId, AtomId, AtomId, Vec<VarId>),

    /// pid ! message (result = message)
    Send(VarId, VarId, VarId),

    /// Fused MakeTuple + Send: constructs tuple directly on receiver heap.
    /// result = pid ! {elem0, elem1, ...}
    /// Avoids allocating the tuple on the sender heap entirely.
    SendDirect(VarId, VarId, Vec<VarId>),

    /// Decrement reduction counter; yield if exhausted
    CheckReductions,

    /// Yield-capable reduction check inserted at CFG back-edges.
    /// Decrements reduction counter; if exhausted, yields the process
    /// with full save/resume support.
    YieldCheck,

    /// result = type_test(val)
    TypeTest(VarId, TypeTestKind, VarId),

    /// Copy a register (including shared results written by alternative branches)
    Move(VarId, VarId),

    /// result = tuple_size(tuple)
    TupleSize(VarId, VarId),

    /// result = alloc_binary_zeroed(byte_size)
    MakeBinary(VarId, VarId),

    /// binary_put_integer(bin, bit_offset, value, bit_size, flags)
    /// Writes an integer value into the binary at the given BIT offset.
    /// bit_offset and bit_size are tagged small ints in bits.
    /// For byte-aligned cases, bit_offset is a multiple of 8.
    BinaryPutInteger(VarId, VarId, VarId, VarId, BinFlags),

    /// binary_put_float(bin, bit_offset, value, bit_size, flags)
    /// Writes a float value (IEEE 754) into the binary at the given BIT offset.
    /// bit_offset is in bits. bit_size must be 16, 32 or 64.
    BinaryPutFloat(VarId, VarId, VarId, VarId, BinFlags),

    /// binary_put_binary(dest_bin, dest_bit_offset, src_bin, bit_count)
    /// Copies bits from a source binary into the destination binary.
    /// dest_bit_offset and bit_count must be byte-aligned (multiples of 8) for now.
    BinaryPutBinary(VarId, VarId, VarId, VarId),

    /// result = binary_get_integer(bin, bit_offset, bit_size, flags)
    /// Extracts an integer value from the binary at the given BIT offset.
    BinaryGetInteger(VarId, VarId, VarId, VarId, BinFlags),

    /// result = binary_get_float(bin, bit_offset, bit_size, flags)
    /// Extracts a float value (IEEE 754) from the binary at the given BIT offset.
    /// Returns a boxed finite float, or nil on match failure.
    /// bit_offset is in bits; bit_size must be 16, 32 or 64.
    BinaryGetFloat(VarId, VarId, VarId, VarId, BinFlags),

    /// result = binary_get_binary(bin, bit_offset, bit_count)
    /// Extracts a sub-binary from the binary at the given BIT offset.
    /// bit_offset and bit_count must be byte-aligned (multiples of 8) for now.
    BinaryGetBinary(VarId, VarId, VarId, VarId),

    /// result = bit_size(binary) — returns total size in BITS as tagged small int.
    BinarySize(VarId, VarId),

    /// (base_ptr, total_bits) = bs_start_match(binary)
    /// Initializes a match context from a binary.
    /// base_ptr (first VarId) is UNTAGGED raw pointer to binary data.
    /// total_bits (second VarId) is tagged small int with total BIT count.
    /// Third VarId is the input binary.
    BsStartMatch(VarId, VarId, VarId),

    /// Tagged intra-byte offset (0..7) of a reused match context, or zero for a binary.
    /// Add this only to raw data extraction offsets, not logical sizes or advances.
    BsMatchBitOffset(VarId, VarId),

    /// result = bs_get_integer_inline(base_ptr, bit_offset, bit_count, flags)
    /// Inlined integer extraction from a match context.
    /// base_ptr is UNTAGGED raw pointer, bit_offset and bit_count are tagged small ints (in bits).
    /// Returns a tagged small int.
    BsGetIntegerInline(VarId, VarId, VarId, VarId, BinFlags),

    /// result = bs_materialize(orig_binary, bit_offset, bit_count)
    /// Materializes a sub-binary when a match context variable escapes.
    /// bit_offset and bit_count are in bits. For byte-aligned cases, delegates to
    /// nera_rt_binary_get_binary after converting to bytes.
    BsMaterialize(VarId, VarId, VarId, VarId),

    /// (match_ctx, base_ptr, total_bits) = bs_match_start2(input)
    /// Like BsStartMatch but also returns a match context term that can be
    /// passed to recursive calls. Input can be a binary OR match context (reuse).
    /// match_ctx (first VarId) is a tagged BOXED_MATCH_CTX term.
    /// base_ptr (second VarId) is UNTAGGED raw pointer to binary data.
    /// total_bits (third VarId) is tagged small int with total BIT count.
    /// Fourth VarId is the input binary/match_ctx.
    BsMatchStart2(VarId, VarId, VarId, VarId),

    /// bs_match_ctx_advance(match_ctx, bit_offset)
    /// Mutates match_ctx.bit_offset += bit_offset in-place (heap write).
    /// Used when a rest segment is passed to a function call instead of materialized.
    BsMatchCtxAdvance(VarId, VarId),

    /// result = make_map([k1, v1, k2, v2, ...])
    /// Creates a new map from interleaved key-value pairs.
    MakeMap(VarId, Vec<VarId>),

    /// result = map_get(map, key)
    /// Extracts a value from a map by key.
    MapGet(VarId, VarId, VarId),

    /// result = map_put(map, key, value)
    /// Returns a new map with key set to value (insert or replace).
    MapPut(VarId, VarId, VarId, VarId),

    /// result = map_update_exact(map, key, value)
    /// Like MapPut but raises {badkey, Key} if the key does not already exist.
    /// Used for the `:=` operator in map updates.
    MapUpdateExact(VarId, VarId, VarId, VarId),

    /// Raise an exception: sets ctx.status = EXCEPTION, ctx.result = reason.
    /// class: 0 = throw, 1 = error, 2 = exit
    RaiseException(VarId, ExceptionClass, VarId),

    /// result = get_exception_class() — reads ctx.exception_class after an exception
    GetExceptionClass(VarId),

    /// result = get_exception_reason() — reads ctx.result after an exception
    GetExceptionReason(VarId),

    /// result = get_exception_stacktrace() — reads the captured stack trace
    GetExceptionStacktrace(VarId),

    /// Clear exception state: sets ctx.status = STATUS_ALIVE
    ClearException,

    /// result = check_exception() — returns true atom if ctx.status == EXCEPTION
    CheckException(VarId),

    /// result = make_closure(mangled_name, user_arity, [env_vars...])
    /// Creates a heap-allocated closure capturing the given variables.
    MakeClosure(VarId, String, u32, Vec<VarId>),

    /// result = make_remote_fun_ref(module_atom, function_atom, arity)
    /// Creates a heap-allocated closure for a remote function reference (fun M:F/A).
    MakeRemoteFunRef(VarId, AtomId, AtomId, u32),

    /// result = make_dynamic_remote_fun_ref(mod_var, fun_var, arity_var)
    /// Like MakeRemoteFunRef but M, F, A are runtime values (VarIds).
    /// mod_var and fun_var must hold atom terms; arity_var must hold a small integer.
    MakeDynamicRemoteFunRef(VarId, VarId, VarId, VarId),

    /// binary_put_utf8(bits_written, bin, bit_offset, codepoint)
    /// Encodes a Unicode codepoint as UTF-8 into the binary. Returns bits written (8/16/24/32).
    /// Raises badarg on invalid codepoint.
    BinaryPutUtf8(VarId, VarId, VarId, VarId),

    /// binary_put_utf16(bits_written, bin, bit_offset, codepoint, flags)
    /// Encodes a Unicode codepoint as UTF-16 into the binary. Returns bits written (16/32).
    /// flags.endianness controls byte order.
    BinaryPutUtf16(VarId, VarId, VarId, VarId, BinFlags),

    /// binary_put_utf32(bits_written, bin, bit_offset, codepoint, flags)
    /// Writes a Unicode codepoint as UTF-32 (always 4 bytes). Returns 32.
    /// flags.endianness controls byte order.
    BinaryPutUtf32(VarId, VarId, VarId, VarId, BinFlags),

    /// (codepoint, bits_consumed) = bs_get_utf8(base_ptr, bit_offset, total_bits)
    /// Decodes one UTF-8 codepoint from a match context.
    /// bits_consumed == 0 signals decode failure (match failure, not exception).
    /// codepoint is read from ctx.result after the FFI call.
    BsGetUtf8(VarId, VarId, VarId, VarId, VarId),

    /// (codepoint, bits_consumed) = bs_get_utf16(base_ptr, bit_offset, total_bits, flags)
    /// Decodes one UTF-16 codepoint from a match context.
    /// flags.endianness controls byte order.
    BsGetUtf16(VarId, VarId, VarId, VarId, VarId, BinFlags),

    /// (codepoint, bits_consumed) = bs_get_utf32(base_ptr, bit_offset, total_bits, flags)
    /// Decodes one UTF-32 codepoint from a match context.
    /// flags.endianness controls byte order.
    BsGetUtf32(VarId, VarId, VarId, VarId, VarId, BinFlags),

    /// binary_resize(bin, new_byte_size)
    /// Shrinks a binary to its actual size after variable-width UTF writes.
    BinaryResize(VarId, VarId),

    /// Set the current source line number on the top stack frame.
    /// Emitted before calls and exception raises for stack trace reporting.
    SetLine(u32),
}

#[derive(Debug, Clone)]
pub enum Literal {
    Integer(i64),
    BigInteger(String),
    Float(f64),
    Atom(AtomId),
    Nil,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeTestKind {
    IsNil,
    IsAtom,
    IsInteger,
    IsFloat,
    IsTuple,
    IsList,
    IsPid,
    IsNumber,
    IsBinary,
    IsBitstring,
    IsMap,
    IsFunction,
    IsReference,
}

/// Flags for binary segment operations (endianness + signedness).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinFlags {
    /// 0 = unsigned, 1 = signed
    pub signedness: u8,
    /// 0 = big, 1 = little, 2 = native
    pub endianness: u8,
}

/// Exception class for throw/error/exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionClass {
    Throw = 0,
    Error = 1,
    Exit = 2,
}

#[derive(Debug, Clone)]
pub enum CallTarget {
    /// Local function in same module
    Local(AtomId),
    /// Remote call module:function
    Remote(AtomId, AtomId),
    /// Built-in function
    Bif(String),
    /// Call through a closure variable
    Closure(VarId),
    /// Dynamic apply: module and function are runtime values (VarIds holding atoms)
    Apply(VarId, VarId),
}

#[derive(Debug, Clone)]
pub enum Terminator {
    /// Return a value from the function
    Return(VarId),

    /// Unconditional jump; arguments are reserved and must currently be empty.
    Jump(BlockId, Vec<VarId>),

    /// Conditional branch
    Branch(VarId, BlockId, BlockId),

    /// Multi-way switch on value
    Switch {
        scrutinee: VarId,
        cases: Vec<(Literal, BlockId)>,
        default: BlockId,
    },

    /// Reserved high-level receive; source receive lowers to blocks and calls.
    Receive {
        clauses: Vec<ReceiveClause>,
        timeout: Option<(VarId, BlockId)>,
        wait_block: BlockId,
    },

    /// Tail call (reuses stack frame)
    TailCall(CallTarget, Vec<VarId>),

    /// Unreachable (after exhaustive match)
    Unreachable,
}

impl Terminator {
    /// Get successor BlockIds from this terminator.
    pub fn successors(&self) -> Vec<BlockId> {
        match self {
            Terminator::Return(_) => vec![],
            Terminator::Jump(target, _) => vec![*target],
            Terminator::Branch(_, then_b, else_b) => vec![*then_b, *else_b],
            Terminator::Switch { cases, default, .. } => {
                let mut succs: Vec<BlockId> = cases.iter().map(|(_, b)| *b).collect();
                succs.push(*default);
                succs
            }
            Terminator::TailCall(_, _) => vec![],
            Terminator::Receive { clauses, timeout, wait_block, .. } => {
                let mut succs: Vec<BlockId> =
                    clauses.iter().flat_map(|c| vec![c.match_block, c.body_block]).collect();
                if let Some((_, b)) = timeout {
                    succs.push(*b);
                }
                succs.push(*wait_block);
                succs
            }
            Terminator::Unreachable => vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReceiveClause {
    pub match_block: BlockId,
    pub body_block: BlockId,
    pub binding: VarId,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // Terminator::successors()
    // ---------------------------------------------------------------

    #[test]
    fn successors_return_empty() {
        let t = Terminator::Return(VarId(0));
        assert!(t.successors().is_empty());
    }

    #[test]
    fn successors_jump_single() {
        let t = Terminator::Jump(BlockId(5), vec![]);
        assert_eq!(t.successors(), vec![BlockId(5)]);
    }

    #[test]
    fn successors_jump_with_args() {
        let t = Terminator::Jump(BlockId(3), vec![VarId(1), VarId(2)]);
        assert_eq!(t.successors(), vec![BlockId(3)]);
    }

    #[test]
    fn successors_branch() {
        let t = Terminator::Branch(VarId(0), BlockId(1), BlockId(2));
        assert_eq!(t.successors(), vec![BlockId(1), BlockId(2)]);
    }

    #[test]
    fn successors_switch_with_cases() {
        let t = Terminator::Switch {
            scrutinee: VarId(0),
            cases: vec![
                (Literal::Integer(1), BlockId(10)),
                (Literal::Integer(2), BlockId(20)),
                (Literal::Atom(AtomId(0)), BlockId(30)),
            ],
            default: BlockId(99),
        };
        assert_eq!(t.successors(), vec![BlockId(10), BlockId(20), BlockId(30), BlockId(99)]);
    }

    #[test]
    fn successors_switch_no_cases() {
        let t = Terminator::Switch {
            scrutinee: VarId(0),
            cases: vec![],
            default: BlockId(1),
        };
        assert_eq!(t.successors(), vec![BlockId(1)]);
    }

    #[test]
    fn successors_tail_call_empty() {
        let t = Terminator::TailCall(CallTarget::Local(AtomId(0)), vec![]);
        assert!(t.successors().is_empty());
    }

    #[test]
    fn successors_unreachable_empty() {
        assert!(Terminator::Unreachable.successors().is_empty());
    }

    #[test]
    fn successors_receive_with_timeout() {
        let t = Terminator::Receive {
            clauses: vec![
                ReceiveClause {
                    match_block: BlockId(1),
                    body_block: BlockId(2),
                    binding: VarId(0),
                },
            ],
            timeout: Some((VarId(1), BlockId(10))),
            wait_block: BlockId(99),
        };
        let succs = t.successors();
        assert_eq!(succs, vec![BlockId(1), BlockId(2), BlockId(10), BlockId(99)]);
    }

    #[test]
    fn successors_receive_without_timeout() {
        let t = Terminator::Receive {
            clauses: vec![
                ReceiveClause {
                    match_block: BlockId(5),
                    body_block: BlockId(6),
                    binding: VarId(0),
                },
                ReceiveClause {
                    match_block: BlockId(7),
                    body_block: BlockId(8),
                    binding: VarId(1),
                },
            ],
            timeout: None,
            wait_block: BlockId(50),
        };
        let succs = t.successors();
        assert_eq!(succs, vec![BlockId(5), BlockId(6), BlockId(7), BlockId(8), BlockId(50)]);
    }

    // ---------------------------------------------------------------
    // VarId and BlockId identity / hashing
    // ---------------------------------------------------------------

    #[test]
    fn var_id_equality() {
        assert_eq!(VarId(0), VarId(0));
        assert_ne!(VarId(0), VarId(1));
    }

    #[test]
    fn block_id_equality() {
        assert_eq!(BlockId(0), BlockId(0));
        assert_ne!(BlockId(0), BlockId(1));
    }

    #[test]
    fn var_id_hash_set() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(VarId(1));
        set.insert(VarId(2));
        set.insert(VarId(1)); // duplicate
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn block_id_hash_set() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(BlockId(1));
        set.insert(BlockId(2));
        set.insert(BlockId(1));
        assert_eq!(set.len(), 2);
    }

    // ---------------------------------------------------------------
    // IrModule / IrFunction construction
    // ---------------------------------------------------------------

    #[test]
    fn empty_module() {
        let m = IrModule {
            name: AtomId(0),
            functions: vec![],
            exports: vec![],
        };
        assert!(m.functions.is_empty());
        assert!(m.exports.is_empty());
    }

    #[test]
    fn function_with_one_block() {
        let f = IrFunction {
            name: AtomId(1),
            arity: 2,
            mangled_name: "test_fn_2".to_string(),
            blocks: vec![
                BasicBlock {
                    id: BlockId(0),
                    params: vec![VarId(0), VarId(1)],
                    instructions: vec![
                        Instruction::Const(VarId(2), Literal::Integer(42)),
                    ],
                    terminator: Terminator::Return(VarId(2)),
                },
            ],
            protected_values: Vec::new(),
            entry_block: BlockId(0),
            params: vec![VarId(0), VarId(1)],
            line: 0,
        };
        assert_eq!(f.arity, 2);
        assert_eq!(f.blocks.len(), 1);
        assert_eq!(f.blocks[0].instructions.len(), 1);
        assert_eq!(f.entry_block, BlockId(0));
    }

    #[test]
    fn module_with_exports() {
        let m = IrModule {
            name: AtomId(0),
            functions: vec![],
            exports: vec![
                IrExport { name: AtomId(1), arity: 0 },
                IrExport { name: AtomId(2), arity: 2 },
            ],
        };
        assert_eq!(m.exports.len(), 2);
        assert_eq!(m.exports[0].arity, 0);
        assert_eq!(m.exports[1].arity, 2);
    }

    // ---------------------------------------------------------------
    // ExceptionClass discriminant values
    // ---------------------------------------------------------------

    #[test]
    fn exception_class_values() {
        assert_eq!(ExceptionClass::Throw as u8, 0);
        assert_eq!(ExceptionClass::Error as u8, 1);
        assert_eq!(ExceptionClass::Exit as u8, 2);
    }

    // ---------------------------------------------------------------
    // TypeTestKind uniqueness
    // ---------------------------------------------------------------

    #[test]
    fn type_test_kinds_are_distinct() {
        let kinds = [
            TypeTestKind::IsNil,
            TypeTestKind::IsAtom,
            TypeTestKind::IsInteger,
            TypeTestKind::IsFloat,
            TypeTestKind::IsTuple,
            TypeTestKind::IsList,
            TypeTestKind::IsPid,
            TypeTestKind::IsNumber,
            TypeTestKind::IsBinary,
            TypeTestKind::IsBitstring,
            TypeTestKind::IsMap,
            TypeTestKind::IsFunction,
            TypeTestKind::IsReference,
        ];
        for (i, a) in kinds.iter().enumerate() {
            for (j, b) in kinds.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b, "kinds at {} and {} should differ", i, j);
                }
            }
        }
    }

    // ---------------------------------------------------------------
    // BinFlags
    // ---------------------------------------------------------------

    #[test]
    fn bin_flags_equality() {
        let a = BinFlags { signedness: 0, endianness: 0 };
        let b = BinFlags { signedness: 0, endianness: 0 };
        let c = BinFlags { signedness: 1, endianness: 0 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
