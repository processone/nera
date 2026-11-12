#![no_main]
use libfuzzer_sys::fuzz_target;
use nera_fmt::{config::FormatConfig, format_source};
use nera_syntax::parser::Parser;

fuzz_target!(|data: &[u8]| {
    let source = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Only fuzz inputs that parse without errors — the formatter is allowed
    // to reject unparseable source, so crashes on those are not interesting.
    let mut parser = Parser::new(source);
    let result = parser.parse_module();
    if result.has_errors() {
        return;
    }

    let config = FormatConfig::default();

    // Phase 1: format must not panic.
    let formatted = match format_source(source, &config) {
        Ok(f) => f,
        Err(_) => return,
    };

    // Phase 2: idempotency — format(format(x)) == format(x).
    let reformatted = format_source(&formatted, &config)
        .expect("formatting already-formatted code should not fail");
    assert_eq!(
        formatted, reformatted,
        "formatter is not idempotent"
    );
});
