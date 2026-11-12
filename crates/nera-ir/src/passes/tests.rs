use super::uses::{collect_instruction_uses, collect_terminator_uses};
use super::*;
use crate::ir::*;
use nera_syntax::ast::{AtomId, BinOp};
use std::collections::HashSet;

fn assert_valid(func: &IrFunction, stage: &str) {
    let errors = crate::verify::verify_function(func);
    assert!(errors.is_empty(), "{stage}: {errors:#?}");
}

// ---------------------------------------------------------------
// eliminate_dead_match_contexts
// ---------------------------------------------------------------

#[test]
fn dead_mc_elim_replaces_unused_mc() {
    // BsMatchStart2(mc=v0, base=v1, total=v2, input=v3) where v0 is never used
    // → should become BsStartMatch(v1, v2, v3)
    let mut func = IrFunction {
        name: AtomId(0),
        arity: 1,
        mangled_name: "test".into(),
        protected_values: Vec::new(),
        entry_block: BlockId(0),
        params: vec![VarId(3)],
        line: 0,
        blocks: vec![BasicBlock {
            id: BlockId(0),
            params: vec![],
            instructions: vec![
                Instruction::BsMatchStart2(VarId(0), VarId(1), VarId(2), VarId(3)),
                // Only v1 and v2 are used, v0 (mc) is dead
                Instruction::BinOp(VarId(4), BinOp::IntDiv, VarId(2), VarId(1)),
            ],
            terminator: Terminator::Return(VarId(4)),
        }],
    };

    assert_valid(&func, "before standalone pass");
    eliminate_dead_match_contexts(&mut func);
    assert_valid(&func, "after standalone pass");

    // Should now be BsStartMatch
    assert!(
        matches!(
            &func.blocks[0].instructions[0],
            Instruction::BsStartMatch(VarId(1), VarId(2), VarId(3))
        ),
        "Expected BsMatchStart2 → BsStartMatch, got: {:?}",
        func.blocks[0].instructions[0]
    );
}

#[test]
fn dead_mc_elim_preserves_used_mc() {
    // BsMatchStart2(mc=v0, base=v1, total=v2, input=v3) where v0 IS used
    // → should NOT be replaced
    let mut func = IrFunction {
        name: AtomId(0),
        arity: 1,
        mangled_name: "test".into(),
        protected_values: Vec::new(),
        entry_block: BlockId(0),
        params: vec![VarId(3)],
        line: 0,
        blocks: vec![BasicBlock {
            id: BlockId(0),
            params: vec![],
            instructions: vec![
                Instruction::BsMatchStart2(VarId(0), VarId(1), VarId(2), VarId(3)),
                // v0 (mc) IS used by BsMatchCtxAdvance
                Instruction::BsMatchCtxAdvance(VarId(0), VarId(2)),
            ],
            terminator: Terminator::Return(VarId(0)),
        }],
    };

    assert_valid(&func, "before standalone pass");
    eliminate_dead_match_contexts(&mut func);
    assert_valid(&func, "after standalone pass");

    // Should still be BsMatchStart2
    assert!(
        matches!(
            &func.blocks[0].instructions[0],
            Instruction::BsMatchStart2(VarId(0), VarId(1), VarId(2), VarId(3))
        ),
        "BsMatchStart2 should NOT be replaced when mc is used, got: {:?}",
        func.blocks[0].instructions[0]
    );
}

#[test]
fn dead_mc_elim_used_in_terminator() {
    // mc used in Return terminator → should NOT be replaced
    let mut func = IrFunction {
        name: AtomId(0),
        arity: 1,
        mangled_name: "test".into(),
        protected_values: Vec::new(),
        entry_block: BlockId(0),
        params: vec![VarId(3)],
        line: 0,
        blocks: vec![BasicBlock {
            id: BlockId(0),
            params: vec![],
            instructions: vec![Instruction::BsMatchStart2(
                VarId(0),
                VarId(1),
                VarId(2),
                VarId(3),
            )],
            terminator: Terminator::Return(VarId(0)), // mc returned!
        }],
    };

    assert_valid(&func, "before standalone pass");
    eliminate_dead_match_contexts(&mut func);
    assert_valid(&func, "after standalone pass");

    assert!(
        matches!(
            &func.blocks[0].instructions[0],
            Instruction::BsMatchStart2(..)
        ),
        "BsMatchStart2 should NOT be replaced when mc is used in terminator"
    );
}

#[test]
fn dead_mc_elim_used_in_other_block() {
    // mc defined in block 0, used in block 1 → should NOT be replaced
    let mut func = IrFunction {
        name: AtomId(0),
        arity: 1,
        mangled_name: "test".into(),
        protected_values: Vec::new(),
        entry_block: BlockId(0),
        params: vec![VarId(3)],
        line: 0,
        blocks: vec![
            BasicBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![Instruction::BsMatchStart2(
                    VarId(0),
                    VarId(1),
                    VarId(2),
                    VarId(3),
                )],
                terminator: Terminator::Jump(BlockId(1), vec![]),
            },
            BasicBlock {
                id: BlockId(1),
                params: vec![],
                instructions: vec![Instruction::BsMatchCtxAdvance(VarId(0), VarId(2))],
                terminator: Terminator::Return(VarId(1)),
            },
        ],
    };

    assert_valid(&func, "before standalone pass");
    eliminate_dead_match_contexts(&mut func);
    assert_valid(&func, "after standalone pass");

    assert!(
        matches!(
            &func.blocks[0].instructions[0],
            Instruction::BsMatchStart2(..)
        ),
        "BsMatchStart2 should NOT be replaced when mc is used in another block"
    );
}

// ---------------------------------------------------------------
// collect_instruction_uses / collect_terminator_uses
// ---------------------------------------------------------------

#[test]
fn collect_uses_bs_match_start2_only_input() {
    let mut uses = HashSet::new();
    let inst = Instruction::BsMatchStart2(VarId(0), VarId(1), VarId(2), VarId(3));
    collect_instruction_uses(&inst, &mut uses);
    // Only input (v3) is a use; mc, base_ptr, total_bits are defs
    assert!(uses.contains(&VarId(3)), "input should be a use");
    assert!(!uses.contains(&VarId(0)), "mc should not be a use");
    assert!(!uses.contains(&VarId(1)), "base_ptr should not be a use");
    assert!(!uses.contains(&VarId(2)), "total_bits should not be a use");
}

#[test]
fn collect_uses_bs_match_ctx_advance() {
    let mut uses = HashSet::new();
    let inst = Instruction::BsMatchCtxAdvance(VarId(5), VarId(6));
    collect_instruction_uses(&inst, &mut uses);
    assert!(uses.contains(&VarId(5)));
    assert!(uses.contains(&VarId(6)));
}

#[test]
fn collect_uses_terminator_return() {
    let mut uses = HashSet::new();
    let term = Terminator::Return(VarId(7));
    collect_terminator_uses(&term, &mut uses);
    assert!(uses.contains(&VarId(7)));
}

#[test]
fn collect_uses_terminator_tail_call() {
    let mut uses = HashSet::new();
    let term = Terminator::TailCall(CallTarget::Closure(VarId(10)), vec![VarId(11), VarId(12)]);
    collect_terminator_uses(&term, &mut uses);
    assert!(uses.contains(&VarId(10)));
    assert!(uses.contains(&VarId(11)));
    assert!(uses.contains(&VarId(12)));
}
