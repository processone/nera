use logos::Logos;

/// Numeric token payload before normalization into small/large token variants.
#[derive(Debug, Clone, PartialEq)]
pub enum IntegerValue {
    Small(i64),
    Large(String),
}

/// Tokens produced by the Erlang lexer.
///
/// Logos resolves conflicts by giving priority to literal `#[token(...)]` matches
/// over regex `#[regex(...)]` matches. This means keywords like `module` are
/// matched by their dedicated token variant rather than the `Atom` regex.
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]       // skip whitespace
pub enum Token {
    // ── Comments ─────────────────────────────────────────────
    /// Line comment starting with one or more `%` characters.
    #[regex(r"%[^\n]*", allow_greedy = true)]
    Comment,
    // ── Literals ──────────────────────────────────────────────

    /// Integer literal, e.g. `42`, `0`, `-3` (negation handled in parser).
    /// Also handles character literals (`$a`, `$\n`) and base-N integers (`16#FF`).
    // Ordinary escapes denote the escaped character itself. Keep octal,
    // hexadecimal and control-prefix syntax out of this fallback.
    #[regex(r"\$\\[^0-7x^\r\n]", parse_char_escape)]
    #[regex(r"\$\\[0-7]{1,3}", |lex| i64::from_str_radix(&lex.slice()[2..], 8).ok())]
    #[regex(r"\$[^\\\s]", parse_char_literal)]
    #[regex(r"\$[ \t]", parse_char_literal)]
    Integer(i64),

    /// Intermediate numeric token, normalized by the lexer wrapper.
    #[regex(r"[0-9][0-9_]*", parse_number)]
    #[regex(r"[0-9][0-9_]*#[0-9a-zA-Z_]+", parse_number)]
    Number(IntegerValue),

    /// Canonical decimal representation of an integer outside i64.
    BigInteger(String),

    /// Float literal, e.g. `3.14`.
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9][0-9_]*)?", parse_float)]
    Float(f64),

    /// Quoted atom, e.g. `'hello world'`. Source slice includes quotes.
    /// Allows backslash escape sequences inside quoted atoms.
    #[regex(r"'([^'\\]|\\.)*'")]
    QuotedAtom,

    /// Double-quoted string literal. Source slice includes quotes.
    /// Allows backslash escape sequences inside strings.
    #[regex(r#""([^"\\]|\\.)*""#)]
    String,

    /// Triple-quoted literals are scanned as a whole for ignored documentation
    /// attributes. Runtime literal decoding is not implemented yet.
    #[token("\"\"\"", scan_triple_string)]
    TripleString,

    /// Binary sigils in ignored OTP documentation attributes. Runtime decoding
    /// remains unsupported, so expression parsing deliberately rejects these.
    #[regex(r#"~"([^"\\]|\\.)*""#)]
    DocBinarySigil,

    /// Variable, e.g. `X`, `_Foo`, `_`, `_stack@2`.
    /// The `@` character is accepted by the BEAM and used by compiler-generated
    /// code (e.g. Elixir, Core Erlang output, thoas).
    #[regex(r"[A-Z_][a-zA-Z0-9_@]*")]
    Variable,

    // ── Keywords ──────────────────────────────────────────────
    // Literal token matches take priority over the Atom regex below.

    #[token("module")]
    KwModule,
    #[token("export")]
    KwExport,
    #[token("import")]
    KwImport,
    #[token("case")]
    KwCase,
    #[token("of")]
    KwOf,
    #[token("end")]
    KwEnd,
    #[token("if")]
    KwIf,
    #[token("when")]
    KwWhen,
    #[token("receive")]
    KwReceive,
    #[token("after")]
    KwAfter,
    #[token("fun")]
    KwFun,
    #[token("true")]
    KwTrue,
    #[token("false")]
    KwFalse,
    #[token("not")]
    KwNot,
    #[token("and")]
    KwAnd,
    #[token("or")]
    KwOr,
    #[token("andalso")]
    KwAndalso,
    #[token("orelse")]
    KwOrelse,
    #[token("div")]
    KwDiv,
    #[token("rem")]
    KwRem,
    #[token("band")]
    KwBand,
    #[token("bor")]
    KwBor,
    #[token("bxor")]
    KwBxor,
    #[token("bnot")]
    KwBnot,
    #[token("bsl")]
    KwBsl,
    #[token("bsr")]
    KwBsr,
    #[token("try")]
    KwTry,
    #[token("catch")]
    KwCatch,
    #[token("throw")]
    KwThrow,
    #[token("begin")]
    KwBegin,
    #[token("cond")]
    KwCond,
    #[token("let")]
    KwLet,
    #[token("xor")]
    KwXor,
    #[token("maybe")]
    KwMaybe,
    #[token("else")]
    KwElse,

    // ── Unquoted atom (must come AFTER keyword tokens) ────────
    // Logos gives priority to literal matches, so `module` etc. will
    // never land here.
    #[regex(r"[a-z][a-zA-Z0-9_@]*")]
    Atom,

    // ── Operators (multi-char before single-char) ─────────────

    #[token("<<")]
    BinOpen,
    #[token(">>")]
    BinClose,

    #[token("=:=")]
    ExactEq,
    #[token("=/=")]
    ExactNotEq,
    #[token("/=")]
    NotEq,
    #[token("==")]
    Eq,
    #[token("=<")]
    LtEq,
    #[token(">=")]
    GtEq,
    #[token("=>")]
    FatArrow,
    #[token(":=")]
    ColonEq,
    #[token("->")]
    Arrow,
    #[token("<-")]
    LArrow,
    #[token("<=")]
    BinLArrow,
    #[token("++")]
    PlusPlus,
    #[token("--")]
    MinusMinus,

    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("!")]
    Bang,
    #[token("=")]
    Equals,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,

    // ── Punctuation ───────────────────────────────────────────

    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(";")]
    Semicolon,
    #[token(".")]
    Dot,
    #[token(":")]
    Colon,
    #[token("||")]
    DoublePipe,
    #[token("|")]
    Pipe,
    #[token("#")]
    Hash,
    #[token("?=")]
    MaybeMatch,
    #[token("?")]
    Question,

    // ── Preprocessor-generated placeholders ──────────────────
    // These are never produced by the lexer — only by the preprocessor
    // when expanding ?FUNCTION_NAME and ?FUNCTION_ARITY.

    /// Placeholder for ?FUNCTION_NAME macro (resolved by the parser).
    MacroFunctionName,

    /// Placeholder for ?FUNCTION_ARITY macro (resolved by the parser).
    MacroFunctionArity,
}

// ── Lexer callbacks for Integer patterns ─────────────────────

/// Parse a character literal like `$a` → 97.
fn parse_char_literal(lex: &mut logos::Lexer<'_, Token>) -> Option<i64> {
    let slice = lex.slice();
    slice.chars().nth(1).map(|c| c as i64)
}

/// Parse a named or ordinary escaped character literal (`$\n` → 10, `$\#` → 35).
fn parse_char_escape(lex: &mut logos::Lexer<'_, Token>) -> Option<i64> {
    let slice = lex.slice();
    match slice.chars().nth(2)? {
        'n' => Some(10),
        't' => Some(9),
        'r' => Some(13),
        'b' => Some(8),
        'f' => Some(12),
        'e' => Some(27),
        's' => Some(32),
        'v' => Some(11),
        '\\' => Some(92),
        '\'' => Some(39),
        '"' => Some(34),
        c => Some(c as i64),
    }
}

/// Separators are legal only between two digits of the same numeric component.
fn valid_separators(text: &str, radix: u32) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().all(|(i, &byte)| byte != b'_' ||
        (i > 0 && i + 1 < bytes.len()
            && (bytes[i - 1] as char).is_digit(radix)
            && (bytes[i + 1] as char).is_digit(radix)))
}

fn parse_float(lex: &mut logos::Lexer<'_, Token>) -> Option<f64> {
    let raw = lex.slice();
    if !valid_separators(raw, 10) { return None; }
    raw.replace('_', "").parse().ok()
}

/// Parse decimal or base-N digits without a machine-integer ceiling.
fn parse_number(lex: &mut logos::Lexer<'_, Token>) -> Option<IntegerValue> {
    let raw = lex.slice();
    let (base, digits) = if let Some((base, digits)) = raw.split_once('#') {
        if !valid_separators(base, 10) { return None; }
        (base.replace('_', "").parse::<u32>().ok()?, digits)
    } else {
        (10, raw)
    };
    if !(2..=36).contains(&base) || !valid_separators(digits, base) { return None; }
    let digits = digits.replace('_', "");
    if let Ok(n) = i64::from_str_radix(&digits, base) {
        return Some(IntegerValue::Small(n));
    }
    num_bigint::BigInt::parse_bytes(digits.as_bytes(), base)
        .map(|n| IntegerValue::Large(n.to_string()))
}

impl Token {
    pub(crate) fn normalized(self) -> Self {
        match self {
            Self::Number(IntegerValue::Small(n)) => Self::Integer(n),
            Self::Number(IntegerValue::Large(n)) => Self::BigInteger(n),
            other => other,
        }
    }

    /// Human-readable description of a token, for error messages.
    pub fn description(&self) -> &'static str {
        match self {
            Token::Comment => "comment",
            Token::Integer(_) | Token::BigInteger(_) | Token::Number(_) => "integer",
            Token::Float(_) => "float",
            Token::Atom => "atom",
            Token::QuotedAtom => "quoted atom",
            Token::Variable => "variable",
            Token::String => "string",
            Token::TripleString => "triple-quoted string",
            Token::DocBinarySigil => "binary string sigil",
            Token::KwModule => "'module'",
            Token::KwExport => "'export'",
            Token::KwImport => "'import'",
            Token::KwCase => "'case'",
            Token::KwOf => "'of'",
            Token::KwEnd => "'end'",
            Token::KwIf => "'if'",
            Token::KwWhen => "'when'",
            Token::KwReceive => "'receive'",
            Token::KwAfter => "'after'",
            Token::KwFun => "'fun'",
            Token::KwTrue => "'true'",
            Token::KwFalse => "'false'",
            Token::KwNot => "'not'",
            Token::KwAnd => "'and'",
            Token::KwOr => "'or'",
            Token::KwAndalso => "'andalso'",
            Token::KwOrelse => "'orelse'",
            Token::KwDiv => "'div'",
            Token::KwRem => "'rem'",
            Token::KwBand => "'band'",
            Token::KwBor => "'bor'",
            Token::KwBxor => "'bxor'",
            Token::KwBnot => "'bnot'",
            Token::KwBsl => "'bsl'",
            Token::KwBsr => "'bsr'",
            Token::KwTry => "'try'",
            Token::KwCatch => "'catch'",
            Token::KwThrow => "'throw'",
            Token::KwBegin => "'begin'",
            Token::KwCond => "'cond'",
            Token::KwLet => "'let'",
            Token::KwXor => "'xor'",
            Token::KwMaybe => "'maybe'",
            Token::KwElse => "'else'",
            Token::BinOpen => "'<<'",
            Token::BinClose => "'>>'",
            Token::ExactEq => "'=:='",
            Token::ExactNotEq => "'=/='",
            Token::NotEq => "'/='",
            Token::Eq => "'=='",
            Token::LtEq => "'=<'",
            Token::GtEq => "'>='",
            Token::FatArrow => "'=>'",
            Token::ColonEq => "':='",
            Token::Arrow => "'->'",
            Token::LArrow => "'<-'",
            Token::BinLArrow => "'<='",
            Token::PlusPlus => "'++'",
            Token::MinusMinus => "'--'",
            Token::Plus => "'+'",
            Token::Minus => "'-'",
            Token::Star => "'*'",
            Token::Slash => "'/'",
            Token::Bang => "'!'",
            Token::Equals => "'='",
            Token::Lt => "'<'",
            Token::Gt => "'>'",
            Token::LParen => "'('",
            Token::RParen => "')'",
            Token::LBrace => "'{'",
            Token::RBrace => "'}'",
            Token::LBracket => "'['",
            Token::RBracket => "']'",
            Token::Comma => "','",
            Token::Semicolon => "';'",
            Token::Dot => "'.'",
            Token::Colon => "':'",
            Token::DoublePipe => "'||'",
            Token::Pipe => "'|'",
            Token::Hash => "'#'",
            Token::MaybeMatch => "'?='",
            Token::Question => "'?'",
            Token::MacroFunctionName => "?FUNCTION_NAME",
            Token::MacroFunctionArity => "?FUNCTION_ARITY",
        }
    }
}

/// Whether an atom needs quotes to remain an atom in Erlang source.
pub fn atom_needs_quoting(text: &str) -> bool {
    let mut chars = text.chars();
    let identifier = chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@');
    let reserved = matches!(text, "after" | "begin" | "case" | "try"
        | "cond" | "catch" | "andalso" | "orelse" | "end" | "fun" | "if"
        | "let" | "of" | "receive" | "when" | "bnot" | "not" | "div"
        | "rem" | "band" | "and" | "bor" | "bxor" | "bsl" | "bsr"
        | "or" | "xor" | "maybe" | "else");
    !identifier || reserved
}

fn scan_triple_string(lex: &mut logos::Lexer<'_, Token>) -> Result<(), ()> {
    let end = lex.remainder().find("\"\"\"").ok_or(())?;
    lex.bump(end + 3);
    Ok(())
}
