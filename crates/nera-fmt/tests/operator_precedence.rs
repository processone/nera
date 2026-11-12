use nera_fmt::{config::FormatConfig, format_source};

#[test]
fn preserves_unary_operand_grouping() {
    for expression in ["not (A < B)", "-(A + B)", "bnot (A bor B)", "-(A = B)"] {
        let source = format!("-module(test).\nf(A, B) -> {expression}.\n");
        let formatted = format_source(&source, &FormatConfig::default()).unwrap();
        assert!(formatted.contains(expression), "lost grouping: {formatted}");
        assert_eq!(
            formatted,
            format_source(&formatted, &FormatConfig::default()).unwrap()
        );
    }
}

#[test]
fn guard_not_fixture_is_idempotent() {
    let source = include_str!("../../../tests/guard_not.erl");
    let first = format_source(source, &FormatConfig::default()).unwrap();
    assert_eq!(
        first,
        format_source(&first, &FormatConfig::default()).unwrap()
    );
}

#[test]
fn preserves_right_nested_binary_grouping() {
    let source = "-module(test).\nf(A, B, C) -> A - (B - C).\n";
    let formatted = format_source(source, &FormatConfig::default()).unwrap();
    assert!(formatted.contains("A - (B - C)"), "{formatted}");
}

#[test]
fn nested_short_circuit_fixture_is_idempotent() {
    let source = include_str!("../../../tests/fun_parallel_scopes.erl");
    let first = format_source(source, &FormatConfig::default()).unwrap();
    assert_eq!(
        first,
        format_source(&first, &FormatConfig::default()).unwrap()
    );
}

#[test]
fn preserves_other_unary_operand_grouping() {
    for expression in ["-(A ! B)", "not (catch A)", "-(-A)", "-(-1)", "-(-1.5)"] {
        let source = format!("-module(test).\nf(A, B) -> {expression}.\n");
        let formatted = format_source(&source, &FormatConfig::default()).unwrap();
        assert!(formatted.contains(expression), "lost grouping: {formatted}");
        assert_eq!(
            formatted,
            format_source(&formatted, &FormatConfig::default()).unwrap()
        );
    }
}

// NERA-102: formatting newly accepted computed remote targets must not turn
// m:(name())(1) into a different call or discard operator grouping.
#[test]
fn preserves_computed_remote_targets() {
    use nera_syntax::{
        ast::{CallTarget, Expr},
        parser::Parser,
    };

    for expression in [
        "(module_name()):(function_name())(1)",
        "(M ++ N):(F ++ G)(1)",
    ] {
        let source = format!("-module(test). f(M, N, F, G) -> {expression}.");
        let formatted = format_source(&source, &FormatConfig::default()).unwrap();
        let mut parser = Parser::new(&formatted);
        let result = parser.parse_module();
        assert!(result.errors.is_empty(), "{:?}: {formatted}", result.errors);
        let Expr::Call(CallTarget::Remote(m, f), args, _) =
            &result.module.functions[0].clauses[0].body[0]
        else {
            panic!("lost remote call: {formatted}")
        };
        assert!(matches!(args.as_slice(), [Expr::Integer(1, _)]));
        for target in [m, f] {
            if expression.contains("++") {
                assert!(matches!(target.as_ref(), Expr::BinOp(_, _, _, _)));
            } else {
                assert!(
                    matches!(target.as_ref(), Expr::Call(CallTarget::Local(_, _), args, _) if args.is_empty())
                );
            }
        }
        assert_eq!(
            formatted,
            format_source(&formatted, &FormatConfig::default()).unwrap()
        );
    }
}

// NERA-102 F2: the captain chose bare argument-less module macro targets.
#[test]
fn keeps_argumentless_module_macro_targets_bare() {
    let source = "-module(macro_targets).\n\nf() -> ?MODULE:f().\n\ng() -> ?SERVER:call(request).\n";
    let formatted = format_source(source, &FormatConfig::default()).unwrap();
    assert_eq!(formatted, source);
    assert_eq!(
        format_source(&formatted, &FormatConfig::default()).unwrap(),
        source
    );
}

// NERA-102 G1: a function macro must not consume the remote call's arguments.
#[test]
fn keeps_function_macro_target_grouped() {
    use nera_syntax::{
        ast::{CallTarget, Expr},
        parser::Parser,
    };

    let source = "-module(function_macro).\n\nf(X) -> m:(?F)(X).\n";
    let formatted = format_source(source, &FormatConfig::default()).unwrap();
    assert_eq!(formatted, source);
    assert_eq!(
        format_source(&formatted, &FormatConfig::default()).unwrap(),
        source
    );
    let result = Parser::new(&formatted).parse_module();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(matches!(&result.module.functions[0].clauses[0].body[0],
        Expr::Call(CallTarget::Remote(_, function), args, _)
            if matches!(function.as_ref(), Expr::MacroCall { args: None, .. })
                && matches!(args.as_slice(), [Expr::Variable(_, _)])));
}
