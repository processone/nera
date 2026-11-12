//! Names a `case`, `if` or `receive` exports to the code after it.
//!
//! In Erlang a variable bound in every branch of one of these expressions,
//! and unbound before it, stays bound after it. Each branch binds the name to
//! its own variable, so the join gets one variable that every branch copies
//! its value into. Only names that later code may use are exported, so a
//! deferred binary tail that no later code names stays deferred. The search
//! for later uses errs towards exporting: see [`LaterUses`].

use std::collections::{HashMap, HashSet};
use nera_syntax::ast::{self, AtomId};
use crate::ir::*;
use super::{IrBuilder, MatchContextInfo};

/// What one branch had bound when it jumped to the join.
pub(super) struct BranchExit {
    /// The block whose terminator jumps to the join.
    block: BlockId,
    /// Names first bound in this branch, and the variable each one reads.
    bindings: HashMap<AtomId, VarId>,
    /// Deferred binary tails among those variables.
    tails: HashMap<VarId, MatchContextInfo>,
}

impl IrBuilder {
    /// Record what the current branch bound before it jumps to the join.
    /// `before` holds the names bound before the expression. `None` when no
    /// block is open, so the branch cannot reach the join.
    pub(super) fn branch_exit(&self, before: &HashMap<AtomId, VarId>) -> Option<BranchExit> {
        let block = self.current_block_id?;
        let bindings: HashMap<AtomId, VarId> = self.collect_all_bindings().into_iter()
            .filter(|(name, _)| !before.contains_key(name))
            .collect();
        let tails = bindings.values()
            .filter_map(|var| self.match_contexts.get(var).map(|info| (*var, info.clone())))
            .collect();
        Some(BranchExit { block, bindings, tails })
    }

    /// Bind, at the join, each name that every branch bound and that the code
    /// after the expression may use. `node` is the whole `case`, `if` or
    /// `receive` expression. Call it once the join block is open and the
    /// clause patterns are compiled, so the patterns do not compare against
    /// the exported names.
    pub(super) fn export_branch_bindings(&mut self, exits: &[BranchExit], node: &ast::Expr) {
        let Some((first, rest)) = exits.split_first() else { return };
        let mut shared: Vec<AtomId> = first.bindings.keys()
            .filter(|name| rest.iter().all(|exit| exit.bindings.contains_key(*name)))
            .copied()
            .collect();
        if shared.is_empty() {
            return;
        }
        self.later_uses.keep_used_after(node, &mut shared);
        shared.sort_by_key(|name| name.0);

        for name in shared {
            let exported = self.fresh_var();
            for exit in exits {
                let var = exit.bindings[&name];
                let mut copy = Vec::with_capacity(2);
                let value = match exit.tails.get(&var) {
                    Some(info) => {
                        let value = self.fresh_var();
                        copy.push(Instruction::BsMaterialize(value, info.match_ctx,
                            info.bit_offset, info.remaining_bits));
                        value
                    }
                    None => var,
                };
                copy.push(Instruction::Move(exported, value));
                self.append_to_block(exit.block, copy);
            }
            self.bind_var(name, exported);
        }
    }

    /// Append instructions to a closed block, before its terminator.
    fn append_to_block(&mut self, block_id: BlockId, instructions: Vec<Instruction>) {
        let block = self.current_blocks.iter_mut()
            .find(|block| block.id == block_id)
            .expect("a branch exit block is closed before its join");
        block.instructions.extend(instructions);
    }
}

/// The names each `case`, `if` and `receive` of some code may meet again after
/// it ends: any occurrence in code that runs after it and sees the names it
/// binds, whether a read, a binding, a pattern comparison, a key, a size or a
/// fun capture. A name it exports was unbound before it, so an occurrence
/// before it, or in code that does not see its bindings, names something
/// else.
///
/// The walk runs backwards, from the end of the code to its start, carrying
/// the names used after the current point. Code that cannot see an
/// expression's bindings does not count for it: another clause of the same
/// expression; the catch clauses and after block of a `try`, or the `else`
/// clauses of a `maybe`, for an expression in its body; and anything after a
/// fun body, a comprehension or a generator source, for an expression inside
/// it, since those start from no names. Where Erlang reports a later use as
/// unsafe, such as after a `try`, the walk still counts the code that runs
/// next, so it errs towards exporting.
///
/// A fun head or a generator pattern binds new names: their uses after it are
/// not uses of the names outside it. The keys and sizes of such a head read
/// the names outside it.
#[derive(Default)]
pub(super) struct LaterUses {
    /// The names used after each `case`, `if` and `receive`, by its address.
    after: HashMap<usize, Names>,
}

type Names = HashSet<AtomId>;

impl LaterUses {
    /// The later uses in one function clause.
    pub(super) fn of_clause(clause: &ast::FunctionClause) -> Self {
        let mut uses = Self::default();
        uses.function_clause(clause, &mut Names::new());
        uses
    }

    /// The later uses in one expression, such as a record field default.
    pub(super) fn of_expr(expr: &ast::Expr) -> Self {
        let mut uses = Self::default();
        uses.expr(expr, &mut Names::new());
        uses
    }

    /// Keep the names in `names` that the code after `node`, a `case`, `if`
    /// or `receive`, may use. Lowering reads the expressions it was given, so
    /// it finds `node`; a node the walk never saw keeps every name.
    pub(super) fn keep_used_after(&self, node: &ast::Expr, names: &mut Vec<AtomId>) {
        if let Some(after) = self.after.get(&address(node)) {
            names.retain(|name| after.contains(name));
        }
    }

    /// Walk a function or fun clause. It runs on its own: what runs after it
    /// does not see its bindings.
    fn function_clause(&mut self, clause: &ast::FunctionClause, live: &mut Names) {
        let mut inside = Names::new();
        self.body(&clause.body, &mut inside);
        if let Some(guard) = &clause.guard {
            self.guard(guard, &mut inside);
        }
        self.head(&clause.patterns, &mut inside);
        live.extend(inside);
    }

    /// Walk a fun head or generator pattern: the names it binds leave
    /// `live`, and the names its keys and sizes read join it.
    fn head<'a>(&mut self, patterns: impl IntoIterator<Item = &'a ast::Pattern>, live: &mut Names) {
        let mut fresh = Vec::new();
        let mut reads = Names::new();
        for pattern in patterns {
            self.pattern_in(pattern, &mut reads, Some(&mut fresh));
        }
        for name in &fresh {
            live.remove(name);
        }
        live.extend(reads);
    }

    /// Walk clauses of which one runs: each starts from what runs after
    /// them all.
    fn case_clauses(&mut self, clauses: &[ast::CaseClause], live: &mut Names) {
        let after = live.clone();
        for clause in clauses {
            let mut uses = after.clone();
            self.body(&clause.body, &mut uses);
            if let Some(guard) = &clause.guard {
                self.guard(guard, &mut uses);
            }
            self.pattern(&clause.pattern, &mut uses);
            live.extend(uses);
        }
    }

    fn if_clauses(&mut self, clauses: &[ast::IfClause], live: &mut Names) {
        let after = live.clone();
        for clause in clauses {
            let mut uses = after.clone();
            self.body(&clause.body, &mut uses);
            self.guard(&clause.guard, &mut uses);
            live.extend(uses);
        }
    }

    fn catch_clauses(&mut self, clauses: &[ast::CatchClause], live: &mut Names) {
        let after = live.clone();
        for clause in clauses {
            let mut uses = after.clone();
            self.body(&clause.body, &mut uses);
            if let Some(guard) = &clause.guard {
                self.guard(guard, &mut uses);
            }
            if let Some(stacktrace) = &clause.stacktrace {
                self.pattern(stacktrace, &mut uses);
            }
            self.pattern(&clause.pattern, &mut uses);
            if let ast::CatchClass::Variable(name, _) = clause.class {
                uses.insert(name);
            }
            live.extend(uses);
        }
    }

    fn body(&mut self, body: &[ast::Expr], live: &mut Names) {
        for expr in body.iter().rev() {
            self.expr(expr, live);
        }
    }

    fn guard(&mut self, guard: &ast::Guard, live: &mut Names) {
        match guard {
            ast::Guard::Test(expr) => self.expr(expr, live),
            ast::Guard::Conjunction(exprs) => self.body(exprs, live),
            ast::Guard::Disjunction(guards) => {
                for guard in guards {
                    self.guard(guard, live);
                }
            }
        }
    }

    /// Walk `expr`: `live` holds the names used after it, and then the names
    /// used from its start on. Parts run in the order lowering evaluates
    /// them, so they are walked in reverse.
    fn expr(&mut self, expr: &ast::Expr, live: &mut Names) {
        match expr {
            ast::Expr::Variable(name, _) => {
                live.insert(*name);
            }
            ast::Expr::Integer(..) | ast::Expr::BigInteger(..) | ast::Expr::Float(..)
            | ast::Expr::Atom(..) | ast::Expr::String(..) | ast::Expr::Nil(_)
            | ast::Expr::RecordIndex { .. } => {}
            ast::Expr::Tuple(exprs, _) | ast::Expr::Block(exprs, _) => self.body(exprs, live),
            ast::Expr::Cons(left, right, _) | ast::Expr::Send(left, right, _)
            | ast::Expr::BinOp(_, left, right, _) => {
                self.expr(right, live);
                self.expr(left, live);
            }
            ast::Expr::UnaryOp(_, expr, _) | ast::Expr::Throw(expr, _)
            | ast::Expr::Catch(expr, _) | ast::Expr::RecordAccess { base: expr, .. } => {
                self.expr(expr, live);
            }
            // The pattern runs after the value, and may read its exports.
            ast::Expr::Match(pattern, value, _) => {
                self.pattern(pattern, live);
                self.expr(value, live);
            }
            ast::Expr::Case(scrutinee, clauses, _) => {
                self.after.insert(address(expr), live.clone());
                self.case_clauses(clauses, live);
                self.expr(scrutinee, live);
            }
            ast::Expr::If(clauses, _) => {
                self.after.insert(address(expr), live.clone());
                self.if_clauses(clauses, live);
            }
            ast::Expr::Receive(clauses, after, _) => {
                self.after.insert(address(expr), live.clone());
                let continuation = live.clone();
                self.case_clauses(clauses, live);
                if let Some(after) = after {
                    // The after body is one more alternative to the clauses.
                    let mut uses = continuation;
                    self.body(&after.body, &mut uses);
                    live.extend(uses);
                    // The timeout runs first.
                    self.expr(&after.timeout, live);
                }
            }
            ast::Expr::Call(target, args, _) => {
                self.body(args, live);
                match target {
                    ast::CallTarget::Local(..) => {}
                    ast::CallTarget::Variable(name, _) => {
                        live.insert(*name);
                    }
                    ast::CallTarget::Remote(module, function) => {
                        self.expr(function, live);
                        self.expr(module, live);
                    }
                    ast::CallTarget::Expr(callee) => self.expr(callee, live),
                }
            }
            ast::Expr::Binary(segments, _) => {
                for segment in segments.iter().rev() {
                    if let Some(size) = &segment.size {
                        self.expr(size, live);
                    }
                    self.expr(&segment.value, live);
                }
            }
            ast::Expr::Map(fields, _) => {
                for field in fields.iter().rev() {
                    self.expr(&field.value, live);
                    self.expr(&field.key, live);
                }
            }
            ast::Expr::MapUpdate(base, fields, _) => {
                for field in fields.iter().rev() {
                    self.expr(&field.value, live);
                    self.expr(&field.key, live);
                }
                self.expr(base, live);
            }
            // A comprehension runs in its own scope: what runs after it does
            // not see its bindings.
            ast::Expr::ListComp(template, qualifiers, _)
            | ast::Expr::BinComp(template, qualifiers, _) => {
                let mut inside = Names::new();
                self.expr(template, &mut inside);
                for qualifier in qualifiers.iter().rev() {
                    match qualifier {
                        ast::LcQualifier::Generator(pattern, source, _)
                        | ast::LcQualifier::BinGenerator(pattern, source, _) => {
                            self.head([pattern], &mut inside);
                            // The names a source binds stay in it.
                            let mut uses = Names::new();
                            self.expr(source, &mut uses);
                            inside.extend(uses);
                        }
                        ast::LcQualifier::Filter(filter, _) => self.expr(filter, &mut inside),
                    }
                }
                live.extend(inside);
            }
            // The body's names reach the `of` clauses; the catch clauses and
            // the after block do not see them.
            ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
                let continuation = live.clone();
                let mut handlers = continuation.clone();
                self.catch_clauses(catch_clauses, &mut handlers);
                if let Some(after_body) = after_body {
                    let mut uses = continuation;
                    self.body(after_body, &mut uses);
                    handlers.extend(uses);
                }
                self.case_clauses(of_clauses, live);
                self.body(body, live);
                live.extend(handlers);
            }
            ast::Expr::Fun(name, clauses, _) => {
                let mut inside = Names::new();
                for clause in clauses {
                    self.function_clause(clause, &mut inside);
                }
                // A named fun's own name is new inside it, like its head.
                if let Some(name) = name {
                    inside.remove(name);
                }
                live.extend(inside);
            }
            ast::Expr::FunRef(target, _) => {
                if let ast::FunRefTarget::DynamicRemote(module, function, arity) = target {
                    self.expr(arity, live);
                    self.expr(function, live);
                    self.expr(module, live);
                }
            }
            ast::Expr::RecordCreate { fields, .. } => self.record_fields(fields, live),
            ast::Expr::RecordUpdate { base, fields, .. } => {
                self.record_fields(fields, live);
                self.expr(base, live);
            }
            // The `else` clauses do not see the body's names.
            ast::Expr::Maybe { body, else_clauses, .. } => {
                let mut otherwise = live.clone();
                self.case_clauses(else_clauses, &mut otherwise);
                for item in body.iter().rev() {
                    match item {
                        ast::MaybeExpr::Expr(expr) => self.expr(expr, live),
                        ast::MaybeExpr::CondMatch(pattern, value, _) => {
                            self.pattern(pattern, live);
                            self.expr(value, live);
                        }
                    }
                }
                live.extend(otherwise);
            }
            ast::Expr::MacroCall { args, .. } => {
                if let Some(args) = args {
                    self.body(args, live);
                }
            }
        }
    }

    fn record_fields(&mut self, fields: &[ast::RecordFieldAssign], live: &mut Names) {
        for field in fields.iter().rev() {
            self.expr(&field.value, live);
        }
    }

    fn pattern(&mut self, pattern: &ast::Pattern, live: &mut Names) {
        self.pattern_in(pattern, live, None);
    }

    /// Walk a pattern. In a head, `fresh` collects the names its variables
    /// bind instead of adding them to `live`.
    fn pattern_in(
        &mut self,
        pattern: &ast::Pattern,
        live: &mut Names,
        mut fresh: Option<&mut Vec<AtomId>>,
    ) {
        match pattern {
            ast::Pattern::Variable(name, _) => match fresh {
                Some(fresh) => fresh.push(*name),
                None => {
                    live.insert(*name);
                }
            },
            ast::Pattern::Wildcard(_) | ast::Pattern::Integer(..) | ast::Pattern::BigInteger(..)
            | ast::Pattern::Float(..) | ast::Pattern::Atom(..) | ast::Pattern::String(..)
            | ast::Pattern::Nil(_) => {}
            ast::Pattern::Tuple(patterns, _) => {
                for pattern in patterns {
                    self.pattern_in(pattern, live, fresh.as_deref_mut());
                }
            }
            ast::Pattern::Cons(left, right, _) | ast::Pattern::Match(left, right, _)
            | ast::Pattern::Concat(left, right, _) => {
                self.pattern_in(left, live, fresh.as_deref_mut());
                self.pattern_in(right, live, fresh);
            }
            ast::Pattern::Binary(segments, _) => {
                for segment in segments {
                    self.pattern_in(&segment.value, live, fresh.as_deref_mut());
                    if let Some(size) = &segment.size {
                        self.expr(size, live);
                    }
                }
            }
            ast::Pattern::Map(fields, _) => {
                for field in fields {
                    self.expr(&field.key, live);
                    self.pattern_in(&field.value, live, fresh.as_deref_mut());
                }
            }
            ast::Pattern::Record { fields, .. } => {
                for field in fields {
                    self.pattern_in(&field.pattern, live, fresh.as_deref_mut());
                }
            }
            ast::Pattern::MacroCall { args, .. } => {
                if let Some(args) = args {
                    for arg in args {
                        self.pattern_in(arg, live, fresh.as_deref_mut());
                    }
                }
            }
        }
    }
}

/// An expression's address, which identifies the node lowering reads.
fn address(expr: &ast::Expr) -> usize {
    std::ptr::from_ref(expr) as usize
}
