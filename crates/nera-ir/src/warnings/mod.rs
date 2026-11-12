//! Compiler warning analysis.
//!
//! Runs as an AST-level pass before IR lowering to detect:
//! - Unused variables (bound in patterns but never referenced)
//! - Unreachable clauses (preceded by a guardless catch-all)
//! - Shadowed variables in nested scopes (fun clauses and list comprehensions)
//! - Unused functions (defined but neither exported nor called locally)
//! - Illegal guard expressions (non-guard-safe calls or constructs in when clauses)
//! - Duplicate function definitions (same name/arity defined more than once)
//! - Invalid binary segment type specifiers (e.g., float with wrong size, signedness on binary)
//! - Illegal map construction (`:=` in `#{}` instead of `=>`)
//! - Module name vs filename mismatch
//! - Missing behaviour callbacks (gen_server, gen_statem, supervisor, application, gen_event)
//! - Duplicate record definitions and duplicate fields within records
//! - Underscore variable reuse in patterns (`_X` already bound, likely meant `_`)
//! - Exported variables (bound in all branches of case/if/receive/try, used after)
//! - Unused records (defined but never referenced in the module)
//! - Unused types (defined but not exported or referenced in specs/types)
//!
//! Warnings are collected as `Diagnostic` values; some (like illegal guards)
//! are errors that block compilation.

mod ast_walkers;
mod behaviour_callbacks;
mod binary_segments;
mod deprecated;
mod duplicate_functions;
mod duplicate_records;
mod exported_vars;
mod format_strings;
mod illegal_guards;
mod illegal_map_construction;
mod match_underscore_var;
mod non_tail_recursion;
mod pragmas;
mod shadowed;
mod unbound_vars;
mod undefined_exports;
mod undefined_functions;
mod undefined_record_fields;
mod unreachable;
mod unsafe_vars;
mod unused_funcs;
pub mod unused_macros;
mod unused_records;
mod unused_types;
mod unused_vars;

use std::collections::HashMap;
use std::path::Path;

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;

// Re-export items used by `lower.rs` (via `crate::warnings::*` paths).
pub(crate) use ast_walkers::{collect_pattern_bindings, collect_pattern_refs, collect_expr_refs, collect_guard_refs};
pub(crate) use binary_segments::first_refused_segment_error;

// Re-export for cross-module unused record detection.
pub use unused_records::collect_used_record_names;

// Re-export for unused macro detection (called from the compilation pipeline).
pub use unused_macros::check_unused_macros;

// Re-export for LSP document symbol annotation.
pub use non_tail_recursion::is_recursive;

/// Analyze a module for compiler warnings.
///
/// `filename` is the source file path (if known). When provided, the module
/// name declared in `-module(Name)` is checked against the filename stem.
///
/// `source` is the optional source text; when provided it is used to
/// cross-validate the module name from the atom table against the text at
/// the `-module(Name)` declaration span, guarding against stale atom tables.
pub fn analyze_module(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    filename: Option<&str>,
) -> Vec<Diagnostic> {
    analyze_module_with_source(module, atom_names, filename, None)
}

/// Like [`analyze_module`] but accepts an optional source text for
/// cross-validation of the module name atom.
pub fn analyze_module_with_source(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    filename: Option<&str>,
    source: Option<&str>,
) -> Vec<Diagnostic> {
    let mut warnings = Vec::new();
    // Module-name vs filename mismatch check.
    if let Some(path) = filename {
        check_module_name_mismatch(module, atom_names, path, source, &mut warnings);
    }
    let policy = crate::resolution::ResolutionPolicy::new(module, atom_names);
    for import in &module.imports {
        for fun in &import.functions {
            if policy.auto_enabled(fun.name, fun.arity) {
                let name = atom_names.get(&fun.name).map(String::as_str).unwrap_or("?");
                warnings.push(Diagnostic::warning_at(
                    format!("import overrides auto-imported BIF {name}/{}", fun.arity), fun.span,
                ).with_code("bif_clash"));
            }
        }
    }
    for func in &module.functions {
        unused_vars::check_unused_variables_fn(func, atom_names, &mut warnings);
        unreachable::check_unreachable_clauses_fn(func, atom_names, &mut warnings);
        shadowed::check_shadowed_variables_fn(func, atom_names, &mut warnings);
        unbound_vars::check_unbound_variables_fn(func, atom_names, &mut warnings);
        unsafe_vars::check_unsafe_variables_fn(func, atom_names, &mut warnings);
        exported_vars::check_exported_variables_fn(func, atom_names, &mut warnings);
        illegal_guards::check_illegal_guards_fn(func, atom_names, &policy, &mut warnings);
        binary_segments::check_binary_segments_fn(func, &mut warnings);
        illegal_map_construction::check_illegal_map_construction_fn(func, &mut warnings);
        match_underscore_var::check_match_underscore_vars_fn(func, atom_names, &mut warnings);
        non_tail_recursion::check_non_tail_recursion_fn(func, atom_names, &mut warnings);
        format_strings::check_format_strings_fn(func, atom_names, &mut warnings);
    }
    unused_funcs::check_unused_functions(module, atom_names, &mut warnings);
    binary_segments::check_binary_segments_records(module, &mut warnings);
    unbound_vars::check_unbound_variables_records(module, atom_names, &mut warnings);
    unused_records::check_unused_records(module, atom_names, &mut warnings);
    unused_types::check_unused_types(module, atom_names, &mut warnings);
    undefined_exports::check_undefined_exports(module, atom_names, &mut warnings);
    undefined_functions::check_undefined_functions(module, atom_names, &policy, &mut warnings);
    duplicate_functions::check_duplicate_functions(module, atom_names, &mut warnings);
    duplicate_records::check_duplicate_records(module, atom_names, &mut warnings);
    undefined_record_fields::check_undefined_record_fields(module, atom_names, &mut warnings);
    deprecated::check_deprecated_calls(module, atom_names, &mut warnings);
    behaviour_callbacks::check_behaviour_callbacks(module, atom_names, &mut warnings);

    // Apply @nera:nowarn pragma suppression.
    if let Some(src) = source {
        let suppressed = pragmas::collect_function_pragmas(module, src);
        pragmas::filter_suppressed(&mut warnings, module, &suppressed);
    }

    warnings
}

/// Warn when `-module(Name)` does not match the source filename stem.
///
/// When `source` is provided, the module name from the atom table is
/// cross-validated against the text at `module.name_span`. If they disagree
/// (e.g. because the atom table is stale after a cache hit), the source text
/// is authoritative and the warning is suppressed.
fn check_module_name_mismatch(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    filepath: &str,
    source: Option<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let module_name = match atom_names.get(&module.name) {
        Some(name) => name,
        None => return,
    };
    // Skip the check for the default "undefined" placeholder (no -module() declaration).
    if module_name == "undefined" {
        return;
    }

    // Cross-validate: if source text is available and the span-derived name
    // disagrees with the atom table, the atom table is stale — bail out.
    if let Some(src) = source {
        let span = module.name_span;
        if span.start < span.end && span.end <= src.len() {
            let span_text = &src[span.start..span.end];
            // Strip quotes from quoted atoms
            let span_name = if span_text.starts_with('\'') && span_text.ends_with('\'') && span_text.len() >= 2 {
                &span_text[1..span_text.len() - 1]
            } else {
                span_text
            };
            if module_name != span_name {
                // Atom table disagrees with source — use source text instead.
                // This handles stale-interner scenarios gracefully.
                let file_stem = match Path::new(filepath).file_stem().and_then(|s| s.to_str()) {
                    Some(stem) => stem,
                    None => return,
                };
                if span_name != file_stem {
                    diagnostics.push(
                        Diagnostic::warning_at(
                            format!(
                                "module '{}' does not match filename '{}'",
                                span_name, file_stem,
                            ),
                            module.name_span,
                        )
                        .with_code("module_name_mismatch")
                        .with_hint(format!(
                            "rename the file to '{}.erl' or change -module({}) to -module({})",
                            span_name, span_name, file_stem,
                        )),
                    );
                }
                return;
            }
        }
    }

    let file_stem = match Path::new(filepath).file_stem().and_then(|s| s.to_str()) {
        Some(stem) => stem,
        None => return,
    };
    if module_name != file_stem {
        diagnostics.push(
            Diagnostic::warning_at(
                format!(
                    "module '{}' does not match filename '{}'",
                    module_name, file_stem,
                ),
                module.name_span,
            )
            .with_code("module_name_mismatch")
            .with_hint(format!(
                "rename the file to '{}.erl' or change -module({}) to -module({})",
                module_name, module_name, file_stem,
            )),
        );
    }
}

#[cfg(test)]
mod tests;
