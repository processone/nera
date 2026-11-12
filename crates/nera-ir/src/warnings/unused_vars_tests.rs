//! Regression for false unused-variable warnings on map keys and binary sizes.

use nera_syntax::error::Severity;
use nera_syntax::parser::Parser;
use nera_syntax::span::Span;

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
