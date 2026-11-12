//! Scheduling checks on explicit CFG back-edges.

use crate::ir::{BasicBlock, BlockId, Instruction, IrFunction};
use std::collections::{HashMap, HashSet};

/// Insert a yield check at each DFS back-edge source lacking a final reduction check.
pub fn insert_backedge_yield_checks(func: &mut IrFunction) {
    // Build block index for quick lookup
    let block_index: HashMap<BlockId, usize> = func
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id, i))
        .collect();

    // DFS to find back-edges
    let mut on_stack: HashSet<BlockId> = HashSet::new();
    let mut visited: HashSet<BlockId> = HashSet::new();
    let mut backedge_sources: HashSet<BlockId> = HashSet::new();

    fn dfs(
        block_id: BlockId,
        blocks: &[BasicBlock],
        block_index: &HashMap<BlockId, usize>,
        visited: &mut HashSet<BlockId>,
        on_stack: &mut HashSet<BlockId>,
        backedge_sources: &mut HashSet<BlockId>,
    ) {
        if !visited.insert(block_id) {
            return;
        }
        on_stack.insert(block_id);

        if let Some(&idx) = block_index.get(&block_id) {
            let succs = blocks[idx].terminator.successors();
            for succ in succs {
                if on_stack.contains(&succ) {
                    // Back-edge: block_id → succ (succ is an ancestor)
                    backedge_sources.insert(block_id);
                } else if !visited.contains(&succ) {
                    dfs(
                        succ,
                        blocks,
                        block_index,
                        visited,
                        on_stack,
                        backedge_sources,
                    );
                }
            }
        }

        on_stack.remove(&block_id);
    }

    dfs(
        func.entry_block,
        &func.blocks,
        &block_index,
        &mut visited,
        &mut on_stack,
        &mut backedge_sources,
    );

    // Insert YieldCheck at each back-edge source block (before the terminator)
    for block in &mut func.blocks {
        if backedge_sources.contains(&block.id) {
            // Don't insert if the last instruction is already a YieldCheck or CheckReductions
            let already_has = block.instructions.last().is_some_and(|inst| {
                matches!(inst, Instruction::YieldCheck | Instruction::CheckReductions)
            });
            if !already_has {
                block.instructions.push(Instruction::YieldCheck);
            }
        }
    }
}
