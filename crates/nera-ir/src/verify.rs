//! Read-only structural verification of completed mutable-register IR.
//!
//! See `docs/IR_CONTRACT.md` for supported structure and limits. This is
//! deliberately separate from cross-module call resolution in `validate`.

use std::collections::{HashMap, HashSet};

use crate::ir::{BlockId, Instruction, IrFunction, IrModule, Terminator, VarId};

mod operands;
use operands::{binary_flags, instruction_operands, terminator_operands};

/// Location within a function. Instruction indices are zero-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    Function,
    Block(BlockId),
    Instruction(BlockId, usize),
    Terminator(BlockId),
}

/// A structural violation, in deterministic function/block/instruction order.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{function} at {location:?}: {kind}")]
pub struct VerificationError {
    pub function: String,
    pub location: Location,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ErrorKind {
    #[error("arity {arity} does not match {params} parameters")]
    ArityMismatch { arity: u32, params: usize },
    #[error("duplicate parameter {0:?}")]
    DuplicateParameter(VarId),
    #[error("duplicate block {0:?}")]
    DuplicateBlock(BlockId),
    #[error("entry block {0:?} does not exist")]
    MissingEntry(BlockId),
    #[error("successor block {0:?} does not exist")]
    MissingSuccessor(BlockId),
    #[error("block parameters are reserved and must be empty")]
    ReservedBlockParameters,
    #[error("jump arguments are reserved and must be empty")]
    ReservedJumpArguments,
    #[error("high-level Receive is reserved; lower it to blocks and calls")]
    ReservedReceive,
    #[error("map operands must be key/value pairs, got {0} operands")]
    OddMapOperandCount(usize),
    #[error("invalid binary flags: signedness {signedness}, endianness {endianness}")]
    InvalidBinFlags { signedness: u8, endianness: u8 },
    #[error("instruction writes result {0:?} more than once")]
    DuplicateResult(VarId),
    #[error("register {0:?} has no parameter or instruction definition")]
    UndeclaredVariable(VarId),
    #[error("register {0:?} is not initialized on every incoming path")]
    UninitializedVariable(VarId),
}

/// Verify each function independently, preserving module function order.
/// Does not resolve calls, exports, atom IDs or closure targets.
pub fn verify_module(module: &IrModule) -> Vec<VerificationError> {
    module.functions.iter().flat_map(verify_function).collect()
}

/// Verify without mutating the function or assuming SSA form.
pub fn verify_function(function: &IrFunction) -> Vec<VerificationError> {
    let mut errors = Vec::new();
    let mut report = |location, kind| {
        errors.push(VerificationError {
            function: function.mangled_name.clone(),
            location,
            kind,
        })
    };
    if function.arity as usize != function.params.len() {
        report(
            Location::Function,
            ErrorKind::ArityMismatch {
                arity: function.arity,
                params: function.params.len(),
            },
        );
    }
    let mut declared: HashSet<VarId> = HashSet::new();
    for &param in &function.params {
        if !declared.insert(param) {
            report(Location::Function, ErrorKind::DuplicateParameter(param));
        }
    }
    let mut indices = HashMap::new();
    let mut graph_valid = true;
    for (index, block) in function.blocks.iter().enumerate() {
        if indices.insert(block.id, index).is_some() {
            report(
                Location::Block(block.id),
                ErrorKind::DuplicateBlock(block.id),
            );
            graph_valid = false;
        }
    }
    if !indices.contains_key(&function.entry_block) {
        report(
            Location::Function,
            ErrorKind::MissingEntry(function.entry_block),
        );
        graph_valid = false;
    }

    // Cache reads/writes once; IDs are keys, never allocation sizes or indices.
    let operands: Vec<_> = function
        .blocks
        .iter()
        .map(|block| {
            block
                .instructions
                .iter()
                .map(instruction_operands)
                .collect::<Vec<_>>()
        })
        .collect();
    let term_uses: Vec<_> = function
        .blocks
        .iter()
        .map(|b| terminator_operands(&b.terminator))
        .collect();
    let mut generated = vec![HashSet::new(); function.blocks.len()];
    for (index, block) in function.blocks.iter().enumerate() {
        if !block.params.is_empty() {
            report(
                Location::Block(block.id),
                ErrorKind::ReservedBlockParameters,
            );
        }
        for (inst_index, inst) in block.instructions.iter().enumerate() {
            let location = Location::Instruction(block.id, inst_index);
            let (defs, _) = &operands[index][inst_index];
            let mut seen = HashSet::new();
            for &var in defs {
                if !seen.insert(var) {
                    report(location, ErrorKind::DuplicateResult(var));
                }
                declared.insert(var);
                generated[index].insert(var);
            }
            if let Instruction::MakeMap(_, pairs) = inst {
                if pairs.len() % 2 != 0 {
                    report(location, ErrorKind::OddMapOperandCount(pairs.len()));
                }
            }
            if let Some(flags) = binary_flags(inst) {
                if flags.signedness > 1 || flags.endianness > 2 {
                    report(
                        location,
                        ErrorKind::InvalidBinFlags {
                            signedness: flags.signedness,
                            endianness: flags.endianness,
                        },
                    );
                }
            }
        }
        let location = Location::Terminator(block.id);
        match &block.terminator {
            Terminator::Jump(_, args) if !args.is_empty() => {
                report(location, ErrorKind::ReservedJumpArguments);
            }
            Terminator::Receive { .. } => {
                report(location, ErrorKind::ReservedReceive);
                // Its binding semantics are not defined; skip definite assignment.
                graph_valid = false;
            }
            _ => {}
        }
        for successor in block.terminator.successors() {
            if !indices.contains_key(&successor) {
                report(location, ErrorKind::MissingSuccessor(successor));
                graph_valid = false;
            }
        }
    }

    let incoming =
        graph_valid.then(|| definite_assignment(function, &indices, &declared, &generated));
    for (index, block) in function.blocks.iter().enumerate() {
        let mut available = incoming.as_ref().and_then(|sets| sets[index].clone());
        let mut check_uses = |uses: &[VarId], location, available: &Option<HashSet<VarId>>| {
            let mut seen = HashSet::new();
            for &var in uses {
                if !seen.insert(var) {
                    continue;
                }
                if !declared.contains(&var) {
                    report(location, ErrorKind::UndeclaredVariable(var));
                } else if available.as_ref().is_some_and(|set| !set.contains(&var)) {
                    report(location, ErrorKind::UninitializedVariable(var));
                }
            }
        };
        for (inst_index, (defs, uses)) in operands[index].iter().enumerate() {
            check_uses(
                uses,
                Location::Instruction(block.id, inst_index),
                &available,
            );
            if let Some(set) = &mut available {
                set.extend(defs);
            }
        }
        check_uses(
            &term_uses[index],
            Location::Terminator(block.id),
            &available,
        );
    }
    errors.sort_by_key(|error| match error.location {
        Location::Function => (0, 0, 0),
        Location::Block(id) => (1, indices[&id], 0),
        Location::Instruction(id, index) => (1, indices[&id], index + 1),
        Location::Terminator(id) => (1, indices[&id], usize::MAX),
    });
    errors
}

/// Must-analysis over reachable explicit CFG edges. Start at top and intersect
/// predecessor outputs until stable; the synthetic entry edge supplies only
/// function parameters, even when a loop jumps back to the entry block.
fn definite_assignment(
    function: &IrFunction,
    indices: &HashMap<BlockId, usize>,
    universe: &HashSet<VarId>,
    generated: &[HashSet<VarId>],
) -> Vec<Option<HashSet<VarId>>> {
    let entry = indices[&function.entry_block];
    let mut reachable = vec![false; function.blocks.len()];
    let mut predecessors = vec![Vec::new(); function.blocks.len()];
    let mut pending = vec![entry];
    while let Some(index) = pending.pop() {
        if reachable[index] {
            continue;
        }
        reachable[index] = true;
        for successor in function.blocks[index].terminator.successors() {
            let next = indices[&successor];
            predecessors[next].push(index);
            pending.push(next);
        }
    }
    let params: HashSet<_> = function.params.iter().copied().collect();
    let mut incoming: Vec<_> = reachable
        .iter()
        .map(|&yes| yes.then(|| universe.clone()))
        .collect();
    let mut outgoing = incoming.clone();
    loop {
        let mut changed = false;
        for index in 0..function.blocks.len() {
            if !reachable[index] {
                continue;
            }
            let mut set = if index == entry {
                params.clone()
            } else {
                universe.clone()
            };
            for &pred in &predecessors[index] {
                set.retain(|var| outgoing[pred].as_ref().unwrap().contains(var));
            }
            let mut out = set.clone();
            out.extend(&generated[index]);
            if incoming[index].as_ref() != Some(&set) || outgoing[index].as_ref() != Some(&out) {
                incoming[index] = Some(set);
                outgoing[index] = Some(out);
                changed = true;
            }
        }
        if !changed {
            return incoming;
        }
    }
}

#[cfg(test)]
mod tests;
