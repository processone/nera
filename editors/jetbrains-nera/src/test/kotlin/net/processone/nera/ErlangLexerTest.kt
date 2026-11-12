package net.processone.nera

import com.intellij.psi.tree.IElementType
import org.junit.Assert.assertEquals
import org.junit.Test

class ErlangLexerTest {

    data class Tok(val type: IElementType, val text: String)

    private fun lex(input: String): List<Tok> {
        val lexer = ErlangLexer()
        lexer.start(input, 0, input.length, 0)
        val tokens = mutableListOf<Tok>()
        while (lexer.tokenType != null) {
            tokens.add(Tok(lexer.tokenType!!, input.substring(lexer.tokenStart, lexer.tokenEnd)))
            lexer.advance()
        }
        return tokens
    }

    /** Filter out whitespace for easier assertion. */
    private fun lexNoWs(input: String): List<Tok> =
        lex(input).filter { it.type != ErlangTokenTypes.WHITESPACE }

    @Test
    fun `test comment`() {
        val tokens = lexNoWs("% this is a comment")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.COMMENT, tokens[0].type)
        assertEquals("% this is a comment", tokens[0].text)
    }

    @Test
    fun `test string`() {
        val tokens = lexNoWs("\"hello world\"")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.STRING, tokens[0].type)
        assertEquals("\"hello world\"", tokens[0].text)
    }

    @Test
    fun `test string with escape`() {
        val tokens = lexNoWs("\"hello\\nworld\"")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.STRING, tokens[0].type)
    }

    @Test
    fun `test triple string`() {
        val tokens = lexNoWs("\"\"\"hello\nworld\"\"\"")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.TRIPLE_STRING, tokens[0].type)
    }

    @Test
    fun `test quoted atom`() {
        val tokens = lexNoWs("'hello world'")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.QUOTED_ATOM, tokens[0].type)
    }

    @Test
    fun `test quoted atom with escape`() {
        val tokens = lexNoWs("'it\\'s'")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.QUOTED_ATOM, tokens[0].type)
        assertEquals("'it\\'s'", tokens[0].text)
    }

    @Test
    fun `test character literal`() {
        val tokens = lexNoWs("\$a")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.CHAR_LITERAL, tokens[0].type)
    }

    @Test
    fun `test character escape`() {
        val tokens = lexNoWs("\$\\n")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.CHAR_LITERAL, tokens[0].type)
        assertEquals("\$\\n", tokens[0].text)
    }

    @Test
    fun `test integer`() {
        val tokens = lexNoWs("42")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.INTEGER, tokens[0].type)
    }

    @Test
    fun `test base integer`() {
        val tokens = lexNoWs("16#FF")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.INTEGER, tokens[0].type)
        assertEquals("16#FF", tokens[0].text)
    }

    @Test
    fun `test binary integer`() {
        val tokens = lexNoWs("2#1010")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.INTEGER, tokens[0].type)
    }

    @Test
    fun `test float`() {
        val tokens = lexNoWs("3.14")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.FLOAT, tokens[0].type)
    }

    @Test
    fun `test float with exponent`() {
        val tokens = lexNoWs("1.0e-10")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.FLOAT, tokens[0].type)
        assertEquals("1.0e-10", tokens[0].text)
    }

    @Test
    fun `test variable`() {
        val tokens = lexNoWs("MyVar")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.VARIABLE, tokens[0].type)
    }

    @Test
    fun `test underscore variable`() {
        val tokens = lexNoWs("_Unused")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.VARIABLE, tokens[0].type)
    }

    @Test
    fun `test anonymous variable`() {
        val tokens = lexNoWs("_")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.VARIABLE, tokens[0].type)
    }

    @Test
    fun `test atom`() {
        val tokens = lexNoWs("hello")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.ATOM, tokens[0].type)
    }

    @Test
    fun `test keywords`() {
        for (kw in listOf("case", "of", "end", "if", "receive", "after",
                          "try", "catch", "begin", "fun", "when", "maybe", "else")) {
            val tokens = lexNoWs(kw)
            assertEquals("$kw should be KEYWORD", ErlangTokenTypes.KEYWORD, tokens[0].type)
        }
    }

    @Test
    fun `test language constants`() {
        for (c in listOf("true", "false", "undefined")) {
            val tokens = lexNoWs(c)
            assertEquals("$c should be LANGUAGE_CONSTANT", ErlangTokenTypes.LANGUAGE_CONSTANT, tokens[0].type)
        }
    }

    @Test
    fun `test operator keywords`() {
        for (kw in listOf("andalso", "orelse", "not", "and", "or",
                          "band", "bor", "bxor", "bnot", "bsl", "bsr", "div", "rem")) {
            val tokens = lexNoWs(kw)
            assertEquals("$kw should be OPERATOR_KEYWORD", ErlangTokenTypes.OPERATOR_KEYWORD, tokens[0].type)
        }
    }

    @Test
    fun `test macro`() {
        val tokens = lexNoWs("?MODULE")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.MACRO, tokens[0].type)
        assertEquals("?MODULE", tokens[0].text)
    }

    @Test
    fun `test double macro`() {
        val tokens = lexNoWs("??VAR")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.MACRO, tokens[0].type)
        assertEquals("??VAR", tokens[0].text)
    }

    @Test
    fun `test attribute`() {
        val tokens = lexNoWs("-module")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.ATTRIBUTE, tokens[0].type)
        assertEquals("-module", tokens[0].text)
    }

    @Test
    fun `test arrow`() {
        val tokens = lexNoWs("->")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.ARROW, tokens[0].type)
    }

    @Test
    fun `test fat arrow`() {
        val tokens = lexNoWs("=>")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.FAT_ARROW, tokens[0].type)
    }

    @Test
    fun `test walrus`() {
        val tokens = lexNoWs(":=")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.WALRUS, tokens[0].type)
    }

    @Test
    fun `test binary brackets`() {
        val tokens = lexNoWs("<<>>")
        assertEquals(2, tokens.size)
        assertEquals(ErlangTokenTypes.LBIN, tokens[0].type)
        assertEquals(ErlangTokenTypes.RBIN, tokens[1].type)
    }

    @Test
    fun `test double colon`() {
        val tokens = lexNoWs("::")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.DOUBLE_COLON, tokens[0].type)
    }

    @Test
    fun `test three char operators`() {
        assertEquals(ErlangTokenTypes.OPERATOR, lexNoWs("=:=")[0].type)
        assertEquals(ErlangTokenTypes.OPERATOR, lexNoWs("=/=")[0].type)
    }

    @Test
    fun `test two char operators`() {
        assertEquals(ErlangTokenTypes.OPERATOR, lexNoWs("++")[0].type)
        assertEquals(ErlangTokenTypes.OPERATOR, lexNoWs("--")[0].type)
        assertEquals(ErlangTokenTypes.OPERATOR, lexNoWs("==")[0].type)
        assertEquals(ErlangTokenTypes.OPERATOR, lexNoWs("/=")[0].type)
        assertEquals(ErlangTokenTypes.OPERATOR, lexNoWs(">=")[0].type)
        assertEquals(ErlangTokenTypes.OPERATOR, lexNoWs("=<")[0].type)
    }

    @Test
    fun `test punctuation`() {
        assertEquals(ErlangTokenTypes.LPAREN, lexNoWs("(")[0].type)
        assertEquals(ErlangTokenTypes.RPAREN, lexNoWs(")")[0].type)
        assertEquals(ErlangTokenTypes.LBRACKET, lexNoWs("[")[0].type)
        assertEquals(ErlangTokenTypes.RBRACKET, lexNoWs("]")[0].type)
        assertEquals(ErlangTokenTypes.LBRACE, lexNoWs("{")[0].type)
        assertEquals(ErlangTokenTypes.RBRACE, lexNoWs("}")[0].type)
        assertEquals(ErlangTokenTypes.COMMA, lexNoWs(",")[0].type)
        assertEquals(ErlangTokenTypes.SEMICOLON, lexNoWs(";")[0].type)
        assertEquals(ErlangTokenTypes.DOT, lexNoWs(".")[0].type)
        assertEquals(ErlangTokenTypes.PIPE, lexNoWs("|")[0].type)
        assertEquals(ErlangTokenTypes.DOUBLE_PIPE, lexNoWs("||")[0].type)
        assertEquals(ErlangTokenTypes.COLON, lexNoWs(":")[0].type)
        assertEquals(ErlangTokenTypes.HASH, lexNoWs("#")[0].type)
    }

    @Test
    fun `test function definition`() {
        val tokens = lexNoWs("hello(X) -> ok.")
        assertEquals(listOf(
            Tok(ErlangTokenTypes.ATOM, "hello"),
            Tok(ErlangTokenTypes.LPAREN, "("),
            Tok(ErlangTokenTypes.VARIABLE, "X"),
            Tok(ErlangTokenTypes.RPAREN, ")"),
            Tok(ErlangTokenTypes.ARROW, "->"),
            Tok(ErlangTokenTypes.ATOM, "ok"),
            Tok(ErlangTokenTypes.DOT, ".")
        ), tokens)
    }

    @Test
    fun `test module directive`() {
        val tokens = lexNoWs("-module(test).")
        assertEquals(listOf(
            Tok(ErlangTokenTypes.ATTRIBUTE, "-module"),
            Tok(ErlangTokenTypes.LPAREN, "("),
            Tok(ErlangTokenTypes.ATOM, "test"),
            Tok(ErlangTokenTypes.RPAREN, ")"),
            Tok(ErlangTokenTypes.DOT, ".")
        ), tokens)
    }

    @Test
    fun `test case expression`() {
        val tokens = lexNoWs("case X of\n  1 -> ok\nend")
        val types = tokens.map { it.type }
        assertEquals(listOf(
            ErlangTokenTypes.KEYWORD,   // case
            ErlangTokenTypes.VARIABLE,  // X
            ErlangTokenTypes.KEYWORD,   // of
            ErlangTokenTypes.INTEGER,   // 1
            ErlangTokenTypes.ARROW,     // ->
            ErlangTokenTypes.ATOM,      // ok
            ErlangTokenTypes.KEYWORD    // end
        ), types)
    }

    @Test
    fun `test remote call`() {
        val tokens = lexNoWs("io:format(\"hello\")")
        assertEquals(listOf(
            Tok(ErlangTokenTypes.ATOM, "io"),
            Tok(ErlangTokenTypes.COLON, ":"),
            Tok(ErlangTokenTypes.ATOM, "format"),
            Tok(ErlangTokenTypes.LPAREN, "("),
            Tok(ErlangTokenTypes.STRING, "\"hello\""),
            Tok(ErlangTokenTypes.RPAREN, ")")
        ), tokens)
    }

    @Test
    fun `test record access`() {
        val tokens = lexNoWs("X#rec.field")
        assertEquals(listOf(
            Tok(ErlangTokenTypes.VARIABLE, "X"),
            Tok(ErlangTokenTypes.HASH, "#"),
            Tok(ErlangTokenTypes.ATOM, "rec"),
            Tok(ErlangTokenTypes.DOT, "."),
            Tok(ErlangTokenTypes.ATOM, "field")
        ), tokens)
    }

    @Test
    fun `test complete coverage with no gaps`() {
        val input = "-module(test).\n\nhello(X) when is_atom(X) ->\n    io:format(\"~p~n\", [X]).\n"
        val tokens = lex(input)
        // Verify tokens cover the entire input with no gaps
        var pos = 0
        for (tok in tokens) {
            assertEquals("Gap before token '${tok.text}' at position $pos", pos, input.indexOf(tok.text, pos))
            pos += tok.text.length
        }
        assertEquals("Tokens should cover entire input", input.length, pos)
    }

    @Test
    fun `test sigil string`() {
        val tokens = lexNoWs("~\"hello\"")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.STRING, tokens[0].type)
    }

    @Test
    fun `test sigil string with modifier`() {
        val tokens = lexNoWs("~b\"hello\"")
        assertEquals(1, tokens.size)
        assertEquals(ErlangTokenTypes.STRING, tokens[0].type)
    }

    @Test
    fun `test list comprehension`() {
        val tokens = lexNoWs("[X || X <- L]")
        val types = tokens.map { it.type }
        assertEquals(listOf(
            ErlangTokenTypes.LBRACKET,
            ErlangTokenTypes.VARIABLE,     // X
            ErlangTokenTypes.DOUBLE_PIPE,  // ||
            ErlangTokenTypes.VARIABLE,     // X
            ErlangTokenTypes.OPERATOR,     // <
            ErlangTokenTypes.OPERATOR,     // -
            ErlangTokenTypes.VARIABLE,     // L
            ErlangTokenTypes.RBRACKET
        ), types)
    }
}
