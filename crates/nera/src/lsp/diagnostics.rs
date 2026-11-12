use std::path::Path;
use std::path::PathBuf;

use tower_lsp::lsp_types::{self, DiagnosticSeverity, DiagnosticTag, NumberOrString, Range, Position, Url};

use codespan_reporting::files::Files;

use nera_syntax::error::{Diagnostic as NeraDiagnostic, Severity as NeraSeverity};
use nera_syntax::MultiSourceMap;

use super::types::FileDiagnostic;

/// Convert a nera Diagnostic + MultiSourceMap into an LSP diagnostic.
pub(crate) fn convert_diagnostic(
    diag: &NeraDiagnostic,
    multi_sm: &MultiSourceMap,
    project_root: &Path,
) -> Option<FileDiagnostic> {
    let span = diag.span?;

    let file_id = multi_sm.file_id_for_offset(span.start);
    let seg_start = multi_sm.segment_start(file_id);
    let filename = multi_sm.segment_filename(file_id)?;

    // Compute segment-relative offset
    let rel_start = span.start - seg_start;
    let rel_end = span.end - seg_start;

    // Use codespan-reporting's Files trait for line/column conversion
    let start_line = multi_sm.line_index(file_id, rel_start).ok()?;
    let start_line_range = multi_sm.line_range(file_id, start_line).ok()?;
    let start_col = rel_start - start_line_range.start;

    let end_line = multi_sm.line_index(file_id, rel_end).ok()?;
    let end_line_range = multi_sm.line_range(file_id, end_line).ok()?;
    let end_col = rel_end - end_line_range.start;

    let range = Range {
        start: Position {
            line: start_line as u32,
            character: start_col as u32,
        },
        end: Position {
            line: end_line as u32,
            character: end_col as u32,
        },
    };

    let severity = match diag.severity {
        NeraSeverity::Error => Some(DiagnosticSeverity::ERROR),
        NeraSeverity::Warning => Some(DiagnosticSeverity::WARNING),
    };

    let mut message = match &diag.label {
        Some(label) if !label.is_empty() => format!("{}\n{}", diag.message, label),
        _ => diag.message.clone(),
    };
    if let Some(hint) = &diag.hint {
        message.push_str(&format!("\nhint: {}", hint));
    }

    // Resolve filename to a file URI
    let file_path = if Path::new(filename).is_absolute() {
        PathBuf::from(filename)
    } else {
        project_root.join(filename)
    };
    let uri = Url::from_file_path(&file_path).ok()?;

    // Tag diagnostics so editors can dim unnecessary/unreachable code.
    let tags = match diag.code.as_deref() {
        Some("unused_variable") | Some("unused_function") | Some("unreachable_clause") | Some("unused_macro") => {
            Some(vec![DiagnosticTag::UNNECESSARY])
        }
        Some("deprecated_function") => {
            Some(vec![DiagnosticTag::DEPRECATED])
        }
        _ => None,
    };

    Some(FileDiagnostic {
        uri,
        diagnostic: lsp_types::Diagnostic {
            range,
            severity,
            source: Some("nera".to_string()),
            message,
            code: diag.code.as_ref().map(|c| NumberOrString::String(c.clone())),
            tags,
            ..Default::default()
        },
    })
}
