//! Semantic token extraction from parsed Erlang AST.
//!
//! Walks the AST and produces a flat list of [`SemanticToken`] entries,
//! each associating a source [`Span`] with a token type and optional
//! modifiers.  The LSP layer converts these into the delta-encoded
//! format required by `textDocument/semanticTokens/full`.

use std::collections::HashSet;

use crate::ast::{self, AtomId, Module};
use crate::lexer::Lexer;
use crate::span::Span;
use crate::token::Token;

// ── Token type / modifier enums ────────────────────────────────────────

/// Semantic token types understood by editors.
///
/// The ordering here must match the legend sent in the LSP
/// `SemanticTokensOptions` capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TokenType {
    Function = 0,
    Variable = 1,
    Namespace = 2,
    Type = 3,
    Property = 4,
    Macro = 5,
}

/// Bit-flag modifiers for semantic tokens.
///
/// The bit positions must match the legend order in the LSP capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenModifiers(pub u32);

impl TokenModifiers {
    pub const NONE: Self = Self(0);
    /// Index 0 in the modifiers legend.
    pub const DECLARATION: Self = Self(1 << 0);
    /// Index 1 in the modifiers legend.
    pub const DEFINITION: Self = Self(1 << 1);
    /// Index 2 in the modifiers legend — marks `_`-prefixed variables.
    pub const UNUSED: Self = Self(1 << 2);
    /// Index 3 in the modifiers legend — marks exported functions.
    pub const EXPORTED: Self = Self(1 << 3);
    /// Index 4 in the modifiers legend — marks deprecated functions.
    pub const DEPRECATED: Self = Self(1 << 4);
    /// Index 5 in the modifiers legend — marks BIFs / standard library functions.
    pub const DEFAULT_LIBRARY: Self = Self(1 << 5);

    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

// ── SemanticToken ──────────────────────────────────────────────────────

/// A single semantic token extracted from the AST.
#[derive(Debug, Clone)]
pub struct SemanticToken {
    pub span: Span,
    pub token_type: TokenType,
    pub modifiers: TokenModifiers,
}

// ── Collector ──────────────────────────────────────────────────────────

/// Collect semantic tokens from a parsed module.
///
/// `source` is the original source text, used to detect `_`-prefixed
/// variables and mark them as `readonly` (editors render these dimmer).
///
/// Tokens are returned sorted by `span.start` (required by the LSP
/// delta encoding).
pub fn collect_semantic_tokens(module: &Module, source: &str) -> Vec<SemanticToken> {
    collect_semantic_tokens_with_bif_check(module, source, &|_, _| false)
}

/// Collect semantic tokens, using `is_bif` to tag auto-imported BIF calls
/// with the `DEFAULT_LIBRARY` modifier.
pub fn collect_semantic_tokens_with_bif_check(
    module: &Module,
    source: &str,
    is_bif: &dyn Fn(&str, u32) -> bool,
) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();

    // Build set of deprecated function signatures for modifier tagging.
    let deprecated_set: HashSet<(AtomId, u32)> = module
        .deprecated
        .iter()
        .map(|fa| (fa.name, fa.arity))
        .collect();

    // Record definitions.
    for rec in &module.records {
        collect_record_def(rec, &mut tokens, source, &deprecated_set, is_bif);
    }

    // Function definitions.
    for func in &module.functions {
        let is_exported = module.exports.iter().any(|e| e.name == func.name && e.arity == func.arity);
        let is_deprecated = deprecated_set.contains(&(func.name, func.arity));
        let mut mods = TokenModifiers::DECLARATION.union(TokenModifiers::DEFINITION);
        if is_exported {
            mods = mods.union(TokenModifiers::EXPORTED);
        }
        if is_deprecated {
            mods = mods.union(TokenModifiers::DEPRECATED);
        }

        for clause in &func.clauses {
            tokens.push(SemanticToken {
                span: clause.name_span,
                token_type: TokenType::Function,
                modifiers: mods,
            });
            // Function clause: all parameters are fresh bindings (empty scope).
            let empty_scope = HashSet::new();
            let mut clause_scope = HashSet::new();
            for pat in &clause.patterns {
                collect_pattern(pat, &mut tokens, source, &empty_scope, &mut clause_scope, &deprecated_set, is_bif);
            }
            if let Some(guard) = &clause.guard {
                collect_guard(guard, &mut tokens, source, &mut clause_scope, &deprecated_set, is_bif);
            }
            collect_body(&clause.body, &mut tokens, source, &mut clause_scope, &deprecated_set, is_bif);
        }
    }

    tokens.sort_by_key(|t| t.span.start);
    tokens
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Check whether the source text at `span` starts with `_`.
fn is_underscore_prefixed(span: Span, source: &str) -> bool {
    source.as_bytes().get(span.start) == Some(&b'_')
}

/// Return modifiers for a variable definition, adding `READONLY` for `_`-prefixed names.
fn var_def_modifiers(span: Span, source: &str) -> TokenModifiers {
    if is_underscore_prefixed(span, source) {
        TokenModifiers::DEFINITION.union(TokenModifiers::UNUSED)
    } else {
        TokenModifiers::DEFINITION
    }
}

/// Return modifiers for a variable use, adding `READONLY` for `_`-prefixed names.
fn var_use_modifiers(span: Span, source: &str) -> TokenModifiers {
    if is_underscore_prefixed(span, source) {
        TokenModifiers::UNUSED
    } else {
        TokenModifiers::NONE
    }
}

// ── Record definitions ─────────────────────────────────────────────────

fn collect_record_def(rec: &ast::RecordDef, tokens: &mut Vec<SemanticToken>, source: &str, deprecated: &HashSet<(AtomId, u32)>, is_bif: &dyn Fn(&str, u32) -> bool) {
    for field in &rec.fields {
        tokens.push(SemanticToken {
            span: field.name_span,
            token_type: TokenType::Property,
            modifiers: TokenModifiers::DEFINITION,
        });
        if let Some(default) = &field.default {
            // Record defaults are at module level — no variable scope.
            collect_expr(default, tokens, source, &mut HashSet::new(), deprecated, is_bif);
        }
    }
}

// ── Patterns ───────────────────────────────────────────────────────────

/// Collect tokens from a pattern.
///
/// `scope` contains variables already bound in the enclosing scope.
/// `new_bindings` accumulates fresh bindings introduced by this pattern.
/// A variable already in `scope` or `new_bindings` is an equality check,
/// not a fresh binding, and gets use-modifiers instead of DEFINITION.
fn collect_pattern(
    pat: &ast::Pattern,
    tokens: &mut Vec<SemanticToken>,
    source: &str,
    scope: &HashSet<AtomId>,
    new_bindings: &mut HashSet<AtomId>,
    deprecated: &HashSet<(AtomId, u32)>,
    is_bif: &dyn Fn(&str, u32) -> bool,
) {
    match pat {
        ast::Pattern::Variable(id, span) => {
            if scope.contains(id) || new_bindings.contains(id) {
                // Already bound — equality check, not a fresh binding.
                tokens.push(SemanticToken {
                    span: *span,
                    token_type: TokenType::Variable,
                    modifiers: var_use_modifiers(*span, source),
                });
            } else {
                new_bindings.insert(*id);
                tokens.push(SemanticToken {
                    span: *span,
                    token_type: TokenType::Variable,
                    modifiers: var_def_modifiers(*span, source),
                });
            }
        }
        ast::Pattern::Tuple(pats, _) => {
            for p in pats {
                collect_pattern(p, tokens, source, scope, new_bindings, deprecated, is_bif);
            }
        }
        ast::Pattern::Cons(h, t, _) => {
            collect_pattern(h, tokens, source, scope, new_bindings, deprecated, is_bif);
            collect_pattern(t, tokens, source, scope, new_bindings, deprecated, is_bif);
        }
        ast::Pattern::Match(p1, p2, _) => {
            collect_pattern(p1, tokens, source, scope, new_bindings, deprecated, is_bif);
            collect_pattern(p2, tokens, source, scope, new_bindings, deprecated, is_bif);
        }
        ast::Pattern::Binary(segments, _) => {
            for seg in segments {
                collect_pattern(&seg.value, tokens, source, scope, new_bindings, deprecated, is_bif);
                if let Some(size) = &seg.size {
                    // Size expressions see scope ∪ new_bindings.
                    let mut size_scope = scope.clone();
                    size_scope.extend(new_bindings.iter().copied());
                    collect_expr(size, tokens, source, &mut size_scope, deprecated, is_bif);
                }
            }
        }
        ast::Pattern::Map(fields, _) => {
            for field in fields {
                // Map keys are expressions that see scope ∪ new_bindings.
                let mut key_scope = scope.clone();
                key_scope.extend(new_bindings.iter().copied());
                collect_expr(&field.key, tokens, source, &mut key_scope, deprecated, is_bif);
                collect_pattern(&field.value, tokens, source, scope, new_bindings, deprecated, is_bif);
            }
        }
        ast::Pattern::Record { fields, .. } => {
            for field in fields {
                tokens.push(SemanticToken {
                    span: field.field_name_span,
                    token_type: TokenType::Property,
                    modifiers: TokenModifiers::NONE,
                });
                collect_pattern(&field.pattern, tokens, source, scope, new_bindings, deprecated, is_bif);
            }
        }
        ast::Pattern::Concat(lhs, rhs, _) => {
            collect_pattern(lhs, tokens, source, scope, new_bindings, deprecated, is_bif);
            collect_pattern(rhs, tokens, source, scope, new_bindings, deprecated, is_bif);
        }
        // Literals and wildcards — no semantic tokens needed.
        ast::Pattern::Wildcard(_)
        | ast::Pattern::Integer(_, _)
        | ast::Pattern::BigInteger(_, _)
        | ast::Pattern::Float(_, _)
        | ast::Pattern::Atom(_, _)
        | ast::Pattern::String(_, _)
        | ast::Pattern::Nil(_) => {}
        // Unexpanded macros — only in formatter path, not reachable from LSP.
        ast::Pattern::MacroCall { .. } => {}
    }
}

// ── Guards ──────────────────────────────────────────────────────────────

fn collect_guard(
    guard: &ast::Guard,
    tokens: &mut Vec<SemanticToken>,
    source: &str,
    scope: &mut HashSet<AtomId>,
    deprecated: &HashSet<(AtomId, u32)>,
    is_bif: &dyn Fn(&str, u32) -> bool,
) {
    match guard {
        ast::Guard::Test(expr) => collect_expr(expr, tokens, source, scope, deprecated, is_bif),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                collect_expr(e, tokens, source, scope, deprecated, is_bif);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                collect_guard(g, tokens, source, scope, deprecated, is_bif);
            }
        }
    }
}

// ── Expressions ─────────────────────────────────────────────────────────

/// Collect tokens from an expression.
///
/// `scope` is mutable: body-level match expressions extend it so that
/// subsequent expressions in the same body see the new bindings.
fn collect_expr(
    expr: &ast::Expr,
    tokens: &mut Vec<SemanticToken>,
    source: &str,
    scope: &mut HashSet<AtomId>,
    deprecated: &HashSet<(AtomId, u32)>,
    is_bif: &dyn Fn(&str, u32) -> bool,
) {
    match expr {
        ast::Expr::Variable(_, span) => {
            tokens.push(SemanticToken {
                span: *span,
                token_type: TokenType::Variable,
                modifiers: var_use_modifiers(*span, source),
            });
        }

        ast::Expr::Call(target, args, _) => {
            match target {
                ast::CallTarget::Local(name, name_span) => {
                    let mut mods = if deprecated.contains(&(*name, args.len() as u32)) {
                        TokenModifiers::DEPRECATED
                    } else {
                        TokenModifiers::NONE
                    };
                    // Check if this is an auto-imported BIF.
                    let fn_text = &source[name_span.start..name_span.end];
                    if is_bif(fn_text, args.len() as u32) {
                        mods = mods.union(TokenModifiers::DEFAULT_LIBRARY);
                    }
                    tokens.push(SemanticToken {
                        span: *name_span,
                        token_type: TokenType::Function,
                        modifiers: mods,
                    });
                }
                ast::CallTarget::Remote(module_expr, fun_expr) => {
                    // Module part — mark as namespace.
                    if let ast::Expr::Atom(_, span) = module_expr.as_ref() {
                        tokens.push(SemanticToken {
                            span: *span,
                            token_type: TokenType::Namespace,
                            modifiers: TokenModifiers::NONE,
                        });
                    } else {
                        collect_expr(module_expr, tokens, source, scope, deprecated, is_bif);
                    }
                    // Function part — mark as function.
                    if let ast::Expr::Atom(_, span) = fun_expr.as_ref() {
                        tokens.push(SemanticToken {
                            span: *span,
                            token_type: TokenType::Function,
                            modifiers: TokenModifiers::NONE,
                        });
                    } else {
                        collect_expr(fun_expr, tokens, source, scope, deprecated, is_bif);
                    }
                }
                ast::CallTarget::Variable(_, var_span) => {
                    tokens.push(SemanticToken {
                        span: *var_span,
                        token_type: TokenType::Variable,
                        modifiers: var_use_modifiers(*var_span, source),
                    });
                }
                ast::CallTarget::Expr(callee) => {
                    collect_expr(callee, tokens, source, scope, deprecated, is_bif);
                }
            }
            for arg in args {
                collect_expr(arg, tokens, source, scope, deprecated, is_bif);
            }
        }

        ast::Expr::FunRef(target, _) => {
            match target {
                ast::FunRefTarget::Local(_, _) => {
                    // FunRefTarget doesn't carry individual spans yet.
                }
                ast::FunRefTarget::Remote(_, _, _) => {
                    // Same — no sub-spans available.
                }
                ast::FunRefTarget::DynamicRemote(m, f, a) => {
                    collect_expr(m, tokens, source, scope, deprecated, is_bif);
                    collect_expr(f, tokens, source, scope, deprecated, is_bif);
                    collect_expr(a, tokens, source, scope, deprecated, is_bif);
                }
            }
        }

        ast::Expr::Match(pat, e, _) => {
            collect_expr(e, tokens, source, scope, deprecated, is_bif);
            let mut new_bindings = HashSet::new();
            collect_pattern(pat, tokens, source, scope, &mut new_bindings, deprecated, is_bif);
            scope.extend(new_bindings);
        }

        ast::Expr::Case(scrutinee, clauses, _) => {
            collect_expr(scrutinee, tokens, source, scope, deprecated, is_bif);
            for clause in clauses {
                collect_case_clause(clause, tokens, source, scope, deprecated, is_bif);
            }
        }

        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                let mut clause_scope = scope.clone();
                collect_guard(&clause.guard, tokens, source, &mut clause_scope, deprecated, is_bif);
                collect_body(&clause.body, tokens, source, &mut clause_scope, deprecated, is_bif);
            }
        }

        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                collect_case_clause(clause, tokens, source, scope, deprecated, is_bif);
            }
            if let Some(after_clause) = after {
                let mut after_scope = scope.clone();
                collect_expr(&after_clause.timeout, tokens, source, &mut after_scope, deprecated, is_bif);
                collect_body(&after_clause.body, tokens, source, &mut after_scope, deprecated, is_bif);
            }
        }

        ast::Expr::Fun(_name, clauses, _) => {
            for clause in clauses {
                let empty_scope = HashSet::new();
                let mut clause_scope = HashSet::new();
                for pat in &clause.patterns {
                    collect_pattern(pat, tokens, source, &empty_scope, &mut clause_scope, deprecated, is_bif);
                }
                if let Some(guard) = &clause.guard {
                    collect_guard(guard, tokens, source, &mut clause_scope, deprecated, is_bif);
                }
                collect_body(&clause.body, tokens, source, &mut clause_scope, deprecated, is_bif);
            }
        }

        ast::Expr::Tuple(exprs, _) => {
            for e in exprs {
                collect_expr(e, tokens, source, scope, deprecated, is_bif);
            }
        }

        ast::Expr::Block(exprs, _) => {
            let mut block_scope = scope.clone();
            collect_body(exprs, tokens, source, &mut block_scope, deprecated, is_bif);
        }

        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) => {
            collect_expr(h, tokens, source, scope, deprecated, is_bif);
            collect_expr(t, tokens, source, scope, deprecated, is_bif);
        }

        ast::Expr::BinOp(_, lhs, rhs, _) => {
            collect_expr(lhs, tokens, source, scope, deprecated, is_bif);
            collect_expr(rhs, tokens, source, scope, deprecated, is_bif);
        }

        ast::Expr::UnaryOp(_, operand, _) => {
            collect_expr(operand, tokens, source, scope, deprecated, is_bif);
        }

        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                collect_expr(&seg.value, tokens, source, scope, deprecated, is_bif);
                if let Some(size) = &seg.size {
                    collect_expr(size, tokens, source, scope, deprecated, is_bif);
                }
            }
        }

        ast::Expr::Map(fields, _) => {
            for field in fields {
                collect_expr(&field.key, tokens, source, scope, deprecated, is_bif);
                collect_expr(&field.value, tokens, source, scope, deprecated, is_bif);
            }
        }

        ast::Expr::MapUpdate(base, fields, _) => {
            collect_expr(base, tokens, source, scope, deprecated, is_bif);
            for field in fields {
                collect_expr(&field.key, tokens, source, scope, deprecated, is_bif);
                collect_expr(&field.value, tokens, source, scope, deprecated, is_bif);
            }
        }

        ast::Expr::ListComp(template, qualifiers, _)
        | ast::Expr::BinComp(template, qualifiers, _) => {
            let mut lc_scope = scope.clone();
            for qual in qualifiers {
                match qual {
                    ast::LcQualifier::Generator(pat, expr, _)
                    | ast::LcQualifier::BinGenerator(pat, expr, _) => {
                        collect_expr(expr, tokens, source, &mut lc_scope, deprecated, is_bif);
                        let mut new_bindings = HashSet::new();
                        collect_pattern(pat, tokens, source, &lc_scope, &mut new_bindings, deprecated, is_bif);
                        lc_scope.extend(new_bindings);
                    }
                    ast::LcQualifier::Filter(expr, _) => {
                        collect_expr(expr, tokens, source, &mut lc_scope, deprecated, is_bif);
                    }
                }
            }
            collect_expr(template, tokens, source, &mut lc_scope, deprecated, is_bif);
        }

        ast::Expr::Try {
            body,
            of_clauses,
            catch_clauses,
            after_body,
            ..
        } => {
            let mut try_scope = scope.clone();
            collect_body(body, tokens, source, &mut try_scope, deprecated, is_bif);
            for clause in of_clauses {
                collect_case_clause(clause, tokens, source, &try_scope, deprecated, is_bif);
            }
            for catch_clause in catch_clauses {
                let mut catch_scope = scope.clone();
                let mut catch_bindings = HashSet::new();
                if let ast::CatchClass::Variable(id, span) = &catch_clause.class {
                    if catch_scope.contains(id) || catch_bindings.contains(id) {
                        tokens.push(SemanticToken {
                            span: *span,
                            token_type: TokenType::Variable,
                            modifiers: var_use_modifiers(*span, source),
                        });
                    } else {
                        catch_bindings.insert(*id);
                        tokens.push(SemanticToken {
                            span: *span,
                            token_type: TokenType::Variable,
                            modifiers: var_def_modifiers(*span, source),
                        });
                    }
                }
                collect_pattern(&catch_clause.pattern, tokens, source, &catch_scope, &mut catch_bindings, deprecated, is_bif);
                if let Some(st) = &catch_clause.stacktrace {
                    collect_pattern(st, tokens, source, &catch_scope, &mut catch_bindings, deprecated, is_bif);
                }
                catch_scope.extend(catch_bindings);
                if let Some(guard) = &catch_clause.guard {
                    collect_guard(guard, tokens, source, &mut catch_scope, deprecated, is_bif);
                }
                collect_body(&catch_clause.body, tokens, source, &mut catch_scope, deprecated, is_bif);
            }
            if let Some(after_exprs) = after_body {
                let mut after_scope = scope.clone();
                collect_body(after_exprs, tokens, source, &mut after_scope, deprecated, is_bif);
            }
        }

        ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            collect_expr(e, tokens, source, scope, deprecated, is_bif);
        }

        ast::Expr::RecordCreate { fields, .. } => {
            for field in fields {
                tokens.push(SemanticToken {
                    span: field.field_name_span,
                    token_type: TokenType::Property,
                    modifiers: TokenModifiers::NONE,
                });
                collect_expr(&field.value, tokens, source, scope, deprecated, is_bif);
            }
        }

        ast::Expr::RecordUpdate { base, fields, .. } => {
            collect_expr(base, tokens, source, scope, deprecated, is_bif);
            for field in fields {
                tokens.push(SemanticToken {
                    span: field.field_name_span,
                    token_type: TokenType::Property,
                    modifiers: TokenModifiers::NONE,
                });
                collect_expr(&field.value, tokens, source, scope, deprecated, is_bif);
            }
        }

        ast::Expr::RecordAccess { base, field_name_span, .. } => {
            collect_expr(base, tokens, source, scope, deprecated, is_bif);
            tokens.push(SemanticToken {
                span: *field_name_span,
                token_type: TokenType::Property,
                modifiers: TokenModifiers::NONE,
            });
        }

        ast::Expr::RecordIndex { field_name_span, .. } => {
            tokens.push(SemanticToken {
                span: *field_name_span,
                token_type: TokenType::Property,
                modifiers: TokenModifiers::NONE,
            });
        }

        ast::Expr::Maybe { body, else_clauses, .. } => {
            let outer_scope = scope.clone();
            let mut maybe_scope = scope.clone();
            for maybe_expr in body {
                match maybe_expr {
                    ast::MaybeExpr::Expr(e) => collect_expr(e, tokens, source, &mut maybe_scope, deprecated, is_bif),
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        collect_expr(e, tokens, source, &mut maybe_scope, deprecated, is_bif);
                        let mut new_bindings = HashSet::new();
                        collect_pattern(pat, tokens, source, &maybe_scope, &mut new_bindings, deprecated, is_bif);
                        maybe_scope.extend(new_bindings);
                    }
                }
            }
            for clause in else_clauses {
                collect_case_clause(clause, tokens, source, &outer_scope, deprecated, is_bif);
            }
        }

        // Literals — no semantic tokens needed (TextMate handles them).
        ast::Expr::Integer(_, _)
        | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(_, _)
        | ast::Expr::Atom(_, _)
        | ast::Expr::String(_, _)
        | ast::Expr::Nil(_) => {}
        // Unexpanded macros — only in formatter path, not reachable from LSP.
        ast::Expr::MacroCall { .. } => {}
    }
}

// ── Case / receive clause helper ───────────────────────────────────────

fn collect_case_clause(
    clause: &ast::CaseClause,
    tokens: &mut Vec<SemanticToken>,
    source: &str,
    outer_scope: &HashSet<AtomId>,
    deprecated: &HashSet<(AtomId, u32)>,
    is_bif: &dyn Fn(&str, u32) -> bool,
) {
    let mut new_bindings = HashSet::new();
    collect_pattern(&clause.pattern, tokens, source, outer_scope, &mut new_bindings, deprecated, is_bif);
    let mut clause_scope = outer_scope.clone();
    clause_scope.extend(new_bindings);
    if let Some(guard) = &clause.guard {
        collect_guard(guard, tokens, source, &mut clause_scope, deprecated, is_bif);
    }
    collect_body(&clause.body, tokens, source, &mut clause_scope, deprecated, is_bif);
}

/// Walk a body (sequence of expressions) where match bindings accumulate.
fn collect_body(
    exprs: &[ast::Expr],
    tokens: &mut Vec<SemanticToken>,
    source: &str,
    scope: &mut HashSet<AtomId>,
    deprecated: &HashSet<(AtomId, u32)>,
    is_bif: &dyn Fn(&str, u32) -> bool,
) {
    for e in exprs {
        collect_expr(e, tokens, source, scope, deprecated, is_bif);
    }
}

// ── Macro scanner ───────────────────────────────────────────────────────

/// Scan source text for `?MACRO` references and emit `Macro` tokens.
///
/// Macros are expanded by the preprocessor before parsing, so they don't
/// appear in the AST.  However the editor displays the original (pre-expansion)
/// source, where byte offsets match, so we can scan for the `?NAME` pattern
/// directly.
///
/// A simple state machine skips `?` characters inside strings (`"..."`) and
/// comments (`% ...` to end of line).
pub fn collect_macro_tokens(source: &str) -> Vec<SemanticToken> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < len {
        match bytes[i] {
            // Skip comments: % to end of line.
            b'%' => {
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            // Skip strings: "..." with \" escapes.
            b'"' => {
                i += 1;
                while i < len {
                    if bytes[i] == b'\\' {
                        i += 2; // skip escaped char
                    } else if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            // Potential macro: ?NAME
            b'?' => {
                let start = i;
                i += 1;
                // Macro name must start with [A-Z_]
                if i < len && (bytes[i].is_ascii_uppercase() || bytes[i] == b'_') {
                    let name_start = i;
                    i += 1;
                    while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                        i += 1;
                    }
                    // Only emit if the name part is non-empty (already guaranteed by the
                    // uppercase/underscore check) and looks like a real macro name.
                    if i > name_start {
                        tokens.push(SemanticToken {
                            span: Span { start, end: i },
                            token_type: TokenType::Macro,
                            modifiers: TokenModifiers::NONE,
                        });
                    }
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    tokens
}

// ── Type attribute scanner ─────────────────────────────────────────────

/// Scan source text for `-spec`, `-type`, `-opaque`, and `-callback` attributes
/// and produce semantic tokens for the identifiers within them.
///
/// These attributes are not represented in the AST (they are `Attr::Unknown`),
/// so we use text-based scanning similar to [`collect_macro_tokens`].
pub fn collect_type_tokens(source: &str) -> Vec<SemanticToken> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < len {
        // Find lines starting with `-spec`, `-type`, `-opaque`, `-callback`
        // (possibly preceded by whitespace).
        if bytes[i] == b'-' {
            i += 1;
            // Read the attribute keyword
            let kw_start = i;
            while i < len && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            let kw = &source[kw_start..i];
            match kw {
                "spec" | "callback" => {
                    // Skip whitespace after keyword
                    while i < len && bytes[i].is_ascii_whitespace() && bytes[i] != b'\n' {
                        i += 1;
                    }
                    // Find the attribute extent (up to the terminating dot)
                    let body_start = i;
                    let attr_end = find_attribute_end(bytes, i);
                    // First identifier is the function name
                    let (name_start, name_end) = scan_identifier(bytes, i);
                    if name_end > name_start {
                        tokens.push(SemanticToken {
                            span: Span { start: name_start, end: name_end },
                            token_type: TokenType::Function,
                            modifiers: TokenModifiers::NONE,
                        });
                    }
                    // Tokenize the body for type refs and variables
                    tokenize_type_body(bytes, source, body_start, attr_end, &mut tokens);
                    i = attr_end;
                }
                "type" | "opaque" => {
                    // Skip whitespace after keyword
                    while i < len && bytes[i].is_ascii_whitespace() && bytes[i] != b'\n' {
                        i += 1;
                    }
                    let body_start = i;
                    let attr_end = find_attribute_end(bytes, i);
                    // First identifier is the type name being defined
                    let (name_start, name_end) = scan_identifier(bytes, i);
                    if name_end > name_start {
                        tokens.push(SemanticToken {
                            span: Span { start: name_start, end: name_end },
                            token_type: TokenType::Type,
                            modifiers: TokenModifiers::DEFINITION,
                        });
                    }
                    // Tokenize the body for type refs and variables
                    tokenize_type_body(bytes, source, body_start, attr_end, &mut tokens);
                    i = attr_end;
                }
                _ => {
                    // Not a type attribute, skip to avoid re-scanning the dash
                    // (i is already past the keyword)
                }
            }
        } else {
            i += 1;
        }
    }

    tokens
}

/// Find the end of an attribute: scan until we find a `.` that terminates
/// the attribute (not inside strings, comments, or nested brackets).
fn find_attribute_end(bytes: &[u8], start: usize) -> usize {
    let len = bytes.len();
    let mut i = start;
    let mut depth: i32 = 0; // track () {} [] nesting

    while i < len {
        match bytes[i] {
            b'%' => {
                // Skip comment to end of line
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < len {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            b'\'' => {
                i += 1;
                while i < len {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == b'\'' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            b'(' | b'{' | b'[' => {
                depth += 1;
                i += 1;
            }
            b')' | b'}' | b']' => {
                depth -= 1;
                i += 1;
            }
            b'.' if depth <= 0 => {
                // Terminating dot (not `..` or `...`)
                return i + 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    len // end of source
}

/// Scan an identifier starting at `pos`. Returns (start, end).
fn scan_identifier(bytes: &[u8], pos: usize) -> (usize, usize) {
    let len = bytes.len();
    let mut i = pos;
    // Skip non-identifier chars
    while i < len && !bytes[i].is_ascii_alphanumeric() && bytes[i] != b'_' {
        if bytes[i] == b'\n' || bytes[i] == b'.' {
            return (pos, pos); // no identifier found
        }
        i += 1;
    }
    if i >= len || (!bytes[i].is_ascii_lowercase() && !bytes[i].is_ascii_uppercase() && bytes[i] != b'_') {
        return (pos, pos);
    }
    let start = i;
    i += 1;
    while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    (start, i)
}

/// Tokenize the body of a type attribute, emitting tokens for:
/// - Type references: lowercase identifier followed by `(`  (but not `fun(` or after `#`)
/// - Remote type references: `module:type(` — Namespace + Type
/// - Type variables: uppercase identifiers not preceded by `?`
///
/// The first identifier (function/type name) is already handled by the caller,
/// so we skip it here.
fn tokenize_type_body(
    bytes: &[u8],
    source: &str,
    start: usize,
    end: usize,
    tokens: &mut Vec<SemanticToken>,
) {
    let mut i = start;
    let mut first_ident = true; // skip the first identifier (already emitted)

    // Erlang keywords that look like type references but aren't
    let keywords: &[&str] = &["fun", "when", "end", "of", "record"];

    while i < end {
        match bytes[i] {
            // Skip comments
            b'%' => {
                while i < end && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            // Skip strings
            b'"' => {
                i += 1;
                while i < end {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            // Skip quoted atoms
            b'\'' => {
                i += 1;
                while i < end {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == b'\'' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            // Skip macros (handled by collect_macro_tokens)
            b'?' => {
                i += 1;
                while i < end && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
            }
            // Skip record references
            b'#' => {
                i += 1;
                while i < end && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
            }
            // Identifier
            b if b.is_ascii_lowercase() || b.is_ascii_uppercase() || b == b'_' => {
                let id_start = i;
                i += 1;
                while i < end && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let id = &source[id_start..i];

                if first_ident {
                    first_ident = false;
                    // Skip — already emitted by caller.
                    // But check if followed by `:` for remote type in spec
                    // (e.g., `-spec mod:func(...)` — rare, handle anyway)
                    continue;
                }

                let first_char = bytes[id_start];
                let is_upper = first_char.is_ascii_uppercase() || first_char == b'_';

                if is_upper {
                    // Type variable — but only if it's not just `_`
                    if id != "_" {
                        tokens.push(SemanticToken {
                            span: Span { start: id_start, end: i },
                            token_type: TokenType::Variable,
                            modifiers: TokenModifiers::NONE,
                        });
                    }
                } else {
                    // Lowercase identifier — check what follows
                    if keywords.contains(&id) {
                        continue;
                    }

                    // Skip whitespace to see what follows
                    let mut j = i;
                    while j < end && bytes[j] == b' ' {
                        j += 1;
                    }

                    if j < end && bytes[j] == b':' && j + 1 < end && bytes[j + 1] != b':' {
                        // Remote type: module:type(
                        // Emit module as Namespace
                        tokens.push(SemanticToken {
                            span: Span { start: id_start, end: i },
                            token_type: TokenType::Namespace,
                            modifiers: TokenModifiers::NONE,
                        });
                        // Skip the colon
                        i = j + 1;
                        // Read the type name
                        let (ts, te) = scan_next_identifier(bytes, i, end);
                        if te > ts {
                            tokens.push(SemanticToken {
                                span: Span { start: ts, end: te },
                                token_type: TokenType::Type,
                                modifiers: TokenModifiers::NONE,
                            });
                            i = te;
                        }
                    } else if j < end && bytes[j] == b'(' {
                        // Type reference: name(
                        tokens.push(SemanticToken {
                            span: Span { start: id_start, end: i },
                            token_type: TokenType::Type,
                            modifiers: TokenModifiers::NONE,
                        });
                    }
                    // Otherwise: bare atom in union etc., skip
                }
            }
            _ => {
                i += 1;
            }
        }
    }
}

/// Scan the next identifier starting from `pos`, skipping whitespace.
fn scan_next_identifier(bytes: &[u8], pos: usize, end: usize) -> (usize, usize) {
    let mut i = pos;
    while i < end && bytes[i] == b' ' {
        i += 1;
    }
    if i >= end || (!bytes[i].is_ascii_alphabetic() && bytes[i] != b'_') {
        return (pos, pos);
    }
    let start = i;
    i += 1;
    while i < end && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    (start, i)
}

// ── Type reference extraction ──────────────────────────────────────────

/// A type reference found in a `-spec`, `-type`, `-opaque`, or `-callback` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeReference {
    pub name: String,
    pub module: Option<String>,
    pub span: Span,
}

/// Scan source text for type references in `-spec`, `-type`, `-opaque`, and
/// `-callback` attributes.  Returns a list of `TypeReference` entries that the
/// caller can validate against known types.
///
/// This mirrors `collect_type_tokens` but collects structured references instead
/// of emitting semantic tokens.
pub fn collect_type_references(source: &str) -> Vec<TypeReference> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut refs = Vec::new();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'-' {
            i += 1;
            let kw_start = i;
            while i < len && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            let kw = &source[kw_start..i];
            match kw {
                "spec" | "callback" | "type" | "opaque" => {
                    // Skip whitespace after keyword
                    while i < len && bytes[i].is_ascii_whitespace() && bytes[i] != b'\n' {
                        i += 1;
                    }
                    let body_start = i;
                    let attr_end = find_attribute_end(bytes, i);
                    collect_type_refs_in_body(bytes, source, body_start, attr_end, &mut refs);
                    i = attr_end;
                }
                _ => {}
            }
        } else {
            i += 1;
        }
    }

    refs
}

/// Scan the body of a type attribute and collect type references.
///
/// Mirrors the logic of `tokenize_type_body` but produces `TypeReference`
/// entries instead of `SemanticToken`s.
fn collect_type_refs_in_body(
    bytes: &[u8],
    source: &str,
    start: usize,
    end: usize,
    refs: &mut Vec<TypeReference>,
) {
    let mut i = start;
    let mut first_ident = true;

    let keywords: &[&str] = &["fun", "when", "end", "of", "record"];

    while i < end {
        match bytes[i] {
            // Skip comments
            b'%' => {
                while i < end && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            // Skip strings
            b'"' => {
                i += 1;
                while i < end {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            // Skip quoted atoms
            b'\'' => {
                i += 1;
                while i < end {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == b'\'' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            // Skip macros
            b'?' => {
                i += 1;
                while i < end && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
            }
            // Skip record references
            b'#' => {
                i += 1;
                while i < end && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
            }
            // Identifier
            b if b.is_ascii_lowercase() || b.is_ascii_uppercase() || b == b'_' => {
                let id_start = i;
                i += 1;
                while i < end && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let id = &source[id_start..i];

                if first_ident {
                    first_ident = false;
                    continue;
                }

                let first_char = bytes[id_start];
                let is_upper = first_char.is_ascii_uppercase() || first_char == b'_';

                if is_upper {
                    // Type variable — skip
                    continue;
                }

                // Lowercase identifier
                if keywords.contains(&id) {
                    continue;
                }

                // Skip whitespace to see what follows
                let mut j = i;
                while j < end && bytes[j] == b' ' {
                    j += 1;
                }

                if j < end && bytes[j] == b':' && j + 1 < end && bytes[j + 1] != b':' {
                    // Remote type: module:type(
                    let module_name = id.to_string();
                    i = j + 1;
                    let (ts, te) = scan_next_identifier(bytes, i, end);
                    if te > ts {
                        let type_name = source[ts..te].to_string();
                        refs.push(TypeReference {
                            name: type_name,
                            module: Some(module_name),
                            span: Span { start: ts, end: te },
                        });
                        i = te;
                    }
                } else if j < end && bytes[j] == b'(' {
                    // Local type reference: name(
                    refs.push(TypeReference {
                        name: id.to_string(),
                        module: None,
                        span: Span { start: id_start, end: i },
                    });
                }
            }
            _ => {
                i += 1;
            }
        }
    }
}

// ── Lexer-based fallback tokenizer ─────────────────────────────────────

/// Check whether the byte offset is at the very start of a line.
fn is_at_line_start(source: &str, offset: usize) -> bool {
    offset == 0 || source.as_bytes().get(offset.wrapping_sub(1)) == Some(&b'\n')
}

/// Collect semantic tokens using only the lexer (no parsing required).
///
/// This is the fallback path used when the file has syntax errors and the
/// parser cannot produce an AST.  It walks the lexer token stream and
/// detects common patterns:
///
/// - Variables (with `_`-prefix → `UNUSED` modifier)
/// - Function definitions (`atom(` at column 0)
/// - Local calls (`atom(` elsewhere)
/// - Remote calls (`atom:atom` with optional `(`)
/// - Record names (`#atom`)
/// - Fun references (`fun atom/N`, `fun atom:atom/N`)
///
/// Module attributes (`-module(...)`, `-export(...)`, etc.) are skipped
/// because the existing [`collect_type_tokens`] already handles `-spec`,
/// `-type`, `-opaque`, and `-callback` attributes via text-based scanning.
pub fn collect_fallback_tokens(source: &str) -> Vec<SemanticToken> {
    // Lex all tokens, skipping errors.
    let toks: Vec<(Token, Span)> = Lexer::new(source)
        .filter_map(|r| r.ok())
        .collect();

    let len = toks.len();
    let mut tokens = Vec::new();

    // State: are we inside a module attribute?  Attributes start with `-`
    // at column 0 and end at the next `.` at nesting depth 0.
    let mut in_attribute = false;
    let mut attr_depth: i32 = 0;

    let mut i = 0;
    while i < len {
        let (ref tok, span) = toks[i];

        // ── Attribute detection ──────────────────────────────────────
        if !in_attribute && *tok == Token::Minus && is_at_line_start(source, span.start) {
            // Check if followed by an atom or keyword (attribute name).
            if i + 1 < len && is_attribute_keyword(&toks[i + 1].0) {
                in_attribute = true;
                attr_depth = 0;
                i += 1; // skip the keyword too
                i += 1;
                continue;
            }
        }

        if in_attribute {
            match tok {
                Token::LParen | Token::LBrace | Token::LBracket => attr_depth += 1,
                Token::RParen | Token::RBrace | Token::RBracket => attr_depth -= 1,
                Token::Dot if attr_depth <= 0 => {
                    in_attribute = false;
                }
                _ => {}
            }
            i += 1;
            continue;
        }

        // ── Regular code patterns ────────────────────────────────────
        match tok {
            Token::Variable => {
                let mods = if is_underscore_prefixed(span, source) {
                    TokenModifiers::UNUSED
                } else {
                    TokenModifiers::NONE
                };
                tokens.push(SemanticToken {
                    span,
                    token_type: TokenType::Variable,
                    modifiers: mods,
                });
                i += 1;
            }

            Token::Atom | Token::QuotedAtom => {
                // Look ahead to determine the pattern.
                if i + 1 < len && toks[i + 1].0 == Token::Colon {
                    if i + 2 < len && matches!(toks[i + 2].0, Token::Atom | Token::QuotedAtom) {
                        // Remote call: atom:atom  (possibly followed by `(`)
                        let mod_span = span;
                        let func_span = toks[i + 2].1;
                        tokens.push(SemanticToken {
                            span: mod_span,
                            token_type: TokenType::Namespace,
                            modifiers: TokenModifiers::NONE,
                        });
                        tokens.push(SemanticToken {
                            span: func_span,
                            token_type: TokenType::Function,
                            modifiers: TokenModifiers::NONE,
                        });
                        i += 3; // skip atom:atom
                        continue;
                    } else {
                        // Incomplete remote call (e.g. `greeter:` while typing)
                        // — emit the module part as Namespace anyway.
                        tokens.push(SemanticToken {
                            span,
                            token_type: TokenType::Namespace,
                            modifiers: TokenModifiers::NONE,
                        });
                        i += 2; // skip atom and colon
                        continue;
                    }
                } else if i + 1 < len && toks[i + 1].0 == Token::LParen {
                    // atom( — function definition or local call
                    if is_at_line_start(source, span.start) {
                        tokens.push(SemanticToken {
                            span,
                            token_type: TokenType::Function,
                            modifiers: TokenModifiers::DECLARATION
                                .union(TokenModifiers::DEFINITION),
                        });
                    } else {
                        tokens.push(SemanticToken {
                            span,
                            token_type: TokenType::Function,
                            modifiers: TokenModifiers::NONE,
                        });
                    }
                    i += 1;
                } else {
                    // Bare atom — no semantic token (TextMate handles it)
                    i += 1;
                }
            }

            Token::Hash => {
                // #atom — record name
                if i + 1 < len {
                    match toks[i + 1].0 {
                        Token::Atom | Token::QuotedAtom => {
                            tokens.push(SemanticToken {
                                span: toks[i + 1].1,
                                token_type: TokenType::Property,
                                modifiers: TokenModifiers::NONE,
                            });
                            i += 2;
                            continue;
                        }
                        _ => {}
                    }
                }
                i += 1;
            }

            Token::KwFun => {
                // fun Name/Arity  or  fun Module:Name/Arity
                if i + 2 < len
                    && matches!(toks[i + 1].0, Token::Atom | Token::QuotedAtom)
                    && toks[i + 2].0 == Token::Slash
                {
                    // fun Name/Arity
                    tokens.push(SemanticToken {
                        span: toks[i + 1].1,
                        token_type: TokenType::Function,
                        modifiers: TokenModifiers::NONE,
                    });
                    i += 3; // skip fun, atom, slash
                    if i < len && matches!(toks[i].0, Token::Integer(_)) {
                        i += 1; // skip arity
                    }
                    continue;
                }
                if i + 4 < len
                    && matches!(toks[i + 1].0, Token::Atom | Token::QuotedAtom)
                    && toks[i + 2].0 == Token::Colon
                    && matches!(toks[i + 3].0, Token::Atom | Token::QuotedAtom)
                    && toks[i + 4].0 == Token::Slash
                {
                    // fun Module:Name/Arity
                    tokens.push(SemanticToken {
                        span: toks[i + 1].1,
                        token_type: TokenType::Namespace,
                        modifiers: TokenModifiers::NONE,
                    });
                    tokens.push(SemanticToken {
                        span: toks[i + 3].1,
                        token_type: TokenType::Function,
                        modifiers: TokenModifiers::NONE,
                    });
                    i += 5; // skip fun, mod, colon, name, slash
                    if i < len && matches!(toks[i].0, Token::Integer(_)) {
                        i += 1; // skip arity
                    }
                    continue;
                }
                i += 1;
            }

            _ => {
                i += 1;
            }
        }
    }

    tokens.sort_by_key(|t| t.span.start);
    tokens
}

/// Returns true if the token looks like a module attribute keyword.
fn is_attribute_keyword(tok: &Token) -> bool {
    matches!(
        tok,
        Token::Atom
            | Token::KwModule
            | Token::KwExport
            | Token::KwImport
            | Token::KwIf
    )
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn parse_and_collect(source: &str) -> Vec<SemanticToken> {
        let mut parser = Parser::new(source);
        let result = parser.parse_module();
        assert!(result.errors.is_empty(), "parse errors: {:#?}", result.errors);
        collect_semantic_tokens(&result.module, source)
    }

    #[test]
    fn function_definition_and_variable() {
        let src = "-module(test).\nfoo(X) -> X.\n";
        let tokens = parse_and_collect(src);

        // Expect: function name `foo` (Function+Declaration+Definition),
        //         variable `X` in pattern (Variable+Definition),
        //         variable `X` in body (Variable).
        assert!(tokens.len() >= 3, "expected at least 3 tokens, got {}", tokens.len());

        let func_tok = tokens.iter().find(|t| t.token_type == TokenType::Function).unwrap();
        assert_eq!(&src[func_tok.span.start..func_tok.span.end], "foo");

        let var_defs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable && t.modifiers == TokenModifiers::DEFINITION)
            .collect();
        assert!(!var_defs.is_empty());
        assert_eq!(&src[var_defs[0].span.start..var_defs[0].span.end], "X");
    }

    #[test]
    fn remote_call_module_and_function() {
        let src = "-module(test).\nstart() -> io:format(\"hello\").\n";
        let tokens = parse_and_collect(src);

        let ns_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Namespace)
            .collect();
        assert!(!ns_tokens.is_empty(), "expected namespace token for module name");
        assert_eq!(&src[ns_tokens[0].span.start..ns_tokens[0].span.end], "io");

        let fn_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function)
            .collect();
        // Should have `start` (definition) and `format` (call).
        assert!(fn_tokens.len() >= 2);
    }

    #[test]
    fn local_call() {
        let src = "-module(test).\nbar() -> ok.\nfoo() -> bar().\n";
        let tokens = parse_and_collect(src);

        let fn_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function)
            .collect();
        // bar (def), foo (def), bar (call)
        assert!(fn_tokens.len() >= 3, "expected at least 3 function tokens, got {}", fn_tokens.len());
    }

    #[test]
    fn case_expression_bindings() {
        let src = "-module(test).\nfoo(X) ->\n    case X of\n        {ok, Val} -> Val;\n        error -> error\n    end.\n";
        let tokens = parse_and_collect(src);

        // Variables in case patterns get `Definition` modifier.
        let var_defs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable && t.modifiers == TokenModifiers::DEFINITION)
            .collect();
        let var_def_names: Vec<&str> = var_defs.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(var_def_names.contains(&"X"), "X should be a variable definition");
        assert!(var_def_names.contains(&"Val"), "Val should be a variable definition in case pattern");

        // Variable uses (X in `case X`, Val in body) get no modifier.
        let var_uses: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable && t.modifiers == TokenModifiers::NONE)
            .collect();
        assert!(var_uses.len() >= 2, "expected at least 2 variable uses");
    }

    #[test]
    fn match_and_comprehension() {
        let src = "-module(test).\nfoo(List) ->\n    Result = [X * 2 || X <- List],\n    Result.\n";
        let tokens = parse_and_collect(src);

        // `Result` in match LHS and `X` in generator pattern are definitions.
        let var_defs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable && t.modifiers == TokenModifiers::DEFINITION)
            .collect();
        let var_def_names: Vec<&str> = var_defs.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(var_def_names.contains(&"List"), "List should be a pattern definition");
        assert!(var_def_names.contains(&"Result"), "Result should be a pattern definition (match LHS)");
        assert!(var_def_names.contains(&"X"), "X should be a pattern definition (generator)");
    }

    #[test]
    fn try_catch_bindings() {
        let src = "-module(test).\nfoo() ->\n    try bar()\n    catch\n        Class:Reason:Stack -> {Class, Reason, Stack}\n    end.\nbar() -> ok.\n";
        let tokens = parse_and_collect(src);

        // Catch class variable, pattern variable, and stacktrace are all definitions.
        let var_defs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable && t.modifiers == TokenModifiers::DEFINITION)
            .collect();
        let var_def_names: Vec<&str> = var_defs.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(var_def_names.contains(&"Class"), "Class should be a catch class definition");
        assert!(var_def_names.contains(&"Reason"), "Reason should be a catch pattern definition");
        assert!(var_def_names.contains(&"Stack"), "Stack should be a stacktrace definition");

        // The body references them as plain variable uses.
        let var_uses: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable && t.modifiers == TokenModifiers::NONE)
            .collect();
        let var_use_names: Vec<&str> = var_uses.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(var_use_names.contains(&"Class"));
        assert!(var_use_names.contains(&"Reason"));
        assert!(var_use_names.contains(&"Stack"));
    }

    #[test]
    fn anonymous_fun() {
        let src = "-module(test).\nfoo() ->\n    F = fun(A, B) -> A + B end,\n    F(1, 2).\n";
        let tokens = parse_and_collect(src);

        // A and B in fun pattern are variable definitions.
        let var_defs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable && t.modifiers == TokenModifiers::DEFINITION)
            .collect();
        let var_def_names: Vec<&str> = var_defs.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(var_def_names.contains(&"A"));
        assert!(var_def_names.contains(&"B"));
        assert!(var_def_names.contains(&"F"), "F should be a definition in match pattern");

        // F(1, 2) — F is a variable call, should appear as Variable use.
        let var_uses: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable && t.modifiers == TokenModifiers::NONE)
            .collect();
        let var_use_names: Vec<&str> = var_uses.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(var_use_names.contains(&"F"), "F in F(1,2) should be a variable use");
    }

    #[test]
    fn exported_function_modifier() {
        let src = "-module(test).\n-export([foo/1]).\nfoo(X) -> X.\nbar() -> ok.\n";
        let tokens = parse_and_collect(src);

        let func_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function)
            .collect();

        // `foo` is exported — should have DECLARATION | DEFINITION | EXPORTED.
        let foo_tok = func_tokens.iter()
            .find(|t| &src[t.span.start..t.span.end] == "foo")
            .expect("expected token for foo");
        let expected_exported = TokenModifiers::DECLARATION
            .union(TokenModifiers::DEFINITION)
            .union(TokenModifiers::EXPORTED);
        assert_eq!(
            foo_tok.modifiers, expected_exported,
            "foo should have DECLARATION | DEFINITION | EXPORTED"
        );

        // `bar` is local — should have DECLARATION | DEFINITION only.
        let bar_tok = func_tokens.iter()
            .find(|t| &src[t.span.start..t.span.end] == "bar")
            .expect("expected token for bar");
        let expected_local = TokenModifiers::DECLARATION.union(TokenModifiers::DEFINITION);
        assert_eq!(
            bar_tok.modifiers, expected_local,
            "bar should have DECLARATION | DEFINITION only"
        );
    }

    #[test]
    fn underscore_prefixed_variables_marked_unused() {
        let src = "-module(test).\nfoo(_App, _Key) -> ok.\nbar(X) -> X.\n";
        let tokens = parse_and_collect(src);

        // _App and _Key in patterns should have DEFINITION | UNUSED.
        let unused_defs: Vec<_> = tokens.iter()
            .filter(|t| {
                t.token_type == TokenType::Variable
                    && t.modifiers == TokenModifiers::DEFINITION.union(TokenModifiers::UNUSED)
            })
            .collect();
        let names: Vec<&str> = unused_defs.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(names.contains(&"_App"), "_App should be marked unused");
        assert!(names.contains(&"_Key"), "_Key should be marked unused");

        // X should have plain DEFINITION (no UNUSED).
        let normal_defs: Vec<_> = tokens.iter()
            .filter(|t| {
                t.token_type == TokenType::Variable
                    && t.modifiers == TokenModifiers::DEFINITION
            })
            .collect();
        let normal_names: Vec<&str> = normal_defs.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(normal_names.contains(&"X"), "X should be a normal definition");
    }

    // ── Record field (property) tests ─────────────────────────────────────

    #[test]
    fn record_field_definition_property() {
        let src = "-module(test).\n-record(person, {name, age = 0}).\nfoo() -> ok.\n";
        let tokens = parse_and_collect(src);

        let prop_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Property)
            .collect();
        let names: Vec<&str> = prop_tokens.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert_eq!(names, vec!["name", "age"]);

        // Field definitions get the DEFINITION modifier.
        for t in &prop_tokens {
            assert_eq!(t.modifiers, TokenModifiers::DEFINITION);
        }
    }

    #[test]
    fn record_create_and_access_property() {
        let src = "-module(test).\n-record(person, {name, age}).\nfoo() ->\n    R = #person{name = \"Alice\", age = 30},\n    R#person.name.\n";
        let tokens = parse_and_collect(src);

        let prop_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Property)
            .collect();
        let names: Vec<&str> = prop_tokens.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        // record def fields + create fields + access field
        assert_eq!(names, vec!["name", "age", "name", "age", "name"]);
    }

    #[test]
    fn record_pattern_property() {
        let src = "-module(test).\n-record(person, {name, age}).\nfoo(#person{name = N}) -> N.\n";
        let tokens = parse_and_collect(src);

        let prop_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Property)
            .collect();
        let names: Vec<&str> = prop_tokens.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        // record def: name, age; pattern: name
        assert_eq!(names, vec!["name", "age", "name"]);

        // Pattern field use should have NONE modifiers (not DEFINITION).
        let pattern_prop = prop_tokens.last().unwrap();
        assert_eq!(pattern_prop.modifiers, TokenModifiers::NONE);
    }

    // ── Macro scanner tests ─────────────────────────────────────────────

    #[test]
    fn macro_module_and_custom() {
        let src = "-module(?MODULE).\n-define(MY_MACRO, 42).\nfoo() -> ?MY_MACRO.\n";
        let tokens = collect_macro_tokens(src);

        let names: Vec<&str> = tokens.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert_eq!(names, vec!["?MODULE", "?MY_MACRO"]);

        // All should be Macro type with no modifiers.
        for t in &tokens {
            assert_eq!(t.token_type, TokenType::Macro);
            assert_eq!(t.modifiers, TokenModifiers::NONE);
        }
    }

    #[test]
    fn macro_skips_strings_and_comments() {
        let src = "foo() ->\n    % ?COMMENTED_OUT\n    S = \"hello ?NOT_A_MACRO world\",\n    ?REAL_MACRO.\n";
        let tokens = collect_macro_tokens(src);

        let names: Vec<&str> = tokens.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert_eq!(names, vec!["?REAL_MACRO"]);
    }

    #[test]
    fn macro_skips_question_mark_not_followed_by_uppercase() {
        // `?` in pattern matching or `?=` should not produce tokens.
        let src = "foo(X) when X =:= 42 -> ok.\n";
        let tokens = collect_macro_tokens(src);
        assert!(tokens.is_empty());
    }

    #[test]
    fn macro_with_underscore_prefix() {
        let src = "foo() -> ?_INTERNAL.\n";
        let tokens = collect_macro_tokens(src);
        assert_eq!(tokens.len(), 1);
        assert_eq!(&src[tokens[0].span.start..tokens[0].span.end], "?_INTERNAL");
    }

    // ── Scope-aware binding tests ─────────────────────────────────────────

    #[test]
    fn bound_variable_in_case_pattern() {
        // X is bound as a function parameter. In the case pattern, X is
        // an equality check (already bound), not a fresh binding.
        let src = "-module(test).\nfoo(X) ->\n    case bar() of\n        X -> matched\n    end.\nbar() -> ok.\n";
        let tokens = parse_and_collect(src);

        let var_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable)
            .collect();
        let var_info: Vec<(&str, TokenModifiers)> = var_tokens.iter()
            .map(|t| (&src[t.span.start..t.span.end], t.modifiers))
            .collect();

        // First X (function param) — DEFINITION.
        assert_eq!(var_info[0], ("X", TokenModifiers::DEFINITION));
        // Second X (case pattern, already bound) — NONE (use, not definition).
        assert_eq!(var_info[1], ("X", TokenModifiers::NONE));
    }

    #[test]
    fn match_extends_scope() {
        // X bound by match expression, then used in case pattern = equality check.
        let src = "-module(test).\nfoo() ->\n    X = 42,\n    case bar() of\n        X -> matched\n    end.\nbar() -> ok.\n";
        let tokens = parse_and_collect(src);

        let var_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable)
            .collect();
        let var_info: Vec<(&str, TokenModifiers)> = var_tokens.iter()
            .map(|t| (&src[t.span.start..t.span.end], t.modifiers))
            .collect();

        // X in match LHS — DEFINITION (fresh binding).
        assert_eq!(var_info[0], ("X", TokenModifiers::DEFINITION));
        // X in case pattern — NONE (already bound by match).
        assert_eq!(var_info[1], ("X", TokenModifiers::NONE));
    }

    #[test]
    fn fun_patterns_always_fresh() {
        // Inner fun shadows the outer X — the fun's X is always fresh.
        let src = "-module(test).\nfoo(X) ->\n    F = fun(X) -> X end,\n    F(X).\n";
        let tokens = parse_and_collect(src);

        let var_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable)
            .collect();
        let var_info: Vec<(&str, TokenModifiers)> = var_tokens.iter()
            .map(|t| (&src[t.span.start..t.span.end], t.modifiers))
            .collect();

        // Outer X (function param) — DEFINITION.
        assert!(var_info.iter().any(|(n, m)| *n == "X" && *m == TokenModifiers::DEFINITION),
            "outer X should be DEFINITION");
        // F in match LHS — DEFINITION.
        assert!(var_info.iter().any(|(n, m)| *n == "F" && *m == TokenModifiers::DEFINITION),
            "F should be DEFINITION");

        // Inner fun's X parameter — DEFINITION (fresh, not an equality check).
        // Count how many X variables have DEFINITION.
        let x_defs: Vec<_> = var_info.iter()
            .filter(|(n, m)| *n == "X" && *m == TokenModifiers::DEFINITION)
            .collect();
        assert_eq!(x_defs.len(), 2, "both outer X and inner fun's X should be DEFINITION");
    }

    #[test]
    fn duplicate_var_in_pattern() {
        // {X, X} — first X is fresh, second X is equality check.
        let src = "-module(test).\nfoo({X, X}) -> X.\n";
        let tokens = parse_and_collect(src);

        let var_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable)
            .collect();
        let var_info: Vec<(&str, TokenModifiers)> = var_tokens.iter()
            .map(|t| (&src[t.span.start..t.span.end], t.modifiers))
            .collect();

        // First X — DEFINITION.
        assert_eq!(var_info[0], ("X", TokenModifiers::DEFINITION));
        // Second X — NONE (duplicate in same pattern = equality check).
        assert_eq!(var_info[1], ("X", TokenModifiers::NONE));
        // Third X in body — NONE (use).
        assert_eq!(var_info[2], ("X", TokenModifiers::NONE));
    }

    #[test]
    fn comprehension_scope() {
        // Generator vars are DEFINITION; template and filter uses are NONE.
        let src = "-module(test).\nfoo(List) ->\n    [{X, Y} || {X, Y} <- List, X > 0].\n";
        let tokens = parse_and_collect(src);

        let var_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable)
            .collect();
        let var_info: Vec<(&str, TokenModifiers)> = var_tokens.iter()
            .map(|t| (&src[t.span.start..t.span.end], t.modifiers))
            .collect();

        // List in function param — DEFINITION.
        assert!(var_info.iter().any(|(n, m)| *n == "List" && *m == TokenModifiers::DEFINITION),
            "List should be DEFINITION");

        // X and Y in generator pattern — DEFINITION (fresh in LC scope).
        // X and Y in template — NONE (uses).
        // X in filter — NONE (use).
        // List in generator source — NONE (use).
        let x_defs = var_info.iter().filter(|(n, m)| *n == "X" && *m == TokenModifiers::DEFINITION).count();
        let x_uses = var_info.iter().filter(|(n, m)| *n == "X" && *m == TokenModifiers::NONE).count();
        assert_eq!(x_defs, 1, "X should have exactly 1 DEFINITION (generator pattern)");
        assert_eq!(x_uses, 2, "X should have 2 uses (template + filter)");

        let y_defs = var_info.iter().filter(|(n, m)| *n == "Y" && *m == TokenModifiers::DEFINITION).count();
        let y_uses = var_info.iter().filter(|(n, m)| *n == "Y" && *m == TokenModifiers::NONE).count();
        assert_eq!(y_defs, 1, "Y should have exactly 1 DEFINITION (generator pattern)");
        assert_eq!(y_uses, 1, "Y should have 1 use (template)");
    }

    // ── BIF (defaultLibrary) modifier tests ──────────────────────────────

    fn parse_and_collect_with_bif(source: &str) -> Vec<SemanticToken> {
        let mut parser = Parser::new(source);
        let result = parser.parse_module();
        assert!(result.errors.is_empty(), "parse errors: {:#?}", result.errors);
        let module = result.module;
        // Simulate a BIF checker that recognizes a few common auto-imported BIFs.
        let fake_is_bif = |name: &str, arity: u32| -> bool {
            matches!(
                (name, arity),
                ("register", 2) | ("self", 0) | ("length", 1) | ("is_integer", 1) | ("hd", 1)
            )
        };
        collect_semantic_tokens_with_bif_check(&module, source, &fake_is_bif)
    }

    #[test]
    fn bif_call_gets_default_library_modifier() {
        let src = "-module(test).\nfoo(Name) -> register(Name, self()).\n";
        let tokens = parse_and_collect_with_bif(src);

        let fn_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function)
            .collect();

        // `register` call should have DEFAULT_LIBRARY modifier.
        let register_tok = fn_tokens.iter()
            .find(|t| &src[t.span.start..t.span.end] == "register")
            .expect("expected token for register");
        assert_eq!(
            register_tok.modifiers, TokenModifiers::DEFAULT_LIBRARY,
            "register should have DEFAULT_LIBRARY modifier"
        );

        // `self` call should have DEFAULT_LIBRARY modifier.
        let self_tok = fn_tokens.iter()
            .find(|t| &src[t.span.start..t.span.end] == "self")
            .expect("expected token for self");
        assert_eq!(
            self_tok.modifiers, TokenModifiers::DEFAULT_LIBRARY,
            "self should have DEFAULT_LIBRARY modifier"
        );

        // `foo` definition should NOT have DEFAULT_LIBRARY.
        let foo_tok = fn_tokens.iter()
            .find(|t| &src[t.span.start..t.span.end] == "foo")
            .expect("expected token for foo");
        assert!(
            foo_tok.modifiers.0 & TokenModifiers::DEFAULT_LIBRARY.0 == 0,
            "foo definition should not have DEFAULT_LIBRARY"
        );
    }

    #[test]
    fn local_call_without_bif_no_default_library() {
        let src = "-module(test).\nbar() -> ok.\nfoo() -> bar().\n";
        let tokens = parse_and_collect_with_bif(src);

        let fn_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function)
            .collect();

        // `bar` call (not a BIF) should NOT have DEFAULT_LIBRARY.
        let bar_calls: Vec<_> = fn_tokens.iter()
            .filter(|t| &src[t.span.start..t.span.end] == "bar")
            .collect();
        for bar_tok in &bar_calls {
            assert!(
                bar_tok.modifiers.0 & TokenModifiers::DEFAULT_LIBRARY.0 == 0,
                "bar should not have DEFAULT_LIBRARY"
            );
        }
    }

    #[test]
    fn bif_in_guard_gets_default_library() {
        let src = "-module(test).\nfoo(X) when is_integer(X) -> X.\n";
        let tokens = parse_and_collect_with_bif(src);

        let fn_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function)
            .collect();

        let is_integer_tok = fn_tokens.iter()
            .find(|t| &src[t.span.start..t.span.end] == "is_integer")
            .expect("expected token for is_integer");
        assert_eq!(
            is_integer_tok.modifiers, TokenModifiers::DEFAULT_LIBRARY,
            "is_integer in guard should have DEFAULT_LIBRARY modifier"
        );
    }

    #[test]
    fn without_bif_check_no_default_library() {
        // The basic collect_semantic_tokens (no BIF check) should never produce DEFAULT_LIBRARY.
        let src = "-module(test).\nfoo(Name) -> register(Name, self()).\n";
        let tokens = parse_and_collect(src);

        for tok in &tokens {
            assert!(
                tok.modifiers.0 & TokenModifiers::DEFAULT_LIBRARY.0 == 0,
                "collect_semantic_tokens without BIF check should never set DEFAULT_LIBRARY"
            );
        }
    }

    #[test]
    fn multi_clause_function_names_all_highlighted() {
        let src = "-module(test).\nfoo(1) -> a;\nfoo(2) -> b;\nfoo(_) -> c.\n";
        let tokens = parse_and_collect(src);

        let decl_def = TokenModifiers::DECLARATION.union(TokenModifiers::DEFINITION);
        let func_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function
                && t.modifiers.0 & decl_def.0 == decl_def.0)
            .collect();

        // Should have 3 function name tokens, one per clause
        assert_eq!(func_tokens.len(), 3,
            "expected 3 function name tokens for 3 clauses, got {}",
            func_tokens.len());

        for tok in &func_tokens {
            assert_eq!(&src[tok.span.start..tok.span.end], "foo");
        }

        // Each should have a distinct span
        let spans: std::collections::HashSet<_> = func_tokens.iter()
            .map(|t| (t.span.start, t.span.end))
            .collect();
        assert_eq!(spans.len(), 3, "each clause name should have a distinct span");
    }

    // ── Type attribute scanner tests ─────────────────────────────────────

    #[test]
    fn type_spec_with_builtin_types() {
        let src = "-module(test).\n-spec foo(integer(), list()) -> atom().\nfoo(X, Y) -> ok.\n";
        let tokens = collect_type_tokens(src);

        // Function name: foo
        let func_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function)
            .collect();
        assert_eq!(func_tokens.len(), 1);
        assert_eq!(&src[func_tokens[0].span.start..func_tokens[0].span.end], "foo");

        // Type references: integer, list, atom
        let type_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Type)
            .collect();
        let type_names: Vec<&str> = type_tokens.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(type_names.contains(&"integer"), "should have integer type ref");
        assert!(type_names.contains(&"list"), "should have list type ref");
        assert!(type_names.contains(&"atom"), "should have atom type ref");
    }

    #[test]
    fn type_spec_with_type_variables() {
        let src = "-module(test).\n-spec foo(T, State) -> {ok, T} when T :: term(), State :: map().\nfoo(X, S) -> {ok, X}.\n";
        let tokens = collect_type_tokens(src);

        // Type variables: T, State (may appear multiple times)
        let var_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable)
            .collect();
        let var_names: Vec<&str> = var_tokens.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(var_names.contains(&"T"), "should have type variable T");
        assert!(var_names.contains(&"State"), "should have type variable State");
    }

    #[test]
    fn type_definition() {
        let src = "-module(test).\n-type my_type() :: integer() | atom().\n";
        let tokens = collect_type_tokens(src);

        // Type name being defined: my_type with DEFINITION modifier
        let type_defs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Type && t.modifiers == TokenModifiers::DEFINITION)
            .collect();
        assert_eq!(type_defs.len(), 1);
        assert_eq!(&src[type_defs[0].span.start..type_defs[0].span.end], "my_type");

        // Type references: integer, atom
        let type_refs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Type && t.modifiers == TokenModifiers::NONE)
            .collect();
        let ref_names: Vec<&str> = type_refs.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(ref_names.contains(&"integer"));
        assert!(ref_names.contains(&"atom"));
    }

    #[test]
    fn opaque_definition() {
        let src = "-module(test).\n-opaque my_opaque() :: {secret, integer()}.\n";
        let tokens = collect_type_tokens(src);

        let type_defs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Type && t.modifiers == TokenModifiers::DEFINITION)
            .collect();
        assert_eq!(type_defs.len(), 1);
        assert_eq!(&src[type_defs[0].span.start..type_defs[0].span.end], "my_opaque");
    }

    #[test]
    fn callback_definition() {
        let src = "-module(test).\n-callback init(Args :: term()) -> {ok, State :: term()}.\n";
        let tokens = collect_type_tokens(src);

        // Function name: init
        let func_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function)
            .collect();
        assert_eq!(func_tokens.len(), 1);
        assert_eq!(&src[func_tokens[0].span.start..func_tokens[0].span.end], "init");

        // Type variables: Args, State
        let var_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable)
            .collect();
        let var_names: Vec<&str> = var_tokens.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(var_names.contains(&"Args"), "should have type variable Args");
        assert!(var_names.contains(&"State"), "should have type variable State");

        // Type refs: term (appears twice)
        let type_refs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Type && t.modifiers == TokenModifiers::NONE)
            .collect();
        let ref_names: Vec<&str> = type_refs.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(ref_names.contains(&"term"));
    }

    #[test]
    fn type_remote_type_reference() {
        let src = "-module(test).\n-type my_type() :: sets:set(integer()).\n";
        let tokens = collect_type_tokens(src);

        // Namespace: sets
        let ns_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Namespace)
            .collect();
        assert_eq!(ns_tokens.len(), 1);
        assert_eq!(&src[ns_tokens[0].span.start..ns_tokens[0].span.end], "sets");

        // Type ref for remote type: set
        let type_refs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Type && t.modifiers == TokenModifiers::NONE)
            .collect();
        let ref_names: Vec<&str> = type_refs.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(ref_names.contains(&"set"), "should have remote type ref 'set'");
        assert!(ref_names.contains(&"integer"), "should have type ref 'integer'");
    }

    #[test]
    fn type_when_constraints() {
        let src = "-module(test).\n-spec merge(A, B) -> C when A :: list(), B :: list(), C :: list().\nmerge(A, B) -> A ++ B.\n";
        let tokens = collect_type_tokens(src);

        // Type variables: A, B, C
        let var_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable)
            .collect();
        let var_names: Vec<&str> = var_tokens.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(var_names.contains(&"A"), "should have variable A");
        assert!(var_names.contains(&"B"), "should have variable B");
        assert!(var_names.contains(&"C"), "should have variable C");
    }

    #[test]
    fn type_skips_macros_and_records() {
        let src = "-module(test).\n-type my_type() :: ?MODULE | #my_record{}.\n";
        let tokens = collect_type_tokens(src);

        // Should not have Macro or any token for ?MODULE (handled by macro scanner)
        // Should not have tokens for #my_record
        let all_names: Vec<&str> = tokens.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(!all_names.iter().any(|n| n.contains('?')), "should not emit macro tokens");
        assert!(!all_names.iter().any(|n| n.contains('#')), "should not emit record tokens");
    }

    #[test]
    fn type_parametric() {
        let src = "-module(test).\n-type option(T) :: {some, T} | none.\n";
        let tokens = collect_type_tokens(src);

        // Type name: option with DEFINITION
        let type_defs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Type && t.modifiers == TokenModifiers::DEFINITION)
            .collect();
        assert_eq!(type_defs.len(), 1);
        assert_eq!(&src[type_defs[0].span.start..type_defs[0].span.end], "option");

        // Variable: T
        let var_tokens: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable)
            .collect();
        let var_names: Vec<&str> = var_tokens.iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert!(var_names.contains(&"T"), "should have type variable T");
    }

    // ── collect_type_references tests ─────────────────────────────────────

    #[test]
    fn type_refs_from_spec() {
        let src = "-module(test).\n-spec foo(integer(), my_type()) -> atom().\nfoo(X, Y) -> ok.\n";
        let refs = collect_type_references(src);

        let names: Vec<&str> = refs.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"integer"), "should find integer ref");
        assert!(names.contains(&"my_type"), "should find my_type ref");
        assert!(names.contains(&"atom"), "should find atom ref");
        // All should be local (no module)
        for r in &refs {
            assert!(r.module.is_none(), "spec refs should be local");
        }
    }

    #[test]
    fn type_refs_remote() {
        let src = "-module(test).\n-type my_type() :: sets:set(integer()).\n";
        let refs = collect_type_references(src);

        let remote: Vec<_> = refs.iter().filter(|r| r.module.is_some()).collect();
        assert_eq!(remote.len(), 1);
        assert_eq!(remote[0].name, "set");
        assert_eq!(remote[0].module.as_deref(), Some("sets"));

        let local: Vec<_> = refs.iter().filter(|r| r.module.is_none()).collect();
        let local_names: Vec<&str> = local.iter().map(|r| r.name.as_str()).collect();
        assert!(local_names.contains(&"integer"));
    }

    #[test]
    fn type_refs_skips_variables_keywords_macros_records() {
        let src = "-module(test).\n-spec foo(T, ?MACRO, #rec{}) -> ok when T :: term().\nfoo(X) -> ok.\n";
        let refs = collect_type_references(src);

        let names: Vec<&str> = refs.iter().map(|r| r.name.as_str()).collect();
        // Should find `term` but not T, MACRO, rec, ok (bare atom without parens)
        assert!(names.contains(&"term"), "should find term ref");
        assert!(!names.iter().any(|n| n.starts_with('?')), "should skip macros");
        assert!(!names.iter().any(|n| n.starts_with('#')), "should skip records");
        // Variables (uppercase) should not appear
        assert!(!names.contains(&"T"), "should skip type variables");
    }

    #[test]
    fn type_refs_from_type_def() {
        let src = "-module(test).\n-type my_type() :: integer() | list(atom()).\n";
        let refs = collect_type_references(src);

        let names: Vec<&str> = refs.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"integer"));
        assert!(names.contains(&"list"));
        assert!(names.contains(&"atom"));
    }

    #[test]
    fn type_refs_span_is_correct() {
        let src = "-module(test).\n-spec foo(integer()) -> ok.\nfoo(X) -> ok.\n";
        let refs = collect_type_references(src);

        let int_ref = refs.iter().find(|r| r.name == "integer").unwrap();
        assert_eq!(&src[int_ref.span.start..int_ref.span.end], "integer");
    }

    #[test]
    fn type_refs_from_callback() {
        let src = "-module(test).\n-callback init(Args :: term()) -> {ok, state()}.\n";
        let refs = collect_type_references(src);

        let names: Vec<&str> = refs.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"term"), "should find term ref in callback");
        assert!(names.contains(&"state"), "should find state ref in callback");
    }

    #[test]
    fn type_refs_from_opaque() {
        let src = "-module(test).\n-opaque my_opaque() :: {secret, integer(), custom_type()}.\n";
        let refs = collect_type_references(src);

        let names: Vec<&str> = refs.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"integer"), "should find integer ref in opaque");
        assert!(names.contains(&"custom_type"), "should find custom_type ref in opaque");
    }

    #[test]
    fn type_refs_multiple_attributes() {
        let src = "-module(test).\n-type a() :: integer().\n-spec foo(a(), binary()) -> atom().\nfoo(X, Y) -> ok.\n";
        let refs = collect_type_references(src);

        let names: Vec<&str> = refs.iter().map(|r| r.name.as_str()).collect();
        // From -type: integer
        assert!(names.contains(&"integer"));
        // From -spec: a, binary, atom
        assert!(names.contains(&"a"));
        assert!(names.contains(&"binary"));
        assert!(names.contains(&"atom"));
    }

    // ── Fallback tokenizer tests ────────────────────────────────────

    /// Helper: get the source text at a token's span.
    fn text_at<'a>(src: &'a str, tok: &SemanticToken) -> &'a str {
        &src[tok.span.start..tok.span.end]
    }

    #[test]
    fn fallback_function_def_at_line_start() {
        let src = "-module(test).\nfoo(X) -> X.\n";
        let tokens = collect_fallback_tokens(src);

        let func_defs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function
                && t.modifiers.0 & TokenModifiers::DECLARATION.0 != 0)
            .collect();
        assert_eq!(func_defs.len(), 1);
        assert_eq!(text_at(src, func_defs[0]), "foo");

        // Variables should be present
        let vars: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable)
            .collect();
        assert!(vars.len() >= 2, "expected at least 2 variable tokens, got {}", vars.len());
    }

    #[test]
    fn fallback_local_call() {
        let src = "-module(test).\nfoo() ->\n    bar(1).\n";
        let tokens = collect_fallback_tokens(src);

        // bar should be a function call (not at line start)
        let calls: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function
                && t.modifiers.0 == TokenModifiers::NONE.0)
            .collect();
        assert!(calls.iter().any(|t| text_at(src, t) == "bar"),
            "expected local call `bar`, got: {:?}", calls.iter().map(|t| text_at(src, t)).collect::<Vec<_>>());
    }

    #[test]
    fn fallback_remote_call() {
        let src = "-module(test).\nfoo() ->\n    io:format(\"hi\").\n";
        let tokens = collect_fallback_tokens(src);

        let ns: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Namespace)
            .collect();
        assert!(ns.iter().any(|t| text_at(src, t) == "io"),
            "expected namespace `io`");

        let funcs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function && t.modifiers.0 == 0)
            .collect();
        assert!(funcs.iter().any(|t| text_at(src, t) == "format"),
            "expected function `format`");
    }

    #[test]
    fn fallback_variables_with_unused() {
        let src = "-module(test).\nfoo(_Unused, Used) -> Used.\n";
        let tokens = collect_fallback_tokens(src);

        let unused: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable
                && t.modifiers.0 & TokenModifiers::UNUSED.0 != 0)
            .collect();
        assert!(unused.iter().any(|t| text_at(src, t) == "_Unused"),
            "expected _Unused with UNUSED modifier");

        let used: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable
                && t.modifiers.0 == TokenModifiers::NONE.0
                && text_at(src, t) == "Used")
            .collect();
        assert!(!used.is_empty(), "expected Used without UNUSED modifier");
    }

    #[test]
    fn fallback_record_name() {
        let src = "-module(test).\nfoo() -> #my_record{field = 1}.\n";
        let tokens = collect_fallback_tokens(src);

        let props: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Property)
            .collect();
        assert!(props.iter().any(|t| text_at(src, t) == "my_record"),
            "expected record name `my_record` as Property");
    }

    #[test]
    fn fallback_fun_reference() {
        let src = "-module(test).\nfoo() -> fun bar/2.\n";
        let tokens = collect_fallback_tokens(src);

        let funcs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function && text_at(src, t) == "bar")
            .collect();
        assert_eq!(funcs.len(), 1, "expected fun reference `bar`");
    }

    #[test]
    fn fallback_remote_fun_reference() {
        let src = "-module(test).\nfoo() -> fun lists:map/2.\n";
        let tokens = collect_fallback_tokens(src);

        assert!(tokens.iter().any(|t| t.token_type == TokenType::Namespace && text_at(src, t) == "lists"),
            "expected namespace `lists`");
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Function && text_at(src, t) == "map"),
            "expected function `map`");
    }

    #[test]
    fn fallback_skips_attributes() {
        let src = "-module(test).\n-export([foo/0]).\nfoo() -> ok.\n";
        let tokens = collect_fallback_tokens(src);

        // `test`, `foo` inside -module/-export should NOT produce tokens.
        // Only `foo` at line start (the function def) should appear.
        let func_defs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function)
            .collect();
        assert_eq!(func_defs.len(), 1, "expected only one function token (the def), got: {:?}",
            func_defs.iter().map(|t| text_at(src, t)).collect::<Vec<_>>());
        assert_eq!(text_at(src, func_defs[0]), "foo");
    }

    #[test]
    fn fallback_works_with_syntax_errors() {
        // Incomplete code — parser would fail, but the fallback still works.
        let src = "-module(test).\nfoo(X) ->\n    io:format(\"hello ~s\", [X]),\n    greeter:\n";
        let tokens = collect_fallback_tokens(src);

        // Should still find the function def, variables, and the remote call
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Function
            && t.modifiers.0 & TokenModifiers::DECLARATION.0 != 0
            && text_at(src, t) == "foo"),
            "expected function def `foo`");
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Namespace && text_at(src, t) == "io"),
            "expected namespace `io`");
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Namespace && text_at(src, t) == "greeter"),
            "expected namespace `greeter` even with incomplete code");
    }

    // ── Fallback edge cases ─────────────────────────────────────────

    #[test]
    fn fallback_multiple_clauses() {
        // Only the first clause starts at column 0; after `;` the next clause
        // also starts at column 0 and should get DECLARATION|DEFINITION.
        let src = "-module(test).\nfoo(1) -> one;\nfoo(2) -> two.\n";
        let tokens = collect_fallback_tokens(src);

        let defs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function
                && t.modifiers.0 & TokenModifiers::DECLARATION.0 != 0)
            .collect();
        assert_eq!(defs.len(), 2, "both clauses of foo should be function defs");
    }

    #[test]
    fn fallback_multiple_functions() {
        let src = "-module(test).\nfoo() -> ok.\nbar(X) -> X.\n";
        let tokens = collect_fallback_tokens(src);

        let defs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function
                && t.modifiers.0 & TokenModifiers::DECLARATION.0 != 0)
            .collect();
        let names: Vec<_> = defs.iter().map(|t| text_at(src, t)).collect();
        assert_eq!(names, vec!["foo", "bar"]);
    }

    #[test]
    fn fallback_nested_case_variables() {
        let src = "-module(test).\nfoo(X) ->\n    case X of\n        {ok, Y} -> Y;\n        error -> X\n    end.\n";
        let tokens = collect_fallback_tokens(src);

        let vars: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable)
            .map(|t| text_at(src, t))
            .collect();
        // X appears 3 times, Y appears 2 times
        assert_eq!(vars.iter().filter(|&&v| v == "X").count(), 3);
        assert_eq!(vars.iter().filter(|&&v| v == "Y").count(), 2);
    }

    #[test]
    fn fallback_quoted_atom_call() {
        let src = "-module(test).\nfoo() -> 'weird-name'(1).\n";
        let tokens = collect_fallback_tokens(src);

        // 'weird-name' should be detected as a local call
        let calls: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function
                && t.modifiers.0 == TokenModifiers::NONE.0
                && text_at(src, t) == "'weird-name'")
            .collect();
        assert_eq!(calls.len(), 1, "expected quoted atom call");
    }

    #[test]
    fn fallback_record_dot_field() {
        // #rec.field — record index
        let src = "-module(test).\nfoo() -> #rec.field.\n";
        let tokens = collect_fallback_tokens(src);

        assert!(tokens.iter().any(|t| t.token_type == TokenType::Property
            && text_at(src, t) == "rec"),
            "expected record name `rec`");
    }

    #[test]
    fn fallback_empty_source() {
        let tokens = collect_fallback_tokens("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn fallback_only_attributes() {
        let src = "-module(test).\n-export([foo/0]).\n";
        let tokens = collect_fallback_tokens(src);
        // No regular code, should produce no tokens
        assert!(tokens.is_empty(), "expected no fallback tokens from attributes only, got: {:?}",
            tokens.iter().map(|t| (text_at(src, t), &t.token_type)).collect::<Vec<_>>());
    }

    #[test]
    fn fallback_chained_remote_calls() {
        let src = "-module(test).\nfoo() ->\n    lists:map(fun bar/1, string:tokens(S, \",\")).\n";
        let tokens = collect_fallback_tokens(src);

        let namespaces: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Namespace)
            .map(|t| text_at(src, t))
            .collect();
        assert!(namespaces.contains(&"lists"), "expected namespace `lists`");
        assert!(namespaces.contains(&"string"), "expected namespace `string`");

        let funcs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function && t.modifiers.0 == 0)
            .map(|t| text_at(src, t))
            .collect();
        assert!(funcs.contains(&"map"), "expected function `map`");
        assert!(funcs.contains(&"tokens"), "expected function `tokens`");
        assert!(funcs.contains(&"bar"), "expected fun ref `bar`");
    }

    #[test]
    fn fallback_record_in_attribute_skipped() {
        // Records defined in attributes should not produce Property tokens.
        let src = "-module(test).\n-record(person, {name, age}).\nfoo() -> #person{name = \"Alice\"}.\n";
        let tokens = collect_fallback_tokens(src);

        let props: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Property)
            .map(|t| text_at(src, t))
            .collect();
        // Only the #person in the body should appear, not the one in -record()
        assert_eq!(props.len(), 1, "expected 1 property (body), got: {:?}", props);
        assert_eq!(props[0], "person");
    }

    #[test]
    fn fallback_wildcard_variable() {
        // `_` alone should get UNUSED but still be a Variable
        let src = "-module(test).\nfoo(_) -> ok.\n";
        let tokens = collect_fallback_tokens(src);

        let wildcards: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Variable && text_at(src, t) == "_")
            .collect();
        assert_eq!(wildcards.len(), 1);
        assert_ne!(wildcards[0].modifiers.0 & TokenModifiers::UNUSED.0, 0,
            "`_` should have UNUSED modifier");
    }

    #[test]
    fn fallback_macro_attribute_like_ifdef() {
        // -ifdef, -ifndef are attribute-like and should be skipped
        let src = "-module(test).\n-ifdef(DEBUG).\ndebug() -> ok.\n-endif.\nfoo() -> ok.\n";
        let tokens = collect_fallback_tokens(src);

        let defs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function
                && t.modifiers.0 & TokenModifiers::DECLARATION.0 != 0)
            .map(|t| text_at(src, t))
            .collect();
        // debug() is between -ifdef/-endif but not inside an attribute per se,
        // because -ifdef ends with `.` and debug() starts at column 0
        // Both foo and debug should be function defs
        assert!(defs.contains(&"foo"), "expected function def `foo`");
        assert!(defs.contains(&"debug"), "expected function def `debug`");
    }

    #[test]
    fn fallback_tokens_sorted_by_position() {
        let src = "-module(test).\nfoo(X, Y) -> bar(X), baz(Y).\n";
        let tokens = collect_fallback_tokens(src);

        for pair in tokens.windows(2) {
            assert!(pair[0].span.start <= pair[1].span.start,
                "tokens must be sorted: {} > {}", pair[0].span.start, pair[1].span.start);
        }
    }

    #[test]
    fn fallback_incomplete_fun_ref() {
        // fun foo — without /arity, should not emit
        let src = "-module(test).\nbar() -> fun foo.\n";
        let tokens = collect_fallback_tokens(src);

        // foo is not followed by / so it should not be a fun ref
        // It's just a bare atom — no token
        let funcs: Vec<_> = tokens.iter()
            .filter(|t| t.token_type == TokenType::Function && text_at(src, t) == "foo")
            .collect();
        assert!(funcs.is_empty(), "bare `fun foo` without /arity should not produce a function token");
    }
}
