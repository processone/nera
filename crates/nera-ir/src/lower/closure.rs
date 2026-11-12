//! Closure and fun-ref lowering.

use nera_syntax::ast::{self, AtomId};
use crate::ir::*;
use crate::pattern;
use crate::passes::insert_backedge_yield_checks;
use super::{IrBuilder, LowerError};

impl IrBuilder {
    // ---- Anonymous function (lambda) lowering ----

    /// Lower an anonymous fun expression by lambda lifting.
    ///
    /// The fun's clauses are compiled as a new top-level IrFunction with
    /// captured free variables prepended as extra parameters. A `MakeClosure`
    /// instruction is emitted in the enclosing function to bundle the
    /// function pointer with captured values.
    pub(super) fn lower_expr_fun(
        &mut self,
        name: Option<AtomId>,
        clauses: &[ast::FunctionClause],
    ) -> Result<VarId, LowerError> {
        let user_arity = clauses[0].patterns.len() as u32;

        // Each clause shadows only its own value bindings. Pattern key/size
        // reads still refer to the enclosing scope, even with the same name.
        let mut referenced = std::collections::HashSet::new();
        for clause in clauses {
            let mut bound = std::collections::HashSet::new();
            let mut body_refs = std::collections::HashSet::new();
            for pat in &clause.patterns {
                self.collect_pattern_bindings(pat, &mut bound);
                crate::warnings::collect_pattern_refs(pat, &mut referenced);
            }
            if let Some(ref guard) = clause.guard {
                self.collect_guard_refs(guard, &mut body_refs);
            }
            for expr in &clause.body {
                self.collect_expr_refs(expr, &mut body_refs);
            }
            referenced.extend(body_refs.difference(&bound).copied());
        }
        let mut free_vars: Vec<AtomId> = referenced
            .into_iter()
            .filter(|n| Some(*n) != name && self.lookup_var(*n).is_some())
            .collect();
        free_vars.sort_by_key(|a| a.0); // deterministic order

        let env_count = free_vars.len() as u32;
        let total_arity = env_count + user_arity;

        // Generate unique lambda name
        let lambda_id = self.lambda_counter;
        self.lambda_counter += 1;
        let lambda_mangled = format!("{}__lambda_{}", self.module_name.0, lambda_id);

        // Synthesize a unique AtomId for the lambda function name
        let lambda_name_str = format!("__lambda_{}", lambda_id);
        let lambda_name_id = self.fresh_atom_id();
        self.atom_names.insert(lambda_name_id, lambda_name_str);
        self.module_functions
            .insert((lambda_name_id, total_arity), lambda_mangled.clone());

        let saved = self.save_state();

        // Create parameters: env vars first, then user params
        let params: Vec<VarId> = (0..total_arity).map(|_| self.fresh_var()).collect();

        // Use the same approach as lower_function: pattern compile + body lowering
        let fail_block = self.fresh_block_with_id();
        self.start_block(fail_block);
        let fc_atom = self.atom_id_for("function_clause");
        let reason = self.emit_const(Literal::Atom(fc_atom));
        let exc_result = self.fresh_var();
        self.emit(Instruction::RaiseException(
            exc_result,
            ExceptionClass::Error,
            reason,
        ));
        self.set_terminator(Terminator::Return(exc_result));
        self.close_current_block();

        // The user-visible params are the ones after the env params.
        let user_params: Vec<VarId> = params[env_count as usize..].to_vec();

        // Build match clauses (using user params for pattern matching)
        let mut match_clauses = Vec::new();
        let mut clause_body_blocks = Vec::new();
        for clause in clauses {
            let body_block = self.fresh_block_with_id();
            clause_body_blocks.push(body_block);
            // Desugar record patterns in lambda clauses
            let desugared_patterns: Vec<_> = clause.patterns.iter()
                .map(|p| self.desugar_record_pattern(p))
                .collect::<Result<_, _>>()?;
            match_clauses.push(pattern::MatchClause {
                patterns: desugared_patterns,
                guard: clause.guard.clone(),
                body_block,
            });
        }

        // Pattern keys and sizes need the environment before any value binds.
        for (i, env_name) in free_vars.iter().enumerate() {
            self.bind_var(*env_name, params[i]);
        }
        let match_entry = self.with_pattern_inputs(self.collect_all_bindings(), |builder| {
            pattern::compile_match(builder, &user_params, &match_clauses, fail_block)
        });

        // Lower each clause body with env vars and pattern vars bound
        for (clause_idx, clause) in clauses.iter().enumerate() {
            let body_block = clause_body_blocks[clause_idx];
            self.push_scope();
            self.start_block(body_block);

            // Bind env vars in scope
            for (i, env_name) in free_vars.iter().enumerate() {
                self.bind_var(*env_name, params[i]);
            }

            // For named funs, bind the fun name to a self-referencing closure
            if let Some(fun_name) = name {
                let self_closure = self.fresh_var();
                let self_env: Vec<VarId> = (0..env_count as usize)
                    .map(|i| params[i])
                    .collect();
                self.emit(Instruction::MakeClosure(
                    self_closure,
                    lambda_mangled.clone(),
                    user_arity,
                    self_env,
                ));
                self.bind_var(fun_name, self_closure);
            }

            // Pre-bind pattern variables (extracts tuple elements, etc.)
            // Desugar record patterns before pre-binding.
            let desugared_patterns: Vec<_> = clause.patterns.iter()
                .map(|p| self.desugar_record_pattern(p))
                .collect::<Result<_, _>>()?;
            self.with_pattern_inputs(self.collect_all_bindings(), |builder| {
                for (pat, param) in desugared_patterns.iter().zip(user_params.iter()) {
                    builder.pre_bind_pattern_vars(pat, *param, false);
                }
            });

            let result = self.lower_body(&clause.body, true)?;
            if self.current_terminator.is_none() {
                self.set_terminator(Terminator::Return(result));
            }
            self.close_current_block();
            self.pop_scope();
        }

        // Build entry block
        let entry_block = self.fresh_block_with_id();
        self.start_block(entry_block);
        self.set_terminator(Terminator::Jump(match_entry, vec![]));
        self.close_current_block();

        // Package the lambda function
        let mut lambda_func = IrFunction {
            name: lambda_name_id,
            arity: total_arity,
            mangled_name: lambda_mangled.clone(),
            blocks: std::mem::take(&mut self.current_blocks),
            protected_values: std::mem::take(&mut self.protected_values),
            entry_block,
            params,
            line: 0,
        };
        #[cfg(test)]
        crate::passes::assert_valid(&lambda_func, "before helper yield insertion");
        insert_backedge_yield_checks(&mut lambda_func);
        #[cfg(test)]
        crate::passes::assert_valid(&lambda_func, "after helper yield insertion");
        self.functions.push(lambda_func);

        self.restore_state(saved);

        // In the original function, emit MakeClosure
        let mut env_var_ids = Vec::new();
        for env_name in &free_vars {
            let var_id = self.lookup_var(*env_name).unwrap();
            // Captures outlive this matching cursor and may cross a heap/GC
            // boundary. Capture an ordinary binary, rebinding for later uses.
            env_var_ids.push(self.materialize_bound_variable(*env_name, var_id));
        }

        let closure_var = self.fresh_var();
        self.emit(Instruction::MakeClosure(
            closure_var,
            lambda_mangled,
            user_arity,
            env_var_ids,
        ));

        Ok(closure_var)
    }

    /// Lower a function reference `fun name/arity` or `fun M:F/A`.
    pub(super) fn lower_expr_funref(
        &mut self,
        target: &ast::FunRefTarget,
    ) -> Result<VarId, LowerError> {
        match target {
            ast::FunRefTarget::Local(name, arity) => {
                let resolved = self.resolution.resolve(*name, *arity);
                if matches!(resolved, crate::resolution::Resolution::Import(_)) {
                    return Err(LowerError::Unsupported(
                        format!("creating a fun from imported name {}/{} is not allowed",
                            self.resolve_atom(*name).unwrap_or("?"), arity),
                        None,
                    ));
                }
                if crate::resolution::ResolutionPolicy::is_compiler_form(resolved, *name, *arity, &self.atom_names) {
                    return Err(LowerError::Unsupported("compiler-specific record test/info fun is not supported".into(), None));
                }
                if let Some(mangled) = self.module_functions.get(&(*name, *arity)).cloned() {
                    // Local function reference
                    let closure_var = self.fresh_var();
                    self.emit(Instruction::MakeClosure(
                        closure_var,
                        mangled,
                        *arity,
                        Vec::new(),
                    ));
                    Ok(closure_var)
                } else if resolved == crate::resolution::Resolution::AutoImport {
                    let mod_id = self.erlang_atom_id;
                    // Auto-imported BIF (e.g. fun abs/1 -> fun erlang:abs/1)
                    let closure_var = self.fresh_var();
                    self.emit(Instruction::MakeRemoteFunRef(
                        closure_var,
                        mod_id,
                        *name,
                        *arity,
                    ));
                    Ok(closure_var)
                } else {
                    let fun_name = self
                        .atom_names
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| format!("{:?}", name));
                    Err(LowerError::UndefinedVariable(
                        format!("fun {}/{}", fun_name, arity),
                        None,
                    ))
                }
            }
            ast::FunRefTarget::Remote(module, function, arity) => {
                if crate::resolution::ResolutionPolicy::is_compiler_form(
                    crate::resolution::Resolution::Import(*module), *function, *arity, &self.atom_names)
                {
                    return Err(LowerError::Unsupported("compiler-specific record test/info fun is not supported".into(), None));
                }
                // Remote fun ref: fun M:F/A
                // Creates a late-bound closure that dispatches via nera_rt_apply at call time
                let closure_var = self.fresh_var();
                self.emit(Instruction::MakeRemoteFunRef(
                    closure_var,
                    *module,
                    *function,
                    *arity,
                ));
                Ok(closure_var)
            }
            ast::FunRefTarget::DynamicRemote(mod_expr, fun_expr, arity_expr) => {
                let mod_var = self.lower_expr(mod_expr, false)?;
                let fun_var = self.lower_expr(fun_expr, false)?;
                let arity_var = self.lower_expr(arity_expr, false)?;
                let closure_var = self.fresh_var();
                self.emit(Instruction::MakeDynamicRemoteFunRef(
                    closure_var,
                    mod_var,
                    fun_var,
                    arity_var,
                ));
                Ok(closure_var)
            }
        }
    }

    // ---- Free variable analysis helpers ----
    // These delegate to the free functions in `warnings.rs` which contain
    // the actual AST walking logic (shared with the warning analyzer).

    /// Collect all variable names bound in a pattern.
    pub(super) fn collect_pattern_bindings(
        &self,
        pat: &ast::Pattern,
        bound: &mut std::collections::HashSet<AtomId>,
    ) {
        crate::warnings::collect_pattern_bindings(pat, bound);
    }

    /// Collect all variable references in an expression.
    pub(super) fn collect_expr_refs(
        &self,
        expr: &ast::Expr,
        refs: &mut std::collections::HashSet<AtomId>,
    ) {
        crate::warnings::collect_expr_refs(expr, refs);
    }

    /// Collect variable references in a guard.
    pub(super) fn collect_guard_refs(
        &self,
        guard: &ast::Guard,
        refs: &mut std::collections::HashSet<AtomId>,
    ) {
        crate::warnings::collect_guard_refs(guard, refs);
    }

}
