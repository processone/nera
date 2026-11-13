use std::collections::HashSet;

use string_interner::{DefaultStringInterner, DefaultSymbol, Symbol};

use crate::ast::*;
use crate::error::ParseError;
use crate::lexer::Lexer;
use crate::span::Span;
use crate::token::Token;

/// Process Erlang escape sequences in a string literal.
///
/// Handles: \n \t \r \b \f \e \s \v \\ \' \" and passthrough for unknown.
pub(crate) fn unescape_erlang_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('b') => out.push('\x08'),
                Some('f') => out.push('\x0C'),
                Some('e') => out.push('\x1B'),
                Some('s') => out.push(' '),
                Some('v') => out.push('\x0B'),
                Some('\\') => out.push('\\'),
                Some('\'') => out.push('\''),
                Some('"') => out.push('"'),
                Some(d) if ('0'..='7').contains(&d) => {
                    // Octal escape: up to 3 octal digits
                    let mut val = (d as u32) - ('0' as u32);
                    for _ in 0..2 {
                        // Clone the iterator to peek
                        let mut peek = chars.clone();
                        if let Some(ch) = peek.next() {
                            if ('0'..='7').contains(&ch) {
                                val = val * 8 + ((ch as u32) - ('0' as u32));
                                chars = peek;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    // Three octal digits cover 0..=511, all valid codepoints.
                    out.push(char::from_u32(val).unwrap());
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Result of parsing a module. Always contains a (possibly partial) AST,
/// plus any errors encountered during parsing. When `errors` is non-empty,
/// the module still contains all forms that were parsed successfully —
/// only the erroneous forms are missing.
pub struct ParseResult {
    pub module: Module,
    pub errors: Vec<ParseError>,
}

impl ParseResult {
    /// Returns `true` if parsing produced any errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Recursive-descent parser for an Erlang subset.
///
/// All tokens are lexed upfront so that arbitrary lookahead is free.
/// Atom strings are interned via `string_interner::StringInterner`.
pub struct Parser<'src> {
    source: &'src str,
    tokens: Vec<(Token, Span)>,
    pos: usize,
    interner: DefaultStringInterner,
    errors: Vec<ParseError>,
    /// Current function name for ?FUNCTION_NAME macro resolution.
    current_function_name: Option<AtomId>,
    /// Current function arity for ?FUNCTION_ARITY macro resolution.
    current_function_arity: Option<u32>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

impl<'src> Parser<'src> {
    /// Create a new parser for the given Erlang source text.
    ///
    /// All tokens are lexed eagerly; lexer errors are stored and will be
    /// returned from `parse_module`.
    pub fn new(source: &'src str) -> Self {
        let (tokens, lex_errors) = match Lexer::lex_all(source) {
            Ok(toks) => (toks, vec![]),
            Err(errs) => (vec![], errs),
        };
        Self {
            source,
            tokens,
            pos: 0,
            interner: DefaultStringInterner::default(),
            errors: lex_errors,
            current_function_name: None,
            current_function_arity: None,
        }
    }

    /// Create a new parser that shares an existing string interner.
    ///
    /// This is used when compiling multiple modules so they all share
    /// the same atom ID space. The caller passes the interner from a
    /// previous parser (via `into_interner()`) and gets it back after
    /// parsing via `into_interner()`.
    pub fn with_interner(source: &'src str, interner: DefaultStringInterner) -> Self {
        let (tokens, lex_errors) = match Lexer::lex_all(source) {
            Ok(toks) => (toks, vec![]),
            Err(errs) => (vec![], errs),
        };
        Self {
            source,
            tokens,
            pos: 0,
            interner,
            errors: lex_errors,
            current_function_name: None,
            current_function_arity: None,
        }
    }

    /// Create a new parser from pre-lexed (and preprocessed) tokens.
    ///
    /// This is used when the token stream has already been produced by the
    /// lexer and processed by the preprocessor. The parser skips internal
    /// lexing and operates directly on the provided token vector.
    pub fn with_tokens(
        source: &'src str,
        tokens: Vec<(Token, Span)>,
        interner: DefaultStringInterner,
    ) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            interner,
            errors: vec![],
            current_function_name: None,
            current_function_arity: None,
        }
    }

    /// Consume the parser and return the interner for reuse.
    pub fn into_interner(self) -> DefaultStringInterner {
        self.interner
    }

    /// Build a map from `AtomId` to its string name.
    ///
    /// Useful for warning analysis and diagnostic messages that need
    /// human-readable variable/function names.
    pub fn atom_names(&self) -> std::collections::HashMap<crate::ast::AtomId, String> {
        use string_interner::Symbol;
        self.interner
            .into_iter()
            .map(|(sym, s)| (crate::ast::AtomId(sym.to_usize() as u32), s.to_string()))
            .collect()
    }

    /// Parse a complete Erlang module.
    ///
    /// Always returns a [`ParseResult`] containing a (possibly partial)
    /// module and any errors encountered. When errors are present, the
    /// module still contains all forms that were parsed successfully —
    /// only the erroneous forms are missing.
    pub fn parse_module(&mut self) -> ParseResult {
        let start = self.current_span().start;
        let mut module_name: Option<AtomId> = None;
        let mut module_name_span: Option<Span> = None;
        let mut exports: Vec<FunArity> = Vec::new();
        let mut type_exports: Vec<FunArity> = Vec::new();
        let mut deprecated_specs: Vec<DeprecatedSpec> = Vec::new();
        let mut imports = Vec::new();
        let mut no_auto_import = NoAutoImport::default();
        let mut functions: Vec<Function> = Vec::new();
        let mut records: Vec<RecordDef> = Vec::new();
        let mut type_defs: Vec<TypeDef> = Vec::new();
        let mut specs: Vec<SpecDef> = Vec::new();
        let mut behaviours: Vec<BehaviourAttr> = Vec::new();
        let mut type_refs: HashSet<AtomId> = HashSet::new();
        let mut flags = CompileFlags::default();

        while !self.at_end() {
            if self.check(&Token::Minus) {
                // attribute
                match self.parse_attribute() {
                    Ok(Attr::Module(name, nspan)) => {
                        module_name = Some(name);
                        module_name_span = Some(nspan);
                    }
                    Ok(Attr::Export(funs)) => exports.extend(funs),
                    Ok(Attr::ExportType(funs)) => type_exports.extend(funs),
                    Ok(Attr::Deprecated(funs)) => deprecated_specs.extend(funs),
                    Ok(Attr::Record(def)) => records.push(def),
                    Ok(Attr::TypeDef(def, refs)) => {
                        type_defs.push(def);
                        type_refs.extend(refs);
                    }
                    Ok(Attr::Spec(def, refs)) => {
                        specs.push(def);
                        type_refs.extend(refs);
                    }
                    Ok(Attr::Behaviour(name, span)) => {
                        behaviours.push(BehaviourAttr { name, span });
                    }
                    Ok(Attr::Import(decl)) => imports.push(decl),
                    Ok(Attr::Compile(suppression, compile)) => {
                        no_auto_import.all |= suppression.all;
                        no_auto_import.functions.extend(suppression.functions);
                        flags.eunit_autoexport |= compile.eunit_autoexport;
                        flags.export_all |= compile.export_all;
                    }
                    Ok(Attr::Unknown) => {} // silently skip unknown attributes
                    Err(e) => {
                        self.errors.push(e);
                        self.synchronize();
                    }
                }
            } else if self.check_atom_or_keyword() {
                // function definition
                match self.parse_function() {
                    Ok(f) => functions.push(f),
                    Err(e) => {
                        self.errors.push(e);
                        self.synchronize();
                    }
                }
            } else if !self.at_end() && Self::is_reserved_keyword(&self.tokens[self.pos].0) {
                let keyword = self.slice_at(self.pos).to_owned();
                let span = self.current_span();
                self.errors.push(ParseError::ReservedKeyword {
                    keyword,
                    context: "a function name".into(),
                    span,
                });
                self.synchronize();
            } else {
                let span = self.current_span();
                self.errors.push(ParseError::UnexpectedToken {
                    expected: "attribute or function definition".into(),
                    found: self.current_description(),
                    span,
                    hint: Some("each function or attribute must end with '.'".into()),
                });
                self.advance();
            }
        }

        // Implement the export part of the standard EUnit autoexport transform.
        // Other parse transforms are not executed by Nera.
        if flags.eunit_autoexport {
            for function in &functions {
                let name = self.resolve(function.name).unwrap_or("");
                if function.arity == 0 && (name.ends_with("_test") || name.ends_with("_test_"))
                    && !exports.iter().any(|f| f.name == function.name && f.arity == 0) {
                    exports.push(FunArity { name: function.name, arity: 0, span: function.name_span });
                }
            }
        }

        // Wildcards refer to exported functions, including export attributes
        // appearing after -deprecated. Keep the public AST as concrete pairs.
        let mut deprecated = Vec::new();
        for spec in deprecated_specs {
            if let (Some(name), Some(arity)) = (spec.name, spec.arity) {
                deprecated.push(FunArity {
                    name,
                    arity,
                    span: spec.span,
                });
            } else {
                for fun in &exports {
                    if spec.name.is_none_or(|name| name == fun.name)
                        && spec.arity.is_none_or(|arity| arity == fun.arity)
                    {
                        deprecated.push(FunArity {
                            name: fun.name,
                            arity: fun.arity,
                            span: spec.span,
                        });
                    }
                }
            }
        }

        let name = module_name.unwrap_or_else(|| self.intern("undefined"));
        let end = if !self.tokens.is_empty() {
            self.tokens.last().unwrap().1.end
        } else {
            start
        };

        ParseResult {
            module: Module {
                name,
                name_span: module_name_span.unwrap_or_else(|| Span::new(0, 0)),
                exports,
                export_all: flags.export_all,
                imports,
                no_auto_import,
                type_exports,
                deprecated,
                functions,
                records,
                type_defs,
                specs,
                behaviours,
                type_refs,
                span: Span::new(start, end),
            },
            errors: self.errors.clone(),
        }
    }

    /// Access the string interner so callers can resolve `AtomId` values.
    pub fn interner(&self) -> &DefaultStringInterner {
        &self.interner
    }

    /// Resolve an `AtomId` back to its string.
    pub fn resolve(&self, id: AtomId) -> Option<&str> {
        let sym = DefaultSymbol::try_from_usize(id.0 as usize)?;
        self.interner.resolve(sym)
    }
}

// ---------------------------------------------------------------------------
// Attribute parsing
// ---------------------------------------------------------------------------

struct DeprecatedSpec {
    name: Option<AtomId>,
    arity: Option<u32>,
    span: Span,
}

enum Attr {
    Module(AtomId, Span),
    Export(Vec<FunArity>),
    ExportType(Vec<FunArity>),
    Deprecated(Vec<DeprecatedSpec>),
    Record(RecordDef),
    TypeDef(TypeDef, Vec<AtomId>),
    Spec(SpecDef, Vec<AtomId>),
    Behaviour(AtomId, Span),
    Import(ImportDecl),
    Compile(NoAutoImport, CompileFlags),
    Unknown,
}

/// Compile options Nera acts on, beyond auto-import suppression.
#[derive(Default)]
struct CompileFlags {
    /// `{parse_transform, eunit_autoexport}`
    eunit_autoexport: bool,
    /// `export_all`
    export_all: bool,
}

impl<'src> Parser<'src> {
    fn parse_attribute(&mut self) -> Result<Attr, ParseError> {
        self.expect(&Token::Minus)?;

        if self.check(&Token::KwModule) {
            self.advance();
            self.expect(&Token::LParen)?;
            let name_span = self.current_span();
            let name = self.expect_atom_name()?;
            let name_span = name_span.merge(self.prev_span());
            self.expect(&Token::RParen)?;
            self.expect(&Token::Dot)?;
            Ok(Attr::Module(name, name_span))
        } else if self.check(&Token::KwExport) {
            self.advance();
            self.expect(&Token::LParen)?;
            self.expect(&Token::LBracket)?;
            let mut funs = Vec::new();
            if !self.check(&Token::RBracket) {
                funs.push(self.parse_fun_arity()?);
                while self.eat(&Token::Comma) {
                    funs.push(self.parse_fun_arity()?);
                }
            }
            self.expect(&Token::RBracket)?;
            self.expect(&Token::RParen)?;
            self.expect(&Token::Dot)?;
            Ok(Attr::Export(funs))
        } else if self.check(&Token::Atom) && self.slice_at(self.pos) == "deprecated" {
            self.advance();
            self.expect(&Token::LParen)?;
            let mut funs = Vec::new();
            if self.eat(&Token::LBracket) {
                if !self.check(&Token::RBracket) {
                    funs.push(self.parse_deprecated_spec()?);
                    while self.eat(&Token::Comma) {
                        funs.push(self.parse_deprecated_spec()?);
                    }
                }
                self.expect(&Token::RBracket)?;
            } else {
                funs.push(self.parse_deprecated_spec()?);
            }
            self.expect(&Token::RParen)?;
            self.expect(&Token::Dot)?;
            Ok(Attr::Deprecated(funs))
        } else if self.check(&Token::Atom) && self.slice_at(self.pos) == "record" {
            self.advance(); // consume 'record'
            self.expect(&Token::LParen)?;
            let name_span = self.current_span();
            let record_name = self.expect_atom_name()?;
            self.expect(&Token::Comma)?;
            self.expect(&Token::LBrace)?;
            let fields = self.parse_record_field_defs()?;
            self.expect(&Token::RBrace)?;
            self.expect(&Token::RParen)?;
            let end = self.prev_span();
            self.expect(&Token::Dot)?;
            Ok(Attr::Record(RecordDef {
                name: record_name,
                name_span,
                fields,
                span: name_span.merge(end),
            }))
        } else if self.check(&Token::KwImport) {
            self.advance();
            self.expect(&Token::LParen)?;
            let module = self.expect_atom_name()?;
            self.expect(&Token::Comma)?;
            self.expect(&Token::LBracket)?;
            let mut functions = Vec::new();
            if !self.check(&Token::RBracket) {
                functions.push(self.parse_fun_arity()?);
                while self.eat(&Token::Comma) {
                    functions.push(self.parse_fun_arity()?);
                }
            }
            self.expect(&Token::RBracket)?;
            self.expect(&Token::RParen)?;
            self.expect(&Token::Dot)?;
            Ok(Attr::Import(ImportDecl { module, functions }))
        } else if self.check(&Token::Atom) && matches!(self.slice_at(self.pos), "spec" | "callback") {
            let is_callback = self.slice_at(self.pos) == "callback";
            let attr_start = self.tokens[self.pos - 1].1; // the '-' token
            self.advance(); // consume 'spec'/'callback'
            // Expect function name (may be in parens for callback)
            let name_span = self.current_span();
            let func_name = self.expect_atom_name()?;
            // Count parameters in first (...) group
            let arity = if self.check(&Token::LParen) {
                self.advance(); // consume '('
                let mut count = 0u32;
                let mut depth: u32 = 1;
                if !self.check(&Token::RParen) {
                    count = 1;
                }
                while !self.at_end() && depth > 0 {
                    match &self.tokens[self.pos].0 {
                        Token::LParen | Token::LBracket | Token::LBrace | Token::BinOpen => {
                            depth += 1;
                            self.advance();
                        }
                        Token::RParen | Token::RBracket | Token::RBrace | Token::BinClose => {
                            depth -= 1;
                            if depth == 0 {
                                self.advance();
                                break;
                            }
                            self.advance();
                        }
                        Token::Comma if depth == 1 => {
                            count += 1;
                            self.advance();
                        }
                        _ => {
                            self.advance();
                        }
                    }
                }
                count
            } else {
                0
            };
            // Skip to terminating dot, collecting type references (atom followed by '(').
            let mut depth: u32 = 0;
            let mut collected_type_refs: Vec<AtomId> = Vec::new();
            while !self.at_end() {
                match &self.tokens[self.pos].0 {
                    Token::Atom | Token::QuotedAtom => {
                        let atom_name = self.intern_current_atom();
                        self.advance();
                        if !self.at_end() && self.tokens[self.pos].0 == Token::LParen {
                            collected_type_refs.push(atom_name);
                        }
                    }
                    Token::LParen | Token::LBracket | Token::LBrace | Token::BinOpen => {
                        depth += 1;
                        self.advance();
                    }
                    Token::RParen | Token::RBracket | Token::RBrace | Token::BinClose => {
                        depth = depth.saturating_sub(1);
                        self.advance();
                    }
                    Token::Dot if depth == 0 => {
                        let dot_end = self.current_span().end;
                        self.advance();
                        if !self.at_end() && self.current_span().start == dot_end {
                            continue;
                        }
                        break;
                    }
                    _ => {
                        self.advance();
                    }
                }
            }
            let attr_end = self.prev_span();
            Ok(Attr::Spec(SpecDef {
                func_name,
                name_span,
                arity,
                is_callback,
                span: attr_start.merge(attr_end),
            }, collected_type_refs))
        } else if self.check(&Token::Atom) && matches!(self.slice_at(self.pos), "type" | "opaque") {
            let is_opaque = self.slice_at(self.pos) == "opaque";
            let attr_start = self.tokens[self.pos - 1].1; // the '-' token
            self.advance(); // consume 'type'/'opaque'
            let parenthesized = self.eat(&Token::LParen);
            // Expect type name
            let name_span = self.current_span();
            let name = self.expect_atom_name()?;
            // Count parameters in first (...) group
            let arity = if self.check(&Token::LParen) {
                self.advance(); // consume '('
                let mut count = 0u32;
                let mut depth: u32 = 1;
                if !self.check(&Token::RParen) {
                    count = 1;
                }
                while !self.at_end() && depth > 0 {
                    match &self.tokens[self.pos].0 {
                        Token::LParen | Token::LBracket | Token::LBrace | Token::BinOpen => {
                            depth += 1;
                            self.advance();
                        }
                        Token::RParen | Token::RBracket | Token::RBrace | Token::BinClose => {
                            depth -= 1;
                            if depth == 0 {
                                self.advance();
                                break;
                            }
                            self.advance();
                        }
                        Token::Comma if depth == 1 => {
                            count += 1;
                            self.advance();
                        }
                        _ => {
                            self.advance();
                        }
                    }
                }
                count
            } else {
                0
            };
            // Skip to terminating dot, collecting type references (atom followed by '(').
            let mut depth: u32 = u32::from(parenthesized);
            let mut collected_type_refs: Vec<AtomId> = Vec::new();
            while !self.at_end() {
                match &self.tokens[self.pos].0 {
                    Token::Atom | Token::QuotedAtom => {
                        let atom_name = self.intern_current_atom();
                        self.advance();
                        if !self.at_end() && self.tokens[self.pos].0 == Token::LParen {
                            collected_type_refs.push(atom_name);
                        }
                    }
                    Token::LParen | Token::LBracket | Token::LBrace | Token::BinOpen => {
                        depth += 1;
                        self.advance();
                    }
                    Token::RParen | Token::RBracket | Token::RBrace | Token::BinClose => {
                        depth = depth.saturating_sub(1);
                        self.advance();
                    }
                    Token::Dot if depth == 0 => {
                        let dot_end = self.current_span().end;
                        self.advance();
                        if !self.at_end() && self.current_span().start == dot_end {
                            continue;
                        }
                        break;
                    }
                    _ => {
                        self.advance();
                    }
                }
            }
            let attr_end = self.prev_span();
            Ok(Attr::TypeDef(TypeDef {
                name,
                name_span,
                arity,
                is_opaque,
                span: attr_start.merge(attr_end),
            }, collected_type_refs))
        } else if self.check(&Token::Atom)
            && (self.slice_at(self.pos) == "behaviour"
                || self.slice_at(self.pos) == "behavior")
        {
            self.advance(); // consume 'behaviour'/'behavior'
            self.expect(&Token::LParen)?;
            let name_span = self.current_span();
            let name = self.expect_atom_name()?;
            let name_span = name_span.merge(self.prev_span());
            self.expect(&Token::RParen)?;
            self.expect(&Token::Dot)?;
            Ok(Attr::Behaviour(name, name_span))
        } else if self.check(&Token::Atom) && self.slice_at(self.pos) == "export_type" {
            self.advance(); // consume 'export_type'
            self.expect(&Token::LParen)?;
            self.expect(&Token::LBracket)?;
            let mut funs = Vec::new();
            if !self.check(&Token::RBracket) {
                funs.push(self.parse_fun_arity()?);
                while self.eat(&Token::Comma) {
                    funs.push(self.parse_fun_arity()?);
                }
            }
            self.expect(&Token::RBracket)?;
            self.expect(&Token::RParen)?;
            self.expect(&Token::Dot)?;
            Ok(Attr::ExportType(funs))
        } else {
            let attribute_start = self.pos;
            let is_compile = self.check(&Token::Atom) && self.slice_at(self.pos) == "compile";
            // Unknown attribute — silently skip to the form-terminating dot.
            //
            // We cannot simply scan for the first Token::Dot because type
            // syntax uses '..' (integer ranges like 0..255) and '...'
            // (fun((...) -> T)) which lex as multiple adjacent Dot tokens.
            //
            // Strategy: track nesting depth of ()[]{}<<>>. A Dot at depth 0
            // is the terminating dot only if the next token is NOT immediately
            // adjacent (in Erlang, the form-ending dot is always followed by
            // whitespace or EOF).
            let mut depth: u32 = 0;
            while !self.at_end() {
                match &self.tokens[self.pos].0 {
                    Token::LParen | Token::LBracket | Token::LBrace | Token::BinOpen => {
                        depth += 1;
                        self.advance();
                    }
                    Token::RParen | Token::RBracket | Token::RBrace | Token::BinClose => {
                        depth = depth.saturating_sub(1);
                        self.advance();
                    }
                    Token::Dot if depth == 0 => {
                        let dot_end = self.current_span().end;
                        self.advance(); // consume the dot
                        // If the very next token starts immediately after this
                        // dot (no whitespace), the dot is part of '..' or '...'
                        // or a range like '0..255' — keep scanning.
                        if !self.at_end() && self.current_span().start == dot_end {
                            continue;
                        }
                        break;
                    }
                    _ => {
                        self.advance();
                    }
                }
            }
            if is_compile {
                // Revisit only the option term, not unrelated attributes. Lists
                // are option containers; tuples belonging to other options are opaque.
                let end = self.pos;
                self.pos = attribute_start + 1;
                self.expect(&Token::LParen)?;
                let mut suppression = NoAutoImport::default();
                let mut flags = CompileFlags::default();
                self.parse_compile_option(&mut suppression, &mut flags)?;
                self.expect(&Token::RParen)?;
                self.expect(&Token::Dot)?;
                debug_assert_eq!(self.pos, end);
                Ok(Attr::Compile(suppression, flags))
            } else {
                Ok(Attr::Unknown)
            }
        }
    }

    fn parse_compile_option(
        &mut self,
        suppression: &mut NoAutoImport,
        flags: &mut CompileFlags,
    ) -> Result<(), ParseError> {
        if self.eat(&Token::LBracket) {
            if !self.check(&Token::RBracket) {
                loop {
                    self.parse_compile_option(suppression, flags)?;
                    if !self.eat(&Token::Comma) { break; }
                }
            }
            self.expect(&Token::RBracket)?;
        } else if self.check(&Token::LBrace)
            && self.tokens.get(self.pos + 1).is_some_and(|(token, _)| matches!(token, Token::Atom | Token::QuotedAtom))
            && self.slice_at(self.pos + 1).trim_matches('\'') == "no_auto_import"
        {
            self.advance(); // {
            self.advance(); // no_auto_import
            self.expect(&Token::Comma)?;
            self.parse_no_auto_functions(&mut suppression.functions)?;
            self.expect(&Token::RBrace)?;
        } else if self.check(&Token::Atom) || self.check(&Token::QuotedAtom) {
            let option = self.expect_atom_name()?;
            match self.resolve(option) {
                Some("no_auto_import") => suppression.all = true,
                Some("export_all") => flags.export_all = true,
                _ => {}
            }
        } else {
            // Other options remain opaque, including their nested atoms.
            // Preserve the existing EUnit transform without interpreting arbitrary tuples.
            if let Some(t) = self.tokens.get(self.pos..self.pos + 5) {
                flags.eunit_autoexport |= t[0].0 == Token::LBrace && t[2].0 == Token::Comma && t[4].0 == Token::RBrace
                    && self.source[t[1].1.start..t[1].1.end].trim_matches('\'') == "parse_transform"
                    && self.source[t[3].1.start..t[3].1.end].trim_matches('\'') == "eunit_autoexport";
            }
            self.skip_compile_term();
        }
        Ok(())
    }

    /// Skip one opaque option term, keeping its containing list/tuple intact.
    fn skip_compile_term(&mut self) {
        let mut depth = 0u32;
        while !self.at_end() {
            match self.tokens[self.pos].0 {
                Token::Comma | Token::RParen | Token::RBracket | Token::RBrace if depth == 0 => break,
                Token::LParen | Token::LBracket | Token::LBrace | Token::BinOpen => depth += 1,
                Token::RParen | Token::RBracket | Token::RBrace | Token::BinClose => depth = depth.saturating_sub(1),
                _ => {}
            }
            self.advance();
        }
    }

    /// OTP accepts both F/A source shorthand and {F,A} terms in option lists.
    fn parse_no_auto_functions(&mut self, functions: &mut Vec<FunArity>) -> Result<(), ParseError> {
        if self.eat(&Token::LBracket) {
            if !self.check(&Token::RBracket) {
                loop {
                    self.parse_no_auto_functions(functions)?;
                    if !self.eat(&Token::Comma) { break; }
                }
            }
            self.expect(&Token::RBracket)?;
        } else {
            // erl_lint ignores non-signatures in this option. Attempt the two
            // signature forms, then skip the complete term if it is not one.
            let start_pos = self.pos;
            let start_span = self.current_span();
            let signature = (|| -> Result<_, ParseError> {
                let tuple = self.eat(&Token::LBrace);
                let name = self.expect_atom_name()?;
                self.expect(if tuple { &Token::Comma } else { &Token::Slash })?;
                let arity = self.expect_integer()?;
                if tuple { self.expect(&Token::RBrace)?; }
                Ok((name, arity))
            })();
            if let Ok((name, arity)) = signature {
                if let Ok(arity) = u32::try_from(arity) {
                    functions.push(FunArity { name, arity, span: start_span.merge(self.prev_span()) });
                }
            } else {
                self.pos = start_pos;
                self.skip_compile_term();
            }
        }
        Ok(())
    }

    /// Preserve function identities from shorthand and OTP tuple attributes.
    /// Reasons/removal markers are accepted but are not part of the AST yet.
    fn parse_deprecated_spec(&mut self) -> Result<DeprecatedSpec, ParseError> {
        let start = self.current_span();
        let tuple = self.eat(&Token::LBrace);
        let name = self.expect_atom_name()?;
        if !tuple && !self.check(&Token::Slash) && name == self.intern("module") {
            return Ok(DeprecatedSpec {
                name: None,
                arity: None,
                span: start.merge(self.prev_span()),
            });
        }
        self.expect(if tuple { &Token::Comma } else { &Token::Slash })?;
        let arity = if self.check(&Token::QuotedAtom) && self.slice_at(self.pos) == "'_'" {
            self.advance();
            None
        } else {
            let span = self.current_span();
            let arity = self.expect_integer()?;
            if !(0..=255).contains(&arity) {
                return Err(ParseError::UnexpectedToken {
                    expected: "function arity between 0 and 255".into(),
                    found: arity.to_string(),
                    span,
                    hint: None,
                });
            }
            Some(arity as u32)
        };
        if tuple {
            if self.eat(&Token::Comma) {
                if self.check(&Token::String) {
                    while self.eat(&Token::String) {}
                } else {
                    self.expect_atom_name()?;
                }
            }
            self.expect(&Token::RBrace)?;
        }
        Ok(DeprecatedSpec {
            name: (name != self.intern("_")).then_some(name),
            arity,
            span: start.merge(self.prev_span()),
        })
    }

    /// Parse `Name/Arity`.
    fn parse_fun_arity(&mut self) -> Result<FunArity, ParseError> {
        let start = self.current_span();
        let name = self.expect_atom_name()?;
        self.expect(&Token::Slash)?;
        let arity = self.expect_integer()? as u32;
        let span = start.merge(self.prev_span());
        Ok(FunArity { name, arity, span })
    }

    /// Parse comma-separated record field definitions (stops at `}`).
    fn parse_record_field_defs(&mut self) -> Result<Vec<RecordFieldDef>, ParseError> {
        let mut fields = Vec::new();
        if self.check(&Token::RBrace) {
            return Ok(fields);
        }
        fields.push(self.parse_record_field_def()?);
        while self.eat(&Token::Comma) {
            fields.push(self.parse_record_field_def()?);
        }
        Ok(fields)
    }

    /// Parse a single record field definition: `name`, `name = default_expr`,
    /// `name :: Type`, or `name = default_expr :: Type`.
    /// Type annotations are parsed and discarded (Nera is type-erased).
    fn parse_record_field_def(&mut self) -> Result<RecordFieldDef, ParseError> {
        let name_span = self.current_span();
        let name = self.expect_atom_name()?;
        let default = if self.eat(&Token::Equals) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        // Optional `:: Type` annotation. Two adjacent `Colon` tokens.
        if self.pos + 1 < self.tokens.len()
            && self.tokens[self.pos].0 == Token::Colon
            && self.tokens[self.pos + 1].0 == Token::Colon
        {
            self.advance();
            self.advance();
            self.skip_record_field_type();
        }
        let end = self.prev_span();
        Ok(RecordFieldDef {
            name,
            name_span,
            default,
            span: name_span.merge(end),
        })
    }

    /// Skip a record field type annotation. Consumes tokens until a `,` or `}`
    /// at depth 0. Tracks ()[]{}<<>> nesting so types like `[pid()]`,
    /// `{atom(), integer()}`, and `#{atom() => term()}` parse correctly.
    fn skip_record_field_type(&mut self) {
        let mut depth: u32 = 0;
        while !self.at_end() {
            let tok = &self.tokens[self.pos].0;
            match tok {
                Token::LParen | Token::LBracket | Token::LBrace | Token::BinOpen => {
                    depth += 1;
                    self.advance();
                }
                Token::RParen | Token::RBracket | Token::BinClose => {
                    depth = depth.saturating_sub(1);
                    self.advance();
                }
                Token::RBrace => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                    self.advance();
                }
                Token::Comma if depth == 0 => return,
                _ => self.advance(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Function parsing
// ---------------------------------------------------------------------------

impl<'src> Parser<'src> {
    fn parse_function(&mut self) -> Result<Function, ParseError> {
        let start_span = self.current_span();
        let name_span = start_span;
        let name = self.expect_atom_name()?;

        // Set function name context early for ?FUNCTION_NAME in first clause
        self.current_function_name = Some(name);

        let first_clause = self.parse_function_clause(name, name_span)?;
        let arity = first_clause.patterns.len() as u32;
        let mut clauses = vec![first_clause];

        while self.eat(&Token::Semicolon) {
            // Next clause: must have same function name
            let clause_name = self.expect_atom_name()?;
            let clause_name_span = self.prev_span();
            if clause_name != name {
                return Err(ParseError::UnexpectedToken {
                    expected: "function clause for same function".to_string(),
                    found: "different function name".to_string(),
                    span: clause_name_span,
                    hint: None,
                });
            }
            let clause = self.parse_function_clause(name, clause_name_span)?;
            if clause.patterns.len() as u32 != arity {
                return Err(ParseError::HeadMismatch {
                    function_name: self.resolve(name).unwrap_or("?").to_string(),
                    expected_arity: arity,
                    found_arity: clause.patterns.len() as u32,
                    span: clause_name_span,
                });
            }
            clauses.push(clause);
        }

        self.expect(&Token::Dot)?;
        let end_span = self.prev_span();

        // Clear function context
        self.current_function_name = None;
        self.current_function_arity = None;

        Ok(Function {
            name,
            name_span,
            arity,
            clauses,
            span: start_span.merge(end_span),
        })
    }

    fn parse_function_clause(
        &mut self,
        _name: AtomId,
        name_span: Span,
    ) -> Result<FunctionClause, ParseError> {
        let start_span = self.current_span();
        self.expect(&Token::LParen)?;
        let patterns = self.parse_pattern_list()?;
        self.expect_rparen_after_args()?;

        // Set arity context for ?FUNCTION_ARITY (available in guard and body)
        self.current_function_arity = Some(patterns.len() as u32);

        let guard = if self.eat(&Token::KwWhen) {
            Some(self.parse_guard()?)
        } else {
            None
        };

        self.expect_or(&Token::Arrow, "missing '->' before clause body")?;
        let body = self.parse_body()?;
        let end_span = self.prev_span();

        Ok(FunctionClause {
            name_span,
            patterns,
            guard,
            body,
            span: start_span.merge(end_span),
        })
    }
}

// ---------------------------------------------------------------------------
// Guard parsing
// ---------------------------------------------------------------------------

impl<'src> Parser<'src> {
    fn parse_guard(&mut self) -> Result<Guard, ParseError> {
        let first = self.parse_guard_conjunction()?;

        if self.check(&Token::Semicolon) {
            // Check if this is a guard disjunction (`;` between guards)
            // vs. a clause separator. We peek: if after `;` there is no
            // `->` or pattern-start that would indicate a new clause,
            // treat it as guard disjunction. However, the simplest rule:
            // in function clauses the `;` separates clauses (preceded by
            // complete body), while in case/if the guard `;` comes before `->`.
            // Since we call parse_guard only when we already consumed `when`,
            // and the guard ends at `->`, we can safely collect `;` separated
            // guards until we see `->`.
            let mut disj = vec![first];
            while self.check(&Token::Semicolon) && !self.lookahead_is_clause_end() {
                self.advance(); // consume `;`
                disj.push(self.parse_guard_conjunction()?);
            }
            if disj.len() == 1 {
                Ok(disj.into_iter().next().unwrap())
            } else {
                Ok(Guard::Disjunction(disj))
            }
        } else {
            Ok(first)
        }
    }

    fn parse_guard_conjunction(&mut self) -> Result<Guard, ParseError> {
        let first = self.parse_guard_test()?;
        let mut exprs = vec![first];

        while self.eat(&Token::Comma) {
            exprs.push(self.parse_guard_test()?);
        }

        if exprs.len() == 1 {
            Ok(Guard::Test(Box::new(exprs.into_iter().next().unwrap())))
        } else {
            Ok(Guard::Conjunction(exprs))
        }
    }

    fn parse_guard_test(&mut self) -> Result<Expr, ParseError> {
        // Guard tests are expressions (but restricted in real Erlang).
        // We reuse the expression parser here for simplicity.
        self.parse_expr()
    }

    /// Check whether the next semicolon is actually a clause separator
    /// (i.e. after it we see a pattern or atom that starts a new clause).
    fn lookahead_is_clause_end(&self) -> bool {
        // If after `;` the next non-consumed token is `->`, it could be
        // a clause-separating semicolon. We use a simple heuristic:
        // We don't consume `;` for guard disjunction if the token after
        // `;` is a pattern-like thing followed eventually by `->`.
        // For simplicity, we do NOT try to disambiguate and just always
        // stop at `;` in guard context. The caller (case/function) handles
        // the clause separators.
        false
    }
}

// ---------------------------------------------------------------------------
// Expression parsing (Pratt / precedence climbing)
// ---------------------------------------------------------------------------

impl<'src> Parser<'src> {
    /// Parse a comma-separated body of expressions.
    fn parse_body(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut exprs = vec![self.parse_expr()?];
        while self.eat(&Token::Comma) {
            exprs.push(self.parse_expr()?);
        }
        Ok(exprs)
    }

    /// Parse the body of a `maybe` expression: comma-separated `MaybeExpr` items.
    fn parse_maybe_body(&mut self) -> Result<Vec<MaybeExpr>, ParseError> {
        let mut items = vec![self.parse_maybe_item()?];
        while self.eat(&Token::Comma) {
            items.push(self.parse_maybe_item()?);
        }
        Ok(items)
    }

    /// Parse a single item in a `maybe` body.
    /// Try `Pattern ?= Expr` first (with backtracking), else parse plain `Expr`.
    fn parse_maybe_item(&mut self) -> Result<MaybeExpr, ParseError> {
        let save = self.pos;
        // Try to parse as `Pattern ?= Expr`
        if let Ok(pat) = self.parse_pattern() {
            if self.eat(&Token::MaybeMatch) {
                let start = self.tokens[save].1;
                let rhs = self.parse_expr()?;
                let end = self.prev_span();
                return Ok(MaybeExpr::CondMatch(pat, rhs, start.merge(end)));
            }
        }
        // Backtrack and parse as plain expression
        self.pos = save;
        let expr = self.parse_expr()?;
        Ok(MaybeExpr::Expr(expr))
    }

    /// Parse a standalone Erlang term (for `.app` / `.app.src` files).
    ///
    /// Parses a single expression and optionally consumes a trailing dot.
    pub fn parse_term(&mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_expr_bp(0)?;
        // Consume optional trailing dot (`.app` files end with `Term.`)
        self.eat(&Token::Dot);
        Ok(expr)
    }

    /// Entry point: parse a single expression.
    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_expr_bp(0)
    }

    /// Parse a complete preprocessor condition, including its directive delimiters.
    pub(crate) fn parse_condition(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Token::LParen)?;
        let expr = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::Dot)?;
        if !self.at_end() {
            return Err(ParseError::UnexpectedToken {
                expected: "end of conditional directive".into(),
                found: "trailing tokens".into(),
                span: expr.span(), hint: None,
            });
        }
        Ok(expr)
    }

    /// Pratt parser with binding power `min_bp`.
    fn parse_expr_bp(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_unary()?;

        loop {
            if self.at_end() {
                break;
            }

            // Try to get an infix operator from the current token.
            let (op_info, right_bp) = match self.peek_infix_op() {
                Some(info) => info,
                None => break,
            };

            let left_bp = op_info.left_bp;
            if left_bp < min_bp {
                break;
            }

            // Consume the operator token(s).
            let op_span = self.current_span();
            self.advance();

            let rhs = self.parse_expr_bp(right_bp)?;
            let span = expr_span(&lhs).merge(expr_span(&rhs));

            lhs = match op_info.kind {
                InfixKind::BinOp(op) => {
                    Expr::BinOp(op, Box::new(lhs), Box::new(rhs), span)
                }
                InfixKind::Send => Expr::Send(Box::new(lhs), Box::new(rhs), span),
                InfixKind::Match => {
                    // lhs must be convertible to a pattern.
                    let pat = self.expr_to_pattern(lhs, op_span)?;
                    Expr::Match(Box::new(pat), Box::new(rhs), span)
                }
            };
        }

        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.check(&Token::Plus) {
            let span_start = self.current_span();
            self.advance();
            let operand = self.parse_expr_bp(14)?;
            let span = span_start.merge(expr_span(&operand));
            return Ok(Expr::UnaryOp(UnaryOp::Pos, Box::new(operand), span));
        }
        if self.check(&Token::Minus) {
            let span_start = self.current_span();
            self.advance();
            // Check for negative numeric literal (fold constant).
            if let Some((Token::Integer(n), int_span)) = self.peek_token() {
                let n = *n;
                let int_span = *int_span;
                self.advance();
                return Ok(Expr::Integer(-n, span_start.merge(int_span)));
            }
            if let Some((Token::BigInteger(value), int_span)) = self.peek_token() {
                let value = format!("-{value}");
                let span = span_start.merge(*int_span);
                self.advance();
                return Ok(match value.parse::<i64>() {
                    Ok(n) => Expr::Integer(n, span),
                    Err(_) => Expr::BigInteger(value, span),
                });
            }
            if let Some((Token::Float(f), f_span)) = self.peek_token() {
                let f = *f;
                let f_span = *f_span;
                self.advance();
                return Ok(Expr::Float(-f, span_start.merge(f_span)));
            }
            let operand = self.parse_expr_bp(14)?; // high bp for unary
            let span = span_start.merge(expr_span(&operand));
            return Ok(Expr::UnaryOp(UnaryOp::Neg, Box::new(operand), span));
        }
        if self.check(&Token::KwNot) {
            let span_start = self.current_span();
            self.advance();
            let operand = self.parse_expr_bp(14)?;
            let span = span_start.merge(expr_span(&operand));
            return Ok(Expr::UnaryOp(UnaryOp::Not, Box::new(operand), span));
        }
        if self.check(&Token::KwBnot) {
            let span_start = self.current_span();
            self.advance();
            let operand = self.parse_expr_bp(14)?;
            let span = span_start.merge(expr_span(&operand));
            return Ok(Expr::UnaryOp(UnaryOp::Bnot, Box::new(operand), span));
        }
        // `catch Expr` — unary prefix with very low precedence.
        // catch captures everything to its right (lower than = and !).
        if self.check(&Token::KwCatch) {
            let span_start = self.current_span();
            self.advance();
            let operand = self.parse_expr_bp(0)?;
            let span = span_start.merge(expr_span(&operand));
            return Ok(Expr::Catch(Box::new(operand), span));
        }
        let mut expr = self.parse_primary()?;

        // A remote call binds before a trailing fun application. Keep this
        // outside parse_primary: binary segments use that parser and own their
        // size colon, including in <<(Value):Size>>.
        if self.check(&Token::Colon)
            && self.tokens.get(self.pos + 1).map(|t| &t.0) != Some(&Token::Colon)
        {
            self.advance();
            let function = self.parse_remote_function()?;
            self.expect(&Token::LParen)?;
            let args = self.parse_expr_list()?;
            self.expect_rparen_after_args()?;
            let span = expr_span(&expr).merge(self.prev_span());
            expr = Expr::Call(
                CallTarget::Remote(Box::new(expr), Box::new(function)),
                args,
                span,
            );
        }

        // Postfix call: Expr(args...).
        // Enables `(fun() -> 42 end)(0)`, `(dup1())()`, `(id(F))(x)`,
        // `foo(1)(2)`, etc. Matches Erlang semantics: any expression whose
        // value is a fun may be applied; otherwise `badfun` at runtime.
        while self.check(&Token::LParen) {
            self.advance();
            let args = self.parse_expr_list()?;
            self.expect_rparen_after_args()?;
            let end = self.prev_span();
            let span = expr_span(&expr).merge(end);
            expr = Expr::Call(CallTarget::Expr(Box::new(expr)), args, span);
        }

        // Postfix: map update Expr#{...}, record update Expr#name{...},
        //          record access Expr#name.field
        while self.check(&Token::Hash) && self.pos + 1 < self.tokens.len() {
            let next_tok = &self.tokens[self.pos + 1].0;
            if *next_tok == Token::LBrace {
                // Map update: Expr#{...}
                self.advance(); // consume #
                self.advance(); // consume {
                let fields = self.parse_map_update_fields()?;
                self.expect(&Token::RBrace)?;
                let end = self.prev_span();
                let span = expr_span(&expr).merge(end);
                expr = Expr::MapUpdate(Box::new(expr), fields, span);
            } else if *next_tok == Token::Atom
                || self.is_keyword_atom(next_tok)
                || self.is_callable_keyword(next_tok)
            {
                // Record update or access: Expr#name{...} or Expr#name.field
                self.advance(); // consume #
                let record_name = self.expect_atom_name()?;
                if self.check(&Token::LBrace) {
                    // Record update: Expr#name{...}
                    self.advance(); // consume {
                    let fields = self.parse_record_fields_expr()?;
                    self.expect(&Token::RBrace)?;
                    let end = self.prev_span();
                    let span = expr_span(&expr).merge(end);
                    expr = Expr::RecordUpdate {
                        base: Box::new(expr),
                        record_name,
                        fields,
                        span,
                    };
                } else if self.eat(&Token::Dot) {
                    // Record access: Expr#name.field
                    let field_name_span = self.current_span();
                    let field_name = self.expect_atom_name()?;
                    let end = self.prev_span();
                    let span = expr_span(&expr).merge(end);
                    expr = Expr::RecordAccess {
                        base: Box::new(expr),
                        record_name,
                        field_name,
                        field_name_span,
                        span,
                    };
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(expr)
    }

    /// Parse the function position without consuming the remote call's args.
    /// In particular, `m:(F)()` applies the remote function, not F as a fun.
    fn parse_remote_function(&mut self) -> Result<Expr, ParseError> {
        let span = self.current_span();
        if self.check(&Token::Variable) {
            let name = self.slice_at(self.pos).to_owned();
            self.advance();
            Ok(Expr::Variable(self.intern(&name), span))
        } else if self.check_atom_or_keyword() {
            let name = self.expect_atom_name()?;
            Ok(Expr::Atom(name, span))
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        if self.at_end() {
            return Err(ParseError::UnexpectedEof { position: self.current_span().start });
        }

        let tok = self.tokens[self.pos].0.clone();
        let span = self.tokens[self.pos].1;

        match tok {
            // ── Integer literal ──
            Token::Integer(n) => {
                self.advance();
                Ok(Expr::Integer(n, span))
            }

            // ── Float literal ──
            Token::BigInteger(value) => {
                self.advance();
                Ok(Expr::BigInteger(value, span))
            }
            Token::Float(f) => {
                self.advance();
                Ok(Expr::Float(f, span))
            }

            // ── String literal (with implicit concatenation) ──
            Token::String => {
                let raw = self.slice_at(self.pos);
                let content = &raw[1..raw.len() - 1];
                let mut s = unescape_erlang_string(content);
                self.advance();
                // Adjacent string literals are concatenated (Erlang standard)
                while self.check(&Token::String) {
                    let raw = self.slice_at(self.pos);
                    let next = &raw[1..raw.len() - 1];
                    s.push_str(&unescape_erlang_string(next));
                    self.advance();
                }
                let full_span = span.merge(self.prev_span());
                Ok(Expr::String(s, full_span))
            }

            // ── Variable ──
            Token::Variable => {
                let name = self.slice_at(self.pos).to_owned();
                self.advance();
                let id = self.intern(&name);

                // Variable call: F(args...)
                if self.check(&Token::LParen) {
                    self.advance();
                    let args = self.parse_expr_list()?;
                    self.expect_rparen_after_args()?;
                    let end = self.prev_span();
                    return Ok(Expr::Call(
                        CallTarget::Variable(id, span),
                        args,
                        span.merge(end),
                    ));
                }

                Ok(Expr::Variable(id, span))
            }

            // Atoms can be local calls or module names, including true/false
            // and quoted atoms (for example Thoas's imported true/0).
            Token::Atom | Token::KwTrue | Token::KwFalse | Token::KwThrow | Token::QuotedAtom => {
                let raw = self.slice_at(self.pos);
                let atom_text = if self.check(&Token::QuotedAtom) {
                    unescape_erlang_string(&raw[1..raw.len() - 1])
                } else {
                    raw.to_owned()
                };
                let atom_id = self.intern(&atom_text);
                self.advance();

                // Local call: atom(...)
                if self.check(&Token::LParen) {
                    self.advance();
                    let args = self.parse_expr_list()?;
                    self.expect_rparen_after_args()?;
                    let end = self.prev_span();
                    let call_span = span.merge(end);
                    return Ok(Expr::Call(
                        CallTarget::Local(atom_id, span),
                        args,
                        call_span,
                    ));
                }

                Ok(Expr::Atom(atom_id, span))
            }

            // ── ?FUNCTION_NAME / ?FUNCTION_ARITY macros ──
            Token::MacroFunctionName => {
                self.advance();
                match self.current_function_name {
                    Some(name) => Ok(Expr::Atom(name, span)),
                    None => Err(ParseError::UnexpectedToken {
                        expected: "?FUNCTION_NAME inside a function".to_string(),
                        found: "?FUNCTION_NAME outside function context".to_string(),
                        span,
                        hint: None,
                    }),
                }
            }
            Token::MacroFunctionArity => {
                self.advance();
                match self.current_function_arity {
                    Some(arity) => Ok(Expr::Integer(arity as i64, span)),
                    None => Err(ParseError::UnexpectedToken {
                        expected: "?FUNCTION_ARITY inside a function".to_string(),
                        found: "?FUNCTION_ARITY outside function context".to_string(),
                        span,
                        hint: None,
                    }),
                }
            }

            // ── Keyword-as-atom for calls (e.g. self, error, etc.) ──
            // Some keywords can appear as function names in calls
            // Handled via atom_or_keyword in expect_atom_name

            // ── Tuple ──
            Token::LBrace => {
                self.advance();
                let elems = self.parse_expr_list()?;
                self.expect(&Token::RBrace)?;
                let end = self.prev_span();
                Ok(Expr::Tuple(elems, span.merge(end)))
            }

            // ── List or List Comprehension ──
            Token::LBracket => {
                self.advance();
                if self.eat(&Token::RBracket) {
                    return Ok(Expr::Nil(span.merge(self.prev_span())));
                }
                let first = self.parse_expr()?;
                // Check for list comprehension: [Expr || Qualifiers]
                if self.eat(&Token::DoublePipe) {
                    let qualifiers = self.parse_lc_qualifiers()?;
                    self.expect(&Token::RBracket)?;
                    let end = self.prev_span();
                    return Ok(Expr::ListComp(
                        Box::new(first),
                        qualifiers,
                        span.merge(end),
                    ));
                }
                let list = self.parse_list_tail(first, span)?;
                Ok(list)
            }

            // ── Binary expression or binary comprehension ──
            Token::BinOpen => {
                self.advance();
                let segments = self.parse_bin_segments_expr()?;
                // Check for binary comprehension: << Body || Qualifiers >>
                if self.eat(&Token::DoublePipe) {
                    // Binary comprehension: extract body expression from the single segment
                    let body_expr = if segments.len() == 1 {
                        segments.into_iter().next().unwrap().value
                    } else {
                        // Multiple segments or empty: wrap back into Binary
                        Expr::Binary(segments, span)
                    };
                    let qualifiers = self.parse_lc_qualifiers()?;
                    self.expect(&Token::BinClose)?;
                    let end = self.prev_span();
                    return Ok(Expr::BinComp(
                        Box::new(body_expr),
                        qualifiers,
                        span.merge(end),
                    ));
                }
                self.expect(&Token::BinClose)?;
                let end = self.prev_span();
                Ok(Expr::Binary(segments, span.merge(end)))
            }

            // ── Map / record expression: #{K => V} or #name{...} or #name.field ──
            Token::Hash => {
                self.advance(); // consume #
                if self.check(&Token::LBrace) {
                    // Map: #{...}
                    self.advance(); // consume {
                    let fields = self.parse_map_fields_expr()?;
                    self.expect(&Token::RBrace)?;
                    let end = self.prev_span();
                    Ok(Expr::Map(fields, span.merge(end)))
                } else if self.check_atom_or_keyword() {
                    // Record: #name{...} or #name.field
                    let record_name = self.expect_atom_name()?;
                    if self.check(&Token::LBrace) {
                        // Record creation: #name{...}
                        self.advance(); // consume {
                        let fields = self.parse_record_fields_expr()?;
                        self.expect(&Token::RBrace)?;
                        let end = self.prev_span();
                        Ok(Expr::RecordCreate {
                            record_name,
                            fields,
                            span: span.merge(end),
                        })
                    } else if self.eat(&Token::Dot) {
                        // Record index: #name.field
                        let field_name_span = self.current_span();
                        let field_name = self.expect_atom_name()?;
                        let end = self.prev_span();
                        Ok(Expr::RecordIndex {
                            record_name,
                            field_name,
                            field_name_span,
                            span: span.merge(end),
                        })
                    } else {
                        Err(ParseError::UnexpectedToken {
                            expected: "'{' or '.' after record name".into(),
                            found: self.current_description(),
                            span: self.current_span(),
                            hint: None,
                        })
                    }
                } else {
                    Err(ParseError::UnexpectedToken {
                        expected: "'{' or atom after '#'".into(),
                        found: self.current_description(),
                        span: self.current_span(),
                        hint: None,
                    })
                }
            }

            // ── Parenthesized expression / begin-end block ──
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }

            // ── case ──
            Token::KwCase => {
                self.advance();
                let scrutinee = self.parse_expr()?;
                self.expect(&Token::KwOf)?;
                let clauses = self.parse_case_clauses()?;
                self.expect_end_or_hint_semicolon("case")?;
                let end = self.prev_span();
                Ok(Expr::Case(
                    Box::new(scrutinee),
                    clauses,
                    span.merge(end),
                ))
            }

            // ── if ──
            Token::KwIf => {
                self.advance();
                let clauses = self.parse_if_clauses()?;
                self.expect_end_or_hint_semicolon("if")?;
                let end = self.prev_span();
                Ok(Expr::If(clauses, span.merge(end)))
            }

            // ── receive ──
            Token::KwReceive => {
                self.advance();
                let mut clauses = Vec::new();
                let mut after = None;

                // There may be no clauses if it's `receive after ...`
                if !self.check(&Token::KwAfter) && !self.check(&Token::KwEnd) {
                    clauses = self.parse_case_clauses()?;
                }

                if self.eat(&Token::KwAfter) {
                    let after_start = self.prev_span();
                    let timeout = self.parse_expr()?;
                    self.expect_or(&Token::Arrow, "missing '->' before 'after' body")?;
                    let body = self.parse_body()?;
                    let after_end = self.prev_span();
                    after = Some(AfterClause {
                        timeout: Box::new(timeout),
                        body,
                        span: after_start.merge(after_end),
                    });
                }

                self.expect_end_or_hint_semicolon("receive")?;
                let end = self.prev_span();
                Ok(Expr::Receive(clauses, after, span.merge(end)))
            }

            // ── try ──
            Token::KwTry => {
                self.advance();
                let body = self.parse_body()?;
                let mut of_clauses = Vec::new();
                let mut catch_clauses = Vec::new();
                let mut after_body = None;

                if self.eat(&Token::KwOf) {
                    of_clauses = self.parse_case_clauses()?;
                }

                if self.eat(&Token::KwCatch) {
                    catch_clauses = self.parse_catch_clauses()?;
                }

                if self.eat(&Token::KwAfter) {
                    after_body = Some(self.parse_body()?);
                }

                self.expect_end_or_hint_semicolon("try")?;
                let end = self.prev_span();
                Ok(Expr::Try {
                    body,
                    of_clauses,
                    catch_clauses,
                    after_body,
                    span: span.merge(end),
                })
            }

            // ── begin...end block ──
            Token::KwBegin => {
                self.advance();
                let body = self.parse_body()?;
                self.expect_or(&Token::KwEnd, "did you forget 'end' to close this 'begin' block?")?;
                let end = self.prev_span();
                Ok(Expr::Block(body, span.merge(end)))
            }

            // ── maybe...end expression (EEP-49) or atom ──
            Token::KwMaybe => {
                // Look ahead: if next token can start an expression, parse as maybe block.
                // Otherwise treat as the atom 'maybe'.
                if self.pos + 1 < self.tokens.len() && {
                    let next = &self.tokens[self.pos + 1].0;
                    // Check if the token after 'maybe' can start a maybe body expression
                    matches!(next,
                        Token::Integer(_) | Token::BigInteger(_) | Token::Float(_) | Token::Atom | Token::QuotedAtom
                        | Token::Variable | Token::String | Token::KwTrue | Token::KwFalse
                        | Token::KwCase | Token::KwIf | Token::KwReceive | Token::KwNot
                        | Token::KwBnot | Token::KwFun | Token::KwModule | Token::KwExport
                        | Token::KwImport | Token::KwThrow | Token::KwBegin | Token::KwMaybe
                        | Token::KwTry | Token::LParen | Token::LBrace | Token::LBracket
                        | Token::BinOpen | Token::Hash | Token::Minus
                        | Token::MacroFunctionName | Token::MacroFunctionArity)
                } {
                    self.advance();
                    let body = self.parse_maybe_body()?;
                    let else_clauses = if self.eat(&Token::KwElse) {
                        self.parse_case_clauses()?
                    } else {
                        vec![]
                    };
                    self.expect_end_or_hint_semicolon("maybe")?;
                    let end = self.prev_span();
                    Ok(Expr::Maybe {
                        body,
                        else_clauses,
                        span: span.merge(end),
                    })
                } else {
                    // Treat as atom 'maybe'
                    let id = self.intern("maybe");
                    self.advance();
                    if self.check(&Token::LParen) {
                        self.advance();
                        let args = self.parse_expr_list()?;
                        self.expect_rparen_after_args()?;
                        let end = self.prev_span();
                        return Ok(Expr::Call(CallTarget::Local(id, span), args, span.merge(end)));
                    }
                    Ok(Expr::Atom(id, span))
                }
            }

            // ── Anonymous fun / function reference ──
            Token::KwFun => {
                self.advance();
                self.parse_fun_expr(span)
            }

            // ── Non-reserved keyword atoms used as regular atoms in expressions ──
            // Includes nera attribute keywords and contextual keywords (maybe, else).
            Token::KwModule | Token::KwExport | Token::KwImport
            | Token::KwElse => {
                let text = self.slice_at(self.pos).to_owned();
                let id = self.intern(&text);
                self.advance();

                // Check for function call
                if self.check(&Token::LParen) {
                    self.advance();
                    let args = self.parse_expr_list()?;
                    self.expect_rparen_after_args()?;
                    let end = self.prev_span();
                    return Ok(Expr::Call(
                        CallTarget::Local(id, span),
                        args,
                        span.merge(end),
                    ));
                }

                Ok(Expr::Atom(id, span))
            }

            // Unexpanded macro call: ?NAME or ?NAME(args)
            // Only reached when parsing without preprocessor (e.g. formatter).
            Token::Question => {
                self.advance(); // consume '?'
                let name = self.expect_macro_name()?;
                let name_end = self.prev_span();
                if self.check(&Token::LParen) {
                    self.advance(); // consume '('
                    let args = self.parse_expr_list()?;
                    self.expect(&Token::RParen)?;
                    let end = self.prev_span();
                    Ok(Expr::MacroCall {
                        name,
                        args: Some(args),
                        span: span.merge(end),
                    })
                } else {
                    Ok(Expr::MacroCall {
                        name,
                        args: None,
                        span: span.merge(name_end),
                    })
                }
            }

            _ => Err(ParseError::UnexpectedToken {
                expected: "expression".into(),
                found: tok.description().into(),
                span,
                hint: None,
            }),
        }
    }

    /// Parse a list tail after the first element has been parsed.
    /// Handles `[E1, E2, ... | Tail]` and `[E1, E2, ...]`.
    fn parse_list_tail(&mut self, first: Expr, start: Span) -> Result<Expr, ParseError> {
        let mut elems = vec![first];

        while self.eat(&Token::Comma) {
            elems.push(self.parse_expr()?);
        }

        let tail = if self.eat(&Token::Pipe) {
            self.parse_expr()?
        } else {
            let end = self.current_span();
            Expr::Nil(end)
        };

        self.expect(&Token::RBracket)?;
        let end = self.prev_span();
        let span = start.merge(end);

        // Build nested Cons from right to left.
        let mut result = tail;
        for elem in elems.into_iter().rev() {
            result = Expr::Cons(Box::new(elem), Box::new(result), span);
        }
        Ok(result)
    }

    /// Parse list comprehension qualifiers (comma-separated generators and filters).
    /// A generator is `Pattern <- Expr`, a filter is a boolean expression.
    fn parse_lc_qualifiers(&mut self) -> Result<Vec<LcQualifier>, ParseError> {
        let mut qualifiers = Vec::new();
        qualifiers.push(self.parse_lc_qualifier()?);
        while self.eat(&Token::Comma) {
            qualifiers.push(self.parse_lc_qualifier()?);
        }
        Ok(qualifiers)
    }

    /// Parse a single list/binary comprehension qualifier.
    ///
    /// We try to parse as `Pattern <- Expr` (list generator),
    /// `<<Pattern>> <= Expr` (binary generator), or filter expression.
    fn parse_lc_qualifier(&mut self) -> Result<LcQualifier, ParseError> {
        let start = self.current_span();
        // Save position for potential backtrack
        let saved_pos = self.pos;

        // Try binary generator: <<Pattern>> <= Expr
        if self.check(&Token::BinOpen) {
            let bin_saved = self.pos;
            self.advance(); // consume <<
            if let Ok(segments) = self.parse_bin_segments_pattern() {
                if self.eat(&Token::BinClose)
                    && self.eat(&Token::BinLArrow) {
                        // Binary generator: <<Pattern>> <= Expr
                        let pat = Pattern::Binary(segments, start.merge(self.prev_span()));
                        let expr = self.parse_expr()?;
                        let end = self.prev_span();
                        return Ok(LcQualifier::BinGenerator(pat, Box::new(expr), start.merge(end)));
                    }
            }
            // Not a binary generator — backtrack
            self.pos = bin_saved;
        }

        // Try to parse a pattern
        if let Ok(pattern) = self.parse_pattern() {
            if self.eat(&Token::LArrow) {
                // It's a generator: Pattern <- Expr
                let expr = self.parse_expr()?;
                let end = self.prev_span();
                return Ok(LcQualifier::Generator(pattern, Box::new(expr), start.merge(end)));
            }
            // Not a generator — backtrack and parse as filter expression
            self.pos = saved_pos;
        } else {
            // Pattern parse failed — backtrack and parse as filter expression
            self.pos = saved_pos;
        }

        // Parse as filter expression
        let expr = self.parse_expr()?;
        let end = self.prev_span();
        Ok(LcQualifier::Filter(Box::new(expr), start.merge(end)))
    }

    /// Comma-separated list of expressions (may be empty).
    fn parse_expr_list(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut exprs = Vec::new();
        if self.is_expr_start() {
            exprs.push(self.parse_expr()?);
            while self.eat(&Token::Comma) {
                exprs.push(self.parse_expr()?);
            }
        }
        Ok(exprs)
    }

    /// Check whether the current token can start an expression.
    fn is_expr_start(&self) -> bool {
        if self.at_end() {
            return false;
        }
        matches!(
            self.tokens[self.pos].0,
            Token::Integer(_)
            | Token::BigInteger(_)
                | Token::Float(_)
                | Token::Atom
                | Token::QuotedAtom
                | Token::Variable
                | Token::String
                | Token::KwTrue
                | Token::KwFalse
                | Token::KwCase
                | Token::KwIf
                | Token::KwReceive
                | Token::KwNot
                | Token::KwBnot
                | Token::KwFun
                | Token::KwModule
                | Token::KwExport
                | Token::KwImport
                | Token::KwElse
                | Token::KwThrow
                | Token::KwBegin
                | Token::KwMaybe
                | Token::KwTry
                | Token::LParen
                | Token::LBrace
                | Token::LBracket
                | Token::BinOpen
                | Token::Hash
                | Token::Minus
                | Token::MacroFunctionName
                | Token::MacroFunctionArity
                | Token::Question
        )
    }
}

// ---------------------------------------------------------------------------
// Infix operator info
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum InfixKind {
    BinOp(BinOp),
    Send,
    Match,
}

struct InfixInfo {
    left_bp: u8,
    kind: InfixKind,
}

impl<'src> Parser<'src> {
    /// Return the infix info for the current token, if it is a binary operator.
    fn peek_infix_op(&self) -> Option<(InfixInfo, u8)> {
        if self.at_end() {
            return None;
        }
        let tok = &self.tokens[self.pos].0;
        // Returns (InfixInfo { left_bp, kind }, right_bp)
        // Left-associative:  right_bp = left_bp + 1
        // Right-associative: right_bp = left_bp
        let (left_bp, right_bp, kind) = match tok {
            // Match and Send: lowest precedence, right-associative
            Token::Equals => (1, 1, InfixKind::Match),
            Token::Bang => (1, 1, InfixKind::Send),

            // orelse
            Token::KwOrelse => (2, 3, InfixKind::BinOp(BinOp::Orelse)),

            // andalso
            Token::KwAndalso => (4, 5, InfixKind::BinOp(BinOp::Andalso)),

            // Comparison operators
            Token::Eq => (6, 7, InfixKind::BinOp(BinOp::Eq)),
            Token::NotEq => (6, 7, InfixKind::BinOp(BinOp::NotEq)),
            Token::ExactEq => (6, 7, InfixKind::BinOp(BinOp::ExactEq)),
            Token::ExactNotEq => (6, 7, InfixKind::BinOp(BinOp::ExactNotEq)),
            Token::Lt => (6, 7, InfixKind::BinOp(BinOp::Lt)),
            Token::Gt => (6, 7, InfixKind::BinOp(BinOp::Gt)),
            Token::LtEq => (6, 7, InfixKind::BinOp(BinOp::LtEq)),
            Token::GtEq => (6, 7, InfixKind::BinOp(BinOp::GtEq)),

            // List append/subtract
            Token::PlusPlus => (8, 8, InfixKind::BinOp(BinOp::ListAppend)), // right assoc
            Token::MinusMinus => (8, 9, InfixKind::BinOp(BinOp::ListSubtract)),

            // Additive: + - bor bxor bsl bsr or
            Token::Plus => (10, 11, InfixKind::BinOp(BinOp::Add)),
            Token::Minus => (10, 11, InfixKind::BinOp(BinOp::Sub)),
            Token::KwBor => (10, 11, InfixKind::BinOp(BinOp::Bor)),
            Token::KwBxor => (10, 11, InfixKind::BinOp(BinOp::Bxor)),
            Token::KwBsl => (10, 11, InfixKind::BinOp(BinOp::Bsl)),
            Token::KwBsr => (10, 11, InfixKind::BinOp(BinOp::Bsr)),
            Token::KwOr => (10, 11, InfixKind::BinOp(BinOp::Or)),
            Token::KwXor => (10, 11, InfixKind::BinOp(BinOp::Xor)),

            // Multiplicative: * / div rem band and
            Token::Star => (12, 13, InfixKind::BinOp(BinOp::Mul)),
            Token::Slash => (12, 13, InfixKind::BinOp(BinOp::Div)),
            Token::KwDiv => (12, 13, InfixKind::BinOp(BinOp::IntDiv)),
            Token::KwRem => (12, 13, InfixKind::BinOp(BinOp::Rem)),
            Token::KwBand => (12, 13, InfixKind::BinOp(BinOp::Band)),
            Token::KwAnd => (12, 13, InfixKind::BinOp(BinOp::And)),

            _ => return None,
        };

        Some((InfixInfo { left_bp, kind }, right_bp))
    }
}

// ---------------------------------------------------------------------------
// Pattern parsing
// ---------------------------------------------------------------------------

impl<'src> Parser<'src> {
    fn parse_pattern_list(&mut self) -> Result<Vec<Pattern>, ParseError> {
        let mut pats = Vec::new();
        if self.is_pattern_start() {
            pats.push(self.parse_pattern()?);
            while self.eat(&Token::Comma) {
                pats.push(self.parse_pattern()?);
            }
        }
        Ok(pats)
    }

    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let lhs = self.parse_pattern_primary()?;

        // Check for match pattern (alias): P1 = P2
        if self.eat(&Token::Equals) {
            let rhs = self.parse_pattern()?;
            let span = pattern_span(&lhs).merge(pattern_span(&rhs));
            return Ok(Pattern::Match(Box::new(lhs), Box::new(rhs), span));
        }

        Ok(lhs)
    }

    fn parse_pattern_primary(&mut self) -> Result<Pattern, ParseError> {
        if self.at_end() {
            return Err(ParseError::UnexpectedEof { position: self.current_span().start });
        }

        let tok = self.tokens[self.pos].0.clone();
        let span = self.tokens[self.pos].1;

        match tok {
            Token::Variable => {
                let name = self.slice_at(self.pos).to_owned();
                self.advance();
                if name == "_" {
                    Ok(Pattern::Wildcard(span))
                } else {
                    let id = self.intern(&name);
                    Ok(Pattern::Variable(id, span))
                }
            }
            Token::Integer(n) => {
                self.advance();
                Ok(Pattern::Integer(n, span))
            }
            Token::BigInteger(value) => {
                self.advance();
                Ok(Pattern::BigInteger(value, span))
            }
            Token::Float(f) => {
                self.advance();
                Ok(Pattern::Float(f, span))
            }
            Token::Atom => {
                let text = self.slice_at(self.pos).to_owned();
                let id = self.intern(&text);
                self.advance();
                Ok(Pattern::Atom(id, span))
            }
            Token::QuotedAtom => {
                let raw = self.slice_at(self.pos);
                let content = unescape_erlang_string(&raw[1..raw.len() - 1]);
                let id = self.intern(&content);
                self.advance();
                Ok(Pattern::Atom(id, span))
            }
            Token::KwTrue => {
                self.advance();
                let id = self.intern("true");
                Ok(Pattern::Atom(id, span))
            }
            Token::KwFalse => {
                self.advance();
                let id = self.intern("false");
                Ok(Pattern::Atom(id, span))
            }
            Token::KwThrow => {
                // `throw` is a keyword in expression position but a valid
                // atom in pattern position (unlike `catch`, `if`, `case`,
                // etc., `throw` is not a reserved word in Erlang).
                self.advance();
                let id = self.intern("throw");
                Ok(Pattern::Atom(id, span))
            }
            Token::MacroFunctionName => {
                self.advance();
                match self.current_function_name {
                    Some(name) => Ok(Pattern::Atom(name, span)),
                    None => Err(ParseError::UnexpectedToken {
                        expected: "?FUNCTION_NAME inside a function".to_string(),
                        found: "?FUNCTION_NAME outside function context".to_string(),
                        span,
                        hint: None,
                    }),
                }
            }
            Token::MacroFunctionArity => {
                self.advance();
                match self.current_function_arity {
                    Some(arity) => Ok(Pattern::Integer(arity as i64, span)),
                    None => Err(ParseError::UnexpectedToken {
                        expected: "?FUNCTION_ARITY inside a function".to_string(),
                        found: "?FUNCTION_ARITY outside function context".to_string(),
                        span,
                        hint: None,
                    }),
                }
            }
            Token::String => {
                let raw = self.slice_at(self.pos);
                let mut content = unescape_erlang_string(&raw[1..raw.len() - 1]);
                self.advance();
                // Adjacent string literals are concatenated (Erlang standard)
                while self.check(&Token::String) {
                    let raw = self.slice_at(self.pos);
                    let next = &raw[1..raw.len() - 1];
                    content.push_str(&unescape_erlang_string(next));
                    self.advance();
                }
                let str_span = span.merge(self.prev_span());
                // Check for ++ (string prefix matching in patterns):
                // "prefix" ++ Rest
                if self.check(&Token::PlusPlus) {
                    self.advance(); // consume ++
                    let tail_pattern = self.parse_pattern()?;
                    let full_span = span.merge(self.prev_span());
                    return Ok(Pattern::Concat(
                        Box::new(Pattern::String(content, str_span)),
                        Box::new(tail_pattern),
                        full_span,
                    ));
                }
                Ok(Pattern::String(content, str_span))
            }
            Token::Minus => {
                // Negative number pattern
                self.advance();
                if let Some((Token::Integer(n), int_span)) = self.peek_token() {
                    let n = *n;
                    let int_span = *int_span;
                    self.advance();
                    Ok(Pattern::Integer(-n, span.merge(int_span)))
                } else if let Some((Token::BigInteger(value), int_span)) = self.peek_token() {
                    let value = format!("-{value}");
                    let span = span.merge(*int_span);
                    self.advance();
                    Ok(match value.parse::<i64>() {
                        Ok(n) => Pattern::Integer(n, span),
                        Err(_) => Pattern::BigInteger(value, span),
                    })
                } else if let Some((Token::Float(f), f_span)) = self.peek_token() {
                    let f = *f;
                    let f_span = *f_span;
                    self.advance();
                    Ok(Pattern::Float(-f, span.merge(f_span)))
                } else {
                    Err(ParseError::UnexpectedToken {
                        expected: "number after '-' in pattern".into(),
                        found: self.current_description(),
                        span,
                        hint: None,
                    })
                }
            }
            Token::LBrace => {
                self.advance();
                let elems = self.parse_pattern_list()?;
                self.expect(&Token::RBrace)?;
                let end = self.prev_span();
                Ok(Pattern::Tuple(elems, span.merge(end)))
            }
            Token::LBracket => {
                self.advance();
                if self.eat(&Token::RBracket) {
                    return Ok(Pattern::Nil(span.merge(self.prev_span())));
                }
                let first = self.parse_pattern()?;
                self.parse_pattern_list_tail(first, span)
            }
            Token::BinOpen => {
                // Binary pattern
                self.advance();
                let segments = self.parse_bin_segments_pattern()?;
                self.expect(&Token::BinClose)?;
                let end = self.prev_span();
                Ok(Pattern::Binary(segments, span.merge(end)))
            }
            Token::Hash => {
                // Map pattern: #{K := V, ...} or Record pattern: #name{...}
                self.advance(); // consume #
                if self.check(&Token::LBrace) {
                    // Map pattern
                    self.advance(); // consume {
                    let fields = self.parse_map_fields_pattern()?;
                    self.expect(&Token::RBrace)?;
                    let end = self.prev_span();
                    Ok(Pattern::Map(fields, span.merge(end)))
                } else if self.check_atom_or_keyword() {
                    // Record pattern: #name{...}
                    let record_name = self.expect_atom_name()?;
                    self.expect(&Token::LBrace)?;
                    let fields = self.parse_record_fields_pattern()?;
                    self.expect(&Token::RBrace)?;
                    let end = self.prev_span();
                    Ok(Pattern::Record {
                        record_name,
                        fields,
                        span: span.merge(end),
                    })
                } else {
                    Err(ParseError::UnexpectedToken {
                        expected: "'{' or atom after '#' in pattern".into(),
                        found: self.current_description(),
                        span: self.current_span(),
                        hint: None,
                    })
                }
            }
            Token::LParen => {
                // Parenthesized pattern
                self.advance();
                let pat = self.parse_pattern()?;
                self.expect(&Token::RParen)?;
                Ok(pat)
            }
            // Unexpanded macro call in pattern: ?NAME or ?NAME(args)
            Token::Question => {
                self.advance(); // consume '?'
                let name = self.expect_macro_name()?;
                let name_end = self.prev_span();
                if self.check(&Token::LParen) {
                    self.advance(); // consume '('
                    let args = self.parse_pattern_list()?;
                    self.expect(&Token::RParen)?;
                    let end = self.prev_span();
                    Ok(Pattern::MacroCall {
                        name,
                        args: Some(args),
                        span: span.merge(end),
                    })
                } else {
                    Ok(Pattern::MacroCall {
                        name,
                        args: None,
                        span: span.merge(name_end),
                    })
                }
            }
            // Handle keyword tokens that might appear as atoms in patterns
            _ if self.is_keyword_atom(&tok) => {
                let text = self.slice_at(self.pos).to_owned();
                let id = self.intern(&text);
                self.advance();
                Ok(Pattern::Atom(id, span))
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "pattern".into(),
                found: tok.description().into(),
                span,
                hint: None,
            }),
        }
    }

    fn parse_pattern_list_tail(
        &mut self,
        first: Pattern,
        start: Span,
    ) -> Result<Pattern, ParseError> {
        let mut elems = vec![first];

        while self.eat(&Token::Comma) {
            elems.push(self.parse_pattern()?);
        }

        let tail = if self.eat(&Token::Pipe) {
            self.parse_pattern()?
        } else {
            Pattern::Nil(self.current_span())
        };

        self.expect(&Token::RBracket)?;
        let end = self.prev_span();
        let span = start.merge(end);

        let mut result = tail;
        for elem in elems.into_iter().rev() {
            result = Pattern::Cons(Box::new(elem), Box::new(result), span);
        }
        Ok(result)
    }

    fn is_pattern_start(&self) -> bool {
        if self.at_end() {
            return false;
        }
        matches!(
            self.tokens[self.pos].0,
            Token::Variable
                | Token::Integer(_)
                | Token::BigInteger(_)
                | Token::Float(_)
                | Token::Atom
                | Token::QuotedAtom
                | Token::String
                | Token::KwTrue
                | Token::KwFalse
                | Token::KwThrow
                | Token::Minus
                | Token::LBrace
                | Token::LBracket
                | Token::LParen
                | Token::BinOpen
                | Token::Hash
                | Token::MacroFunctionName
                | Token::MacroFunctionArity
                | Token::Question
        ) || self.is_keyword_atom(&self.tokens[self.pos].0)
    }

    /// Check if a token is a non-reserved keyword that can also be used as an atom.
    /// Includes nera-specific attribute keywords and `maybe`/`else` which, per
    /// OTP 25+, are contextual keywords (only reserved at expression-start position
    /// for `maybe...end` blocks).
    fn is_keyword_atom(&self, tok: &Token) -> bool {
        matches!(tok, Token::KwModule | Token::KwExport | Token::KwImport
                    | Token::KwMaybe | Token::KwElse)
    }

    /// Check if a token is an Erlang reserved keyword.
    fn is_reserved_keyword(tok: &Token) -> bool {
        matches!(
            tok,
            Token::KwAfter
                | Token::KwAnd
                | Token::KwAndalso
                | Token::KwBand
                | Token::KwBegin
                | Token::KwBnot
                | Token::KwBor
                | Token::KwBsl
                | Token::KwBsr
                | Token::KwBxor
                | Token::KwCase
                | Token::KwCatch
                | Token::KwCond
                | Token::KwDiv
                | Token::KwEnd
                | Token::KwFun
                | Token::KwIf
                | Token::KwLet
                | Token::KwNot
                | Token::KwOf
                | Token::KwOr
                | Token::KwOrelse
                | Token::KwReceive
                | Token::KwRem
                | Token::KwTry
                | Token::KwWhen
                | Token::KwXor
        )
    }

    /// Convert an expression to a pattern (for match expressions like `X = E`).
    fn expr_to_pattern(&self, expr: Expr, err_span: Span) -> Result<Pattern, ParseError> {
        match expr {
            Expr::Variable(id, span) => {
                // Check if it's the wildcard
                if let Some(name) = self.resolve_sym(id) {
                    if name == "_" {
                        return Ok(Pattern::Wildcard(span));
                    }
                }
                Ok(Pattern::Variable(id, span))
            }
            Expr::Integer(n, span) => Ok(Pattern::Integer(n, span)),
            Expr::BigInteger(n, span) => Ok(Pattern::BigInteger(n, span)),
            Expr::Float(f, span) => Ok(Pattern::Float(f, span)),
            Expr::Atom(id, span) => Ok(Pattern::Atom(id, span)),
            Expr::String(s, span) => Ok(Pattern::String(s, span)),
            Expr::Nil(span) => Ok(Pattern::Nil(span)),
            Expr::Tuple(elems, span) => {
                let pats: Result<Vec<_>, _> = elems
                    .into_iter()
                    .map(|e| self.expr_to_pattern(e, err_span))
                    .collect();
                Ok(Pattern::Tuple(pats?, span))
            }
            Expr::Cons(head, tail, span) => {
                let hp = self.expr_to_pattern(*head, err_span)?;
                let tp = self.expr_to_pattern(*tail, err_span)?;
                Ok(Pattern::Cons(Box::new(hp), Box::new(tp), span))
            }
            Expr::Binary(segments, span) => {
                let pat_segments: Result<Vec<_>, _> = segments
                    .into_iter()
                    .map(|seg| {
                        let pat_value = self.expr_to_pattern(seg.value, err_span)?;
                        Ok(BinSegment {
                            value: pat_value,
                            size: seg.size,
                            type_spec: seg.type_spec,
                            span: seg.span,
                        })
                    })
                    .collect();
                Ok(Pattern::Binary(pat_segments?, span))
            }
            Expr::Map(fields, span) => {
                let pat_fields: Result<Vec<_>, _> = fields
                    .into_iter()
                    .map(|field| {
                        let pat_value = self.expr_to_pattern(field.value, err_span)?;
                        Ok(MapPatternField {
                            key: field.key,
                            value: pat_value,
                            span: field.span,
                        })
                    })
                    .collect();
                Ok(Pattern::Map(pat_fields?, span))
            }
            Expr::RecordCreate { record_name, fields, span } => {
                let pat_fields: Result<Vec<_>, _> = fields
                    .into_iter()
                    .map(|field| {
                        let pat = self.expr_to_pattern(field.value, err_span)?;
                        Ok(RecordPatternField {
                            field_name: field.field_name,
                            field_name_span: field.field_name_span,
                            pattern: pat,
                            span: field.span,
                        })
                    })
                    .collect();
                Ok(Pattern::Record { record_name, fields: pat_fields?, span })
            }
            Expr::BinOp(BinOp::ListAppend, lhs, rhs, span) => {
                let lhs_pat = self.expr_to_pattern(*lhs, err_span)?;
                let rhs_pat = self.expr_to_pattern(*rhs, err_span)?;
                Ok(Pattern::Concat(Box::new(lhs_pat), Box::new(rhs_pat), span))
            }
            Expr::Match(lhs_pat, rhs_expr, span) => {
                // Alias pattern appearing inside an outer pattern, e.g.
                // `{outer, X = {inner, Y}} = ...`. The LHS is already a
                // Pattern; convert the RHS expression to a pattern.
                let rhs_pat = self.expr_to_pattern(*rhs_expr, err_span)?;
                Ok(Pattern::Match(lhs_pat, Box::new(rhs_pat), span))
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "pattern".into(),
                found: "complex expression".into(),
                span: err_span,
                hint: None,
            }),
        }
    }

    fn resolve_sym(&self, id: AtomId) -> Option<&str> {
        let sym = DefaultSymbol::try_from_usize(id.0 as usize)?;
        self.interner.resolve(sym)
    }
}

// ---------------------------------------------------------------------------
// Binary segment parsing
// ---------------------------------------------------------------------------

impl<'src> Parser<'src> {
    /// Parse comma-separated binary segments in an expression context.
    fn parse_bin_segments_expr(&mut self) -> Result<Vec<BinSegment<Expr>>, ParseError> {
        let mut segments = Vec::new();
        if self.check(&Token::BinClose) {
            return Ok(segments); // empty binary <<>>
        }
        segments.push(self.parse_bin_segment_expr()?);
        while self.eat(&Token::Comma) {
            segments.push(self.parse_bin_segment_expr()?);
        }
        Ok(segments)
    }

    /// Parse a single binary segment expression: `Value[:Size][/TypeSpecifiers]`
    fn parse_bin_segment_expr(&mut self) -> Result<BinSegment<Expr>, ParseError> {
        let start = self.current_span();
        // Handle unary minus for negative numeric literals in binary segments
        // e.g., <<-1.5:64/float>> or <<-42:16>>
        let value = if self.check(&Token::Minus) {
            let neg_start = self.current_span();
            self.advance();
            if let Some((Token::Integer(n), int_span)) = self.peek_token() {
                let n = *n;
                let int_span = *int_span;
                self.advance();
                Expr::Integer(-n, neg_start.merge(int_span))
            } else if let Some((Token::BigInteger(value), int_span)) = self.peek_token() {
                let value = format!("-{value}");
                let span = neg_start.merge(*int_span);
                self.advance();
                match value.parse::<i64>() {
                    Ok(n) => Expr::Integer(n, span),
                    Err(_) => Expr::BigInteger(value, span),
                }
            } else if let Some((Token::Float(f), f_span)) = self.peek_token() {
                let f = *f;
                let f_span = *f_span;
                self.advance();
                Expr::Float(-f, neg_start.merge(f_span))
            } else {
                return Err(ParseError::UnexpectedToken {
                    expected: "numeric literal after '-' in binary segment".into(),
                    found: self.current_description(),
                    span: self.current_span(),
                    hint: None,
                });
            }
        } else {
            self.parse_primary()?
        };
        let size = if self.eat(&Token::Colon) {
            Some(Box::new(self.parse_primary()?))
        } else {
            None
        };
        let type_spec = if self.eat(&Token::Slash) {
            self.parse_type_specifiers()?
        } else {
            BinSegmentType::default()
        };
        let end = self.prev_span();
        Ok(BinSegment {
            value,
            size,
            type_spec,
            span: start.merge(end),
        })
    }

    /// Parse comma-separated binary segments in a pattern context.
    fn parse_bin_segments_pattern(&mut self) -> Result<Vec<BinSegment<Pattern>>, ParseError> {
        let mut segments = Vec::new();
        if self.check(&Token::BinClose) {
            return Ok(segments); // empty binary <<>>
        }
        segments.push(self.parse_bin_segment_pattern()?);
        while self.eat(&Token::Comma) {
            segments.push(self.parse_bin_segment_pattern()?);
        }
        Ok(segments)
    }

    /// Parse a single binary segment pattern: `Value[:Size][/TypeSpecifiers]`
    fn parse_bin_segment_pattern(&mut self) -> Result<BinSegment<Pattern>, ParseError> {
        let start = self.current_span();
        let value = self.parse_pattern_primary()?;
        let size = if self.eat(&Token::Colon) {
            Some(Box::new(self.parse_primary()?))
        } else {
            None
        };
        let type_spec = if self.eat(&Token::Slash) {
            self.parse_type_specifiers()?
        } else {
            BinSegmentType::default()
        };
        let end = self.prev_span();
        Ok(BinSegment {
            value,
            size,
            type_spec,
            span: start.merge(end),
        })
    }

    /// Parse written type specifiers without diagnosing semantic conflicts.
    fn parse_type_specifiers(&mut self) -> Result<BinSegmentType, ParseError> {
        let start = self.current_span();
        let mut spec = BinSegmentType { explicit: true, ..BinSegmentType::default() };
        let mut sign = None;
        let mut endian = None;
        let mut alias_kind = None;
        loop {
            let name = self.expect_atom_name()?;
            let name_str = self.resolve(name).unwrap_or("").to_string();
            let written = match name_str.as_str() {
                "integer" | "float" | "binary" | "bytes" | "bitstring" | "bits"
                | "utf8" | "utf16" | "utf32" => {
                    let kind = match name_str.as_str() {
                        "integer" => BinSegmentKind::Integer,
                        "float" => BinSegmentKind::Float,
                        "binary" | "bytes" => BinSegmentKind::Binary,
                        "bitstring" | "bits" => BinSegmentKind::Bitstring,
                        "utf8" => BinSegmentKind::Utf8,
                        "utf16" => BinSegmentKind::Utf16,
                        "utf32" => BinSegmentKind::Utf32,
                        _ => unreachable!(),
                    };
                    if matches!(name_str.as_str(), "bytes" | "bitstring" | "bits") {
                        alias_kind = Some(kind);
                    }
                    spec.typ = kind;
                    if name_str == "bytes" { BinSpecifier::Bytes } else { BinSpecifier::Type(kind) }
                }
                "signed" | "unsigned" => {
                    let value = if name_str == "signed" { Signedness::Signed } else { Signedness::Unsigned };
                    sign = Some(value);
                    BinSpecifier::Signedness(value)
                }
                "big" | "little" | "native" => {
                    let value = match name_str.as_str() {
                        "big" => Endianness::Big,
                        "little" => Endianness::Little,
                        "native" => Endianness::Native,
                        _ => unreachable!(),
                    };
                    endian = Some(value);
                    BinSpecifier::Endianness(value)
                }
                "unit" => {
                    self.expect(&Token::Colon)?;
                    let n = self.expect_integer()?;
                    if !(1..=256).contains(&n) {
                        return Err(ParseError::UnexpectedToken {
                            expected: "binary segment unit in 1..=256".into(),
                            found: n.to_string(),
                            span: self.prev_span(),
                            hint: None,
                        });
                    }
                    spec.unit = Some(n as u32);
                    BinSpecifier::Unit(n as u32)
                }
                _ => {
                    return Err(ParseError::UnexpectedToken {
                        expected: "binary segment type specifier".into(),
                        found: name_str,
                        span: self.prev_span(),
                        hint: None,
                    });
                }
            };
            spec.specifiers.push(written);
            if !self.eat(&Token::Minus) {
                break;
            }
        }
        // Preserve an alias's default unit even when plain binary follows it,
        // without making that implicit unit look like a written `unit:N`.
        if matches!(spec.typ, BinSegmentKind::Binary | BinSegmentKind::Bitstring) {
            spec.typ = alias_kind.unwrap_or(BinSegmentKind::Binary);
        }
        spec.signedness = sign.unwrap_or(Signedness::Unsigned);
        spec.endianness = endian.unwrap_or(Endianness::Big);
        spec.specifiers_span = Some(start.merge(self.prev_span()));
        Ok(spec)
    }
}

// ---------------------------------------------------------------------------
// Case / if clause parsing
// ---------------------------------------------------------------------------

impl<'src> Parser<'src> {
    /// Parse a fun expression after the `fun` keyword has been consumed.
    ///
    /// Forms:
    /// - `fun name/arity` → FunRef (local)
    /// - `fun Module:Function/Arity` → FunRef (remote, static or dynamic)
    /// - `fun(Patterns) [when Guard] -> Body ; ... end` → Fun (anonymous)
    fn parse_fun_expr(&mut self, start: Span) -> Result<Expr, ParseError> {
        // throw is lexed as a keyword, but is an ordinary resolvable function.
        // Check for function reference: fun name/arity or fun M:F/A
        if matches!(self.tokens.get(self.pos).map(|t| &t.0),
            Some(Token::Atom | Token::QuotedAtom | Token::KwThrow | Token::KwTrue | Token::KwFalse)) {
            let save_pos = self.pos;
            let first_id = self.expect_atom_name()?;
            let first_span = self.prev_span();

            if self.eat(&Token::Colon) {
                // fun Module:Function/Arity — components may be dynamic
                let fun_expr = self.parse_fun_ref_component()?;
                self.expect(&Token::Slash)?;
                let arity_expr = self.parse_fun_ref_component()?;
                let end = self.prev_span();
                let span = start.merge(end);

                // Optimise: if all static, use the compact Remote variant
                if let (Expr::Atom(f, _), Expr::Integer(a, _)) = (&fun_expr, &arity_expr) {
                    return Ok(Expr::FunRef(
                        FunRefTarget::Remote(first_id, *f, *a as u32),
                        span,
                    ));
                }
                return Ok(Expr::FunRef(
                    FunRefTarget::DynamicRemote(
                        Box::new(Expr::Atom(first_id, first_span)),
                        Box::new(fun_expr),
                        Box::new(arity_expr),
                    ),
                    span,
                ));
            } else if self.eat(&Token::Slash) {
                // fun name/arity
                let tok = &self.tokens[self.pos].0;
                if let Token::Integer(n) = tok {
                    let arity = *n as u32;
                    self.advance();
                    let end = self.prev_span();
                    return Ok(Expr::FunRef(
                        FunRefTarget::Local(first_id, arity),
                        start.merge(end),
                    ));
                } else {
                    return Err(ParseError::UnexpectedToken {
                        expected: "arity (integer)".into(),
                        found: self.tokens[self.pos].0.description().into(),
                        span: self.current_span(),
                        hint: None,
                    });
                }
            }
            // Check for named fun: fun Name(Patterns) -> Body end
            // If we see atom followed by '(', it's a named fun
            if self.check(&Token::LParen) {
                // Named fun: first_id is the name
                let clauses = self.parse_named_fun_clauses(first_id)?;
                self.expect_end_or_hint_semicolon("fun")?;
                let end = self.prev_span();
                return Ok(Expr::Fun(Some(first_id), clauses, start.merge(end)));
            }

            // Not a fun ref or named fun — backtrack and try as anonymous fun
            self.pos = save_pos;
        }

        // Check for variable-led forms: fun Var:Fun/Arity or fun Name(Patterns) -> Body end
        if self.check(&Token::Variable) {
            let save_pos = self.pos;
            let name_text = self.slice_at(self.pos).to_owned();
            let var_span = self.current_span();
            self.advance();

            if self.eat(&Token::Colon) {
                // fun Var:Fun/Arity — dynamic remote fun ref
                let name_id = self.intern(&name_text);
                let mod_expr = Expr::Variable(name_id, var_span);
                let fun_expr = self.parse_fun_ref_component()?;
                self.expect(&Token::Slash)?;
                let arity_expr = self.parse_fun_ref_component()?;
                let end = self.prev_span();
                return Ok(Expr::FunRef(
                    FunRefTarget::DynamicRemote(
                        Box::new(mod_expr),
                        Box::new(fun_expr),
                        Box::new(arity_expr),
                    ),
                    start.merge(end),
                ));
            }

            if self.check(&Token::LParen) {
                // Named fun with variable-style name
                let name_id = self.intern(&name_text);
                let clauses = self.parse_named_fun_clauses(name_id)?;
                self.expect_end_or_hint_semicolon("fun")?;
                let end = self.prev_span();
                return Ok(Expr::Fun(Some(name_id), clauses, start.merge(end)));
            }
            // Not a named fun — backtrack
            self.pos = save_pos;
        }

        // Anonymous fun: fun(Patterns) [when Guard] -> Body ; ... end
        let clauses = self.parse_fun_clauses()?;
        self.expect_end_or_hint_semicolon("fun")?;
        let end = self.prev_span();
        Ok(Expr::Fun(None, clauses, start.merge(end)))
    }

    /// Parse a component of a dynamic fun reference (M, F, or A).
    /// Accepts: atom, variable, integer, or parenthesized expression.
    fn parse_fun_ref_component(&mut self) -> Result<Expr, ParseError> {
        match &self.tokens[self.pos].0 {
            Token::Atom | Token::QuotedAtom => {
                let text = self.slice_at(self.pos).to_owned();
                let id = self.intern(&text);
                self.advance();
                Ok(Expr::Atom(id, self.prev_span()))
            }
            Token::Variable => {
                let text = self.slice_at(self.pos).to_owned();
                let id = self.intern(&text);
                self.advance();
                Ok(Expr::Variable(id, self.prev_span()))
            }
            Token::Integer(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::Integer(n, self.prev_span()))
            }
            Token::BigInteger(n) => {
                let n = n.clone();
                self.advance();
                Ok(Expr::BigInteger(n, self.prev_span()))
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "atom, variable, integer, or '('".into(),
                found: self.tokens[self.pos].0.description().into(),
                span: self.current_span(),
                hint: None,
            }),
        }
    }

    /// Parse one or more fun clauses: `(Patterns) [when Guard] -> Body`.
    /// Clauses are separated by `;`.
    fn parse_fun_clauses(&mut self) -> Result<Vec<FunctionClause>, ParseError> {
        let mut clauses = vec![self.parse_fun_clause()?];
        while self.eat(&Token::Semicolon) {
            clauses.push(self.parse_fun_clause()?);
        }
        Ok(clauses)
    }

    fn parse_fun_clause(&mut self) -> Result<FunctionClause, ParseError> {
        let start = self.current_span();
        self.expect(&Token::LParen)?;
        let patterns = self.parse_pattern_list()?;
        self.expect_rparen_after_args()?;

        let guard = if self.eat(&Token::KwWhen) {
            Some(self.parse_guard()?)
        } else {
            None
        };

        self.expect_or(&Token::Arrow, "missing '->' before clause body")?;
        let body = self.parse_body()?;
        let end = self.prev_span();

        Ok(FunctionClause {
            name_span: start,
            patterns,
            guard,
            body,
            span: start.merge(end),
        })
    }

    /// Parse one or more named fun clauses: `Name(Patterns) [when Guard] -> Body`.
    /// Each clause must repeat the fun name. Clauses are separated by `;`.
    /// The name can be an atom or a variable (uppercase, which is the common case in Erlang).
    fn parse_named_fun_clauses(&mut self, name: AtomId) -> Result<Vec<FunctionClause>, ParseError> {
        let mut clauses = vec![self.parse_named_fun_clause(name)?];
        while self.eat(&Token::Semicolon) {
            // Each clause must start with the fun name (atom or variable)
            let clause_name = self.expect_fun_clause_name()?;
            if clause_name != name {
                return Err(ParseError::UnexpectedToken {
                    expected: "same fun name in each clause".into(),
                    found: "different name".into(),
                    span: self.current_span(),
                    hint: None,
                });
            }
            clauses.push(self.parse_named_fun_clause(name)?);
        }
        Ok(clauses)
    }

    /// Expect an atom or variable name for a named fun clause header.
    fn expect_fun_clause_name(&mut self) -> Result<AtomId, ParseError> {
        if self.at_end() {
            return Err(ParseError::UnexpectedEof { position: self.current_span().start });
        }
        match &self.tokens[self.pos].0 {
            Token::Variable | Token::Atom => {
                let text = self.slice_at(self.pos).to_owned();
                self.advance();
                Ok(self.intern(&text))
            }
            Token::QuotedAtom => {
                let raw = self.slice_at(self.pos);
                let content = unescape_erlang_string(&raw[1..raw.len() - 1]);
                self.advance();
                Ok(self.intern(&content))
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "fun clause name".into(),
                found: self.current_description(),
                span: self.current_span(),
                hint: None,
            }),
        }
    }

    /// Parse a single named fun clause: `(Patterns) [when Guard] -> Body`.
    /// The name has already been consumed; we expect `(` next.
    fn parse_named_fun_clause(&mut self, _name: AtomId) -> Result<FunctionClause, ParseError> {
        let start = self.current_span();
        self.expect(&Token::LParen)?;
        let patterns = self.parse_pattern_list()?;
        self.expect_rparen_after_args()?;

        let guard = if self.eat(&Token::KwWhen) {
            Some(self.parse_guard()?)
        } else {
            None
        };

        self.expect_or(&Token::Arrow, "missing '->' before clause body")?;
        let body = self.parse_body()?;
        let end = self.prev_span();

        Ok(FunctionClause {
            name_span: start,
            patterns,
            guard,
            body,
            span: start.merge(end),
        })
    }

    fn parse_case_clauses(&mut self) -> Result<Vec<CaseClause>, ParseError> {
        let mut clauses = vec![self.parse_case_clause()?];
        while self.eat(&Token::Semicolon) {
            clauses.push(self.parse_case_clause()?);
        }
        Ok(clauses)
    }

    fn parse_case_clause(&mut self) -> Result<CaseClause, ParseError> {
        let start = self.current_span();
        let pattern = self.parse_pattern()?;

        let guard = if self.eat(&Token::KwWhen) {
            Some(self.parse_guard()?)
        } else {
            None
        };

        self.expect_or(&Token::Arrow, "missing '->' before clause body")?;
        let body = self.parse_body()?;
        let end = self.prev_span();

        Ok(CaseClause {
            pattern,
            guard,
            body,
            span: start.merge(end),
        })
    }

    fn parse_if_clauses(&mut self) -> Result<Vec<IfClause>, ParseError> {
        let mut clauses = vec![self.parse_if_clause()?];
        while self.eat(&Token::Semicolon) {
            clauses.push(self.parse_if_clause()?);
        }
        Ok(clauses)
    }

    fn parse_if_clause(&mut self) -> Result<IfClause, ParseError> {
        let start = self.current_span();
        let guard = self.parse_guard()?;
        self.expect_or(&Token::Arrow, "missing '->' before clause body")?;
        let body = self.parse_body()?;
        let end = self.prev_span();

        Ok(IfClause {
            guard,
            body,
            span: start.merge(end),
        })
    }

    // ── Catch clause parsing ──────────────────────────────────────

    /// Parse catch clauses: `Class:Pattern [when Guard] -> Body ; ...`
    fn parse_catch_clauses(&mut self) -> Result<Vec<CatchClause>, ParseError> {
        let mut clauses = vec![self.parse_catch_clause()?];
        while self.eat(&Token::Semicolon) {
            // Stop if we hit 'after' or 'end' (next clause section)
            if self.check(&Token::KwAfter) || self.check(&Token::KwEnd) {
                break;
            }
            clauses.push(self.parse_catch_clause()?);
        }
        Ok(clauses)
    }

    /// Parse a single catch clause: `[Class:]Pattern[:Stacktrace] [when Guard] -> Body`
    fn parse_catch_clause(&mut self) -> Result<CatchClause, ParseError> {
        let start = self.current_span();

        // Parse the exception class (optional: defaults to throw)
        // Form: `throw:Pattern`, `error:Pattern`, `exit:Pattern`, `_:Pattern`
        let class = self.parse_catch_class()?;

        let pattern = self.parse_pattern()?;

        // Optional stacktrace: `:Stacktrace` (3-part catch)
        // Must be `:` (not `:=` which is ColonEq)
        let stacktrace = if self.check(&Token::Colon) {
            self.advance(); // consume `:`
            Some(self.parse_pattern()?)
        } else {
            None
        };

        let guard = if self.eat(&Token::KwWhen) {
            Some(self.parse_guard()?)
        } else {
            None
        };

        self.expect_or(&Token::Arrow, "missing '->' before clause body")?;
        let body = self.parse_body()?;
        let end = self.prev_span();

        Ok(CatchClause {
            class,
            pattern,
            stacktrace,
            guard,
            body,
            span: start.merge(end),
        })
    }

    /// Parse the exception class prefix of a catch clause.
    /// Looks for `throw:`, `error:`, `exit:`, `Var:`, or `_:`.
    /// If no class prefix, defaults to `throw`.
    fn parse_catch_class(&mut self) -> Result<CatchClass, ParseError> {
        // Try to identify if there's a `Class:` prefix by looking ahead.
        // We need: (atom_or_var) followed by `:` (not `:=` or `::`)
        let saved_pos = self.pos;

        let current = if !self.at_end() { Some(self.tokens[self.pos].0.clone()) } else { None };
        let class_result = match current {
            Some(Token::KwThrow) => {
                self.advance();
                if self.eat(&Token::Colon) {
                    Some(CatchClass::Throw)
                } else {
                    None // Not a class prefix, backtrack
                }
            }
            Some(Token::Atom) => {
                let text = self.slice_at(self.pos).to_owned();
                self.advance();
                if self.eat(&Token::Colon) {
                    match text.as_str() {
                        "error" => Some(CatchClass::Error),
                        "exit" => Some(CatchClass::Exit),
                        "throw" => Some(CatchClass::Throw),
                        _ => {
                            // Treat as a variable-like class binding
                            let id = self.intern(&text);
                            Some(CatchClass::Variable(id, self.tokens[saved_pos].1))
                        }
                    }
                } else {
                    None // Not a class prefix, backtrack
                }
            }
            Some(Token::Variable) => {
                let name = self.slice_at(self.pos).to_owned();
                self.advance();
                if self.eat(&Token::Colon) {
                    if name == "_" {
                        // Wildcard class — match any class. We use a variable
                        // that will be ignored during pattern matching.
                        let id = self.intern("_");
                        Some(CatchClass::Variable(id, self.tokens[saved_pos].1))
                    } else {
                        let id = self.intern(&name);
                        Some(CatchClass::Variable(id, self.tokens[saved_pos].1))
                    }
                } else {
                    None // Not a class prefix, backtrack
                }
            }
            _ => None,
        };

        match class_result {
            Some(class) => Ok(class),
            None => {
                // No class prefix found, restore position and default to 'throw'
                self.pos = saved_pos;
                Ok(CatchClass::Throw)
            }
        }
    }

    // ── Map expression parsing ──────────────────────────────────────

    /// Parse map fields: `K1 => V1, K2 => V2, ...`
    /// May be empty for `#{}`.
    fn parse_map_fields_expr(&mut self) -> Result<Vec<MapField>, ParseError> {
        let mut fields = Vec::new();
        if self.check(&Token::RBrace) {
            return Ok(fields);
        }
        fields.push(self.parse_map_field_expr()?);
        while self.eat(&Token::Comma) {
            fields.push(self.parse_map_field_expr()?);
        }
        Ok(fields)
    }

    /// Parse a single map field: `KeyExpr => ValueExpr` or `KeyExpr := ValueExpr`
    fn parse_map_field_expr(&mut self) -> Result<MapField, ParseError> {
        let start = self.current_span();
        let key = self.parse_expr()?;
        let op_span = self.current_span();
        let op = if self.eat(&Token::FatArrow) {
            MapFieldOp::Assoc
        } else if self.eat(&Token::ColonEq) {
            MapFieldOp::Exact
        } else {
            return Err(ParseError::UnexpectedToken {
                expected: "`=>` or `:=`".to_string(),
                found: self.current_description(),
                span: self.current_span(),
                hint: None,
            });
        };
        let value = self.parse_expr()?;
        let end = self.prev_span();
        Ok(MapField {
            key,
            value,
            op,
            op_span,
            span: start.merge(end),
        })
    }

    // ── Map pattern parsing ─────────────────────────────────────────

    /// Parse map pattern fields: `K1 := V1, K2 := V2, ...`
    /// May be empty for `#{}`.
    fn parse_map_fields_pattern(&mut self) -> Result<Vec<MapPatternField>, ParseError> {
        let mut fields = Vec::new();
        if self.check(&Token::RBrace) {
            return Ok(fields);
        }
        fields.push(self.parse_map_field_pattern()?);
        while self.eat(&Token::Comma) {
            fields.push(self.parse_map_field_pattern()?);
        }
        Ok(fields)
    }

    /// Parse a single map pattern field: `KeyExpr := ValuePattern`
    fn parse_map_field_pattern(&mut self) -> Result<MapPatternField, ParseError> {
        let start = self.current_span();
        let key = self.parse_expr()?;
        self.expect(&Token::ColonEq)?;
        let value = self.parse_pattern()?;
        let end = self.prev_span();
        Ok(MapPatternField {
            key,
            value,
            span: start.merge(end),
        })
    }

    // ── Map update parsing ─────────────────────────────────────────

    /// Parse map update fields: `K1 => V1, K2 := V2, ...`
    /// Each field may use `=>` (assoc) or `:=` (exact update).
    fn parse_map_update_fields(&mut self) -> Result<Vec<MapUpdateField>, ParseError> {
        let mut fields = Vec::new();
        if self.check(&Token::RBrace) {
            return Ok(fields);
        }
        fields.push(self.parse_map_update_field()?);
        while self.eat(&Token::Comma) {
            fields.push(self.parse_map_update_field()?);
        }
        Ok(fields)
    }

    /// Parse a single map update field: `KeyExpr => ValueExpr` or `KeyExpr := ValueExpr`
    fn parse_map_update_field(&mut self) -> Result<MapUpdateField, ParseError> {
        let start = self.current_span();
        let key = self.parse_expr()?;
        let op = if self.eat(&Token::FatArrow) {
            MapUpdateOp::Assoc
        } else if self.eat(&Token::ColonEq) {
            MapUpdateOp::Exact
        } else {
            return Err(ParseError::UnexpectedToken {
                expected: "'=>' or ':='".into(),
                found: format!("{:?}", self.tokens.get(self.pos).map(|t| &t.0)),
                span: self.current_span(),
                hint: None,
            });
        };
        let value = self.parse_expr()?;
        let end = self.prev_span();
        Ok(MapUpdateField {
            key,
            value,
            op,
            span: start.merge(end),
        })
    }
    // ── Record field helpers ────────────────────────────────────────────

    /// Parse comma-separated record fields in expressions: `field1 = Expr1, field2 = Expr2`
    fn parse_record_fields_expr(&mut self) -> Result<Vec<RecordFieldAssign>, ParseError> {
        let mut fields = Vec::new();
        if self.check(&Token::RBrace) {
            return Ok(fields);
        }
        fields.push(self.parse_record_field_expr()?);
        while self.eat(&Token::Comma) {
            fields.push(self.parse_record_field_expr()?);
        }
        Ok(fields)
    }

    /// Parse a single record field assignment: `field_name = Expr`
    fn parse_record_field_expr(&mut self) -> Result<RecordFieldAssign, ParseError> {
        let field_name_span = self.current_span();
        let field_name = self.expect_atom_name()?;
        self.expect(&Token::Equals)?;
        let value = self.parse_expr()?;
        let end = self.prev_span();
        Ok(RecordFieldAssign {
            field_name,
            field_name_span,
            value,
            span: field_name_span.merge(end),
        })
    }

    /// Parse comma-separated record fields in patterns: `field1 = Pat1, field2 = Pat2`
    fn parse_record_fields_pattern(&mut self) -> Result<Vec<RecordPatternField>, ParseError> {
        let mut fields = Vec::new();
        if self.check(&Token::RBrace) {
            return Ok(fields);
        }
        fields.push(self.parse_record_field_pattern()?);
        while self.eat(&Token::Comma) {
            fields.push(self.parse_record_field_pattern()?);
        }
        Ok(fields)
    }

    /// Parse a single record field pattern: `field_name = Pattern`
    fn parse_record_field_pattern(&mut self) -> Result<RecordPatternField, ParseError> {
        let field_name_span = self.current_span();
        let field_name = self.expect_atom_name()?;
        self.expect(&Token::Equals)?;
        let pattern = self.parse_pattern()?;
        let end = self.prev_span();
        Ok(RecordPatternField {
            field_name,
            field_name_span,
            pattern,
            span: field_name_span.merge(end),
        })
    }
}

// ---------------------------------------------------------------------------
// Token helpers
// ---------------------------------------------------------------------------

impl<'src> Parser<'src> {
    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek_token(&self) -> Option<&(Token, Span)> {
        self.tokens.get(self.pos)
    }

    fn current_span(&self) -> Span {
        if self.pos < self.tokens.len() {
            self.tokens[self.pos].1
        } else if !self.tokens.is_empty() {
            let last = self.tokens.last().unwrap().1;
            Span::new(last.end, last.end)
        } else {
            Span::new(0, 0)
        }
    }

    fn prev_span(&self) -> Span {
        if self.pos > 0 {
            self.tokens[self.pos - 1].1
        } else {
            Span::new(0, 0)
        }
    }

    fn advance(&mut self) {
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn check(&self, expected: &Token) -> bool {
        if self.at_end() {
            return false;
        }
        std::mem::discriminant(&self.tokens[self.pos].0) == std::mem::discriminant(expected)
    }

    fn eat(&mut self, expected: &Token) -> bool {
        if self.check(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<(), ParseError> {
        if self.eat(expected) {
            Ok(())
        } else if self.at_end() {
            Err(ParseError::UnexpectedEof { position: self.current_span().start })
        } else {
            Err(ParseError::UnexpectedToken {
                expected: expected.description().into(),
                found: self.current_description(),
                span: self.current_span(),
                hint: None,
            })
        }
    }

    /// Like `expect()` but attaches a contextual hint to the error.
    /// Like `expect_or`, but when `expected` is `end` and the current token
    /// looks like it could start a new clause (pattern or guard), we suggest
    /// the user may have forgotten a `;` between clauses instead of the
    /// generic "did you forget 'end'" hint.
    fn expect_end_or_hint_semicolon(&mut self, block_keyword: &str) -> Result<(), ParseError> {
        if self.eat(&Token::KwEnd) {
            return Ok(());
        }
        if self.at_end() {
            return Err(ParseError::UnexpectedEof { position: self.current_span().start });
        }
        let hint = if self.is_pattern_start() {
            format!("missing ';' between clauses in this '{}' expression?", block_keyword)
        } else {
            format!("did you forget 'end' to close this '{}' expression?", block_keyword)
        };
        Err(ParseError::UnexpectedToken {
            expected: "'end'".into(),
            found: self.current_description(),
            span: self.current_span(),
            hint: Some(hint),
        })
    }

    fn expect_or(&mut self, expected: &Token, hint: &str) -> Result<(), ParseError> {
        if self.eat(expected) {
            Ok(())
        } else if self.at_end() {
            Err(ParseError::UnexpectedEof { position: self.current_span().start })
        } else {
            Err(ParseError::UnexpectedToken {
                expected: expected.description().into(),
                found: self.current_description(),
                span: self.current_span(),
                hint: Some(hint.into()),
            })
        }
    }

    /// Expect `)` after an argument/pattern list; if the current token looks
    /// like the start of another expression, hint about a missing comma.
    fn expect_rparen_after_args(&mut self) -> Result<(), ParseError> {
        if self.eat(&Token::RParen) {
            return Ok(());
        }
        if self.at_end() {
            return Err(ParseError::UnexpectedEof { position: self.current_span().start });
        }
        let hint = if self.is_expr_start() {
            Some("missing comma between arguments?".into())
        } else {
            None
        };
        Err(ParseError::UnexpectedToken {
            expected: "')'".into(),
            found: self.current_description(),
            span: self.current_span(),
            hint,
        })
    }

    fn current_description(&self) -> String {
        if self.at_end() {
            "end of file".into()
        } else {
            let desc = self.tokens[self.pos].0.description();
            // Generic descriptions (atom, integer, etc.) benefit from showing the
            // actual source slice.  Specific descriptions (keywords / punctuation)
            // are already quoted and the slice would be redundant.
            if desc.starts_with('\'') {
                desc.to_string()
            } else {
                format!("{} '{}'", desc, self.slice_at(self.pos))
            }
        }
    }

    /// Get the source slice for the token at the given position.
    fn slice_at(&self, pos: usize) -> &str {
        let span = self.tokens[pos].1;
        &self.source[span.start..span.end]
    }

    /// Expect an atom name (unquoted atom, quoted atom, or a keyword used as an
    /// atom name). Returns the interned `AtomId`.
    fn expect_atom_name(&mut self) -> Result<AtomId, ParseError> {
        if self.at_end() {
            return Err(ParseError::UnexpectedEof { position: self.current_span().start });
        }
        match &self.tokens[self.pos].0 {
            Token::Atom => {
                let text = self.slice_at(self.pos).to_owned();
                self.advance();
                Ok(self.intern(&text))
            }
            Token::QuotedAtom => {
                let raw = self.slice_at(self.pos);
                let content = unescape_erlang_string(&raw[1..raw.len() - 1]);
                self.advance();
                Ok(self.intern(&content))
            }
            // Allow non-reserved keywords as atom names (e.g. function named `module`)
            tok if self.is_keyword_atom(tok) || self.is_callable_keyword(tok) => {
                let text = self.slice_at(self.pos).to_owned();
                self.advance();
                Ok(self.intern(&text))
            }
            tok if Self::is_reserved_keyword(tok) => {
                let keyword = self.slice_at(self.pos).to_owned();
                let span = self.current_span();
                Err(ParseError::ReservedKeyword {
                    keyword,
                    context: "an atom name".into(),
                    span,
                })
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "atom".into(),
                found: self.current_description(),
                span: self.current_span(),
                hint: None,
            }),
        }
    }

    /// Expect a macro name after `?`. Accepts atoms, variables (uppercase macro
    /// names like `ERROR_MSG`), quoted atoms, and keywords.
    fn expect_macro_name(&mut self) -> Result<AtomId, ParseError> {
        if self.at_end() {
            return Err(ParseError::UnexpectedEof { position: self.current_span().start });
        }
        match &self.tokens[self.pos].0 {
            Token::Atom | Token::Variable => {
                let text = self.slice_at(self.pos).to_owned();
                self.advance();
                Ok(self.intern(&text))
            }
            Token::QuotedAtom => {
                let raw = self.slice_at(self.pos);
                let content = unescape_erlang_string(&raw[1..raw.len() - 1]);
                self.advance();
                Ok(self.intern(&content))
            }
            // Keywords can be macro names (e.g. ?MODULE is an atom `module` uppercased
            // by convention, but the lexer sees Variable; some macros may match keywords)
            tok if self.is_keyword_atom(tok)
                || self.is_callable_keyword(tok)
                || Self::is_reserved_keyword(tok) =>
            {
                let text = self.slice_at(self.pos).to_owned();
                self.advance();
                Ok(self.intern(&text))
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "macro name after '?'".into(),
                found: self.current_description(),
                span: self.current_span(),
                hint: None,
            }),
        }
    }

    /// Check if the current token is an atom or keyword (for function definitions).
    fn check_atom_or_keyword(&self) -> bool {
        if self.at_end() {
            return false;
        }
        matches!(
            self.tokens[self.pos].0,
            Token::Atom | Token::QuotedAtom
        ) || self.is_keyword_atom(&self.tokens[self.pos].0)
            || self.is_callable_keyword(&self.tokens[self.pos].0)
    }

    /// Keywords that might appear as function names.
    fn is_callable_keyword(&self, tok: &Token) -> bool {
        matches!(
            tok,
            Token::KwTrue | Token::KwFalse | Token::KwThrow
        )
    }

    fn expect_integer(&mut self) -> Result<i64, ParseError> {
        if self.at_end() {
            return Err(ParseError::UnexpectedEof { position: self.current_span().start });
        }
        if let Token::Integer(n) = self.tokens[self.pos].0 {
            self.advance();
            Ok(n)
        } else {
            Err(ParseError::UnexpectedToken {
                expected: "integer".into(),
                found: self.current_description(),
                span: self.current_span(),
                hint: None,
            })
        }
    }

    fn intern(&mut self, s: &str) -> AtomId {
        let sym = self.interner.get_or_intern(s);
        AtomId(sym.to_usize() as u32)
    }

    /// Intern the current atom (unquoted or quoted) without advancing.
    fn intern_current_atom(&mut self) -> AtomId {
        match &self.tokens[self.pos].0 {
            Token::QuotedAtom => {
                let raw = self.slice_at(self.pos);
                let content = unescape_erlang_string(&raw[1..raw.len() - 1]);
                self.intern(&content)
            }
            _ => {
                let text = self.slice_at(self.pos).to_owned();
                self.intern(&text)
            }
        }
    }

    /// Discard tokens until we find a synchronization point (`.` at end of
    /// form, or end-of-file).
    fn synchronize(&mut self) {
        while !self.at_end() {
            if self.check(&Token::Dot) {
                self.advance();
                return;
            }
            self.advance();
        }
    }
}

// ---------------------------------------------------------------------------
// Span helpers for AST nodes
// ---------------------------------------------------------------------------

fn expr_span(expr: &Expr) -> Span {
    match expr {
        Expr::Integer(_, s)
        | Expr::BigInteger(_, s)
        | Expr::Float(_, s)
        | Expr::Atom(_, s)
        | Expr::String(_, s)
        | Expr::Nil(s)
        | Expr::Variable(_, s)
        | Expr::Tuple(_, s)
        | Expr::Cons(_, _, s)
        | Expr::BinOp(_, _, _, s)
        | Expr::UnaryOp(_, _, s)
        | Expr::Match(_, _, s)
        | Expr::Case(_, _, s)
        | Expr::If(_, s)
        | Expr::Receive(_, _, s)
        | Expr::Call(_, _, s)
        | Expr::Send(_, _, s)
        | Expr::Block(_, s)
        | Expr::Binary(_, s)
        | Expr::Map(_, s)
        | Expr::MapUpdate(_, _, s)
        | Expr::ListComp(_, _, s)
        | Expr::BinComp(_, _, s)
        | Expr::Throw(_, s)
        | Expr::Catch(_, s)
        | Expr::Fun(_, _, s)
        | Expr::FunRef(_, s) => *s,
        Expr::Try { span, .. }
        | Expr::RecordCreate { span, .. }
        | Expr::RecordUpdate { span, .. }
        | Expr::RecordAccess { span, .. }
        | Expr::RecordIndex { span, .. }
        | Expr::Maybe { span, .. }
        | Expr::MacroCall { span, .. } => *span,
    }
}

fn pattern_span(pat: &Pattern) -> Span {
    match pat {
        Pattern::Wildcard(s)
        | Pattern::Variable(_, s)
        | Pattern::Integer(_, s)
        | Pattern::BigInteger(_, s)
        | Pattern::Float(_, s)
        | Pattern::Atom(_, s)
        | Pattern::String(_, s)
        | Pattern::Tuple(_, s)
        | Pattern::Cons(_, _, s)
        | Pattern::Nil(s)
        | Pattern::Match(_, _, s)
        | Pattern::Binary(_, s)
        | Pattern::Map(_, s)
        | Pattern::Concat(_, _, s) => *s,
        Pattern::Record { span, .. }
        | Pattern::MacroCall { span, .. } => *span,
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    #[test]
    fn parenthesized_type_attribute_from_otp() {
        let mut parser = Parser::new("-module(probe). -type(chars() :: [char() | any()]). f() -> ok.");
        let result = parser.parse_module();
        assert!(!result.has_errors(), "{:?}", result.errors);
        assert_eq!(result.module.type_defs.len(), 1);
        assert_eq!(parser.resolve(result.module.type_defs[0].name), Some("chars"));
    }

    #[test]
    fn literal_space_character_from_otp_sources() {
        let tokens = Lexer::lex_all("$ ").unwrap();
        assert!(matches!(tokens[0].0, Token::Integer(32)));
    }

    #[test]
    fn remote_throw_is_an_ordinary_remote_call() {
        let mut parser = Parser::new("-module(probe). f() -> erlang:throw(boom).");
        let result = parser.parse_module();
        assert!(!result.has_errors(), "{:?}", result.errors);
        assert!(matches!(result.module.functions[0].clauses[0].body[0], Expr::Call(CallTarget::Remote(_, _), _, _)));
    }

    #[test]
    fn otp_documentation_binary_sigil_is_ignored() {
        let mut parser = Parser::new(r#"-module(doc_sigil).
-doc #{since => ~"OTP 29.0"}.
f() -> ok.
"#);
        let result = parser.parse_module();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.module.functions.len(), 1);
    }

    #[test]
    fn otp_documentation_triple_strings_are_ignored_as_one_token() {
        let source = r##"-module(probe). -doc """
Example "name" \_test, 1.0.
""". probe() -> ok."##;
        let mut parser = Parser::new(source);
        let result = parser.parse_module();
        assert!(!result.has_errors(), "{:?}", result.errors);
        assert_eq!(result.module.functions.len(), 1);
    }

    #[test]
    fn eunit_autoexport_only_exports_zero_arity_tests() {
        let mut parser = Parser::new("-module(probe). -compile({parse_transform,eunit_autoexport}). -export([pass_test/0]). pass_test() -> ok. values_test_() -> []. helper() -> ok. other_test(X) -> X.");
        let result = parser.parse_module();
        assert!(!result.has_errors());
        let exports: Vec<_> = result.module.exports.iter().map(|f| (parser.atom_names()[&f.name].clone(), f.arity)).collect();
        assert_eq!(exports, vec![("pass_test".into(), 0), ("values_test_".into(), 0)]);
    }

    use super::*;

    /// Helper: parse source as a module, expect success.
    fn parse_ok(source: &str) -> (Module, Parser<'_>) {
        let mut parser = Parser::new(source);
        let result = parser.parse_module();
        assert!(
            result.errors.is_empty(),
            "parse errors: {:#?}",
            result.errors
        );
        (result.module, parser)
    }

    fn resolve<'a>(parser: &'a Parser, id: AtomId) -> &'a str {
        parser.resolve(id).expect("unresolved atom")
    }

    // ── Simple module ──────────────────────────────────────────

    #[test]
    fn parse_simple_module() {
        let src = r#"
            -module(test).
            -export([foo/1]).
            foo(X) -> X + 1.
        "#;
        let (module, parser) = parse_ok(src);
        assert_eq!(resolve(&parser, module.name), "test");
        assert_eq!(module.exports.len(), 1);
        assert_eq!(resolve(&parser, module.exports[0].name), "foo");
        assert_eq!(module.exports[0].arity, 1);
        assert_eq!(module.functions.len(), 1);
        assert_eq!(resolve(&parser, module.functions[0].name), "foo");
        assert_eq!(module.functions[0].arity, 1);

        let clause = &module.functions[0].clauses[0];
        assert_eq!(clause.patterns.len(), 1);
        // Body should be a BinOp(Add, Variable(X), Integer(1))
        assert_eq!(clause.body.len(), 1);
        match &clause.body[0] {
            Expr::BinOp(BinOp::Add, lhs, rhs, _) => {
                assert!(matches!(lhs.as_ref(), Expr::Variable(_, _)));
                assert!(matches!(rhs.as_ref(), Expr::Integer(1, _)));
            }
            other => panic!("expected BinOp(Add), got {:?}", other),
        }
    }

    // ── Pattern matching (tuple) ───────────────────────────────

    #[test]
    fn parse_tuple_pattern() {
        let src = r#"
            -module(test).
            foo({ok, X}) -> X.
        "#;
        let (module, _parser) = parse_ok(src);
        let clause = &module.functions[0].clauses[0];
        match &clause.patterns[0] {
            Pattern::Tuple(elems, _) => {
                assert_eq!(elems.len(), 2);
                assert!(matches!(&elems[0], Pattern::Atom(_, _)));
                assert!(matches!(&elems[1], Pattern::Variable(_, _)));
            }
            other => panic!("expected Tuple pattern, got {:?}", other),
        }
    }

    // ── Case expression ────────────────────────────────────────

    #[test]
    fn parse_case_expr() {
        let src = r#"
            -module(test).
            f(X) ->
                case X of
                    ok -> 1;
                    error -> 2
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        assert_eq!(body.len(), 1);
        match &body[0] {
            Expr::Case(_, clauses, _) => {
                assert_eq!(clauses.len(), 2);
            }
            other => panic!("expected Case, got {:?}", other),
        }
    }

    // ── Receive with after ─────────────────────────────────────

    #[test]
    fn parse_receive_after() {
        let src = r#"
            -module(test).
            f() ->
                receive
                    {msg, X} -> X
                after
                    1000 -> timeout
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Receive(clauses, after, _) => {
                assert_eq!(clauses.len(), 1);
                assert!(after.is_some());
                let after = after.as_ref().unwrap();
                assert!(matches!(after.timeout.as_ref(), Expr::Integer(1000, _)));
            }
            other => panic!("expected Receive, got {:?}", other),
        }
    }

    // ── Factorial (multiple clauses + guards) ──────────────────

    #[test]
    fn parse_factorial() {
        let src = r#"
            -module(math).
            -export([factorial/1]).
            factorial(0) -> 1;
            factorial(N) when N > 0 -> N * factorial(N - 1).
        "#;
        let (module, parser) = parse_ok(src);
        assert_eq!(resolve(&parser, module.name), "math");
        let func = &module.functions[0];
        assert_eq!(func.clauses.len(), 2);

        // First clause: factorial(0) -> 1
        let c0 = &func.clauses[0];
        assert!(matches!(&c0.patterns[0], Pattern::Integer(0, _)));
        assert!(c0.guard.is_none());

        // Second clause: factorial(N) when N > 0 -> ...
        let c1 = &func.clauses[1];
        assert!(matches!(&c1.patterns[0], Pattern::Variable(_, _)));
        assert!(c1.guard.is_some());
    }

    // ── Ping-pong (send + receive + self()) ────────────────────

    #[test]
    fn parse_ping_pong() {
        let src = r#"
            -module(pingpong).
            -export([start/0]).
            start() ->
                Pid = self(),
                Pid ! {ping, self()}.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        // Two expressions: a match and a send
        assert_eq!(body.len(), 2);

        match &body[0] {
            Expr::Match(pat, expr, _) => {
                assert!(matches!(pat.as_ref(), Pattern::Variable(_, _)));
                assert!(matches!(expr.as_ref(), Expr::Call(CallTarget::Local(_, _), args, _) if args.is_empty()));
            }
            other => panic!("expected Match, got {:?}", other),
        }

        match &body[1] {
            Expr::Send(_, msg, _) => {
                assert!(matches!(msg.as_ref(), Expr::Tuple(_, _)));
            }
            other => panic!("expected Send, got {:?}", other),
        }
    }

    // ── Remote call ────────────────────────────────────────────

    #[test]
    fn parse_remote_call() {
        let src = r#"
            -module(test).
            f() -> io:format("hello~n").
        "#;
        let (module, parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Call(CallTarget::Remote(mod_expr, fun_expr), args, _) => {
                match mod_expr.as_ref() {
                    Expr::Atom(id, _) => assert_eq!(resolve(&parser, *id), "io"),
                    other => panic!("expected Atom, got {:?}", other),
                }
                match fun_expr.as_ref() {
                    Expr::Atom(id, _) => assert_eq!(resolve(&parser, *id), "format"),
                    other => panic!("expected Atom for fun, got {:?}", other),
                }
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected remote Call, got {:?}", other),
        }
    }

    #[test]
    fn parse_dynamic_remote_var_mod() {
        let src = r#"
            -module(test).
            f(Mod) -> Mod:foo(1, 2).
        "#;
        let (module, parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Call(CallTarget::Remote(mod_expr, fun_expr), args, _) => {
                match mod_expr.as_ref() {
                    Expr::Variable(id, _) => assert_eq!(resolve(&parser, *id), "Mod"),
                    other => panic!("expected Variable, got {:?}", other),
                }
                match fun_expr.as_ref() {
                    Expr::Atom(id, _) => assert_eq!(resolve(&parser, *id), "foo"),
                    other => panic!("expected Atom for fun, got {:?}", other),
                }
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected remote Call, got {:?}", other),
        }
    }

    #[test]
    fn parse_dynamic_remote_var_fun() {
        let src = r#"
            -module(test).
            f(Fun) -> bar:Fun(1).
        "#;
        let (module, parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Call(CallTarget::Remote(mod_expr, fun_expr), args, _) => {
                match mod_expr.as_ref() {
                    Expr::Atom(id, _) => assert_eq!(resolve(&parser, *id), "bar"),
                    other => panic!("expected Atom, got {:?}", other),
                }
                match fun_expr.as_ref() {
                    Expr::Variable(id, _) => assert_eq!(resolve(&parser, *id), "Fun"),
                    other => panic!("expected Variable for fun, got {:?}", other),
                }
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected remote Call, got {:?}", other),
        }
    }

    #[test]
    fn parse_dynamic_remote_var_both() {
        let src = r#"
            -module(test).
            f(Mod, Fun) -> Mod:Fun(a, b, c).
        "#;
        let (module, parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Call(CallTarget::Remote(mod_expr, fun_expr), args, _) => {
                match mod_expr.as_ref() {
                    Expr::Variable(id, _) => assert_eq!(resolve(&parser, *id), "Mod"),
                    other => panic!("expected Variable, got {:?}", other),
                }
                match fun_expr.as_ref() {
                    Expr::Variable(id, _) => assert_eq!(resolve(&parser, *id), "Fun"),
                    other => panic!("expected Variable for fun, got {:?}", other),
                }
                assert_eq!(args.len(), 3);
            }
            other => panic!("expected remote Call, got {:?}", other),
        }
    }

    // NERA-102: OTP accepts parenthesized expressions in either remote target.
    #[test]
    fn parse_parenthesized_remote_targets() {
        for call in [
            "m:(F)()",
            "M:(F)()",
            "(M):f()",
            "(M):F()",
            "(M):(F)()",
            "((M)):((F))()",
        ] {
            let src = format!("-module(test). f(M, F) -> {call}.");
            let (module, parser) = parse_ok(&src);
            let Expr::Call(CallTarget::Remote(m, f), args, _) =
                &module.functions[0].clauses[0].body[0]
            else {
                panic!("expected remote call for {call}");
            };
            assert!(args.is_empty(), "{call}");
            assert!(
                matches!(m.as_ref(), Expr::Atom(id, _) | Expr::Variable(id, _)
                if ["m", "M"].contains(&resolve(&parser, *id)))
            );
            assert!(
                matches!(f.as_ref(), Expr::Atom(id, _) | Expr::Variable(id, _)
                if ["f", "F"].contains(&resolve(&parser, *id)))
            );
        }
    }

    #[test]
    fn parse_computed_remote_targets_and_trailing_fun_call() {
        let (module, _) =
            parse_ok("-module(test). f() -> (module_name()):(function_name())(1)(2).");
        let Expr::Call(CallTarget::Expr(callee), outer_args, _) =
            &module.functions[0].clauses[0].body[0]
        else {
            panic!("expected trailing fun application")
        };
        assert!(matches!(outer_args.as_slice(), [Expr::Integer(2, _)]));
        let Expr::Call(CallTarget::Remote(m, f), args, _) = callee.as_ref() else {
            panic!("expected inner remote call")
        };
        assert!(matches!(args.as_slice(), [Expr::Integer(1, _)]));
        for target in [m, f] {
            assert!(
                matches!(target.as_ref(), Expr::Call(CallTarget::Local(_, _), args, _) if args.is_empty())
            );
        }

        // The reported NERA-101 source is valid syntax even though its
        // unsized generator is rejected later by semantic validation.
        let (module, _) =
            parse_ok("-module(test). f(B) -> m:(fun() -> [X || <<X/binary>> <= B] end)().");
        assert!(matches!(&module.functions[0].clauses[0].body[0],
            Expr::Call(CallTarget::Remote(_, f), args, _)
                if matches!(f.as_ref(), Expr::Fun(_, _, _)) && args.is_empty()));
    }

    #[test]
    fn remote_call_colon_boundaries() {
        // Binary segment colons belong to the segment parser, not remote calls.
        let (module, _) = parse_ok("-module(test). f(X, N) -> <<(X):N, X:8, (X):(N)>>.");
        let Expr::Binary(segments, _) = &module.functions[0].clauses[0].body[0] else {
            panic!("expected binary")
        };
        assert_eq!(segments.len(), 3);
        assert!(
            segments
                .iter()
                .all(|s| matches!(s.value, Expr::Variable(_, _)) && s.size.is_some())
        );

        for call in [
            "m:()",
            "(M):()",
            "m:(F)(",
            "(M):(F)(1",
            "m:F:G()",
            "m:(F) + 1",
        ] {
            let src = format!("-module(test). f(M, F) -> {call}.");
            assert!(
                !parse_err(&src).is_empty(),
                "accepted malformed call: {call}"
            );
        }
    }

    // ── Send operator ──────────────────────────────────────────

    #[test]
    fn parse_send_operator() {
        let src = r#"
            -module(test).
            f(Pid) -> Pid ! {msg, self()}.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Send(target, msg, _) => {
                assert!(matches!(target.as_ref(), Expr::Variable(_, _)));
                assert!(matches!(msg.as_ref(), Expr::Tuple(_, _)));
            }
            other => panic!("expected Send, got {:?}", other),
        }
    }

    // ── Guards ─────────────────────────────────────────────────

    #[test]
    fn parse_guards() {
        let src = r#"
            -module(test).
            foo(N) when N > 0 -> N.
        "#;
        let (module, _parser) = parse_ok(src);
        let clause = &module.functions[0].clauses[0];
        assert!(clause.guard.is_some());
        match clause.guard.as_ref().unwrap() {
            Guard::Test(expr) => {
                assert!(matches!(expr.as_ref(), Expr::BinOp(BinOp::Gt, _, _, _)));
            }
            other => panic!("expected Guard::Test, got {:?}", other),
        }
    }

    // ── List operations ────────────────────────────────────────

    #[test]
    fn parse_list_cons_pattern() {
        let src = r#"
            -module(test).
            head([H|T]) -> H.
        "#;
        let (module, _parser) = parse_ok(src);
        let clause = &module.functions[0].clauses[0];
        match &clause.patterns[0] {
            Pattern::Cons(head, tail, _) => {
                assert!(matches!(head.as_ref(), Pattern::Variable(_, _)));
                assert!(matches!(tail.as_ref(), Pattern::Variable(_, _)));
            }
            other => panic!("expected Cons pattern, got {:?}", other),
        }
    }

    // ── List append ────────────────────────────────────────────

    #[test]
    fn parse_list_append() {
        let src = r#"
            -module(test).
            f(A, B) -> A ++ B.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::BinOp(BinOp::ListAppend, _, _, _) => {}
            other => panic!("expected ListAppend, got {:?}", other),
        }
    }

    // ── If expression ──────────────────────────────────────────

    #[test]
    fn parse_if_expr() {
        let src = r#"
            -module(test).
            f(X) ->
                if
                    X > 0 -> positive;
                    X < 0 -> negative;
                    true -> zero
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::If(clauses, _) => {
                assert_eq!(clauses.len(), 3);
            }
            other => panic!("expected If, got {:?}", other),
        }
    }

    // ── Negative number literal ────────────────────────────────

    #[test]
    fn parse_negative_number() {
        let src = r#"
            -module(test).
            f() -> -42.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Integer(-42, _) => {}
            other => panic!("expected Integer(-42), got {:?}", other),
        }
    }

    // ── String literal ─────────────────────────────────────────

    #[test]
    fn parse_string_literal() {
        let src = r#"
            -module(test).
            f() -> "hello world".
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::String(s, _) => assert_eq!(s, "hello world"),
            other => panic!("expected String, got {:?}", other),
        }
    }

    #[test]
    fn parse_implicit_string_concatenation() {
        let src = r#"
            -module(test).
            f() -> "hello " "world".
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::String(s, _) => assert_eq!(s, "hello world"),
            other => panic!("expected String, got {:?}", other),
        }
    }

    #[test]
    fn parse_multiline_string_concatenation() {
        let src = r#"
            -module(test).
            f() -> "a"
                   "b"
                   "c".
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::String(s, _) => assert_eq!(s, "abc"),
            other => panic!("expected String, got {:?}", other),
        }
    }

    // ── String prefix pattern (++ in patterns) ─────────────────

    #[test]
    fn parse_pattern_string_prefix_match() {
        let src = r#"
            -module(test).
            f("set " ++ Rest) -> Rest.
        "#;
        let (module, _parser) = parse_ok(src);
        let pat = &module.functions[0].clauses[0].patterns[0];
        match pat {
            Pattern::Concat(lhs, rhs, _) => {
                match lhs.as_ref() {
                    Pattern::String(s, _) => assert_eq!(s, "set "),
                    other => panic!("expected String LHS, got {:?}", other),
                }
                assert!(matches!(rhs.as_ref(), Pattern::Variable(_, _)),
                    "expected Variable RHS, got {:?}", rhs);
            }
            other => panic!("expected Concat pattern, got {:?}", other),
        }
    }

    #[test]
    fn parse_pattern_string_prefix_empty() {
        let src = r#"
            -module(test).
            f("" ++ Rest) -> Rest.
        "#;
        let (module, _parser) = parse_ok(src);
        let pat = &module.functions[0].clauses[0].patterns[0];
        match pat {
            Pattern::Concat(lhs, _, _) => {
                match lhs.as_ref() {
                    Pattern::String(s, _) => assert_eq!(s, ""),
                    other => panic!("expected String LHS, got {:?}", other),
                }
            }
            other => panic!("expected Concat pattern, got {:?}", other),
        }
    }

    // ── Nil / empty list ───────────────────────────────────────

    #[test]
    fn parse_empty_list() {
        let src = r#"
            -module(test).
            f() -> [].
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        assert!(matches!(&body[0], Expr::Nil(_)));
    }

    // ── Multiple exports ───────────────────────────────────────

    #[test]
    fn parse_multiple_exports() {
        let src = r#"
            -module(test).
            -export([foo/1, bar/2]).
            foo(X) -> X.
            bar(X, Y) -> X + Y.
        "#;
        let (module, parser) = parse_ok(src);
        assert_eq!(module.exports.len(), 2);
        assert_eq!(resolve(&parser, module.exports[0].name), "foo");
        assert_eq!(module.exports[0].arity, 1);
        assert_eq!(resolve(&parser, module.exports[1].name), "bar");
        assert_eq!(module.exports[1].arity, 2);
        assert_eq!(module.functions.len(), 2);
    }

    // ── Operator precedence ────────────────────────────────────

    #[test]
    fn operator_precedence() {
        // 1 + 2 * 3 should parse as 1 + (2 * 3)
        let src = r#"
            -module(test).
            f() -> 1 + 2 * 3.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::BinOp(BinOp::Add, lhs, rhs, _) => {
                assert!(matches!(lhs.as_ref(), Expr::Integer(1, _)));
                match rhs.as_ref() {
                    Expr::BinOp(BinOp::Mul, l, r, _) => {
                        assert!(matches!(l.as_ref(), Expr::Integer(2, _)));
                        assert!(matches!(r.as_ref(), Expr::Integer(3, _)));
                    }
                    other => panic!("expected Mul, got {:?}", other),
                }
            }
            other => panic!("expected Add, got {:?}", other),
        }
    }

    // ── Quoted atom ────────────────────────────────────────────

    #[test]
    fn parse_quoted_atom() {
        let src = r#"
            -module(test).
            f() -> 'hello world'.
        "#;
        let (module, parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Atom(id, _) => assert_eq!(resolve(&parser, *id), "hello world"),
            other => panic!("expected Atom, got {:?}", other),
        }
    }

    // ── Tuple expression ───────────────────────────────────────

    #[test]
    fn parse_tuple_expr() {
        let src = r#"
            -module(test).
            f() -> {ok, 42, "hello"}.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Tuple(elems, _) => {
                assert_eq!(elems.len(), 3);
                assert!(matches!(&elems[0], Expr::Atom(_, _)));
                assert!(matches!(&elems[1], Expr::Integer(42, _)));
                assert!(matches!(&elems[2], Expr::String(s, _) if s == "hello"));
            }
            other => panic!("expected Tuple, got {:?}", other),
        }
    }

    // ── Comparison operators ───────────────────────────────────

    #[test]
    fn parse_comparison_ops() {
        let src = r#"
            -module(test).
            f(X) -> X =:= 5.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::BinOp(BinOp::ExactEq, _, _, _) => {}
            other => panic!("expected ExactEq, got {:?}", other),
        }
    }

    // ── Match expression in body ───────────────────────────────

    #[test]
    fn parse_match_in_body() {
        let src = r#"
            -module(test).
            f() ->
                X = 42,
                X.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        assert_eq!(body.len(), 2);
        match &body[0] {
            Expr::Match(pat, expr, _) => {
                assert!(matches!(pat.as_ref(), Pattern::Variable(_, _)));
                assert!(matches!(expr.as_ref(), Expr::Integer(42, _)));
            }
            other => panic!("expected Match, got {:?}", other),
        }
    }

    // ── List with elements ─────────────────────────────────────

    #[test]
    fn parse_list_elements() {
        let src = r#"
            -module(test).
            f() -> [1, 2, 3].
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        // Should be Cons(1, Cons(2, Cons(3, Nil)))
        match &body[0] {
            Expr::Cons(head, tail, _) => {
                assert!(matches!(head.as_ref(), Expr::Integer(1, _)));
                match tail.as_ref() {
                    Expr::Cons(head2, tail2, _) => {
                        assert!(matches!(head2.as_ref(), Expr::Integer(2, _)));
                        match tail2.as_ref() {
                            Expr::Cons(head3, tail3, _) => {
                                assert!(matches!(head3.as_ref(), Expr::Integer(3, _)));
                                assert!(matches!(tail3.as_ref(), Expr::Nil(_)));
                            }
                            other => panic!("expected inner Cons, got {:?}", other),
                        }
                    }
                    other => panic!("expected middle Cons, got {:?}", other),
                }
            }
            other => panic!("expected outer Cons, got {:?}", other),
        }
    }

    // ── Boolean operators ──────────────────────────────────────

    #[test]
    fn parse_boolean_ops() {
        let src = r#"
            -module(test).
            f(X, Y) -> X andalso Y orelse false.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        // orelse has lower precedence than andalso
        // so: (X andalso Y) orelse false
        match &body[0] {
            Expr::BinOp(BinOp::Orelse, lhs, rhs, _) => {
                assert!(matches!(lhs.as_ref(), Expr::BinOp(BinOp::Andalso, _, _, _)));
                assert!(matches!(rhs.as_ref(), Expr::Atom(_, _)));
            }
            other => panic!("expected Orelse, got {:?}", other),
        }
    }

    // ── Unary not ──────────────────────────────────────────────

    #[test]
    fn parse_unary_not() {
        let src = r#"
            -module(test).
            f(X) -> not X.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::UnaryOp(UnaryOp::Not, _, _) => {}
            other => panic!("expected UnaryOp(Not), got {:?}", other),
        }
    }

    // ── Multiple function clauses ──────────────────────────────

    #[test]
    fn parse_multi_clause_function() {
        let src = r#"
            -module(test).
            f(0) -> zero;
            f(1) -> one;
            f(_) -> other.
        "#;
        let (module, _parser) = parse_ok(src);
        assert_eq!(module.functions[0].clauses.len(), 3);
    }

    // ── Wildcard pattern ───────────────────────────────────────

    #[test]
    fn parse_wildcard_pattern() {
        let src = r#"
            -module(test).
            f(_) -> ok.
        "#;
        let (module, _parser) = parse_ok(src);
        let clause = &module.functions[0].clauses[0];
        assert!(matches!(&clause.patterns[0], Pattern::Wildcard(_)));
    }

    // ── Binary expressions ─────────────────────────────────────

    #[test]
    fn parse_empty_binary() {
        let src = r#"
            -module(test).
            f() -> <<>>.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Binary(segments, _) => {
                assert_eq!(segments.len(), 0);
            }
            other => panic!("expected Binary, got {:?}", other),
        }
    }

    #[test]
    fn parse_binary_literal_bytes() {
        let src = r#"
            -module(test).
            f() -> <<1, 2, 3>>.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Binary(segments, _) => {
                assert_eq!(segments.len(), 3);
                assert!(matches!(&segments[0].value, Expr::Integer(1, _)));
                assert!(matches!(&segments[1].value, Expr::Integer(2, _)));
                assert!(matches!(&segments[2].value, Expr::Integer(3, _)));
                // Default type_spec
                assert_eq!(segments[0].type_spec.typ, BinSegmentKind::Integer);
                assert!(segments[0].size.is_none());
            }
            other => panic!("expected Binary, got {:?}", other),
        }
    }

    #[test]
    fn parse_binary_with_size() {
        let src = r#"
            -module(test).
            f(X) -> <<X:16>>.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Binary(segments, _) => {
                assert_eq!(segments.len(), 1);
                assert!(matches!(&segments[0].value, Expr::Variable(_, _)));
                match segments[0].size.as_ref() {
                    Some(size) => assert!(matches!(size.as_ref(), Expr::Integer(16, _))),
                    None => panic!("expected size"),
                }
            }
            other => panic!("expected Binary, got {:?}", other),
        }
    }

    #[test]
    fn parse_binary_with_type_specifiers() {
        let src = r#"
            -module(test).
            f(X) -> <<X:16/integer-big-unsigned>>.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Binary(segments, _) => {
                assert_eq!(segments.len(), 1);
                assert_eq!(segments[0].type_spec.typ, BinSegmentKind::Integer);
                assert_eq!(segments[0].type_spec.endianness, Endianness::Big);
                assert_eq!(segments[0].type_spec.signedness, Signedness::Unsigned);
            }
            other => panic!("expected Binary, got {:?}", other),
        }
    }

    #[test]
    fn parse_binary_pattern() {
        let src = r#"
            -module(test).
            f(<<A:8, B:8, C:8>>) -> {A, B, C}.
        "#;
        let (module, _parser) = parse_ok(src);
        let clause = &module.functions[0].clauses[0];
        match &clause.patterns[0] {
            Pattern::Binary(segments, _) => {
                assert_eq!(segments.len(), 3);
                assert!(matches!(&segments[0].value, Pattern::Variable(_, _)));
                assert!(matches!(&segments[1].value, Pattern::Variable(_, _)));
                assert!(matches!(&segments[2].value, Pattern::Variable(_, _)));
                // Each has size 8
                for seg in segments {
                    match seg.size.as_ref() {
                        Some(size) => assert!(matches!(size.as_ref(), Expr::Integer(8, _))),
                        None => panic!("expected size 8"),
                    }
                }
            }
            other => panic!("expected Binary pattern, got {:?}", other),
        }
    }

    #[test]
    fn parse_binary_pattern_with_rest() {
        let src = r#"
            -module(test).
            f(<<H:8, Rest/binary>>) -> {H, Rest}.
        "#;
        let (module, _parser) = parse_ok(src);
        let clause = &module.functions[0].clauses[0];
        match &clause.patterns[0] {
            Pattern::Binary(segments, _) => {
                assert_eq!(segments.len(), 2);
                // First segment: H:8
                assert!(matches!(&segments[0].value, Pattern::Variable(_, _)));
                match segments[0].size.as_ref() {
                    Some(size) => assert!(matches!(size.as_ref(), Expr::Integer(8, _))),
                    None => panic!("expected size 8"),
                }
                // Second segment: Rest/binary
                assert!(matches!(&segments[1].value, Pattern::Variable(_, _)));
                assert!(segments[1].size.is_none());
                assert_eq!(segments[1].type_spec.typ, BinSegmentKind::Binary);
            }
            other => panic!("expected Binary pattern, got {:?}", other),
        }
    }

    #[test]
    fn parse_binary_pattern_with_bitstring_rest() {
        // bitstring/bits type specifier — like binary but default unit:1
        let src = r#"
            -module(test).
            f(<<Byte/integer, Rest/bitstring>>) -> {Byte, Rest}.
        "#;
        let (module, _parser) = parse_ok(src);
        let clause = &module.functions[0].clauses[0];
        match &clause.patterns[0] {
            Pattern::Binary(segments, _) => {
                assert_eq!(segments.len(), 2);
                assert_eq!(segments[0].type_spec.typ, BinSegmentKind::Integer);
                assert!(segments[1].size.is_none());
                assert_eq!(segments[1].type_spec.typ, BinSegmentKind::Bitstring);
            }
            other => panic!("expected Binary pattern, got {:?}", other),
        }
    }

    #[test]
    fn parse_binary_bits_alias() {
        // `bits` is an alias for `bitstring`
        let src = r#"
            -module(test).
            f(<<Rest/bits>>) -> Rest.
        "#;
        let (module, _parser) = parse_ok(src);
        let clause = &module.functions[0].clauses[0];
        match &clause.patterns[0] {
            Pattern::Binary(segments, _) => {
                assert_eq!(segments.len(), 1);
                assert_eq!(segments[0].type_spec.typ, BinSegmentKind::Bitstring);
            }
            other => panic!("expected Binary pattern, got {:?}", other),
        }
    }

    #[test]
    fn parse_variable_with_at_sign() {
        // The BEAM accepts @ in variable names; used by compiler-generated code.
        let src = r#"
            -module(test).
            f(_stack@2) -> _stack@2.
        "#;
        let (module, _parser) = parse_ok(src);
        assert_eq!(module.functions.len(), 1);
    }

    #[test]
    fn parse_binary_case_match() {
        let src = r#"
            -module(test).
            f(Bin) ->
                case Bin of
                    <<>> -> empty;
                    <<X:8, _Rest/binary>> -> X
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Case(_, clauses, _) => {
                assert_eq!(clauses.len(), 2);
                // First clause: <<>>
                match &clauses[0].pattern {
                    Pattern::Binary(segments, _) => assert_eq!(segments.len(), 0),
                    other => panic!("expected empty Binary pattern, got {:?}", other),
                }
                // Second clause: <<X:8, _Rest/binary>>
                match &clauses[1].pattern {
                    Pattern::Binary(segments, _) => assert_eq!(segments.len(), 2),
                    other => panic!("expected Binary pattern with 2 segments, got {:?}", other),
                }
            }
            other => panic!("expected Case, got {:?}", other),
        }
    }

    #[test]
    fn parse_binary_little_endian() {
        let src = r#"
            -module(test).
            f(<<X:32/little-signed>>) -> X.
        "#;
        let (module, _parser) = parse_ok(src);
        let clause = &module.functions[0].clauses[0];
        match &clause.patterns[0] {
            Pattern::Binary(segments, _) => {
                assert_eq!(segments.len(), 1);
                assert_eq!(segments[0].type_spec.endianness, Endianness::Little);
                assert_eq!(segments[0].type_spec.signedness, Signedness::Signed);
            }
            other => panic!("expected Binary pattern, got {:?}", other),
        }
    }

    #[test]
    fn parse_binary_size_arithmetic_pattern() {
        // <<Data:(Sz - 2)/binary, "\r\n">> — size requires parentheses (standard Erlang)
        let src = r#"
            -module(test).
            f(Bin) ->
                Sz = byte_size(Bin),
                case Bin of
                    <<Data:(Sz - 2)/binary, "\r\n">> -> Data;
                    _ -> Bin
                end.
        "#;
        let (_module, _parser) = parse_ok(src);
    }

    #[test]
    fn parse_binary_size_multiply_expr() {
        // <<X:(N * 8)>> — size with multiplication requires parentheses
        let src = r#"
            -module(test).
            f(N) -> <<X:(N * 8)>>.
        "#;
        let (_module, _parser) = parse_ok(src);
    }

    #[test]
    fn parse_binary_size_bare_arithmetic_is_error() {
        // <<Data:Sz - 2/binary>> is NOT valid standard Erlang — requires parentheses
        let src = r#"
            -module(test).
            f(Bin) ->
                Sz = byte_size(Bin),
                case Bin of
                    <<Data:Sz - 2/binary, "\r\n">> -> Data;
                    _ -> Bin
                end.
        "#;
        let _errs = parse_err(src);
    }

    #[test]
    fn parse_binary_size_bare_multiply_is_error() {
        // <<X:N * 8>> is NOT valid standard Erlang — requires parentheses
        let src = r#"
            -module(test).
            f(N) -> <<X:N * 8>>.
        "#;
        let _errs = parse_err(src);
    }

    // ── Map expressions and patterns ──────────────────────────

    #[test]
    fn parse_empty_map_expr() {
        let src = r#"
            -module(test).
            f() -> #{}.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Map(fields, _) => assert_eq!(fields.len(), 0),
            other => panic!("expected Map, got {:?}", other),
        }
    }

    #[test]
    fn parse_map_expr_with_fields() {
        let src = r#"
            -module(test).
            f() -> #{a => 1, b => 2}.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Map(fields, _) => {
                assert_eq!(fields.len(), 2);
                // First field key should be atom 'a'
                assert!(matches!(&fields[0].key, Expr::Atom(_, _)));
                // First field value should be integer 1
                assert!(matches!(&fields[0].value, Expr::Integer(1, _)));
                // Second field key should be atom 'b'
                assert!(matches!(&fields[1].key, Expr::Atom(_, _)));
                // Second field value should be integer 2
                assert!(matches!(&fields[1].value, Expr::Integer(2, _)));
            }
            other => panic!("expected Map, got {:?}", other),
        }
    }

    #[test]
    fn parse_empty_map_pattern() {
        let src = r#"
            -module(test).
            f(#{}) -> ok.
        "#;
        let (module, _parser) = parse_ok(src);
        let clause = &module.functions[0].clauses[0];
        match &clause.patterns[0] {
            Pattern::Map(fields, _) => assert_eq!(fields.len(), 0),
            other => panic!("expected Map pattern, got {:?}", other),
        }
    }

    #[test]
    fn parse_map_pattern_with_fields() {
        let src = r#"
            -module(test).
            f(#{a := X, b := Y}) -> {X, Y}.
        "#;
        let (module, _parser) = parse_ok(src);
        let clause = &module.functions[0].clauses[0];
        match &clause.patterns[0] {
            Pattern::Map(fields, _) => {
                assert_eq!(fields.len(), 2);
                // First field: a := X
                assert!(matches!(&fields[0].key, Expr::Atom(_, _)));
                assert!(matches!(&fields[0].value, Pattern::Variable(_, _)));
                // Second field: b := Y
                assert!(matches!(&fields[1].key, Expr::Atom(_, _)));
                assert!(matches!(&fields[1].value, Pattern::Variable(_, _)));
            }
            other => panic!("expected Map pattern, got {:?}", other),
        }
    }

    #[test]
    fn parse_map_in_case() {
        let src = r#"
            -module(test).
            f(M) ->
                case M of
                    #{status := ok} -> ok;
                    #{status := error} -> error
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Case(_, clauses, _) => {
                assert_eq!(clauses.len(), 2);
                assert!(matches!(&clauses[0].pattern, Pattern::Map(fields, _) if fields.len() == 1));
                assert!(matches!(&clauses[1].pattern, Pattern::Map(fields, _) if fields.len() == 1));
            }
            other => panic!("expected Case, got {:?}", other),
        }
    }

    #[test]
    fn parse_map_nested_value() {
        let src = r#"
            -module(test).
            f() -> #{key => {1, 2, 3}}.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Map(fields, _) => {
                assert_eq!(fields.len(), 1);
                assert!(matches!(&fields[0].value, Expr::Tuple(elems, _) if elems.len() == 3));
            }
            other => panic!("expected Map, got {:?}", other),
        }
    }

    // ── Integer division and remainder ─────────────────────────

    #[test]
    fn parse_int_div_rem() {
        let src = r#"
            -module(test).
            f(X) -> X div 2 + X rem 3.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        // div and rem have higher precedence than +
        // so: (X div 2) + (X rem 3)
        match &body[0] {
            Expr::BinOp(BinOp::Add, lhs, rhs, _) => {
                assert!(matches!(lhs.as_ref(), Expr::BinOp(BinOp::IntDiv, _, _, _)));
                assert!(matches!(rhs.as_ref(), Expr::BinOp(BinOp::Rem, _, _, _)));
            }
            other => panic!("expected Add(IntDiv, Rem), got {:?}", other),
        }
    }

    // ── List comprehensions ────────────────────────────────────

    #[test]
    fn parse_simple_list_comp() {
        let src = r#"
            -module(test).
            f() -> [X * 2 || X <- [1, 2, 3]].
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        assert_eq!(body.len(), 1);
        match &body[0] {
            Expr::ListComp(expr, qualifiers, _) => {
                // Body expression: X * 2
                assert!(matches!(expr.as_ref(), Expr::BinOp(BinOp::Mul, _, _, _)));
                // One generator qualifier
                assert_eq!(qualifiers.len(), 1);
                assert!(matches!(&qualifiers[0], LcQualifier::Generator(_, _, _)));
            }
            other => panic!("expected ListComp, got {:?}", other),
        }
    }

    #[test]
    fn parse_list_comp_with_filter() {
        let src = r#"
            -module(test).
            f(L) -> [X || X <- L, X > 0].
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::ListComp(expr, qualifiers, _) => {
                // Body is just variable X
                assert!(matches!(expr.as_ref(), Expr::Variable(_, _)));
                // Generator + filter
                assert_eq!(qualifiers.len(), 2);
                assert!(matches!(&qualifiers[0], LcQualifier::Generator(_, _, _)));
                assert!(matches!(&qualifiers[1], LcQualifier::Filter(_, _)));
            }
            other => panic!("expected ListComp, got {:?}", other),
        }
    }

    #[test]
    fn parse_list_comp_with_multiple_filters() {
        let src = r#"
            -module(test).
            f(L) -> [X || X <- L, X > 0, X < 10].
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::ListComp(_, qualifiers, _) => {
                assert_eq!(qualifiers.len(), 3); // 1 generator + 2 filters
                assert!(matches!(&qualifiers[0], LcQualifier::Generator(_, _, _)));
                assert!(matches!(&qualifiers[1], LcQualifier::Filter(_, _)));
                assert!(matches!(&qualifiers[2], LcQualifier::Filter(_, _)));
            }
            other => panic!("expected ListComp, got {:?}", other),
        }
    }

    #[test]
    fn parse_list_comp_tuple_pattern() {
        let src = r#"
            -module(test).
            f(L) -> [V || {K, V} <- L, K =:= key].
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::ListComp(_, qualifiers, _) => {
                match &qualifiers[0] {
                    LcQualifier::Generator(pat, _, _) => {
                        assert!(matches!(pat, Pattern::Tuple(_, _)));
                    }
                    _ => panic!("expected Generator"),
                }
                assert!(matches!(&qualifiers[1], LcQualifier::Filter(_, _)));
            }
            other => panic!("expected ListComp, got {:?}", other),
        }
    }

    // ── Exception handling ────────────────────────────────────

    #[test]
    fn parse_simple_try_catch() {
        let src = r#"
            -module(test).
            f() ->
                try
                    throw(hello)
                catch
                    X -> {caught, X}
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Try { catch_clauses, after_body, .. } => {
                assert_eq!(catch_clauses.len(), 1);
                assert!(after_body.is_none());
                assert!(matches!(&catch_clauses[0].class, CatchClass::Throw));
                assert!(matches!(&catch_clauses[0].pattern, Pattern::Variable(_, _)));
            }
            other => panic!("expected Try, got {:?}", other),
        }
    }

    #[test]
    fn parse_try_catch_multiple_clauses() {
        let src = r#"
            -module(test).
            f() ->
                try
                    something()
                catch
                    throw:X -> {thrown, X};
                    error:Y -> {errored, Y};
                    exit:Z -> {exited, Z}
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Try { catch_clauses, .. } => {
                assert_eq!(catch_clauses.len(), 3);
                assert!(matches!(&catch_clauses[0].class, CatchClass::Throw));
                assert!(matches!(&catch_clauses[1].class, CatchClass::Error));
                assert!(matches!(&catch_clauses[2].class, CatchClass::Exit));
            }
            other => panic!("expected Try, got {:?}", other),
        }
    }

    #[test]
    fn parse_try_catch_with_after() {
        let src = r#"
            -module(test).
            f() ->
                try
                    42
                catch
                    _ -> error
                after
                    cleanup()
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Try { after_body, catch_clauses, .. } => {
                assert_eq!(catch_clauses.len(), 1);
                assert!(after_body.is_some());
                assert_eq!(after_body.as_ref().unwrap().len(), 1);
            }
            other => panic!("expected Try, got {:?}", other),
        }
    }

    #[test]
    fn parse_throw_expr() {
        let src = r#"
            -module(test).
            f() -> throw({error, badarg}).
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Call(CallTarget::Local(_, _), args, _) => {
                // NERA-92: throw/1 must obey the same import policy as other calls.
                assert!(matches!(&args[0], Expr::Tuple(_, _)));
            }
            other => panic!("expected throw call, got {:?}", other),
        }
    }

    #[test]
    fn parse_catch_expr() {
        let src = r#"
            -module(test).
            f() -> catch 1 + 2.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Catch(operand, _) => {
                assert!(matches!(operand.as_ref(), Expr::BinOp(BinOp::Add, _, _, _)));
            }
            other => panic!("expected Catch, got {:?}", other),
        }
    }

    #[test]
    fn parse_case_catch_expr() {
        let src = r#"
            -module(test).
            f(X) ->
                case catch foo(X) of
                    ok -> ok;
                    _ -> error
                end.
            foo(X) -> X.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        // The case expression's scrutinee should be a Catch wrapping a call
        match &body[0] {
            Expr::Case(scrutinee, _, _) => {
                assert!(matches!(scrutinee.as_ref(), Expr::Catch(_, _)),
                    "expected Catch as scrutinee, got {:?}", scrutinee);
            }
            other => panic!("expected Case, got {:?}", other),
        }
    }

    #[test]
    fn parse_try_catch_pattern_matching() {
        let src = r#"
            -module(test).
            f() ->
                try
                    throw({error, badarg})
                catch
                    {error, What} -> {got_error, What}
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Try { catch_clauses, .. } => {
                assert_eq!(catch_clauses.len(), 1);
                assert!(matches!(&catch_clauses[0].pattern, Pattern::Tuple(elems, _) if elems.len() == 2));
            }
            other => panic!("expected Try, got {:?}", other),
        }
    }

    // ── Map update expressions ──────────────────────────────────

    #[test]
    fn parse_map_update_assoc() {
        let src = r#"
            -module(test).
            f(M) -> M#{a => 1}.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::MapUpdate(base, fields, _) => {
                assert!(matches!(base.as_ref(), Expr::Variable(_, _)));
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].op, MapUpdateOp::Assoc);
                assert!(matches!(&fields[0].key, Expr::Atom(_, _)));
                assert!(matches!(&fields[0].value, Expr::Integer(1, _)));
            }
            other => panic!("expected MapUpdate, got {:?}", other),
        }
    }

    #[test]
    fn parse_map_update_exact() {
        let src = r#"
            -module(test).
            f(M) -> M#{a := 1}.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::MapUpdate(base, fields, _) => {
                assert!(matches!(base.as_ref(), Expr::Variable(_, _)));
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].op, MapUpdateOp::Exact);
            }
            other => panic!("expected MapUpdate, got {:?}", other),
        }
    }

    #[test]
    fn parse_map_update_multiple_fields() {
        let src = r#"
            -module(test).
            f(M) -> M#{a := 1, b => 2, c := 3}.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::MapUpdate(_, fields, _) => {
                assert_eq!(fields.len(), 3);
                assert_eq!(fields[0].op, MapUpdateOp::Exact);
                assert_eq!(fields[1].op, MapUpdateOp::Assoc);
                assert_eq!(fields[2].op, MapUpdateOp::Exact);
            }
            other => panic!("expected MapUpdate, got {:?}", other),
        }
    }

    #[test]
    fn parse_map_update_on_expr() {
        // Map update on a function call result
        let src = r#"
            -module(test).
            f() -> (get_map())#{a => 1}.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::MapUpdate(base, fields, _) => {
                assert!(matches!(base.as_ref(), Expr::Call(_, _, _)));
                assert_eq!(fields.len(), 1);
            }
            other => panic!("expected MapUpdate, got {:?}", other),
        }
    }

    #[test]
    fn parse_map_update_chained() {
        let src = r#"
            -module(test).
            f(M) -> (M#{a := 1})#{b := 2}.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::MapUpdate(base, fields, _) => {
                // Outer update: #{b := 2}
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].op, MapUpdateOp::Exact);
                // Inner: M#{a := 1}
                assert!(matches!(base.as_ref(), Expr::MapUpdate(_, _, _)));
            }
            other => panic!("expected MapUpdate, got {:?}", other),
        }
    }

    #[test]
    fn parse_map_update_in_match() {
        let src = r#"
            -module(test).
            f(M) ->
                M2 = M#{a := 10},
                M2.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Match(_, rhs, _) => {
                assert!(matches!(rhs.as_ref(), Expr::MapUpdate(_, _, _)));
            }
            other => panic!("expected Match with MapUpdate, got {:?}", other),
        }
    }

    // ── Record parser tests ──

    #[test]
    fn parse_record_attribute() {
        let src = r#"
            -module(test).
            -record(person, {name, age = 0}).
            -export([f/0]).
            f() -> ok.
        "#;
        let (module, _parser) = parse_ok(src);
        assert_eq!(module.records.len(), 1);
        let rec = &module.records[0];
        assert_eq!(rec.fields.len(), 2);
        // First field has no default
        assert!(rec.fields[0].default.is_none());
        // Second field has default 0
        assert!(rec.fields[1].default.is_some());
    }

    // ── Typed record fields (added for poolboy) ────────────────
    //
    // Nera is type-erased: `::` annotations on record fields must parse
    // successfully but be discarded. These tests cover the syntax shapes
    // used by poolboy and the OTP standard library.

    #[test]
    fn parse_typed_record_field_no_default() {
        // `name :: Type` — typed field without default.
        let src = r#"
            -module(test).
            -record(state, {pid :: pid()}).
            f() -> ok.
        "#;
        let (module, parser) = parse_ok(src);
        assert_eq!(module.records.len(), 1);
        let rec = &module.records[0];
        assert_eq!(rec.fields.len(), 1);
        assert_eq!(resolve(&parser, rec.fields[0].name), "pid");
        assert!(rec.fields[0].default.is_none());
    }

    #[test]
    fn parse_typed_record_field_with_default() {
        // `name = default_expr :: Type` — typed field with default.
        let src = r#"
            -module(test).
            -record(state, {size = 0 :: non_neg_integer()}).
            f() -> ok.
        "#;
        let (module, parser) = parse_ok(src);
        let rec = &module.records[0];
        assert_eq!(rec.fields.len(), 1);
        assert_eq!(resolve(&parser, rec.fields[0].name), "size");
        // Default expression must still be retained after the `::` skip.
        assert!(rec.fields[0].default.is_some());
    }

    #[test]
    fn parse_typed_record_field_nested_types() {
        // Nested parens/brackets/braces/maps inside the type annotation must
        // all be skipped correctly by skip_record_field_type.
        let src = r#"
            -module(test).
            -record(state, {
                workers :: [pid()],
                pair :: {atom(), integer()},
                attrs :: #{atom() => term()},
                bin :: <<_:8>>
            }).
            f() -> ok.
        "#;
        let (module, parser) = parse_ok(src);
        let rec = &module.records[0];
        assert_eq!(rec.fields.len(), 4);
        assert_eq!(resolve(&parser, rec.fields[0].name), "workers");
        assert_eq!(resolve(&parser, rec.fields[1].name), "pair");
        assert_eq!(resolve(&parser, rec.fields[2].name), "attrs");
        assert_eq!(resolve(&parser, rec.fields[3].name), "bin");
        for f in &rec.fields {
            assert!(f.default.is_none());
        }
    }

    #[test]
    fn parse_typed_record_field_union_types() {
        // Union types like `pid() | undefined` appear throughout poolboy.
        let src = r#"
            -module(test).
            -record(state, {
                supervisor :: pid() | undefined,
                size = 0 :: non_neg_integer() | infinity
            }).
            f() -> ok.
        "#;
        let (module, parser) = parse_ok(src);
        let rec = &module.records[0];
        assert_eq!(rec.fields.len(), 2);
        assert_eq!(resolve(&parser, rec.fields[0].name), "supervisor");
        assert!(rec.fields[0].default.is_none());
        assert_eq!(resolve(&parser, rec.fields[1].name), "size");
        assert!(rec.fields[1].default.is_some());
    }

    #[test]
    fn parse_typed_record_field_mixed_with_untyped() {
        // Record mixing untyped, typed-without-default, and typed-with-default
        // fields — this is the shape poolboy's `#state{}` uses.
        let src = r#"
            -module(test).
            -record(state, {
                name,
                supervisor :: pid(),
                workers = [] :: [pid()],
                size = 5 :: non_neg_integer()
            }).
            f() -> ok.
        "#;
        let (module, parser) = parse_ok(src);
        let rec = &module.records[0];
        assert_eq!(rec.fields.len(), 4);
        assert_eq!(resolve(&parser, rec.fields[0].name), "name");
        assert!(rec.fields[0].default.is_none());
        assert_eq!(resolve(&parser, rec.fields[1].name), "supervisor");
        assert!(rec.fields[1].default.is_none());
        assert_eq!(resolve(&parser, rec.fields[2].name), "workers");
        assert!(rec.fields[2].default.is_some());
        assert_eq!(resolve(&parser, rec.fields[3].name), "size");
        assert!(rec.fields[3].default.is_some());
    }

    #[test]
    fn parse_typed_record_then_used_in_expression() {
        // A typed record must still be usable in create/update/access/pattern
        // positions — the `::` skip must not corrupt the parser state.
        let src = r#"
            -module(test).
            -record(state, {pid :: pid(), count = 0 :: integer()}).
            new(P) -> #state{pid = P, count = 1}.
            bump(S) -> S#state{count = S#state.count + 1}.
            pid_of(#state{pid = P}) -> P.
        "#;
        let (module, _parser) = parse_ok(src);
        assert_eq!(module.records.len(), 1);
        assert_eq!(module.functions.len(), 3);

        let new_body = &module.functions[0].clauses[0].body;
        assert!(matches!(&new_body[0], Expr::RecordCreate { .. }));

        let bump_body = &module.functions[1].clauses[0].body;
        assert!(matches!(&bump_body[0], Expr::RecordUpdate { .. }));

        let pid_of_pat = &module.functions[2].clauses[0].patterns[0];
        assert!(matches!(pid_of_pat, Pattern::Record { .. }));
    }

    #[test]
    fn parse_record_create() {
        let src = r#"
            -module(test).
            -record(person, {name, age = 0}).
            f() -> #person{name = alice, age = 30}.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        assert!(matches!(&body[0], Expr::RecordCreate { .. }));
    }

    #[test]
    fn parse_record_update() {
        let src = r#"
            -module(test).
            -record(person, {name, age = 0}).
            f(R) -> R#person{name = bob}.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        assert!(matches!(&body[0], Expr::RecordUpdate { .. }));
    }

    #[test]
    fn parse_record_access() {
        let src = r#"
            -module(test).
            -record(person, {name, age = 0}).
            f(R) -> R#person.name.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        assert!(matches!(&body[0], Expr::RecordAccess { .. }));
    }

    #[test]
    fn parse_record_index() {
        let src = r#"
            -module(test).
            -record(person, {name, age = 0}).
            f() -> #person.name.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        assert!(matches!(&body[0], Expr::RecordIndex { .. }));
    }

    #[test]
    fn parse_record_pattern_in_match() {
        let src = r#"
            -module(test).
            -record(person, {name, age = 0}).
            f(P) ->
                #person{name = N} = P,
                N.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Match(pat, _, _) => {
                assert!(matches!(pat.as_ref(), Pattern::Record { .. }));
            }
            other => panic!("expected Match with Record pattern, got {:?}", other),
        }
    }

    #[test]
    fn parse_unknown_attribute_skipped() {
        let src = r#"
            -module(test).
            -type my_type() :: integer().
            -spec f() -> ok.
            f() -> ok.
        "#;
        // Should not error — unknown attributes are silently skipped
        let (module, _parser) = parse_ok(src);
        assert_eq!(module.functions.len(), 1);
    }

    // ── Reserved keyword checks ──────────────────────────────────

    /// Helper: parse source and assert it produces errors.
    fn parse_err(src: &str) -> Vec<ParseError> {
        let mut parser = Parser::new(src);
        let result = parser.parse_module();
        assert!(result.has_errors(), "expected parse error, but parsed successfully");
        result.errors
    }

    #[test]
    fn reserved_keyword_rejected_as_function_name() {
        let src = r#"
            -module(test).
            div(X, Y) -> X.
        "#;
        let errs = parse_err(src);
        assert!(
            errs.iter().any(|e| matches!(e, ParseError::ReservedKeyword { keyword, .. } if keyword == "div")),
            "expected ReservedKeyword error for 'div', got: {:?}", errs
        );
    }

    #[test]
    fn reserved_keyword_rejected_in_export() {
        let src = r#"
            -module(test).
            -export([case/1]).
            f(X) -> X.
        "#;
        let errs = parse_err(src);
        assert!(
            errs.iter().any(|e| matches!(e, ParseError::ReservedKeyword { keyword, .. } if keyword == "case")),
            "expected ReservedKeyword error for 'case', got: {:?}", errs
        );
    }

    #[test]
    fn quoted_reserved_keyword_allowed_as_function() {
        let src = r#"
            -module(test).
            -export(['div'/2]).
            'div'(X, Y) -> X.
        "#;
        let (module, parser) = parse_ok(src);
        assert_eq!(resolve(&parser, module.functions[0].name), "div");
    }

    #[test]
    fn non_reserved_keyword_still_allowed_as_function() {
        // 'module', 'export', 'import' are nera keywords but NOT Erlang reserved words
        let src = r#"
            -module(test).
            -export([module/1]).
            module(X) -> X.
        "#;
        let (module, parser) = parse_ok(src);
        assert_eq!(resolve(&parser, module.functions[0].name), "module");
    }

    #[test]
    fn cond_let_xor_reserved() {
        for kw in &["cond", "let", "xor"] {
            let src = format!(
                "-module(test).\n-export([{kw}/0]).\n{kw}() -> ok.\n"
            );
            let mut parser = Parser::new(&src);
            let result = parser.parse_module();
            assert!(
                result.has_errors(),
                "expected parse error for reserved keyword '{kw}', but parsed OK"
            );
        }
    }

    #[test]
    fn multiple_reserved_keywords_rejected() {
        // 'and', 'or', 'rem' are all reserved
        for kw in &["and", "or", "rem", "band", "bor", "bxor", "bnot", "bsl", "bsr",
                     "after", "begin", "not", "andalso", "orelse", "fun", "if",
                     "case", "try", "catch", "receive", "when", "of", "end"] {
            let src = format!(
                "-module(test).\n{kw}() -> ok.\n"
            );
            let mut parser = Parser::new(&src);
            let result = parser.parse_module();
            assert!(
                result.has_errors(),
                "expected parse error for reserved keyword '{kw}', but parsed OK"
            );
        }
    }

    #[test]
    fn reserved_keyword_error_message_suggests_quoting() {
        let src = r#"
            -module(test).
            -export([div/1]).
            f(X) -> X.
        "#;
        let errs = parse_err(src);
        let msg = errs[0].to_string();
        assert!(
            msg.contains("'div'") && msg.contains("quoted"),
            "error message should suggest quoting: {msg}"
        );
    }

    // ── String escape sequence tests ──────────────────────────────

    #[test]
    fn unescape_basic_escapes() {
        assert_eq!(unescape_erlang_string(r"\n"), "\n");
        assert_eq!(unescape_erlang_string(r"\t"), "\t");
        assert_eq!(unescape_erlang_string(r"\r"), "\r");
        assert_eq!(unescape_erlang_string(r"\r\n"), "\r\n");
        assert_eq!(unescape_erlang_string(r"\\"), "\\");
        assert_eq!(unescape_erlang_string(r"\b"), "\x08");
        assert_eq!(unescape_erlang_string(r"\f"), "\x0C");
        assert_eq!(unescape_erlang_string(r"\e"), "\x1B");
        assert_eq!(unescape_erlang_string(r"\s"), " ");
        assert_eq!(unescape_erlang_string(r"\v"), "\x0B");
        assert_eq!(unescape_erlang_string(r#"\""#), "\"");
        assert_eq!(unescape_erlang_string(r"\'"), "'");
    }

    #[test]
    fn unescape_no_escape() {
        assert_eq!(unescape_erlang_string("hello"), "hello");
        assert_eq!(unescape_erlang_string(""), "");
        assert_eq!(unescape_erlang_string("abc def"), "abc def");
    }

    #[test]
    fn unescape_mixed_content() {
        assert_eq!(unescape_erlang_string(r"hello\nworld"), "hello\nworld");
        assert_eq!(
            unescape_erlang_string(r"STORED\r\n"),
            "STORED\r\n"
        );
        assert_eq!(
            unescape_erlang_string(r"line1\nline2\nline3"),
            "line1\nline2\nline3"
        );
    }

    #[test]
    fn unescape_octal_escapes() {
        // \012 = 10 = newline
        assert_eq!(unescape_erlang_string(r"\012"), "\n");
        // \101 = 65 = 'A'
        assert_eq!(unescape_erlang_string(r"\101"), "A");
        // \0 = null
        assert_eq!(unescape_erlang_string(r"\0"), "\0");
    }

    #[test]
    fn parse_string_with_escape_sequences() {
        let src = r#"
            -module(test).
            -export([test/0]).
            test() -> "hello\r\nworld".
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::String(s, _) => {
                assert_eq!(s, "hello\r\nworld");
                assert_eq!(s.len(), 12); // h e l l o \r \n w o r l d
            }
            other => panic!("expected String, got {:?}", other),
        }
    }

    #[test]
    fn parse_string_with_tab_and_newline() {
        let src = r#"
            -module(test).
            -export([test/0]).
            test() -> "a\tb\nc".
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::String(s, _) => {
                assert_eq!(s, "a\tb\nc");
            }
            other => panic!("expected String, got {:?}", other),
        }
    }

    #[test]
    fn parse_string_concatenation_with_escapes() {
        let src = r#"
            -module(test).
            -export([test/0]).
            test() -> "line1\n" "line2\n".
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::String(s, _) => {
                assert_eq!(s, "line1\nline2\n");
            }
            other => panic!("expected String, got {:?}", other),
        }
    }

    // ── -import declarations ────

    #[test]
    fn parse_import_is_preserved() {
        let src = r#"
            -module(test).
            -import(lists, [map/2, filter/2]).
            -export([foo/0]).
            foo() -> lists:map(fun(X) -> X end, [1,2,3]).
        "#;
        let (module, parser) = parse_ok(src);
        assert_eq!(module.imports.len(), 1);
        assert_eq!(resolve(&parser, module.imports[0].module), "lists");
        assert_eq!(module.imports[0].functions.len(), 2);
        assert_eq!(resolve(&parser, module.name), "test");
        assert_eq!(module.exports.len(), 1);
        assert_eq!(module.functions.len(), 1);
        assert_eq!(resolve(&parser, module.functions[0].name), "foo");
    }

    #[test]
    fn parse_multiple_imports_preserved() {
        let src = r#"
            -module(test).
            -import(lists, [map/2]).
            -import(maps, [get/2]).
            -export([foo/0]).
            foo() -> ok.
        "#;
        let (module, _parser) = parse_ok(src);
        assert_eq!(module.functions.len(), 1);
    }

    #[test]
    fn parse_catch_with_stacktrace() {
        let src = r#"
            -module(test).
            f() ->
                try
                    error(boom)
                catch
                    error:Reason:Stack -> {Reason, Stack}
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Try { catch_clauses, .. } => {
                assert_eq!(catch_clauses.len(), 1);
                assert!(matches!(&catch_clauses[0].class, CatchClass::Error));
                assert!(catch_clauses[0].stacktrace.is_some());
            }
            other => panic!("expected Try, got {:?}", other),
        }
    }

    #[test]
    fn parse_catch_without_stacktrace() {
        let src = r#"
            -module(test).
            f() ->
                try error(boom)
                catch error:Reason -> Reason
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Try { catch_clauses, .. } => {
                assert_eq!(catch_clauses.len(), 1);
                assert!(catch_clauses[0].stacktrace.is_none());
            }
            other => panic!("expected Try, got {:?}", other),
        }
    }

    #[test]
    fn parse_named_fun() {
        let src = r#"
            -module(test).
            f() ->
                fun Loop(0) -> done;
                    Loop(N) -> Loop(N - 1)
                end.
        "#;
        let (module, parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Fun(Some(name), clauses, _) => {
                assert_eq!(resolve(&parser, *name), "Loop");
                assert_eq!(clauses.len(), 2);
            }
            other => panic!("expected named Fun, got {:?}", other),
        }
    }

    #[test]
    fn parse_anonymous_fun_has_no_name() {
        let src = r#"
            -module(test).
            f() -> fun(X) -> X end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Fun(None, clauses, _) => {
                assert_eq!(clauses.len(), 1);
            }
            other => panic!("expected anonymous Fun, got {:?}", other),
        }
    }

    #[test]
    fn parse_dynamic_fun_ref_all_vars() {
        let src = r#"
            -module(test).
            f(M, F, A) -> fun M:F/A.
        "#;
        let (module, parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::FunRef(FunRefTarget::DynamicRemote(m, f, a), _) => {
                match m.as_ref() {
                    Expr::Variable(id, _) => assert_eq!(resolve(&parser, *id), "M"),
                    other => panic!("expected Variable for mod, got {:?}", other),
                }
                match f.as_ref() {
                    Expr::Variable(id, _) => assert_eq!(resolve(&parser, *id), "F"),
                    other => panic!("expected Variable for fun, got {:?}", other),
                }
                match a.as_ref() {
                    Expr::Variable(id, _) => assert_eq!(resolve(&parser, *id), "A"),
                    other => panic!("expected Variable for arity, got {:?}", other),
                }
            }
            other => panic!("expected DynamicRemote FunRef, got {:?}", other),
        }
    }

    #[test]
    fn parse_dynamic_fun_ref_var_mod_atom_fun() {
        let src = r#"
            -module(test).
            f(M) -> fun M:foo/1.
        "#;
        let (module, parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::FunRef(FunRefTarget::DynamicRemote(m, f, a), _) => {
                match m.as_ref() {
                    Expr::Variable(id, _) => assert_eq!(resolve(&parser, *id), "M"),
                    other => panic!("expected Variable for mod, got {:?}", other),
                }
                match f.as_ref() {
                    Expr::Atom(id, _) => assert_eq!(resolve(&parser, *id), "foo"),
                    other => panic!("expected Atom for fun, got {:?}", other),
                }
                match a.as_ref() {
                    Expr::Integer(1, _) => {}
                    other => panic!("expected Integer(1) for arity, got {:?}", other),
                }
            }
            other => panic!("expected DynamicRemote FunRef, got {:?}", other),
        }
    }

    #[test]
    fn parse_dynamic_fun_ref_atom_mod_var_fun() {
        let src = r#"
            -module(test).
            f(F) -> fun erlang:F/1.
        "#;
        let (module, parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::FunRef(FunRefTarget::DynamicRemote(m, f, a), _) => {
                match m.as_ref() {
                    Expr::Atom(id, _) => assert_eq!(resolve(&parser, *id), "erlang"),
                    other => panic!("expected Atom for mod, got {:?}", other),
                }
                match f.as_ref() {
                    Expr::Variable(id, _) => assert_eq!(resolve(&parser, *id), "F"),
                    other => panic!("expected Variable for fun, got {:?}", other),
                }
                match a.as_ref() {
                    Expr::Integer(1, _) => {}
                    other => panic!("expected Integer(1) for arity, got {:?}", other),
                }
            }
            other => panic!("expected DynamicRemote FunRef, got {:?}", other),
        }
    }

    #[test]
    fn parse_static_fun_ref_still_works() {
        // Ensure that fully-static fun M:F/A still produces Remote, not DynamicRemote
        let src = r#"
            -module(test).
            f() -> fun erlang:length/1.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::FunRef(FunRefTarget::Remote(_, _, 1), _) => {}
            other => panic!("expected static Remote FunRef, got {:?}", other),
        }
    }

    #[test]
    fn parse_quoted_atom_with_escapes() {
        let src = r#"
            -module(test).
            f() -> 'it\'s'.
        "#;
        let (module, parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Atom(id, _) => {
                assert_eq!(resolve(&parser, *id), "it's");
            }
            other => panic!("expected Atom, got {:?}", other),
        }
    }

    // ── maybe expression tests ───────────────────────────────

    #[test]
    fn parse_maybe_basic() {
        let src = r#"
            -module(test).
            -export([f/1]).
            f(X) ->
                maybe
                    {ok, V} ?= X,
                    V
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        assert_eq!(body.len(), 1);
        match &body[0] {
            Expr::Maybe { body, else_clauses, .. } => {
                assert_eq!(body.len(), 2);
                assert!(matches!(&body[0], MaybeExpr::CondMatch(..)));
                assert!(matches!(&body[1], MaybeExpr::Expr(..)));
                assert!(else_clauses.is_empty());
            }
            other => panic!("expected Maybe, got {:?}", other),
        }
    }

    #[test]
    fn parse_maybe_with_else() {
        let src = r#"
            -module(test).
            -export([f/1]).
            f(X) ->
                maybe
                    {ok, V} ?= X,
                    V
                else
                    {error, R} -> R
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Maybe { body, else_clauses, .. } => {
                assert_eq!(body.len(), 2);
                assert_eq!(else_clauses.len(), 1);
            }
            other => panic!("expected Maybe, got {:?}", other),
        }
    }

    #[test]
    fn parse_maybe_plain_block() {
        let src = r#"
            -module(test).
            -export([f/0]).
            f() ->
                maybe
                    1 + 2
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Maybe { body, else_clauses, .. } => {
                assert_eq!(body.len(), 1);
                assert!(matches!(&body[0], MaybeExpr::Expr(..)));
                assert!(else_clauses.is_empty());
            }
            other => panic!("expected Maybe, got {:?}", other),
        }
    }

    #[test]
    fn parse_maybe_multiple_cond_match() {
        let src = r#"
            -module(test).
            -export([f/2]).
            f(X, Y) ->
                maybe
                    {ok, A} ?= X,
                    {ok, B} ?= Y,
                    A + B
                end.
        "#;
        let (module, _parser) = parse_ok(src);
        let body = &module.functions[0].clauses[0].body;
        match &body[0] {
            Expr::Maybe { body, .. } => {
                assert_eq!(body.len(), 3);
                assert!(matches!(&body[0], MaybeExpr::CondMatch(..)));
                assert!(matches!(&body[1], MaybeExpr::CondMatch(..)));
                assert!(matches!(&body[2], MaybeExpr::Expr(..)));
            }
            other => panic!("expected Maybe, got {:?}", other),
        }
    }

    // ── Hint tests ────────────────────────────────────────────────

    /// Helper: parse source expecting failure, return the first error.
    fn parse_first_error(source: &str) -> ParseError {
        let mut parser = Parser::new(source);
        let result = parser.parse_module();
        assert!(
            result.has_errors(),
            "expected parse errors but got none"
        );
        result.errors.into_iter().next().unwrap()
    }

    fn error_hint(err: &ParseError) -> Option<&str> {
        match err {
            ParseError::UnexpectedToken { hint, .. } => hint.as_deref(),
            _ => None,
        }
    }

    #[test]
    fn hint_missing_comma_between_args() {
        let err = parse_first_error("-module(test).\nf(X Y) -> ok.\n");
        assert_eq!(error_hint(&err), Some("missing comma between arguments?"));
    }

    #[test]
    fn hint_missing_arrow_before_clause_body() {
        let err = parse_first_error("-module(test).\nf(X) ok.\n");
        assert!(
            error_hint(&err).unwrap().contains("'->'"),
            "expected hint about ->, got {:?}",
            error_hint(&err)
        );
    }

    #[test]
    fn hint_missing_end_for_case() {
        let err = parse_first_error("-module(test).\nf(X) -> case X of 1 -> ok.\n");
        let hint = error_hint(&err).unwrap_or("");
        assert!(hint.contains("'end'") && hint.contains("'case'"),
            "expected hint about end/case, got {:?}", hint);
    }

    #[test]
    fn hint_missing_semicolon_between_case_clauses() {
        // Missing ';' between `ok` and `_ ->` — the `_` looks like a new clause pattern
        let err = parse_first_error("-module(test).\nf(X) -> case X of 1 -> ok _ -> error end.\n");
        let hint = error_hint(&err).unwrap_or("");
        assert!(hint.contains("';'") && hint.contains("'case'"),
            "expected hint about missing semicolon, got {:?}", hint);
    }

    #[test]
    fn hint_missing_semicolon_between_if_clauses() {
        let err = parse_first_error("-module(test).\nf(X) -> if X > 0 -> ok X < 0 -> error end.\n");
        let hint = error_hint(&err).unwrap_or("");
        assert!(hint.contains("';'") && hint.contains("'if'"),
            "expected hint about missing semicolon in if, got {:?}", hint);
    }

    #[test]
    fn hint_missing_semicolon_between_receive_clauses() {
        let err = parse_first_error("-module(test).\nf() -> receive a -> ok b -> error end.\n");
        let hint = error_hint(&err).unwrap_or("");
        assert!(hint.contains("';'") && hint.contains("'receive'"),
            "expected hint about missing semicolon in receive, got {:?}", hint);
    }

    #[test]
    fn hint_module_level_unexpected_token() {
        // `hello` is an atom, so the parser tries to parse it as a function
        // definition and fails expecting '(' for arguments — use a truly
        // unexpected token like a number at module level instead.
        let err = parse_first_error("-module(test).\n42\n");
        assert_eq!(
            error_hint(&err),
            Some("each function or attribute must end with '.'")
        );
    }

    // ── Head mismatch (arity) tests ──────────────────────────────

    #[test]
    fn head_mismatch_different_arity() {
        let src = "-module(test).\nfoo(X) -> X;\nfoo(X, Y) -> X + Y.\n";
        let err = parse_first_error(src);
        match &err {
            ParseError::HeadMismatch { function_name, expected_arity, found_arity, .. } => {
                assert_eq!(function_name, "foo");
                assert_eq!(*expected_arity, 1);
                assert_eq!(*found_arity, 2);
            }
            other => panic!("expected HeadMismatch, got {:?}", other),
        }
    }

    #[test]
    fn consistent_arity_parses_ok() {
        let src = r#"
            -module(test).
            foo(1) -> a;
            foo(2) -> b;
            foo(_) -> c.
        "#;
        let (module, _parser) = parse_ok(src);
        assert_eq!(module.functions[0].clauses.len(), 3);
        assert_eq!(module.functions[0].arity, 1);
    }

    // ── Module name_span tests ───────────────────────────────────

    #[test]
    fn module_name_span_covers_name_atom() {
        let src = r#"
            -module(hello).
            -export([f/0]).
            f() -> ok.
        "#;
        let (module, _parser) = parse_ok(src);
        let name_text = &src[module.name_span.start..module.name_span.end];
        assert_eq!(name_text, "hello");
    }

    #[test]
    fn module_name_span_default_when_missing() {
        // No -module() declaration at all — name_span should be (0,0).
        let src = r#"
            -export([f/0]).
            f() -> ok.
        "#;
        let (module, _parser) = parse_ok(src);
        assert_eq!(module.name_span.start, 0);
        assert_eq!(module.name_span.end, 0);
    }

    // ── FunctionClause name_span tests ───────────────────────────

    #[test]
    fn clause_name_spans_are_distinct() {
        let src = r#"
            -module(test).
            foo(1) -> a;
            foo(2) -> b.
        "#;
        let (module, _parser) = parse_ok(src);
        let func = &module.functions[0];
        assert_eq!(func.clauses.len(), 2);
        // Each clause should have its own name_span pointing at "foo"
        assert_ne!(func.clauses[0].name_span, func.clauses[1].name_span);
        // Both should resolve to "foo" in source
        let name0 = &src[func.clauses[0].name_span.start..func.clauses[0].name_span.end];
        let name1 = &src[func.clauses[1].name_span.start..func.clauses[1].name_span.end];
        assert_eq!(name0, "foo");
        assert_eq!(name1, "foo");
    }

    #[test]
    fn parse_type_def() {
        let src = "-module(test).\n-type my_type() :: integer() | atom().\nfoo() -> ok.\n";
        let mut parser = Parser::new(src);
        let module = { let r = parser.parse_module(); assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors); r.module };
        assert_eq!(module.type_defs.len(), 1);
        let td = &module.type_defs[0];
        assert_eq!(&src[td.name_span.start..td.name_span.end], "my_type");
        assert_eq!(td.arity, 0);
        assert!(!td.is_opaque);
    }

    #[test]
    fn parse_parametric_type() {
        let src = "-module(test).\n-type option(T) :: {some, T} | none.\nfoo() -> ok.\n";
        let mut parser = Parser::new(src);
        let module = { let r = parser.parse_module(); assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors); r.module };
        assert_eq!(module.type_defs.len(), 1);
        let td = &module.type_defs[0];
        assert_eq!(&src[td.name_span.start..td.name_span.end], "option");
        assert_eq!(td.arity, 1);
    }

    #[test]
    fn parse_opaque_type() {
        let src = "-module(test).\n-opaque my_opaque() :: {secret, integer()}.\nfoo() -> ok.\n";
        let mut parser = Parser::new(src);
        let module = { let r = parser.parse_module(); assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors); r.module };
        assert_eq!(module.type_defs.len(), 1);
        assert!(module.type_defs[0].is_opaque);
    }

    #[test]
    fn parse_spec() {
        let src = "-module(test).\n-spec foo(integer(), atom()) -> ok.\nfoo(X, Y) -> ok.\n";
        let mut parser = Parser::new(src);
        let module = { let r = parser.parse_module(); assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors); r.module };
        assert_eq!(module.specs.len(), 1);
        let spec = &module.specs[0];
        assert_eq!(&src[spec.name_span.start..spec.name_span.end], "foo");
        assert_eq!(spec.arity, 2);
        assert!(!spec.is_callback);
    }

    #[test]
    fn parse_callback() {
        let src = "-module(test).\n-callback init(Args :: term()) -> {ok, State :: term()}.\nfoo() -> ok.\n";
        let mut parser = Parser::new(src);
        let module = { let r = parser.parse_module(); assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors); r.module };
        assert_eq!(module.specs.len(), 1);
        assert!(module.specs[0].is_callback);
        assert_eq!(module.specs[0].arity, 1);
    }

    #[test]
    fn parse_spec_with_range() {
        // Test that '..' in type ranges doesn't confuse the dot terminator
        let src = "-module(test).\n-type byte() :: 0..255.\nfoo() -> ok.\n";
        let mut parser = Parser::new(src);
        let module = { let r = parser.parse_module(); assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors); r.module };
        assert_eq!(module.type_defs.len(), 1);
        assert_eq!(module.functions.len(), 1);
    }

    #[test]
    fn parse_multiple_type_attrs() {
        let src = "-module(test).\n-type a() :: integer().\n-type b(X) :: {ok, X}.\n-spec foo() -> a().\nfoo() -> 42.\n";
        let mut parser = Parser::new(src);
        let module = { let r = parser.parse_module(); assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors); r.module };
        assert_eq!(module.type_defs.len(), 2);
        assert_eq!(module.specs.len(), 1);
        assert_eq!(module.functions.len(), 1);
    }

    // ── Fault-tolerant parsing tests ──────────────────────────────

    #[test]
    fn partial_parse_preserves_valid_functions() {
        // One broken function in the middle — the other two should parse fine.
        let src = r#"
            -module(test).
            -export([foo/0, baz/1]).
            foo() -> ok.
            bar( -> broken.
            baz(X) -> X + 1.
        "#;
        let mut parser = Parser::new(src);
        let result = parser.parse_module();
        assert!(result.has_errors(), "should have parse errors");
        let module = result.module;
        assert_eq!(resolve(&parser, module.name), "test");
        assert_eq!(module.exports.len(), 2);
        // foo and baz should be parsed; bar is broken
        assert_eq!(module.functions.len(), 2);
        assert_eq!(resolve(&parser, module.functions[0].name), "foo");
        assert_eq!(resolve(&parser, module.functions[1].name), "baz");
    }

    #[test]
    fn partial_parse_preserves_attributes() {
        // Broken function but attributes should still be captured.
        let src = r#"
            -module(mymod).
            -export([good/0]).
            -record(state, {count, name}).
            broken( -> oops.
            good() -> ok.
        "#;
        let mut parser = Parser::new(src);
        let result = parser.parse_module();
        assert!(result.has_errors());
        let module = result.module;
        assert_eq!(resolve(&parser, module.name), "mymod");
        assert_eq!(module.exports.len(), 1);
        assert_eq!(module.records.len(), 1);
        assert_eq!(module.functions.len(), 1);
        assert_eq!(resolve(&parser, module.functions[0].name), "good");
    }

    #[test]
    fn partial_parse_multiple_errors() {
        // Multiple broken forms — valid forms between them should survive.
        let src = r#"
            -module(test).
            first() -> 1.
            bad1( -> .
            second() -> 2.
            bad2( -> .
            third() -> 3.
        "#;
        let mut parser = Parser::new(src);
        let result = parser.parse_module();
        assert!(result.errors.len() >= 2, "should have at least 2 errors");
        let module = result.module;
        assert_eq!(module.functions.len(), 3);
        assert_eq!(resolve(&parser, module.functions[0].name), "first");
        assert_eq!(resolve(&parser, module.functions[1].name), "second");
        assert_eq!(resolve(&parser, module.functions[2].name), "third");
    }

    #[test]
    fn partial_parse_error_has_span() {
        let src = "-module(test).\nbroken( -> oops.\ngood() -> ok.\n";
        let mut parser = Parser::new(src);
        let result = parser.parse_module();
        assert!(result.has_errors());
        let err = &result.errors[0];
        let span = err.span().expect("error should have a span");
        // The error span should point somewhere within the broken form
        assert!(span.start >= 15, "error span should be after -module(test).\\n");
        assert!(span.end <= src.len());
    }

    #[test]
    fn fully_valid_file_has_no_errors() {
        let src = r#"
            -module(test).
            -export([foo/1, bar/0]).
            foo(X) -> X * 2.
            bar() -> foo(21).
        "#;
        let mut parser = Parser::new(src);
        let result = parser.parse_module();
        assert!(!result.has_errors());
        assert_eq!(result.module.functions.len(), 2);
    }

    // ── parse_term tests ───────────────────────────────────────────

    #[test]
    fn parse_term_atom() {
        let mut parser = Parser::new("myapp.");
        let expr = parser.parse_term().unwrap();
        assert!(matches!(expr, Expr::Atom(_, _)));
    }

    #[test]
    fn parse_term_tuple() {
        let mut parser = Parser::new("{a, b, c}.");
        let expr = parser.parse_term().unwrap();
        match expr {
            Expr::Tuple(elems, _) => assert_eq!(elems.len(), 3),
            other => panic!("expected Tuple, got {:?}", other),
        }
    }

    #[test]
    fn parse_term_nested_app() {
        let mut parser = Parser::new(r#"{application, myapp, [{vsn, "1.0"}]}."#);
        let expr = parser.parse_term().unwrap();
        match expr {
            Expr::Tuple(elems, _) => assert_eq!(elems.len(), 3),
            other => panic!("expected Tuple, got {:?}", other),
        }
    }

    #[test]
    fn parse_term_empty_list() {
        let mut parser = Parser::new("[].");
        let expr = parser.parse_term().unwrap();
        assert!(matches!(expr, Expr::Nil(_)));
    }

    #[test]
    fn parse_term_no_trailing_dot() {
        let mut parser = Parser::new("{ok, value}");
        let expr = parser.parse_term().unwrap();
        assert!(matches!(expr, Expr::Tuple(_, _)));
    }

    #[test]
    fn parse_term_negative_integer() {
        let mut parser = Parser::new("{key, -1}.");
        let expr = parser.parse_term().unwrap();
        match expr {
            Expr::Tuple(elems, _) => assert_eq!(elems.len(), 2),
            other => panic!("expected Tuple, got {:?}", other),
        }
    }

    // ── Attributes after functions ──────────────────────────────

    #[test]
    fn spec_after_function() {
        let src = "-module(test).\nfoo(X) -> X.\n-spec foo(integer()) -> integer().\n";
        let mut parser = Parser::new(src);
        let r = parser.parse_module();
        assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors);
        assert_eq!(r.module.functions.len(), 1);
        assert_eq!(r.module.specs.len(), 1);
        assert_eq!(r.module.specs[0].arity, 1);
    }

    #[test]
    fn record_after_function() {
        let src = "-module(test).\nfoo() -> ok.\n-record(person, {name, age}).\nbar() -> #person{name = a, age = 1}.\n";
        let mut parser = Parser::new(src);
        let r = parser.parse_module();
        assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors);
        assert_eq!(r.module.functions.len(), 2);
        assert_eq!(r.module.records.len(), 1);
    }

    #[test]
    fn type_after_function() {
        let src = "-module(test).\nfoo() -> 42.\n-type my_type() :: integer().\n";
        let mut parser = Parser::new(src);
        let r = parser.parse_module();
        assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors);
        assert_eq!(r.module.functions.len(), 1);
        assert_eq!(r.module.type_defs.len(), 1);
    }

    #[test]
    fn export_after_function() {
        let src = "-module(test).\nfoo() -> ok.\n-export([foo/0]).\n";
        let mut parser = Parser::new(src);
        let r = parser.parse_module();
        assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors);
        assert_eq!(r.module.functions.len(), 1);
        assert_eq!(r.module.exports.len(), 1);
    }

    #[test]
    fn export_type_after_function() {
        let src = "-module(test).\n-type t() :: atom().\nfoo() -> ok.\n-export_type([t/0]).\n";
        let mut parser = Parser::new(src);
        let r = parser.parse_module();
        assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors);
        assert_eq!(r.module.type_exports.len(), 1);
    }

    #[test]
    fn behaviour_after_function() {
        let src = "-module(test).\ninit(_) -> {ok, #{}}.\n-behaviour(gen_server).\n";
        let mut parser = Parser::new(src);
        let r = parser.parse_module();
        assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors);
        assert_eq!(r.module.behaviours.len(), 1);
    }

    #[test]
    fn interleaved_attrs_and_functions() {
        let src = r#"
            -module(test).
            -export([foo/0, bar/0]).
            foo() -> ok.
            -record(state, {count}).
            -type state() :: #state{}.
            bar() -> #state{count = 0}.
            -spec baz(integer()) -> integer().
            baz(X) -> X + 1.
        "#;
        let mut parser = Parser::new(src);
        let r = parser.parse_module();
        assert!(r.errors.is_empty(), "parse errors: {:#?}", r.errors);
        assert_eq!(r.module.functions.len(), 3);
        assert_eq!(r.module.records.len(), 1);
        assert_eq!(r.module.type_defs.len(), 1);
        assert_eq!(r.module.specs.len(), 1);
    }
}
