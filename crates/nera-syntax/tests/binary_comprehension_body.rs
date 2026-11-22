use nera_syntax::{ast::Expr, error::ParseError, parser::Parser};

fn parse(body: &str) -> (String, nera_syntax::parser::ParseResult) {
    let source = format!("-module(t).\nf(X) -> << {body} || _ <- [a] >>.\n");
    let result = Parser::new(&source).parse_module();
    (source, result)
}

// Each form is a syntax error in OTP's erl_parse, which takes one expression
// before `||`. Nera used to keep the value and drop the rest.
#[test]
fn a_body_written_as_a_segment_is_a_syntax_error() {
    for body in [
        "X:16",
        "X/binary",
        "X/utf8",
        "X:16/little",
        "X:8/integer-unit:1",
        "<<X>>:8",
        "(X):8",
        "1:8",
        "\"a\":8",
        "-1:8",
        "+1:8",
        "?S:8",
        "erlang:error(boom)",
        "lists:reverse(X)",
        "?MODULE:id(X)",
        "m:f",
        "X, X",
        "X:8, X",
        "<<X>>, <<X>>",
    ] {
        let (source, result) = parse(body);
        let [ParseError::UnexpectedToken { span, hint, .. }] = result.errors.as_slice() else {
            panic!("{body} should be one syntax error, got {:?}", result.errors);
        };
        assert_eq!(&source[span.start..span.end], body, "the error should mark the body");
        assert!(hint.is_some(), "{body} should say how to write it");
    }
}

#[test]
fn a_body_of_one_expression_keeps_its_value() {
    for (body, is_expected) in [
        ("X", (|e| matches!(e, Expr::Variable(..))) as fn(&Expr) -> bool),
        ("<<X:16>>", |e| matches!(e, Expr::Binary(segments, _) if segments[0].size.is_some())),
        ("<<X, 1>>", |e| matches!(e, Expr::Binary(segments, _) if segments.len() == 2)),
        ("<< <<Y>> || Y <- [X] >>", |e| matches!(e, Expr::BinComp(..))),
        ("(erlang:error(boom))", |e| matches!(e, Expr::Call(..))),
    ] {
        let (_, result) = parse(body);
        assert!(!result.has_errors(), "{body}: {:?}", result.errors);
        let Expr::BinComp(kept, ..) = &result.module.functions[0].clauses[0].body[0] else {
            panic!("{body} is not a binary comprehension");
        };
        assert!(is_expected(kept), "{body} parsed as {kept:?}");
    }
}
