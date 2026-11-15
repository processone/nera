use nera_fmt::{config::FormatConfig, format_source};
use nera_syntax::{ast::Expr, parser::Parser};

#[test]
fn short_circuit_binding_fixtures_preserve_rhs_matches() {
    // The unsafe-binding compiler fixtures exposed lost parentheses in CI.
    // Formatting must retain their syntax and the match inside the operand.
    for source in [
        include_str!("../../../tests/andalso_unsafe_binding.erl"),
        include_str!("../../../tests/orelse_unsafe_binding.erl"),
    ] {
        let formatted = format_source(source, &FormatConfig::default()).unwrap();
        let parsed = Parser::new(&formatted).parse_module();
        assert!(!parsed.has_errors(), "{:?}\n{formatted}", parsed.errors);
        let Expr::Match(_, value, _) = &parsed.module.functions[0].clauses[0].body[0] else {
            panic!("lost outer match: {formatted}");
        };
        let Expr::BinOp(_, _, rhs, _) = value.as_ref() else {
            panic!("lost short-circuit expression: {formatted}");
        };
        assert!(matches!(rhs.as_ref(), Expr::Match(..)), "{formatted}");
        assert_eq!(
            formatted,
            format_source(&formatted, &FormatConfig::default()).unwrap()
        );
    }
}

#[test]
fn weak_operands_keep_their_binary_operator_boundary() {
    // The shared binary-child formatter handles both operands and flattened
    // chains. Match, send and catch all bind more weakly than binary operators.
    for (expression, left) in [
        ("(Y = A) andalso B", true),
        ("A andalso (Y = B)", false),
        ("(Y = A) andalso B andalso true", true),
        ("A andalso B andalso (Y = true)", false),
        ("(Pid ! true) orelse A", true),
        ("A orelse (Pid ! true)", false),
        ("(catch A) orelse B", true),
        ("A orelse (catch B)", false),
    ] {
        let source = format!("-module(test). f(A, B, Pid) -> {expression}.");
        let original = Parser::new(&source).parse_module();
        assert!(!original.has_errors(), "{:?}", original.errors);
        let formatted = format_source(&source, &FormatConfig::default()).unwrap();
        let parsed = Parser::new(&formatted).parse_module();
        assert!(!parsed.has_errors(), "{:?}\n{formatted}", parsed.errors);
        let before = &original.module.functions[0].clauses[0].body[0];
        let after = &parsed.module.functions[0].clauses[0].body[0];
        let (
            Expr::BinOp(before_op, before_lhs, before_rhs, _),
            Expr::BinOp(after_op, after_lhs, after_rhs, _),
        ) = (before, after)
        else {
            panic!("lost binary operator boundary: {formatted}");
        };
        assert_eq!(before_op, after_op);
        let (mut before_child, mut after_child) = if left {
            (before_lhs.as_ref(), after_lhs.as_ref())
        } else {
            (before_rhs.as_ref(), after_rhs.as_ref())
        };
        if left {
            while let Expr::BinOp(_, lhs, _, _) = before_child {
                before_child = lhs;
            }
            while let Expr::BinOp(_, lhs, _, _) = after_child {
                after_child = lhs;
            }
        }
        assert_eq!(
            std::mem::discriminant(before_child),
            std::mem::discriminant(after_child),
            "changed operand kind: {formatted}"
        );
        assert_eq!(
            formatted,
            format_source(&formatted, &FormatConfig::default()).unwrap()
        );
    }
}
