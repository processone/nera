//! Exercise the public verifier on real source fixtures, including constructs
//! with shared branch results, loops, multiple binary results and exceptions.

use std::path::Path;

use nera_ir::{lower::IrBuilder, verify::verify_module};
use nera_syntax::{ast::AtomId, lexer::Lexer, parser::Parser, preproc::Preprocessor};

fn verify_fixture(relative_path: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative_path);
    let source = std::fs::read_to_string(&path).unwrap();
    let tokens = Lexer::lex_all(&source).unwrap();
    let mut preproc = Preprocessor::new(&source, &path);
    let tokens = preproc.process(tokens).unwrap();
    let combined = preproc.take_combined_source();
    let mut parser = Parser::with_tokens(&combined, tokens, Default::default());
    let parsed = parser.parse_module();
    assert!(!parsed.has_errors(), "{relative_path}: {:?}", parsed.errors);
    let mut names = parser.atom_names();
    let mut next_id = names.keys().map(|id| id.0).max().unwrap_or(0) + 1;
    let mut intern = |name: &str| {
        if let Some((&id, _)) = names.iter().find(|(_, value)| value.as_str() == name) {
            return id;
        }
        let id = AtomId(next_id);
        next_id += 1;
        names.insert(id, name.to_owned());
        id
    };
    let (true_id, false_id, self_id) = (intern("true"), intern("false"), intern("self"));
    let (module, _) =
        IrBuilder::lower_module(&parsed.module, true_id, false_id, self_id, names, None).unwrap();
    assert!(
        !module.functions.is_empty(),
        "fixture must exercise lowering"
    );
    let errors = verify_module(&module);
    assert!(errors.is_empty(), "{relative_path}: {errors:#?}");
}

macro_rules! fixtures {
    ($($name:ident => $path:literal),* $(,)?) => {
        $(#[test] fn $name() { verify_fixture($path); })*
    };
}

fixtures! {
    control_flow => "tests/control_flow.erl",
    short_circuit => "tests/bool_ops.erl",
    guards => "tests/guard_tricky.erl",
    binary_utf => "tests/binary_utf.erl",
    binary_rest => "tests/multi_clause_binary_rest.erl",
    map_update => "tests/map_exact_update.erl",
    list_comprehension => "tests/lc_nested.erl",
    generator_pattern => "tests/lc_generator_pattern.erl",
    closures => "tests/fun_nested.erl",
    recursive_closure => "tests/tco_closure_tail.erl",
    pattern_aliases => "tests/match_alias_chain.erl",
    exceptions => "tests/exception_basic.erl",
    try_of => "tests/try_of.erl",
    catch_expression => "tests/catch_expr.erl",
    receive => "tests/selective_receive.erl",
    receive_timeout => "tests/receive_after.erl",
    resume_exception => "tests/resume_exception.erl",
    tail_call_exception => "tests/tco_trampoline_exception.erl",
    include_record => "tests/include_record.erl",
    stdlib_lists => "erlang/stdlib/lists.erl",
    stdlib_gen_server => "erlang/stdlib/gen_server.erl",
}
