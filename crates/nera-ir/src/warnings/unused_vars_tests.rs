//! Regressions for unused-variable diagnostics on real Erlang programs.

use nera_syntax::error::Severity;
use nera_syntax::parser::Parser;
use nera_syntax::span::Span;

#[test]
fn bound_catch_class_is_used_inside_fun() {
    // Intent: an outer name read only as a catch class inside a fun is used.
    // This fixture owns the requested runtime behavior natively and on BEAM;
    // this test owns its unused-variable diagnostics.
    let source = include_str!("../../../../tests/fun_catch_class.erl");
    let mut parser = Parser::new(source);
    let parsed = parser.parse_module();
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    let diagnostics = crate::warnings::analyze_module(&parsed.module, &parser.atom_names(), None);
    assert!(
        diagnostics.iter().all(|d| d.severity != Severity::Error),
        "unexpected errors: {diagnostics:#?}"
    );
    let unused: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("unused_variable"))
        .collect();
    assert!(unused.is_empty(), "unexpected unused variables: {unused:#?}");
}

#[test]
fn branch_exports_read_afterward_are_used() {
    // Intent: exported branch bindings read after case/if/receive are used,
    // as with erlc -Wall. Unread and fun-local bindings must still warn.
    let source = include_str!("../../../../tests/branch_export_warnings.erl");
    let mut parser = Parser::new(source);
    let parsed = parser.parse_module();
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    let diagnostics = crate::warnings::analyze_module(&parsed.module, &parser.atom_names(), None);
    assert!(
        diagnostics.iter().all(|d| d.severity != Severity::Error),
        "unexpected errors: {diagnostics:#?}"
    );
    let mut actual: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("unused_variable"))
        .map(|d| (d.message.clone(), d.span.expect("binding span").start))
        .collect();
    let mut expected: Vec<_> = [
        ("UnusedFilter", "UnusedFilter = Input"),
        ("LocalCode", "LocalCode = 200"),
        ("LocalCode", "LocalCode = 503"),
        ("UnusedPrimary", "UnusedPrimary = 200"),
        ("UnusedFallback", "UnusedFallback = 503"),
        ("UnusedIf", "UnusedIf = 200"),
        ("UnusedIf", "UnusedIf = 503"),
        ("UnusedResponse", "UnusedResponse = 200"),
        ("UnusedTimeout", "UnusedTimeout = 504"),
        ("ShadowedCode", "ShadowedCode = 200"),
        ("ShadowedCode", "ShadowedCode = 503"),
    ]
    .into_iter()
    .map(|(name, binding)| {
        (
            format!("variable '{name}' is unused"),
            source.find(binding).expect("control binding in fixture"),
        )
    })
    .collect();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected, "unexpected diagnostics: {diagnostics:#?}");
}

#[test]
fn nested_case_exports_read_afterward_are_used() {
    // The existing native fixture supplied the original false-warning report.
    let source = include_str!("../../../../tests/nested_case_exports.erl");
    let mut parser = Parser::new(source);
    let parsed = parser.parse_module();
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    let diagnostics = crate::warnings::analyze_module(&parsed.module, &parser.atom_names(), None);
    assert!(
        diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error && d.code.as_deref() != Some("unused_variable")),
        "unexpected diagnostics: {diagnostics:#?}"
    );
}

#[test]
fn pattern_expression_reads_preserve_unused_value_warnings() {
    // Intent: keys and sizes count as uses without treating value bindings as
    // reads. The shared HTTP-shaped fixture also runs natively and on BEAM.
    let source = include_str!("../../../../tests/pattern_expr_warnings.erl");
    let mut parser = Parser::new(source);
    let parsed = parser.parse_module();
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    let diagnostics = crate::warnings::analyze_module(&parsed.module, &parser.atom_names(), None);

    assert!(
        diagnostics.iter().all(|d| d.severity != Severity::Error),
        "unexpected errors: {diagnostics:#?}"
    );
    let actual: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("unused_variable"))
        .map(|diagnostic| {
            (
                diagnostic.severity,
                diagnostic.code.as_deref(),
                diagnostic.message.clone(),
                diagnostic.span,
            )
        })
        .collect();
    let expected: Vec<_> = [
        ("ShadowedKey", "ShadowedKey, ShadowedKey := Value"),
        ("UnusedArgument", "UnusedArgument"),
        ("UnusedBinaryValue", "UnusedBinaryValue"),
        ("UnusedMapValue", "UnusedMapValue"),
    ]
    .into_iter()
    .map(|(name, binding)| {
        let start = source.find(binding).expect("control binding in fixture");
        (
            Severity::Warning,
            Some("unused_variable"),
            format!("variable '{name}' is unused"),
            Some(Span::new(start, start + name.len())),
        )
    })
    .collect();
    assert_eq!(actual, expected, "unexpected diagnostics: {diagnostics:#?}");
}
