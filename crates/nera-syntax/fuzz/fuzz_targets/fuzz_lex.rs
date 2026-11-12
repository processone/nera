#![no_main]
use libfuzzer_sys::fuzz_target;
use nera_syntax::lexer::Lexer;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        let _ = Lexer::lex_all(source);
    }
});
