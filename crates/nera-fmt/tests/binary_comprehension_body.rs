use nera_fmt::{config::FormatConfig, format_source};
use nera_syntax::{ast::Expr, parser::Parser};
use std::mem::discriminant;

const HEADER: &str = "-module(t).\n-define(M, <<1>>).\n-record(r, {a}).\n";

fn comprehension(body: &str) -> String {
    format!("{HEADER}f(X) -> << {body} || _ <- [a] >>.\n")
}

fn parsed_body(source: &str) -> Expr {
    let parsed = Parser::new(source).parse_module();
    assert!(!parsed.has_errors(), "{:?}\n{source}", parsed.errors);
    let Expr::BinComp(body, ..) = &parsed.module.functions[0].clauses[0].body[0] else {
        panic!("lost the binary comprehension: {source}");
    };
    (**body).clone()
}

// Formats the body, then checks the output reads as the same kind of
// expression and that a second format changes nothing.
fn check(written: &str, printed: &str) {
    let source = comprehension(written);
    let config = FormatConfig::default();
    let formatted = format_source(&source, &config).unwrap();
    assert!(
        formatted.contains(&format!("<< {printed} ||")),
        "{written} should print as {printed}:\n{formatted}"
    );
    assert_eq!(
        discriminant(&parsed_body(&source)),
        discriminant(&parsed_body(&formatted)),
        "{written} changed meaning:\n{formatted}"
    );
    assert_eq!(formatted, format_source(&formatted, &config).unwrap());
}

#[test]
fn call_bodies_keep_their_parentheses() {
    // A bare remote call would read as a segment with a size, and OTP rejects
    // any bare call in this position.
    check("(erlang:error(boom))", "(erlang:error(boom))");
    check("(?MODULE:id(X))", "(?MODULE:id(X))");
    check("(id(1))", "(id(1))");
    check("id(1)", "(id(1))");
}

#[test]
fn bodies_otp_rejects_bare_get_parentheses() {
    for body in [
        "-1",
        "+1",
        "-1.5",
        "-123456789012345678901234567890",
        "-X",
        "not X",
        "X + 1",
        "X = 1",
        "self() ! X",
        "catch X",
        "#{a => X}",
        "#r{a = X}",
        "#r.a",
    ] {
        check(&format!("({body})"), &format!("({body})"));
    }
}

#[test]
fn bodies_valid_bare_keep_how_they_were_written() {
    for body in ["1", "1.5", "$a", "a", "\"s\"", "[]", "[X]", "{X}", "fun id/1", "?M"] {
        check(body, body);
        check(&format!("({body})"), &format!("({body})"));
    }
}

#[test]
fn binary_and_variable_bodies_stay_bare() {
    for body in ["X", "<<X>>", "<<X, 1>>", "<< <<Y>> || Y <- [X] >>"] {
        check(body, body);
        check(&format!("({body})"), body);
    }
}

#[test]
fn a_parenthesis_in_a_leading_comment_is_not_a_parenthesis() {
    let source = format!("{HEADER}f(X) -> << % note (\n    1 || _ <- [a] >>.\n");
    let formatted = format_source(&source, &FormatConfig::default()).unwrap();
    assert!(formatted.contains("1 ||") && !formatted.contains("(1)"), "{formatted}");
}
