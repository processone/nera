//! Shared list and binary comprehension lowering.

use std::collections::{HashMap, HashSet};
use nera_syntax::ast::{self, AtomId, BinOp};
use crate::ir::*;
use crate::pattern::{self, MatchClause};
use crate::passes::insert_backedge_yield_checks;
use super::{IrBuilder, LowerError};

/// A generator's pattern, its source and its filters. Sources and filters
/// are the comprehension's own expressions: branch exports find them by
/// address.
type GeneratorGroup<'a> = (ast::Pattern, &'a ast::Expr, Vec<&'a ast::Expr>);
type QualifierGroups<'a> = (Vec<GeneratorGroup<'a>>, Vec<bool>);

impl IrBuilder {
    /// Lower either comprehension form with tail-recursive auxiliary helpers.
    /// Binary comprehensions use the same iteration, capture and accumulation
    /// path, then finalize with `list_to_bitstring(lists:reverse(Acc))`, which
    /// packs bodies that are not whole bytes (`<<X:4>>`) without padding.
    ///
    /// Uses the **accumulator + reverse** approach (like BEAM): each helper
    /// prepends elements to an accumulator list and tail-calls itself. When
    /// the iterator is exhausted, the outermost helper reverses the
    /// accumulator to restore correct order. This avoids O(N) OS stack
    /// frames that the old head-recursive approach required.
    ///
    /// For `[Body || P0 <- E0, F0…, P1 <- E1, F1…, …]`:
    ///
    ///   helper_0(Iter0, Acc, Captured…)
    ///     [H|T] → bind P0, check F0…, tail-call helper_1(eval(E1), Acc, T, H, Captured…)
    ///     []    → return lists:reverse(Acc)
    ///
    ///   helper_K(IterK, Acc, Tail0…TailK-1, Head0…HeadK-1, Captured…)
    ///     [H|T] → rebind outer patterns, bind PK, check FK…,
    ///             if innermost: tail-call self(T, [Body|Acc], …)
    ///             else:         tail-call helper_{K+1}(eval(E_{K+1}), Acc, …)
    ///     []    → tail-call helper_{K-1}(TailK-1, Acc, …)
    pub(super) fn lower_comprehension(
        &mut self,
        body_expr: &ast::Expr,
        qualifiers: &[ast::LcQualifier],
        finalize_as_binary: bool,
    ) -> Result<VarId, LowerError> {
        // ── 1. Split qualifiers into generator groups ──
        let (groups, bin_flags) = Self::split_lc_qualifiers(qualifiers)?;
        let n = groups.len();

        // ── 2. Evaluate outermost source expression in enclosing scope ──
        // Names it binds stay local to it, as on BEAM: neither the other
        // qualifiers nor the code after the comprehension see them.
        let source_0 = self.in_local_scope(|builder| builder.lower_expr(groups[0].1, false))?;

        // ── 3. Analyse free variables ──
        let captured_vars = self.collect_lc_captures(&groups, body_expr);
        let num_captured = captured_vars.len() as u32;

        // ── 4. Pre-generate helper function names for all generators ──
        let mut helpers: Vec<(AtomId, String, u32)> = Vec::new(); // (name_id, mangled, arity)
        let prefix = if finalize_as_binary { "bc" } else { "lc" };
        for k in 0..n {
            let id = self.lc_counter;
            self.lc_counter += 1;
            let name_str = format!("__{prefix}_{id}");
            let mangled = format!("{}__{prefix}_{id}", self.module_name.0);
            let name_id = self.fresh_atom_id();
            self.atom_names.insert(name_id, name_str);
            // params: iter + acc + k tails + k heads + captured
            let arity = 2 + 2 * k as u32 + num_captured;
            self.module_functions
                .insert((name_id, arity), mangled.clone());
            helpers.push((name_id, mangled, arity));
        }

        // ── 5. Build all helper functions ──
        let saved = self.save_state();

        for k in 0..n {
            if k > 0 {
                // Reset builder state for the next helper function.
                self.current_blocks = Vec::new();
                self.current_block_id = None;
                self.current_instructions = Vec::new();
                self.current_terminator = None;
                self.next_var = 0;
                self.next_block = 0;
                self.scopes = vec![HashMap::new()];
            }
            self.build_lc_helper(k, n, &groups, &helpers, &captured_vars, body_expr, finalize_as_binary, &bin_flags)?;
        }

        self.restore_state(saved);

        // ── 6. Call outermost helper from the enclosing function ──
        let call_result = self.fresh_var();
        let nil_acc = self.emit_const(Literal::Nil);
        let mut args = vec![source_0, nil_acc];
        // helper_0 has no tails/heads (k=0), only captured vars
        for (_, vid) in &captured_vars {
            args.push(*vid);
        }
        self.emit(Instruction::Call(
            call_result,
            CallTarget::Local(helpers[0].0),
            args,
        ));
        // An error in the comprehension skips the rest of a try body.
        self.branch_on_pending_exception();

        Ok(call_result)
    }

    /// Split LC qualifiers into generator groups.
    /// Each group: (pattern, source_expr, [filter_exprs]).
    /// Keep binary patterns intact: their sizes and segment boundaries define
    /// how much source each iteration consumes.
    /// Returns (groups, bin_gen_flags).
    pub(super) fn split_lc_qualifiers(
        qualifiers: &[ast::LcQualifier],
    ) -> Result<QualifierGroups<'_>, LowerError> {
        let mut groups: Vec<GeneratorGroup<'_>> = Vec::new();
        let mut bin_flags: Vec<bool> = Vec::new();

        for q in qualifiers {
            match q {
                ast::LcQualifier::Generator(pat, expr, _) => {
                    groups.push((pat.clone(), expr.as_ref(), Vec::new()));
                    bin_flags.push(false);
                }
                ast::LcQualifier::BinGenerator(pat, expr, _) => {
                    // Every field is sized: lower_module rejected unsized
                    // `<=` fields before lowering any function, so this
                    // generator always advances its cursor.
                    groups.push((pat.clone(), expr.as_ref(), Vec::new()));
                    bin_flags.push(true);
                }
                ast::LcQualifier::Filter(expr, _) => {
                    if let Some(last) = groups.last_mut() {
                        last.2.push(expr.as_ref());
                    }
                }
            }
        }

        if groups.is_empty() {
            return Err(LowerError::Unsupported(
                "list comprehension requires at least one generator".into(),
                None,
            ));
        }
        Ok((groups, bin_flags))
    }

    /// Capture expression reads before the generator that shadows their name.
    /// Earlier generator values travel through head parameters, not captures.
    fn collect_lc_captures(
        &mut self,
        groups: &[GeneratorGroup<'_>],
        body: &ast::Expr,
    ) -> Vec<(AtomId, VarId)> {
        let mut bound = HashSet::new();
        let mut referenced = HashSet::new();
        for (index, (pat, source, filters)) in groups.iter().enumerate() {
            let mut reads = HashSet::new();
            if index > 0 {
                self.collect_expr_refs(source, &mut reads);
            }
            crate::warnings::collect_pattern_refs(pat, &mut reads);
            referenced.extend(reads.difference(&bound).copied());
            self.collect_pattern_bindings(pat, &mut bound);
            for filter in filters {
                let mut reads = HashSet::new();
                self.collect_expr_refs(filter, &mut reads);
                referenced.extend(reads.difference(&bound).copied());
            }
        }
        let mut reads = HashSet::new();
        self.collect_expr_refs(body, &mut reads);
        referenced.extend(reads.difference(&bound).copied());
        let mut names: Vec<_> = referenced.into_iter()
            .filter(|name| self.lookup_var(*name).is_some()).collect();
        names.sort_by_key(|name| name.0);
        names.into_iter().map(|name| {
            let var = self.lookup_var(name).expect("capture exists");
            (name, self.materialize_bound_variable(name, var))
        }).collect()
    }

    /// Generator value bindings are fresh; keys and sizes still read the
    /// inputs before this generator. Repeated value names remain comparisons.
    fn bind_generator_pattern(
        &mut self, pattern: &ast::Pattern, element: VarId, binary_bind_only: bool,
    ) {
        self.with_pattern_inputs(self.collect_all_bindings(), |builder| {
            builder.pre_bind_pattern_vars(pattern, element, binary_bind_only);
        });
    }

    /// Build one LC helper function (helper_k of n total).
    ///
    /// Parameters for helper_k:
    ///   iter_k, acc, tail_0…tail_{k-1}, head_0…head_{k-1}, captured…
    ///
    /// The accumulator collects elements in reverse order. When the outermost
    /// helper (k==0) reaches the nil case, it reverses the accumulator via
    /// `lists:reverse/1`.
    #[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
    pub(super) fn build_lc_helper(
        &mut self,
        k: usize,
        n: usize,
        groups: &[GeneratorGroup<'_>],
        helpers: &[(AtomId, String, u32)], // (name_id, mangled, arity)
        captured_vars: &[(AtomId, VarId)],
        body_expr: &ast::Expr,
        finalize_as_binary: bool,
        bin_gen_flags: &[bool],
    ) -> Result<(), LowerError> {
        let (helper_name_id, ref helper_mangled, helper_arity) = helpers[k];
        // Each helper is its own function. Deferred tails recorded while
        // building the previous helper name that helper's variables.
        self.match_contexts.clear();

        // ── Create parameters ──
        let entry_block = self.fresh_block_with_id();
        let iter_param = self.fresh_var();
        let acc_param = self.fresh_var();

        let mut tail_params: Vec<VarId> = Vec::new();
        for _ in 0..k {
            tail_params.push(self.fresh_var());
        }
        let mut head_params: Vec<VarId> = Vec::new();
        for _ in 0..k {
            head_params.push(self.fresh_var());
        }
        let mut env_params: Vec<VarId> = Vec::new();
        for (name, _) in captured_vars {
            let param = self.fresh_var();
            env_params.push(param);
            self.bind_var(*name, param);
        }

        // ── Nil block ──
        let nil_block = self.fresh_block_with_id();
        self.start_block(nil_block);
        if k == 0 {
            // Outermost helper: reverse the accumulator and return.
            let reversed = self.fresh_var();
            self.emit(Instruction::Call(
                reversed,
                CallTarget::Bif("lists_reverse".to_string()),
                vec![acc_param],
            ));
            if finalize_as_binary {
                // Binary comprehension: concatenate the reversed list's
                // bitstrings bit by bit.
                let binary_result = self.fresh_var();
                self.emit(Instruction::Call(
                    binary_result,
                    CallTarget::Bif("list_to_bitstring".to_string()),
                    vec![reversed],
                ));
                self.set_terminator(Terminator::Return(binary_result));
            } else {
                self.set_terminator(Terminator::Return(reversed));
            }
        } else {
            // Chain back to helper_{k-1}: resume iterating the outer list,
            // passing the accumulator through.
            let mut chain_args = vec![tail_params[k - 1], acc_param];
            for item in tail_params[..k - 1].iter() {
                chain_args.push(*item);
            }
            for item in head_params[..k - 1].iter() {
                chain_args.push(*item);
            }
            chain_args.extend_from_slice(&env_params);
            self.set_terminator(Terminator::TailCall(
                CallTarget::Local(helpers[k - 1].0),
                chain_args,
            ));
        }
        self.close_current_block();

        // ── Cons block ──
        let cons_block = self.fresh_block_with_id();
        self.start_block(cons_block);
        self.push_scope();

        let head_var = self.fresh_var();
        let tail_var = self.fresh_var();
        if !bin_gen_flags[k] {
            self.emit(Instruction::ListHead(head_var, iter_param));
            self.emit(Instruction::ListTail(tail_var, iter_param));
        }

        // Re-bind outer generator patterns from their head params. They have
        // already matched, and this generator's match may read them (a
        // segment size), so they are extracted before it runs.
        for j in 0..k {
            self.bind_generator_pattern(&groups[j].0, head_params[j], false);
        }

        let generator_inputs = self.collect_all_bindings();
        let generator_scopes = self.scopes.clone();
        let generator_match_contexts = self.match_contexts.clone();

        // Validate this generator before destructuring, filtering or evaluating
        // its body. A mismatch skips the element with the accumulator unchanged.
        let dispatch_block = self.current_block_id.expect("cons block is open");
        let matched_block = self.fresh_block_with_id();
        let pattern_skip_block = self.fresh_block_with_id();
        self.close_current_block();
        self.start_block(matched_block);

        // Bind current generator pattern.
        let generator_pattern = self.desugar_record_pattern(&groups[k].0)?;
        // Binary-prefix matching defines these placeholders directly. Do not
        // materialize and extract the same prefix again in the innermost body.
        self.bind_generator_pattern(&generator_pattern, head_var, bin_gen_flags[k]);
        // Direct binary matching writes only these destination placeholders.
        // Keep them, but not later filter/body bindings, when compiling the
        // match. Key and size reads still use generator_inputs above.
        let (generator_scopes, generator_match_contexts) = if bin_gen_flags[k] {
            (self.scopes.clone(), self.match_contexts.clone())
        } else {
            (generator_scopes, generator_match_contexts)
        };

        let filters = &groups[k].2;
        let is_innermost = k == n - 1;

        // Helper: build args for a self tail-call.
        // self(tail_var, acc_var, tail_0…tail_{k-1}, head_0…head_{k-1}, captured…)
        let build_self_args = |tail_params: &[VarId],
                               head_params: &[VarId],
                               env_params: &[VarId],
                               tail_var: VarId,
                               acc_var: VarId|
         -> Vec<VarId> {
            let mut a = vec![tail_var, acc_var];
            a.extend_from_slice(tail_params);
            a.extend_from_slice(head_params);
            a.extend_from_slice(env_params);
            a
        };

        if filters.is_empty() {
            // ── No filters ──
            if is_innermost {
                let elem = self.lower_expr(body_expr, false)?;

                // Prepend element to accumulator: new_acc = [elem | acc]
                let new_acc = self.fresh_var();
                self.emit(Instruction::MakeCons(new_acc, elem, acc_param));
                // Tail-call self with updated accumulator
                let self_args =
                    build_self_args(&tail_params, &head_params, &env_params, tail_var, new_acc);
                self.set_terminator(Terminator::TailCall(
                    CallTarget::Local(helper_name_id),
                    self_args,
                ));
            } else {
                // Delegate to helper_{k+1}, passing accumulator through.
                let next_iter = self.lower_expr(groups[k + 1].1, false)?;

                let mut inner_args = vec![next_iter, acc_param];
                // tails for k+1: tail_0…tail_{k-1}, tail_var
                inner_args.extend_from_slice(&tail_params);
                inner_args.push(tail_var);
                // heads for k+1: head_0…head_{k-1}, head_var
                inner_args.extend_from_slice(&head_params);
                inner_args.push(head_var);
                inner_args.extend_from_slice(&env_params);
                self.set_terminator(Terminator::TailCall(
                    CallTarget::Local(helpers[k + 1].0),
                    inner_args,
                ));
            }
        } else {
            // ── With filters ──
            let produce_block = self.fresh_block_with_id();
            let skip_block = self.fresh_block_with_id();

            // Filters run in order and stop at the first that is not true,
            // as on BEAM: `X /= 0.0, 1 / X > 0` never divides by zero.
            // A branch takes its first target only when its condition is the
            // atom `true`.
            for (index, filter_expr) in filters.iter().enumerate() {
                let filter_var = self.lower_expr(filter_expr, false)?;
                let next_block = if index + 1 == filters.len() {
                    produce_block
                } else {
                    self.fresh_block_with_id()
                };
                self.set_terminator(Terminator::Branch(filter_var, next_block, skip_block));
                self.close_current_block();
                if next_block != produce_block {
                    self.start_block(next_block);
                }
            }

            // ── Produce block ──
            self.start_block(produce_block);
            if is_innermost {
                let elem = self.lower_expr(body_expr, false)?;

                // Prepend element to accumulator: new_acc = [elem | acc]
                let new_acc = self.fresh_var();
                self.emit(Instruction::MakeCons(new_acc, elem, acc_param));
                // Tail-call self with updated accumulator
                let self_args =
                    build_self_args(&tail_params, &head_params, &env_params, tail_var, new_acc);
                self.set_terminator(Terminator::TailCall(
                    CallTarget::Local(helper_name_id),
                    self_args,
                ));
            } else {
                let next_iter = self.lower_expr(groups[k + 1].1, false)?;

                let mut inner_args = vec![next_iter, acc_param];
                inner_args.extend_from_slice(&tail_params);
                inner_args.push(tail_var);
                inner_args.extend_from_slice(&head_params);
                inner_args.push(head_var);
                inner_args.extend_from_slice(&env_params);
                self.set_terminator(Terminator::TailCall(
                    CallTarget::Local(helpers[k + 1].0),
                    inner_args,
                ));
            }
            self.close_current_block();

            // ── Skip block ──
            self.start_block(skip_block);
            let skip_args =
                build_self_args(&tail_params, &head_params, &env_params, tail_var, acc_param);
            self.set_terminator(Terminator::TailCall(
                CallTarget::Local(helper_name_id),
                skip_args,
            ));
            self.close_current_block();
        }

        if self.current_block_id.is_some() {
            self.close_current_block();
        }

        self.start_block(pattern_skip_block);
        let skip_args = build_self_args(
            &tail_params, &head_params, &env_params, tail_var, acc_param,
        );
        self.set_terminator(Terminator::TailCall(
            CallTarget::Local(helper_name_id), skip_args,
        ));
        self.close_current_block();

        // Generator variables introduce a fresh scope: unlike ordinary match
        // expressions, they do not pin names already bound outside the generator.
        // The body was lowered first; none of its bindings may supply inputs
        // to the match that runs before it.
        self.scopes = generator_scopes;
        self.match_contexts = generator_match_contexts;
        let match_entry = self.with_pattern_inputs(generator_inputs, |builder| {
            if bin_gen_flags[k] {
                pattern::compile_binary_generator(
                    builder, iter_param, &generator_pattern,
                    ((k + 1 < n).then_some(head_var), tail_var),
                    matched_block, pattern_skip_block, nil_block,
                )
            } else {
                let clause = MatchClause {
                    patterns: vec![generator_pattern],
                    guard: None,
                    body_block: matched_block,
                };
                pattern::compile_match(builder, &[head_var], &[clause], pattern_skip_block)
            }
        });
        self.patch_block_terminator(dispatch_block, Terminator::Jump(match_entry, vec![]));
        self.pop_scope();

        // Binary iterators match prefixes directly, including partial-byte
        // sources. A non-bitstring source is a bad_generator, not exhaustion.
        self.start_block(entry_block);
        if bin_gen_flags[k] {
            let invalid_block = self.fresh_block_with_id();
            let is_bits = self.fresh_var();
            self.emit(Instruction::TypeTest(is_bits, TypeTestKind::IsBitstring, iter_param));
            self.set_terminator(Terminator::Branch(is_bits, cons_block, invalid_block));
            self.close_current_block();
            self.start_block(invalid_block);
            let tag = self.atom_id_for("bad_generator");
            let tag = self.emit_const(Literal::Atom(tag));
            let reason = self.fresh_var();
            self.emit(Instruction::MakeTuple(reason, vec![tag, iter_param]));
            let result = self.fresh_var();
            self.emit(Instruction::RaiseException(result, ExceptionClass::Error, reason));
            self.set_terminator(Terminator::Return(result));
        } else {
            let nil_const = self.emit_const(Literal::Nil);
            let is_cons = self.fresh_var();
            self.emit(Instruction::BinOp(
                is_cons, BinOp::ExactNotEq, iter_param, nil_const,
            ));
            self.set_terminator(Terminator::Branch(is_cons, cons_block, nil_block));
        }
        self.close_current_block();

        // ── Package the helper function ──
        let mut all_params = vec![iter_param, acc_param];
        all_params.extend_from_slice(&tail_params);
        all_params.extend_from_slice(&head_params);
        all_params.extend_from_slice(&env_params);
        let mut helper_func = IrFunction {
            name: helper_name_id,
            arity: helper_arity,
            mangled_name: helper_mangled.clone(),
            blocks: std::mem::take(&mut self.current_blocks),
            protected_values: std::mem::take(&mut self.protected_values),
            entry_block,
            params: all_params,
            line: 0,
        };
        #[cfg(test)]
        crate::passes::assert_valid(&helper_func, "before helper yield insertion");
        insert_backedge_yield_checks(&mut helper_func);
        #[cfg(test)]
        crate::passes::assert_valid(&helper_func, "after helper yield insertion");
        self.functions.push(helper_func);

        Ok(())
    }

}
