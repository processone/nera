//! Translates IR functions into Cranelift IR.
//!
//! Each Erlang function becomes an `extern "C"` function taking a
//! ProcessContext pointer and ErlTerm arguments, returning the result
//! as a u64.

mod atoms;
mod calls;
mod constants;
mod ffi_emit;
mod instructions;
mod liveness;
mod resume;
mod signatures;
mod terminators;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use cranelift_codegen::ir::{
    types, AbiParam, Function, InstBuilder, MemFlags, Signature, Value,
};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataId, FuncId, Linkage, Module};
use cranelift_object::ObjectModule;

use nera_ir::ir::{
    BlockId, CallTarget, IrFunction, IrModule, Instruction, Terminator, VarId,
};
use crate::term_layout::*;

pub use signatures::build_erlang_signature;
use signatures::{
    declare_runtime_func, declare_runtime_func_void,
    declare_tuple_element,
};
use liveness::{compute_suspend_points, SuspendPoint};
#[cfg(test)]
use liveness::{collect_instruction_defs, collect_instruction_uses, collect_terminator_uses, terminator_successors};
#[cfg(test)]
use std::collections::HashSet;

/// Error during function translation.
#[derive(Debug)]
pub enum TranslateError {
    Module(String),
    Codegen {
        block_id: Option<BlockId>,
        reason: String,
    },
}

/// Compiled module context, tracking function IDs across functions.
pub struct ModuleContext {
    /// Map from mangled function name to Cranelift FuncId
    pub func_ids: HashMap<String, FuncId>,
    /// Map from (module_name_atom_id, func_name_atom_id, arity) -> mangled name
    pub module_funcs: HashMap<(u32, u32), String>,
    /// Module name atom id
    pub module_name_id: u32,
    /// Cross-module functions for direct linking: (module_atom_id, func_atom_id, arity) -> FuncId
    pub cross_module_funcs: HashMap<(u32, u32, u32), FuncId>,
    /// Map from (module_atom_id, func_atom_id, arity) -> func_index for compact stack frames
    pub func_index_map: HashMap<(u32, u32, u32), u32>,
}

impl ModuleContext {
    /// Build context for an IR module, declaring all functions upfront.
    pub fn new(
        module: &mut ObjectModule,
        ir_module: &IrModule,
    ) -> Result<Self, TranslateError> {
        let mut func_ids = HashMap::new();
        let mut module_funcs = HashMap::new();

        // Declare all module functions
        for func in &ir_module.functions {
            let sig = build_erlang_signature(func.arity);
            let func_id = module
                .declare_function(&func.mangled_name, Linkage::Export, &sig)
                .map_err(|e| TranslateError::Module(e.to_string()))?;
            func_ids.insert(func.mangled_name.clone(), func_id);
            module_funcs.insert((func.name.0, func.arity), func.mangled_name.clone());
        }

        Ok(ModuleContext {
            func_ids,
            module_funcs,
            module_name_id: ir_module.name.0,
            cross_module_funcs: HashMap::new(),
            func_index_map: HashMap::new(),
        })
    }
}

pub struct FuncTranslator {
    /// Cache of declared runtime function IDs
    pub(super) runtime_funcs: HashMap<String, FuncId>,
    /// Atom resolution: AtomId(u32) -> String
    pub(super) atom_table: HashMap<u32, String>,
    /// C calling convention for the target platform (SystemV on Unix, WindowsFastCall on Windows)
    pub(super) c_call_conv: CallConv,
    /// DataId for the global atom lookup table (__nera_atom_table)
    pub(super) atom_table_data_id: Option<DataId>,
    /// Parser atom ID for `true` (looked up from atom_table)
    pub(super) true_atom_id: Option<u32>,
    /// Parser atom ID for `false` (looked up from atom_table)
    pub(super) false_atom_id: Option<u32>,
    /// Per-function: wrapper FuncId for the current function (used by set_resume).
    /// Set at the start of translate_function, cleared at the end.
    pub(super) current_wrapper_func_id: Option<FuncId>,
    /// Per-function: parameter VarIds for the current function.
    pub(super) current_param_vars: Vec<VarId>,
    /// Per-function: the 1-based index of the current suspend point being
    /// translated. This covers both receive points (first) and call-site
    /// resume points (after). Used by emit_set_resume.
    pub(super) current_receive_index: u32,
    /// Per-function: for each suspend point (indexed by 0-based index),
    /// the list of VarIds that are live across the point and must be
    /// saved/restored by the resume mechanism. Receive points come first,
    /// then call-site points.
    pub(super) current_live_across_vars: Vec<Vec<VarId>>,
    /// Per-function: suspend point info (from compute_suspend_points).
    /// Used during instruction translation to identify call-site resume points.
    pub(super) current_suspend_points: Vec<(SuspendPoint, Vec<VarId>)>,
    /// Per-function: maps (block_id, inst_idx) to the 0-based suspend point
    /// index, for quick lookup during instruction translation.
    pub(super) current_suspend_map: HashMap<(BlockId, usize), usize>,
    /// Per-function: call-site resume blocks. For each call-site suspend point
    /// (indexed by call-site 0-based index), this is the Cranelift block that
    /// loads saved vars, reads ctx.result, and continues execution.
    pub(super) current_call_resume_blocks: Vec<cranelift_codegen::ir::Block>,
    /// Per-function: call-site continuation blocks. For each call-site suspend
    /// point, this is the block where execution continues after the call (in
    /// both normal flow and resume flow).
    pub(super) current_call_cont_blocks: Vec<cranelift_codegen::ir::Block>,
    /// Per-function: the VarId that receives the result of each local call,
    /// indexed by call-site 0-based index.
    pub(super) current_call_result_vars: Vec<VarId>,
    /// Per-function: the current IR block being translated.
    pub(super) current_ir_block_id: Option<BlockId>,
    /// Per-function: the instruction index within the current block.
    pub(super) current_inst_idx: usize,
    /// Per-function: set of block IDs that contain CheckException.
    /// BinOp/UnaryOp FFI calls in these blocks should not emit exception
    /// checks because try/catch will handle them.
    pub(super) current_try_protected_values: std::collections::HashSet<VarId>,
    /// Per-function: Cranelift Variable that holds the reduction counter.
    /// Kept in a CPU register across self-tail-call loop iterations;
    /// synced to/from ctx.reductions at function boundaries (calls, returns, yields).
    pub(super) reds_var: Option<Variable>,
    /// Raw predicate from the last instruction only; consumed by its terminator.
    /// Never reused across an intervening instruction, block, or function.
    pub(super) last_branch_predicate: Option<(VarId, cranelift_codegen::ir::Value)>,
}

impl FuncTranslator {
    pub fn new() -> Self {
        Self {
            runtime_funcs: HashMap::new(),
            atom_table: HashMap::new(),
            c_call_conv: CallConv::SystemV,
            atom_table_data_id: None,
            true_atom_id: None,
            false_atom_id: None,
            current_wrapper_func_id: None,
            current_param_vars: Vec::new(),
            current_receive_index: 0,
            current_live_across_vars: Vec::new(),
            current_suspend_points: Vec::new(),
            current_suspend_map: HashMap::new(),
            current_call_resume_blocks: Vec::new(),
            current_call_cont_blocks: Vec::new(),
            current_call_result_vars: Vec::new(),
            current_ir_block_id: None,
            current_inst_idx: 0,
            current_try_protected_values: std::collections::HashSet::new(),
            reds_var: None,
            last_branch_predicate: None,
        }
    }

    /// Store reds_var to ctx.reductions (sync register → memory).
    /// Called before calls to other Erlang functions, returns, and yields.
    pub(super) fn sync_reds_to_ctx(
        &self,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
    ) {
        if let Some(rv) = self.reds_var {
            let reds = builder.use_var(rv);
            let ctx_val = builder.use_var(ctx_var);
            builder.ins().store(MemFlags::trusted(), reds, ctx_val, CTX_OFFSET_REDUCTIONS);
        }
    }

    /// Load ctx.reductions into reds_var (sync memory → register).
    /// Called after calls return and at function/resume entry.
    pub(super) fn reload_reds_from_ctx(
        &self,
        builder: &mut FunctionBuilder,
        ctx_var: Variable,
    ) {
        if let Some(rv) = self.reds_var {
            let ctx_val = builder.use_var(ctx_var);
            let reds = builder.ins().load(types::I64, MemFlags::trusted(), ctx_val, CTX_OFFSET_REDUCTIONS);
            builder.def_var(rv, reds);
        }
    }


    /// Set the C calling convention for the target platform.
    pub fn set_c_call_conv(&mut self, call_conv: CallConv) {
        self.c_call_conv = call_conv;
    }

    /// Set the data ID for the global atom lookup table.
    pub fn set_atom_table_data_id(&mut self, data_id: DataId) {
        self.atom_table_data_id = Some(data_id);
    }

    /// Set the atom table for resolving AtomId values to strings.
    /// Also looks up the parser atom IDs for `true` and `false`.
    pub fn set_atom_table(&mut self, table: HashMap<u32, String>) {
        // Find parser atom IDs for true and false
        for (&id, name) in &table {
            if name == "true" {
                self.true_atom_id = Some(id);
            } else if name == "false" {
                self.false_atom_id = Some(id);
            }
        }
        self.atom_table = table;
    }

    /// Translate an IR function into Cranelift IR and define it in the module.
    ///
    /// `wrapper_func_id` is the FuncId of the ErlFn-ABI wrapper for this
    /// function. It's used to emit `set_resume` calls before receive-wait
    /// returns so the scheduler can re-call the correct function.
    pub fn translate_function(
        &mut self,
        module: &mut ObjectModule,
        ir_func: &IrFunction,
        mod_ctx: &ModuleContext,
        wrapper_func_id: Option<FuncId>,
    ) -> Result<(), TranslateError> {
        // Store per-function context for use by translate_instruction.
        self.current_wrapper_func_id = wrapper_func_id;
        self.current_param_vars = ir_func.params.clone();

        // Compute all suspend points (receives first, then local calls) and
        // the variables live across each.
        let no_exclusions = std::collections::HashSet::new();
        let suspend_info = compute_suspend_points(ir_func, &no_exclusions);
        self.current_suspend_points = suspend_info.clone();

        // Build quick lookup: (block_id, inst_idx) -> suspend point index
        self.current_suspend_map = suspend_info.iter().enumerate()
            .map(|(i, (sp, _))| ((sp.block_id, sp.inst_idx), i))
            .collect();

        // Separate the live-across vars into the flat vec
        self.current_live_across_vars = suspend_info.iter().map(|(_, vars)| vars.clone()).collect();

        let func_id = mod_ctx.func_ids[&ir_func.mangled_name];

        let sig = build_erlang_signature(ir_func.arity);
        let mut func = Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );

        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_ctx);

        // Create Cranelift blocks for each IR block
        let mut block_map: HashMap<BlockId, cranelift_codegen::ir::Block> = HashMap::new();
        for ir_block in &ir_func.blocks {
            let cl_block = builder.create_block();
            block_map.insert(ir_block.id, cl_block);
        }

        // Variable map: IR VarId -> Cranelift Variable
        let mut var_map: HashMap<VarId, Variable> = HashMap::new();

        let declare_var = |var_id: VarId,
                               var_map: &mut HashMap<VarId, Variable>,
                               builder: &mut FunctionBuilder| {
            var_map.entry(var_id).or_insert_with(|| builder.declare_var(types::I64));
        };

        // Pre-declare all variables used in the function
        for ir_block in &ir_func.blocks {
            for inst in &ir_block.instructions {
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
                        declare_var(*v, &mut var_map, &mut builder);
                    }
                    Instruction::MakeClosure(v, _, _, ref env_vars) => {
                        declare_var(*v, &mut var_map, &mut builder);
                        for ev in env_vars {
                            declare_var(*ev, &mut var_map, &mut builder);
                        }
                    }
                    Instruction::MakeRemoteFunRef(v, _, _, _) => {
                        declare_var(*v, &mut var_map, &mut builder);
                    }
                    Instruction::MakeDynamicRemoteFunRef(v, _, _, _) => {
                        declare_var(*v, &mut var_map, &mut builder);
                    }
                    Instruction::BsStartMatch(base_ptr, total_bytes, _) => {
                        declare_var(*base_ptr, &mut var_map, &mut builder);
                        declare_var(*total_bytes, &mut var_map, &mut builder);
                    }
                    Instruction::BsMatchStart2(match_ctx, base_ptr, total_bits, _) => {
                        declare_var(*match_ctx, &mut var_map, &mut builder);
                        declare_var(*base_ptr, &mut var_map, &mut builder);
                        declare_var(*total_bits, &mut var_map, &mut builder);
                    }
                    Instruction::BsMatchCtxAdvance(_, _) => {
                        // No result variables to declare — side effect only
                    }
                    Instruction::BinaryPutInteger(_, _, _, _, _) => {}
                    Instruction::BinaryPutFloat(_, _, _, _, _) => {}
                    Instruction::BinaryPutBinary(_, _, _, _) => {}
                    Instruction::BinaryPutUtf8(bits_written, _, _, _) => {
                        declare_var(*bits_written, &mut var_map, &mut builder);
                    }
                    Instruction::BinaryPutUtf16(bits_written, _, _, _, _) => {
                        declare_var(*bits_written, &mut var_map, &mut builder);
                    }
                    Instruction::BinaryPutUtf32(bits_written, _, _, _, _) => {
                        declare_var(*bits_written, &mut var_map, &mut builder);
                    }
                    Instruction::BsGetUtf8(cp, bits_consumed, _, _, _) => {
                        declare_var(*cp, &mut var_map, &mut builder);
                        declare_var(*bits_consumed, &mut var_map, &mut builder);
                    }
                    Instruction::BsGetUtf16(cp, bits_consumed, _, _, _, _) => {
                        declare_var(*cp, &mut var_map, &mut builder);
                        declare_var(*bits_consumed, &mut var_map, &mut builder);
                    }
                    Instruction::BsGetUtf32(cp, bits_consumed, _, _, _, _) => {
                        declare_var(*cp, &mut var_map, &mut builder);
                        declare_var(*bits_consumed, &mut var_map, &mut builder);
                    }
                    Instruction::BinaryResize(_, _) => {}
                    Instruction::CheckReductions => {}
                    Instruction::YieldCheck => {}
                    Instruction::SetLine(_) => {}
                    Instruction::ClearException => {}
                }
            }
        }

        // Also declare variables referenced in terminators (e.g., Return, Branch, TailCall)
        for ir_block in &ir_func.blocks {
            match &ir_block.terminator {
                Terminator::Return(v) => {
                    declare_var(*v, &mut var_map, &mut builder);
                }
                Terminator::Branch(v, _, _) => {
                    declare_var(*v, &mut var_map, &mut builder);
                }
                Terminator::Switch { scrutinee, .. } => {
                    declare_var(*scrutinee, &mut var_map, &mut builder);
                }
                Terminator::TailCall(_, args) => {
                    for a in args {
                        declare_var(*a, &mut var_map, &mut builder);
                    }
                }
                Terminator::Jump(_, args) => {
                    for a in args {
                        declare_var(*a, &mut var_map, &mut builder);
                    }
                }
                Terminator::Receive { clauses, timeout, .. } => {
                    for clause in clauses {
                        declare_var(clause.binding, &mut var_map, &mut builder);
                    }
                    if let Some((t, _)) = timeout {
                        declare_var(*t, &mut var_map, &mut builder);
                    }
                }
                Terminator::Unreachable => {}
            }
        }

        // Also declare parameter variables
        for param in &ir_func.params {
            declare_var(*param, &mut var_map, &mut builder);
        }

        // Entry block: define parameters
        let entry_cl_block = block_map[&ir_func.entry_block];
        builder.append_block_params_for_function_params(entry_cl_block);
        builder.switch_to_block(entry_cl_block);

        // Bind parameters
        let block_params: Vec<Value> = builder.block_params(entry_cl_block).to_vec();

        // param[0] is the context pointer, rest are Erlang args
        let ctx_var = builder.declare_var(types::I64);
        if !block_params.is_empty() {
            builder.def_var(ctx_var, block_params[0]);
        }

        // Declare reds_var: holds the reduction counter in a CPU register.
        let reds_var = builder.declare_var(types::I64);
        self.reds_var = Some(reds_var);

        for (i, param) in ir_func.params.iter().enumerate() {
            if let Some(var) = var_map.get(param) {
                if i + 1 < block_params.len() {
                    builder.def_var(*var, block_params[i + 1]);
                }
            }
        }

        // Push the current function on the process call stack.
        let func_index = mod_ctx.func_index_map
            .get(&(mod_ctx.module_name_id, ir_func.name.0, ir_func.arity))
            .copied()
            .unwrap_or(0);
        self.emit_stack_push(
            module,
            &mut builder,
            ctx_var,
            func_index,
        )?;

        // Find ALL blocks containing receive_peek calls, with their instruction
        // indices. Each receive point gets a unique 1-based index so the resume
        // mechanism can dispatch to the correct one.
        let receive_infos: Vec<(BlockId, usize)> = ir_func.blocks.iter().filter_map(|b| {
            for (idx, inst) in b.instructions.iter().enumerate() {
                if let Instruction::Call(_, CallTarget::Bif(name), _) = inst {
                    if name == "receive_peek" {
                        return Some((b.id, idx));
                    }
                }
            }
            None
        }).collect();

        // Find call-site suspend points (local non-tail calls).
        // These get resume indices AFTER the receive points.
        // Include both local calls and remote calls — remote calls may
        // go through nera_rt_apply and transitively hit a receive.
        // 4th field: is_try_protected — the lexical try body recorded by lowering.
        // A future guard or try expression must not protect earlier calls.
        // Try-protected calls should NOT propagate exceptions because the
        // try/catch handler will check and handle them via CheckException.
        // Pure local calls are kept in call_site_infos for correct block splitting,
        // but they get a simplified status check (exception-only, no save/resume).
        let call_site_infos: Vec<(BlockId, usize, VarId, bool)> = ir_func.blocks.iter().flat_map(|b| {
            b.instructions.iter().enumerate().filter_map(move |(idx, inst)| {
                if let Instruction::Call(result, target, _) = inst {
                    match target {
                        CallTarget::Local(_) | CallTarget::Remote(_, _) | CallTarget::Closure(_) | CallTarget::Apply(_, _) => {
                            let is_try_protected = ir_func.protected_values.contains(result);
                            Some((b.id, idx, *result, is_try_protected))
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            })
        }).collect();

        // Find yield check points (back-edge instrumentation).
        // These get resume indices AFTER receive + call-site points.
        let yield_check_infos: Vec<(BlockId, usize)> = ir_func.blocks.iter().flat_map(|b| {
            b.instructions.iter().enumerate().filter_map(move |(idx, inst)| {
                if matches!(inst, Instruction::YieldCheck) {
                    Some((b.id, idx))
                } else {
                    None
                }
            })
        }).collect();

        let num_receive_points = receive_infos.len();
        let num_call_sites = call_site_infos.len();
        let num_yield_checks = yield_check_infos.len();
        let total_resume_points = num_receive_points + num_call_sites + num_yield_checks;

        // Create a separate Cranelift entry block for each receive point.
        // When resuming, the function jumps directly to the right one.
        // Normal (non-resume) flow also jumps here from the pre-receive code.
        let recv_entry_blocks: Vec<cranelift_codegen::ir::Block> = receive_infos
            .iter()
            .map(|_| builder.create_block())
            .collect();

        // Create resume preamble blocks for ALL suspend points (receives + calls).
        // These are entered ONLY on resume and load any saved variables.
        let all_resume_blocks: Vec<cranelift_codegen::ir::Block> = (0..total_resume_points)
            .map(|_| builder.create_block())
            .collect();

        // For call-site suspend points, create continuation blocks.
        // These blocks are where execution continues after the call in both
        // normal flow and resume flow.
        let call_cont_blocks: Vec<cranelift_codegen::ir::Block> = call_site_infos
            .iter()
            .map(|_| builder.create_block())
            .collect();

        // Store call-site resume info for use during instruction translation.
        self.current_call_resume_blocks = call_cont_blocks.clone();
        self.current_call_cont_blocks = call_cont_blocks.clone();
        self.current_call_result_vars = call_site_infos.iter().map(|(_, _, v, _)| *v).collect();

        // Map from call-site index to is_try_protected flag
        let call_site_try_protected: Vec<bool> = call_site_infos.iter().map(|(_, _, _, tp)| *tp).collect();

        // Lexically protected operations defer exception handling to their try.
        self.last_branch_predicate = None;
        self.current_try_protected_values = ir_func.protected_values.iter().copied().collect();

        // Map from IR BlockId to (receive_index_0based, instruction_index)
        // for quick lookup during block translation.
        let recv_block_map: HashMap<BlockId, (usize, usize)> = receive_infos
            .iter()
            .enumerate()
            .map(|(i, (bid, idx))| (*bid, (i, *idx)))
            .collect();

        // Map from (block_id, inst_idx) to call-site 0-based index
        let call_site_map: HashMap<(BlockId, usize), usize> = call_site_infos
            .iter()
            .enumerate()
            .map(|(i, (bid, idx, _, _))| ((*bid, *idx), i))
            .collect();

        // For yield-check suspend points, create continuation blocks.
        let yield_check_cont_blocks: Vec<cranelift_codegen::ir::Block> = yield_check_infos
            .iter()
            .map(|_| builder.create_block())
            .collect();

        // Map from (block_id, inst_idx) to yield-check 0-based index
        let yield_check_map: HashMap<(BlockId, usize), usize> = yield_check_infos
            .iter()
            .enumerate()
            .map(|(i, (bid, idx))| ((*bid, *idx), i))
            .collect();

        // Create a loop block for self-tail-call optimization.
        // The entry block sets up params then jumps to loop_block.
        // Self-tail-calls reassign params and jump back to loop_block.
        let loop_block = builder.create_block();

        // Load ctx.reductions into reds_var ONCE at function entry.
        // This must happen in entry_cl_block (NOT loop_block) so that
        // self-tail-call jumps back to loop_block don't re-load the
        // reduction counter from memory, which would reset it to 4000
        // on every iteration and prevent yielding.
        // Resume blocks have their own reload_reds_from_ctx.
        self.reload_reds_from_ctx(&mut builder, ctx_var);

        if total_resume_points > 0 {
            // This function has suspend points. At entry, check ctx.resume_receive:
            // 0 = no resume (go to loop_block), N = resume to Nth suspend point.
            // The flag is only cleared in the cold resume path (not on every entry)
            // to save 2 instructions on the hot (non-resume) path.
            let ctx_val = builder.use_var(ctx_var);
            let resume_flag = builder.ins().load(
                types::I32,
                MemFlags::trusted(),
                ctx_val,
                CTX_OFFSET_RESUME_RECEIVE,
            );
            let zero = builder.ins().iconst(types::I32, 0);

            // Branch: flag == 0 → loop_block (hot), flag != 0 → cold resume dispatch
            let resume_dispatch = builder.create_block();
            builder.set_cold_block(resume_dispatch);
            let is_resume = builder.ins().icmp(IntCC::NotEqual, resume_flag, zero);
            builder.ins().brif(is_resume, resume_dispatch, &[], loop_block, &[]);

            // Cold resume dispatch: clear the flag, then branch to the right resume block.
            builder.switch_to_block(resume_dispatch);
            let ctx_val2 = builder.use_var(ctx_var);
            builder.ins().store(MemFlags::trusted(), zero, ctx_val2, CTX_OFFSET_RESUME_RECEIVE);

            if total_resume_points == 1 {
                // Single suspend point: jump directly.
                builder.ins().jump(all_resume_blocks[0], &[]);
            } else {
                // Multiple suspend points: use a switch on the resume index.
                let mut switch = cranelift_frontend::Switch::new();
                for (i, &resume_block) in all_resume_blocks.iter().enumerate() {
                    switch.set_entry((i + 1) as u128, resume_block); // 1-based index
                }
                switch.emit(&mut builder, resume_flag, loop_block);
            }

            // Define ALL resume preamble blocks: load saved variables, then
            // jump to the appropriate target.
            let live_across = self.current_live_across_vars.clone();

            // Helper to declare and get the load_resume_var function
            let load_var_id = {
                let name = "nera_rt_load_resume_var";
                if let Some(id) = self.runtime_funcs.get(name) {
                    *id
                } else {
                    let mut sig = Signature::new(self.c_call_conv);
                    sig.params.push(AbiParam::new(types::I64)); // ctx
                    sig.params.push(AbiParam::new(types::I32)); // index
                    sig.returns.push(AbiParam::new(types::I64)); // value
                    let func_id = module
                        .declare_function(name, Linkage::Import, &sig)
                        .map_err(|e| TranslateError::Module(e.to_string()))?;
                    self.runtime_funcs.insert(name.to_string(), func_id);
                    func_id
                }
            };

            for (i, &resume_block) in all_resume_blocks.iter().enumerate() {
                builder.switch_to_block(resume_block);

                let vars_to_load = if i < live_across.len() {
                    &live_across[i]
                } else {
                    &[][..]
                };

                if !vars_to_load.is_empty() {
                    let load_var_ref = module.declare_func_in_func(load_var_id, builder.func);
                    let ctx_val2 = builder.use_var(ctx_var);
                    for (j, var_id) in vars_to_load.iter().enumerate() {
                        let idx_val = builder.ins().iconst(types::I32, j as i64);
                        let call = builder.ins().call(load_var_ref, &[ctx_val2, idx_val]);
                        let loaded = builder.inst_results(call)[0];
                        if let Some(var) = var_map.get(var_id) {
                            builder.def_var(*var, loaded);
                        }
                    }
                }

                // Reload reds_var from ctx.reductions at resume entry
                // (scheduler resets reductions before re-calling).
                self.reload_reds_from_ctx(&mut builder, ctx_var);

                if i < num_receive_points {
                    // Receive resume: jump to recv_entry_block
                    builder.ins().jump(recv_entry_blocks[i], &[]);
                } else if i < num_receive_points + num_call_sites {
                    // Call-site resume: read ctx.result into the call's result
                    // variable, then jump to the continuation block.
                    let call_idx = i - num_receive_points;
                    // An exceptional resumed callee must unwind unprotected callers
                    // without executing their continuation. Protected calls continue
                    // to their existing CheckException instruction.
                    if !call_site_try_protected[call_idx] {
                        let ctx = builder.use_var(ctx_var);
                        let status = builder.ins().load(
                            types::I32, MemFlags::trusted(), ctx, CTX_OFFSET_STATUS,
                        );
                        let exceptional = builder.ins().icmp_imm(
                            IntCC::Equal, status, STATUS_EXCEPTION as i64,
                        );
                        let unwind = builder.create_block();
                        let success = builder.create_block();
                        builder.set_cold_block(unwind);
                        builder.ins().brif(exceptional, unwind, &[], success, &[]);
                        builder.switch_to_block(unwind);
                        let nil = builder.ins().iconst(types::I64, NIL_VALUE as i64);
                        self.emit_stack_pop_and_return(module, &mut builder, ctx_var, nil)?;
                        builder.switch_to_block(success);
                    }
                    let result_var_id = self.current_call_result_vars[call_idx];
                    let ctx_val3 = builder.use_var(ctx_var);
                    let result_val = builder.ins().load(
                        types::I64,
                        MemFlags::trusted(),
                        ctx_val3,
                        CTX_OFFSET_RESULT,
                    );
                    if let Some(var) = var_map.get(&result_var_id) {
                        builder.def_var(*var, result_val);
                    }
                    builder.ins().jump(call_cont_blocks[call_idx], &[]);
                } else {
                    // Yield-check resume: no result to load, just jump to
                    // the continuation block where execution continues.
                    let yc_idx = i - num_receive_points - num_call_sites;
                    builder.ins().jump(yield_check_cont_blocks[yc_idx], &[]);
                }
            }
        } else {
            builder.ins().jump(loop_block, &[]);
        }

        builder.switch_to_block(loop_block);
        // NOTE: reds_var is loaded in entry_cl_block (before the jump here),
        // NOT here. Loading here would reset it on every self-tail-call iteration.
        // Reset receive index counter for this function
        self.current_receive_index = 0;

        // Translate each block
        // We process the entry block first (its instructions go into the loop block),
        // then process all remaining blocks.
        let entry_ir_block = ir_func
            .blocks
            .iter()
            .find(|b| b.id == ir_func.entry_block)
            .expect("entry block must exist in function");

        let non_entry_blocks: Vec<&nera_ir::ir::BasicBlock> = ir_func
            .blocks
            .iter()
            .filter(|b| b.id != ir_func.entry_block)
            .collect();

        let current_func_name = Some(&ir_func.name);

        // --- Translate entry block instructions (in the loop block) ---
        self.current_ir_block_id = Some(entry_ir_block.id);
        for (inst_idx, inst) in entry_ir_block.instructions.iter().enumerate() {
            self.current_inst_idx = inst_idx;
            self.translate_instruction(
                module,
                &mut builder,
                inst,
                &var_map,
                ctx_var,
                mod_ctx,
            )?;
            // After a local call, check if we need to emit a status check
            // and branch to a continuation block.
            if let Some(&cs_idx) = call_site_map.get(&(entry_ir_block.id, inst_idx)) {
                self.emit_call_site_status_check(
                    module,
                    &mut builder,
                    &var_map,
                    ctx_var,
                    cs_idx,
                    num_receive_points,
                    &call_cont_blocks,
                    call_site_try_protected[cs_idx],
                )?;
                // Reload reds_var: callee may have consumed reductions.
                self.reload_reds_from_ctx(&mut builder, ctx_var);
            }
            if let Some(&yc_idx) = yield_check_map.get(&(entry_ir_block.id, inst_idx)) {
                self.emit_yield_check_status(
                    module,
                    &mut builder,
                    &var_map,
                    ctx_var,
                    yc_idx,
                    num_receive_points,
                    num_call_sites,
                    &yield_check_cont_blocks,
                )?;
            }
        }
        self.translate_terminator(
            module,
            &mut builder,
            &entry_ir_block.terminator,
            &var_map,
            &block_map,
            ctx_var,
            mod_ctx,
            current_func_name,
            &ir_func.params,
            Some(loop_block),
        )?;

        // --- Translate remaining blocks ---
        for ir_block in non_entry_blocks {
            let cl_block = block_map[&ir_block.id];
            builder.switch_to_block(cl_block);
            self.current_ir_block_id = Some(ir_block.id);

            // Check if this block needs to be split at a receive_peek instruction.
            let split_info = recv_block_map.get(&ir_block.id).copied();

            if let Some((recv_0idx, recv_inst_idx)) = split_info {
                // This block contains a receive_peek. Split it:
                // - Instructions before receive_peek go into cl_block
                // - Jump to the corresponding recv_entry_block
                // - receive_peek and subsequent instructions go into recv_entry_block
                //
                // Set current_receive_index so emit_set_resume knows which
                // receive point this is (1-based).
                self.current_receive_index = (recv_0idx + 1) as u32;

                for (inst_idx, inst) in ir_block.instructions[..recv_inst_idx].iter().enumerate() {
                    self.current_inst_idx = inst_idx;
                    self.translate_instruction(
                        module,
                        &mut builder,
                        inst,
                        &var_map,
                        ctx_var,
                        mod_ctx,
                    )?;
                    if let Some(&cs_idx) = call_site_map.get(&(ir_block.id, inst_idx)) {
                        self.emit_call_site_status_check(
                            module,
                            &mut builder,
                            &var_map,
                            ctx_var,
                            cs_idx,
                            num_receive_points,
                            &call_cont_blocks,
                            call_site_try_protected[cs_idx],
                        )?;
                        self.reload_reds_from_ctx(&mut builder, ctx_var);
                    }
                    if let Some(&yc_idx) = yield_check_map.get(&(ir_block.id, inst_idx)) {
                        self.emit_yield_check_status(
                            module,
                            &mut builder,
                            &var_map,
                            ctx_var,
                            yc_idx,
                            num_receive_points,
                            num_call_sites,
                            &yield_check_cont_blocks,
                        )?;
                    }
                }
                let recv_entry = recv_entry_blocks[recv_0idx];
                builder.ins().jump(recv_entry, &[]);

                // Switch to the receive entry block and translate from receive_peek onward.
                builder.switch_to_block(recv_entry);
                for (rel_idx, inst) in ir_block.instructions[recv_inst_idx..].iter().enumerate() {
                    let abs_idx = recv_inst_idx + rel_idx;
                    self.current_inst_idx = abs_idx;
                    self.translate_instruction(
                        module,
                        &mut builder,
                        inst,
                        &var_map,
                        ctx_var,
                        mod_ctx,
                    )?;
                    if let Some(&cs_idx) = call_site_map.get(&(ir_block.id, abs_idx)) {
                        self.emit_call_site_status_check(
                            module,
                            &mut builder,
                            &var_map,
                            ctx_var,
                            cs_idx,
                            num_receive_points,
                            &call_cont_blocks,
                            call_site_try_protected[cs_idx],
                        )?;
                        self.reload_reds_from_ctx(&mut builder, ctx_var);
                    }
                    if let Some(&yc_idx) = yield_check_map.get(&(ir_block.id, abs_idx)) {
                        self.emit_yield_check_status(
                            module,
                            &mut builder,
                            &var_map,
                            ctx_var,
                            yc_idx,
                            num_receive_points,
                            num_call_sites,
                            &yield_check_cont_blocks,
                        )?;
                    }
                }
            } else {
                // Normal block: translate all instructions.
                for (inst_idx, inst) in ir_block.instructions.iter().enumerate() {
                    self.current_inst_idx = inst_idx;
                    self.translate_instruction(
                        module,
                        &mut builder,
                        inst,
                        &var_map,
                        ctx_var,
                        mod_ctx,
                    )?;
                    if let Some(&cs_idx) = call_site_map.get(&(ir_block.id, inst_idx)) {
                        self.emit_call_site_status_check(
                            module,
                            &mut builder,
                            &var_map,
                            ctx_var,
                            cs_idx,
                            num_receive_points,
                            &call_cont_blocks,
                            call_site_try_protected[cs_idx],
                        )?;
                        self.reload_reds_from_ctx(&mut builder, ctx_var);
                    }
                    if let Some(&yc_idx) = yield_check_map.get(&(ir_block.id, inst_idx)) {
                        self.emit_yield_check_status(
                            module,
                            &mut builder,
                            &var_map,
                            ctx_var,
                            yc_idx,
                            num_receive_points,
                            num_call_sites,
                            &yield_check_cont_blocks,
                        )?;
                    }
                }
            }

            // Translate terminator (always goes into the current block,
            // which is recv_entry_block for split blocks).
            self.translate_terminator(
                module,
                &mut builder,
                &ir_block.terminator,
                &var_map,
                &block_map,
                ctx_var,
                mod_ctx,
                current_func_name,
                &ir_func.params,
                Some(loop_block),
            )?;
        }

        builder.seal_all_blocks();
        builder.finalize();

        // Define the function in the module
        let mut ctx = cranelift_codegen::Context::for_function(func);
        module
            .define_function(func_id, &mut ctx)
            .map_err(|e| TranslateError::Codegen {
                block_id: self.current_ir_block_id,
                reason: e.to_string(),
            })?;

        // Clear per-function state
        self.current_suspend_points.clear();
        self.current_suspend_map.clear();
        self.current_call_resume_blocks.clear();
        self.current_call_cont_blocks.clear();
        self.current_call_result_vars.clear();
        self.current_ir_block_id = None;
        self.current_inst_idx = 0;
        self.current_try_protected_values.clear();
        self.reds_var = None;

        Ok(())
    }
}

impl Default for FuncTranslator {
    fn default() -> Self {
        Self::new()
    }
}
