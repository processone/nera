//! Liveness analysis for variables across suspend points.

use std::collections::{HashMap, HashSet};

use nera_ir::ir::{BlockId, CallTarget, IrFunction, Instruction, Terminator, VarId};

// ---------------------------------------------------------------------------
// Liveness analysis for variables across suspend points (receive & calls)
// ---------------------------------------------------------------------------

/// A suspend point is a location in the function where execution may need
/// to be suspended and later resumed. This includes receive_peek calls (the
/// process waits for a message) and local function calls (the callee may
/// transitively wait for a message).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct SuspendPoint {
    /// Block containing the suspend point
    pub(crate) block_id: BlockId,
    /// Instruction index within the block
    pub(crate) inst_idx: usize,
    /// The result VarId of the call instruction at this point
    pub(crate) result_var: VarId,
    /// Whether this is a receive_peek point (vs a local call)
    pub(crate) is_receive: bool,
}

/// For each suspend point in the function (receive points first, then local
/// call sites), compute the set of IR VarIds that are defined before the
/// point and used after it. These variables must be saved when the process
/// suspends and restored when it resumes.
///
/// Returns a Vec of (SuspendPoint, Vec<VarId>) pairs.
pub(super) fn compute_suspend_points(
    ir_func: &IrFunction,
    exclude_call_sites: &HashSet<(BlockId, usize)>,
) -> Vec<(SuspendPoint, Vec<VarId>)> {
    // Step 1: Find all suspend points — receive_peek calls first, then local calls,
    // then yield checks (back-edge instrumentation)
    let mut receive_points = Vec::new();
    let mut local_call_points = Vec::new();
    let mut yield_check_points = Vec::new();

    for b in &ir_func.blocks {
        for (idx, inst) in b.instructions.iter().enumerate() {
            match inst {
                Instruction::Call(result, target, _) => {
                    match target {
                        CallTarget::Bif(name) if name == "receive_peek" => {
                            receive_points.push(SuspendPoint {
                                block_id: b.id,
                                inst_idx: idx,
                                result_var: *result,
                                is_receive: true,
                            });
                        }
                        CallTarget::Local(_) | CallTarget::Remote(_, _) | CallTarget::Apply(_, _) | CallTarget::Closure(_) => {
                            if exclude_call_sites.contains(&(b.id, idx)) {
                                continue;
                            }
                            local_call_points.push(SuspendPoint {
                                block_id: b.id,
                                inst_idx: idx,
                                result_var: *result,
                                is_receive: false,
                            });
                        }
                        _ => {}
                    }
                }
                Instruction::YieldCheck => {
                    yield_check_points.push(SuspendPoint {
                        block_id: b.id,
                        inst_idx: idx,
                        result_var: VarId(u32::MAX), // sentinel — no result
                        is_receive: false,
                    });
                }
                _ => {}
            }
        }
    }

    // Combine: receives first, then local calls, then yield checks
    let mut all_points = Vec::new();
    all_points.extend(receive_points);
    all_points.extend(local_call_points);
    all_points.extend(yield_check_points);

    if all_points.is_empty() {
        return Vec::new();
    }

    // Step 2: Build CFG
    let block_by_id: HashMap<BlockId, &nera_ir::ir::BasicBlock> = ir_func.blocks
        .iter()
        .map(|b| (b.id, b))
        .collect();

    let mut successors: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
    for block in &ir_func.blocks {
        let succs = terminator_successors(&block.terminator);
        successors.insert(block.id, succs);
    }

    // Step 3: For each suspend point, compute live-across variables
    let param_set: HashSet<VarId> = ir_func.params.iter().copied().collect();

    let mut result = Vec::new();
    for sp in &all_points {
        // Collect all variables DEFINED before this point
        let mut defs_before: HashSet<VarId> = HashSet::new();

        let mut visited: HashSet<BlockId> = HashSet::new();
        let mut queue: Vec<BlockId> = vec![ir_func.entry_block];
        while let Some(bid) = queue.pop() {
            if !visited.insert(bid) {
                continue;
            }
            let block = match block_by_id.get(&bid) {
                Some(b) => b,
                None => continue,
            };
            if bid == sp.block_id {
                // Only collect defs from instructions before the suspend point
                for inst in &block.instructions[..sp.inst_idx] {
                    collect_instruction_defs(inst, &mut defs_before);
                }
                continue;
            }
            for inst in &block.instructions {
                collect_instruction_defs(inst, &mut defs_before);
            }
            if let Some(succs) = successors.get(&bid) {
                for succ in succs {
                    queue.push(*succ);
                }
            }
        }

        // Collect all variables USED after this point
        let mut uses_after: HashSet<VarId> = HashSet::new();

        let block = block_by_id[&sp.block_id];
        // For local calls, "after" means from the instruction AFTER the call
        // (inst_idx+1). For receives, it's from the receive_peek itself onward.
        let after_start = if sp.is_receive { sp.inst_idx } else { sp.inst_idx + 1 };
        for inst in &block.instructions[after_start..] {
            collect_instruction_uses(inst, &mut uses_after);
        }
        collect_terminator_uses(&block.terminator, &mut uses_after);

        // BFS from successors
        let mut visited_after: HashSet<BlockId> = HashSet::new();
        visited_after.insert(sp.block_id);
        let mut after_queue: Vec<BlockId> = Vec::new();
        if let Some(succs) = successors.get(&sp.block_id) {
            for succ in succs {
                after_queue.push(*succ);
            }
        }
        while let Some(bid) = after_queue.pop() {
            if !visited_after.insert(bid) {
                continue;
            }
            let block = match block_by_id.get(&bid) {
                Some(b) => b,
                None => continue,
            };
            for inst in &block.instructions {
                collect_instruction_uses(inst, &mut uses_after);
            }
            collect_terminator_uses(&block.terminator, &mut uses_after);
            if let Some(succs) = successors.get(&bid) {
                for succ in succs {
                    after_queue.push(*succ);
                }
            }
        }

        // Live-across = defined before AND used after, excluding function params
        let mut live_across: Vec<VarId> = defs_before
            .intersection(&uses_after)
            .copied()
            .filter(|v| !param_set.contains(v))
            .collect();
        live_across.sort_by_key(|v| v.0);

        result.push((sp.clone(), live_across));
    }

    result
}

/// Collect VarIds defined (written) by an instruction.
pub(super) fn collect_instruction_defs(inst: &Instruction, defs: &mut HashSet<VarId>) {
    match inst {
        Instruction::Const(v, _)
        | Instruction::BinOp(v, _, _, _)
        | Instruction::UnaryOp(v, _, _)
        | Instruction::MakeTuple(v, _)
        | Instruction::MakeCons(v, _, _)
        | Instruction::TupleElement(v, _, _)
        | Instruction::ListHead(v, _)
        | Instruction::ListTail(v, _)
        | Instruction::Call(v, _, _)
        | Instruction::GetSelf(v)
        | Instruction::Spawn(v, _, _, _)
        | Instruction::Send(v, _, _)
        | Instruction::SendDirect(v, _, _)
        | Instruction::TypeTest(v, _, _)
        | Instruction::Move(v, _)
        | Instruction::TupleSize(v, _)
        | Instruction::MakeBinary(v, _)
        | Instruction::BinaryGetInteger(v, _, _, _, _)
        | Instruction::BinaryGetFloat(v, _, _, _, _)
        | Instruction::BinaryGetBinary(v, _, _, _)
        | Instruction::BsMatchBitOffset(v, _)
        | Instruction::BinarySize(v, _)
        | Instruction::BsGetIntegerInline(v, _, _, _, _)
        | Instruction::BsMaterialize(v, _, _, _)
        | Instruction::MakeMap(v, _)
        | Instruction::MapGet(v, _, _)
        | Instruction::MapPut(v, _, _, _)
        | Instruction::MapUpdateExact(v, _, _, _)
        | Instruction::RaiseException(v, _, _)
        | Instruction::GetExceptionClass(v)
        | Instruction::GetExceptionReason(v)
        | Instruction::GetExceptionStacktrace(v)
        | Instruction::CheckException(v) => {
            defs.insert(*v);
        }
        Instruction::MakeClosure(v, _, _, _) => {
            defs.insert(*v);
        }
        Instruction::MakeRemoteFunRef(v, _, _, _) => {
            defs.insert(*v);
        }
        Instruction::MakeDynamicRemoteFunRef(v, _, _, _) => {
            defs.insert(*v);
        }
        Instruction::BsStartMatch(base_ptr, total_bytes, _) => {
            defs.insert(*base_ptr);
            defs.insert(*total_bytes);
        }
        Instruction::BsMatchStart2(match_ctx, base_ptr, total_bits, _) => {
            defs.insert(*match_ctx);
            defs.insert(*base_ptr);
            defs.insert(*total_bits);
        }
        Instruction::BsMatchCtxAdvance(_, _) => {
            // Side effect only (in-place mutation), no defs
        }
        Instruction::BinaryPutInteger(_, _, _, _, _) => {}
        Instruction::BinaryPutFloat(_, _, _, _, _) => {}
        Instruction::BinaryPutBinary(_, _, _, _) => {}
        Instruction::BinaryPutUtf8(bits_written, _, _, _) => {
            defs.insert(*bits_written);
        }
        Instruction::BinaryPutUtf16(bits_written, _, _, _, _) => {
            defs.insert(*bits_written);
        }
        Instruction::BinaryPutUtf32(bits_written, _, _, _, _) => {
            defs.insert(*bits_written);
        }
        Instruction::BsGetUtf8(cp, bits_consumed, _, _, _) => {
            defs.insert(*cp);
            defs.insert(*bits_consumed);
        }
        Instruction::BsGetUtf16(cp, bits_consumed, _, _, _, _) => {
            defs.insert(*cp);
            defs.insert(*bits_consumed);
        }
        Instruction::BsGetUtf32(cp, bits_consumed, _, _, _, _) => {
            defs.insert(*cp);
            defs.insert(*bits_consumed);
        }
        Instruction::BinaryResize(_, _) => {}
        Instruction::CheckReductions => {}
        Instruction::YieldCheck => {}
        Instruction::SetLine(_) => {}
        Instruction::ClearException => {}
    }
}

/// Collect VarIds used (read) by an instruction.
pub(super) fn collect_instruction_uses(inst: &Instruction, uses: &mut HashSet<VarId>) {
    match inst {
        Instruction::Const(_, _) => {}
        Instruction::BinOp(_, _, a, b) => { uses.insert(*a); uses.insert(*b); }
        Instruction::UnaryOp(_, _, a) => { uses.insert(*a); }
        Instruction::MakeTuple(_, args) => { for a in args { uses.insert(*a); } }
        Instruction::MakeCons(_, h, t) => { uses.insert(*h); uses.insert(*t); }
        Instruction::TupleElement(_, t, _) => { uses.insert(*t); }
        Instruction::ListHead(_, l) => { uses.insert(*l); }
        Instruction::ListTail(_, l) => { uses.insert(*l); }
        Instruction::Call(_, target, args) => {
            for a in args { uses.insert(*a); }
            // Also track VarIds embedded in the CallTarget itself
            match target {
                CallTarget::Closure(var_id) => { uses.insert(*var_id); }
                CallTarget::Apply(mod_var, fun_var) => { uses.insert(*mod_var); uses.insert(*fun_var); }
                _ => {}
            }
        }
        Instruction::GetSelf(_) => {}
        Instruction::Spawn(_, _, _, args) => { for a in args { uses.insert(*a); } }
        Instruction::Send(_, pid, msg) => { uses.insert(*pid); uses.insert(*msg); }
        Instruction::SendDirect(_, pid, elems) => {
            uses.insert(*pid);
            for e in elems { uses.insert(*e); }
        }
        Instruction::TypeTest(_, _, val) => { uses.insert(*val); }
        Instruction::Move(_, src) => { uses.insert(*src); }
        Instruction::TupleSize(_, t) => { uses.insert(*t); }
        Instruction::MakeBinary(_, size) => { uses.insert(*size); }
        Instruction::BinaryPutInteger(bin, off, val, size, _) => {
            uses.insert(*bin); uses.insert(*off); uses.insert(*val); uses.insert(*size);
        }
        Instruction::BinaryPutFloat(bin, off, val, size, _) => {
            uses.insert(*bin); uses.insert(*off); uses.insert(*val); uses.insert(*size);
        }
        Instruction::BinaryPutBinary(dest, off, src, count) => {
            uses.insert(*dest); uses.insert(*off); uses.insert(*src); uses.insert(*count);
        }
        Instruction::BinaryGetInteger(_, bin, off, size, _) => {
            uses.insert(*bin); uses.insert(*off); uses.insert(*size);
        }
        Instruction::BinaryGetFloat(_, bin, off, size, _) => {
            uses.insert(*bin); uses.insert(*off); uses.insert(*size);
        }
        Instruction::BinaryGetBinary(_, bin, off, size) => {
            uses.insert(*bin); uses.insert(*off); uses.insert(*size);
        }
        Instruction::BsMatchBitOffset(_, bin) | Instruction::BinarySize(_, bin) => { uses.insert(*bin); }
        Instruction::BsStartMatch(_, _, bin) => { uses.insert(*bin); }
        Instruction::BsMatchStart2(_, _, _, input) => { uses.insert(*input); }
        Instruction::BsMatchCtxAdvance(match_ctx, bit_offset) => {
            uses.insert(*match_ctx);
            uses.insert(*bit_offset);
        }
        Instruction::BsGetIntegerInline(_, base, off, count, _) => {
            uses.insert(*base); uses.insert(*off); uses.insert(*count);
        }
        Instruction::BsMaterialize(_, orig, off, count) => {
            uses.insert(*orig); uses.insert(*off); uses.insert(*count);
        }
        Instruction::MakeMap(_, pairs) => { for p in pairs { uses.insert(*p); } }
        Instruction::MapGet(_, map, key) => { uses.insert(*map); uses.insert(*key); }
        Instruction::MapPut(_, map, key, val) => { uses.insert(*map); uses.insert(*key); uses.insert(*val); }
        Instruction::MapUpdateExact(_, map, key, val) => { uses.insert(*map); uses.insert(*key); uses.insert(*val); }
        Instruction::CheckReductions => {}
        Instruction::YieldCheck => {}
        Instruction::RaiseException(_, _, reason) => { uses.insert(*reason); }
        Instruction::GetExceptionClass(_) => {}
        Instruction::GetExceptionReason(_) => {}
        Instruction::GetExceptionStacktrace(_) => {}
        Instruction::ClearException => {}
        Instruction::CheckException(_) => {}
        Instruction::MakeClosure(_, _, _, env_vars) => {
            for ev in env_vars { uses.insert(*ev); }
        }
        Instruction::MakeRemoteFunRef(_, _, _, _) => {}
        Instruction::MakeDynamicRemoteFunRef(_, m, f, a) => {
            uses.insert(*m); uses.insert(*f); uses.insert(*a);
        }
        Instruction::BinaryPutUtf8(_, bin, off, cp) => {
            uses.insert(*bin); uses.insert(*off); uses.insert(*cp);
        }
        Instruction::BinaryPutUtf16(_, bin, off, cp, _) => {
            uses.insert(*bin); uses.insert(*off); uses.insert(*cp);
        }
        Instruction::BinaryPutUtf32(_, bin, off, cp, _) => {
            uses.insert(*bin); uses.insert(*off); uses.insert(*cp);
        }
        Instruction::BsGetUtf8(_, _, base, off, total) => {
            uses.insert(*base); uses.insert(*off); uses.insert(*total);
        }
        Instruction::BsGetUtf16(_, _, base, off, total, _) => {
            uses.insert(*base); uses.insert(*off); uses.insert(*total);
        }
        Instruction::BsGetUtf32(_, _, base, off, total, _) => {
            uses.insert(*base); uses.insert(*off); uses.insert(*total);
        }
        Instruction::BinaryResize(bin, size) => {
            uses.insert(*bin); uses.insert(*size);
        }
        Instruction::SetLine(_) => {}
    }
}

/// Collect VarIds used (read) by a terminator.
pub(super) fn collect_terminator_uses(term: &Terminator, uses: &mut HashSet<VarId>) {
    match term {
        Terminator::Return(v) => { uses.insert(*v); }
        Terminator::Jump(_, args) => { for a in args { uses.insert(*a); } }
        Terminator::Branch(v, _, _) => { uses.insert(*v); }
        Terminator::Switch { scrutinee, .. } => { uses.insert(*scrutinee); }
        Terminator::TailCall(target, args) => {
            for a in args { uses.insert(*a); }
            match target {
                CallTarget::Closure(var_id) => { uses.insert(*var_id); }
                CallTarget::Apply(mod_var, fun_var) => { uses.insert(*mod_var); uses.insert(*fun_var); }
                _ => {}
            }
        }
        Terminator::Receive { clauses, timeout, .. } => {
            for c in clauses { uses.insert(c.binding); }
            if let Some((t, _)) = timeout { uses.insert(*t); }
        }
        Terminator::Unreachable => {}
    }
}

/// Get successor BlockIds from a terminator.
/// Delegates to `Terminator::successors()` in the IR crate.
pub(super) fn terminator_successors(term: &Terminator) -> Vec<BlockId> {
    term.successors()
}
