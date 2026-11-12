package net.processone.nera

import org.junit.Assert.assertEquals
import org.junit.Test

class ErlangSyntaxHighlighterTest {

    private val highlighter = ErlangSyntaxHighlighter()

    @Test
    fun `arrows route to structural arrow key`() {
        // ->, =>, := are structural separators (clause head/body, map k/v).
        // They must route to STRUCTURAL_ARROW_KEY so users can theme them
        // independently from regular operators.
        assertEquals(
            ErlangSyntaxHighlighter.STRUCTURAL_ARROW_KEY,
            highlighter.getTokenHighlights(ErlangTokenTypes.ARROW).single()
        )
        assertEquals(
            ErlangSyntaxHighlighter.STRUCTURAL_ARROW_KEY,
            highlighter.getTokenHighlights(ErlangTokenTypes.FAT_ARROW).single()
        )
        assertEquals(
            ErlangSyntaxHighlighter.STRUCTURAL_ARROW_KEY,
            highlighter.getTokenHighlights(ErlangTokenTypes.WALRUS).single()
        )
    }

    @Test
    fun `regular operators stay on operator key`() {
        // Regression: operators other than the structural arrows must still
        // resolve to the plain OPERATOR_KEY.
        assertEquals(
            ErlangSyntaxHighlighter.OPERATOR_KEY,
            highlighter.getTokenHighlights(ErlangTokenTypes.OPERATOR).single()
        )
        assertEquals(
            ErlangSyntaxHighlighter.OPERATOR_KEY,
            highlighter.getTokenHighlights(ErlangTokenTypes.PIPE).single()
        )
        assertEquals(
            ErlangSyntaxHighlighter.OPERATOR_KEY,
            highlighter.getTokenHighlights(ErlangTokenTypes.COLON).single()
        )
        assertEquals(
            ErlangSyntaxHighlighter.OPERATOR_KEY,
            highlighter.getTokenHighlights(ErlangTokenTypes.DOUBLE_COLON).single()
        )
        assertEquals(
            ErlangSyntaxHighlighter.OPERATOR_KEY,
            highlighter.getTokenHighlights(ErlangTokenTypes.HASH).single()
        )
        assertEquals(
            ErlangSyntaxHighlighter.OPERATOR_KEY,
            highlighter.getTokenHighlights(ErlangTokenTypes.DOUBLE_PIPE).single()
        )
    }

    @Test
    fun `punctuation keys are distinct from operators`() {
        // Regression guard: commas, dots, semicolons, brackets each have their
        // own keys and must not collapse into the operator bucket.
        assertEquals(
            ErlangSyntaxHighlighter.COMMA_KEY,
            highlighter.getTokenHighlights(ErlangTokenTypes.COMMA).single()
        )
        assertEquals(
            ErlangSyntaxHighlighter.DOT_KEY,
            highlighter.getTokenHighlights(ErlangTokenTypes.DOT).single()
        )
        assertEquals(
            ErlangSyntaxHighlighter.SEMICOLON_KEY,
            highlighter.getTokenHighlights(ErlangTokenTypes.SEMICOLON).single()
        )
    }
}
