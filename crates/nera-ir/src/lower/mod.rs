//! AST-to-IR lowering.
//!
//! Transforms the parsed Erlang AST into a mutable-register control-flow IR suitable
//! for code generation. Handles variable scoping, tail call detection,
//! and delegates pattern matching to the pattern compiler.

mod closure;
mod exception;
mod exports;
mod expr;
mod list_comp;
mod pattern_lower;
mod record;

use std::collections::HashMap;
use nera_syntax::ast::{self, AtomId};
use nera_syntax::source_map::SourceMap;
use nera_syntax::span::Span;
use crate::ir::*;

// Preserve the original public paths for callers of individual passes.
pub use crate::passes::{
    eliminate_dead_code, eliminate_dead_match_contexts, fold_constants, fuse_make_tuple_send,
    insert_backedge_yield_checks,
};
use crate::pattern::{self, MatchClause};

/// Error during IR lowering.
#[derive(Debug, Clone, thiserror::Error)]
pub enum LowerError {
    #[error("undefined variable: '{0}'")]
    UndefinedVariable(String, Option<Span>),

    #[error("unsupported expression: {0}")]
    Unsupported(String, Option<Span>),

    #[error("undefined record '{0}'")]
    UndefinedRecord(String, Option<Span>),

    /// A pattern OTP rejects at compile time; the message is OTP's own.
    #[error("{0}")]
    InvalidPattern(String, Option<Span>),
}

impl LowerError {
    /// Return the source span associated with this error, if any.
    pub fn span(&self) -> Option<Span> {
        match self {
            LowerError::UndefinedVariable(_, s)
            | LowerError::Unsupported(_, s)
            | LowerError::UndefinedRecord(_, s)
            | LowerError::InvalidPattern(_, s) => *s,
        }
    }
}

/// Builds IR for a module.
pub struct IrBuilder {
    #[allow(dead_code)]
    pub(super) module_name: AtomId,
    pub(super) functions: Vec<IrFunction>,
    /// Blocks for the current function
    pub(crate) current_blocks: Vec<BasicBlock>,
    /// Current block being built
    pub(super) current_block_id: Option<BlockId>,
    /// Instructions for the current block
    pub(super) current_instructions: Vec<Instruction>,
    /// Terminator for the current block (set before closing)
    pub(super) current_terminator: Option<Terminator>,

    pub(super) next_var: u32,
    pub(super) next_block: u32,

    /// Variable scope stack
    pub(super) scopes: Vec<HashMap<AtomId, VarId>>,

    /// Function names in this module for local call detection
    pub(super) module_functions: HashMap<(AtomId, u32), String>,

    /// Local argument positions whose every clause requires a binary pattern.
    /// Only these callees can safely receive an internal binary match cursor.
    pub(super) binary_match_args: HashMap<(AtomId, u32), Vec<bool>>,

    pub(super) imported_functions: HashMap<(AtomId, u32), AtomId>,
    pub(super) resolution: crate::resolution::ResolutionPolicy,

    /// Parser atom ID for `true`
    pub(crate) true_atom_id: AtomId,
    /// Parser atom ID for `false`
    pub(crate) false_atom_id: AtomId,
    /// Parser atom ID for `self`
    pub(crate) self_atom_id: AtomId,

    /// Atom name resolver: maps AtomId -> string name.
    /// Used to resolve auto-imported BIF names.
    pub(super) atom_names: HashMap<AtomId, String>,

    /// Atom ID for the "erlang" module (for auto-import rewriting)
    pub(super) erlang_atom_id: AtomId,
    /// Atom ID for the "io" module (reserved for future auto-import)
    #[allow(dead_code)]
    pub(super) io_atom_id: AtomId,
    /// Atom ID for the "maps" module (reserved for future auto-import)
    #[allow(dead_code)]
    pub(super) maps_atom_id: AtomId,

    /// Stack of exception landing-pad blocks for try contexts.
    /// When a `RaiseException` occurs inside a try body, control jumps to
    /// the top of this stack instead of returning from the function.
    pub(super) try_exception_targets: Vec<BlockId>,
    pub(super) protected_values: Vec<VarId>,

    /// Counter for generating unique list comprehension helper names.
    pub(super) lc_counter: u32,
    /// Counter for generating unique lambda names.
    pub(super) lambda_counter: u32,
    /// Counter for generating fresh atom IDs without O(n) max() scans.
    pub(super) next_atom_id: u32,

    /// Record definitions from the module, keyed by record name AtomId.
    pub(super) record_defs: HashMap<AtomId, ast::RecordDef>,

    /// Match context tracking for binary pattern optimization.
    /// Maps a VarId (the "rest" variable from a binary pattern) to its
    /// match context info. When byte_size is called on such a variable,
    /// we can round the remaining bits up to bytes instead of materializing.
    pub(crate) match_contexts: HashMap<VarId, MatchContextInfo>,

    /// True while a pattern is pre-bound or compiled. Both run out of program
    /// order (clauses in reverse, bodies before their patterns), so a deferred
    /// tail that a map key, segment size or guard reads there is materialized
    /// on that path only. Its record stays for the other clauses, for later
    /// comparisons of the same name, and for the code after the match.
    pub(crate) matching: bool,

    /// Inputs of a shadowing fun/generator pattern. Keys read these inputs;
    /// sizes additionally see earlier segments of their own binary.
    pub(super) pattern_inputs: Option<HashMap<AtomId, VarId>>,

    /// Where names are used again after each `case`, `if` and `receive` of
    /// the function clause being lowered, including its funs and
    /// comprehensions. Decides which names a branch exports.
    later_uses: exports::LaterUses,

    /// Source map for converting byte offsets to line numbers.
    /// None in unit tests where no source file is available.
    source_map: Option<SourceMap>,
}

/// Information about a deferred binary match context.
/// When a rest segment variable is in this map, it means the sub-binary
/// hasn't been allocated yet — we have the raw components to reconstruct it.
/// All offsets and sizes are in BITS (not bytes).
#[derive(Debug, Clone)]
pub(crate) struct MatchContextInfo {
    /// Raw (untagged) pointer to binary data — from BsStartMatch/BsMatchStart2
    pub(crate) _base_ptr: VarId,
    /// Tagged small int: current bit offset into the binary
    pub(crate) bit_offset: VarId,
    /// Tagged small int: remaining bits from offset to end
    pub(crate) remaining_bits: VarId,
    /// The match context term (tagged BOXED_MATCH_CTX) — for materialization via BsMaterialize
    /// and for passing to callees that do BsMatchStart2 (which can reuse it).
    pub(crate) match_ctx: VarId,
}

/// Saved state for nested function compilation (list comprehensions, lambdas).
/// The builder state is saved before entering a nested function context and
/// restored after the nested function is fully compiled.
pub(super) struct BuilderState {
    pub(super) blocks: Vec<BasicBlock>,
    pub(super) block_id: Option<BlockId>,
    pub(super) instructions: Vec<Instruction>,
    pub(super) terminator: Option<Terminator>,
    pub(super) next_var: u32,
    pub(super) next_block: u32,
    pub(super) scopes: Vec<HashMap<AtomId, VarId>>,
    pub(super) try_exception_targets: Vec<BlockId>,
    pub(super) protected_values: Vec<VarId>,
    // VarIds are local to each function, including deferred binary bindings.
    pub(super) match_contexts: HashMap<VarId, MatchContextInfo>,
    pattern_inputs: Option<HashMap<AtomId, VarId>>,
}

impl IrBuilder {
    /// Save the current function-building state so we can compile a nested
    /// helper function (LC helper or lambda) and then return to the enclosing
    /// function exactly where we left off.
    pub(super) fn save_state(&mut self) -> BuilderState {
        let state = BuilderState {
            blocks: std::mem::take(&mut self.current_blocks),
            block_id: self.current_block_id.take(),
            instructions: std::mem::take(&mut self.current_instructions),
            terminator: self.current_terminator.take(),
            next_var: self.next_var,
            next_block: self.next_block,
            scopes: std::mem::take(&mut self.scopes),
            try_exception_targets: std::mem::take(&mut self.try_exception_targets),
            protected_values: std::mem::take(&mut self.protected_values),
            match_contexts: std::mem::take(&mut self.match_contexts),
            pattern_inputs: self.pattern_inputs.take(),
        };
        // Reset for the nested function
        self.current_blocks = Vec::new();
        self.protected_values.clear();
        self.next_var = 0;
        self.next_block = 0;
        self.scopes = vec![HashMap::new()];
        self.try_exception_targets = Vec::new();
        state
    }

    /// Restore builder state after compiling a nested function.
    pub(super) fn restore_state(&mut self, state: BuilderState) {
        self.current_blocks = state.blocks;
        self.current_block_id = state.block_id;
        self.current_instructions = state.instructions;
        self.current_terminator = state.terminator;
        self.next_var = state.next_var;
        self.next_block = state.next_block;
        self.scopes = state.scopes;
        self.try_exception_targets = state.try_exception_targets;
        self.protected_values = state.protected_values;
        self.match_contexts = state.match_contexts;
        self.pattern_inputs = state.pattern_inputs;
    }

    pub fn new(
        module_name: AtomId,
        true_atom_id: AtomId,
        false_atom_id: AtomId,
        self_atom_id: AtomId,
        mut atom_names: HashMap<AtomId, String>,
    ) -> Self {
        // Find well-known module atom IDs from the name map, or synthesize them.
        let mut next_atom_id = atom_names.keys().map(|id| id.0).max().unwrap_or(0) + 1;
        let mut synth_id = next_atom_id;

        let erlang_atom_id = atom_names.iter()
            .find(|(_, name)| name.as_str() == "erlang")
            .map(|(id, _)| *id)
            .unwrap_or_else(|| {
                let id = AtomId(synth_id);
                atom_names.insert(id, "erlang".to_string());
                synth_id += 1;
                id
            });
        let io_atom_id = atom_names.iter()
            .find(|(_, name)| name.as_str() == "io")
            .map(|(id, _)| *id)
            .unwrap_or_else(|| {
                let id = AtomId(synth_id);
                atom_names.insert(id, "io".to_string());
                synth_id += 1;
                id
            });
        let maps_atom_id = atom_names.iter()
            .find(|(_, name)| name.as_str() == "maps")
            .map(|(id, _)| *id)
            .unwrap_or_else(|| {
                let id = AtomId(synth_id);
                atom_names.insert(id, "maps".to_string());
                synth_id += 1;
                id
            });
        // Update next_atom_id to account for any atoms synthesized above
        next_atom_id = synth_id;

        Self {
            module_name,
            functions: Vec::new(),
            current_blocks: Vec::new(),
            current_block_id: None,
            current_instructions: Vec::new(),
            current_terminator: None,
            next_var: 0,
            next_block: 0,
            scopes: vec![HashMap::new()],
            module_functions: HashMap::new(),
            binary_match_args: HashMap::new(),
            imported_functions: HashMap::new(),
            resolution: crate::resolution::ResolutionPolicy::default(),
            true_atom_id,
            false_atom_id,
            self_atom_id,
            atom_names,
            erlang_atom_id,
            io_atom_id,
            maps_atom_id,
            try_exception_targets: Vec::new(),
            protected_values: Vec::new(),
            lc_counter: 0,
            lambda_counter: 0,
            next_atom_id,
            record_defs: HashMap::new(),
            match_contexts: HashMap::new(),
            matching: false,
            pattern_inputs: None,
            later_uses: exports::LaterUses::default(),
            source_map: None,
        }
    }

    /// Convert a source span to a 1-based line number. Returns 0 if no source map.
    pub(super) fn span_line(&self, span: Span) -> u32 {
        match &self.source_map {
            Some(sm) => sm.offset_to_position(span.start).line as u32,
            None => 0,
        }
    }

    /// Emit a SetLine instruction if the line number is known (non-zero).
    pub(super) fn emit_set_line(&mut self, span: Span) {
        let line = self.span_line(span);
        if line > 0 {
            self.emit(Instruction::SetLine(line));
        }
    }

    /// Lower an AST module to IR. Returns the IR module and the (possibly
    /// extended) atom name map — lowering may intern new atoms (e.g.
    /// `case_clause`, `if_clause`) that the code generator needs.
    pub fn lower_module(
        module: &ast::Module,
        true_atom_id: AtomId,
        false_atom_id: AtomId,
        self_atom_id: AtomId,
        atom_names: HashMap<AtomId, String>,
        source_map: Option<SourceMap>,
    ) -> Result<(IrModule, HashMap<AtomId, String>), LowerError> {
        // Structural guard before any function is lowered: embedded modules
        // skip lint, and guard, size and map-key lowering discard errors. An
        // unsized `<=` field would make its generator match without advancing,
        // a sized or non-UTF string pattern would match something else, and
        // conflicting specifiers would silently use whichever type was last.
        if let Some(error) = crate::warnings::first_refused_segment_error(module) {
            return Err(LowerError::InvalidPattern(error.message, error.span));
        }

        let mut builder = IrBuilder::new(module.name, true_atom_id, false_atom_id, self_atom_id, atom_names);
        builder.source_map = source_map;
        builder.resolution = crate::resolution::ResolutionPolicy::new(module, &builder.atom_names);

        // Register record definitions for desugaring during lowering
        for rec in &module.records {
            builder.record_defs.insert(rec.name, rec.clone());
        }

        // First pass: register all function names for local call resolution
        for func in &module.functions {
            let mangled = format!(
                "{}__{}_{}",
                module.name.0, func.name.0, func.arity
            );
            builder
                .module_functions
                .insert((func.name, func.arity), mangled);
            let binary_args = (0..func.arity as usize).map(|index| {
                !func.clauses.is_empty() && func.clauses.iter().all(|clause| {
                    matches!(clause.patterns.get(index), Some(ast::Pattern::Binary(_, _)))
                })
            }).collect();
            builder.binary_match_args.insert((func.name, func.arity), binary_args);
        }

        for import in &module.imports {
            for fun in &import.functions {
                let key = (fun.name, fun.arity);
                if builder.module_functions.contains_key(&key)
                    || builder.imported_functions.contains_key(&key) {
                    return Err(LowerError::Unsupported(
                        format!("conflicting import for {}/{}", builder.resolve_atom(fun.name).unwrap_or("?"), fun.arity),
                        Some(fun.span),
                    ));
                }
                builder.imported_functions.insert(key, import.module);
            }
        }

        // Second pass: lower each function
        for func in &module.functions {
            builder.lower_function(func)?;
        }

        let exports = module
            .exports
            .iter()
            .map(|fa| IrExport {
                name: fa.name,
                arity: fa.arity,
            })
            .collect();

        let ir_module = IrModule {
            name: module.name,
            functions: builder.functions,
            exports,
        };

        Ok((ir_module, builder.atom_names))
    }

    fn lower_function(&mut self, func: &ast::Function) -> Result<(), LowerError> {
        let mangled = self
            .module_functions
            .get(&(func.name, func.arity))
            .cloned()
            .unwrap_or_else(|| format!("{}_{}", func.name.0, func.arity));

        // Reset state for new function
        self.current_blocks = Vec::new();
        self.protected_values.clear();
        self.next_block = 0;
        self.next_var = 0;
        self.scopes = vec![HashMap::new()];
        self.match_contexts.clear();

        // Create entry block with parameters (don't start it yet — we build it last)
        let entry_block = self.fresh_block_with_id();
        let params: Vec<VarId> = (0..func.arity).map(|_| self.fresh_var()).collect();

        // Compute function definition line number from the span.
        let func_line = self.span_line(func.span);

        // Lower function clauses as pattern matching on parameters
        let fail_block = self.fresh_block_with_id();
        self.start_block(fail_block);
        // On match failure, raise function_clause exception
        self.emit_set_line(func.span);
        let fc_atom = self.atom_id_for("function_clause");
        let reason = self.emit_const(Literal::Atom(fc_atom));
        let exc_result = self.fresh_var();
        self.emit(Instruction::RaiseException(exc_result, ExceptionClass::Error, reason));
        self.set_terminator(Terminator::Return(exc_result));
        self.close_current_block();

        // Phase 1: Build match clauses with body block IDs only (no body code yet)
        let mut match_clauses = Vec::new();
        let mut clause_body_blocks = Vec::new();
        for clause in &func.clauses {
            let body_block = self.fresh_block_with_id();
            clause_body_blocks.push(body_block);

            // Desugar record patterns in function head before pattern compilation
            let desugared_patterns: Vec<_> = clause.patterns.iter()
                .map(|p| self.desugar_record_pattern(p))
                .collect::<Result<_, _>>()?;
            match_clauses.push(MatchClause {
                patterns: desugared_patterns,
                guard: clause.guard.clone(),
                body_block,
            });
        }

        // Phase 2: Compile pattern matching (this binds variables in the pattern compiler)
        let match_entry =
            pattern::compile_match(self, &params, &match_clauses, fail_block);

        // Phase 3: Now lower each clause body. Pattern variables are bound by
        // pre-binding them from the clause patterns before lowering the body.
        for (clause_idx, clause) in func.clauses.iter().enumerate() {
            let body_block = clause_body_blocks[clause_idx];

            self.later_uses = exports::LaterUses::of_clause(clause);
            self.push_scope();
            self.start_block(body_block);

            // Pre-bind pattern variables to corresponding parameter VarIds.
            // This MUST happen after start_block so that destructuring
            // instructions (ListHead, ListTail, TupleElement) end up in
            // the body block, not lost before it.
            // Desugar record patterns before pre-binding.
            let desugared_patterns: Vec<_> = clause.patterns.iter()
                .map(|p| self.desugar_record_pattern(p))
                .collect::<Result<_, _>>()?;
            for (pat, param) in desugared_patterns.iter().zip(params.iter()) {
                self.pre_bind_pattern_vars(pat, *param, false);
            }
            let result = self.lower_body(&clause.body, true)?;
            // Only set Return if the body didn't already set a terminator
            // (e.g., TailCall from a tail-position call)
            if self.current_terminator.is_none() {
                self.set_terminator(Terminator::Return(result));
            }
            self.close_current_block();
            self.pop_scope();
        }

        // Wire entry block to match entry
        self.start_block(entry_block);
        self.set_terminator(Terminator::Jump(match_entry, vec![]));
        self.close_current_block();

        let mut ir_func = IrFunction {
            name: func.name,
            arity: func.arity,
            mangled_name: mangled,
            blocks: std::mem::take(&mut self.current_blocks),
            protected_values: std::mem::take(&mut self.protected_values),
            entry_block,
            params,
            line: func_line,
        };

        crate::passes::run_function_passes(&mut ir_func, self.true_atom_id, self.false_atom_id);
        self.functions.push(ir_func);
        Ok(())
    }

    /// Lower a sequence of expressions (function body).
    /// If `is_tail` is true, the last expression is in tail position.
    pub(super) fn lower_body(
        &mut self,
        body: &[ast::Expr],
        is_tail: bool,
    ) -> Result<VarId, LowerError> {
        let mut result = self.emit_const(Literal::Atom(AtomId(4))); // undefined

        for (i, expr) in body.iter().enumerate() {
            let is_last = i == body.len() - 1;
            result = self.lower_expr(expr, is_last && is_tail)?;
        }

        Ok(result)
    }

    /// Allocate a fresh, unique AtomId.
    pub(super) fn fresh_atom_id(&mut self) -> AtomId {
        let id = AtomId(self.next_atom_id);
        self.next_atom_id += 1;
        id
    }

    /// Get or create an AtomId for a string (used for exception class atoms).
    pub(super) fn atom_id_for(&mut self, name: &str) -> AtomId {
        // Look for existing atom
        for (id, s) in &self.atom_names {
            if s == name {
                return *id;
            }
        }
        // Create a new atom
        let id = self.fresh_atom_id();
        self.atom_names.insert(id, name.to_string());
        id
    }

    // ---- Builder helpers ----

    pub fn fresh_var(&mut self) -> VarId {
        let id = VarId(self.next_var);
        self.next_var += 1;
        id
    }

    pub fn fresh_block_with_id(&mut self) -> BlockId {
        let id = BlockId(self.next_block);
        self.next_block += 1;
        id
    }

    pub fn start_block(&mut self, id: BlockId) {
        // Close any open block first
        if self.current_block_id.is_some() {
            self.close_current_block();
        }
        self.current_block_id = Some(id);
        self.current_instructions = Vec::new();
        self.current_terminator = None;
    }

    pub fn close_current_block(&mut self) {
        if let Some(id) = self.current_block_id.take() {
            let terminator = self
                .current_terminator
                .take()
                .unwrap_or(Terminator::Unreachable);

            self.current_blocks.push(BasicBlock {
                id,
                params: Vec::new(),
                instructions: std::mem::take(&mut self.current_instructions),
                terminator,
            });
        }
    }

    pub fn emit(&mut self, inst: Instruction) {
        if !self.try_exception_targets.is_empty() {
            let result = match &inst {
                Instruction::Call(v, _, _) | Instruction::BinOp(v, _, _, _)
                | Instruction::UnaryOp(v, _, _) | Instruction::ListHead(v, _)
                | Instruction::ListTail(v, _) | Instruction::MapGet(v, _, _)
                | Instruction::MapPut(v, _, _, _) | Instruction::MapUpdateExact(v, _, _, _)
                | Instruction::MakeBinary(v, _)
                | Instruction::BinaryPutUtf8(v, _, _, _)
                | Instruction::BinaryPutUtf16(v, _, _, _, _)
                | Instruction::BinaryPutUtf32(v, _, _, _, _)
                | Instruction::Send(v, _, _) | Instruction::Spawn(v, _, _, _) => Some(*v),
                _ => None,
            };
            if let Some(result) = result { self.protected_values.push(result); }
        }
        self.current_instructions.push(inst);
    }

    pub fn emit_const(&mut self, lit: Literal) -> VarId {
        let var = self.fresh_var();
        self.emit(Instruction::Const(var, lit));
        var
    }

    pub fn set_terminator(&mut self, term: Terminator) {
        self.current_terminator = Some(term);
    }

    /// Patch the terminator of an already-closed block.
    pub fn patch_block_terminator(&mut self, block_id: BlockId, terminator: Terminator) {
        if let Some(block) = self.current_blocks.iter_mut().find(|b| b.id == block_id) {
            block.terminator = terminator;
        }
    }

    pub fn push_scope(&mut self) {
        let current = self.scopes.last().cloned().unwrap_or_default();
        self.scopes.push(current);
    }

    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub fn bind_var(&mut self, name: AtomId, var: VarId) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, var);
        }
    }

    pub fn lookup_var(&self, name: AtomId) -> Option<VarId> {
        for scope in self.scopes.iter().rev() {
            if let Some(var) = scope.get(&name) {
                return Some(*var);
            }
        }
        None
    }

    /// Collect all currently-bound variables across all scopes.
    /// Later scopes shadow earlier ones (last write wins).
    pub fn collect_all_bindings(&self) -> std::collections::HashMap<AtomId, VarId> {
        let mut result = std::collections::HashMap::new();
        for scope in &self.scopes {
            for (&name, &var) in scope {
                result.insert(name, var);
            }
        }
        result
    }

    /// Resolve an atom ID to its string name, if known.
    pub(super) fn resolve_atom(&self, id: AtomId) -> Option<&str> {
        self.atom_names.get(&id).map(|s| s.as_str())
    }


}

#[cfg(test)]
mod tests;
