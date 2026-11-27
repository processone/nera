use super::*;
use nera_ir::lower::IrBuilder;
use nera_syntax::parser::Parser;

#[test]
fn mime_slots_keep_parameters_and_cross_block_returns_distinct() {
    // The MIME allocation fix must share only local temporaries, not the
    // extension, caller-provided fallback, or result selected by another block.
    let source = r#"
        -module(mime_slots).
        -export([lookup/2]).
        lookup(Extension, Default) ->
            Mime = case Extension of
                <<"html">> -> {<<"text">>, <<"html">>, []};
                <<"png">> -> {<<"image">>, <<"png">>, []};
                _ -> Default
            end,
            Mime.
    "#;
    let mut parser = Parser::new(source);
    let parsed = parser.parse_module();
    assert!(!parsed.has_errors(), "{:?}", parsed.errors);
    let mut atoms = parser.atom_names();
    let lookup = *atoms.iter().find(|(_, name)| *name == "lookup").unwrap().0;
    let fresh = atoms.keys().map(|atom| atom.0).max().unwrap() + 1;
    for (offset, name) in ["true", "false", "self"].into_iter().enumerate() {
        atoms.insert(nera_syntax::ast::AtomId(fresh + offset as u32), name.into());
    }
    let (module, _) = IrBuilder::lower_module(
        &parsed.module,
        nera_syntax::ast::AtomId(fresh),
        nera_syntax::ast::AtomId(fresh + 1),
        nera_syntax::ast::AtomId(fresh + 2),
        atoms,
        None,
    )
    .unwrap();
    let function = module
        .functions
        .iter()
        .find(|function| function.name == lookup)
        .unwrap();
    let locals = block_local_variables(function);
    assert!(locals.iter().filter(|vars| !vars.is_empty()).count() >= 2);
    let pooled: HashSet<_> = locals.into_iter().flatten().collect();
    for param in &function.params {
        assert!(
            !pooled.contains(param),
            "parameter {param:?} cannot share a slot"
        );
    }
    let mut cross_block_returns = Vec::new();
    for block in &function.blocks {
        if let Terminator::Return(result) = block.terminator {
            let mut defs = HashSet::new();
            for inst in &block.instructions {
                collect_instruction_defs(inst, &mut defs);
            }
            if !defs.contains(&result) {
                cross_block_returns.push(result);
            }
        }
    }
    assert!(!cross_block_returns.is_empty());
    for result in cross_block_returns {
        assert!(
            !pooled.contains(&result),
            "cross-block return {result:?} cannot share a slot"
        );
    }
}
