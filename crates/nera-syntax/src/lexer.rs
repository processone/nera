use logos::Logos;

use crate::error::ParseError;
use crate::span::Span;
use crate::token::Token;

/// A thin wrapper around the `logos` lexer that produces `(Token, Span)` pairs
/// and converts lexer errors into `ParseError::LexerError`.
pub struct Lexer<'src> {
    inner: logos::Lexer<'src, Token>,
    finished: bool,
}

/// Result of lexing with comments preserved.
pub struct LexResult {
    pub tokens: Vec<(Token, Span)>,
    pub comments: Vec<(String, Span)>,
}

impl<'src> Lexer<'src> {
    /// Create a new lexer for the given source text.
    pub fn new(source: &'src str) -> Self {
        Self {
            inner: Token::lexer(source),
            finished: false,
        }
    }

    /// Lex all tokens into a vector, filtering out comments.
    /// Lexer errors are collected as `ParseError::LexerError`.
    pub fn lex_all(source: &str) -> Result<Vec<(Token, Span)>, Vec<ParseError>> {
        let mut tokens = Vec::new();
        let mut errors = Vec::new();
        let mut lexer = Token::lexer(source);

        while let Some(result) = lexer.next() {
            let span_range = lexer.span();
            let span = Span::new(span_range.start, span_range.end);
            match result {
                Ok(Token::Comment) => {} // filter out comments
                Ok(tok) => tokens.push((tok.normalized(), span)),
                Err(()) => errors.push(ParseError::LexerError {
                    position: span_range.start,
                }),
            }
        }

        if errors.is_empty() {
            Ok(tokens)
        } else {
            Err(errors)
        }
    }

    /// Lex all tokens, returning both the token stream (without comments)
    /// and a separate list of comments with their text and spans.
    pub fn lex_all_with_comments(source: &str) -> Result<LexResult, Vec<ParseError>> {
        let mut tokens = Vec::new();
        let mut comments = Vec::new();
        let mut errors = Vec::new();
        let mut lexer = Token::lexer(source);

        while let Some(result) = lexer.next() {
            let span_range = lexer.span();
            let span = Span::new(span_range.start, span_range.end);
            match result {
                Ok(Token::Comment) => {
                    let text = source[span_range].to_string();
                    comments.push((text, span));
                }
                Ok(tok) => tokens.push((tok.normalized(), span)),
                Err(()) => errors.push(ParseError::LexerError {
                    position: span_range.start,
                }),
            }
        }

        if errors.is_empty() {
            Ok(LexResult { tokens, comments })
        } else {
            Err(errors)
        }
    }
}

impl<'src> Iterator for Lexer<'src> {
    type Item = Result<(Token, Span), ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        loop {
            match self.inner.next() {
                Some(result) => {
                    let span_range = self.inner.span();
                    let span = Span::new(span_range.start, span_range.end);
                    match result {
                        Ok(Token::Comment) => continue, // skip comments in iterator
                        Ok(tok) => return Some(Ok((tok.normalized(), span))),
                        Err(()) => return Some(Err(ParseError::LexerError {
                            position: span_range.start,
                        })),
                    }
                }
                None => {
                    self.finished = true;
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lex_simple_tokens() {
        let tokens = Lexer::lex_all("foo(X) -> X + 1.").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                Token::Atom,       // foo
                Token::LParen,     // (
                Token::Variable,   // X
                Token::RParen,     // )
                Token::Arrow,      // ->
                Token::Variable,   // X
                Token::Plus,       // +
                Token::Integer(1), // 1
                Token::Dot,        // .
            ]
        );
    }

    #[test]
    fn lex_keywords() {
        let tokens = Lexer::lex_all("module case of end if when receive after").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                Token::KwModule,
                Token::KwCase,
                Token::KwOf,
                Token::KwEnd,
                Token::KwIf,
                Token::KwWhen,
                Token::KwReceive,
                Token::KwAfter,
            ]
        );
    }

    #[test]
    fn lex_string_and_quoted_atom() {
        let tokens = Lexer::lex_all(r#""hello" 'world'"#).unwrap();
        let kinds: Vec<_> = tokens.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(kinds, vec![Token::String, Token::QuotedAtom]);
    }

    #[test]
    fn lex_operators() {
        let tokens = Lexer::lex_all("=:= =/= /= == =< >= ++ -- -> <-").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                Token::ExactEq,
                Token::ExactNotEq,
                Token::NotEq,
                Token::Eq,
                Token::LtEq,
                Token::GtEq,
                Token::PlusPlus,
                Token::MinusMinus,
                Token::Arrow,
                Token::LArrow,
            ]
        );
    }

    #[test]
    fn lex_comments_are_skipped() {
        let tokens = Lexer::lex_all("foo % this is a comment\nbar").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(kinds, vec![Token::Atom, Token::Atom]);
    }

    #[test]
    fn lex_comments_are_captured() {
        let result = Lexer::lex_all_with_comments("foo % this is a comment\nbar").unwrap();
        let kinds: Vec<_> = result.tokens.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(kinds, vec![Token::Atom, Token::Atom]);
        assert_eq!(result.comments.len(), 1);
        assert_eq!(result.comments[0].0, "% this is a comment");
    }

    #[test]
    fn lex_float() {
        let tokens = Lexer::lex_all("3.14 1.0e10").unwrap();
        assert!(matches!(tokens[0].0, Token::Float(_)));
        assert!(matches!(tokens[1].0, Token::Float(_)));
    }

    #[test]
    fn lex_spans_are_correct() {
        let tokens = Lexer::lex_all("ab cd").unwrap();
        assert_eq!(tokens[0].1, Span::new(0, 2));
        assert_eq!(tokens[1].1, Span::new(3, 5));
    }

    #[test]
    fn lex_char_literals() {
        let tokens = Lexer::lex_all("$a $Z $0 $@").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                Token::Integer(97),  // $a
                Token::Integer(90),  // $Z
                Token::Integer(48),  // $0
                Token::Integer(64),  // $@
            ]
        );
    }

    #[test]
    fn lex_char_escape_sequences() {
        let tokens = Lexer::lex_all(r"$\n $\t $\r $\\ $\s").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                Token::Integer(10),  // $\n
                Token::Integer(9),   // $\t
                Token::Integer(13),  // $\r
                Token::Integer(92),  // $\\
                Token::Integer(32),  // $\s
            ]
        );
    }

    #[test]
    fn lex_numeric_separators_for_cow_hpack() {
        for (source, expected) in [
            ("10_000", Token::Integer(10000)),
            ("1_6#F_F", Token::Integer(255)),
            ("2#1_0_1", Token::Integer(5)),
            ("123_456_789_012_345_678_901", Token::BigInteger("123456789012345678901".into())),
            ("1_2.3_4e+0_3", Token::Float(12340.0)),
        ] {
            let expected = vec![(expected, Span::new(0, source.len()))];
            assert_eq!(Lexer::lex_all(source).unwrap(), expected);
            assert_eq!(Lexer::new(source).collect::<Result<Vec<_>, _>>().unwrap(), expected);
            assert_eq!(Lexer::lex_all_with_comments(source).unwrap().tokens, expected);
        }
        for source in ["1__0", "1_", "16#_ff", "16#f__f", "1_.0", "1.0_e2"] {
            assert!(Lexer::lex_all(source).is_err(), "{source}");
        }
    }

    #[test]
    fn lex_large_integer_literals() {
        for source in ["99999999999999999999", "16#56BC75E2D630FFFFF"] {
            assert_eq!(Lexer::lex_all(source).unwrap(),
                vec![(Token::BigInteger("99999999999999999999".into()), Span::new(0, source.len()))]);
            assert_eq!(Lexer::new(source).collect::<Result<Vec<_>, _>>().unwrap(),
                Lexer::lex_all(source).unwrap());
            assert_eq!(Lexer::lex_all_with_comments(source).unwrap().tokens,
                Lexer::lex_all(source).unwrap());
        }
    }

    #[test]
    fn lex_integer_boundaries_and_leading_zeroes() {
        for source in ["9223372036854775807", "16#7FFFFFFFFFFFFFFF"] {
            assert_eq!(Lexer::lex_all(source).unwrap()[0].0, Token::Integer(i64::MAX));
        }
        for source in ["9223372036854775808", "16#8000000000000000"] {
            assert_eq!(Lexer::lex_all(source).unwrap()[0].0,
                Token::BigInteger("9223372036854775808".into()));
        }
        assert_eq!(Lexer::lex_all("000000000000000000000000000001").unwrap()[0].0,
            Token::Integer(1));
        for source in ["1#123", "37#1", "2#102", "16#GG"] {
            assert!(Lexer::lex_all(source).is_err(), "{source}");
        }
    }

    #[test]
    fn lex_char_escaped_ordinary_characters() {
        for (source, value) in [(r"$\#", 35), (r"$\?", 63), (r"$\a", 97),
            (r"$\8", 56), (r"$\9", 57), (r"$\é", 233)] {
            assert_eq!(Lexer::lex_all(source).unwrap(),
                vec![(Token::Integer(value), Span::new(0, source.len()))]);
        }
    }

    #[test]
    fn lex_char_incomplete_reserved_escapes_are_errors() {
        for source in [r"$\", r"$\x", r"$\^"] {
            assert!(Lexer::lex_all(source).is_err(), "{source}");
        }
    }

    #[test]
    fn lex_char_octal_escapes() {
        for (source, value) in [(r"$\0", 0), (r"$\7", 7), (r"$\77", 63),
            (r"$\013", 11), (r"$\014", 12), (r"$\377", 255), (r"$\777", 511)] {
            assert_eq!(Lexer::lex_all(source).unwrap(),
                vec![(Token::Integer(value), Span::new(0, source.len()))]);
        }
    }

    #[test]
    fn lex_char_octal_stops_at_three_digits_or_non_octal() {
        for (source, value, end, following) in [(r"$\1234", 83, 5, 4), (r"$\78", 7, 3, 8)] {
            assert_eq!(Lexer::lex_all(source).unwrap(), vec![
                (Token::Integer(value), Span::new(0, end)),
                (Token::Integer(following), Span::new(end, source.len())),
            ]);
        }
    }

    #[test]
    fn lex_quoted_atom_with_escape() {
        let tokens = Lexer::lex_all(r"'it\'s'").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(kinds, vec![Token::QuotedAtom]);
    }

    #[test]
    fn lex_quoted_atom_with_newline_escape() {
        let tokens = Lexer::lex_all(r"'hello\nworld'").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(kinds, vec![Token::QuotedAtom]);
    }

    #[test]
    fn lex_base_n_integers() {
        let tokens = Lexer::lex_all("16#FF 2#1010 8#77 36#Z").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                Token::Integer(255), // 16#FF
                Token::Integer(10),  // 2#1010
                Token::Integer(63),  // 8#77
                Token::Integer(35),  // 36#Z
            ]
        );
    }

    #[test]
    fn lex_variable_with_at() {
        // The BEAM accepts @ in variable names (used by compiler-generated code,
        // e.g. thoas, Elixir output, Core Erlang).
        let tokens = Lexer::lex_all("_stack@2 _@1 _hex@1 Rest@3").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                Token::Variable,  // _stack@2
                Token::Variable,  // _@1
                Token::Variable,  // _hex@1
                Token::Variable,  // Rest@3
            ]
        );
    }
}
