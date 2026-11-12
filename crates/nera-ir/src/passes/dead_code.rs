use crate::ir::{Instruction, IrFunction, Literal, Terminator, VarId};
use nera_syntax::ast::AtomId;

/// Compare two `Literal` values structurally, using `to_bits()` for floats
/// (since `Literal` doesn't derive `PartialEq` due to `f64`).
fn literal_matches(a: &Literal, b: &Literal) -> bool {
    match (a, b) {
        (Literal::Integer(x), Literal::Integer(y)) => x == y,
        (Literal::BigInteger(x), Literal::BigInteger(y)) => x == y,
        (Literal::Float(x), Literal::Float(y)) => x.to_bits() == y.to_bits(),
        (Literal::Atom(x), Literal::Atom(y)) => x == y,
        (Literal::Nil, Literal::Nil) => true,
        _ => false,
    }
}

/// Eliminate dead code after constant folding.
///
/// Phase 1: Simplify constant branches and switches into unconditional jumps.
/// Phase 2: Remove blocks unreachable from the entry block.
pub fn eliminate_dead_code(func: &mut IrFunction, true_atom_id: AtomId, false_atom_id: AtomId) {
    use std::collections::{HashMap, HashSet, VecDeque};

    // Phase 1: Simplify constant branches/switches
    for block in &mut func.blocks {
        // Build map of known constant values in this block
        let mut known: HashMap<VarId, Literal> = HashMap::new();
        for inst in &block.instructions {
            if let Instruction::Const(var, lit) = inst {
                known.insert(*var, lit.clone());
            }
        }

        match &block.terminator {
            Terminator::Branch(cond, then_block, else_block) => {
                if let Some(Literal::Atom(id)) = known.get(cond) {
                    let then_b = *then_block;
                    let else_b = *else_block;
                    if *id == true_atom_id {
                        block.terminator = Terminator::Jump(then_b, vec![]);
                    } else if *id == false_atom_id {
                        block.terminator = Terminator::Jump(else_b, vec![]);
                    }
                }
            }
            Terminator::Switch {
                scrutinee,
                cases,
                default,
            } => {
                if let Some(lit) = known.get(scrutinee) {
                    let target = cases
                        .iter()
                        .find(|(case_lit, _)| literal_matches(lit, case_lit))
                        .map(|(_, block_id)| *block_id)
                        .unwrap_or(*default);
                    block.terminator = Terminator::Jump(target, vec![]);
                }
            }
            _ => {}
        }
    }

    // Phase 2: Remove unreachable blocks via BFS from entry
    let mut reachable = HashSet::new();
    let mut queue = VecDeque::new();
    reachable.insert(func.entry_block);
    queue.push_back(func.entry_block);

    while let Some(block_id) = queue.pop_front() {
        if let Some(block) = func.blocks.iter().find(|b| b.id == block_id) {
            for succ in block.terminator.successors() {
                if reachable.insert(succ) {
                    queue.push_back(succ);
                }
            }
        }
    }

    func.blocks.retain(|b| reachable.contains(&b.id));
}
