//! Format string validation for `io:format` and `io_lib:format`.
//!
//! Warns when the number of `~`-directives that consume arguments does not
//! match the length of the argument list.  Only fires when both the format
//! string and the argument list are literals (no false positives for dynamic
//! values).

use std::collections::HashMap;

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;

/// Check a function for format string / argument list mismatches.
pub(super) fn check_format_strings_fn(
    func: &ast::Function,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    for clause in &func.clauses {
        if let Some(ref guard) = clause.guard {
            check_guard(guard, atom_names, warnings);
        }
        for expr in &clause.body {
            check_expr(expr, atom_names, warnings);
        }
    }
}

// ── Format string parser ────────────────────────────────

/// Count how many arguments the format string consumes.
///
/// Returns `None` if the format string contains constructs we cannot
/// statically analyze (e.g. `~*` which reads width from the arg list,
/// or unrecognised directives).
fn count_format_args(format: &str) -> Option<usize> {
    let mut count = 0usize;
    let bytes = format.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] != b'~' {
            i += 1;
            continue;
        }
        i += 1; // skip '~'
        if i >= len {
            return None; // trailing ~
        }

        // Skip optional field width: digits, '-', or '*'
        // '*' means width is taken from the arg list — bail out.
        while i < len {
            match bytes[i] {
                b'0'..=b'9' | b'-' => i += 1,
                b'*' => return None, // dynamic width from args
                _ => break,
            }
        }
        if i >= len {
            return None;
        }

        // Skip optional precision: '.' followed by digits or '*'
        if bytes[i] == b'.' {
            i += 1;
            if i < len && bytes[i] == b'*' {
                return None; // dynamic precision from args
            }
            while i < len && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i >= len {
            return None;
        }

        // Skip optional padding char modifier
        if bytes[i] == b'.' {
            i += 1;
            if i >= len {
                return None;
            }
            // The padding character itself
            i += 1;
            if i >= len {
                return None;
            }
        }

        // Now we should be at the directive character.
        let directive = bytes[i];
        i += 1;

        match directive {
            // Consumes 1 argument
            b'c' | b'd' | b'e' | b'f' | b'g' | b'p' | b's' | b'w' | b'b' | b'B' | b'x'
            | b'X' | b'+' | b'#' | b'i' => {
                count += 1;
            }
            // Consumes 2 arguments (term + depth)
            b'P' | b'W' => {
                count += 2;
            }
            // Consumes 0 arguments
            b'n' | b'~' | b't' | b'l' => {}
            // Unknown directive — bail out to avoid false positives
            _ => return None,
        }
    }
    Some(count)
}

/// Count elements in a proper list literal.
///
/// Returns `None` if the list has a variable tail (improper or dynamic).
fn count_list_elements(expr: &ast::Expr) -> Option<usize> {
    let mut count = 0;
    let mut current = expr;
    loop {
        match current {
            ast::Expr::Nil(_) => return Some(count),
            ast::Expr::Cons(_, tail, _) => {
                count += 1;
                current = tail;
            }
            _ => return None,
        }
    }
}

// ── Call detection ──────────────────────────────────────

/// Check whether a remote call is `io:format` or `io_lib:format` and
/// validate the format string against the argument list.
fn check_format_call(
    target: &ast::CallTarget,
    args: &[ast::Expr],
    _span: nera_syntax::span::Span,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    let (module_id, fun_id) = match target {
        ast::CallTarget::Remote(module_expr, fun_expr) => {
            match (module_expr.as_ref(), fun_expr.as_ref()) {
                (ast::Expr::Atom(m, _), ast::Expr::Atom(f, _)) => (*m, *f),
                _ => return,
            }
        }
        _ => return,
    };

    let module_name = match atom_names.get(&module_id) {
        Some(n) => n.as_str(),
        None => return,
    };
    let fun_name = match atom_names.get(&fun_id) {
        Some(n) => n.as_str(),
        None => return,
    };

    if fun_name != "format" && fun_name != "fwrite" {
        return;
    }

    // Determine which args are (format_string, arg_list) based on module and arity.
    let (fmt_expr, args_expr) = match (module_name, args.len()) {
        // io:format(FormatStr, Args) or io_lib:format(FormatStr, Args)
        ("io" | "io_lib", 2) => (&args[0], &args[1]),
        // io:format(Device, FormatStr, Args)
        ("io", 3) => (&args[1], &args[2]),
        _ => return,
    };

    // Extract the literal format string.
    let (format_str, fmt_span) = match fmt_expr {
        ast::Expr::String(s, sp) => (s.as_str(), *sp),
        _ => return, // dynamic format string — can't check
    };

    // Count expected arguments from the format string.
    let expected = match count_format_args(format_str) {
        Some(n) => n,
        None => return, // complex format — skip
    };

    // Count provided arguments from the list literal.
    let provided = match count_list_elements(args_expr) {
        Some(n) => n,
        None => return, // dynamic list — can't check
    };

    if expected != provided {
        warnings.push(
            Diagnostic::warning_at(
                format!(
                    "format string expects {} argument(s) but {} provided",
                    expected, provided,
                ),
                fmt_span,
            )
            .with_code("format_string_mismatch")
            .with_hint(format!(
                "the call at this location passes {} element(s) in the argument list",
                provided,
            )),
        );
    }
}

// ── Expression walker ───────────────────────────────────

fn check_expr(
    expr: &ast::Expr,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    match expr {
        ast::Expr::Call(target, args, span) => {
            check_format_call(target, args, *span, atom_names, warnings);
            // Recurse into sub-expressions.
            if let ast::CallTarget::Remote(m, f) = target {
                check_expr(m, atom_names, warnings);
                check_expr(f, atom_names, warnings);
            }
            for arg in args {
                check_expr(arg, atom_names, warnings);
            }
        }
        ast::Expr::Tuple(exprs, _) | ast::Expr::Block(exprs, _) => {
            for e in exprs {
                check_expr(e, atom_names, warnings);
            }
        }
        ast::Expr::Cons(h, t, _) | ast::Expr::Send(h, t, _) | ast::Expr::BinOp(_, h, t, _) => {
            check_expr(h, atom_names, warnings);
            check_expr(t, atom_names, warnings);
        }
        ast::Expr::Match(_, e, _)
        | ast::Expr::UnaryOp(_, e, _)
        | ast::Expr::Throw(e, _)
        | ast::Expr::Catch(e, _) => {
            check_expr(e, atom_names, warnings);
        }
        ast::Expr::Case(scrutinee, clauses, _) => {
            check_expr(scrutinee, atom_names, warnings);
            for clause in clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, atom_names, warnings);
                }
                for e in &clause.body {
                    check_expr(e, atom_names, warnings);
                }
            }
        }
        ast::Expr::If(clauses, _) => {
            for clause in clauses {
                check_guard(&clause.guard, atom_names, warnings);
                for e in &clause.body {
                    check_expr(e, atom_names, warnings);
                }
            }
        }
        ast::Expr::Receive(clauses, after, _) => {
            for clause in clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, atom_names, warnings);
                }
                for e in &clause.body {
                    check_expr(e, atom_names, warnings);
                }
            }
            if let Some(after) = after {
                check_expr(&after.timeout, atom_names, warnings);
                for e in &after.body {
                    check_expr(e, atom_names, warnings);
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
                check_expr(e, atom_names, warnings);
            }
            for clause in of_clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, atom_names, warnings);
                }
                for e in &clause.body {
                    check_expr(e, atom_names, warnings);
                }
            }
            for clause in catch_clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, atom_names, warnings);
                }
                for e in &clause.body {
                    check_expr(e, atom_names, warnings);
                }
            }
            if let Some(after) = after_body {
                for e in after {
                    check_expr(e, atom_names, warnings);
                }
            }
        }
        ast::Expr::Fun(_, clauses, _) => {
            for clause in clauses {
                if let Some(ref guard) = clause.guard {
                    check_guard(guard, atom_names, warnings);
                }
                for e in &clause.body {
                    check_expr(e, atom_names, warnings);
                }
            }
        }
        ast::Expr::ListComp(body, qualifiers, _) | ast::Expr::BinComp(body, qualifiers, _) => {
            check_expr(body, atom_names, warnings);
            for q in qualifiers {
                match q {
                    ast::LcQualifier::Generator(_, e, _)
                    | ast::LcQualifier::BinGenerator(_, e, _)
                    | ast::LcQualifier::Filter(e, _) => {
                        check_expr(e, atom_names, warnings);
                    }
                }
            }
        }
        ast::Expr::Binary(segments, _) => {
            for seg in segments {
                check_expr(&seg.value, atom_names, warnings);
                if let Some(ref size) = seg.size {
                    check_expr(size, atom_names, warnings);
                }
            }
        }
        ast::Expr::Map(fields, _) => {
            for f in fields {
                check_expr(&f.key, atom_names, warnings);
                check_expr(&f.value, atom_names, warnings);
            }
        }
        ast::Expr::MapUpdate(base, fields, _) => {
            check_expr(base, atom_names, warnings);
            for f in fields {
                check_expr(&f.key, atom_names, warnings);
                check_expr(&f.value, atom_names, warnings);
            }
        }
        ast::Expr::RecordCreate { fields, .. } => {
            for f in fields {
                check_expr(&f.value, atom_names, warnings);
            }
        }
        ast::Expr::RecordUpdate { base, fields, .. } => {
            check_expr(base, atom_names, warnings);
            for f in fields {
                check_expr(&f.value, atom_names, warnings);
            }
        }
        ast::Expr::RecordAccess { base, .. } => {
            check_expr(base, atom_names, warnings);
        }
        ast::Expr::Maybe {
            body,
            else_clauses,
            ..
        } => {
            for item in body {
                match item {
                    ast::MaybeExpr::Expr(e) => check_expr(e, atom_names, warnings),
                    ast::MaybeExpr::CondMatch(_, e, _) => check_expr(e, atom_names, warnings),
                }
            }
            for clause in else_clauses {
                for e in &clause.body {
                    check_expr(e, atom_names, warnings);
                }
            }
        }
        // Leaf nodes
        ast::Expr::Variable(_, _)
        | ast::Expr::Integer(_, _)
        | ast::Expr::BigInteger(_, _)
        | ast::Expr::Float(_, _)
        | ast::Expr::Atom(_, _)
        | ast::Expr::String(_, _)
        | ast::Expr::Nil(_)
        | ast::Expr::FunRef(_, _)
        | ast::Expr::RecordIndex { .. }
        | ast::Expr::MacroCall { .. } => {}
    }
}

fn check_guard(
    guard: &ast::Guard,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    match guard {
        ast::Guard::Test(e) => check_expr(e, atom_names, warnings),
        ast::Guard::Conjunction(exprs) => {
            for e in exprs {
                check_expr(e, atom_names, warnings);
            }
        }
        ast::Guard::Disjunction(guards) => {
            for g in guards {
                check_guard(g, atom_names, warnings);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_simple_directives() {
        assert_eq!(count_format_args("~p ~p ~p"), Some(3));
        assert_eq!(count_format_args("~s: ~d"), Some(2));
        assert_eq!(count_format_args("hello~n"), Some(0));
        assert_eq!(count_format_args("~~escaped~~"), Some(0));
        assert_eq!(count_format_args(""), Some(0));
    }

    #[test]
    fn count_two_arg_directives() {
        assert_eq!(count_format_args("~P"), Some(2));
        assert_eq!(count_format_args("~W"), Some(2));
        assert_eq!(count_format_args("~p ~P"), Some(3));
    }

    #[test]
    fn count_ignore_directive() {
        assert_eq!(count_format_args("~p~i~p"), Some(3));
    }

    #[test]
    fn count_with_width() {
        assert_eq!(count_format_args("~10p"), Some(1));
        assert_eq!(count_format_args("~-10s"), Some(1));
    }

    #[test]
    fn count_with_precision() {
        assert_eq!(count_format_args("~.5f"), Some(1));
        assert_eq!(count_format_args("~10.5f"), Some(1));
    }

    #[test]
    fn count_star_width_bails() {
        assert_eq!(count_format_args("~*p"), None);
    }

    #[test]
    fn count_unknown_directive_bails() {
        assert_eq!(count_format_args("~z"), None);
    }

    #[test]
    fn count_trailing_tilde_bails() {
        assert_eq!(count_format_args("hello~"), None);
    }

    #[test]
    fn count_no_arg_directives() {
        assert_eq!(count_format_args("~n~n~n"), Some(0));
        assert_eq!(count_format_args("~t~l"), Some(0));
    }
}
