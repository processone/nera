//! AST walker free functions for collecting pattern bindings, expression
//! references, and guard references.
//!
//! These are used by both the warning analyzer and by `IrBuilder` in
//! `lower.rs` (which delegates to them via `pub(crate)` re-exports).

use std::collections::HashSet;

use nera_syntax::ast::{self, AtomId, Pattern};
use nera_syntax::span::Span;

/// Collect all variable bindings from a pattern, preserving source spans.
pub(super) fn collect_pattern_bindings_with_spans(pat: &ast::Pattern, bound: &mut Vec<(AtomId, Span)>) {
    match pat {
        ast::Pattern::Variable(id, span) => {
            bound.push((*id, *span));
        }
        ast::Pattern::Wildcard(_)
        | ast::Pattern::Integer(_, _)
        | ast::Pattern::BigInteger(_, _)
        | ast::Pattern::Float(_, _)
        | ast::Pattern::Atom(_, _)
        | ast::Pattern::String(_, _)
        | ast::Pattern::Nil(_) => {}
        ast::Pattern::Tuple(pats, _) => {
            for p in pats {
                collect_pattern_bindings_with_spans(p, bound);
            }
        }
        ast::Pattern::Cons(h, t, _) => {
            collect_pattern_bindings_with_spans(h, bound);
            collect_pattern_bindings_with_spans(t, bound);
        }
        ast::Pattern::Match(p1, p2, _) => {
            collect_pattern_bindings_with_spans(p1, bound);
            collect_pattern_bindings_with_spans(p2, bound);
        }
        ast::Pattern::Binary(segments, _) => {
            for seg in segments {
                collect_pattern_bindings_with_spans(&seg.value, bound);
            }
        }
        ast::Pattern::Map(fields, _) => {
            for field in fields {
                collect_pattern_bindings_with_spans(&field.value, bound);
            }
        }
        ast::Pattern::Record { fields, .. } => {
            for field in fields {
                collect_pattern_bindings_with_spans(&field.pattern, bound);
            }
        }
        ast::Pattern::Concat(lhs, rhs, _) => {
            collect_pattern_bindings_with_spans(lhs, bound);
            collect_pattern_bindings_with_spans(rhs, bound);
        }
        ast::Pattern::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    collect_pattern_bindings_with_spans(arg, bound);
                }
            }
        }
    }
}

/// Collect all variable bindings from a pattern (AtomId only, no spans).
pub(crate) fn collect_pattern_bindings(
    pat: &ast::Pattern,
    bound: &mut HashSet<AtomId>,
) {
    match pat {
        ast::Pattern::Variable(id, _) => {
            bound.insert(*id);
        }
        ast::Pattern::Wildcard(_)
        | ast::Pattern::Integer(_, _)
        | ast::Pattern::BigInteger(_, _)
        | ast::Pattern::Float(_, _)
        | ast::Pattern::Atom(_, _)
        | ast::Pattern::String(_, _)
        | ast::Pattern::Nil(_) => {}
        ast::Pattern::Tuple(pats, _) => {
            for p in pats {
                collect_pattern_bindings(p, bound);
            }
        }
        ast::Pattern::Cons(h, t, _) => {
            collect_pattern_bindings(h, bound);
            collect_pattern_bindings(t, bound);
        }
        ast::Pattern::Match(p1, p2, _) => {
            collect_pattern_bindings(p1, bound);
            collect_pattern_bindings(p2, bound);
        }
        ast::Pattern::Binary(segments, _) => {
            for seg in segments {
                collect_pattern_bindings(&seg.value, bound);
            }
        }
        ast::Pattern::Map(fields, _) => {
            for field in fields {
                collect_pattern_bindings(&field.value, bound);
            }
        }
        ast::Pattern::Record { fields, .. } => {
            for field in fields {
                collect_pattern_bindings(&field.pattern, bound);
            }
        }
        ast::Pattern::Concat(lhs, rhs, _) => {
            collect_pattern_bindings(lhs, bound);
            collect_pattern_bindings(rhs, bound);
        }
        ast::Pattern::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    collect_pattern_bindings(arg, bound);
                }
            }
        }
    }
}

/// Collect reads supplied from outside a pattern, not the names it binds.
/// Sizes reading earlier segments of the same binary are local, not inputs.
pub(crate) fn collect_pattern_refs(pat: &ast::Pattern, refs: &mut HashSet<AtomId>) {
    collect_pattern_refs_in_scope(pat, refs, PatternRefScope::Inputs);
}

/// Collect every read in pattern keys and sizes, including local binary sizes.
/// Unlike input/capture collection, unused-variable analysis needs those reads
/// to mark earlier segment bindings as used.
pub(super) fn collect_pattern_expr_refs(pat: &ast::Pattern, refs: &mut HashSet<AtomId>) {
    collect_pattern_refs_in_scope(pat, refs, PatternRefScope::All);
}

/// Collect reads of bindings introduced within a pattern's own binary.
/// Other keys and sizes are inputs: in a fun head they must not mark a
/// same-named, shadowing value binding as used.
pub(super) fn collect_pattern_local_refs(pat: &ast::Pattern, refs: &mut HashSet<AtomId>) {
    collect_pattern_refs_in_scope(pat, refs, PatternRefScope::Local);
}

#[derive(Clone, Copy)]
enum PatternRefScope {
    Inputs,
    Local,
    All,
}

fn collect_pattern_refs_in_scope(
    pat: &ast::Pattern,
    refs: &mut HashSet<AtomId>,
    scope: PatternRefScope,
) {
    match pat {
        ast::Pattern::Tuple(pats, _) => {
            for pat in pats {
                collect_pattern_refs_in_scope(pat, refs, scope);
            }
        }
        ast::Pattern::Cons(left, right, _)
        | ast::Pattern::Match(left, right, _)
        | ast::Pattern::Concat(left, right, _) => {
            collect_pattern_refs_in_scope(left, refs, scope);
            collect_pattern_refs_in_scope(right, refs, scope);
        }
        ast::Pattern::Binary(segments, _) => {
            let mut earlier = HashSet::new();
            for seg in segments {
                if let Some(size) = &seg.size {
                    let mut size_refs = HashSet::new();
                    collect_expr_refs(size, &mut size_refs);
                    match scope {
                        PatternRefScope::Inputs => {
                            refs.extend(size_refs.difference(&earlier).copied());
                        }
                        PatternRefScope::Local => {
                            refs.extend(size_refs.intersection(&earlier).copied());
                        }
                        PatternRefScope::All => refs.extend(size_refs),
                    }
                }
                collect_pattern_refs_in_scope(&seg.value, refs, scope);
                collect_pattern_bindings(&seg.value, &mut earlier);
            }
        }
        ast::Pattern::Map(fields, _) => {
            for field in fields {
                if !matches!(scope, PatternRefScope::Local) {
                    collect_expr_refs(&field.key, refs);
                }
                collect_pattern_refs_in_scope(&field.value, refs, scope);
            }
        }
        ast::Pattern::Record { fields, .. } => {
            for field in fields {
                collect_pattern_refs_in_scope(&field.pattern, refs, scope);
            }
        }
        _ => {}
    }
}

/// Collect all variable references in an expression.
pub(crate) fn collect_expr_refs(
    expr: &ast::Expr,
    refs: &mut HashSet<AtomId>,
) {
    collect_refs(expr, refs, RefScoping::Flat);
}

/// Collect reads of names from the enclosing scope.
///
/// Unlike [`collect_expr_refs`], a fun head, a named fun's name and a
/// comprehension generator pattern bind fresh names, as in `erl_lint`: reads of
/// those names inside the fun clause or comprehension are not reads of a
/// same-named enclosing variable. Reads of names the construct does not bind
/// still count: fun head map keys and binary sizes, generator sources, and
/// guard, filter and body reads.
pub(super) fn collect_enclosing_refs(expr: &ast::Expr, refs: &mut HashSet<AtomId>) {
    collect_refs(expr, refs, RefScoping::Lexical);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefScoping {
    /// Every name read anywhere inside the expression, including names a fun
    /// head or generator pattern binds.
    Flat,
    /// Only names that resolve to the enclosing scope.
    Lexical,
}

fn collect_refs(expr: &ast::Expr, refs: &mut HashSet<AtomId>, scoping: RefScoping) {
    match expr {
        ast::Expr::Variable(id, _) => {
            refs.insert(*id);
        }
        ast::Expr::Integer(_, _)
        | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(_, _)
        | ast::Expr::Atom(_, _)
        | ast::Expr::String(_, _)
        | ast::Expr::Nil(_) => {}
        ast::Expr::Tuple(exprs, _) | ast::Expr::Block(exprs, _) => {
            for e in exprs {
                collect_refs(e, refs, scoping);
            }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) => {
            collect_refs(h, refs, scoping);
            collect_refs(t, refs, scoping);
        }
        ast::Expr::BinOp(_, l, r, _) => {
            collect_refs(l, refs, scoping);
            collect_refs(r, refs, scoping);
        }
        ast::Expr::UnaryOp(_, e, _) | ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            collect_refs(e, refs, scoping);
        }
        ast::Expr::Match(pat, e, _) => {
            // Variables in the LHS pattern may be references to already-bound
            // variables (equality assertions), e.g. `B = Expected` where B is
            // a function parameter.  Collect them as references so they are not
            // falsely reported as unused.
            collect_pattern_bindings(pat, refs);
            collect_pattern_refs(pat, refs);
            collect_refs(e, refs, scoping);
        }
        ast::Expr::Call(target, args, _) => {
            if let ast::CallTarget::Variable(id, _) = target {
                refs.insert(*id);
            }
            if let ast::CallTarget::Remote(module_expr, fun_expr) = target {
                collect_refs(module_expr, refs, scoping);
                collect_refs(fun_expr, refs, scoping);
            }
            // The callee itself may contain a closure using outer variables,
            // e.g. (fun() -> Expected end)() inside an EUnit assertion.
            if let ast::CallTarget::Expr(callee) = target {
                collect_refs(callee, refs, scoping);
            }
            for arg in args {
                collect_refs(arg, refs, scoping);
            }
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            collect_refs(scrutinee, refs, scoping);
            for clause in clauses {
                collect_pattern_bindings(&clause.pattern, refs);
                collect_pattern_refs(&clause.pattern, refs);
                if let Some(ref guard) = clause.guard {
                    collect_guard_refs(guard, refs);
                }
                for e in &clause.body {
                    collect_refs(e, refs, scoping);
                }
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                collect_guard_refs(&clause.guard, refs);
                for e in &clause.body {
                    collect_refs(e, refs, scoping);
                }
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                collect_pattern_bindings(&clause.pattern, refs);
                collect_pattern_refs(&clause.pattern, refs);
                if let Some(ref guard) = clause.guard {
                    collect_guard_refs(guard, refs);
                }
                for e in &clause.body {
                    collect_refs(e, refs, scoping);
                }
            }
            if let Some(after) = after {
                collect_refs(&after.timeout, refs, scoping);
                for e in &after.body {
                    collect_refs(e, refs, scoping);
                }
            }
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                collect_refs(&seg.value, refs, scoping);
                if let Some(ref size) = seg.size {
                    collect_refs(size, refs, scoping);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for field in fields {
                collect_refs(&field.key, refs, scoping);
                collect_refs(&field.value, refs, scoping);
            }
        }
        ast::Expr::ListComp(body, qualifiers, _) | ast::Expr::BinComp(body, qualifiers, _)
            if scoping == RefScoping::Lexical =>
        {
            // Generator patterns bind fresh names for later qualifiers and the
            // body. A generator's source and pattern inputs are read before
            // its own pattern binds.
            let mut fresh = HashSet::new();
            let mut local = HashSet::new();
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(pat, expr, _)
                    | ast::LcQualifier::BinGenerator(pat, expr, _) => {
                        collect_pattern_refs(pat, &mut local);
                        collect_refs(expr, &mut local, scoping);
                        refs.extend(local.drain().filter(|id| !fresh.contains(id)));
                        collect_pattern_bindings(pat, &mut fresh);
                    }
                    ast::LcQualifier::Filter(expr, _) => {
                        collect_refs(expr, &mut local, scoping);
                        refs.extend(local.drain().filter(|id| !fresh.contains(id)));
                    }
                }
            }
            collect_refs(body, &mut local, scoping);
            refs.extend(local.difference(&fresh).copied());
        }
        ast::Expr::ListComp(body, qualifiers, _) | ast::Expr::BinComp(body, qualifiers, _) => {
            collect_refs(body, refs, scoping);
            let mut earlier = HashSet::new();
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(pat, expr, _)
                    | ast::LcQualifier::BinGenerator(pat, expr, _) => {
                        // Later pattern reads refer to earlier generators, not
                        // same-named variables outside the comprehension.
                        let mut pattern_refs = HashSet::new();
                        collect_pattern_refs(pat, &mut pattern_refs);
                        refs.extend(pattern_refs.difference(&earlier).copied());
                        collect_refs(expr, refs, scoping);
                        collect_pattern_bindings(pat, &mut earlier);
                    }
                    ast::LcQualifier::Filter(expr, _) => {
                        collect_refs(expr, refs, scoping);
                    }
                }
            }
        }
        ast::Expr::Try {
            body,
            of_clauses,
            catch_clauses,
            after_body,
            ..
        } => {
            for e in body {
                collect_refs(e, refs, scoping);
            }
            for clause in of_clauses {
                collect_pattern_bindings(&clause.pattern, refs);
                collect_pattern_refs(&clause.pattern, refs);
                if let Some(ref guard) = clause.guard {
                    collect_guard_refs(guard, refs);
                }
                for e in &clause.body {
                    collect_refs(e, refs, scoping);
                }
            }
            for clause in catch_clauses {
                collect_pattern_bindings(&clause.pattern, refs);
                collect_pattern_refs(&clause.pattern, refs);
                if let Some(ref guard) = clause.guard {
                    collect_guard_refs(guard, refs);
                }
                for e in &clause.body {
                    collect_refs(e, refs, scoping);
                }
            }
            if let Some(after) = after_body {
                for e in after {
                    collect_refs(e, refs, scoping);
                }
            }
        }
        ast::Expr::Fun(name, clauses, _) if scoping == RefScoping::Lexical => {
            for clause in clauses {
                // The head binds fresh names for this clause's guard and body.
                // Head keys and sizes read the enclosing scope, except that a
                // named fun's own name is already in scope there.
                let mut local = HashSet::new();
                for pat in &clause.patterns {
                    collect_pattern_refs(pat, &mut local);
                }
                refs.extend(local.drain().filter(|id| name.as_ref() != Some(id)));
                let mut fresh: HashSet<AtomId> = name.iter().copied().collect();
                for pat in &clause.patterns {
                    collect_pattern_bindings(pat, &mut fresh);
                }
                if let Some(ref guard) = clause.guard {
                    collect_guard_refs(guard, &mut local);
                }
                for e in &clause.body {
                    collect_refs(e, &mut local, scoping);
                }
                refs.extend(local.difference(&fresh).copied());
            }
        }
        ast::Expr::Fun(_name, clauses, _) => {
            for clause in clauses {
                for pat in &clause.patterns {
                    collect_pattern_bindings(pat, refs);
                    collect_pattern_refs(pat, refs);
                }
                if let Some(ref guard) = clause.guard {
                    collect_guard_refs(guard, refs);
                }
                for e in &clause.body {
                    collect_refs(e, refs, scoping);
                }
            }
        }
        ast::Expr::FunRef(ast::FunRefTarget::Local(..) | ast::FunRefTarget::Remote(..), _) => {}
        ast::Expr::FunRef(ast::FunRefTarget::DynamicRemote(m, f, a), _) => {
            collect_refs(m, refs, scoping);
            collect_refs(f, refs, scoping);
            collect_refs(a, refs, scoping);
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => collect_refs(e, refs, scoping),
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        collect_pattern_refs(pat, refs);
                        collect_refs(e, refs, scoping);
                    }
                }
            }
            for clause in else_clauses {
                collect_pattern_bindings(&clause.pattern, refs);
                collect_pattern_refs(&clause.pattern, refs);
                if let Some(ref guard) = clause.guard {
                    collect_guard_refs(guard, refs);
                }
                for e in &clause.body {
                    collect_refs(e, refs, scoping);
                }
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            collect_refs(base, refs, scoping);
            for field in fields {
                collect_refs(&field.key, refs, scoping);
                collect_refs(&field.value, refs, scoping);
            }
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for field in fields {
                collect_refs(&field.value, refs, scoping);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            collect_refs(base, refs, scoping);
            for field in fields {
                collect_refs(&field.value, refs, scoping);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            collect_refs(base, refs, scoping);
        }
        ast::Expr::RecordIndex { .. } => {}
        ast::Expr::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    collect_refs(arg, refs, scoping);
                }
            }
        }
    }
}

/// Collect variable references in a guard.
pub(crate) fn collect_guard_refs(
    guard: &ast::Guard,
    refs: &mut HashSet<AtomId>,
) {
    match guard {
        ast::Guard::Test(e) => collect_expr_refs(e, refs),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                collect_expr_refs(e, refs);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                collect_guard_refs(g, refs);
            }
        }
    }
}

/// Collect all `Pattern::Variable` AtomIds from a pattern, preserving
/// duplicates (unlike `collect_pattern_bindings` which uses a HashSet).
pub(super) fn collect_pattern_var_ids(pat: &Pattern, ids: &mut Vec<AtomId>) {
    match pat {
        Pattern::Variable(id, _) => ids.push(*id),
        Pattern::Wildcard(_)
        | Pattern::Integer(_, _)
        | Pattern::BigInteger(_, _)
        | Pattern::Float(_, _)
        | Pattern::Atom(_, _)
        | Pattern::String(_, _)
        | Pattern::Nil(_) => {}
        Pattern::Tuple(pats, _) => {
            for p in pats {
                collect_pattern_var_ids(p, ids);
            }
        }
        Pattern::Cons(h, t, _) => {
            collect_pattern_var_ids(h, ids);
            collect_pattern_var_ids(t, ids);
        }
        Pattern::Match(p1, p2, _) => {
            collect_pattern_var_ids(p1, ids);
            collect_pattern_var_ids(p2, ids);
        }
        Pattern::Binary(segments, _) => {
            for seg in segments {
                collect_pattern_var_ids(&seg.value, ids);
            }
        }
        Pattern::Map(fields, _) => {
            for field in fields {
                collect_pattern_var_ids(&field.value, ids);
            }
        }
        Pattern::Record { fields, .. } => {
            for field in fields {
                collect_pattern_var_ids(&field.pattern, ids);
            }
        }
        Pattern::Concat(lhs, rhs, _) => {
            collect_pattern_var_ids(lhs, ids);
            collect_pattern_var_ids(rhs, ids);
        }
        Pattern::MacroCall { args, .. } => {
            if let Some(args) = args {
                for arg in args {
                    collect_pattern_var_ids(arg, ids);
                }
            }
        }
    }
}
