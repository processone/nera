use nera_fmt::{config::FormatConfig, format_source};
use nera_syntax::parser::Parser;
use proptest::prelude::*;
use std::fs;
use std::path::PathBuf;

/// Collect all .erl test fixtures as seed sources for proptest.
fn test_fixture_sources() -> Vec<(PathBuf, String)> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let tests_dir = PathBuf::from(manifest).join("../../tests");
    let mut sources = Vec::new();
    if let Ok(entries) = fs::read_dir(&tests_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "erl") {
                if let Ok(content) = fs::read_to_string(&path) {
                    // Only keep files that parse without errors.
                    let mut parser = Parser::new(&content);
                    let result = parser.parse_module();
                    if !result.has_errors() {
                        sources.push((path, content));
                    }
                }
            }
        }
    }
    assert!(!sources.is_empty(), "no .erl test fixtures found");
    sources
}

/// Strategy that picks a random test fixture source.
fn valid_erlang_source() -> impl Strategy<Value = String> {
    let sources = test_fixture_sources();
    let len = sources.len();
    (0..len).prop_map(move |i| sources[i].1.clone())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// format(format(x)) == format(x) for all valid sources.
    #[test]
    fn idempotent(src in valid_erlang_source()) {
        let config = FormatConfig::default();
        if let Ok(first) = format_source(&src, &config) {
            let second = format_source(&first, &config)
                .expect("formatting already-formatted code should not fail");
            prop_assert_eq!(first, second, "formatter is not idempotent");
        }
    }

    /// Formatting must preserve the AST structure — same number of functions.
    #[test]
    fn preserves_function_count(src in valid_erlang_source()) {
        let config = FormatConfig::default();
        if let Ok(formatted) = format_source(&src, &config) {
            let orig = Parser::new(&src).parse_module();
            let fmt = Parser::new(&formatted).parse_module();
            prop_assert!(
                !fmt.has_errors(),
                "formatted code has parse errors"
            );
            prop_assert_eq!(
                orig.module.functions.len(),
                fmt.module.functions.len(),
                "formatting changed function count"
            );
        }
    }

    /// Formatting must preserve exports.
    #[test]
    fn preserves_exports(src in valid_erlang_source()) {
        let config = FormatConfig::default();
        if let Ok(formatted) = format_source(&src, &config) {
            let orig = Parser::new(&src).parse_module();
            let fmt = Parser::new(&formatted).parse_module();
            if !fmt.has_errors() {
                prop_assert_eq!(
                    orig.module.exports.len(),
                    fmt.module.exports.len(),
                    "formatting changed export count"
                );
            }
        }
    }

    /// Formatted output must itself parse without errors.
    #[test]
    fn output_is_valid_erlang(src in valid_erlang_source()) {
        let config = FormatConfig::default();
        if let Ok(formatted) = format_source(&src, &config) {
            let result = Parser::new(&formatted).parse_module();
            prop_assert!(
                !result.has_errors(),
                "formatted output has parse errors: {:?}",
                result.errors
            );
        }
    }
}
