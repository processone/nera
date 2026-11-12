//! Behavioral checks at the public pass boundary. Every fixture is structurally
//! verified before and after transformation.
use nera_ir::ir::*;
use nera_ir::passes::{fuse_make_tuple_send, insert_backedge_yield_checks, run_function_passes};
use nera_ir::verify::verify_function;
use nera_syntax::ast::{AtomId, BinOp};

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
        name: AtomId(0),
        arity: 2,
        mangled_name: "fixture/2".into(),
        blocks,
        protected_values: Vec::new(),
        entry_block: BlockId(0),
        params: vec![VarId(0), VarId(1)],
        line: 0,
    }
}

fn run_verified(func: &mut IrFunction, pass: fn(&mut IrFunction)) {
    assert_eq!(verify_function(func), vec![], "before pass");
    pass(func);
    assert_eq!(verify_function(func), vec![], "after pass");
}

#[test]
fn yield_is_inserted_only_at_loop_backedge_and_is_idempotent() {
    let mut func = function(vec![
        block(
            0,
            vec![],
            Terminator::Branch(VarId(0), BlockId(1), BlockId(2)),
        ),
        block(1, vec![], Terminator::Jump(BlockId(0), vec![])),
        block(2, vec![], Terminator::Return(VarId(1))),
    ]);
    run_verified(&mut func, insert_backedge_yield_checks);
    assert!(func.blocks[0].instructions.is_empty());
    assert!(matches!(
        func.blocks[1].instructions.as_slice(),
        [Instruction::YieldCheck]
    ));
    assert!(func.blocks[2].instructions.is_empty());
    let once = format!("{func:#?}");
    run_verified(&mut func, insert_backedge_yield_checks);
    assert_eq!(format!("{func:#?}"), once);
}

#[test]
fn existing_reduction_check_is_preserved() {
    let mut func = function(vec![block(
        0,
        vec![Instruction::CheckReductions],
        Terminator::Jump(BlockId(0), vec![]),
    )]);
    let before = format!("{func:#?}");
    run_verified(&mut func, insert_backedge_yield_checks);
    assert_eq!(format!("{func:#?}"), before);
}

#[test]
fn tuple_send_fuses_and_preserves_result_register() {
    let mut func = function(vec![block(
        0,
        vec![
            Instruction::MakeTuple(VarId(2), vec![VarId(1)]),
            Instruction::Send(VarId(3), VarId(0), VarId(2)),
        ],
        Terminator::Return(VarId(3)),
    )]);
    run_verified(&mut func, fuse_make_tuple_send);
    assert!(matches!(
        func.blocks[0].instructions[0],
        Instruction::Const(VarId(2), Literal::Nil)
    ));
    match &func.blocks[0].instructions[1] {
        Instruction::SendDirect(VarId(3), VarId(0), elems) => assert_eq!(elems, &[VarId(1)]),
        other => panic!("expected fused send, got {other:?}"),
    }
}

#[test]
fn large_tuple_send_is_not_fused() {
    let mut func = function(vec![block(
        0,
        vec![
            Instruction::MakeTuple(VarId(2), vec![VarId(1); 5]),
            Instruction::Send(VarId(3), VarId(0), VarId(2)),
        ],
        Terminator::Return(VarId(3)),
    )]);
    let before = format!("{func:#?}");
    run_verified(&mut func, fuse_make_tuple_send);
    assert_eq!(format!("{func:#?}"), before);
}

#[test]
fn tuple_used_in_another_block_is_not_fused() {
    let mut func = function(vec![
        block(
            0,
            vec![
                Instruction::MakeTuple(VarId(2), vec![VarId(1)]),
                Instruction::Send(VarId(3), VarId(0), VarId(2)),
            ],
            Terminator::Jump(BlockId(1), vec![]),
        ),
        block(1, vec![], Terminator::Return(VarId(2))),
    ]);
    let before = format!("{func:#?}");
    run_verified(&mut func, fuse_make_tuple_send);
    assert_eq!(format!("{func:#?}"), before);
}

#[test]
fn pipeline_preserves_yield_before_folding_and_dead_code() {
    let mut func = function(vec![
        block(
            0,
            vec![
                Instruction::BsMatchStart2(VarId(3), VarId(4), VarId(5), VarId(1)),
                Instruction::MakeTuple(VarId(6), vec![VarId(1)]),
                Instruction::Send(VarId(7), VarId(0), VarId(6)),
                Instruction::Const(VarId(8), Literal::Integer(1)),
                Instruction::Const(VarId(9), Literal::Integer(2)),
                Instruction::BinOp(VarId(10), BinOp::Lt, VarId(8), VarId(9)),
            ],
            Terminator::Branch(VarId(10), BlockId(1), BlockId(0)),
        ),
        block(1, vec![], Terminator::Return(VarId(7))),
    ]);
    run_verified(&mut func, |func| {
        run_function_passes(func, AtomId(100), AtomId(101))
    });
    let expected = function(vec![
        block(
            0,
            vec![
                Instruction::BsStartMatch(VarId(4), VarId(5), VarId(1)),
                Instruction::Const(VarId(6), Literal::Nil),
                Instruction::SendDirect(VarId(7), VarId(0), vec![VarId(1)]),
                Instruction::Const(VarId(8), Literal::Integer(1)),
                Instruction::Const(VarId(9), Literal::Integer(2)),
                Instruction::Const(VarId(10), Literal::Atom(AtomId(100))),
                // Scheduling was inserted before folding removed the back-edge.
                Instruction::YieldCheck,
            ],
            Terminator::Jump(BlockId(1), vec![]),
        ),
        block(1, vec![], Terminator::Return(VarId(7))),
    ]);
    assert_eq!(format!("{func:#?}"), format!("{expected:#?}"));
}
