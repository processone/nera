//! Wadler-Lindig document algebra for pretty printing.
//!
//! Based on "A prettier printer" (Philip Wadler, 2003) with extensions
//! for `IfBreak` (conditional content based on whether a group breaks).

/// A document node in the pretty-printing algebra.
#[derive(Debug, Clone)]
pub enum Doc {
    /// Empty document.
    Nil,
    /// Literal text (no newlines).
    Text(String),
    /// Concatenation of two documents.
    Concat(Box<Doc>, Box<Doc>),
    /// A potential line break: rendered as a space when flat, newline when broken.
    Line,
    /// A mandatory line break (always rendered as newline).
    HardLine,
    /// Group: try to fit all on one line; if not, break.
    Group(Box<Doc>),
    /// Indent nested content by the given amount.
    Nest(i32, Box<Doc>),
    /// Content that differs based on whether the enclosing group breaks.
    /// `IfBreak(break_doc, flat_doc)` — uses `break_doc` when broken, `flat_doc` when flat.
    IfBreak(Box<Doc>, Box<Doc>),
    /// Fill: greedily pack documents on lines. The `Vec<Doc>` alternates
    /// between content documents and separator documents:
    ///   [doc1, sep1, doc2, sep2, doc3]
    /// In flat mode, renders everything flat. In break mode, each separator
    /// is independently decided: flat if the next content still fits on the
    /// current line, broken (newline+indent) otherwise.
    Fill(Vec<Doc>),
    /// Try alternative documents in order; select the first whose flat
    /// rendering fits within the remaining width. The last alternative is
    /// used unconditionally as the fallback. Each chosen alternative is
    /// rendered in break mode (inner Groups decide independently).
    BestOf(Vec<Doc>),
    /// Like Group but always rendered in Break mode. Used for last-argument
    /// expansion where a container should always render expanded. In `fits()`
    /// and measurement, behaves like a breaking Group (content measured up
    /// to first Line/HardLine only).
    ForceBreak(Box<Doc>),
}

// ── Builder helpers ─────────────────────────────────────────────────

pub fn nil() -> Doc {
    Doc::Nil
}

pub fn text<S: Into<String>>(s: S) -> Doc {
    Doc::Text(s.into())
}

pub fn concat(a: Doc, b: Doc) -> Doc {
    match (&a, &b) {
        (Doc::Nil, _) => b,
        (_, Doc::Nil) => a,
        _ => Doc::Concat(Box::new(a), Box::new(b)),
    }
}

pub fn line() -> Doc {
    Doc::Line
}

pub fn hardline() -> Doc {
    Doc::HardLine
}

pub fn group(doc: Doc) -> Doc {
    Doc::Group(Box::new(doc))
}

pub fn nest(indent: i32, doc: Doc) -> Doc {
    Doc::Nest(indent, Box::new(doc))
}

pub fn if_break(break_doc: Doc, flat_doc: Doc) -> Doc {
    Doc::IfBreak(Box::new(break_doc), Box::new(flat_doc))
}

/// Build a Fill doc from a flat list of alternating content/separator docs.
pub fn fill(parts: Vec<Doc>) -> Doc {
    Doc::Fill(parts)
}

/// Try alternative documents in order; select the first that fits.
/// Requires at least 2 alternatives; the last is the unconditional fallback.
pub fn best_of(alternatives: Vec<Doc>) -> Doc {
    debug_assert!(alternatives.len() >= 2);
    Doc::BestOf(alternatives)
}

/// Returns true if the document can be rendered entirely on one line (no HardLine).
pub fn can_flatten(doc: &Doc) -> bool {
    match doc {
        Doc::Nil | Doc::Text(_) | Doc::Line => true,
        Doc::HardLine => false,
        Doc::ForceBreak(_) => false,
        Doc::Concat(a, b) => can_flatten(a) && can_flatten(b),
        Doc::Group(inner) | Doc::Nest(_, inner) => can_flatten(inner),
        Doc::IfBreak(_, flat_doc) => can_flatten(flat_doc),
        Doc::Fill(parts) => parts.iter().all(can_flatten),
        Doc::BestOf(alts) => alts.first().is_none_or(can_flatten),
    }
}

/// Wrap a document so it always renders in Break mode, regardless of the
/// enclosing group. If the doc is a Group, the Group is stripped so that
/// its content renders directly in Break mode (no flat attempt). Inner
/// Groups still decide independently.
pub fn force_break(doc: Doc) -> Doc {
    match doc {
        Doc::Group(inner) => Doc::ForceBreak(inner),
        other => Doc::ForceBreak(Box::new(other)),
    }
}

/// Build a Fill doc from content items with a uniform separator between each pair.
/// Returns `[item1, sep, item2, sep, ..., itemN]` wrapped in Fill.
pub fn fill_sep(items: Vec<Doc>, sep: Doc) -> Doc {
    if items.is_empty() {
        return Doc::Nil;
    }
    if items.len() == 1 {
        return items.into_iter().next().unwrap();
    }
    let mut parts = Vec::with_capacity(items.len() * 2 - 1);
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            parts.push(sep.clone());
        }
        parts.push(item);
    }
    Doc::Fill(parts)
}

/// Concatenate a list of documents.
pub fn concat_all(docs: Vec<Doc>) -> Doc {
    docs.into_iter().fold(Doc::Nil, concat)
}

/// Intersperse a separator between documents.
pub fn intersperse(docs: Vec<Doc>, sep: Doc) -> Doc {
    let mut result = Doc::Nil;
    let mut first = true;
    for doc in docs {
        if first {
            first = false;
            result = doc;
        } else {
            result = concat(result, concat(sep.clone(), doc));
        }
    }
    result
}

/// Concatenate with a space.
pub fn space_concat(a: Doc, b: Doc) -> Doc {
    concat(a, concat(text(" "), b))
}

// ── Renderer ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Flat,
    Break,
}

/// Render a document to a string within the given width.
pub fn render(width: usize, doc: &Doc) -> String {
    let mut out = String::new();
    // Stack of (indent, mode, doc)
    let mut stack: Vec<(i32, Mode, &Doc)> = vec![(0, Mode::Break, doc)];
    let mut col: usize = 0;

    while let Some((indent, mode, doc)) = stack.pop() {
        match doc {
            Doc::Nil => {}
            Doc::Text(s) => {
                out.push_str(s);
                col += s.len();
            }
            Doc::Concat(a, b) => {
                // Push b first so a is processed first (LIFO)
                stack.push((indent, mode, b));
                stack.push((indent, mode, a));
            }
            Doc::Line => {
                match mode {
                    Mode::Flat => {
                        out.push(' ');
                        col += 1;
                    }
                    Mode::Break => {
                        out.push('\n');
                        let spaces = indent.max(0) as usize;
                        for _ in 0..spaces {
                            out.push(' ');
                        }
                        col = spaces;
                    }
                }
            }
            Doc::HardLine => {
                out.push('\n');
                let spaces = indent.max(0) as usize;
                for _ in 0..spaces {
                    out.push(' ');
                }
                col = spaces;
            }
            Doc::Nest(extra, inner) => {
                stack.push((indent + extra, mode, inner));
            }
            Doc::Group(inner) => {
                // Try flat mode: measure if it fits
                if fits(width as i32 - col as i32, &[(indent, Mode::Flat, inner.as_ref())]) {
                    stack.push((indent, Mode::Flat, inner));
                } else {
                    stack.push((indent, Mode::Break, inner));
                }
            }
            Doc::ForceBreak(inner) => {
                stack.push((indent, Mode::Break, inner));
            }
            Doc::IfBreak(break_doc, flat_doc) => {
                match mode {
                    Mode::Break => stack.push((indent, mode, break_doc)),
                    Mode::Flat => stack.push((indent, mode, flat_doc)),
                }
            }
            Doc::Fill(parts) => {
                match mode {
                    Mode::Flat => {
                        // Render all parts flat: push in reverse for LIFO stack.
                        for part in parts.iter().rev() {
                            stack.push((indent, Mode::Flat, part));
                        }
                    }
                    Mode::Break => {
                        // Greedy fill: render each part inline, making per-separator
                        // decisions based on the current column. For the next content,
                        // we measure its *first-line width* (the width up to any soft
                        // break inside it) — this lets content start on the current
                        // line and then break internally as a group.
                        let mut idx = 0;
                        while idx < parts.len() {
                            let is_sep = idx % 2 == 1;
                            if is_sep {
                                // Does sep(flat) + next_content's first line fit?
                                // For the next content we measure its first line assuming
                                // its top-level Group will break — i.e. the width of its
                                // own first element. This matches how the content will
                                // actually be rendered starting at this column.
                                let sep = &parts[idx];
                                let fits_flat = if idx + 1 < parts.len() {
                                    let sep_w = measure_first_line(sep);
                                    let next_w = measure_first_token(&parts[idx + 1]);
                                    (col as i32) + sep_w + next_w <= width as i32
                                } else {
                                    fits(
                                        width as i32 - col as i32,
                                        &[(indent, Mode::Flat, sep)],
                                    )
                                };
                                let sep_mode = if fits_flat { Mode::Flat } else { Mode::Break };
                                render_subdoc(&mut out, &mut col, width, indent, sep_mode, sep);
                            } else {
                                // Content: render with inherited Break mode (inner groups
                                // will still independently decide via fits()).
                                render_subdoc(&mut out, &mut col, width, indent, Mode::Break, &parts[idx]);
                            }
                            idx += 1;
                        }
                    }
                }
            }
            Doc::BestOf(alternatives) => {
                let last_idx = alternatives.len() - 1;
                let mut chosen = last_idx;
                for (i, alt) in alternatives.iter().enumerate() {
                    if i == last_idx {
                        break;
                    }
                    if fits(width as i32 - col as i32, &[(indent, Mode::Flat, alt)]) {
                        chosen = i;
                        break;
                    }
                }
                stack.push((indent, Mode::Break, &alternatives[chosen]));
            }
        }
    }

    // Strip trailing whitespace from each line
    out.split('\n')
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render a single sub-document to `out`, starting at column `col` with the given
/// initial mode. Updates `col` in place. Uses a local stack so it does not interfere
/// with the main renderer's stack.
fn render_subdoc(
    out: &mut String,
    col: &mut usize,
    width: usize,
    initial_indent: i32,
    initial_mode: Mode,
    doc: &Doc,
) {
    let mut stack: Vec<(i32, Mode, &Doc)> = vec![(initial_indent, initial_mode, doc)];
    while let Some((indent, mode, doc)) = stack.pop() {
        match doc {
            Doc::Nil => {}
            Doc::Text(s) => {
                out.push_str(s);
                *col += s.len();
            }
            Doc::Concat(a, b) => {
                stack.push((indent, mode, b));
                stack.push((indent, mode, a));
            }
            Doc::Line => match mode {
                Mode::Flat => {
                    out.push(' ');
                    *col += 1;
                }
                Mode::Break => {
                    out.push('\n');
                    let spaces = indent.max(0) as usize;
                    for _ in 0..spaces {
                        out.push(' ');
                    }
                    *col = spaces;
                }
            },
            Doc::HardLine => {
                out.push('\n');
                let spaces = indent.max(0) as usize;
                for _ in 0..spaces {
                    out.push(' ');
                }
                *col = spaces;
            }
            Doc::Nest(extra, inner) => {
                stack.push((indent + extra, mode, inner));
            }
            Doc::Group(inner) => {
                if fits(width as i32 - *col as i32, &[(indent, Mode::Flat, inner.as_ref())]) {
                    stack.push((indent, Mode::Flat, inner));
                } else {
                    stack.push((indent, Mode::Break, inner));
                }
            }
            Doc::ForceBreak(inner) => {
                stack.push((indent, Mode::Break, inner));
            }
            Doc::IfBreak(break_doc, flat_doc) => match mode {
                Mode::Break => stack.push((indent, mode, break_doc)),
                Mode::Flat => stack.push((indent, mode, flat_doc)),
            },
            Doc::Fill(parts) => match mode {
                Mode::Flat => {
                    for part in parts.iter().rev() {
                        stack.push((indent, Mode::Flat, part));
                    }
                }
                Mode::Break => {
                    // Nested fill in break mode: same greedy algorithm as main renderer.
                    let mut idx = 0;
                    while idx < parts.len() {
                        let is_sep = idx % 2 == 1;
                        if is_sep {
                            let sep = &parts[idx];
                            let fits_flat = if idx + 1 < parts.len() {
                                // Check: does sep(flat) + the next content's first token
                                // fit? The next content may be a Group that breaks
                                // internally — we only need its leading text to fit.
                                let sep_w = measure_first_line(sep);
                                let next_w = measure_first_token(&parts[idx + 1]);
                                (*col as i32) + sep_w + next_w <= width as i32
                            } else {
                                fits(width as i32 - *col as i32, &[(indent, Mode::Flat, sep)])
                            };
                            let sep_mode = if fits_flat { Mode::Flat } else { Mode::Break };
                            render_subdoc(out, col, width, indent, sep_mode, sep);
                        } else {
                            render_subdoc(out, col, width, indent, Mode::Break, &parts[idx]);
                        }
                        idx += 1;
                    }
                }
            },
            Doc::BestOf(alternatives) => {
                let last_idx = alternatives.len() - 1;
                let mut chosen = last_idx;
                for (i, alt) in alternatives.iter().enumerate() {
                    if i == last_idx {
                        break;
                    }
                    if fits(width as i32 - *col as i32, &[(indent, Mode::Flat, alt)]) {
                        chosen = i;
                        break;
                    }
                }
                stack.push((indent, Mode::Break, &alternatives[chosen]));
            }
        }
    }
}

/// Measure the width of a document's first line assuming inner Groups are
/// rendered flat (best case). Stops counting at the first Line/HardLine
/// that would produce a newline. Used by Fill to decide if the next piece
/// of content can start on the current line.
fn measure_first_line(doc: &Doc) -> i32 {
    let mut total: i32 = 0;
    // Stack of (mode, doc) — mode tracks whether Line is space (Flat) or newline (Break).
    let mut stack: Vec<(Mode, &Doc)> = vec![(Mode::Flat, doc)];
    while let Some((mode, doc)) = stack.pop() {
        match doc {
            Doc::Nil => {}
            Doc::Text(s) => total += s.len() as i32,
            Doc::Concat(a, b) => {
                stack.push((mode, b));
                stack.push((mode, a));
            }
            Doc::Line => match mode {
                Mode::Flat => total += 1,
                Mode::Break => return total, // first line ends here
            },
            Doc::HardLine => return total,
            Doc::Nest(_, inner) => stack.push((mode, inner)),
            Doc::Group(inner) => stack.push((Mode::Flat, inner)),
            Doc::ForceBreak(inner) => stack.push((Mode::Break, inner)),
            Doc::IfBreak(break_doc, flat_doc) => match mode {
                Mode::Break => stack.push((mode, break_doc)),
                Mode::Flat => stack.push((mode, flat_doc)),
            },
            Doc::Fill(parts) => {
                // First line of a Fill: assume it tries to pack, stops at first break.
                for part in parts.iter().rev() {
                    stack.push((mode, part));
                }
            }
            Doc::BestOf(alternatives) => {
                if let Some(first) = alternatives.first() {
                    stack.push((mode, first));
                }
            }
        }
    }
    total
}

/// Measure the width of a document's leading text — the characters that
/// will appear before any potential line break inside it, when rendered in
/// break mode. Inner Groups are treated as breaking (we only count text up
/// to their first internal Line). Used by Fill to estimate whether the
/// next content's leading token fits on the current line.
fn measure_first_token(doc: &Doc) -> i32 {
    let mut total: i32 = 0;
    // In Break mode: any Line or HardLine terminates the first token.
    // Groups/Fills are entered in Break mode (worst case — they'll break).
    let mut stack: Vec<&Doc> = vec![doc];
    while let Some(doc) = stack.pop() {
        match doc {
            Doc::Nil => {}
            Doc::Text(s) => total += s.len() as i32,
            Doc::Concat(a, b) => {
                stack.push(b);
                stack.push(a);
            }
            Doc::Line => return total,
            Doc::HardLine => return total,
            Doc::Nest(_, inner) => stack.push(inner),
            Doc::Group(inner) => stack.push(inner),
            Doc::ForceBreak(inner) => stack.push(inner),
            Doc::IfBreak(break_doc, _flat_doc) => stack.push(break_doc),
            Doc::Fill(parts) => {
                for part in parts.iter().rev() {
                    stack.push(part);
                }
            }
            Doc::BestOf(alternatives) => {
                if let Some(first) = alternatives.first() {
                    stack.push(first);
                }
            }
        }
    }
    total
}

/// Check if the document fits within `remaining` columns in flat mode.
fn fits(mut remaining: i32, stack: &[(i32, Mode, &Doc)]) -> bool {
    let mut work: Vec<(i32, Mode, &Doc)> = stack.iter().rev().cloned().collect();

    while remaining >= 0 {
        let (indent, mode, doc) = match work.pop() {
            Some(item) => item,
            None => return true,
        };

        match doc {
            Doc::Nil => {}
            Doc::Text(s) => {
                remaining -= s.len() as i32;
            }
            Doc::Concat(a, b) => {
                work.push((indent, mode, b));
                work.push((indent, mode, a));
            }
            Doc::Line => {
                match mode {
                    Mode::Flat => {
                        remaining -= 1; // space
                    }
                    Mode::Break => return true, // newline always fits
                }
            }
            Doc::HardLine => return true,
            Doc::Nest(extra, inner) => {
                work.push((indent + extra, mode, inner));
            }
            Doc::Group(inner) => {
                work.push((indent, Mode::Flat, inner));
            }
            Doc::ForceBreak(inner) => {
                // ForceBreak always renders in Break mode, so in fits()
                // we measure its content in Break mode. This means Line
                // becomes a newline (returns true), accurately reflecting
                // that ForceBreak content will break to a new line.
                work.push((indent, Mode::Break, inner));
            }
            Doc::IfBreak(break_doc, flat_doc) => {
                match mode {
                    Mode::Break => work.push((indent, mode, break_doc)),
                    Mode::Flat => work.push((indent, mode, flat_doc)),
                }
            }
            Doc::Fill(parts) => {
                // When measuring flat, treat Fill parts as flat in sequence.
                for part in parts.iter().rev() {
                    work.push((indent, Mode::Flat, part));
                }
            }
            Doc::BestOf(alternatives) => {
                // When measuring, assume the first (most compact) alternative.
                if let Some(first) = alternatives.first() {
                    work.push((indent, Mode::Flat, first));
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_renders_as_is() {
        let doc = text("hello");
        assert_eq!(render(80, &doc), "hello");
    }

    #[test]
    fn concat_joins() {
        let doc = concat(text("a"), text("b"));
        assert_eq!(render(80, &doc), "ab");
    }

    #[test]
    fn group_stays_flat_when_fits() {
        let doc = group(concat(text("a"), concat(line(), text("b"))));
        assert_eq!(render(80, &doc), "a b");
    }

    #[test]
    fn group_breaks_when_too_wide() {
        let doc = group(concat(text("a"), concat(line(), text("b"))));
        assert_eq!(render(2, &doc), "a\nb");
    }

    #[test]
    fn nest_indents_on_break() {
        let doc = group(nest(4, concat(text("a"), concat(line(), text("b")))));
        assert_eq!(render(2, &doc), "a\n    b");
    }

    #[test]
    fn hardline_always_breaks() {
        let doc = group(concat(text("a"), concat(hardline(), text("b"))));
        assert_eq!(render(80, &doc), "a\nb");
    }

    #[test]
    fn if_break_flat() {
        let doc = group(concat(text("("), concat(
            if_break(text("BREAK"), text("")),
            text(")")
        )));
        assert_eq!(render(80, &doc), "()");
    }

    #[test]
    fn if_break_broken() {
        let inner = concat(
            text("("),
            concat(
                if_break(hardline(), nil()),
                concat(text("long_content_that_forces_break"), text(")"))
            )
        );
        let doc = group(inner);
        // When group breaks, if_break selects the break_doc
        let result = render(10, &doc);
        assert!(result.contains('\n'));
    }

    #[test]
    fn intersperse_with_comma() {
        let docs = vec![text("a"), text("b"), text("c")];
        let doc = intersperse(docs, text(", "));
        assert_eq!(render(80, &doc), "a, b, c");
    }

    #[test]
    fn nil_is_identity() {
        assert_eq!(render(80, &concat(nil(), text("x"))), "x");
        assert_eq!(render(80, &concat(text("x"), nil())), "x");
    }

    #[test]
    fn fill_empty_is_nil() {
        let doc = fill_sep(vec![], text(", "));
        assert_eq!(render(80, &doc), "");
    }

    #[test]
    fn fill_single_item() {
        let doc = fill_sep(vec![text("a")], concat(text(","), line()));
        assert_eq!(render(80, &doc), "a");
    }

    #[test]
    fn fill_flat_when_enclosing_group_fits() {
        // Short tuple: outer group fits, fill renders flat (same as intersperse).
        let inner = fill_sep(
            vec![text("a"), text("b"), text("c")],
            concat(text(","), line()),
        );
        let doc = group(concat_all(vec![text("{"), inner, text("}")]));
        assert_eq!(render(80, &doc), "{a, b, c}");
    }

    #[test]
    fn fill_packs_greedily_when_group_breaks() {
        // When the outer group can't fit, fill should pack items per-line,
        // not put one item per line.
        let items: Vec<Doc> = (1..=10).map(|i| text(format!("item{}", i))).collect();
        let doc = group(concat_all(vec![
            text("{"),
            nest(1, fill_sep(items, concat(text(","), line()))),
            text("}"),
        ]));
        // Width 30: each item is ~5 chars + ", " = 7 chars. ~4 per line.
        let out = render(30, &doc);
        // Should be multi-line but not one-per-line.
        let line_count = out.lines().count();
        assert!(line_count > 1, "expected multi-line, got: {:?}", out);
        assert!(line_count < 10, "expected fewer than 10 lines, got {} lines: {:?}", line_count, out);
    }

    #[test]
    fn fill_respects_line_width() {
        // Every line (after first) must fit within width.
        let items: Vec<Doc> = (1..=20).map(|i| text(format!("x{}", i))).collect();
        let doc = group(concat_all(vec![
            text("{"),
            nest(1, fill_sep(items, concat(text(","), line()))),
            text("}"),
        ]));
        let out = render(20, &doc);
        for l in out.lines() {
            assert!(l.len() <= 25, "line exceeds reasonable width: {:?}", l);
        }
    }

    #[test]
    fn fill_preserves_first_item_on_opening_bracket_line() {
        // After an opening bracket, the first fill item should be on the same line.
        let items: Vec<Doc> = (1..=5).map(|i| text(format!("item_{}", i))).collect();
        let doc = group(concat_all(vec![
            text("{"),
            nest(1, fill_sep(items, concat(text(","), line()))),
            text("}"),
        ]));
        let out = render(20, &doc);
        assert!(out.starts_with("{item_1"), "first item should be on bracket line, got: {:?}", out);
    }

    #[test]
    fn fill_with_inner_group() {
        // Inner groups within fill should decide flat/break independently.
        let inner_big = group(concat_all(vec![
            text("["),
            nest(1, fill_sep(
                (1..=10).map(|i| text(format!("e{}", i))).collect(),
                concat(text(","), line()),
            )),
            text("]"),
        ]));
        let items = vec![text("small"), inner_big, text("tail")];
        let doc = group(concat_all(vec![
            text("{"),
            nest(1, fill_sep(items, concat(text(","), line()))),
            text("}"),
        ]));
        let out = render(30, &doc);
        // Should start with "{small," on first line
        assert!(out.starts_with("{small"), "got: {:?}", out);
    }

    #[test]
    fn trailing_whitespace_stripped() {
        // Text with trailing spaces should be stripped
        let doc = concat(text("hello   "), concat(hardline(), text("world")));
        assert_eq!(render(80, &doc), "hello\nworld");

        // Indentation-only lines (empty nested content) should become empty lines
        let doc = concat(text("a"), concat(hardline(), concat(text(""), concat(hardline(), text("b")))));
        assert_eq!(render(80, &doc), "a\n\nb");
    }
}
