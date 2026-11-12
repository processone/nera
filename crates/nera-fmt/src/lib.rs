pub mod config;
pub mod comment;
pub mod doc;
pub mod format;
pub(crate) mod spec_format;

use config::FormatConfig;

/// Format Erlang source code.
///
/// Operates on raw source (before preprocessing), preserving macros and
/// preprocessor directives verbatim. Functions and expressions are
/// formatted through the AST using a Wadler-Lindig document algebra.
pub fn format_source(source: &str, config: &FormatConfig) -> Result<String, String> {
    format::format_source_forms(source, config)
}

/// Check if source is already formatted.
/// Returns `Ok(true)` if no changes needed, `Ok(false)` if formatting would change the file.
pub fn check_format(source: &str, config: &FormatConfig) -> Result<bool, String> {
    let formatted = format_source(source, config)?;
    Ok(formatted == source)
}

/// Format a standalone Erlang term file (`.app` / `.app.src`).
pub fn format_term_source(source: &str, config: &FormatConfig) -> Result<String, String> {
    format::format_term_file(source, config)
}

/// Check if a term file is already formatted.
pub fn check_term_format(source: &str, config: &FormatConfig) -> Result<bool, String> {
    let formatted = format_term_source(source, config)?;
    Ok(formatted == source)
}
