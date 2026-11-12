//! Expression lowering: the main lower_expr dispatcher and sub-expression handlers.

use nera_syntax::ast::{self, BinOp, BinSegmentKind};
use crate::ir::*;
use crate::pattern::{self, MatchClause};
use super::{IrBuilder, LowerError};

impl IrBuilder {
    /// Resolve a deferred binding before it escapes through a value or capture.
    /// The term is bound in the current scope. The tail's record is consumed,
    /// except while a pattern is lowered (see `matching`).
    pub(super) fn materialize_bound_variable(&mut self, name: ast::AtomId, var_id: VarId) -> VarId {
        let info = if self.matching {
            self.match_contexts.get(&var_id).cloned()
        } else {
            self.match_contexts.remove(&var_id)
        };
        if let Some(info) = info {
            let value = self.fresh_var();
            self.emit(Instruction::BsMaterialize(value, info.match_ctx,
                info.bit_offset, info.remaining_bits));
            self.bind_var(name, value);
            value
        } else { var_id }
    }

    /// Lower a single expression to IR, returning the VarId holding the result.
    pub fn lower_expr(
        &mut self,
        expr: &ast::Expr,
        is_tail: bool,
    ) -> Result<VarId, LowerError> {
        match expr {
            ast::Expr::Integer(val, _) => Ok(self.emit_const(Literal::Integer(*val))),
            ast::Expr::BigInteger(val, _) => Ok(self.emit_const(Literal::BigInteger(val.clone()))),

            ast::Expr::Float(val, _) => Ok(self.emit_const(Literal::Float(*val))),

            ast::Expr::Atom(id, _) => Ok(self.emit_const(Literal::Atom(*id))),

            ast::Expr::Nil(_) => Ok(self.emit_const(Literal::Nil)),

            ast::Expr::String(s, _) => {
                // Erlang strings are lists of Unicode codepoints, not UTF-8 bytes.
                let mut result = self.emit_const(Literal::Nil);
                for ch in s.chars().rev() {
                    let ch_var = self.emit_const(Literal::Integer(ch as i64));
                    let cons = self.fresh_var();
                    self.emit(Instruction::MakeCons(cons, ch_var, result));
                    result = cons;
                }
                Ok(result)
            }

            ast::Expr::Variable(name, span) => {
                let var_id = self.lookup_var(*name)
                    .ok_or_else(|| {
                        let var_name = self.atom_names.get(name)
                            .cloned()
                            .unwrap_or_else(|| format!("{:?}", name));
                        LowerError::UndefinedVariable(var_name, Some(*span))
                    })?;
                Ok(self.materialize_bound_variable(*name, var_id))
            }

            ast::Expr::Tuple(elems, _) => {
                let mut elem_vars = Vec::with_capacity(elems.len());
                for elem in elems {
                    elem_vars.push(self.lower_expr(elem, false)?);
                }
                let result = self.fresh_var();
                self.emit(Instruction::MakeTuple(result, elem_vars));
                Ok(result)
            }

            ast::Expr::Cons(head, tail, _) => {
                let head_var = self.lower_expr(head, false)?;
                let tail_var = self.lower_expr(tail, false)?;
                let result = self.fresh_var();
                self.emit(Instruction::MakeCons(result, head_var, tail_var));
                Ok(result)
            }

            ast::Expr::BinOp(op, lhs, rhs, _) => {
                // `andalso` and `orelse` must short-circuit: the right-hand
                // side is evaluated only when the left-hand side's truth
                // value requires it. Emit control flow to achieve that.
                match op {
                    BinOp::Andalso | BinOp::Orelse => {
                        let lhs_var = self.lower_expr(lhs, false)?;
                        let result = self.fresh_var();

                        let true_atom = self.emit_const(Literal::Atom(self.true_atom_id));
                        let is_true = self.fresh_var();
                        self.emit(Instruction::BinOp(is_true, BinOp::ExactEq, lhs_var, true_atom));

                        let eval_rhs_block = self.fresh_block_with_id();
                        let skip_block = self.fresh_block_with_id();
                        let merge_block = self.fresh_block_with_id();

                        // andalso: true → eval rhs, else → lhs is the result (false)
                        // orelse:  true → lhs is the result (true), else → eval rhs
                        let (true_target, false_target) = match op {
                            BinOp::Andalso => (eval_rhs_block, skip_block),
                            BinOp::Orelse => (skip_block, eval_rhs_block),
                            _ => unreachable!(),
                        };
                        self.set_terminator(Terminator::Branch(is_true, true_target, false_target));
                        self.close_current_block();

                        // eval_rhs: evaluate rhs and use it as result
                        self.start_block(eval_rhs_block);
                        let branch = self.enter_branch();
                        let rhs_var = self.lower_expr(rhs, false)?;
                        self.emit(Instruction::Move(result, rhs_var));
                        self.set_terminator(Terminator::Jump(merge_block, vec![]));
                        self.close_current_block();
                        self.leave_branch(&branch);

                        // skip: short-circuit — result is lhs (false for andalso, true for orelse)
                        self.start_block(skip_block);
                        self.emit(Instruction::Move(result, lhs_var));
                        self.set_terminator(Terminator::Jump(merge_block, vec![]));
                        self.close_current_block();

                        self.start_block(merge_block);
                        Ok(result)
                    }
                    _ => {
                        let lhs_var = self.lower_expr(lhs, false)?;
                        let rhs_var = self.lower_expr(rhs, false)?;
                        let result = self.fresh_var();
                        self.emit(Instruction::BinOp(result, *op, lhs_var, rhs_var));
                        if matches!(op, BinOp::ListAppend | BinOp::ListSubtract) {
                            self.branch_on_construction_exception();
                        }
                        Ok(result)
                    }
                }
            }

            ast::Expr::UnaryOp(op, operand, _) => {
                let operand_var = self.lower_expr(operand, false)?;
                let result = self.fresh_var();
                self.emit(Instruction::UnaryOp(result, *op, operand_var));
                Ok(result)
            }

            ast::Expr::Match(pattern, expr, _) => self.lower_expr_match(pattern, expr),

            ast::Expr::Call(target, args, _) => {
                let arity = args.len() as u32;
                let (resolved, function) = match target {
                    ast::CallTarget::Local(name, _) => (self.resolution.resolve(*name, arity), Some(*name)),
                    ast::CallTarget::Remote(m, f) => match (m.as_ref(), f.as_ref()) {
                        (ast::Expr::Atom(module, _), ast::Expr::Atom(name, _)) =>
                            (crate::resolution::Resolution::Import(*module), Some(*name)),
                        _ => (crate::resolution::Resolution::Undefined, None),
                    },
                    _ => (crate::resolution::Resolution::Undefined, None),
                };
                let compiler_form = function.is_some_and(|name|
                    crate::resolution::ResolutionPolicy::is_compiler_form(resolved, name, arity, &self.atom_names));
                if compiler_form {
                    return Err(LowerError::Unsupported("compiler-specific record test/info form is not supported".into(), Some(expr.span())));
                }

                // Preserve exception lowering, but only after resolving the name.
                if let ast::CallTarget::Local(name, _) = target {
                    if args.len() == 1 && self.resolve_atom(*name) == Some("throw")
                        && (resolved == crate::resolution::Resolution::AutoImport
                            || resolved == crate::resolution::Resolution::Import(self.erlang_atom_id))
                    {
                        return self.lower_throw(&args[0]);
                    }
                }

                // Match context optimization: byte_size(X) where X is a deferred
                // rest segment → round remaining bits up to bytes without materializing.
                // Must be checked BEFORE arguments are lowered (which would materialize).
                if let ast::CallTarget::Local(name, _) = target {
                    if resolved == crate::resolution::Resolution::AutoImport && args.len() == 1 {
                        let fn_name = self.atom_names.get(name).map(|s| s.as_str());
                        if matches!(fn_name, Some("byte_size")) {
                            // Check if the argument is a simple variable in match_contexts.
                            // byte_size counts a partial final byte, including /bits
                            // and /binary-unit:N tails whose unit is not byte-sized.
                            if let ast::Expr::Variable(arg_name, _) = &args[0] {
                                if let Some(arg_var) = self.lookup_var(*arg_name) {
                                    let mc_remaining = self.match_contexts.get(&arg_var).map(|c| c.remaining_bits);
                                    if let Some(remaining_bits) = mc_remaining {
                                        let seven = self.emit_const(Literal::Integer(7));
                                        let rounded_bits = self.fresh_var();
                                        self.emit(Instruction::BinOp(rounded_bits, BinOp::Add, remaining_bits, seven));
                                        let eight = self.emit_const(Literal::Integer(8));
                                        let result = self.fresh_var();
                                        self.emit(Instruction::BinOp(result, BinOp::IntDiv, rounded_bits, eight));
                                        return Ok(result);
                                    }
                                }
                            }
                        }
                        if matches!(fn_name, Some("bit_size")) {
                            // Match context optimization: bit_size(X) where X is deferred
                            // → return remaining_bits directly (already in bits).
                            if let ast::Expr::Variable(arg_name, _) = &args[0] {
                                if let Some(arg_var) = self.lookup_var(*arg_name) {
                                    let mc_remaining = self.match_contexts.get(&arg_var).map(|c| c.remaining_bits);
                                    if let Some(remaining_bits) = mc_remaining {
                                        return Ok(remaining_bits);
                                    }
                                }
                            }
                        }
                    }
                }

                // OTP evaluates the remote module, function (or fun callee)
                // before the arguments. Preserve their effects and first error.
                let ir_target = match target {
                    ast::CallTarget::Local(name, _) => match resolved {
                        crate::resolution::Resolution::Import(module) => CallTarget::Remote(module, *name),
                        crate::resolution::Resolution::AutoImport => CallTarget::Remote(self.erlang_atom_id, *name),
                        _ => CallTarget::Local(*name),
                    },
                    ast::CallTarget::Remote(module_expr, fun_expr) => {
                        // Static remote call: atom:atom(args)
                        if let (ast::Expr::Atom(mod_atom, _), ast::Expr::Atom(fun_atom, _)) =
                            (module_expr.as_ref(), fun_expr.as_ref())
                        {
                            CallTarget::Remote(*mod_atom, *fun_atom)
                        } else {
                            // Dynamic remote call: lower module and function exprs,
                            // desugar to Apply (dispatched via nera_rt_apply at runtime)
                            let mod_var = self.lower_expr(module_expr, false)?;
                            let fun_var = self.lower_expr(fun_expr, false)?;
                            CallTarget::Apply(mod_var, fun_var)
                        }
                    }
                    ast::CallTarget::Variable(var_name, _) => {
                        let closure_var = self.lookup_var(*var_name)
                            .ok_or_else(|| {
                                let name = self.atom_names.get(var_name)
                                    .cloned()
                                    .unwrap_or_else(|| format!("{:?}", var_name));
                                LowerError::UndefinedVariable(name, Some(expr.span()))
                            })?;
                        CallTarget::Closure(closure_var)
                    }
                    ast::CallTarget::Expr(callee) => {
                        // Callee is an arbitrary expression that should evaluate to a fun.
                        let closure_var = self.lower_expr(callee, false)?;
                        CallTarget::Closure(closure_var)
                    }
                };

                // A cursor may only be transferred to one argument. Other
                // arguments (including nested expressions) must not observe a
                // cursor after it has advanced. Count uses by context identity,
                // so distinct deferred bindings sharing a cursor are covered.
                let mut context_uses = std::collections::HashMap::new();
                if is_tail && !self.match_contexts.is_empty() {
                    for arg in args {
                        let mut refs = std::collections::HashSet::new();
                        self.collect_expr_refs(arg, &mut refs);
                        for name in refs {
                            if let Some(info) = self.lookup_var(name)
                                .and_then(|id| self.match_contexts.get(&id)) {
                                *context_uses.entry(info.match_ctx).or_insert(0_usize) += 1;
                            }
                        }
                    }
                }

                let mut arg_vars = Vec::with_capacity(args.len());
                for (index, arg) in args.iter().enumerate() {
                    // A tail call prevents later cursor use in the caller, but the
                    // callee must also consume it through binary patterns in every
                    // clause. A variable fallback, BIF or opaque callee can expose
                    // the internal cursor as an Erlang value instead of a binary.
                    let accepts_match_context = match target {
                        ast::CallTarget::Local(name, _) => self.binary_match_args
                            .get(&(*name, args.len() as u32))
                            .and_then(|positions| positions.get(index))
                            .copied().unwrap_or(false),
                        _ => false,
                    };
                    if is_tail && accepts_match_context {
                        if let ast::Expr::Variable(var_name, _) = arg {
                            if let Some(var_id) = self.lookup_var(*var_name) {
                                let exclusive = self.match_contexts.get(&var_id)
                                    .is_some_and(|info| context_uses.get(&info.match_ctx) == Some(&1));
                                if exclusive {
                                    let ctx_info = self.match_contexts.remove(&var_id).unwrap();
                                    self.emit(Instruction::BsMatchCtxAdvance(
                                        ctx_info.match_ctx,
                                        ctx_info.bit_offset,
                                    ));
                                    arg_vars.push(ctx_info.match_ctx);
                                    continue;
                                }
                            }
                        }
                    }
                    arg_vars.push(self.lower_expr(arg, false)?);
                }

                if let ast::CallTarget::Local(name, _) = target {
                    if resolved == crate::resolution::Resolution::AutoImport {
                        // Check if this is spawn/3 or spawn_link/3
                        // (NOT spawn_monitor/3 which returns {Pid, Ref})
                        if args.len() == 3 {
                            let fn_name = self.atom_names.get(name).map(|s| s.as_str());
                            let is_spawn_like = matches!(fn_name, Some("spawn") | Some("spawn_link"));
                            if is_spawn_like {
                                if let (ast::Expr::Atom(mod_atom, _), ast::Expr::Atom(fun_atom, _)) =
                                    (&args[0], &args[1])
                                {
                                    let result = self.fresh_var();
                                    self.emit(Instruction::Spawn(
                                        result,
                                        *mod_atom,
                                        *fun_atom,
                                        vec![arg_vars[2]],
                                    ));
                                    return Ok(result);
                                }
                            }
                        }

                        // Check if this is self/0
                        if args.is_empty() && *name == self.self_atom_id {
                            let result = self.fresh_var();
                            self.emit(Instruction::GetSelf(result));
                            return Ok(result);
                        }
                    }
                }

                // Tail call optimization is only safe when there is no active
                // try/catch handler on the call stack. Inside a try body,
                // a TailCall would bypass the exception handler.
                let can_tco = is_tail && self.try_exception_targets.is_empty();
                if can_tco {
                    self.emit_set_line(expr.span());
                    self.set_terminator(Terminator::TailCall(ir_target, arg_vars));
                    // Return a dummy var (terminator handles the actual return)
                    Ok(self.fresh_var())
                } else {
                    self.emit_set_line(expr.span());
                    // No CheckReductions here — the callee already decrements
                    // at its own function entry. Avoids double-counting.
                    let result = self.fresh_var();
                    self.emit(Instruction::Call(result, ir_target, arg_vars));
                    if let Some(&landing_pad) = self.try_exception_targets.last() {
                        // Do not run later arguments or expressions after a failed call.
                        let failed = self.fresh_var();
                        self.emit(Instruction::CheckException(failed));
                        let next = self.fresh_block_with_id();
                        self.set_terminator(Terminator::Branch(failed, landing_pad, next));
                        self.close_current_block();
                        self.start_block(next);
                    }
                    Ok(result)
                }
            }

            ast::Expr::Send(pid_expr, msg_expr, _) => {
                let pid = self.lower_expr(pid_expr, false)?;
                let msg = self.lower_expr(msg_expr, false)?;
                let result = self.fresh_var();
                self.emit(Instruction::Send(result, pid, msg));
                Ok(result)
            }

            ast::Expr::Case(scrutinee_expr, clauses, _) => {
                self.lower_expr_case(expr, scrutinee_expr, clauses, is_tail)
            }

            ast::Expr::Maybe { body, else_clauses, span } => {
                self.lower_maybe(body, else_clauses, *span, is_tail)
            }

            ast::Expr::If(clauses, _) => self.lower_expr_if(expr, clauses, is_tail),

            ast::Expr::Receive(clauses, after, _) => {
                self.lower_expr_receive(expr, clauses, after.as_ref(), is_tail)
            }

            // `begin ... end` is not a scope: its bindings stay for later code.
            ast::Expr::Block(exprs, _) => self.lower_body(exprs, is_tail),

            ast::Expr::Binary(segments, _) => self.lower_expr_binary(segments),

            ast::Expr::Map(fields, _span) => {
                // Allocate a map: lower each key-value pair, then call MakeMap
                let mut key_val_vars = Vec::new();
                for field in fields {
                    let k = self.lower_expr(&field.key, false)?;
                    let v = self.lower_expr(&field.value, false)?;
                    key_val_vars.push((k, v));
                }
                let result = self.fresh_var();
                let flat: Vec<VarId> = key_val_vars.iter()
                    .flat_map(|(k, v)| vec![*k, *v])
                    .collect();
                self.emit(Instruction::MakeMap(result, flat));
                Ok(result)
            }

            ast::Expr::MapUpdate(base_expr, fields, _span) => {
                // Lower base map, then apply each field.
                // `=>` (Assoc) fields use MapPut (insert or replace).
                // `:=` (Exact) fields use MapUpdateExact (key must exist).
                let mut current_map = self.lower_expr(base_expr, false)?;

                for field in fields {
                    let k = self.lower_expr(&field.key, false)?;
                    let v = self.lower_expr(&field.value, false)?;
                    let new_map = self.fresh_var();
                    match field.op {
                        ast::MapUpdateOp::Assoc => {
                            self.emit(Instruction::MapPut(new_map, current_map, k, v));
                        }
                        ast::MapUpdateOp::Exact => {
                            self.emit(Instruction::MapUpdateExact(new_map, current_map, k, v));
                        }
                    }
                    current_map = new_map;
                }

                Ok(current_map)
            }

            // ── List Comprehension ──
            ast::Expr::ListComp(body_expr, qualifiers, _span) => {
                self.lower_comprehension(body_expr, qualifiers, false)
            }

            // ── Binary Comprehension ──
            ast::Expr::BinComp(body_expr, qualifiers, _span) => {
                self.lower_comprehension(body_expr, qualifiers, true)
            }

            // ── Try/Catch ──
            ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
                self.lower_try(body, of_clauses, catch_clauses, after_body)
            }

            // ── Throw ──
            ast::Expr::Throw(reason_expr, _span) => {
                self.lower_throw(reason_expr)
            }

            // ── Catch (unary) ──
            // Desugar: catch Expr →
            //   try Expr catch
            //     throw:V -> V;
            //     exit:V  -> {'EXIT', V};
            //     error:V -> {'EXIT', {V, []}}
            //   end
            ast::Expr::Catch(operand, span) => {
                let val_id = self.atom_id_for("__catch_val__");
                let exit_id = self.atom_id_for("EXIT");
                let s = *span;
                let val_pat = || ast::Pattern::Variable(val_id, s);
                let val_var = || ast::Expr::Variable(val_id, s);
                let exit_atom = || ast::Expr::Atom(exit_id, s);

                let catch_clauses = vec![
                    ast::CatchClause {
                        class: ast::CatchClass::Throw,
                        pattern: val_pat(),
                        stacktrace: None,
                        guard: None,
                        body: vec![val_var()],
                        span: s,
                    },
                    ast::CatchClause {
                        class: ast::CatchClass::Exit,
                        pattern: val_pat(),
                        stacktrace: None,
                        guard: None,
                        body: vec![ast::Expr::Tuple(vec![
                            exit_atom(),
                            val_var(),
                        ], s)],
                        span: s,
                    },
                    ast::CatchClause {
                        class: ast::CatchClass::Error,
                        pattern: val_pat(),
                        stacktrace: None,
                        guard: None,
                        body: vec![ast::Expr::Tuple(vec![
                            exit_atom(),
                            ast::Expr::Tuple(vec![val_var(), ast::Expr::Nil(s)], s),
                        ], s)],
                        span: s,
                    },
                ];

                // The operand itself, not a copy: branch exports find it by address.
                self.lower_try(std::slice::from_ref(operand.as_ref()), &[], &catch_clauses, &None)
            }

            // ── Anonymous fun ──
            ast::Expr::Fun(name, clauses, _span) => {
                self.lower_expr_fun(*name, clauses)
            }

            // ── Function reference ──
            ast::Expr::FunRef(target, _span) => {
                self.lower_expr_funref(target)
            }

            // ── Record expressions ──
            ast::Expr::RecordCreate { record_name, fields, span } => {
                self.lower_record_create(*record_name, fields, *span)
            }
            ast::Expr::RecordAccess { base, record_name, field_name, span, .. } => {
                self.lower_record_access(base, *record_name, *field_name, *span)
            }
            ast::Expr::RecordIndex { record_name, field_name, span, .. } => {
                self.lower_record_index(*record_name, *field_name, *span)
            }
            ast::Expr::RecordUpdate { base, record_name, fields, span } => {
                self.lower_record_update(base, *record_name, fields, *span)
            }
            ast::Expr::MacroCall { .. } => unreachable!("MacroCall only in formatter path"),
        }
    }

    // ---- Extracted sub-methods for lower_expr ----

    /// Lower a `=` match expression with runtime pattern validation.
    /// Simple variable/wildcard patterns bind directly; complex patterns
    /// use the pattern compiler to validate at runtime, raising `{badmatch, Value}` on failure.
    pub(super) fn lower_expr_match(
        &mut self,
        pattern: &ast::Pattern,
        expr: &ast::Expr,
    ) -> Result<VarId, LowerError> {
        let val = self.lower_expr(expr, false)?;

        // Desugar record patterns before matching
        let pattern = self.desugar_record_pattern(pattern)?;

        // For wildcard or a *fresh* variable, bind directly. If the variable
        // is already bound in scope, fall through to the pattern compiler so
        // it emits an equality check (Erlang's "pinned variable" semantics —
        // rebinding to a non-equal value raises {badmatch, _}).
        let is_fresh_var = match &pattern {
            ast::Pattern::Variable(name, _) => self.lookup_var(*name).is_none(),
            ast::Pattern::Wildcard(_) => true,
            _ => false,
        };
        if is_fresh_var {
            self.bind_pattern(&pattern, val)?;
            return Ok(val);
        }

        // For complex patterns, use the pattern compiler to validate
        // the value matches the pattern structure at runtime.
        let dispatch_block = self.current_block_id.unwrap();
        self.close_current_block();

        // Capture outer bindings BEFORE pre_bind_pattern_vars overwrites them.
        let outer_bindings = self.collect_all_bindings();

        let body_block = self.fresh_block_with_id();
        let cont_block = self.fresh_block_with_id();

        self.start_block(body_block);
        self.pre_bind_pattern_vars(&pattern, val, true);
        self.keep_outer_bindings(&outer_bindings);
        self.set_terminator(Terminator::Jump(cont_block, vec![]));
        self.close_current_block();

        let match_clause = pattern::MatchClause {
            patterns: vec![pattern.clone()],
            guard: None,
            body_block,
        };

        // Fail block: raise {badmatch, Value}
        let fail_block = self.fresh_block_with_id();
        self.start_block(fail_block);
        self.emit_set_line(expr.span());
        let bm_atom = self.atom_id_for("badmatch");
        let reason_atom = self.emit_const(Literal::Atom(bm_atom));
        let reason_tuple = self.fresh_var();
        self.emit(Instruction::MakeTuple(reason_tuple, vec![reason_atom, val]));
        let exc_result = self.fresh_var();
        self.emit(Instruction::RaiseException(exc_result, ExceptionClass::Error, reason_tuple));
        self.set_raise_terminator(exc_result);
        self.close_current_block();

        let match_entry = pattern::compile_match_with_outer_bindings(self, &[val], &[match_clause], fail_block, &outer_bindings);
        self.patch_block_terminator(dispatch_block, Terminator::Jump(match_entry, vec![]));

        self.start_block(cont_block);
        Ok(val)
    }

    /// Lower a case expression with pattern matching. `node` is the whole
    /// expression, which decides the names it exports.
    pub(super) fn lower_expr_case(
        &mut self,
        node: &ast::Expr,
        scrutinee_expr: &ast::Expr,
        clauses: &[ast::CaseClause],
        is_tail: bool,
    ) -> Result<VarId, LowerError> {
        let scrutinee = self.lower_expr(scrutinee_expr, false)?;

        let join_block = self.fresh_block_with_id();
        let result_var = self.fresh_var();

        let case_dispatch_block = self.current_block_id.unwrap();
        self.close_current_block();

        // Deferred binary tails are path-local, just like variable bindings.
        // Materializing/advancing a tail in one arm must not consume its
        // definition in sibling arms, or at the join after a non-tail case.
        let outer_match_contexts = self.match_contexts.clone();
        let before = self.collect_all_bindings();
        let mut exits = Vec::new();
        let mut match_clauses = Vec::new();
        for clause in clauses {
            self.match_contexts = outer_match_contexts.clone();
            let body_block = self.fresh_block_with_id();

            // Desugar record patterns in case clauses
            let desugared_pat = self.desugar_record_pattern(&clause.pattern)?;

            self.push_scope();
            self.start_block(body_block);
            self.pre_bind_pattern_vars(&desugared_pat, scrutinee, false);
            let body_result = self.lower_body(&clause.body, is_tail)?;
            if !is_tail {
                self.emit(Instruction::Move(result_var, body_result));
                self.set_terminator(Terminator::Jump(join_block, vec![]));
                exits.extend(self.branch_exit(&before));
            } else if self.current_terminator.is_none() {
                self.set_terminator(Terminator::Return(body_result));
            }
            self.close_current_block();
            self.pop_scope();

            match_clauses.push(MatchClause {
                patterns: vec![desugared_pat],
                guard: clause.guard.clone(),
                body_block,
            });
        }
        self.match_contexts = outer_match_contexts;

        // Fail block: raise case_clause exception
        let fail_block = self.fresh_block_with_id();
        self.start_block(fail_block);
        self.emit_set_line(scrutinee_expr.span());
        let cc_atom = self.atom_id_for("case_clause");
        let reason_atom = self.emit_const(Literal::Atom(cc_atom));
        let reason = self.fresh_var();
        self.emit(Instruction::MakeTuple(reason, vec![reason_atom, scrutinee]));
        let exc_result = self.fresh_var();
        self.emit(Instruction::RaiseException(exc_result, ExceptionClass::Error, reason));
        self.set_raise_terminator(exc_result);
        self.close_current_block();

        let outer_bindings = self.collect_all_bindings();
        let match_entry = pattern::compile_match_with_outer_bindings(self, &[scrutinee], &match_clauses, fail_block, &outer_bindings);
        self.patch_block_terminator(case_dispatch_block, Terminator::Jump(match_entry, vec![]));

        if !is_tail {
            self.start_block(join_block);
            self.export_branch_bindings(&exits, node);
        }
        Ok(result_var)
    }

    /// Lower an if expression (guard-based branching). `node` is the whole
    /// expression, which decides the names it exports.
    pub(super) fn lower_expr_if(
        &mut self,
        node: &ast::Expr,
        clauses: &[ast::IfClause],
        is_tail: bool,
    ) -> Result<VarId, LowerError> {
        let join_block = self.fresh_block_with_id();
        let result_var = self.fresh_var();
        let fail_block = self.fresh_block_with_id();
        let branch = self.enter_branch();
        let before = self.collect_all_bindings();
        let mut exits = Vec::new();

        for (i, clause) in clauses.iter().enumerate() {
            let guard_val = self.lower_guard(&clause.guard);
            let true_atom = self.emit_const(Literal::Atom(self.true_atom_id));
            let cmp = self.fresh_var();
            self.emit(Instruction::BinOp(cmp, BinOp::ExactEq, guard_val, true_atom));

            let then_block = self.fresh_block_with_id();
            let else_block = if i + 1 < clauses.len() {
                self.fresh_block_with_id()
            } else {
                fail_block
            };

            self.set_terminator(Terminator::Branch(cmp, then_block, else_block));
            self.close_current_block();

            self.push_scope();
            self.start_block(then_block);
            let body_result = self.lower_body(&clause.body, is_tail)?;
            if !is_tail {
                self.emit(Instruction::Move(result_var, body_result));
                self.set_terminator(Terminator::Jump(join_block, vec![]));
                exits.extend(self.branch_exit(&before));
            } else if self.current_terminator.is_none() {
                self.set_terminator(Terminator::Return(body_result));
            }
            self.close_current_block();
            self.pop_scope();
            // The next guard and the join do not run this clause.
            self.leave_branch(&branch);

            if i + 1 < clauses.len() {
                self.start_block(else_block);
            }
        }

        // Populate the fail block: raise if_clause exception.
        self.start_block(fail_block);
        if let Some(first_clause) = clauses.first() {
            self.emit_set_line(first_clause.span);
        }
        let ic_atom = self.atom_id_for("if_clause");
        let reason = self.emit_const(Literal::Atom(ic_atom));
        let exc_result = self.fresh_var();
        self.emit(Instruction::RaiseException(exc_result, ExceptionClass::Error, reason));
        self.set_raise_terminator(exc_result);
        self.close_current_block();

        if !is_tail {
            self.start_block(join_block);
            self.export_branch_bindings(&exits, node);
        }
        Ok(result_var)
    }

    /// Lower a receive expression with selective receive semantics.
    ///
    /// Uses a three-BIF protocol (peek/next/accept) to scan through the
    /// mailbox without removing non-matching messages. When the scan ends,
    /// peek and next return a sentinel that is not a term, which
    /// `receive_has_message` tests: `[]` is a message like any other.
    ///
    /// ```text
    /// pre_block:
    ///   [receive_set_timeout(timeout)]   // only on first entry (before block split)
    ///   msg = receive_peek()             // SUSPEND POINT — reset scan, first candidate
    ///   found = receive_has_message(msg)
    ///   Branch(found, match_trampoline, empty_block)
    ///
    /// match_trampoline → match_entry (decision tree):
    ///   match → body_block: receive_accept() + body
    ///   fail  → fail_block
    ///
    /// fail_block (scan loop):
    ///   next = receive_next()            // advance scan, next candidate
    ///   Move(msg_var, next)              // update scrutinee for re-entry
    ///   found = receive_has_message(next)
    ///   Branch(found, match_trampoline, empty_block)
    ///
    /// empty_block:
    ///   [check timeout → after_block | wait_block]
    /// ```
    ///
    /// An `after` value that is not a valid timeout raises `timeout_value`
    /// from the timeout check in `empty_block`, as on BEAM: a matching
    /// message already in the mailbox is received, and the receive raises only
    /// when it would have to wait.
    pub(super) fn lower_expr_receive(
        &mut self,
        node: &ast::Expr,
        clauses: &[ast::CaseClause],
        after: Option<&ast::AfterClause>,
        is_tail: bool,
    ) -> Result<VarId, LowerError> {
        let join_block = self.fresh_block_with_id();
        let result_var = self.fresh_var();

        // If there's an after clause, evaluate timeout and set deadline
        // BEFORE receive_peek so it's only executed on first entry (not on resume).
        if let Some(after_clause) = after {
            let timeout_var = self.lower_expr(&after_clause.timeout, false)?;
            // Inside try, an operation that raised earlier, in the timeout
            // expression or before it, reaches the handler here, before the
            // scan can receive a message or the timeout can be judged.
            if let Some(&landing_pad) = self.try_exception_targets.last() {
                let failed = self.fresh_var();
                self.emit(Instruction::CheckException(failed));
                let next = self.fresh_block_with_id();
                self.set_terminator(Terminator::Branch(failed, landing_pad, next));
                self.close_current_block();
                self.start_block(next);
            }
            let _set_result = self.fresh_var();
            self.emit(Instruction::Call(
                _set_result,
                CallTarget::Bif("receive_set_timeout".to_string()),
                vec![timeout_var],
            ));
        }

        // Peek at the first message without removing it.
        // This is the suspend point — codegen splits the block here.
        let msg_var = self.fresh_var();
        self.emit(Instruction::Call(
            msg_var,
            CallTarget::Bif("receive_peek".to_string()),
            vec![],
        ));

        // Did the scan find a message? An empty mailbox returns the
        // no-message sentinel, never `[]`, which is a valid message.
        let found = self.fresh_var();
        self.emit(Instruction::Call(
            found,
            CallTarget::Bif("receive_has_message".to_string()),
            vec![msg_var],
        ));

        let match_trampoline = self.fresh_block_with_id();
        let empty_block = self.fresh_block_with_id();
        self.set_terminator(Terminator::Branch(found, match_trampoline, empty_block));

        // Deferred binary tails are path-local, as in `case`: the after body,
        // each clause and the patterns (compiled after the bodies) all start
        // from these records.
        let outer_match_contexts = self.match_contexts.clone();
        let before = self.collect_all_bindings();
        let mut exits = Vec::new();

        // --- After body block (shared target for timeout paths) ---
        let after_block = if let Some(after_clause) = after {
            let ab = self.fresh_block_with_id();
            // Like a clause body, the after body binds in its own scope.
            self.push_scope();
            self.start_block(ab);
            let after_result = self.lower_body(&after_clause.body, is_tail)?;
            if !is_tail {
                self.emit(Instruction::Move(result_var, after_result));
                self.set_terminator(Terminator::Jump(join_block, vec![]));
                exits.extend(self.branch_exit(&before));
            } else if self.current_terminator.is_none() {
                self.set_terminator(Terminator::Return(after_result));
            }
            self.close_current_block();
            self.pop_scope();
            Some(ab)
        } else {
            None
        };

        // --- Empty block: no messages (or all scanned, none matched) ---
        self.start_block(empty_block);
        if let Some(ab) = after_block {
            // Raises timeout_value for an invalid after value: nothing
            // matched, so the receive would wait.
            let timed_out = self.fresh_var();
            self.emit(Instruction::Call(
                timed_out,
                CallTarget::Bif("receive_check_timeout".to_string()),
                vec![],
            ));
            if let Some(&landing_pad) = self.try_exception_targets.last() {
                let failed = self.fresh_var();
                self.emit(Instruction::CheckException(failed));
                let next = self.fresh_block_with_id();
                self.set_terminator(Terminator::Branch(failed, landing_pad, next));
                self.close_current_block();
                self.start_block(next);
            }
            let wait_block = self.fresh_block_with_id();
            self.set_terminator(Terminator::Branch(timed_out, ab, wait_block));
            self.close_current_block();

            self.start_block(wait_block);
            let wait_result = self.fresh_var();
            self.emit(Instruction::Call(
                wait_result,
                CallTarget::Bif("receive_wait".to_string()),
                vec![],
            ));
            self.set_terminator(Terminator::Return(wait_result));
            self.close_current_block();
        } else {
            let wait_result = self.fresh_var();
            self.emit(Instruction::Call(
                wait_result,
                CallTarget::Bif("receive_wait".to_string()),
                vec![],
            ));
            self.set_terminator(Terminator::Return(wait_result));
            self.close_current_block();
        }

        // --- Build match clauses ---
        // Each body block calls receive_accept() to remove the matched message
        // before executing the clause body.
        let mut match_clauses = Vec::new();
        for clause in clauses {
            self.match_contexts = outer_match_contexts.clone();
            let body_block = self.fresh_block_with_id();

            let desugared_pat = self.desugar_record_pattern(&clause.pattern)?;

            self.push_scope();
            self.start_block(body_block);

            // Accept (remove) the matched message from the mailbox.
            let _accept_result = self.fresh_var();
            self.emit(Instruction::Call(
                _accept_result,
                CallTarget::Bif("receive_accept".to_string()),
                vec![],
            ));

            self.pre_bind_pattern_vars(&desugared_pat, msg_var, false);
            let body_result = self.lower_body(&clause.body, is_tail)?;
            if !is_tail {
                self.emit(Instruction::Move(result_var, body_result));
                self.set_terminator(Terminator::Jump(join_block, vec![]));
                exits.extend(self.branch_exit(&before));
            } else if self.current_terminator.is_none() {
                self.set_terminator(Terminator::Return(body_result));
            }
            self.close_current_block();
            self.pop_scope();

            match_clauses.push(MatchClause {
                patterns: vec![desugared_pat],
                guard: clause.guard.clone(),
                body_block,
            });
        }
        self.match_contexts = outer_match_contexts;

        // --- Fail block: no clause matched this message → scan next ---
        let fail_block = self.fresh_block_with_id();
        self.start_block(fail_block);
        // Advance to the next message in the mailbox (no removal).
        let next_msg = self.fresh_var();
        self.emit(Instruction::Call(
            next_msg,
            CallTarget::Bif("receive_next".to_string()),
            vec![],
        ));
        // Update msg_var so the decision tree re-reads the new message
        // on the next iteration. Cranelift's Variable system handles the
        // resulting PHI at the match_trampoline entry.
        self.emit(Instruction::Move(msg_var, next_msg));
        let found_next = self.fresh_var();
        self.emit(Instruction::Call(
            found_next,
            CallTarget::Bif("receive_has_message".to_string()),
            vec![next_msg],
        ));
        // If exhausted → empty_block (wait or timeout).
        // If not → loop back to match_trampoline for another round.
        self.set_terminator(Terminator::Branch(found_next, match_trampoline, empty_block));
        self.close_current_block();

        // Compile the pattern match decision tree (Maranget algorithm).
        // The fail_block is the target when no clause matches the current message.
        let outer_bindings = self.collect_all_bindings();
        let match_entry = pattern::compile_match_with_outer_bindings(self, &[msg_var], &match_clauses, fail_block, &outer_bindings);

        // match_trampoline: jump to the decision tree entry.
        // Entered from both the initial peek nil-check and the fail_block loop-back.
        self.start_block(match_trampoline);
        self.set_terminator(Terminator::Jump(match_entry, vec![]));
        self.close_current_block();

        if !is_tail {
            self.start_block(join_block);
            self.export_branch_bindings(&exits, node);
        }
        Ok(result_var)
    }

    /// Compute the effective unit (in bits per size unit) for a binary segment type.
    /// In BEAM: integer defaults to unit:1 (bits), binary defaults to unit:8 (bytes),
    /// float defaults to unit:1 (bits). Explicit `unit:N` overrides.
    pub(crate) fn effective_unit(spec: &ast::BinSegmentType) -> i64 {
        if let Some(u) = spec.unit {
            return u as i64;
        }
        match spec.typ {
            BinSegmentKind::Integer | BinSegmentKind::Float => 1,
            BinSegmentKind::Binary => 8,
            BinSegmentKind::Bitstring => 1,
            _ => 1,
        }
    }

    /// Stop construction immediately when an operation fails inside try/catch.
    fn branch_on_construction_exception(&mut self) {
        if let Some(&landing_pad) = self.try_exception_targets.last() {
            let failed = self.fresh_var();
            self.emit(Instruction::CheckException(failed));
            let next = self.fresh_block_with_id();
            self.set_terminator(Terminator::Branch(failed, landing_pad, next));
            self.close_current_block();
            self.start_block(next);
        }
    }

    /// Lower a segment value or size. Inside try/catch a failed expression,
    /// such as `1 div 0`, leaves its exception pending; branch to the handler
    /// before a segment check can replace it with badarg.
    fn lower_construction_operand(&mut self, expr: &ast::Expr) -> Result<VarId, LowerError> {
        let var = self.lower_expr(expr, false)?;
        if !matches!(expr, ast::Expr::Integer(..) | ast::Expr::BigInteger(..) | ast::Expr::Float(..)
            | ast::Expr::Atom(..) | ast::Expr::String(..) | ast::Expr::Nil(..) | ast::Expr::Variable(..)) {
            self.branch_on_construction_exception();
        }
        Ok(var)
    }

    /// Lower binary construction to IR.
    /// Evaluates segments, computes total size, allocates binary, writes segments.
    /// All offsets and sizes are tracked in BITS throughout.
    pub(super) fn lower_expr_binary(
        &mut self,
        segments: &[ast::BinSegment<ast::Expr>],
    ) -> Result<VarId, LowerError> {
        // Phase 1a: evaluate every segment value and size, left to right and
        // value before size, before checking any segment. BEAM only builds
        // once all of them ran, so <<X:Neg, (1 div Z)>> raises badarith.
        // A string is one segment per codepoint, each with the string's own
        // size, unit, type and flags: <<"ab":16>> is <<$a:16, $b:16>>, as OTP
        // documents and its evaluator does. Its size is evaluated once, even
        // for "", which writes nothing. A UTF segment takes no size.
        let mut operands: Vec<(Vec<VarId>, Option<VarId>)> = Vec::with_capacity(segments.len());
        for seg in segments {
            if seg.type_spec.unit.is_some_and(|unit| !(1..=256).contains(&unit)) {
                return Err(LowerError::Unsupported(
                    "binary segment unit must be in 1..=256".into(), Some(seg.value.span())));
            }
            let values: Vec<VarId> = match &seg.value {
                ast::Expr::String(s, _) => s.chars()
                    .map(|ch| self.emit_const(Literal::Integer(ch as i64)))
                    .collect(),
                value => vec![self.lower_construction_operand(value)?],
            };
            let is_utf = matches!(
                seg.type_spec.typ,
                BinSegmentKind::Utf8 | BinSegmentKind::Utf16 | BinSegmentKind::Utf32
            );
            let size = match &seg.size {
                Some(size_expr) if !is_utf => Some(self.lower_construction_operand(size_expr)?),
                _ => None,
            };
            operands.push((values, size));
        }

        // Phase 1b: check each segment and compute its total_bits.
        // Each entry: (val, total_bits_var, total_bits_const, flags, is_binary_seg, is_float_seg, utf_kind)
        // All sizes are in BITS. For UTF segments, total_bits is WORST-CASE (32 bits).
        type SegInfo = (VarId, VarId, Option<i64>, BinFlags, bool, bool, Option<BinSegmentKind>);
        let mut seg_info: Vec<SegInfo> = Vec::new();
        let mut has_variable_utf = false;

        for (seg, (values, size)) in segments.iter().zip(operands) {
            let is_utf = matches!(
                seg.type_spec.typ,
                BinSegmentKind::Utf8 | BinSegmentKind::Utf16 | BinSegmentKind::Utf32
            );
            let is_binary_seg = matches!(seg.type_spec.typ, BinSegmentKind::Binary | BinSegmentKind::Bitstring);
            let is_float_seg = seg.type_spec.typ == BinSegmentKind::Float;
            let unit = Self::effective_unit(&seg.type_spec);

            if is_binary_seg && matches!(seg.value, ast::Expr::String(..)) {
                // A codepoint is an integer, never a binary source, so this
                // check raises badarg as on BEAM, even for "" (checked as 0).
                let codepoint = match values.first() {
                    Some(&codepoint) => codepoint,
                    None => self.emit_const(Literal::Integer(0)),
                };
                let source_unit = self.emit_const(Literal::Integer(unit));
                let checked = self.fresh_var();
                self.emit(Instruction::Call(checked, CallTarget::Bif("binary_source_size".to_string()), vec![codepoint, source_unit]));
                self.branch_on_construction_exception();
                continue;
            }

            if is_utf {
                // UTF segments: no user-specified size allowed.
                // Use worst-case bits for allocation.
                let flags = self.ast_type_spec_to_bin_flags(&seg.type_spec);
                let worst_case_bits: i64 = match seg.type_spec.typ {
                    BinSegmentKind::Utf8 => 32,  // 1-4 bytes
                    BinSegmentKind::Utf16 => 32,  // 2-4 bytes (surrogate pair)
                    BinSegmentKind::Utf32 => 32,  // always 4 bytes
                    _ => unreachable!(),
                };
                if matches!(seg.type_spec.typ, BinSegmentKind::Utf8 | BinSegmentKind::Utf16) {
                    has_variable_utf = true;
                }
                let bits_var = self.emit_const(Literal::Integer(worst_case_bits));
                for val_var in values {
                    seg_info.push((val_var, bits_var, Some(worst_case_bits), flags, false, false, Some(seg.type_spec.typ)));
                }
                continue;
            }

            if is_binary_seg && seg.size.is_none() {
                // No explicit size: <<Bin/binary>> — use all bits of Bin.
                // BinarySize returns bits.
                let flags = self.ast_type_spec_to_bin_flags(&seg.type_spec);
                for val_var in values {
                    let sz = self.fresh_var();
                    let source_unit = self.emit_const(Literal::Integer(unit));
                    self.emit(Instruction::Call(sz, CallTarget::Bif("binary_source_size".to_string()), vec![val_var, source_unit]));
                    self.branch_on_construction_exception();
                    seg_info.push((val_var, sz, None, flags, true, false, None));
                }
            } else {
                // Explicit size or non-binary type: total_bits = size * unit
                let (size_var, size_const) = match size {
                    Some(sv) => (sv, None),
                    None => {
                        // Default size per type (in SIZE units, before unit multiplication)
                        let default_size = match seg.type_spec.typ {
                            BinSegmentKind::Integer => 8i64,
                            BinSegmentKind::Float => 64,
                            _ => 8,
                        };
                        let sv = self.emit_const(Literal::Integer(default_size));
                        (sv, Some(default_size))
                    }
                };
                // Constant valid sizes need no runtime validation. Every dynamic
                // segment is checked before multiplication or summing, so a
                // negative size cannot be hidden by another positive segment.
                let known_size = size_const.or(match seg.size.as_deref() {
                    Some(ast::Expr::Integer(n, _)) => Some(*n),
                    _ => None,
                });
                let known_bits = known_size.filter(|&n| n >= 0)
                    .and_then(|n| n.checked_mul(unit))
                    .filter(|&n| n < (1_i64 << 60));
                let (total_bits_var, total_bits_const) = if is_float_seg && !known_bits.is_some_and(|n| matches!(n, 16 | 32 | 64)) {
                    let unit_var = self.emit_const(Literal::Integer(unit));
                    let checked = self.fresh_var();
                    self.emit(Instruction::Call(checked,
                        CallTarget::Bif("float_segment_size".to_string()), vec![size_var, unit_var]));
                    self.branch_on_construction_exception();
                    (checked, None)
                } else if let Some(bits) = known_bits {
                    (self.emit_const(Literal::Integer(bits)), Some(bits))
                } else {
                    let unit_var = self.emit_const(Literal::Integer(unit));
                    let product = self.fresh_var();
                    self.emit(Instruction::Call(product,
                        CallTarget::Bif("binary_segment_size".to_string()), vec![size_var, unit_var]));
                    self.branch_on_construction_exception();
                    (product, None)
                };
                let flags = self.ast_type_spec_to_bin_flags(&seg.type_spec);
                for val_var in values {
                    seg_info.push((val_var, total_bits_var, total_bits_const, flags, is_binary_seg, is_float_seg, None));
                }
            }
        }

        // Phase 2: compute total bit size (passed directly to MakeBinary).
        let has_runtime_sizes = seg_info.iter().any(|si| si.2.is_none());

        let constant_total = if !has_runtime_sizes {
            seg_info.iter().try_fold(0_i64, |sum, si| sum.checked_add(si.2.unwrap()))
                .filter(|&n| n < (1_i64 << 60))
        } else { None };
        let total_bits_var = if let Some(total_bits) = constant_total {
            self.emit_const(Literal::Integer(total_bits))
        } else {
            // Validated segment sizes are integers. Seed with the first size
            // instead of emitting a redundant checked addition to zero.
            let mut sizes = seg_info.iter();
            let mut sum = sizes.next().map(|si| si.1)
                .unwrap_or_else(|| self.emit_const(Literal::Integer(0)));
            for si in sizes {
                let new_sum = self.fresh_var();
                self.emit(Instruction::BinOp(new_sum, BinOp::Add, sum, si.1));
                sum = new_sum;
            }
            sum
        };

        // Phase 3: allocate binary (MakeBinary now takes bit count)
        let bin_var = self.fresh_var();
        self.emit(Instruction::MakeBinary(bin_var, total_bits_var));
        self.branch_on_construction_exception();

        // Phase 4: put each segment — bit_offset accumulator
        let mut bit_offset_var = self.emit_const(Literal::Integer(0));
        for (index, (val_var, bits_var, _, flags, is_bin, is_float, utf_kind)) in seg_info.iter().enumerate() {
            if let Some(kind) = utf_kind {
                // UTF segment: emit Put instruction that returns bits_written
                let bits_written = self.fresh_var();
                match kind {
                    BinSegmentKind::Utf8 => {
                        self.emit(Instruction::BinaryPutUtf8(bits_written, bin_var, bit_offset_var, *val_var));
                    }
                    BinSegmentKind::Utf16 => {
                        self.emit(Instruction::BinaryPutUtf16(bits_written, bin_var, bit_offset_var, *val_var, *flags));
                    }
                    BinSegmentKind::Utf32 => {
                        self.emit(Instruction::BinaryPutUtf32(bits_written, bin_var, bit_offset_var, *val_var, *flags));
                    }
                    _ => unreachable!(),
                }
                self.branch_on_construction_exception();
                // Advance offset by actual bits written (variable for UTF-8/16)
                let new_offset = self.fresh_var();
                self.emit(Instruction::BinOp(new_offset, BinOp::Add, bit_offset_var, bits_written));
                bit_offset_var = new_offset;
            } else if *is_bin {
                self.emit(Instruction::BinaryPutBinary(bin_var, bit_offset_var, *val_var, *bits_var));
                self.branch_on_construction_exception();
                if index == 0 {
                    bit_offset_var = *bits_var;
                } else {
                    let new_offset = self.fresh_var();
                    self.emit(Instruction::BinOp(new_offset, BinOp::Add, bit_offset_var, *bits_var));
                    bit_offset_var = new_offset;
                }
            } else if *is_float {
                self.emit(Instruction::BinaryPutFloat(bin_var, bit_offset_var, *val_var, *bits_var, *flags));
                self.branch_on_construction_exception();
                if index == 0 {
                    bit_offset_var = *bits_var;
                } else {
                    let new_offset = self.fresh_var();
                    self.emit(Instruction::BinOp(new_offset, BinOp::Add, bit_offset_var, *bits_var));
                    bit_offset_var = new_offset;
                }
            } else {
                self.emit(Instruction::BinaryPutInteger(bin_var, bit_offset_var, *val_var, *bits_var, *flags));
                self.branch_on_construction_exception();
                if index == 0 {
                    bit_offset_var = *bits_var;
                } else {
                    let new_offset = self.fresh_var();
                    self.emit(Instruction::BinOp(new_offset, BinOp::Add, bit_offset_var, *bits_var));
                    bit_offset_var = new_offset;
                }
            }
        }

        // Phase 5: Resize binary if any variable-width UTF segments were used.
        // The binary was allocated with worst-case size; shrink to actual bit size.
        if has_variable_utf {
            self.emit(Instruction::BinaryResize(bin_var, bit_offset_var));
        }

        Ok(bin_var)
    }

    /// Lower a guard expression. Guards are side-effect-free.
    /// Any exception raised by a guard BIF (e.g. hd([]), element(3, {a,b}))
    /// must be caught and treated as guard failure (false).
    pub fn lower_guard(&mut self, guard: &ast::Guard) -> VarId {
        match guard {
            ast::Guard::Test(expr) => {
                self.lower_guard_expr(expr)
            }
            ast::Guard::Conjunction(exprs) => {
                // All must be true
                let mut result = self.emit_const(Literal::Atom(self.true_atom_id)); // true
                for expr in exprs {
                    let val = self.lower_guard_expr(expr);
                    let and_result = self.fresh_var();
                    self.emit(Instruction::BinOp(
                        and_result,
                        BinOp::Andalso,
                        result,
                        val,
                    ));
                    result = and_result;
                }
                result
            }
            ast::Guard::Disjunction(guards) => {
                // Any must be true
                let mut result = self.emit_const(Literal::Atom(self.false_atom_id)); // false
                for guard in guards {
                    let val = self.lower_guard(guard);
                    let or_result = self.fresh_var();
                    self.emit(Instruction::BinOp(
                        or_result,
                        BinOp::Orelse,
                        result,
                        val,
                    ));
                    result = or_result;
                }
                result
            }
        }
    }

    /// Lower a single guard test expression, wrapping it in a try-catch so
    /// that any runtime exception (e.g. hd([]) raising badarg) is caught and
    /// the guard evaluates to `false` instead of crashing.
    fn lower_guard_expr(&mut self, expr: &ast::Expr) -> VarId {
        // Check if the expression might call a failable BIF.
        // Simple literals, variables, and comparisons can't fail at runtime,
        // so we skip the try-catch overhead for those.
        if !Self::guard_expr_may_throw(expr) {
            return self.lower_expr(expr, false).unwrap_or_else(|_| {
                self.emit_const(Literal::Atom(self.false_atom_id))
            });
        }

        // Wrap in a mini try-catch: if the expression raises an exception,
        // catch it and return false.
        let normal_block = self.fresh_block_with_id();
        let catch_block = self.fresh_block_with_id();
        let merge_block = self.fresh_block_with_id();
        let result_var = self.fresh_var();

        // Push exception landing pad for inline RaiseException
        let exc_landing_pad = self.fresh_block_with_id();
        self.try_exception_targets.push(exc_landing_pad);

        // A raise can leave the test before it reads a deferred tail, so the
        // code after it (the next test of `G1; G2`, the clause body) starts
        // from the bindings and tail records from before the test.
        let branch = self.enter_branch();

        // Lower the guard expression
        let expr_val = self.lower_expr(expr, false).unwrap_or_else(|_| {
            self.emit_const(Literal::Atom(self.false_atom_id))
        });

        self.try_exception_targets.pop();

        // Check if an FFI call raised an exception
        let exc_check = self.fresh_var();
        self.emit(Instruction::CheckException(exc_check));
        let true_val = self.emit_const(Literal::Atom(self.true_atom_id));
        let is_exc = self.fresh_var();
        self.emit(Instruction::BinOp(is_exc, BinOp::ExactEq, exc_check, true_val));
        self.set_terminator(Terminator::Branch(is_exc, catch_block, normal_block));
        self.close_current_block();

        // Normal block: no exception, use expression result
        self.start_block(normal_block);
        self.emit(Instruction::Move(result_var, expr_val));
        self.set_terminator(Terminator::Jump(merge_block, vec![]));
        self.close_current_block();

        // Catch block: exception occurred, clear it and return false
        self.start_block(catch_block);
        self.emit(Instruction::ClearException);
        let false_val = self.emit_const(Literal::Atom(self.false_atom_id));
        self.emit(Instruction::Move(result_var, false_val));
        self.set_terminator(Terminator::Jump(merge_block, vec![]));
        self.close_current_block();

        // Exception landing pad (for inline RaiseException): same as catch
        self.start_block(exc_landing_pad);
        self.emit(Instruction::ClearException);
        let false_val2 = self.emit_const(Literal::Atom(self.false_atom_id));
        self.emit(Instruction::Move(result_var, false_val2));
        self.set_terminator(Terminator::Jump(merge_block, vec![]));
        self.close_current_block();

        // Merge block: result_var has been set by either path
        self.start_block(merge_block);
        self.leave_branch(&branch);
        result_var
    }

    /// Check if a guard expression might throw a runtime exception.
    /// Simple comparisons and type tests can't fail, but BIF calls like
    /// hd/1, tl/1, element/2, etc. can raise badarg.
    /// Lower a `maybe...end` expression (EEP-49).
    ///
    /// Uses branch-based control flow (not exceptions) for `?=` mismatches.
    pub(super) fn lower_maybe(
        &mut self,
        body: &[ast::MaybeExpr],
        else_clauses: &[ast::CaseClause],
        span: nera_syntax::span::Span,
        is_tail: bool,
    ) -> Result<VarId, LowerError> {
        // If there are no ?= items, treat as a simple scoped block
        let has_cond_match = body.iter().any(|item| matches!(item, ast::MaybeExpr::CondMatch(..)));
        if !has_cond_match {
            return self.in_local_scope(|builder| {
                let exprs: Vec<&ast::Expr> = body.iter().map(|item| match item {
                    ast::MaybeExpr::Expr(e) => e,
                    _ => unreachable!(),
                }).collect();
                let mut result = builder.emit_const(Literal::Atom(builder.true_atom_id)); // placeholder
                for (i, expr) in exprs.iter().enumerate() {
                    let is_last_expr = i == exprs.len() - 1;
                    result = builder.lower_expr(expr, is_last_expr && is_tail)?;
                }
                Ok(result)
            });
        }

        let join_block = self.fresh_block_with_id();
        let result_var = self.fresh_var();
        let else_scrutinee = self.fresh_var();

        // Build the else handler block
        let else_handler = self.fresh_block_with_id();

        // Push scope — variables bound inside maybe are NOT exported
        self.push_scope();

        // Deferred binary tails are path-local, as in `case`: the success
        // path, each else clause and the join each start from these records.
        let outer_match_contexts = self.match_contexts.clone();

        // Process body items sequentially
        for (i, item) in body.iter().enumerate() {
            let is_last = i == body.len() - 1;
            match item {
                ast::MaybeExpr::Expr(expr) => {
                    let val = self.lower_expr(expr, is_last && is_tail)?;
                    if is_last {
                        if is_tail {
                            if self.current_terminator.is_none() {
                                self.set_terminator(Terminator::Return(val));
                            }
                            self.close_current_block();
                        } else {
                            self.emit(Instruction::Move(result_var, val));
                            self.set_terminator(Terminator::Jump(join_block, vec![]));
                            self.close_current_block();
                        }
                    }
                }
                ast::MaybeExpr::CondMatch(pattern, rhs_expr, _cond_span) => {
                    let rhs_val = self.lower_expr(rhs_expr, false)?;

                    // Desugar record patterns
                    let desugared_pat = self.desugar_record_pattern(pattern)?;

                    // Collect outer bindings BEFORE pre_bind (so pattern vars aren't
                    // in the set — they'll be bound by the pattern match compiler)
                    let outer_bindings = self.collect_all_bindings();

                    // Save the dispatch block (current block after lowering RHS)
                    let dispatch_block = self.current_block_id.unwrap();
                    self.close_current_block();

                    // Create fail block for this ?= : on mismatch, jump to else_handler
                    let fail_block = self.fresh_block_with_id();
                    self.start_block(fail_block);
                    self.emit(Instruction::Move(else_scrutinee, rhs_val));
                    self.set_terminator(Terminator::Jump(else_handler, vec![]));
                    self.close_current_block();

                    // Create success continuation block
                    let success_block = self.fresh_block_with_id();

                    // Build the match body block that pre-binds and jumps to success
                    let match_body_block = self.fresh_block_with_id();
                    self.start_block(match_body_block);
                    self.pre_bind_pattern_vars(&desugared_pat, rhs_val, false);
                    self.keep_outer_bindings(&outer_bindings);
                    self.set_terminator(Terminator::Jump(success_block, vec![]));
                    self.close_current_block();

                    let match_clause = pattern::MatchClause {
                        patterns: vec![desugared_pat],
                        guard: None,
                        body_block: match_body_block,
                    };

                    let match_entry = pattern::compile_match_with_outer_bindings(
                        self,
                        &[rhs_val],
                        &[match_clause],
                        fail_block,
                        &outer_bindings,
                    );
                    self.patch_block_terminator(
                        dispatch_block,
                        Terminator::Jump(match_entry, vec![]),
                    );

                    // Continue from success block
                    self.start_block(success_block);

                    // If this is the last item, write result and jump to join
                    if is_last {
                        if is_tail {
                            if self.current_terminator.is_none() {
                                self.set_terminator(Terminator::Return(rhs_val));
                            }
                            self.close_current_block();
                        } else {
                            self.emit(Instruction::Move(result_var, rhs_val));
                            self.set_terminator(Terminator::Jump(join_block, vec![]));
                            self.close_current_block();
                        }
                    }
                }
            }
        }

        self.pop_scope();
        self.match_contexts = outer_match_contexts.clone();

        // Build else handler
        self.start_block(else_handler);
        if !else_clauses.is_empty() {
            // Pattern-match else_scrutinee against else clauses (like case)
            let else_dispatch = self.current_block_id.unwrap();
            self.close_current_block();

            let mut match_clauses = Vec::new();
            for clause in else_clauses {
                self.match_contexts = outer_match_contexts.clone();
                let body_block = self.fresh_block_with_id();
                let desugared_pat = self.desugar_record_pattern(&clause.pattern)?;

                self.push_scope();
                self.start_block(body_block);
                self.pre_bind_pattern_vars(&desugared_pat, else_scrutinee, false);
                let body_result = self.lower_body(&clause.body, is_tail)?;
                if !is_tail {
                    self.emit(Instruction::Move(result_var, body_result));
                    self.set_terminator(Terminator::Jump(join_block, vec![]));
                } else if self.current_terminator.is_none() {
                    self.set_terminator(Terminator::Return(body_result));
                }
                self.close_current_block();
                self.pop_scope();

                match_clauses.push(MatchClause {
                    patterns: vec![desugared_pat],
                    guard: clause.guard.clone(),
                    body_block,
                });
            }
            self.match_contexts = outer_match_contexts.clone();

            // Fail block: raise {else_clause, Value}
            let else_fail_block = self.fresh_block_with_id();
            self.start_block(else_fail_block);
            self.emit_set_line(span);
            let ec_atom = self.atom_id_for("else_clause");
            let reason_atom = self.emit_const(Literal::Atom(ec_atom));
            let reason_tuple = self.fresh_var();
            self.emit(Instruction::MakeTuple(reason_tuple, vec![reason_atom, else_scrutinee]));
            let exc_result = self.fresh_var();
            self.emit(Instruction::RaiseException(exc_result, ExceptionClass::Error, reason_tuple));
            self.set_raise_terminator(exc_result);
            self.close_current_block();

            let outer_bindings = self.collect_all_bindings();
            let match_entry = pattern::compile_match_with_outer_bindings(
                self,
                &[else_scrutinee],
                &match_clauses,
                else_fail_block,
                &outer_bindings,
            );
            self.patch_block_terminator(else_dispatch, Terminator::Jump(match_entry, vec![]));
        } else {
            // No else clauses: raise {else_clause, Value}
            self.emit_set_line(span);
            let ec_atom = self.atom_id_for("else_clause");
            let reason_atom = self.emit_const(Literal::Atom(ec_atom));
            let reason_tuple = self.fresh_var();
            self.emit(Instruction::MakeTuple(reason_tuple, vec![reason_atom, else_scrutinee]));
            let exc_result = self.fresh_var();
            self.emit(Instruction::RaiseException(exc_result, ExceptionClass::Error, reason_tuple));
            self.set_raise_terminator(exc_result);
            self.close_current_block();
        }
        self.match_contexts = outer_match_contexts;

        if !is_tail {
            self.start_block(join_block);
        }
        Ok(result_var)
    }

    fn guard_expr_may_throw(expr: &ast::Expr) -> bool {
        match expr {
            // Comparisons, type tests, boolean ops are safe
            ast::Expr::BinOp(op, left, right, _) => {
                match op {
                    // Arithmetic can throw (badarith)
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div
                    | BinOp::IntDiv | BinOp::Rem | BinOp::Band | BinOp::Bor
                    | BinOp::Bxor | BinOp::Bsl | BinOp::Bsr => true,
                    // Boolean ops can throw (badarg) on non-boolean operands
                    BinOp::And | BinOp::Or | BinOp::Xor => true,
                    // Comparisons are safe but operands might throw
                    _ => Self::guard_expr_may_throw(left) || Self::guard_expr_may_throw(right),
                }
            }
            ast::Expr::UnaryOp(op, _operand, _) => match op {
                // not/neg/bnot can throw (badarg/badarith) on non-boolean/non-numeric operands
                ast::UnaryOp::Not | ast::UnaryOp::Neg | ast::UnaryOp::Bnot => true,
            },
            // Function calls can throw
            ast::Expr::Call(_, _, _) => true,
            // Literals, variables, type tests are safe
            ast::Expr::Atom(_, _) | ast::Expr::Integer(_, _) | ast::Expr::BigInteger(_, _)
            | ast::Expr::Float(_, _)
            | ast::Expr::String(_, _) | ast::Expr::Variable(_, _) | ast::Expr::Nil(_) => false,
            // Tuple/list construction is safe but elements might throw
            ast::Expr::Tuple(elems, _) => elems.iter().any(Self::guard_expr_may_throw),
            ast::Expr::Cons(h, t, _) => Self::guard_expr_may_throw(h) || Self::guard_expr_may_throw(t),
            // Default: assume it might throw
            _ => true,
        }
    }

}
