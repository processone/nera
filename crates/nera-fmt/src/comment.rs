use nera_syntax::span::Span;

/// A source comment extracted from the lexer.
#[derive(Debug, Clone)]
pub struct Comment {
    /// Full comment text including the leading `%` characters.
    pub text: String,
    /// Byte span in the original source.
    pub span: Span,
    /// Whether there is code on the same line before this comment.
    pub is_trailing: bool,
    /// Number of leading `%` signs (1 = `%`, 2 = `%%`, 3 = `%%%`).
    pub level: u8,
}

impl Comment {
    /// Build a `Comment` from raw lexer output, classifying it based on context.
    pub fn from_raw(text: String, span: Span, source: &str) -> Self {
        let level = text.bytes().take_while(|&b| b == b'%').count() as u8;
        let is_trailing = has_code_before(source, span.start);
        Comment { text, span, is_trailing, level }
    }
}

/// Returns true if there is non-whitespace content on the same line before `pos`.
fn has_code_before(source: &str, pos: usize) -> bool {
    let before = &source[..pos];
    match before.rfind('\n') {
        Some(nl) => !before[nl + 1..].trim().is_empty(),
        None => !before.trim().is_empty(),
    }
}

/// Extract comments from raw lexer output.
pub fn extract_comments(raw_comments: Vec<(String, Span)>, source: &str) -> Vec<Comment> {
    raw_comments
        .into_iter()
        .map(|(text, span)| Comment::from_raw(text, span, source))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_standalone_comment() {
        let src = "    % standalone\n";
        let c = Comment::from_raw("% standalone".into(), Span::new(4, 16), src);
        assert!(!c.is_trailing);
        assert_eq!(c.level, 1);
    }

    #[test]
    fn classify_trailing_comment() {
        let src = "foo(X) -> X. % trailing\n";
        let c = Comment::from_raw("% trailing".into(), Span::new(13, 23), src);
        assert!(c.is_trailing);
        assert_eq!(c.level, 1);
    }

    #[test]
    fn classify_double_percent() {
        let src = "%% module doc\n";
        let c = Comment::from_raw("%% module doc".into(), Span::new(0, 13), src);
        assert!(!c.is_trailing);
        assert_eq!(c.level, 2);
    }
}
