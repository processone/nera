//! Existing IR transformations and their ordered function pipeline.
//!
//! Generated lambdas and list-comprehension helpers retain their separate,
//! yield-only pipeline. See `docs/IR_PASSES.md` for the current boundaries.

mod constant_folding;
mod dead_code;
mod match_contexts;
mod tuple_send;
mod uses;
mod yield_checks;

pub use constant_folding::fold_constants;
pub use dead_code::eliminate_dead_code;
pub use match_contexts::eliminate_dead_match_contexts;
pub use tuple_send::fuse_make_tuple_send;
pub use yield_checks::insert_backedge_yield_checks;

use crate::ir::IrFunction;
use nera_syntax::ast::AtomId;

/// Run the existing source-function passes in lowering order.
///
/// Structural verification surrounds every pass in unit-test builds only.
/// Production behavior and the generated-helper pipeline are unchanged.
pub fn run_function_passes(func: &mut IrFunction, true_atom_id: AtomId, false_atom_id: AtomId) {
    #[cfg(test)]
    assert_valid(func, "before passes");
    insert_backedge_yield_checks(func);
    #[cfg(test)]
    assert_valid(func, "after yield insertion");
    fuse_make_tuple_send(func);
    #[cfg(test)]
    assert_valid(func, "after tuple/send fusion");
    eliminate_dead_match_contexts(func);
    #[cfg(test)]
    assert_valid(func, "after dead match contexts");
    fold_constants(func, true_atom_id, false_atom_id);
    #[cfg(test)]
    assert_valid(func, "after constant folding");
    eliminate_dead_code(func, true_atom_id, false_atom_id);
    #[cfg(test)]
    assert_valid(func, "after dead code");
}

#[cfg(test)]
pub(crate) fn assert_valid(func: &IrFunction, stage: &str) {
    let errors = crate::verify::verify_function(func);
    assert!(errors.is_empty(), "{stage}: {errors:#?}");
}

#[cfg(test)]
mod tests;
