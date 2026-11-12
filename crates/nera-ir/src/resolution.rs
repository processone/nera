//! One per-module language policy. It says nothing about native availability.
use std::collections::{HashMap, HashSet};

use nera_syntax::ast::{AtomId, Module};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Local,
    Import(AtomId),
    AutoImport,
    Undefined,
}

#[derive(Default)]
pub struct ResolutionPolicy {
    locals: HashSet<(AtomId, u32)>,
    imports: HashMap<(AtomId, u32), AtomId>,
    auto_imports: HashSet<(AtomId, u32)>,
}

impl ResolutionPolicy {
    pub fn new(module: &Module, names: &HashMap<AtomId, String>) -> Self {
        let mut policy = Self {
            locals: module.functions.iter().map(|f| (f.name, f.arity)).collect(),
            imports: module
                .imports
                .iter()
                .flat_map(|i| {
                    i.functions
                        .iter()
                        .map(move |f| ((f.name, f.arity), i.module))
                })
                .collect(),
            auto_imports: HashSet::new(),
        };
        if !module.no_auto_import.all {
            let suppressed: HashSet<_> = module
                .no_auto_import
                .functions
                .iter()
                .map(|f| (f.name, f.arity))
                .collect();
            let atom_ids: HashMap<&str, AtomId> = names
                .iter()
                .map(|(&id, name)| (name.as_str(), id))
                .collect();
            for &(function, arity) in crate::otp_auto_imports::OTP_AUTO_IMPORTS {
                if let Some(&id) = atom_ids.get(function) {
                    if !suppressed.contains(&(id, arity)) {
                        policy.auto_imports.insert((id, arity));
                    }
                }
            }
        }
        policy
    }

    pub fn resolve(&self, name: AtomId, arity: u32) -> Resolution {
        let key = (name, arity);
        // Conflicting declarations are rejected separately before lowering.
        if let Some(&module) = self.imports.get(&key) {
            Resolution::Import(module)
        } else if self.locals.contains(&key) {
            Resolution::Local
        } else if self.auto_enabled(name, arity) {
            Resolution::AutoImport
        } else {
            Resolution::Undefined
        }
    }

    /// Classify a resolved language target, independently of its native support.
    /// Explicit remote targets use Import(module), just like explicit imports.
    pub fn is_compiler_form(
        target: Resolution,
        function: AtomId,
        arity: u32,
        names: &HashMap<AtomId, String>,
    ) -> bool {
        let name = names.get(&function).map(String::as_str).unwrap_or("");
        let targets_erlang = match target {
            Resolution::AutoImport => true,
            Resolution::Import(module) => names.get(&module).is_some_and(|name| name == "erlang"),
            Resolution::Undefined => name == "record_info",
            Resolution::Local => false,
        };
        targets_erlang && compiler_form(name, arity)
    }

    pub fn auto_enabled(&self, name: AtomId, arity: u32) -> bool {
        self.auto_imports.contains(&(name, arity))
    }
}

/// These forms need compiler expansion, not a fictitious native BIF entry.
fn compiler_form(name: &str, arity: u32) -> bool {
    matches!((name, arity), ("is_record", 1..=3) | ("record_info", 2))
}
