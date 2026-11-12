//! Duplicate record and field detection.
//!
//! When the same record name is defined twice via separate `-record(...)` attributes,
//! the second silently shadows the first.  OTP emits `{redefine_record, T}`.
//! Similarly, duplicate field names within a single record definition are invalid;
//! OTP emits `{redefine_field, T, F}`.  Both are errors.

use std::collections::HashMap;

use nera_syntax::ast::{self, AtomId};
use nera_syntax::error::Diagnostic;
use nera_syntax::span::Span;

/// Check for duplicate record definitions and duplicate fields within records.
pub(super) fn check_duplicate_records(
    module: &ast::Module,
    atom_names: &HashMap<AtomId, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut seen_records: HashMap<AtomId, Span> = HashMap::new();

    for rec in &module.records {
        // Check for duplicate record name.
        if let Some(_first_span) = seen_records.get(&rec.name) {
            let rec_name = atom_names
                .get(&rec.name)
                .map(|s| s.as_str())
                .unwrap_or("?");
            diagnostics.push(
                Diagnostic::error_at(
                    format!("record '{}' already defined", rec_name),
                    rec.name_span,
                )
                .with_code("redefine_record")
                .with_hint("each record name must be defined only once"),
            );
        } else {
            seen_records.insert(rec.name, rec.name_span);
        }

        // Check for duplicate fields within this record.
        let mut seen_fields: HashMap<AtomId, Span> = HashMap::new();
        for field in &rec.fields {
            if let Some(_first_span) = seen_fields.get(&field.name) {
                let rec_name = atom_names
                    .get(&rec.name)
                    .map(|s| s.as_str())
                    .unwrap_or("?");
                let field_name = atom_names
                    .get(&field.name)
                    .map(|s| s.as_str())
                    .unwrap_or("?");
                diagnostics.push(
                    Diagnostic::error_at(
                        format!(
                            "field '{}' already defined in record '{}'",
                            field_name, rec_name,
                        ),
                        field.name_span,
                    )
                    .with_code("redefine_field")
                    .with_hint("each field name must be unique within a record"),
                );
            } else {
                seen_fields.insert(field.name, field.name_span);
            }
        }
    }
}
