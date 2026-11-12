//! Remove unused heap-allocated binary match contexts.

use super::uses::{collect_instruction_uses, collect_terminator_uses};
use crate::ir::{Instruction, IrFunction, VarId};
use std::collections::HashSet;

/// Replace `BsMatchStart2(mc, base, total, input)` with `BsStartMatch(base, total, input)`
/// when the `mc` VarId is never used by any other instruction or terminator.
/// This eliminates unnecessary heap allocation of match context objects.
pub fn eliminate_dead_match_contexts(func: &mut IrFunction) {
    // Pass 1: collect all VarIds that are *used* (read) across the function.
    let mut used: HashSet<VarId> = HashSet::new();

    for block in &func.blocks {
        for inst in &block.instructions {
            collect_instruction_uses(inst, &mut used);
        }
        collect_terminator_uses(&block.terminator, &mut used);
        // Block params are definitions, not uses — skip them.
    }

    // Pass 2: replace BsMatchStart2 → BsStartMatch where mc is dead.
    for block in &mut func.blocks {
        for inst in &mut block.instructions {
            let should_replace =
                matches!(inst, Instruction::BsMatchStart2(mc, _, _, _) if !used.contains(mc));
            if should_replace {
                if let Instruction::BsMatchStart2(_, base_ptr, total_bits, input) = inst.clone() {
                    *inst = Instruction::BsStartMatch(base_ptr, total_bits, input);
                }
            }
        }
    }
}
