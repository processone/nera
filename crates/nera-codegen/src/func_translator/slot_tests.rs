use super::liveness::block_local_variables;
use nera_ir::ir::{BasicBlock, BlockId, Instruction, IrFunction, Literal, Terminator, VarId};
use nera_syntax::ast::AtomId;

#[test]
fn mime_slots_keep_parameters_and_cross_block_returns_distinct() {
    // The MIME allocation fix may share only local temporaries. A fixed IR
    // boundary makes the expected ownership independent of operand discovery
    // and frontend choices about where to put the return.
    let extension = VarId(0);
    let default = VarId(1);
    let selected = VarId(2);
    let first_nil = VarId(3);
    let first_tuple = VarId(4);
    let second_nil = VarId(5);
    let second_tuple = VarId(6);
    let entry = BlockId(0);
    let exit = BlockId(1);
    let function = IrFunction {
        name: AtomId(0),
        arity: 2,
        mangled_name: "mime_slots__lookup_2".into(),
        params: vec![extension, default],
        entry_block: entry,
        protected_values: Vec::new(),
        line: 1,
        blocks: vec![
            BasicBlock {
                id: entry,
                params: Vec::new(),
                instructions: vec![
                    Instruction::Const(first_nil, Literal::Nil),
                    Instruction::MakeTuple(first_tuple, vec![extension, first_nil]),
                    Instruction::Move(selected, first_tuple),
                ],
                terminator: Terminator::Jump(exit, Vec::new()),
            },
            BasicBlock {
                id: exit,
                params: Vec::new(),
                instructions: vec![
                    Instruction::Const(second_nil, Literal::Nil),
                    Instruction::MakeTuple(second_tuple, vec![default, second_nil]),
                ],
                terminator: Terminator::Return(selected),
            },
        ],
    };
    assert_eq!(
        block_local_variables(&function),
        vec![vec![first_nil, first_tuple], vec![second_nil, second_tuple]],
    );
}
