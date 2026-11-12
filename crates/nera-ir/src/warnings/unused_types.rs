//! Unused type detection.
//!
//! A type defined via `-type` or `-opaque` that is never referenced in the
//! module (not exported via `-export_type`, not referenced in any `-spec`,
//! `-callback`, or other `-type`/`-opaque` body) is dead code.

use std::collections::HashMap;

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;

/// Check for types that are defined but never used.
pub(super) fn check_unused_types(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    if module.type_defs.is_empty() {
        return;
    }

    // Collect exported type names.
    let exported: std::collections::HashSet<AtomId> = module
        .type_exports
        .iter()
        .map(|fa| fa.name)
        .collect();

    for td in &module.type_defs {
        if exported.contains(&td.name) {
            continue;
        }
        if module.type_refs.contains(&td.name) {
            continue;
        }
        let name = atom_names
            .get(&td.name)
            .map(|s| s.as_str())
            .unwrap_or("?");
        warnings.push(
            Diagnostic::warning_at(
                format!("type '{}/{}' is unused", name, td.arity),
                td.name_span,
            )
            .with_code("unused_type"),
        );
    }
}
