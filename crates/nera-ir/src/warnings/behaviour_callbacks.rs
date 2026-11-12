//! Behaviour callback validation.
//!
//! Checks that modules declaring `-behaviour(Name).` implement the
//! required callback functions for known OTP behaviours.

use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;

/// Return the required callbacks for a known OTP behaviour, or `None` for
/// unknown (user-defined) behaviours.
fn required_callbacks(behaviour: &str) -> Option<&'static [(&'static str, u32)]> {
    match behaviour {
        "gen_server" => Some(&[("init", 1), ("handle_call", 3), ("handle_cast", 2)]),
        "gen_statem" => Some(&[("init", 1), ("callback_mode", 0)]),
        "supervisor" => Some(&[("init", 1)]),
        "application" => Some(&[("start", 2), ("stop", 1)]),
        "gen_event" => Some(&[("init", 1), ("handle_event", 2)]),
        _ => None,
    }
}

/// Check that modules declaring a `-behaviour(...)` attribute implement the
/// required callbacks.
pub(super) fn check_behaviour_callbacks(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    warnings: &mut Vec<Diagnostic>,
) {
    if module.behaviours.is_empty() {
        return;
    }

    // Build set of defined functions: (name_str, arity).
    let defined: HashSet<(&str, u32)> = module
        .functions
        .iter()
        .filter_map(|f| {
            let name = atom_names.get(&f.name)?;
            Some((name.as_str(), f.arity))
        })
        .collect();

    for behav in &module.behaviours {
        let behav_name = match atom_names.get(&behav.name) {
            Some(n) => n,
            None => continue,
        };

        let callbacks = match required_callbacks(behav_name) {
            Some(cbs) => cbs,
            None => continue, // unknown behaviour — skip
        };

        for &(cb_name, cb_arity) in callbacks {
            if !defined.contains(&(cb_name, cb_arity)) {
                warnings.push(
                    Diagnostic::warning_at(
                        format!(
                            "undefined callback function '{}/{}' (behaviour '{}')",
                            cb_name, cb_arity, behav_name,
                        ),
                        behav.span,
                    )
                    .with_code("missing_behaviour_callback")
                    .with_hint(format!(
                        "add function {}/{} to implement the '{}' behaviour",
                        cb_name, cb_arity, behav_name,
                    )),
                );
            }
        }
    }
}
