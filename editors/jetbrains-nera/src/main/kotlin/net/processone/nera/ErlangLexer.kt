package net.processone.nera

import com.intellij.lexer.LexerBase
import com.intellij.psi.TokenType
import com.intellij.psi.tree.IElementType

object ErlangTokenTypes {
    @JvmField val WHITESPACE = TokenType.WHITE_SPACE
    @JvmField val BAD_CHARACTER = TokenType.BAD_CHARACTER

    @JvmField val COMMENT = IElementType("COMMENT", ErlangLanguage)

    // Literals
    @JvmField val STRING = IElementType("STRING", ErlangLanguage)
    @JvmField val TRIPLE_STRING = IElementType("TRIPLE_STRING", ErlangLanguage)
    @JvmField val QUOTED_ATOM = IElementType("QUOTED_ATOM", ErlangLanguage)
    @JvmField val CHAR_LITERAL = IElementType("CHAR_LITERAL", ErlangLanguage)
    @JvmField val INTEGER = IElementType("INTEGER", ErlangLanguage)
    @JvmField val FLOAT = IElementType("FLOAT", ErlangLanguage)

    // Identifiers
    @JvmField val ATOM = IElementType("ATOM", ErlangLanguage)
    @JvmField val VARIABLE = IElementType("VARIABLE", ErlangLanguage)
    @JvmField val MACRO = IElementType("MACRO", ErlangLanguage)

    // Keywords
    @JvmField val KEYWORD = IElementType("KEYWORD", ErlangLanguage)
    @JvmField val OPERATOR_KEYWORD = IElementType("OPERATOR_KEYWORD", ErlangLanguage)
    @JvmField val LANGUAGE_CONSTANT = IElementType("LANGUAGE_CONSTANT", ErlangLanguage)

    // Attribute/directive
    @JvmField val ATTRIBUTE = IElementType("ATTRIBUTE", ErlangLanguage)

    // Arrows
    @JvmField val ARROW = IElementType("ARROW", ErlangLanguage)           // ->
    @JvmField val FAT_ARROW = IElementType("FAT_ARROW", ErlangLanguage)   // =>
    @JvmField val WALRUS = IElementType("WALRUS", ErlangLanguage)         // :=

    // Brackets
    @JvmField val LPAREN = IElementType("LPAREN", ErlangLanguage)
    @JvmField val RPAREN = IElementType("RPAREN", ErlangLanguage)
    @JvmField val LBRACKET = IElementType("LBRACKET", ErlangLanguage)
    @JvmField val RBRACKET = IElementType("RBRACKET", ErlangLanguage)
    @JvmField val LBRACE = IElementType("LBRACE", ErlangLanguage)
    @JvmField val RBRACE = IElementType("RBRACE", ErlangLanguage)
    @JvmField val LBIN = IElementType("LBIN", ErlangLanguage)             // <<
    @JvmField val RBIN = IElementType("RBIN", ErlangLanguage)             // >>

    // Punctuation
    @JvmField val COMMA = IElementType("COMMA", ErlangLanguage)
    @JvmField val SEMICOLON = IElementType("SEMICOLON", ErlangLanguage)
    @JvmField val DOT = IElementType("DOT", ErlangLanguage)
    @JvmField val PIPE = IElementType("PIPE", ErlangLanguage)
    @JvmField val DOUBLE_PIPE = IElementType("DOUBLE_PIPE", ErlangLanguage)
    @JvmField val COLON = IElementType("COLON", ErlangLanguage)
    @JvmField val DOUBLE_COLON = IElementType("DOUBLE_COLON", ErlangLanguage)
    @JvmField val HASH = IElementType("HASH", ErlangLanguage)

    // Generic operator
    @JvmField val OPERATOR = IElementType("OPERATOR", ErlangLanguage)

    private val KEYWORDS = setOf(
        "after", "begin", "case", "catch", "cond", "else",
        "end", "fun", "if", "let", "maybe", "of",
        "receive", "try", "when"
    )

    private val OPERATOR_KEYWORDS = setOf(
        "andalso", "orelse",
        "not", "and", "or",
        "band", "bor", "bxor", "bnot", "bsl", "bsr",
        "div", "rem"
    )

    private val LANGUAGE_CONSTANTS = setOf("true", "false", "undefined")

    fun classifyWord(word: String): IElementType = when {
        word in LANGUAGE_CONSTANTS -> LANGUAGE_CONSTANT
        word in KEYWORDS -> KEYWORD
        word in OPERATOR_KEYWORDS -> OPERATOR_KEYWORD
        else -> ATOM
    }
}

class ErlangLexer : LexerBase() {

    private var buffer: CharSequence = ""
    private var bufferEnd = 0
    private var tokenStart = 0
    private var tokenEnd = 0
    private var tokenType: IElementType? = null

    override fun start(buffer: CharSequence, startOffset: Int, endOffset: Int, initialState: Int) {
        this.buffer = buffer
        this.bufferEnd = endOffset
        this.tokenStart = startOffset
        this.tokenEnd = startOffset
        this.tokenType = null
        advance()
    }

    override fun getState(): Int = 0
    override fun getTokenType(): IElementType? = tokenType
    override fun getTokenStart(): Int = tokenStart
    override fun getTokenEnd(): Int = tokenEnd
    override fun getBufferSequence(): CharSequence = buffer
    override fun getBufferEnd(): Int = bufferEnd

    override fun advance() {
        tokenStart = tokenEnd
        if (tokenStart >= bufferEnd) {
            tokenType = null
            return
        }

        val ch = buffer[tokenStart]

        // Whitespace
        if (ch.isWhitespace()) {
            tokenEnd = tokenStart + 1
            while (tokenEnd < bufferEnd && buffer[tokenEnd].isWhitespace()) tokenEnd++
            tokenType = ErlangTokenTypes.WHITESPACE
            return
        }

        // Comment
        if (ch == '%') {
            tokenEnd = tokenStart + 1
            while (tokenEnd < bufferEnd && buffer[tokenEnd] != '\n') tokenEnd++
            tokenType = ErlangTokenTypes.COMMENT
            return
        }

        // Triple-quoted string
        if (ch == '"' && lookAhead(1) == '"' && lookAhead(2) == '"') {
            lexTripleString()
            return
        }

        // String
        if (ch == '"') {
            lexQuotedSequence('"', ErlangTokenTypes.STRING)
            return
        }

        // Quoted atom
        if (ch == '\'') {
            lexQuotedSequence('\'', ErlangTokenTypes.QUOTED_ATOM)
            return
        }

        // Character literal
        if (ch == '$') {
            lexCharLiteral()
            return
        }

        // Macro: ?Name or ??Name
        if (ch == '?') {
            lexMacro()
            return
        }

        // Number
        if (ch.isDigit()) {
            lexNumber()
            return
        }

        // Sigil string: ~b"..." or ~s"..." or ~"..."
        if (ch == '~' && isSigilStart()) {
            lexSigilString()
            return
        }

        // Variable (uppercase or _)
        if (ch.isUpperCase() || (ch == '_' && tokenStart + 1 < bufferEnd && buffer[tokenStart + 1].isLetterOrDigit())) {
            lexIdentifier(ErlangTokenTypes.VARIABLE)
            return
        }

        // Lone underscore
        if (ch == '_') {
            tokenEnd = tokenStart + 1
            tokenType = ErlangTokenTypes.VARIABLE
            return
        }

        // Atom or keyword (lowercase start)
        if (ch.isLowerCase()) {
            lexWord()
            return
        }

        // Attribute: - at start of a line followed by a lowercase letter
        if (ch == '-' && isAtLineStart() && tokenStart + 1 < bufferEnd && buffer[tokenStart + 1].isLowerCase()) {
            lexAttribute()
            return
        }

        // Multi-char operators (must check before single-char)
        if (lexMultiCharOperator()) return

        // Single-char tokens
        val singleType = when (ch) {
            '(' -> ErlangTokenTypes.LPAREN
            ')' -> ErlangTokenTypes.RPAREN
            '[' -> ErlangTokenTypes.LBRACKET
            ']' -> ErlangTokenTypes.RBRACKET
            '{' -> ErlangTokenTypes.LBRACE
            '}' -> ErlangTokenTypes.RBRACE
            ',' -> ErlangTokenTypes.COMMA
            ';' -> ErlangTokenTypes.SEMICOLON
            '.' -> ErlangTokenTypes.DOT
            '#' -> ErlangTokenTypes.HASH
            '!' -> ErlangTokenTypes.OPERATOR
            '+' -> ErlangTokenTypes.OPERATOR
            '*' -> ErlangTokenTypes.OPERATOR
            '/' -> ErlangTokenTypes.OPERATOR
            '-' -> ErlangTokenTypes.OPERATOR
            '=' -> ErlangTokenTypes.OPERATOR
            '<' -> ErlangTokenTypes.OPERATOR
            '>' -> ErlangTokenTypes.OPERATOR
            '~' -> ErlangTokenTypes.OPERATOR
            '\\' -> ErlangTokenTypes.OPERATOR
            else -> ErlangTokenTypes.BAD_CHARACTER
        }
        tokenEnd = tokenStart + 1
        tokenType = singleType
    }

    // --- Lexing helpers ---

    private fun lookAhead(offset: Int): Char? {
        val idx = tokenStart + offset
        return if (idx < bufferEnd) buffer[idx] else null
    }

    private fun isAtLineStart(): Boolean {
        if (tokenStart == 0) return true
        for (i in tokenStart - 1 downTo 0) {
            val c = buffer[i]
            if (c == '\n') return true
            if (!c.isWhitespace()) return false
        }
        return true
    }

    private fun isSigilStart(): Boolean {
        val next = lookAhead(1) ?: return false
        if (next == '"') return true
        if ((next == 'b' || next == 's' || next == 'B' || next == 'S') && lookAhead(2) == '"') return true
        // Sigil with triple quotes
        if (next == '"' && lookAhead(2) == '"' && lookAhead(3) == '"') return true
        if ((next == 'b' || next == 's' || next == 'B' || next == 'S') && lookAhead(2) == '"' && lookAhead(3) == '"' && lookAhead(4) == '"') return true
        return false
    }

    private fun lexTripleString() {
        tokenEnd = tokenStart + 3 // skip opening """
        while (tokenEnd < bufferEnd) {
            if (buffer[tokenEnd] == '"' && tokenEnd + 2 < bufferEnd && buffer[tokenEnd + 1] == '"' && buffer[tokenEnd + 2] == '"') {
                tokenEnd += 3 // skip closing """
                break
            }
            if (buffer[tokenEnd] == '\\' && tokenEnd + 1 < bufferEnd) {
                tokenEnd += 2
            } else {
                tokenEnd++
            }
        }
        tokenType = ErlangTokenTypes.TRIPLE_STRING
    }

    private fun lexQuotedSequence(quote: Char, type: IElementType) {
        tokenEnd = tokenStart + 1 // skip opening quote
        while (tokenEnd < bufferEnd) {
            val c = buffer[tokenEnd]
            if (c == '\\' && tokenEnd + 1 < bufferEnd) {
                tokenEnd += 2
            } else if (c == quote) {
                tokenEnd++
                break
            } else {
                tokenEnd++
            }
        }
        tokenType = type
    }

    private fun lexCharLiteral() {
        tokenEnd = tokenStart + 1 // skip $
        if (tokenEnd >= bufferEnd) {
            tokenType = ErlangTokenTypes.CHAR_LITERAL
            return
        }
        if (buffer[tokenEnd] == '\\' && tokenEnd + 1 < bufferEnd) {
            tokenEnd += 2
            // Handle \xNN, \x{NNNN}
            if (tokenEnd <= bufferEnd && buffer[tokenEnd - 1] == 'x') {
                if (tokenEnd < bufferEnd && buffer[tokenEnd] == '{') {
                    tokenEnd++
                    while (tokenEnd < bufferEnd && buffer[tokenEnd] != '}') tokenEnd++
                    if (tokenEnd < bufferEnd) tokenEnd++ // skip }
                } else {
                    // up to 2 hex digits
                    repeat(2) { if (tokenEnd < bufferEnd && buffer[tokenEnd].isHexDigit()) tokenEnd++ }
                }
            }
        } else {
            tokenEnd++ // single character
        }
        tokenType = ErlangTokenTypes.CHAR_LITERAL
    }

    private fun lexMacro() {
        tokenEnd = tokenStart + 1 // skip ?
        if (tokenEnd < bufferEnd && buffer[tokenEnd] == '?') tokenEnd++ // skip ??
        // Read the macro name
        while (tokenEnd < bufferEnd && (buffer[tokenEnd].isLetterOrDigit() || buffer[tokenEnd] == '_')) {
            tokenEnd++
        }
        tokenType = ErlangTokenTypes.MACRO
    }

    private fun lexNumber() {
        tokenEnd = tokenStart
        // Read digits
        while (tokenEnd < bufferEnd && buffer[tokenEnd].isDigit()) tokenEnd++

        // Check for Base#Value notation
        if (tokenEnd < bufferEnd && buffer[tokenEnd] == '#') {
            tokenEnd++ // skip #
            while (tokenEnd < bufferEnd && (buffer[tokenEnd].isLetterOrDigit() || buffer[tokenEnd] == '_')) {
                tokenEnd++
            }
            tokenType = ErlangTokenTypes.INTEGER
            return
        }

        // Check for float: digits followed by . and digit
        if (tokenEnd < bufferEnd && buffer[tokenEnd] == '.' && tokenEnd + 1 < bufferEnd && buffer[tokenEnd + 1].isDigit()) {
            tokenEnd++ // skip .
            while (tokenEnd < bufferEnd && buffer[tokenEnd].isDigit()) tokenEnd++
            // Exponent part
            if (tokenEnd < bufferEnd && (buffer[tokenEnd] == 'e' || buffer[tokenEnd] == 'E')) {
                tokenEnd++
                if (tokenEnd < bufferEnd && (buffer[tokenEnd] == '+' || buffer[tokenEnd] == '-')) tokenEnd++
                while (tokenEnd < bufferEnd && buffer[tokenEnd].isDigit()) tokenEnd++
            }
            tokenType = ErlangTokenTypes.FLOAT
            return
        }

        // Allow underscore separators in integer literals
        while (tokenEnd < bufferEnd && buffer[tokenEnd] == '_' && tokenEnd + 1 < bufferEnd && buffer[tokenEnd + 1].isDigit()) {
            tokenEnd++ // skip _
            while (tokenEnd < bufferEnd && buffer[tokenEnd].isDigit()) tokenEnd++
        }

        tokenType = ErlangTokenTypes.INTEGER
    }

    private fun lexSigilString() {
        tokenEnd = tokenStart + 1 // skip ~
        // Skip optional modifier (b/s/B/S)
        if (tokenEnd < bufferEnd && buffer[tokenEnd] != '"') tokenEnd++
        // Now at the opening quote(s)
        if (tokenEnd + 2 < bufferEnd && buffer[tokenEnd] == '"' && buffer[tokenEnd + 1] == '"' && buffer[tokenEnd + 2] == '"') {
            // Triple-quoted sigil
            tokenEnd += 3
            while (tokenEnd < bufferEnd) {
                if (buffer[tokenEnd] == '"' && tokenEnd + 2 < bufferEnd && buffer[tokenEnd + 1] == '"' && buffer[tokenEnd + 2] == '"') {
                    tokenEnd += 3
                    break
                }
                tokenEnd++
            }
        } else {
            // Single-quoted sigil
            tokenEnd++ // skip "
            while (tokenEnd < bufferEnd) {
                if (buffer[tokenEnd] == '\\' && tokenEnd + 1 < bufferEnd) {
                    tokenEnd += 2
                } else if (buffer[tokenEnd] == '"') {
                    tokenEnd++
                    break
                } else {
                    tokenEnd++
                }
            }
        }
        tokenType = ErlangTokenTypes.STRING
    }

    private fun lexIdentifier(type: IElementType) {
        tokenEnd = tokenStart + 1
        while (tokenEnd < bufferEnd && (buffer[tokenEnd].isLetterOrDigit() || buffer[tokenEnd] == '_' || buffer[tokenEnd] == '@')) {
            tokenEnd++
        }
        tokenType = type
    }

    private fun lexWord() {
        tokenEnd = tokenStart + 1
        while (tokenEnd < bufferEnd && (buffer[tokenEnd].isLetterOrDigit() || buffer[tokenEnd] == '_' || buffer[tokenEnd] == '@')) {
            tokenEnd++
        }
        val word = buffer.subSequence(tokenStart, tokenEnd).toString()
        tokenType = ErlangTokenTypes.classifyWord(word)
    }

    private fun lexAttribute() {
        tokenEnd = tokenStart + 1 // skip -
        while (tokenEnd < bufferEnd && (buffer[tokenEnd].isLetterOrDigit() || buffer[tokenEnd] == '_')) {
            tokenEnd++
        }
        tokenType = ErlangTokenTypes.ATTRIBUTE
    }

    private fun lexMultiCharOperator(): Boolean {
        val ch = buffer[tokenStart]
        val next = lookAhead(1)
        val next2 = lookAhead(2)

        // Three-char operators
        if (next != null && next2 != null) {
            val three = "$ch$next$next2"
            val threeType = when (three) {
                "=:=" -> ErlangTokenTypes.OPERATOR
                "=/=" -> ErlangTokenTypes.OPERATOR
                else -> null
            }
            if (threeType != null) {
                tokenEnd = tokenStart + 3
                tokenType = threeType
                return true
            }
        }

        // Two-char operators
        if (next != null) {
            val two = "$ch$next"
            val twoType = when (two) {
                "->" -> ErlangTokenTypes.ARROW
                "=>" -> ErlangTokenTypes.FAT_ARROW
                ":=" -> ErlangTokenTypes.WALRUS
                "::" -> ErlangTokenTypes.DOUBLE_COLON
                "++" -> ErlangTokenTypes.OPERATOR
                "--" -> ErlangTokenTypes.OPERATOR
                "==" -> ErlangTokenTypes.OPERATOR
                "/=" -> ErlangTokenTypes.OPERATOR
                ">=" -> ErlangTokenTypes.OPERATOR
                "=<" -> ErlangTokenTypes.OPERATOR
                "<<" -> ErlangTokenTypes.LBIN
                ">>" -> ErlangTokenTypes.RBIN
                "||" -> ErlangTokenTypes.DOUBLE_PIPE
                else -> null
            }
            if (twoType != null) {
                tokenEnd = tokenStart + 2
                tokenType = twoType
                return true
            }
        }

        // Single colon and pipe (not matched above)
        if (ch == ':') {
            tokenEnd = tokenStart + 1
            tokenType = ErlangTokenTypes.COLON
            return true
        }
        if (ch == '|') {
            tokenEnd = tokenStart + 1
            tokenType = ErlangTokenTypes.PIPE
            return true
        }

        return false
    }

    private fun Char.isHexDigit(): Boolean =
        this in '0'..'9' || this in 'a'..'f' || this in 'A'..'F'
}
