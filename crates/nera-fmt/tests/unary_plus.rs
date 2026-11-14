use nera_fmt::{config::FormatConfig, format_source};
use nera_syntax::{
    ast::{BinOp, Expr, UnaryOp},
    parser::Parser,
};

// Newly accepted unary plus must not become list append or lose the grouping
// of a weaker-binding operand when a program is formatted.
#[test]
fn formatted_unary_plus_keeps_the_expression_tree() {
    let source = "-module(unary_plus).
        f(X) -> +(+X).
        g(X) -> +(X + 2) * 2.
        h(X) -> id((+X)), m:f((+X)), F((+X)), {(+X)}, ?IDENTITY((+X)), maybe (+X) end.";
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
    let body = &result.module.functions[2].clauses[0].body;
    for call in &body[..3] {
        assert!(matches!(call, Expr::Call(_, args, _)
            if matches!(args.as_slice(), [Expr::UnaryOp(UnaryOp::Pos, _, _)])));
    }
    assert!(matches!(&body[3], Expr::Tuple(elements, _)
        if matches!(elements.as_slice(), [Expr::UnaryOp(UnaryOp::Pos, _, _)])));
    assert!(matches!(&body[4], Expr::MacroCall { args: Some(args), .. }
        if matches!(args.as_slice(), [Expr::UnaryOp(UnaryOp::Pos, _, _)])));
    assert!(matches!(&body[5], Expr::Maybe { .. }));
    assert_eq!(
        formatted,
        format_source(&formatted, &FormatConfig::default()).unwrap()
    );
}

// Numeric normalization must keep the whole consumed spelling, including the
// closing parenthesis, in expressions and expression-derived match patterns.
#[test]
fn formatted_parenthesized_positive_literals_keep_complete_spans() {
    for literal in ["1", "9223372036854775808", "1.5"] {
        let source = format!(
            "-module(unary_plus). f() -> +({literal}).
             g() -> +({literal}) = id({literal}).
             h() -> <<+({literal})/float>>."
        );
        let formatted = format_source(&source, &FormatConfig::default()).unwrap();
        let result = Parser::new(&formatted).parse_module();
        assert!(result.errors.is_empty(), "{:?}: {formatted}", result.errors);
        assert!(matches!(
            &result.module.functions[0].clauses[0].body[0],
            Expr::Integer(..) | Expr::BigInteger(..) | Expr::Float(..)
        ));
        assert!(matches!(
            &result.module.functions[1].clauses[0].body[0],
            Expr::Match(..)
        ));
        assert_eq!(
            formatted,
            format_source(&formatted, &FormatConfig::default()).unwrap()
        );
    }
}

// Construction and pattern segments share size formatting; both must preserve
// the parentheses required around a signed size, including comprehension sites.
#[test]
fn formatted_positive_binary_sizes_keep_required_grouping() {
    let source = "-module(unary_plus).
        f() -> <<1:(+8)>>.
        g(<<X:(+8)>>) -> X.
        h() -> << <<X:(+8)>> || <<X:(+8)>> <= <<1, 2>> >>.";
    let formatted = format_source(source, &FormatConfig::default()).unwrap();
    let result = Parser::new(&formatted).parse_module();
    assert!(result.errors.is_empty(), "{:?}: {formatted}", result.errors);
    assert_eq!(result.module.functions.len(), 3);
    assert_eq!(
        formatted,
        format_source(&formatted, &FormatConfig::default()).unwrap()
    );
}

// A folded number may retain compound signs in its spelling. Such spellings
// are expressions, not bare binary values, even though the AST is a literal.
#[test]
fn formatted_compound_signed_binary_values_keep_grouping() {
    let source = "-module(unary_plus). f() -> <<(+-0.0)/float, (+-1):8, (+ +1):8>>.";
    let formatted = format_source(source, &FormatConfig::default()).unwrap();
    let result = Parser::new(&formatted).parse_module();
    assert!(result.errors.is_empty(), "{:?}: {formatted}", result.errors);
    let Expr::Binary(segments, _) = &result.module.functions[0].clauses[0].body[0] else {
        panic!("lost binary construction: {formatted}");
    };
    assert_eq!(segments.len(), 3);
    assert!(matches!(&segments[0].value, Expr::Float(value, _) if value.is_sign_negative()));
    assert!(matches!(&segments[1].value, Expr::Integer(-1, _)));
    assert!(matches!(&segments[2].value, Expr::Integer(1, _)));
    assert_eq!(
        formatted,
        format_source(&formatted, &FormatConfig::default()).unwrap()
    );
}
