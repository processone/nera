use nera_syntax::parser::Parser;

#[test]
fn malformed_deprecated_attributes_report_errors_without_losing_next_function() {
    for attribute in [
        "-deprecated([{old,no_arity}]).",
        "-deprecated([{old,256}]).",
        "-deprecated([{old,-1}]).",
        "-deprecated([{old,1,42}]).",
        "-deprecated([old/1,]).",
    ] {
        let source = format!("-module(test). {attribute} following() -> ok.");
        let mut parser = Parser::new(&source);
        let result = parser.parse_module();
        assert!(!result.errors.is_empty(), "accepted {attribute}");
        assert_eq!(
            result.module.functions.len(),
            1,
            "lost following function: {attribute}"
        );
        let names = parser.atom_names();
        assert_eq!(names[&result.module.functions[0].name], "following");
    }
}
