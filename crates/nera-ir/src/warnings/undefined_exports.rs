//! Undefined export detection.
//!
//! Checks that every function listed in `-export([...])` is actually
//! defined in the module.  Missing definitions cause `undef` errors at
//! runtime, so we report them as compilation errors (matching OTP behaviour).

use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;

/// Check that every exported function/arity is defined in the module.
pub(super) fn check_undefined_exports(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    let defined: HashSet<(AtomId, u32)> = module
        .functions
        .iter()
        .map(|f| (f.name, f.arity))
        .collect();

    for fa in &module.exports {
        if !defined.contains(&(fa.name, fa.arity)) {
            let func_name = atom_names
                .get(&fa.name)
                .map(|s| s.as_str())
                .unwrap_or("?");
            warnings.push(
                Diagnostic::error_at(
                    format!("function '{}/{}' is exported but not defined", func_name, fa.arity),
                    fa.span,
                )
                .with_code("undefined_export")
                .with_hint("exported functions must be defined in the module"),
            );
        }
    }
}
