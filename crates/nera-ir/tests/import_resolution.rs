//! NERA-92 accepted OTP resolver contract: declarations, suppression, guards,
//! funs and optimized paths must all select the same language target.
use nera_ir::{
    bif_table,
    ir::{CallTarget, Instruction, Terminator},
    lower::IrBuilder,
    resolution::{Resolution, ResolutionPolicy},
    warnings,
};
use nera_syntax::{ast::AtomId, parser::Parser};
use std::collections::HashMap;

fn parse(source: &str) -> (nera_syntax::ast::Module, HashMap<AtomId, String>) {
    let mut parser = Parser::new(source);
    let result = parser.parse_module();
    assert!(!result.has_errors(), "{:?}", result.errors);
    let names = (0..1000)
        .filter_map(|n| {
            parser
                .resolve(AtomId(n))
                .map(|name| (AtomId(n), name.to_owned()))
        })
        .collect();
    (result.module, names)
}

fn lower(
    module: &nera_syntax::ast::Module,
    names: HashMap<AtomId, String>,
) -> Result<(nera_ir::ir::IrModule, HashMap<AtomId, String>), nera_ir::lower::LowerError> {
    let id = |name: &str, fallback| {
        names
            .iter()
            .find(|(_, n)| n.as_str() == name)
            .map(|(id, _)| *id)
            .unwrap_or(AtomId(fallback))
    };
    IrBuilder::lower_module(
        module,
        id("true", 9998),
        id("false", 9999),
        id("self", 9997),
        names,
        None,
    )
}

#[test]
fn catalogue_names_do_not_claim_native_availability() {
    for &(name, arity) in nera_ir::otp_auto_imports::OTP_AUTO_IMPORTS {
        assert!(bif_table::is_auto_import(name, arity));
        let source = format!("-module(p). f() -> fun {name}/{arity}.");
        let (module, names) = parse(&source);
        let id = names.iter().find(|(_, n)| n.as_str() == name).unwrap().0;
        assert_eq!(
            ResolutionPolicy::new(&module, &names).resolve(*id, arity),
            Resolution::AutoImport
        );
    }
    for (name, arity) in [("alias", 0), ("port_close", 1), ("is_record", 2)] {
        assert!(!bif_table::is_known_bif("erlang", name, arity));
    }
}

#[test]
fn extensions_require_prefixes_but_keep_native_support() {
    // The 13 signatures deliberately removed by the captain's otp-only choice.
    for (name, arity) in [
        ("make_tuple", 2),
        ("make_tuple", 3),
        ("append_element", 2),
        ("phash2", 1),
        ("phash2", 2),
        ("get_stacktrace", 0),
        ("monotonic_time", 0),
        ("monotonic_time", 1),
        ("system_time", 1),
        ("hibernate", 3),
        ("start_timer", 3),
        ("cancel_timer", 1),
        ("cancel_timer", 2),
    ] {
        assert!(!bif_table::is_auto_import(name, arity));
        assert!(bif_table::is_known_bif("erlang", name, arity));
    }
}

#[test]
fn suppression_reaches_calls_guards_funs_and_special_paths() {
    for option in [
        "no_auto_import",
        "{no_auto_import,[self/0,spawn/3,byte_size/1,bit_size/1,length/1,throw/1]}",
        "[debug_info,{no_auto_import,[self/0,spawn/3,byte_size/1,bit_size/1,length/1,throw/1]}]",
        "[debug_info,[no_auto_import]]",
    ] {
        for expr in [
            "self()",
            "spawn(p,f,[])",
            "byte_size(<<>>)",
            "bit_size(<<>>)",
            "length([])",
            "fun length/1",
            "throw(boom)",
        ] {
            let source = format!("-module(p). -compile({option}). f() -> {expr}.");
            let (module, names) = parse(&source);
            let diagnostics = warnings::analyze_module(&module, &names, None);
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.code.as_deref() == Some("undefined_function")),
                "{source}: {diagnostics:?}"
            );
            // Calls must remain local even if a consumer invokes lowering directly.
            if !expr.starts_with("fun ") {
                let (ir, _) = lower(&module, names).unwrap();
                assert!(
                    ir.functions[0].blocks.iter().any(|block| matches!(
                        block.terminator,
                        Terminator::TailCall(CallTarget::Local(_), _)
                    )),
                    "{source}: {ir:?}"
                );
                assert!(
                    !ir.functions[0]
                        .blocks
                        .iter()
                        .flat_map(|block| &block.instructions)
                        .any(|i| matches!(i, Instruction::GetSelf(_) | Instruction::Spawn(..)))
                );
            }
        }
    }
}

#[test]
fn module_context_controls_guards_and_clashes() {
    for (declarations, body, code, expected) in [
        (
            "",
            "f(X) when length(X) > 0 -> ok. length(_) -> 1.",
            "illegal_guard_local_call",
            true,
        ),
        (
            "-compile({no_auto_import,[length/1]}).",
            "f(X) when length(X) > 0 -> ok.",
            "illegal_guard_local_call",
            true,
        ),
        (
            "-compile(no_auto_import). -import(erlang,[length/1]).",
            "f(X) when length(X) > 0 -> ok.",
            "illegal_guard_local_call",
            false,
        ),
        (
            "-import(other,[length/1]).",
            "f(X) when length(X) > 0 -> ok.",
            "illegal_guard_local_call",
            true,
        ),
        ("", "f(X) -> length(X). length(_) -> 1.", "bif_clash", true),
        (
            "-compile({no_auto_import,[length/1]}).",
            "f(X) -> length(X). length(_) -> 1.",
            "bif_clash",
            false,
        ),
        (
            "-import(other,[float/1]).",
            "f(X) -> float(X).",
            "bif_clash",
            true,
        ),
        (
            "-compile(no_auto_import). -import(other,[float/1]).",
            "f(X) -> float(X).",
            "bif_clash",
            false,
        ),
        (
            "",
            "f() -> fun length/1. length(_) -> 1.",
            "bif_clash",
            true,
        ),
    ] {
        let source = format!("-module(p). {declarations} {body}");
        let (module, names) = parse(&source);
        let diagnostics = warnings::analyze_module(&module, &names, None);
        assert_eq!(
            diagnostics.iter().any(|d| d.code.as_deref() == Some(code)),
            expected,
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn unrelated_option_data_is_not_suppression() {
    let (module, names) = parse("-module(p). -compile({custom,[no_auto_import]}). f() -> self().");
    let id = *names.iter().find(|(_, n)| *n == "self").unwrap().0;
    assert_eq!(
        ResolutionPolicy::new(&module, &names).resolve(id, 0),
        Resolution::AutoImport
    );
}

#[test]
fn suppressed_deferred_sizes_do_not_take_the_binary_shortcut() {
    for name in ["byte_size", "bit_size"] {
        let (module, names) = parse(&format!(
            "-module(p). -compile({{no_auto_import,[{name}/1]}}). f(<<_:8,Rest/bits>>) -> {name}(Rest)."
        ));
        let id = *names.iter().find(|(_, n)| n.as_str() == name).unwrap().0;
        let (ir, _) = lower(&module, names).unwrap();
        assert!(
            ir.functions[0]
                .blocks
                .iter()
                .any(|block| matches!(block.terminator,
            Terminator::TailCall(CallTarget::Local(target), _) if target == id)),
            "{ir:?}"
        );
    }
}

#[test]
fn compiler_forms_are_not_fictitious_runtime_functions() {
    for expr in [
        "is_record({},r)",
        "erlang:is_record({},r,1)",
        "fun is_record/2",
        "fun erlang:is_record/3",
    ] {
        let (module, names) = parse(&format!("-module(p). f() -> {expr}."));
        assert!(
            lower(&module, names)
                .unwrap_err()
                .to_string()
                .contains("compiler-specific")
        );
    }
    // A genuinely local function with the same spelling still remains local.
    let (module, names) = parse(
        "-module(p). -compile(no_auto_import). f() -> is_record(a,b). is_record(_,_) -> local.",
    );
    lower(&module, names).unwrap();
}

#[test]
fn compile_options_accumulate_and_tuple_signatures_work() {
    let (module, names) = parse(
        "-module(p). -compile({no_auto_import,[{length,1}]}). -compile([{no_auto_import,[[self/0]]}]). f() -> {length([]),self()}. ",
    );
    let policy = ResolutionPolicy::new(&module, &names);
    for (name, arity) in [("length", 1), ("self", 0)] {
        let id = *names.iter().find(|(_, n)| n.as_str() == name).unwrap().0;
        assert_eq!(policy.resolve(id, arity), Resolution::Undefined);
    }
}

#[test]
fn missing_auto_import_reaches_remote_capability_validation() {
    let (module, names) = parse("-module(p). -export([f/0]). f() -> alias().");
    assert!(
        !warnings::analyze_module(&module, &names, None)
            .iter()
            .any(|d| d.code.as_deref() == Some("undefined_function"))
    );
    let (ir, names) = lower(&module, names).unwrap();
    let unresolved = nera_ir::validate::check_unresolved_calls(&[ir], &names);
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].target_module, "erlang");
    assert_eq!(unresolved[0].target_function, "alias");
    assert_eq!(unresolved[0].arity, 0);
}

#[test]
fn compiler_form_errors_reach_analysis_before_guard_lowering() {
    // NERA-94 F1: Unsupported from lowering used to become an always-false guard.
    for expr in [
        "is_record(X)",
        "is_record(X,r)",
        "is_record(X,r,2)",
        "erlang:is_record(X)",
        "erlang:is_record(X,r)",
        "erlang:is_record(X,r,2)",
        "record_info(fields,r)",
        "erlang:record_info(fields,r)",
    ] {
        for function in [
            format!("f(X) -> {expr}."),
            format!("f(X) when {expr} -> yes; f(_) -> no."),
            format!(
                "f(X) -> (fun(Y) when {} -> yes; (_) -> no end)(X).",
                expr.replace('X', "Y")
            ),
        ] {
            let (module, names) = parse(&format!("-module(p). -record(r,{{x}}). {function}"));
            let diagnostics = warnings::analyze_module(&module, &names, None);
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.code.as_deref() == Some("unsupported_compiler_form")
                        && d.severity == nera_syntax::error::Severity::Error),
                "{function}: {diagnostics:?}"
            );
        }
    }
    let (module, names) = parse(
        "-module(p). -compile(no_auto_import). -import(erlang,[is_record/2]). f(X) when is_record(X,r) -> yes; f(_) -> no.",
    );
    assert!(
        warnings::analyze_module(&module, &names, None)
            .iter()
            .any(|d| d.code.as_deref() == Some("unsupported_compiler_form"))
    );
    let (module, names) = parse(
        "-module(p). -compile(no_auto_import). f() -> is_record(a,b). is_record(_,_) -> local.",
    );
    assert!(
        !warnings::analyze_module(&module, &names, None)
            .iter()
            .any(|d| d.code.as_deref() == Some("unsupported_compiler_form"))
    );
}

#[test]
fn no_auto_import_ignores_non_signatures_like_otp() {
    // NERA-107 R3: invalid entries must not reject valid Erlang or suppress
    // another arity by truncating an out-of-range integer.
    let (module, names) = parse(
        "-module(p). -compile({no_auto_import,[ignored,42,{length,bad},{7,1},{length,1,extra},#{x=>1},{length,4294967297},{length,-1},[self/0]]}). f() -> {self(),length([])}.",
    );
    let policy = ResolutionPolicy::new(&module, &names);
    for (name, arity, expected) in [
        ("self", 0, Resolution::Undefined),
        ("length", 1, Resolution::AutoImport),
    ] {
        let id = *names.iter().find(|(_, n)| n.as_str() == name).unwrap().0;
        assert_eq!(policy.resolve(id, arity), expected);
    }
}

#[test]
fn editor_metadata_auto_imports_are_module_specific() {
    // NERA-107 R6: metadata must not expose maps:size/1 or a Nera-only
    // extension as an implicit Erlang name just because a native entry exists.
    for (module, function, arity, expected) in [
        ("erlang", "size", 1, true),
        ("maps", "size", 1, false),
        ("erlang", "monotonic_time", 0, false),
        ("erlang", "get_keys", 1, true),
    ] {
        let entry = bif_table::BIF_TABLE
            .iter()
            .find(|entry| {
                entry.module == module && entry.function == function && entry.arity == arity
            })
            .unwrap();
        assert_eq!(bif_table::is_auto_imported_entry(entry), expected);
    }
}
