//! Validates binary segment type specifiers.
//!
//! Detects invalid combinations that OTP's compiler rejects at compile time:
//! - Float size must be 16, 32, or 64 bits
//! - UTF types cannot have explicit size
//! - Explicit fields must not conflict; irrelevant flags are accepted as in OTP
//! - Unit does not apply to UTF types
//! - A string literal in a pattern takes no size and only a UTF type

use nera_syntax::ast::{self, BinSegmentKind, BinSpecifier};
use nera_syntax::span::Span;
use nera_syntax::error::Diagnostic;

/// OTP's messages for an unsized binary or bitstring field that is not the
/// last one of a pattern, and for one ending a `<=` generator pattern.
const UNSIZED_FIELD_NOT_LAST: &str =
    "a binary segment without size is only allowed at the end of a binary pattern";
const UNSIZED_GENERATOR_FIELD: &str =
    "binary fields without size are not allowed in patterns of bit string generators";
/// OTP's typed_literal_string message for a string literal in a binary
/// pattern with a size, or with a type list that names no UTF type.
const TYPED_LITERAL_STRING: &str =
    "a literal string in a binary pattern must not have a type or a size";

/// Check all binary segments in a function for invalid type specifiers.
pub(super) fn check_binary_segments_fn(
    func: &ast::Function,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for clause in &func.clauses {
        check_fun_clause(clause, diagnostics);
    }
}

/// The first segment anywhere in the module that OTP rejects and lowering
/// would give another meaning: conflicting specifiers, an unsized field before
/// the end of a pattern or ending a `<=` generator pattern, or a string literal
/// with a size or a non-UTF type. Lowering refuses the module before any function
/// is lowered: embedded modules skip lint, and guard, size and map-key
/// lowering discard errors. The walkers below match every expression and
/// pattern variant, so a new position cannot slip past them.
pub(crate) fn first_refused_segment_error(module: &ast::Module) -> Option<Diagnostic> {
    let mut diagnostics = Vec::new();
    for func in &module.functions {
        check_binary_segments_fn(func, &mut diagnostics);
    }
    check_binary_segments_records(module, &mut diagnostics);
    diagnostics.into_iter().find(|d| {
        matches!(
            d.message.as_str(),
            UNSIZED_FIELD_NOT_LAST | UNSIZED_GENERATOR_FIELD | TYPED_LITERAL_STRING
        ) || d.code.as_deref() == Some("conflicting_binary_specifiers")
    })
}

/// Record field defaults are expressions outside every function. OTP checks
/// them whether or not a record is ever created or the field overridden.
pub(super) fn check_binary_segments_records(
    module: &ast::Module,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for record in &module.records {
        for field in &record.fields {
            if let Some(default) = &field.default {
                check_expr(default, diagnostics);
            }
        }
    }
}

fn check_fun_clause(clause: &ast::FunctionClause, diagnostics: &mut Vec<Diagnostic>) {
    for pat in &clause.patterns {
        check_pattern(pat, diagnostics);
    }
    if let Some(guard) = &clause.guard {
        check_guard(guard, diagnostics);
    }
    for expr in &clause.body {
        check_expr(expr, diagnostics);
    }
}

fn check_case_clause(clause: &ast::CaseClause, diagnostics: &mut Vec<Diagnostic>) {
    check_pattern(&clause.pattern, diagnostics);
    if let Some(guard) = &clause.guard {
        check_guard(guard, diagnostics);
    }
    for e in &clause.body {
        check_expr(e, diagnostics);
    }
}

fn check_guard(guard: &ast::Guard, diagnostics: &mut Vec<Diagnostic>) {
    match guard {
        ast::Guard::Test(e) => check_expr(e, diagnostics),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                check_expr(e, diagnostics);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                check_guard(g, diagnostics);
            }
        }
    }
}

// ── Pattern walker ─────────────────────────────────────────

fn check_pattern(pat: &ast::Pattern, diagnostics: &mut Vec<Diagnostic>) {
    match pat {
        ast::Pattern::Binary(segments, _) => {
            for seg in segments {
                // As in OTP, a refused string segment gets this error alone.
                if is_typed_literal_string(seg) {
                    diagnostics.push(
                        Diagnostic::error_at(TYPED_LITERAL_STRING, seg.span)
                            .with_code("invalid_binary_segment"),
                    );
                    continue;
                }
                validate_segment(&seg.type_spec, &seg.size, seg.span, diagnostics);
                check_pattern(&seg.value, diagnostics);
                if let Some(size) = &seg.size {
                    check_expr(size, diagnostics);
                }
            }
            // Unsized binary segment must be last.
            for (i, seg) in segments.iter().enumerate() {
                if matches!(seg.type_spec.typ, BinSegmentKind::Binary | BinSegmentKind::Bitstring)
                    && seg.size.is_none()
                    && specifier_conflict(&seg.type_spec).is_none()
                    && !is_typed_literal_string(seg)
                    && i < segments.len() - 1
                {
                    diagnostics.push(
                        Diagnostic::error_at(UNSIZED_FIELD_NOT_LAST, seg.span)
                            .with_code("invalid_binary_segment"),
                    );
                }
            }
        }
        ast::Pattern::Tuple(pats, _) => {
            for p in pats {
                check_pattern(p, diagnostics);
            }
        }
        ast::Pattern::Cons(hd, tl, _) => {
            check_pattern(hd, diagnostics);
            check_pattern(tl, diagnostics);
        }
        ast::Pattern::Match(lhs, rhs, _) => {
            check_pattern(lhs, diagnostics);
            check_pattern(rhs, diagnostics);
        }
        ast::Pattern::Map(fields, _) => {
            for f in fields {
                check_expr(&f.key, diagnostics);
                check_pattern(&f.value, diagnostics);
            }
        }
        ast::Pattern::Record { fields, .. } => {
            for f in fields {
                check_pattern(&f.pattern, diagnostics);
            }
        }
        ast::Pattern::Concat(prefix, rest, _) => {
            check_pattern(prefix, diagnostics);
            check_pattern(rest, diagnostics);
        }
        ast::Pattern::MacroCall { args, .. } => {
            for p in args.iter().flatten() {
                check_pattern(p, diagnostics);
            }
        }
        ast::Pattern::Wildcard(_)
        | ast::Pattern::Variable(..)
        | ast::Pattern::Integer(..)
        | ast::Pattern::BigInteger(..)
        | ast::Pattern::Float(..)
        | ast::Pattern::Atom(..)
        | ast::Pattern::String(..)
        | ast::Pattern::Nil(_) => {}
    }
}

// ── Expression walker ──────────────────────────────────────

fn check_expr(expr: &ast::Expr, diagnostics: &mut Vec<Diagnostic>) {
    match expr {
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                validate_segment(&seg.type_spec, &seg.size, seg.span, diagnostics);
                check_expr(&seg.value, diagnostics);
                if let Some(size) = &seg.size {
                    check_expr(size, diagnostics);
                }
            }
            // The "unsized binary segment must be last" rule only applies
            // to binary PATTERNS (for unambiguous decoding). In binary
            // construction unsized `/binary` segments are legal anywhere.
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            check_expr(scrutinee, diagnostics);
            for clause in clauses {
                check_case_clause(clause, diagnostics);
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                check_guard(&clause.guard, diagnostics);
                for e in &clause.body {
                    check_expr(e, diagnostics);
                }
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                check_case_clause(clause, diagnostics);
            }
            if let Some(after) = after {
                check_expr(&after.timeout, diagnostics);
                for e in &after.body {
                    check_expr(e, diagnostics);
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
                check_expr(e, diagnostics);
            }
            for clause in of_clauses {
                check_case_clause(clause, diagnostics);
            }
            for clause in catch_clauses {
                check_pattern(&clause.pattern, diagnostics);
                if let Some(stacktrace) = &clause.stacktrace {
                    check_pattern(stacktrace, diagnostics);
                }
                if let Some(guard) = &clause.guard {
                    check_guard(guard, diagnostics);
                }
                for e in &clause.body {
                    check_expr(e, diagnostics);
                }
            }
            if let Some(after) = after_body {
                for e in after {
                    check_expr(e, diagnostics);
                }
            }
        }
        ast::Expr::Fun(_, clauses, _) => {
            for clause in clauses {
                check_fun_clause(clause, diagnostics);
            }
        }
        ast::Expr::Block(exprs, _) => {
            for e in exprs {
                check_expr(e, diagnostics);
            }
        }
        ast::Expr::Match(pat, rhs, _) => {
            check_pattern(pat, diagnostics);
            check_expr(rhs, diagnostics);
        }
        ast::Expr::Tuple(exprs, _) => {
            for e in exprs {
                check_expr(e, diagnostics);
            }
        }
        ast::Expr::Cons(hd, tl, _) => {
            check_expr(hd, diagnostics);
            check_expr(tl, diagnostics);
        }
        ast::Expr::BinOp(_, lhs, rhs, _) => {
            check_expr(lhs, diagnostics);
            check_expr(rhs, diagnostics);
        }
        ast::Expr::UnaryOp(_, operand, _) => {
            check_expr(operand, diagnostics);
        }
        ast::Expr::Call(target, args, _) => {
            // Callee expressions hold code too: `(fun() -> ... end)()`
            // and `Mod:Fun(...)` with computed module or function terms.
            match target {
                ast::CallTarget::Remote(module_expr, fun_expr) => {
                    check_expr(module_expr, diagnostics);
                    check_expr(fun_expr, diagnostics);
                }
                ast::CallTarget::Expr(callee) => check_expr(callee, diagnostics),
                ast::CallTarget::Local(..) | ast::CallTarget::Variable(..) => {}
            }
            for a in args {
                check_expr(a, diagnostics);
            }
        }
        ast::Expr::Send(lhs, rhs, _) => {
            check_expr(lhs, diagnostics);
            check_expr(rhs, diagnostics);
        }
        ast::Expr::ListComp(expr, qualifiers, _) | ast::Expr::BinComp(expr, qualifiers, _) => {
            check_expr(expr, diagnostics);
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(pat, e, _)
                    | ast::LcQualifier::BinGenerator(pat, e, _) => {
                        check_pattern(pat, diagnostics);
                        // As in OTP, only the last field is checked here: an
                        // earlier unsized field already got check_pattern's
                        // "only allowed at the end" error, so each field
                        // reports once.
                        if let ast::LcQualifier::BinGenerator(ast::Pattern::Binary(segments, _), _, _) = q {
                            if let Some(seg) = segments.last().filter(|seg| {
                                matches!(seg.type_spec.typ, BinSegmentKind::Binary | BinSegmentKind::Bitstring)
                                    && seg.size.is_none()
                            }) {
                                diagnostics.push(Diagnostic::error_at(
                                    UNSIZED_GENERATOR_FIELD,
                                    seg.span,
                                ).with_code("invalid_binary_segment"));
                            }
                        }
                        check_expr(e, diagnostics);
                    }
                    ast::LcQualifier::Filter(e, _) => {
                        check_expr(e, diagnostics);
                    }
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                check_expr(&f.key, diagnostics);
                check_expr(&f.value, diagnostics);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_expr(base, diagnostics);
            for f in fields {
                check_expr(&f.key, diagnostics);
                check_expr(&f.value, diagnostics);
            }
        }
        ast::Expr::Throw(e, _) | ast::Expr::Catch(e, _) => {
            check_expr(e, diagnostics);
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                check_expr(&f.value, diagnostics);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            check_expr(base, diagnostics);
            for f in fields {
                check_expr(&f.value, diagnostics);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            check_expr(base, diagnostics);
        }
        ast::Expr::Maybe { body, else_clauses, .. } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => check_expr(e, diagnostics),
                    ast::MaybeExpr::CondMatch(pat, e, _) => {
                        check_pattern(pat, diagnostics);
                        check_expr(e, diagnostics);
                    }
                }
            }
            for clause in else_clauses {
                check_case_clause(clause, diagnostics);
            }
        }
        ast::Expr::FunRef(ast::FunRefTarget::DynamicRemote(module, function, arity), _) => {
            check_expr(module, diagnostics);
            check_expr(function, diagnostics);
            check_expr(arity, diagnostics);
        }
        ast::Expr::MacroCall { args, .. } => {
            for e in args.iter().flatten() {
                check_expr(e, diagnostics);
            }
        }
        ast::Expr::FunRef(ast::FunRefTarget::Local(..) | ast::FunRefTarget::Remote(..), _)
        | ast::Expr::RecordIndex { .. }
        | ast::Expr::Integer(..)
        | ast::Expr::BigInteger(..)
        | ast::Expr::Float(..)
        | ast::Expr::Atom(..)
        | ast::Expr::String(..)
        | ast::Expr::Nil(_)
        | ast::Expr::Variable(..) => {}
    }
}

// ── Segment validation ─────────────────────────────────────

/// OTP's good_string_size_type, inverted: a string literal in a pattern may
/// have no size, and a type list only when it names utf8, utf16 or utf32.
/// Lowering expands it into one unsized segment per codepoint, so a size or
/// a binary type would otherwise be dropped or read as a rest capture.
fn is_typed_literal_string(seg: &ast::BinSegment<ast::Pattern>) -> bool {
    matches!(seg.value, ast::Pattern::String(..))
        && (seg.size.is_some()
            || (seg.type_spec.explicit && !seg.type_spec.specifiers.iter().any(|s| matches!(s,
                BinSpecifier::Type(BinSegmentKind::Utf8 | BinSegmentKind::Utf16 | BinSegmentKind::Utf32)))))
}

/// OTP merges written declarations left to right, before defaults, and
/// names the new value first. Syntax retains the declarations; this semantic
/// layer owns both their conflict policy and the diagnostic wording.
fn specifier_conflict(spec: &ast::BinSegmentType) -> Option<String> {
    let mut typ = None;
    let mut sign = None;
    let mut endian = None;
    let mut unit = None;
    for written in &spec.specifiers {
        let result = match *written {
            BinSpecifier::Type(kind) => {
                let name = match kind {
                    BinSegmentKind::Integer => "integer",
                    BinSegmentKind::Float => "float",
                    BinSegmentKind::Binary | BinSegmentKind::Bitstring => "binary",
                    BinSegmentKind::Utf8 => "utf8",
                    BinSegmentKind::Utf16 => "utf16",
                    BinSegmentKind::Utf32 => "utf32",
                };
                merge_specifier(&mut typ, name, "type").and_then(|()| {
                    if kind == BinSegmentKind::Bitstring {
                        merge_specifier(&mut unit, 1, "unit")
                    } else {
                        Ok(())
                    }
                })
            }
            BinSpecifier::Bytes => merge_specifier(&mut typ, "binary", "type")
                .and_then(|()| merge_specifier(&mut unit, 8, "unit")),
            BinSpecifier::Signedness(value) => merge_specifier(&mut sign, value, "sign"),
            BinSpecifier::Endianness(value) => merge_specifier(&mut endian, value, "endianness"),
            BinSpecifier::Unit(value) => merge_specifier(&mut unit, value, "unit"),
        };
        if let Err(message) = result {
            return Some(message);
        }
    }
    None
}

fn merge_specifier<T: PartialEq + std::fmt::Display>(
    field: &mut Option<T>,
    value: T,
    category: &str,
) -> Result<(), String> {
    if let Some(previous) = field {
        if *previous != value {
            return Err(format!(
                "conflict in {category} specification for bit field: '{value}' and '{previous}'"
            ));
        }
    } else {
        *field = Some(value);
    }
    Ok(())
}

fn validate_segment(
    type_spec: &ast::BinSegmentType,
    size: &Option<Box<ast::Expr>>,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(message) = specifier_conflict(type_spec) {
        diagnostics.push(
            Diagnostic::error_at(message, span).with_code("conflicting_binary_specifiers"),
        );
        return;
    }
    match type_spec.typ {
        BinSegmentKind::Integer | BinSegmentKind::Binary | BinSegmentKind::Bitstring => {
            // OTP accepts signedness and endianness even when ignored.
        }
        BinSegmentKind::Float => {
            // A literal size must give 16, 32 or 64 bits once scaled by the
            // unit, as in OTP's bit_size_check: `<<X:1/float-unit:64>>` is valid.
            if let Some(size_expr) = size {
                if let ast::Expr::Integer(n, _) = size_expr.as_ref() {
                    let bits = n.checked_mul(i64::from(type_spec.unit.unwrap_or(1)));
                    if !matches!(bits, Some(16 | 32 | 64)) {
                        diagnostics.push(
                            Diagnostic::error_at(
                                format!(
                                    "invalid size {} for float segment (must be 16, 32, or 64)",
                                    bits.map_or_else(|| n.to_string(), |bits| bits.to_string())
                                ),
                                span,
                            )
                            .with_code("invalid_binary_segment"),
                        );
                    }
                }
            }
        }
        BinSegmentKind::Utf8 | BinSegmentKind::Utf16 | BinSegmentKind::Utf32 => {
            let kind_name = match type_spec.typ {
                BinSegmentKind::Utf8 => "utf8",
                BinSegmentKind::Utf16 => "utf16",
                BinSegmentKind::Utf32 => "utf32",
                _ => unreachable!(),
            };
            // UTF types cannot have explicit size.
            if size.is_some() {
                diagnostics.push(
                    Diagnostic::error_at(
                        format!("size not allowed for {} segments", kind_name),
                        span,
                    )
                    .with_code("invalid_binary_segment"),
                );
            }
            // Unit does not apply to UTF types.
            if type_spec.unit.is_some() {
                diagnostics.push(
                    Diagnostic::error_at(
                        format!("unit specifier not allowed for {} segments", kind_name),
                        span,
                    )
                    .with_code("invalid_binary_segment"),
                );
            }
        }
    }
}
