package net.processone.nera

enum class TokenType {
    // Keywords
    CASE, OF, END, IF, RECEIVE, AFTER, TRY, CATCH, BEGIN, FUN, WHEN, MAYBE,

    // Arrows and operators
    ARROW,       // ->

    // Separators
    COMMA,       // ,
    SEMICOLON,   // ;
    DOT,         // .

    // Brackets
    LPAREN, RPAREN,     // ( )
    LBRACKET, RBRACKET, // [ ]
    LBRACE, RBRACE,     // { }
    LBIN, RBIN,         // << >>

    // Identifiers
    ATOM,        // lower-case identifier or quoted atom
    VARIABLE,    // upper-case identifier or _

    // Attribute
    ATTRIBUTE,   // -module, -export, etc.

    // Other
    OTHER
}

data class Token(
    val type: TokenType,
    val text: String,
    val line: Int,
    val column: Int
)

class ErlangTokenizer(private val text: String) {

    private var pos = 0
    private var line = 0
    private var col = 0

    fun tokenize(): List<Token> {
        val tokens = mutableListOf<Token>()
        while (pos < text.length) {
            skipWhitespace()
            if (pos >= text.length) break

            val ch = text[pos]

            // Comment: skip to end of line
            if (ch == '%') {
                skipToEndOfLine()
                continue
            }

            // String literal: skip content
            if (ch == '"') {
                skipString('"')
                continue
            }

            // Quoted atom: skip content
            if (ch == '\'') {
                skipString('\'')
                continue
            }

            val startLine = line
            val startCol = col

            // Arrow ->
            if (ch == '-' && peek(1) == '>') {
                tokens.add(Token(TokenType.ARROW, "->", startLine, startCol))
                advance(2)
                continue
            }

            // Attribute: - at start of line followed by a letter
            if (ch == '-' && startCol == 0 && pos + 1 < text.length && text[pos + 1].isLetter()) {
                val sb = StringBuilder("-")
                advance(1)
                while (pos < text.length && (text[pos].isLetterOrDigit() || text[pos] == '_')) {
                    sb.append(text[pos])
                    advance(1)
                }
                tokens.add(Token(TokenType.ATTRIBUTE, sb.toString(), startLine, startCol))
                continue
            }

            // Binary brackets << >>
            if (ch == '<' && peek(1) == '<') {
                tokens.add(Token(TokenType.LBIN, "<<", startLine, startCol))
                advance(2)
                continue
            }
            if (ch == '>' && peek(1) == '>') {
                tokens.add(Token(TokenType.RBIN, ">>", startLine, startCol))
                advance(2)
                continue
            }

            // Single-char tokens
            val singleType = when (ch) {
                '(' -> TokenType.LPAREN
                ')' -> TokenType.RPAREN
                '[' -> TokenType.LBRACKET
                ']' -> TokenType.RBRACKET
                '{' -> TokenType.LBRACE
                '}' -> TokenType.RBRACE
                ',' -> TokenType.COMMA
                ';' -> TokenType.SEMICOLON
                '.' -> TokenType.DOT
                else -> null
            }
            if (singleType != null) {
                tokens.add(Token(singleType, ch.toString(), startLine, startCol))
                advance(1)
                continue
            }

            // Identifier or keyword
            if (ch.isLetter() || ch == '_') {
                val sb = StringBuilder()
                while (pos < text.length && (text[pos].isLetterOrDigit() || text[pos] == '_' || text[pos] == '@')) {
                    sb.append(text[pos])
                    advance(1)
                }
                val word = sb.toString()
                val type = when (word) {
                    "case" -> TokenType.CASE
                    "of" -> TokenType.OF
                    "end" -> TokenType.END
                    "if" -> TokenType.IF
                    "receive" -> TokenType.RECEIVE
                    "after" -> TokenType.AFTER
                    "try" -> TokenType.TRY
                    "catch" -> TokenType.CATCH
                    "begin" -> TokenType.BEGIN
                    "fun" -> TokenType.FUN
                    "when" -> TokenType.WHEN
                    "maybe" -> TokenType.MAYBE
                    else -> if (ch.isUpperCase() || ch == '_') TokenType.VARIABLE else TokenType.ATOM
                }
                tokens.add(Token(type, word, startLine, startCol))
                continue
            }

            // Skip any other character
            advance(1)
        }
        return tokens
    }

    private fun peek(offset: Int): Char? {
        val idx = pos + offset
        return if (idx < text.length) text[idx] else null
    }

    private fun advance(count: Int) {
        repeat(count) {
            if (pos < text.length) {
                if (text[pos] == '\n') {
                    line++
                    col = 0
                } else {
                    col++
                }
                pos++
            }
        }
    }

    private fun skipWhitespace() {
        while (pos < text.length && text[pos].isWhitespace()) {
            advance(1)
        }
    }

    private fun skipToEndOfLine() {
        while (pos < text.length && text[pos] != '\n') {
            advance(1)
        }
    }

    private fun skipString(quote: Char) {
        advance(1) // skip opening quote
        while (pos < text.length) {
            if (text[pos] == '\\') {
                advance(2) // skip escaped char
            } else if (text[pos] == quote) {
                advance(1) // skip closing quote
                return
            } else {
                advance(1)
            }
        }
    }
}
