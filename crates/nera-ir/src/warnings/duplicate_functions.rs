//! Duplicate function definition detection.
//!
//! When the same `name/arity` is defined twice as separate function definitions
//! (separated by `.`), the second silently shadows the first.  At runtime only
//! the last definition survives, causing silent data loss.  OTP emits
//! `{redefine_function, {F,A}}`; we do the same.

use std::collections::HashMap;

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;
use nera_syntax::span::Span;

/// Check for duplicate function definitions (same name/arity).
pub(super) fn check_duplicate_functions(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut seen: HashMap<(AtomId, u32), Span> = HashMap::new();

    for func in &module.functions {
        let key = (func.name, func.arity);
        if let Some(_first_span) = seen.get(&key) {
            let func_name = atom_names
                .get(&func.name)
                .map(|s| s.as_str())
                .unwrap_or("?");
            diagnostics.push(
                Diagnostic::error_at(
                    format!("function '{}/{}' already defined", func_name, func.arity),
                    func.name_span,
                )
                .with_code("duplicate_function")
                .with_hint("each function name/arity must be defined only once"),
            );
        } else {
            seen.insert(key, func.name_span);
        }
    }
}
