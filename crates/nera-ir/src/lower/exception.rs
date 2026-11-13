//! Exception handling lowering: try/catch, throw, error.

use nera_syntax::ast::{self, BinOp};
use crate::ir::*;
use crate::pattern;
use super::{IrBuilder, LowerError};

impl IrBuilder {
    /// Collect variable references in a pattern (e.g. in Match(pat, expr)).
    /// Set terminator after a `RaiseException` instruction.
    ///
    /// Inside a try body, jump to the exception landing pad so the catch
    /// handler can intercept the exception.  Outside a try body, return
    /// from the function (the wrapper will propagate the exception).
    pub(super) fn set_raise_terminator(&mut self, exc_result: VarId) {
        if let Some(&landing_pad) = self.try_exception_targets.last() {
            self.set_terminator(Terminator::Jump(landing_pad, vec![]));
        } else {
            self.set_terminator(Terminator::Return(exc_result));
        }
    }

    /// Inside a try body, branch to the handler when the operation just
    /// emitted raised. A protected operation only records its exception, so
    /// without this branch the rest of the body would still run: a later
    /// send, receive or call would happen, and a later operation could
    /// replace the reason. Outside a try, code generation returns at once.
    pub(super) fn branch_on_pending_exception(&mut self) {
        if let Some(&landing_pad) = self.try_exception_targets.last() {
            let failed = self.fresh_var();
            self.emit(Instruction::CheckException(failed));
            let next = self.fresh_block_with_id();
            self.set_terminator(Terminator::Branch(failed, landing_pad, next));
            self.close_current_block();
            self.start_block(next);
        }
    }

    /// Comparisons never raise; every other operator can, on a wrong type.
    pub(super) fn binop_may_raise(op: BinOp) -> bool {
        !matches!(
            op,
            BinOp::Eq
                | BinOp::ExactEq
                | BinOp::NotEq
                | BinOp::ExactNotEq
                | BinOp::Lt
                | BinOp::Gt
                | BinOp::LtEq
                | BinOp::GtEq
        )
    }

    // ---- Exception handling ----

    /// Lower `throw(Reason)` to IR.
    pub(super) fn lower_throw(&mut self, reason_expr: &ast::Expr) -> Result<VarId, LowerError> {
        let reason = self.lower_expr(reason_expr, false)?;
        let result = self.fresh_var();
        self.emit(Instruction::RaiseException(
            result,
            ExceptionClass::Throw,
            reason,
        ));
        // Nothing after a throw runs: inside a try it goes to the handler,
        // outside one it leaves the function with the exception.
        if self.try_exception_targets.is_empty() {
            let failed = self.fresh_var();
            self.emit(Instruction::CheckException(failed));
            let leave = self.fresh_block_with_id();
            let next = self.fresh_block_with_id();
            self.set_terminator(Terminator::Branch(failed, leave, next));
            self.close_current_block();
            self.start_block(leave);
            self.set_terminator(Terminator::Return(result));
            self.close_current_block();
            self.start_block(next);
        } else {
            self.branch_on_pending_exception();
        }
        // The result is a placeholder; no code after this reads it at run time.
        Ok(result)
    }

    /// Lower `try Body [of OfClauses] [catch CatchClauses] [after AfterBody] end` to IR.
    pub(super) fn lower_try(
        &mut self,
        body: &[ast::Expr],
        of_clauses: &[ast::CaseClause],
        catch_clauses: &[ast::CatchClause],
        after_body: &Option<Vec<ast::Expr>>,
    ) -> Result<VarId, LowerError> {
        match after_body {
            Some(after_exprs) => self.lower_try_after(body, of_clauses, catch_clauses, after_exprs),
            None => self.lower_try_catch(body, of_clauses, catch_clauses),
        }
    }

    /// Lower `try ... after AfterBody end`.
    ///
    /// The body, `of` clauses and catch clauses run under one more handler.
    /// The after body runs once on every way out of them. An exception is
    /// held while it runs, with the exception state cleared, and is then
    /// raised again with its class, reason and stacktrace; an exception
    /// from the after body itself replaces it, as on BEAM.
    fn lower_try_after(
        &mut self,
        body: &[ast::Expr],
        of_clauses: &[ast::CaseClause],
        catch_clauses: &[ast::CatchClause],
        after_exprs: &[ast::Expr],
    ) -> Result<VarId, LowerError> {
        let exc_landing_pad = self.fresh_block_with_id();
        let normal_block = self.fresh_block_with_id();
        let exc_block = self.fresh_block_with_id();
        let after_block = self.fresh_block_with_id();
        let raise_check_block = self.fresh_block_with_id();
        let pending_block = self.fresh_block_with_id();
        let reraise_block = self.fresh_block_with_id();
        let done_block = self.fresh_block_with_id();

        // Written on both ways into the after body.
        let try_result = self.fresh_var();
        let raised = self.fresh_var();
        let class = self.fresh_var();
        let reason = self.fresh_var();
        let stacktrace = self.fresh_var();

        let branch = self.enter_branch();
        let before_bindings = self.collect_all_bindings();

        self.try_exception_targets.push(exc_landing_pad);
        let protected_result = if catch_clauses.is_empty() && of_clauses.is_empty() {
            self.lower_body(body, false)?
        } else {
            self.lower_try_catch(body, of_clauses, catch_clauses)?
        };
        self.try_exception_targets.pop();

        // An exception still pending from an operation that does not branch
        // to the landing pad itself.
        let pending = self.fresh_var();
        self.emit(Instruction::CheckException(pending));
        self.set_terminator(Terminator::Branch(pending, exc_block, normal_block));
        self.close_current_block();

        self.start_block(exc_landing_pad);
        self.set_terminator(Terminator::Jump(exc_block, vec![]));
        self.close_current_block();

        // Normal way out: keep the result, nothing to raise again.
        self.start_block(normal_block);
        self.emit(Instruction::Move(try_result, protected_result));
        let false_val = self.emit_const(Literal::Atom(self.false_atom_id));
        self.emit(Instruction::Move(raised, false_val));
        let nil = self.emit_const(Literal::Nil);
        self.emit(Instruction::Move(class, nil));
        self.emit(Instruction::Move(reason, nil));
        self.emit(Instruction::Move(stacktrace, nil));
        self.set_terminator(Terminator::Jump(after_block, vec![]));
        self.close_current_block();

        // Exception: hold it and clear it, so the after body runs normally.
        self.start_block(exc_block);
        let exc_class = self.fresh_var();
        self.emit(Instruction::GetExceptionClass(exc_class));
        let exc_reason = self.fresh_var();
        self.emit(Instruction::GetExceptionReason(exc_reason));
        let exc_stacktrace = self.fresh_var();
        self.emit(Instruction::GetExceptionStacktrace(exc_stacktrace));
        self.emit(Instruction::ClearException);
        self.emit(Instruction::Move(class, exc_class));
        self.emit(Instruction::Move(reason, exc_reason));
        self.emit(Instruction::Move(stacktrace, exc_stacktrace));
        let true_val = self.emit_const(Literal::Atom(self.true_atom_id));
        self.emit(Instruction::Move(raised, true_val));
        // Not read on this way out, but defined on every way into the after body.
        let unused = self.emit_const(Literal::Nil);
        self.emit(Instruction::Move(try_result, unused));
        self.set_terminator(Terminator::Jump(after_block, vec![]));
        self.close_current_block();

        // The after body sees only the bindings from before the try.
        self.start_block(after_block);
        if let Some(scope) = self.scopes.last_mut() {
            *scope = before_bindings;
        }
        self.leave_branch(&branch);
        let _after_result = self.lower_body(after_exprs, false)?;
        // An exception the after body left pending replaces the held one.
        let after_failed = self.fresh_var();
        self.emit(Instruction::CheckException(after_failed));
        self.set_terminator(Terminator::Branch(after_failed, pending_block, raise_check_block));
        self.close_current_block();

        self.start_block(pending_block);
        self.set_raise_terminator(after_failed);
        self.close_current_block();

        self.start_block(raise_check_block);
        self.set_terminator(Terminator::Branch(raised, reraise_block, done_block));
        self.close_current_block();

        // erlang:raise/3 keeps the original class and stacktrace.
        self.start_block(reraise_block);
        let erlang_atom = self.atom_id_for("erlang");
        let raise_atom = self.atom_id_for("raise");
        let reraise_result = self.fresh_var();
        self.emit(Instruction::Call(
            reraise_result,
            CallTarget::Remote(erlang_atom, raise_atom),
            vec![class, reason, stacktrace],
        ));
        self.set_raise_terminator(reraise_result);
        self.close_current_block();

        self.start_block(done_block);
        Ok(try_result)
    }

    /// Lower `try Body [of OfClauses] catch CatchClauses end` to IR.
    ///
    /// Strategy:
    /// 1. Lower the try body
    /// 2. Check if an exception occurred (CheckException)
    /// 3. If exception -> jump to catch handler
    /// 4. If no exception -> jump to continuation
    fn lower_try_catch(
        &mut self,
        body: &[ast::Expr],
        of_clauses: &[ast::CaseClause],
        catch_clauses: &[ast::CatchClause],
    ) -> Result<VarId, LowerError> {
        let catch_block = self.fresh_block_with_id();
        let normal_block = self.fresh_block_with_id();
        let merge_block = self.fresh_block_with_id();

        // Exception landing pad: inline RaiseException (from badmatch,
        // case_clause, if_clause, etc.) jumps here instead of returning.
        let exc_landing_pad = self.fresh_block_with_id();
        self.try_exception_targets.push(exc_landing_pad);

        // Allocate a shared result variable that both paths will write to
        let try_result = self.fresh_var();

        // The catch path may leave the body anywhere, and the of clauses
        // are siblings: each starts from the bindings before the body.
        let branch = self.enter_branch();

        // Only bindings from before the protected body are valid on every
        // exception path. Body bindings are available to of clauses, not catch.
        let catch_bindings = self.collect_all_bindings();
        let body_result = self.lower_body(body, false)?;

        self.try_exception_targets.pop();

        // Check if exception occurred (from function calls — their status
        // is set but emit_call_site_status_check skips propagation in
        // try-protected blocks, so CheckException picks it up here).
        let exc_check = self.fresh_var();
        self.emit(Instruction::CheckException(exc_check));
        let true_val = self.emit_const(Literal::Atom(self.true_atom_id));
        let is_exc = self.fresh_var();
        self.emit(Instruction::BinOp(is_exc, BinOp::ExactEq, exc_check, true_val));
        self.set_terminator(Terminator::Branch(is_exc, catch_block, normal_block));
        self.close_current_block();

        // Normal block: no exception
        self.start_block(normal_block);
        if of_clauses.is_empty() {
            // No `of` clauses — write body result directly to shared var
            self.emit(Instruction::Move(try_result, body_result));
            self.set_terminator(Terminator::Jump(merge_block, vec![]));
            self.close_current_block();
        } else {
            // `of` clauses present — pattern-match body result (like case)
            let of_dispatch_block = self.current_block_id.unwrap();
            self.close_current_block();

            let outer_bindings = self.collect_all_bindings();
            let mut match_clauses = Vec::new();
            for clause in of_clauses {
                let body_block = self.fresh_block_with_id();
                let desugared_pat = self.desugar_record_pattern(&clause.pattern)?;

                self.push_scope();
                self.start_block(body_block);
                self.pre_bind_pattern_vars(&desugared_pat, body_result, false);
                let clause_result = self.lower_body(&clause.body, false)?;
                self.emit(Instruction::Move(try_result, clause_result));
                self.set_terminator(Terminator::Jump(merge_block, vec![]));
                self.close_current_block();
                self.pop_scope();
                self.leave_branch(&branch);

                match_clauses.push(pattern::MatchClause {
                    patterns: vec![desugared_pat],
                    guard: clause.guard.clone(),
                    body_block,
                });
            }

            // Fail block: raise try_clause exception (no matching of clause)
            let fail_block = self.fresh_block_with_id();
            self.start_block(fail_block);
            if let Some(first_clause) = of_clauses.first() {
                self.emit_set_line(first_clause.span);
            }
            let tc_atom = self.atom_id_for("try_clause");
            let reason_atom = self.emit_const(Literal::Atom(tc_atom));
            let reason = self.fresh_var();
            self.emit(Instruction::MakeTuple(reason, vec![reason_atom, body_result]));
            let exc_result = self.fresh_var();
            self.emit(Instruction::RaiseException(exc_result, ExceptionClass::Error, reason));
            self.set_raise_terminator(exc_result);
            self.close_current_block();

            let match_entry = pattern::compile_match_with_outer_bindings(
                self, &[body_result], &match_clauses, fail_block, &outer_bindings,
            );
            self.patch_block_terminator(of_dispatch_block, Terminator::Jump(match_entry, vec![]));
        }

        self.leave_branch(&branch);

        // Exception landing pad: inline exception → catch handler
        self.start_block(exc_landing_pad);
        self.set_terminator(Terminator::Jump(catch_block, vec![]));
        self.close_current_block();

        // Catch block: exception occurred — match catch clauses
        self.start_block(catch_block);
        // Read exception class, reason, and stacktrace from context
        let exc_class = self.fresh_var();
        self.emit(Instruction::GetExceptionClass(exc_class));
        let exc_reason = self.fresh_var();
        self.emit(Instruction::GetExceptionReason(exc_reason));
        let exc_stacktrace = self.fresh_var();
        self.emit(Instruction::GetExceptionStacktrace(exc_stacktrace));
        // Clear exception state so we don't re-trigger
        self.emit(Instruction::ClearException);

        // Restore lookup as well as matching to the pre-body scope: binary
        // pre-binding must not mistake a body-only name for an outer variable.
        if let Some(scope) = self.scopes.last_mut() {
            *scope = catch_bindings;
        }
        let catch_result = self.lower_catch_clauses(
            catch_clauses, exc_class, exc_reason, exc_stacktrace,
        )?;
        // Write catch result to shared var
        self.emit(Instruction::Move(try_result, catch_result));
        self.set_terminator(Terminator::Jump(merge_block, vec![]));
        self.close_current_block();

        // Merge block: both paths have written to try_result
        self.start_block(merge_block);
        self.leave_branch(&branch);

        Ok(try_result)
    }

    /// Lower catch clauses by matching exception class and reason.
    /// Chains through clauses: check class, then pattern-match reason.
    /// Falls through to the next clause on mismatch. Re-raises if none match.
    pub(super) fn lower_catch_clauses(
        &mut self,
        clauses: &[ast::CatchClause],
        exc_class_var: VarId,
        exc_reason_var: VarId,
        exc_stacktrace_var: VarId,
    ) -> Result<VarId, LowerError> {
        if clauses.is_empty() {
            // No catch clauses — reraise with the ORIGINAL class and
            // stacktrace via erlang:raise/3. RaiseException would force
            // the Throw class, losing the distinction between throw /
            // error / exit.
            let erlang_atom = self.atom_id_for("erlang");
            let raise_atom = self.atom_id_for("raise");
            let result = self.fresh_var();
            self.emit(Instruction::Call(
                result,
                CallTarget::Remote(erlang_atom, raise_atom),
                vec![exc_class_var, exc_reason_var, exc_stacktrace_var],
            ));
            return Ok(result);
        }

        let catch_result = self.fresh_var();
        let merge_block = self.fresh_block_with_id();
        let branch = self.enter_branch();

        // Save current block — we'll patch its terminator later.
        let entry_block = self.current_block_id.unwrap();
        self.close_current_block();

        // Re-raise block: jump here if no clause matches.
        // Use erlang:raise/3 so the ORIGINAL class and stacktrace are
        // preserved (RaiseException would force a Throw class).
        let reraise_block = self.fresh_block_with_id();
        self.start_block(reraise_block);
        let erlang_atom = self.atom_id_for("erlang");
        let raise_atom = self.atom_id_for("raise");
        let reraise_result = self.fresh_var();
        self.emit(Instruction::Call(
            reraise_result,
            CallTarget::Remote(erlang_atom, raise_atom),
            vec![exc_class_var, exc_reason_var, exc_stacktrace_var],
        ));
        self.set_raise_terminator(reraise_result);
        self.close_current_block();

        // Pre-allocate entry blocks for each clause so we can reference
        // clause[i+1]'s entry as the fail target for clause[i].
        let mut clause_entries: Vec<BlockId> = Vec::new();
        for _ in clauses {
            clause_entries.push(self.fresh_block_with_id());
        }

        let outer_bindings = self.collect_all_bindings();
        for (i, clause) in clauses.iter().enumerate() {
            let fail_target = if i + 1 < clauses.len() {
                clause_entries[i + 1]
            } else {
                reraise_block
            };

            // Desugar record patterns in catch clause
            let desugared_pat = self.desugar_record_pattern(&clause.pattern)?;

            // Class and reason share a pattern scope. This also compares an
            // outer class variable, or a class name repeated in the reason.
            let class_pattern = match &clause.class {
                ast::CatchClass::Throw => ast::Pattern::Atom(self.atom_id_for("throw"), clause.span),
                ast::CatchClass::Error => ast::Pattern::Atom(self.atom_id_for("error"), clause.span),
                ast::CatchClass::Exit => ast::Pattern::Atom(self.atom_id_for("exit"), clause.span),
                ast::CatchClass::Variable(name, span) => {
                    if self.resolve_atom(*name) == Some("_") {
                        ast::Pattern::Wildcard(*span)
                    } else {
                        ast::Pattern::Variable(*name, *span)
                    }
                }
            };
            let body_block = self.fresh_block_with_id();
            let match_clause = pattern::MatchClause {
                patterns: vec![class_pattern.clone(), desugared_pat.clone()],
                guard: clause.guard.clone(),
                body_block,
            };
            self.start_block(clause_entries[i]);
            self.close_current_block();
            // Even a bare reason variable needs matching when already bound;
            // it cannot take the former always-matching catch fast path.
            let match_entry = pattern::compile_match_with_outer_bindings(
                self, &[exc_class_var, exc_reason_var], &[match_clause], fail_target,
                &outer_bindings,
            );
            self.patch_block_terminator(clause_entries[i], Terminator::Jump(match_entry, vec![]));

            // --- Body block ---
            self.start_block(body_block);
            self.push_scope();
            self.pre_bind_pattern_vars(&class_pattern, exc_class_var, false);
            self.pre_bind_pattern_vars(&desugared_pat, exc_reason_var, false);
            // Bind stacktrace variable if present (3-part catch)
            if let Some(ref st_pat) = clause.stacktrace {
                let desugared_st = self.desugar_record_pattern(st_pat)?;
                self.pre_bind_pattern_vars(&desugared_st, exc_stacktrace_var, false);
            }
            let body_result = self.lower_body(&clause.body, false)?;
            self.emit(Instruction::Move(catch_result, body_result));
            self.set_terminator(Terminator::Jump(merge_block, vec![]));
            self.close_current_block();
            self.pop_scope();
            self.leave_branch(&branch);
        }

        // Patch entry block → first clause
        self.patch_block_terminator(entry_block, Terminator::Jump(clause_entries[0], vec![]));

        self.start_block(merge_block);
        Ok(catch_result)
    }

}
