use super::*;
use crate::ir::*;
use nera_syntax::ast::AtomId;

fn block(id: u32, instructions: Vec<Instruction>, terminator: Terminator) -> BasicBlock {
    BasicBlock {
        id: BlockId(id),
        params: vec![],
        instructions,
        terminator,
    }
}

fn function(blocks: Vec<BasicBlock>) -> IrFunction {
    IrFunction {
        name: AtomId(1),
        arity: 1,
        mangled_name: "fixture/1".into(),
        blocks,
        protected_values: Vec::new(),
        entry_block: BlockId(0),
        params: vec![VarId(0)],
        line: 0,
    }
}

fn simple() -> IrFunction {
    function(vec![block(0, vec![], Terminator::Return(VarId(0)))])
}

fn has_error(f: &IrFunction, kind: ErrorKind, location: Location) {
    let errors = verify_function(f);
    assert!(
        errors
            .iter()
            .any(|e| e.kind == kind && e.location == location),
        "expected {kind:?} at {location:?}, got {errors:?}"
    );
    assert!(errors.iter().all(|e| e.function == f.mangled_name));
    assert_eq!(
        errors,
        verify_function(f),
        "diagnostics must be deterministic"
    );
}

#[test]
fn rejects_missing_entry_and_empty_function() {
    has_error(
        &function(vec![]),
        ErrorKind::MissingEntry(BlockId(0)),
        Location::Function,
    );
    let mut f = simple();
    f.entry_block = BlockId(7);
    has_error(&f, ErrorKind::MissingEntry(BlockId(7)), Location::Function);
}

#[test]
fn rejects_duplicate_block_ids() {
    let mut f = simple();
    f.blocks.push(f.blocks[0].clone());
    has_error(
        &f,
        ErrorKind::DuplicateBlock(BlockId(0)),
        Location::Block(BlockId(0)),
    );
}

#[test]
fn checks_function_parameter_shape() {
    let mut f = simple();
    f.arity = 2;
    has_error(
        &f,
        ErrorKind::ArityMismatch {
            arity: 2,
            params: 1,
        },
        Location::Function,
    );
    f.params.push(VarId(0));
    has_error(
        &f,
        ErrorKind::DuplicateParameter(VarId(0)),
        Location::Function,
    );
}

#[test]
fn checks_every_successor_even_in_dead_blocks() {
    let terms = [
        Terminator::Jump(BlockId(99), vec![]),
        Terminator::Branch(VarId(0), BlockId(0), BlockId(99)),
        Terminator::Branch(VarId(0), BlockId(99), BlockId(0)),
        Terminator::Switch {
            scrutinee: VarId(0),
            cases: vec![(Literal::Integer(1), BlockId(99))],
            default: BlockId(0),
        },
        Terminator::Switch {
            scrutinee: VarId(0),
            cases: vec![],
            default: BlockId(99),
        },
        Terminator::Receive {
            clauses: vec![ReceiveClause {
                match_block: BlockId(99),
                body_block: BlockId(0),
                binding: VarId(0),
            }],
            timeout: None,
            wait_block: BlockId(0),
        },
        Terminator::Receive {
            clauses: vec![],
            timeout: Some((VarId(0), BlockId(99))),
            wait_block: BlockId(0),
        },
        Terminator::Receive {
            clauses: vec![],
            timeout: None,
            wait_block: BlockId(99),
        },
    ];
    for term in terms {
        let mut f = simple();
        f.blocks.push(block(7, vec![], term));
        has_error(
            &f,
            ErrorKind::MissingSuccessor(BlockId(99)),
            Location::Terminator(BlockId(7)),
        );
    }
}

#[test]
fn rejects_reserved_fields_and_receive() {
    let mut f = simple();
    f.blocks[0].params.push(VarId(1));
    has_error(
        &f,
        ErrorKind::ReservedBlockParameters,
        Location::Block(BlockId(0)),
    );
    f.blocks[0].params.clear();
    f.blocks[0].terminator = Terminator::Jump(BlockId(0), vec![VarId(0)]);
    has_error(
        &f,
        ErrorKind::ReservedJumpArguments,
        Location::Terminator(BlockId(0)),
    );
    f.blocks[0].terminator = Terminator::Receive {
        clauses: vec![],
        timeout: None,
        wait_block: BlockId(0),
    };
    has_error(
        &f,
        ErrorKind::ReservedReceive,
        Location::Terminator(BlockId(0)),
    );
}

#[test]
fn checks_map_pairs_and_binary_flag_ranges() {
    let mut f = simple();
    f.blocks[0].instructions = vec![Instruction::MakeMap(VarId(1), vec![VarId(0)])];
    has_error(
        &f,
        ErrorKind::OddMapOperandCount(1),
        Location::Instruction(BlockId(0), 0),
    );
    for flags in [
        BinFlags {
            signedness: 2,
            endianness: 0,
        },
        BinFlags {
            signedness: 0,
            endianness: 3,
        },
    ] {
        f.blocks[0].instructions = vec![Instruction::BinaryPutInteger(
            VarId(0),
            VarId(0),
            VarId(0),
            VarId(0),
            flags,
        )];
        has_error(
            &f,
            ErrorKind::InvalidBinFlags {
                signedness: flags.signedness,
                endianness: flags.endianness,
            },
            Location::Instruction(BlockId(0), 0),
        );
    }
}

#[test]
fn checks_all_results_of_binary_instructions() {
    let input = VarId(0);
    let a = VarId(1);
    let b = VarId(2);
    let c = VarId(3);
    let flags = BinFlags {
        signedness: 0,
        endianness: 0,
    };
    let cases = [
        (Instruction::BsMatchBitOffset(a, input), vec![a]),
        (Instruction::BsStartMatch(a, b, input), vec![a, b]),
        (Instruction::BsMatchStart2(a, b, c, input), vec![a, b, c]),
        (
            Instruction::BsGetUtf8(a, b, input, input, input),
            vec![a, b],
        ),
        (
            Instruction::BsGetUtf16(a, b, input, input, input, flags),
            vec![a, b],
        ),
        (
            Instruction::BsGetUtf32(a, b, input, input, input, flags),
            vec![a, b],
        ),
    ];
    for (instruction, results) in cases {
        let f = function(vec![block(
            0,
            vec![instruction, Instruction::MakeTuple(VarId(4), results)],
            Terminator::Return(VarId(4)),
        )]);
        assert_eq!(verify_function(&f), vec![]);
    }
    let f = function(vec![block(
        0,
        vec![Instruction::BsMatchStart2(a, b, a, input)],
        Terminator::Return(a),
    )]);
    has_error(
        &f,
        ErrorKind::DuplicateResult(a),
        Location::Instruction(BlockId(0), 0),
    );
}

#[test]
fn accepts_sparse_ids_reordered_blocks_and_mutable_branch_results() {
    let f = function(vec![
        block(70, vec![], Terminator::Return(VarId(800))),
        block(
            20,
            vec![Instruction::Move(VarId(800), VarId(0))],
            Terminator::Jump(BlockId(70), vec![]),
        ),
        block(
            10,
            vec![Instruction::Const(VarId(800), Literal::Nil)],
            Terminator::Jump(BlockId(70), vec![]),
        ),
        block(
            0,
            vec![],
            Terminator::Branch(VarId(0), BlockId(10), BlockId(20)),
        ),
    ]);
    assert_eq!(verify_function(&f), vec![]);
}

#[test]
fn rejects_undeclared_operands_including_dynamic_targets_and_dead_code() {
    let missing = VarId(999);
    let instructions = [
        Instruction::Move(VarId(1), missing),
        Instruction::Call(VarId(1), CallTarget::Closure(missing), vec![]),
        Instruction::Call(VarId(1), CallTarget::Apply(missing, VarId(0)), vec![]),
        Instruction::Call(VarId(1), CallTarget::Apply(VarId(0), missing), vec![]),
        Instruction::Call(VarId(1), CallTarget::Local(AtomId(0)), vec![missing]),
        Instruction::MakeClosure(VarId(1), "lambda".into(), 0, vec![missing]),
        Instruction::BinaryResize(VarId(0), missing),
        Instruction::BsMatchCtxAdvance(missing, VarId(0)),
    ];
    for instruction in instructions {
        let mut f = simple();
        f.blocks
            .push(block(9, vec![instruction], Terminator::Unreachable));
        has_error(
            &f,
            ErrorKind::UndeclaredVariable(missing),
            Location::Instruction(BlockId(9), 0),
        );
    }
    for target in [
        CallTarget::Closure(missing),
        CallTarget::Apply(VarId(0), missing),
    ] {
        let f = function(vec![block(0, vec![], Terminator::TailCall(target, vec![]))]);
        has_error(
            &f,
            ErrorKind::UndeclaredVariable(missing),
            Location::Terminator(BlockId(0)),
        );
    }
}

#[test]
fn rejects_read_before_write_even_when_result_is_same_register() {
    let f = function(vec![block(
        0,
        vec![Instruction::Move(VarId(1), VarId(1))],
        Terminator::Return(VarId(1)),
    )]);
    has_error(
        &f,
        ErrorKind::UninitializedVariable(VarId(1)),
        Location::Instruction(BlockId(0), 0),
    );
}

#[test]
fn rejects_definition_on_only_one_branch() {
    let f = function(vec![
        block(
            0,
            vec![],
            Terminator::Branch(VarId(0), BlockId(1), BlockId(2)),
        ),
        block(
            1,
            vec![Instruction::Move(VarId(1), VarId(0))],
            Terminator::Jump(BlockId(3), vec![]),
        ),
        block(2, vec![], Terminator::Jump(BlockId(3), vec![])),
        block(3, vec![], Terminator::Return(VarId(1))),
    ]);
    has_error(
        &f,
        ErrorKind::UninitializedVariable(VarId(1)),
        Location::Terminator(BlockId(3)),
    );
}

#[test]
fn backedge_does_not_initialize_first_iteration() {
    let mut f = function(vec![
        block(0, vec![], Terminator::Jump(BlockId(1), vec![])),
        block(
            1,
            vec![
                Instruction::Move(VarId(2), VarId(1)),
                Instruction::Move(VarId(1), VarId(0)),
            ],
            Terminator::Branch(VarId(0), BlockId(1), BlockId(2)),
        ),
        block(2, vec![], Terminator::Return(VarId(1))),
    ]);
    has_error(
        &f,
        ErrorKind::UninitializedVariable(VarId(1)),
        Location::Instruction(BlockId(1), 0),
    );
    f.blocks[0]
        .instructions
        .push(Instruction::Move(VarId(1), VarId(0)));
    assert_eq!(
        verify_function(&f),
        vec![],
        "initialized loop-carried registers are legal"
    );
}

#[test]
fn unreachable_predecessor_does_not_poison_reachable_join() {
    let f = function(vec![
        block(
            0,
            vec![Instruction::Move(VarId(1), VarId(0))],
            Terminator::Jump(BlockId(2), vec![]),
        ),
        block(1, vec![], Terminator::Jump(BlockId(2), vec![])),
        block(2, vec![], Terminator::Return(VarId(1))),
    ]);
    assert_eq!(verify_function(&f), vec![]);
}

#[test]
fn definition_in_dead_block_does_not_initialize_reachable_use() {
    let f = function(vec![
        block(0, vec![], Terminator::Return(VarId(1))),
        block(
            1,
            vec![Instruction::Const(VarId(1), Literal::Nil)],
            Terminator::Unreachable,
        ),
    ]);
    has_error(
        &f,
        ErrorKind::UninitializedVariable(VarId(1)),
        Location::Terminator(BlockId(0)),
    );
}

#[test]
fn reads_precede_writes_and_instruction_order_matters() {
    let mut f = function(vec![block(
        0,
        vec![
            Instruction::Move(VarId(1), VarId(2)),
            Instruction::Const(VarId(2), Literal::Nil),
        ],
        Terminator::Return(VarId(1)),
    )]);
    has_error(
        &f,
        ErrorKind::UninitializedVariable(VarId(2)),
        Location::Instruction(BlockId(0), 0),
    );
    f.blocks[0].instructions.swap(0, 1);
    assert!(verify_function(&f).is_empty());
    f.blocks[0]
        .instructions
        .push(Instruction::Move(VarId(0), VarId(0)));
    assert!(
        verify_function(&f).is_empty(),
        "reassigning initialized parameters is legal"
    );
}

#[test]
fn entry_backedge_cannot_supply_initial_values() {
    let f = function(vec![
        block(
            0,
            vec![Instruction::Move(VarId(2), VarId(1))],
            Terminator::Jump(BlockId(1), vec![]),
        ),
        block(
            1,
            vec![Instruction::Const(VarId(1), Literal::Nil)],
            Terminator::Jump(BlockId(0), vec![]),
        ),
    ]);
    has_error(
        &f,
        ErrorKind::UninitializedVariable(VarId(1)),
        Location::Instruction(BlockId(0), 0),
    );
}

#[test]
fn switch_edges_participate_in_definite_assignment() {
    let f = function(vec![
        block(
            0,
            vec![],
            Terminator::Switch {
                scrutinee: VarId(0),
                cases: vec![(Literal::Integer(1), BlockId(1))],
                default: BlockId(2),
            },
        ),
        block(
            1,
            vec![Instruction::Const(VarId(1), Literal::Nil)],
            Terminator::Jump(BlockId(3), vec![]),
        ),
        block(2, vec![], Terminator::Jump(BlockId(3), vec![])),
        block(3, vec![], Terminator::Return(VarId(1))),
    ]);
    has_error(
        &f,
        ErrorKind::UninitializedVariable(VarId(1)),
        Location::Terminator(BlockId(3)),
    );
}

#[test]
fn module_verification_preserves_function_identity_and_allows_empty_modules() {
    let mut module = IrModule {
        name: AtomId(0),
        functions: vec![],
        exports: vec![],
    };
    assert!(verify_module(&module).is_empty());
    let mut f = function(vec![]);
    module.functions.push(f.clone());
    f.mangled_name = "second/1".into();
    module.functions.push(f);
    let errors = verify_module(&module);
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0].function, "fixture/1");
    assert_eq!(errors[1].function, "second/1");
    assert!(errors[0].to_string().contains("entry block"));
}

#[test]
fn side_effects_read_operands_without_defining_them() {
    let v = VarId(1);
    let flags = BinFlags {
        signedness: 0,
        endianness: 0,
    };
    for inst in [
        Instruction::BinaryPutInteger(v, VarId(0), VarId(0), VarId(0), flags),
        Instruction::BinaryPutFloat(v, VarId(0), VarId(0), VarId(0), flags),
        Instruction::BinaryPutBinary(v, VarId(0), VarId(0), VarId(0)),
        Instruction::BinaryResize(v, VarId(0)),
        Instruction::BsMatchCtxAdvance(v, VarId(0)),
    ] {
        let f = function(vec![block(0, vec![inst], Terminator::Return(VarId(0)))]);
        has_error(
            &f,
            ErrorKind::UndeclaredVariable(v),
            Location::Instruction(BlockId(0), 0),
        );
    }
}

#[test]
fn binary_flag_validation_covers_each_flag_bearing_variant() {
    let v = VarId(0);
    let r = VarId(1);
    let flags = BinFlags {
        signedness: 0,
        endianness: 7,
    };
    for inst in [
        Instruction::BinaryPutInteger(v, v, v, v, flags),
        Instruction::BinaryPutFloat(v, v, v, v, flags),
        Instruction::BinaryGetInteger(r, v, v, v, flags),
        Instruction::BinaryGetFloat(r, v, v, v, flags),
        Instruction::BsGetIntegerInline(r, v, v, v, flags),
        Instruction::BinaryPutUtf16(r, v, v, v, flags),
        Instruction::BinaryPutUtf32(r, v, v, v, flags),
        Instruction::BsGetUtf16(r, VarId(2), v, v, v, flags),
        Instruction::BsGetUtf32(r, VarId(2), v, v, v, flags),
    ] {
        let f = function(vec![block(0, vec![inst], Terminator::Return(v))]);
        has_error(
            &f,
            ErrorKind::InvalidBinFlags {
                signedness: 0,
                endianness: 7,
            },
            Location::Instruction(BlockId(0), 0),
        );
    }
}
