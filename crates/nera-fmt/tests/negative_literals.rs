use nera_fmt::{config::FormatConfig, format_source};
use nera_syntax::{ast::Expr, parser::Parser};

// A negated negative literal must remain valid Erlang after formatting, with
// the same numeric operand and stable output on subsequent formatting passes.
fn check_negated_literal(expression: &str) {
    let source =
        format!("-module(negative_literals).\n-export([value/0]).\nvalue() -> {expression}.\n");
    let original = Parser::new(&source).parse_module();
    assert!(!original.has_errors(), "{:?}", original.errors);

    let config = FormatConfig::default();
    let formatted = format_source(&source, &config).unwrap();
    let parsed = Parser::new(&formatted).parse_module();
    assert!(
        !parsed.has_errors(),
        "formatted {expression} does not parse: {:?}\n{formatted}",
        parsed.errors
    );
    assert_eq!(
        parsed.module.functions.len(),
        original.module.functions.len()
    );
    check_numeric_expression(
        &original.module.functions[0].clauses[0].body[0],
        &parsed.module.functions[0].clauses[0].body[0],
    );
    assert_eq!(
        formatted,
        format_source(&formatted, &config).unwrap(),
        "formatting {expression} is not idempotent"
    );
}

fn check_numeric_expression(original: &Expr, formatted: &Expr) {
    match (original, formatted) {
        (Expr::Integer(a, _), Expr::Integer(b, _)) => assert_eq!(a, b),
        (Expr::BigInteger(a, _), Expr::BigInteger(b, _)) => assert_eq!(a, b),
        (Expr::Float(a, _), Expr::Float(b, _)) => assert_eq!(a.to_bits(), b.to_bits()),
        (Expr::UnaryOp(a, x, _), Expr::UnaryOp(b, y, _)) => {
            assert_eq!(a, b);
            check_numeric_expression(x, y);
        }
        _ => panic!("numeric expression changed: {original:?} -> {formatted:?}"),
    }
}

#[test]
fn negated_negative_zero_remains_parseable_and_idempotent() {
    // Integer zero loses its sign in the AST value, not in the printed source.
    for expression in [
        "-(-0)", "-(- 0)", "-(-16#0)", "-(-0_0)", "-(-(-0))", "-(-0.0)",
    ] {
        check_negated_literal(expression);
    }
}

#[test]
fn negated_negative_big_integer_remains_parseable_and_idempotent() {
    for expression in [
        "-(-99999999999999999999)",
        "-(-99_999_999_999_999_999_999)",
        "-(-16#FFFFFFFFFFFFFFFFF)",
        "-(- 99999999999999999999)",
        "-(-(-99999999999999999999))",
    ] {
        check_negated_literal(expression);
    }
}
