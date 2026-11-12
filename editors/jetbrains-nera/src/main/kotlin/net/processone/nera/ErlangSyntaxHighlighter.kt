package net.processone.nera

import com.intellij.lexer.Lexer
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.editor.colors.TextAttributesKey.createTextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.fileTypes.SyntaxHighlighterBase
import com.intellij.openapi.fileTypes.SyntaxHighlighterFactory
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.tree.IElementType

class ErlangSyntaxHighlighter : SyntaxHighlighterBase() {

    companion object {
        val COMMENT_KEY = createTextAttributesKey("ERLANG_COMMENT", DefaultLanguageHighlighterColors.LINE_COMMENT)
        val STRING_KEY = createTextAttributesKey("ERLANG_STRING", DefaultLanguageHighlighterColors.STRING)
        val QUOTED_ATOM_KEY = createTextAttributesKey("ERLANG_QUOTED_ATOM", DefaultLanguageHighlighterColors.STRING)
        val CHAR_KEY = createTextAttributesKey("ERLANG_CHAR", DefaultLanguageHighlighterColors.STRING)
        val NUMBER_KEY = createTextAttributesKey("ERLANG_NUMBER", DefaultLanguageHighlighterColors.NUMBER)
        val KEYWORD_KEY = createTextAttributesKey("ERLANG_KEYWORD", DefaultLanguageHighlighterColors.KEYWORD)
        val LANGUAGE_CONSTANT_KEY = createTextAttributesKey("ERLANG_LANGUAGE_CONSTANT", DefaultLanguageHighlighterColors.KEYWORD)
        val ATOM_KEY = createTextAttributesKey("ERLANG_ATOM", DefaultLanguageHighlighterColors.CONSTANT)
        val VARIABLE_KEY = createTextAttributesKey("ERLANG_VARIABLE", DefaultLanguageHighlighterColors.LOCAL_VARIABLE)
        val MACRO_KEY = createTextAttributesKey("ERLANG_MACRO", DefaultLanguageHighlighterColors.CONSTANT)
        val ATTRIBUTE_KEY = createTextAttributesKey("ERLANG_ATTRIBUTE", DefaultLanguageHighlighterColors.METADATA)
        val OPERATOR_KEY = createTextAttributesKey("ERLANG_OPERATOR", DefaultLanguageHighlighterColors.OPERATION_SIGN)
        // Structural arrows (->, =>, :=) separate clause heads from bodies and
        // map keys from values. Defaulting to KEYWORD color makes them
        // visually belong with `case`/`of`/`end` etc. — users can override.
        val STRUCTURAL_ARROW_KEY = createTextAttributesKey("ERLANG_STRUCTURAL_ARROW", DefaultLanguageHighlighterColors.KEYWORD)
        val PAREN_KEY = createTextAttributesKey("ERLANG_PAREN", DefaultLanguageHighlighterColors.PARENTHESES)
        val BRACKET_KEY = createTextAttributesKey("ERLANG_BRACKET", DefaultLanguageHighlighterColors.BRACKETS)
        val BRACE_KEY = createTextAttributesKey("ERLANG_BRACE", DefaultLanguageHighlighterColors.BRACES)
        val COMMA_KEY = createTextAttributesKey("ERLANG_COMMA", DefaultLanguageHighlighterColors.COMMA)
        val SEMICOLON_KEY = createTextAttributesKey("ERLANG_SEMICOLON", DefaultLanguageHighlighterColors.SEMICOLON)
        val DOT_KEY = createTextAttributesKey("ERLANG_DOT", DefaultLanguageHighlighterColors.DOT)

        // Used by LSP semantic tokens for function names — color set via colorSchemes/*.xml
        val FUNCTION_KEY = createTextAttributesKey("ERLANG_FUNCTION", DefaultLanguageHighlighterColors.FUNCTION_DECLARATION)
        // Used by LSP semantic tokens for type names in -spec, -type, -opaque, -callback
        val TYPE_KEY = createTextAttributesKey("ERLANG_TYPE", DefaultLanguageHighlighterColors.CLASS_NAME)
        // Used by LSP semantic tokens for bound variables (pattern matching, not fresh binding)
        val BOUND_VARIABLE_KEY = createTextAttributesKey("ERLANG_BOUND_VARIABLE", DefaultLanguageHighlighterColors.LOCAL_VARIABLE)

        private val COMMENT_KEYS = arrayOf(COMMENT_KEY)
        private val STRING_KEYS = arrayOf(STRING_KEY)
        private val QUOTED_ATOM_KEYS = arrayOf(QUOTED_ATOM_KEY)
        private val CHAR_KEYS = arrayOf(CHAR_KEY)
        private val NUMBER_KEYS = arrayOf(NUMBER_KEY)
        private val KEYWORD_KEYS = arrayOf(KEYWORD_KEY)
        private val LANGUAGE_CONSTANT_KEYS = arrayOf(LANGUAGE_CONSTANT_KEY)
        private val ATOM_KEYS = arrayOf(ATOM_KEY)
        private val VARIABLE_KEYS = arrayOf(VARIABLE_KEY)
        private val MACRO_KEYS = arrayOf(MACRO_KEY)
        private val ATTRIBUTE_KEYS = arrayOf(ATTRIBUTE_KEY)
        private val OPERATOR_KEYS = arrayOf(OPERATOR_KEY)
        private val STRUCTURAL_ARROW_KEYS = arrayOf(STRUCTURAL_ARROW_KEY)
        private val PAREN_KEYS = arrayOf(PAREN_KEY)
        private val BRACKET_KEYS = arrayOf(BRACKET_KEY)
        private val BRACE_KEYS = arrayOf(BRACE_KEY)
        private val COMMA_KEYS = arrayOf(COMMA_KEY)
        private val SEMICOLON_KEYS = arrayOf(SEMICOLON_KEY)
        private val DOT_KEYS = arrayOf(DOT_KEY)
        private val EMPTY = emptyArray<TextAttributesKey>()
    }

    override fun getHighlightingLexer(): Lexer = ErlangLexer()

    override fun getTokenHighlights(tokenType: IElementType): Array<TextAttributesKey> = when (tokenType) {
        ErlangTokenTypes.COMMENT -> COMMENT_KEYS
        ErlangTokenTypes.STRING, ErlangTokenTypes.TRIPLE_STRING -> STRING_KEYS
        ErlangTokenTypes.QUOTED_ATOM -> QUOTED_ATOM_KEYS
        ErlangTokenTypes.CHAR_LITERAL -> CHAR_KEYS
        ErlangTokenTypes.INTEGER, ErlangTokenTypes.FLOAT -> NUMBER_KEYS
        ErlangTokenTypes.LANGUAGE_CONSTANT -> LANGUAGE_CONSTANT_KEYS
        ErlangTokenTypes.KEYWORD, ErlangTokenTypes.OPERATOR_KEYWORD -> KEYWORD_KEYS
        ErlangTokenTypes.ATOM -> ATOM_KEYS
        ErlangTokenTypes.VARIABLE -> VARIABLE_KEYS
        ErlangTokenTypes.MACRO -> MACRO_KEYS
        ErlangTokenTypes.ATTRIBUTE -> ATTRIBUTE_KEYS
        ErlangTokenTypes.ARROW, ErlangTokenTypes.FAT_ARROW, ErlangTokenTypes.WALRUS -> STRUCTURAL_ARROW_KEYS
        ErlangTokenTypes.OPERATOR, ErlangTokenTypes.DOUBLE_COLON,
        ErlangTokenTypes.HASH -> OPERATOR_KEYS
        ErlangTokenTypes.LPAREN, ErlangTokenTypes.RPAREN -> PAREN_KEYS
        ErlangTokenTypes.LBRACKET, ErlangTokenTypes.RBRACKET,
        ErlangTokenTypes.LBIN, ErlangTokenTypes.RBIN -> BRACKET_KEYS
        ErlangTokenTypes.LBRACE, ErlangTokenTypes.RBRACE -> BRACE_KEYS
        ErlangTokenTypes.COMMA -> COMMA_KEYS
        ErlangTokenTypes.SEMICOLON -> SEMICOLON_KEYS
        ErlangTokenTypes.DOT -> DOT_KEYS
        ErlangTokenTypes.COLON, ErlangTokenTypes.PIPE, ErlangTokenTypes.DOUBLE_PIPE -> OPERATOR_KEYS
        else -> EMPTY
    }
}

class ErlangSyntaxHighlighterFactory : SyntaxHighlighterFactory() {
    override fun getSyntaxHighlighter(project: Project?, virtualFile: VirtualFile?): SyntaxHighlighter =
        ErlangSyntaxHighlighter()
}
