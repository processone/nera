package net.processone.nera

import com.intellij.openapi.editor.Document

class ErlangIndentCalculator(private val document: Document) {

    companion object {
        const val INDENT_WIDTH = 4
        const val GUARD_INDENT = 2

        private val BLOCK_OPENERS = setOf(
            TokenType.CASE, TokenType.IF, TokenType.RECEIVE,
            TokenType.TRY, TokenType.BEGIN, TokenType.FUN, TokenType.MAYBE
        )

        private val OPEN_BRACKETS = setOf(
            TokenType.LPAREN, TokenType.LBRACKET, TokenType.LBRACE, TokenType.LBIN
        )

        private val CLOSE_BRACKETS = setOf(
            TokenType.RPAREN, TokenType.RBRACKET, TokenType.RBRACE, TokenType.RBIN
        )

        private fun matchingOpen(close: TokenType): TokenType = when (close) {
            TokenType.RPAREN -> TokenType.LPAREN
            TokenType.RBRACKET -> TokenType.LBRACKET
            TokenType.RBRACE -> TokenType.LBRACE
            TokenType.RBIN -> TokenType.LBIN
            else -> throw IllegalArgumentException("Not a close bracket: $close")
        }
    }

    /**
     * Context stack entry for tracking nesting during backward scanning.
     */
    private sealed class StackEntry {
        data class Block(val keyword: TokenType, val column: Int, val line: Int) : StackEntry()
        // `column` = column of the open bracket itself.
        // `lineIndent` = indent of the line the bracket opens on (column of the
        // first non-whitespace token on that line). Used for canonical
        // hanging-bracket layout, where content is `lineIndent + INDENT_WIDTH`
        // and the closing bracket returns to `lineIndent`, regardless of where
        // on its line the open bracket actually appears.
        data class Bracket(val type: TokenType, val column: Int, val lineIndent: Int, val line: Int) : StackEntry()
        data class Arrow(val column: Int, val line: Int) : StackEntry()
    }

    /**
     * Compute the canonical indentation (number of spaces) for the given line number.
     */
    fun computeIndent(targetLine: Int): Int {
        if (targetLine < 0 || targetLine >= document.lineCount) return 0

        // Find clause start: scan back for a line ending with '.' or start of file
        val clauseStartLine = findClauseStart(targetLine)

        // Get text from clause start to end of line before target
        val clauseStartOffset = document.getLineStartOffset(clauseStartLine)
        val contextEndOffset = if (targetLine > 0) {
            document.getLineEndOffset(targetLine - 1)
        } else {
            clauseStartOffset
        }

        // Check what the target line starts with (for end/of/catch/after/when alignment)
        val targetLineText = getLineText(targetLine).trimStart()
        val targetFirstToken = firstTokenOnLine(targetLineText)

        // If target line starts with an attribute, it's column 0
        if (targetLineText.startsWith("-") && targetLineText.length > 1 && targetLineText[1].isLetter()) {
            return 0
        }

        // If this is the first line of the file or the first line of a new clause at top-level
        if (targetLine == 0 || clauseStartLine == targetLine) {
            return 0
        }

        // Tokenize context (clause start to line before target)
        val contextText = document.getText(com.intellij.openapi.util.TextRange(clauseStartOffset, contextEndOffset))
        val tokens = ErlangTokenizer(contextText).tokenize()

        if (tokens.isEmpty()) return 0

        // Build the context stack
        val stack = buildContextStack(tokens)
        val lastToken = tokens.last()

        // --- Dispatch rules based on target line's first token ---

        // 'end' aligns with its matching block opener
        if (targetFirstToken?.type == TokenType.END) {
            return findMatchingBlockOpener(stack)
        }

        // 'of' aligns with 'case'/'try'
        if (targetFirstToken?.type == TokenType.OF) {
            return findKeywordColumn(stack, setOf(TokenType.CASE, TokenType.TRY))
        }

        // 'catch' aligns with 'try'
        if (targetFirstToken?.type == TokenType.CATCH) {
            return findKeywordColumn(stack, setOf(TokenType.TRY))
        }

        // 'after' aligns with 'receive'/'try'
        if (targetFirstToken?.type == TokenType.AFTER) {
            return findKeywordColumn(stack, setOf(TokenType.RECEIVE, TokenType.TRY))
        }

        // Close brackets: align with their open bracket (column 0 in attribute context)
        if (targetFirstToken != null && targetFirstToken.type in CLOSE_BRACKETS) {
            if (isAttributeContext(tokens)) return 0
            return findMatchingBracketColumn(stack, matchingOpen(targetFirstToken.type))
        }

        // --- Dispatch rules based on last token in context ---

        // After '.' -> new function, column 0
        if (lastToken.type == TokenType.DOT) {
            return 0
        }

        // After '->' -> clause body
        if (lastToken.type == TokenType.ARROW) {
            return computeArrowBodyIndent(stack, tokens)
        }

        // After ',' -> continue at same level as previous sibling
        if (lastToken.type == TokenType.COMMA) {
            return computeCommaIndent(stack, tokens)
        }

        // After ';' -> new clause at pattern level
        if (lastToken.type == TokenType.SEMICOLON) {
            return computeSemicolonIndent(stack)
        }

        // After block openers (case, if, receive, etc.) -> indent for pattern/expression
        if (lastToken.type in BLOCK_OPENERS) {
            return lastToken.column + INDENT_WIDTH
        }

        // After 'of' -> indent for case patterns
        if (lastToken.type == TokenType.OF) {
            // Find the matching case
            val caseCol = findKeywordColumn(stack, setOf(TokenType.CASE, TokenType.TRY))
            return caseCol + INDENT_WIDTH
        }

        // After 'catch' (in try) -> indent for catch patterns
        if (lastToken.type == TokenType.CATCH) {
            val tryCol = findKeywordColumn(stack, setOf(TokenType.TRY))
            return tryCol + INDENT_WIDTH
        }

        // After 'after' -> indent for after body
        if (lastToken.type == TokenType.AFTER) {
            val recvCol = findKeywordColumn(stack, setOf(TokenType.RECEIVE, TokenType.TRY))
            return recvCol + INDENT_WIDTH
        }

        // After open bracket -> hanging-bracket content at line_indent + INDENT_WIDTH,
        // not bracket's column + INDENT_WIDTH. This matches nera-fmt.
        if (lastToken.type in OPEN_BRACKETS) {
            if (isAttributeContext(tokens)) return INDENT_WIDTH
            // Find the most recently pushed Bracket matching lastToken (top of stack).
            val bracket = stack.lastOrNull { it is StackEntry.Bracket } as? StackEntry.Bracket
            val base = bracket?.lineIndent ?: lastToken.column
            return base + INDENT_WIDTH
        }

        // After 'when' -> guard continuation indent
        if (lastToken.type == TokenType.WHEN) {
            return computeWhenIndent(stack, tokens)
        }

        // After 'begin' -> indent body
        if (lastToken.type == TokenType.BEGIN) {
            return lastToken.column + INDENT_WIDTH
        }

        // Default: look at context stack for enclosing structure
        return computeDefaultIndent(stack, tokens)
    }

    private fun buildContextStack(tokens: List<Token>): MutableList<StackEntry> {
        // Precompute each line's indent — the column of its first non-whitespace
        // token. Used to align bracket content canonically (line_indent + 4)
        // rather than relative to where the open bracket appears inline.
        val lineIndent = mutableMapOf<Int, Int>()
        for (token in tokens) {
            lineIndent.putIfAbsent(token.line, token.column)
        }

        val stack = mutableListOf<StackEntry>()

        for (token in tokens) {
            when (token.type) {
                in BLOCK_OPENERS -> {
                    stack.add(StackEntry.Block(token.type, token.column, token.line))
                }
                TokenType.END -> {
                    // Pop matching block opener
                    val idx = stack.indexOfLast { it is StackEntry.Block }
                    if (idx >= 0) {
                        // Also remove any arrows above this block
                        while (stack.size > idx) {
                            stack.removeAt(stack.size - 1)
                        }
                    }
                }
                in OPEN_BRACKETS -> {
                    val indent = lineIndent[token.line] ?: token.column
                    stack.add(StackEntry.Bracket(token.type, token.column, indent, token.line))
                }
                in CLOSE_BRACKETS -> {
                    val openType = matchingOpen(token.type)
                    val idx = stack.indexOfLast { it is StackEntry.Bracket && it.type == openType }
                    if (idx >= 0) {
                        while (stack.size > idx) {
                            stack.removeAt(stack.size - 1)
                        }
                    }
                }
                TokenType.ARROW -> {
                    stack.add(StackEntry.Arrow(token.column, token.line))
                }
                TokenType.SEMICOLON -> {
                    // Pop arrows back to the enclosing block or bracket
                    while (stack.isNotEmpty() && stack.last() is StackEntry.Arrow) {
                        stack.removeAt(stack.size - 1)
                    }
                }
                else -> { /* no stack change */ }
            }
        }

        return stack
    }

    private fun isAttributeContext(tokens: List<Token>): Boolean {
        return tokens.firstOrNull()?.type == TokenType.ATTRIBUTE
    }

    private fun findMatchingBlockOpener(stack: List<StackEntry>): Int {
        val entry = stack.lastOrNull { it is StackEntry.Block } as? StackEntry.Block
        return entry?.column ?: 0
    }

    private fun findKeywordColumn(stack: List<StackEntry>, types: Set<TokenType>): Int {
        val entry = stack.lastOrNull { it is StackEntry.Block && it.keyword in types } as? StackEntry.Block
        return entry?.column ?: 0
    }

    /**
     * Closing bracket alignment — returns to the indent of the LINE that
     * contains the opening bracket, not the bracket's column. This matches
     * nera-fmt's hanging-bracket layout:
     *
     *     ets:select_delete(..., [      <- `[` on this line (line indent = 4)
     *         content
     *     ]).                           <- `]` at line indent (4), not col 33
     */
    private fun findMatchingBracketColumn(stack: List<StackEntry>, openType: TokenType): Int {
        val entry = stack.lastOrNull { it is StackEntry.Bracket && it.type == openType } as? StackEntry.Bracket
        return entry?.lineIndent ?: 0
    }

    private fun computeArrowBodyIndent(stack: List<StackEntry>, tokens: List<Token>): Int {
        // Find the innermost block on the stack
        val block = stack.lastOrNull { it is StackEntry.Block } as? StackEntry.Block

        if (block != null) {
            // Inside a case/if/receive/try/fun block: pattern_col + 4
            // The pattern column is keyword_col + 4
            return block.column + INDENT_WIDTH + INDENT_WIDTH
        }

        // Top-level function clause -> indent to 4
        return INDENT_WIDTH
    }

    private fun computeCommaIndent(stack: List<StackEntry>, tokens: List<Token>): Int {
        // Inside a bracket: next element aligns with the bracket's line indent + 4
        // (hanging-bracket canonical layout), not with the bracket's column.
        val bracket = stack.lastOrNull { it is StackEntry.Bracket } as? StackEntry.Bracket
        if (bracket != null) {
            if (isAttributeContext(tokens)) return INDENT_WIDTH
            return bracket.lineIndent + INDENT_WIDTH
        }

        // After an arrow (in clause body): stay at same level
        val arrow = stack.lastOrNull { it is StackEntry.Arrow } as? StackEntry.Arrow
        if (arrow != null) {
            val block = stack.lastOrNull { it is StackEntry.Block } as? StackEntry.Block
            return if (block != null) {
                block.column + INDENT_WIDTH + INDENT_WIDTH
            } else {
                INDENT_WIDTH
            }
        }

        // After a when guard: find the expression start after 'when'
        // Default: maintain current indentation
        return findPreviousExpressionIndent(tokens)
    }

    private fun computeSemicolonIndent(stack: List<StackEntry>): Int {
        // Back to pattern level in enclosing block
        val block = stack.lastOrNull { it is StackEntry.Block } as? StackEntry.Block
        if (block != null) {
            return block.column + INDENT_WIDTH
        }
        // Top-level function: new clause at column 0
        return 0
    }

    private fun computeWhenIndent(stack: List<StackEntry>, tokens: List<Token>): Int {
        // Find the 'when' token to get its position
        val whenToken = tokens.lastOrNull { it.type == TokenType.WHEN }
        if (whenToken != null) {
            return whenToken.column + INDENT_WIDTH + 1
        }
        return GUARD_INDENT
    }

    private fun computeDefaultIndent(stack: List<StackEntry>, tokens: List<Token>): Int {
        // Check enclosing context from top of stack
        for (i in stack.indices.reversed()) {
            when (val entry = stack[i]) {
                is StackEntry.Arrow -> {
                    // We're inside a clause body
                    val block = stack.subList(0, i).lastOrNull { it is StackEntry.Block } as? StackEntry.Block
                    return if (block != null) {
                        block.column + INDENT_WIDTH + INDENT_WIDTH
                    } else {
                        INDENT_WIDTH
                    }
                }
                is StackEntry.Bracket -> {
                    if (isAttributeContext(tokens)) return INDENT_WIDTH
                    return entry.lineIndent + INDENT_WIDTH
                }
                is StackEntry.Block -> {
                    return entry.column + INDENT_WIDTH
                }
            }
        }
        return 0
    }

    /**
     * Find the indentation of the previous expression (for comma continuation).
     */
    private fun findPreviousExpressionIndent(tokens: List<Token>): Int {
        // Find the last comma, then the token right after it gives the indent
        for (i in tokens.indices.reversed()) {
            if (tokens[i].type == TokenType.COMMA && i + 1 < tokens.size) {
                return tokens[i + 1].column
            }
        }
        // Fall back to the first token's column
        return tokens.firstOrNull()?.column ?: 0
    }

    /**
     * Find the start of the current clause by scanning backward for a line ending with '.'.
     */
    private fun findClauseStart(targetLine: Int): Int {
        for (lineNum in (targetLine - 1) downTo 0) {
            val lineText = getLineText(lineNum).trimEnd()
            if (lineText.endsWith(".")) {
                // Check it's not inside a string or comment by quick scan
                // The line ending with '.' is the end of the previous clause,
                // so the next line starts the new clause
                return lineNum + 1
            }
        }
        return 0
    }

    private fun getLineText(lineNum: Int): String {
        if (lineNum < 0 || lineNum >= document.lineCount) return ""
        val start = document.getLineStartOffset(lineNum)
        val end = document.getLineEndOffset(lineNum)
        return document.getText(com.intellij.openapi.util.TextRange(start, end))
    }

    private fun firstTokenOnLine(trimmedLineText: String): Token? {
        if (trimmedLineText.isEmpty()) return null
        val tokens = ErlangTokenizer(trimmedLineText).tokenize()
        return tokens.firstOrNull()
    }
}
