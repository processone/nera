use nera_syntax::{
    ast::{BinOp, Expr, UnaryOp},
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
