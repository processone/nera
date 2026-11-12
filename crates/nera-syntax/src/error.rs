use codespan_reporting::diagnostic::{self as cs_diag, Label};
use codespan_reporting::term;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream, NoColor};

use crate::span::Span;
use crate::source_map::{SourceMap, MultiSourceMap};
use thiserror::Error;

/// Errors that can occur during parsing.
#[derive(Error, Debug, Clone)]
pub enum ParseError {
    #[error("unexpected token: expected {expected}, found {found}")]
    UnexpectedToken {
        expected: String,
        found: String,
        span: Span,
        hint: Option<String>,
    },

    #[error("unexpected end of file")]
    UnexpectedEof { position: usize },

    #[error("invalid integer literal")]
    InvalidInteger { span: Span },

    #[error("invalid float literal")]
    InvalidFloat { span: Span },

    #[error("lexer error")]
    LexerError { position: usize },

    #[error("undefined macro: {name}")]
    UndefinedMacro { name: String, span: Span },

    #[error("unclosed conditional: -ifdef/-ifndef without -endif")]
    UnclosedConditional { span: Span },

    #[error("unexpected -{directive} without matching -ifdef/-ifndef")]
    UnexpectedDirective { directive: String, span: Span },

    #[error("cannot read include file: {path}")]
    IncludeError { path: String, span: Span },

    #[error("macro recursion depth exceeded for {name}")]
    MacroRecursion { name: String, span: Span },

    #[error("reserved keyword '{keyword}' cannot be used as {context}; use '{keyword}' (quoted) instead")]
    ReservedKeyword {
        keyword: String,
        context: String,
        span: Span,
    },

    #[error("head mismatch: function '{function_name}' has arity {expected_arity} in first clause but {found_arity} here")]
    HeadMismatch {
        function_name: String,
        expected_arity: u32,
        found_arity: u32,
        span: Span,
    },
}

impl ParseError {
    /// Return the span associated with this error, if any.
    pub fn span(&self) -> Option<Span> {
        match self {
            ParseError::UnexpectedToken { span, .. } => Some(*span),
            ParseError::InvalidInteger { span } => Some(*span),
            ParseError::InvalidFloat { span } => Some(*span),
            ParseError::LexerError { position } => {
                Some(Span::new(*position, *position + 1))
            }
            ParseError::UnexpectedEof { position } => {
                Some(Span::new(*position, *position))
            }
            ParseError::UndefinedMacro { span, .. } => Some(*span),
            ParseError::UnclosedConditional { span } => Some(*span),
            ParseError::UnexpectedDirective { span, .. } => Some(*span),
            ParseError::IncludeError { span, .. } => Some(*span),
            ParseError::MacroRecursion { span, .. } => Some(*span),
            ParseError::ReservedKeyword { span, .. } => Some(*span),
            ParseError::HeadMismatch { span, .. } => Some(*span),
        }
    }

    /// Return a short error label (used under the caret line).
    pub fn label(&self) -> &str {
        match self {
            ParseError::UnexpectedToken { .. } => "unexpected token",
            ParseError::UnexpectedEof { .. } => "unexpected end of file",
            ParseError::InvalidInteger { .. } => "invalid integer literal",
            ParseError::InvalidFloat { .. } => "invalid float literal",
            ParseError::LexerError { .. } => "unrecognized token",
            ParseError::UndefinedMacro { .. } => "undefined macro",
            ParseError::UnclosedConditional { .. } => "unclosed conditional",
            ParseError::UnexpectedDirective { .. } => "unexpected directive",
            ParseError::IncludeError { .. } => "include error",
            ParseError::MacroRecursion { .. } => "macro recursion",
            ParseError::ReservedKeyword { .. } => "reserved keyword",
            ParseError::HeadMismatch { .. } => "arity mismatch",
        }
    }
}

/// Severity level for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
        }
    }
}

/// A compiler diagnostic with source context.
///
/// Combines an error message with file location and source snippet.
/// This is the primary type used for user-facing error output.
#[derive(Debug)]
#[derive(Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
    pub label: Option<String>,
    pub code: Option<String>,
    pub hint: Option<String>,
}

impl Diagnostic {
    /// Create an error diagnostic from a ParseError.
    pub fn from_parse_error(error: &ParseError) -> Self {
        let hint = match error {
            ParseError::UnexpectedToken { hint, .. } => hint.clone(),
            _ => None,
        };
        Self {
            severity: Severity::Error,
            message: error.to_string(),
            span: error.span(),
            label: Some(error.label().to_string()),
            code: None,
            hint,
        }
    }

    /// Create a generic error diagnostic.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            span: None,
            label: None,
            code: None,
            hint: None,
        }
    }

    /// Create an error diagnostic with a span.
    pub fn error_at(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            span: Some(span),
            label: None,
            code: None,
            hint: None,
        }
    }

    /// Set the label text shown under the underlined span.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Create a warning diagnostic with a span.
    pub fn warning_at(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            span: Some(span),
            label: None,
            code: None,
            hint: None,
        }
    }

    /// Set a machine-readable diagnostic code (e.g. "unused_variable").
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Attach a contextual hint that will be rendered as a note.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Convert to a codespan-reporting diagnostic.
    fn to_codespan(&self) -> cs_diag::Diagnostic<()> {
        let severity = match self.severity {
            Severity::Error => cs_diag::Severity::Error,
            Severity::Warning => cs_diag::Severity::Warning,
        };

        let mut diag = cs_diag::Diagnostic::new(severity).with_message(&self.message);

        if let Some(span) = self.span {
            let label_text = self.label.as_deref().unwrap_or("");
            let label = Label::primary((), span.start..span.end).with_message(label_text);
            diag = diag.with_labels(vec![label]);
        }

        if let Some(hint) = &self.hint {
            diag = diag.with_notes(vec![format!("hint: {}", hint)]);
        }

        diag
    }

    /// Format this diagnostic as a plain string (no colors).
    ///
    /// Used when errors must be propagated as strings through `map_err`.
    pub fn render(&self, source_map: &SourceMap) -> String {
        let cs_diag = self.to_codespan();
        let config = term::Config::default();
        let mut buf = NoColor::new(Vec::new());
        let _ = term::emit(&mut buf, &config, source_map, &cs_diag);
        String::from_utf8_lossy(buf.get_ref()).into_owned()
    }

    /// Emit this diagnostic to stderr with colors.
    pub fn emit(&self, source_map: &SourceMap) {
        let cs_diag = self.to_codespan();
        let config = term::Config::default();
        let mut writer = StandardStream::stderr(ColorChoice::Auto);
        let _ = term::emit(&mut writer, &config, source_map, &cs_diag);
    }

    /// Convert to a codespan-reporting diagnostic using a multi-file source map.
    ///
    /// The `FileId` is the segment index determined by looking up the span's
    /// start offset in the multi-source map. Span byte offsets are adjusted
    /// to be relative to the segment start (as required by codespan-reporting).
    fn to_codespan_multi(&self, source_map: &MultiSourceMap) -> cs_diag::Diagnostic<usize> {
        let severity = match self.severity {
            Severity::Error => cs_diag::Severity::Error,
            Severity::Warning => cs_diag::Severity::Warning,
        };

        let mut diag = cs_diag::Diagnostic::new(severity).with_message(&self.message);

        if let Some(span) = self.span {
            let file_id = source_map.file_id_for_offset(span.start);
            let seg_start = source_map.segment_start(file_id);
            let label_text = self.label.as_deref().unwrap_or("");
            let label = Label::primary(
                file_id,
                (span.start - seg_start)..(span.end - seg_start),
            )
            .with_message(label_text);
            diag = diag.with_labels(vec![label]);
        }

        if let Some(hint) = &self.hint {
            diag = diag.with_notes(vec![format!("hint: {}", hint)]);
        }

        diag
    }

    /// Format this diagnostic as a plain string using a multi-file source map.
    pub fn render_multi(&self, source_map: &MultiSourceMap) -> String {
        let cs_diag = self.to_codespan_multi(source_map);
        let config = term::Config::default();
        let mut buf = NoColor::new(Vec::new());
        let _ = term::emit(&mut buf, &config, source_map, &cs_diag);
        String::from_utf8_lossy(buf.get_ref()).into_owned()
    }

    /// Emit this diagnostic to stderr with colors using a multi-file source map.
    pub fn emit_multi(&self, source_map: &MultiSourceMap) {
        let cs_diag = self.to_codespan_multi(source_map);
        let config = term::Config::default();
        let mut writer = StandardStream::stderr(ColorChoice::Auto);
        let _ = term::emit(&mut writer, &config, source_map, &cs_diag);
    }
}

/// Emit a list of parse errors to stderr with colors, and return the count.
pub fn emit_parse_errors(errors: &[ParseError], source_map: &SourceMap) -> usize {
    let config = term::Config::default();
    let mut writer = StandardStream::stderr(ColorChoice::Auto);
    for error in errors {
        let diag = Diagnostic::from_parse_error(error);
        let cs_diag = diag.to_codespan();
        let _ = term::emit(&mut writer, &config, source_map, &cs_diag);
    }
    errors.len()
}

/// Emit a list of parse errors to stderr using a multi-file source map, and return the count.
pub fn emit_parse_errors_multi(errors: &[ParseError], source_map: &MultiSourceMap) -> usize {
    let config = term::Config::default();
    let mut writer = StandardStream::stderr(ColorChoice::Auto);
    for error in errors {
        let diag = Diagnostic::from_parse_error(error);
        let cs_diag = diag.to_codespan_multi(source_map);
        let _ = term::emit(&mut writer, &config, source_map, &cs_diag);
    }
    errors.len()
}

/// Format a list of parse errors into a single diagnostic string (no colors).
///
/// Used when errors must be propagated as strings through `map_err`.
pub fn format_parse_errors(errors: &[ParseError], source_map: &SourceMap) -> String {
    let config = term::Config::default();
    let mut buf = NoColor::new(Vec::new());
    for error in errors {
        let diag = Diagnostic::from_parse_error(error);
        let cs_diag = diag.to_codespan();
        let _ = term::emit(&mut buf, &config, source_map, &cs_diag);
    }

    let mut output = String::from_utf8_lossy(buf.get_ref()).into_owned();

    let count = errors.len();
    if count > 0 {
        output.push_str(&format!(
            "{} error{} emitted\n",
            count,
            if count == 1 { "" } else { "s" }
        ));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_error_span() {
        let err = ParseError::UnexpectedToken {
            expected: "','".into(),
            found: "')'".into(),
            span: Span::new(10, 11),
            hint: None,
        };
        assert_eq!(err.span(), Some(Span::new(10, 11)));
    }

    #[test]
    fn parse_error_eof_has_span() {
        let err = ParseError::UnexpectedEof { position: 100 };
        assert_eq!(err.span(), Some(Span::new(100, 100)));
    }

    #[test]
    fn lexer_error_creates_unit_span() {
        let err = ParseError::LexerError { position: 42 };
        assert_eq!(err.span(), Some(Span::new(42, 43)));
    }

    #[test]
    fn diagnostic_render_with_span() {
        let src = "-module(test).\nstart() -> ok\n";
        let sm = SourceMap::with_filename(src, "test.erl");

        let diag = Diagnostic::error_at(
            "unexpected token: expected '.', found 'EOF'",
            Span::new(28, 29),
        );

        let rendered = diag.render(&sm);
        assert!(rendered.contains("error"));
        assert!(rendered.contains("test.erl"));
        assert!(rendered.contains("unexpected token"));
    }

    #[test]
    fn diagnostic_render_without_span() {
        let src = "";
        let sm = SourceMap::with_filename(src, "test.erl");

        let diag = Diagnostic::error("unexpected end of file");
        let rendered = diag.render(&sm);
        assert!(rendered.contains("error"));
        assert!(rendered.contains("unexpected end of file"));
    }

    #[test]
    fn format_multiple_errors() {
        let src = "bad input\nmore bad\n";
        let sm = SourceMap::with_filename(src, "test.erl");

        let errors = vec![
            ParseError::LexerError { position: 0 },
            ParseError::LexerError { position: 10 },
        ];

        let formatted = format_parse_errors(&errors, &sm);
        assert!(formatted.contains("2 errors emitted"));
    }

    #[test]
    fn diagnostic_hint_renders_in_output() {
        let src = "f(X Y) -> ok.\n";
        let sm = SourceMap::with_filename(src, "test.erl");

        let diag = Diagnostic::error_at("unexpected token", Span::new(4, 5))
            .with_hint("missing comma between arguments?");
        let rendered = diag.render(&sm);
        assert!(rendered.contains("hint: missing comma between arguments?"),
            "rendered output should contain hint note, got:\n{}", rendered);
    }
}
