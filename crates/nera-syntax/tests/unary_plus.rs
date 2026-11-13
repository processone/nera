use nera_syntax::{
    ast::{BinOp, Expr, Pattern, UnaryOp},
    parser::Parser,
};

// Unary plus must survive parsing as a checked operation and bind more tightly
// than multiplication/addition (the native/BEAM fixture checks its values).
#[test]
fn unary_plus_preserves_its_operand_and_precedence() {
    let source = "-module(unary_plus). f(X) -> +X * 2 + 1. g() -> +id(1).";
    let result = Parser::new(source).parse_module();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let Expr::BinOp(BinOp::Add, product, _, _) = &result.module.functions[0].clauses[0].body[0]
    else {
        panic!("expected outer addition");
    };
    let Expr::BinOp(BinOp::Mul, positive, _, _) = product.as_ref() else {
        panic!("expected multiplication inside addition");
    };
    assert!(
        matches!(positive.as_ref(), Expr::UnaryOp(UnaryOp::Pos, operand, _)
        if matches!(operand.as_ref(), Expr::Variable(..)))
    );
    assert!(matches!(&result.module.functions[1].clauses[0].body[0],
        Expr::UnaryOp(UnaryOp::Pos, operand, _)
            if matches!(operand.as_ref(), Expr::Call(_, args, _) if args.len() == 1)));
}

// Calls, tuple elements, macros, maybe bodies, binary segments and literal
// patterns use different entry points; all must admit the same prefix token.
#[test]
fn unary_plus_reaches_shared_expression_and_literal_entry_points() {
    let source = "-module(unary_plus).
        f(+1, +1.5, +9223372036854775808) ->
            id(+X), m:f(+X), F(+X), {+X}, ?IDENTITY(+X), maybe +X end,
            <<+X:16/integer>>, <<+1>>, +1 = 1.
        g() -> maybe +1 ?= 1, ok end.";
    let result = Parser::new(source).parse_module();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let clause = &result.module.functions[0].clauses[0];
    assert!(matches!(
        clause.patterns.as_slice(),
        [
            Pattern::Integer(1, _),
            Pattern::Float(_, _),
            Pattern::BigInteger(_, _)
        ]
    ));
    let Expr::Binary(segments, _) = &clause.body[6] else {
        panic!("expected binary construction");
    };
    assert!(
        matches!(&segments[0].value, Expr::UnaryOp(UnaryOp::Pos, operand, _)
        if matches!(operand.as_ref(), Expr::Variable(..)))
    );
    assert!(matches!(
        segments[0].size.as_deref(),
        Some(Expr::Integer(16, _))
    ));
    assert!(matches!(&clause.body[8], Expr::Match(pattern, _, _)
        if matches!(pattern.as_ref(), Pattern::Integer(1, _))));
}
