//! Pattern match compilation.
//!
//! Compiles Erlang pattern matching into decision trees made of
//! IR basic blocks with type tests, equality checks, and destructuring.
//!
//! Based on Maranget's "Compiling Pattern Matching to Good Decision Trees".

use std::collections::{HashMap, HashSet};
use nera_syntax::ast::{self, AtomId, BinSegment, BinSegmentKind, Endianness, Pattern, Signedness};
#[allow(unused_imports)]
use nera_syntax::ast::MapPatternField;
use crate::ir::*;
use crate::lower::{IrBuilder, MatchContextInfo};

/// Exclusive binary-size ABI limit; also an impossible-size placeholder.
pub(crate) const BINARY_PATTERN_SIZE_LIMIT: i64 = 1_i64 << 60;

/// The same constant segment-size policy for matching and pre-binding.
pub(crate) fn checked_pattern_segment_bits(size: i64, unit: i64) -> Option<i64> {
    size.checked_mul(unit).filter(|bits| (0..BINARY_PATTERN_SIZE_LIMIT).contains(bits))
}

/// A test to perform on a scrutinee value.
#[derive(Debug, Clone)]
pub enum PatternTest {
    /// Value is an atom with this id
    IsAtomEq(AtomId),
    /// Value is an integer with this value
    IsIntegerEq(i64),
    /// Value is a float with this value
    IsFloatEq(f64),
    /// Value is a tuple with this arity
    IsTupleArity(u32),
    /// Value is a non-empty list (cons cell)
    IsCons,
    /// Value is the empty list
    IsNil,
    /// Wildcard (always matches)
    Wildcard,
}

/// Represents a single clause to match against.
#[derive(Debug, Clone)]
pub struct MatchClause {
    pub patterns: Vec<Pattern>,
    pub guard: Option<ast::Guard>,
    pub body_block: BlockId,
}

/// Compile a set of match clauses into IR blocks.
///
/// `scrutinees` are the IR variables holding the values being matched.
/// `clauses` are the match clauses (patterns + guard + body block).
/// `fail_block` is where to jump if no clause matches.
///
/// Returns the entry block for the compiled match.
pub fn compile_match(
    builder: &mut IrBuilder,
    scrutinees: &[VarId],
    clauses: &[MatchClause],
    fail_block: BlockId,
) -> BlockId {
    let empty = HashMap::new();
    compile_match_with_outer_bindings(builder, scrutinees, clauses, fail_block, &empty)
}

/// Like `compile_match`, but with explicit outer-bound variables.
/// Variables in `outer_bindings` trigger equality checks against their VarId
/// instead of being treated as fresh bindings.
pub fn compile_match_with_outer_bindings(
    builder: &mut IrBuilder,
    scrutinees: &[VarId],
    clauses: &[MatchClause],
    fail_block: BlockId,
    outer_bindings: &HashMap<AtomId, VarId>,
) -> BlockId {
    // Keys, sizes and guards read deferred tails without consuming them.
    let outer_matching = std::mem::replace(&mut builder.matching, true);
    let entry = compile_match_impl(
        builder,
        scrutinees,
        clauses,
        fail_block,
        None,
        outer_bindings,
    );
    builder.matching = outer_matching;
    entry
}

/// Match one complete binary-generator prefix. Extraction failure ends the
/// generator; a value mismatch skips the prefix, not the remaining source.
/// `head_tail` receives an optional saved prefix and the exclusive tail cursor
/// before value tests run. The cursor never escapes as an Erlang value.
pub(crate) fn compile_binary_generator(
    builder: &mut IrBuilder,
    source: VarId,
    pattern: &Pattern,
    head_tail: (Option<VarId>, VarId),
    matched: BlockId,
    skip: BlockId,
    done: BlockId,
) -> BlockId {
    debug_assert!(matches!(pattern, Pattern::Binary(..)));
    let outer_matching = std::mem::replace(&mut builder.matching, true);
    builder.push_scope();
    let entry = compile_single_pattern_impl(
        builder, source, pattern, &[], &[], None, matched, done,
        &mut HashSet::new(), &HashMap::new(),
        Some(BinaryGenerator { head: head_tail.0, tail: head_tail.1, skip }),
    );
    builder.pop_scope();
    builder.matching = outer_matching;
    entry
}

#[derive(Clone, Copy)]
struct BinaryGenerator {
    head: Option<VarId>,
    tail: VarId,
    skip: BlockId,
}

/// Internal match compilation.
///
/// `seen_vars`: when `Some`, we are inside sub-pattern recursion for a
/// single clause and must keep accumulating variable names.  When `None`,
/// we are at the top level and `compile_column` will create a fresh set
/// per clause.
fn compile_match_impl(
    builder: &mut IrBuilder,
    scrutinees: &[VarId],
    clauses: &[MatchClause],
    fail_block: BlockId,
    seen_vars: Option<&mut HashSet<AtomId>>,
    outer_bindings: &HashMap<AtomId, VarId>,
) -> BlockId {
    if clauses.is_empty() {
        return fail_block;
    }

    if scrutinees.is_empty() {
        // No more scrutinees to test — first clause with empty patterns wins.
        // Check guard if present, otherwise jump to body.
        let clause = &clauses[0];
        if let Some(guard) = &clause.guard {
            let guard_block = builder.fresh_block_with_id();
            builder.start_block(guard_block);

            let guard_val = builder.lower_guard(guard);
            let true_atom = builder.emit_const(Literal::Atom(builder.true_atom_id)); // ATOM_TRUE

            // Compare guard result to 'true'
            let cmp = builder.fresh_var();
            builder.emit(Instruction::BinOp(
                cmp,
                nera_syntax::ast::BinOp::ExactEq,
                guard_val,
                true_atom,
            ));

            // If guard fails, try next clause
            let rest = compile_match_impl(builder, scrutinees, &clauses[1..], fail_block, seen_vars, outer_bindings);
            builder.set_terminator(Terminator::Branch(cmp, clause.body_block, rest));

            guard_block
        } else {
            clause.body_block
        }
    } else {
        // Pick the first scrutinee to split on
        let scrutinee = scrutinees[0];
        let remaining_scrutinees = &scrutinees[1..];

        compile_column(builder, scrutinee, remaining_scrutinees, clauses, fail_block, seen_vars, outer_bindings)
    }
}

/// Compile matching on a single column (scrutinee).
fn compile_column(
    builder: &mut IrBuilder,
    scrutinee: VarId,
    remaining_scrutinees: &[VarId],
    clauses: &[MatchClause],
    fail_block: BlockId,
    seen_vars: Option<&mut HashSet<AtomId>>,
    outer_bindings: &HashMap<AtomId, VarId>,
) -> BlockId {
    let entry_block = builder.fresh_block_with_id();

    // Build a chain of tests
    let mut current_fail = fail_block;

    // Process clauses in reverse so that later clauses become the fallback
    // for earlier ones
    match seen_vars {
        Some(seen) => {
            // Sub-pattern recursion: use existing seen set (always 1 clause)
            for clause in clauses.iter().rev() {
                let pattern = &clause.patterns[0];
                let remaining_patterns: Vec<Pattern> = clause.patterns[1..].to_vec();

                current_fail = compile_single_pattern(
                    builder,
                    scrutinee,
                    pattern,
                    remaining_scrutinees,
                    &remaining_patterns,
                    clause.guard.as_ref(),
                    clause.body_block,
                    current_fail,
                    seen,
                    outer_bindings,
                );
            }
        }
        None => {
            // Top-level: fresh seen set per clause.
            // Push/pop a scope per clause so that variable bindings from one
            // clause don't leak to subsequent clauses (processed in reverse).
            // Without this, a variable like `R` in clause 2's pattern would
            // overwrite clause 1's `R`, causing the wrong VarId to be used
            // in clause 1's body block.
            for clause in clauses.iter().rev() {
                let pattern = &clause.patterns[0];
                let remaining_patterns: Vec<Pattern> = clause.patterns[1..].to_vec();

                let mut fresh_seen = HashSet::new();
                builder.push_scope();
                current_fail = compile_single_pattern(
                    builder,
                    scrutinee,
                    pattern,
                    remaining_scrutinees,
                    &remaining_patterns,
                    clause.guard.as_ref(),
                    clause.body_block,
                    current_fail,
                    &mut fresh_seen,
                    outer_bindings,
                );
                builder.pop_scope();
            }
        }
    }

    // Build the entry block — just jumps to the first test
    builder.start_block(entry_block);
    builder.set_terminator(Terminator::Jump(current_fail, vec![]));
    builder.close_current_block();

    entry_block
}

/// Compile a single pattern test, returning the block that starts the test.
#[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
fn compile_single_pattern(
    builder: &mut IrBuilder,
    scrutinee: VarId,
    pattern: &Pattern,
    remaining_scrutinees: &[VarId],
    remaining_patterns: &[Pattern],
    guard: Option<&ast::Guard>,
    body_block: BlockId,
    fail_block: BlockId,
    seen_vars: &mut HashSet<AtomId>,
    outer_bindings: &HashMap<AtomId, VarId>,
) -> BlockId {
    compile_single_pattern_impl(
        builder, scrutinee, pattern, remaining_scrutinees, remaining_patterns,
        guard, body_block, fail_block, seen_vars, outer_bindings, None,
    )
}

#[expect(clippy::too_many_arguments, reason = "Explicit pattern continuations and optional binary-generator progress")]
fn compile_single_pattern_impl(
    builder: &mut IrBuilder,
    scrutinee: VarId,
    pattern: &Pattern,
    remaining_scrutinees: &[VarId],
    remaining_patterns: &[Pattern],
    guard: Option<&ast::Guard>,
    body_block: BlockId,
    fail_block: BlockId,
    seen_vars: &mut HashSet<AtomId>,
    outer_bindings: &HashMap<AtomId, VarId>,
    binary_generator: Option<BinaryGenerator>,
) -> BlockId {
    let test_block = builder.fresh_block_with_id();
    builder.start_block(test_block);

    match pattern {
        Pattern::Wildcard(_) => {
            // Always matches — go to body or next scrutinee
            if remaining_scrutinees.is_empty() && remaining_patterns.is_empty() {
                if let Some(guard) = guard {
                    let guard_val = builder.lower_guard(guard);
                    let true_atom = builder.emit_const(Literal::Atom(builder.true_atom_id));
                    let cmp = builder.fresh_var();
                    builder.emit(Instruction::BinOp(
                        cmp,
                        nera_syntax::ast::BinOp::ExactEq,
                        guard_val,
                        true_atom,
                    ));
                    builder.set_terminator(Terminator::Branch(cmp, body_block, fail_block));
                } else {
                    builder.set_terminator(Terminator::Jump(body_block, vec![]));
                }
            } else {
                // Close test_block before compile_match opens new blocks
                builder.close_current_block();
                let next_clauses = vec![MatchClause {
                    patterns: remaining_patterns.to_vec(),
                    guard: guard.cloned(),
                    body_block,
                }];
                let next = compile_match_impl(builder, remaining_scrutinees, &next_clauses, fail_block, Some(seen_vars), outer_bindings);
                // Fix up test_block's terminator
                if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == test_block) {
                    block.terminator = Terminator::Jump(next, vec![]);
                }
            }
        }

        Pattern::Variable(name, _) => {
            // Track which block to patch for wildcard continuation.
            // If we emit an equality check, we create a continue_block
            // and must NOT overwrite test_block's Branch terminator.
            let patch_block = if seen_vars.contains(name) || outer_bindings.contains_key(name) {
                // Repeated variable within the same clause, or variable
                // already bound in outer scope: emit equality check.
                let existing_var = outer_bindings.get(name)
                    .copied()
                    .or_else(|| builder.lookup_var(*name))
                    .expect("bound pattern variable must be resolvable");
                // A bound binary tail may still be a deferred slice rather
                // than a term. Materialize it on this comparison path without
                // consuming the definition needed by sibling clauses.
                let existing_var = if let Some(info) = builder.match_contexts.get(&existing_var).cloned() {
                    let materialized = builder.fresh_var();
                    builder.emit(Instruction::BsMaterialize(
                        materialized, info.match_ctx, info.bit_offset, info.remaining_bits,
                    ));
                    materialized
                } else {
                    existing_var
                };
                let cmp = builder.fresh_var();
                builder.emit(Instruction::BinOp(
                    cmp,
                    nera_syntax::ast::BinOp::ExactEq,
                    existing_var,
                    scrutinee,
                ));
                let continue_block = builder.fresh_block_with_id();
                builder.set_terminator(Terminator::Branch(cmp, continue_block, fail_block));
                builder.close_current_block();
                builder.start_block(continue_block);
                continue_block
            } else {
                // First occurrence — record and bind variable to scrutinee.
                seen_vars.insert(*name);
                builder.bind_var(*name, scrutinee);
                test_block
            };
            // Same as wildcard from here
            if remaining_scrutinees.is_empty() && remaining_patterns.is_empty() {
                if let Some(guard) = guard {
                    let guard_val = builder.lower_guard(guard);
                    let true_atom = builder.emit_const(Literal::Atom(builder.true_atom_id));
                    let cmp = builder.fresh_var();
                    builder.emit(Instruction::BinOp(
                        cmp,
                        nera_syntax::ast::BinOp::ExactEq,
                        guard_val,
                        true_atom,
                    ));
                    builder.set_terminator(Terminator::Branch(cmp, body_block, fail_block));
                } else {
                    builder.set_terminator(Terminator::Jump(body_block, vec![]));
                }
            } else {
                // Close current block before compile_match opens new blocks
                builder.close_current_block();
                let next_clauses = vec![MatchClause {
                    patterns: remaining_patterns.to_vec(),
                    guard: guard.cloned(),
                    body_block,
                }];
                let next = compile_match_impl(builder, remaining_scrutinees, &next_clauses, fail_block, Some(seen_vars), outer_bindings);
                // Fix up the right block's terminator (continue_block or test_block)
                if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == patch_block) {
                    block.terminator = Terminator::Jump(next, vec![]);
                }
            }
        }

        Pattern::Integer(_, _) | Pattern::BigInteger(_, _) => {
            // Test: is_integer(scrutinee) && scrutinee == val
            let test_var = builder.fresh_var();
            builder.emit(Instruction::TypeTest(test_var, TypeTestKind::IsInteger, scrutinee));

            let eq_block = builder.fresh_block_with_id();
            builder.set_terminator(Terminator::Branch(test_var, eq_block, fail_block));

            builder.start_block(eq_block);
            let literal = match pattern {
                Pattern::Integer(n, _) => Literal::Integer(*n),
                Pattern::BigInteger(n, _) => Literal::BigInteger(n.clone()),
                _ => unreachable!(),
            };
            let expected = builder.emit_const(literal);
            let cmp = builder.fresh_var();
            builder.emit(Instruction::BinOp(
                cmp,
                nera_syntax::ast::BinOp::ExactEq,
                scrutinee,
                expected,
            ));

            if remaining_scrutinees.is_empty() && remaining_patterns.is_empty() {
                if let Some(guard) = guard {
                    let guard_block = builder.fresh_block_with_id();
                    builder.set_terminator(Terminator::Branch(cmp, guard_block, fail_block));
                    builder.start_block(guard_block);
                    let guard_val = builder.lower_guard(guard);
                    let true_atom = builder.emit_const(Literal::Atom(builder.true_atom_id));
                    let gcmp = builder.fresh_var();
                    builder.emit(Instruction::BinOp(
                        gcmp,
                        nera_syntax::ast::BinOp::ExactEq,
                        guard_val,
                        true_atom,
                    ));
                    builder.set_terminator(Terminator::Branch(gcmp, body_block, fail_block));
                } else {
                    builder.set_terminator(Terminator::Branch(cmp, body_block, fail_block));
                }
            } else {
                let cont_block = builder.fresh_block_with_id();
                builder.set_terminator(Terminator::Branch(cmp, cont_block, fail_block));
                builder.start_block(cont_block);
                builder.close_current_block(); // close before compile_match
                let next_clauses = vec![MatchClause {
                    patterns: remaining_patterns.to_vec(),
                    guard: guard.cloned(),
                    body_block,
                }];
                let next = compile_match_impl(builder, remaining_scrutinees, &next_clauses, fail_block, Some(seen_vars), outer_bindings);
                if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == cont_block) {
                    block.terminator = Terminator::Jump(next, vec![]);
                }
            }
        }

        Pattern::Atom(atom_id, _) => {
            let test_var = builder.fresh_var();
            builder.emit(Instruction::TypeTest(test_var, TypeTestKind::IsAtom, scrutinee));

            let eq_block = builder.fresh_block_with_id();
            builder.set_terminator(Terminator::Branch(test_var, eq_block, fail_block));

            builder.start_block(eq_block);
            let expected = builder.emit_const(Literal::Atom(*atom_id));
            let cmp = builder.fresh_var();
            builder.emit(Instruction::BinOp(
                cmp,
                nera_syntax::ast::BinOp::ExactEq,
                scrutinee,
                expected,
            ));

            if remaining_scrutinees.is_empty() && remaining_patterns.is_empty() {
                if let Some(guard) = guard {
                    let guard_block = builder.fresh_block_with_id();
                    builder.set_terminator(Terminator::Branch(cmp, guard_block, fail_block));
                    builder.start_block(guard_block);
                    let guard_val = builder.lower_guard(guard);
                    let true_atom = builder.emit_const(Literal::Atom(builder.true_atom_id));
                    let gcmp = builder.fresh_var();
                    builder.emit(Instruction::BinOp(
                        gcmp,
                        nera_syntax::ast::BinOp::ExactEq,
                        guard_val,
                        true_atom,
                    ));
                    builder.set_terminator(Terminator::Branch(gcmp, body_block, fail_block));
                } else {
                    builder.set_terminator(Terminator::Branch(cmp, body_block, fail_block));
                }
            } else {
                let cont_block = builder.fresh_block_with_id();
                builder.set_terminator(Terminator::Branch(cmp, cont_block, fail_block));
                builder.start_block(cont_block);
                builder.close_current_block();
                let next_clauses = vec![MatchClause {
                    patterns: remaining_patterns.to_vec(),
                    guard: guard.cloned(),
                    body_block,
                }];
                let next = compile_match_impl(builder, remaining_scrutinees, &next_clauses, fail_block, Some(seen_vars), outer_bindings);
                if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == cont_block) {
                    block.terminator = Terminator::Jump(next, vec![]);
                }
            }
        }

        Pattern::Nil(_) => {
            // Test: scrutinee is nil
            let test_var = builder.fresh_var();
            let nil_val = builder.emit_const(Literal::Nil);
            builder.emit(Instruction::BinOp(
                test_var,
                nera_syntax::ast::BinOp::ExactEq,
                scrutinee,
                nil_val,
            ));

            if remaining_scrutinees.is_empty() && remaining_patterns.is_empty() {
                if let Some(guard) = guard {
                    let guard_block = builder.fresh_block_with_id();
                    builder.set_terminator(Terminator::Branch(test_var, guard_block, fail_block));
                    builder.start_block(guard_block);
                    let guard_val = builder.lower_guard(guard);
                    let true_atom = builder.emit_const(Literal::Atom(builder.true_atom_id));
                    let gcmp = builder.fresh_var();
                    builder.emit(Instruction::BinOp(
                        gcmp,
                        nera_syntax::ast::BinOp::ExactEq,
                        guard_val,
                        true_atom,
                    ));
                    builder.set_terminator(Terminator::Branch(gcmp, body_block, fail_block));
                } else {
                    builder.set_terminator(Terminator::Branch(test_var, body_block, fail_block));
                }
            } else {
                let cont_block = builder.fresh_block_with_id();
                builder.set_terminator(Terminator::Branch(test_var, cont_block, fail_block));
                builder.start_block(cont_block);
                builder.close_current_block();
                let next_clauses = vec![MatchClause {
                    patterns: remaining_patterns.to_vec(),
                    guard: guard.cloned(),
                    body_block,
                }];
                let next = compile_match_impl(builder, remaining_scrutinees, &next_clauses, fail_block, Some(seen_vars), outer_bindings);
                if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == cont_block) {
                    block.terminator = Terminator::Jump(next, vec![]);
                }
            }
        }

        Pattern::Tuple(elem_patterns, _) => {
            let arity = elem_patterns.len() as u32;

            // Test: is_tuple(scrutinee)
            let is_tup = builder.fresh_var();
            builder.emit(Instruction::TypeTest(is_tup, TypeTestKind::IsTuple, scrutinee));

            let size_check_block = builder.fresh_block_with_id();
            builder.set_terminator(Terminator::Branch(is_tup, size_check_block, fail_block));

            // Check arity
            builder.start_block(size_check_block);
            let size_var = builder.fresh_var();
            builder.emit(Instruction::TupleSize(size_var, scrutinee));
            let expected_size = builder.emit_const(Literal::Integer(arity as i64));
            let size_match = builder.fresh_var();
            builder.emit(Instruction::BinOp(
                size_match,
                nera_syntax::ast::BinOp::ExactEq,
                size_var,
                expected_size,
            ));

            let destructure_block = builder.fresh_block_with_id();
            builder.set_terminator(Terminator::Branch(size_match, destructure_block, fail_block));

            // Destructure: extract elements
            builder.start_block(destructure_block);
            let mut elem_vars = Vec::with_capacity(arity as usize);
            for i in 0..arity {
                let elem_var = builder.fresh_var();
                builder.emit(Instruction::TupleElement(elem_var, scrutinee, i));
                elem_vars.push(elem_var);
            }
            // Close destructure_block before compile_match (which opens new blocks)
            builder.close_current_block();

            // Now match each element against its sub-pattern
            // Combine element scrutinees with remaining scrutinees
            let mut all_scrutinees = elem_vars;
            all_scrutinees.extend_from_slice(remaining_scrutinees);

            let mut all_patterns = elem_patterns.clone();
            all_patterns.extend_from_slice(remaining_patterns);

            let next_clauses = vec![MatchClause {
                patterns: all_patterns,
                guard: guard.cloned(),
                body_block,
            }];
            let next = compile_match_impl(builder, &all_scrutinees, &next_clauses, fail_block, Some(seen_vars), outer_bindings);

            // Fix up destructure_block's terminator to jump to the sub-match entry
            if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == destructure_block) {
                block.terminator = Terminator::Jump(next, vec![]);
            }
        }

        Pattern::Cons(head_pat, tail_pat, _) => {
            // Test: is_cons(scrutinee) - check it's a list
            let is_cons = builder.fresh_var();
            builder.emit(Instruction::TypeTest(is_cons, TypeTestKind::IsList, scrutinee));

            let cons_check_block = builder.fresh_block_with_id();
            builder.set_terminator(Terminator::Branch(is_cons, cons_check_block, fail_block));

            // Check it's not nil (it's a proper cons cell)
            builder.start_block(cons_check_block);
            let not_nil = builder.fresh_var();
            let nil_val = builder.emit_const(Literal::Nil);
            builder.emit(Instruction::BinOp(
                not_nil,
                nera_syntax::ast::BinOp::ExactNotEq,
                scrutinee,
                nil_val,
            ));

            let destructure_block = builder.fresh_block_with_id();
            builder.set_terminator(Terminator::Branch(not_nil, destructure_block, fail_block));

            // Destructure: extract head and tail
            builder.start_block(destructure_block);
            let head_var = builder.fresh_var();
            builder.emit(Instruction::ListHead(head_var, scrutinee));
            let tail_var = builder.fresh_var();
            builder.emit(Instruction::ListTail(tail_var, scrutinee));
            // Close destructure_block before compile_match
            builder.close_current_block();

            // Sub-match head and tail patterns
            let mut all_scrutinees = vec![head_var, tail_var];
            all_scrutinees.extend_from_slice(remaining_scrutinees);

            let mut all_patterns = vec![*head_pat.clone(), *tail_pat.clone()];
            all_patterns.extend_from_slice(remaining_patterns);

            let next_clauses = vec![MatchClause {
                patterns: all_patterns,
                guard: guard.cloned(),
                body_block,
            }];
            let next = compile_match_impl(builder, &all_scrutinees, &next_clauses, fail_block, Some(seen_vars), outer_bindings);

            // Fix up destructure_block's terminator
            if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == destructure_block) {
                block.terminator = Terminator::Jump(next, vec![]);
            }
        }

        Pattern::Match(p1, p2, _) => {
            // Alias pattern: both p1 and p2 must match the same scrutinee
            // Close the test_block before compile_match opens new blocks
            builder.close_current_block();

            let mut all_scrutinees = vec![scrutinee, scrutinee];
            all_scrutinees.extend_from_slice(remaining_scrutinees);

            let mut all_patterns = vec![*p1.clone(), *p2.clone()];
            all_patterns.extend_from_slice(remaining_patterns);

            let next_clauses = vec![MatchClause {
                patterns: all_patterns,
                guard: guard.cloned(),
                body_block,
            }];
            let next = compile_match_impl(builder, &all_scrutinees, &next_clauses, fail_block, Some(seen_vars), outer_bindings);

            // Fix up test_block's terminator
            if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == test_block) {
                block.terminator = Terminator::Jump(next, vec![]);
            }
        }

        Pattern::Float(val, _) => {
            let test_var = builder.fresh_var();
            builder.emit(Instruction::TypeTest(test_var, TypeTestKind::IsFloat, scrutinee));

            let eq_block = builder.fresh_block_with_id();
            builder.set_terminator(Terminator::Branch(test_var, eq_block, fail_block));

            builder.start_block(eq_block);
            let expected = builder.emit_const(Literal::Float(*val));
            let cmp = builder.fresh_var();
            builder.emit(Instruction::BinOp(
                cmp,
                nera_syntax::ast::BinOp::ExactEq,
                scrutinee,
                expected,
            ));
            builder.set_terminator(Terminator::Branch(cmp, body_block, fail_block));
        }

        Pattern::Binary(segments, _span) => {
            // Binary pattern matching: type test → size check → sequential segment extraction.
            //
            // Strategy:
            // 1. TypeTest(IsBinary) — verify it's a binary
            // 2. Check minimum size in BITS — sum of all fixed-size segments
            // 3. For each segment, extract and bind/compare:
            //    - Integer segments: BsGetIntegerInline → bind variable or compare literal
            //    - Float segments: BinaryGetFloat → bind variable
            //    - Binary/rest segments: deferred materialization
            // 4. Track BIT offset through segments

            let segments = expand_binary_pattern_strings(segments);
            let segments = &segments;

            // Step 1: Is this a binary/bitstring?
            let is_bin = builder.fresh_var();
            builder.emit(Instruction::TypeTest(is_bin, TypeTestKind::IsBitstring, scrutinee));

            let size_check_block = builder.fresh_block_with_id();
            builder.set_terminator(Terminator::Branch(is_bin, size_check_block, fail_block));

            // Step 2: Get binary size and check minimum.
            // BsMatchStart2 returns (match_ctx, base_ptr, total_bits).
            builder.start_block(size_check_block);
            let mc_var = builder.fresh_var();
            let mc_base_ptr = builder.fresh_var();
            let total_bits = builder.fresh_var();
            builder.emit(Instruction::BsMatchStart2(mc_var, mc_base_ptr, total_bits, scrutinee));
            let initial_bit_offset = builder.fresh_var();
            builder.emit(Instruction::BsMatchBitOffset(initial_bit_offset, scrutinee));

            // Compute minimum required BITS from fixed-size segments
            let mut min_bits: i64 = 0;
            let mut has_rest_segment = false;
            let mut has_variable_utf = false;
            let mut has_dynamic_size = false;
            for seg in segments {
                match seg.type_spec.typ {
                    BinSegmentKind::Binary | BinSegmentKind::Bitstring if seg.size.is_none() => {
                        // Rest segment `/binary` or `/bitstring` — consumes remainder
                        has_rest_segment = true;
                    }
                    BinSegmentKind::Utf8 => {
                        min_bits = min_bits.saturating_add(8); // Minimum 1 byte per UTF-8 codepoint
                        has_variable_utf = true;
                    }
                    BinSegmentKind::Utf16 => {
                        min_bits = min_bits.saturating_add(16); // Minimum 2 bytes per UTF-16 codepoint
                        has_variable_utf = true;
                    }
                    BinSegmentKind::Utf32 => {
                        min_bits = min_bits.saturating_add(32); // Always 4 bytes per UTF-32 codepoint
                    }
                    _ => {
                        // Fixed-size segment: total_bits = size * effective_unit
                        let unit = IrBuilder::effective_unit(&seg.type_spec);
                        let size = match &seg.size {
                            Some(size_expr) => {
                                if let ast::Expr::Integer(n, _) = size_expr.as_ref() {
                                    *n
                                } else {
                                    // dynamic size — contributes 0 to min_bits
                                    has_dynamic_size = true;
                                    0
                                }
                            }
                            None => match seg.type_spec.typ {
                                BinSegmentKind::Integer => 8,
                                BinSegmentKind::Float => 64,
                                _ => 8,
                            },
                        };
                        if let Some(bits) = size.checked_mul(unit).filter(|n| *n >= 0) {
                            if let Some(sum) = min_bits.checked_add(bits) { min_bits = sum; }
                            else { has_dynamic_size = true; }
                        } else { has_dynamic_size = true; }
                    }
                }
            }

            // Check: total_bits >= min_bits (or == for exact match)
            let expected_bits = builder.emit_const(Literal::Integer(min_bits));
            let size_ok = builder.fresh_var();
            if has_rest_segment || has_variable_utf || has_dynamic_size || binary_generator.is_some() {
                // Prefix generators, rest segments and variable widths may leave bits over.
                builder.emit(Instruction::BinOp(
                    size_ok,
                    nera_syntax::ast::BinOp::GtEq,
                    total_bits,
                    expected_bits,
                ));
            } else {
                builder.emit(Instruction::BinOp(
                    size_ok,
                    nera_syntax::ast::BinOp::ExactEq,
                    total_bits,
                    expected_bits,
                ));
            }

            let mut extract_block = builder.fresh_block_with_id();
            builder.set_terminator(Terminator::Branch(size_ok, extract_block, fail_block));

            // Step 3: Extract segments sequentially — tracking BIT offset
            builder.start_block(extract_block);
            let mut bit_offset_var = builder.emit_const(Literal::Integer(0));
            // Track compile-time known bit offset to fold constant arithmetic
            let mut known_bit_offset: Option<i64> = Some(0);

            // A dynamic field must leave room for every later fixed field.
            // Reserve that space once, avoiding repeated guards after it.
            // UTF decoding consumes a variable number of bits and invalidates
            // this reservation; its next field establishes a new one.
            let mut fixed_tail = vec![0_i64; segments.len() + 1];
            let mut fixed_tail_fields = vec![0_usize; segments.len() + 1];
            for (index, seg) in segments.iter().enumerate().rev() {
                let fixed = match seg.type_spec.typ {
                    BinSegmentKind::Utf8 | BinSegmentKind::Utf16 | BinSegmentKind::Utf32 => 0,
                    BinSegmentKind::Binary | BinSegmentKind::Bitstring if seg.size.is_none() => 0,
                    _ => {
                        let size = match seg.size.as_deref() {
                            Some(ast::Expr::Integer(n, _)) => Some(*n),
                            None => Some(if seg.type_spec.typ == BinSegmentKind::Float { 64 } else { 8 }),
                            _ => None,
                        };
                        size.and_then(|n| n.checked_mul(IrBuilder::effective_unit(&seg.type_spec)))
                            .filter(|n| *n >= 0).unwrap_or(0)
                    }
                };
                // 2^60 exceeds the binary-size ABI. This cap always forces
                // failure for an impossible tail, without overflowing the IR.
                fixed_tail[index] = fixed_tail[index + 1].saturating_add(fixed).min(BINARY_PATTERN_SIZE_LIMIT);
                fixed_tail_fields[index] = fixed_tail_fields[index + 1] + usize::from(fixed != 0);
            }
            let mut fixed_fields_reserved = false;

            // Collect sub-scrutinees and sub-patterns for variables bound in segments
            let mut seg_scrutinees = Vec::new();
            let mut seg_patterns = Vec::new();
            let mut generator_failures = Vec::new();

            let mut earlier_segments = HashSet::new();
            for (segment_index, seg) in segments.iter().enumerate() {
                let flags = BinFlags {
                    signedness: match seg.type_spec.signedness {
                        Signedness::Unsigned => 0,
                        Signedness::Signed => 1,
                    },
                    endianness: match seg.type_spec.endianness {
                        Endianness::Big => 0,
                        Endianness::Little => 1,
                        Endianness::Native => 2,
                    },
                };
                let unit = IrBuilder::effective_unit(&seg.type_spec);

                match seg.type_spec.typ {
                    BinSegmentKind::Binary | BinSegmentKind::Bitstring if seg.size.is_none() => {
                        // Rest segment: compute remaining BITS but DON'T allocate a sub-binary.
                        // Register a match context for deferred materialization.
                        let remaining_bits = if let Some(off) = known_bit_offset {
                            if off == 0 {
                                total_bits
                            } else {
                                let off_const = builder.emit_const(Literal::Integer(off));
                                let rem = builder.fresh_var();
                                builder.emit(Instruction::BinOp(
                                    rem,
                                    nera_syntax::ast::BinOp::Sub,
                                    total_bits,
                                    off_const,
                                ));
                                rem
                            }
                        } else {
                            let rem = builder.fresh_var();
                            builder.emit(Instruction::BinOp(
                                rem,
                                nera_syntax::ast::BinOp::Sub,
                                total_bits,
                                bit_offset_var,
                            ));
                            rem
                        };
                        // An unsized tail must contain a whole number of units.
                        // This constrains its length, not the input's bit alignment.
                        if unit > 1 {
                            let unit_var = builder.emit_const(Literal::Integer(unit));
                            let remainder = builder.fresh_var();
                            builder.emit(Instruction::BinOp(
                                remainder, nera_syntax::ast::BinOp::Rem, remaining_bits, unit_var,
                            ));
                            let zero = builder.emit_const(Literal::Integer(0));
                            let unit_ok = builder.fresh_var();
                            builder.emit(Instruction::BinOp(
                                unit_ok, nera_syntax::ast::BinOp::ExactEq, remainder, zero,
                            ));
                            let continue_block = builder.fresh_block_with_id();
                            builder.set_terminator(Terminator::Branch(unit_ok, continue_block, fail_block));
                            builder.start_block(continue_block);
                            extract_block = continue_block;
                        }
                        match &seg.value {
                            Pattern::Variable(name, _)
                                if seen_vars.contains(name) || outer_bindings.contains_key(name) =>
                            {
                                // A bound tail is compared as a whole sub-binary.
                                let tail = builder.fresh_var();
                                builder.emit(Instruction::BsMaterialize(
                                    tail, scrutinee, bit_offset_var, remaining_bits,
                                ));
                                seg_scrutinees.push(tail);
                                seg_patterns.push(seg.value.clone());
                            }
                            // Bind variable to a placeholder. The actual sub-binary will be
                            // materialized on demand if the variable escapes.
                            Pattern::Variable(name, _) => {
                                seen_vars.insert(*name);
                                let rest_var = builder.fresh_var();
                                if let Some(existing) = builder.lookup_pattern_var(*name) {
                                    builder.match_contexts.insert(existing, MatchContextInfo {
                                        _base_ptr: mc_base_ptr,
                                        bit_offset: bit_offset_var,
                                        remaining_bits,
                                        match_ctx: mc_var,
                                    });
                                } else {
                                    builder.bind_var(*name, rest_var);
                                    builder.match_contexts.insert(rest_var, MatchContextInfo {
                                        _base_ptr: mc_base_ptr,
                                        bit_offset: bit_offset_var,
                                        remaining_bits,
                                        match_ctx: mc_var,
                                    });
                                }
                            }
                            _ => {}
                        }
                        // Don't advance offset since this consumes the rest
                    }
                    BinSegmentKind::Utf8 | BinSegmentKind::Utf16 | BinSegmentKind::Utf32 => {
                        // UTF segment extraction: decode one codepoint, get bits_consumed.
                        // bits_consumed == 0 signals decode failure → branch to fail_block.
                        let codepoint = builder.fresh_var();
                        let bits_consumed = builder.fresh_var();
                        // Raw UTF readers use the containing byte as their base,
                        // just like integer extraction. Include a reused cursor's
                        // intra-byte prefix in both the offset and window end.
                        let extraction_offset = builder.fresh_var();
                        builder.emit(Instruction::BinOp(extraction_offset,
                            ast::BinOp::Add, bit_offset_var, initial_bit_offset));
                        let extraction_end = builder.fresh_var();
                        builder.emit(Instruction::BinOp(extraction_end,
                            ast::BinOp::Add, total_bits, initial_bit_offset));

                        match seg.type_spec.typ {
                            BinSegmentKind::Utf8 => {
                                builder.emit(Instruction::BsGetUtf8(
                                    codepoint, bits_consumed, mc_base_ptr, extraction_offset, extraction_end,
                                ));
                            }
                            BinSegmentKind::Utf16 => {
                                builder.emit(Instruction::BsGetUtf16(
                                    codepoint, bits_consumed, mc_base_ptr, extraction_offset, extraction_end, flags,
                                ));
                            }
                            BinSegmentKind::Utf32 => {
                                builder.emit(Instruction::BsGetUtf32(
                                    codepoint, bits_consumed, mc_base_ptr, extraction_offset, extraction_end, flags,
                                ));
                            }
                            _ => unreachable!(),
                        }

                        // Check for decode failure: bits_consumed == 0 → fail
                        let zero = builder.emit_const(Literal::Integer(0));
                        let is_zero = builder.fresh_var();
                        builder.emit(Instruction::BinOp(
                            is_zero,
                            nera_syntax::ast::BinOp::ExactEq,
                            bits_consumed,
                            zero,
                        ));
                        let continue_block = builder.fresh_block_with_id();
                        builder.set_terminator(Terminator::Branch(is_zero, fail_block, continue_block));
                        builder.start_block(continue_block);
                        // Update extract_block so post-loop terminator patching
                        // targets this block, not the one before the Branch.
                        extract_block = continue_block;

                        // Bind the codepoint variable. A bound variable falls
                        // through to an equality sub-pattern.
                        match &seg.value {
                            Pattern::Variable(name, _)
                                if !seen_vars.contains(name) && !outer_bindings.contains_key(name) =>
                            {
                                bind_segment_value(builder, seen_vars, *name, codepoint);
                            }
                            Pattern::Wildcard(_) => {
                                // Don't bind, just skip
                            }
                            Pattern::Integer(_, _) | Pattern::BigInteger(_, _) | Pattern::Atom(_, _) => {
                                seg_scrutinees.push(codepoint);
                                seg_patterns.push(seg.value.clone());
                            }
                            _ => {
                                seg_scrutinees.push(codepoint);
                                seg_patterns.push(seg.value.clone());
                            }
                        }

                        // Advance bit offset by actual bits consumed (variable)
                        let new_offset = builder.fresh_var();
                        builder.emit(Instruction::BinOp(
                            new_offset,
                            nera_syntax::ast::BinOp::Add,
                            bit_offset_var,
                            bits_consumed,
                        ));
                        bit_offset_var = new_offset;
                        known_bit_offset = None; // Variable-width
                        fixed_fields_reserved = false;
                    }
                    _ => {
                        // Integer, float, or fixed-size binary segment
                        let is_float_seg = seg.type_spec.typ == BinSegmentKind::Float;
                        let is_binary_seg = matches!(seg.type_spec.typ, BinSegmentKind::Binary | BinSegmentKind::Bitstring);

                        // Compute total bits for this segment: size * unit
                        let known_total_bits: Option<i64> = match &seg.size {
                            Some(size_expr) => {
                                if let ast::Expr::Integer(n, _) = size_expr.as_ref() {
                                    checked_pattern_segment_bits(*n, unit)
                                } else {
                                    None
                                }
                            }
                            None => Some(match seg.type_spec.typ {
                                BinSegmentKind::Integer => 8,
                                BinSegmentKind::Float => 64,
                                _ => 8,
                            } * unit),
                        };

                        let seg_bit_count = match known_total_bits {
                            Some(n) => builder.emit_const(Literal::Integer(n)),
                            None => {
                                let size_var = if let Some(size_expr) = &seg.size {
                                    builder.lower_pattern_expr(size_expr, &earlier_segments).unwrap_or_else(|_| {
                                        builder.emit_const(Literal::Integer(8))
                                    })
                                } else {
                                    builder.emit_const(Literal::Integer(8))
                                };
                                let unit_var = builder.emit_const(Literal::Integer(unit));
                                let checked = builder.fresh_var();
                                builder.emit(Instruction::Call(checked,
                                    CallTarget::Bif("binary_match_segment_size".to_string()), vec![size_var, unit_var]));
                                let failed = builder.fresh_var();
                                builder.emit(Instruction::TypeTest(failed, TypeTestKind::IsNil, checked));
                                let success = builder.fresh_block_with_id();
                                builder.set_terminator(Terminator::Branch(failed, fail_block, success));
                                builder.start_block(success);
                                extract_block = success;
                                checked
                            }
                        };

                        // Runtime bounds check for dynamic-sized segments
                        if known_total_bits.is_none() || (known_bit_offset.is_none() && !fixed_fields_reserved) {
                            let remaining = builder.fresh_var();
                            builder.emit(Instruction::BinOp(
                                remaining,
                                nera_syntax::ast::BinOp::Sub,
                                total_bits,
                                bit_offset_var,
                            ));
                            // A single later guard is cheaper to keep local.
                            // Reserve a tail only when it replaces multiple guards.
                            let reserve_tail = fixed_tail_fields[segment_index + 1] > 1;
                            let remaining = if reserve_tail {
                                let tail = builder.emit_const(Literal::Integer(fixed_tail[segment_index + 1]));
                                let available = builder.fresh_var();
                                builder.emit(Instruction::BinOp(available,
                                    nera_syntax::ast::BinOp::Sub, remaining, tail));
                                available
                            } else { remaining };
                            let enough = builder.fresh_var();
                            builder.emit(Instruction::BinOp(
                                enough,
                                nera_syntax::ast::BinOp::GtEq,
                                remaining,
                                seg_bit_count,
                            ));
                            let continue_block = builder.fresh_block_with_id();
                            builder.set_terminator(Terminator::Branch(enough, continue_block, fail_block));
                            builder.start_block(continue_block);
                            extract_block = continue_block;
                            fixed_fields_reserved = reserve_tail;
                        }

                        // Check if this segment's value is a wildcard
                        let is_wildcard = matches!(&seg.value, Pattern::Wildcard(_));

                        // Only emit extraction if the value is actually used
                        if !is_wildcard || is_float_seg {
                            let extracted = builder.fresh_var();
                            if is_float_seg {
                                // Float segments: use BinaryGetFloat (takes bit offset)
                                builder.emit(Instruction::BinaryGetFloat(
                                    extracted,
                                    scrutinee,
                                    bit_offset_var,
                                    seg_bit_count,
                                    flags,
                                ));
                                // The float reader returns nil on malformed or nonfinite input.
                                let failed = builder.fresh_var();
                                builder.emit(Instruction::TypeTest(failed, TypeTestKind::IsNil, extracted));
                                if binary_generator.is_some() {
                                    // Nonfinite floats reject this element, not the
                                    // rest of the source. Finish consuming its other
                                    // fields before entering the skip continuation.
                                    generator_failures.push(failed);
                                } else {
                                    let success = builder.fresh_block_with_id();
                                    builder.set_terminator(Terminator::Branch(failed, fail_block, success));
                                    builder.start_block(success);
                                    extract_block = success;
                                }
                            } else if is_binary_seg {
                                // Recursive callers may pass an advanced match
                                // context, so resolve its underlying binary and offset.
                                builder.emit(Instruction::BsMaterialize(
                                    extracted,
                                    scrutinee,
                                    bit_offset_var,
                                    seg_bit_count,
                                ));
                            } else if !is_wildcard {
                                // Integer segments: use inlined match context extraction
                                // The raw base starts at the containing byte; logical
                                // offsets and remaining sizes exclude the prefix bits.
                                let extraction_offset = builder.fresh_var();
                                builder.emit(Instruction::BinOp(extraction_offset,
                                    nera_syntax::ast::BinOp::Add, bit_offset_var, initial_bit_offset));
                                builder.emit(Instruction::BsGetIntegerInline(
                                    extracted,
                                    mc_base_ptr,
                                    extraction_offset,
                                    seg_bit_count,
                                    flags,
                                ));
                            }

                            // Handle the segment value pattern. A bound variable
                            // falls through to an equality sub-pattern.
                            match &seg.value {
                                Pattern::Variable(name, _)
                                    if !seen_vars.contains(name) && !outer_bindings.contains_key(name) =>
                                {
                                    bind_segment_value(builder, seen_vars, *name, extracted);
                                }
                                Pattern::Wildcard(_) => {
                                    // Don't bind, just skip
                                }
                                Pattern::Integer(_, _) | Pattern::BigInteger(_, _) | Pattern::Atom(_, _) => {
                                    seg_scrutinees.push(extracted);
                                    seg_patterns.push(seg.value.clone());
                                }
                                _ => {
                                    seg_scrutinees.push(extracted);
                                    seg_patterns.push(seg.value.clone());
                                }
                            }
                        }

                        // Advance BIT offset using compile-time folding when possible
                        let constant_offset = known_bit_offset.zip(known_total_bits)
                            .and_then(|(off, bits)| off.checked_add(bits));
                        if let Some(new_off) = constant_offset {
                            bit_offset_var = builder.emit_const(Literal::Integer(new_off));
                            known_bit_offset = Some(new_off);
                        } else {
                            let new_offset = builder.fresh_var();
                            builder.emit(Instruction::BinOp(
                                new_offset,
                                nera_syntax::ast::BinOp::Add,
                                bit_offset_var,
                                seg_bit_count,
                            ));
                            bit_offset_var = new_offset;
                            known_bit_offset = None;
                        }
                    }
                }
                crate::warnings::collect_pattern_bindings(&seg.value, &mut earlier_segments);
            }

            // The initial size check is only a minimum for dynamic sizes and
            // variable-width UTF. Without a rest segment, the match must also
            // consume the entire input before any guard or continuation runs.
            if binary_generator.is_none() && !has_rest_segment && (has_dynamic_size || has_variable_utf) {
                let consumed_all = builder.fresh_var();
                builder.emit(Instruction::BinOp(
                    consumed_all, nera_syntax::ast::BinOp::ExactEq, bit_offset_var, total_bits,
                ));
                let complete_block = builder.fresh_block_with_id();
                builder.set_terminator(Terminator::Branch(consumed_all, complete_block, fail_block));
                builder.start_block(complete_block);
                extract_block = complete_block;
            }

            // A generator owns this cursor exclusively. Reuse it for the tail
            // instead of copying the entire remainder at every unaligned step.
            // Only outer generators need a saved prefix for later helpers to
            // reconstruct their bindings; innermost bodies use the values above.
            if let Some(generator) = binary_generator {
                if let Some(head) = generator.head {
                    let zero = builder.emit_const(Literal::Integer(0));
                    builder.emit(Instruction::BsMaterialize(
                        head, scrutinee, zero, bit_offset_var,
                    ));
                }
                builder.emit(Instruction::BsMatchCtxAdvance(mc_var, bit_offset_var));
                builder.emit(Instruction::Move(generator.tail, mc_var));
                if let Some((&first, rest)) = generator_failures.split_first() {
                    let failed = rest.iter().fold(first, |failed, next| {
                        let either = builder.fresh_var();
                        builder.emit(Instruction::BinOp(either, ast::BinOp::Or, failed, *next));
                        either
                    });
                    let success = builder.fresh_block_with_id();
                    builder.set_terminator(Terminator::Branch(failed, generator.skip, success));
                    builder.start_block(success);
                    extract_block = success;
                }
            }
            let fail_block = binary_generator.map_or(fail_block, |generator| generator.skip);

            // Close extract_block before compile_match
            builder.close_current_block();

            // If there are sub-patterns to match (literal segments), do it
            if seg_scrutinees.is_empty() {
                // No sub-patterns — just jump to body (or next match)
                if remaining_scrutinees.is_empty() && remaining_patterns.is_empty() {
                    if let Some(guard) = guard {
                        let guard_block = builder.fresh_block_with_id();
                        builder.start_block(guard_block);
                        let guard_val = builder.lower_guard(guard);
                        let true_atom = builder.emit_const(Literal::Atom(builder.true_atom_id));
                        let gcmp = builder.fresh_var();
                        builder.emit(Instruction::BinOp(
                            gcmp,
                            nera_syntax::ast::BinOp::ExactEq,
                            guard_val,
                            true_atom,
                        ));
                        builder.set_terminator(Terminator::Branch(gcmp, body_block, fail_block));
                        if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == extract_block) {
                            block.terminator = Terminator::Jump(guard_block, vec![]);
                        }
                    } else {
                        if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == extract_block) {
                            block.terminator = Terminator::Jump(body_block, vec![]);
                        }
                    }
                } else {
                    // More scrutinees to match
                    let next_clauses = vec![MatchClause {
                        patterns: remaining_patterns.to_vec(),
                        guard: guard.cloned(),
                        body_block,
                    }];
                    let next = compile_match_impl(builder, remaining_scrutinees, &next_clauses, fail_block, Some(seen_vars), outer_bindings);
                    if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == extract_block) {
                        block.terminator = Terminator::Jump(next, vec![]);
                    }
                }
            } else {
                // Have sub-patterns to match (literal segments) + remaining
                seg_scrutinees.extend_from_slice(remaining_scrutinees);
                seg_patterns.extend_from_slice(remaining_patterns);

                let next_clauses = vec![MatchClause {
                    patterns: seg_patterns,
                    guard: guard.cloned(),
                    body_block,
                }];
                let next = compile_match_impl(builder, &seg_scrutinees, &next_clauses, fail_block, Some(seen_vars), outer_bindings);
                if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == extract_block) {
                    block.terminator = Terminator::Jump(next, vec![]);
                }
            }
        }

        Pattern::String(s, span) => {
            // Desugar string pattern "abc" into nested cons pattern [97, 98, 99].
            // A string in Erlang is just a list of character code points.
            let desugared = string_to_cons_pattern(s, *span);
            // Close current test_block (we'll reopen via recursive compile)
            builder.close_current_block();

            let next = compile_single_pattern(
                builder,
                scrutinee,
                &desugared,
                remaining_scrutinees,
                remaining_patterns,
                guard,
                body_block,
                fail_block,
                seen_vars,
                outer_bindings,
            );

            // Fix up test_block's terminator to jump to the desugared match
            if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == test_block) {
                block.terminator = Terminator::Jump(next, vec![]);
            }
        }

        Pattern::Concat(lhs, rhs, _span) => {
            // String prefix match: "prefix" ++ Rest
            // Desugar to nested cons pattern with the RHS as the tail.
            if let Pattern::String(s, sspan) = lhs.as_ref() {
                let desugared = string_to_cons_pattern_with_tail(s, *sspan, *rhs.clone());
                // Close current test_block (we'll reopen via recursive compile)
                builder.close_current_block();

                let next = compile_single_pattern(
                    builder,
                    scrutinee,
                    &desugared,
                    remaining_scrutinees,
                    remaining_patterns,
                    guard,
                    body_block,
                    fail_block,
                    seen_vars,
                    outer_bindings,
                );

                // Fix up test_block's terminator to jump to the desugared match
                if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == test_block) {
                    block.terminator = Terminator::Jump(next, vec![]);
                }
            } else {
                // LHS must be a string literal in pattern context
                builder.close_current_block();
                if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == test_block) {
                    block.terminator = Terminator::Jump(fail_block, vec![]);
                }
            }
        }

        Pattern::Map(fields, _span) => {
            // Map pattern matching:
            // 1. TypeTest(IsMap) — verify it's a map
            // 2. For each field: MapGet(key) → sub-match value pattern
            //    (MapGet returns nil/crashes if key not found — the FFI
            //     function returns a sentinel or uses has_key check)
            //
            // Strategy: IsMap → for each field, lower the key expr,
            // call MapGet, collect extracted values + value patterns,
            // then recurse into compile_match with those as sub-scrutinees.

            // Step 1: Is this a map?
            let is_map = builder.fresh_var();
            builder.emit(Instruction::TypeTest(is_map, TypeTestKind::IsMap, scrutinee));

            let mut extract_block = builder.fresh_block_with_id();
            builder.set_terminator(Terminator::Branch(is_map, extract_block, fail_block));

            // Step 2: Extract values for each field
            builder.start_block(extract_block);

            let mut sub_scrutinees = Vec::new();
            let mut sub_patterns = Vec::new();

            for field in fields {
                // Lower the key expression to get a key variable
                let key_var = builder.lower_pattern_expr(&field.key, &HashSet::new()).unwrap_or_else(|_| {
                    builder.emit_const(Literal::Nil)
                });

                // A missing key rejects this pattern; maps:get itself raises badkey.
                let is_key = builder.atom_id_for("is_key");
                let present = builder.fresh_var();
                builder.emit(Instruction::Call(present,
                    CallTarget::Remote(builder.maps_atom_id, is_key), vec![key_var, scrutinee]));
                let value_block = builder.fresh_block_with_id();
                builder.set_terminator(Terminator::Branch(present, value_block, fail_block));
                builder.close_current_block();
                builder.start_block(value_block);
                extract_block = value_block;

                // Extract the value using MapGet
                let val_var = builder.fresh_var();
                builder.emit(Instruction::MapGet(val_var, scrutinee, key_var));

                // Handle the value pattern. A first occurrence binds the
                // value; a name already bound in this clause or outside it
                // falls through to a sub-pattern compared with =:=.
                match &field.value {
                    Pattern::Variable(name, _)
                        if !seen_vars.contains(name) && !outer_bindings.contains_key(name) =>
                    {
                        seen_vars.insert(*name);
                        builder.bind_var(*name, val_var);
                    }
                    Pattern::Wildcard(_) => {
                        // Don't bind, just skip
                    }
                    _ => {
                        // Need sub-matching
                        sub_scrutinees.push(val_var);
                        sub_patterns.push(field.value.clone());
                    }
                }
            }

            // Close extract_block before compile_match
            builder.close_current_block();

            if sub_scrutinees.is_empty() {
                // No sub-patterns — just jump to body (or continue matching)
                if remaining_scrutinees.is_empty() && remaining_patterns.is_empty() {
                    if let Some(guard) = guard {
                        let guard_block = builder.fresh_block_with_id();
                        builder.start_block(guard_block);
                        let guard_val = builder.lower_guard(guard);
                        let true_atom = builder.emit_const(Literal::Atom(builder.true_atom_id));
                        let gcmp = builder.fresh_var();
                        builder.emit(Instruction::BinOp(
                            gcmp,
                            nera_syntax::ast::BinOp::ExactEq,
                            guard_val,
                            true_atom,
                        ));
                        builder.set_terminator(Terminator::Branch(gcmp, body_block, fail_block));
                        if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == extract_block) {
                            block.terminator = Terminator::Jump(guard_block, vec![]);
                        }
                    } else {
                        if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == extract_block) {
                            block.terminator = Terminator::Jump(body_block, vec![]);
                        }
                    }
                } else {
                    let next_clauses = vec![MatchClause {
                        patterns: remaining_patterns.to_vec(),
                        guard: guard.cloned(),
                        body_block,
                    }];
                    let next = compile_match_impl(builder, remaining_scrutinees, &next_clauses, fail_block, Some(seen_vars), outer_bindings);
                    if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == extract_block) {
                        block.terminator = Terminator::Jump(next, vec![]);
                    }
                }
            } else {
                // Have sub-patterns to match + remaining
                sub_scrutinees.extend_from_slice(remaining_scrutinees);
                sub_patterns.extend_from_slice(remaining_patterns);

                let next_clauses = vec![MatchClause {
                    patterns: sub_patterns,
                    guard: guard.cloned(),
                    body_block,
                }];
                let next = compile_match_impl(builder, &sub_scrutinees, &next_clauses, fail_block, Some(seen_vars), outer_bindings);
                if let Some(block) = builder.current_blocks.iter_mut().find(|b| b.id == extract_block) {
                    block.terminator = Terminator::Jump(next, vec![]);
                }
            }
        }

        Pattern::Record { .. } => {
            panic!("Pattern::Record should have been desugared to Pattern::Tuple before reaching the pattern compiler");
        }

        Pattern::MacroCall { .. } => unreachable!("MacroCall only in formatter path"),
    }

    test_block
}

/// Bind a fresh binary segment variable to its extracted term.
///
/// A variable the name already resolves to is copied into: match expressions
/// pre-bind a bare variable that only this copy defines. Its deferred-tail
/// record, if any, is left alone, because it may describe a live outer tail
/// (try-of and catch patterns do not pin outer names yet) or the pre-binder's
/// own extraction for the body (`maybe ?=`, generators), which runs only after
/// the match. Later checks in this clause instead read the matched term
/// through a binding in the clause's own scope.
fn bind_segment_value(
    builder: &mut IrBuilder,
    seen_vars: &mut HashSet<AtomId>,
    name: AtomId,
    value: VarId,
) {
    seen_vars.insert(name);
    if let Some(existing) = builder.lookup_pattern_var(name) {
        builder.emit(Instruction::Move(existing, value));
    }
    builder.bind_var(name, value);
}

/// Expand one segment per codepoint, not per UTF-8 byte, for both matching
/// and body pre-binding so their offsets agree. Keep the full value: unlike
/// construction, an 8-bit pattern above 255 never matches on BEAM. UTF
/// segments compare decoded codepoints. Lowering has already refused a
/// string with a size or a non-UTF type (OTP's typed_literal_string), so
/// every segment here is a default 8-bit or UTF one and needs no size.
pub(crate) fn expand_binary_pattern_strings(segments: &[BinSegment<Pattern>]) -> Vec<BinSegment<Pattern>> {
    segments.iter().flat_map(|seg| {
        if let Pattern::String(ref s, ref span) = seg.value {
            s.chars().map(|ch| BinSegment {
                value: Pattern::Integer(ch as i64, *span),
                size: None,
                type_spec: seg.type_spec.clone(),
                span: *span,
            }).collect::<Vec<_>>()
        } else {
            vec![seg.clone()]
        }
    }).collect()
}

/// Desugar a string literal into a nested Cons pattern of integer character codes,
/// with a specified tail pattern.
///
/// `"abc"` with tail `Nil` becomes `Cons(97, Cons(98, Cons(99, Nil)))`.
/// `"abc"` with tail `Variable(Rest)` becomes `Cons(97, Cons(98, Cons(99, Variable(Rest))))`.
///
/// In Erlang, strings are simply lists of integers (Unicode code points),
/// so pattern matching on `"hello"` is identical to matching on `[$h,$e,$l,$l,$o]`.
fn string_to_cons_pattern_with_tail(s: &str, span: nera_syntax::span::Span, tail: Pattern) -> Pattern {
    let mut result = tail;
    // Build from right to left: last char wraps tail, then each preceding char wraps the rest
    for ch in s.chars().rev() {
        result = Pattern::Cons(
            Box::new(Pattern::Integer(ch as i64, span)),
            Box::new(result),
            span,
        );
    }
    result
}

/// Desugar a string literal into a nested Cons pattern terminated by Nil.
fn string_to_cons_pattern(s: &str, span: nera_syntax::span::Span) -> Pattern {
    string_to_cons_pattern_with_tail(s, span, Pattern::Nil(span))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nera_syntax::span::Span;

    fn dummy_span() -> Span {
        Span { start: 0, end: 0 }
    }

    // ---------------------------------------------------------------
    // string_to_cons_pattern
    // ---------------------------------------------------------------

    #[test]
    fn empty_string_is_nil() {
        let pat = string_to_cons_pattern("", dummy_span());
        assert!(matches!(pat, Pattern::Nil(_)));
    }

    #[test]
    fn single_char_string() {
        let pat = string_to_cons_pattern("a", dummy_span());
        // Should be Cons(Integer(97), Nil)
        if let Pattern::Cons(head, tail, _) = pat {
            assert!(matches!(*head, Pattern::Integer(97, _)));
            assert!(matches!(*tail, Pattern::Nil(_)));
        } else {
            panic!("expected Cons pattern, got {:?}", pat);
        }
    }

    #[test]
    fn multi_char_string() {
        let pat = string_to_cons_pattern("ab", dummy_span());
        // Should be Cons(Integer(97), Cons(Integer(98), Nil))
        if let Pattern::Cons(head, tail, _) = pat {
            assert!(matches!(*head, Pattern::Integer(97, _)));
            if let Pattern::Cons(head2, tail2, _) = *tail {
                assert!(matches!(*head2, Pattern::Integer(98, _)));
                assert!(matches!(*tail2, Pattern::Nil(_)));
            } else {
                panic!("expected inner Cons, got {:?}", tail);
            }
        } else {
            panic!("expected Cons pattern, got {:?}", pat);
        }
    }

    #[test]
    fn string_preserves_char_order() {
        // "hello" → [104, 101, 108, 108, 111]
        let pat = string_to_cons_pattern("hello", dummy_span());
        let expected = [104i64, 101, 108, 108, 111];
        let mut current = pat;
        for (i, &expected_val) in expected.iter().enumerate() {
            if let Pattern::Cons(head, tail, _) = current {
                if let Pattern::Integer(val, _) = *head {
                    assert_eq!(val, expected_val, "mismatch at position {}", i);
                } else {
                    panic!("expected Integer at position {}, got {:?}", i, head);
                }
                current = *tail;
            } else {
                panic!("expected Cons at position {}, got {:?}", i, current);
            }
        }
        assert!(matches!(current, Pattern::Nil(_)), "expected Nil at end");
    }

    #[test]
    fn unicode_string() {
        // Unicode: each char is a codepoint
        let pat = string_to_cons_pattern("é", dummy_span());
        if let Pattern::Cons(head, tail, _) = pat {
            if let Pattern::Integer(val, _) = *head {
                assert_eq!(val, 'é' as i64);
            } else {
                panic!("expected Integer");
            }
            assert!(matches!(*tail, Pattern::Nil(_)));
        } else {
            panic!("expected Cons");
        }
    }

    // ---------------------------------------------------------------
    // PatternTest variants
    // ---------------------------------------------------------------

    #[test]
    fn pattern_test_debug() {
        // Ensure PatternTest variants can be constructed and printed
        let tests = vec![
            PatternTest::IsAtomEq(AtomId(0)),
            PatternTest::IsIntegerEq(42),
            PatternTest::IsFloatEq(3.25),
            PatternTest::IsTupleArity(3),
            PatternTest::IsCons,
            PatternTest::IsNil,
            PatternTest::Wildcard,
        ];
        for t in &tests {
            let _ = format!("{:?}", t);
        }
    }

    // ---------------------------------------------------------------
    // MatchClause construction
    // ---------------------------------------------------------------

    #[test]
    fn match_clause_without_guard() {
        let clause = MatchClause {
            patterns: vec![Pattern::Wildcard(dummy_span())],
            guard: None,
            body_block: BlockId(5),
        };
        assert!(clause.guard.is_none());
        assert_eq!(clause.body_block, BlockId(5));
        assert_eq!(clause.patterns.len(), 1);
    }
}
