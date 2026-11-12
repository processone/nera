//! Cross-module unused record and macro detection.
//!
//! Records and macros defined in `.hrl` header files get inlined into every
//! `.erl` module that includes them.  A record/macro may appear unused in one
//! module but used in another.  This module provides shared logic for both the
//! CLI (`check()`) and LSP (`check_for_lsp()`) paths to defer such warnings
//! and only emit them if no module uses the record/macro.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;
use nera_syntax::preproc::MacroLocation;
use nera_syntax::MultiSourceMap;

/// A deferred `unused_record` warning for a record defined in an included file.
pub(crate) struct DeferredRecordWarning {
    /// The original diagnostic.
    pub warning: Diagnostic,
    /// Index into the parsed_modules list, for resolving the source map.
    pub module_idx: usize,
    /// The record name string (for cross-module lookup).
    pub record_name: String,
}

/// Result of analyzing warnings for a single module with cross-module awareness.
pub(crate) struct ModuleWarningResult {
    /// Warnings to emit immediately (not deferred).
    pub immediate: Vec<Diagnostic>,
    /// Deferred .hrl record warnings that need cross-module resolution.
    pub deferred: Vec<DeferredRecordWarning>,
    /// Number of analysis errors (block compilation if > 0).
    pub error_count: usize,
    /// Number of emitted warnings (excludes deferred).
    pub warning_count: usize,
}

/// Check whether a module is from the standard library.
pub(crate) fn is_stdlib_module(
    display_path: &str,
    is_embedded: bool,
    stdlib_dir_canonical: Option<&Path>,
) -> bool {
    is_embedded
        || stdlib_dir_canonical
            .map(|sd| {
                PathBuf::from(display_path)
                    .canonicalize()
                    .ok()
                    .map(|cp| cp.starts_with(sd))
                    .unwrap_or(false)
            })
            .unwrap_or(false)
}

/// Collect record names used across all non-stdlib modules.
pub(crate) fn collect_global_record_usage(
    parsed_modules: &[(String, bool, ast::Module, MultiSourceMap)],
    atom_names: &HashMap<AtomId, String>,
    stdlib_dir_canonical: Option<&Path>,
) -> HashSet<String> {
    let mut used = HashSet::new();
    for (display_path, is_embedded, module, _) in parsed_modules {
        if !is_stdlib_module(display_path, *is_embedded, stdlib_dir_canonical) {
            used.extend(nera_ir::warnings::collect_used_record_names(module, atom_names));
        }
    }
    used
}

/// Analyze warnings for a single module, deferring `unused_record` warnings
/// for records that originate from included `.hrl` files.
///
/// `source` is optional; when provided, `analyze_module_with_source` is used
/// for additional cross-validation (the LSP path provides this).
pub(crate) fn analyze_module_warnings(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    display_path: &str,
    multi_sm: &MultiSourceMap,
    module_idx: usize,
    source: Option<&str>,
) -> ModuleWarningResult {
    let warnings = match source {
        Some(src) => nera_ir::warnings::analyze_module_with_source(
            module, atom_names, Some(display_path), Some(src),
        ),
        None => nera_ir::warnings::analyze_module(module, atom_names, Some(display_path)),
    };

    let mut immediate = Vec::new();
    let mut deferred = Vec::new();

    for w in &warnings {
        if w.code.as_deref() == Some("unused_record") {
            if let Some(ref span) = w.span {
                let file_id = multi_sm.file_id_for_offset(span.start);
                if file_id != 0 {
                    // Record is from an included file — defer.
                    let record_name = extract_record_name_from_warning(&w.message);
                    if !record_name.is_empty() {
                        deferred.push(DeferredRecordWarning {
                            warning: w.clone(),
                            module_idx,
                            record_name,
                        });
                    }
                    continue;
                }
            }
        }
        immediate.push(w.clone());
    }

    let error_count = immediate
        .iter()
        .filter(|w| w.severity == nera_syntax::error::Severity::Error)
        .count();
    let warning_count = immediate
        .iter()
        .filter(|w| w.severity == nera_syntax::error::Severity::Warning)
        .count();

    ModuleWarningResult {
        immediate,
        deferred,
        error_count,
        warning_count,
    }
}

/// Resolve deferred .hrl record warnings: only keep those for records not
/// used in any module across the project.
pub(crate) fn resolve_deferred_warnings<'a>(
    deferred: &'a [DeferredRecordWarning],
    globally_used_records: &HashSet<String>,
) -> Vec<&'a DeferredRecordWarning> {
    deferred
        .iter()
        .filter(|d| !globally_used_records.contains(&d.record_name))
        .collect()
}

/// Extract the record name from an "unused_record" warning message.
///
/// Expected format: "record 'name' is unused"
fn extract_record_name_from_warning(message: &str) -> String {
    message
        .strip_prefix("record '")
        .and_then(|s| s.strip_suffix("' is unused"))
        .unwrap_or("")
        .to_string()
}

// ── Cross-module unused macro detection ────────────────────────────

/// A deferred `unused_macro` warning for a macro defined in an included file.
pub(crate) struct DeferredMacroWarning {
    /// The original diagnostic.
    pub warning: Diagnostic,
    /// Index into the parsed_modules list, for resolving the source map.
    pub module_idx: usize,
    /// The macro name string (for cross-module lookup).
    pub macro_name: String,
}

/// Collect macro names used across all non-stdlib modules.
pub(crate) fn collect_global_macro_usage(
    module_macro_data: &[(HashSet<String>, bool, &str)],
    stdlib_dir_canonical: Option<&Path>,
) -> HashSet<String> {
    let mut used = HashSet::new();
    for (used_macros, is_embedded, display_path) in module_macro_data {
        if !is_stdlib_module(display_path, *is_embedded, stdlib_dir_canonical) {
            used.extend(used_macros.iter().cloned());
        }
    }
    used
}

/// Analyze macro warnings for a single module, deferring `unused_macro` warnings
/// for macros that originate from included `.hrl` files.
pub(crate) fn analyze_module_macro_warnings(
    macro_locations: &[MacroLocation],
    used_macros: &HashSet<String>,
    multi_sm: &MultiSourceMap,
    module_idx: usize,
) -> (Vec<Diagnostic>, Vec<DeferredMacroWarning>) {
    let mut raw_warnings = Vec::new();
    nera_ir::warnings::check_unused_macros(macro_locations, used_macros, &mut raw_warnings);

    let mut immediate = Vec::new();
    let mut deferred = Vec::new();

    for w in raw_warnings {
        if let Some(ref span) = w.span {
            let file_id = multi_sm.file_id_for_offset(span.start);
            if file_id != 0 {
                // Macro is from an included file — defer.
                let macro_name = extract_macro_name_from_warning(&w.message);
                if !macro_name.is_empty() {
                    deferred.push(DeferredMacroWarning {
                        warning: w,
                        module_idx,
                        macro_name,
                    });
                    continue;
                }
            }
        }
        immediate.push(w);
    }

    (immediate, deferred)
}

/// Resolve deferred .hrl macro warnings: only keep those for macros not
/// used in any module across the project.
pub(crate) fn resolve_deferred_macro_warnings<'a>(
    deferred: &'a [DeferredMacroWarning],
    globally_used_macros: &HashSet<String>,
) -> Vec<&'a DeferredMacroWarning> {
    deferred
        .iter()
        .filter(|d| !globally_used_macros.contains(&d.macro_name))
        .collect()
}

/// Extract the macro name from an "unused_macro" warning message.
///
/// Expected format: "macro 'NAME' is unused"
fn extract_macro_name_from_warning(message: &str) -> String {
    message
        .strip_prefix("macro '")
        .and_then(|s| s.strip_suffix("' is unused"))
        .unwrap_or("")
        .to_string()
}
