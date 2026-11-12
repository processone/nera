package net.processone.nera

import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Caret
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.actionSystem.EditorActionHandler
import com.intellij.openapi.editor.actionSystem.EditorActionManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.startup.ProjectActivity

class ErlangTabActionHandler(private val originalHandler: EditorActionHandler) : EditorActionHandler() {

    override fun doExecute(editor: Editor, caret: Caret?, dataContext: DataContext) {
        if (!isErlangFile(editor)) {
            originalHandler.execute(editor, caret, dataContext)
            return
        }

        val project = editor.project ?: run {
            originalHandler.execute(editor, caret, dataContext)
            return
        }

        val document = editor.document
        val currentCaret = caret ?: editor.caretModel.currentCaret

        if (currentCaret.hasSelection()) {
            reindentSelectedLines(editor, currentCaret, project)
            return
        }

        reindentSingleLine(editor, currentCaret, project)
    }

    private fun reindentSelectedLines(editor: Editor, caret: Caret, project: Project) {
        val document = editor.document
        val startLine = document.getLineNumber(caret.selectionStart)
        val endLine = document.getLineNumber(caret.selectionEnd)
        val calculator = ErlangIndentCalculator(document)

        WriteCommandAction.runWriteCommandAction(project) {
            // Process lines bottom-to-top so earlier offsets remain valid
            for (lineNum in endLine downTo startLine) {
                val lineStart = document.getLineStartOffset(lineNum)
                val lineEnd = document.getLineEndOffset(lineNum)
                val lineText = document.getText(com.intellij.openapi.util.TextRange(lineStart, lineEnd))

                // Skip blank lines
                if (lineText.isBlank()) continue

                val targetIndent = calculator.computeIndent(lineNum)
                val targetIndentStr = " ".repeat(targetIndent)

                val firstNonWs = lineText.indexOfFirst { !it.isWhitespace() }
                val currentWhitespaceEnd = lineStart + if (firstNonWs < 0) lineText.length else firstNonWs
                val currentWhitespace = document.getText(com.intellij.openapi.util.TextRange(lineStart, currentWhitespaceEnd))

                if (currentWhitespace != targetIndentStr) {
                    document.replaceString(lineStart, currentWhitespaceEnd, targetIndentStr)
                }
            }
        }

        // Clear selection and place caret at end of reindented block
        caret.removeSelection()
    }

    private fun reindentSingleLine(editor: Editor, currentCaret: Caret, project: Project) {
        val document = editor.document
        val lineNum = document.getLineNumber(currentCaret.offset)
        val lineStart = document.getLineStartOffset(lineNum)
        val lineEnd = document.getLineEndOffset(lineNum)
        val lineText = document.getText(com.intellij.openapi.util.TextRange(lineStart, lineEnd))

        // Compute canonical indent
        val calculator = ErlangIndentCalculator(document)
        val targetIndent = calculator.computeIndent(lineNum)
        val targetIndentStr = " ".repeat(targetIndent)

        // Find current leading whitespace
        val currentWhitespaceEnd = lineStart + lineText.indexOfFirst { !it.isWhitespace() }.let {
            if (it < 0) lineText.length else it
        }
        val currentWhitespace = lineText.substring(0, currentWhitespaceEnd - lineStart)

        if (currentWhitespace == targetIndentStr) {
            // Already at canonical indent; do nothing
            return
        }

        // Preserve cursor position relative to text content
        val cursorOffsetFromTextStart = currentCaret.offset - currentWhitespaceEnd
        val cursorInWhitespace = currentCaret.offset < currentWhitespaceEnd

        WriteCommandAction.runWriteCommandAction(project) {
            document.replaceString(lineStart, currentWhitespaceEnd, targetIndentStr)
        }

        // Adjust cursor position
        val newTextStart = lineStart + targetIndent
        if (cursorInWhitespace) {
            currentCaret.moveToOffset(newTextStart)
        } else {
            val newOffset = (newTextStart + cursorOffsetFromTextStart).coerceAtLeast(newTextStart)
            currentCaret.moveToOffset(newOffset.coerceAtMost(document.textLength))
        }
    }

    private fun isErlangFile(editor: Editor): Boolean {
        val file = editor.virtualFile ?: return false
        val ext = file.extension?.lowercase() ?: return false
        return ext in setOf("erl", "hrl", "xrl", "yrl", "escript", "app") ||
               file.name.endsWith(".app.src")
    }

    class Registration : ProjectActivity {
        override suspend fun execute(project: Project) {
            val actionManager = EditorActionManager.getInstance()
            val originalHandler = actionManager.getActionHandler("EditorTab")
            // Avoid double-wrapping if already registered
            if (originalHandler !is ErlangTabActionHandler) {
                actionManager.setActionHandler("EditorTab", ErlangTabActionHandler(originalHandler))
            }
        }
    }
}
