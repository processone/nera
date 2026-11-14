//! Exercise the public verifier on real source fixtures, including constructs
//! with shared branch results, loops, multiple binary results and exceptions.

use std::path::Path;

use nera_ir::ir::*;
use nera_ir::{
    lower::IrBuilder,
    verify::{verify_function, verify_module, ErrorKind},
};
use nera_syntax::{ast::AtomId, lexer::Lexer, parser::Parser, preproc::Preprocessor};

fn lower_fixture_with_atoms(relative_path: &str) -> (IrModule, AtomId, AtomId) {
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
    (module, true_id, false_id)
}

fn lower_fixture(relative_path: &str) -> IrModule {
    lower_fixture_with_atoms(relative_path).0
}

fn verify_fixture(relative_path: &str) {
    lower_fixture(relative_path);
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

// The arithmetic IR contract requires site identities independent of results
// and verified failure boundaries, including generated functions.
// These cases mutate the actual compiler output of the owning runtime fixtures.
#[test]
fn arithmetic_contexts_survive_lowering_and_generated_helpers() {
    let mut contexts = Vec::new();
    for fixture in [
        "tests/try_operation_short_circuit.erl",
        "tests/try_after_reraise.erl",
    ] {
        let module = lower_fixture(fixture);
        for function in &module.functions {
            for inst in function.blocks.iter().flat_map(|b| &b.instructions) {
                if let Instruction::Arithmetic(site) = inst {
                    contexts.push(site.context);
                }
            }
        }
    }
    for expected in [
        EvaluationContext::Body,
        EvaluationContext::Try,
        EvaluationContext::Guard,
        EvaluationContext::Cleanup,
    ] {
        assert!(contexts.contains(&expected), "missing {expected:?}");
    }
}

#[test]
fn arithmetic_site_identity_does_not_depend_on_a_result_register() {
    let module = lower_fixture("tests/arith_basic.erl");
    let mut function = module
        .functions
        .into_iter()
        .find(|f| {
            f.blocks
                .iter()
                .flat_map(|b| &b.instructions)
                .any(|i| matches!(i, Instruction::Arithmetic(_)))
        })
        .unwrap();
    let block = function
        .blocks
        .iter_mut()
        .find(|b| {
            b.instructions
                .iter()
                .any(|i| matches!(i, Instruction::Arithmetic(_)))
        })
        .unwrap();
    let index = block
        .instructions
        .iter()
        .position(|i| matches!(i, Instruction::Arithmetic(_)))
        .unwrap();
    let Instruction::Arithmetic(mut repeated) = block.instructions[index].clone() else {
        unreachable!()
    };
    repeated.id = OperationId(u32::MAX);
    // Repeated writes to the same mutable register are legal.
    block
        .instructions
        .insert(index, Instruction::Arithmetic(repeated.clone()));
    // So is a raising operation with no result at all.
    repeated.id = OperationId(u32::MAX - 1);
    repeated.result = None;
    block
        .instructions
        .insert(index, Instruction::Arithmetic(repeated));
    assert!(verify_function(&function).is_empty());
    let block = function
        .blocks
        .iter_mut()
        .find(|b| {
            b.instructions
                .iter()
                .any(|i| matches!(i, Instruction::Arithmetic(_)))
        })
        .unwrap();
    let mut ids = block.instructions.iter_mut().filter_map(|i| match i {
        Instruction::Arithmetic(site) => Some(site),
        _ => None,
    });
    let first_id = ids.next().unwrap().id;
    ids.next().unwrap().id = first_id;
    assert!(verify_function(&function)
        .iter()
        .any(|e| e.kind == ErrorKind::DuplicateOperation(first_id)));
}

fn protected_arithmetic() -> IrFunction {
    lower_fixture("tests/try_operation_short_circuit.erl")
        .functions
        .into_iter()
        .find(|f| {
            f.blocks.iter().flat_map(|b| &b.instructions).any(|i| {
                matches!(
                    i,
                    Instruction::Arithmetic(ArithmeticSite {
                        context: EvaluationContext::Try,
                        ..
                    })
                )
            })
        })
        .unwrap()
}

#[test]
fn arithmetic_failure_requires_a_real_handling_destination() {
    let original = protected_arithmetic();
    for failure in [
        FailureDestination::Propagate,
        FailureDestination::Block(BlockId(u32::MAX)),
        FailureDestination::Block(original.entry_block),
    ] {
        let mut function = original.clone();
        let site = function
            .blocks
            .iter_mut()
            .flat_map(|b| &mut b.instructions)
            .find_map(|i| match i {
                Instruction::Arithmetic(site) => Some(site),
                _ => None,
            })
            .unwrap();
        site.failure = failure;
        let expected = match failure {
            FailureDestination::Propagate => ErrorKind::MissingFailureHandler,
            FailureDestination::Block(BlockId(u32::MAX)) => {
                ErrorKind::MissingSuccessor(BlockId(u32::MAX))
            }
            _ => ErrorKind::UnsafeFailurePath,
        };
        assert!(
            verify_function(&function)
                .iter()
                .any(|e| e.kind == expected),
            "{failure:?}"
        );
    }
}

#[test]
fn arithmetic_failure_cannot_initialize_its_result_in_the_handler() {
    let mut function = protected_arithmetic();
    let site = function
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .find_map(|i| match i {
            Instruction::Arithmetic(site) => Some(site.clone()),
            _ => None,
        })
        .unwrap();
    let FailureDestination::Block(mut target) = site.failure else {
        unreachable!()
    };
    loop {
        let block = function.blocks.iter_mut().find(|b| b.id == target).unwrap();
        if let Some(index) = block
            .instructions
            .iter()
            .position(|i| matches!(i, Instruction::ClearException))
        {
            let result = site.result.unwrap();
            block
                .instructions
                .insert(index + 1, Instruction::Move(result, result));
            assert!(verify_function(&function)
                .iter()
                .any(|e| e.kind == ErrorKind::UninitializedVariable(result)));
            break;
        }
        let Terminator::Jump(next, _) = block.terminator else {
            unreachable!()
        };
        target = next;
    }
}

#[test]
fn migrated_arithmetic_cannot_use_the_unrouted_representation() {
    let mut function = protected_arithmetic();
    let instruction = function
        .blocks
        .iter_mut()
        .flat_map(|b| &mut b.instructions)
        .find(|i| {
            matches!(
                i,
                Instruction::Arithmetic(ArithmeticSite {
                    operation: ArithmeticOp::Add(..),
                    ..
                })
            )
        })
        .unwrap();
    let Instruction::Arithmetic(site) = instruction else {
        unreachable!()
    };
    let ArithmeticOp::Add(a, b) = site.operation else {
        unreachable!()
    };
    *instruction = Instruction::BinOp(site.result.unwrap(), nera_syntax::ast::BinOp::Add, a, b);
    assert!(verify_function(&function)
        .iter()
        .any(|e| e.kind == ErrorKind::MissingOperationSite));
}

#[test]
fn arithmetic_failure_edges_survive_folding_and_cfg_pruning() {
    let (module, true_id, false_id) =
        lower_fixture_with_atoms("tests/try_operation_short_circuit.erl");
    let mut function = module
        .functions
        .into_iter()
        .find(|f| {
            f.blocks.iter().flat_map(|b| &b.instructions).any(|i| {
                matches!(
                    i,
                    Instruction::Arithmetic(ArithmeticSite {
                        context: EvaluationContext::Try,
                        ..
                    })
                )
            })
        })
        .unwrap();
    let block = function
        .blocks
        .iter_mut()
        .find(|b| {
            b.instructions
                .iter()
                .any(|i| matches!(i, Instruction::Arithmetic(_)))
        })
        .unwrap();
    let index = block
        .instructions
        .iter()
        .position(|i| matches!(i, Instruction::Arithmetic(_)))
        .unwrap();
    let Instruction::Arithmetic(site) = block.instructions[index].clone() else {
        unreachable!()
    };
    // Remove the redundant normal-CFG route to the catch block, leaving the
    // instruction's declared failure as the only edge to its landing pad.
    block.instructions.truncate(index + 1);
    block.terminator = Terminator::Return(site.result.unwrap());
    assert!(verify_function(&function).is_empty());
    nera_ir::passes::fold_constants(&mut function, true_id, false_id);
    assert!(verify_function(&function).is_empty());
    nera_ir::passes::eliminate_dead_code(&mut function, true_id, false_id);
    assert!(verify_function(&function).is_empty());
    let FailureDestination::Block(target) = site.failure else {
        unreachable!()
    };
    assert!(function.blocks.iter().any(|b| b.id == target));
    assert!(function.blocks.iter().flat_map(|b| &b.instructions).any(|i|
        matches!(i, Instruction::Arithmetic(kept) if kept.id == site.id && kept.failure == site.failure && kept.context == site.context)));
}
