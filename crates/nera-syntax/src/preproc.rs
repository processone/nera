//! Token-level Erlang preprocessor.
//!
//! Handles `-define`, `-undef`, `-ifdef/-ifndef/-else/-endif`,
//! `-include`, and `?MACRO` expansions. Operates on the token stream
//! produced by the lexer, before the parser sees it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::ParseError;
use crate::lexer::Lexer;
use crate::span::Span;
use crate::token::Token;

/// Location of a `-define(NAME, ...)` macro definition, tracked for go-to-definition.
/// The `combined_span` is in terms of the combined source so it can be resolved
/// via `resolve_span_to_location` just like any other span.
#[derive(Debug, Clone)]
pub struct MacroLocation {
    pub name: String,
    /// `None` for object-like macros, `Some(n)` for function-like macros with n parameters.
    pub arity: Option<u32>,
    pub combined_span: Span,
}

/// Describes a byte range in `combined_source` that belongs to a specific file.
#[derive(Debug, Clone)]
pub struct FileSegment {
    /// Start byte offset in `combined_source` (inclusive).
    pub start: usize,
    /// End byte offset in `combined_source` (exclusive).
    pub end: usize,
    /// The filename (display path) for this segment.
    pub filename: String,
}

/// Maximum macro expansion depth to prevent infinite recursion.
const MAX_EXPANSION_DEPTH: usize = 64;

/// A macro definition: either object-like or function-like.
///
/// Each variant stores the source text from which the body tokens were defined.
/// This is needed because body tokens carry spans referencing their definition
/// source (e.g., a header file), but expansion may happen in a different file
/// where `self.source` is different.
#[derive(Debug, Clone)]
enum MacroDef {
    /// Object-like: `-define(NAME, replacement tokens).`
    Object {
        body: Vec<(Token, Span)>,
        source: String,
    },
    /// Function-like: `-define(NAME(Arg1, Arg2), body tokens).`
    Function {
        params: Vec<String>,
        body: Vec<(Token, Span)>,
        source: String,
    },
}

/// State for conditional compilation (`-ifdef`/`-ifndef`/`-else`/`-endif`).
#[derive(Debug, Clone)]
struct CondState {
    /// Is the current branch active (should tokens be emitted)?
    active: bool,
    /// Have we seen `-else` for this level?
    seen_else: bool,
    /// Was the condition true for the if-branch?
    if_was_true: bool,
    /// Span of the opening `-ifdef`/`-ifndef` for error reporting.
    open_span: Span,
}

/// Callback resolving Erlang include paths.
pub type IncludeLibResolver = dyn Fn(&str) -> Option<PathBuf>;

// Constant guards needed by version-dependent dependency headers.
fn eval_condition(expr: &crate::ast::Expr, names: &HashMap<crate::ast::AtomId, String>) -> Option<bool> {
    use crate::ast::{BinOp, Expr, UnaryOp};
    match expr {
        Expr::Atom(id, _) => match names.get(id)?.as_str() {
            "true" => Some(true), "false" => Some(false), _ => None,
        },
        Expr::UnaryOp(UnaryOp::Not, inner, _) => Some(!eval_condition(inner, names)?),
        Expr::BinOp(op, a, b, _) => {
            match op {
                BinOp::Andalso => return Some(eval_condition(a, names)? && eval_condition(b, names)?),
                BinOp::Orelse => return Some(eval_condition(a, names)? || eval_condition(b, names)?),
                BinOp::And => return Some(eval_condition(a, names)? & eval_condition(b, names)?),
                BinOp::Or => return Some(eval_condition(a, names)? | eval_condition(b, names)?),
                BinOp::Xor => return Some(eval_condition(a, names)? ^ eval_condition(b, names)?),
                _ => {}
            }
            let (Expr::Integer(a, _), Expr::Integer(b, _)) = (&**a, &**b) else { return None; };
            Some(match op {
                BinOp::Eq | BinOp::ExactEq => a == b,
                BinOp::NotEq | BinOp::ExactNotEq => a != b,
                BinOp::Lt => a < b, BinOp::LtEq => a <= b,
                BinOp::Gt => a > b, BinOp::GtEq => a >= b,
                _ => return None,
            })
        },
        _ => None,
    }
}

/// Token-level Erlang preprocessor.
pub struct Preprocessor<'src> {
    /// Macro definitions keyed by name and arity (`None` for object macros).
    macros: HashMap<(String, Option<usize>), MacroDef>,
    /// Source text for extracting token slices.
    source: &'src str,
    /// Path of the current file (for `?FILE` and `-include` resolution).
    file_path: PathBuf,
    /// Module name and its span (tracked from `-module(name).` for `?MODULE`).
    module_name: Option<(String, Span)>,
    /// Conditional compilation stack.
    cond_stack: Vec<CondState>,
    /// Current macro expansion depth (for recursion detection).
    expansion_depth: usize,
    /// Additional directories to search for `-include` files (like `-I` in C).
    include_paths: Vec<PathBuf>,
    /// Combined source text: main source + all included file sources.
    /// Used to ensure all token spans are valid against a single source buffer.
    combined_source: String,
    /// Segment table mapping byte ranges in `combined_source` to filenames.
    segments: Vec<FileSegment>,
    /// Tracked macro definition locations for go-to-definition.
    macro_locations: Vec<MacroLocation>,
    /// Names of user-defined macros that were actually referenced (via `?M`,
    /// `-ifdef(M)`, or `-ifndef(M)`).  Used for unused-macro detection.
    used_macros: HashSet<String>,
    /// Optional resolver for `-include_lib("app/path")` directives.
    /// Takes the raw path (e.g. `"kernel/include/file.hrl"`) and returns
    /// the resolved absolute path, or `None` if the app is not found.
    include_lib_resolver: Option<std::rc::Rc<IncludeLibResolver>>,
}

impl<'src> Preprocessor<'src> {
    /// Create a new preprocessor for the given source text and file path.
    pub fn new(source: &'src str, file_path: &Path) -> Self {
        let display_path = file_path.display().to_string();
        Self {
            macros: HashMap::new(),
            source,
            file_path: file_path.to_path_buf(),
            module_name: None,
            cond_stack: Vec::new(),
            expansion_depth: 0,
            include_paths: Vec::new(),
            combined_source: source.to_string(),
            segments: vec![FileSegment {
                start: 0,
                end: source.len(),
                filename: display_path,
            }],
            macro_locations: Vec::new(),
            used_macros: HashSet::new(),
            include_lib_resolver: None,
        }
    }

    /// Supply the release of the selected external OTP installation.
    pub fn set_otp_release(&mut self, release: u32) {
        self.macros.insert(("OTP_RELEASE".into(), None), MacroDef::Object {
            body: vec![(Token::Integer(release.into()), Span { start: 0, end: 0 })],
            source: String::new(),
        });
    }

    /// Set additional directories to search for `-include` files.
    ///
    /// These paths are tried in order when the default resolution (relative
    /// to the current file) fails.  Analogous to the `-I` flag in C compilers.
    pub fn set_include_paths(&mut self, paths: Vec<PathBuf>) {
        self.include_paths = paths;
    }

    /// Set the resolver for `-include_lib` directives.
    ///
    /// The closure receives the raw path string (e.g. `"kernel/include/file.hrl"`)
    /// and should return the resolved absolute path, or `None` if the application
    /// cannot be found.
    pub fn set_include_lib_resolver(&mut self, resolver: Box<IncludeLibResolver>) {
        self.include_lib_resolver = Some(resolver.into());
    }

    /// Return the combined source text (main source + all included files).
    /// All token spans produced by `process()` are valid offsets into this string.
    pub fn take_combined_source(self) -> String {
        self.combined_source
    }

    /// Return the combined source text and file segments.
    /// Use the segments to build a `MultiSourceMap` for accurate error reporting
    /// across included files.
    pub fn take_combined_source_and_segments(self) -> (String, Vec<FileSegment>) {
        (self.combined_source, self.segments)
    }

    /// Return the combined source, file segments, macro definition locations,
    /// and the set of macro names that were actually referenced.
    pub fn take_all(self) -> (String, Vec<FileSegment>, Vec<MacroLocation>, HashSet<String>) {
        (self.combined_source, self.segments, self.macro_locations, self.used_macros)
    }

    /// Process a token stream, expanding macros and handling directives.
    ///
    /// Returns the processed token stream ready for the parser.
    pub fn process(
        &mut self,
        tokens: Vec<(Token, Span)>,
    ) -> Result<Vec<(Token, Span)>, Vec<ParseError>> {
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let mut pos = 0;

        while pos < tokens.len() {
            // Check if this is a directive: Minus followed by a directive atom
            if tokens[pos].0 == Token::Minus && pos + 1 < tokens.len() {
                let directive_name = self.token_text(&tokens[pos + 1]);
                match directive_name.as_deref() {
                    Some("define") => {
                        match self.parse_define(&tokens, &mut pos) {
                            Ok(()) => continue,
                            Err(e) => {
                                errors.push(e);
                                // Skip to next dot to recover
                                self.skip_to_dot(&tokens, &mut pos);
                                continue;
                            }
                        }
                    }
                    Some("undef") => {
                        match self.parse_undef(&tokens, &mut pos) {
                            Ok(()) => continue,
                            Err(e) => {
                                errors.push(e);
                                self.skip_to_dot(&tokens, &mut pos);
                                continue;
                            }
                        }
                    }
                    Some("elif") => {
                        errors.push(ParseError::UnexpectedDirective {
                            directive: "elif (unsupported)".into(), span: tokens[pos].1,
                        });
                        self.skip_to_dot(&tokens, &mut pos);
                        continue;
                    }
                    Some("if") => {
                        if let Err(e) = self.parse_if(&tokens, &mut pos) {
                            errors.push(e);
                            self.skip_to_dot(&tokens, &mut pos);
                        }
                        continue;
                    }
                    Some("ifdef") => {
                        match self.parse_ifdef(&tokens, &mut pos, false) {
                            Ok(()) => continue,
                            Err(e) => {
                                errors.push(e);
                                self.skip_to_dot(&tokens, &mut pos);
                                continue;
                            }
                        }
                    }
                    Some("ifndef") => {
                        match self.parse_ifdef(&tokens, &mut pos, true) {
                            Ok(()) => continue,
                            Err(e) => {
                                errors.push(e);
                                self.skip_to_dot(&tokens, &mut pos);
                                continue;
                            }
                        }
                    }
                    Some("else") => {
                        match self.parse_else(&tokens, &mut pos) {
                            Ok(()) => continue,
                            Err(e) => {
                                errors.push(e);
                                self.skip_to_dot(&tokens, &mut pos);
                                continue;
                            }
                        }
                    }
                    Some("endif") => {
                        match self.parse_endif(&tokens, &mut pos) {
                            Ok(()) => continue,
                            Err(e) => {
                                errors.push(e);
                                self.skip_to_dot(&tokens, &mut pos);
                                continue;
                            }
                        }
                    }
                    Some("include") => {
                        if self.is_active() {
                            match self.parse_include(&tokens, &mut pos) {
                                Ok(included_tokens) => {
                                    output.extend(included_tokens);
                                    continue;
                                }
                                Err(e) => {
                                    errors.push(e);
                                    self.skip_to_dot(&tokens, &mut pos);
                                    continue;
                                }
                            }
                        } else {
                            self.skip_to_dot(&tokens, &mut pos);
                            continue;
                        }
                    }
                    Some("include_lib") => {
                        if self.is_active() {
                            match self.parse_include_lib(&tokens, &mut pos) {
                                Ok(included_tokens) => {
                                    output.extend(included_tokens);
                                    continue;
                                }
                                Err(e) => {
                                    errors.push(e);
                                    self.skip_to_dot(&tokens, &mut pos);
                                    continue;
                                }
                            }
                        } else {
                            self.skip_to_dot(&tokens, &mut pos);
                            continue;
                        }
                    }
                    Some("module") if self.is_active() => {
                        // Track the module name for ?MODULE
                        self.track_module_name(&tokens, pos);
                        // Fall through — output the -module attribute normally
                    }
                    _ => {
                        // Not a preprocessor directive — fall through
                    }
                }
            }

            // Check if this is a macro invocation: ?NAME
            if tokens[pos].0 == Token::Question && self.is_active() {
                let src = self.source.to_string();
                match self.expand_macro(&tokens, &mut pos, &mut errors, &src) {
                    Ok(expanded) => {
                        output.extend(expanded);
                        continue;
                    }
                    Err(e) => {
                        errors.push(e);
                        pos += 1;
                        continue;
                    }
                }
            }

            // Regular token — output if we're in an active conditional branch
            if self.is_active() {
                output.push(tokens[pos].clone());
            }
            pos += 1;
        }

        // Check for unclosed conditionals
        if let Some(cond) = self.cond_stack.last() {
            errors.push(ParseError::UnclosedConditional {
                span: cond.open_span,
            });
        }

        if errors.is_empty() {
            Ok(output)
        } else {
            Err(errors)
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Check if we're in an active branch (all conditionals on the stack are active).
    fn is_active(&self) -> bool {
        self.cond_stack.iter().all(|c| c.active)
    }

    /// Get the text of a token by slicing the source.
    fn token_text(&self, token_span: &(Token, Span)) -> Option<String> {
        let (ref tok, ref span) = token_span;
        match tok {
            Token::Atom | Token::Variable | Token::QuotedAtom => {
                let text = &self.source[span.start..span.end];
                // Strip quotes from quoted atoms
                if matches!(tok, Token::QuotedAtom) {
                    Some(text[1..text.len() - 1].to_string())
                } else {
                    Some(text.to_string())
                }
            }
            // Keywords that might be used as macro names
            Token::KwModule => Some("module".to_string()),
            Token::KwExport => Some("export".to_string()),
            Token::KwImport => Some("import".to_string()),
            Token::KwElse => Some("else".to_string()),
            Token::KwIf => Some("if".to_string()),
            Token::KwMaybe => Some("maybe".to_string()),
            _ => None,
        }
    }

    /// Get the text of a token that could be a macro name (atom, variable, or keyword).
    fn macro_name_text(&self, token_span: &(Token, Span)) -> Option<String> {
        self.macro_name_text_in(token_span, self.source)
    }

    /// Get the text of a token that could be a macro name, using the given source text.
    ///
    /// This is needed because macro body tokens carry spans from their definition
    /// source (which may be a different file, e.g. an included header), not from
    /// `self.source`. When expanding nested macros inside a body, we must use the
    /// definition source to resolve span-based token text.
    ///
    /// After parameter substitution, a macro body contains a mix of tokens:
    /// body tokens (spans from definition source) and substituted arg tokens
    /// (spans from the caller's source, i.e. `self.source`). We try the
    /// preferred source first, validate the result looks like an identifier,
    /// and fall back to `self.source` if needed.
    fn macro_name_text_in(&self, token_span: &(Token, Span), source: &str) -> Option<String> {
        let (ref tok, ref span) = token_span;
        match tok {
            Token::Atom | Token::Variable => {
                // Try the preferred source first
                if let Some(text) = Self::extract_if_valid(span, source, tok) {
                    return Some(text);
                }
                // Fall back to self.source (for arg tokens substituted from caller)
                if let Some(text) = Self::extract_if_valid(span, self.source, tok) {
                    return Some(text);
                }
                None
            }
            // Keywords can be macro names (e.g., ?MODULE maps to KwModule internally
            // but macros are usually uppercase variables)
            Token::KwModule => Some("module".to_string()),
            Token::KwExport => Some("export".to_string()),
            _ => {
                // Any other keyword or token — try to get the text if it's a simple token
                if span.start < span.end && span.end <= source.len() {
                    let text = &source[span.start..span.end];
                    if !text.is_empty() {
                        return Some(text.to_string());
                    }
                }
                if span.start < span.end && span.end <= self.source.len() {
                    let text = &self.source[span.start..span.end];
                    if !text.is_empty() {
                        return Some(text.to_string());
                    }
                }
                None
            }
        }
    }

    /// Extract text from a source if the span is valid and the text matches
    /// the expected pattern for the token type.
    fn extract_if_valid(span: &Span, source: &str, tok: &Token) -> Option<String> {
        if span.start >= span.end || span.end > source.len() {
            return None;
        }
        let text = &source[span.start..span.end];
        // Validate: Atom must start with lowercase, Variable with uppercase or _
        let valid = match tok {
            Token::Atom => text.starts_with(|c: char| c.is_ascii_lowercase()),
            Token::Variable => text.starts_with(|c: char| c.is_ascii_uppercase() || c == '_'),
            _ => !text.is_empty(),
        };
        if valid {
            Some(text.to_string())
        } else {
            None
        }
    }

    /// Skip tokens until we hit a `Dot` (and consume it).
    fn skip_to_dot(&self, tokens: &[(Token, Span)], pos: &mut usize) {
        while *pos < tokens.len() {
            if tokens[*pos].0 == Token::Dot {
                *pos += 1;
                return;
            }
            *pos += 1;
        }
    }

    /// Compute line number (1-based) from a byte offset in the source.
    fn line_number(&self, offset: usize) -> i64 {
        let offset = offset.min(self.source.len());
        self.source[..offset].chars().filter(|&c| c == '\n').count() as i64 + 1
    }

    /// Track the module name from a `-module(name).` attribute.
    fn track_module_name(&mut self, tokens: &[(Token, Span)], minus_pos: usize) {
        // Pattern: Minus, KwModule, LParen, Atom/QuotedAtom, RParen, Dot
        let mut i = minus_pos + 1; // skip Minus
        if i >= tokens.len() {
            return;
        }
        i += 1; // skip 'module' keyword
        if i >= tokens.len() || tokens[i].0 != Token::LParen {
            return;
        }
        i += 1; // skip '('
        if i >= tokens.len() {
            return;
        }
        if let Some(name) = self.token_text(&tokens[i]) {
            let name_span = tokens[i].1;
            self.module_name = Some((name, name_span));
        }
    }

    // -----------------------------------------------------------------------
    // Directive parsing
    // -----------------------------------------------------------------------

    /// Parse `-define(NAME, Body).` or `-define(NAME(Args), Body).`
    fn parse_define(
        &mut self,
        tokens: &[(Token, Span)],
        pos: &mut usize,
    ) -> Result<(), ParseError> {
        let start_span = tokens[*pos].1;
        *pos += 1; // skip '-'
        *pos += 1; // skip 'define'

        // Expect '('
        if *pos >= tokens.len() || tokens[*pos].0 != Token::LParen {
            return Err(ParseError::UnexpectedToken {
                expected: "'(' after -define".into(),
                found: self.current_desc(tokens, *pos),
                span: self.current_span_or(tokens, *pos, start_span),
                hint: None,
            });
        }
        *pos += 1; // skip '('

        // Read macro name
        if *pos >= tokens.len() {
            return Err(ParseError::UnexpectedEof { position: tokens.last().map(|t| t.1.end).unwrap_or(0) });
        }
        let macro_name = self
            .macro_name_text(&tokens[*pos])
            .ok_or_else(|| ParseError::UnexpectedToken {
                expected: "macro name (atom or variable)".into(),
                found: self.current_desc(tokens, *pos),
                span: tokens[*pos].1,
                hint: None,
            })?;
        *pos += 1;

        // Check if function-like: next token is '(' immediately (no comma)
        let macro_def = if *pos < tokens.len() && tokens[*pos].0 == Token::LParen {
            // Function-like macro: -define(NAME(Arg1, Arg2), Body).
            *pos += 1; // skip '('
            let mut params = Vec::new();
            if *pos < tokens.len() && tokens[*pos].0 != Token::RParen {
                params.push(self.expect_variable_name(tokens, pos)?);
                while *pos < tokens.len() && tokens[*pos].0 == Token::Comma {
                    *pos += 1; // skip ','
                    params.push(self.expect_variable_name(tokens, pos)?);
                }
            }
            // Expect ')'
            if *pos >= tokens.len() || tokens[*pos].0 != Token::RParen {
                return Err(ParseError::UnexpectedToken {
                    expected: "')' closing macro parameters".into(),
                    found: self.current_desc(tokens, *pos),
                    span: self.current_span_or(tokens, *pos, start_span),
                    hint: None,
                });
            }
            *pos += 1; // skip ')'

            // Expect ','
            if *pos >= tokens.len() || tokens[*pos].0 != Token::Comma {
                return Err(ParseError::UnexpectedToken {
                    expected: "',' after macro parameters".into(),
                    found: self.current_desc(tokens, *pos),
                    span: self.current_span_or(tokens, *pos, start_span),
                    hint: None,
                });
            }
            *pos += 1; // skip ','

            // Read body tokens until ')' '.' with proper nesting
            let body = self.read_macro_body(tokens, pos)?;
            MacroDef::Function { params, body, source: self.source.to_string() }
        } else if *pos < tokens.len() && tokens[*pos].0 == Token::Comma {
            // Object-like macro with body: -define(NAME, Body).
            *pos += 1; // skip ','
            let body = self.read_macro_body(tokens, pos)?;
            MacroDef::Object { body, source: self.source.to_string() }
        } else if *pos < tokens.len() && tokens[*pos].0 == Token::RParen {
            // Object-like macro with no body: -define(NAME).
            *pos += 1; // skip ')'
            // Expect '.'
            if *pos < tokens.len() && tokens[*pos].0 == Token::Dot {
                *pos += 1; // skip '.'
            }
            MacroDef::Object { body: Vec::new(), source: self.source.to_string() }
        } else {
            return Err(ParseError::UnexpectedToken {
                expected: "',' or '(' or ')' after macro name".into(),
                found: self.current_desc(tokens, *pos),
                span: self.current_span_or(tokens, *pos, start_span),
                hint: None,
            });
        };

        // Only store the definition if we're in an active branch
        if self.is_active() {
            // Track the location of this macro definition for go-to-definition.
            // The span covers from the `-` of `-define` to the end of the last
            // consumed token (the `.` after `)`).
            let end_offset = if *pos > 0 { tokens[*pos - 1].1.end } else { start_span.end };
            let arity = match &macro_def {
                MacroDef::Function { params, .. } => Some(params.len() as u32),
                MacroDef::Object { .. } => None,
            };
            self.macro_locations.push(MacroLocation {
                name: macro_name.clone(),
                arity,
                combined_span: Span::new(start_span.start, end_offset),
            });
            self.macros.insert((macro_name, arity.map(|n| n as usize)), macro_def);
        }
        Ok(())
    }

    /// Read macro body tokens until the closing `)` `.` at depth 0.
    fn read_macro_body(
        &self,
        tokens: &[(Token, Span)],
        pos: &mut usize,
    ) -> Result<Vec<(Token, Span)>, ParseError> {
        let mut body = Vec::new();
        let mut paren_depth = 0;

        while *pos < tokens.len() {
            let tok = &tokens[*pos].0;

            if *tok == Token::RParen && paren_depth == 0 {
                // This closes the -define(...)
                *pos += 1; // skip ')'
                // Expect '.'
                if *pos < tokens.len() && tokens[*pos].0 == Token::Dot {
                    *pos += 1; // skip '.'
                }
                return Ok(body);
            }

            match tok {
                Token::LParen => paren_depth += 1,
                Token::RParen => paren_depth -= 1,
                _ => {}
            }

            body.push(tokens[*pos].clone());
            *pos += 1;
        }

        Err(ParseError::UnexpectedEof { position: tokens.last().map(|t| t.1.end).unwrap_or(0) })
    }

    /// Parse `-undef(NAME).`
    fn parse_undef(
        &mut self,
        tokens: &[(Token, Span)],
        pos: &mut usize,
    ) -> Result<(), ParseError> {
        let start_span = tokens[*pos].1;
        *pos += 1; // skip '-'
        *pos += 1; // skip 'undef'

        // Expect '('
        if *pos >= tokens.len() || tokens[*pos].0 != Token::LParen {
            return Err(ParseError::UnexpectedToken {
                expected: "'(' after -undef".into(),
                found: self.current_desc(tokens, *pos),
                span: self.current_span_or(tokens, *pos, start_span),
                hint: None,
            });
        }
        *pos += 1;

        // Read macro name
        if *pos >= tokens.len() {
            return Err(ParseError::UnexpectedEof { position: tokens.last().map(|t| t.1.end).unwrap_or(0) });
        }
        let macro_name = self
            .macro_name_text(&tokens[*pos])
            .ok_or_else(|| ParseError::UnexpectedToken {
                expected: "macro name".into(),
                found: self.current_desc(tokens, *pos),
                span: tokens[*pos].1,
                hint: None,
            })?;
        *pos += 1;

        // Expect ')' '.'
        if *pos >= tokens.len() || tokens[*pos].0 != Token::RParen {
            return Err(ParseError::UnexpectedToken {
                expected: "')'".into(),
                found: self.current_desc(tokens, *pos),
                span: self.current_span_or(tokens, *pos, start_span),
                hint: None,
            });
        }
        *pos += 1;
        if *pos < tokens.len() && tokens[*pos].0 == Token::Dot {
            *pos += 1;
        }

        if self.is_active() {
            self.macros.retain(|(name, _), _| name != &macro_name);
        }
        Ok(())
    }

    /// Evaluate a constant conditional using the normal expression parser.
    /// Unsupported expressions are rejected rather than selecting a wrong branch.
    fn parse_if(&mut self, tokens: &[(Token, Span)], pos: &mut usize) -> Result<(), ParseError> {
        let start = tokens[*pos].1;
        *pos += 2;
        let begin = *pos;
        self.skip_to_dot(tokens, pos);
        let end = *pos;
        let parent_active = self.is_active();
        let active = if parent_active {
            let mut expanded = Vec::new();
            let mut i = begin;
            while i < end {
                if tokens[i].0 == Token::Question {
                    let mut errors = Vec::new();
                    let source = self.source.to_string();
                    expanded.extend(self.expand_macro(tokens, &mut i, &mut errors, &source)?);
                    if let Some(error) = errors.into_iter().next() { return Err(error); }
                } else {
                    expanded.push(tokens[i].clone());
                    i += 1;
                }
            }
            let mut parser = crate::parser::Parser::with_tokens(
                &self.combined_source, expanded, Default::default());
            let expr = parser.parse_condition()?;
            let names = parser.atom_names();
            eval_condition(&expr, &names).ok_or(ParseError::UnexpectedToken {
                expected: "supported constant boolean expression in -if".into(),
                found: "unsupported conditional expression".into(), span: start, hint: None,
            })?
        } else { false };
        self.cond_stack.push(CondState {
            active: parent_active && active, seen_else: false,
            if_was_true: active, open_span: start,
        });
        Ok(())
    }

    /// Parse `-ifdef(NAME).` or `-ifndef(NAME).`
    fn parse_ifdef(
        &mut self,
        tokens: &[(Token, Span)],
        pos: &mut usize,
        negated: bool,
    ) -> Result<(), ParseError> {
        let start_span = tokens[*pos].1;
        *pos += 1; // skip '-'
        *pos += 1; // skip 'ifdef'/'ifndef'

        // Expect '('
        if *pos >= tokens.len() || tokens[*pos].0 != Token::LParen {
            return Err(ParseError::UnexpectedToken {
                expected: "'('".into(),
                found: self.current_desc(tokens, *pos),
                span: self.current_span_or(tokens, *pos, start_span),
                hint: None,
            });
        }
        *pos += 1;

        // Read macro name
        if *pos >= tokens.len() {
            return Err(ParseError::UnexpectedEof { position: tokens.last().map(|t| t.1.end).unwrap_or(0) });
        }
        let macro_name = self
            .macro_name_text(&tokens[*pos])
            .ok_or_else(|| ParseError::UnexpectedToken {
                expected: "macro name".into(),
                found: self.current_desc(tokens, *pos),
                span: tokens[*pos].1,
                hint: None,
            })?;
        *pos += 1;

        // Expect ')' '.'
        if *pos >= tokens.len() || tokens[*pos].0 != Token::RParen {
            return Err(ParseError::UnexpectedToken {
                expected: "')'".into(),
                found: self.current_desc(tokens, *pos),
                span: self.current_span_or(tokens, *pos, start_span),
                hint: None,
            });
        }
        *pos += 1;
        if *pos < tokens.len() && tokens[*pos].0 == Token::Dot {
            *pos += 1;
        }

        let is_defined = self.macros.keys().any(|(name, _)| name == &macro_name);
        let active = if negated { !is_defined } else { is_defined };

        // Track that this macro was referenced (for unused-macro detection).
        self.used_macros.insert(macro_name.clone());

        // If we're already in an inactive branch, this nested ifdef is also inactive
        let parent_active = self.is_active();

        self.cond_stack.push(CondState {
            active: parent_active && active,
            seen_else: false,
            if_was_true: active,
            open_span: start_span,
        });

        Ok(())
    }

    /// Parse `-else.`
    fn parse_else(
        &mut self,
        tokens: &[(Token, Span)],
        pos: &mut usize,
    ) -> Result<(), ParseError> {
        let start_span = tokens[*pos].1;
        *pos += 1; // skip '-'
        *pos += 1; // skip 'else'

        // Expect '.'
        if *pos < tokens.len() && tokens[*pos].0 == Token::Dot {
            *pos += 1;
        }

        let cond = self.cond_stack.last_mut().ok_or(ParseError::UnexpectedDirective {
            directive: "else".into(),
            span: start_span,
        })?;

        if cond.seen_else {
            return Err(ParseError::UnexpectedDirective {
                directive: "else (duplicate)".into(),
                span: start_span,
            });
        }

        cond.seen_else = true;

        // Check if parent conditionals are active
        let parent_active = if self.cond_stack.len() > 1 {
            self.cond_stack[..self.cond_stack.len() - 1]
                .iter()
                .all(|c| c.active)
        } else {
            true
        };

        // Flip: if the if-branch was true, else is inactive, and vice versa
        let cond = self.cond_stack.last_mut().unwrap();
        cond.active = parent_active && !cond.if_was_true;

        Ok(())
    }

    /// Parse `-endif.`
    fn parse_endif(
        &mut self,
        tokens: &[(Token, Span)],
        pos: &mut usize,
    ) -> Result<(), ParseError> {
        let start_span = tokens[*pos].1;
        *pos += 1; // skip '-'
        *pos += 1; // skip 'endif'

        // Expect '.'
        if *pos < tokens.len() && tokens[*pos].0 == Token::Dot {
            *pos += 1;
        }

        if self.cond_stack.pop().is_none() {
            return Err(ParseError::UnexpectedDirective {
                directive: "endif".into(),
                span: start_span,
            });
        }

        Ok(())
    }

    /// Shift all body token spans in a macro definition by `offset`.
    fn offset_macro_body_spans(def: &mut MacroDef, offset: usize) {
        let body = match def {
            MacroDef::Object { body, .. } | MacroDef::Function { body, .. } => body,
        };
        for (_, span) in body.iter_mut() {
            *span = Span::new(span.start + offset, span.end + offset);
        }
    }

    /// Parse the `("path").` portion of an include directive and return the
    /// raw path string and its span.  Works for both `-include` and `-include_lib`.
    fn parse_include_path(
        &self,
        tokens: &[(Token, Span)],
        pos: &mut usize,
        directive: &str,
    ) -> Result<(String, Span), ParseError> {
        let start_span = tokens[*pos].1;
        *pos += 1; // skip '-'
        *pos += 1; // skip 'include' or 'include_lib'

        // Expect '('
        if *pos >= tokens.len() || tokens[*pos].0 != Token::LParen {
            return Err(ParseError::UnexpectedToken {
                expected: format!("'(' after -{directive}"),
                found: self.current_desc(tokens, *pos),
                span: self.current_span_or(tokens, *pos, start_span),
                hint: None,
            });
        }
        *pos += 1;

        // Expect string literal with the file path
        if *pos >= tokens.len() || tokens[*pos].0 != Token::String {
            return Err(ParseError::UnexpectedToken {
                expected: "string literal (include path)".into(),
                found: self.current_desc(tokens, *pos),
                span: self.current_span_or(tokens, *pos, start_span),
                hint: None,
            });
        }
        let include_span = tokens[*pos].1;
        let raw_path = &self.source[include_span.start..include_span.end];
        // Strip quotes
        let include_path_str = raw_path[1..raw_path.len() - 1].to_string();
        *pos += 1;

        // Expect ')' '.'
        if *pos >= tokens.len() || tokens[*pos].0 != Token::RParen {
            return Err(ParseError::UnexpectedToken {
                expected: "')'".into(),
                found: self.current_desc(tokens, *pos),
                span: self.current_span_or(tokens, *pos, start_span),
                hint: None,
            });
        }
        *pos += 1;
        if *pos < tokens.len() && tokens[*pos].0 == Token::Dot {
            *pos += 1;
        }

        Ok((include_path_str, include_span))
    }

    /// Process an included file: read, lex, preprocess, and merge into the
    /// parent's combined source.  Shared by `-include` and `-include_lib`.
    fn process_included_file(
        &mut self,
        include_path: &Path,
        include_span: Span,
    ) -> Result<Vec<(Token, Span)>, ParseError> {
        // Read the included file
        let include_source = std::fs::read_to_string(include_path).map_err(|_| {
            ParseError::IncludeError {
                path: include_path.display().to_string(),
                span: include_span,
            }
        })?;

        // Lex the included file
        let include_tokens = Lexer::lex_all(&include_source).map_err(|errs| {
            errs.into_iter().next().unwrap_or(ParseError::IncludeError {
                path: include_path.display().to_string(),
                span: include_span,
            })
        })?;

        // Create a child preprocessor with the same macro table and include paths
        let parent_macro_names: HashSet<(String, Option<usize>)> = self.macros.keys().cloned().collect();
        let mut child = Preprocessor::new(&include_source, include_path);
        child.macros = self.macros.clone();
        child.module_name = self.module_name.clone();
        child.expansion_depth = self.expansion_depth;
        child.include_paths = self.include_paths.clone();
        child.include_lib_resolver = self.include_lib_resolver.clone();

        let result = child.process(include_tokens).map_err(|errs| {
            errs.into_iter().next().unwrap_or(ParseError::IncludeError {
                path: include_path.display().to_string(),
                span: include_span,
            })
        })?;

        // Adjust included token spans so they index into our combined source.
        // The child's combined_source contains the include file text (+ any
        // nested includes).  We append it to our combined_source and offset
        // every span by the insertion point.
        let span_offset = self.combined_source.len() + 1; // +1 for newline separator
        self.combined_source.push('\n');
        self.combined_source.push_str(&child.combined_source);

        // Incorporate the child's file segments with adjusted offsets.
        for mut seg in child.segments {
            seg.start += span_offset;
            seg.end += span_offset;
            self.segments.push(seg);
        }

        // Merge child's used macro names (for unused-macro detection).
        self.used_macros.extend(child.used_macros);

        // Merge child macro locations with adjusted spans.
        for mut loc in child.macro_locations {
            loc.combined_span = Span::new(
                loc.combined_span.start + span_offset,
                loc.combined_span.end + span_offset,
            );
            self.macro_locations.push(loc);
        }

        // Adjust macro body spans for macros newly defined by the child.
        // Inherited macros already have spans valid for the parent's
        // combined_source prefix, so they don't need adjustment.
        for (name, def) in child.macros.iter_mut() {
            if !parent_macro_names.contains(name) {
                Self::offset_macro_body_spans(def, span_offset);
                match def {
                    MacroDef::Object { source, .. }
                    | MacroDef::Function { source, .. } => {
                        *source = self.combined_source.clone();
                    }
                }
            }
        }
        self.macros = child.macros;

        let adjusted: Vec<(Token, Span)> = result
            .into_iter()
            .map(|(tok, span)| {
                (tok, Span::new(span.start + span_offset, span.end + span_offset))
            })
            .collect();

        Ok(adjusted)
    }

    /// Parse `-include("path").` and return the preprocessed tokens from the included file.
    fn parse_include(
        &mut self,
        tokens: &[(Token, Span)],
        pos: &mut usize,
    ) -> Result<Vec<(Token, Span)>, ParseError> {
        let (include_path_str, include_span) =
            self.parse_include_path(tokens, pos, "include")?;

        // Resolve the include path:
        // 1. Absolute paths are used as-is.
        // 2. Relative paths are tried against the parent directory of the
        //    current file first, then against each -I include path in order.
        let include_path = if Path::new(&include_path_str).is_absolute() {
            PathBuf::from(&include_path_str)
        } else {
            let parent = self.file_path.parent().unwrap_or(Path::new("."));
            let candidate = parent.join(&include_path_str);
            if candidate.exists() {
                candidate
            } else {
                // Try each include search path
                self.include_paths
                    .iter()
                    .map(|dir| dir.join(&include_path_str))
                    .find(|p| p.exists())
                    .unwrap_or(candidate) // fall back to original for error message
            }
        };

        self.process_included_file(&include_path, include_span)
    }

    /// Parse `-include_lib("app/path").` and return the preprocessed tokens.
    ///
    /// The first path component is resolved as an OTP application name via
    /// the `include_lib_resolver` closure.  Falls back to trying the path
    /// as a regular include if no resolver is set.
    fn parse_include_lib(
        &mut self,
        tokens: &[(Token, Span)],
        pos: &mut usize,
    ) -> Result<Vec<(Token, Span)>, ParseError> {
        let (include_path_str, include_span) =
            self.parse_include_path(tokens, pos, "include_lib")?;

        // Try the resolver first
        if let Some(ref resolver) = self.include_lib_resolver {
            if let Some(resolved) = resolver(&include_path_str) {
                return self.process_included_file(&resolved, include_span);
            }
        }

        // Fallback: try resolving as a regular include path.
        // This handles the case where the app directory is already on the
        // include search path (e.g. via --deps-dir), or when the resolver
        // is not configured.
        let parent = self.file_path.parent().unwrap_or(Path::new("."));
        let candidate = parent.join(&include_path_str);
        if candidate.exists() {
            return self.process_included_file(&candidate, include_span);
        }
        for dir in &self.include_paths {
            let p = dir.join(&include_path_str);
            if p.exists() {
                return self.process_included_file(&p, include_span);
            }
        }

        Err(ParseError::IncludeError {
            path: include_path_str,
            span: include_span,
        })
    }

    // -----------------------------------------------------------------------
    // Macro expansion
    // -----------------------------------------------------------------------

    /// Expand a `?MACRO` or `?MACRO(args)` invocation.
    ///
    /// `pos` points to the `?` token. On success, `pos` is advanced past the
    /// macro invocation and the expanded tokens are returned.
    ///
    /// `token_source` is the source text for resolving span-based token text
    /// from the input `tokens` (which may come from a macro body defined in a
    /// different file).
    fn expand_macro(
        &mut self,
        tokens: &[(Token, Span)],
        pos: &mut usize,
        errors: &mut Vec<ParseError>,
        token_source: &str,
    ) -> Result<Vec<(Token, Span)>, ParseError> {
        let question_span = tokens[*pos].1;
        *pos += 1; // skip '?'

        if *pos >= tokens.len() {
            return Err(ParseError::UnexpectedEof { position: tokens.last().map(|t| t.1.end).unwrap_or(0) });
        }

        let name_span = tokens[*pos].1;
        let macro_name = self
            .macro_name_text_in(&tokens[*pos], token_source)
            .ok_or_else(|| ParseError::UnexpectedToken {
                expected: "macro name after '?'".into(),
                found: self.current_desc(tokens, *pos),
                span: tokens[*pos].1,
                hint: None,
            })?;
        *pos += 1; // skip the macro name

        let combined_span = question_span.merge(name_span);

        // Handle predefined macros
        match macro_name.as_str() {
            "MODULE" | "MODULE_STRING" => {
                if let Some((ref _mod_name, mod_span)) = self.module_name {
                    if macro_name == "MODULE_STRING" {
                        // Return as a string token — use combined_span (parser handles it)
                        return Ok(vec![(Token::String, combined_span)]);
                    }
                    // Return as an atom token with the span of the module name
                    // in the -module(name) declaration, so the parser slices the
                    // correct text.
                    return Ok(vec![(Token::Atom, mod_span)]);
                }
                // MODULE not set yet — output as an atom (the parser will handle it)
                return Ok(vec![(Token::Atom, combined_span)]);
            }
            "LINE" => {
                let line = self.line_number(question_span.start);
                return Ok(vec![(Token::Integer(line), combined_span)]);
            }
            "FILE" => {
                return Ok(vec![(Token::String, combined_span)]);
            }
            "FUNCTION_NAME" => {
                return Ok(vec![(Token::MacroFunctionName, combined_span)]);
            }
            "FUNCTION_ARITY" => {
                return Ok(vec![(Token::MacroFunctionArity, combined_span)]);
            }
            _ => {}
        }

        // Look up user-defined macro
        let mut args = Vec::new();
        let arity = if self.macros.contains_key(&(macro_name.clone(), None)) {
            None
        } else if *pos < tokens.len() && tokens[*pos].0 == Token::LParen {
            *pos += 1;
            args = self.parse_macro_args(tokens, pos)?;
            Some(args.len())
        } else {
            None
        };
        let macro_def = self.macros.get(&(macro_name.clone(), arity)).cloned().ok_or_else(|| {
            ParseError::UndefinedMacro {
                name: macro_name.clone(),
                span: combined_span,
            }
        })?;

        // Track that this macro was referenced (for unused-macro detection).
        self.used_macros.insert(macro_name.clone());

        match macro_def {
            MacroDef::Object { body, ref source } => {
                // Recursively expand the body using the macro's definition source
                if self.expansion_depth >= MAX_EXPANSION_DEPTH {
                    return Err(ParseError::MacroRecursion {
                        name: macro_name,
                        span: combined_span,
                    });
                }
                self.expansion_depth += 1;
                let expanded = self.expand_tokens_with_source(body, errors, source)?;
                self.expansion_depth -= 1;
                Ok(expanded)
            }
            MacroDef::Function { params, body, ref source } => {
                // Substitute parameters in body (use macro's definition source)
                let substituted = self.substitute_params(&body, &params, &args, source, token_source);

                // Recursively expand using the macro's definition source.
                // Note: after substitution, arg tokens have spans from the caller
                // while body tokens have spans from the definition source. Since
                // `?MACRO` tokens in the body always consist of `?` + name from
                // the same source (body or arg), and args are typically not `?MACRO`
                // invocations, this is correct for the common case.
                if self.expansion_depth >= MAX_EXPANSION_DEPTH {
                    return Err(ParseError::MacroRecursion {
                        name: macro_name,
                        span: combined_span,
                    });
                }
                let def_source = source.clone();
                self.expansion_depth += 1;
                let expanded = self.expand_tokens_with_source(substituted, errors, &def_source)?;
                self.expansion_depth -= 1;
                Ok(expanded)
            }
        }
    }

    /// Parse macro arguments: comma-separated groups of tokens with proper nesting.
    fn parse_macro_args(
        &self,
        tokens: &[(Token, Span)],
        pos: &mut usize,
    ) -> Result<Vec<Vec<(Token, Span)>>, ParseError> {
        let mut args: Vec<Vec<(Token, Span)>> = Vec::new();
        let mut current_arg: Vec<(Token, Span)> = Vec::new();
        let mut depth = 0;
        let mut block_depth = 0;

        while *pos < tokens.len() {
            let tok = &tokens[*pos].0;

            if *tok == Token::RParen && depth == 0 && block_depth == 0 {
                // End of arguments
                if !current_arg.is_empty() || !args.is_empty() {
                    args.push(current_arg);
                }
                *pos += 1; // skip ')'
                return Ok(args);
            }

            if *tok == Token::Comma && depth == 0 && block_depth == 0 {
                args.push(std::mem::take(&mut current_arg));
                *pos += 1;
                continue;
            }

            match tok {
                Token::LParen | Token::LBrace | Token::LBracket | Token::BinOpen => depth += 1,
                Token::RParen | Token::RBrace | Token::RBracket | Token::BinClose => depth -= 1,
                Token::KwBegin | Token::KwCase | Token::KwIf | Token::KwReceive
                    | Token::KwTry | Token::KwMaybe => block_depth += 1,
                // Function references have no `end`; anonymous and named funs do.
                Token::KwFun if matches!(tokens.get(*pos + 1).map(|t| &t.0), Some(Token::LParen))
                    || (matches!(tokens.get(*pos + 1).map(|t| &t.0),
                            Some(Token::Variable | Token::Atom | Token::QuotedAtom))
                        && matches!(tokens.get(*pos + 2).map(|t| &t.0), Some(Token::LParen))) => {
                    block_depth += 1;
                }
                Token::KwEnd if block_depth > 0 => block_depth -= 1,
                _ => {}
            }

            current_arg.push(tokens[*pos].clone());
            *pos += 1;
        }

        Err(ParseError::UnexpectedEof { position: tokens.last().map(|t| t.1.end).unwrap_or(0) })
    }

    fn quote_text(text: &str, delimiter: char) -> String {
        let mut result = String::new();
        result.push(delimiter);
        for c in text.chars() {
            match c {
                '\\' => result.push_str("\\\\"),
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\t' => result.push_str("\\t"),
                '\u{8}' => result.push_str("\\b"),
                '\u{c}' => result.push_str("\\f"),
                '\u{1b}' => result.push_str("\\e"),
                '\u{b}' => result.push_str("\\v"),
                c if c == delimiter => { result.push('\\'); result.push(c); }
                c if c.is_control() => result.push_str(&format!("\\{:03o}", c as u32)),
                c => result.push(c),
            }
        }
        result.push(delimiter);
        result
    }

    fn stringify_token(token: &Token, raw: &str) -> String {
        match token {
            Token::Integer(value) if raw.starts_with('$') => {
                let c = char::from_u32(*value as u32).unwrap();
                if c == ' ' { return "$\\s".into(); }
                let quoted = Self::quote_text(&c.to_string(), '\0');
                format!("${}", &quoted[1..quoted.len() - 1])
            }
            Token::Integer(value) => value.to_string(),
            Token::BigInteger(value) => value.clone(),
            Token::Float(value) => {
                if !value.is_finite() { return raw.to_string(); }
                let mut fixed = value.to_string();
                if !fixed.contains('.') { fixed.push_str(".0"); }
                let scientific = format!("{value:e}");
                let (mantissa, exponent) = scientific.split_once('e').unwrap();
                let scientific = format!("{}{}e{}", mantissa,
                    if mantissa.contains('.') { "" } else { ".0" }, exponent);
                if value.abs() >= 9_007_199_254_740_992.0 || scientific.len() < fixed.len() { scientific } else { fixed }
            }
            Token::String | Token::QuotedAtom => {
                let text = crate::parser::unescape_erlang_string(&raw[1..raw.len() - 1]);
                if *token == Token::QuotedAtom && !crate::token::atom_needs_quoting(&text) {
                    return text;
                }
                Self::quote_text(&text, if *token == Token::String { '"' } else { '\'' })
            }
            _ => raw.to_string(),
        }
    }

    /// Substitute parameter references in body tokens with argument tokens.
    fn substitute_params(
        &mut self,
        body: &[(Token, Span)],
        params: &[String],
        args: &[Vec<(Token, Span)>],
        body_source: &str,
        argument_source: &str,
    ) -> Vec<(Token, Span)> {
        let mut result = Vec::new();
        let mut pos = 0;
        while pos < body.len() {
            // ??Parameter quotes the unexpanded argument token sequence.
            if pos + 2 < body.len()
                && body[pos].0 == Token::Question
                && body[pos + 1].0 == Token::Question
                && body[pos + 2].0 == Token::Variable
            {
                let span = body[pos + 2].1;
                if let Some(name) = body_source.get(span.start..span.end) {
                    if let Some(index) = params.iter().position(|p| p == name) {
                        let text = args[index].iter().map(|(token, span)| {
                            let raw = argument_source.get(span.start..span.end)
                                .or_else(|| self.combined_source.get(span.start..span.end))
                                .unwrap_or("");
                            Self::stringify_token(token, raw)
                        }).collect::<Vec<_>>().join(" ");
                        let quoted = Self::quote_text(&text, '"');
                        self.combined_source.push('\n');
                        let start = self.combined_source.len();
                        self.combined_source.push_str(&quoted);
                        result.push((Token::String, Span::new(start, self.combined_source.len())));
                        pos += 3;
                        continue;
                    }
                }
            }
            let (tok, span) = &body[pos];
            if *tok == Token::Variable {
                if let Some(name) = body_source.get(span.start..span.end) {
                    if let Some(index) = params.iter().position(|p| p == name) {
                        result.extend(args[index].clone());
                        pos += 1;
                        continue;
                    }
                }
            }
            result.push(body[pos].clone());
            pos += 1;
        }
        result
    }

    /// Recursively expand `?MACRO` invocations using a specific source text
    /// for resolving span-based token text.
    ///
    /// This is used when expanding macro bodies that were defined in a different
    /// file (e.g., an included header). The body tokens carry spans from the
    /// definition source, not from `self.source`.
    fn expand_tokens_with_source(
        &mut self,
        tokens: Vec<(Token, Span)>,
        errors: &mut Vec<ParseError>,
        source: &str,
    ) -> Result<Vec<(Token, Span)>, ParseError> {
        let mut output = Vec::new();
        let mut pos = 0;

        while pos < tokens.len() {
            if tokens[pos].0 == Token::Question {
                match self.expand_macro(&tokens, &mut pos, errors, source) {
                    Ok(expanded) => {
                        output.extend(expanded);
                        continue;
                    }
                    Err(e) => {
                        errors.push(e);
                        pos += 1;
                        continue;
                    }
                }
            }
            output.push(tokens[pos].clone());
            pos += 1;
        }

        Ok(output)
    }

    // -----------------------------------------------------------------------
    // Token description helpers (for error messages)
    // -----------------------------------------------------------------------

    fn current_desc(&self, tokens: &[(Token, Span)], pos: usize) -> String {
        if pos >= tokens.len() {
            "end of file".to_string()
        } else {
            tokens[pos].0.description().to_string()
        }
    }

    fn current_span_or(&self, tokens: &[(Token, Span)], pos: usize, fallback: Span) -> Span {
        if pos < tokens.len() {
            tokens[pos].1
        } else {
            fallback
        }
    }

    /// Read a variable name token, returning the text.
    fn expect_variable_name(
        &self,
        tokens: &[(Token, Span)],
        pos: &mut usize,
    ) -> Result<String, ParseError> {
        if *pos >= tokens.len() {
            return Err(ParseError::UnexpectedEof { position: tokens.last().map(|t| t.1.end).unwrap_or(0) });
        }
        let (ref tok, ref span) = tokens[*pos];
        match tok {
            Token::Variable => {
                let name = self.source[span.start..span.end].to_string();
                *pos += 1;
                Ok(name)
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "variable name".into(),
                found: tok.description().to_string(),
                span: *span,
                hint: None,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use std::path::PathBuf;

    fn preprocess(source: &str) -> Result<Vec<(Token, Span)>, Vec<ParseError>> {
        let tokens = Lexer::lex_all(source)?;
        let path = PathBuf::from("test.erl");
        let mut pp = Preprocessor::new(source, &path);
        pp.process(tokens)
    }

    fn token_kinds(tokens: &[(Token, Span)]) -> Vec<Token> {
        tokens.iter().map(|(t, _)| t.clone()).collect()
    }

    #[test]
    fn test_simple_object_macro() {
        let src = "-define(FOO, 42).\nstart() -> ?FOO.";
        let tokens = preprocess(src).unwrap();
        let kinds = token_kinds(&tokens);
        // Should contain: Atom(start) LParen RParen Arrow Integer(42) Dot
        assert!(kinds.contains(&Token::Integer(42)));
        // Should NOT contain Question
        assert!(!kinds.contains(&Token::Question));
    }

    #[test]
    fn test_object_macro_no_body() {
        let src = "-define(DEBUG).\n-ifdef(DEBUG).\nfoo() -> ok.\n-endif.\n";
        let tokens = preprocess(src).unwrap();
        let kinds = token_kinds(&tokens);
        // foo() -> ok. should be present
        assert!(kinds.contains(&Token::Arrow));
    }

    #[test]
    fn macro_args_keep_begin_block_commas() {
        let source = "-define(PAIR(A, B), {A, B}). test() -> ?PAIR(begin X = 1, X + 1 end, 3).";
        let result = preprocess(source);
        assert!(result.is_ok(), "block commas must not change macro arity: {result:?}");
        assert_eq!(token_kinds(&result.unwrap()), token_kinds(&Lexer::lex_all(
            "test() -> {begin X = 1, X + 1 end, 3}."
        ).unwrap()));
    }

    #[test]
    fn macro_args_keep_nested_blocks_and_fun_references() {
        let expressions = [
            "begin case 1 of 1 -> A = 2, A end, 3 end",
            "case 1 of 1 -> A = 2, A end",
            "if true -> A = 2, A end",
            "receive A -> B = A, B after 0 -> C = 2, C end",
            "try A = 2, A catch _:_ -> B = 3, B after ok end",
            "fun(A) -> B = A, B end",
            "fun Loop(0) -> ok; Loop(N) -> A = N - 1, Loop(A) end",
            "begin F = fun local/1, G = fun erlang:abs/1, {F, G} end",
            "fun local/1",
            "fun erlang:abs/1",
            "fun M:F/A",
            "maybe {ok, A} ?= {ok, 1}, A else _ -> 0 end",
        ];
        for expr in expressions {
            let source = format!("-define(PAIR(A, B), {{A, B}}). test() -> ?PAIR({expr}, 9).");
            let result = preprocess(&source).unwrap_or_else(|e| panic!("{expr}: {e:?}"));
            let expected = Lexer::lex_all(&format!("test() -> {{{expr}, 9}}.")).unwrap();
            assert_eq!(token_kinds(&result), token_kinds(&expected), "{expr}");
        }
    }

    #[test]
    fn test_function_macro() {
        let src = "-define(ADD(X, Y), X + Y).\nstart() -> ?ADD(1, 2).";
        let tokens = preprocess(src).unwrap();
        let kinds = token_kinds(&tokens);
        // Should see: start ( ) -> 1 + 2 .
        assert!(kinds.contains(&Token::Plus));
        assert!(kinds.contains(&Token::Integer(1)));
        assert!(kinds.contains(&Token::Integer(2)));
    }

    #[test]
    fn constant_conditionals_and_invalid_conditions() {
        for condition in ["true", "2 >= 1", "(1 < 2) andalso not false", "false orelse true"] {
            let source = format!("-if({condition}). test() -> selected. -else. test() -> ?MISSING. -endif.");
            assert!(preprocess(&source).is_ok(), "{condition}");
        }
        for condition in ["false", "2 < 1", "true andalso false"] {
            let source = format!("-if({condition}). test() -> ?MISSING. -else. test() -> selected. -endif.");
            assert!(preprocess(&source).is_ok(), "{condition}");
        }
        assert!(preprocess("-if(false). -if(?MISSING > 0). test() -> ?NO. -endif. -else. test() -> ok. -endif.").is_ok());
        for source in ["-if(false). -elif(true). test() -> ok. -endif.",
            "-if(true, false). test() -> ok. -endif.",
            "-if true. test() -> ok. -endif.", "-if(erlang:self()). test() -> ok. -endif.",
            "-if(1). test() -> ok. -endif.", "-if(?MISSING >= 25). test() -> ok. -endif."] {
            assert!(preprocess(source).is_err(), "{source}");
        }
    }

    #[test]
    fn otp_release_selects_header_branches() {
        let dir = std::env::temp_dir().join(format!("nera-otp-release-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("version.hrl"),
            "-if(?OTP_RELEASE >= 25). -define(PEER, modern). -else. -define(PEER, legacy). -endif.").unwrap();
        let source = "-include(\"version.hrl\"). test() -> {?OTP_RELEASE, ?PEER}.";
        for (release, expected) in [(24, "legacy"), (29, "modern")] {
            let mut pp = Preprocessor::new(source, &dir.join("probe.erl"));
            pp.set_otp_release(release);
            let tokens = pp.process(Lexer::lex_all(source).unwrap()).unwrap();
            let combined = pp.take_combined_source();
            assert!(tokens.iter().any(|(t, _)| *t == Token::Integer(release.into())));
            assert!(tokens.iter().any(|(t, span)| *t == Token::Atom && &combined[span.start..span.end] == expected));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_predefined_line() {
        let src = "-module(test).\nstart() -> ?LINE.";
        let tokens = preprocess(src).unwrap();
        let kinds = token_kinds(&tokens);
        // ?LINE on line 2 should produce an integer
        let has_integer = kinds.iter().any(|t| matches!(t, Token::Integer(2)));
        assert!(has_integer, "Expected integer 2 for ?LINE, got: {:?}", kinds);
    }

    #[test]
    fn test_predefined_module() {
        let src = "-module(mymod).\nstart() -> ?MODULE.";
        let tokens = preprocess(src).unwrap();
        let kinds = token_kinds(&tokens);
        // ?MODULE should produce an Atom token
        // Count atoms — there should be at least one from ?MODULE expansion
        let atom_count = kinds.iter().filter(|t| matches!(t, Token::Atom)).count();
        assert!(atom_count >= 2); // 'mymod' in -module + 'start' + ?MODULE expansion
    }

    #[test]
    fn test_ifdef_defined() {
        let src = "-define(FOO, 1).\n-ifdef(FOO).\nyes() -> ok.\n-endif.\n";
        let tokens = preprocess(src).unwrap();
        let kinds = token_kinds(&tokens);
        // yes() -> ok. should be present
        assert!(kinds.contains(&Token::Arrow));
    }

    #[test]
    fn test_ifdef_undefined() {
        let src = "-ifdef(FOO).\nyes() -> ok.\n-endif.\n";
        let tokens = preprocess(src).unwrap();
        let kinds = token_kinds(&tokens);
        // yes() -> ok. should NOT be present
        assert!(!kinds.contains(&Token::Arrow));
    }

    #[test]
    fn test_ifdef_else() {
        let src = "-define(FOO, 1).\n-ifdef(FOO).\nyes() -> ok.\n-else.\nno() -> ok.\n-endif.\n";
        let tokens = preprocess(src).unwrap();
        // Should contain 'yes' but not 'no'
        let has_yes = tokens.iter().any(|(t, s)| {
            matches!(t, Token::Atom) && &src[s.start..s.end] == "yes"
        });
        let has_no = tokens.iter().any(|(t, s)| {
            matches!(t, Token::Atom) && &src[s.start..s.end] == "no"
        });
        assert!(has_yes, "Expected 'yes' to be present");
        assert!(!has_no, "Expected 'no' to be absent");
    }

    #[test]
    fn test_ifndef() {
        let src = "-ifndef(FOO).\nyes() -> ok.\n-endif.\n";
        let tokens = preprocess(src).unwrap();
        let kinds = token_kinds(&tokens);
        // yes() -> ok. should be present (FOO not defined)
        assert!(kinds.contains(&Token::Arrow));
    }

    #[test]
    fn test_ifndef_defined() {
        let src = "-define(FOO, 1).\n-ifndef(FOO).\nyes() -> ok.\n-endif.\n";
        let tokens = preprocess(src).unwrap();
        let kinds = token_kinds(&tokens);
        // yes() -> ok. should NOT be present (FOO IS defined)
        assert!(!kinds.contains(&Token::Arrow));
    }

    #[test]
    fn test_nested_ifdef() {
        let src = "\
-define(A, 1).
-ifdef(A).
-ifdef(B).
inner() -> ok.
-endif.
outer() -> ok.
-endif.
";
        let tokens = preprocess(src).unwrap();
        let has_inner = tokens.iter().any(|(t, s)| {
            matches!(t, Token::Atom) && &src[s.start..s.end] == "inner"
        });
        let has_outer = tokens.iter().any(|(t, s)| {
            matches!(t, Token::Atom) && &src[s.start..s.end] == "outer"
        });
        assert!(!has_inner, "inner should be absent (B not defined)");
        assert!(has_outer, "outer should be present (A defined)");
    }

    #[test]
    fn test_undef() {
        let src = "\
-define(FOO, 1).
-ifdef(FOO).
a() -> ok.
-endif.
-undef(FOO).
-ifdef(FOO).
b() -> ok.
-endif.
";
        let tokens = preprocess(src).unwrap();
        let has_a = tokens.iter().any(|(t, s)| {
            matches!(t, Token::Atom) && &src[s.start..s.end] == "a"
        });
        let has_b = tokens.iter().any(|(t, s)| {
            matches!(t, Token::Atom) && &src[s.start..s.end] == "b"
        });
        assert!(has_a, "a should be present");
        assert!(!has_b, "b should be absent after -undef");
    }

    #[test]
    fn test_undefined_macro_error() {
        let src = "start() -> ?UNDEFINED.";
        let result = preprocess(src);
        assert!(result.is_err());
        let errs = result.unwrap_err();
        assert!(errs.iter().any(|e| matches!(e, ParseError::UndefinedMacro { .. })));
    }

    #[test]
    fn test_unclosed_conditional_error() {
        let src = "-ifdef(FOO).\nfoo() -> ok.\n";
        let result = preprocess(src);
        assert!(result.is_err());
        let errs = result.unwrap_err();
        assert!(errs.iter().any(|e| matches!(e, ParseError::UnclosedConditional { .. })));
    }

    #[test]
    fn test_unexpected_endif_error() {
        let src = "-endif.\n";
        let result = preprocess(src);
        assert!(result.is_err());
        let errs = result.unwrap_err();
        assert!(errs.iter().any(|e| matches!(e, ParseError::UnexpectedDirective { .. })));
    }

    #[test]
    fn test_passthrough_no_directives() {
        let src = "-module(test).\n-export([start/0]).\nstart() -> ok.";
        let tokens = preprocess(src).unwrap();
        let kinds = token_kinds(&tokens);
        // Everything should pass through unchanged
        assert_eq!(kinds[0], Token::Minus);
        assert_eq!(kinds[1], Token::KwModule);
    }

    #[test]
    fn test_multiple_macros() {
        let src = "-define(A, 1).\n-define(B, 2).\nstart() -> {?A, ?B}.";
        let tokens = preprocess(src).unwrap();
        let kinds = token_kinds(&tokens);
        assert!(kinds.contains(&Token::Integer(1)));
        assert!(kinds.contains(&Token::Integer(2)));
    }

    /// Simulate `-include("header.hrl")` by processing a header source with
    /// a child preprocessor, then transferring its macros to the main
    /// preprocessor — exactly what `parse_include` does.
    ///
    /// This is a regression test for the cross-file macro expansion bug:
    /// macro body tokens carry spans referencing the header's source text,
    /// but `self.source` in the main preprocessor points to the .erl file.
    /// Without the fix (tracking definition source in MacroDef), span-based
    /// text extraction would produce garbage or panic.
    fn preprocess_with_header(header_src: &str, main_src: &str) -> Result<Vec<(Token, Span)>, Vec<ParseError>> {
        // 1. Process the header (like parse_include does)
        let header_tokens = Lexer::lex_all(header_src)?;
        let header_path = PathBuf::from("header.hrl");
        let mut child = Preprocessor::new(header_src, &header_path);
        let _header_output = child.process(header_tokens)?;

        // 2. Process the main file, inheriting macros from the header
        let main_tokens = Lexer::lex_all(main_src)?;
        let main_path = PathBuf::from("main.erl");
        let mut pp = Preprocessor::new(main_src, &main_path);
        pp.macros = child.macros; // transfer macros (spans reference header_src)
        pp.process(main_tokens)
    }

    #[test]
    fn test_cross_file_object_macro_with_atom() {
        // Header defines a macro whose body is an atom — the span points into
        // the header source, not the main source.
        let header = "-define(PORT, 11211).";
        let main = "-module(test).\nstart() -> ?PORT.";
        let tokens = preprocess_with_header(header, main).unwrap();
        let kinds = token_kinds(&tokens);
        assert!(kinds.contains(&Token::Integer(11211)),
            "Expected 11211 from cross-file macro, got: {:?}", kinds);
    }

    #[test]
    fn test_cross_file_function_macro_with_atoms() {
        // Function-like macro with atom body tokens from header source.
        // The body contains `io:format(Msg)` — atoms `io` and `format` have
        // spans into the header source, while `Msg` is a parameter that gets
        // substituted with tokens from the main source.
        let header = "-define(LOG(Msg), io:format(Msg)).";
        let main = "-module(test).\nstart() -> ?LOG(hello).";
        let tokens = preprocess_with_header(header, main).unwrap();
        let kinds = token_kinds(&tokens);
        // Should contain: ... -> io : format ( hello ) .
        assert!(kinds.contains(&Token::Colon),
            "Expected colon from io:format, got: {:?}", kinds);
        // Verify no errors — the key assertion is that it doesn't panic
        // or produce "undefined macro" errors from garbage span extraction.
    }

    #[test]
    fn test_cross_file_nested_macro_in_body() {
        // Header defines two macros; one references the other.
        // ?WRAPPER expands to ?PORT, which must resolve correctly even though
        // both body tokens have spans from the header source.
        let header = "-define(PORT, 8080).\n-define(WRAPPER, ?PORT).";
        let main = "-module(test).\nstart() -> ?WRAPPER.";
        let tokens = preprocess_with_header(header, main).unwrap();
        let kinds = token_kinds(&tokens);
        assert!(kinds.contains(&Token::Integer(8080)),
            "Expected 8080 from nested cross-file macro, got: {:?}", kinds);
    }

    #[test]
    fn test_cross_file_function_macro_with_macro_in_arg() {
        // Macro argument contains a ?MACRO reference from the main source.
        // The function-like macro is from the header, but args are from main.
        // After substitution, body tokens (header spans) mix with arg tokens
        // (main spans) — the fallback logic must handle both.
        let header = "-define(WRAP(X), {wrapped, X}).";
        let main = "-module(test).\n-define(VAL, 42).\nstart() -> ?WRAP(?VAL).";
        let tokens = preprocess_with_header(header, main).unwrap();
        let kinds = token_kinds(&tokens);
        assert!(kinds.contains(&Token::Integer(42)),
            "Expected 42 from macro-in-arg, got: {:?}", kinds);
    }

    #[test]
    fn stringify_from_nested_header_keeps_caller_text() {
        let (tokens, source) = preprocess_with_include_files(
            &[("outer.hrl", "-include(\"inner.hrl\")."),
              ("inner.hrl", "-define(S(X), ??X). -define(WRAP(X), ?S(X)).")],
            "probe.erl",
            "-module(probe). -include(\"outer.hrl\"). test() -> ?WRAP({hello, 16#ff}).",
        );
        let (_, span) = tokens.iter().find(|(t, _)| *t == Token::String).unwrap();
        assert_eq!(&source[span.start..span.end], "\"{ hello , 255 }\"");
    }

    #[test]
    fn stringify_macro_argument_without_evaluating_it() {
        let source = "-define(S(X), ??X). test() -> ?S(missing(16#ff) + Value).";
        let path = PathBuf::from("probe.erl");
        let mut pp = Preprocessor::new(source, &path);
        let result = pp.process(Lexer::lex_all(source).unwrap());
        assert!(result.is_ok(), "stringification failed: {result:?}");
        let tokens = result.unwrap();
        let (_, span) = tokens.iter().find(|(t, _)| *t == Token::String).unwrap();
        assert_eq!(&pp.combined_source[span.start..span.end], "\"missing ( 255 ) + Value\"");
    }

    #[test]
    fn undef_removes_all_macro_arities() {
        let result = preprocess("-define(M(), 1). -define(M(X), X). -undef(M). -ifdef(M). test() -> ?UNDEFINED. -else. test() -> ok. -endif.");
        assert!(result.is_ok(), "undef left an overload: {result:?}");
        assert!(preprocess("-define(M(X), X). test() -> ?M(1,2).").is_err());
    }

    #[test]
    fn macros_with_distinct_arities_coexist() {
        let result = preprocess("-define(M(X), X). -define(M(X,Y), {X,Y}). test() -> {?M(1), ?M(2,3)}.");
        assert!(result.is_ok(), "overloaded macros failed: {result:?}");
        let kinds = token_kinds(&result.unwrap());
        for value in [1, 2, 3] { assert!(kinds.contains(&Token::Integer(value))); }
    }

    #[test]
    fn nested_include_lib_keeps_application_resolver() {
        let dir = std::env::temp_dir().join(format!("nera_nested_lib_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("outer.hrl"), "-include_lib(\"stdlib/include/assert.hrl\").").unwrap();
        std::fs::write(dir.join("inner.hrl"), "-define(VALUE, 42).").unwrap();
        let source = "-module(probe). -include_lib(\"eunit/include/eunit.hrl\"). test() -> ?VALUE.";
        let mut pp = Preprocessor::new(source, &dir.join("probe.erl"));
        let root = dir.clone();
        pp.set_include_lib_resolver(Box::new(move |path| match path {
            "eunit/include/eunit.hrl" => Some(root.join("outer.hrl")),
            "stdlib/include/assert.hrl" => Some(root.join("inner.hrl")),
            _ => None,
        }));
        let result = pp.process(Lexer::lex_all(source).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
        assert!(result.is_ok(), "nested include failed: {result:?}");
        assert!(token_kinds(&result.unwrap()).contains(&Token::Integer(42)));
    }

    #[test]
    fn test_include_span_validity() {
        // Regression: tokens from included files had spans pointing into the
        // include file's source text. When the parser used them against the
        // main source, it got wrong text. The combined_source mechanism fixes
        // this by adjusting spans so all tokens index into a single buffer.
        use std::io::Write;
        let dir = std::env::temp_dir().join("nera_test_include_span");
        let _ = std::fs::create_dir_all(&dir);

        // Write a header file
        let hrl_path = dir.join("test_inc.hrl");
        let mut f = std::fs::File::create(&hrl_path).unwrap();
        writeln!(f, "-record(from_hrl, {{x, y}}).").unwrap();

        // Write a main file that includes it
        let erl_path = dir.join("test_inc.erl");
        let main_src = "-module(test_inc).\n-include(\"test_inc.hrl\").\n-export([test/0]).\ntest() -> ok.\n";
        std::fs::write(&erl_path, main_src).unwrap();

        // Lex + preprocess
        let tokens = Lexer::lex_all(main_src).unwrap();
        let mut pp = Preprocessor::new(main_src, &erl_path);
        let processed = pp.process(tokens).unwrap();
        let combined = pp.take_combined_source();

        // Every token's span must be valid within the combined source
        for (tok, span) in &processed {
            assert!(
                span.end <= combined.len(),
                "Token {:?} span ({}, {}) exceeds combined source len {}",
                tok, span.start, span.end, combined.len()
            );
            // For Atom tokens, the text should be resolvable
            if *tok == Token::Atom {
                let text = &combined[span.start..span.end];
                assert!(
                    !text.is_empty(),
                    "Atom token has empty text at span ({}, {})",
                    span.start, span.end
                );
            }
        }

        // The combined source should contain the record name from the header
        assert!(
            combined.contains("from_hrl"),
            "Combined source should contain 'from_hrl' from include"
        );

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Helper: create temp dir, write files, preprocess, return (tokens, combined_source).
    fn preprocess_with_include_files(
        files: &[(&str, &str)], // [(filename, content), ...]
        main_filename: &str,
        main_src: &str,
    ) -> (Vec<(Token, Span)>, String) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("nera_test_macro_spans_{}", id));
        let _ = std::fs::create_dir_all(&dir);

        for (name, content) in files {
            std::fs::write(dir.join(name), content).unwrap();
        }

        let erl_path = dir.join(main_filename);
        std::fs::write(&erl_path, main_src).unwrap();

        let tokens = Lexer::lex_all(main_src).unwrap();
        let mut pp = Preprocessor::new(main_src, &erl_path);
        let processed = pp.process(tokens).unwrap();
        let combined = pp.take_combined_source();

        let _ = std::fs::remove_dir_all(&dir);
        (processed, combined)
    }

    /// Assert every token span is valid in the combined source, and return
    /// the text of all Atom tokens for further verification.
    fn assert_spans_valid(tokens: &[(Token, Span)], combined: &str) -> Vec<String> {
        let mut atom_texts = Vec::new();
        for (tok, span) in tokens {
            assert!(
                span.end <= combined.len(),
                "Token {:?} span ({}, {}) exceeds combined source len {}",
                tok, span.start, span.end, combined.len()
            );
            if span.start < span.end {
                let text = &combined[span.start..span.end];
                if *tok == Token::Atom {
                    atom_texts.push(text.to_string());
                }
            }
        }
        atom_texts
    }

    #[test]
    fn test_include_function_macro_span_remapping() {
        // Regression test: function-like macros defined in include files had
        // body token spans pointing into the include file's raw source. After
        // expansion in the main file, the parser would read garbled text from
        // combined_source because the spans weren't adjusted.
        let hrl = "-define(CRITICAL_MSG(Format, Args),\n    error_logger:error_msg(\"CRITICAL:~n\"++Format, Args)).";
        let main = "-module(test_mod).\n-include(\"test.hrl\").\ntest() -> ?CRITICAL_MSG(\"hello ~p\", [42]).";

        let (tokens, combined) = preprocess_with_include_files(
            &[("test.hrl", hrl)],
            "test_mod.erl",
            main,
        );

        let atom_texts = assert_spans_valid(&tokens, &combined);

        // The expanded macro body should contain 'error_logger' and 'error_msg'
        // as properly resolved atoms (not garbled text).
        assert!(
            atom_texts.contains(&"error_logger".to_string()),
            "Expected 'error_logger' atom from include macro, got atoms: {:?}", atom_texts
        );
        assert!(
            atom_texts.contains(&"error_msg".to_string()),
            "Expected 'error_msg' atom from include macro, got atoms: {:?}", atom_texts
        );
    }

    #[test]
    fn test_include_object_macro_span_remapping() {
        // Object-like macro with atom body from include file.
        let hrl = "-define(DEFAULT_PORT, 8080).";
        let main = "-module(test_mod).\n-include(\"test.hrl\").\ntest() -> ?DEFAULT_PORT.";

        let (tokens, combined) = preprocess_with_include_files(
            &[("test.hrl", hrl)],
            "test_mod.erl",
            main,
        );

        assert_spans_valid(&tokens, &combined);
        let kinds = token_kinds(&tokens);
        assert!(
            kinds.contains(&Token::Integer(8080)),
            "Expected 8080 from include object macro, got: {:?}", kinds
        );
    }

    #[test]
    fn test_include_macro_with_module_colon_function() {
        // Macro body contains module:function pattern (the exact case that
        // produced garbled output like 'jerome.sautr:t@proces/2').
        let hrl = "-define(LOG(Msg), io:format(Msg)).";
        let main = "-module(test_mod).\n-include(\"test.hrl\").\ntest() -> ?LOG(\"hello\").";

        let (tokens, combined) = preprocess_with_include_files(
            &[("test.hrl", hrl)],
            "test_mod.erl",
            main,
        );

        let atom_texts = assert_spans_valid(&tokens, &combined);

        assert!(
            atom_texts.contains(&"io".to_string()),
            "Expected 'io' atom from include macro body, got atoms: {:?}", atom_texts
        );
        assert!(
            atom_texts.contains(&"format".to_string()),
            "Expected 'format' atom from include macro body, got atoms: {:?}", atom_texts
        );
    }

    #[test]
    fn test_include_nested_macro_from_include() {
        // Header defines two macros where one references the other.
        // Both body token spans need remapping.
        let hrl = "-define(PORT, 8080).\n-define(ADDR, {localhost, ?PORT}).";
        let main = "-module(test_mod).\n-include(\"test.hrl\").\ntest() -> ?ADDR.";

        let (tokens, combined) = preprocess_with_include_files(
            &[("test.hrl", hrl)],
            "test_mod.erl",
            main,
        );

        assert_spans_valid(&tokens, &combined);
        let kinds = token_kinds(&tokens);
        assert!(
            kinds.contains(&Token::Integer(8080)),
            "Expected 8080 from nested macro in include, got: {:?}", kinds
        );
    }

    #[test]
    fn test_include_macro_with_ifdef() {
        // Include file uses -ifdef to conditionally define a macro.
        let hrl = "-define(DEBUG_MODE, true).\n-ifdef(DEBUG_MODE).\n-define(DEBUG(Format, Args), error_logger:info_msg(\"D:\"++Format, Args)).\n-else.\n-define(DEBUG(F,A),[]).\n-endif.";
        let main = "-module(test_mod).\n-include(\"test.hrl\").\ntest() -> ?DEBUG(\"test ~p\", [1]).";

        let (tokens, combined) = preprocess_with_include_files(
            &[("test.hrl", hrl)],
            "test_mod.erl",
            main,
        );

        let atom_texts = assert_spans_valid(&tokens, &combined);

        // The DEBUG branch should be active, producing error_logger:info_msg
        assert!(
            atom_texts.contains(&"error_logger".to_string()),
            "Expected 'error_logger' from ifdef-guarded macro, got atoms: {:?}", atom_texts
        );
        assert!(
            atom_texts.contains(&"info_msg".to_string()),
            "Expected 'info_msg' from ifdef-guarded macro, got atoms: {:?}", atom_texts
        );
    }

    #[test]
    fn test_include_multiple_macro_expansions() {
        // Multiple expansions of the same include-defined macro in one file.
        // All expansions should produce valid spans.
        let hrl = "-define(LOG(Msg), io:format(Msg)).";
        let main = "-module(test_mod).\n-include(\"test.hrl\").\ntest() -> ?LOG(\"a\"), ?LOG(\"b\").";

        let (tokens, combined) = preprocess_with_include_files(
            &[("test.hrl", hrl)],
            "test_mod.erl",
            main,
        );

        let atom_texts = assert_spans_valid(&tokens, &combined);

        // 'io' and 'format' should appear at least twice (once per expansion)
        let io_count = atom_texts.iter().filter(|s| *s == "io").count();
        let format_count = atom_texts.iter().filter(|s| *s == "format").count();
        assert!(
            io_count >= 2,
            "Expected 'io' at least 2 times, got {} in {:?}", io_count, atom_texts
        );
        assert!(
            format_count >= 2,
            "Expected 'format' at least 2 times, got {} in {:?}", format_count, atom_texts
        );
    }

    #[test]
    fn test_include_macro_mixed_with_local_macro() {
        // Include-defined macro and locally-defined macro used together.
        // Ensures span adjustment doesn't break local macros.
        let hrl = "-define(REMOTE_FUN, lists:reverse).";
        let main = "-module(test_mod).\n-include(\"test.hrl\").\n-define(LOCAL, 42).\ntest() -> {?REMOTE_FUN, ?LOCAL}.";

        let (tokens, combined) = preprocess_with_include_files(
            &[("test.hrl", hrl)],
            "test_mod.erl",
            main,
        );

        let atom_texts = assert_spans_valid(&tokens, &combined);
        let kinds = token_kinds(&tokens);

        assert!(
            atom_texts.contains(&"lists".to_string()),
            "Expected 'lists' from include macro, got atoms: {:?}", atom_texts
        );
        assert!(
            atom_texts.contains(&"reverse".to_string()),
            "Expected 'reverse' from include macro, got atoms: {:?}", atom_texts
        );
        assert!(
            kinds.contains(&Token::Integer(42)),
            "Expected 42 from local macro, got: {:?}", kinds
        );
    }

    fn preprocess_with_locations(source: &str) -> (Vec<(Token, Span)>, Vec<MacroLocation>) {
        let tokens = Lexer::lex_all(source).unwrap();
        let path = PathBuf::from("test.erl");
        let mut pp = Preprocessor::new(source, &path);
        let result = pp.process(tokens).unwrap();
        let (_, _, locs, _) = pp.take_all();
        (result, locs)
    }

    #[test]
    fn test_macro_location_object_arity() {
        let src = "-define(PORT, 11211).\n-define(DEBUG).\nstart() -> ?PORT.";
        let (_, locs) = preprocess_with_locations(src);
        assert_eq!(locs.len(), 2);
        let port = locs.iter().find(|m| m.name == "PORT").unwrap();
        assert_eq!(port.arity, None, "object-like macro should have arity None");
        let debug = locs.iter().find(|m| m.name == "DEBUG").unwrap();
        assert_eq!(debug.arity, None, "bodyless macro should have arity None");
    }

    #[test]
    fn test_macro_location_function_arity() {
        let src = "-define(ADD(X, Y), X + Y).\n-define(ID(X), X).\nstart() -> ?ADD(1, 2).";
        let (_, locs) = preprocess_with_locations(src);
        assert_eq!(locs.len(), 2);
        let add = locs.iter().find(|m| m.name == "ADD").unwrap();
        assert_eq!(add.arity, Some(2), "ADD should have arity 2");
        let id = locs.iter().find(|m| m.name == "ID").unwrap();
        assert_eq!(id.arity, Some(1), "ID should have arity 1");
    }

    #[test]
    fn test_macro_location_zero_arity_function() {
        let src = "-define(NOW(), erlang:system_time()).\nstart() -> ?NOW().";
        let (_, locs) = preprocess_with_locations(src);
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0].name, "NOW");
        assert_eq!(locs[0].arity, Some(0), "zero-arg function macro should have arity Some(0)");
    }

    // ── Used macro tracking tests ─────────────────────────────────

    fn preprocess_used_macros(source: &str) -> HashSet<String> {
        let tokens = Lexer::lex_all(source).unwrap();
        let path = PathBuf::from("test.erl");
        let mut pp = Preprocessor::new(source, &path);
        let _ = pp.process(tokens).unwrap();
        let (_, _, _, used) = pp.take_all();
        used
    }

    #[test]
    fn used_macros_tracks_expansion() {
        let src = "-define(PORT, 8080).\nstart() -> ?PORT.";
        let used = preprocess_used_macros(src);
        assert!(used.contains("PORT"), "expanded macro should be tracked");
    }

    #[test]
    fn used_macros_unused_macro_not_tracked() {
        let src = "-define(PORT, 8080).\n-define(HOST, localhost).\nstart() -> ?PORT.";
        let used = preprocess_used_macros(src);
        assert!(used.contains("PORT"), "expanded macro should be tracked");
        assert!(!used.contains("HOST"), "unused macro should not be tracked");
    }

    #[test]
    fn used_macros_ifdef_counts_as_reference() {
        let src = "-define(DEBUG).\n-ifdef(DEBUG).\nstart() -> ok.\n-endif.";
        let used = preprocess_used_macros(src);
        assert!(used.contains("DEBUG"), "-ifdef should count as a reference");
    }

    #[test]
    fn used_macros_ifndef_counts_as_reference() {
        let src = "-define(PROD).\n-ifndef(PROD).\nstart() -> debug.\n-else.\nstart() -> prod.\n-endif.";
        let used = preprocess_used_macros(src);
        assert!(used.contains("PROD"), "-ifndef should count as a reference");
    }

    #[test]
    fn used_macros_function_macro_tracked() {
        let src = "-define(ADD(X, Y), X + Y).\nstart() -> ?ADD(1, 2).";
        let used = preprocess_used_macros(src);
        assert!(used.contains("ADD"), "function-like macro should be tracked");
    }
}
