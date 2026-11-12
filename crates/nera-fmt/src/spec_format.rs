//! Formatter for `-spec`, `-callback`, `-type`, and `-opaque` declarations.
//!
//! Works at the token level (no type expression AST) — lex the form, walk
//! tokens to build a lightweight structure, convert to `Doc`, render.
//! Returns `None` on any unexpected token so the caller can fall back to
//! verbatim passthrough.

use nera_syntax::lexer::Lexer;
use nera_syntax::token::Token;

use crate::config::FormatConfig;
use crate::doc::*;

// ── Detection ──────────────────────────────────────────────────────

/// Check whether a (trimmed, leading-comments-stripped) form is a spec,
/// callback, type, or opaque attribute that we should try to format.
pub(crate) fn is_spec_or_type_attribute(s: &str) -> bool {
    let s = strip_leading_comments(s);
    s.starts_with("-spec ")
        || s.starts_with("-spec(")
        || s.starts_with("-callback ")
        || s.starts_with("-callback(")
        || s.starts_with("-type ")
        || s.starts_with("-opaque ")
}

/// Try to format a spec/callback/type/opaque form.
/// Returns `None` if lexing or parsing fails (caller preserves verbatim).
pub(crate) fn try_format_spec_form(source: &str, config: &FormatConfig) -> Option<String> {
    let tokens = Lexer::lex_all(source).ok()?;
    let tok_texts: Vec<(Token, String)> = tokens
        .iter()
        .map(|(tok, span)| (tok.clone(), source[span.start..span.end].to_string()))
        .collect();

    let mut cursor = Cursor::new(&tok_texts);
    let form = cursor.parse_form()?;
    let doc = form_to_doc(&form, config);
    Some(render(config.max_width, &doc))
}

// ── Lightweight IR ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum FormKind {
    Spec,
    Callback,
    Type,
    Opaque,
}

#[derive(Debug)]
struct SpecForm {
    kind: FormKind,
    name: String,
    clauses: Vec<SpecClause>,
}

#[derive(Debug)]
struct SpecClause {
    /// Parameter type expressions (comma-separated groups of tokens).
    params: Vec<Vec<(Token, String)>>,
    /// Return type tokens (after `->`, before `when`/`;`/`.`).
    return_type: Vec<(Token, String)>,
    /// When constraints.
    when_constraints: Vec<WhenConstraint>,
}

#[derive(Debug)]
struct TypeForm {
    kind: FormKind,
    name: String,
    params: Vec<Vec<(Token, String)>>,
    body: Vec<(Token, String)>,
}

#[derive(Debug)]
struct WhenConstraint {
    var: String,
    type_expr: Vec<(Token, String)>,
}

#[derive(Debug)]
enum ParsedForm {
    Spec(SpecForm),
    TypeDef(TypeForm),
}

// ── Token cursor ───────────────────────────────────────────────────

struct Cursor<'a> {
    tokens: &'a [(Token, String)],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(tokens: &'a [(Token, String)]) -> Self {
        Cursor { tokens, pos: 0 }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|(t, _)| t)
    }

    fn peek_text(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(|(_, s)| s.as_str())
    }

    fn advance(&mut self) -> Option<&(Token, String)> {
        if self.pos < self.tokens.len() {
            let item = &self.tokens[self.pos];
            self.pos += 1;
            Some(item)
        } else {
            None
        }
    }

    fn expect(&mut self, expected: &Token) -> Option<&(Token, String)> {
        if self.peek() == Some(expected) {
            self.advance()
        } else {
            None
        }
    }

    /// Check if the current token is an atom-like token (atom, quoted atom,
    /// or a keyword used as an identifier).
    fn is_atom_like(&self) -> bool {
        matches!(
            self.peek(),
            Some(
                Token::Atom
                    | Token::QuotedAtom
                    | Token::KwModule
                    | Token::KwExport
                    | Token::KwImport
                    | Token::KwCase
                    | Token::KwOf
                    | Token::KwEnd
                    | Token::KwIf
                    | Token::KwWhen
                    | Token::KwReceive
                    | Token::KwAfter
                    | Token::KwFun
                    | Token::KwTrue
                    | Token::KwFalse
                    | Token::KwNot
                    | Token::KwAnd
                    | Token::KwOr
                    | Token::KwAndalso
                    | Token::KwOrelse
                    | Token::KwDiv
                    | Token::KwRem
                    | Token::KwBand
                    | Token::KwBor
                    | Token::KwBxor
                    | Token::KwBnot
                    | Token::KwBsl
                    | Token::KwBsr
                    | Token::KwTry
                    | Token::KwCatch
                    | Token::KwThrow
                    | Token::KwBegin
                    | Token::KwCond
                    | Token::KwLet
                    | Token::KwXor
                    | Token::KwMaybe
                    | Token::KwElse
            )
        )
    }

    fn consume_atom_name(&mut self) -> Option<String> {
        if self.is_atom_like() {
            let (_, text) = self.advance()?;
            Some(text.clone())
        } else {
            None
        }
    }

    // ── Form parser ────────────────────────────────────────────

    fn parse_form(&mut self) -> Option<ParsedForm> {
        // Consume `-`
        self.expect(&Token::Minus)?;

        // Consume keyword: spec/callback/type/opaque
        let kind_text = self.peek_text()?.to_string();
        let kind = match kind_text.as_str() {
            "spec" => FormKind::Spec,
            "callback" => FormKind::Callback,
            "type" => FormKind::Type,
            "opaque" => FormKind::Opaque,
            _ => return None,
        };
        self.advance(); // consume keyword

        // Handle optional outer parenthesization: -spec(name(...) -> T).
        let outer_paren = if kind == FormKind::Spec || kind == FormKind::Callback {
            if self.peek() == Some(&Token::LParen) {
                // Peek ahead: is the token after `(` an atom-like (the name)?
                // If so, treat as parenthesized form.
                if self.tokens.get(self.pos + 1).map(|(t, _)| t) == Some(&Token::Atom)
                    || self.tokens.get(self.pos + 1).map(|(t, _)| t) == Some(&Token::QuotedAtom)
                    || self
                        .tokens
                        .get(self.pos + 1)
                        .map(|(t, _)| matches!(t, Token::Atom | Token::QuotedAtom))
                        .unwrap_or(false)
                {
                    // Check if the atom is followed by ( — that confirms it's a
                    // function name, not a type expression starting with (
                    if self
                        .tokens
                        .get(self.pos + 2)
                        .map(|(t, _)| t == &Token::LParen || t == &Token::Colon)
                        .unwrap_or(false)
                    {
                        self.advance(); // consume outer (
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        // Consume name (possibly module-qualified: Module:Name)
        let mut name = self.consume_atom_name()?;
        if self.peek() == Some(&Token::Colon) {
            // Check it's not `::` (two colons)
            if self
                .tokens
                .get(self.pos + 1)
                .map(|(t, _)| t != &Token::Colon)
                .unwrap_or(true)
            {
                self.advance(); // consume ':'
                let local = self.consume_atom_name()?;
                name = format!("{}:{}", name, local);
            }
        }

        match kind {
            FormKind::Spec | FormKind::Callback => {
                let spec = self.parse_spec_body(kind, name, outer_paren)?;
                Some(ParsedForm::Spec(spec))
            }
            FormKind::Type | FormKind::Opaque => {
                let typedef = self.parse_type_body(kind, name)?;
                Some(ParsedForm::TypeDef(typedef))
            }
        }
    }

    fn parse_spec_body(
        &mut self,
        kind: FormKind,
        name: String,
        outer_paren: bool,
    ) -> Option<SpecForm> {
        let mut clauses = Vec::new();

        loop {
            let clause = self.parse_spec_clause()?;
            clauses.push(clause);

            // After a clause: `;` means another clause, `.` means end
            if self.peek() == Some(&Token::Semicolon) {
                self.advance(); // consume `;`
            } else {
                break;
            }
        }

        // Consume optional outer `)` for parenthesized form
        if outer_paren {
            self.expect(&Token::RParen);
        }

        // Consume terminal `.`
        // (may already be consumed or at end)
        if self.peek() == Some(&Token::Dot) {
            self.advance();
        }

        Some(SpecForm {
            kind,
            name,
            clauses,
        })
    }

    fn parse_spec_clause(&mut self) -> Option<SpecClause> {
        // Consume `(` params `)`
        self.expect(&Token::LParen)?;
        let params = self.collect_comma_separated()?;

        // Consume `->`
        self.expect(&Token::Arrow)?;

        // Collect return type tokens until `when`, `;`, `)` (outer paren), or `.` at depth 0
        let mut return_type = Vec::new();
        let mut depth: i32 = 0;
        while !self.at_end() {
            match self.peek()? {
                Token::LParen | Token::LBrace | Token::LBracket | Token::BinOpen => {
                    depth += 1;
                    return_type.push(self.advance()?.clone());
                }
                Token::RParen | Token::RBrace | Token::RBracket | Token::BinClose => {
                    if depth == 0 {
                        break; // outer paren
                    }
                    depth -= 1;
                    return_type.push(self.advance()?.clone());
                }
                Token::KwWhen if depth == 0 => break,
                Token::Semicolon if depth == 0 => break,
                Token::Dot if depth == 0 => break,
                _ => {
                    return_type.push(self.advance()?.clone());
                }
            }
        }

        // Parse optional `when` constraints
        let when_constraints = if self.peek() == Some(&Token::KwWhen) {
            self.advance(); // consume `when`
            self.parse_when_constraints()?
        } else {
            Vec::new()
        };

        Some(SpecClause {
            params,
            return_type,
            when_constraints,
        })
    }

    fn parse_type_body(&mut self, kind: FormKind, name: String) -> Option<TypeForm> {
        // Consume optional `(` params `)`
        let params = if self.peek() == Some(&Token::LParen) {
            self.advance(); // consume `(`
            self.collect_comma_separated()?
        } else {
            Vec::new()
        };

        // Consume `::` (two Colon tokens)
        self.expect(&Token::Colon)?;
        self.expect(&Token::Colon)?;

        // Collect body tokens until terminal `.`
        // Be careful: `..` (two dots) is a range operator, not a terminator.
        let mut body = Vec::new();
        let mut depth: i32 = 0;
        while !self.at_end() {
            match self.peek()? {
                Token::LParen | Token::LBrace | Token::LBracket | Token::BinOpen => {
                    depth += 1;
                    body.push(self.advance()?.clone());
                }
                Token::RParen | Token::RBrace | Token::RBracket | Token::BinClose => {
                    depth -= 1;
                    body.push(self.advance()?.clone());
                }
                Token::Dot if depth == 0 => {
                    // Check if this is `..` (range) or `...` (ellipsis) rather
                    // than a terminating `.`
                    if self
                        .tokens
                        .get(self.pos + 1)
                        .map(|(t, _)| t == &Token::Dot)
                        .unwrap_or(false)
                    {
                        // It's `..` or `...` — consume all consecutive dots
                        body.push(self.advance()?.clone()); // first dot
                        body.push(self.advance()?.clone()); // second dot
                        // Check for third dot (`...`)
                        if self.peek() == Some(&Token::Dot) {
                            body.push(self.advance()?.clone());
                        }
                    } else {
                        self.advance(); // consume terminal `.`
                        break;
                    }
                }
                _ => {
                    body.push(self.advance()?.clone());
                }
            }
        }

        Some(TypeForm {
            kind,
            name,
            params,
            body,
        })
    }

    /// Collect comma-separated groups of tokens inside balanced parens.
    /// Assumes the opening `(` has already been consumed.
    /// Consumes the closing `)`.
    fn collect_comma_separated(&mut self) -> Option<Vec<Vec<(Token, String)>>> {
        let mut groups: Vec<Vec<(Token, String)>> = Vec::new();
        let mut current: Vec<(Token, String)> = Vec::new();
        let mut depth: i32 = 0;

        // Handle empty params: ()
        if self.peek() == Some(&Token::RParen) {
            self.advance();
            return Some(groups);
        }

        while !self.at_end() {
            match self.peek()? {
                Token::LParen | Token::LBrace | Token::LBracket | Token::BinOpen => {
                    depth += 1;
                    current.push(self.advance()?.clone());
                }
                Token::RParen | Token::RBrace | Token::RBracket | Token::BinClose => {
                    if depth == 0 {
                        // Closing paren of the parameter list
                        if !current.is_empty() {
                            groups.push(current);
                        }
                        self.advance(); // consume `)`
                        return Some(groups);
                    }
                    depth -= 1;
                    current.push(self.advance()?.clone());
                }
                Token::Comma if depth == 0 => {
                    groups.push(std::mem::take(&mut current));
                    self.advance(); // consume `,`
                }
                _ => {
                    current.push(self.advance()?.clone());
                }
            }
        }
        None // unexpected end
    }

    /// Parse `when` constraints: `Var :: TypeExpr, Var :: TypeExpr, ...`
    /// Stops at `;` or `.` at depth 0.
    fn parse_when_constraints(&mut self) -> Option<Vec<WhenConstraint>> {
        let mut constraints = Vec::new();

        loop {
            // Expect a variable name
            if self.peek() != Some(&Token::Variable) {
                break;
            }
            let var = self.advance()?.1.clone();

            // Expect `::`
            self.expect(&Token::Colon)?;
            self.expect(&Token::Colon)?;

            // Collect type expression tokens until `,`, `;`, or `.` at depth 0
            let mut type_expr = Vec::new();
            let mut depth: i32 = 0;
            while !self.at_end() {
                match self.peek()? {
                    Token::LParen | Token::LBrace | Token::LBracket | Token::BinOpen => {
                        depth += 1;
                        type_expr.push(self.advance()?.clone());
                    }
                    Token::RParen | Token::RBrace | Token::RBracket | Token::BinClose => {
                        if depth == 0 {
                            break; // outer paren
                        }
                        depth -= 1;
                        type_expr.push(self.advance()?.clone());
                    }
                    Token::Comma if depth == 0 => break,
                    Token::Semicolon if depth == 0 => break,
                    Token::Dot if depth == 0 => break,
                    _ => {
                        type_expr.push(self.advance()?.clone());
                    }
                }
            }

            constraints.push(WhenConstraint { var, type_expr });

            // If next is `,`, consume and continue; otherwise stop
            if self.peek() == Some(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }

        Some(constraints)
    }
}

// ── Doc generation ─────────────────────────────────────────────────

fn form_to_doc(form: &ParsedForm, config: &FormatConfig) -> Doc {
    match form {
        ParsedForm::Spec(spec) => spec_to_doc(spec, config),
        ParsedForm::TypeDef(typedef) => typedef_to_doc(typedef, config),
    }
}

fn spec_to_doc(spec: &SpecForm, config: &FormatConfig) -> Doc {
    let indent = config.indent_size as i32;
    let kind_str = match spec.kind {
        FormKind::Spec => "-spec ",
        FormKind::Callback => "-callback ",
        _ => unreachable!(),
    };

    if spec.clauses.len() == 1 {
        // Single clause: wrap in a group for flat/break decisions
        let clause = &spec.clauses[0];
        let clause_doc = spec_clause_to_doc(clause, indent);
        group(concat_all(vec![
            text(kind_str),
            text(&spec.name),
            clause_doc,
            text("."),
        ]))
    } else {
        // Multi-clause: subsequent clauses align under the opening `(`
        let prefix_width = kind_str.len() + spec.name.len();
        let mut parts = Vec::new();

        for (i, clause) in spec.clauses.iter().enumerate() {
            if i > 0 {
                parts.push(text(";"));
                parts.push(hardline());
                parts.push(text(" ".repeat(prefix_width)));
            }
            parts.push(spec_clause_to_doc(clause, indent));
        }

        concat_all(vec![
            text(kind_str),
            text(&spec.name),
            concat_all(parts),
            text("."),
        ])
    }
}

fn spec_clause_to_doc(clause: &SpecClause, indent: i32) -> Doc {
    let params_doc = params_to_doc(&clause.params);
    let return_with_unions = format_with_union_breaks(&clause.return_type);

    let mut parts = vec![text("("), params_doc, text(")"), text(" ->")];

    if clause.when_constraints.is_empty() {
        // No when: return type can stay on arrow line or break
        parts.push(group(nest(indent, concat(line(), return_with_unions))));
    } else {
        // With when: return type then `when` on next line
        parts.push(group(nest(indent, concat(line(), return_with_unions))));
        parts.push(text(" when"));
        let constraint_docs: Vec<Doc> = clause
            .when_constraints
            .iter()
            .map(|c| {
                let type_doc = tokens_to_doc(&c.type_expr);
                concat_all(vec![text(&c.var), text(" :: "), type_doc])
            })
            .collect();
        parts.push(nest(
            indent,
            concat(
                line(),
                intersperse(constraint_docs, concat(text(","), line())),
            ),
        ));
    }

    concat_all(parts)
}

fn typedef_to_doc(typedef: &TypeForm, config: &FormatConfig) -> Doc {
    let indent = config.indent_size as i32;
    let kind_str = match typedef.kind {
        FormKind::Type => "-type ",
        FormKind::Opaque => "-opaque ",
        _ => unreachable!(),
    };

    let params_doc = if typedef.params.is_empty() {
        text("()")
    } else {
        let inner = params_to_doc(&typedef.params);
        concat_all(vec![text("("), inner, text(")")])
    };

    let body_doc = format_with_union_breaks(&typedef.body);

    group(concat_all(vec![
        text(kind_str),
        text(&typedef.name),
        params_doc,
        text(" ::"),
        group(nest(indent, concat(line(), body_doc))),
        text("."),
    ]))
}

// ── Token-to-Doc helpers ───────────────────────────────────────────

/// Format comma-separated parameter groups.
fn params_to_doc(params: &[Vec<(Token, String)>]) -> Doc {
    if params.is_empty() {
        return nil();
    }
    let items: Vec<Doc> = params.iter().map(|p| tokens_to_doc(p)).collect();
    intersperse(items, concat(text(","), text(" ")))
}

/// Convert a sequence of tokens to a Doc with proper spacing.
fn tokens_to_doc(tokens: &[(Token, String)]) -> Doc {
    if tokens.is_empty() {
        return nil();
    }

    let mut parts = Vec::new();
    let mut i = 0;
    // Track what the previous token was for spacing decisions
    let mut prev_tok: Option<Token> = None;

    while i < tokens.len() {
        let (tok, txt) = &tokens[i];

        // Check for `::` (two consecutive colons)
        if *tok == Token::Colon && tokens.get(i + 1).map(|(t, _)| t) == Some(&Token::Colon) {
            ensure_space(&mut parts);
            parts.push(text("::"));
            i += 2;
            prev_tok = Some(Token::Colon); // marker for ::
            if i < tokens.len() {
                parts.push(text(" "));
            }
            continue;
        }

        // Check for `..` (two consecutive dots — range type)
        if *tok == Token::Dot && tokens.get(i + 1).map(|(t, _)| t) == Some(&Token::Dot) {
            // Check for `...` (three dots — ellipsis in fun types)
            if tokens.get(i + 2).map(|(t, _)| t) == Some(&Token::Dot) {
                parts.push(text("..."));
                i += 3;
                prev_tok = Some(Token::Dot);
            } else {
                parts.push(text(".."));
                i += 2;
                prev_tok = Some(Token::Dot);
            }
            continue;
        }

        match tok {
            // Opening brackets: may need space before, never space after
            Token::LParen | Token::LBrace | Token::LBracket | Token::BinOpen => {
                // No space before `(` (function call syntax)
                // No space before `{` after atom/keyword (record syntax: #rec{})
                let space = match tok {
                    Token::LParen => false,
                    Token::LBrace => {
                        // After an atom it's record syntax (#rec{...}), no space
                        !matches!(
                            prev_tok,
                            Some(
                                Token::Atom
                                    | Token::QuotedAtom
                                    | Token::KwModule
                                    | Token::KwExport
                                    | Token::KwImport
                                    | Token::KwCase
                                    | Token::KwOf
                                    | Token::KwEnd
                                    | Token::KwIf
                                    | Token::KwWhen
                                    | Token::KwReceive
                                    | Token::KwAfter
                                    | Token::KwFun
                                    | Token::KwTrue
                                    | Token::KwFalse
                                    | Token::KwNot
                                    | Token::KwAnd
                                    | Token::KwOr
                                    | Token::KwAndalso
                                    | Token::KwOrelse
                                    | Token::KwDiv
                                    | Token::KwRem
                                    | Token::KwBand
                                    | Token::KwBor
                                    | Token::KwBxor
                                    | Token::KwBnot
                                    | Token::KwBsl
                                    | Token::KwBsr
                                    | Token::KwTry
                                    | Token::KwCatch
                                    | Token::KwThrow
                                    | Token::KwBegin
                                    | Token::KwCond
                                    | Token::KwLet
                                    | Token::KwXor
                                    | Token::KwMaybe
                                    | Token::KwElse
                                    | Token::Hash
                            )
                        ) && needs_space_between(prev_tok.as_ref(), tok)
                    }
                    _ => needs_space_between(prev_tok.as_ref(), tok),
                };
                if space {
                    ensure_space(&mut parts);
                }
                parts.push(text(txt.as_str()));
                prev_tok = Some(tok.clone());
                i += 1;
            }
            // Closing brackets: no space before
            Token::RParen | Token::RBrace | Token::RBracket | Token::BinClose => {
                remove_trailing_space(&mut parts);
                parts.push(text(txt.as_str()));
                prev_tok = Some(tok.clone());
                i += 1;
            }
            // Comma: no space before, space after
            Token::Comma => {
                remove_trailing_space(&mut parts);
                parts.push(text(","));
                prev_tok = Some(Token::Comma);
                i += 1;
                if i < tokens.len() {
                    parts.push(text(" "));
                }
            }
            // Pipe: space around
            Token::Pipe => {
                ensure_space(&mut parts);
                parts.push(text("|"));
                prev_tok = Some(Token::Pipe);
                i += 1;
                if i < tokens.len() {
                    parts.push(text(" "));
                }
            }
            // Arrow: space around
            Token::Arrow => {
                ensure_space(&mut parts);
                parts.push(text("->"));
                prev_tok = Some(Token::Arrow);
                i += 1;
                if i < tokens.len() {
                    parts.push(text(" "));
                }
            }
            // Fat arrow (maps): space around
            Token::FatArrow => {
                ensure_space(&mut parts);
                parts.push(text("=>"));
                prev_tok = Some(Token::FatArrow);
                i += 1;
                if i < tokens.len() {
                    parts.push(text(" "));
                }
            }
            // ColonEq (maps): space around
            Token::ColonEq => {
                ensure_space(&mut parts);
                parts.push(text(":="));
                prev_tok = Some(Token::ColonEq);
                i += 1;
                if i < tokens.len() {
                    parts.push(text(" "));
                }
            }
            // Single colon: no spaces (module qualifier)
            Token::Colon => {
                remove_trailing_space(&mut parts);
                parts.push(text(":"));
                prev_tok = Some(Token::Colon);
                i += 1;
            }
            // Hash (record type): no space after
            Token::Hash => {
                // Space before # only if preceded by identifier/close bracket
                if needs_space_between(prev_tok.as_ref(), tok) {
                    ensure_space(&mut parts);
                }
                parts.push(text("#"));
                prev_tok = Some(Token::Hash);
                i += 1;
            }
            // Minus: no space after (unary in type context)
            Token::Minus => {
                parts.push(text("-"));
                prev_tok = Some(Token::Minus);
                i += 1;
            }
            // `fun` keyword
            Token::KwFun => {
                if needs_space_between(prev_tok.as_ref(), tok) {
                    ensure_space(&mut parts);
                }
                parts.push(text("fun"));
                prev_tok = Some(Token::KwFun);
                i += 1;
                // fun( — no space before paren
            }
            // DoublePipe ||: space around
            Token::DoublePipe => {
                ensure_space(&mut parts);
                parts.push(text("||"));
                prev_tok = Some(Token::DoublePipe);
                i += 1;
                if i < tokens.len() {
                    parts.push(text(" "));
                }
            }
            // Dot: single dot
            Token::Dot => {
                parts.push(text("."));
                prev_tok = Some(Token::Dot);
                i += 1;
            }
            // Semicolon
            Token::Semicolon => {
                remove_trailing_space(&mut parts);
                parts.push(text(";"));
                prev_tok = Some(Token::Semicolon);
                i += 1;
                if i < tokens.len() {
                    parts.push(text(" "));
                }
            }
            // Everything else (atoms, variables, integers, keywords, etc.)
            _ => {
                if needs_space_between(prev_tok.as_ref(), tok) {
                    ensure_space(&mut parts);
                }
                parts.push(text(txt.as_str()));
                prev_tok = Some(tok.clone());
                i += 1;
            }
        }
    }

    concat_all(parts)
}

/// Split tokens on `|` at depth 0 and format with union line breaks.
fn format_with_union_breaks(tokens: &[(Token, String)]) -> Doc {
    let alternatives = split_on_pipe(tokens);
    if alternatives.len() <= 1 {
        return tokens_to_doc(tokens);
    }

    let alt_docs: Vec<Doc> = alternatives.iter().map(|alt| tokens_to_doc(alt)).collect();

    let first = alt_docs[0].clone();
    let rest: Vec<Doc> = alt_docs[1..]
        .iter()
        .map(|d| concat(line(), concat(text("| "), d.clone())))
        .collect();

    group(concat(first, concat_all(rest)))
}

/// Split a token slice on `|` at bracket depth 0.
fn split_on_pipe(tokens: &[(Token, String)]) -> Vec<Vec<(Token, String)>> {
    let mut groups: Vec<Vec<(Token, String)>> = Vec::new();
    let mut current: Vec<(Token, String)> = Vec::new();
    let mut depth: i32 = 0;

    for (tok, txt) in tokens {
        match tok {
            Token::LParen | Token::LBrace | Token::LBracket | Token::BinOpen => {
                depth += 1;
                current.push((tok.clone(), txt.clone()));
            }
            Token::RParen | Token::RBrace | Token::RBracket | Token::BinClose => {
                depth -= 1;
                current.push((tok.clone(), txt.clone()));
            }
            Token::Pipe if depth == 0 => {
                groups.push(std::mem::take(&mut current));
            }
            _ => {
                current.push((tok.clone(), txt.clone()));
            }
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

/// Remove a trailing space `Doc::Text(" ")` from parts if present.
fn remove_trailing_space(parts: &mut Vec<Doc>) {
    if let Some(Doc::Text(s)) = parts.last() {
        if s == " " {
            parts.pop();
        }
    }
}

/// Ensure there is a trailing space in parts (add one if not present).
fn ensure_space(parts: &mut Vec<Doc>) {
    match parts.last() {
        Some(Doc::Text(s)) if s == " " => {} // already have space
        _ => parts.push(text(" ")),
    }
}

/// Decide whether a space is needed between two adjacent tokens.
fn needs_space_between(prev: Option<&Token>, cur: &Token) -> bool {
    let prev = match prev {
        Some(p) => p,
        None => return false, // first token — no leading space
    };

    // Never space before closing brackets, comma, semicolon, colon, dot
    if matches!(
        cur,
        Token::RParen
            | Token::RBrace
            | Token::RBracket
            | Token::BinClose
            | Token::Comma
            | Token::Semicolon
            | Token::Colon
            | Token::Dot
    ) {
        return false;
    }

    // Never space after opening brackets
    if matches!(
        prev,
        Token::LParen | Token::LBrace | Token::LBracket | Token::BinOpen
    ) {
        return false;
    }

    // Never space after `#` (record syntax)
    if matches!(prev, Token::Hash) {
        return false;
    }

    // Never space after single `:` (module qualifier) or `.` (range `..`)
    if matches!(prev, Token::Colon | Token::Dot) {
        return false;
    }

    // Never space after unary `-`
    if matches!(prev, Token::Minus) {
        return false;
    }

    // Space between identifier/literal/close-bracket and open bracket
    // EXCEPT `(` which never needs space before (function call syntax)
    if *cur == Token::LParen {
        return false;
    }

    // Default: space between two words/identifiers/literals
    true
}

/// Strip leading comment lines (same logic as format.rs).
fn strip_leading_comments(s: &str) -> &str {
    let mut start = 0;
    for line in s.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('%') || trimmed.is_empty() {
            start += line.len() + 1;
        } else {
            break;
        }
    }
    if start >= s.len() {
        s
    } else {
        &s[start..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(source: &str) -> String {
        let config = FormatConfig::default();
        try_format_spec_form(source, &config).unwrap_or_else(|| source.to_string())
    }

    #[test]
    fn simple_spec() {
        assert_eq!(fmt("-spec test() -> ok."), "-spec test() -> ok.");
    }

    #[test]
    fn spec_with_params() {
        assert_eq!(
            fmt("-spec foo(atom(), integer()) -> ok."),
            "-spec foo(atom(), integer()) -> ok."
        );
    }

    #[test]
    fn simple_type() {
        assert_eq!(fmt("-type name() :: atom()."), "-type name() :: atom().");
    }

    #[test]
    fn opaque_type() {
        assert_eq!(
            fmt("-opaque internal() :: {secret, term()}."),
            "-opaque internal() :: {secret, term()}."
        );
    }

    #[test]
    fn spec_with_when() {
        assert_eq!(
            fmt("-spec id(X) -> X when X :: term()."),
            "-spec id(X) -> X when X :: term()."
        );
    }

    #[test]
    fn type_with_union() {
        assert_eq!(
            fmt("-type result() :: {ok, term()} | {error, term()}."),
            "-type result() :: {ok, term()} | {error, term()}."
        );
    }

    #[test]
    fn callback_simple() {
        assert_eq!(
            fmt("-callback init(Args :: term()) -> {ok, term()}."),
            "-callback init(Args :: term()) -> {ok, term()}."
        );
    }

    #[test]
    fn type_with_range() {
        assert_eq!(
            fmt("-type byte_val() :: 0..255."),
            "-type byte_val() :: 0..255."
        );
    }

    #[test]
    fn type_with_fun() {
        assert_eq!(
            fmt("-type handler() :: fun((term()) -> ok | error)."),
            "-type handler() :: fun((term()) -> ok | error)."
        );
    }

    #[test]
    fn type_with_ellipsis_fun() {
        assert_eq!(
            fmt("-type any_fun() :: fun((...) -> term())."),
            "-type any_fun() :: fun((...) -> term())."
        );
    }

    #[test]
    fn multi_clause_spec() {
        let input = "-spec convert(integer()) -> binary(); (binary()) -> integer().";
        let result = fmt(input);
        assert!(result.contains("-spec convert(integer()) -> binary();"));
        assert!(result.contains("             (binary()) -> integer()."));
    }

    #[test]
    fn spec_idempotent() {
        let inputs = vec![
            "-spec test() -> ok.",
            "-spec foo(atom(), integer()) -> ok.",
            "-spec id(X) -> X when X :: term().",
            "-type name() :: atom().",
            "-type result() :: {ok, term()} | {error, term()}.",
            "-callback init(Args :: term()) -> {ok, term()}.",
        ];
        let config = FormatConfig::default();
        for input in inputs {
            let first = try_format_spec_form(input, &config).unwrap();
            let second = try_format_spec_form(first.trim(), &config).unwrap();
            assert_eq!(first, second, "not idempotent for: {}", input);
        }
    }

    #[test]
    fn parenthesized_spec() {
        let input = "-spec(foo(atom()) -> ok).";
        let result = fmt(input);
        // Should format without the outer parens
        assert_eq!(result, "-spec foo(atom()) -> ok.");
    }

    #[test]
    fn type_with_map() {
        assert_eq!(
            fmt("-type my_map() :: #{atom() => term()}."),
            "-type my_map() :: #{atom() => term()}."
        );
    }

    #[test]
    fn type_with_record() {
        assert_eq!(
            fmt("-type state() :: #state{}."),
            "-type state() :: #state{}."
        );
    }

    #[test]
    fn remote_type() {
        assert_eq!(
            fmt("-type ref() :: pid() | atom() | {atom(), node()}."),
            "-type ref() :: pid() | atom() | {atom(), node()}."
        );
    }
}
