//! Compiler-generated module_info/1 metadata used by native test discovery.
use std::collections::HashSet;
use nera_syntax::ast::{AtomId, CallTarget, Expr, FunArity, Function, FunctionClause, Module, Pattern};
use string_interner::{DefaultStringInterner, Symbol};

pub(crate) fn add_exports_info(module: &mut Module, atoms: &mut DefaultStringInterner) -> Result<(), String> {
    let span = module.name_span;
    let mut intern = |name: &str| AtomId(atoms.get_or_intern(name).to_usize() as u32);
    let info = intern("module_info");
    if module.functions.iter().any(|f| f.name == info && f.arity == 1) {
        return Err("module_info/1 is reserved for compiler-generated metadata".into());
    }
    let exports = intern("exports");
    let module_key = intern("module");
    let error = intern("error");
    let erlang = intern("erlang");
    let badarg = intern("badarg");
    module.exports.push(FunArity { name: info, arity: 1, span });
    // As on BEAM, export_all lists every function, not only the declared ones.
    let defined = module.functions.iter().filter(|_| module.export_all).map(|f| (f.name, f.arity));
    let mut seen = HashSet::new();
    let entries: Vec<_> = module.exports.iter().map(|f| (f.name, f.arity)).chain(defined)
        .filter(|&key| seen.insert(key))
        .map(|(name, arity)| Expr::Tuple(vec![Expr::Atom(name, span), Expr::Integer(arity as i64, span)], span))
        .collect();
    let exports_value = entries.into_iter().rev().fold(Expr::Nil(span), |tail, head|
        Expr::Cons(Box::new(head), Box::new(tail), span));
    let clause = |pattern, value| FunctionClause {
        name_span: span, patterns: vec![pattern], guard: None, body: vec![value], span,
    };
    module.functions.push(Function {
        name: info, name_span: span, arity: 1, span,
        clauses: vec![
            clause(Pattern::Atom(exports, span), exports_value),
            clause(Pattern::Atom(module_key, span), Expr::Atom(module.name, span)),
            clause(Pattern::Wildcard(span), Expr::Call(CallTarget::Remote(Box::new(Expr::Atom(erlang, span)), Box::new(Expr::Atom(error, span))),
                vec![Expr::Atom(badarg, span)], span)),
        ],
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nera_syntax::parser::Parser;

    #[test]
    fn metadata_contains_exports_not_private_functions() {
        let mut p = Parser::new("-module(probe). -export([visible/0]). visible() -> ok. hidden() -> ok.");
        let mut module = p.parse_module().module;
        let mut atoms = p.into_interner();
        add_exports_info(&mut module, &mut atoms).unwrap();
        let generated = module.functions.last().unwrap();
        let Expr::Cons(first, rest, _) = &generated.clauses[0].body[0] else { panic!("missing export list") };
        let Expr::Tuple(first, _) = &**first else { panic!("missing export tuple") };
        let Expr::Atom(name, _) = first[0] else { panic!("missing exported name") };
        assert_eq!(atoms.resolve(string_interner::DefaultSymbol::try_from_usize(name.0 as usize).unwrap()), Some("visible"));
        let Expr::Cons(_, tail, _) = &**rest else { panic!("missing module_info export") };
        assert!(matches!(**tail, Expr::Nil(_)));
    }

    #[test]
    fn export_all_lists_every_function() {
        let mut p = Parser::new("-module(probe). -compile([export_all]). -export([visible/0]). visible() -> ok. hidden(X) -> X.");
        let mut module = p.parse_module().module;
        let mut atoms = p.into_interner();
        add_exports_info(&mut module, &mut atoms).unwrap();
        let mut listed = Vec::new();
        let mut list = &module.functions.last().unwrap().clauses[0].body[0];
        while let Expr::Cons(head, tail, _) = list {
            let Expr::Tuple(pair, _) = &**head else { panic!("missing export tuple") };
            let (Expr::Atom(name, _), Expr::Integer(arity, _)) = (&pair[0], &pair[1]) else { panic!("bad export entry") };
            listed.push((atoms.resolve(string_interner::DefaultSymbol::try_from_usize(name.0 as usize).unwrap()).unwrap().to_owned(), *arity));
            list = tail;
        }
        assert_eq!(listed, [("visible".to_owned(), 0), ("module_info".to_owned(), 1), ("hidden".to_owned(), 1)]);
    }

    #[test]
    fn reserved_metadata_function_is_rejected() {
        let mut p = Parser::new("-module(probe). module_info(X) -> X.");
        let mut m = p.parse_module().module;
        assert!(add_exports_info(&mut m, &mut p.into_interner()).is_err());
    }
}
