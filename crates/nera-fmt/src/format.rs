use string_interner::DefaultStringInterner;

use nera_syntax::ast::*;
use nera_syntax::span::Span;

use crate::comment::Comment;
use crate::config::FormatConfig;
use crate::doc::*;

/// Formats an Erlang module AST into a Doc.
pub struct Formatter<'src> {
    source: &'src str,
    comments: Vec<Comment>,
    comment_cursor: usize,
    /// Trailing comments skipped by `drain_comments_before`, waiting to be
    /// picked up by `take_trailing_comment`.
    pending_trailing: Vec<Comment>,
    interner: &'src DefaultStringInterner,
    config: FormatConfig,
}

impl<'src> Formatter<'src> {
    pub fn new(
        source: &'src str,
        comments: Vec<Comment>,
        interner: &'src DefaultStringInterner,
        config: FormatConfig,
    ) -> Self {
        Formatter {
            source,
            comments,
            comment_cursor: 0,
            pending_trailing: Vec::new(),
            interner,
            config,
        }
    }

    /// Resolve an AtomId to its string representation.
    fn atom_str(&self, id: AtomId) -> &str {
        use string_interner::Symbol;
        let sym = string_interner::DefaultSymbol::try_from_usize(id.0 as usize).unwrap();
        self.interner.resolve(sym).unwrap_or("???")
    }

    /// Get the original source text for a span.
    fn span_text(&self, span: Span) -> &str {
        &self.source[span.start..span.end]
    }

    /// Returns true if the expression is a multi-line "container" that
    /// benefits from last-argument expansion in function calls.
    fn is_expandable_expr(expr: &Expr) -> bool {
        matches!(
            expr,
            Expr::Fun(..)
                | Expr::Block(..)
                | Expr::Case(..)
                | Expr::If(..)
                | Expr::Receive(..)
                | Expr::Try { .. }
                | Expr::Tuple(..)
                | Expr::Cons(..)
                | Expr::Map(..)
                | Expr::MapUpdate(..)
                | Expr::Binary(..)
                | Expr::ListComp(..)
                | Expr::BinComp(..)
                | Expr::RecordCreate { .. }
                | Expr::Maybe { .. }
        )
    }

    /// Build a hanging-bracket container with comment draining between items.
    /// Each `(span_start, Doc)` pair allows draining comments before each element.
    /// If any item has leading comments, the container is forced to break so
    /// comments appear on their own indented lines.
    fn hanging_container_with_comments(
        &mut self,
        open: &str,
        items: Vec<(usize, Doc)>,
        close: &str,
    ) -> Doc {
        if items.is_empty() {
            return text(format!("{}{}", open, close));
        }
        let indent = self.config.indent_size as i32;
        // Check if any item has leading comments — forces container to break
        let has_inner_comments = items.iter().any(|(s, _)| self.has_comments_before(*s));
        if has_inner_comments {
            // Force break: comments need their own lines
            let mut item_docs = Vec::new();
            for (i, (span_start, doc)) in items.into_iter().enumerate() {
                let comments = self.drain_comments_before(span_start);
                if i > 0 {
                    item_docs.push(text(","));
                    if let Some(tc) = self.take_trailing_comment() {
                        item_docs.push(tc);
                    }
                    item_docs.push(hardline());
                }
                item_docs.push(comments);
                item_docs.push(doc);
            }
            concat_all(vec![
                text(open.to_string()),
                nest(
                    indent,
                    concat(hardline(), concat_all(item_docs)),
                ),
                hardline(),
                text(close.to_string()),
            ])
        } else {
            let mut item_docs = Vec::new();
            for (i, (span_start, doc)) in items.into_iter().enumerate() {
                self.drain_comments_before(span_start);
                if i > 0 {
                    item_docs.push(text(","));
                    item_docs.push(line());
                }
                item_docs.push(doc);
            }
            group(concat_all(vec![
                text(open.to_string()),
                nest(
                    indent,
                    concat(if_break(hardline(), nil()), concat_all(item_docs)),
                ),
                if_break(hardline(), nil()),
                text(close.to_string()),
            ]))
        }
    }

    /// Build a hanging-bracket container: when flat, renders as `OPEN a, b, c CLOSE`;
    /// when broken, renders the open and close on their own lines with each item
    /// indented by `indent_size`, one item per line. Used for tuples, lists, and maps.
    fn hanging_container(&self, open: &str, items: Vec<Doc>, close: &str) -> Doc {
        if items.is_empty() {
            return text(format!("{}{}", open, close));
        }
        let indent = self.config.indent_size as i32;
        group(concat_all(vec![
            text(open.to_string()),
            nest(
                indent,
                concat(
                    if_break(hardline(), nil()),
                    intersperse(items, concat(text(","), line())),
                ),
            ),
            if_break(hardline(), nil()),
            text(close.to_string()),
        ]))
    }

    /// Format an atom, quoting and escaping if necessary.
    fn format_atom(&self, id: AtomId) -> Doc {
        let s = self.atom_str(id);
        if needs_quoting(s) {
            text(format!("'{}'", escape_atom(s)))
        } else {
            text(s.to_string())
        }
    }

    /// Check whether the original source has at least one blank line between
    /// two byte positions.  A blank line is a line containing only whitespace.
    /// Used to preserve the user's vertical spacing (erlfmt-style).
    fn has_blank_line_between(&self, prev_end: usize, next_start: usize) -> bool {
        if next_start <= prev_end {
            return false;
        }
        let between = &self.source[prev_end..next_start];
        // Look for two newlines with only whitespace (spaces/tabs/CR) between them.
        let mut saw_newline = false;
        for b in between.bytes() {
            if b == b'\n' {
                if saw_newline {
                    return true;
                }
                saw_newline = true;
            } else if b == b' ' || b == b'\t' || b == b'\r' {
                // whitespace doesn't reset
            } else {
                saw_newline = false;
            }
        }
        false
    }

    /// Check if there are unconsumed standalone comments before `pos`.
    fn has_comments_before(&self, pos: usize) -> bool {
        let mut i = self.comment_cursor;
        while i < self.comments.len() && self.comments[i].span.start < pos {
            if !self.comments[i].is_trailing {
                return true;
            }
            i += 1;
        }
        false
    }

    /// Drain standalone comments that appear before `pos` and return them as a Doc.
    /// Trailing comments (code on the same line before the comment) are skipped
    /// so that `trailing_comment_at` can attach them inline after `;` or `.`.
    /// Preserves blank lines between consecutive comments and between the
    /// last comment and the code at `pos`.
    fn drain_comments_before(&mut self, pos: usize) -> Doc {
        let mut docs = Vec::new();
        let mut prev_end: Option<usize> = None;
        while self.comment_cursor < self.comments.len()
            && self.comments[self.comment_cursor].span.start < pos
        {
            let comment = &self.comments[self.comment_cursor];
            if comment.is_trailing {
                // Save trailing comments for take_trailing_comment() to
                // attach inline after `;` or `.`. Advance cursor so we
                // don't block subsequent standalone comments.
                self.pending_trailing.push(comment.clone());
                self.comment_cursor += 1;
                continue;
            }
            // Preserve blank line between consecutive comments
            if let Some(pe) = prev_end {
                if self.has_blank_line_between(pe, comment.span.start) {
                    docs.push(hardline());
                }
            }
            prev_end = Some(comment.span.end);
            docs.push(text(comment.text.clone()));
            docs.push(hardline());
            self.comment_cursor += 1;
        }
        // Preserve blank line between last comment and the following code
        if let Some(pe) = prev_end {
            if self.has_blank_line_between(pe, pos) {
                docs.push(hardline());
            }
        }
        concat_all(docs)
    }

    /// Take the oldest pending trailing comment (saved by `drain_comments_before`)
    /// and return it as an inline Doc (` % comment`).
    fn take_trailing_comment(&mut self) -> Option<Doc> {
        if !self.pending_trailing.is_empty() {
            let comment = self.pending_trailing.remove(0);
            Some(concat(text(" "), text(comment.text)))
        } else {
            None
        }
    }

    /// Drain all remaining comments, preserving blank lines between them.
    fn drain_remaining_comments(&mut self) -> Doc {
        let mut docs = Vec::new();
        let mut prev_end: Option<usize> = None;
        while self.comment_cursor < self.comments.len() {
            let comment = &self.comments[self.comment_cursor];
            if let Some(pe) = prev_end {
                if self.has_blank_line_between(pe, comment.span.start) {
                    docs.push(hardline());
                }
            }
            prev_end = Some(comment.span.end);
            docs.push(text(comment.text.clone()));
            docs.push(hardline());
            self.comment_cursor += 1;
        }
        concat_all(docs)
    }

    // ── Export list ──────────────────────────────────────────────────

    fn format_export_list(&mut self, exports: &[FunArity]) -> Doc {
        let entries: Vec<Doc> = exports
            .iter()
            .map(|fa| {
                concat(
                    self.format_atom(fa.name),
                    text(format!("/{}", fa.arity)),
                )
            })
            .collect();

        let body = intersperse(entries, concat(text(","), line()));

        group(concat_all(vec![
            text("-export(["),
            nest(
                self.config.indent_size as i32,
                concat(if_break(line(), nil()), body),
            ),
            if_break(line(), nil()),
            text("])."),
        ]))
    }

    // ── Record definition ───────────────────────────────────────────

    fn format_record_def(&mut self, rec: &RecordDef) -> Doc {
        let fields: Vec<Doc> = rec
            .fields
            .iter()
            .map(|f| {
                let name = self.format_atom(f.name);
                match &f.default {
                    Some(expr) => concat(name, concat(text(" = "), self.format_expr(expr))),
                    None => name,
                }
            })
            .collect();

        let body = intersperse(fields, concat(text(","), line()));

        group(concat_all(vec![
            text("-record("),
            self.format_atom(rec.name),
            text(", {"),
            nest(
                self.config.indent_size as i32,
                concat(if_break(line(), nil()), body),
            ),
            if_break(line(), nil()),
            text("})."),
        ]))
    }

    // ── Function ────────────────────────────────────────────────────

    fn format_function(&mut self, func: &Function) -> Doc {
        let mut clause_docs = Vec::new();
        for clause in &func.clauses {
            let mut parts = Vec::new();

            // Comments before clause
            parts.push(self.drain_comments_before(clause.span.start));

            // function name
            parts.push(self.format_atom(func.name));

            // parameters
            let params: Vec<Doc> = clause
                .patterns
                .iter()
                .map(|p| self.format_pattern(p))
                .collect();
            parts.push(self.hanging_container("(", params, ")"));

            // guard
            if let Some(guard) = &clause.guard {
                parts.push(self.when_guard(guard));
            }

            // arrow and body
            let has_body_comment = !clause.body.is_empty()
                && self.has_comments_before(clause.body[0].span().start);
            let body = self.format_body(&clause.body);
            let can_flatten = Self::body_can_flatten(&clause.body) && !has_body_comment;
            parts.push(self.arrow_body(body, can_flatten));

            // Drain up to clause end to capture any trailing comments into pending_trailing.
            // (Standalone comments here would be unusual but are appended for safety.)
            parts.push(self.drain_comments_before(clause.span.end));

            clause_docs.push(concat_all(parts));
        }

        // Join clauses with semicolons, preserving blank lines between them.
        // Trailing comments go after `;` or `.` (erlfmt-style).
        let mut result_parts = Vec::new();
        for (i, doc) in clause_docs.into_iter().enumerate() {
            if i > 0 {
                result_parts.push(text(";"));
                // Attach trailing comment from previous clause after `;`
                if let Some(tc) = self.take_trailing_comment() {
                    result_parts.push(tc);
                }
                // Preserve blank line between clauses
                let prev_clause = &func.clauses[i - 1];
                let curr_clause = &func.clauses[i];
                if self.has_blank_line_between(prev_clause.span.end, curr_clause.span.start) {
                    result_parts.push(hardline());
                }
                result_parts.push(hardline());
            }
            result_parts.push(doc);
        }
        result_parts.push(text("."));
        // Drain up to end-of-source to capture trailing comment after last clause
        self.drain_comments_before(self.source.len());
        // Attach trailing comment from last clause after `.`
        if let Some(tc) = self.take_trailing_comment() {
            result_parts.push(tc);
        }
        concat_all(result_parts)
    }

    // ── Body (sequence of expressions) ──────────────────────────────

    fn format_body(&mut self, exprs: &[Expr]) -> Doc {
        let mut docs = Vec::new();
        for (i, expr) in exprs.iter().enumerate() {
            // Drain comments between previous expression and this one BEFORE
            // emitting ',' so trailing comments from the preceding line are
            // available for take_trailing_comment() to attach to the comma.
            let pre_comments = self.drain_comments_before(expr.span().start);
            if i > 0 {
                docs.push(text(","));
                // Trailing comment after `,`
                if let Some(tc) = self.take_trailing_comment() {
                    docs.push(tc);
                }
                // Preserve a single blank line between body expressions
                if let Some(prev) = exprs.get(i - 1) {
                    if self.has_blank_line_between(prev.span().end, expr.span().start) {
                        docs.push(hardline());
                    }
                }
                docs.push(hardline());
            }
            docs.push(pre_comments);
            docs.push(self.format_expr(expr));
        }
        concat_all(docs)
    }

    // ── Guard ───────────────────────────────────────────────────────

    fn format_guard(&mut self, guard: &Guard) -> Doc {
        match guard {
            Guard::Test(expr) => self.format_expr(expr),
            Guard::Conjunction(exprs) => {
                let docs: Vec<Doc> = exprs.iter().map(|e| self.format_expr(e)).collect();
                intersperse(docs, text(", "))
            }
            Guard::Disjunction(guards) => {
                let docs: Vec<Doc> = guards.iter().map(|g| self.format_guard(g)).collect();
                intersperse(docs, text("; "))
            }
        }
    }

    // ── Pattern ─────────────────────────────────────────────────────

    fn format_pattern(&mut self, pat: &Pattern) -> Doc {
        match pat {
            Pattern::Wildcard(_) => text("_"),
            Pattern::Variable(id, _) => text(self.atom_str(*id).to_string()),
            Pattern::Integer(_, span) | Pattern::BigInteger(_, span) | Pattern::Float(_, span) => {
                text(self.span_text(*span).to_string())
            }
            Pattern::Atom(id, _) => self.format_atom(*id),
            Pattern::String(s, _) => text(format!("\"{}\"", escape_string(s))),
            Pattern::Tuple(elems, _) => {
                let docs: Vec<Doc> = elems.iter().map(|p| self.format_pattern(p)).collect();
                self.hanging_container("{", docs, "}")
            }
            Pattern::Cons(head, tail, _) => {
                let mut elems = vec![self.format_pattern(head)];
                let mut current = tail.as_ref();
                loop {
                    match current {
                        Pattern::Cons(h, t, _) => {
                            elems.push(self.format_pattern(h));
                            current = t.as_ref();
                        }
                        Pattern::Nil(_) => break,
                        _ => {
                            // Improper list
                            let last = elems.pop().unwrap();
                            elems.push(concat(last, concat(text(" | "), self.format_pattern(current))));
                            break;
                        }
                    }
                }
                self.hanging_container("[", elems, "]")
            }
            Pattern::Nil(_) => text("[]"),
            Pattern::Match(lhs, rhs, _) => {
                concat(
                    self.format_pattern(lhs),
                    concat(text(" = "), self.format_pattern(rhs)),
                )
            }
            Pattern::Binary(segments, _) => self.format_bin_segments_pat(segments),
            Pattern::Map(fields, _) => {
                let docs: Vec<Doc> = fields
                    .iter()
                    .map(|f| {
                        concat(
                            self.format_expr(&f.key),
                            concat(text(" := "), self.format_pattern(&f.value)),
                        )
                    })
                    .collect();
                self.hanging_container("#{", docs, "}")
            }
            Pattern::Record { record_name, fields, .. } => {
                let field_docs: Vec<Doc> = fields
                    .iter()
                    .map(|f| {
                        concat(
                            self.format_atom(f.field_name),
                            concat(text(" = "), self.format_pattern(&f.pattern)),
                        )
                    })
                    .collect();
                group(concat_all(vec![
                    text("#"),
                    self.format_atom(*record_name),
                    text("{"),
                    nest(
                        self.config.indent_size as i32,
                        intersperse(field_docs, concat(text(","), line())),
                    ),
                    text("}"),
                ]))
            }
            Pattern::Concat(lhs, rhs, _) => {
                concat(
                    self.format_pattern(lhs),
                    concat(text(" ++ "), self.format_pattern(rhs)),
                )
            }
            Pattern::MacroCall { name, args, .. } => {
                let name_str = self.atom_str(*name);
                let prefix = format!("?{}", name_str);
                match args {
                    Some(args) if !args.is_empty() => {
                        let open = format!("{}(", prefix);
                        let arg_docs: Vec<Doc> =
                            args.iter().map(|p| self.format_pattern(p)).collect();
                        self.hanging_container(&open, arg_docs, ")")
                    }
                    Some(_) => concat(text(prefix), text("()")),
                    None => text(prefix),
                }
            }
        }
    }

    // ── Expression ──────────────────────────────────────────────────

    fn format_expr(&mut self, expr: &Expr) -> Doc {
        match expr {
            Expr::Integer(_, span) | Expr::BigInteger(_, span) | Expr::Float(_, span) => {
                // Preserve separators, bases, characters and float exponents.
                text(self.span_text(*span).to_string())
            }
            Expr::Atom(id, _) => self.format_atom(*id),
            Expr::String(s, _) => text(format!("\"{}\"", escape_string(s))),
            Expr::Nil(_) => text("[]"),
            Expr::Variable(id, _) => text(self.atom_str(*id).to_string()),
            Expr::Tuple(elems, _) => {
                let items: Vec<(usize, Doc)> = elems
                    .iter()
                    .map(|e| (e.span().start, self.format_expr(e)))
                    .collect();
                self.hanging_container_with_comments("{", items, "}")
            }
            Expr::Cons(head, tail, _) => {
                self.format_list_expr(head, tail)
            }
            Expr::BinOp(op, lhs, rhs, _) => {
                // Flatten chains of the same operator (3+ operands)
                if let Some((chain_op, operands)) = Self::collect_binop_chain(expr) {
                    self.format_binop_chain(chain_op, &operands)
                } else {
                    let op_str = binop_str(*op);
                    let prec = binop_precedence(*op);
                    let lhs_doc = self.format_binop_child(lhs, prec);
                    // Keep a right-nested expression grouped for left-associative
                    // operators; otherwise re-parsing changes the expression tree.
                    let rhs_prec = if *op == BinOp::ListAppend { prec } else { prec + 1 };
                    let rhs_doc = self.format_binop_child(rhs, rhs_prec);
                    group(concat_all(vec![
                        lhs_doc,
                        text(" "),
                        text(op_str.to_string()),
                        nest(
                            self.config.indent_size as i32,
                            concat(line(), rhs_doc),
                        ),
                    ]))
                }
            }
            Expr::UnaryOp(op, operand, _) => {
                let op_str = match op {
                    UnaryOp::Pos => "+",
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "not ",
                    UnaryOp::Bnot => "bnot ",
                };
                let operand_doc = self.format_expr(operand);
                // Preserve weaker-binding operands and avoid turning repeated
                // signs into the `++` or `--` token after grouping is discarded.
                let needs_grouping = matches!(operand.as_ref(),
                    Expr::BinOp(..) | Expr::Match(..) | Expr::Send(..) | Expr::Catch(..))
                    || (*op == UnaryOp::Pos && matches!(operand.as_ref(),
                        Expr::UnaryOp(UnaryOp::Pos, ..)))
                    || (*op == UnaryOp::Neg && match operand.as_ref() {
                        Expr::UnaryOp(UnaryOp::Neg, ..) => true,
                        Expr::Integer(value, _) => *value < 0,
                        Expr::Float(value, _) => value.is_sign_negative(),
                        _ => false,
                    });
                let operand_doc = if needs_grouping {
                    concat_all(vec![text("("), operand_doc, text(")")])
                } else {
                    operand_doc
                };
                concat(text(op_str), operand_doc)
            }
            Expr::Match(pat, expr, _) => {
                group(concat_all(vec![
                    self.format_pattern(pat),
                    text(" ="),
                    nest(
                        self.config.indent_size as i32,
                        concat(line(), self.format_expr(expr)),
                    ),
                ]))
            }
            Expr::Case(scrutinee, clauses, _) => {
                let mut parts = vec![
                    text("case "),
                    self.format_expr(scrutinee),
                    text(" of"),
                ];
                let mut clause_parts = Vec::new();
                for (i, c) in clauses.iter().enumerate() {
                    if i > 0 {
                        clause_parts.push(text(";"));
                        if let Some(tc) = self.take_trailing_comment() {
                            clause_parts.push(tc);
                        }
                        clause_parts.push(hardline());
                    }
                    let comments = self.drain_comments_before(c.span.start);
                    clause_parts.push(comments);
                    clause_parts.push(self.format_case_clause(c));
                }
                parts.push(nest(
                    self.config.indent_size as i32,
                    concat(hardline(), concat_all(clause_parts)),
                ));
                parts.push(hardline());
                parts.push(text("end"));
                concat_all(parts)
            }
            Expr::If(clauses, _) => {
                let mut clause_parts = Vec::new();
                for (i, c) in clauses.iter().enumerate() {
                    if i > 0 {
                        clause_parts.push(text(";"));
                        if let Some(tc) = self.take_trailing_comment() {
                            clause_parts.push(tc);
                        }
                        clause_parts.push(hardline());
                    }
                    let comments = self.drain_comments_before(c.span.start);
                    clause_parts.push(comments);
                    clause_parts.push(self.format_if_clause(c));
                }
                concat_all(vec![
                    text("if"),
                    nest(
                        self.config.indent_size as i32,
                        concat(hardline(), concat_all(clause_parts)),
                    ),
                    hardline(),
                    text("end"),
                ])
            }
            Expr::Receive(clauses, after, _) => {
                let mut parts = vec![text("receive")];
                if !clauses.is_empty() {
                    let mut clause_parts = Vec::new();
                    for (i, c) in clauses.iter().enumerate() {
                        if i > 0 {
                            clause_parts.push(text(";"));
                            if let Some(tc) = self.take_trailing_comment() {
                                clause_parts.push(tc);
                            }
                            clause_parts.push(hardline());
                        }
                        let comments = self.drain_comments_before(c.span.start);
                        clause_parts.push(comments);
                        clause_parts.push(self.format_case_clause(c));
                    }
                    parts.push(nest(
                        self.config.indent_size as i32,
                        concat(hardline(), concat_all(clause_parts)),
                    ));
                }
                if let Some(after_clause) = after {
                    parts.push(hardline());
                    parts.push(text("after"));
                    // Drain comments before the timeout expression
                    let timeout_comments = self.drain_comments_before(after_clause.timeout.span().start);
                    let has_body_comment = !after_clause.body.is_empty()
                        && self.has_comments_before(after_clause.body[0].span().start);
                    let after_body = self.format_body_exprs(&after_clause.body);
                    let can_flatten = Self::body_can_flatten(&after_clause.body) && !has_body_comment;
                    parts.push(nest(
                        self.config.indent_size as i32,
                        concat_all(vec![
                            hardline(),
                            timeout_comments,
                            self.format_expr(&after_clause.timeout),
                            self.arrow_body(after_body, can_flatten),
                        ]),
                    ));
                }
                parts.push(hardline());
                parts.push(text("end"));
                concat_all(parts)
            }
            Expr::Call(target, args, _) => {
                let target_doc = self.format_call_target(target);
                let arg_docs: Vec<Doc> = args.iter().map(|a| self.format_expr(a)).collect();
                let indent = self.config.indent_size as i32;

                let last_expandable = args.len() >= 2
                    && Self::is_expandable_expr(args.last().unwrap());
                let first_expandable = args.len() >= 2
                    && Self::is_expandable_expr(args.first().unwrap());

                if !last_expandable && !first_expandable {
                    // Standard layout: single group with fill_sep
                    group(concat_all(vec![
                        target_doc,
                        text("("),
                        nest(
                            indent,
                            concat(
                                if_break(nil(), nil()),
                                fill_sep(arg_docs, concat(text(","), line())),
                            ),
                        ),
                        text(")"),
                    ]))
                } else {
                    self.format_call_with_expansion(
                        target_doc, args, arg_docs,
                        first_expandable, last_expandable, indent,
                    )
                }
            }
            Expr::Send(dest, msg, _) => {
                concat(
                    self.format_expr(dest),
                    concat(text(" ! "), self.format_expr(msg)),
                )
            }
            Expr::Block(exprs, _) => {
                concat_all(vec![
                    text("begin"),
                    nest(
                        self.config.indent_size as i32,
                        concat(hardline(), self.format_body_exprs(exprs)),
                    ),
                    hardline(),
                    text("end"),
                ])
            }
            Expr::Binary(segments, _) => self.format_bin_segments_expr(segments),
            Expr::Map(fields, _) => {
                let items: Vec<(usize, Doc)> = fields
                    .iter()
                    .map(|f| (f.span.start, self.format_map_field(f)))
                    .collect();
                self.hanging_container_with_comments("#{", items, "}")
            }
            Expr::MapUpdate(base, fields, _) => {
                let items: Vec<(usize, Doc)> = fields
                    .iter()
                    .map(|f| (f.span.start, self.format_map_update_field(f)))
                    .collect();
                concat(self.format_expr(base), self.hanging_container_with_comments("#{", items, "}"))
            }
            Expr::ListComp(body, quals, _) => {
                let has_qual_comments = quals.iter().any(|q| self.has_comments_before(qualifier_span(q).start));
                let brk = if has_qual_comments { hardline() } else { line() };
                let mut qual_parts = Vec::new();
                for (i, q) in quals.iter().enumerate() {
                    let pre_comments = self.drain_comments_before(qualifier_span(q).start);
                    if i > 0 {
                        qual_parts.push(text(","));
                        if let Some(tc) = self.take_trailing_comment() {
                            qual_parts.push(tc);
                        }
                        qual_parts.push(brk.clone());
                    }
                    qual_parts.push(pre_comments);
                    qual_parts.push(self.format_qualifier(q));
                }
                group(concat_all(vec![
                    text("["),
                    nest(
                        self.config.indent_size as i32,
                        concat_all(vec![
                            self.format_expr(body),
                            text(" ||"),
                            brk,
                            concat_all(qual_parts),
                        ]),
                    ),
                    text("]"),
                ]))
            }
            Expr::BinComp(body, quals, _) => {
                let has_qual_comments = quals.iter().any(|q| self.has_comments_before(qualifier_span(q).start));
                let brk = if has_qual_comments { hardline() } else { line() };
                let mut qual_parts = Vec::new();
                for (i, q) in quals.iter().enumerate() {
                    let pre_comments = self.drain_comments_before(qualifier_span(q).start);
                    if i > 0 {
                        qual_parts.push(text(","));
                        if let Some(tc) = self.take_trailing_comment() {
                            qual_parts.push(tc);
                        }
                        qual_parts.push(brk.clone());
                    }
                    qual_parts.push(pre_comments);
                    qual_parts.push(self.format_qualifier(q));
                }
                group(concat_all(vec![
                    text("<< "),
                    nest(
                        self.config.indent_size as i32,
                        concat_all(vec![
                            self.format_expr(body),
                            text(" ||"),
                            brk,
                            concat_all(qual_parts),
                        ]),
                    ),
                    text(" >>"),
                ]))
            }
            Expr::Try { body, of_clauses, catch_clauses, after_body, .. } => {
                let mut parts = vec![text("try")];
                parts.push(nest(
                    self.config.indent_size as i32,
                    concat(hardline(), self.format_body_exprs(body)),
                ));
                if !of_clauses.is_empty() {
                    parts.push(hardline());
                    parts.push(text("of"));
                    let mut clause_parts = Vec::new();
                    for (i, c) in of_clauses.iter().enumerate() {
                        if i > 0 {
                            clause_parts.push(text(";"));
                            if let Some(tc) = self.take_trailing_comment() {
                                clause_parts.push(tc);
                            }
                            clause_parts.push(hardline());
                        }
                        let comments = self.drain_comments_before(c.span.start);
                        clause_parts.push(comments);
                        clause_parts.push(self.format_case_clause(c));
                    }
                    parts.push(nest(
                        self.config.indent_size as i32,
                        concat(hardline(), concat_all(clause_parts)),
                    ));
                }
                if !catch_clauses.is_empty() {
                    parts.push(hardline());
                    parts.push(text("catch"));
                    let mut clause_parts = Vec::new();
                    for (i, c) in catch_clauses.iter().enumerate() {
                        if i > 0 {
                            clause_parts.push(text(";"));
                            if let Some(tc) = self.take_trailing_comment() {
                                clause_parts.push(tc);
                            }
                            clause_parts.push(hardline());
                        }
                        let comments = self.drain_comments_before(c.span.start);
                        clause_parts.push(comments);
                        clause_parts.push(self.format_catch_clause(c));
                    }
                    parts.push(nest(
                        self.config.indent_size as i32,
                        concat(hardline(), concat_all(clause_parts)),
                    ));
                }
                if let Some(after) = after_body {
                    parts.push(hardline());
                    parts.push(text("after"));
                    parts.push(nest(
                        self.config.indent_size as i32,
                        concat(hardline(), self.format_body_exprs(after)),
                    ));
                }
                parts.push(hardline());
                parts.push(text("end"));
                concat_all(parts)
            }
            Expr::Throw(expr, _) => {
                concat(text("throw("), concat(self.format_expr(expr), text(")")))
            }
            Expr::Catch(expr, _) => {
                concat(text("catch "), self.format_expr(expr))
            }
            Expr::Fun(name, clauses, _) => {
                let mut parts = vec![text("fun")];
                let mut clause_parts = Vec::new();
                for (i, c) in clauses.iter().enumerate() {
                    if i > 0 {
                        clause_parts.push(text(";"));
                        if let Some(tc) = self.take_trailing_comment() {
                            clause_parts.push(tc);
                        }
                        clause_parts.push(hardline());
                    } else {
                        // Before the first clause: check for leading comments
                        if self.has_comments_before(c.span.start) {
                            clause_parts.push(hardline());
                        } else {
                            clause_parts.push(text(" "));
                        }
                    }
                    let comments = self.drain_comments_before(c.span.start);
                    clause_parts.push(comments);
                    clause_parts.push(self.format_fun_clause(c, name));
                }
                parts.push(concat_all(clause_parts));
                parts.push(hardline());
                parts.push(text("end"));
                concat_all(parts)
            }
            Expr::FunRef(target, _) => self.format_fun_ref(target),
            Expr::RecordCreate { record_name, fields, .. } => {
                let items: Vec<(usize, Doc)> = fields
                    .iter()
                    .map(|f| {
                        let doc = concat(
                            self.format_atom(f.field_name),
                            concat(text(" = "), self.format_expr(&f.value)),
                        );
                        (f.span.start, doc)
                    })
                    .collect();
                let name_doc = concat(text("#"), self.format_atom(*record_name));
                concat(name_doc, self.hanging_container_with_comments("{", items, "}"))
            }
            Expr::RecordUpdate { base, record_name, fields, .. } => {
                let items: Vec<(usize, Doc)> = fields
                    .iter()
                    .map(|f| {
                        let doc = concat(
                            self.format_atom(f.field_name),
                            concat(text(" = "), self.format_expr(&f.value)),
                        );
                        (f.span.start, doc)
                    })
                    .collect();
                concat_all(vec![
                    self.format_expr(base),
                    text("#"),
                    self.format_atom(*record_name),
                    self.hanging_container_with_comments("{", items, "}"),
                ])
            }
            Expr::RecordAccess { base, record_name, field_name, .. } => {
                concat_all(vec![
                    self.format_expr(base),
                    text("#"),
                    self.format_atom(*record_name),
                    text("."),
                    self.format_atom(*field_name),
                ])
            }
            Expr::RecordIndex { record_name, field_name, .. } => {
                concat_all(vec![
                    text("#"),
                    self.format_atom(*record_name),
                    text("."),
                    self.format_atom(*field_name),
                ])
            }
            Expr::MacroCall { name, args, .. } => {
                let name_str = self.atom_str(*name);
                let prefix = format!("?{}", name_str);
                match args {
                    Some(args) if !args.is_empty() => {
                        let open = format!("{}(", prefix);
                        let arg_docs: Vec<Doc> =
                            args.iter().map(|a| self.format_expr(a)).collect();
                        self.hanging_container(&open, arg_docs, ")")
                    }
                    Some(_) => {
                        // ?MACRO() with empty args
                        concat(text(prefix), text("()"))
                    }
                    None => text(prefix),
                }
            }
            Expr::Maybe { body, else_clauses, .. } => {
                let mut parts = vec![text("maybe")];
                let body_docs: Vec<Doc> = body
                    .iter()
                    .map(|me| match me {
                        MaybeExpr::Expr(e) => self.format_expr(e),
                        MaybeExpr::CondMatch(pat, expr, _) => {
                            concat(
                                self.format_pattern(pat),
                                concat(text(" ?= "), self.format_expr(expr)),
                            )
                        }
                    })
                    .collect();
                parts.push(nest(
                    self.config.indent_size as i32,
                    concat(
                        hardline(),
                        intersperse(body_docs, concat(text(","), hardline())),
                    ),
                ));
                if !else_clauses.is_empty() {
                    parts.push(hardline());
                    parts.push(text("else"));
                    let mut clause_parts = Vec::new();
                    for (i, c) in else_clauses.iter().enumerate() {
                        if i > 0 {
                            clause_parts.push(text(";"));
                            if let Some(tc) = self.take_trailing_comment() {
                                clause_parts.push(tc);
                            }
                            clause_parts.push(hardline());
                        }
                        let comments = self.drain_comments_before(c.span.start);
                        clause_parts.push(comments);
                        clause_parts.push(self.format_case_clause(c));
                    }
                    parts.push(nest(
                        self.config.indent_size as i32,
                        concat(hardline(), concat_all(clause_parts)),
                    ));
                }
                parts.push(hardline());
                parts.push(text("end"));
                concat_all(parts)
            }
        }
    }

    /// Format a child expression of a binary operator, wrapping it in
    /// parentheses when its own precedence is lower than the parent operator.
    /// This prevents `(A =:= B) and (C =:= D)` from being emitted as
    /// `A =:= B and C =:= D` (which re-parses with different grouping).
    fn format_binop_child(&mut self, child: &Expr, parent_prec: u8) -> Doc {
        if let Expr::BinOp(child_op, _, _, _) = child {
            if binop_precedence(*child_op) < parent_prec {
                return concat_all(vec![
                    text("("),
                    self.format_expr(child),
                    text(")"),
                ]);
            }
        }
        self.format_expr(child)
    }

    // ── Helper formatters ───────────────────────────────────────────

    /// Walk the left spine of same-operator `BinOp` nodes, collecting
    /// operands into a flat list.  Returns `None` when the chain has
    /// fewer than 3 operands (i.e. a single `A op B`).
    fn collect_binop_chain(expr: &Expr) -> Option<(BinOp, Vec<&Expr>)> {
        if let Expr::BinOp(op, lhs, _, _) = expr {
            if let Expr::BinOp(inner_op, _, _, _) = lhs.as_ref() {
                if inner_op == op {
                    let mut operands = Vec::new();
                    let mut current: &Expr = expr;
                    while let Expr::BinOp(cur_op, cur_lhs, cur_rhs, _) = current {
                        if cur_op == op {
                            operands.push(cur_rhs.as_ref());
                            current = cur_lhs.as_ref();
                        } else {
                            break;
                        }
                    }
                    operands.push(current);
                    operands.reverse();
                    return Some((*op, operands));
                }
            }
        }
        None
    }

    /// Format a flattened chain of same-operator binary expressions.
    ///
    /// Flat:   `A op B op C op D`
    /// Broken: `A\nop B\nop C\nop D`  (all continuations aligned)
    fn format_binop_chain(&mut self, op: BinOp, operands: &[&Expr]) -> Doc {
        let op_str = binop_str(op);
        let prec = binop_precedence(op);
        let mut parts = vec![self.format_binop_child(operands[0], prec)];
        for operand in &operands[1..] {
            parts.push(line());
            parts.push(text(format!("{} ", op_str)));
            let rhs_prec = if op == BinOp::ListAppend { prec } else { prec + 1 };
            parts.push(self.format_binop_child(operand, rhs_prec));
        }
        group(concat_all(parts))
    }

    fn format_list_expr(&mut self, head: &Expr, tail: &Expr) -> Doc {
        let mut items: Vec<(usize, Doc)> = vec![(head.span().start, self.format_expr(head))];
        let mut current: &Expr = tail;
        loop {
            match current {
                Expr::Cons(h, t, _) => {
                    items.push((h.span().start, self.format_expr(h)));
                    current = t.as_ref();
                }
                Expr::Nil(_) => break,
                _ => {
                    // Improper list: last elem | tail
                    let (_, last) = items.pop().unwrap();
                    let improper = group(concat_all(vec![
                        last,
                        line(),
                        text("| "),
                        self.format_expr(current),
                    ]));
                    items.push((current.span().start, improper));
                    break;
                }
            }
        }
        self.hanging_container_with_comments("[", items, "]")
    }

    /// True for expressions that always render across multiple lines
    /// (their own internal structure forces a break). When such an expression
    /// is the body of a clause, the arrow must move to the next line — sitting
    /// a multi-line construct right after `->` produces ugly output like
    /// `has_item(E, K) -> case get_item(E, K) of` followed by the rest of the
    /// case on subsequent lines.
    fn is_block_expr(expr: &Expr) -> bool {
        matches!(
            expr,
            Expr::Case(..)
                | Expr::If(..)
                | Expr::Receive(..)
                | Expr::Try { .. }
                | Expr::Block(..)
                | Expr::Maybe { .. }
        )
    }

    /// Decide whether a clause body can share its line with `->`. Bodies that
    /// contain a single, non-block expression can flatten when they fit;
    /// everything else breaks after the arrow.
    fn body_can_flatten(body: &[Expr]) -> bool {
        body.len() == 1 && !Self::is_block_expr(&body[0])
    }

    /// Build ` when <guard>` as a breakable group. When the clause head fits
    /// on one line, renders as ` when Guard`. When the head is too long,
    /// breaks the `when` onto its own line indented by `indent_size`:
    ///
    /// ```text
    /// function_name(Args)
    ///     when Guard ->
    /// ```
    ///
    /// This is the standard Erlang layout for long clause heads.
    fn when_guard(&mut self, guard: &Guard) -> Doc {
        group(nest(
            self.config.indent_size as i32,
            concat(line(), concat(text("when "), self.format_guard(guard))),
        ))
    }

    /// Build `" ->"` followed by the clause body, deciding whether the body
    /// can share a line with the arrow. A single-expression body stays on the
    /// arrow's line when it fits (erlfmt style: `ok -> 1`); a multi-expression
    /// body always starts on the next line — flattening a body that contains
    /// hardlines produces inconsistent half-broken output.
    fn arrow_body(&self, body: Doc, single_expr: bool) -> Doc {
        let indent = self.config.indent_size as i32;
        if single_expr {
            concat(
                text(" ->"),
                group(nest(indent, concat(line(), body))),
            )
        } else {
            concat(
                text(" ->"),
                nest(indent, concat(hardline(), body)),
            )
        }
    }

    fn format_body_exprs(&mut self, exprs: &[Expr]) -> Doc {
        let mut docs = Vec::new();
        for (i, expr) in exprs.iter().enumerate() {
            // Drain comments BEFORE emitting ',' — same reasoning as format_body.
            let pre_comments = self.drain_comments_before(expr.span().start);
            if i > 0 {
                docs.push(text(","));
                // Trailing comment after `,`
                if let Some(tc) = self.take_trailing_comment() {
                    docs.push(tc);
                }
                // Preserve a single blank line between body expressions
                if let Some(prev) = exprs.get(i - 1) {
                    if self.has_blank_line_between(prev.span().end, expr.span().start) {
                        docs.push(hardline());
                    }
                }
                docs.push(hardline());
            }
            docs.push(pre_comments);
            docs.push(self.format_expr(expr));
        }
        concat_all(docs)
    }

    /// Build a call doc with argument expansion (first or last expandable arg).
    /// Tries three layouts in priority order:
    ///   1. All flat (only if the expandable arg has no HardLine)
    ///   2. Expandable arg expanded in place, other args flat
    ///   3. Full cascade (one arg per line)
    fn format_call_with_expansion(
        &mut self,
        target_doc: Doc,
        _args: &[Expr],
        arg_docs: Vec<Doc>,
        _first_expandable: bool,
        last_expandable: bool,
        indent: i32,
    ) -> Doc {
        // Decide which arg to expand: prefer last, fall back to first.
        let expand_idx = if last_expandable {
            arg_docs.len() - 1
        } else {
            0
        };

        let expand_doc = &arg_docs[expand_idx];
        let expandable_is_flattenable = can_flatten(expand_doc);

        let mut alternatives = Vec::new();

        // Layout 1: everything flat — only if expandable arg has no HardLine
        if expandable_is_flattenable {
            alternatives.push(group(concat_all(vec![
                target_doc.clone(),
                text("("),
                nest(
                    indent,
                    concat(
                        if_break(nil(), nil()),
                        fill_sep(arg_docs.clone(), concat(text(","), line())),
                    ),
                ),
                text(")"),
            ])));
        }

        // Layout 2: expandable arg expanded in place, rest flat
        {
            let mut parts = vec![target_doc.clone(), text("(")];
            for (i, doc) in arg_docs.iter().enumerate() {
                if i > 0 {
                    parts.push(text(","));
                    parts.push(text(" "));
                }
                if i == expand_idx {
                    parts.push(force_break(doc.clone()));
                } else {
                    parts.push(doc.clone());
                }
            }
            parts.push(text(")"));
            alternatives.push(concat_all(parts));
        }

        // Layout 3: full cascade (one arg per line)
        {
            alternatives.push(concat_all(vec![
                target_doc,
                text("("),
                nest(
                    indent,
                    concat(
                        hardline(),
                        intersperse(arg_docs, concat(text(","), hardline())),
                    ),
                ),
                hardline(),
                text(")"),
            ]));
        }

        best_of(alternatives)
    }

    fn format_call_target(&mut self, target: &CallTarget) -> Doc {
        match target {
            CallTarget::Local(name, _) => self.format_atom(*name),
            CallTarget::Remote(module, func) => {
                concat(
                    self.format_remote_component(module, true),
                    concat(text(":"), self.format_remote_component(func, false)),
                )
            }
            CallTarget::Variable(name, _) => text(self.atom_str(*name).to_string()),
            CallTarget::Expr(callee) => {
                concat(text("("), concat(self.format_expr(callee), text(")")))
            }
        }
    }

    // The AST drops source parentheses. Computed remote targets need them
    // when printed, especially a call in the function position: m:(name())().
    fn format_remote_component(&mut self, expr: &Expr, module_position: bool) -> Doc {
        let doc = self.format_expr(expr);
        match expr {
            Expr::Atom(_, _) | Expr::Variable(_, _) => doc,
            // In the function position, ?F(X) would invoke a parameterized
            // macro instead of applying the object-like ?F as a remote target.
            Expr::MacroCall { args: None, .. } if module_position => doc,
            _ => concat(text("("), concat(doc, text(")"))),
        }
    }

    fn format_case_clause(&mut self, clause: &CaseClause) -> Doc {
        let mut parts = vec![self.format_pattern(&clause.pattern)];
        if let Some(guard) = &clause.guard {
            parts.push(self.when_guard(guard));
        }
        let has_body_comment = !clause.body.is_empty()
            && self.has_comments_before(clause.body[0].span().start);
        let body = self.format_body_exprs(&clause.body);
        let can_flatten = Self::body_can_flatten(&clause.body) && !has_body_comment;
        parts.push(self.arrow_body(body, can_flatten));
        // Drain up to clause end to capture trailing comments into pending_trailing
        parts.push(self.drain_comments_before(clause.span.end));
        concat_all(parts)
    }

    fn format_if_clause(&mut self, clause: &IfClause) -> Doc {
        let mut parts = vec![self.format_guard(&clause.guard)];
        let has_body_comment = !clause.body.is_empty()
            && self.has_comments_before(clause.body[0].span().start);
        let body = self.format_body_exprs(&clause.body);
        let can_flatten = Self::body_can_flatten(&clause.body) && !has_body_comment;
        parts.push(self.arrow_body(body, can_flatten));
        parts.push(self.drain_comments_before(clause.span.end));
        concat_all(parts)
    }

    fn format_catch_clause(&mut self, clause: &CatchClause) -> Doc {
        let mut parts = Vec::new();
        // class:pattern
        let class_doc = match &clause.class {
            CatchClass::Throw => text("throw"),
            CatchClass::Error => text("error"),
            CatchClass::Exit => text("exit"),
            CatchClass::Variable(id, _) => text(self.atom_str(*id).to_string()),
        };
        parts.push(class_doc);
        parts.push(text(":"));
        parts.push(self.format_pattern(&clause.pattern));
        if let Some(st) = &clause.stacktrace {
            parts.push(text(":"));
            parts.push(self.format_pattern(st));
        }
        if let Some(guard) = &clause.guard {
            parts.push(self.when_guard(guard));
        }
        let has_body_comment = !clause.body.is_empty()
            && self.has_comments_before(clause.body[0].span().start);
        let body = self.format_body_exprs(&clause.body);
        let can_flatten = Self::body_can_flatten(&clause.body) && !has_body_comment;
        parts.push(self.arrow_body(body, can_flatten));
        parts.push(self.drain_comments_before(clause.span.end));
        concat_all(parts)
    }

    fn format_fun_clause(&mut self, clause: &FunctionClause, name: &Option<AtomId>) -> Doc {
        let mut parts = Vec::new();
        if let Some(name_id) = name {
            parts.push(self.format_atom(*name_id));
        }
        let params: Vec<Doc> = clause
            .patterns
            .iter()
            .map(|p| self.format_pattern(p))
            .collect();
        parts.push(self.hanging_container("(", params, ")"));
        if let Some(guard) = &clause.guard {
            parts.push(self.when_guard(guard));
        }
        let has_body_comment = !clause.body.is_empty()
            && self.has_comments_before(clause.body[0].span().start);
        let body = self.format_body_exprs(&clause.body);
        let can_flatten = Self::body_can_flatten(&clause.body) && !has_body_comment;
        parts.push(self.arrow_body(body, can_flatten));
        parts.push(self.drain_comments_before(clause.span.end));
        concat_all(parts)
    }

    fn format_fun_ref(&mut self, target: &FunRefTarget) -> Doc {
        match target {
            FunRefTarget::Local(name, arity) => {
                concat_all(vec![
                    text("fun "),
                    self.format_atom(*name),
                    text(format!("/{}", arity)),
                ])
            }
            FunRefTarget::Remote(module, func, arity) => {
                concat_all(vec![
                    text("fun "),
                    self.format_atom(*module),
                    text(":"),
                    self.format_atom(*func),
                    text(format!("/{}", arity)),
                ])
            }
            FunRefTarget::DynamicRemote(mod_expr, fun_expr, arity_expr) => {
                concat_all(vec![
                    text("fun "),
                    self.format_expr(mod_expr),
                    text(":"),
                    self.format_expr(fun_expr),
                    text("/"),
                    self.format_expr(arity_expr),
                ])
            }
        }
    }

    fn format_map_field(&mut self, field: &MapField) -> Doc {
        let op = match field.op {
            MapFieldOp::Assoc => " => ",
            MapFieldOp::Exact => " := ",
        };
        concat(
            self.format_expr(&field.key),
            concat(text(op), self.format_expr(&field.value)),
        )
    }

    fn format_map_update_field(&mut self, field: &MapUpdateField) -> Doc {
        let op = match field.op {
            MapUpdateOp::Assoc => " => ",
            MapUpdateOp::Exact => " := ",
        };
        concat(
            self.format_expr(&field.key),
            concat(text(op), self.format_expr(&field.value)),
        )
    }

    fn format_qualifier(&mut self, qual: &LcQualifier) -> Doc {
        match qual {
            LcQualifier::Generator(pat, expr, _) => {
                concat(
                    self.format_pattern(pat),
                    concat(text(" <- "), self.format_expr(expr)),
                )
            }
            LcQualifier::BinGenerator(pat, expr, _) => {
                concat(
                    self.format_pattern(pat),
                    concat(text(" <= "), self.format_expr(expr)),
                )
            }
            LcQualifier::Filter(expr, _) => self.format_expr(expr),
        }
    }

    // ── Binary segments ─────────────────────────────────────────────

    fn format_bin_segments_expr(&mut self, segments: &[BinSegment<Expr>]) -> Doc {
        let docs: Vec<Doc> = segments.iter().map(|s| self.format_bin_segment_expr(s)).collect();
        group(concat_all(vec![
            text("<<"),
            nest(
                self.config.indent_size as i32,
                concat(
                    if_break(nil(), nil()),
                    intersperse(docs, concat(text(","), line())),
                ),
            ),
            text(">>"),
        ]))
    }

    fn format_bin_segment_expr(&mut self, seg: &BinSegment<Expr>) -> Doc {
        let value_doc = self.format_expr(&seg.value);
        let mut parts = if needs_parens_in_bin_segment(&seg.value) {
            vec![text("("), value_doc, text(")")]
        } else {
            vec![value_doc]
        };
        if let Some(size) = &seg.size {
            parts.push(text(":"));
            let size_doc = self.format_expr(size);
            if self.bin_size_needs_parens(size) {
                parts.push(text("("));
                parts.push(size_doc);
                parts.push(text(")"));
            } else {
                parts.push(size_doc);
            }
        }
        let type_specs = self.format_bin_type_spec(&seg.type_spec);
        if !type_specs.is_empty() {
            parts.push(text("/"));
            parts.push(text(type_specs));
        }
        concat_all(parts)
    }

    fn format_bin_segments_pat(&mut self, segments: &[BinSegment<Pattern>]) -> Doc {
        let docs: Vec<Doc> = segments.iter().map(|s| self.format_bin_segment_pat(s)).collect();
        group(concat_all(vec![
            text("<<"),
            nest(
                self.config.indent_size as i32,
                concat(
                    if_break(nil(), nil()),
                    intersperse(docs, concat(text(","), line())),
                ),
            ),
            text(">>"),
        ]))
    }

    fn format_bin_segment_pat(&mut self, seg: &BinSegment<Pattern>) -> Doc {
        let mut parts = vec![self.format_pattern(&seg.value)];
        if let Some(size) = &seg.size {
            parts.push(text(":"));
            let size_doc = self.format_expr(size);
            if self.bin_size_needs_parens(size) {
                parts.push(text("("));
                parts.push(size_doc);
                parts.push(text(")"));
            } else {
                parts.push(size_doc);
            }
        }
        let type_specs = self.format_bin_type_spec(&seg.type_spec);
        if !type_specs.is_empty() {
            parts.push(text("/"));
            parts.push(text(type_specs));
        }
        concat_all(parts)
    }

    fn bin_size_needs_parens(&self, size: &Expr) -> bool {
        // The parser folds negative numbers into literals, but their leading
        // minus still needs grouping in a size, including spellings like -0.
        needs_parens_in_bin_segment(size) || self.span_text(size.span()).starts_with('-')
    }

    fn format_bin_type_spec(&mut self, spec: &BinSegmentType) -> String {
        // A written list is source, not just effective flags. Reconstructing
        // it can erase a semantic conflict or change an alias's spelling.
        if let Some(span) = spec.specifiers_span {
            // Comments inside this raw span are already in its text; the
            // ordinary comment drain must not emit them a second time.
            let outside = |c: &Comment| c.span.start < span.start || c.span.start >= span.end;
            let consumed = self.comments[..self.comment_cursor].iter().filter(|c| !outside(c)).count();
            self.comments.retain(outside);
            self.comment_cursor -= consumed;
            self.pending_trailing.retain(outside);
            return self.span_text(span).to_string();
        }
        String::new()
    }
}

// ── Utility functions ───────────────────────────────────────────────

/// Get the span of a list comprehension qualifier.
fn qualifier_span(q: &LcQualifier) -> Span {
    match q {
        LcQualifier::Generator(_, _, span) => *span,
        LcQualifier::BinGenerator(_, _, span) => *span,
        LcQualifier::Filter(_, span) => *span,
    }
}

/// Check if an atom needs quoting.
fn needs_quoting(s: &str) -> bool {
    nera_syntax::token::atom_needs_quoting(s)
}

/// Escape a string for output.
fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out
}

/// Escape a quoted atom for output (single-quote delimited).
fn escape_atom(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            _ => out.push(c),
        }
    }
    out
}

/// Returns the precedence level of a binary operator (higher = tighter binding).
/// Matches the Pratt parser binding powers in parser.rs.
fn binop_precedence(op: BinOp) -> u8 {
    match op {
        BinOp::Orelse => 2,
        BinOp::Andalso => 4,
        BinOp::Eq | BinOp::NotEq | BinOp::ExactEq | BinOp::ExactNotEq
        | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => 6,
        BinOp::ListAppend | BinOp::ListSubtract => 8,
        BinOp::Add | BinOp::Sub | BinOp::Bor | BinOp::Bxor
        | BinOp::Bsl | BinOp::Bsr | BinOp::Or | BinOp::Xor => 10,
        BinOp::Mul | BinOp::Div | BinOp::IntDiv | BinOp::Rem
        | BinOp::Band | BinOp::And => 12,
    }
}

/// Returns true if an expression requires parenthesization when used as a
/// value or size in a binary segment (`<<(Expr):Size/Type>>`).
/// Only primary expressions (literals, variables, strings) are allowed bare.
fn needs_parens_in_bin_segment(expr: &Expr) -> bool {
    !matches!(
        expr,
        Expr::Integer(_, _)
            | Expr::BigInteger(_, _)
            | Expr::Float(_, _)
            | Expr::Atom(_, _)
            | Expr::String(_, _)
            | Expr::Nil(_)
            | Expr::Variable(_, _)
    )
}

fn binop_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::IntDiv => "div",
        BinOp::Rem => "rem",
        BinOp::Eq => "==",
        BinOp::ExactEq => "=:=",
        BinOp::NotEq => "/=",
        BinOp::ExactNotEq => "=/=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::LtEq => "=<",
        BinOp::GtEq => ">=",
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::Xor => "xor",
        BinOp::Andalso => "andalso",
        BinOp::Orelse => "orelse",
        BinOp::Band => "band",
        BinOp::Bor => "bor",
        BinOp::Bxor => "bxor",
        BinOp::Bsl => "bsl",
        BinOp::Bsr => "bsr",
        BinOp::ListAppend => "++",
        BinOp::ListSubtract => "--",
    }
}

// ── Form-based source formatter ─────────────────────────────────────

/// Split source into top-level forms and format them.
/// This operates on raw source, preserving preprocessor directives verbatim.
pub fn format_source_forms(
    source: &str,
    config: &FormatConfig,
) -> Result<String, String> {
    let forms = split_into_forms(source);
    let mut output = String::new();
    let mut prev_was_blank = false;

    for form in &forms {
        let trimmed = form.text.trim();
        if trimmed.is_empty() {
            if !prev_was_blank && !output.is_empty() {
                output.push('\n');
                prev_was_blank = true;
            }
            continue;
        }

        // If the raw form text has a leading blank line (at least two newlines
        // before any non-whitespace), preserve one blank line in the output.
        if !output.is_empty() && !prev_was_blank && has_leading_blank_line(&form.text) {
            output.push('\n');
        }
        prev_was_blank = false;

        // Extract trailing comment from the form text (after the `.` on the same line).
        // We'll re-attach it after formatting so formatters don't need to handle it.
        let (form_text, trailing_comment) = split_trailing_comment(trimmed);

        if is_define_directive(form_text) {
            match try_format_define_form(form_text, config) {
                Some(formatted) => {
                    push_formatted(&mut output, &formatted, trailing_comment);
                }
                None => {
                    output.push_str(trimmed);
                    output.push('\n');
                }
            }
        } else if is_simple_preprocessor_directive(form_text) {
            let normalized = normalize_simple_directive(form_text);
            push_formatted(&mut output, &normalized, trailing_comment);
        } else if is_export_attribute(form_text) {
            match try_format_export_form(form_text, config) {
                Some(formatted) => {
                    push_formatted(&mut output, &formatted, trailing_comment);
                }
                None => {
                    output.push_str(trimmed);
                    output.push('\n');
                }
            }
        } else if is_record_attribute(form_text) {
            match try_format_record_form(form_text, config) {
                Some(formatted) => {
                    push_formatted(&mut output, &formatted, trailing_comment);
                }
                None => {
                    output.push_str(trimmed);
                    output.push('\n');
                }
            }
        } else if crate::spec_format::is_spec_or_type_attribute(form_text) {
            // Preserve leading comments before the spec/type attribute
            let leading_comments = extract_leading_comments(form_text);
            match crate::spec_format::try_format_spec_form(form_text, config) {
                Some(formatted) => {
                    output.push_str(&leading_comments);
                    push_formatted(&mut output, &formatted, trailing_comment);
                }
                None => {
                    output.push_str(trimmed);
                    output.push('\n');
                }
            }
        } else if is_attribute_only(form_text) {
            // Preserve other attributes verbatim (with trailing comment)
            output.push_str(trimmed);
            output.push('\n');
        } else if looks_like_function(form_text) {
            // Try to parse and format as a function
            match try_format_function_form(form_text, config) {
                Some(formatted) => {
                    push_formatted(&mut output, &formatted, trailing_comment);
                }
                None => {
                    // Fallback: preserve verbatim
                    output.push_str(trimmed);
                    output.push('\n');
                }
            }
        } else {
            output.push_str(trimmed);
            output.push('\n');
        }
    }

    // Ensure trailing newline
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }

    Ok(output)
}

/// A raw form extracted from source.
struct RawForm {
    text: String,
}

/// Split source into top-level forms by scanning for `.` followed by
/// whitespace/EOF at statement level (not inside strings, brackets, etc.).
fn split_into_forms(source: &str) -> Vec<RawForm> {
    let mut forms = Vec::new();
    let mut current = String::new();
    let mut chars = source.char_indices().peekable();
    let mut depth: i32 = 0; // bracket/paren depth
    let mut in_string = false;
    let mut in_quoted_atom = false;
    let mut in_comment = false;
    let mut escape_next = false;

    while let Some((_, ch)) = chars.next() {
        current.push(ch);

        if escape_next {
            escape_next = false;
            continue;
        }

        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            continue;
        }

        if in_string {
            match ch {
                '\\' => escape_next = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        if in_quoted_atom {
            match ch {
                '\\' => escape_next = true,
                '\'' => in_quoted_atom = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '%' => in_comment = true,
            '"' => in_string = true,
            '\'' => in_quoted_atom = true,
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            '.' if depth <= 0 => {
                // Check if followed by whitespace, comment, or EOF
                let next = chars.peek().map(|(_, c)| *c);
                if next.is_none() || next == Some('\n') || next == Some('\r')
                    || next == Some(' ') || next == Some('\t') || next == Some('%')
                {
                    // Also consume any trailing comment on the same line
                    // (e.g., `f() -> ok. % comment\n`)
                    if next == Some(' ') || next == Some('\t') || next == Some('%') {
                        // Peek ahead to see if there's a comment on this line
                        let mut saw_comment = next == Some('%');
                        if !saw_comment {
                            // Consume whitespace to check for comment
                            let mut temp = Vec::new();
                            while let Some(&(_, c)) = chars.peek() {
                                if c == ' ' || c == '\t' {
                                    temp.push(chars.next().unwrap());
                                } else if c == '%' {
                                    saw_comment = true;
                                    break;
                                } else {
                                    break;
                                }
                            }
                            // Add consumed whitespace to current
                            for (_, c) in temp {
                                current.push(c);
                            }
                        }
                        if saw_comment {
                            // Consume the rest of the line (the comment)
                            while let Some(&(_, c)) = chars.peek() {
                                if c == '\n' {
                                    break;
                                }
                                current.push(chars.next().unwrap().1);
                            }
                        }
                    }
                    forms.push(RawForm { text: std::mem::take(&mut current) });
                }
            }
            _ => {}
        }
    }

    // Leftover (comments, blank lines after last form)
    let leftover = current.trim().to_string();
    if !leftover.is_empty() {
        forms.push(RawForm { text: current });
    }

    forms
}

/// Check if the raw form text starts with a blank line (two newlines with only
/// whitespace between them before any non-whitespace content).  This means the
/// original source had an empty line separating this form from the previous one.
fn has_leading_blank_line(text: &str) -> bool {
    let mut saw_newline = false;
    for b in text.bytes() {
        if b == b'\n' {
            if saw_newline {
                return true;
            }
            saw_newline = true;
        } else if b == b' ' || b == b'\t' || b == b'\r' {
            // whitespace doesn't reset
        } else {
            return false;
        }
    }
    false
}

/// Check if a form is a `-define(...)` directive.
fn is_define_directive(s: &str) -> bool {
    let stripped = strip_leading_comments(s);
    stripped.starts_with("-define(")
}

/// Check if a form is a simple preprocessor directive (not `-define`).
fn is_simple_preprocessor_directive(s: &str) -> bool {
    let stripped = strip_leading_comments(s);
    stripped.starts_with("-ifdef(")
        || stripped.starts_with("-ifndef(")
        || stripped.starts_with("-else.")
        || stripped.starts_with("-endif.")
        || stripped.starts_with("-undef(")
        || stripped.starts_with("-include(")
        || stripped.starts_with("-include_lib(")
}

/// Normalize whitespace in simple preprocessor directives.
///
/// For directives like `-ifdef( DEBUG ).` → `-ifdef(DEBUG).`
fn normalize_simple_directive(s: &str) -> String {
    let leading_comments = extract_leading_comments(s);
    let stripped = strip_leading_comments(s);

    // Find the directive keyword and its argument
    if stripped.starts_with("-else.") || stripped.starts_with("-endif.") {
        // No arguments to normalize
        return format!("{}{}", leading_comments, stripped.trim());
    }

    // Pattern: -keyword(content).
    if let Some(open) = stripped.find('(') {
        // Find matching close paren (last one before the dot)
        if let Some(close) = stripped.rfind(')') {
            let keyword = &stripped[..open];
            let inner = stripped[open + 1..close].trim();
            return format!("{}{}({}).", leading_comments, keyword, inner);
        }
    }

    // Fallback: return as-is
    format!("{}{}", leading_comments, stripped)
}

/// Try to format a `-define(...)` directive.
///
/// Parses the define to extract the macro header and body, then formats
/// the body expression using the standard expression formatter.
/// Returns `None` if parsing fails (caller preserves verbatim).
fn try_format_define_form(source: &str, config: &FormatConfig) -> Option<String> {
    let leading_comments = extract_leading_comments(source);
    let stripped = strip_leading_comments(source);

    // Parse the define structure: -define(Name, Body). or -define(Name(Args), Body).
    // or -define(Name). (no body)
    let after_define = stripped.strip_prefix("-define(")?;

    // Find the macro header (name + optional params) and optional body.
    // We need to find the first top-level comma that separates header from body.
    let (header, body_opt) = split_define_header_body(after_define)?;

    let header = header.trim();

    match body_opt {
        None => {
            // No body: -define(NAME).
            Some(format!("{}-define({}).\n", leading_comments, header))
        }
        Some(body_text) => {
            let body_text = body_text.trim();

            // Try to parse the body as an expression by wrapping it
            // Use a valid lowercase function name so the parser recognizes it
            let wrapper = format!("x() -> {}.", body_text);
            let formatted_body = try_format_define_body(&wrapper, config);

            let body_str = match &formatted_body {
                Some(s) => s.as_str(),
                None => body_text,
            };

            // Try single-line first
            let single_line = format!("-define({}, {}).", header, body_str);
            if !body_str.contains('\n') && single_line.len() <= config.max_width {
                Some(format!("{}{}\n", leading_comments, single_line))
            } else {
                // Multi-line: indent body 4 spaces from the define
                let indented_body = indent_body(body_str, config.indent_size);
                Some(format!(
                    "{}-define({},\n{}\n).\n",
                    leading_comments, header, indented_body
                ))
            }
        }
    }
}

/// Split the define content after `-define(` into header and optional body.
///
/// Returns `(header, Some(body))` or `(header, None)` for defines without body.
/// The header includes the macro name and optional parameter list.
/// Returns `None` if the structure cannot be parsed.
fn split_define_header_body(s: &str) -> Option<(&str, Option<&str>)> {
    // We need to find the first comma at depth 0 (not inside parens/brackets/braces)
    // or the closing `)` followed by `.` if there's no body.
    let mut depth: i32 = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    // Track whether we're inside the macro's parameter list
    let mut in_string = false;
    let mut escape_next = false;

    while i < bytes.len() {
        if escape_next {
            escape_next = false;
            i += 1;
            continue;
        }

        match bytes[i] {
            b'\\' if in_string => {
                escape_next = true;
            }
            b'"' => {
                in_string = !in_string;
            }
            _ if in_string => {}
            b'(' | b'[' | b'{' => {
                depth += 1;
            }
            b')' | b']' | b'}' => {
                if depth == 0 {
                    // This is the closing paren of -define(...)
                    let header = &s[..i];
                    return Some((header.trim(), None));
                }
                depth -= 1;
            }
            b',' if depth == 0 => {
                // First top-level comma separates header from body
                let header = &s[..i];
                // Body is everything after the comma up to the final `).`
                let rest = &s[i + 1..];
                // Find the final `)` (last occurrence)
                if let Some(close) = rest.rfind(')') {
                    let body = &rest[..close];
                    return Some((header.trim(), Some(body.trim())));
                }
                return None;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Try to parse and format a define body expression.
///
/// Wraps the body in a fake function `_() -> Body.` and extracts the
/// formatted expression from the result.
fn try_format_define_body(wrapper_source: &str, config: &FormatConfig) -> Option<String> {
    use nera_syntax::lexer::Lexer;
    use nera_syntax::parser::Parser;

    let lex_result = Lexer::lex_all_with_comments(wrapper_source).ok()?;
    let mut parser = Parser::new(wrapper_source);
    let result = parser.parse_module();

    if result.errors.is_empty() && !result.module.functions.is_empty() {
        let func = &result.module.functions[0];
        if !func.clauses.is_empty() {
            let clause = &func.clauses[0];
            let interner = parser.into_interner();
            let comments = crate::comment::extract_comments(lex_result.comments, wrapper_source);
            let mut formatter =
                Formatter::new(wrapper_source, comments, &interner, config.clone());
            // Format body expressions, comma-separated
            let body_doc = if clause.body.len() == 1 {
                formatter.format_expr(&clause.body[0])
            } else {
                formatter.format_body(&clause.body)
            };
            let rendered = render(config.max_width, &body_doc);
            return Some(rendered);
        }
    }
    None
}

/// Indent a multi-line body string by the given number of spaces.
fn indent_body(body: &str, indent: usize) -> String {
    let prefix: String = " ".repeat(indent);
    body.lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("{}{}", prefix, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Check if a form is an export attribute that should be formatted.
fn is_export_attribute(s: &str) -> bool {
    let stripped = strip_leading_comments(s);
    stripped.starts_with("-export(")
}

/// Check if a form is a record definition that should be formatted.
fn is_record_attribute(s: &str) -> bool {
    let stripped = strip_leading_comments(s);
    stripped.starts_with("-record(")
}

/// Try to parse and format an export attribute form.
fn try_format_export_form(source: &str, config: &FormatConfig) -> Option<String> {
    use nera_syntax::lexer::Lexer;
    use nera_syntax::parser::Parser;

    let lex_result = Lexer::lex_all_with_comments(source).ok()?;
    let mut parser = Parser::new(source);
    let result = parser.parse_module();

    if !result.module.exports.is_empty() {
        let interner = parser.into_interner();
        let comments = crate::comment::extract_comments(lex_result.comments, source);
        let mut formatter = Formatter::new(source, comments, &interner, config.clone());
        // Drain leading comments before the export attribute
        let leading = formatter.drain_comments_before(lex_result.tokens[0].1.start);
        let doc = formatter.format_export_list(&result.module.exports);
        let full_doc = concat(leading, doc);
        let mut output = render(config.max_width, &full_doc);
        // Safety net: emit any remaining comments not yet consumed
        let remaining = formatter.drain_remaining_comments();
        let rem_text = render(config.max_width, &remaining);
        if !rem_text.is_empty() {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&rem_text);
        }
        Some(output)
    } else {
        None
    }
}

/// Try to parse and format a record definition form.
fn try_format_record_form(source: &str, config: &FormatConfig) -> Option<String> {
    use nera_syntax::lexer::Lexer;
    use nera_syntax::parser::Parser;

    let lex_result = Lexer::lex_all_with_comments(source).ok()?;
    let mut parser = Parser::new(source);
    let result = parser.parse_module();

    if !result.module.records.is_empty() {
        let interner = parser.into_interner();
        let comments = crate::comment::extract_comments(lex_result.comments, source);
        let mut formatter = Formatter::new(source, comments, &interner, config.clone());
        // Drain leading comments before the record attribute
        let leading = formatter.drain_comments_before(lex_result.tokens[0].1.start);
        let doc = formatter.format_record_def(&result.module.records[0]);
        let full_doc = concat(leading, doc);
        let mut output = render(config.max_width, &full_doc);
        // Safety net: emit any remaining comments not yet consumed
        let remaining = formatter.drain_remaining_comments();
        let rem_text = render(config.max_width, &remaining);
        if !rem_text.is_empty() {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&rem_text);
        }
        Some(output)
    } else {
        None
    }
}

/// Check if a form is a simple attribute (not a function).
fn is_attribute_only(s: &str) -> bool {
    let stripped = strip_leading_comments(s);
    if !stripped.starts_with('-') {
        return false;
    }
    // It's an attribute if it starts with -atom( and is not a negative number
    if let Some(rest) = stripped.strip_prefix('-') {
        let first_char = rest.chars().next().unwrap_or(' ');
        first_char.is_ascii_lowercase()
    } else {
        false
    }
}

/// Check if a form looks like a function definition.
fn looks_like_function(s: &str) -> bool {
    let stripped = strip_leading_comments(s);
    if stripped.is_empty() {
        return false;
    }
    let first_char = stripped.chars().next().unwrap_or(' ');
    // Functions start with lowercase atom or quoted atom
    first_char.is_ascii_lowercase() || first_char == '\''
}

/// Append formatted text to output, re-attaching an optional trailing comment
/// inline after the last `.` (before the final newline).
fn push_formatted(output: &mut String, formatted: &str, trailing_comment: Option<&str>) {
    let text = formatted.trim_end_matches('\n');
    output.push_str(text);
    if let Some(comment) = trailing_comment {
        output.push(' ');
        output.push_str(comment);
    }
    output.push('\n');
}

/// Extract a trailing comment from the end of a form text.
/// A trailing comment is a `% ...` after the terminating `.` on the same line.
/// Returns `(form_without_trailing, Option<trailing_comment_text>)`.
fn split_trailing_comment(s: &str) -> (&str, Option<&str>) {
    // Find the last `.` that terminates the form
    // The trailing comment would be after it on the same line
    if let Some(dot_pos) = s.rfind('.') {
        let after_dot = &s[dot_pos + 1..];
        // Check if there's a comment on the same line after the dot
        if let Some(pct_offset) = after_dot.find('%') {
            let between = &after_dot[..pct_offset];
            // Only if no newline between dot and comment
            if !between.contains('\n') {
                let comment_start = dot_pos + 1 + pct_offset;
                let comment = s[comment_start..].trim_end();
                return (&s[..dot_pos + 1], Some(comment));
            }
        }
    }
    (s, None)
}

/// Extract leading comment lines (including their trailing newlines) from a
/// form.  Returns the comment text as a string ready to emit.
fn extract_leading_comments(s: &str) -> String {
    let mut out = String::new();
    for line in s.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('%') {
            out.push_str(trimmed);
            out.push('\n');
        } else if trimmed.is_empty() {
            // Preserve blank lines between leading comment blocks
            if !out.is_empty() {
                out.push('\n');
            }
        } else {
            break;
        }
    }
    out
}

/// Strip leading comment lines.
fn strip_leading_comments(s: &str) -> &str {
    let mut start = 0;
    for line in s.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('%') || trimmed.is_empty() {
            start += line.len() + 1; // +1 for newline
        } else {
            break;
        }
    }
    if start >= s.len() { s } else { &s[start..] }
}

/// Try to parse and format a function form.
fn try_format_function_form(source: &str, config: &FormatConfig) -> Option<String> {
    use nera_syntax::lexer::Lexer;
    use nera_syntax::parser::Parser;

    let lex_result = Lexer::lex_all_with_comments(source).ok()?;
    let mut parser = Parser::new(source);
    let result = parser.parse_module();

    if !result.module.functions.is_empty() {
        let interner = parser.into_interner();
        let comments = crate::comment::extract_comments(lex_result.comments, source);
        let mut formatter = Formatter::new(source, comments, &interner, config.clone());
        let mut output = String::new();
        for (i, func) in result.module.functions.iter().enumerate() {
            if i > 0 {
                output.push('\n');
            }
            let doc = formatter.format_function(func);
            output.push_str(&render(config.max_width, &doc));
        }
        // Safety net: emit any remaining comments not yet consumed
        let remaining = formatter.drain_remaining_comments();
        let rem_text = render(config.max_width, &remaining);
        if !rem_text.is_empty() {
            output.push_str(&rem_text);
        }
        Some(output)
    } else {
        None
    }
}

/// Format a standalone Erlang term file (`.app` / `.app.src`).
///
/// Parses the source as a single term, formats it using the expression
/// formatter, and appends a trailing dot.
pub fn format_term_file(
    source: &str,
    config: &FormatConfig,
) -> Result<String, String> {
    use nera_syntax::lexer::Lexer;
    use nera_syntax::parser::Parser;

    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Ok(String::from("\n"));
    }

    let lex_result = Lexer::lex_all_with_comments(source)
        .map_err(|e| format!("lex error: {:?}", e))?;
    let mut parser = Parser::new(source);
    let expr = parser
        .parse_term()
        .map_err(|e| format!("parse error: {:?}", e))?;

    let interner = parser.into_interner();
    let comments = crate::comment::extract_comments(lex_result.comments, source);
    let mut formatter = Formatter::new(source, comments, &interner, config.clone());

    // Drain comments that appear before the term
    let leading = formatter.drain_comments_before(expr.span().start);
    let expr_doc = formatter.format_expr(&expr);

    let doc = concat(leading, expr_doc);
    let mut output = render(config.max_width, &doc);

    // Append trailing dot
    output.push_str(".\n");

    // Append any remaining comments (after the term)
    let remaining = formatter.drain_remaining_comments();
    let rem_text = render(config.max_width, &remaining);
    if !rem_text.is_empty() {
        output.push_str(&rem_text);
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format_source(source: &str) -> String {
        let config = FormatConfig::default();
        format_source_forms(source, &config).unwrap()
    }

    fn format_source_width(source: &str, width: usize) -> String {
        let config = FormatConfig { max_width: width, indent_size: 4 };
        format_source_forms(source, &config).unwrap()
    }

    #[test]
    fn format_simple_module() {
        let src = "-module(test).\n-export([hello/0]).\nhello() -> world.\n";
        let result = format_source(src);
        assert!(result.contains("-module(test)."));
        assert!(result.contains("-export(["));
        assert!(result.contains("hello() ->"));
        assert!(result.contains("world."));
    }

    #[test]
    fn preserve_numeric_lexemes_in_patterns_and_expressions() {
        // NERA-21 F4: formatting must retain cow_hpack-style digit separators,
        // including the sibling big-integer, float-pattern and exponent paths.
        let source = "-module(test).\n-export([pick/1]).\npick(10_000) -> 1_000.5;\npick(16#FF_FF) -> 16#1_0000_0000_0000_0000;\npick(-10_000) -> -1_0.0e+1_0;\npick(1_000.5) -> 1_0.0e-1_0;\npick(16#1_0000_0000_0000_0000) -> $a.\n";
        let formatted = format_source(source);
        assert_eq!(formatted, source);
        assert_eq!(format_source(&formatted), formatted);
    }

    #[test]
    fn preserve_written_binary_specifiers() {
        // Captain's NERA-129 decision: formatting must preserve conflicts so
        // compilation still diagnoses them, and must not rewrite aliases.
        for spec in [
            "binary-utf8", "signed-unsigned-utf8", "bytes-bits",
            "bits", "bitstring", "bytes", "binary-bits", "bits-binary",
            "bits-unit:1", "bytes-unit:8", "binary-unit:1",
            "binary - utf8", "'binary'-'utf8'", "binary- % keep this\nutf8",
            "binary % note\n-utf8",
        ] {
            let source = format!("-module(test).\nf(<<B/{spec}>>) -> <<B/{spec}>>.\n");
            let formatted = format_source(&source);
            assert_eq!(formatted, source, "{spec}");
            assert_eq!(format_source(&formatted), formatted, "{spec}");
        }
    }

    #[test]
    fn format_case_expression() {
        let src = "-module(test).\n-export([f/1]).\nf(X) -> case X of 1 -> one; 2 -> two end.\n";
        let result = format_source(src);
        assert!(result.contains("case X of"));
        assert!(result.contains("end"));
    }

    #[test]
    fn format_pattern_match() {
        let src = "-module(test).\n-export([f/1]).\nf({ok, V}) -> V.\n";
        let result = format_source(src);
        assert!(result.contains("{ok, V}"));
    }

    #[test]
    fn format_binary_op() {
        let src = "-module(test).\n-export([add/2]).\nadd(A, B) -> A + B.\n";
        let result = format_source(src);
        assert!(result.contains("A + B"));
    }

    #[test]
    fn format_record_create() {
        let src = "-module(test).\n-export([f/0]).\n-record(person, {name, age}).\nf() -> #person{name = \"Alice\", age = 30}.\n";
        let result = format_source(src);
        assert!(result.contains("#person{"));
    }

    #[test]
    fn format_list_comprehension() {
        let src = "-module(test).\n-export([f/1]).\nf(L) -> [X * 2 || X <- L].\n";
        let result = format_source(src);
        assert!(result.contains("||"));
        assert!(result.contains("<-"));
    }

    #[test]
    fn format_try_catch() {
        let src = "-module(test).\n-export([f/0]).\nf() -> try 1 + 2 catch error:E -> E end.\n";
        let result = format_source(src);
        assert!(result.contains("try"));
        assert!(result.contains("catch"));
        assert!(result.contains("end"));
    }

    #[test]
    fn format_fun_ref() {
        let src = "-module(test).\n-export([f/0]).\nf() -> fun g/1.\ng(X) -> X.\n";
        let result = format_source(src);
        assert!(result.contains("fun g/1"));
    }

    #[test]
    fn atom_quoting() {
        assert!(!needs_quoting("hello"));
        assert!(needs_quoting("case"));
        assert!(needs_quoting("maybe"));
        assert!(!needs_quoting("module"));
        assert!(!needs_quoting("foo_bar"));
        assert!(needs_quoting("Hello"));
        assert!(needs_quoting("foo bar"));
        assert!(needs_quoting(""));
    }

    #[test]
    fn split_forms_basic() {
        let src = "-module(test).\nhello() -> ok.\n";
        let forms = split_into_forms(src);
        assert_eq!(forms.len(), 2);
    }

    // ── Term formatting tests ──────────────────────────────────────

    fn format_term(source: &str) -> String {
        let config = FormatConfig::default();
        format_term_file(source, &config).unwrap()
    }

    #[test]
    fn format_term_simple_tuple() {
        let result = format_term("{application, myapp, []}.");
        assert_eq!(result, "{application, myapp, []}.\n");
    }

    #[test]
    fn format_term_atom() {
        let result = format_term("hello.");
        assert_eq!(result, "hello.\n");
    }

    #[test]
    fn format_term_nested() {
        let src = "{application, myapp, [{vsn, \"1.0\"}, {modules, [mod1, mod2]}]}.";
        let result = format_term(src);
        assert!(result.starts_with("{application"));
        assert!(result.ends_with(".\n"));
        assert!(result.contains("vsn"));
        assert!(result.contains("mod1"));
    }

    #[test]
    fn format_term_idempotent() {
        let src = "{application, myapp, [{vsn, \"1.0\"}, {modules, [mod1, mod2]}]}.";
        let first = format_term(src);
        let second = format_term(&first);
        assert_eq!(first, second);
    }

    #[test]
    fn format_term_empty_source() {
        let result = format_term("");
        assert_eq!(result, "\n");
    }

    #[test]
    fn format_term_without_trailing_dot() {
        let result = format_term("{ok, value}");
        assert_eq!(result, "{ok, value}.\n");
    }

    #[test]
    fn format_term_realistic_app() {
        let src = r#"{application, myapp, [{description, "My Application"}, {vsn, "1.0.0"}, {modules, [myapp_app, myapp_sup, myapp_server]}, {registered, [myapp_sup]}, {applications, [kernel, stdlib]}, {mod, {myapp_app, []}}]}."#;
        let result = format_term(src);
        // Should break across lines due to width
        assert!(result.contains('\n'));
        assert!(result.ends_with(".\n"));
        // Idempotent
        let second = format_term(&result);
        assert_eq!(result, second);
    }

    #[test]
    fn format_term_with_leading_comment() {
        let src = "% Application resource file\n{application, myapp, []}.\n";
        let result = format_term(src);
        assert!(result.contains("% Application resource file"));
        assert!(result.contains("{application, myapp, []}"));
        assert!(result.ends_with(".\n"));
    }

    #[test]
    fn format_term_with_trailing_comment() {
        let src = "{application, myapp, []}.\n% end of file\n";
        let result = format_term(src);
        assert!(result.contains("{application, myapp, []}"));
        assert!(result.contains("% end of file"));
    }

    #[test]
    fn format_term_whitespace_only() {
        let result = format_term("   \n  \n  ");
        assert_eq!(result, "\n");
    }

    #[test]
    fn format_term_integer() {
        let result = format_term("42.");
        assert_eq!(result, "42.\n");
    }

    #[test]
    fn format_term_string() {
        let result = format_term(r#""hello world"."#);
        assert_eq!(result, "\"hello world\".\n");
    }

    #[test]
    fn format_term_nested_list_of_tuples() {
        let src = "[{a, 1}, {b, 2}, {c, 3}].";
        let result = format_term(src);
        assert!(result.contains("{a, 1}"));
        assert!(result.ends_with(".\n"));
        // Idempotent
        let second = format_term(&result);
        assert_eq!(result, second);
    }

    #[test]
    fn format_term_negative_integer() {
        let result = format_term("{port, -1}.");
        assert!(result.contains("-1"));
        assert!(result.ends_with(".\n"));
    }

    // ── Hanging-bracket container tests ───────────────────────────
    //
    // Containers (tuples, lists, maps) should stay flat on one line when they
    // fit within max_width, and when they don't fit they should break with
    // the open bracket on its own line, each element on its own line indented
    // by indent_size, and the close bracket back at the outer indent.
    // This matches erlfmt / prettier "consistent break" behaviour.

    #[test]
    fn hanging_tuple_flat_when_it_fits() {
        let result = format_term("{a, 1, ok}.");
        assert_eq!(result, "{a, 1, ok}.\n");
    }

    #[test]
    fn hanging_list_flat_when_it_fits() {
        let result = format_term("[a, b, c].");
        assert_eq!(result, "[a, b, c].\n");
    }

    #[test]
    fn hanging_list_breaks_one_per_line_when_too_wide() {
        // Each tuple is long enough that the whole list can't fit on one 100-col line.
        let src = "[{aaaaaaaaaaaaaaa, 1}, {bbbbbbbbbbbbbbb, 2}, {ccccccccccccccc, 3}, \
                   {ddddddddddddddd, 4}, {eeeeeeeeeeeeeee, 5}, {fffffffffffffff, 6}].";
        let result = format_term(src);
        // Opening bracket on its own line
        assert!(
            result.starts_with("[\n"),
            "list should hang: open bracket on its own line, got:\n{}",
            result
        );
        // Closing bracket on its own line at outer indent (column 0 for a term)
        assert!(
            result.contains("\n]."),
            "closing bracket should be on its own line at outer indent, got:\n{}",
            result
        );
        // Items indented by indent_size (4), one per line — count commas-at-end-of-line
        let trailing_commas = result.lines().filter(|l| l.trim_end().ends_with(',')).count();
        assert_eq!(
            trailing_commas, 5,
            "expected 5 comma-separated breaks for 6 items, got:\n{}",
            result
        );
        // Each item line starts with 4 spaces of indent
        let indented = result.lines().filter(|l| l.starts_with("    {")).count();
        assert_eq!(
            indented, 6,
            "expected 6 items indented by 4 spaces, got:\n{}",
            result
        );
        // Idempotent
        let second = format_term(&result);
        assert_eq!(result, second);
    }

    #[test]
    fn hanging_tuple_breaks_one_per_line_when_too_wide() {
        let src = "{aaaaaaaaaaaaaaaaaaaaa, bbbbbbbbbbbbbbbbbbbbb, ccccccccccccccccccccc, \
                   ddddddddddddddddddddd, eeeeeeeeeeeeeeeeeeeee, fffffffffffffffffffff}.";
        let result = format_term(src);
        assert!(result.starts_with("{\n"), "got:\n{}", result);
        assert!(result.contains("\n}."), "got:\n{}", result);
        let indented = result.lines().filter(|l| l.starts_with("    ")).count();
        assert_eq!(indented, 6, "got:\n{}", result);
        let second = format_term(&result);
        assert_eq!(result, second);
    }

    #[test]
    fn hanging_list_matches_erlfmt_supervisor_shape() {
        // Child-spec list longer than 100 cols — classic supervisor init shape.
        // Each child spec itself is >96 chars so at depth-4 indent it also breaks,
        // cascading through the nested tuples. This matches how erlfmt and prettier
        // behave: once a group breaks, inner groups decide independently.
        let src = "[{onecached_expiry, {onecached_expiry, start_link, []}, permanent, 5000, \
                   worker, [onecached_expiry]}, {onecached_listener, {onecached_listener, \
                   start_link, []}, permanent, brutal_kill, worker, [onecached_listener]}].";
        let result = format_term(src);
        // Open bracket on its own line
        assert!(result.starts_with("[\n"), "got:\n{}", result);
        // Close bracket on its own line
        assert!(result.contains("\n]."), "got:\n{}", result);
        // Each of the two child specs opens its tuple on one line then breaks.
        // Their opening "{" should appear at column 4 (indent inside the list).
        let child_open_lines: Vec<_> = result
            .lines()
            .filter(|l| l.trim_start() == "{")
            .collect();
        assert_eq!(
            child_open_lines.len(),
            2,
            "expected two child-spec tuples opening at indent 4, got:\n{}",
            result
        );
        for line in &child_open_lines {
            assert!(
                line.starts_with("    {"),
                "child-tuple open should be at indent 4, got: {:?}",
                line
            );
        }
        // The two atom tags appear one level deeper (indent 8) on their own line.
        assert!(
            result.contains("\n        onecached_expiry,\n"),
            "expected 'onecached_expiry,' at indent 8, got:\n{}",
            result
        );
        assert!(
            result.contains("\n        onecached_listener,\n"),
            "expected 'onecached_listener,' at indent 8, got:\n{}",
            result
        );
        // Idempotent
        let second = format_term(&result);
        assert_eq!(result, second);
    }

    #[test]
    fn hanging_empty_containers() {
        assert_eq!(format_term("{}."), "{}.\n");
        assert_eq!(format_term("[]."), "[].\n");
    }

    #[test]
    fn hanging_single_item_container() {
        assert_eq!(format_term("{a}."), "{a}.\n");
        assert_eq!(format_term("[a]."), "[a].\n");
    }

    #[test]
    fn hanging_list_inside_tuple_cascades() {
        // When an inner container forces the outer container to exceed width in
        // flat form, the outer also breaks — this is correct Wadler behaviour
        // (groups are consistent, not fill). Argument expansion (BestOf) is
        // used for Call expressions; tuples still use standard group behaviour.
        let src = "{ok, [aaaaaaaaaaaaaaaa, bbbbbbbbbbbbbbbb, cccccccccccccccc, dddddddddddddddd, \
                   eeeeeeeeeeeeeeee, ffffffffffffffff, gggggggggggggggg]}.";
        let result = format_term(src);
        // Outer tuple breaks
        assert!(result.starts_with("{\n"), "got:\n{}", result);
        assert!(result.contains("\n}."), "got:\n{}", result);
        // Inner list also breaks, at deeper indent
        assert!(
            result.contains("\n    [\n"),
            "inner list should hang at indent 4, got:\n{}",
            result
        );
        assert!(
            result.contains("\n    ]\n"),
            "inner list close should be at indent 4, got:\n{}",
            result
        );
        // Atoms are at indent 8 (4 for outer tuple + 4 for inner list)
        assert!(
            result.contains("\n        aaaaaaaaaaaaaaaa,\n"),
            "got:\n{}",
            result
        );
        let second = format_term(&result);
        assert_eq!(result, second);
    }

    #[test]
    fn hanging_small_tuple_with_small_list_stays_flat() {
        // When everything fits, nothing breaks.
        let result = format_term("{ok, [a, b, c]}.");
        assert_eq!(result, "{ok, [a, b, c]}.\n");
    }

    // ── Clause head-to-body flattening ────────────────────────────
    //
    // erlfmt rule: a clause's "Pat -> Body" stays on one line if the body
    // is a single expression AND it fits under max_width. Multi-expression
    // bodies always break after the arrow (otherwise you get half-flat,
    // half-broken output with comma-hardline between exprs).

    #[test]
    fn short_case_clauses_flatten_single_expr() {
        let src = "-module(t).\n-export([f/1]).\nf(X) -> case X of ok -> 1; error -> 2 end.\n";
        let result = format_source(src);
        assert!(
            result.contains("        ok -> 1;"),
            "expected 'ok -> 1;' on one line, got:\n{}",
            result
        );
        assert!(
            result.contains("        error -> 2"),
            "expected 'error -> 2' on one line, got:\n{}",
            result
        );
    }

    #[test]
    fn multi_expr_case_body_breaks_after_arrow() {
        let src = "-module(t).\n-export([f/1]).\n\
                   f(X) -> case X of ok -> a = 1, b = 2, a + b; error -> 0 end.\n";
        let result = format_source(src);
        // Multi-expr body: arrow should be followed by a newline, not a space+expr
        assert!(
            !result.contains("ok -> a = 1"),
            "multi-expr body should NOT sit on the arrow line, got:\n{}",
            result
        );
        assert!(
            result.contains("ok ->\n"),
            "expected newline after '->' for multi-expr body, got:\n{}",
            result
        );
        // Single-expr clause (error) still flattens
        assert!(
            result.contains("error -> 0"),
            "single-expr clause 'error -> 0' should flatten, got:\n{}",
            result
        );
    }

    #[test]
    fn long_single_expr_body_breaks_after_arrow() {
        // Body is one expression but too long for the arrow line.
        let src = "-module(t).\n-export([f/1]).\n\
                   f(X) -> case X of ok -> some_really_long_function_name_here(argument_one, \
                   argument_two, argument_three, argument_four, argument_five); error -> 0 end.\n";
        let result = format_source(src);
        // Arrow followed by newline because body doesn't fit on arrow line
        assert!(
            result.contains("ok ->\n"),
            "expected arrow to break when single-expr body doesn't fit, got:\n{}",
            result
        );
    }

    #[test]
    fn if_clauses_flatten_single_expr() {
        let src = "-module(t).\n-export([f/1]).\nf(X) -> if X > 0 -> pos; X < 0 -> neg; true -> zero end.\n";
        let result = format_source(src);
        assert!(
            result.contains("X > 0 -> pos;"),
            "expected 'X > 0 -> pos;' on one line, got:\n{}",
            result
        );
        assert!(
            result.contains("true -> zero"),
            "expected 'true -> zero' on one line, got:\n{}",
            result
        );
    }

    #[test]
    fn try_catch_clauses_flatten_single_expr() {
        let src = "-module(t).\n-export([f/0]).\nf() -> try risky() catch \
                   throw:E -> E; error:E -> {error, E} end.\n";
        let result = format_source(src);
        assert!(
            result.contains("throw:E -> E;"),
            "expected 'throw:E -> E;' on one line, got:\n{}",
            result
        );
        assert!(
            result.contains("error:E -> {error, E}"),
            "expected catch body to flatten, got:\n{}",
            result
        );
    }

    #[test]
    fn function_clauses_flatten_single_expr() {
        // Function clauses should follow the same rule: short single-expr bodies
        // stay on the same line as the arrow.
        let src = "-module(t).\n-export([size/1]).\nsize([]) -> 0; size([_|T]) -> 1 + size(T).\n";
        let result = format_source(src);
        assert!(
            result.contains("size([]) -> 0;"),
            "expected 'size([]) -> 0;' on one line, got:\n{}",
            result
        );
        assert!(
            result.contains("size([_ | T]) -> 1 + size(T)"),
            "expected second clause to flatten, got:\n{}",
            result
        );
    }

    #[test]
    fn clauses_stay_one_per_line_even_when_short() {
        // Even trivially short clauses never share a line with another clause.
        let src = "-module(t).\n-export([f/1]).\nf(X) -> case X of a -> 1; b -> 2 end.\n";
        let result = format_source(src);
        assert!(
            !result.contains("a -> 1; b -> 2"),
            "clauses must not share a line, got:\n{}",
            result
        );
        // Each clause on its own line
        assert!(
            result.lines().filter(|l| l.contains(" -> ")).count() >= 2,
            "expected 2 clause lines, got:\n{}",
            result
        );
    }

    #[test]
    fn when_guard_breaks_when_head_too_wide() {
        // Long function head + guard should break the `when` onto its own line.
        let src = "-module(t).\n-export([store_item/2]).\n\
                   store_item(ets, #storage_command{key = Key, flags = Flags, exptime = Exptime, data = Data}) when Exptime =:= 0; Exptime > 60 * 60 * 24 * 30 -> Key.\n";
        let result = format_source(src);
        // `when` should NOT be on the same line as `})` since the head is too wide.
        assert!(
            !result.contains("}) when"),
            "when should break onto its own line, got:\n{}",
            result
        );
        // `when` should appear at start of a line, indented by 4.
        assert!(
            result.contains("\n    when "),
            "expected 'when' at indent 4 on its own line, got:\n{}",
            result
        );
    }

    #[test]
    fn when_guard_stays_inline_when_head_fits() {
        // Short function head + guard: stays on one line.
        let src = "-module(t).\n-export([f/1]).\nf(X) when X > 0 -> X.\n";
        let result = format_source(src);
        assert!(
            result.contains("f(X) when X > 0 -> X"),
            "short head+guard should stay flat, got:\n{}",
            result
        );
    }

    #[test]
    fn when_guard_case_clause_flat_when_fits() {
        let src = "-module(t).\n-export([f/1]).\n\
                   f(X) -> case X of Y when Y > 0 -> pos; _ -> other end.\n";
        let result = format_source(src);
        assert!(
            result.contains("Y when Y > 0 -> pos;"),
            "short case guard should stay flat, got:\n{}",
            result
        );
    }

    #[test]
    fn case_clauses_indent_under_case_keyword() {
        // Regression: case clauses should be indented deeper than the `case` keyword.
        let src = "-module(t).\n-export([f/1]).\n\
                   f(Name) -> case catch ets:lookup(stats, Name) of [{_, V}] when is_integer(V) -> V; _ -> 0 end.\n";
        let result = format_source(src);
        // `case` sits at indent 4 (inside function body).
        assert!(result.contains("    case "), "got:\n{}", result);
        // Each case clause should sit at indent 8 (4 deeper than `case`).
        assert!(
            result.contains("\n        [{_, V}] when"),
            "expected clause at indent 8, got:\n{}",
            result
        );
        assert!(
            result.contains("\n        _ ->"),
            "expected second clause at indent 8, got:\n{}",
            result
        );
        // `end` back at indent 4.
        assert!(
            result.contains("\n    end"),
            "expected 'end' at indent 4, got:\n{}",
            result
        );
    }

    #[test]
    fn block_expr_body_never_flattens_on_arrow() {
        // A single-expression body that IS a block construct (case, if,
        // receive, try, begin, maybe) must break the arrow onto the next
        // line — pasting a multi-line construct after `->` looks wrong.
        let src = "-module(t).\n-export([has_item/2]).\n\
                   has_item(E, K) -> case get_item(E, K) of {ok, _} -> true; _ -> false end.\n";
        let result = format_source(src);
        assert!(
            !result.contains("-> case "),
            "case-body should not sit on the arrow line, got:\n{}",
            result
        );
        assert!(
            result.contains("has_item(E, K) ->\n"),
            "function arrow should break before case-body, got:\n{}",
            result
        );
        // The nested case clauses themselves DO flatten (they have single-expr bodies
        // that are not blocks).
        assert!(
            result.contains("{ok, _} -> true;"),
            "inner case clause should still flatten, got:\n{}",
            result
        );
    }

    #[test]
    fn block_body_variants_all_break_arrow() {
        // Same rule for if, receive, try, begin.
        for (name, body) in [
            ("f_if", "if X > 0 -> pos; true -> nonpos end"),
            ("f_try", "try risky() catch _:_ -> failed end"),
            ("f_begin", "begin a = 1, b = 2, a + b end"),
        ] {
            let src = format!(
                "-module(t).\n-export([{n}/1]).\n{n}(X) -> {b}.\n",
                n = name,
                b = body
            );
            let result = format_source(&src);
            let arrow_line = format!("{}(X) ->\n", name);
            assert!(
                result.contains(&arrow_line),
                "{}-body should break the arrow, got:\n{}",
                name,
                result
            );
        }
    }

    #[test]
    fn clause_flattening_is_idempotent() {
        let src = "-module(t).\n-export([f/1]).\nf(X) -> case X of ok -> 1; error -> 2 end.\n";
        let first = format_source(src);
        let second = format_source(&first);
        assert_eq!(first, second, "clause formatting not idempotent:\n{}", first);
    }

    // ── Blank line preservation tests ─────────────────────────────

    #[test]
    fn preserves_blank_line_before_export() {
        let src = "-module(test).\n\n-export([f/0]).\nf() -> ok.\n";
        let result = format_source(src);
        assert!(
            result.contains("-module(test).\n\n-export("),
            "blank line before -export should be preserved: {:?}",
            result
        );
    }

    #[test]
    fn no_blank_line_before_export_when_absent() {
        let src = "-module(test).\n-export([f/0]).\nf() -> ok.\n";
        let result = format_source(src);
        assert!(
            result.contains("-module(test).\n-export("),
            "should not insert blank line when absent: {:?}",
            result
        );
    }

    #[test]
    fn preserves_blank_line_between_functions() {
        let src = "-module(test).\n-export([f/0, g/0]).\nf() -> ok.\n\ng() -> ok.\n";
        let result = format_source(src);
        assert!(
            result.contains("ok.\n\ng() ->"),
            "blank line between functions should be preserved: {:?}",
            result
        );
    }

    #[test]
    fn no_blank_line_between_functions_when_absent() {
        let src = "-module(test).\n-export([f/0, g/0]).\nf() -> ok.\ng() -> ok.\n";
        let result = format_source(src);
        assert!(
            result.contains("ok.\ng() ->"),
            "should not insert blank line between functions when absent: {:?}",
            result
        );
    }

    #[test]
    fn collapses_multiple_blank_lines() {
        let src = "-module(test).\n\n\n\n-export([f/0]).\nf() -> ok.\n";
        let result = format_source(src);
        // Should have exactly one blank line, not multiple
        assert!(
            result.contains("-module(test).\n\n-export("),
            "multiple blank lines should collapse to one: {:?}",
            result
        );
        assert!(
            !result.contains("-module(test).\n\n\n"),
            "should not have more than one blank line: {:?}",
            result
        );
    }

    #[test]
    fn preserves_blank_line_between_comments() {
        let src = "-module(test).\n-export([init/0]).\n\n%%===\n%% API\n%%===\n\n%% Init.\n\ninit() -> ok.\n";
        let result = format_source(src);
        // Blank line between section header and function comment
        assert!(
            result.contains("%%===\n\n%% Init."),
            "blank line between section comments and function comment should be preserved:\n{}",
            result
        );
        // Blank line between function comment and function
        assert!(
            result.contains("%% Init.\n\ninit()"),
            "blank line between comment and function should be preserved:\n{}",
            result
        );
    }

    #[test]
    fn no_blank_line_between_adjacent_comments() {
        // Comments without a blank line between them should stay adjacent
        let src = "-module(test).\n-export([f/0]).\n%% First.\n%% Second.\nf() -> ok.\n";
        let result = format_source(src);
        assert!(
            result.contains("%% First.\n%% Second."),
            "adjacent comments should not get a blank line inserted:\n{}",
            result
        );
    }

    #[test]
    fn preserves_blank_line_comment_before_function_no_section() {
        // Single comment separated from function by a blank line
        let src = "-module(test).\n-export([f/0]).\n\n%% Do something.\n\nf() -> ok.\n";
        let result = format_source(src);
        assert!(
            result.contains("%% Do something.\n\nf()"),
            "blank line between comment and function should be preserved:\n{}",
            result
        );
    }

    #[test]
    fn preserves_blank_line_between_comment_blocks_idempotent() {
        // Full section header + function comment + function, verify idempotence
        let src = "-module(test).\n-export([f/0]).\n\n%%===\n%% API\n%%===\n\n%% F doc.\n\nf() -> ok.\n";
        let first = format_source(src);
        let second = format_source(&first);
        assert_eq!(first, second, "comment blank line preservation is not idempotent:\n{}", first);
    }

    #[test]
    fn no_blank_line_when_comment_directly_above_function() {
        // Comment directly above function (no blank line) should stay that way
        let src = "-module(test).\n-export([f/0]).\n%% Doc.\nf() -> ok.\n";
        let result = format_source(src);
        assert!(
            result.contains("%% Doc.\nf()"),
            "should not insert blank line when absent:\n{}",
            result
        );
    }

    #[test]
    fn preserves_blank_line_before_behaviour() {
        let src =
            "-module(test).\n\n-behaviour(gen_server).\n-export([f/0]).\nf() -> ok.\n";
        let result = format_source(src);
        assert!(
            result.contains("-module(test).\n\n-behaviour("),
            "blank line before -behaviour should be preserved: {:?}",
            result
        );
    }

    // ── Argument expansion ──────────────────────────────────────────────

    #[test]
    fn first_arg_fun_expands_inline() {
        // Fun is the first arg in lists:map — it expands in place,
        // with the remaining args following on the `end` line.
        let src = "-module(test).\n-export([f/0]).\n\
                   f() -> lists:map(fun(X) -> X end, L).\n";
        let result = format_source(src);
        // Fun always has a hardline before `end`, so it expands
        assert!(
            result.contains("lists:map(fun (X) -> X"),
            "fun should expand on opening line:\n{}",
            result
        );
        assert!(
            result.contains("end, L)"),
            "trailing arg should follow end:\n{}",
            result
        );
    }

    #[test]
    fn first_arg_fun_expands_at_narrow_width() {
        // At width 40, the fun in foldl expands and trailing args follow `end`
        let src = "-module(test).\n-export([f/1]).\n\
                   f(List) -> lists:foldl(fun(X, Acc) -> Acc + X end, 0, List).\n";
        let result = format_source_width(src, 40);
        assert!(
            result.contains("lists:foldl(fun (X, Acc) ->"),
            "fun should start on opening line:\n{}",
            result
        );
        assert!(
            result.contains("end, 0, List)"),
            "trailing args should follow end:\n{}",
            result
        );
    }

    #[test]
    fn last_arg_list_expands() {
        // The list is the last argument and should expand while the first arg stays flat
        let src = "-module(test).\n-export([f/0]).\n\
                   f() -> foo(bar, [aaaa, bbbb, cccc, dddd, eeee, ffff, gggg, hhhh]).\n";
        let result = format_source_width(src, 50);
        // Layout 2: prefix arg flat, list expands
        assert!(
            result.contains("foo(bar, ["),
            "prefix arg should stay flat with list on same line:\n{}",
            result
        );
    }

    #[test]
    fn last_arg_single_arg_no_expansion() {
        // Only one arg — should NOT use expansion (needs 2+).
        // With a list that exceeds width, the standard cascade is used.
        let src = "-module(test).\n-export([f/0]).\n\
                   f() -> foo([aaaa, bbbb, cccc, dddd, eeee, ffff]).\n";
        let result = format_source_width(src, 30);
        // Single-arg call: standard group with fill_sep, should cascade
        assert!(
            result.contains("foo(\n") || result.contains("foo(["),
            "single-arg call should use standard layout:\n{}",
            result
        );
    }

    #[test]
    fn last_arg_non_expandable_no_expansion() {
        // Last arg is a variable, not expandable — standard layout
        let src = "-module(test).\n-export([f/0]).\n\
                   f() -> some_long_function_name(some_long_arg_a, some_long_arg_b, some_long_arg_c).\n";
        let result = format_source_width(src, 50);
        // Should not contain "best_of" text (sanity check)
        assert!(
            !result.contains("best_of"),
            "non-expandable args should use standard layout:\n{}",
            result
        );
    }

    #[test]
    fn first_arg_expansion_idempotent() {
        let src = "-module(test).\n-export([f/1]).\n\
                   f(List) -> lists:foldl(fun(X, Acc) -> Acc + X end, 0, List).\n";
        let first = format_source_width(src, 40);
        let second = format_source_width(&first, 40);
        assert_eq!(first, second, "expansion should be idempotent:\n{}", first);
    }

    #[test]
    fn last_arg_cascade_when_prefix_too_wide() {
        // Even Layout 2 doesn't fit (prefix args too wide), so cascade to Layout 3
        let src = "-module(test).\n-export([f/0]).\n\
                   f() -> foo(very_long_argument_name_a, very_long_argument_name_b, [a, b, c]).\n";
        let result = format_source_width(src, 40);
        // Should cascade all args
        assert!(
            result.contains("foo(\n"),
            "should cascade when prefix too wide:\n{}",
            result
        );
    }

    #[test]
    fn last_arg_case_expands() {
        let src = "-module(test).\n-export([f/1]).\n\
                   f(X) -> handle(X, case X of ok -> 1; error -> 2 end).\n";
        let result = format_source_width(src, 45);
        assert!(
            result.contains("handle(X, case X of"),
            "case as last arg should expand on same line:\n{}",
            result
        );
    }

    // ── Preprocessor directive formatting ──────────────────────────────

    #[test]
    fn define_no_body() {
        let src = "-module(test).\n-define(DEBUG).\n";
        let result = format_source(src);
        assert!(
            result.contains("-define(DEBUG)."),
            "define without body:\n{}",
            result
        );
    }

    #[test]
    fn define_simple_constant() {
        let src = "-module(test).\n-define(TIMEOUT,  5000).\n";
        let result = format_source(src);
        assert!(
            result.contains("-define(TIMEOUT, 5000)."),
            "simple constant define:\n{}",
            result
        );
    }

    #[test]
    fn define_string_body() {
        let src = "-module(test).\n-define(APP_NAME,   \"myapp\").\n";
        let result = format_source(src);
        assert!(
            result.contains("-define(APP_NAME, \"myapp\")."),
            "string define:\n{}",
            result
        );
    }

    #[test]
    fn define_parameterized_simple() {
        let src = "-module(test).\n-define(ADD( X,  Y ),  X + Y).\n";
        let result = format_source(src);
        assert!(
            result.contains("-define(ADD( X,  Y ), X + Y)."),
            "parameterized define:\n{}",
            result
        );
    }

    #[test]
    fn define_tuple_body() {
        let src = "-module(test).\n-define(PAIR(A, B),  {A,  B}).\n";
        let result = format_source(src);
        assert!(
            result.contains("-define(PAIR(A, B), {A, B})."),
            "tuple body define:\n{}",
            result
        );
    }

    #[test]
    fn define_case_body_breaks() {
        let src = "-module(test).\n-define(CHOOSE(X), case X of true -> yes; false -> no end).\n";
        let result = format_source_width(src, 40);
        assert!(
            result.contains("-define(CHOOSE(X),\n"),
            "complex define body should break:\n{}",
            result
        );
        // Should be idempotent
        let second = format_source_width(&result, 40);
        assert_eq!(result, second, "define formatting not idempotent:\n{}", result);
    }

    #[test]
    fn define_idempotent() {
        let src = "-module(test).\n-define(TIMEOUT, 5000).\n-define(LOG(X), io:format(\"~p~n\", [X])).\n";
        let first = format_source(src);
        let second = format_source(&first);
        assert_eq!(first, second, "define formatting not idempotent");
    }

    #[test]
    fn normalize_ifdef_whitespace() {
        let src = "-module(test).\n-ifdef(  DEBUG  ).\n-endif.\n";
        let result = format_source(src);
        assert!(
            result.contains("-ifdef(DEBUG)."),
            "ifdef whitespace not normalized:\n{}",
            result
        );
    }

    #[test]
    fn normalize_include_whitespace() {
        let src = "-module(test).\n-include(  \"header.hrl\"  ).\n";
        let result = format_source(src);
        assert!(
            result.contains("-include(\"header.hrl\")."),
            "include whitespace not normalized:\n{}",
            result
        );
    }

    #[test]
    fn normalize_include_lib() {
        let src = "-module(test).\n-include_lib(  \"kernel/include/file.hrl\"  ).\n";
        let result = format_source(src);
        assert!(
            result.contains("-include_lib(\"kernel/include/file.hrl\")."),
            "include_lib not normalized:\n{}",
            result
        );
    }

    #[test]
    fn normalize_undef_whitespace() {
        let src = "-module(test).\n-undef(  MACRO  ).\n";
        let result = format_source(src);
        assert!(
            result.contains("-undef(MACRO)."),
            "undef whitespace not normalized:\n{}",
            result
        );
    }

    #[test]
    fn preprocessor_trailing_comment() {
        let src = "-module(test).\n-define(TIMEOUT, 5000). % default timeout\n";
        let result = format_source(src);
        assert!(
            result.contains("-define(TIMEOUT, 5000). % default timeout"),
            "trailing comment on define:\n{}",
            result
        );
    }

    #[test]
    fn preprocessor_leading_comment() {
        let src = "-module(test).\n%% Timeout in ms\n-define(TIMEOUT, 5000).\n";
        let result = format_source(src);
        assert!(
            result.contains("%% Timeout in ms\n-define(TIMEOUT, 5000)."),
            "leading comment on define:\n{}",
            result
        );
    }

    // ── Macro call formatting tests ────────────────────────────────

    #[test]
    fn macro_call_with_args_formats_correctly() {
        let src = "-module(t).\n-export([f/1]).\n\
                   f(X) -> ?ERROR_MSG(\"bad ~p\", [X]), error.\n";
        let result = format_source(src);
        assert!(
            result.contains("?ERROR_MSG(\"bad ~p\", [X])"),
            "macro call should be preserved, got:\n{}",
            result
        );
        // Verify function is actually formatted (indented)
        assert!(
            result.contains("    ?ERROR_MSG"),
            "macro call should be indented in function body, got:\n{}",
            result
        );
    }

    #[test]
    fn object_like_macro_formats_correctly() {
        let src = "-module(t).\n-export([f/0]).\n\
                   f() -> {ok, ?MODULE}.\n";
        let result = format_source(src);
        assert!(
            result.contains("?MODULE"),
            "object-like macro should be preserved, got:\n{}",
            result
        );
    }

    #[test]
    fn case_catch_with_macro_in_clause() {
        // Regression: case catch with ?MACRO in a clause body must format
        // with correct clause indentation (deeper than `case`).
        let src = "-module(t).\n-export([f/2]).\n\
                   f(Storage, Key) -> \
                   case catch storage:get(Storage, Key) of \
                   {ok, V} -> V; \
                   Other -> ?ERROR_MSG(\"error ~p\", [Other]), error \
                   end.\n";
        let result = format_source(src);
        // `case` sits at indent 4 (inside function body).
        assert!(
            result.contains("    case catch"),
            "case should be at indent 4, got:\n{}",
            result
        );
        // Each clause should be at indent 8 (4 deeper than `case`).
        assert!(
            result.contains("\n        {ok, V}"),
            "first clause should be at indent 8, got:\n{}",
            result
        );
        assert!(
            result.contains("\n        Other ->"),
            "second clause should be at indent 8, got:\n{}",
            result
        );
        // `end` back at indent 4.
        assert!(
            result.contains("\n    end"),
            "end should be at indent 4, got:\n{}",
            result
        );
        // ?ERROR_MSG preserved
        assert!(
            result.contains("?ERROR_MSG("),
            "macro call should be preserved, got:\n{}",
            result
        );
    }

    #[test]
    fn macro_in_pattern_position() {
        let src = "-module(t).\n-export([f/1]).\n\
                   f(X) -> case X of ?OK -> good; _ -> bad end.\n";
        let result = format_source(src);
        assert!(
            result.contains("?OK ->"),
            "macro in pattern should be preserved, got:\n{}",
            result
        );
    }

    #[test]
    fn macro_formatting_is_idempotent() {
        let src = "-module(t).\n-export([f/2]).\n\
                   f(Socket, X) -> \
                   case catch foo(X) of \
                   {ok, V} -> V; \
                   Other -> ?LOG(\"err ~p\", [Other]), send(Socket, error), error \
                   end.\n";
        let first = format_source(src);
        let second = format_source(&first);
        assert_eq!(
            first, second,
            "formatting with macros should be idempotent.\nFirst:\n{}\nSecond:\n{}",
            first, second
        );
    }
}
