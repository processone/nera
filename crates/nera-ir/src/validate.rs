//! Post-lowering validation: detect unresolved remote calls.
//!
//! After all modules have been lowered to IR, this pass scans every
//! `CallTarget::Remote` and `MakeRemoteFunRef` instruction to verify
//! that the target function is either:
//! - defined in one of the co-compiled modules, or
//! - a known BIF from the unified BIF table.
//!
//! Unresolved calls are reported as errors so the user knows which
//! modules/functions are missing before attempting to run the binary.

use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{self, AtomId};
use nera_syntax::span::Span;

use crate::bif_table;
use crate::ir::{CallTarget, IrModule, Instruction, Terminator};

/// Why a call could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnresolvedKind {
    /// The target function is not defined anywhere.
    NotDefined,
    /// The target function is defined in the module but not exported.
    NotExported,
}

/// A single unresolved call site.
#[derive(Debug, Clone)]
pub struct UnresolvedCall {
    /// Source module where the call originates.
    pub caller_module: String,
    /// Target module name.
    pub target_module: String,
    /// Target function name.
    pub target_function: String,
    /// Arity.
    pub arity: u32,
    /// Why the call could not be resolved.
    pub kind: UnresolvedKind,
}

/// Validate that all remote calls across `modules` resolve to a known target.
///
/// `atom_names` maps AtomId → string for resolving symbolic names.
///
/// Returns a list of unresolved calls (empty = all good).
pub fn check_unresolved_calls(
    modules: &[IrModule],
    atom_names: &HashMap<AtomId, String>,
) -> Vec<UnresolvedCall> {
    // 1. Collect every defined function: (module_atom_id, function_atom_id, arity)
    let mut defined: HashSet<(u32, u32, u32)> = HashSet::new();
    let mut exported: HashSet<(u32, u32, u32)> = HashSet::new();
    for module in modules {
        for func in &module.functions {
            defined.insert((module.name.0, func.name.0, func.arity));
        }
        for exp in &module.exports {
            exported.insert((module.name.0, exp.name.0, exp.arity));
        }
    }

    // 2. Scan all instructions for remote calls and fun refs.
    let mut unresolved = Vec::new();
    // Track unique (mod, fun, arity) to avoid duplicate reports.
    let mut seen: HashSet<(u32, u32, u32)> = HashSet::new();

    for module in modules {
        let caller_mod_name = atom_names
            .get(&module.name)
            .cloned()
            .unwrap_or_else(|| format!("?{}", module.name.0));

        for func in &module.functions {
            for block in &func.blocks {
                for instr in &block.instructions {
                    match instr {
                        Instruction::Call(_result, CallTarget::Remote(mod_id, fun_id), args) => {
                            let arity = args.len() as u32;
                            // Skip calls within the same module (local-to-remote
                            // rewrites such as auto-imported BIFs share the
                            // module atom ID, but intra-module calls don't need
                            // export checks).
                            if mod_id.0 != module.name.0 {
                                check_target(
                                    mod_id.0,
                                    fun_id.0,
                                    arity,
                                    &caller_mod_name,
                                    &defined,
                                    &exported,
                                    atom_names,
                                    &mut seen,
                                    &mut unresolved,
                                );
                            }
                        }
                        Instruction::MakeRemoteFunRef(_result, mod_id, fun_id, arity) => {
                            if mod_id.0 != module.name.0 {
                                check_target(
                                    mod_id.0,
                                    fun_id.0,
                                    *arity,
                                    &caller_mod_name,
                                    &defined,
                                    &exported,
                                    atom_names,
                                    &mut seen,
                                    &mut unresolved,
                                );
                            }
                        }
                        Instruction::Spawn(_result, mod_id, fun_id, _args)
                            // Spawn passes an args list variable whose length
                            // is only known at runtime, so we can only check
                            // that the target module has *any* function with
                            // this name (ignoring arity).
                            if mod_id.0 != module.name.0 => {
                                check_spawn_target(
                                    mod_id.0,
                                    fun_id.0,
                                    &caller_mod_name,
                                    &defined,
                                    &exported,
                                    atom_names,
                                    &mut seen,
                                    &mut unresolved,
                                );
                            }
                        _ => {}
                    }
                }
                // Also check terminators (TailCall can carry Remote targets)
                {
                    if let Terminator::TailCall(CallTarget::Remote(mod_id, fun_id), args) = &block.terminator {
                        let arity = args.len() as u32;
                        if mod_id.0 != module.name.0 {
                            check_target(
                                mod_id.0,
                                fun_id.0,
                                arity,
                                &caller_mod_name,
                                &defined,
                                &exported,
                                atom_names,
                                &mut seen,
                                &mut unresolved,
                            );
                        }
                    }
                }
            }
        }
    }

    // Sort for deterministic output.
    unresolved.sort_by(|a, b| {
        a.target_module
            .cmp(&b.target_module)
            .then_with(|| a.target_function.cmp(&b.target_function))
            .then_with(|| a.arity.cmp(&b.arity))
    });

    unresolved
}

#[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
fn check_target(
    mod_id: u32,
    fun_id: u32,
    arity: u32,
    caller_mod_name: &str,
    defined: &HashSet<(u32, u32, u32)>,
    exported: &HashSet<(u32, u32, u32)>,
    atom_names: &HashMap<AtomId, String>,
    seen: &mut HashSet<(u32, u32, u32)>,
    unresolved: &mut Vec<UnresolvedCall>,
) {
    let key = (mod_id, fun_id, arity);
    if seen.contains(&key) {
        return;
    }

    let mod_name = atom_names
        .get(&AtomId(mod_id))
        .cloned()
        .unwrap_or_else(|| format!("?{}", mod_id));
    let fun_name = atom_names
        .get(&AtomId(fun_id))
        .cloned()
        .unwrap_or_else(|| format!("?{}", fun_id));

    // Check BIF table first (BIFs are always available).
    if bif_table::is_known_bif(&mod_name, &fun_name, arity) {
        return;
    }

    // Check co-compiled modules: defined AND exported.
    if defined.contains(&key) {
        if exported.contains(&key) {
            return; // Defined and exported — all good.
        }
        // Defined but not exported.
        seen.insert(key);
        unresolved.push(UnresolvedCall {
            caller_module: caller_mod_name.to_string(),
            target_module: mod_name,
            target_function: fun_name,
            arity,
            kind: UnresolvedKind::NotExported,
        });
        return;
    }

    // Not defined at all in co-compiled modules (and not a BIF).
    seen.insert(key);
    unresolved.push(UnresolvedCall {
        caller_module: caller_mod_name.to_string(),
        target_module: mod_name,
        target_function: fun_name,
        arity,
        kind: UnresolvedKind::NotDefined,
    });
}

/// Check a spawn target (module + function, any arity).
///
/// For `spawn(Mod, Fun, ArgsList)`, the arity is only known at runtime
/// (it's the length of ArgsList), so we check that *some* function with
/// that name exists in the target module, regardless of arity.
#[expect(clippy::too_many_arguments, reason = "Explicit compiler state and continuation operands are kept aligned with the existing lowering interface")]
fn check_spawn_target(
    mod_id: u32,
    fun_id: u32,
    caller_mod_name: &str,
    defined: &HashSet<(u32, u32, u32)>,
    exported: &HashSet<(u32, u32, u32)>,
    atom_names: &HashMap<AtomId, String>,
    seen: &mut HashSet<(u32, u32, u32)>,
    unresolved: &mut Vec<UnresolvedCall>,
) {
    // Use a sentinel arity (u32::MAX) for dedup of spawn checks.
    let key = (mod_id, fun_id, u32::MAX);
    if seen.contains(&key) {
        return;
    }

    let mod_name = atom_names
        .get(&AtomId(mod_id))
        .cloned()
        .unwrap_or_else(|| format!("?{}", mod_id));
    let fun_name = atom_names
        .get(&AtomId(fun_id))
        .cloned()
        .unwrap_or_else(|| format!("?{}", fun_id));

    // Check if any BIF with this module+function exists at any arity.
    if bif_table::BIF_TABLE
        .iter()
        .any(|e| e.module == mod_name && e.function == fun_name)
    {
        return;
    }

    // Check if any function with this (module, function) pair exists at any arity.
    let has_any_defined = defined.iter().any(|(m, f, _)| *m == mod_id && *f == fun_id);
    if has_any_defined {
        // Check if any of those are exported.
        let has_any_exported = exported.iter().any(|(m, f, _)| *m == mod_id && *f == fun_id);
        if has_any_exported {
            return;
        }
        // Defined but none exported.
        seen.insert(key);
        unresolved.push(UnresolvedCall {
            caller_module: caller_mod_name.to_string(),
            target_module: mod_name,
            target_function: fun_name,
            arity: u32::MAX,
            kind: UnresolvedKind::NotExported,
        });
        return;
    }

    // Not defined at all.
    seen.insert(key);
    unresolved.push(UnresolvedCall {
        caller_module: caller_mod_name.to_string(),
        target_module: mod_name,
        target_function: fun_name,
        arity: u32::MAX, // sentinel: arity unknown
        kind: UnresolvedKind::NotDefined,
    });
}

// ---------------------------------------------------------------------------
// AST call-site finder — used for rich error reporting
// ---------------------------------------------------------------------------

/// Find the source span of the first call to `target_module:target_function/arity`
/// in the given AST module.  Returns `None` if no matching call site is found.
///
/// When `target_arity` is `u32::MAX`, any arity is accepted (used for spawn targets).
pub fn find_call_site(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    target_module: &str,
    target_function: &str,
    target_arity: u32,
) -> Option<Span> {
    for func in &module.functions {
        for clause in &func.clauses {
            if let Some(span) = find_in_exprs(&clause.body, atom_names, target_module, target_function, target_arity) {
                return Some(span);
            }
            if let Some(guard) = &clause.guard {
                if let Some(span) = find_in_guard(guard, atom_names, target_module, target_function, target_arity) {
                    return Some(span);
                }
            }
        }
    }
    None
}

fn find_in_exprs(
    exprs: &[ast::Expr],
    atoms: &HashMap<AtomId, String>,
    tm: &str,
    tf: &str,
    ta: u32,
) -> Option<Span> {
    for expr in exprs {
        if let Some(s) = find_in_expr(expr, atoms, tm, tf, ta) {
            return Some(s);
        }
    }
    None
}

fn find_in_expr(
    expr: &ast::Expr,
    atoms: &HashMap<AtomId, String>,
    tm: &str,
    tf: &str,
    ta: u32,
) -> Option<Span> {
    match expr {
        ast::Expr::Call(ast::CallTarget::Remote(mod_expr, fun_expr), args, span) => {
            // Check if this is the call we're looking for
            if let (ast::Expr::Atom(mod_id, _), ast::Expr::Atom(fun_id, _)) =
                (mod_expr.as_ref(), fun_expr.as_ref())
            {
                let m = atoms.get(mod_id).map(|s| s.as_str()).unwrap_or("");
                let f = atoms.get(fun_id).map(|s| s.as_str()).unwrap_or("");
                if m == tm && f == tf && (ta == u32::MAX || args.len() as u32 == ta) {
                    return Some(*span);
                }
            }
            // Also recurse into module/function expressions and arguments
            if let Some(s) = find_in_expr(mod_expr, atoms, tm, tf, ta) {
                return Some(s);
            }
            if let Some(s) = find_in_expr(fun_expr, atoms, tm, tf, ta) {
                return Some(s);
            }
            find_in_exprs(args, atoms, tm, tf, ta)
        }
        ast::Expr::FunRef(ast::FunRefTarget::Remote(mod_id, fun_id, arity), span) => {
            let m = atoms.get(mod_id).map(|s| s.as_str()).unwrap_or("");
            let f = atoms.get(fun_id).map(|s| s.as_str()).unwrap_or("");
            if m == tm && f == tf && (ta == u32::MAX || *arity == ta) {
                return Some(*span);
            }
            None
        }
        // Recurse into all sub-expressions
        ast::Expr::Call(_, args, _) => find_in_exprs(args, atoms, tm, tf, ta),
        ast::Expr::Tuple(elems, _) => find_in_exprs(elems, atoms, tm, tf, ta),
        ast::Expr::Cons(head, tail, _) => {
            find_in_expr(head, atoms, tm, tf, ta).or_else(|| find_in_expr(tail, atoms, tm, tf, ta))
        }
        ast::Expr::BinOp(_, lhs, rhs, _) => {
            find_in_expr(lhs, atoms, tm, tf, ta).or_else(|| find_in_expr(rhs, atoms, tm, tf, ta))
        }
        ast::Expr::UnaryOp(_, operand, _) => find_in_expr(operand, atoms, tm, tf, ta),
        ast::Expr::Match(_, rhs, _) => find_in_expr(rhs, atoms, tm, tf, ta),
        ast::Expr::Case(scrutinee, clauses, _) => {
            find_in_expr(scrutinee, atoms, tm, tf, ta)
                .or_else(|| find_in_case_clauses(clauses, atoms, tm, tf, ta))
        }
        ast::Expr::If(clauses, _) => {
            for c in clauses {
                if let Some(s) = find_in_guard(&c.guard, atoms, tm, tf, ta) {
                    return Some(s);
                }
                if let Some(s) = find_in_exprs(&c.body, atoms, tm, tf, ta) {
                    return Some(s);
                }
            }
            None
        }
        ast::Expr::Receive(clauses, after, _) => {
            find_in_case_clauses(clauses, atoms, tm, tf, ta).or_else(|| {
                after.as_ref().and_then(|a| {
                    find_in_expr(&a.timeout, atoms, tm, tf, ta)
                        .or_else(|| find_in_exprs(&a.body, atoms, tm, tf, ta))
                })
            })
        }
        ast::Expr::Send(lhs, rhs, _) => {
            find_in_expr(lhs, atoms, tm, tf, ta).or_else(|| find_in_expr(rhs, atoms, tm, tf, ta))
        }
        ast::Expr::Block(body, _) => find_in_exprs(body, atoms, tm, tf, ta),
        ast::Expr::Binary(segs, _) => {
            for seg in segs {
                if let Some(s) = find_in_expr(&seg.value, atoms, tm, tf, ta) {
                    return Some(s);
                }
                if let Some(size) = &seg.size {
                    if let Some(s) = find_in_expr(size, atoms, tm, tf, ta) {
                        return Some(s);
                    }
                }
            }
            None
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                if let Some(s) = find_in_expr(&f.key, atoms, tm, tf, ta)
                    .or_else(|| find_in_expr(&f.value, atoms, tm, tf, ta))
                {
                    return Some(s);
                }
            }
            None
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            if let Some(s) = find_in_expr(base, atoms, tm, tf, ta) {
                return Some(s);
            }
            for f in fields {
                if let Some(s) = find_in_expr(&f.key, atoms, tm, tf, ta)
                    .or_else(|| find_in_expr(&f.value, atoms, tm, tf, ta))
                {
                    return Some(s);
                }
            }
            None
        }
        ast::Expr::ListComp(body, quals, _) | ast::Expr::BinComp(body, quals, _) => {
            if let Some(s) = find_in_expr(body, atoms, tm, tf, ta) {
                return Some(s);
            }
            for q in quals {
                match q {
                    ast::LcQualifier::Generator(_, gen_expr, _)
                    | ast::LcQualifier::BinGenerator(_, gen_expr, _) => {
                        if let Some(s) = find_in_expr(gen_expr, atoms, tm, tf, ta) {
                            return Some(s);
                        }
                    }
                    ast::LcQualifier::Filter(filter_expr, _) => {
                        if let Some(s) = find_in_expr(filter_expr, atoms, tm, tf, ta) {
                            return Some(s);
                        }
                    }
                }
            }
            None
        }
        ast::Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
            find_in_exprs(body, atoms, tm, tf, ta)
                .or_else(|| find_in_case_clauses(of_clauses, atoms, tm, tf, ta))
                .or_else(|| {
                    for c in catch_clauses {
                        if let Some(s) = find_in_exprs(&c.body, atoms, tm, tf, ta) {
                            return Some(s);
                        }
                    }
                    None
                })
                .or_else(|| after_body.as_ref().and_then(|b| find_in_exprs(b, atoms, tm, tf, ta)))
        }
        ast::Expr::Throw(inner, _) | ast::Expr::Catch(inner, _) => {
            find_in_expr(inner, atoms, tm, tf, ta)
        }
        ast::Expr::Fun(_name, clauses, _) => {
            for c in clauses {
                if let Some(s) = find_in_exprs(&c.body, atoms, tm, tf, ta) {
                    return Some(s);
                }
            }
            None
        }
        ast::Expr::RecordCreate { fields, .. } | ast::Expr::RecordUpdate { fields, .. } => {
            for f in fields {
                if let Some(s) = find_in_expr(&f.value, atoms, tm, tf, ta) {
                    return Some(s);
                }
            }
            if let ast::Expr::RecordUpdate { base, .. } = expr {
                return find_in_expr(base, atoms, tm, tf, ta);
            }
            None
        }
        ast::Expr::RecordAccess { base, .. } => find_in_expr(base, atoms, tm, tf, ta),
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                let result = match item {
                    ast::MaybeExpr::Expr(e) => find_in_expr(e, atoms, tm, tf, ta),
                    ast::MaybeExpr::CondMatch(_, e, _) => find_in_expr(e, atoms, tm, tf, ta),
                };
                if result.is_some() {
                    return result;
                }
            }
            find_in_case_clauses(else_clauses, atoms, tm, tf, ta)
        }
        // Leaf nodes — no sub-expressions
        ast::Expr::Integer(..)
        | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(..)
        | ast::Expr::Atom(..)
        | ast::Expr::String(..)
        | ast::Expr::Nil(..)
        | ast::Expr::Variable(..)
        | ast::Expr::FunRef(..)
        | ast::Expr::RecordIndex { .. } => None,
        ast::Expr::MacroCall { .. } => unreachable!("MacroCall only in formatter path"),
    }
}

fn find_in_case_clauses(
    clauses: &[ast::CaseClause],
    atoms: &HashMap<AtomId, String>,
    tm: &str,
    tf: &str,
    ta: u32,
) -> Option<Span> {
    for c in clauses {
        if let Some(guard) = &c.guard {
            if let Some(s) = find_in_guard(guard, atoms, tm, tf, ta) {
                return Some(s);
            }
        }
        if let Some(s) = find_in_exprs(&c.body, atoms, tm, tf, ta) {
            return Some(s);
        }
    }
    None
}

fn find_in_guard(
    guard: &ast::Guard,
    atoms: &HashMap<AtomId, String>,
    tm: &str,
    tf: &str,
    ta: u32,
) -> Option<Span> {
    match guard {
        ast::Guard::Test(expr) => find_in_expr(expr, atoms, tm, tf, ta),
        ast::Guard::Conjunction(exprs) => find_in_exprs(exprs, atoms, tm, tf, ta),
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                if let Some(s) = find_in_guard(g, atoms, tm, tf, ta) {
                    return Some(s);
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    /// Helper: create an atom name map from a slice of (id, name) pairs.
    fn atom_map(entries: &[(u32, &str)]) -> HashMap<AtomId, String> {
        entries
            .iter()
            .map(|(id, name)| (AtomId(*id), name.to_string()))
            .collect()
    }

    /// Helper: build a minimal IrModule with one function containing given instructions.
    /// The function is NOT exported.
    fn make_module(
        mod_id: u32,
        func_name: u32,
        arity: u32,
        instructions: Vec<Instruction>,
    ) -> IrModule {
        IrModule {
            name: AtomId(mod_id),
            functions: vec![IrFunction {
                name: AtomId(func_name),
                arity,
                mangled_name: format!("{}__{}_{}", mod_id, func_name, arity),
                blocks: vec![BasicBlock {
                    id: BlockId(0),
                    params: vec![],
                    instructions,
                    terminator: Terminator::Return(VarId(0)),
                }],
                protected_values: Vec::new(),
                entry_block: BlockId(0),
                params: vec![],
                line: 0,
            }],
            exports: vec![],
        }
    }

    /// Helper: build a minimal IrModule with one exported function.
    fn make_exported_module(
        mod_id: u32,
        func_name: u32,
        arity: u32,
        instructions: Vec<Instruction>,
    ) -> IrModule {
        IrModule {
            name: AtomId(mod_id),
            functions: vec![IrFunction {
                name: AtomId(func_name),
                arity,
                mangled_name: format!("{}__{}_{}", mod_id, func_name, arity),
                blocks: vec![BasicBlock {
                    id: BlockId(0),
                    params: vec![],
                    instructions,
                    terminator: Terminator::Return(VarId(0)),
                }],
                protected_values: Vec::new(),
                entry_block: BlockId(0),
                params: vec![],
                line: 0,
            }],
            exports: vec![IrExport {
                name: AtomId(func_name),
                arity,
            }],
        }
    }

    #[test]
    fn test_call_to_cocompiled_module_is_resolved() {
        // Module A calls B:foo/1; B defines and exports foo/1 → no errors.
        let atoms = atom_map(&[(0, "mod_a"), (1, "mod_b"), (2, "test"), (3, "foo")]);

        let mod_a = make_module(
            0,
            2,
            0,
            vec![Instruction::Call(
                VarId(1),
                CallTarget::Remote(AtomId(1), AtomId(3)),
                vec![VarId(0)],
            )],
        );
        let mod_b = make_exported_module(1, 3, 1, vec![]);

        let result = check_unresolved_calls(&[mod_a, mod_b], &atoms);
        assert!(result.is_empty(), "expected no unresolved calls, got: {:?}", result);
    }

    #[test]
    fn test_call_to_missing_module_is_unresolved() {
        // Module A calls missing:bar/2 → error.
        let atoms = atom_map(&[
            (0, "mod_a"),
            (1, "test"),
            (2, "missing"),
            (3, "bar"),
        ]);

        let mod_a = make_module(
            0,
            1,
            0,
            vec![Instruction::Call(
                VarId(1),
                CallTarget::Remote(AtomId(2), AtomId(3)),
                vec![VarId(0), VarId(0)],
            )],
        );

        let result = check_unresolved_calls(&[mod_a], &atoms);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].target_module, "missing");
        assert_eq!(result[0].target_function, "bar");
        assert_eq!(result[0].arity, 2);
    }

    #[test]
    fn test_bif_call_is_resolved() {
        // Call to erlang:length/1 (a known BIF) → no error.
        let atoms = atom_map(&[(0, "mod_a"), (1, "test"), (2, "erlang"), (3, "length")]);

        let mod_a = make_module(
            0,
            1,
            0,
            vec![Instruction::Call(
                VarId(1),
                CallTarget::Remote(AtomId(2), AtomId(3)),
                vec![VarId(0)],
            )],
        );

        let result = check_unresolved_calls(&[mod_a], &atoms);
        assert!(result.is_empty(), "BIF call should resolve, got: {:?}", result);
    }

    #[test]
    fn test_bif_without_ffi_is_resolved() {
        // Call to erlang:apply/3 — BIF with ffi_symbol: None → still resolved.
        let atoms = atom_map(&[(0, "mod_a"), (1, "test"), (2, "erlang"), (3, "apply")]);

        let mod_a = make_module(
            0,
            1,
            0,
            vec![Instruction::Call(
                VarId(1),
                CallTarget::Remote(AtomId(2), AtomId(3)),
                vec![VarId(0), VarId(0), VarId(0)],
            )],
        );

        let result = check_unresolved_calls(&[mod_a], &atoms);
        assert!(result.is_empty(), "BIF without FFI should still resolve, got: {:?}", result);
    }

    #[test]
    fn test_remote_fun_ref_to_missing_is_unresolved() {
        // fun unknown:baz/2 → error.
        let atoms = atom_map(&[(0, "mod_a"), (1, "test"), (2, "unknown"), (3, "baz")]);

        let mod_a = make_module(
            0,
            1,
            0,
            vec![Instruction::MakeRemoteFunRef(
                VarId(1),
                AtomId(2),
                AtomId(3),
                2,
            )],
        );

        let result = check_unresolved_calls(&[mod_a], &atoms);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].target_module, "unknown");
        assert_eq!(result[0].target_function, "baz");
        assert_eq!(result[0].arity, 2);
    }

    #[test]
    fn test_spawn_to_existing_function_is_resolved() {
        // spawn(mod_b, start, []) where mod_b exports start/0 → no error.
        let atoms = atom_map(&[(0, "mod_a"), (1, "test"), (2, "mod_b"), (3, "start")]);

        let mod_a = make_module(
            0,
            1,
            0,
            vec![Instruction::Spawn(
                VarId(1),
                AtomId(2),
                AtomId(3),
                vec![VarId(0)], // args list (runtime length)
            )],
        );
        let mod_b = make_exported_module(2, 3, 0, vec![]);

        let result = check_unresolved_calls(&[mod_a, mod_b], &atoms);
        assert!(result.is_empty(), "spawn to exported function should resolve, got: {:?}", result);
    }

    #[test]
    fn test_spawn_to_missing_module_is_unresolved() {
        // spawn(nonexistent, go, []) → error.
        let atoms = atom_map(&[(0, "mod_a"), (1, "test"), (2, "nonexistent"), (3, "go")]);

        let mod_a = make_module(
            0,
            1,
            0,
            vec![Instruction::Spawn(
                VarId(1),
                AtomId(2),
                AtomId(3),
                vec![VarId(0)],
            )],
        );

        let result = check_unresolved_calls(&[mod_a], &atoms);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].target_module, "nonexistent");
        assert_eq!(result[0].target_function, "go");
        assert_eq!(result[0].arity, u32::MAX); // sentinel
    }

    #[test]
    fn test_duplicate_calls_reported_once() {
        // Two calls to the same missing function → only one report.
        let atoms = atom_map(&[(0, "mod_a"), (1, "test"), (2, "missing"), (3, "foo")]);

        let mod_a = make_module(
            0,
            1,
            0,
            vec![
                Instruction::Call(
                    VarId(1),
                    CallTarget::Remote(AtomId(2), AtomId(3)),
                    vec![VarId(0)],
                ),
                Instruction::Call(
                    VarId(2),
                    CallTarget::Remote(AtomId(2), AtomId(3)),
                    vec![VarId(0)],
                ),
            ],
        );

        let result = check_unresolved_calls(&[mod_a], &atoms);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_local_calls_not_checked() {
        // Local calls (CallTarget::Local) should not trigger unresolved errors.
        let atoms = atom_map(&[(0, "mod_a"), (1, "test"), (2, "helper")]);

        let mod_a = make_module(
            0,
            1,
            0,
            vec![Instruction::Call(
                VarId(1),
                CallTarget::Local(AtomId(2)),
                vec![],
            )],
        );

        let result = check_unresolved_calls(&[mod_a], &atoms);
        assert!(result.is_empty());
    }

    #[test]
    fn test_call_to_unexported_function_is_not_exported() {
        // Module A calls B:foo/1; B defines foo/1 but does NOT export it → NotExported.
        let atoms = atom_map(&[(0, "mod_a"), (1, "mod_b"), (2, "test"), (3, "foo")]);

        let mod_a = make_module(
            0,
            2,
            0,
            vec![Instruction::Call(
                VarId(1),
                CallTarget::Remote(AtomId(1), AtomId(3)),
                vec![VarId(0)],
            )],
        );
        let mod_b = make_module(1, 3, 1, vec![]); // defined but NOT exported

        let result = check_unresolved_calls(&[mod_a, mod_b], &atoms);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].target_module, "mod_b");
        assert_eq!(result[0].target_function, "foo");
        assert_eq!(result[0].arity, 1);
        assert_eq!(result[0].kind, UnresolvedKind::NotExported);
    }

    #[test]
    fn test_call_to_missing_has_not_defined_kind() {
        // Module A calls missing:bar/2 → NotDefined.
        let atoms = atom_map(&[
            (0, "mod_a"),
            (1, "test"),
            (2, "missing"),
            (3, "bar"),
        ]);

        let mod_a = make_module(
            0,
            1,
            0,
            vec![Instruction::Call(
                VarId(1),
                CallTarget::Remote(AtomId(2), AtomId(3)),
                vec![VarId(0), VarId(0)],
            )],
        );

        let result = check_unresolved_calls(&[mod_a], &atoms);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, UnresolvedKind::NotDefined);
    }

    #[test]
    fn test_spawn_to_unexported_function() {
        // spawn(mod_b, start, []) where mod_b:start/0 is defined but not exported.
        let atoms = atom_map(&[(0, "mod_a"), (1, "test"), (2, "mod_b"), (3, "start")]);

        let mod_a = make_module(
            0,
            1,
            0,
            vec![Instruction::Spawn(
                VarId(1),
                AtomId(2),
                AtomId(3),
                vec![VarId(0)],
            )],
        );
        let mod_b = make_module(2, 3, 0, vec![]); // defined but NOT exported

        let result = check_unresolved_calls(&[mod_a, mod_b], &atoms);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, UnresolvedKind::NotExported);
    }
}
