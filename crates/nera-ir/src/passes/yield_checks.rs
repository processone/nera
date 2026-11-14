//! Scheduling checks on explicit CFG back-edges.

use crate::ir::{BasicBlock, BlockId, FailureDestination, Instruction, IrFunction};
use std::collections::{HashMap, HashSet};

/// Check reductions before the earliest backedge exit in each source block.
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
    let mut backedge_sources: HashMap<BlockId, usize> = HashMap::new();

    fn dfs(
        block_id: BlockId,
        blocks: &[BasicBlock],
        block_index: &HashMap<BlockId, usize>,
        visited: &mut HashSet<BlockId>,
        on_stack: &mut HashSet<BlockId>,
        backedge_sources: &mut HashMap<BlockId, usize>,
    ) {
        if !visited.insert(block_id) {
            return;
        }
        on_stack.insert(block_id);

        if let Some(&idx) = block_index.get(&block_id) {
            let block = &blocks[idx];
            // Keep the normal-edge DFS order, but retain the instruction
            // position at which each exceptional edge leaves the block.
            let mut exits: Vec<_> = block
                .terminator
                .successors()
                .into_iter()
                .map(|target| (target, block.instructions.len()))
                .collect();
            for (position, inst) in block.instructions.iter().enumerate() {
                if let Instruction::Arithmetic(site) = inst {
                    if let FailureDestination::Block(target) = site.failure {
                        exits.push((target, position));
                    }
                }
            }
            for (succ, position) in exits {
                if on_stack.contains(&succ) {
                    // Back-edge: block_id → succ (succ is an ancestor)
                    backedge_sources
                        .entry(block_id)
                        .and_modify(|earliest| *earliest = (*earliest).min(position))
                        .or_insert(position);
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

    // A landing-pad chain still carries a pending exception. Its backedge
    // cannot yield until ClearException has consumed that state. Defer only
    // accessor/empty jump prefixes; ordinary source blocks keep their checks.
    let mut checkpoints: HashMap<BlockId, usize> = HashMap::new();
    for (source, position) in backedge_sources {
        let block = &func.blocks[block_index[&source]];
        let consumed = if position == block.instructions.len()
            && block.instructions.iter().all(is_exception_accessor)
        {
            match &block.terminator {
                crate::ir::Terminator::Jump(next, args) if args.is_empty() => {
                    exception_consumption(*next, &func.blocks, &block_index)
                }
                _ => None,
            }
        } else {
            None
        };
        let (target, position) = consumed.unwrap_or((source, position));
        checkpoints
            .entry(target)
            .and_modify(|earliest| *earliest = (*earliest).min(position))
            .or_insert(position);
    }

    // A failing arithmetic instruction skips its suffix, so a final check
    // cannot cover its backedge. At a deferred consumption point a check may
    // already follow ClearException; retain it as well as preceding checks.
    for block in &mut func.blocks {
        if let Some(&position) = checkpoints.get(&block.id) {
            let is_check = |inst: &Instruction| {
                matches!(inst, Instruction::YieldCheck | Instruction::CheckReductions)
            };
            let already_has = block.instructions[..position].last().is_some_and(is_check)
                || block.instructions.get(position).is_some_and(is_check);
            if !already_has {
                block.instructions.insert(position, Instruction::YieldCheck);
            }
        }
    }
}

fn is_exception_accessor(inst: &Instruction) -> bool {
    matches!(
        inst,
        Instruction::GetExceptionClass(_)
            | Instruction::GetExceptionReason(_)
            | Instruction::GetExceptionStacktrace(_)
    )
}

/// Follow only the pending-exception prefixes accepted by the IR contract.
fn exception_consumption(
    mut target: BlockId,
    blocks: &[BasicBlock],
    block_index: &HashMap<BlockId, usize>,
) -> Option<(BlockId, usize)> {
    let mut visited = HashSet::new();
    while visited.insert(target) {
        let block = &blocks[*block_index.get(&target)?];
        for (position, inst) in block.instructions.iter().enumerate() {
            if matches!(inst, Instruction::ClearException) {
                return Some((target, position + 1));
            }
            if !is_exception_accessor(inst) {
                return None;
            }
        }
        match &block.terminator {
            crate::ir::Terminator::Jump(next, args) if args.is_empty() => target = *next,
            _ => return None,
        }
    }
    None
}
