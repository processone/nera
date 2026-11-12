//! Tuple/send fusion using the existing per-block use-count heuristic.

use super::uses::{collect_instruction_uses, collect_terminator_uses};
use crate::ir::{Instruction, IrFunction, Literal, VarId};
use std::collections::HashSet;

/// Fuse MakeTuple + Send into SendDirect when the tuple is single-use.
/// This avoids allocating the tuple on the sender heap; the FFI function
/// constructs it directly on the receiver heap.
pub fn fuse_make_tuple_send(func: &mut IrFunction) {
    use std::collections::HashMap;

    // Pass 1: count uses of each VarId across all blocks.
    let mut use_count: HashMap<VarId, u32> = HashMap::new();
    for block in &func.blocks {
        let mut uses = HashSet::new();
        for inst in &block.instructions {
            collect_instruction_uses(inst, &mut uses);
        }
        collect_terminator_uses(&block.terminator, &mut uses);
        for v in uses {
            *use_count.entry(v).or_insert(0) += 1;
        }
    }

    // Pass 2: within each block, find MakeTuple(msg, elems) followed by
    // Send(result, pid, msg) where msg is single-use and arity <= 4.
    for block in &mut func.blocks {
        // Build a map from VarId -> instruction index for MakeTuple in this block.
        let mut tuple_defs: HashMap<VarId, usize> = HashMap::new();
        for (i, inst) in block.instructions.iter().enumerate() {
            if let Instruction::MakeTuple(var, _) = inst {
                tuple_defs.insert(*var, i);
            }
        }

        // Find Send instructions that reference a single-use MakeTuple.
        let mut fusions: Vec<(usize, usize, VarId, VarId, Vec<VarId>)> = Vec::new(); // (tuple_idx, send_idx, result, pid, elems)
        for (i, inst) in block.instructions.iter().enumerate() {
            if let Instruction::Send(result, pid, msg) = inst {
                if let Some(&tuple_idx) = tuple_defs.get(msg) {
                    if use_count.get(msg).copied().unwrap_or(0) == 1 {
                        if let Instruction::MakeTuple(_, elems) = &block.instructions[tuple_idx] {
                            if elems.len() <= 4 {
                                fusions.push((tuple_idx, i, *result, *pid, elems.clone()));
                            }
                        }
                    }
                }
            }
        }

        // Apply fusions in reverse order to preserve indices.
        for (tuple_idx, send_idx, result, pid, elems) in fusions.into_iter().rev() {
            // Extract the MakeTuple's result var for the dead Const replacement.
            let dead_var = match &block.instructions[tuple_idx] {
                Instruction::MakeTuple(v, _) => *v,
                _ => unreachable!(),
            };
            block.instructions[send_idx] = Instruction::SendDirect(result, pid, elems);
            // Replace MakeTuple with a dead Const nil — it's unused now.
            block.instructions[tuple_idx] = Instruction::Const(dead_var, Literal::Nil);
        }
    }
}
