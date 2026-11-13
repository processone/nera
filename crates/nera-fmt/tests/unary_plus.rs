use nera_fmt::{config::FormatConfig, format_source};
use nera_syntax::{
    ast::{BinOp, Expr, UnaryOp},
    parser::Parser,
};

// Newly accepted unary plus must not become list append or lose the grouping
// of a weaker-binding operand when a program is formatted.
#[test]
fn formatted_unary_plus_keeps_the_expression_tree() {
    let source = "-module(unary_plus). f(X) -> +(+X). g(X) -> +(X + 2) * 2.";
    let formatted = format_source(source, &FormatConfig::default()).unwrap();
    let result = Parser::new(&formatted).parse_module();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(matches!(&result.module.functions[0].clauses[0].body[0],
        Expr::UnaryOp(UnaryOp::Pos, operand, _)
            if matches!(operand.as_ref(), Expr::UnaryOp(UnaryOp::Pos, inner, _)
                if matches!(inner.as_ref(), Expr::Variable(..)))));
    let Expr::BinOp(BinOp::Mul, positive, _, _) = &result.module.functions[1].clauses[0].body[0]
    else {
        panic!("lost outer multiplication: {formatted}");
    };
    assert!(
        matches!(positive.as_ref(), Expr::UnaryOp(UnaryOp::Pos, operand, _)
        if matches!(operand.as_ref(), Expr::BinOp(BinOp::Add, ..)))
    );
    assert_eq!(
        formatted,
        format_source(&formatted, &FormatConfig::default()).unwrap()
    );
}
